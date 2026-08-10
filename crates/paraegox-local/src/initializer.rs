//! Offline, fail-closed publication of one DeveloperLocal chat workspace.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

use nix::errno::Errno;
use nix::fcntl::{AtFlags, OFlag, openat};
use nix::sys::stat::{Mode, SFlag, fchmod, fstatat, mkdirat};
use nix::unistd::{Gid, UnlinkatFlags, fchown, getegid, geteuid, linkat, unlinkat};

use crate::error::LocalProcessError;

pub(crate) const INIT_PROFILE: &str = "deterministic-echo-v1";
pub(crate) const INIT_CONFIG_RELATIVE_PATH: &str = "paraegox.toml";
pub(crate) const INIT_STATE_RELATIVE_PATH: &str = "state";

const INIT_FABRIC_LISTEN: &str = "tcp/127.0.0.1:7447";
const INIT_CONFIG_TEMP_FILE: &str = ".paraegox.toml.tmp";
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
const MODE_MASK: u32 = 0o7777;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InitOutcomeV1 {
    changed: bool,
}

impl InitOutcomeV1 {
    pub(crate) const fn changed(self) -> bool {
        self.changed
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InitFailureV1 {
    error: LocalProcessError,
    changed: bool,
}

impl InitFailureV1 {
    const fn new(error: LocalProcessError, changed: bool) -> Self {
        Self { error, changed }
    }

    const fn with_prior_change(self, prior_changed: bool) -> Self {
        Self {
            error: self.error,
            changed: self.changed || prior_changed,
        }
    }

    pub(crate) const fn error(self) -> LocalProcessError {
        self.error
    }

    pub(crate) const fn changed(self) -> bool {
        self.changed
    }
}

/// Creates or strictly reopens one private config workspace.
///
/// This function does not create the configured state directory, read a
/// Secret, open a socket, access the network, or start any owner.
pub(crate) fn initialize(directory: &Path) -> Result<InitOutcomeV1, InitFailureV1> {
    let uid = geteuid();
    let gid = getegid();
    if uid.is_root() || gid.as_raw() == 0 {
        return Err(InitFailureV1::new(
            LocalProcessError::InitUnsafeExecutionIdentity,
            false,
        ));
    }
    let expected =
        expected_config_wire(directory).map_err(|error| InitFailureV1::new(error, false))?;
    let workspace = open_or_create_workspace(directory, uid.as_raw(), gid.as_raw())?;
    let config_changed = publish_or_verify_config(&workspace, &expected)
        .map_err(|failure| failure.with_prior_change(workspace.created))?;
    let changed = workspace.created || config_changed;
    validate_final_workspace(&workspace, &expected, changed)
        .map_err(|error| InitFailureV1::new(error, changed))?;
    Ok(InitOutcomeV1 { changed })
}

fn validate_final_workspace(
    workspace: &WorkspaceV1,
    expected: &[u8],
    changed: bool,
) -> Result<(), LocalProcessError> {
    let context = if changed {
        VerifyContextV1::Published
    } else {
        VerifyContextV1::Existing
    };
    workspace
        .validate_named_identity()
        .map_err(|error| if changed { context.invalid() } else { error })?;
    if read_named_metadata(&workspace.directory, OsStr::new(INIT_CONFIG_TEMP_FILE))
        .map_err(|error| if changed { context.io() } else { error })?
        .is_some()
    {
        return Err(context.invalid());
    }
    verify_config(workspace, expected, 1, context)
}

fn expected_config_wire(directory: &Path) -> Result<Vec<u8>, LocalProcessError> {
    let state_root = directory.join(INIT_STATE_RELATIVE_PATH);
    let state_root = state_root
        .to_str()
        .ok_or(LocalProcessError::InitWorkspaceConflict)?;
    let quoted_state_root = toml::Value::String(state_root.to_owned()).to_string();
    Ok(format!(
        "schema_version = 1\nstate_root = {quoted_state_root}\nfabric_listen = \"{INIT_FABRIC_LISTEN}\"\n\n[model]\nprovider = \"{INIT_PROFILE}\"\n"
    )
    .into_bytes())
}

struct WorkspaceV1 {
    parent: File,
    parent_path: PathBuf,
    directory: File,
    directory_path: PathBuf,
    directory_name: OsString,
    uid: u32,
    gid: u32,
    created: bool,
}

impl WorkspaceV1 {
    fn validate_named_identity(&self) -> Result<(), LocalProcessError> {
        validate_existing_path_chain(&self.parent_path)?;
        let parent_named = fs::symlink_metadata(&self.parent_path).map_err(classify_path_io)?;
        let parent_opened = self
            .parent
            .metadata()
            .map_err(|_| LocalProcessError::InitIo)?;
        if !parent_named.is_dir()
            || !parent_opened.is_dir()
            || !same_std_file(&parent_named, &parent_opened)
        {
            return Err(LocalProcessError::InitWorkspaceConflict);
        }

        let named = read_named_metadata(&self.parent, &self.directory_name)?
            .ok_or(LocalProcessError::InitWorkspaceConflict)?;
        let opened = self
            .directory
            .metadata()
            .map_err(|_| LocalProcessError::InitIo)?;
        let absolute = fs::symlink_metadata(&self.directory_path).map_err(classify_path_io)?;
        validate_private_directory(&named, self.uid, self.gid)?;
        validate_private_directory_std(&opened, self.uid, self.gid)?;
        validate_private_directory_std(&absolute, self.uid, self.gid)?;
        if !named.same_std_identity(&opened)
            || !same_std_file(&opened, &absolute)
            || self.directory_path.parent() != Some(self.parent_path.as_path())
        {
            return Err(LocalProcessError::InitWorkspaceConflict);
        }
        Ok(())
    }
}

fn open_or_create_workspace(
    directory: &Path,
    uid: u32,
    gid: u32,
) -> Result<WorkspaceV1, InitFailureV1> {
    let parent_path = directory
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .ok_or(InitFailureV1::new(
            LocalProcessError::InitWorkspaceConflict,
            false,
        ))?;
    let directory_name = directory
        .file_name()
        .ok_or(InitFailureV1::new(
            LocalProcessError::InitWorkspaceConflict,
            false,
        ))?
        .to_os_string();
    let parent =
        open_existing_parent(parent_path).map_err(|error| InitFailureV1::new(error, false))?;
    let mut created = false;
    match read_named_metadata(&parent, &directory_name)
        .map_err(|error| InitFailureV1::new(error, false))?
    {
        Some(metadata) => validate_private_directory(&metadata, uid, gid)
            .map_err(|error| InitFailureV1::new(error, false))?,
        None => {
            mkdirat(
                &parent,
                directory_name.as_os_str(),
                Mode::from_bits_truncate(0o700),
            )
            .map_err(|error| {
                let mapped = if error == Errno::EEXIST {
                    LocalProcessError::InitWorkspaceConflict
                } else {
                    LocalProcessError::InitIo
                };
                InitFailureV1::new(mapped, false)
            })?;
            created = true;
        }
    }

    let owned = openat(
        &parent,
        directory_name.as_os_str(),
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| {
        InitFailureV1::new(
            classify_named_open(error, VerifyContextV1::Existing),
            created,
        )
    })?;
    let directory_file = File::from(owned);
    if created {
        let created_metadata = directory_file
            .metadata()
            .map_err(|_| InitFailureV1::new(LocalProcessError::InitIo, true))?;
        if created_metadata.gid() != gid {
            fchown(&directory_file, None, Some(Gid::from_raw(gid)))
                .map_err(|_| InitFailureV1::new(LocalProcessError::InitIo, true))?;
        }
        fchmod(&directory_file, Mode::from_bits_truncate(0o700))
            .map_err(|_| InitFailureV1::new(LocalProcessError::InitIo, true))?;
    }
    let workspace = WorkspaceV1 {
        parent,
        parent_path: parent_path.to_path_buf(),
        directory: directory_file,
        directory_path: directory.to_path_buf(),
        directory_name,
        uid,
        gid,
        created,
    };
    workspace
        .validate_named_identity()
        .map_err(|error| InitFailureV1::new(error, created))?;
    if created {
        workspace
            .directory
            .sync_all()
            .and_then(|()| workspace.parent.sync_all())
            .map_err(|_| InitFailureV1::new(LocalProcessError::InitIo, true))?;
    }
    Ok(workspace)
}

fn open_existing_parent(path: &Path) -> Result<File, LocalProcessError> {
    validate_existing_path_chain(path)?;
    let before = fs::symlink_metadata(path).map_err(classify_path_io)?;
    if !before.is_dir() || before.file_type().is_symlink() {
        return Err(LocalProcessError::InitWorkspaceConflict);
    }
    let parent = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| LocalProcessError::InitIo)?;
    let opened = parent.metadata().map_err(|_| LocalProcessError::InitIo)?;
    let after = fs::symlink_metadata(path).map_err(classify_path_io)?;
    if !opened.is_dir()
        || !after.is_dir()
        || !same_std_file(&before, &opened)
        || !same_std_file(&opened, &after)
    {
        return Err(LocalProcessError::InitWorkspaceConflict);
    }
    Ok(parent)
}

fn validate_existing_path_chain(path: &Path) -> Result<(), LocalProcessError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(value) => {
                current.push(value);
                let metadata = fs::symlink_metadata(&current).map_err(classify_path_io)?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(LocalProcessError::InitWorkspaceConflict);
                }
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(LocalProcessError::InitWorkspaceConflict);
            }
        }
    }
    Ok(())
}

