//! Descriptor-relative Unix owner for the Artifact F0 materialization store.
//!
//! Public values returned by this module are fully owned. No file descriptor,
//! advisory-lock guard, directory cursor, or filesystem borrow crosses the
//! facade boundary.

use core::fmt;
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs::{File, Metadata, TryLockError};
use std::io::{Read, Write};
use std::num::NonZeroU64;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use nix::dir::Dir;
use nix::fcntl::{AtFlags, OFlag, open, openat, renameat};
use nix::sys::stat::{Mode, fchmod, fstatat, mkdirat};
use nix::unistd::{UnlinkatFlags, getegid, geteuid, unlinkat};
use rustix::fs::{RenameFlags, renameat_with};

use crate::{
    ArtifactCapacityInputV1, ArtifactConfigCommitmentV1, ArtifactFilesystemClaimV1,
    ArtifactManifestV1, ArtifactObjectRecordV1, ArtifactObjectRefV1, ArtifactOperationIdV1,
    ArtifactQuarantineFactsV1, ArtifactRecoveryStartV1, ArtifactSnapshotSuccessorV1,
    ArtifactStoreInstanceV1, ArtifactStoreSnapshotCandidateV1, ArtifactStoreSnapshotV1,
    MaterializationAdmissionV1, MaterializationOperationV1, MaterializationReceiptRefV1,
    MaterializationReceiptV1, MaterializationRequestV1, MaterializationTerminalStateV1,
    MaterializationTerminalV1, MaterializingRecordV1, VerifiedArtifactPairV1,
    VerifiedMaterializationReadBundleV1,
};

const MAX_STATE_ROOT_UTF8_BYTES: usize = 3917;
const STORE_ROOT_NAME: &str = "artifact-store-v1";
const STORE_STAGING_NAME: &str = ".artifact-store-v1.initializing";
const STORE_LOCK_NAME: &str = "artifact.lock";
const STORE_SNAPSHOT_NAME: &str = "artifact.snapshot";
const STORE_SNAPSHOT_NEXT_NAME: &str = ".artifact.snapshot.next";
const OBJECTS_NAME: &str = "objects";
const MANIFEST_NAME: &str = "manifest.pxam";
const MANIFEST_NEXT_NAME: &str = ".manifest.pxam.next";
const PAYLOAD_NAME: &str = "payload.bin";
const PAYLOAD_NEXT_NAME: &str = ".payload.bin.next";
const MAX_SNAPSHOT_BYTES: usize = 1_241_344;
const MAX_OBJECT_DIRECTORIES: usize = 65;
const DIRECTORY_MODE_BITS: u32 = 0o700;
const FILE_MODE_BITS: u32 = 0o600;
const MODE_MASK: u32 = 0o7777;
const DIRECTORY_MODE: Mode = Mode::S_IRUSR.union(Mode::S_IWUSR).union(Mode::S_IXUSR);
const FILE_MODE: Mode = Mode::S_IRUSR.union(Mode::S_IWUSR);

/// One revalidated, owned view of the current configuration authority.
///
/// This value deliberately carries no descriptor. Every store invocation pins
/// and validates the path again, then asks the authority to revalidate it at
/// each publication boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactStoreAuthorityBindingV1 {
    state_root: PathBuf,
    config_commitment: ArtifactConfigCommitmentV1,
}

impl ArtifactStoreAuthorityBindingV1 {
    pub fn try_new(
        state_root: PathBuf,
        config_commitment: ArtifactConfigCommitmentV1,
    ) -> Result<Self, ArtifactStoreAuthorityRecheckFailureV1> {
        validate_state_root_path(&state_root)?;
        Ok(Self {
            state_root,
            config_commitment,
        })
    }

    #[must_use]
    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    #[must_use]
    pub const fn config_commitment(&self) -> ArtifactConfigCommitmentV1 {
        self.config_commitment
    }
}

/// Failure reported while re-reading the caller-owned configuration authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactStoreAuthorityRecheckFailureV1 {
    UnsafePath,
    Configuration,
    Io,
}

impl fmt::Display for ArtifactStoreAuthorityRecheckFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsafePath => "artifact store authority path is unsafe",
            Self::Configuration => "artifact store configuration authority changed",
            Self::Io => "artifact store configuration authority could not be read",
        })
    }
}

impl std::error::Error for ArtifactStoreAuthorityRecheckFailureV1 {}

/// Caller-owned seam used to re-read the current state-root/config binding.
pub trait ArtifactStoreAuthorityV1 {
    fn revalidate(
        &mut self,
    ) -> Result<ArtifactStoreAuthorityBindingV1, ArtifactStoreAuthorityRecheckFailureV1>;
}

/// What this invocation can prove about canonical owner mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactStoreChangeV1 {
    Unchanged,
    Changed,
    Unknown,
}

/// Public state derived only from one canonical materialization operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactStoreOperationStateV1 {
    Admitted,
    Materializing,
    Materialized,
    AlreadyMaterialized,
    Failed,
    Uncertain,
}

/// Fully owned public projection of one verified operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactStoreOperationViewV1 {
    operation: MaterializationOperationV1,
}

impl ArtifactStoreOperationViewV1 {
    fn new(operation: MaterializationOperationV1) -> Self {
        Self { operation }
    }

    #[must_use]
    pub const fn operation(&self) -> &MaterializationOperationV1 {
        &self.operation
    }

    #[must_use]
    pub fn state(&self) -> ArtifactStoreOperationStateV1 {
        use crate::MaterializationTerminalStateV1;

        match self.operation.terminal().map(|terminal| terminal.state()) {
            Some(MaterializationTerminalStateV1::Materialized) => {
                ArtifactStoreOperationStateV1::Materialized
            }
            Some(MaterializationTerminalStateV1::AlreadyMaterialized) => {
                ArtifactStoreOperationStateV1::AlreadyMaterialized
            }
            Some(MaterializationTerminalStateV1::Failed) => ArtifactStoreOperationStateV1::Failed,
            Some(MaterializationTerminalStateV1::Uncertain) => {
                ArtifactStoreOperationStateV1::Uncertain
            }
            None if self.operation.materializing().is_some() => {
                ArtifactStoreOperationStateV1::Materializing
            }
            None => ArtifactStoreOperationStateV1::Admitted,
        }
    }

    #[must_use]
    pub const fn operation_id(&self) -> ArtifactOperationIdV1 {
        self.operation.operation_id()
    }

    #[must_use]
    pub const fn object_ref(&self) -> ArtifactObjectRefV1 {
        self.operation.request().object_ref()
    }

    #[must_use]
    pub fn receipt_ref(&self) -> Option<MaterializationReceiptRefV1> {
        self.operation
            .receipt()
            .map(MaterializationReceiptRefV1::from_receipt)
    }
}

/// Stable store-level failure classes consumed by CLI/Controller adapters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtifactStoreFailureV1 {
    UnsafePath,
    ConfigurationMismatch,
    Conflict,
    Capacity,
    NotFound,
    Contended,
    PublicationUncertain {
        operation: Option<Box<MaterializationOperationV1>>,
    },
    Owner,
    Io,
}

impl ArtifactStoreFailureV1 {
    #[must_use]
    pub fn operation(&self) -> Option<&MaterializationOperationV1> {
        match self {
            Self::PublicationUncertain { operation } => operation.as_deref(),
            Self::UnsafePath
            | Self::ConfigurationMismatch
            | Self::Conflict
            | Self::Capacity
            | Self::NotFound
            | Self::Contended
            | Self::Owner
            | Self::Io => None,
        }
    }
}

impl fmt::Display for ArtifactStoreFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsafePath => "artifact store path is unsafe",
            Self::ConfigurationMismatch => "artifact store configuration authority changed",
            Self::Conflict => "artifact operation conflicts with its durable request",
            Self::Capacity => "artifact store capacity is exhausted",
            Self::NotFound => "artifact operation was not found",
            Self::Contended => "artifact store lock is contended",
            Self::PublicationUncertain { .. } => "artifact operation outcome is uncertain",
            Self::Owner => "artifact store owner state failed strict validation",
            Self::Io => "artifact operation could not complete",
        })
    }
}

impl std::error::Error for ArtifactStoreFailureV1 {}

/// Fully owned result. `changed` remains independent of command success.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactStoreInvocationV1 {
    change: ArtifactStoreChangeV1,
    result: Result<ArtifactStoreOperationViewV1, ArtifactStoreFailureV1>,
}

impl ArtifactStoreInvocationV1 {
    fn success(change: ArtifactStoreChangeV1, operation: MaterializationOperationV1) -> Self {
        Self {
            change,
            result: Ok(ArtifactStoreOperationViewV1::new(operation)),
        }
    }

    fn failure(change: ArtifactStoreChangeV1, failure: ArtifactStoreFailureV1) -> Self {
        Self {
            change,
            result: Err(failure),
        }
    }

    #[must_use]
    pub const fn change(&self) -> ArtifactStoreChangeV1 {
        self.change
    }

    pub fn result(&self) -> Result<&ArtifactStoreOperationViewV1, &ArtifactStoreFailureV1> {
        self.result.as_ref()
    }

    pub fn into_result(self) -> Result<ArtifactStoreOperationViewV1, ArtifactStoreFailureV1> {
        self.result
    }
}

/// Exact read-port classification required by D0b admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactStoreReadFailureV1 {
    ReferenceMismatch,
    ConfigurationMismatch,
    NotFound,
    Contended,
    Owner,
    Io,
}

impl fmt::Display for ArtifactStoreReadFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ReferenceMismatch => "artifact materialization reference does not match",
            Self::ConfigurationMismatch => "artifact store configuration authority changed",
            Self::NotFound => "artifact materialization was not found",
            Self::Contended => "artifact store lock is contended",
            Self::Owner => "artifact store owner state failed strict validation",
            Self::Io => "artifact materialization could not be read",
        })
    }
}

impl std::error::Error for ArtifactStoreReadFailureV1 {}

/// One-shot namespace facade. It never retains an owner handle between calls.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ArtifactStoreV1;

impl ArtifactStoreV1 {
    pub fn materialize(
        authority: &mut dyn ArtifactStoreAuthorityV1,
        request: &MaterializationRequestV1,
        pair: &VerifiedArtifactPairV1,
    ) -> ArtifactStoreInvocationV1 {
        run_materialize(authority, request, pair)
    }

    pub fn query(
        authority: &mut dyn ArtifactStoreAuthorityV1,
        operation_id: ArtifactOperationIdV1,
    ) -> ArtifactStoreInvocationV1 {
        run_query(authority, operation_id)
    }

    pub fn read_verified(
        authority: &mut dyn ArtifactStoreAuthorityV1,
        object_ref: ArtifactObjectRefV1,
        receipt_ref: MaterializationReceiptRefV1,
    ) -> Result<VerifiedMaterializationReadBundleV1, ArtifactStoreReadFailureV1> {
        run_read_verified(authority, object_ref, receipt_ref)
    }
}

