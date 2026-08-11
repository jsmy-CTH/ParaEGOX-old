use crate::config::ConfigError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalProcessError {
    Configuration(ConfigError),
    OfflineJsonOutput,
    ArtifactPath,
    ArtifactCompatibility,
    ArtifactConflict,
    ArtifactCapacity,
    ArtifactNotFound,
    ArtifactMaterializationFailed,
    ArtifactUncertain,
    ArtifactOwner,
    ArtifactIo,
    ArtifactJsonOutput,
    InitUnsafeExecutionIdentity,
    InitWorkspaceConflict,
    InitIo,
    InitPublicationUncertain,
    InitJsonOutput,
    LifecycleConfiguration,
    LifecycleState,
    LifecycleControl,
    LifecycleUnavailable,
    LifecycleStartup,
    LifecycleShutdown,
    LifecycleReconcileRequired,
    LifecycleJsonOutput,
    LocalDeployLifecycle,
    LocalDeployQuery,
    LocalDeployEvidence,
    LocalDeployJsonOutput,
    LocalInspectionNotRunning,
    LocalInspectionLocator,
    LocalInspectionBootstrap,
    LocalInspectionPeer,
    LocalInspectionProtocol,
    LocalInspectionNotFound,
    LocalInspectionIo,
    LocalInspectionJsonOutput,
    LocalReceiptNotRunning,
    LocalReceiptLocator,
    LocalReceiptBootstrap,
    LocalReceiptPeer,
    LocalReceiptProtocol,
    LocalReceiptNotFound,
    LocalReceiptIo,
    LocalReceiptJsonOutput,
    LocalTuiNotRunning,
    LocalTuiTerminal,
    LocalTuiLocator,
    LocalTuiHandoff,
    LocalTuiBootstrap,
    LocalTuiPeer,
    LocalTuiProtocol,
    LocalTuiIo,
    LocalTuiChild,
    UnsafeExecutionIdentity,
    SignalHandling,
    IdentityManifest,
    LayoutPreparation,
    IdentityDerivation,
    ProviderSecret,
    ProviderConfiguration,
    AuthorityStartup,
    RuntimeStartup,
    NodeBootstrap,
    NodeCredentialFiles,
    NodeStartup,
    DeploymentPreparation,
    DeploymentStartup,
    DeploymentReconcileRequired,
    DeploymentOwnerExit,
    DeploymentReadyOutput,
    DeploymentJoinedShutdown,
    DeploymentActivation,
    ConversationConfiguration,
    ConversationCapability,
    ConversationIpc,
    InspectionIpc,
    ConversationChild,
    NodeChild,
    JoinedShutdown,
    DistributedIdentityInitialization,
    DistributedIdentityManifest,
    DistributedEnrollmentPlan,
    DistributedLayoutPreparation,
    DistributedAuthorityStartup,
    DistributedRuntimeAStartup,
    DistributedRuntimeBStartup,
    DistributedNodeAStartup,
    DistributedNodeBStartup,
    DistributedDeploymentActivation,
    DistributedJoinedShutdown,
}