fn classify_path_io(error: io::Error) -> LocalProcessError {
    if error.kind() == io::ErrorKind::NotFound {
        LocalProcessError::InitWorkspaceConflict
    } else {
        LocalProcessError::InitIo
    }
}

fn publish_or_verify_config(
    workspace: &WorkspaceV1,
    expected: &[u8],
) -> Result<bool, InitFailureV1> {
    let expected_length = u64::try_from(expected.len())
        .map_err(|_| InitFailureV1::new(LocalProcessError::InitWorkspaceConflict, false))?;
    workspace
        .validate_named_identity()
        .map_err(|error| InitFailureV1::new(error, false))?;
    if read_named_metadata(&workspace.directory, OsStr::new(INIT_CONFIG_TEMP_FILE))
        .map_err(|error| InitFailureV1::new(error, false))?
        .is_some()
    {
        return Err(InitFailureV1::new(
            LocalProcessError::InitWorkspaceConflict,
            false,
        ));
    }
    match read_named_metadata(&workspace.directory, OsStr::new(INIT_CONFIG_RELATIVE_PATH))
        .map_err(|error| InitFailureV1::new(error, false))?
    {
        Some(metadata) => {
            validate_private_file(&metadata, workspace.uid, workspace.gid, 1, expected_length)
                .map_err(|error| InitFailureV1::new(error, false))?;
            verify_config(workspace, expected, 1, VerifyContextV1::Existing)
                .map_err(|error| InitFailureV1::new(error, false))?;
            Ok(false)
        }
        None => {
            publish_new_config(workspace, expected)?;
            Ok(true)
        }
    }
}