fn validate_state_root_path(path: &Path) -> Result<(), ArtifactStoreAuthorityRecheckFailureV1> {
    let text = path
        .to_str()
        .ok_or(ArtifactStoreAuthorityRecheckFailureV1::UnsafePath)?;
    if text == "/"
        || !text.starts_with('/')
        || text.ends_with('/')
        || text.contains("//")
        || text.as_bytes().contains(&0)
        || text.len() > MAX_STATE_ROOT_UTF8_BYTES
    {
        return Err(ArtifactStoreAuthorityRecheckFailureV1::UnsafePath);
    }
    let mut components = path.components();
    if components.next() != Some(Component::RootDir)
        || components
            .clone()
            .any(|component| !matches!(component, Component::Normal(_)))
        || components.next().is_none()
    {
        return Err(ArtifactStoreAuthorityRecheckFailureV1::UnsafePath);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum StoreError {
    UnsafePath,
    ConfigurationMismatch,
    ReferenceMismatch,
    Conflict,
    Capacity,
    NotFound,
    AlreadyExists,
    Contended,
    PublicationUncertain(Option<Box<MaterializationOperationV1>>),
    OwnerEffectUnknown,
    Owner,
    Io,
}

impl StoreError {
    fn into_public(self) -> ArtifactStoreFailureV1 {
        match self {
            Self::UnsafePath => ArtifactStoreFailureV1::UnsafePath,
            Self::ConfigurationMismatch => ArtifactStoreFailureV1::ConfigurationMismatch,
            Self::ReferenceMismatch | Self::Conflict => ArtifactStoreFailureV1::Conflict,
            Self::Capacity => ArtifactStoreFailureV1::Capacity,
            Self::NotFound => ArtifactStoreFailureV1::NotFound,
            Self::AlreadyExists => ArtifactStoreFailureV1::Owner,
            Self::Contended => ArtifactStoreFailureV1::Contended,
            Self::PublicationUncertain(operation) => {
                ArtifactStoreFailureV1::PublicationUncertain { operation }
            }
            Self::OwnerEffectUnknown | Self::Owner => ArtifactStoreFailureV1::Owner,
            Self::Io => ArtifactStoreFailureV1::Io,
        }
    }

    fn into_read(self) -> ArtifactStoreReadFailureV1 {
        match self {
            Self::ReferenceMismatch | Self::Conflict | Self::Capacity => {
                ArtifactStoreReadFailureV1::ReferenceMismatch
            }
            Self::ConfigurationMismatch => ArtifactStoreReadFailureV1::ConfigurationMismatch,
            Self::NotFound => ArtifactStoreReadFailureV1::NotFound,
            Self::AlreadyExists => ArtifactStoreReadFailureV1::Owner,
            Self::Contended => ArtifactStoreReadFailureV1::Contended,
            Self::UnsafePath
            | Self::OwnerEffectUnknown
            | Self::Owner
            | Self::PublicationUncertain(_) => ArtifactStoreReadFailureV1::Owner,
            Self::Io => ArtifactStoreReadFailureV1::Io,
        }
    }

    const fn owner_effect_unknown(&self) -> bool {
        matches!(self, Self::OwnerEffectUnknown)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ChangeTracker {
    proved_owner_commit: bool,
    ambiguous_effect: bool,
}

impl ChangeTracker {
    const fn change(self) -> ArtifactStoreChangeV1 {
        if self.ambiguous_effect {
            ArtifactStoreChangeV1::Unknown
        } else if self.proved_owner_commit {
            ArtifactStoreChangeV1::Changed
        } else {
            ArtifactStoreChangeV1::Unchanged
        }
    }

    fn committed(&mut self) {
        self.proved_owner_commit = true;
        self.ambiguous_effect = false;
    }

    fn ambiguous(&mut self) {
        self.ambiguous_effect = true;
    }

    fn restore(&mut self, checkpoint: Self) {
        *self = checkpoint;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: i128,
    inode: i128,
}

impl FileIdentity {
    fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            device: i128::from(metadata.dev()),
            inode: i128::from(metadata.ino()),
        }
    }
}

struct DirectoryHandle {
    file: File,
    identity: FileIdentity,
    owner_uid: u32,
    owner_gid: u32,
}

struct StateRootHandle {
    parent: DirectoryHandle,
    leaf_name: OsString,
    leaf: DirectoryHandle,
}

struct LockedStore {
    state_root: StateRootHandle,
    root: DirectoryHandle,
    objects: DirectoryHandle,
    lock: Option<File>,
    lock_identity: FileIdentity,
    snapshot_identity: FileIdentity,
    snapshot_bytes: Box<[u8]>,
    snapshot: ArtifactStoreSnapshotV1,
}

impl LockedStore {
    fn release(mut self) -> Result<(), StoreError> {
        let unlock_result = self
            .lock
            .as_ref()
            .ok_or(StoreError::Owner)
            .and_then(|lock| lock.unlock().map_err(|_| StoreError::Io));
        self.lock.take();
        drop(self);
        unlock_result
    }
}

impl Drop for LockedStore {
    fn drop(&mut self) {
        if let Some(lock) = self.lock.take() {
            let _ = lock.unlock();
        }
    }
}

#[derive(Clone, Copy)]
enum LockMode {
    Shared,
    Exclusive,
}

fn authority_binding(
    authority: &mut dyn ArtifactStoreAuthorityV1,
) -> Result<ArtifactStoreAuthorityBindingV1, StoreError> {
    let binding = authority.revalidate().map_err(|failure| match failure {
        ArtifactStoreAuthorityRecheckFailureV1::UnsafePath => StoreError::UnsafePath,
        ArtifactStoreAuthorityRecheckFailureV1::Configuration => StoreError::ConfigurationMismatch,
        ArtifactStoreAuthorityRecheckFailureV1::Io => StoreError::Io,
    })?;
    validate_state_root_path(binding.state_root()).map_err(|_| StoreError::UnsafePath)?;
    Ok(binding)
}

fn require_same_authority(
    authority: &mut dyn ArtifactStoreAuthorityV1,
    expected: &ArtifactStoreAuthorityBindingV1,
) -> Result<(), StoreError> {
    let current = authority_binding(authority)?;
    if current != *expected {
        return Err(StoreError::ConfigurationMismatch);
    }
    Ok(())
}

fn validate_directory_metadata(
    metadata: &Metadata,
    owner_uid: u32,
    owner_gid: u32,
    strict_mode: bool,
) -> Result<(), StoreError> {
    if !metadata.file_type().is_dir() {
        return Err(StoreError::Owner);
    }
    if strict_mode
        && (metadata.uid() != owner_uid
            || metadata.gid() != owner_gid
            || metadata.mode() & MODE_MASK != DIRECTORY_MODE_BITS)
    {
        return Err(StoreError::Owner);
    }
    if !strict_mode && metadata.uid() != 0 && metadata.uid() != owner_uid {
        return Err(StoreError::UnsafePath);
    }
    if !strict_mode && metadata.mode() & 0o022 != 0 {
        return Err(StoreError::UnsafePath);
    }
    Ok(())
}

fn validate_regular_metadata(
    metadata: &Metadata,
    owner_uid: u32,
    owner_gid: u32,
) -> Result<(), StoreError> {
    if !metadata.file_type().is_file()
        || metadata.nlink() != 1
        || metadata.uid() != owner_uid
        || metadata.gid() != owner_gid
        || metadata.mode() & MODE_MASK != FILE_MODE_BITS
    {
        return Err(StoreError::Owner);
    }
    Ok(())
}

fn directory_from_owned(
    owned: OwnedFd,
    owner_uid: u32,
    owner_gid: u32,
    strict_mode: bool,
) -> Result<DirectoryHandle, StoreError> {
    let file = File::from(owned);
    let metadata = file.metadata().map_err(|_| StoreError::Io)?;
    validate_directory_metadata(&metadata, owner_uid, owner_gid, strict_mode)?;
    Ok(DirectoryHandle {
        file,
        identity: FileIdentity::from_metadata(&metadata),
        owner_uid,
        owner_gid,
    })
}

fn open_directory_at(
    parent: &DirectoryHandle,
    name: &OsStr,
) -> Result<DirectoryHandle, StoreError> {
    let owned = openat(
        &parent.file,
        name,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| {
        if matches!(
            error,
            nix::errno::Errno::ENOENT | nix::errno::Errno::ELOOP | nix::errno::Errno::ENOTDIR
        ) {
            StoreError::Owner
        } else {
            StoreError::Io
        }
    })?;
    directory_from_owned(owned, parent.owner_uid, parent.owner_gid, true)
}

fn open_state_leaf_at(
    parent: &DirectoryHandle,
    name: &OsStr,
) -> Result<DirectoryHandle, StoreError> {
    let owned = openat(
        &parent.file,
        name,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| match error {
        nix::errno::Errno::ENOENT => StoreError::NotFound,
        nix::errno::Errno::ELOOP | nix::errno::Errno::ENOTDIR => StoreError::UnsafePath,
        _ => StoreError::Io,
    })?;
    directory_from_owned(owned, parent.owner_uid, parent.owner_gid, true).map_err(|error| {
        if error == StoreError::Owner {
            StoreError::UnsafePath
        } else {
            error
        }
    })
}

fn open_state_parent(path: &Path) -> Result<(DirectoryHandle, OsString), StoreError> {
    validate_state_root_path(path).map_err(|_| StoreError::UnsafePath)?;
    let owner_uid = geteuid().as_raw();
    let owner_gid = getegid().as_raw();
    let leaf_name = path
        .file_name()
        .ok_or(StoreError::UnsafePath)?
        .to_os_string();
    let parent_path = path.parent().ok_or(StoreError::UnsafePath)?;
    let root = open(
        Path::new("/"),
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| StoreError::Io)?;
    let mut current = directory_from_owned(root, owner_uid, owner_gid, false)?;
    for component in parent_path.components().skip(1) {
        let Component::Normal(name) = component else {
            return Err(StoreError::UnsafePath);
        };
        let owned = openat(
            &current.file,
            name,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|error| match error {
            nix::errno::Errno::ENOENT => StoreError::NotFound,
            nix::errno::Errno::ELOOP | nix::errno::Errno::ENOTDIR => StoreError::UnsafePath,
            _ => StoreError::Io,
        })?;
        let next = directory_from_owned(owned, owner_uid, owner_gid, false)?;
        drop(current);
        current = next;
    }
    let metadata = current.file.metadata().map_err(|_| StoreError::Io)?;
    validate_directory_metadata(&metadata, owner_uid, owner_gid, true)?;
    Ok((current, leaf_name))
}

fn pin_existing_state_root(
    binding: &ArtifactStoreAuthorityBindingV1,
) -> Result<StateRootHandle, StoreError> {
    let (parent, leaf_name) = open_state_parent(binding.state_root())?;
    let leaf = open_state_leaf_at(&parent, &leaf_name)?;
    Ok(StateRootHandle {
        parent,
        leaf_name,
        leaf,
    })
}

fn revalidate_directory(handle: &DirectoryHandle) -> Result<(), StoreError> {
    let metadata = handle.file.metadata().map_err(|_| StoreError::Io)?;
    validate_directory_metadata(&metadata, handle.owner_uid, handle.owner_gid, true)?;
    if FileIdentity::from_metadata(&metadata) != handle.identity {
        return Err(StoreError::Owner);
    }
    Ok(())
}

fn reopen_named_directory(
    parent: &DirectoryHandle,
    name: &OsStr,
    expected: FileIdentity,
) -> Result<DirectoryHandle, StoreError> {
    revalidate_directory(parent)?;
    let opened = open_directory_at(parent, name)?;
    if opened.identity != expected {
        return Err(StoreError::Owner);
    }
    Ok(opened)
}

fn scan_names(directory: &DirectoryHandle) -> Result<BTreeSet<OsString>, StoreError> {
    revalidate_directory(directory)?;
    let owned = openat(
        &directory.file,
        ".",
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| StoreError::Io)?;
    let mut stream = Dir::from_fd(owned).map_err(|_| StoreError::Io)?;
    let mut names = BTreeSet::new();
    for entry in stream.iter() {
        let entry = entry.map_err(|_| StoreError::Io)?;
        let bytes = entry.file_name().to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        if bytes.contains(&0) {
            return Err(StoreError::Owner);
        }
        if !names.insert(OsStr::from_bytes(bytes).to_os_string()) {
            return Err(StoreError::Owner);
        }
    }
    Ok(names)
}

fn exact_names(directory: &DirectoryHandle, expected: &[&str]) -> Result<(), StoreError> {
    let actual = scan_names(directory)?;
    let expected = expected.iter().map(OsString::from).collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(StoreError::Owner);
    }
    Ok(())
}

fn open_regular_at(
    parent: &DirectoryHandle,
    name: &OsStr,
    access: OFlag,
) -> Result<(File, FileIdentity), StoreError> {
    let owned = openat(
        &parent.file,
        name,
        access | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| {
        if matches!(
            error,
            nix::errno::Errno::ENOENT | nix::errno::Errno::ELOOP | nix::errno::Errno::ENOTDIR
        ) {
            StoreError::Owner
        } else {
            StoreError::Io
        }
    })?;
    let file = File::from(owned);
    let metadata = file.metadata().map_err(|_| StoreError::Io)?;
    validate_regular_metadata(&metadata, parent.owner_uid, parent.owner_gid)?;
    Ok((file, FileIdentity::from_metadata(&metadata)))
}

fn validate_named_regular(
    parent: &DirectoryHandle,
    name: &OsStr,
    expected_identity: FileIdentity,
    expected_len: Option<u64>,
) -> Result<(), StoreError> {
    let (file, identity) = open_regular_at(parent, name, OFlag::O_RDONLY)?;
    let metadata = file.metadata().map_err(|_| StoreError::Io)?;
    if identity != expected_identity || expected_len.is_some_and(|length| metadata.len() != length)
    {
        return Err(StoreError::Owner);
    }
    drop(file);
    Ok(())
}

fn read_regular_bounded(
    parent: &DirectoryHandle,
    name: &OsStr,
    maximum: usize,
    allow_empty: bool,
) -> Result<(Box<[u8]>, FileIdentity), StoreError> {
    let (mut file, identity) = open_regular_at(parent, name, OFlag::O_RDONLY)?;
    let before = file.metadata().map_err(|_| StoreError::Io)?;
    let length = usize::try_from(before.len()).map_err(|_| StoreError::Owner)?;
    if length > maximum || (!allow_empty && length == 0) {
        return Err(StoreError::Owner);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| StoreError::Io)?;
    bytes.resize(length, 0);
    file.read_exact(&mut bytes).map_err(|_| StoreError::Io)?;
    let mut trailing = [0_u8; 1];
    if file.read(&mut trailing).map_err(|_| StoreError::Io)? != 0 {
        return Err(StoreError::Owner);
    }
    let after = file.metadata().map_err(|_| StoreError::Io)?;
    validate_regular_metadata(&after, parent.owner_uid, parent.owner_gid)?;
    if FileIdentity::from_metadata(&after) != identity || after.len() != before.len() {
        return Err(StoreError::Owner);
    }
    validate_named_regular(parent, name, identity, Some(before.len()))?;
    Ok((bytes.into_boxed_slice(), identity))
}

fn acquire_lock(
    root: &DirectoryHandle,
    mode: LockMode,
) -> Result<(File, FileIdentity), StoreError> {
    let (lock, identity) = open_regular_at(root, OsStr::new(STORE_LOCK_NAME), OFlag::O_RDWR)?;
    if lock.metadata().map_err(|_| StoreError::Io)?.len() != 0 {
        return Err(StoreError::Owner);
    }
    let result = match mode {
        LockMode::Shared => lock.try_lock_shared(),
        LockMode::Exclusive => lock.try_lock(),
    };
    result.map_err(|error| match error {
        TryLockError::WouldBlock => StoreError::Contended,
        TryLockError::Error(_) => StoreError::Io,
    })?;
    if let Err(error) = validate_named_regular(root, OsStr::new(STORE_LOCK_NAME), identity, Some(0))
    {
        let _ = lock.unlock();
        drop(lock);
        return Err(error);
    }
    Ok((lock, identity))
}

fn finish_locked(
    store: LockedStore,
    tracker: ChangeTracker,
    result: Result<MaterializationOperationV1, StoreError>,
) -> ArtifactStoreInvocationV1 {
    let release = store.release();
    match (result, release) {
        (Ok(operation), Ok(())) => ArtifactStoreInvocationV1::success(tracker.change(), operation),
        (Err(error), Ok(())) => {
            ArtifactStoreInvocationV1::failure(tracker.change(), error.into_public())
        }
        (_, Err(error)) => ArtifactStoreInvocationV1::failure(
            if tracker.change() == ArtifactStoreChangeV1::Unchanged {
                ArtifactStoreChangeV1::Unknown
            } else {
                tracker.change()
            },
            error.into_public(),
        ),
    }
}

fn push_lower_hex(output: &mut String, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
}

fn object_directory_name(object_ref: ArtifactObjectRefV1) -> String {
    let mut name = String::with_capacity(131);
    name.push_str("o-");
    push_lower_hex(&mut name, object_ref.payload_digest().as_bytes());
    name.push('-');
    push_lower_hex(&mut name, object_ref.manifest_digest().as_bytes());
    name
}

fn strict_pair_from_directory(
    directory: &DirectoryHandle,
) -> Result<VerifiedArtifactPairV1, StoreError> {
    exact_names(directory, &[MANIFEST_NAME, PAYLOAD_NAME])?;
    let (manifest, _) = read_regular_bounded(directory, OsStr::new(MANIFEST_NAME), 206, false)?;
    let (payload, _) = read_regular_bounded(directory, OsStr::new(PAYLOAD_NAME), 64, false)?;
    VerifiedArtifactPairV1::verify(&manifest, &payload).map_err(|_| StoreError::Owner)
}

fn strict_indexed_object(
    objects: &DirectoryHandle,
    record: &ArtifactObjectRecordV1,
) -> Result<VerifiedArtifactPairV1, StoreError> {
    let name = object_directory_name(record.object_ref());
    let directory = open_directory_at(objects, OsStr::new(&name))?;
    let pair = strict_pair_from_directory(&directory)?;
    if pair.object_ref() != record.object_ref()
        || pair.manifest().payload_len() != record.payload_len()
    {
        return Err(StoreError::Owner);
    }
    let reopened = reopen_named_directory(objects, OsStr::new(&name), directory.identity)?;
    let reopened_pair = strict_pair_from_directory(&reopened)?;
    if reopened_pair != pair {
        return Err(StoreError::Owner);
    }
    drop(reopened);
    drop(directory);
    Ok(pair)
}

#[derive(Debug, Eq, PartialEq)]
struct PartialPairFacts {
    regular_file_bytes: u64,
    entries: Vec<(OsString, FileIdentity)>,
}

fn validate_partial_pair_entry(
    directory: &DirectoryHandle,
    expected_ref: ArtifactObjectRefV1,
) -> Result<PartialPairFacts, StoreError> {
    let names = scan_names(directory)?;
    let allowed = [
        MANIFEST_NEXT_NAME,
        PAYLOAD_NEXT_NAME,
        MANIFEST_NAME,
        PAYLOAD_NAME,
    ]
    .into_iter()
    .map(OsString::from)
    .collect::<BTreeSet<_>>();
    if !names.is_subset(&allowed) {
        return Err(StoreError::Owner);
    }

    let mut regular_bytes = 0_u64;
    let mut entries = Vec::with_capacity(names.len());
    let mut manifest_bytes: Option<Box<[u8]>> = None;
    let mut payload_bytes: Option<Box<[u8]>> = None;
    for name in names {
        let is_manifest =
            name == OsStr::new(MANIFEST_NAME) || name == OsStr::new(MANIFEST_NEXT_NAME);
        let is_temporary =
            name == OsStr::new(MANIFEST_NEXT_NAME) || name == OsStr::new(PAYLOAD_NEXT_NAME);
        let maximum = if is_manifest { 206 } else { 64 };
        let (bytes, identity) = read_regular_bounded(directory, &name, maximum, is_temporary)?;
        entries.push((name, identity));
        regular_bytes = regular_bytes
            .checked_add(u64::try_from(bytes.len()).map_err(|_| StoreError::Owner)?)
            .ok_or(StoreError::Owner)?;
        if bytes.is_empty() {
            continue;
        }
        if is_manifest {
            let manifest = ArtifactManifestV1::decode(&bytes).map_err(|_| StoreError::Owner)?;
            if ArtifactObjectRefV1::from_manifest(&manifest).map_err(|_| StoreError::Owner)?
                != expected_ref
            {
                return Err(StoreError::Owner);
            }
            if manifest_bytes
                .as_ref()
                .is_some_and(|existing| existing.as_ref() != bytes.as_ref())
            {
                return Err(StoreError::Owner);
            }
            manifest_bytes = Some(bytes);
        } else {
            let pair =
                VerifiedArtifactPairV1::from_payload(&bytes).map_err(|_| StoreError::Owner)?;
            if pair.object_ref() != expected_ref {
                return Err(StoreError::Owner);
            }
            if payload_bytes
                .as_ref()
                .is_some_and(|existing| existing.as_ref() != bytes.as_ref())
            {
                return Err(StoreError::Owner);
            }
            payload_bytes = Some(bytes);
        }
    }
    if let (Some(manifest), Some(payload)) = (&manifest_bytes, &payload_bytes)
        && VerifiedArtifactPairV1::verify(manifest, payload)
            .map_err(|_| StoreError::Owner)?
            .object_ref()
            != expected_ref
    {
        return Err(StoreError::Owner);
    }
    Ok(PartialPairFacts {
        regular_file_bytes: regular_bytes,
        entries,
    })
}

fn strict_partial_child(
    objects: &DirectoryHandle,
    name: &OsStr,
    expected_ref: ArtifactObjectRefV1,
) -> Result<(FileIdentity, PartialPairFacts), StoreError> {
    let child = open_directory_at(objects, name)?;
    let identity = child.identity;
    let facts = validate_partial_pair_entry(&child, expected_ref)?;
    let reopened = reopen_named_directory(objects, name, identity)?;
    let reopened_facts = validate_partial_pair_entry(&reopened, expected_ref)?;
    if reopened_facts != facts {
        return Err(StoreError::Owner);
    }
    drop(reopened);
    drop(child);
    Ok((identity, facts))
}

fn validate_candidate_filesystem(
    objects: &DirectoryHandle,
    candidate: ArtifactStoreSnapshotCandidateV1,
) -> Result<ArtifactStoreSnapshotV1, StoreError> {
    let claim = candidate.filesystem_claim().clone();
    let actual = scan_names(objects)?;
    if actual.len() > MAX_OBJECT_DIRECTORIES {
        return Err(StoreError::Owner);
    }
    let mut indexed = BTreeSet::new();
    for record in candidate.objects() {
        let name = object_directory_name(record.object_ref());
        if !indexed.insert(OsString::from(&name)) || !actual.contains(OsStr::new(&name)) {
            return Err(StoreError::Owner);
        }
        strict_indexed_object(objects, record)?;
    }
    let extras = actual.difference(&indexed).cloned().collect::<Vec<_>>();
    match &claim {
        ArtifactFilesystemClaimV1::Stable => {
            if !extras.is_empty() {
                return Err(StoreError::Owner);
            }
        }
        ArtifactFilesystemClaimV1::Materializing {
            operation_id: _,
            object_ref,
        } => {
            let has_matching_indexed = candidate
                .objects()
                .iter()
                .any(|record| record.object_ref() == *object_ref);
            if has_matching_indexed {
                if !extras.is_empty() {
                    return Err(StoreError::Owner);
                }
            } else if extras.len() > 1 {
                return Err(StoreError::Owner);
            } else if let Some(extra) = extras.first() {
                let expected = object_directory_name(*object_ref);
                if extra.as_os_str() != OsStr::new(&expected) {
                    return Err(StoreError::Owner);
                }
                strict_partial_child(objects, extra, *object_ref)?;
            }
        }
        ArtifactFilesystemClaimV1::Quarantined {
            operation_id: _,
            object_ref,
            regular_file_bytes,
        } => {
            if candidate
                .objects()
                .iter()
                .any(|record| record.object_ref() == *object_ref)
                || extras.len() != 1
            {
                return Err(StoreError::Owner);
            }
            let expected = object_directory_name(*object_ref);
            if extras[0].as_os_str() != OsStr::new(&expected) {
                return Err(StoreError::Owner);
            }
            let (_, facts) = strict_partial_child(objects, &extras[0], *object_ref)?;
            if facts.regular_file_bytes != *regular_file_bytes {
                return Err(StoreError::Owner);
            }
        }
    }
    candidate
        .validate_filesystem(claim)
        .map_err(|_| StoreError::Owner)
}

fn decode_snapshot(
    root: &DirectoryHandle,
    name: &str,
) -> Result<(ArtifactStoreSnapshotCandidateV1, Box<[u8]>, FileIdentity), StoreError> {
    let (bytes, identity) =
        read_regular_bounded(root, OsStr::new(name), MAX_SNAPSHOT_BYTES, false)?;
    let candidate = ArtifactStoreSnapshotCandidateV1::decode_canonical(&bytes)
        .map_err(|_| StoreError::Owner)?;
    Ok((candidate, bytes, identity))
}

fn open_final_locked(
    state_root: StateRootHandle,
    binding: &ArtifactStoreAuthorityBindingV1,
    mode: LockMode,
) -> Result<LockedStore, StoreError> {
    let root = open_directory_at(&state_root.leaf, OsStr::new(STORE_ROOT_NAME))?;
    let initial_names = scan_names(&root)?;
    let stable = [STORE_LOCK_NAME, STORE_SNAPSHOT_NAME, OBJECTS_NAME]
        .into_iter()
        .map(OsString::from)
        .collect::<BTreeSet<_>>();
    let with_next = [
        STORE_LOCK_NAME,
        STORE_SNAPSHOT_NAME,
        STORE_SNAPSHOT_NEXT_NAME,
        OBJECTS_NAME,
    ]
    .into_iter()
    .map(OsString::from)
    .collect::<BTreeSet<_>>();
    if initial_names != stable && initial_names != with_next {
        return Err(StoreError::Owner);
    }
    let (lock, lock_identity) = acquire_lock(&root, mode)?;
    let opened = (|| {
        let names = scan_names(&root)?;
        if names != stable && names != with_next {
            return Err(StoreError::Owner);
        }
        let (candidate, snapshot_bytes, snapshot_identity) =
            decode_snapshot(&root, STORE_SNAPSHOT_NAME)?;
        if candidate.config_commitment() != binding.config_commitment() {
            return Err(StoreError::ConfigurationMismatch);
        }
        let objects = open_directory_at(&root, OsStr::new(OBJECTS_NAME))?;
        let snapshot = validate_candidate_filesystem(&objects, candidate)?;
        revalidate_directory(&root)?;
        validate_named_regular(&root, OsStr::new(STORE_LOCK_NAME), lock_identity, Some(0))?;
        validate_named_regular(
            &root,
            OsStr::new(STORE_SNAPSHOT_NAME),
            snapshot_identity,
            Some(u64::try_from(snapshot_bytes.len()).map_err(|_| StoreError::Owner)?),
        )?;
        let (has_final, has_staging) = root_selection(&state_root.leaf)?;
        if !has_final || has_staging {
            return Err(StoreError::Owner);
        }
        let public_root =
            reopen_named_directory(&state_root.leaf, OsStr::new(STORE_ROOT_NAME), root.identity)?;
        let public_names = scan_names(&public_root)?;
        if public_names != stable && public_names != with_next {
            return Err(StoreError::Owner);
        }
        let public_objects =
            reopen_named_directory(&public_root, OsStr::new(OBJECTS_NAME), objects.identity)?;
        validate_named_regular(
            &public_root,
            OsStr::new(STORE_LOCK_NAME),
            lock_identity,
            Some(0),
        )?;
        validate_named_regular(
            &public_root,
            OsStr::new(STORE_SNAPSHOT_NAME),
            snapshot_identity,
            Some(u64::try_from(snapshot_bytes.len()).map_err(|_| StoreError::Owner)?),
        )?;
        let (public_candidate, public_bytes, public_identity) =
            decode_snapshot(&public_root, STORE_SNAPSHOT_NAME)?;
        if public_identity != snapshot_identity || public_bytes != snapshot_bytes {
            return Err(StoreError::Owner);
        }
        if validate_candidate_filesystem(&public_objects, public_candidate)? != snapshot {
            return Err(StoreError::Owner);
        }
        drop(objects);
        Ok((
            public_root,
            public_objects,
            snapshot,
            snapshot_bytes,
            snapshot_identity,
        ))
    })();
    let (public_root, objects, snapshot, snapshot_bytes, snapshot_identity) = match opened {
        Ok(opened) => opened,
        Err(error) => {
            let unlock = lock.unlock().map_err(|_| StoreError::Io);
            drop(lock);
            if unlock.is_err() {
                return Err(StoreError::Io);
            }
            return Err(error);
        }
    };
    drop(root);
    Ok(LockedStore {
        state_root,
        root: public_root,
        objects,
        lock: Some(lock),
        lock_identity,
        snapshot_identity,
        snapshot_bytes,
        snapshot,
    })
}

fn revalidate_public_store(store: &LockedStore, next_expected: bool) -> Result<(), StoreError> {
    let (has_final, has_staging) = root_selection(&store.state_root.leaf)?;
    if !has_final || has_staging {
        return Err(StoreError::Owner);
    }
    let public_root = reopen_named_directory(
        &store.state_root.leaf,
        OsStr::new(STORE_ROOT_NAME),
        store.root.identity,
    )?;
    let expected_names: &[&str] = if next_expected {
        &[
            STORE_LOCK_NAME,
            STORE_SNAPSHOT_NAME,
            STORE_SNAPSHOT_NEXT_NAME,
            OBJECTS_NAME,
        ]
    } else {
        &[STORE_LOCK_NAME, STORE_SNAPSHOT_NAME, OBJECTS_NAME]
    };
    exact_names(&public_root, expected_names)?;
    validate_named_regular(
        &public_root,
        OsStr::new(STORE_LOCK_NAME),
        store.lock_identity,
        Some(0),
    )?;
    validate_named_regular(
        &public_root,
        OsStr::new(STORE_SNAPSHOT_NAME),
        store.snapshot_identity,
        Some(u64::try_from(store.snapshot_bytes.len()).map_err(|_| StoreError::Owner)?),
    )?;
    let public_objects = reopen_named_directory(
        &public_root,
        OsStr::new(OBJECTS_NAME),
        store.objects.identity,
    )?;
    drop(public_objects);
    drop(public_root);
    Ok(())
}

fn permitted_next(
    active: &ArtifactStoreSnapshotV1,
    next: &ArtifactStoreSnapshotV1,
) -> Result<ArtifactOperationIdV1, StoreError> {
    let successor = if next.operations().len() == active.operations().len() + 1
        && next.objects().len() == active.objects().len()
    {
        let operation = next.operations().last().ok_or(StoreError::Owner)?;
        ArtifactSnapshotSuccessorV1::Admission {
            request: operation.request().clone(),
            admission: operation.admission().clone(),
        }
    } else if next.operations().len() == active.operations().len()
        && next.objects().len() == active.objects().len() + 1
    {
        let object = next.objects().last().cloned().ok_or(StoreError::Owner)?;
        let operation_id = active
            .operations()
            .iter()
            .find(|operation| {
                operation.materializing().is_some()
                    && operation.terminal().is_none()
                    && operation.request().object_ref() == object.object_ref()
            })
            .map(MaterializationOperationV1::operation_id)
            .ok_or(StoreError::Owner)?;
        ArtifactSnapshotSuccessorV1::Object {
            operation_id,
            object,
        }
    } else if next.operations().len() == active.operations().len()
        && next.objects().len() == active.objects().len()
    {
        let changed = active
            .operations()
            .iter()
            .zip(next.operations())
            .filter(|(before, after)| before != after)
            .collect::<Vec<_>>();
        if changed.len() != 1 {
            return Err(StoreError::Owner);
        }
        let (before, after) = changed[0];
        if before.operation_id() != after.operation_id()
            || before.request() != after.request()
            || before.admission() != after.admission()
        {
            return Err(StoreError::Owner);
        }
        if before.materializing().is_none()
            && before.terminal().is_none()
            && after.terminal().is_none()
            && after.receipt().is_none()
        {
            ArtifactSnapshotSuccessorV1::Materializing {
                materializing: after.materializing().cloned().ok_or(StoreError::Owner)?,
            }
        } else if before.terminal().is_none() && after.receipt().is_none() {
            ArtifactSnapshotSuccessorV1::Terminal {
                terminal: after.terminal().cloned().ok_or(StoreError::Owner)?,
                quarantine: next.quarantine(),
            }
        } else if before.receipt().is_none() {
            ArtifactSnapshotSuccessorV1::Receipt {
                receipt: after.receipt().cloned().ok_or(StoreError::Owner)?,
            }
        } else {
            return Err(StoreError::Owner);
        }
    } else {
        return Err(StoreError::Owner);
    };
    let operation_id = match &successor {
        ArtifactSnapshotSuccessorV1::Admission { request, .. } => request.operation_id(),
        ArtifactSnapshotSuccessorV1::Materializing { materializing } => {
            materializing.operation_id()
        }
        ArtifactSnapshotSuccessorV1::Object { operation_id, .. } => *operation_id,
        ArtifactSnapshotSuccessorV1::Terminal { terminal, .. } => terminal.operation_id(),
        ArtifactSnapshotSuccessorV1::Receipt { receipt } => receipt.operation_id(),
    };
    if active
        .try_successor(successor)
        .map_err(|_| StoreError::Owner)?
        != *next
    {
        return Err(StoreError::Owner);
    }
    Ok(operation_id)
}

struct ValidatedNext {
    snapshot: ArtifactStoreSnapshotV1,
    operation_id: ArtifactOperationIdV1,
    bytes: Box<[u8]>,
    identity: FileIdentity,
}

fn read_and_validate_next(store: &LockedStore) -> Result<Option<ValidatedNext>, StoreError> {
    let names = scan_names(&store.root)?;
    if !names.contains(OsStr::new(STORE_SNAPSHOT_NEXT_NAME)) {
        return Ok(None);
    }
    let (candidate, bytes, identity) = decode_snapshot(&store.root, STORE_SNAPSHOT_NEXT_NAME)?;
    if candidate.config_commitment() != store.snapshot.config_commitment() {
        return Err(StoreError::Owner);
    }
    let next = validate_candidate_filesystem(&store.objects, candidate)?;
    let operation_id = permitted_next(&store.snapshot, &next)?;
    Ok(Some(ValidatedNext {
        snapshot: next,
        operation_id,
        bytes,
        identity,
    }))
}

fn root_selection(state_root: &DirectoryHandle) -> Result<(bool, bool), StoreError> {
    let names = scan_names(state_root)?;
    Ok((
        names.contains(OsStr::new(STORE_ROOT_NAME)),
        names.contains(OsStr::new(STORE_STAGING_NAME)),
    ))
}

enum InitialStagingInspection {
    NotFound,
    Operation(Box<MaterializationOperationV1>),
    FinalAppeared,
}

fn inspect_initial_staging(
    state_root: &StateRootHandle,
    binding: &ArtifactStoreAuthorityBindingV1,
    operation_id: ArtifactOperationIdV1,
) -> Result<InitialStagingInspection, StoreError> {
    let staging = open_directory_at(&state_root.leaf, OsStr::new(STORE_STAGING_NAME))?;
    let staging_identity = staging.identity;
    let names = scan_names(&staging)?;
    if names.is_empty() {
        drop(staging);
        return Ok(InitialStagingInspection::NotFound);
    }
    if !names.contains(OsStr::new(STORE_LOCK_NAME)) {
        drop(staging);
        return Err(StoreError::Owner);
    }
    let (lock, lock_identity) = acquire_lock(&staging, LockMode::Shared)?;
    let lock_only = [STORE_LOCK_NAME]
        .into_iter()
        .map(OsString::from)
        .collect::<BTreeSet<_>>();
    let lock_objects = [STORE_LOCK_NAME, OBJECTS_NAME]
        .into_iter()
        .map(OsString::from)
        .collect::<BTreeSet<_>>();
    let complete = [STORE_LOCK_NAME, OBJECTS_NAME, STORE_SNAPSHOT_NAME]
        .into_iter()
        .map(OsString::from)
        .collect::<BTreeSet<_>>();
    let result = (|| {
        let (has_final, has_staging) = root_selection(&state_root.leaf)?;
        if has_final && !has_staging {
            return Ok(InitialStagingInspection::FinalAppeared);
        }
        if has_final || !has_staging {
            return Err(StoreError::Owner);
        }
        let reopened_staging = reopen_named_directory(
            &state_root.leaf,
            OsStr::new(STORE_STAGING_NAME),
            staging_identity,
        )?;
        validate_named_regular(
            &reopened_staging,
            OsStr::new(STORE_LOCK_NAME),
            lock_identity,
            Some(0),
        )?;
        let names = scan_names(&reopened_staging)?;
        if names == lock_only {
            Ok(InitialStagingInspection::NotFound)
        } else if names == lock_objects {
            let objects = open_directory_at(&reopened_staging, OsStr::new(OBJECTS_NAME))?;
            exact_names(&objects, &[])?;
            drop(objects);
            Ok(InitialStagingInspection::NotFound)
        } else if names == complete {
            let objects = open_directory_at(&reopened_staging, OsStr::new(OBJECTS_NAME))?;
            let (candidate, _, _) = decode_snapshot(&reopened_staging, STORE_SNAPSHOT_NAME)?;
            let operation = if candidate.config_commitment() != binding.config_commitment() {
                Err(StoreError::ConfigurationMismatch)
            } else {
                let snapshot = validate_candidate_filesystem(&objects, candidate)?;
                initial_staging_operation(&snapshot, operation_id)
            }?;
            drop(objects);
            Ok(InitialStagingInspection::Operation(Box::new(operation)))
        } else {
            Err(StoreError::Owner)
        }
    })();
    let unlock = lock.unlock().map_err(|_| StoreError::Io);
    drop(lock);
    drop(staging);
    unlock?;
    result
}

fn readonly_finish(
    store: LockedStore,
    result: Result<MaterializationOperationV1, StoreError>,
) -> ArtifactStoreInvocationV1 {
    let release = store.release();
    match (result, release) {
        (Ok(operation), Ok(())) => {
            ArtifactStoreInvocationV1::success(ArtifactStoreChangeV1::Unchanged, operation)
        }
        (Err(error), Ok(())) => ArtifactStoreInvocationV1::failure(
            ArtifactStoreChangeV1::Unchanged,
            error.into_public(),
        ),
        (_, Err(error)) => ArtifactStoreInvocationV1::failure(
            ArtifactStoreChangeV1::Unchanged,
            error.into_public(),
        ),
    }
}

fn run_query(
    authority: &mut dyn ArtifactStoreAuthorityV1,
    operation_id: ArtifactOperationIdV1,
) -> ArtifactStoreInvocationV1 {
    let binding = match authority_binding(authority) {
        Ok(binding) => binding,
        Err(error) => {
            return ArtifactStoreInvocationV1::failure(
                ArtifactStoreChangeV1::Unchanged,
                error.into_public(),
            );
        }
    };
    let state_root = match pin_existing_state_root(&binding) {
        Ok(state_root) => state_root,
        Err(error) => {
            return ArtifactStoreInvocationV1::failure(
                ArtifactStoreChangeV1::Unchanged,
                error.into_public(),
            );
        }
    };
    let (has_final, has_staging) = match root_selection(&state_root.leaf) {
        Ok(selection) => selection,
        Err(error) => {
            return ArtifactStoreInvocationV1::failure(
                ArtifactStoreChangeV1::Unchanged,
                error.into_public(),
            );
        }
    };
    if has_final && has_staging {
        return ArtifactStoreInvocationV1::failure(
            ArtifactStoreChangeV1::Unchanged,
            ArtifactStoreFailureV1::Owner,
        );
    }
    if !has_final {
        if has_staging {
            match inspect_initial_staging(&state_root, &binding, operation_id) {
                Ok(InitialStagingInspection::Operation(operation)) => {
                    return ArtifactStoreInvocationV1::failure(
                        ArtifactStoreChangeV1::Unchanged,
                        ArtifactStoreFailureV1::PublicationUncertain {
                            operation: Some(operation),
                        },
                    );
                }
                Ok(InitialStagingInspection::NotFound) => {
                    return ArtifactStoreInvocationV1::failure(
                        ArtifactStoreChangeV1::Unchanged,
                        ArtifactStoreFailureV1::NotFound,
                    );
                }
                Ok(InitialStagingInspection::FinalAppeared) => {}
                Err(error) => {
                    return ArtifactStoreInvocationV1::failure(
                        ArtifactStoreChangeV1::Unchanged,
                        error.into_public(),
                    );
                }
            }
        } else {
            return ArtifactStoreInvocationV1::failure(
                ArtifactStoreChangeV1::Unchanged,
                ArtifactStoreFailureV1::NotFound,
            );
        }
    }
    let store = match open_final_locked(state_root, &binding, LockMode::Shared) {
        Ok(store) => store,
        Err(error) => {
            return ArtifactStoreInvocationV1::failure(
                ArtifactStoreChangeV1::Unchanged,
                error.into_public(),
            );
        }
    };
    let result = (|| {
        require_same_authority(authority, &binding)?;
        let active_operation = store.snapshot.operation(operation_id).cloned();
        if let Some(next) = read_and_validate_next(&store)? {
            if active_operation
                .as_ref()
                .is_some_and(|operation| operation.terminal().is_some())
            {
                return Ok(active_operation.expect("checked as present"));
            }
            if next.operation_id == operation_id {
                return Err(StoreError::PublicationUncertain(
                    next.snapshot.operation(operation_id).cloned().map(Box::new),
                ));
            }
            return Err(StoreError::Owner);
        }
        active_operation.ok_or(StoreError::NotFound)
    })();
    readonly_finish(store, result)
}

fn run_read_verified(
    authority: &mut dyn ArtifactStoreAuthorityV1,
    object_ref: ArtifactObjectRefV1,
    receipt_ref: MaterializationReceiptRefV1,
) -> Result<VerifiedMaterializationReadBundleV1, ArtifactStoreReadFailureV1> {
    let binding = authority_binding(authority).map_err(StoreError::into_read)?;
    let state_root = pin_existing_state_root(&binding).map_err(StoreError::into_read)?;
    let (has_final, has_staging) =
        root_selection(&state_root.leaf).map_err(StoreError::into_read)?;
    if has_staging || !has_final {
        return Err(if has_staging {
            ArtifactStoreReadFailureV1::Owner
        } else {
            ArtifactStoreReadFailureV1::NotFound
        });
    }
    let store =
        open_final_locked(state_root, &binding, LockMode::Shared).map_err(StoreError::into_read)?;
    let result = (|| {
        require_same_authority(authority, &binding)?;
        let next = read_and_validate_next(&store)?;
        let operation = store
            .snapshot
            .operation(receipt_ref.operation_id())
            .ok_or(StoreError::ReferenceMismatch)?;
        if next.is_some() && (operation.terminal().is_none() || operation.receipt().is_none()) {
            return Err(StoreError::Owner);
        }
        let pair = match operation.terminal().map(MaterializationTerminalV1::state) {
            Some(MaterializationTerminalStateV1::Materialized)
            | Some(MaterializationTerminalStateV1::AlreadyMaterialized) => {
                let record = store
                    .snapshot
                    .objects()
                    .iter()
                    .find(|record| record.object_ref() == object_ref)
                    .ok_or(StoreError::ReferenceMismatch)?;
                Some(strict_indexed_object(&store.objects, record)?)
            }
            Some(MaterializationTerminalStateV1::Failed)
            | Some(MaterializationTerminalStateV1::Uncertain) => None,
            None => return Err(StoreError::ReferenceMismatch),
        };
        store
            .snapshot
            .verified_read_bundle(receipt_ref, object_ref, pair)
            .map_err(|_| StoreError::ReferenceMismatch)
    })();
    let release = store.release();
    match (result, release) {
        (Ok(bundle), Ok(())) => Ok(bundle),
        (Err(error), Ok(())) => Err(error.into_read()),
        (_, Err(error)) => Err(error.into_read()),
    }
}

fn pin_or_create_state_root(
    authority: &mut dyn ArtifactStoreAuthorityV1,
    binding: &ArtifactStoreAuthorityBindingV1,
    tracker: &mut ChangeTracker,
) -> Result<StateRootHandle, StoreError> {
    let (parent, leaf_name) =
        open_state_parent(binding.state_root()).map_err(|error| match error {
            StoreError::NotFound => StoreError::UnsafePath,
            other => other,
        })?;
    match open_state_leaf_at(&parent, &leaf_name) {
        Ok(leaf) => Ok(StateRootHandle {
            parent,
            leaf_name,
            leaf,
        }),
        Err(StoreError::NotFound) => {
            revalidate_directory(&parent)?;
            match mkdirat(&parent.file, leaf_name.as_os_str(), DIRECTORY_MODE) {
                Ok(()) => {}
                Err(nix::errno::Errno::EEXIST) => {
                    let expected_parent = parent.identity;
                    parent.file.sync_all().map_err(|_| StoreError::Io)?;
                    drop(parent);
                    let (reopened_parent, reopened_leaf_name) =
                        open_state_parent(binding.state_root())?;
                    if reopened_parent.identity != expected_parent
                        || reopened_leaf_name != leaf_name
                    {
                        return Err(StoreError::Owner);
                    }
                    let reopened_leaf =
                        open_state_leaf_at(&reopened_parent, &leaf_name).map_err(|error| {
                            match error {
                                StoreError::NotFound => StoreError::PublicationUncertain(None),
                                other => other,
                            }
                        })?;
                    require_same_authority(authority, binding)?;
                    return Ok(StateRootHandle {
                        parent: reopened_parent,
                        leaf_name,
                        leaf: reopened_leaf,
                    });
                }
                Err(_) => return Err(StoreError::Io),
            }
            let mut durability_proven = false;
            let result = (|| {
                parent.file.sync_all().map_err(|_| StoreError::Io)?;
                let leaf = open_directory_at(&parent, &leaf_name)?;
                exact_names(&leaf, &[])?;
                let expected_parent = parent.identity;
                let expected_leaf = leaf.identity;
                drop(leaf);
                drop(parent);
                let (reopened_parent, reopened_leaf_name) =
                    open_state_parent(binding.state_root())?;
                if reopened_parent.identity != expected_parent || reopened_leaf_name != leaf_name {
                    return Err(StoreError::Owner);
                }
                let reopened_leaf =
                    reopen_named_directory(&reopened_parent, &leaf_name, expected_leaf)?;
                exact_names(&reopened_leaf, &[])?;
                durability_proven = true;
                require_same_authority(authority, binding)?;
                Ok(StateRootHandle {
                    parent: reopened_parent,
                    leaf_name,
                    leaf: reopened_leaf,
                })
            })();
            if result.is_err() && !durability_proven {
                tracker.ambiguous();
            }
            result
        }
        Err(error) => Err(error),
    }
}

fn named_identity(
    parent: &DirectoryHandle,
    name: &OsStr,
) -> Result<Option<FileIdentity>, StoreError> {
    let metadata = match fstatat(&parent.file, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(metadata) => metadata,
        Err(nix::errno::Errno::ENOENT) => return Ok(None),
        Err(_) => return Err(StoreError::Io),
    };
    Ok(Some(FileIdentity {
        device: i128::from(metadata.st_dev),
        inode: i128::from(metadata.st_ino),
    }))
}

fn validate_cleanup_identity(
    actual: Option<FileIdentity>,
    expected: FileIdentity,
) -> Result<(), StoreError> {
    if actual != Some(expected) {
        return Err(StoreError::Owner);
    }
    Ok(())
}

fn cleanup_created_regular(
    parent: &DirectoryHandle,
    name: &str,
    file: &File,
    identity: FileIdentity,
) -> Result<(), StoreError> {
    validate_cleanup_identity(named_identity(parent, OsStr::new(name))?, identity)?;
    unlinkat(&parent.file, name, UnlinkatFlags::NoRemoveDir).map_err(|_| StoreError::Io)?;
    let metadata = file.metadata();
    let parent_sync = parent.file.sync_all();
    let absence = (|| {
        revalidate_directory(parent)?;
        if named_identity(parent, OsStr::new(name))?.is_some()
            || scan_names(parent)?.contains(OsStr::new(name))
        {
            return Err(StoreError::Owner);
        }
        Ok(())
    })();
    let metadata = metadata.map_err(|_| StoreError::Io)?;
    if FileIdentity::from_metadata(&metadata) != identity || metadata.nlink() != 0 {
        return Err(StoreError::Owner);
    }
    parent_sync.map_err(|_| StoreError::Io)?;
    absence
}

fn create_regular(
    parent: &DirectoryHandle,
    name: &str,
    access: OFlag,
) -> Result<(File, FileIdentity), StoreError> {
    let owned = openat(
        &parent.file,
        name,
        access | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
        FILE_MODE,
    )
    .map_err(|error| {
        if error == nix::errno::Errno::EEXIST {
            StoreError::AlreadyExists
        } else {
            StoreError::Io
        }
    })?;
    let file = File::from(owned);
    let identity = FileIdentity::from_metadata(
        &file
            .metadata()
            .map_err(|_| StoreError::OwnerEffectUnknown)?,
    );
    let prepared = (|| {
        fchmod(&file, FILE_MODE).map_err(|_| StoreError::Io)?;
        let metadata = file.metadata().map_err(|_| StoreError::Io)?;
        validate_regular_metadata(&metadata, parent.owner_uid, parent.owner_gid)?;
        if FileIdentity::from_metadata(&metadata) != identity || metadata.len() != 0 {
            return Err(StoreError::Owner);
        }
        Ok(())
    })();
    if let Err(error) = prepared {
        if cleanup_created_regular(parent, name, &file, identity).is_err() {
            return Err(StoreError::OwnerEffectUnknown);
        }
        return Err(error);
    }
    Ok((file, identity))
}

fn write_new_exact(
    parent: &DirectoryHandle,
    name: &str,
    bytes: &[u8],
) -> Result<FileIdentity, StoreError> {
    let (mut file, identity) = create_regular(parent, name, OFlag::O_WRONLY)?;
    file.write_all(bytes).map_err(|_| StoreError::Io)?;
    file.sync_all().map_err(|_| StoreError::Io)?;
    drop(file);
    let (reopened, reopened_bytes_identity) =
        read_regular_bounded(parent, OsStr::new(name), bytes.len(), bytes.is_empty())?;
    if reopened_bytes_identity != identity || reopened.as_ref() != bytes {
        return Err(StoreError::Owner);
    }
    Ok(identity)
}

fn create_and_lock_staging(staging: &DirectoryHandle) -> Result<(File, FileIdentity), StoreError> {
    let (lock, identity) = create_regular(staging, STORE_LOCK_NAME, OFlag::O_RDWR)?;
    let durable_prefix = (|| {
        lock.sync_all().map_err(|_| StoreError::Io)?;
        staging.file.sync_all().map_err(|_| StoreError::Io)?;
        validate_named_regular(staging, OsStr::new(STORE_LOCK_NAME), identity, Some(0))
    })();
    if durable_prefix.is_err() {
        drop(lock);
        return Err(StoreError::OwnerEffectUnknown);
    }
    lock.try_lock().map_err(|error| match error {
        TryLockError::WouldBlock => StoreError::Contended,
        TryLockError::Error(_) => StoreError::Io,
    })?;
    Ok((lock, identity))
}

fn classify_bootstrap_lock_collision(error: StoreError) -> StoreError {
    match error {
        StoreError::Contended => StoreError::Contended,
        _ => StoreError::PublicationUncertain(None),
    }
}

fn create_objects_directory(staging: &DirectoryHandle) -> Result<DirectoryHandle, StoreError> {
    mkdirat(&staging.file, OBJECTS_NAME, DIRECTORY_MODE).map_err(|error| {
        if error == nix::errno::Errno::EEXIST {
            StoreError::Owner
        } else {
            StoreError::Io
        }
    })?;
    let objects = open_directory_at(staging, OsStr::new(OBJECTS_NAME))?;
    exact_names(&objects, &[])?;
    objects.file.sync_all().map_err(|_| StoreError::Io)?;
    staging.file.sync_all().map_err(|_| StoreError::Io)?;
    let reopened = reopen_named_directory(staging, OsStr::new(OBJECTS_NAME), objects.identity)?;
    exact_names(&reopened, &[])?;
    drop(objects);
    Ok(reopened)
}

fn draw_store_instance() -> Result<ArtifactStoreInstanceV1, StoreError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| StoreError::Io)?;
    ArtifactStoreInstanceV1::try_from_bytes(bytes).map_err(|_| StoreError::Io)
}