impl LocalProcessError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Configuration(error) => error.code(),
            Self::OfflineJsonOutput => "PXLC-OFFLINE-JSON-OUTPUT",
            Self::ArtifactPath => "PXLC-ARTIFACT-PATH",
            Self::ArtifactCompatibility => "PXLC-ARTIFACT-COMPATIBILITY",
            Self::ArtifactConflict => "PXLC-ARTIFACT-CONFLICT",
            Self::ArtifactCapacity => "PXLC-ARTIFACT-CAPACITY",
            Self::ArtifactNotFound => "PXLC-ARTIFACT-NOT-FOUND",
            Self::ArtifactMaterializationFailed => "PXLC-ARTIFACT-MATERIALIZATION-FAILED",
            Self::ArtifactUncertain => "PXLC-ARTIFACT-UNCERTAIN",
            Self::ArtifactOwner => "PXLC-ARTIFACT-OWNER",
            Self::ArtifactIo => "PXLC-ARTIFACT-IO",
            Self::ArtifactJsonOutput => "PXLC-ARTIFACT-JSON-OUTPUT",
            Self::InitUnsafeExecutionIdentity => "PXLC-INIT-EXECUTION-IDENTITY",
            Self::InitWorkspaceConflict => "PXLC-INIT-WORKSPACE-CONFLICT",
            Self::InitIo => "PXLC-INIT-IO",
            Self::InitPublicationUncertain => "PXLC-INIT-PUBLICATION-UNCERTAIN",
            Self::InitJsonOutput => "PXLC-INIT-JSON-OUTPUT",
            Self::LifecycleConfiguration => "PXLC-LIFECYCLE-CONFIGURATION",
            Self::LifecycleState => "PXLC-LIFECYCLE-STATE",
            Self::LifecycleControl => "PXLC-LIFECYCLE-CONTROL",
            Self::LifecycleUnavailable => "PXLC-LIFECYCLE-UNAVAILABLE",
            Self::LifecycleStartup => "PXLC-LIFECYCLE-STARTUP",
            Self::LifecycleShutdown => "PXLC-LIFECYCLE-SHUTDOWN",
            Self::LifecycleReconcileRequired => "PXLC-LIFECYCLE-RECONCILE-REQUIRED",
            Self::LifecycleJsonOutput => "PXLC-LIFECYCLE-JSON-OUTPUT",
            Self::LocalDeployLifecycle => "PXLC-DEPLOY-LIFECYCLE",
            Self::LocalDeployQuery => "PXLC-DEPLOY-QUERY",
            Self::LocalDeployEvidence => "PXLC-DEPLOY-EVIDENCE",
            Self::LocalDeployJsonOutput => "PXLC-DEPLOY-JSON-OUTPUT",
            Self::LocalInspectionNotRunning => "PXLC-INSPECTION-NOT-RUNNING",
            Self::LocalInspectionLocator => "PXLC-INSPECTION-LOCATOR",
            Self::LocalInspectionBootstrap => "PXLC-INSPECTION-BOOTSTRAP",
            Self::LocalInspectionPeer => "PXLC-INSPECTION-PEER",
            Self::LocalInspectionProtocol => "PXLC-INSPECTION-PROTOCOL",
            Self::LocalInspectionNotFound => "PXLC-INSPECTION-NOT-FOUND",
            Self::LocalInspectionIo => "PXLC-INSPECTION-IO",
            Self::LocalInspectionJsonOutput => "PXLC-INSPECTION-JSON-OUTPUT",
            Self::LocalReceiptNotRunning => "PXLC-RECEIPT-NOT-RUNNING",
            Self::LocalReceiptLocator => "PXLC-RECEIPT-LOCATOR",
            Self::LocalReceiptBootstrap => "PXLC-RECEIPT-BOOTSTRAP",
            Self::LocalReceiptPeer => "PXLC-RECEIPT-PEER",
            Self::LocalReceiptProtocol => "PXLC-RECEIPT-PROTOCOL",
            Self::LocalReceiptNotFound => "PXLC-RECEIPT-NOT-FOUND",
            Self::LocalReceiptIo => "PXLC-RECEIPT-IO",
            Self::LocalReceiptJsonOutput => "PXLC-RECEIPT-JSON-OUTPUT",
            Self::LocalTuiNotRunning => "PXLC-TUI-NOT-RUNNING",
            Self::LocalTuiTerminal => "PXLC-TUI-TERMINAL",
            Self::LocalTuiLocator => "PXLC-TUI-LOCATOR",
            Self::LocalTuiHandoff => "PXLC-TUI-HANDOFF",
            Self::LocalTuiBootstrap => "PXLC-TUI-BOOTSTRAP",
            Self::LocalTuiPeer => "PXLC-TUI-PEER",
            Self::LocalTuiProtocol => "PXLC-TUI-PROTOCOL",
            Self::LocalTuiIo => "PXLC-TUI-IO",
            Self::LocalTuiChild => "PXLC-TUI-CHILD",
            Self::UnsafeExecutionIdentity => "PXLC-EXECUTION-IDENTITY",
            Self::SignalHandling => "PXLC-SIGNAL-HANDLING",
            Self::IdentityManifest => "PXLC-IDENTITY-MANIFEST",
            Self::LayoutPreparation => "PXLC-LAYOUT-PREPARATION",
            Self::IdentityDerivation => "PXLC-IDENTITY-DERIVATION",
            Self::ProviderSecret => "PXLC-PROVIDER-SECRET",
            Self::ProviderConfiguration => "PXLC-PROVIDER-CONFIGURATION",
            Self::AuthorityStartup => "PXLC-AUTHORITY-STARTUP",
            Self::RuntimeStartup => "PXLC-RUNTIME-STARTUP",
            Self::NodeBootstrap => "PXLC-NODE-BOOTSTRAP",
            Self::NodeCredentialFiles => "PXLC-NODE-CREDENTIAL-FILES",
            Self::NodeStartup => "PXLC-NODE-STARTUP",
            Self::DeploymentPreparation => "PXLC-DEPLOYMENT-PREPARATION",
            Self::DeploymentStartup => "PXLC-DEPLOYMENT-STARTUP",
            Self::DeploymentReconcileRequired => "PXLC-DEPLOYMENT-RECONCILE-REQUIRED",
            Self::DeploymentOwnerExit => "PXLC-DEPLOYMENT-OWNER-EXIT",
            Self::DeploymentReadyOutput => "PXLC-DEPLOYMENT-READY-OUTPUT",
            Self::DeploymentJoinedShutdown => "PXLC-DEPLOYMENT-JOINED-SHUTDOWN",
            Self::DeploymentActivation => "PXLC-DEPLOYMENT-ACTIVATION",
            Self::ConversationConfiguration => "PXLC-CONVERSATION-CONFIGURATION",
            Self::ConversationCapability => "PXLC-CONVERSATION-CAPABILITY",
            Self::ConversationIpc => "PXLC-CONVERSATION-IPC",
            Self::InspectionIpc => "PXLC-INSPECTION-IPC",
            Self::ConversationChild => "PXLC-CONVERSATION-CHILD",
            Self::NodeChild => "PXLC-NODE-CHILD",
            Self::JoinedShutdown => "PXLC-JOINED-SHUTDOWN",
            Self::DistributedIdentityInitialization => "PXLC-DISTRIBUTED-IDENTITY-INITIALIZATION",
            Self::DistributedIdentityManifest => "PXLC-DISTRIBUTED-IDENTITY-MANIFEST",
            Self::DistributedEnrollmentPlan => "PXLC-DISTRIBUTED-ENROLLMENT-PLAN",
            Self::DistributedLayoutPreparation => "PXLC-DISTRIBUTED-LAYOUT-PREPARATION",
            Self::DistributedAuthorityStartup => "PXLC-DISTRIBUTED-AUTHORITY-STARTUP",
            Self::DistributedRuntimeAStartup => "PXLC-DISTRIBUTED-RUNTIME-A-STARTUP",
            Self::DistributedRuntimeBStartup => "PXLC-DISTRIBUTED-RUNTIME-B-STARTUP",
            Self::DistributedNodeAStartup => "PXLC-DISTRIBUTED-NODE-A-STARTUP",
            Self::DistributedNodeBStartup => "PXLC-DISTRIBUTED-NODE-B-STARTUP",
            Self::DistributedDeploymentActivation => "PXLC-DISTRIBUTED-DEPLOYMENT-ACTIVATION",
            Self::DistributedJoinedShutdown => "PXLC-DISTRIBUTED-JOINED-SHUTDOWN",
        }
    }

    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Configuration(error) => error.message(),
            Self::OfflineJsonOutput => "offline machine-readable output failed",
            Self::ArtifactPath => "artifact path is invalid or unsafe",
            Self::ArtifactCompatibility => "artifact bytes are invalid or incompatible",
            Self::ArtifactConflict => "artifact operation conflicts with its durable request",
            Self::ArtifactCapacity => "artifact store capacity is exhausted",
            Self::ArtifactNotFound => "artifact operation was not found",
            Self::ArtifactMaterializationFailed => "artifact materialization failed",
            Self::ArtifactUncertain => "artifact operation outcome is uncertain",
            Self::ArtifactOwner => "artifact store owner state failed strict validation",
            Self::ArtifactIo => "artifact operation could not complete",
            Self::ArtifactJsonOutput => "artifact JSON output could not be written",
            Self::InitUnsafeExecutionIdentity => "init requires a non-root Unix user and group",
            Self::InitWorkspaceConflict => {
                "init workspace or configuration conflicts with the strict private layout"
            }
            Self::InitIo => "init workspace I/O failed",
            Self::InitPublicationUncertain => {
                "init configuration publication is uncertain and requires inspection"
            }
            Self::InitJsonOutput => "init machine-readable output failed",
            Self::LifecycleConfiguration => {
                "managed-local lifecycle configuration authority changed"
            }
            Self::LifecycleState => "managed-local lifecycle state failed strict validation",
            Self::LifecycleControl => "managed-local lifecycle control exchange failed closed",
            Self::LifecycleUnavailable => "managed-local lifecycle owner is unavailable",
            Self::LifecycleStartup => "managed-local owner composition failed to become ready",
            Self::LifecycleShutdown => {
                "managed-local owner composition did not complete joined shutdown"
            }
            Self::LifecycleReconcileRequired => {
                "managed-local lifecycle authority is uncertain and requires explicit recovery"
            }
            Self::LifecycleJsonOutput => "managed-local lifecycle JSON output failed",
            Self::LocalDeployLifecycle => {
                "compiled local deployment did not reach the running owner generation"
            }
            Self::LocalDeployQuery => "compiled local deployment projection query failed closed",
            Self::LocalDeployEvidence => {
                "compiled local deployment terminal evidence failed strict validation"
            }
            Self::LocalDeployJsonOutput => "compiled local deployment JSON output failed",
            Self::LocalInspectionNotRunning => {
                "local Inspection requires the current owner generation to be running"
            }
            Self::LocalInspectionLocator => "local Inspection owner locator query failed closed",
            Self::LocalInspectionBootstrap => "local Inspection bootstrap failed strict validation",
            Self::LocalInspectionPeer => {
                "local Inspection endpoint identity failed strict validation"
            }
            Self::LocalInspectionProtocol => {
                "local Inspection response failed strict protocol validation"
            }
            Self::LocalInspectionNotFound => {
                "local Inspection projection is not available for this generation"
            }
            Self::LocalInspectionIo => "local Inspection one-shot exchange failed closed",
            Self::LocalInspectionJsonOutput => "local Inspection machine-readable output failed",
            Self::LocalReceiptNotRunning => {
                "local Receipt snapshot requires the current owner generation to be running"
            }
            Self::LocalReceiptLocator => "local Receipt owner locator query failed closed",
            Self::LocalReceiptBootstrap => "local Receipt bootstrap failed strict validation",
            Self::LocalReceiptPeer => "local Receipt endpoint identity failed strict validation",
            Self::LocalReceiptProtocol => {
                "local Receipt response failed strict protocol validation"
            }
            Self::LocalReceiptNotFound => {
                "local Receipt is retiring and unavailable for this generation"
            }
            Self::LocalReceiptIo => "local Receipt one-shot exchange failed closed",
            Self::LocalReceiptJsonOutput => "local Receipt machine-readable output failed",
            Self::LocalTuiNotRunning => {
                "local TUI requires the current owner generation to be running"
            }
            Self::LocalTuiTerminal => "local TUI terminal state failed closed",
            Self::LocalTuiLocator => "local TUI atomic owner locator query failed closed",
            Self::LocalTuiHandoff => "local TUI child handoff failed strict validation",
            Self::LocalTuiBootstrap => "local TUI bootstrap failed strict validation",
            Self::LocalTuiPeer => "local TUI endpoint identity failed strict validation",
            Self::LocalTuiProtocol => "local TUI response failed strict protocol validation",
            Self::LocalTuiIo => "local TUI bounded exchange failed closed",
            Self::LocalTuiChild => "local TUI presentation child failed joined execution",
            Self::UnsafeExecutionIdentity => {
                "DeveloperLocal commands require a non-root user and group"
            }
            Self::SignalHandling => "DeveloperLocal process signal handling failed closed",
            Self::IdentityManifest => "DeveloperLocal identity manifest failed closed",
            Self::LayoutPreparation => "DeveloperLocal filesystem layout failed closed",
            Self::IdentityDerivation => "DeveloperLocal identity derivation failed closed",
            Self::ProviderSecret => "provisioned model provider Secret resolution failed closed",
            Self::ProviderConfiguration => "provisioned model provider configuration failed closed",
            Self::AuthorityStartup => "DeveloperLocal tenure Authority failed to start",
            Self::RuntimeStartup => "DeveloperLocal Runtime failed to start",
            Self::NodeBootstrap => "DeveloperLocal Node registration bootstrap failed closed",
            Self::NodeCredentialFiles => "DeveloperLocal Node TLS credential files failed closed",
            Self::NodeStartup => "DeveloperLocal NodeDaemon failed to start",
            Self::DeploymentPreparation => {
                "DeveloperLocal DeploymentController inputs or owner layout failed closed"
            }
            Self::DeploymentStartup => {
                "DeveloperLocal DeploymentController owner graph failed to start"
            }
            Self::DeploymentReconcileRequired => {
                "DeveloperLocal DeploymentController requires explicit reconciliation before readiness"
            }
            Self::DeploymentOwnerExit => {
                "DeveloperLocal DeploymentController owner exited before process shutdown"
            }
            Self::DeploymentReadyOutput => {
                "DeveloperLocal DeploymentController readiness output failed"
            }
            Self::DeploymentJoinedShutdown => {
                "DeveloperLocal DeploymentController owners did not complete joined shutdown"
            }
            Self::DeploymentActivation => {
                "DeploymentController failed to activate the Fabric and Agent stack"
            }
            Self::ConversationConfiguration => "local conversation configuration is invalid",
            Self::ConversationCapability => {
                "Runtime refused the committed Agent conversation capability"
            }
            Self::ConversationIpc => "Runtime-backed local conversation IPC failed closed",
            Self::InspectionIpc => "node-local read-only Inspection IPC failed closed",
            Self::ConversationChild => {
                "local Textual console child failed to complete joined execution"
            }
            Self::NodeChild => "DeveloperLocal NodeDaemon child process failed",
            Self::JoinedShutdown => "DeveloperLocal owners did not complete joined shutdown",
            Self::DistributedIdentityInitialization => {
                "distributed DeveloperLocal identity initialization failed closed"
            }
            Self::DistributedIdentityManifest => {
                "distributed DeveloperLocal identity manifest failed closed"
            }
            Self::DistributedEnrollmentPlan => {
                "distributed DeveloperLocal certificate enrollment plan failed closed"
            }
            Self::DistributedLayoutPreparation => {
                "distributed DeveloperLocal filesystem layout failed closed"
            }
            Self::DistributedAuthorityStartup => {
                "distributed DeveloperLocal tenure Authority failed to start"
            }
            Self::DistributedRuntimeAStartup => {
                "distributed DeveloperLocal Runtime A failed to start"
            }
            Self::DistributedRuntimeBStartup => {
                "distributed DeveloperLocal Runtime B failed to start"
            }
            Self::DistributedNodeAStartup => {
                "distributed DeveloperLocal logical Node A failed to start"
            }
            Self::DistributedNodeBStartup => {
                "distributed DeveloperLocal logical Node B failed to start"
            }
            Self::DistributedDeploymentActivation => {
                "distributed DeploymentController failed to activate both target stacks"
            }
            Self::DistributedJoinedShutdown => {
                "distributed DeveloperLocal owners did not complete joined shutdown"
            }
        }
    }

    pub(crate) const fn exit_code(self) -> u8 {
        match self {
            Self::Configuration(_)
            | Self::ArtifactPath
            | Self::ArtifactCompatibility
            | Self::ArtifactConflict
            | Self::ArtifactCapacity
            | Self::InitUnsafeExecutionIdentity
            | Self::InitWorkspaceConflict
            | Self::LifecycleConfiguration => 2,
            _ => 1,
        }
    }
}

