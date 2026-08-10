use crate::config::ConfigError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalProcessError {
    Configuration(ConfigError),
    OfflineJsonOutput,
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