fn seal_initial_prefix(
    mut state_root: StateRootHandle,
    staging: DirectoryHandle,
    objects: DirectoryHandle,
    lock: &File,
    lock_identity: FileIdentity,
    snapshot: Option<(FileIdentity, &[u8])>,
) -> Result<(StateRootHandle, DirectoryHandle, DirectoryHandle), StoreError> {
    lock.sync_all().map_err(|_| StoreError::Io)?;
    validate_named_regular(
        &staging,
        OsStr::new(STORE_LOCK_NAME),
        lock_identity,
        Some(0),
    )?;
    if let Some((snapshot_identity, snapshot_bytes)) = snapshot {
        let (snapshot_file, identity) =
            open_regular_at(&staging, OsStr::new(STORE_SNAPSHOT_NAME), OFlag::O_RDONLY)?;
        if identity != snapshot_identity {
            return Err(StoreError::Owner);
        }
        snapshot_file.sync_all().map_err(|_| StoreError::Io)?;
        drop(snapshot_file);
        let (reopened, reopened_identity) = read_regular_bounded(
            &staging,
            OsStr::new(STORE_SNAPSHOT_NAME),
            MAX_SNAPSHOT_BYTES,
            false,
        )?;
        if reopened_identity != snapshot_identity || reopened.as_ref() != snapshot_bytes {
            return Err(StoreError::Owner);
        }
    }
    exact_names(&objects, &[])?;
    objects.file.sync_all().map_err(|_| StoreError::Io)?;
    staging.file.sync_all().map_err(|_| StoreError::Io)?;
    state_root
        .leaf
        .file
        .sync_all()
        .map_err(|_| StoreError::Io)?;

    let staging_identity = staging.identity;
    let objects_identity = objects.identity;
    let reopened_leaf = reopen_named_directory(
        &state_root.parent,
        &state_root.leaf_name,
        state_root.leaf.identity,
    )?;
    drop(state_root.leaf);
    state_root.leaf = reopened_leaf;
    let reopened_staging = reopen_named_directory(
        &state_root.leaf,
        OsStr::new(STORE_STAGING_NAME),
        staging_identity,
    )?;
    let expected_names: &[&str] = if snapshot.is_some() {
        &[STORE_LOCK_NAME, OBJECTS_NAME, STORE_SNAPSHOT_NAME]
    } else {
        &[STORE_LOCK_NAME, OBJECTS_NAME]
    };
    exact_names(&reopened_staging, expected_names)?;
    validate_named_regular(
        &reopened_staging,
        OsStr::new(STORE_LOCK_NAME),
        lock_identity,
        Some(0),
    )?;
    let reopened_objects = reopen_named_directory(
        &reopened_staging,
        OsStr::new(OBJECTS_NAME),
        objects_identity,
    )?;
    exact_names(&reopened_objects, &[])?;
    if let Some((snapshot_identity, snapshot_bytes)) = snapshot {
        let (reopened, identity) = read_regular_bounded(
            &reopened_staging,
            OsStr::new(STORE_SNAPSHOT_NAME),
            MAX_SNAPSHOT_BYTES,
            false,
        )?;
        if identity != snapshot_identity || reopened.as_ref() != snapshot_bytes {
            return Err(StoreError::Owner);
        }
    }
    drop(objects);
    drop(staging);
    Ok((state_root, reopened_staging, reopened_objects))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InitialRenameState {
    Staging,
    Final,
    Ambiguous,
}

struct InitialDirectoryClaim<'a> {
    directory_identity: FileIdentity,
    objects_identity: FileIdentity,
    lock_identity: FileIdentity,
    snapshot_identity: FileIdentity,
    snapshot_bytes: &'a [u8],
}