fn publish_new_config(workspace: &WorkspaceV1, expected: &[u8]) -> Result<(), InitFailureV1> {
    let owned = openat(
        &workspace.directory,
        INIT_CONFIG_TEMP_FILE,
        OFlag::O_RDWR | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
        Mode::from_bits_truncate(0o600),
    )
    .map_err(|error| {
        let mapped = if error == Errno::EEXIST {
            LocalProcessError::InitWorkspaceConflict
        } else {
            LocalProcessError::InitIo
        };
        InitFailureV1::new(mapped, false)
    })?;
    let mut temporary = File::from(owned);
    let mut linked = false;
    let result = publish_new_config_inner(workspace, expected, &mut temporary, &mut linked);
    if let Err(error) = result {
        if cleanup_owned_temporary(workspace, &temporary, linked).is_err() {
            return Err(InitFailureV1::new(
                LocalProcessError::InitPublicationUncertain,
                true,
            ));
        }
        let observable_or_uncertain_change =
            linked || error == LocalProcessError::InitPublicationUncertain;
        let error = if linked {
            LocalProcessError::InitPublicationUncertain
        } else {
            error
        };
        return Err(InitFailureV1::new(error, observable_or_uncertain_change));
    }
    Ok(())
}

fn publish_new_config_inner(
    workspace: &WorkspaceV1,
    expected: &[u8],
    temporary: &mut File,
    linked: &mut bool,
) -> Result<(), LocalProcessError> {
    fchmod(&*temporary, Mode::from_bits_truncate(0o600)).map_err(|_| LocalProcessError::InitIo)?;
    validate_open_named_file(
        &workspace.directory,
        OsStr::new(INIT_CONFIG_TEMP_FILE),
        temporary,
        ExpectedFileV1 {
            uid: workspace.uid,
            gid: workspace.gid,
            links: 1,
            length: 0,
            context: VerifyContextV1::Existing,
        },
    )?;
    temporary
        .write_all(expected)
        .and_then(|()| temporary.sync_all())
        .map_err(|_| LocalProcessError::InitIo)?;
    validate_open_named_file(
        &workspace.directory,
        OsStr::new(INIT_CONFIG_TEMP_FILE),
        temporary,
        ExpectedFileV1 {
            uid: workspace.uid,
            gid: workspace.gid,
            links: 1,
            length: expected.len(),
            context: VerifyContextV1::Existing,
        },
    )?;
    workspace.validate_named_identity()?;
    if read_named_metadata(&workspace.directory, OsStr::new(INIT_CONFIG_RELATIVE_PATH))?.is_some() {
        return Err(LocalProcessError::InitWorkspaceConflict);
    }
    linkat(
        &workspace.directory,
        INIT_CONFIG_TEMP_FILE,
        &workspace.directory,
        INIT_CONFIG_RELATIVE_PATH,
        AtFlags::empty(),
    )
    .map_err(|error| {
        if error == Errno::EEXIST {
            LocalProcessError::InitWorkspaceConflict
        } else {
            LocalProcessError::InitPublicationUncertain
        }
    })?;
    *linked = true;
    validate_open_named_file(
        &workspace.directory,
        OsStr::new(INIT_CONFIG_TEMP_FILE),
        temporary,
        ExpectedFileV1 {
            uid: workspace.uid,
            gid: workspace.gid,
            links: 2,
            length: expected.len(),
            context: VerifyContextV1::Published,
        },
    )?;
    validate_open_named_file(
        &workspace.directory,
        OsStr::new(INIT_CONFIG_RELATIVE_PATH),
        temporary,
        ExpectedFileV1 {
            uid: workspace.uid,
            gid: workspace.gid,
            links: 2,
            length: expected.len(),
            context: VerifyContextV1::Published,
        },
    )?;
    workspace
        .directory
        .sync_all()
        .map_err(|_| LocalProcessError::InitPublicationUncertain)?;
    unlinkat(
        &workspace.directory,
        INIT_CONFIG_TEMP_FILE,
        UnlinkatFlags::NoRemoveDir,
    )
    .map_err(|_| LocalProcessError::InitPublicationUncertain)?;
    workspace
        .directory
        .sync_all()
        .map_err(|_| LocalProcessError::InitPublicationUncertain)?;
    verify_config(workspace, expected, 1, VerifyContextV1::Published)?;
    workspace
        .validate_named_identity()
        .map_err(|_| LocalProcessError::InitPublicationUncertain)
}

