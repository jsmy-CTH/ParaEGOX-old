use std::{
    env,
    ffi::{OsStr, OsString},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    process::ExitCode,
};

use config::{
    Command, DeveloperDeploymentConfigV1, DeveloperDistributedFixtureActionV1,
    DeveloperDistributedFixtureConfigV1, DeveloperFixtureConfigV1, DeveloperNodeConfigV1,
    DeveloperProvisionedConfigV1, LocalLifecycleActionV1, LocalLifecycleJsonIntentV1,
    OfflineCommandV1, OfflineConfigSummaryV1, OfflineJsonIntentV1,
};
#[cfg(unix)]
use config::{LocalLifecycleCommandV1, LocalManagedChatConfigV1};
use error::LocalProcessError;
use serde_json::json;

#[cfg(unix)]
mod composition;
mod config;
mod error;
#[cfg(unix)]
mod identity;
#[cfg(unix)]
mod initializer;
#[cfg(unix)]
mod inspection;
#[cfg(unix)]
mod layout;
#[cfg(unix)]
mod lifecycle;

pub(crate) const NODE_DAEMON_CHILD_MODE: &str = "__node-daemon-child-v1";
pub(crate) const NODE_BOOTSTRAP_FILE_OPTION: &str = "--node-bootstrap-file";
pub(crate) const NODE_OBSERVATION_BOOTSTRAP_FILE_OPTION: &str = "--node-observation-bootstrap-file";
const OFFLINE_OUTPUT_SCHEMA_VERSION: u16 = 1;
const LIFECYCLE_OUTPUT_SCHEMA_VERSION: u16 = 1;
const INIT_OUTPUT_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DispatchOutcome {
    Success,
    DiagnosticFailure,
    ConfigurationFailure,
    #[cfg(unix)]
    HiddenSupervisorContended,
}

impl DispatchOutcome {
    fn exit_code(self) -> ExitCode {
        match self {
            Self::Success => ExitCode::SUCCESS,
            Self::DiagnosticFailure => ExitCode::from(1),
            Self::ConfigurationFailure => ExitCode::from(2),
            #[cfg(unix)]
            Self::HiddenSupervisorContended => {
                ExitCode::from(lifecycle::LOCAL_CHAT_SUPERVISOR_CONTENTION_EXIT_CODE_V1)
            }
        }
    }