fn initial_directory_matches(
    parent: &DirectoryHandle,
    name: &str,
    claim: &InitialDirectoryClaim<'_>,
) -> bool {
    let validation = (|| {
        let directory = reopen_named_directory(parent, OsStr::new(name), claim.directory_identity)?;
        exact_names(
            &directory,
            &[STORE_LOCK_NAME, STORE_SNAPSHOT_NAME, OBJECTS_NAME],
        )?;
        validate_named_regular(
            &directory,
            OsStr::new(STORE_LOCK_NAME),
            claim.lock_identity,
            Some(0),
        )?;
        let (snapshot_bytes, snapshot_identity) = read_regular_bounded(
            &directory,
            OsStr::new(STORE_SNAPSHOT_NAME),
            MAX_SNAPSHOT_BYTES,
            false,
        )?;
        if snapshot_identity != claim.snapshot_identity
            || snapshot_bytes.as_ref() != claim.snapshot_bytes
        {
            return Err(StoreError::Owner);
        }
        let objects =
            reopen_named_directory(&directory, OsStr::new(OBJECTS_NAME), claim.objects_identity)?;
        exact_names(&objects, &[])?;
        drop(objects);
        drop(directory);
        Ok(())
    })();
    validation.is_ok()
}

fn classify_initial_rename_error(
    state_root: &StateRootHandle,
    claim: &InitialDirectoryClaim<'_>,
) -> InitialRenameState {
    match root_selection(&state_root.leaf) {
        Ok((false, true))
            if initial_directory_matches(&state_root.leaf, STORE_STAGING_NAME, claim) =>
        {
            InitialRenameState::Staging
        }
        Ok((true, false))
            if initial_directory_matches(&state_root.leaf, STORE_ROOT_NAME, claim) =>
        {
            InitialRenameState::Final
        }
        _ => InitialRenameState::Ambiguous,
    }
}

struct InitialPublicationInput {
    state_root: StateRootHandle,
    staging: DirectoryHandle,
    objects_identity: FileIdentity,
    lock: File,
    lock_identity: FileIdentity,
    snapshot: ArtifactStoreSnapshotV1,
    snapshot_bytes: Box<[u8]>,
    snapshot_identity: FileIdentity,
}

fn publish_initial_staging(
    authority: &mut dyn ArtifactStoreAuthorityV1,
    binding: &ArtifactStoreAuthorityBindingV1,
    input: InitialPublicationInput,
    tracker: &mut ChangeTracker,
) -> Result<LockedStore, StoreError> {
    let InitialPublicationInput {
        mut state_root,
        staging,
        objects_identity,
        lock,
        lock_identity,
        snapshot,
        snapshot_bytes,
        snapshot_identity,
    } = input;
    let mut lock = Some(lock);
    let staging_identity = staging.identity;
    let result = (|| {
        exact_names(
            &staging,
            &[STORE_LOCK_NAME, STORE_SNAPSHOT_NAME, OBJECTS_NAME],
        )?;
        validate_named_regular(
            &staging,
            OsStr::new(STORE_LOCK_NAME),
            lock_identity,
            Some(0),
        )?;
        validate_named_regular(
            &staging,
            OsStr::new(STORE_SNAPSHOT_NAME),
            snapshot_identity,
            Some(u64::try_from(snapshot_bytes.len()).map_err(|_| StoreError::Owner)?),
        )?;
        let objects = reopen_named_directory(&staging, OsStr::new(OBJECTS_NAME), objects_identity)?;
        exact_names(&objects, &[])?;
        staging.file.sync_all().map_err(|_| StoreError::Io)?;
        state_root
            .leaf
            .file
            .sync_all()
            .map_err(|_| StoreError::Io)?;
        let reopened_staging = reopen_named_directory(
            &state_root.leaf,
            OsStr::new(STORE_STAGING_NAME),
            staging.identity,
        )?;
        drop(objects);
        drop(staging);
        require_same_authority(authority, binding)?;
        revalidate_directory(&state_root.leaf)?;
        let (has_final, has_staging) = root_selection(&state_root.leaf)?;
        if has_final || !has_staging {
            return Err(StoreError::Owner);
        }
        let publish_checkpoint = *tracker;
        tracker.ambiguous();
        if renameat_with(
            &state_root.leaf.file,
            STORE_STAGING_NAME,
            &state_root.leaf.file,
            STORE_ROOT_NAME,
            RenameFlags::NOREPLACE,
        )
        .is_err()
        {
            let claim = InitialDirectoryClaim {
                directory_identity: staging_identity,
                objects_identity,
                lock_identity,
                snapshot_identity,
                snapshot_bytes: &snapshot_bytes,
            };
            match classify_initial_rename_error(&state_root, &claim) {
                InitialRenameState::Staging => {
                    tracker.restore(publish_checkpoint);
                    drop(reopened_staging);
                    return Err(StoreError::Io);
                }
                InitialRenameState::Final => {}
                InitialRenameState::Ambiguous => {
                    drop(reopened_staging);
                    return Err(StoreError::PublicationUncertain(None));
                }
            }
        }
        drop(reopened_staging);
        state_root
            .leaf
            .file
            .sync_all()
            .map_err(|_| StoreError::PublicationUncertain(None))?;
        let reopened_leaf = reopen_named_directory(
            &state_root.parent,
            &state_root.leaf_name,
            state_root.leaf.identity,
        )
        .map_err(|_| StoreError::Owner)?;
        drop(state_root.leaf);
        state_root.leaf = reopened_leaf;
        require_same_authority(authority, binding).map_err(|error| match error {
            StoreError::ConfigurationMismatch => StoreError::Owner,
            other => other,
        })?;
        let root = reopen_named_directory(
            &state_root.leaf,
            OsStr::new(STORE_ROOT_NAME),
            staging_identity,
        )?;
        exact_names(&root, &[STORE_LOCK_NAME, STORE_SNAPSHOT_NAME, OBJECTS_NAME])?;
        validate_named_regular(&root, OsStr::new(STORE_LOCK_NAME), lock_identity, Some(0))?;
        validate_named_regular(
            &root,
            OsStr::new(STORE_SNAPSHOT_NAME),
            snapshot_identity,
            Some(u64::try_from(snapshot_bytes.len()).map_err(|_| StoreError::Owner)?),
        )?;
        let objects = open_directory_at(&root, OsStr::new(OBJECTS_NAME))?;
        if objects.identity != objects_identity {
            return Err(StoreError::Owner);
        }
        let candidate = ArtifactStoreSnapshotCandidateV1::decode_canonical(&snapshot_bytes)
            .map_err(|_| StoreError::Owner)?;
        let verified = validate_candidate_filesystem(&objects, candidate)?;
        if verified != snapshot {
            return Err(StoreError::Owner);
        }
        tracker.committed();
        Ok(LockedStore {
            state_root,
            root,
            objects,
            lock: Some(lock.take().ok_or(StoreError::Owner)?),
            lock_identity,
            snapshot_identity,
            snapshot_bytes,
            snapshot,
        })
    })();
    if result.is_err()
        && let Some(lock) = lock.take()
    {
        let _ = lock.unlock();
        drop(lock);
    }
    result
}