fn cleanup_owned_temporary(
    workspace: &WorkspaceV1,
    temporary: &File,
    linked: bool,
) -> Result<(), LocalProcessError> {
    let Some(named) = read_named_metadata(&workspace.directory, OsStr::new(INIT_CONFIG_TEMP_FILE))?
    else {
        return Ok(());
    };
    let opened = temporary
        .metadata()
        .map_err(|_| LocalProcessError::InitPublicationUncertain)?;
    let expected_links = if linked { 2 } else { 1 };
    if !named.same_std_identity(&opened)
        || named.kind & SFlag::S_IFMT != SFlag::S_IFREG
        || !opened.is_file()
        || named.links != expected_links
        || opened.nlink() != expected_links
    {
        return Err(LocalProcessError::InitPublicationUncertain);
    }
    unlinkat(
        &workspace.directory,
        INIT_CONFIG_TEMP_FILE,
        UnlinkatFlags::NoRemoveDir,
    )
    .map_err(|_| LocalProcessError::InitPublicationUncertain)?;
    workspace
        .directory
        .sync_all()
        .map_err(|_| LocalProcessError::InitPublicationUncertain)
}

#[derive(Clone, Copy)]
enum VerifyContextV1 {
    Existing,
    Published,
}

#[derive(Clone, Copy)]
struct ExpectedFileV1 {
    uid: u32,
    gid: u32,
    links: u64,
    length: usize,
    context: VerifyContextV1,
}

impl VerifyContextV1 {
    const fn invalid(self) -> LocalProcessError {
        match self {
            Self::Existing => LocalProcessError::InitWorkspaceConflict,
            Self::Published => LocalProcessError::InitPublicationUncertain,
        }
    }