impl From<ConfigError> for LocalProcessError {
    fn from(error: ConfigError) -> Self {
        Self::Configuration(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_and_integration_failures_have_distinct_exit_codes() {
        let configuration = LocalProcessError::Configuration(ConfigError::MissingMode);
        assert_eq!(configuration.exit_code(), 2);
        assert_eq!(configuration.code(), "PXLC-MODE-MISSING");

        let integration = LocalProcessError::DeploymentActivation;
        assert_eq!(integration.exit_code(), 1);
        assert_eq!(integration.code(), "PXLC-DEPLOYMENT-ACTIVATION");
        assert!(!integration.message().contains("ready"));

        let output = LocalProcessError::OfflineJsonOutput;
        assert_eq!(output.exit_code(), 1);
        assert_eq!(output.code(), "PXLC-OFFLINE-JSON-OUTPUT");
        assert_eq!(output.message(), "offline machine-readable output failed");

        let lifecycle_config = LocalProcessError::LifecycleConfiguration;
        assert_eq!(lifecycle_config.exit_code(), 2);
        assert_eq!(lifecycle_config.code(), "PXLC-LIFECYCLE-CONFIGURATION");

        let lifecycle_output = LocalProcessError::LifecycleJsonOutput;
        assert_eq!(lifecycle_output.exit_code(), 1);
        assert_eq!(lifecycle_output.code(), "PXLC-LIFECYCLE-JSON-OUTPUT");

        for failure in [
            LocalProcessError::LocalDeployLifecycle,
            LocalProcessError::LocalDeployQuery,
            LocalProcessError::LocalDeployEvidence,
            LocalProcessError::LocalDeployJsonOutput,
        ] {
            assert_eq!(failure.exit_code(), 1);
            assert!(failure.code().starts_with("PXLC-DEPLOY-"));
            assert!(!failure.message().is_empty());
        }

        for (failure, code) in [
            (
                LocalProcessError::LocalInspectionNotRunning,
                "PXLC-INSPECTION-NOT-RUNNING",
            ),
            (
                LocalProcessError::LocalInspectionLocator,
                "PXLC-INSPECTION-LOCATOR",
            ),
            (
                LocalProcessError::LocalInspectionBootstrap,
                "PXLC-INSPECTION-BOOTSTRAP",
            ),
            (
                LocalProcessError::LocalInspectionPeer,
                "PXLC-INSPECTION-PEER",
            ),
            (
                LocalProcessError::LocalInspectionProtocol,
                "PXLC-INSPECTION-PROTOCOL",
            ),
            (
                LocalProcessError::LocalInspectionNotFound,
                "PXLC-INSPECTION-NOT-FOUND",
            ),
            (LocalProcessError::LocalInspectionIo, "PXLC-INSPECTION-IO"),
            (
                LocalProcessError::LocalInspectionJsonOutput,
                "PXLC-INSPECTION-JSON-OUTPUT",
            ),
        ] {
            assert_eq!(failure.exit_code(), 1);
            assert_eq!(failure.code(), code);
            assert!(!failure.message().is_empty());
            assert!(!failure.message().contains('/'));
        }

        for (failure, code) in [
            (
                LocalProcessError::LocalReceiptNotRunning,
                "PXLC-RECEIPT-NOT-RUNNING",
            ),
            (
                LocalProcessError::LocalReceiptLocator,
                "PXLC-RECEIPT-LOCATOR",
            ),
            (
                LocalProcessError::LocalReceiptBootstrap,
                "PXLC-RECEIPT-BOOTSTRAP",
            ),
            (LocalProcessError::LocalReceiptPeer, "PXLC-RECEIPT-PEER"),
            (
                LocalProcessError::LocalReceiptProtocol,
                "PXLC-RECEIPT-PROTOCOL",
            ),
            (
                LocalProcessError::LocalReceiptNotFound,
                "PXLC-RECEIPT-NOT-FOUND",
            ),
            (LocalProcessError::LocalReceiptIo, "PXLC-RECEIPT-IO"),
            (
                LocalProcessError::LocalReceiptJsonOutput,
                "PXLC-RECEIPT-JSON-OUTPUT",
            ),
        ] {
            assert_eq!(failure.exit_code(), 1);
            assert_eq!(failure.code(), code);
            assert!(!failure.message().is_empty());
            assert!(!failure.message().contains('/'));
        }

        for (failure, code) in [
            (
                LocalProcessError::LocalTuiNotRunning,
                "PXLC-TUI-NOT-RUNNING",
            ),
            (LocalProcessError::LocalTuiTerminal, "PXLC-TUI-TERMINAL"),
            (LocalProcessError::LocalTuiLocator, "PXLC-TUI-LOCATOR"),
            (LocalProcessError::LocalTuiHandoff, "PXLC-TUI-HANDOFF"),
            (LocalProcessError::LocalTuiBootstrap, "PXLC-TUI-BOOTSTRAP"),
            (LocalProcessError::LocalTuiPeer, "PXLC-TUI-PEER"),
            (LocalProcessError::LocalTuiProtocol, "PXLC-TUI-PROTOCOL"),
            (LocalProcessError::LocalTuiIo, "PXLC-TUI-IO"),
            (LocalProcessError::LocalTuiChild, "PXLC-TUI-CHILD"),
        ] {
            assert_eq!(failure.exit_code(), 1);
            assert_eq!(failure.code(), code);
            assert!(!failure.message().is_empty());
            assert!(!failure.message().contains('/'));
        }

        for failure in [
            ConfigError::InvalidLocalDeployGrammar,
            ConfigError::UnsupportedLocalDeployProfile,
            ConfigError::InvalidInspectionSnapshotGrammar,
            ConfigError::InvalidReceiptSnapshotGrammar,
            ConfigError::InvalidTuiGrammar,
        ] {
            let failure = LocalProcessError::Configuration(failure);
            assert_eq!(failure.exit_code(), 2);
            assert!(
                failure.code().starts_with("PXLC-DEPLOY-")
                    || failure.code() == "PXLC-INSPECTION-GRAMMAR"
                    || failure.code() == "PXLC-RECEIPT-GRAMMAR"
                    || failure.code() == "PXLC-TUI-GRAMMAR"
            );
        }

        let init_conflict = LocalProcessError::InitWorkspaceConflict;
        assert_eq!(init_conflict.exit_code(), 2);
        assert_eq!(init_conflict.code(), "PXLC-INIT-WORKSPACE-CONFLICT");

        for failure in [
            LocalProcessError::InitIo,
            LocalProcessError::InitPublicationUncertain,
            LocalProcessError::InitJsonOutput,
        ] {
            assert_eq!(failure.exit_code(), 1);
            assert!(failure.code().starts_with("PXLC-INIT-"));
        }
    }

    #[test]
    fn distributed_owner_stages_have_distinct_stable_error_codes() {
        let stages = [
            LocalProcessError::DistributedIdentityInitialization,
            LocalProcessError::DistributedIdentityManifest,
            LocalProcessError::DistributedEnrollmentPlan,
            LocalProcessError::DistributedLayoutPreparation,
            LocalProcessError::DistributedAuthorityStartup,
            LocalProcessError::DistributedRuntimeAStartup,
            LocalProcessError::DistributedRuntimeBStartup,
            LocalProcessError::DistributedNodeAStartup,
            LocalProcessError::DistributedNodeBStartup,
            LocalProcessError::DistributedDeploymentActivation,
            LocalProcessError::DistributedJoinedShutdown,
        ];
        let mut codes = std::collections::BTreeSet::new();
        for stage in stages {
            assert!(stage.code().starts_with("PXLC-DISTRIBUTED-"));
            assert!(codes.insert(stage.code()), "duplicate stage error code");
            assert!(!stage.message().is_empty());
            assert_eq!(stage.exit_code(), 1);
        }
    }

    #[test]
    fn public_deployment_lifecycle_failures_are_distinct_and_never_claim_ready() {
        let stages = [
            LocalProcessError::DeploymentPreparation,
            LocalProcessError::DeploymentStartup,
            LocalProcessError::DeploymentReconcileRequired,
            LocalProcessError::DeploymentOwnerExit,
            LocalProcessError::DeploymentReadyOutput,
            LocalProcessError::DeploymentJoinedShutdown,
        ];
        let mut codes = std::collections::BTreeSet::new();
        for stage in stages {
            assert!(stage.code().starts_with("PXLC-DEPLOYMENT-"));
            assert!(
                codes.insert(stage.code()),
                "duplicate Deployment stage code"
            );
            assert_eq!(stage.exit_code(), 1);
        }
        assert!(
            !LocalProcessError::DeploymentReconcileRequired
                .message()
                .contains("is ready")
        );
        assert!(
            !LocalProcessError::DeploymentOwnerExit
                .message()
                .contains("is ready")
        );
    }
}