fn open_or_initialize_store(
    authority: &mut dyn ArtifactStoreAuthorityV1,
    binding: &ArtifactStoreAuthorityBindingV1,
    request: &MaterializationRequestV1,
    tracker: &mut ChangeTracker,
) -> Result<LockedStore, StoreError> {
    let state_root = pin_or_create_state_root(authority, binding, tracker)?;
    let (has_final, has_staging) = root_selection(&state_root.leaf)?;
    if has_final && has_staging {
        return Err(StoreError::Owner);
    }
    if has_final {
        return open_final_locked(state_root, binding, LockMode::Exclusive);
    }

    let staging = if has_staging {
        match open_directory_at(&state_root.leaf, OsStr::new(STORE_STAGING_NAME)) {
            Ok(staging) => staging,
            Err(error) => {
                let (final_after_race, staging_after_race) = root_selection(&state_root.leaf)?;
                if final_after_race && !staging_after_race {
                    return open_final_locked(state_root, binding, LockMode::Exclusive);
                }
                if final_after_race && staging_after_race {
                    return Err(StoreError::Owner);
                }
                if !staging_after_race {
                    return Err(StoreError::PublicationUncertain(None));
                }
                return Err(error);
            }
        }
    } else {
        let staging_checkpoint = *tracker;
        tracker.ambiguous();
        revalidate_directory(&state_root.leaf)?;
        if mkdirat(&state_root.leaf.file, STORE_STAGING_NAME, DIRECTORY_MODE).is_err() {
            tracker.restore(staging_checkpoint);
            let (final_after_race, staging_after_race) = root_selection(&state_root.leaf)?;
            if final_after_race && !staging_after_race {
                return open_final_locked(state_root, binding, LockMode::Exclusive);
            }
            if final_after_race && staging_after_race {
                return Err(StoreError::Owner);
            }
            if !staging_after_race {
                return Err(StoreError::PublicationUncertain(None));
            }
            open_directory_at(&state_root.leaf, OsStr::new(STORE_STAGING_NAME))
                .map_err(|_| StoreError::PublicationUncertain(None))?
        } else {
            let created = open_directory_at(&state_root.leaf, OsStr::new(STORE_STAGING_NAME))?;
            let created_identity = created.identity;
            exact_names(&created, &[])?;
            created.file.sync_all().map_err(|_| StoreError::Io)?;
            state_root
                .leaf
                .file
                .sync_all()
                .map_err(|_| StoreError::Io)?;
            let reopened = reopen_named_directory(
                &state_root.leaf,
                OsStr::new(STORE_STAGING_NAME),
                created_identity,
            )?;
            exact_names(&reopened, &[])?;
            drop(created);
            tracker.restore(staging_checkpoint);
            reopened
        }
    };
    let names_before_lock = scan_names(&staging)?;
    if !names_before_lock.is_empty() && !names_before_lock.contains(OsStr::new(STORE_LOCK_NAME)) {
        return Err(StoreError::Owner);
    }

    let (lock, lock_identity) = if names_before_lock.is_empty() {
        let lock_checkpoint = *tracker;
        tracker.ambiguous();
        match create_and_lock_staging(&staging) {
            Ok(locked) => {
                tracker.restore(lock_checkpoint);
                locked
            }
            Err(StoreError::AlreadyExists) => {
                tracker.restore(lock_checkpoint);
                acquire_lock(&staging, LockMode::Exclusive)
                    .map_err(classify_bootstrap_lock_collision)?
            }
            Err(error) => {
                if !error.owner_effect_unknown() {
                    tracker.restore(lock_checkpoint);
                }
                return Err(error);
            }
        }
    } else {
        acquire_lock(&staging, LockMode::Exclusive)?
    };

    let (final_after_lock, staging_after_lock) = match root_selection(&state_root.leaf) {
        Ok(selection) => selection,
        Err(error) => {
            let unlock = lock.unlock().map_err(|_| StoreError::Io);
            drop(lock);
            drop(staging);
            unlock?;
            return Err(error);
        }
    };
    if final_after_lock && !staging_after_lock {
        let unlock = lock.unlock().map_err(|_| StoreError::Io);
        drop(lock);
        drop(staging);
        unlock?;
        return open_final_locked(state_root, binding, LockMode::Exclusive);
    }
    if final_after_lock && staging_after_lock {
        let unlock = lock.unlock().map_err(|_| StoreError::Io);
        drop(lock);
        drop(staging);
        unlock?;
        return Err(StoreError::Owner);
    }
    if !staging_after_lock {
        let unlock = lock.unlock().map_err(|_| StoreError::Io);
        drop(lock);
        drop(staging);
        unlock?;
        return Err(StoreError::PublicationUncertain(None));
    }

    let names = match scan_names(&staging) {
        Ok(names) => names,
        Err(error) => {
            let unlock = lock.unlock().map_err(|_| StoreError::Io);
            drop(lock);
            drop(staging);
            unlock?;
            return Err(error);
        }
    };
    let lock_only = [STORE_LOCK_NAME]
        .into_iter()
        .map(OsString::from)
        .collect::<BTreeSet<_>>();
    let lock_objects = [STORE_LOCK_NAME, OBJECTS_NAME]
        .into_iter()
        .map(OsString::from)
        .collect::<BTreeSet<_>>();
    let complete = [STORE_LOCK_NAME, OBJECTS_NAME, STORE_SNAPSHOT_NAME]
        .into_iter()
        .map(OsString::from)
        .collect::<BTreeSet<_>>();
    if names != lock_only && names != lock_objects && names != complete {
        let unlock = lock.unlock().map_err(|_| StoreError::Io);
        drop(lock);
        drop(staging);
        unlock?;
        return Err(StoreError::Owner);
    }
    continue_initialization(
        authority,
        binding,
        state_root,
        staging,
        names,
        lock_only,
        complete,
        lock,
        lock_identity,
        request,
        tracker,
    )
}

fn validate_initial_snapshot_shape(snapshot: &ArtifactStoreSnapshotV1) -> Result<(), StoreError> {
    if snapshot.snapshot_sequence().get() != 1
        || snapshot.operation_high_water() != 1
        || snapshot.object_high_water() != 0
        || !snapshot.objects().is_empty()
        || snapshot.operations().len() != 1
        || snapshot.quarantine() != ArtifactQuarantineFactsV1::Absent
    {
        return Err(StoreError::Owner);
    }
    let operation = &snapshot.operations()[0];
    if operation.admission().operation_sequence().get() != 1
        || operation.materializing().is_some()
        || operation.terminal().is_some()
        || operation.receipt().is_some()
    {
        return Err(StoreError::Owner);
    }
    Ok(())
}

fn initial_staging_operation(
    snapshot: &ArtifactStoreSnapshotV1,
    operation_id: ArtifactOperationIdV1,
) -> Result<MaterializationOperationV1, StoreError> {
    validate_initial_snapshot_shape(snapshot)?;
    let operation = snapshot.operations().first().ok_or(StoreError::Owner)?;
    if operation.operation_id() != operation_id {
        return Err(StoreError::Owner);
    }
    Ok(operation.clone())
}