    const fn io(self) -> LocalProcessError {
        match self {
            Self::Existing => LocalProcessError::InitIo,
            Self::Published => LocalProcessError::InitPublicationUncertain,
        }
    }
}

fn verify_config(
    workspace: &WorkspaceV1,
    expected: &[u8],
    expected_links: u64,
    context: VerifyContextV1,
) -> Result<(), LocalProcessError> {
    let owned = openat(
        &workspace.directory,
        INIT_CONFIG_RELATIVE_PATH,
        OFlag::O_RDONLY | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| classify_named_open(error, context))?;
    let mut file = File::from(owned);
    validate_open_named_file(
        &workspace.directory,
        OsStr::new(INIT_CONFIG_RELATIVE_PATH),
        &file,
        ExpectedFileV1 {
            uid: workspace.uid,
            gid: workspace.gid,
            links: expected_links,
            length: expected.len(),
            context,
        },
    )?;
    let mut actual = Vec::with_capacity(expected.len() + 1);
    (&mut file)
        .take(u64::try_from(expected.len()).map_err(|_| context.invalid())? + 1)
        .read_to_end(&mut actual)
        .map_err(|_| context.io())?;
    if actual.as_slice() != expected {
        return Err(context.invalid());
    }
    validate_open_named_file(
        &workspace.directory,
        OsStr::new(INIT_CONFIG_RELATIVE_PATH),
        &file,
        ExpectedFileV1 {
            uid: workspace.uid,
            gid: workspace.gid,
            links: expected_links,
            length: expected.len(),
            context,
        },
    )
}

fn classify_named_open(error: Errno, context: VerifyContextV1) -> LocalProcessError {
    if matches!(context, VerifyContextV1::Published) {
        return LocalProcessError::InitPublicationUncertain;
    }
    if matches!(
        error,
        Errno::ENOENT | Errno::ENOTDIR | Errno::ELOOP | Errno::EACCES | Errno::EPERM
    ) {
        LocalProcessError::InitWorkspaceConflict
    } else {
        LocalProcessError::InitIo
    }
}