    const fn from_lifecycle_exit_code(exit_code: u8) -> Self {
        match exit_code {
            0 => Self::Success,
            2 => Self::ConfigurationFailure,
            _ => Self::DiagnosticFailure,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DoctorEnvironmentV1 {
    platform_supported: bool,
    execution_identity_non_root: Option<bool>,
}

struct LifecycleJsonLineV1<'a> {
    action: LocalLifecycleActionV1,
    ok: bool,
    state: &'a str,
    generation: Option<&'a str>,
    changed: bool,
    owner_readiness_observed: bool,
    diagnostic: Option<(&'a str, &'a str)>,
}

#[cfg(unix)]
struct LocalChatSupervisorInvocationV1 {
    config: LocalManagedChatConfigV1,
    expected_commitment: [u8; 32],
    expected_generation: [u8; 16],
}

fn main() -> ExitCode {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    if config::init_json_intent(&arguments) {
        return dispatch_init_to(&mut io::stdout().lock(), &arguments).exit_code();
    }
    let offline_json_intent = config::offline_json_intent(&arguments);
    let lifecycle_json_intent = config::lifecycle_json_intent(&arguments);
    match dispatch(arguments) {
        Ok(outcome) => outcome.exit_code(),
        Err(error) => match (lifecycle_json_intent, offline_json_intent) {
            (Some(intent), _) => ExitCode::from(finish_lifecycle_dispatch_error(
                &mut io::stdout().lock(),
                intent,
                error,
            )),
            (None, Some(intent)) => ExitCode::from(finish_offline_dispatch_error(
                &mut io::stdout().lock(),
                intent,
                error,
            )),
            (None, None) => {
                eprintln!(
                    "paraegox: code={} message={}",
                    error.code(),
                    error.message()
                );
                if matches!(error, LocalProcessError::Configuration(_)) {
                    eprintln!();
                    print_usage_to_stderr();
                }
                ExitCode::from(error.exit_code())
            }
        },
    }
}

fn dispatch_init_to(output: &mut impl Write, arguments: &[OsString]) -> DispatchOutcome {
    let parsed = match config::parse_init(arguments) {
        Ok(command) => command,
        Err(error) => {
            return finish_init_result(output, false, LocalProcessError::Configuration(error));
        }
    };
    #[cfg(unix)]
    {
        match initializer::initialize(parsed.directory()) {
            Ok(outcome) => {
                if write_init_json_line(output, true, outcome.changed(), None).is_ok() {
                    DispatchOutcome::Success
                } else {
                    DispatchOutcome::DiagnosticFailure
                }
            }
            Err(failure) => finish_init_result(output, failure.changed(), failure.error()),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = parsed;
        unreachable!("init parser rejects the unsupported non-Unix platform")
    }
}

fn finish_init_result(
    output: &mut impl Write,
    changed: bool,
    error: LocalProcessError,
) -> DispatchOutcome {
    if write_init_json_line(output, false, changed, Some(error)).is_err() {
        return DispatchOutcome::DiagnosticFailure;
    }
    if error.exit_code() == 2 {
        DispatchOutcome::ConfigurationFailure
    } else {
        DispatchOutcome::DiagnosticFailure
    }
}

fn write_init_json_line(
    output: &mut impl Write,
    ok: bool,
    changed: bool,
    error: Option<LocalProcessError>,
) -> Result<(), LocalProcessError> {
    let diagnostics = error.map_or_else(Vec::new, |error| {
        vec![json!({"code": error.code(), "message": error.message()})]
    });
    #[cfg(unix)]
    let (profile, config_relative_path, state_relative_path) = if ok {
        (
            Some(initializer::INIT_PROFILE),
            Some(initializer::INIT_CONFIG_RELATIVE_PATH),
            Some(initializer::INIT_STATE_RELATIVE_PATH),
        )
    } else {
        (None, None, None)
    };
    #[cfg(not(unix))]
    let (profile, config_relative_path, state_relative_path): (
        Option<&str>,
        Option<&str>,
        Option<&str>,
    ) = (None, None, None);
    serde_json::to_writer(
        &mut *output,
        &json!({
            "schema_version": INIT_OUTPUT_SCHEMA_VERSION,
            "command": "init",
            "ok": ok,
            "changed": changed,
            "profile": profile,
            "config_relative_path": config_relative_path,
            "state_relative_path": state_relative_path,
            "diagnostics": diagnostics,
        }),
    )
    .map_err(|_| LocalProcessError::InitJsonOutput)?;
    output
        .write_all(b"\n")
        .map_err(|_| LocalProcessError::InitJsonOutput)
}

fn finish_offline_dispatch_error(
    output: &mut impl Write,
    intent: OfflineJsonIntentV1,
    error: LocalProcessError,
) -> u8 {
    if matches!(error, LocalProcessError::OfflineJsonOutput) {
        return 1;
    }
    let exit_code = error.exit_code();
    if write_offline_error_json(output, intent, error).is_err() {
        1
    } else {
        exit_code
    }
}

fn finish_lifecycle_dispatch_error(
    output: &mut impl Write,
    intent: LocalLifecycleJsonIntentV1,
    error: LocalProcessError,
) -> u8 {
    if matches!(error, LocalProcessError::LifecycleJsonOutput) {
        return 1;
    }
    let exit_code = error.exit_code();
    if write_lifecycle_json_line(
        output,
        LifecycleJsonLineV1 {
            action: intent.action(),
            ok: false,
            state: "unknown",
            generation: None,
            changed: false,
            owner_readiness_observed: false,
            diagnostic: Some((error.code(), error.message())),
        },
    )
    .is_err()
    {
        1
    } else {
        exit_code
    }
}

#[cfg(unix)]
fn dispatch_lifecycle_to(
    output: &mut impl Write,
    command: LocalLifecycleCommandV1,
) -> Result<DispatchOutcome, LocalProcessError> {
    let action = command.action();
    let config = command.into_config();
    let result = match action {
        LocalLifecycleActionV1::Up => lifecycle::run_up(&config),
        LocalLifecycleActionV1::Status => lifecycle::run_status(&config),
        LocalLifecycleActionV1::Down => lifecycle::run_down(&config),
    };
    let observation =
        result.unwrap_or_else(|error| lifecycle::project_failure(&config, action, error));
    let diagnostic = observation
        .diagnostic()
        .map(|value| (value.code(), value.message()));
    write_lifecycle_json_line(
        output,
        LifecycleJsonLineV1 {
            action,
            ok: observation.ok(),
            state: observation.state().as_str(),
            generation: observation.generation(),
            changed: observation.changed(),
            owner_readiness_observed: observation.owner_readiness_observed(),
            diagnostic,
        },
    )?;
    Ok(DispatchOutcome::from_lifecycle_exit_code(
        observation.exit_code(),
    ))
}

fn write_lifecycle_json_line(
    output: &mut impl Write,
    line: LifecycleJsonLineV1<'_>,
) -> Result<(), LocalProcessError> {
    let diagnostics = line.diagnostic.map_or_else(Vec::new, |(code, message)| {
        vec![json!({"code": code, "message": message})]
    });
    serde_json::to_writer(
        &mut *output,
        &json!({
            "schema_version": LIFECYCLE_OUTPUT_SCHEMA_VERSION,
            "command": line.action.as_str(),
            "ok": line.ok,
            "state": line.state,
            "generation": line.generation,
            "changed": line.changed,
            "owner_readiness_observed": line.owner_readiness_observed,
            "inspection_checked": false,
            "diagnostics": diagnostics,
        }),
    )
    .map_err(|_| LocalProcessError::LifecycleJsonOutput)?;
    output
        .write_all(b"\n")
        .map_err(|_| LocalProcessError::LifecycleJsonOutput)
}

fn dispatch(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<DispatchOutcome, LocalProcessError> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    #[cfg(not(unix))]
    if let Some(intent) = config::offline_json_intent(&arguments)
        .filter(|intent| intent.command() == "doctor.offline")
    {
        return write_unsupported_platform_doctor_json(&mut io::stdout().lock(), intent);
    }
    if let Some(command) = config::parse_offline(&arguments)? {
        return dispatch_offline_to(
            &mut io::stdout().lock(),
            command,
            current_doctor_environment(),
        );
    }
    if let Some(command) = config::parse_lifecycle(&arguments)? {
        #[cfg(unix)]
        {
            return dispatch_lifecycle_to(&mut io::stdout().lock(), command);
        }
        #[cfg(not(unix))]
        {
            let _ = command;
            unreachable!("configuration rejects managed lifecycle before Unix dispatch")
        }
    }
    #[cfg(unix)]
    if let Some(supervisor) = parse_local_chat_supervisor(&arguments)? {
        return match lifecycle::run_supervisor(
            supervisor.config,
            supervisor.expected_commitment,
            supervisor.expected_generation,
        ) {
            Ok(lifecycle::LocalChatSupervisorResultV1::Completed) => Ok(DispatchOutcome::Success),
            Ok(lifecycle::LocalChatSupervisorResultV1::Contended) => {
                Ok(DispatchOutcome::HiddenSupervisorContended)
            }
            Err(error) => Err(error),
        };
    }
    if let Some(paths) = parse_node_daemon_child(&arguments)? {
        return run_node_daemon_child(&paths).map(|()| DispatchOutcome::Success);
    }
    match config::parse(arguments)? {
        Command::Help => {
            print_usage();
            Ok(DispatchOutcome::Success)
        }
        Command::DeveloperNodeV1(config) => {
            compose_real_node(*config).map(|()| DispatchOutcome::Success)
        }
        Command::DeveloperDeploymentV1(config) => {
            compose_real_deployment(*config).map(|()| DispatchOutcome::Success)
        }
        Command::DeveloperFixtureV1(config) => {
            compose_real_local_stack(config).map(|()| DispatchOutcome::Success)
        }
        Command::DeveloperDistributedFixtureV1(config) => match config.action() {
            DeveloperDistributedFixtureActionV1::Run => {
                compose_real_distributed_fixture_stack(config).map(|()| DispatchOutcome::Success)
            }
            DeveloperDistributedFixtureActionV1::InitializeIdentity => {
                initialize_distributed_identity(config).map(|()| DispatchOutcome::Success)
            }
        },
        Command::DeveloperProvisionedV1(config) => {
            compose_real_provisioned_stack(config).map(|()| DispatchOutcome::Success)
        }
    }
}

#[cfg(not(unix))]
fn write_unsupported_platform_doctor_json(
    output: &mut impl Write,
    intent: OfflineJsonIntentV1,
) -> Result<DispatchOutcome, LocalProcessError> {
    write_offline_json_line(
        output,
        &json!({
            "schema_version": OFFLINE_OUTPUT_SCHEMA_VERSION,
            "command": "doctor.offline",
            "kind": intent.target().map(|target| target.as_str()),
            "ok": false,
            "config_schema_version": null,
            "diagnostics": [{
                "code": "PXLC-DOCTOR-PLATFORM",
                "message": "offline doctor requires a supported Unix platform",
            }],
            "checks_performed": [{
                "id": "platform",
                "ok": false,
                "supported": false,
            }],
            "checks_skipped": [
                {
                    "id": "configuration",
                    "reason": "configuration was not opened on the unsupported platform",
                },
                {
                    "id": "execution_identity_non_root",
                    "reason": "execution identity is not available on the unsupported platform",
                },
                {
                    "id": "secret_input_reference_presence",
                    "reason": "configuration was not opened on the unsupported platform",
                },
                {
                    "id": "owner_startup",
                    "reason": "offline doctor does not start owners",
                },
                {
                    "id": "network_connectivity",
                    "reason": "offline doctor does not access the network or establish network readiness",
                },
                {
                    "id": "runtime_readiness",
                    "reason": "offline doctor does not establish runtime readiness",
                },
                {
                    "id": "secret_value_resolution",
                    "reason": "offline doctor does not resolve or read Secret values",
                },
                {
                    "id": "credential_and_key_files",
                    "reason": "offline doctor does not open credential, seed, or private-key files",
                },
            ],
        }),
    )?;
    Ok(DispatchOutcome::DiagnosticFailure)
}

fn dispatch_offline_to(
    output: &mut impl Write,
    command: OfflineCommandV1,
    doctor_environment: DoctorEnvironmentV1,
) -> Result<DispatchOutcome, LocalProcessError> {
    match command {
        OfflineCommandV1::Version => {
            write_offline_json_line(
                output,
                &json!({
                    "schema_version": OFFLINE_OUTPUT_SCHEMA_VERSION,
                    "command": "version",
                    "ok": true,
                    "version": env!("CARGO_PKG_VERSION"),
                    "diagnostics": [],
                    "checks_performed": [],
                    "checks_skipped": [],
                }),
            )?;
            Ok(DispatchOutcome::Success)
        }
        OfflineCommandV1::ConfigCheck(summary) => {
            write_offline_config_check_json(output, summary)?;
            Ok(DispatchOutcome::Success)
        }
        OfflineCommandV1::Doctor(summary) => {
            write_offline_doctor_json(output, summary, doctor_environment)
        }
    }
}

fn write_offline_config_check_json(
    output: &mut impl Write,
    summary: OfflineConfigSummaryV1,
) -> Result<(), LocalProcessError> {
    write_offline_json_line(
        output,
        &json!({
            "schema_version": OFFLINE_OUTPUT_SCHEMA_VERSION,
            "command": "config.check",
            "kind": summary.target().as_str(),
            "ok": true,
            "config_schema_version": summary.schema_version(),
            "profile": summary.profile(),
            "secret_input_reference_present": summary.secret_input_reference_present(),
            "diagnostics": [],
            "checks_performed": [
                {"id": "configuration", "ok": true},
            ],
            "checks_skipped": [
                {
                    "id": "owner_startup",
                    "reason": "offline config check does not start owners",
                },
                {
                    "id": "network_connectivity",
                    "reason": "offline config check does not access the network",
                },
                {
                    "id": "runtime_readiness",
                    "reason": "offline config check does not establish runtime readiness",
                },
                {
                    "id": "secret_value_resolution",
                    "reason": "offline config check does not resolve Secret values",
                },
            ],
        }),
    )
}

fn write_offline_doctor_json(
    output: &mut impl Write,
    summary: OfflineConfigSummaryV1,
    environment: DoctorEnvironmentV1,
) -> Result<DispatchOutcome, LocalProcessError> {
    let mut diagnostics = Vec::new();
    if !environment.platform_supported {
        diagnostics.push(json!({
            "code": "PXLC-DOCTOR-PLATFORM",
            "message": "offline doctor requires a supported Unix platform",
        }));
    }
    if environment.execution_identity_non_root == Some(false) {
        diagnostics.push(json!({
            "code": "PXLC-DOCTOR-EXECUTION-IDENTITY",
            "message": "offline doctor requires a non-root user and group",
        }));
    }

    let mut checks_performed = vec![
        json!({"id": "configuration", "ok": true}),
        json!({
            "id": "platform",
            "ok": environment.platform_supported,
            "supported": environment.platform_supported,
        }),
    ];
    let mut checks_skipped = Vec::new();
    match environment.execution_identity_non_root {
        Some(non_root) => checks_performed.push(json!({
            "id": "execution_identity_non_root",
            "ok": non_root,
            "non_root": non_root,
        })),
        None => checks_skipped.push(json!({
            "id": "execution_identity_non_root",
            "reason": "execution identity is not available on the unsupported platform",
        })),
    }
    checks_performed.push(json!({
        "id": "secret_input_reference_presence",
        "ok": true,
        "present": summary.secret_input_reference_present(),
    }));
    checks_skipped.extend([
        json!({
            "id": "owner_startup",
            "reason": "offline doctor does not start owners",
        }),
        json!({
            "id": "network_connectivity",
            "reason": "offline doctor does not access the network or establish network readiness",
        }),
        json!({
            "id": "runtime_readiness",
            "reason": "offline doctor does not establish runtime readiness",
        }),
        json!({
            "id": "secret_value_resolution",
            "reason": "offline doctor does not resolve or read Secret values",
        }),
        json!({
            "id": "credential_and_key_files",
            "reason": "offline doctor does not open credential, seed, or private-key files",
        }),
    ]);

    let ok = diagnostics.is_empty();
    write_offline_json_line(
        output,
        &json!({
            "schema_version": OFFLINE_OUTPUT_SCHEMA_VERSION,
            "command": "doctor.offline",
            "kind": summary.target().as_str(),
            "ok": ok,
            "config_schema_version": summary.schema_version(),
            "profile": summary.profile(),
            "secret_input_reference_present": summary.secret_input_reference_present(),
            "diagnostics": diagnostics,
            "checks_performed": checks_performed,
            "checks_skipped": checks_skipped,
        }),
    )?;
    if ok {
        Ok(DispatchOutcome::Success)
    } else {
        Ok(DispatchOutcome::DiagnosticFailure)
    }
}

fn write_offline_error_json(
    output: &mut impl Write,
    intent: OfflineJsonIntentV1,
    error: LocalProcessError,
) -> Result<(), LocalProcessError> {
    let checks_performed = if intent.command() == "version" {
        Vec::new()
    } else {
        vec![json!({"id": "configuration", "ok": false})]
    };
    let checks_skipped = if intent.command() == "doctor.offline" {
        vec![
            json!({
                "id": "platform",
                "reason": "configuration validation failed before the platform check",
            }),
            json!({
                "id": "execution_identity_non_root",
                "reason": "configuration validation failed before the execution identity check",
            }),
            json!({
                "id": "secret_input_reference_presence",
                "reason": "configuration validation did not produce a safe summary",
            }),
            json!({
                "id": "owner_startup",
                "reason": "offline doctor does not start owners",
            }),
            json!({
                "id": "network_connectivity",
                "reason": "offline doctor does not access the network or establish network readiness",
            }),
            json!({
                "id": "runtime_readiness",
                "reason": "offline doctor does not establish runtime readiness",
            }),
            json!({
                "id": "secret_value_resolution",
                "reason": "offline doctor does not resolve or read Secret values",
            }),
            json!({
                "id": "credential_and_key_files",
                "reason": "offline doctor does not open credential, seed, or private-key files",
            }),
        ]
    } else {
        Vec::new()
    };
    write_offline_json_line(
        output,
        &json!({
            "schema_version": OFFLINE_OUTPUT_SCHEMA_VERSION,
            "command": intent.command(),
            "kind": intent.target().map(|target| target.as_str()),
            "ok": false,
            "config_schema_version": null,
            "diagnostics": [
                {"code": error.code(), "message": error.message()},
            ],
            "checks_performed": checks_performed,
            "checks_skipped": checks_skipped,
        }),
    )
}

fn write_offline_json_line(
    output: &mut impl Write,
    value: &serde_json::Value,
) -> Result<(), LocalProcessError> {
    serde_json::to_writer(&mut *output, value).map_err(|_| LocalProcessError::OfflineJsonOutput)?;
    output
        .write_all(b"\n")
        .map_err(|_| LocalProcessError::OfflineJsonOutput)
}

fn current_doctor_environment() -> DoctorEnvironmentV1 {
    #[cfg(unix)]
    {
        DoctorEnvironmentV1 {
            platform_supported: true,
            execution_identity_non_root: Some(
                !nix::unistd::Uid::effective().is_root()
                    && nix::unistd::Gid::effective().as_raw() != 0,
            ),
        }
    }
    #[cfg(not(unix))]
    {
        DoctorEnvironmentV1 {
            platform_supported: false,
            execution_identity_non_root: None,
        }
    }
}

fn compose_real_deployment(config: DeveloperDeploymentConfigV1) -> Result<(), LocalProcessError> {
    #[cfg(unix)]
    {
        composition::run_deployment(config)
    }
    #[cfg(not(unix))]
    {
        let _ = config;
        unreachable!("configuration rejects DeveloperLocal before Deployment composition")
    }
}

fn compose_real_node(config: DeveloperNodeConfigV1) -> Result<(), LocalProcessError> {
    #[cfg(unix)]
    {
        composition::run_node(config)
    }
    #[cfg(not(unix))]
    {
        let _ = config;
        unreachable!("configuration rejects DeveloperLocal before node composition on non-Unix")
    }
}

#[cfg(unix)]
fn parse_local_chat_supervisor(
    arguments: &[OsString],
) -> Result<Option<LocalChatSupervisorInvocationV1>, LocalProcessError> {
    if arguments.first().map(OsString::as_os_str)
        != Some(OsStr::new(lifecycle::LOCAL_CHAT_SUPERVISOR_MODE_V1))
    {
        return Ok(None);
    }
    if arguments.len() != 7
        || arguments[1].as_os_str() != OsStr::new("--config")
        || arguments[3].as_os_str() != OsStr::new(lifecycle::EXPECTED_CONFIG_COMMITMENT_OPTION)
        || arguments[5].as_os_str() != OsStr::new(lifecycle::EXPECTED_GENERATION_OPTION)
    {
        return Err(LocalProcessError::LifecycleConfiguration);
    }
    let config = config::parse_managed_chat_supervisor_config(arguments[2].clone())?;
    let expected_commitment = lifecycle::decode_config_commitment_hex(arguments[4].as_os_str())?;
    let expected_generation = lifecycle::decode_generation_hex(arguments[6].as_os_str())?;
    Ok(Some(LocalChatSupervisorInvocationV1 {
        config,
        expected_commitment,
        expected_generation,
    }))
}

fn parse_node_daemon_child(
    arguments: &[OsString],
) -> Result<Option<NodeChildBootstrapPathsV1>, LocalProcessError> {
    if arguments.first().map(OsString::as_os_str) != Some(OsStr::new(NODE_DAEMON_CHILD_MODE)) {
        return Ok(None);
    }
    if !matches!(arguments.len(), 3 | 5)
        || arguments[1].as_os_str() != OsStr::new(NODE_BOOTSTRAP_FILE_OPTION)
        || arguments.len() == 5
            && arguments[3].as_os_str() != OsStr::new(NODE_OBSERVATION_BOOTSTRAP_FILE_OPTION)
    {
        return Err(LocalProcessError::NodeBootstrap);
    }
    let bootstrap_path = PathBuf::from(&arguments[2]);
    if !is_lexically_absolute_file(&bootstrap_path) {
        return Err(LocalProcessError::NodeBootstrap);
    }
    let observation_bootstrap_path = arguments.get(4).map(PathBuf::from);
    if observation_bootstrap_path
        .as_ref()
        .is_some_and(|path| !is_lexically_absolute_file(path) || path == &bootstrap_path)
    {
        return Err(LocalProcessError::NodeBootstrap);
    }
    Ok(Some(NodeChildBootstrapPathsV1 {
        bootstrap_path,
        observation_bootstrap_path,
    }))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NodeChildBootstrapPathsV1 {
    bootstrap_path: PathBuf,
    observation_bootstrap_path: Option<PathBuf>,
}

fn is_lexically_absolute_file(path: &Path) -> bool {
    path.is_absolute()
        && path.file_name().is_some()
        && path
            .components()
            .all(|component| !matches!(component, Component::CurDir | Component::ParentDir))
}

fn run_node_daemon_child(paths: &NodeChildBootstrapPathsV1) -> Result<(), LocalProcessError> {
    #[cfg(unix)]
    {
        match &paths.observation_bootstrap_path {
            Some(observation_bootstrap_path) => {
                paraegox_node::process::serve_developer_local_runtime_observation_node_daemon_v1(
                    &paths.bootstrap_path,
                    observation_bootstrap_path,
                )
            }
            None => paraegox_node::process::serve_developer_local_reference_node_daemon_v1(
                &paths.bootstrap_path,
            ),
        }
        .map_err(|_| LocalProcessError::NodeChild)
    }
    #[cfg(not(unix))]
    {
        let _ = paths;
        Err(LocalProcessError::NodeBootstrap)
    }
}

fn compose_real_provisioned_stack(
    config: DeveloperProvisionedConfigV1,
) -> Result<(), LocalProcessError> {
    #[cfg(unix)]
    {
        composition::run_provisioned(config)
    }
    #[cfg(not(unix))]
    {
        let _ = config;
        unreachable!("configuration rejects DeveloperLocal before composition on non-Unix")
    }
}

fn compose_real_local_stack(config: DeveloperFixtureConfigV1) -> Result<(), LocalProcessError> {
    #[cfg(unix)]
    {
        composition::run(config)
    }
    #[cfg(not(unix))]
    {
        let _ = config;
        unreachable!("configuration rejects DeveloperLocal before composition on non-Unix")
    }
}

fn compose_real_distributed_fixture_stack(
    config: DeveloperDistributedFixtureConfigV1,
) -> Result<(), LocalProcessError> {
    #[cfg(unix)]
    {
        composition::run_distributed(config)
    }
    #[cfg(not(unix))]
    {
        let _ = config;
        unreachable!("configuration rejects DeveloperLocal before composition on non-Unix")
    }
}

fn initialize_distributed_identity(
    config: DeveloperDistributedFixtureConfigV1,
) -> Result<(), LocalProcessError> {
    #[cfg(unix)]
    {
        let manifest = identity::initialize_distributed(config.state_root())
            .map_err(|_| LocalProcessError::DistributedIdentityInitialization)?;
        let enrollment =
            identity::distributed_certificate_enrollment_plan_json_v1(&config, &manifest)
                .map_err(|_| LocalProcessError::DistributedEnrollmentPlan)?;
        write_distributed_enrollment_plan(&mut io::stdout().lock(), &enrollment)
    }
    #[cfg(not(unix))]
    {
        let _ = config;
        unreachable!("configuration rejects DeveloperLocal before identity initialization")
    }
}

fn write_distributed_enrollment_plan(
    output: &mut impl Write,
    enrollment: &str,
) -> Result<(), LocalProcessError> {
    writeln!(output, "{enrollment}").map_err(|_| LocalProcessError::DistributedEnrollmentPlan)
}

fn print_usage() {
    println!("{}", usage());
}

fn print_usage_to_stderr() {
    eprintln!("{}", usage());
}

fn usage() -> &'static str {
    r"Usage: paraegox chat --config <absolute-paraegox.toml>
       paraegox init --directory <absolute-directory> --json
       paraegox up --config <absolute-paraegox.toml> --json
       paraegox status --config <absolute-paraegox.toml> --json
       paraegox down --config <absolute-paraegox.toml> --json
       paraegox node --config <absolute-paraegox-node.toml>
       paraegox deployment --config <absolute-paraegox-deployment.toml>
       paraegox version --json
       paraegox config check <chat|node|deployment> --config <absolute-config.toml> --json
       paraegox doctor <chat|node|deployment> --config <absolute-config.toml> --offline --json
       paraegox --help

init creates or strictly reopens one private DeveloperLocal config workspace
and atomically publishes paraegox.toml. It configures but does not create the
state directory. It does not read Secrets, access the network, or start owners.

version, config check, and doctor are machine-readable offline commands. They
emit one JSON object and do not start owners, access the network, resolve Secret
values, or open credential, seed, or private-key files. doctor checks only the
strict configuration, supported platform, non-root execution identity, and
Secret-input-reference presence. It does not establish runtime or network
readiness.

up runs the same configured chat owner chain without starting Textual. status
reports only the authenticated local lifecycle state; it is not an Inspection
or health result. down asks the still-owning supervisor to perform joined
shutdown and reports stopped only after that cleanup completes. These commands
never use a PID as control authority. restart and crash recovery are not part
of this first lifecycle slice.

chat starts the configured ParaEGOX conversation owner chain and Textual console.
The absolute versioned configuration is the sole public input for provider and
model selection, Fabric settings, and durable state location. Secret fields
contain references only; Secret values are obtained from the configured
resolver and injected at the owning boundary. Secret values are neither CLI
inputs nor persisted in the versioned configuration.

node starts one split-trust local Runtime and one NodeDaemon. Node config
schema v1 retains the G1 host-local feature-only profile. Additive schema v2
starts the G2 host-side Runtime-control listener and authenticated Node-control
ingress/observation bridge. Additive schema v3 also installs the exact
deterministic Agent-provider projection without activating an Agent. All
schemas contain verification keys, opaque
references, and credential file paths, never Controller or Authority private
keys. This command does not run Controller, Authority, the managed Fabric
CoreService, Agent, Model, Inspection, or Textual.

deployment consumes one independently SHA-256-pinned enrollment artifact and
starts the single DeploymentController, tenure Authority, Runtime-control
connector, and Node-control connector owner graph. Config schema v1 prints its
original readiness marker only after the durable managed-successor
reconciliation reports ManagedReady. Schema v2 additionally consumes its
enrollment-pinned Agent-provider projection plus config-owned Fabric/Agent
service identities, loopback listener, and fixed limits profile. It prints a
distinct Agent-bootstrap readiness marker only after the complete durable
bootstrap facade reports Ready. This bounded command is not evidence of a
two-host system proof, remote Agent conversation, remote TUI, or reconnect
policy."
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    struct BrokenEnrollmentOutput;

    impl std::io::Write for BrokenEnrollmentOutput {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct FaultingJsonOutput {
        bytes: Vec<u8>,
        successful_byte_budget: usize,
        write_attempts: usize,
    }

    impl FaultingJsonOutput {
        fn with_successful_byte_budget(successful_byte_budget: usize) -> Self {
            Self {
                bytes: Vec::new(),
                successful_byte_budget,
                write_attempts: 0,
            }
        }
    }

    impl std::io::Write for FaultingJsonOutput {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.write_attempts += 1;
            if self.successful_byte_budget == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
            }
            let written = buffer.len().min(self.successful_byte_budget);
            self.bytes.extend_from_slice(&buffer[..written]);
            self.successful_byte_budget -= written;
            Ok(written)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn exact_version_intent() -> OfflineJsonIntentV1 {
        config::offline_json_intent(&[OsString::from("version"), OsString::from("--json")])
            .expect("exact version JSON grammar")
    }

    fn exact_status_intent() -> LocalLifecycleJsonIntentV1 {
        config::lifecycle_json_intent(&[
            OsString::from("status"),
            OsString::from("--config"),
            OsString::from("/private/tmp/paraegox.toml"),
            OsString::from("--json"),
        ])
        .expect("recognized lifecycle JSON command")
    }

    #[test]
    fn init_json_v1_always_has_exact_eight_path_safe_fields() {
        let mut success = Vec::new();
        write_init_json_line(&mut success, true, true, None).expect("init success JSON");
        assert_eq!(success.last(), Some(&b'\n'));
        assert_eq!(success.iter().filter(|byte| **byte == b'\n').count(), 1);
        let success: Value = serde_json::from_slice(&success).expect("init success object");
        assert_eq!(success.as_object().expect("JSON object").len(), 8);
        assert_eq!(success["schema_version"], INIT_OUTPUT_SCHEMA_VERSION);
        assert_eq!(success["command"], "init");
        assert_eq!(success["ok"], true);
        assert_eq!(success["changed"], true);
        #[cfg(unix)]
        {
            assert_eq!(success["profile"], initializer::INIT_PROFILE);
            assert_eq!(
                success["config_relative_path"],
                initializer::INIT_CONFIG_RELATIVE_PATH
            );
            assert_eq!(
                success["state_relative_path"],
                initializer::INIT_STATE_RELATIVE_PATH
            );
        }
        assert_eq!(success["diagnostics"], json!([]));

        let mut failure = Vec::new();
        write_init_json_line(
            &mut failure,
            false,
            false,
            Some(LocalProcessError::InitWorkspaceConflict),
        )
        .expect("init failure JSON");
        let failure: Value = serde_json::from_slice(&failure).expect("init failure object");
        assert_eq!(failure.as_object().expect("JSON object").len(), 8);
        assert_eq!(failure["command"], "init");
        assert_eq!(failure["ok"], false);
        assert_eq!(failure["changed"], false);
        assert_eq!(failure["profile"], Value::Null);
        assert_eq!(failure["config_relative_path"], Value::Null);
        assert_eq!(failure["state_relative_path"], Value::Null);
        assert_eq!(
            failure["diagnostics"],
            json!([{
                "code": LocalProcessError::InitWorkspaceConflict.code(),
                "message": LocalProcessError::InitWorkspaceConflict.message(),
            }])
        );
    }

    #[cfg(unix)]
    #[test]
    fn recognized_init_grammar_failure_stays_on_json_and_exit_two() {
        let mut output = Vec::new();
        let outcome = dispatch_init_to(
            &mut output,
            &[
                OsString::from("init"),
                OsString::from("--directory"),
                OsString::from("relative"),
                OsString::from("--json"),
            ],
        );
        assert_eq!(outcome, DispatchOutcome::ConfigurationFailure);
        assert_eq!(output.last(), Some(&b'\n'));
        let parsed: Value = serde_json::from_slice(&output).expect("init path failure JSON");
        assert_eq!(parsed["ok"], false);
        assert_eq!(parsed["changed"], false);
        assert_eq!(
            parsed["diagnostics"][0]["code"],
            "PXLC-INIT-DIRECTORY-INVALID"
        );
    }

    #[test]
    fn offline_version_is_one_stable_json_object_and_succeeds() {
        let mut output = Vec::new();
        let outcome = dispatch_offline_to(
            &mut output,
            OfflineCommandV1::Version,
            DoctorEnvironmentV1 {
                platform_supported: true,
                execution_identity_non_root: Some(true),
            },
        )
        .expect("version JSON output");

        assert_eq!(outcome, DispatchOutcome::Success);
        assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
        assert_eq!(output.last(), Some(&b'\n'));
        let parsed: Value = serde_json::from_slice(&output).expect("version JSON object");
        assert_eq!(parsed["schema_version"], OFFLINE_OUTPUT_SCHEMA_VERSION);
        assert_eq!(parsed["command"], "version");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(parsed["diagnostics"], json!([]));
        assert_eq!(parsed["checks_performed"], json!([]));
        assert_eq!(parsed["checks_skipped"], json!([]));
    }

    #[cfg(unix)]
    #[test]
    fn hidden_supervisor_contention_has_no_public_lifecycle_exit_code() {
        assert_eq!(
            DispatchOutcome::HiddenSupervisorContended.exit_code(),
            ExitCode::from(lifecycle::LOCAL_CHAT_SUPERVISOR_CONTENTION_EXIT_CODE_V1),
        );
        assert_eq!(
            DispatchOutcome::from_lifecycle_exit_code(
                lifecycle::LOCAL_CHAT_SUPERVISOR_CONTENTION_EXIT_CODE_V1,
            ),
            DispatchOutcome::DiagnosticFailure,
        );

        let source = include_str!("main.rs");
        let dispatch = source
            .split("fn dispatch(")
            .nth(1)
            .and_then(|tail| tail.split("fn current_doctor_environment(").next())
            .expect("bounded main dispatch source");
        let hidden = dispatch
            .split("if let Some(supervisor) = parse_local_chat_supervisor(&arguments)?")
            .nth(1)
            .and_then(|tail| tail.split("parse_node_daemon_child(&arguments)?").next())
            .expect("bounded hidden supervisor dispatch");
        assert!(hidden.contains("LocalChatSupervisorResultV1::Contended"));
        assert!(hidden.contains("DispatchOutcome::HiddenSupervisorContended"));
    }

    #[test]
    fn offline_parser_is_the_first_dispatch_gate_before_children_and_composition() {
        assert!(matches!(
            config::parse_offline(&[OsString::from("version"), OsString::from("--json"),]),
            Ok(Some(OfflineCommandV1::Version))
        ));

        let source = include_str!("main.rs");
        let dispatch = &source[source.find("fn dispatch(").expect("dispatch function")..];
        let offline = dispatch
            .find("config::parse_offline(&arguments)?")
            .expect("offline parser gate");
        let node_child = dispatch
            .find("parse_node_daemon_child(&arguments)?")
            .expect("Node child gate");
        let lifecycle = dispatch
            .find("config::parse_lifecycle(&arguments)?")
            .expect("lifecycle parser gate");
        let supervisor = dispatch
            .find("parse_local_chat_supervisor(&arguments)?")
            .expect("hidden lifecycle supervisor gate");
        let ordinary = dispatch
            .find("match config::parse(arguments)?")
            .expect("ordinary config gate");
        let composition = dispatch
            .find("compose_real_node(*config)")
            .expect("composition dispatch");
        assert!(offline < lifecycle);
        assert!(lifecycle < supervisor);
        assert!(supervisor < node_child);
        assert!(node_child < ordinary);
        assert!(ordinary < composition);
    }

    #[test]
    fn offline_output_failure_gate_precedes_any_error_json_write() {
        let source = include_str!("main.rs");
        let start = source
            .find("fn finish_offline_dispatch_error(")
            .expect("offline dispatch error finisher");
        let remaining = &source[start..];
        let end = remaining.find("\nfn dispatch(").expect("dispatch function");
        let finisher = &remaining[..end];
        let output_failure = finisher
            .find("LocalProcessError::OfflineJsonOutput")
            .expect("output failure short circuit");
        let error_json = finisher
            .find("write_offline_error_json(output, intent, error)")
            .expect("configuration error JSON writer");

        assert!(output_failure < error_json);
        assert!(finisher[output_failure..error_json].contains("return 1;"));
    }

    #[test]
    fn partial_offline_json_write_failure_never_attempts_a_second_json_object() {
        let mut output = FaultingJsonOutput::with_successful_byte_budget(1);
        let error = dispatch_offline_to(
            &mut output,
            OfflineCommandV1::Version,
            DoctorEnvironmentV1 {
                platform_supported: true,
                execution_identity_non_root: Some(true),
            },
        )
        .expect_err("partial JSON write must fail closed");
        assert_eq!(error, LocalProcessError::OfflineJsonOutput);
        assert!(!output.bytes.is_empty());
        let write_attempts_after_failure = output.write_attempts;

        assert_eq!(
            finish_offline_dispatch_error(&mut output, exact_version_intent(), error),
            1
        );
        assert_eq!(output.write_attempts, write_attempts_after_failure);
    }

    #[test]
    fn immediate_offline_json_write_failure_never_attempts_an_error_json_object() {
        let mut output = FaultingJsonOutput::with_successful_byte_budget(0);
        let error = dispatch_offline_to(
            &mut output,
            OfflineCommandV1::Version,
            DoctorEnvironmentV1 {
                platform_supported: true,
                execution_identity_non_root: Some(true),
            },
        )
        .expect_err("immediate JSON write must fail closed");
        assert_eq!(error, LocalProcessError::OfflineJsonOutput);
        assert!(output.bytes.is_empty());
        let write_attempts_after_failure = output.write_attempts;

        assert_eq!(
            finish_offline_dispatch_error(&mut output, exact_version_intent(), error),
            1
        );
        assert_eq!(output.write_attempts, write_attempts_after_failure);
    }

    #[test]
    fn lifecycle_configuration_failure_is_one_exact_path_free_json_object() {
        let mut output = Vec::new();
        let exit_code = finish_lifecycle_dispatch_error(
            &mut output,
            exact_status_intent(),
            LocalProcessError::LifecycleConfiguration,
        );

        assert_eq!(exit_code, 2);
        assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
        let parsed: Value = serde_json::from_slice(&output).expect("lifecycle JSON object");
        let object = parsed.as_object().expect("object");
        assert_eq!(object.len(), 9);
        for key in [
            "changed",
            "command",
            "diagnostics",
            "generation",
            "inspection_checked",
            "ok",
            "owner_readiness_observed",
            "schema_version",
            "state",
        ] {
            assert!(object.contains_key(key));
        }
        assert_eq!(parsed["schema_version"], LIFECYCLE_OUTPUT_SCHEMA_VERSION);
        assert_eq!(parsed["command"], "status");
        assert_eq!(parsed["ok"], false);
        assert_eq!(parsed["state"], "unknown");
        assert_eq!(parsed["generation"], Value::Null);
        assert_eq!(parsed["changed"], false);
        assert_eq!(parsed["owner_readiness_observed"], false);
        assert_eq!(parsed["inspection_checked"], false);
        assert_eq!(
            parsed["diagnostics"][0]["code"],
            "PXLC-LIFECYCLE-CONFIGURATION"
        );
        let text = String::from_utf8(output).expect("UTF-8 lifecycle JSON");
        assert!(!text.contains("private/tmp"));
    }

    #[test]
    fn partial_lifecycle_json_failure_never_attempts_a_second_json_object() {
        let mut output = FaultingJsonOutput::with_successful_byte_budget(1);
        let error = write_lifecycle_json_line(
            &mut output,
            LifecycleJsonLineV1 {
                action: LocalLifecycleActionV1::Status,
                ok: true,
                state: "never_started",
                generation: None,
                changed: false,
                owner_readiness_observed: false,
                diagnostic: None,
            },
        )
        .expect_err("partial lifecycle JSON write must fail closed");
        assert_eq!(error, LocalProcessError::LifecycleJsonOutput);
        let write_attempts_after_failure = output.write_attempts;

        assert_eq!(
            finish_lifecycle_dispatch_error(&mut output, exact_status_intent(), error),
            1
        );
        assert_eq!(output.write_attempts, write_attempts_after_failure);
    }

    #[test]
    fn exact_offline_config_failure_is_one_path_free_json_object() {
        let arguments = [
            OsString::from("config"),
            OsString::from("check"),
            OsString::from("chat"),
            OsString::from("--config"),
            OsString::from("/private/tmp/must-not-appear.toml"),
            OsString::from("--json"),
        ];
        let intent = config::offline_json_intent(&arguments).expect("exact JSON grammar");
        let mut output = Vec::new();
        let exit_code = finish_offline_dispatch_error(
            &mut output,
            intent,
            LocalProcessError::Configuration(config::ConfigError::InvalidConfigPath),
        );

        assert_eq!(exit_code, 2);
        assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
        let text = String::from_utf8(output).expect("UTF-8 JSON");
        assert!(!text.contains("must-not-appear"));
        let parsed: Value = serde_json::from_str(&text).expect("configuration error JSON object");
        assert_eq!(parsed["schema_version"], OFFLINE_OUTPUT_SCHEMA_VERSION);
        assert_eq!(parsed["command"], "config.check");
        assert_eq!(parsed["kind"], "chat");
        assert_eq!(parsed["ok"], false);
        assert_eq!(parsed["config_schema_version"], Value::Null);
        assert_eq!(parsed["diagnostics"][0]["code"], "PXLC-CONFIG-PATH-INVALID");
        assert_eq!(parsed["checks_performed"][0]["ok"], false);
    }

    #[cfg(unix)]
    #[test]
    fn offline_config_and_doctor_emit_safe_bounded_json() {
        let sequence = std::process::id();
        let temporary_root =
            std::fs::canonicalize(std::env::temp_dir()).expect("canonical test temp dir");
        let path = temporary_root.join(format!("paraegox-main-offline-doctor-{sequence}.toml"));
        std::fs::write(
            &path,
            "schema_version = 1\nstate_root = \"/tmp/doctor-secret-path-sentinel\"\nfabric_listen = \"tcp/127.0.0.1:7447\"\n\n[model]\nprovider = \"openai-responses-v1\"\nmodel = \"gpt-test-model\"\nsecret_ref = \"env:OPENAI_API_KEY\"\n",
        )
        .expect("write doctor config");
        let check_arguments = [
            OsString::from("config"),
            OsString::from("check"),
            OsString::from("chat"),
            OsString::from("--config"),
            path.clone().into_os_string(),
            OsString::from("--json"),
        ];
        let check_command = config::parse_offline(&check_arguments)
            .expect("valid offline config check")
            .expect("offline command");
        let arguments = [
            OsString::from("doctor"),
            OsString::from("chat"),
            OsString::from("--config"),
            path.clone().into_os_string(),
            OsString::from("--offline"),
            OsString::from("--json"),
        ];
        let command = config::parse_offline(&arguments)
            .expect("valid offline doctor config")
            .expect("offline command");
        std::fs::remove_file(&path).expect("remove doctor config");

        let mut check_output = Vec::new();
        let check_outcome = dispatch_offline_to(
            &mut check_output,
            check_command,
            DoctorEnvironmentV1 {
                platform_supported: true,
                execution_identity_non_root: Some(false),
            },
        )
        .expect("config-check JSON");
        assert_eq!(check_outcome, DispatchOutcome::Success);
        let check_text = String::from_utf8(check_output).expect("UTF-8 config-check JSON");
        assert!(!check_text.contains("doctor-secret-path-sentinel"));
        assert!(!check_text.contains("OPENAI_API_KEY"));
        let check_parsed: Value =
            serde_json::from_str(&check_text).expect("config-check JSON object");
        assert_eq!(check_parsed["command"], "config.check");
        assert_eq!(check_parsed["kind"], "chat");
        assert_eq!(check_parsed["ok"], true);
        assert_eq!(check_parsed["config_schema_version"], 1);
        assert_eq!(check_parsed["secret_input_reference_present"], true);

        let mut output = Vec::new();
        let outcome = dispatch_offline_to(
            &mut output,
            command,
            DoctorEnvironmentV1 {
                platform_supported: true,
                execution_identity_non_root: Some(false),
            },
        )
        .expect("doctor diagnostic JSON");

        assert_eq!(outcome, DispatchOutcome::DiagnosticFailure);
        assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
        let text = String::from_utf8(output).expect("UTF-8 doctor JSON");
        assert!(!text.contains("doctor-secret-path-sentinel"));
        assert!(!text.contains("OPENAI_API_KEY"));
        let parsed: Value = serde_json::from_str(&text).expect("doctor JSON object");
        assert_eq!(parsed["command"], "doctor.offline");
        assert_eq!(parsed["kind"], "chat");
        assert_eq!(parsed["ok"], false);
        assert_eq!(
            parsed["diagnostics"][0]["code"],
            "PXLC-DOCTOR-EXECUTION-IDENTITY"
        );
        assert!(
            parsed["checks_skipped"]
                .as_array()
                .expect("skipped checks")
                .iter()
                .any(|check| check["id"] == "runtime_readiness")
        );
        assert!(
            parsed["checks_skipped"]
                .as_array()
                .expect("skipped checks")
                .iter()
                .any(|check| check["id"] == "network_connectivity")
        );
    }

    #[test]
    fn chat_rejects_a_non_absolute_config_path_before_composition() {
        let error = dispatch([
            OsString::from("chat"),
            OsString::from("--config"),
            OsString::from("paraegox.toml"),
        ])
        .expect_err("a relative config path must fail before composition");

        assert!(matches!(error, LocalProcessError::Configuration(_)));
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn node_rejects_a_non_absolute_config_path_before_composition() {
        let error = dispatch([
            OsString::from("node"),
            OsString::from("--config"),
            OsString::from("paraegox-node.toml"),
        ])
        .expect_err("a relative node config path must fail before composition");

        assert!(matches!(error, LocalProcessError::Configuration(_)));
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn deployment_rejects_a_non_absolute_config_path_before_composition() {
        let error = dispatch([
            OsString::from("deployment"),
            OsString::from("--config"),
            OsString::from("paraegox-deployment.toml"),
        ])
        .expect_err("a relative Deployment config path must fail before composition");

        assert!(matches!(error, LocalProcessError::Configuration(_)));
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn bare_controller_is_not_a_public_command() {
        let error = dispatch([OsString::from("controller")])
            .expect_err("bare controller must not select Deployment");

        assert_eq!(
            error,
            LocalProcessError::Configuration(config::ConfigError::UnknownMode)
        );
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn in_progress_distributed_fixture_is_not_a_public_command() {
        let error = dispatch([OsString::from("developer-distributed-fixture-v1")])
            .expect_err("the in-progress distributed fixture must not be public");

        assert_eq!(
            error,
            LocalProcessError::Configuration(config::ConfigError::UnknownMode)
        );
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn internal_distributed_fixture_still_validates_before_composition() {
        let error = dispatch([
            OsString::from("__developer-distributed-fixture-v1"),
            OsString::from("--state-root"),
            OsString::from("/tmp/paraegox-local-distributed"),
            OsString::from("--fabric-listen-a"),
            OsString::from("tcp/127.0.0.1:7451"),
        ])
        .expect_err("missing target B locator must fail before composition");

        assert_eq!(
            error,
            LocalProcessError::Configuration(config::ConfigError::MissingFabricListenB)
        );
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn hidden_distributed_identity_init_uses_the_same_complete_configuration_gate() {
        let error = dispatch([
            OsString::from("__developer-distributed-identity-init-v1"),
            OsString::from("--state-root"),
            OsString::from("/tmp/paraegox-local-distributed-init"),
            OsString::from("--fabric-listen-a"),
            OsString::from("tcp/127.0.0.1:7451"),
        ])
        .expect_err("identity init must require the complete distributed configuration");

        assert_eq!(
            error,
            LocalProcessError::Configuration(config::ConfigError::MissingFabricListenB)
        );
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn distributed_enrollment_output_failure_uses_the_stable_error_surface() {
        assert_eq!(
            write_distributed_enrollment_plan(&mut BrokenEnrollmentOutput, "{}"),
            Err(LocalProcessError::DistributedEnrollmentPlan)
        );
    }

    #[test]
    fn operational_distributed_composition_cannot_implicitly_initialize_pxdi() {
        let source = include_str!("composition.rs");
        assert!(source.contains("identity::open_distributed(config.state_root())"));
        assert!(!source.contains("identity::initialize_distributed(config.state_root())"));
    }

    #[test]
    fn help_paths_succeed_without_starting_the_composition() {
        assert_eq!(
            dispatch([OsString::from("--help")]),
            Ok(DispatchOutcome::Success)
        );
        assert_eq!(
            dispatch([OsString::from("-h")]),
            Ok(DispatchOutcome::Success)
        );
    }

    #[test]
    fn usage_exposes_runtime_and_offline_commands_without_internal_modes() {
        let text = usage();
        assert_eq!(
            text.lines().take(11).collect::<Vec<_>>(),
            [
                "Usage: paraegox chat --config <absolute-paraegox.toml>",
                "       paraegox init --directory <absolute-directory> --json",
                "       paraegox up --config <absolute-paraegox.toml> --json",
                "       paraegox status --config <absolute-paraegox.toml> --json",
                "       paraegox down --config <absolute-paraegox.toml> --json",
                "       paraegox node --config <absolute-paraegox-node.toml>",
                "       paraegox deployment --config <absolute-paraegox-deployment.toml>",
                "       paraegox version --json",
                "       paraegox config check <chat|node|deployment> --config <absolute-config.toml> --json",
                "       paraegox doctor <chat|node|deployment> --config <absolute-config.toml> --offline --json",
                "       paraegox --help",
            ]
        );
        assert!(text.contains("paraegox init --directory <absolute-directory> --json"));
        assert!(text.contains("does not create the\nstate directory"));
        assert!(text.contains("paraegox chat --config <absolute-paraegox.toml>"));
        assert!(text.contains("paraegox up --config <absolute-paraegox.toml> --json"));
        assert!(text.contains("paraegox status --config <absolute-paraegox.toml> --json"));
        assert!(text.contains("paraegox down --config <absolute-paraegox.toml> --json"));
        assert!(text.contains("reports only the authenticated local lifecycle state"));
        assert!(text.contains("never use a PID as control authority"));
        assert!(text.contains("restart and crash recovery are not part"));
        assert!(text.contains("paraegox node --config <absolute-paraegox-node.toml>"));
        assert!(text.contains("paraegox deployment --config <absolute-paraegox-deployment.toml>"));
        assert!(text.contains("paraegox version --json"));
        assert!(text.contains(
            "paraegox config check <chat|node|deployment> --config <absolute-config.toml> --json"
        ));
        assert!(text.contains(
            "paraegox doctor <chat|node|deployment> --config <absolute-config.toml> --offline --json"
        ));
        assert!(text.contains("does not establish runtime or network\nreadiness"));
        assert!(text.contains("split-trust local Runtime and one NodeDaemon"));
        assert!(text.contains("schema v1 retains the G1 host-local feature-only profile"));
        assert!(text.contains("schema v2\nstarts the G2 host-side Runtime-control listener"));
        assert!(text.contains("authenticated Node-control\ningress/observation bridge"));
        assert!(text.contains("schema v3 also installs the exact\ndeterministic Agent-provider"));
        assert!(text.contains("without activating an Agent"));
        assert!(text.contains("never Controller or Authority private\nkeys"));
        assert!(text.contains("does not run Controller, Authority"));
        assert!(text.contains("independently SHA-256-pinned enrollment artifact"));
        assert!(text.contains("single DeploymentController"));
        assert!(text.contains("Config schema v1 prints its\noriginal readiness marker only"));
        assert!(text.contains("reports ManagedReady"));
        assert!(
            text.contains("Schema v2 additionally consumes its\nenrollment-pinned Agent-provider")
        );
        assert!(text.contains("config-owned Fabric/Agent\nservice identities"));
        assert!(text.contains("distinct Agent-bootstrap readiness marker"));
        assert!(text.contains("complete durable\nbootstrap facade reports Ready"));
        assert!(text.contains("not evidence of a\ntwo-host system proof"));
        assert!(text.contains("remote Agent conversation, remote TUI"));
        assert!(text.contains("remote TUI"));
        assert!(text.contains("reconnect\npolicy"));
        assert!(text.contains("absolute versioned configuration"));
        assert!(text.contains("provider and\nmodel selection"));
        assert!(text.contains("Fabric settings"));
        assert!(text.contains("durable state location"));
        assert!(text.contains("Secret fields\ncontain references only"));
        assert!(text.contains("configured\nresolver"));
        assert!(text.contains("injected at the owning boundary"));
        assert!(text.contains("neither CLI\ninputs nor persisted"));
        assert!(!text.contains("chat fixture-v1"));
        assert!(!text.contains("chat openai-v1"));
        assert!(!text.contains("chat deepseek-v1"));
        assert!(!text.contains("paraegox controller"));
        assert!(!text.contains("developer-distributed-fixture-v1"));
        assert!(!text.contains("developer-fixture-v1"));
        assert!(!text.contains("developer-openai-v1"));
        assert!(!text.contains("__developer-distributed-fixture-v1"));
        assert!(!text.contains("__developer-distributed-identity-init-v1"));
        assert!(!text.contains("--state-root"));
        assert!(!text.contains("--fabric-listen"));
        assert!(!text.contains("--model"));
        assert!(!text.contains("--fabric-listen-a"));
        assert!(!text.contains("--fabric-listen-b"));
        assert!(!text.contains("--provider"));
        assert!(!text.contains("--api-key"));
        assert!(!text.contains("--endpoint"));
        assert!(!text.contains("--proxy"));
        assert!(!text.contains("--retry"));
        assert!(!text.contains("--nonce"));
        assert!(!text.contains("--identity"));
        assert!(!text.contains("paraegox-console"));
        assert!(!text.contains("--runtime-bootstrap-file"));
        assert!(!text.contains("--inspection-bootstrap-file"));
        assert!(!text.contains(NODE_DAEMON_CHILD_MODE));
        assert!(!text.contains(NODE_BOOTSTRAP_FILE_OPTION));
        assert!(!text.contains(NODE_OBSERVATION_BOOTSTRAP_FILE_OPTION));
    }

    #[test]
    fn hidden_node_child_accepts_reference_or_runtime_observation_bootstraps() {
        let bootstrap = OsString::from("/private/tmp/pxl-test/node/bootstrap/node.pxnb");
        assert_eq!(
            parse_node_daemon_child(&[
                OsString::from(NODE_DAEMON_CHILD_MODE),
                OsString::from(NODE_BOOTSTRAP_FILE_OPTION),
                bootstrap.clone(),
            ]),
            Ok(Some(NodeChildBootstrapPathsV1 {
                bootstrap_path: PathBuf::from(bootstrap),
                observation_bootstrap_path: None,
            }))
        );
        assert_eq!(
            parse_node_daemon_child(&[
                OsString::from(NODE_DAEMON_CHILD_MODE),
                OsString::from(NODE_BOOTSTRAP_FILE_OPTION),
                OsString::from("/private/tmp/pxl-test/node/bootstrap/node.pxnb"),
                OsString::from(NODE_OBSERVATION_BOOTSTRAP_FILE_OPTION),
                OsString::from("/private/tmp/pxl-test/node/bootstrap/observe.pxob"),
            ]),
            Ok(Some(NodeChildBootstrapPathsV1 {
                bootstrap_path: PathBuf::from("/private/tmp/pxl-test/node/bootstrap/node.pxnb"),
                observation_bootstrap_path: Some(PathBuf::from(
                    "/private/tmp/pxl-test/node/bootstrap/observe.pxob",
                )),
            }))
        );
        assert_eq!(
            parse_node_daemon_child(&[
                OsString::from(NODE_DAEMON_CHILD_MODE),
                OsString::from(NODE_BOOTSTRAP_FILE_OPTION),
                OsString::from("relative.pxnb"),
            ]),
            Err(LocalProcessError::NodeBootstrap)
        );
        assert_eq!(
            parse_node_daemon_child(&[
                OsString::from(NODE_DAEMON_CHILD_MODE),
                OsString::from("--wrong-option"),
                OsString::from("/private/tmp/pxl-test/node.pxnb"),
            ]),
            Err(LocalProcessError::NodeBootstrap)
        );
        assert_eq!(
            parse_node_daemon_child(&[
                OsString::from(NODE_DAEMON_CHILD_MODE),
                OsString::from(NODE_BOOTSTRAP_FILE_OPTION),
                OsString::from("/private/tmp/pxl-test/node/bootstrap/node.pxnb"),
                OsString::from(NODE_OBSERVATION_BOOTSTRAP_FILE_OPTION),
                OsString::from("relative.pxob"),
            ]),
            Err(LocalProcessError::NodeBootstrap)
        );
        assert_eq!(
            parse_node_daemon_child(&[
                OsString::from(NODE_DAEMON_CHILD_MODE),
                OsString::from(NODE_BOOTSTRAP_FILE_OPTION),
                OsString::from("/private/tmp/pxl-test/node/bootstrap/node.pxnb"),
                OsString::from(NODE_OBSERVATION_BOOTSTRAP_FILE_OPTION),
                OsString::from("/private/tmp/pxl-test/node/bootstrap/node.pxnb"),
            ]),
            Err(LocalProcessError::NodeBootstrap)
        );
        assert_eq!(parse_node_daemon_child(&[OsString::from("chat")]), Ok(None));
    }
}