fn validate_initial_request(
    snapshot: &ArtifactStoreSnapshotV1,
    request: &MaterializationRequestV1,
) -> Result<(), StoreError> {
    validate_initial_snapshot_shape(snapshot)?;
    let operation = snapshot.operations().first().ok_or(StoreError::Owner)?;
    if operation.operation_id() != request.operation_id() {
        return Err(StoreError::Owner);
    }
    if operation.request() != request {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn continue_initialization(
    authority: &mut dyn ArtifactStoreAuthorityV1,
    binding: &ArtifactStoreAuthorityBindingV1,
    state_root: StateRootHandle,
    staging: DirectoryHandle,
    names: BTreeSet<OsString>,
    lock_only: BTreeSet<OsString>,
    complete: BTreeSet<OsString>,
    lock: File,
    lock_identity: FileIdentity,
    request: &MaterializationRequestV1,
    tracker: &mut ChangeTracker,
) -> Result<LockedStore, StoreError> {
    let mut lock = Some(lock);
    let result = (|| {
        let (final_after_lock, staging_after_lock) = root_selection(&state_root.leaf)?;
        if final_after_lock || !staging_after_lock {
            return Err(StoreError::Owner);
        }
        let prefix_checkpoint = *tracker;
        let objects = if names == lock_only {
            tracker.ambiguous();
            create_objects_directory(&staging)?
        } else {
            let objects = open_directory_at(&staging, OsStr::new(OBJECTS_NAME))?;
            exact_names(&objects, &[])?;
            objects
        };

        if names == complete {
            let (candidate, snapshot_bytes, snapshot_identity) =
                decode_snapshot(&staging, STORE_SNAPSHOT_NAME)?;
            if candidate.config_commitment() != binding.config_commitment() {
                return Err(StoreError::ConfigurationMismatch);
            }
            let snapshot = validate_candidate_filesystem(&objects, candidate)?;
            validate_initial_request(&snapshot, request)?;
            let (state_root, staging, objects) = seal_initial_prefix(
                state_root,
                staging,
                objects,
                lock.as_ref().ok_or(StoreError::Owner)?,
                lock_identity,
                Some((snapshot_identity, &snapshot_bytes)),
            )?;
            require_same_authority(authority, binding)?;
            let (sealed_candidate, sealed_bytes, sealed_identity) =
                decode_snapshot(&staging, STORE_SNAPSHOT_NAME)?;
            if sealed_identity != snapshot_identity || sealed_bytes != snapshot_bytes {
                return Err(StoreError::Owner);
            }
            if validate_candidate_filesystem(&objects, sealed_candidate)? != snapshot {
                return Err(StoreError::Owner);
            }
            let objects_identity = objects.identity;
            drop(objects);
            return publish_initial_staging(
                authority,
                binding,
                InitialPublicationInput {
                    state_root,
                    staging,
                    objects_identity,
                    lock: lock.take().ok_or(StoreError::Owner)?,
                    lock_identity,
                    snapshot,
                    snapshot_bytes,
                    snapshot_identity,
                },
                tracker,
            );
        }

        let (state_root, staging, objects) = seal_initial_prefix(
            state_root,
            staging,
            objects,
            lock.as_ref().ok_or(StoreError::Owner)?,
            lock_identity,
            None,
        )?;
        tracker.restore(prefix_checkpoint);
        require_same_authority(authority, binding)?;
        let instance = draw_store_instance()?;
        let sequence = NonZeroU64::new(1).expect("one is nonzero");
        let admission = MaterializationAdmissionV1::new(instance, sequence, request);
        let snapshot = ArtifactStoreSnapshotV1::initial(
            instance,
            binding.config_commitment(),
            request.clone(),
            admission,
        )
        .map_err(|_| StoreError::Owner)?;
        validate_initial_snapshot_shape(&snapshot)?;
        let snapshot_bytes = snapshot
            .encode_canonical()
            .map_err(|_| StoreError::Capacity)?;
        tracker.ambiguous();
        let snapshot_identity = write_new_exact(&staging, STORE_SNAPSHOT_NAME, &snapshot_bytes)?;
        staging.file.sync_all().map_err(|_| StoreError::Io)?;
        let objects_identity = objects.identity;
        drop(objects);
        publish_initial_staging(
            authority,
            binding,
            InitialPublicationInput {
                state_root,
                staging,
                objects_identity,
                lock: lock.take().ok_or(StoreError::Owner)?,
                lock_identity,
                snapshot,
                snapshot_bytes,
                snapshot_identity,
            },
            tracker,
        )
    })();
    if result.is_err()
        && let Some(lock) = lock.take()
    {
        let _ = lock.unlock();
        drop(lock);
    }
    result
}

fn cleanup_owned_next(
    root: &DirectoryHandle,
    expected_identity: FileIdentity,
) -> Result<(), StoreError> {
    let (file, identity) =
        open_regular_at(root, OsStr::new(STORE_SNAPSHOT_NEXT_NAME), OFlag::O_RDONLY)?;
    if identity != expected_identity {
        return Err(StoreError::Owner);
    }
    unlinkat(
        &root.file,
        STORE_SNAPSHOT_NEXT_NAME,
        UnlinkatFlags::NoRemoveDir,
    )
    .map_err(|_| StoreError::Io)?;
    if file.metadata().map_err(|_| StoreError::Io)?.nlink() != 0 {
        return Err(StoreError::Owner);
    }
    drop(file);
    root.file.sync_all().map_err(|_| StoreError::Io)?;
    revalidate_directory(root)?;
    exact_names(root, &[STORE_LOCK_NAME, STORE_SNAPSHOT_NAME, OBJECTS_NAME])
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SnapshotRenameState {
    OldWithNext,
    NewWithoutNext,
    Ambiguous,
}

#[derive(Clone, Copy)]
struct SnapshotRenameObservation<'a> {
    expected_active_identity: FileIdentity,
    expected_active_bytes: &'a [u8],
    active_identity: FileIdentity,
    active_bytes: &'a [u8],
    expected_next_identity: FileIdentity,
    expected_next_bytes: &'a [u8],
    named_next_identity: Option<FileIdentity>,
    observed_next: Option<(FileIdentity, &'a [u8])>,
    stable_root: bool,
    root_with_next: bool,
}

fn classify_snapshot_rename_observation(
    observation: &SnapshotRenameObservation<'_>,
) -> SnapshotRenameState {
    if observation.active_identity == observation.expected_active_identity
        && observation.active_bytes == observation.expected_active_bytes
        && observation.named_next_identity == Some(observation.expected_next_identity)
        && observation.observed_next
            == Some((
                observation.expected_next_identity,
                observation.expected_next_bytes,
            ))
        && observation.root_with_next
    {
        SnapshotRenameState::OldWithNext
    } else if observation.active_identity == observation.expected_next_identity
        && observation.active_bytes == observation.expected_next_bytes
        && observation.named_next_identity.is_none()
        && observation.stable_root
    {
        SnapshotRenameState::NewWithoutNext
    } else {
        SnapshotRenameState::Ambiguous
    }
}

fn classify_snapshot_rename_error(
    store: &LockedStore,
    next_bytes: &[u8],
    next_identity: FileIdentity,
) -> SnapshotRenameState {
    let observed: Result<SnapshotRenameState, StoreError> = (|| {
        let (active_bytes, active_identity) = read_regular_bounded(
            &store.root,
            OsStr::new(STORE_SNAPSHOT_NAME),
            MAX_SNAPSHOT_BYTES,
            false,
        )?;
        let named_next = named_identity(&store.root, OsStr::new(STORE_SNAPSHOT_NEXT_NAME))?;
        let names = scan_names(&store.root)?;
        let stable = [STORE_LOCK_NAME, STORE_SNAPSHOT_NAME, OBJECTS_NAME]
            .into_iter()
            .map(OsString::from)
            .collect::<BTreeSet<_>>();
        let with_next = [
            STORE_LOCK_NAME,
            STORE_SNAPSHOT_NAME,
            STORE_SNAPSHOT_NEXT_NAME,
            OBJECTS_NAME,
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<BTreeSet<_>>();
        let objects = reopen_named_directory(
            &store.root,
            OsStr::new(OBJECTS_NAME),
            store.objects.identity,
        )?;
        drop(objects);
        let observed_next = if named_next == Some(next_identity) {
            let (observed_next, observed_next_identity) = read_regular_bounded(
                &store.root,
                OsStr::new(STORE_SNAPSHOT_NEXT_NAME),
                MAX_SNAPSHOT_BYTES,
                false,
            )?;
            Some((observed_next, observed_next_identity))
        } else {
            None
        };
        let observation = SnapshotRenameObservation {
            expected_active_identity: store.snapshot_identity,
            expected_active_bytes: &store.snapshot_bytes,
            active_identity,
            active_bytes: &active_bytes,
            expected_next_identity: next_identity,
            expected_next_bytes: next_bytes,
            named_next_identity: named_next,
            observed_next: observed_next
                .as_ref()
                .map(|(bytes, identity)| (*identity, bytes.as_ref())),
            stable_root: names == stable,
            root_with_next: names == with_next,
        };
        Ok(classify_snapshot_rename_observation(&observation))
    })();
    observed.unwrap_or(SnapshotRenameState::Ambiguous)
}

fn commit_successor(
    store: &mut LockedStore,
    authority: &mut dyn ArtifactStoreAuthorityV1,
    binding: &ArtifactStoreAuthorityBindingV1,
    next: ArtifactStoreSnapshotV1,
    tracker: &mut ChangeTracker,
) -> Result<(), StoreError> {
    let operation_id = permitted_next(&store.snapshot, &next)?;
    let operation = next.operation(operation_id).cloned().map(Box::new);
    if scan_names(&store.root)?.contains(OsStr::new(STORE_SNAPSHOT_NEXT_NAME)) {
        return Err(StoreError::Owner);
    }
    let bytes = next.encode_canonical().map_err(|error| match error {
        crate::ArtifactContractError::CapacityExceeded
        | crate::ArtifactContractError::ArithmeticOverflow => StoreError::Capacity,
        _ => StoreError::Owner,
    })?;
    let (mut file, next_identity) =
        create_regular(&store.root, STORE_SNAPSHOT_NEXT_NAME, OFlag::O_WRONLY)?;
    let prepared = (|| {
        file.write_all(&bytes).map_err(|_| StoreError::Io)?;
        file.sync_all().map_err(|_| StoreError::Io)?;
        drop(file);
        let (reopened, identity) = read_regular_bounded(
            &store.root,
            OsStr::new(STORE_SNAPSHOT_NEXT_NAME),
            MAX_SNAPSHOT_BYTES,
            false,
        )?;
        if identity != next_identity || reopened != bytes {
            return Err(StoreError::Owner);
        }
        let candidate = ArtifactStoreSnapshotCandidateV1::decode_canonical(&reopened)
            .map_err(|_| StoreError::Owner)?;
        if validate_candidate_filesystem(&store.objects, candidate)? != next {
            return Err(StoreError::Owner);
        }
        Ok(())
    })();
    if let Err(error) = prepared {
        if cleanup_owned_next(&store.root, next_identity).is_err() {
            tracker.ambiguous();
            return Err(StoreError::Owner);
        }
        return Err(error);
    }
    let before_publish = (|| {
        revalidate_public_store(store, true)?;
        let (active_bytes, active_identity) = read_regular_bounded(
            &store.root,
            OsStr::new(STORE_SNAPSHOT_NAME),
            MAX_SNAPSHOT_BYTES,
            false,
        )?;
        if active_identity != store.snapshot_identity || active_bytes != store.snapshot_bytes {
            return Err(StoreError::Owner);
        }
        require_same_authority(authority, binding)
    })();
    if let Err(error) = before_publish {
        if cleanup_owned_next(&store.root, next_identity).is_err() {
            tracker.ambiguous();
            return Err(StoreError::Owner);
        }
        return Err(error);
    }
    let publish_checkpoint = *tracker;
    tracker.ambiguous();
    if renameat(
        &store.root.file,
        STORE_SNAPSHOT_NEXT_NAME,
        &store.root.file,
        STORE_SNAPSHOT_NAME,
    )
    .is_err()
    {
        match classify_snapshot_rename_error(store, &bytes, next_identity) {
            SnapshotRenameState::OldWithNext => {
                if cleanup_owned_next(&store.root, next_identity).is_err() {
                    return Err(StoreError::Owner);
                }
                tracker.restore(publish_checkpoint);
                return Err(StoreError::Io);
            }
            SnapshotRenameState::NewWithoutNext => {}
            SnapshotRenameState::Ambiguous => {
                return Err(StoreError::PublicationUncertain(operation));
            }
        }
    }
    store
        .root
        .file
        .sync_all()
        .map_err(|_| StoreError::PublicationUncertain(operation.clone()))?;
    let (candidate, active_bytes, active_identity) =
        decode_snapshot(&store.root, STORE_SNAPSHOT_NAME)
            .map_err(|_| StoreError::PublicationUncertain(operation.clone()))?;
    if active_identity != next_identity {
        return Err(StoreError::Owner);
    }
    if active_bytes != bytes {
        return Err(StoreError::PublicationUncertain(operation));
    }
    let (has_final, has_staging) = root_selection(&store.state_root.leaf)?;
    if !has_final || has_staging {
        return Err(StoreError::Owner);
    }
    let public_root = reopen_named_directory(
        &store.state_root.leaf,
        OsStr::new(STORE_ROOT_NAME),
        store.root.identity,
    )?;
    let public_objects = reopen_named_directory(
        &public_root,
        OsStr::new(OBJECTS_NAME),
        store.objects.identity,
    )?;
    validate_named_regular(
        &public_root,
        OsStr::new(STORE_LOCK_NAME),
        store.lock_identity,
        Some(0),
    )?;
    validate_named_regular(
        &public_root,
        OsStr::new(STORE_SNAPSHOT_NAME),
        next_identity,
        Some(u64::try_from(active_bytes.len()).map_err(|_| StoreError::Owner)?),
    )?;
    let active = validate_candidate_filesystem(&public_objects, candidate)?;
    if active != next {
        return Err(StoreError::PublicationUncertain(operation));
    }
    drop(public_objects);
    drop(public_root);
    store.snapshot = active;
    store.snapshot_bytes = active_bytes;
    store.snapshot_identity = active_identity;
    tracker.committed();
    Ok(())
}

fn settle_existing_next(
    store: &mut LockedStore,
    authority: &mut dyn ArtifactStoreAuthorityV1,
    binding: &ArtifactStoreAuthorityBindingV1,
    request: &MaterializationRequestV1,
    tracker: &mut ChangeTracker,
) -> Result<(), StoreError> {
    let Some(next) = read_and_validate_next(store)? else {
        return Ok(());
    };
    let operation = next
        .snapshot
        .operation(next.operation_id)
        .cloned()
        .ok_or(StoreError::Owner)?;
    if next.operation_id != request.operation_id() || operation.request() != request {
        return Err(StoreError::Owner);
    }
    revalidate_public_store(store, true)?;
    require_same_authority(authority, binding)?;
    let publish_checkpoint = *tracker;
    tracker.ambiguous();
    if renameat(
        &store.root.file,
        STORE_SNAPSHOT_NEXT_NAME,
        &store.root.file,
        STORE_SNAPSHOT_NAME,
    )
    .is_err()
    {
        match classify_snapshot_rename_error(store, &next.bytes, next.identity) {
            SnapshotRenameState::OldWithNext => {
                tracker.restore(publish_checkpoint);
                return Err(StoreError::Io);
            }
            SnapshotRenameState::NewWithoutNext => {}
            SnapshotRenameState::Ambiguous => {
                return Err(StoreError::PublicationUncertain(Some(Box::new(operation))));
            }
        }
    }
    store
        .root
        .file
        .sync_all()
        .map_err(|_| StoreError::PublicationUncertain(Some(Box::new(operation.clone()))))?;
    let (candidate, bytes, identity) = decode_snapshot(&store.root, STORE_SNAPSHOT_NAME)
        .map_err(|_| StoreError::PublicationUncertain(Some(Box::new(operation.clone()))))?;
    if identity != next.identity {
        return Err(StoreError::Owner);
    }
    if bytes != next.bytes {
        return Err(StoreError::PublicationUncertain(Some(Box::new(
            operation.clone(),
        ))));
    }
    let (has_final, has_staging) = root_selection(&store.state_root.leaf)?;
    if !has_final || has_staging {
        return Err(StoreError::Owner);
    }
    let public_root = reopen_named_directory(
        &store.state_root.leaf,
        OsStr::new(STORE_ROOT_NAME),
        store.root.identity,
    )?;
    let public_objects = reopen_named_directory(
        &public_root,
        OsStr::new(OBJECTS_NAME),
        store.objects.identity,
    )?;
    validate_named_regular(
        &public_root,
        OsStr::new(STORE_LOCK_NAME),
        store.lock_identity,
        Some(0),
    )?;
    validate_named_regular(
        &public_root,
        OsStr::new(STORE_SNAPSHOT_NAME),
        next.identity,
        Some(u64::try_from(bytes.len()).map_err(|_| StoreError::Owner)?),
    )?;
    let reopened = validate_candidate_filesystem(&public_objects, candidate)?;
    if reopened != next.snapshot {
        return Err(StoreError::PublicationUncertain(Some(Box::new(
            operation.clone(),
        ))));
    }
    drop(public_objects);
    drop(public_root);
    store.snapshot = reopened;
    store.snapshot_bytes = bytes;
    store.snapshot_identity = identity;
    tracker.committed();
    Ok(())
}

enum PairPublication {
    Complete(Box<VerifiedArtifactPairV1>),
    Quarantined(u64),
    NoEffectFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StoreFaultPoint {
    ExistingChildBeforeObjectsSync,
    ExistingChildBeforeObjectsReopen,
    ExistingChildBeforeChildReopen,
    ExistingChildBeforeFileSync,
    ExistingChildBeforeChildSync,
    ExistingChildBeforeParentSync,
    ExistingChildBeforeParentReopen,
    ExistingChildBeforeFinalChildReopen,
    FreshPairAfterSecondFinalBeforeDurabilityProof,
    PairMkdirFailureBeforeObjectsSync,
    PairMkdirFailureBeforeObjectsReopen,
}

trait StoreFaultObserver {
    fn checkpoint(&mut self, point: StoreFaultPoint) -> Result<(), StoreError>;
}

struct NoStoreFaultObserver;

impl StoreFaultObserver for NoStoreFaultObserver {
    fn checkpoint(&mut self, _point: StoreFaultPoint) -> Result<(), StoreError> {
        Ok(())
    }
}

fn recovery_fault_checkpoint(
    observer: &mut dyn StoreFaultObserver,
    point: StoreFaultPoint,
) -> Result<(), StoreError> {
    observer.checkpoint(point).map_err(|_| StoreError::Owner)
}

fn sync_partial_child(
    root: &DirectoryHandle,
    objects: &DirectoryHandle,
    child_name: &str,
    expected_ref: ArtifactObjectRefV1,
    expected_identity: FileIdentity,
    observer: &mut dyn StoreFaultObserver,
) -> Result<u64, StoreError> {
    let child = open_directory_at(objects, OsStr::new(child_name))?;
    if child.identity != expected_identity {
        drop(child);
        return Err(StoreError::Owner);
    }
    let facts = validate_partial_pair_entry(&child, expected_ref)?;
    for (name, expected_file_identity) in &facts.entries {
        let (file, file_identity) = open_regular_at(&child, name, OFlag::O_RDONLY)?;
        if file_identity != *expected_file_identity {
            drop(file);
            drop(child);
            return Err(StoreError::Owner);
        }
        recovery_fault_checkpoint(observer, StoreFaultPoint::ExistingChildBeforeFileSync)?;
        file.sync_all().map_err(|_| StoreError::Io)?;
        drop(file);
    }
    if validate_partial_pair_entry(&child, expected_ref)? != facts {
        drop(child);
        return Err(StoreError::Owner);
    }
    recovery_fault_checkpoint(observer, StoreFaultPoint::ExistingChildBeforeChildSync)?;
    child.file.sync_all().map_err(|_| StoreError::Io)?;
    recovery_fault_checkpoint(observer, StoreFaultPoint::ExistingChildBeforeParentSync)?;
    objects.file.sync_all().map_err(|_| StoreError::Io)?;
    recovery_fault_checkpoint(observer, StoreFaultPoint::ExistingChildBeforeParentReopen)?;
    let reopened_objects =
        reopen_named_directory(root, OsStr::new(OBJECTS_NAME), objects.identity)?;
    recovery_fault_checkpoint(
        observer,
        StoreFaultPoint::ExistingChildBeforeFinalChildReopen,
    )?;
    let reopened_child =
        reopen_named_directory(&reopened_objects, OsStr::new(child_name), child.identity)?;
    if validate_partial_pair_entry(&reopened_child, expected_ref)? != facts {
        drop(reopened_child);
        drop(reopened_objects);
        drop(child);
        return Err(StoreError::Owner);
    }
    drop(reopened_child);
    drop(reopened_objects);
    drop(child);
    Ok(facts.regular_file_bytes)
}

fn sync_complete_child(
    root: &DirectoryHandle,
    objects: &DirectoryHandle,
    child_name: &str,
    expected_pair: &VerifiedArtifactPairV1,
    expected_identity: FileIdentity,
    observer: &mut dyn StoreFaultObserver,
) -> Result<VerifiedArtifactPairV1, StoreError> {
    let child = open_directory_at(objects, OsStr::new(child_name))?;
    if child.identity != expected_identity {
        drop(child);
        return Err(StoreError::Owner);
    }
    let (manifest, _) = open_regular_at(&child, OsStr::new(MANIFEST_NAME), OFlag::O_RDONLY)?;
    let (payload, _) = open_regular_at(&child, OsStr::new(PAYLOAD_NAME), OFlag::O_RDONLY)?;
    recovery_fault_checkpoint(observer, StoreFaultPoint::ExistingChildBeforeFileSync)?;
    manifest.sync_all().map_err(|_| StoreError::Io)?;
    recovery_fault_checkpoint(observer, StoreFaultPoint::ExistingChildBeforeFileSync)?;
    payload.sync_all().map_err(|_| StoreError::Io)?;
    drop(payload);
    drop(manifest);
    recovery_fault_checkpoint(observer, StoreFaultPoint::ExistingChildBeforeChildSync)?;
    child.file.sync_all().map_err(|_| StoreError::Io)?;
    recovery_fault_checkpoint(observer, StoreFaultPoint::ExistingChildBeforeParentSync)?;
    objects.file.sync_all().map_err(|_| StoreError::Io)?;
    let pair = strict_pair_from_directory(&child)?;
    if &pair != expected_pair {
        drop(child);
        return Err(StoreError::Owner);
    }
    recovery_fault_checkpoint(observer, StoreFaultPoint::ExistingChildBeforeParentReopen)?;
    let reopened_objects =
        reopen_named_directory(root, OsStr::new(OBJECTS_NAME), objects.identity)?;
    recovery_fault_checkpoint(
        observer,
        StoreFaultPoint::ExistingChildBeforeFinalChildReopen,
    )?;
    let reopened_child =
        reopen_named_directory(&reopened_objects, OsStr::new(child_name), child.identity)?;
    let reopened_pair = strict_pair_from_directory(&reopened_child)?;
    if reopened_pair != pair {
        drop(reopened_child);
        drop(reopened_objects);
        drop(child);
        return Err(StoreError::Owner);
    }
    drop(reopened_child);
    drop(reopened_objects);
    drop(child);
    Ok(pair)
}

fn recover_pair_from_existing_child(
    store: &LockedStore,
    pair: &VerifiedArtifactPairV1,
    tracker: &mut ChangeTracker,
    observer: &mut dyn StoreFaultObserver,
) -> Result<PairPublication, StoreError> {
    tracker.ambiguous();
    recovery_fault_checkpoint(observer, StoreFaultPoint::ExistingChildBeforeObjectsSync)?;
    store
        .objects
        .file
        .sync_all()
        .map_err(|_| StoreError::Owner)?;
    recovery_fault_checkpoint(observer, StoreFaultPoint::ExistingChildBeforeObjectsReopen)?;
    let objects = reopen_named_directory(
        &store.root,
        OsStr::new(OBJECTS_NAME),
        store.objects.identity,
    )
    .map_err(|_| StoreError::Owner)?;
    let child_name = object_directory_name(pair.object_ref());
    recovery_fault_checkpoint(observer, StoreFaultPoint::ExistingChildBeforeChildReopen)?;
    let (child_identity, facts) =
        strict_partial_child(&objects, OsStr::new(&child_name), pair.object_ref())
            .map_err(|_| StoreError::Owner)?;
    let complete_names = [MANIFEST_NAME, PAYLOAD_NAME]
        .into_iter()
        .map(OsString::from)
        .collect::<BTreeSet<_>>();
    let actual_names = facts
        .entries
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<BTreeSet<_>>();
    let result = if actual_names == complete_names {
        sync_complete_child(
            &store.root,
            &objects,
            &child_name,
            pair,
            child_identity,
            observer,
        )
        .map_err(|_| StoreError::Owner)
        .map(Box::new)
        .map(PairPublication::Complete)
    } else {
        sync_partial_child(
            &store.root,
            &objects,
            &child_name,
            pair.object_ref(),
            child_identity,
            observer,
        )
        .map(PairPublication::Quarantined)
        .map_err(|_| StoreError::Owner)
    };
    drop(objects);
    result
}

fn publish_pair_file(
    child: &DirectoryHandle,
    temporary_name: &str,
    final_name: &str,
    bytes: &[u8],
) -> Result<(), StoreError> {
    let identity = write_new_exact(child, temporary_name, bytes)?;
    validate_named_regular(
        child,
        OsStr::new(temporary_name),
        identity,
        Some(u64::try_from(bytes.len()).map_err(|_| StoreError::Owner)?),
    )?;
    renameat_with(
        &child.file,
        temporary_name,
        &child.file,
        final_name,
        RenameFlags::NOREPLACE,
    )
    .map_err(|_| StoreError::Io)?;
    child.file.sync_all().map_err(|_| StoreError::Io)?;
    let (reopened, reopened_identity) =
        read_regular_bounded(child, OsStr::new(final_name), bytes.len(), false)?;
    if reopened_identity != identity || reopened.as_ref() != bytes {
        return Err(StoreError::Owner);
    }
    Ok(())
}

fn publish_or_recover_pair_observed(
    store: &LockedStore,
    pair: &VerifiedArtifactPairV1,
    tracker: &mut ChangeTracker,
    observer: &mut dyn StoreFaultObserver,
) -> Result<PairPublication, StoreError> {
    revalidate_public_store(store, false)?;
    let child_name = object_directory_name(pair.object_ref());
    let names = scan_names(&store.objects)?;
    if names.contains(OsStr::new(&child_name)) {
        return recover_pair_from_existing_child(store, pair, tracker, observer);
    }

    if mkdirat(&store.objects.file, child_name.as_str(), DIRECTORY_MODE).is_err() {
        let checkpoint = *tracker;
        tracker.ambiguous();
        recovery_fault_checkpoint(observer, StoreFaultPoint::PairMkdirFailureBeforeObjectsSync)?;
        store
            .objects
            .file
            .sync_all()
            .map_err(|_| StoreError::Owner)?;
        recovery_fault_checkpoint(
            observer,
            StoreFaultPoint::PairMkdirFailureBeforeObjectsReopen,
        )?;
        let reopened_objects = reopen_named_directory(
            &store.root,
            OsStr::new(OBJECTS_NAME),
            store.objects.identity,
        )
        .map_err(|_| StoreError::Owner)?;
        let after = scan_names(&reopened_objects).map_err(|_| StoreError::Owner)?;
        drop(reopened_objects);
        if after == names && !after.contains(OsStr::new(&child_name)) {
            tracker.restore(checkpoint);
            return Ok(PairPublication::NoEffectFailure);
        }
        if after.contains(OsStr::new(&child_name)) {
            return recover_pair_from_existing_child(store, pair, tracker, observer);
        }
        return Err(StoreError::Owner);
    }
    tracker.ambiguous();
    let child = open_directory_at(&store.objects, OsStr::new(&child_name))?;
    let child_identity = child.identity;
    let publication = (|| {
        exact_names(&child, &[])?;
        child.file.sync_all().map_err(|_| StoreError::Io)?;
        store.objects.file.sync_all().map_err(|_| StoreError::Io)?;
        let reopened_objects = reopen_named_directory(
            &store.root,
            OsStr::new(OBJECTS_NAME),
            store.objects.identity,
        )?;
        let reopened_child =
            reopen_named_directory(&reopened_objects, OsStr::new(&child_name), child_identity)?;
        drop(child);
        publish_pair_file(
            &reopened_child,
            MANIFEST_NEXT_NAME,
            MANIFEST_NAME,
            pair.manifest_bytes(),
        )?;
        publish_pair_file(
            &reopened_child,
            PAYLOAD_NEXT_NAME,
            PAYLOAD_NAME,
            pair.payload(),
        )?;
        recovery_fault_checkpoint(
            observer,
            StoreFaultPoint::FreshPairAfterSecondFinalBeforeDurabilityProof,
        )?;
        drop(reopened_child);
        let verified = sync_complete_child(
            &store.root,
            &reopened_objects,
            &child_name,
            pair,
            child_identity,
            observer,
        );
        drop(reopened_objects);
        verified
    })();
    match publication {
        Ok(verified) => Ok(PairPublication::Complete(Box::new(verified))),
        Err(_) => recover_pair_from_existing_child(store, pair, tracker, observer),
    }
}

fn publish_or_recover_pair(
    store: &LockedStore,
    pair: &VerifiedArtifactPairV1,
    tracker: &mut ChangeTracker,
) -> Result<PairPublication, StoreError> {
    let mut observer = NoStoreFaultObserver;
    publish_or_recover_pair_observed(store, pair, tracker, &mut observer)
}

fn commit_receipt(
    store: &mut LockedStore,
    authority: &mut dyn ArtifactStoreAuthorityV1,
    binding: &ArtifactStoreAuthorityBindingV1,
    operation_id: ArtifactOperationIdV1,
    tracker: &mut ChangeTracker,
) -> Result<MaterializationOperationV1, StoreError> {
    let operation = store
        .snapshot
        .operation(operation_id)
        .cloned()
        .ok_or(StoreError::Owner)?;
    if operation.receipt().is_some() {
        return Ok(operation);
    }
    let terminal = operation.terminal().ok_or(StoreError::Owner)?;
    let receipt = MaterializationReceiptV1::new(terminal);
    let next = store
        .snapshot
        .try_successor(ArtifactSnapshotSuccessorV1::Receipt { receipt })
        .map_err(|_| StoreError::Owner)?;
    commit_successor(store, authority, binding, next, tracker)?;
    store
        .snapshot
        .operation(operation_id)
        .cloned()
        .ok_or(StoreError::Owner)
}

struct TerminalCommitInput<'a> {
    operation_id: ArtifactOperationIdV1,
    object: Option<&'a ArtifactObjectRecordV1>,
    state: MaterializationTerminalStateV1,
    quarantine: ArtifactQuarantineFactsV1,
}

fn commit_terminal_and_receipt(
    store: &mut LockedStore,
    authority: &mut dyn ArtifactStoreAuthorityV1,
    binding: &ArtifactStoreAuthorityBindingV1,
    input: TerminalCommitInput<'_>,
    tracker: &mut ChangeTracker,
) -> Result<MaterializationOperationV1, StoreError> {
    let TerminalCommitInput {
        operation_id,
        object,
        state,
        quarantine,
    } = input;
    let operation = store
        .snapshot
        .operation(operation_id)
        .cloned()
        .ok_or(StoreError::Owner)?;
    if operation.terminal().is_none() {
        let terminal = MaterializationTerminalV1::new(
            operation.admission(),
            operation.materializing(),
            object,
            state,
        )
        .map_err(|_| StoreError::Owner)?;
        let next = store
            .snapshot
            .try_successor(ArtifactSnapshotSuccessorV1::Terminal {
                terminal,
                quarantine,
            })
            .map_err(|_| StoreError::Owner)?;
        commit_successor(store, authority, binding, next, tracker)?;
    }
    commit_receipt(store, authority, binding, operation_id, tracker)
}

fn admit_operation(
    store: &mut LockedStore,
    authority: &mut dyn ArtifactStoreAuthorityV1,
    binding: &ArtifactStoreAuthorityBindingV1,
    request: &MaterializationRequestV1,
    tracker: &mut ChangeTracker,
) -> Result<(), StoreError> {
    if let Some(existing) = store.snapshot.operation(request.operation_id()) {
        return if existing.request() == request {
            Ok(())
        } else {
            Err(StoreError::Conflict)
        };
    }
    if !matches!(
        store.snapshot.quarantine(),
        ArtifactQuarantineFactsV1::Absent
    ) {
        return Err(StoreError::Capacity);
    }
    let sequence = store
        .snapshot
        .operation_high_water()
        .checked_add(1)
        .and_then(NonZeroU64::new)
        .ok_or(StoreError::Capacity)?;
    let admission =
        MaterializationAdmissionV1::new(store.snapshot.store_instance(), sequence, request);
    let next = store
        .snapshot
        .try_successor(ArtifactSnapshotSuccessorV1::Admission {
            request: request.clone(),
            admission,
        })
        .map_err(|error| match error {
            crate::ArtifactContractError::CapacityExceeded
            | crate::ArtifactContractError::ArithmeticOverflow => StoreError::Capacity,
            _ => StoreError::Owner,
        })?;
    commit_successor(store, authority, binding, next, tracker)
}

fn ensure_materializing(
    store: &mut LockedStore,
    authority: &mut dyn ArtifactStoreAuthorityV1,
    binding: &ArtifactStoreAuthorityBindingV1,
    operation_id: ArtifactOperationIdV1,
    tracker: &mut ChangeTracker,
) -> Result<(), StoreError> {
    let operation = store
        .snapshot
        .operation(operation_id)
        .cloned()
        .ok_or(StoreError::Owner)?;
    if operation.materializing().is_some() {
        return Ok(());
    }
    if operation.terminal().is_some() {
        return Err(StoreError::Owner);
    }
    let materializing = MaterializingRecordV1::new(operation.admission());
    let next = store
        .snapshot
        .try_successor(ArtifactSnapshotSuccessorV1::Materializing { materializing })
        .map_err(|_| StoreError::Owner)?;
    commit_successor(store, authority, binding, next, tracker)
}

fn object_candidate(
    store: &LockedStore,
    operation_id: ArtifactOperationIdV1,
    pair: &VerifiedArtifactPairV1,
) -> Result<(ArtifactObjectRecordV1, ArtifactStoreSnapshotV1), StoreError> {
    let sequence = store
        .snapshot
        .object_high_water()
        .checked_add(1)
        .and_then(NonZeroU64::new)
        .ok_or(StoreError::Capacity)?;
    let object = ArtifactObjectRecordV1::new(store.snapshot.store_instance(), sequence, pair);
    let next = store
        .snapshot
        .try_successor(ArtifactSnapshotSuccessorV1::Object {
            operation_id,
            object: object.clone(),
        })
        .map_err(|error| match error {
            crate::ArtifactContractError::CapacityExceeded
            | crate::ArtifactContractError::ArithmeticOverflow => StoreError::Capacity,
            _ => StoreError::Owner,
        })?;
    let frame_bytes = next.encode_canonical().map_err(|_| StoreError::Capacity)?;
    let pair_bytes = u64::try_from(pair.manifest_bytes().len())
        .ok()
        .and_then(|manifest| {
            u64::try_from(pair.payload().len())
                .ok()
                .and_then(|payload| manifest.checked_add(payload))
        })
        .ok_or(StoreError::Capacity)?;
    ArtifactCapacityInputV1::new(
        store
            .snapshot
            .accounted_rest_bytes()
            .map_err(|_| StoreError::Owner)?,
        u64::try_from(frame_bytes.len()).map_err(|_| StoreError::Capacity)?,
        0,
        pair_bytes,
        next.objects().len(),
        next.operations().len(),
        next.quarantine().regular_file_bytes(),
    )
    .checked_total()
    .map_err(|_| StoreError::Capacity)?;
    Ok((object, next))
}

fn drive_materialization(
    store: &mut LockedStore,
    authority: &mut dyn ArtifactStoreAuthorityV1,
    binding: &ArtifactStoreAuthorityBindingV1,
    request: &MaterializationRequestV1,
    pair: &VerifiedArtifactPairV1,
    tracker: &mut ChangeTracker,
) -> Result<MaterializationOperationV1, StoreError> {
    settle_existing_next(store, authority, binding, request, tracker)?;
    admit_operation(store, authority, binding, request, tracker)?;
    let operation_id = request.operation_id();
    let operation = store
        .snapshot
        .operation(operation_id)
        .cloned()
        .ok_or(StoreError::Owner)?;
    if operation.request() != request {
        return Err(StoreError::Conflict);
    }
    if operation.receipt().is_some() {
        return Ok(operation);
    }
    if operation.terminal().is_some() {
        return commit_receipt(store, authority, binding, operation_id, tracker);
    }
    ensure_materializing(store, authority, binding, operation_id, tracker)?;
    match store
        .snapshot
        .recovery_start(operation_id)
        .map_err(|_| StoreError::Owner)?
    {
        ArtifactRecoveryStartV1::UnreferencedCurrent(object) => {
            let verified = strict_indexed_object(&store.objects, &object)?;
            if &verified != pair {
                return Err(StoreError::Owner);
            }
            commit_terminal_and_receipt(
                store,
                authority,
                binding,
                TerminalCommitInput {
                    operation_id,
                    object: Some(&object),
                    state: MaterializationTerminalStateV1::Materialized,
                    quarantine: ArtifactQuarantineFactsV1::Absent,
                },
                tracker,
            )
        }
        ArtifactRecoveryStartV1::EarlierReferenced(object) => {
            let verified = strict_indexed_object(&store.objects, &object)?;
            if &verified != pair {
                return Err(StoreError::Owner);
            }
            commit_terminal_and_receipt(
                store,
                authority,
                binding,
                TerminalCommitInput {
                    operation_id,
                    object: Some(&object),
                    state: MaterializationTerminalStateV1::AlreadyMaterialized,
                    quarantine: ArtifactQuarantineFactsV1::Absent,
                },
                tracker,
            )
        }
        ArtifactRecoveryStartV1::NoMatchingObject => {
            let candidate = object_candidate(store, operation_id, pair);
            let (object, object_successor) = match candidate {
                Ok(candidate) => candidate,
                Err(StoreError::Capacity) => {
                    return commit_terminal_and_receipt(
                        store,
                        authority,
                        binding,
                        TerminalCommitInput {
                            operation_id,
                            object: None,
                            state: MaterializationTerminalStateV1::Failed,
                            quarantine: ArtifactQuarantineFactsV1::Absent,
                        },
                        tracker,
                    );
                }
                Err(error) => return Err(error),
            };
            match publish_or_recover_pair(store, pair, tracker)? {
                PairPublication::NoEffectFailure => commit_terminal_and_receipt(
                    store,
                    authority,
                    binding,
                    TerminalCommitInput {
                        operation_id,
                        object: None,
                        state: MaterializationTerminalStateV1::Failed,
                        quarantine: ArtifactQuarantineFactsV1::Absent,
                    },
                    tracker,
                ),
                PairPublication::Quarantined(regular_file_bytes) => commit_terminal_and_receipt(
                    store,
                    authority,
                    binding,
                    TerminalCommitInput {
                        operation_id,
                        object: None,
                        state: MaterializationTerminalStateV1::Uncertain,
                        quarantine: ArtifactQuarantineFactsV1::Present {
                            operation_id,
                            regular_file_bytes,
                        },
                    },
                    tracker,
                ),
                PairPublication::Complete(verified) => {
                    if verified.as_ref() != pair {
                        return Err(StoreError::Owner);
                    }
                    commit_successor(store, authority, binding, object_successor, tracker)?;
                    commit_terminal_and_receipt(
                        store,
                        authority,
                        binding,
                        TerminalCommitInput {
                            operation_id,
                            object: Some(&object),
                            state: MaterializationTerminalStateV1::Materialized,
                            quarantine: ArtifactQuarantineFactsV1::Absent,
                        },
                        tracker,
                    )
                }
            }
        }
    }
}

fn run_materialize(
    authority: &mut dyn ArtifactStoreAuthorityV1,
    request: &MaterializationRequestV1,
    pair: &VerifiedArtifactPairV1,
) -> ArtifactStoreInvocationV1 {
    let mut tracker = ChangeTracker::default();
    let binding = match authority_binding(authority) {
        Ok(binding) => binding,
        Err(error) => {
            return ArtifactStoreInvocationV1::failure(tracker.change(), error.into_public());
        }
    };
    if request.config_commitment() != binding.config_commitment() {
        return ArtifactStoreInvocationV1::failure(
            tracker.change(),
            ArtifactStoreFailureV1::ConfigurationMismatch,
        );
    }
    if request.object_ref() != pair.object_ref() {
        return ArtifactStoreInvocationV1::failure(
            tracker.change(),
            ArtifactStoreFailureV1::Conflict,
        );
    }
    let mut store = match open_or_initialize_store(authority, &binding, request, &mut tracker) {
        Ok(store) => store,
        Err(error) => {
            if error.owner_effect_unknown() {
                tracker.ambiguous();
            }
            return ArtifactStoreInvocationV1::failure(tracker.change(), error.into_public());
        }
    };
    let result =
        drive_materialization(&mut store, authority, &binding, request, pair, &mut tracker);
    if let Err(error) = &result
        && error.owner_effect_unknown()
    {
        tracker.ambiguous();
    }
    finish_locked(store, tracker, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions, Permissions};
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt;

    struct FailingObserver {
        target: StoreFaultPoint,
        seen: Vec<StoreFaultPoint>,
        fired: bool,
    }

    impl StoreFaultObserver for FailingObserver {
        fn checkpoint(&mut self, point: StoreFaultPoint) -> Result<(), StoreError> {
            self.seen.push(point);
            if point == self.target && !self.fired {
                self.fired = true;
                Err(StoreError::Io)
            } else {
                Ok(())
            }
        }
    }

    struct TestTempDir {
        canonical_base: PathBuf,
        path: PathBuf,
        identity: FileIdentity,
    }

    impl TestTempDir {
        fn new() -> Self {
            let canonical_base = std::env::temp_dir()
                .canonicalize()
                .expect("canonical test temporary base");
            for _ in 0..16 {
                let mut random = [0_u8; 16];
                getrandom::fill(&mut random).expect("test temporary random suffix");
                let mut name = String::from("paraegox-artifact-store-");
                push_lower_hex(&mut name, &random);
                let path = canonical_base.join(name);
                match fs::create_dir(&path) {
                    Ok(()) => {
                        fs::set_permissions(&path, Permissions::from_mode(DIRECTORY_MODE_BITS))
                            .expect("strict test temporary mode");
                        let canonical_path = path.canonicalize().expect("canonical test directory");
                        assert_eq!(canonical_path, path);
                        let metadata =
                            fs::symlink_metadata(&path).expect("test directory metadata");
                        return Self {
                            canonical_base,
                            path,
                            identity: FileIdentity::from_metadata(&metadata),
                        };
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("test temporary directory: {error}"),
                }
            }
            panic!("could not allocate unique test temporary directory")
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestTempDir {
        fn drop(&mut self) {
            let safe_name = self
                .path
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with("paraegox-artifact-store-"));
            if !safe_name
                || self.path.parent() != Some(self.canonical_base.as_path())
                || !self.path.starts_with(&self.canonical_base)
            {
                return;
            }
            let Ok(metadata) = fs::symlink_metadata(&self.path) else {
                return;
            };
            if !metadata.file_type().is_dir()
                || FileIdentity::from_metadata(&metadata) != self.identity
            {
                return;
            }
            let Ok(canonical_path) = self.path.canonicalize() else {
                return;
            };
            if canonical_path != self.path
                || canonical_path.parent() != Some(self.canonical_base.as_path())
            {
                return;
            }
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[derive(Clone)]
    struct FixedAuthority {
        binding: ArtifactStoreAuthorityBindingV1,
    }

    impl ArtifactStoreAuthorityV1 for FixedAuthority {
        fn revalidate(
            &mut self,
        ) -> Result<ArtifactStoreAuthorityBindingV1, ArtifactStoreAuthorityRecheckFailureV1>
        {
            Ok(self.binding.clone())
        }
    }

    struct TestStoreFixture {
        _temp: TestTempDir,
        authority: FixedAuthority,
        request: MaterializationRequestV1,
        pair: VerifiedArtifactPairV1,
    }

    impl TestStoreFixture {
        fn new(payload: &[u8], operation_byte: u8) -> Self {
            let temp = TestTempDir::new();
            let pair = VerifiedArtifactPairV1::from_payload(payload).expect("valid test pair");
            let binding =
                ArtifactStoreAuthorityBindingV1::try_new(temp.path().join("state"), config(0x33))
                    .expect("valid test store binding");
            let request = MaterializationRequestV1::new(
                operation(operation_byte),
                binding.config_commitment(),
                pair.object_ref(),
            );
            Self {
                _temp: temp,
                authority: FixedAuthority { binding },
                request,
                pair,
            }
        }

        fn state_root(&self) -> &Path {
            self.authority.binding.state_root()
        }
    }

    #[derive(Debug, Eq, PartialEq)]
    struct TestTreeEntry {
        relative: PathBuf,
        mode: u32,
        device: i128,
        inode: i128,
        length: u64,
        bytes: Option<Vec<u8>>,
    }

    fn tree_fingerprint(root: &Path) -> Vec<TestTreeEntry> {
        fn visit(root: &Path, current: &Path, output: &mut Vec<TestTreeEntry>) {
            let metadata = fs::symlink_metadata(current).expect("test tree metadata");
            output.push(TestTreeEntry {
                relative: current
                    .strip_prefix(root)
                    .expect("test tree relative path")
                    .to_path_buf(),
                mode: metadata.mode(),
                device: i128::from(metadata.dev()),
                inode: i128::from(metadata.ino()),
                length: metadata.len(),
                bytes: metadata
                    .file_type()
                    .is_file()
                    .then(|| fs::read(current).expect("test tree file bytes")),
            });
            if metadata.file_type().is_dir() {
                let mut children = fs::read_dir(current)
                    .expect("test tree directory")
                    .collect::<Result<Vec<_>, _>>()
                    .expect("test tree entries");
                children.sort_by_key(|entry| entry.file_name());
                for child in children {
                    visit(root, &child.path(), output);
                }
            }
        }

        let mut output = Vec::new();
        visit(root, root, &mut output);
        output
    }

    fn create_test_directory(path: &Path) {
        fs::create_dir(path).expect("create test directory");
        fs::set_permissions(path, Permissions::from_mode(DIRECTORY_MODE_BITS))
            .expect("strict test directory mode");
    }

    fn create_test_regular(path: &Path, bytes: &[u8]) -> File {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
            .expect("create test regular file");
        file.set_permissions(Permissions::from_mode(FILE_MODE_BITS))
            .expect("strict test regular mode");
        file.write_all(bytes).expect("write test regular file");
        file.sync_all().expect("sync test regular file");
        file
    }

    fn open_test_directory(path: &Path) -> DirectoryHandle {
        let owned = open(
            path,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
            Mode::empty(),
        )
        .expect("open test directory");
        directory_from_owned(owned, geteuid().as_raw(), getegid().as_raw(), true)
            .expect("strict test directory")
    }

    fn open_materializing_fixture(
        fixture: &mut TestStoreFixture,
    ) -> (LockedStore, ArtifactStoreAuthorityBindingV1, ChangeTracker) {
        let binding = fixture.authority.binding.clone();
        let request = fixture.request.clone();
        let mut tracker = ChangeTracker::default();
        let mut store =
            open_or_initialize_store(&mut fixture.authority, &binding, &request, &mut tracker)
                .expect("initialize test store");
        ensure_materializing(
            &mut store,
            &mut fixture.authority,
            &binding,
            request.operation_id(),
            &mut tracker,
        )
        .expect("publish test materializing state");
        let operation = store
            .snapshot
            .operation(request.operation_id())
            .expect("test materializing operation");
        assert!(operation.materializing().is_some());
        assert!(operation.terminal().is_none());
        assert!(store.snapshot.objects().is_empty());
        (store, binding, tracker)
    }

    fn write_full_unindexed_pair(
        store: &LockedStore,
        pair: &VerifiedArtifactPairV1,
    ) -> Result<(), StoreError> {
        revalidate_public_store(store, false)?;
        let child_name = object_directory_name(pair.object_ref());
        mkdirat(&store.objects.file, child_name.as_str(), DIRECTORY_MODE)
            .map_err(|_| StoreError::Io)?;
        let child = open_directory_at(&store.objects, OsStr::new(&child_name))?;
        exact_names(&child, &[])?;
        publish_pair_file(
            &child,
            MANIFEST_NEXT_NAME,
            MANIFEST_NAME,
            pair.manifest_bytes(),
        )?;
        publish_pair_file(&child, PAYLOAD_NEXT_NAME, PAYLOAD_NAME, pair.payload())?;
        drop(child);
        Ok(())
    }

    fn assert_prefix_query_not_found_and_unchanged(fixture: &mut TestStoreFixture) {
        let state_root = fixture.state_root().to_path_buf();
        let before = tree_fingerprint(&state_root);
        let invocation =
            ArtifactStoreV1::query(&mut fixture.authority, fixture.request.operation_id());
        assert_eq!(invocation.change(), ArtifactStoreChangeV1::Unchanged);
        assert_eq!(
            invocation.into_result().expect_err("prefix is not found"),
            ArtifactStoreFailureV1::NotFound,
        );
        assert_eq!(tree_fingerprint(&state_root), before);
    }

    fn config(byte: u8) -> ArtifactConfigCommitmentV1 {
        ArtifactConfigCommitmentV1::try_from_bytes([byte; 32]).expect("nonzero config")
    }

    fn operation(byte: u8) -> ArtifactOperationIdV1 {
        ArtifactOperationIdV1::try_from_bytes([byte; 16]).expect("nonzero operation")
    }

    fn store(byte: u8) -> ArtifactStoreInstanceV1 {
        ArtifactStoreInstanceV1::try_from_bytes([byte; 32]).expect("nonzero store")
    }

    fn initial_snapshot() -> (ArtifactStoreSnapshotV1, VerifiedArtifactPairV1) {
        let pair = VerifiedArtifactPairV1::from_payload(b"store-focused-test ")
            .expect("valid fixture pair");
        let request =
            MaterializationRequestV1::new(operation(0x22), config(0x33), pair.object_ref());
        let admission = MaterializationAdmissionV1::new(
            store(0x44),
            NonZeroU64::new(1).expect("one is nonzero"),
            &request,
        );
        let snapshot =
            ArtifactStoreSnapshotV1::initial(store(0x44), config(0x33), request, admission)
                .expect("valid initial snapshot");
        (snapshot, pair)
    }

    #[test]
    fn unix_virgin_materialize_query_read_and_replay_reacquire_lock() {
        let mut fixture = TestStoreFixture::new(b"unix-store-e2e ", 0x51);
        let first =
            ArtifactStoreV1::materialize(&mut fixture.authority, &fixture.request, &fixture.pair);
        assert_eq!(first.change(), ArtifactStoreChangeV1::Changed);
        let first_view = first.result().expect("virgin materialization succeeds");
        assert_eq!(
            first_view.state(),
            ArtifactStoreOperationStateV1::Materialized,
        );
        let canonical_operation = first_view.operation().clone();
        let receipt_ref = first_view.receipt_ref().expect("materialized receipt");

        let query = ArtifactStoreV1::query(&mut fixture.authority, fixture.request.operation_id());
        assert_eq!(query.change(), ArtifactStoreChangeV1::Unchanged);
        assert_eq!(
            query.result().expect("query succeeds").operation(),
            &canonical_operation,
        );

        let bundle = ArtifactStoreV1::read_verified(
            &mut fixture.authority,
            fixture.pair.object_ref(),
            receipt_ref,
        )
        .expect("verified read succeeds");
        assert_eq!(bundle.request(), &fixture.request);
        assert_eq!(bundle.pair(), Some(&fixture.pair));
        assert_eq!(
            bundle.terminal().state(),
            MaterializationTerminalStateV1::Materialized,
        );

        let replay =
            ArtifactStoreV1::materialize(&mut fixture.authority, &fixture.request, &fixture.pair);
        assert_eq!(replay.change(), ArtifactStoreChangeV1::Unchanged);
        assert_eq!(
            replay
                .result()
                .expect("same request replay succeeds")
                .operation(),
            &canonical_operation,
        );
    }

    #[test]
    fn unix_bootstrap_exclusive_contention_is_one_shot() {
        let mut fixture = TestStoreFixture::new(b"unix-lock-contention ", 0x52);
        create_test_directory(fixture.state_root());
        let staging = fixture.state_root().join(STORE_STAGING_NAME);
        create_test_directory(&staging);
        let lock = create_test_regular(&staging.join(STORE_LOCK_NAME), &[]);
        lock.try_lock().expect("hold bootstrap exclusive lock");
        let before = tree_fingerprint(fixture.state_root());

        let invocation =
            ArtifactStoreV1::materialize(&mut fixture.authority, &fixture.request, &fixture.pair);
        assert_eq!(invocation.change(), ArtifactStoreChangeV1::Unchanged);
        assert_eq!(
            invocation
                .into_result()
                .expect_err("bootstrap lock is contended"),
            ArtifactStoreFailureV1::Contended,
        );
        assert_eq!(tree_fingerprint(fixture.state_root()), before);
        lock.unlock().expect("release bootstrap exclusive lock");
        drop(lock);
    }

    #[test]
    fn unix_noreplace_preserves_directory_and_pair_destinations() {
        let temp = TestTempDir::new();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        create_test_directory(&source);
        create_test_directory(&destination);
        drop(create_test_regular(&source.join("marker"), b"source"));
        drop(create_test_regular(
            &destination.join("marker"),
            b"destination",
        ));
        let parent = open_test_directory(temp.path());
        assert_eq!(
            renameat_with(
                &parent.file,
                "source",
                &parent.file,
                "destination",
                RenameFlags::NOREPLACE,
            ),
            Err(rustix::io::Errno::EXIST),
        );
        let source_marker = fs::read(source.join("marker")).expect("source marker");
        let destination_marker = fs::read(destination.join("marker")).expect("destination marker");
        assert_eq!(source_marker.as_slice(), b"source");
        assert_eq!(destination_marker.as_slice(), b"destination");

        let child_path = temp.path().join("pair");
        create_test_directory(&child_path);
        let child = open_test_directory(&child_path);
        let destination_bytes = b"preserved destination";
        drop(create_test_regular(
            &child_path.join(MANIFEST_NAME),
            destination_bytes,
        ));
        let pair =
            VerifiedArtifactPairV1::from_payload(b"noreplace pair ").expect("valid noreplace pair");
        assert_eq!(
            publish_pair_file(
                &child,
                MANIFEST_NEXT_NAME,
                MANIFEST_NAME,
                pair.manifest_bytes(),
            ),
            Err(StoreError::Io),
        );
        let preserved =
            fs::read(child_path.join(MANIFEST_NAME)).expect("preserved pair destination");
        let retained =
            fs::read(child_path.join(MANIFEST_NEXT_NAME)).expect("retained pair candidate");
        assert_eq!(preserved.as_slice(), destination_bytes);
        assert_eq!(retained.as_slice(), pair.manifest_bytes());
        drop(child);
        drop(parent);
    }

    #[test]
    fn unix_existing_child_sync_and_reopen_faults_are_owner_unknown() {
        for (operation_byte, point) in [
            (0x53, StoreFaultPoint::ExistingChildBeforeObjectsSync),
            (0x54, StoreFaultPoint::ExistingChildBeforeObjectsReopen),
        ] {
            let mut fixture = TestStoreFixture::new(b"existing-child-fault ", operation_byte);
            let (store, _binding, mut tracker) = open_materializing_fixture(&mut fixture);
            write_full_unindexed_pair(&store, &fixture.pair).expect("matching full unindexed pair");
            let mut observer = FailingObserver {
                target: point,
                seen: Vec::new(),
                fired: false,
            };
            assert!(matches!(
                publish_or_recover_pair_observed(
                    &store,
                    &fixture.pair,
                    &mut tracker,
                    &mut observer,
                ),
                Err(StoreError::Owner),
            ));
            assert!(observer.fired);
            assert_eq!(tracker.change(), ArtifactStoreChangeV1::Unknown);
            store.release().expect("release faulted test store");

            let query =
                ArtifactStoreV1::query(&mut fixture.authority, fixture.request.operation_id());
            assert_eq!(query.change(), ArtifactStoreChangeV1::Unchanged);
            let view = query.result().expect("materializing query after fault");
            assert_eq!(view.state(), ArtifactStoreOperationStateV1::Materializing);
            assert!(view.operation().terminal().is_none());
        }
    }

    #[test]
    fn unix_fresh_late_full_pair_fault_recovers_to_materialized() {
        let mut fixture = TestStoreFixture::new(b"fresh-late-full-pair ", 0x55);
        let request = fixture.request.clone();
        let pair = fixture.pair.clone();
        let (mut store, binding, mut tracker) = open_materializing_fixture(&mut fixture);
        let (object, object_successor) =
            object_candidate(&store, request.operation_id(), &pair).expect("object successor");
        let mut observer = FailingObserver {
            target: StoreFaultPoint::FreshPairAfterSecondFinalBeforeDurabilityProof,
            seen: Vec::new(),
            fired: false,
        };
        let publication =
            publish_or_recover_pair_observed(&store, &pair, &mut tracker, &mut observer)
                .expect("late full pair is recovered in the same call");
        let PairPublication::Complete(verified) = publication else {
            panic!("late full pair must recover as complete");
        };
        assert!(observer.fired);
        assert_eq!(
            observer.seen.first(),
            Some(&StoreFaultPoint::FreshPairAfterSecondFinalBeforeDurabilityProof),
        );
        assert!(
            observer
                .seen
                .iter()
                .skip(1)
                .any(|point| *point == StoreFaultPoint::ExistingChildBeforeObjectsSync)
        );
        assert_eq!(
            observer
                .seen
                .iter()
                .filter(|point| {
                    **point == StoreFaultPoint::FreshPairAfterSecondFinalBeforeDurabilityProof
                })
                .count(),
            1,
        );
        assert_eq!(verified.as_ref(), &pair);

        commit_successor(
            &mut store,
            &mut fixture.authority,
            &binding,
            object_successor,
            &mut tracker,
        )
        .expect("commit recovered object successor");
        let operation = commit_terminal_and_receipt(
            &mut store,
            &mut fixture.authority,
            &binding,
            TerminalCommitInput {
                operation_id: request.operation_id(),
                object: Some(&object),
                state: MaterializationTerminalStateV1::Materialized,
                quarantine: ArtifactQuarantineFactsV1::Absent,
            },
            &mut tracker,
        )
        .expect("commit recovered materialized terminal");
        assert_eq!(
            operation.terminal().expect("materialized terminal").state(),
            MaterializationTerminalStateV1::Materialized,
        );
        assert_eq!(
            store.snapshot.quarantine(),
            ArtifactQuarantineFactsV1::Absent
        );
        assert_eq!(tracker.change(), ArtifactStoreChangeV1::Changed);
        store.release().expect("release recovered test store");

        let query = ArtifactStoreV1::query(&mut fixture.authority, request.operation_id());
        assert_eq!(query.change(), ArtifactStoreChangeV1::Unchanged);
        assert_eq!(
            query
                .result()
                .expect("query recovered materialization")
                .state(),
            ArtifactStoreOperationStateV1::Materialized,
        );
    }

    #[test]
    fn unix_initial_non_authoritative_prefixes_are_not_found_and_unchanged() {
        let mut fixture = TestStoreFixture::new(b"initial-prefix-query ", 0x56);
        create_test_directory(fixture.state_root());
        let staging = fixture.state_root().join(STORE_STAGING_NAME);
        create_test_directory(&staging);
        assert_prefix_query_not_found_and_unchanged(&mut fixture);

        drop(create_test_regular(&staging.join(STORE_LOCK_NAME), &[]));
        assert_prefix_query_not_found_and_unchanged(&mut fixture);

        create_test_directory(&staging.join(OBJECTS_NAME));
        assert_prefix_query_not_found_and_unchanged(&mut fixture);
    }

    #[test]
    fn authority_binding_rejects_noncanonical_or_overlong_paths() {
        let commitment = config(0x11);
        for path in ["", ".", "relative", "/", "/a/", "/a//b", "/a/../b"] {
            assert_eq!(
                ArtifactStoreAuthorityBindingV1::try_new(PathBuf::from(path), commitment),
                Err(ArtifactStoreAuthorityRecheckFailureV1::UnsafePath),
                "path {path:?}",
            );
        }
        let overlong = format!("/{}", "a".repeat(MAX_STATE_ROOT_UTF8_BYTES));
        assert_eq!(
            ArtifactStoreAuthorityBindingV1::try_new(PathBuf::from(overlong), commitment),
            Err(ArtifactStoreAuthorityRecheckFailureV1::UnsafePath),
        );
        let accepted =
            ArtifactStoreAuthorityBindingV1::try_new(PathBuf::from("/strict/state"), commitment)
                .expect("canonical absolute path");
        assert_eq!(accepted.state_root(), Path::new("/strict/state"));
        assert_eq!(accepted.config_commitment(), commitment);
    }

    #[test]
    fn changed_projection_is_exactly_three_state() {
        let mut tracker = ChangeTracker::default();
        assert_eq!(tracker.change(), ArtifactStoreChangeV1::Unchanged);
        tracker.ambiguous();
        assert_eq!(tracker.change(), ArtifactStoreChangeV1::Unknown);
        tracker.restore(ChangeTracker::default());
        assert_eq!(tracker.change(), ArtifactStoreChangeV1::Unchanged);
        tracker.committed();
        assert_eq!(tracker.change(), ArtifactStoreChangeV1::Changed);
        tracker.ambiguous();
        assert_eq!(tracker.change(), ArtifactStoreChangeV1::Unknown);
    }

    #[test]
    fn recovery_faults_map_to_owner_with_unknown_change() {
        for point in [
            StoreFaultPoint::ExistingChildBeforeObjectsSync,
            StoreFaultPoint::ExistingChildBeforeObjectsReopen,
            StoreFaultPoint::ExistingChildBeforeChildReopen,
            StoreFaultPoint::ExistingChildBeforeFileSync,
            StoreFaultPoint::ExistingChildBeforeChildSync,
            StoreFaultPoint::ExistingChildBeforeParentSync,
            StoreFaultPoint::ExistingChildBeforeParentReopen,
            StoreFaultPoint::ExistingChildBeforeFinalChildReopen,
            StoreFaultPoint::FreshPairAfterSecondFinalBeforeDurabilityProof,
            StoreFaultPoint::PairMkdirFailureBeforeObjectsSync,
            StoreFaultPoint::PairMkdirFailureBeforeObjectsReopen,
        ] {
            let mut observer = FailingObserver {
                target: point,
                seen: Vec::new(),
                fired: false,
            };
            let mut tracker = ChangeTracker::default();
            tracker.ambiguous();
            assert_eq!(
                recovery_fault_checkpoint(&mut observer, point),
                Err(StoreError::Owner),
            );
            assert_eq!(observer.seen, vec![point]);
            assert_eq!(tracker.change(), ArtifactStoreChangeV1::Unknown);
        }
    }

    #[test]
    fn bootstrap_lock_collision_is_one_shot_and_never_guesses_owner() {
        assert_eq!(
            classify_bootstrap_lock_collision(StoreError::Contended),
            StoreError::Contended,
        );
        assert_eq!(
            classify_bootstrap_lock_collision(StoreError::Owner),
            StoreError::PublicationUncertain(None),
        );
        assert_eq!(
            classify_bootstrap_lock_collision(StoreError::Io),
            StoreError::PublicationUncertain(None),
        );
    }

    #[test]
    fn cleanup_identity_guard_rejects_missing_or_replaced_entry() {
        let expected = FileIdentity {
            device: 7,
            inode: 11,
        };
        assert_eq!(validate_cleanup_identity(Some(expected), expected), Ok(()));
        assert_eq!(
            validate_cleanup_identity(None, expected),
            Err(StoreError::Owner),
        );
        assert_eq!(
            validate_cleanup_identity(
                Some(FileIdentity {
                    device: 7,
                    inode: 12,
                }),
                expected,
            ),
            Err(StoreError::Owner),
        );
    }

    #[test]
    fn snapshot_rename_settle_requires_exact_old_or_new_inode_state() {
        let active_identity = FileIdentity {
            device: 7,
            inode: 11,
        };
        let next_identity = FileIdentity {
            device: 7,
            inode: 12,
        };
        let replacement_identity = FileIdentity {
            device: 7,
            inode: 13,
        };
        let old = b"old snapshot";
        let next = b"next snapshot";
        let base = SnapshotRenameObservation {
            expected_active_identity: active_identity,
            expected_active_bytes: old,
            active_identity,
            active_bytes: old,
            expected_next_identity: next_identity,
            expected_next_bytes: next,
            named_next_identity: Some(next_identity),
            observed_next: Some((next_identity, next)),
            stable_root: false,
            root_with_next: true,
        };
        assert_eq!(
            classify_snapshot_rename_observation(&base),
            SnapshotRenameState::OldWithNext,
        );
        assert_eq!(
            classify_snapshot_rename_observation(&SnapshotRenameObservation {
                active_identity: next_identity,
                active_bytes: next,
                named_next_identity: None,
                observed_next: None,
                stable_root: true,
                root_with_next: false,
                ..base
            }),
            SnapshotRenameState::NewWithoutNext,
        );
        assert_eq!(
            classify_snapshot_rename_observation(&SnapshotRenameObservation {
                active_identity: replacement_identity,
                active_bytes: next,
                named_next_identity: None,
                observed_next: None,
                stable_root: true,
                root_with_next: false,
                ..base
            }),
            SnapshotRenameState::Ambiguous,
        );
        assert_eq!(
            classify_snapshot_rename_observation(&SnapshotRenameObservation {
                root_with_next: false,
                ..base
            }),
            SnapshotRenameState::Ambiguous,
        );
    }

    #[test]
    fn object_directory_name_is_fixed_lowercase_digest_name() {
        let pair =
            VerifiedArtifactPairV1::from_payload(b"pair-name-test ").expect("valid fixture pair");
        let name = object_directory_name(pair.object_ref());
        assert_eq!(name.len(), 131);
        assert!(name.starts_with("o-"));
        assert_eq!(name.as_bytes()[66], b'-');
        assert!(
            name[2..66]
                .bytes()
                .chain(name[67..].bytes())
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
    }

    #[test]
    fn permitted_next_accepts_only_contract_successor() {
        let (initial, _) = initial_snapshot();
        let operation = initial.operations().first().expect("initial operation");
        let materializing = MaterializingRecordV1::new(operation.admission());
        let next = initial
            .try_successor(ArtifactSnapshotSuccessorV1::Materializing { materializing })
            .expect("valid materializing successor");
        assert_eq!(
            permitted_next(&initial, &next),
            Ok(operation.operation_id()),
        );
        assert_eq!(permitted_next(&initial, &initial), Err(StoreError::Owner));
    }

    #[test]
    fn complete_initial_staging_query_rejects_materializing_successor() {
        let (initial, _) = initial_snapshot();
        let operation_id = initial.operations()[0].operation_id();
        assert_eq!(
            initial_staging_operation(&initial, operation_id),
            Ok(initial.operations()[0].clone()),
        );
        assert_eq!(
            initial_staging_operation(&initial, operation(0x23)),
            Err(StoreError::Owner),
        );
        let materializing = MaterializingRecordV1::new(initial.operations()[0].admission());
        let progressed = initial
            .try_successor(ArtifactSnapshotSuccessorV1::Materializing { materializing })
            .expect("valid materializing successor");
        assert_eq!(
            initial_staging_operation(&progressed, operation_id),
            Err(StoreError::Owner),
        );
    }

    #[test]
    fn complete_initial_staging_keeps_operation_attribution_distinct_from_request_drift() {
        let (initial, _) = initial_snapshot();
        let durable = initial.operations()[0].request();
        let different_operation = MaterializationRequestV1::new(
            operation(0x23),
            durable.config_commitment(),
            durable.object_ref(),
        );
        assert_eq!(
            validate_initial_request(&initial, &different_operation),
            Err(StoreError::Owner),
        );

        let other_pair = VerifiedArtifactPairV1::from_payload(b"request-drift-test ")
            .expect("valid request drift pair");
        let request_drift = MaterializationRequestV1::new(
            durable.operation_id(),
            durable.config_commitment(),
            other_pair.object_ref(),
        );
        assert_eq!(
            validate_initial_request(&initial, &request_drift),
            Err(StoreError::Conflict),
        );
    }

    #[test]
    fn public_operation_view_tracks_canonical_presence() {
        let (initial, _) = initial_snapshot();
        let admitted = initial.operations()[0].clone();
        assert_eq!(
            ArtifactStoreOperationViewV1::new(admitted).state(),
            ArtifactStoreOperationStateV1::Admitted,
        );
        let materializing = MaterializingRecordV1::new(initial.operations()[0].admission());
        let progressing = initial
            .try_successor(ArtifactSnapshotSuccessorV1::Materializing { materializing })
            .expect("valid materializing successor");
        assert_eq!(
            ArtifactStoreOperationViewV1::new(progressing.operations()[0].clone()).state(),
            ArtifactStoreOperationStateV1::Materializing,
        );
    }

    #[test]
    fn read_failure_keeps_reference_and_configuration_distinct() {
        assert_eq!(
            StoreError::ReferenceMismatch.into_read(),
            ArtifactStoreReadFailureV1::ReferenceMismatch,
        );
        assert_eq!(
            StoreError::ConfigurationMismatch.into_read(),
            ArtifactStoreReadFailureV1::ConfigurationMismatch,
        );
        assert_eq!(
            StoreError::Contended.into_read(),
            ArtifactStoreReadFailureV1::Contended,
        );
    }
}