fn validate_open_named_file(
    directory: &File,
    name: &OsStr,
    file: &File,
    expected: ExpectedFileV1,
) -> Result<(), LocalProcessError> {
    let before = read_named_metadata(directory, name)?.ok_or(expected.context.invalid())?;
    let opened = file.metadata().map_err(|_| expected.context.io())?;
    let after = read_named_metadata(directory, name)?.ok_or(expected.context.invalid())?;
    let expected_length = u64::try_from(expected.length).map_err(|_| expected.context.invalid())?;
    for metadata in [&before, &after] {
        validate_private_file(
            metadata,
            expected.uid,
            expected.gid,
            expected.links,
            expected_length,
        )
        .map_err(|_| expected.context.invalid())?;
    }
    validate_private_file_std(
        &opened,
        expected.uid,
        expected.gid,
        expected.links,
        expected_length,
    )
    .map_err(|_| expected.context.invalid())?;
    if before.identity != after.identity
        || !before.same_std_identity(&opened)
        || !after.same_std_identity(&opened)
    {
        return Err(expected.context.invalid());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentityV1 {
    device: i128,
    inode: i128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NamedMetadataV1 {
    identity: FileIdentityV1,
    kind: SFlag,
    uid: u64,
    gid: u64,
    mode: u64,
    links: u64,
    length: u64,
}

impl NamedMetadataV1 {
    fn same_std_identity(self, metadata: &fs::Metadata) -> bool {
        self.identity
            == FileIdentityV1 {
                device: i128::from(metadata.dev()),
                inode: i128::from(metadata.ino()),
            }
    }
}

fn read_named_metadata(
    directory: &File,
    name: &OsStr,
) -> Result<Option<NamedMetadataV1>, LocalProcessError> {
    fn unsigned_to_u64<T: Into<u64>>(value: T) -> u64 {
        value.into()
    }

    let metadata = match fstatat(directory, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(metadata) => metadata,
        Err(Errno::ENOENT) => return Ok(None),
        Err(_) => return Err(LocalProcessError::InitIo),
    };
    Ok(Some(NamedMetadataV1 {
        identity: FileIdentityV1 {
            device: i128::from(metadata.st_dev),
            inode: i128::from(metadata.st_ino),
        },
        kind: SFlag::from_bits_truncate(metadata.st_mode),
        uid: u64::from(metadata.st_uid),
        gid: u64::from(metadata.st_gid),
        mode: u64::from(metadata.st_mode) & u64::from(MODE_MASK),
        links: unsigned_to_u64(metadata.st_nlink),
        length: u64::try_from(metadata.st_size).map_err(|_| LocalProcessError::InitIo)?,
    }))
}

fn validate_private_directory(
    metadata: &NamedMetadataV1,
    uid: u32,
    gid: u32,
) -> Result<(), LocalProcessError> {
    if metadata.kind & SFlag::S_IFMT != SFlag::S_IFDIR
        || metadata.uid != u64::from(uid)
        || metadata.gid != u64::from(gid)
        || metadata.mode != u64::from(PRIVATE_DIRECTORY_MODE)
    {
        return Err(LocalProcessError::InitWorkspaceConflict);
    }
    Ok(())
}

fn validate_private_directory_std(
    metadata: &fs::Metadata,
    uid: u32,
    gid: u32,
) -> Result<(), LocalProcessError> {
    if !metadata.is_dir()
        || metadata.uid() != uid
        || metadata.gid() != gid
        || metadata.mode() & MODE_MASK != PRIVATE_DIRECTORY_MODE
    {
        return Err(LocalProcessError::InitWorkspaceConflict);
    }
    Ok(())
}

fn validate_private_file(
    metadata: &NamedMetadataV1,
    uid: u32,
    gid: u32,
    expected_links: u64,
    expected_length: u64,
) -> Result<(), LocalProcessError> {
    if metadata.kind & SFlag::S_IFMT != SFlag::S_IFREG
        || metadata.uid != u64::from(uid)
        || metadata.gid != u64::from(gid)
        || metadata.mode != u64::from(PRIVATE_FILE_MODE)
        || metadata.links != expected_links
        || metadata.length != expected_length
    {
        return Err(LocalProcessError::InitWorkspaceConflict);
    }
    Ok(())
}

fn validate_private_file_std(
    metadata: &fs::Metadata,
    uid: u32,
    gid: u32,
    expected_links: u64,
    expected_length: u64,
) -> Result<(), LocalProcessError> {
    if !metadata.is_file()
        || metadata.uid() != uid
        || metadata.gid() != gid
        || metadata.mode() & MODE_MASK != PRIVATE_FILE_MODE
        || metadata.nlink() != expected_links
        || metadata.len() != expected_length
    {
        return Err(LocalProcessError::InitWorkspaceConflict);
    }
    Ok(())
}

fn same_std_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::config::{Command, parse_chat_config_toml_for_test};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct InitFixtureV1 {
        root: PathBuf,
    }

    impl InitFixtureV1 {
        fn new(label: &str) -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let parent = fs::canonicalize(std::env::temp_dir()).expect("canonical test temp");
            let root = parent.join(format!(
                "paraegox-init-{label}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("create private test parent");
            fs::set_permissions(&root, fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE))
                .expect("set private test parent");
            Self { root }
        }

        fn workspace(&self) -> PathBuf {
            self.root.join("workspace")
        }
    }

    impl Drop for InitFixtureV1 {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("remove init test fixture");
        }
    }

    #[test]
    fn fresh_init_is_private_valid_and_idempotent_without_creating_state() {
        let fixture = InitFixtureV1::new("idempotent");
        let workspace = fixture.workspace();
        let first = initialize(&workspace).expect("fresh init");
        assert!(first.changed());

        let config_path = workspace.join(INIT_CONFIG_RELATIVE_PATH);
        let config_metadata = fs::symlink_metadata(&config_path).expect("config metadata");
        let workspace_metadata = fs::symlink_metadata(&workspace).expect("workspace metadata");
        assert_eq!(
            workspace_metadata.mode() & MODE_MASK,
            PRIVATE_DIRECTORY_MODE
        );
        assert_eq!(config_metadata.mode() & MODE_MASK, PRIVATE_FILE_MODE);
        assert_eq!(config_metadata.nlink(), 1);
        assert!(!workspace.join(INIT_STATE_RELATIVE_PATH).exists());
        assert!(!workspace.join(INIT_CONFIG_TEMP_FILE).exists());

        let text = fs::read_to_string(&config_path).expect("read generated config");
        match parse_chat_config_toml_for_test(&text).expect("strict generated chat config") {
            Command::DeveloperFixtureV1(config) => {
                assert_eq!(
                    config.state_root(),
                    workspace.join(INIT_STATE_RELATIVE_PATH)
                );
                assert_eq!(config.fabric_listen(), INIT_FABRIC_LISTEN);
            }
            _ => panic!("generated config must select deterministic fixture"),
        }

        fs::create_dir(workspace.join(INIT_STATE_RELATIVE_PATH)).expect("later owner state");
        let second = initialize(&workspace).expect("idempotent re-init");
        assert!(!second.changed());
    }

    #[test]
    fn different_content_and_extra_publication_temp_fail_without_overwrite() {
        let fixture = InitFixtureV1::new("conflicts");
        let workspace = fixture.workspace();
        fs::create_dir(&workspace).expect("create workspace");
        fs::set_permissions(
            &workspace,
            fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE),
        )
        .expect("private workspace");
        let config_path = workspace.join(INIT_CONFIG_RELATIVE_PATH);
        fs::write(&config_path, b"different\n").expect("write conflicting config");
        fs::set_permissions(&config_path, fs::Permissions::from_mode(PRIVATE_FILE_MODE))
            .expect("private conflicting config");

        let failure = initialize(&workspace).expect_err("different content conflicts");
        assert_eq!(failure.error(), LocalProcessError::InitWorkspaceConflict);
        assert!(!failure.changed());
        assert_eq!(
            fs::read(&config_path).expect("conflict preserved"),
            b"different\n"
        );

        fs::remove_file(&config_path).expect("remove first conflict");
        fs::write(workspace.join(INIT_CONFIG_TEMP_FILE), b"stale").expect("extra temp");
        let failure = initialize(&workspace).expect_err("extra temp conflicts");
        assert_eq!(failure.error(), LocalProcessError::InitWorkspaceConflict);
        assert!(!failure.changed());
        assert!(!config_path.exists());
    }

    #[test]
    fn symlink_hardlink_and_wrong_modes_fail_closed() {
        let fixture = InitFixtureV1::new("metadata-conflicts");
        let workspace = fixture.workspace();
        fs::create_dir(&workspace).expect("create workspace");
        fs::set_permissions(
            &workspace,
            fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE),
        )
        .expect("private workspace");
        let config_path = workspace.join(INIT_CONFIG_RELATIVE_PATH);
        let outside = fixture.root.join("outside");
        fs::write(
            &outside,
            expected_config_wire(&workspace).expect("expected config"),
        )
        .expect("outside config");
        fs::set_permissions(&outside, fs::Permissions::from_mode(PRIVATE_FILE_MODE))
            .expect("private outside config");

        symlink(&outside, &config_path).expect("symlink config");
        assert_eq!(
            initialize(&workspace)
                .expect_err("symlink conflicts")
                .error(),
            LocalProcessError::InitWorkspaceConflict
        );
        fs::remove_file(&config_path).expect("remove symlink");

        fs::hard_link(&outside, &config_path).expect("hardlink config");
        assert_eq!(
            initialize(&workspace)
                .expect_err("hardlink conflicts")
                .error(),
            LocalProcessError::InitWorkspaceConflict
        );
        fs::remove_file(&config_path).expect("remove hardlink");

        fs::write(
            &config_path,
            expected_config_wire(&workspace).expect("expected config"),
        )
        .expect("write mode-conflicting config");
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o644))
            .expect("make config public");
        assert_eq!(
            initialize(&workspace)
                .expect_err("wrong config mode conflicts")
                .error(),
            LocalProcessError::InitWorkspaceConflict
        );
        assert_eq!(
            fs::symlink_metadata(&config_path)
                .expect("conflicting config metadata")
                .mode()
                & MODE_MASK,
            0o644
        );
        fs::remove_file(&config_path).expect("remove mode-conflicting config");

        fs::set_permissions(&workspace, fs::Permissions::from_mode(0o755))
            .expect("make workspace public");
        assert_eq!(
            initialize(&workspace)
                .expect_err("wrong workspace mode conflicts")
                .error(),
            LocalProcessError::InitWorkspaceConflict
        );
        assert!(!config_path.exists());
    }
}
