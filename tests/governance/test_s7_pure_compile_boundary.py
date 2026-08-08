from __future__ import annotations

import re
import tomllib
from collections.abc import Mapping
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]
CRATES_ROOT = REPO_ROOT / "crates"
DEPLOYMENT_ROOT = CRATES_ROOT / "paraegox-deployment"
DEPLOYMENT_SRC = DEPLOYMENT_ROOT / "src"
PURE_COMPILE_SOURCES = (
    DEPLOYMENT_SRC / "deck.rs",
    DEPLOYMENT_SRC / "planner.rs",
)
RESTRICTED_APPLY_OWNER_SOURCE = DEPLOYMENT_SRC / "distributed_agent_stack_apply.rs"
DEPLOYMENT_PROCESS_SOURCE = DEPLOYMENT_SRC / "deployment_process.rs"
RESTRICTED_APPLY_FABRIC_SOURCE = CRATES_ROOT / "paraegox-fabric" / "src" / "runtime_apply.rs"
RESTRICTED_APPLY_FABRIC_LIBRARY = CRATES_ROOT / "paraegox-fabric" / "src" / "lib.rs"
RUNTIME_ROOT = CRATES_ROOT / "paraegox-runtime"
RUNTIME_SRC = RUNTIME_ROOT / "src"
REMOTE_AGENT_D0_SOURCES = (
    RUNTIME_SRC / "remote_agent_outbox.rs",
    RUNTIME_SRC / "remote_agent_one_echo.rs",
)
REMOTE_AGENT_DESCRIPTOR_EVIDENCE_SOURCE = RUNTIME_SRC / "remote_agent_descriptor_evidence.rs"
RUNTIME_STORE_SOURCE = RUNTIME_SRC / "runtime_store.rs"
RUNTIME_CONTROL_ENDPOINT_SOURCE = RUNTIME_SRC / "runtime_control_endpoint.rs"

DEPENDENCY_TABLES = {"dependencies", "dev-dependencies", "build-dependencies"}
FORBIDDEN_RUNTIME_DEPENDENCIES = {"paraegox-deployment", "paraegox-decks"}
FORBIDDEN_GRAPH_NAMES = {
    "graph",
    "graph-foundation",
    "graph_foundation",
    "paraegox-graph",
    "paraegox-graph-foundation",
}
PUBLIC_DEPLOYMENT_PROCESS_SYMBOLS = {
    "DeploymentdProcessError",
    "TenureAuthorityProcessError",
    "run_deploymentd_process",
    "run_tenure_authority_process",
}
PUBLIC_DEVELOPER_LOCAL_SYMBOLS = {
    "DeveloperLocalPeerIdentityV1",
    "DeveloperLocalTenureAuthorityIdentityBytesV1",
    "DeveloperLocalTenureAuthorityConfigV1",
    "DeveloperLocalTenureAuthorityFactsV1",
    "DeveloperLocalTenureAuthorityV1",
    "DeveloperLocalTenureAuthorityError",
    "DeveloperFixtureIdentitySeedV1",
    "DeveloperFixtureDerivedIdentityV1",
    "DeveloperFixturePathsV1",
    "DeveloperFixtureRuntimePinsV1",
    "DeveloperFixtureControllerCredentialsV1",
    "DeveloperFixtureFabricEndpointV1",
    "DeveloperFixtureAgentStackInputV1",
    "DeveloperFixtureAgentStackOutcomeV1",
    "DeveloperProvisionedAgentStackInputV1",
    "DeveloperProvisionedAgentStackOutcomeV1",
    "DeveloperFixtureAgentStackDeactivationOutcomeV1",
    "DeveloperFixtureAgentStackError",
    "run_developer_fixture_agent_stack_v1",
    "run_developer_provisioned_agent_stack_v1",
    "deactivate_developer_fixture_agent_stack_v1",
    "DeveloperFixtureModelAgentStackInputV1",
    "DeveloperFixtureModelAgentStackOutcomeV1",
    "DeveloperProvisionedModelAgentStackInputV1",
    "DeveloperProvisionedModelAgentStackOutcomeV1",
    "DeveloperFixtureModelAgentStackDeactivationOutcomeV1",
    "DeveloperFixtureModelAgentStackError",
    "run_developer_fixture_model_agent_stack_v1",
    "run_developer_provisioned_model_agent_stack_v1",
    "deactivate_developer_fixture_model_agent_stack_v1",
    "deactivate_developer_provisioned_model_agent_stack_v1",
    "DeveloperFixtureDistributedCoordinatorV1",
    "DeveloperFixtureDistributedTransportV1",
    "DeveloperFixtureDistributedTargetV1",
    "DeveloperFixtureDistributedAgentStackInputV1",
    "DeveloperFixtureDistributedNodeV1",
    "PreparedDeveloperFixtureDistributedAgentStackV1",
    "DeveloperFixtureDistributedAgentStackOutcomeV1",
    "DeveloperFixtureDistributedAgentStackError",
    "prepare_developer_fixture_distributed_agent_stack_v1",
    "complete_developer_fixture_distributed_agent_stack_v1",
}
PUBLIC_DEVELOPER_DEPLOYMENT_SYMBOLS = {
    "DeveloperDeploymentEnrollmentFactsFieldsV1",
    "DeveloperDeploymentEnrollmentFactsV1",
    "DeveloperDeploymentStartFieldsV1",
    "DeveloperDeploymentStartInputV1",
    "DeveloperDeploymentStartModeV1",
    "DeveloperDeploymentOwnerV1",
    "DeveloperDeploymentReadyV1",
    "DeveloperDeploymentStartOutcomeV1",
    "DeveloperDeploymentErrorV1",
    "start_developer_deployment_v1",
}
PUBLIC_DEVELOPER_AGENT_BOOTSTRAP_SYMBOLS = {
    "DeveloperDeploymentAgentBootstrapStartFieldsV1",
    "DeveloperDeploymentAgentBootstrapStartInputV1",
    "DeveloperDeploymentAgentBootstrapReadyV1",
    "DeveloperDeploymentAgentBootstrapStartOutcomeV1",
    "start_developer_deployment_agent_bootstrap_v1",
}
DEVELOPER_LOCAL_ENTRYPOINT = (
    "paraegox_deployment::{DeveloperLocalPeerIdentityV1, "
    "DeveloperLocalTenureAuthorityIdentityBytesV1, "
    "DeveloperLocalTenureAuthorityConfigV1, DeveloperLocalTenureAuthorityFactsV1, "
    "DeveloperLocalTenureAuthorityV1, DeveloperLocalTenureAuthorityError, "
    "DeveloperFixtureIdentitySeedV1, DeveloperFixtureDerivedIdentityV1, "
    "DeveloperFixturePathsV1, DeveloperFixtureRuntimePinsV1, "
    "DeveloperFixtureControllerCredentialsV1, DeveloperFixtureFabricEndpointV1, "
    "DeveloperFixtureAgentStackInputV1, DeveloperFixtureAgentStackOutcomeV1, "
    "DeveloperProvisionedAgentStackInputV1, DeveloperProvisionedAgentStackOutcomeV1, "
    "DeveloperFixtureAgentStackDeactivationOutcomeV1, "
    "DeveloperFixtureAgentStackError, run_developer_fixture_agent_stack_v1, "
    "run_developer_provisioned_agent_stack_v1, "
    "deactivate_developer_fixture_agent_stack_v1, "
    "DeveloperFixtureModelAgentStackInputV1, "
    "DeveloperFixtureModelAgentStackOutcomeV1, "
    "DeveloperProvisionedModelAgentStackInputV1, "
    "DeveloperProvisionedModelAgentStackOutcomeV1, "
    "DeveloperFixtureModelAgentStackDeactivationOutcomeV1, "
    "DeveloperFixtureModelAgentStackError, "
    "run_developer_fixture_model_agent_stack_v1, "
    "run_developer_provisioned_model_agent_stack_v1, "
    "deactivate_developer_fixture_model_agent_stack_v1, "
    "deactivate_developer_provisioned_model_agent_stack_v1}"
)
DEVELOPER_DISTRIBUTED_FIXTURE_ENTRYPOINT = (
    "paraegox_deployment::{DeveloperFixtureDistributedCoordinatorV1, "
    "DeveloperFixtureDistributedTransportV1, DeveloperFixtureDistributedTargetV1, "
    "DeveloperFixtureDistributedAgentStackInputV1, DeveloperFixtureDistributedNodeV1, "
    "PreparedDeveloperFixtureDistributedAgentStackV1, "
    "DeveloperFixtureDistributedAgentStackOutcomeV1, "
    "DeveloperFixtureDistributedAgentStackError, "
    "prepare_developer_fixture_distributed_agent_stack_v1, "
    "complete_developer_fixture_distributed_agent_stack_v1}"
)
DEVELOPER_DEPLOYMENT_ENTRYPOINT = (
    "paraegox_deployment::{DeveloperDeploymentEnrollmentFactsFieldsV1, "
    "DeveloperDeploymentEnrollmentFactsV1, DeveloperDeploymentStartFieldsV1, "
    "DeveloperDeploymentStartInputV1, DeveloperDeploymentStartModeV1, "
    "DeveloperDeploymentOwnerV1, DeveloperDeploymentReadyV1, "
    "DeveloperDeploymentStartOutcomeV1, DeveloperDeploymentErrorV1, "
    "start_developer_deployment_v1}"
)
DEVELOPER_AGENT_BOOTSTRAP_ENTRYPOINT = (
    "paraegox_deployment::{DeveloperDeploymentAgentBootstrapStartFieldsV1, "
    "DeveloperDeploymentAgentBootstrapStartInputV1, "
    "DeveloperDeploymentAgentBootstrapReadyV1, "
    "DeveloperDeploymentAgentBootstrapStartOutcomeV1, "
    "start_developer_deployment_agent_bootstrap_v1}"
)


def _read_required(path: Path) -> str:
    assert path.is_file(), f"required S7-C source is missing: {path.relative_to(REPO_ROOT)}"
    return path.read_text(encoding="utf-8")


def _load_toml(path: Path) -> dict[str, Any]:
    return tomllib.loads(_read_required(path))


def _normalized_dependency_names(manifest: Mapping[str, Any]) -> set[str]:
    names: set[str] = set()

    def visit(value: object) -> None:
        if isinstance(value, Mapping):
            for key, nested in value.items():
                if key in DEPENDENCY_TABLES and isinstance(nested, Mapping):
                    for alias, specification in nested.items():
                        names.add(str(alias).replace("_", "-"))
                        if isinstance(specification, Mapping):
                            package = specification.get("package")
                            if isinstance(package, str):
                                names.add(package.replace("_", "-"))
                visit(nested)
        elif isinstance(value, list):
            for nested in value:
                visit(nested)

    visit(manifest)
    return names


def test_deck_and_planner_stay_crate_private() -> None:
    library = _read_required(DEPLOYMENT_SRC / "lib.rs")
    for source_path in PURE_COMPILE_SOURCES:
        _read_required(source_path)
        module = source_path.stem
        assert re.search(rf"(?m)^\s*mod\s+{module}\s*;\s*$", library)

    assert not re.search(
        r"(?m)^\s*pub(?:\s*\([^)]*\))?\s+mod\s+(?:deck|planner)\s*;\s*$",
        library,
    )
    assert not re.search(
        r"\bpub(?:\s*\([^)]*\))?\s+use\s+[^;]*\b(?:deck|planner)\s*::",
        library,
    )


def test_no_graph_foundation_crate_or_generic_module_is_admitted() -> None:
    workspace = _load_toml(REPO_ROOT / "Cargo.toml")
    members = workspace["workspace"]["members"]
    assert isinstance(members, list)

    for member in members:
        member_path = REPO_ROOT / str(member)
        member_name = member_path.name
        package_name = _load_toml(member_path / "Cargo.toml")["package"]["name"]
        assert member_name not in FORBIDDEN_GRAPH_NAMES
        assert package_name not in FORBIDDEN_GRAPH_NAMES
        assert not member_name.startswith("paraegox-graph")
        assert not package_name.startswith("paraegox-graph")

    for path in CRATES_ROOT.rglob("*"):
        relative_parts = path.relative_to(CRATES_ROOT).parts
        assert not any(part in FORBIDDEN_GRAPH_NAMES for part in relative_parts)

    graph_module = re.compile(
        r"(?m)^\s*(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+(?:graph|graph_foundation)\s*;"
    )
    for crate_root in (*CRATES_ROOT.glob("*/src/lib.rs"), *CRATES_ROOT.rglob("mod.rs")):
        assert not graph_module.search(crate_root.read_text(encoding="utf-8"))


def test_runtime_layers_do_not_depend_on_deployment_compile_layers() -> None:
    for crate_name in ("paraegox-runtime", "paraegox-runtime-host"):
        manifest = _load_toml(CRATES_ROOT / crate_name / "Cargo.toml")
        dependencies = _normalized_dependency_names(manifest)
        assert dependencies.isdisjoint(FORBIDDEN_RUNTIME_DEPENDENCIES), (
            f"{crate_name} reverses the Runtime-to-Deployment dependency boundary: "
            f"{sorted(dependencies & FORBIDDEN_RUNTIME_DEPENDENCIES)}"
        )


def test_t2_d0_remote_agent_owner_stays_private_and_fake_only() -> None:
    library = _read_required(RUNTIME_SRC / "lib.rs")
    for source_path in REMOTE_AGENT_D0_SOURCES:
        module = source_path.stem
        source = _read_required(source_path)
        production = source.partition("#[cfg(test)]")[0]
        assert re.search(rf"(?m)^\s*mod\s+{module}\s*;\s*$", library)
        assert not re.search(
            rf"(?m)^\s*pub(?:\s*\([^)]*\))?\s+mod\s+{module}\s*;\s*$",
            library,
        )
        assert not re.search(
            rf"\bpub(?:\s*\([^)]*\))?\s+use\s+[^;]*\b{module}\b",
            library,
        )
        assert not re.search(
            r"(?m)^\s*pub\s+(?:const|enum|fn|mod|static|struct|trait|type|use)\b",
            source,
        )
        for forbidden in (
            "std::fs",
            "tokio::",
            "zenoh::",
            "paraegox_fabric",
            "getrandom::",
            "clap::",
        ):
            assert forbidden not in production

    outbox = _read_required(REMOTE_AGENT_D0_SOURCES[0]).partition("#[cfg(test)]")[0]
    owner = _read_required(REMOTE_AGENT_D0_SOURCES[1])
    owner_production = owner.partition("#[cfg(test)]")[0]
    assert "impl RemoteAgentOutboxCommitV1 for" not in outbox
    assert "impl RemoteAgentOutboxCommitV1 for" not in owner_production
    assert "impl RemoteAgentDescribeSourceV1 for" not in owner_production
    assert "impl RemoteAgentOnceTransportV1 for" not in owner_production
    assert owner.count("#[test]") == 21

    runtime_manifest = _load_toml(RUNTIME_ROOT / "Cargo.toml")
    runtime_dependencies = _normalized_dependency_names(runtime_manifest)
    assert not any(name == "zenoh" or name.startswith("zenoh-") for name in runtime_dependencies)

    governance = _load_toml(REPO_ROOT / "governance.toml")
    runtime_rows = [
        package
        for package in governance["registry"]["packages"]
        if package.get("cargo_package") == "paraegox-runtime"
    ]
    assert len(runtime_rows) == 1
    runtime_row = runtime_rows[0]
    assert {str(path.relative_to(REPO_ROOT)) for path in REMOTE_AGENT_D0_SOURCES}.issubset(
        runtime_row["first_tests"]
    )
    assert "crate-private PXOJ v1" in runtime_row["responsibility"]
    assert (
        "commit trait and deterministic in-memory fake" in runtime_row["current_capability_limit"]
    )
    for entrypoint in runtime_row["public_entrypoints"]:
        assert "RemoteAgent" not in entrypoint
        assert "remote_agent" not in entrypoint
        assert "PXOJ" not in entrypoint

    for api in governance["registry"]["public_apis"]:
        if str(api["module"]).replace("-", "_") == "paraegox_runtime":
            symbols = {str(symbol) for symbol in api["symbols"]}
            assert not any("RemoteAgent" in symbol or "PXOJ" in symbol for symbol in symbols)


def test_t2_c0_descriptor_evidence_stays_private_and_documents_exact_limits() -> None:
    library = _read_required(RUNTIME_SRC / "lib.rs")
    evidence = _read_required(REMOTE_AGENT_DESCRIPTOR_EVIDENCE_SOURCE)
    store = _read_required(RUNTIME_STORE_SOURCE)
    endpoint = _read_required(RUNTIME_CONTROL_ENDPOINT_SOURCE)
    module = REMOTE_AGENT_DESCRIPTOR_EVIDENCE_SOURCE.stem

    assert re.search(rf"(?m)^\s*mod\s+{module}\s*;\s*$", library)
    assert not re.search(
        rf"(?m)^\s*pub(?:\s*\([^)]*\))?\s+mod\s+{module}\s*;\s*$",
        library,
    )
    assert not re.search(
        rf"\bpub(?:\s*\([^)]*\))?\s+use\s+[^;]*\b{module}\b",
        library,
    )
    assert not re.search(
        r"(?m)^\s*pub\s+(?:const|enum|fn|mod|static|struct|trait|type|use)\b",
        evidence,
    )
    assert "single-slot owner-private ledger" in evidence
    assert "not an access grant or a PXRA" in evidence
    assert "paraegox_runtime_contracts::remote_agent_access" not in evidence

    assert 'const LOCK_FILE_NAME: &str = "runtime.lock";' in store
    assert '"remote-agent-descriptor-evidence-v1"' in store
    assert '".remote-agent-descriptor-evidence-v1.tmp-"' in store
    assert "remote-agent-descriptor-evidence-v1.lock" not in store

    handler = endpoint.split(
        "async fn handle_authenticated_runtime_agent_control_request_v1", 1
    )[1].split("/// Revalidates the latest durable Describe record", 1)[0]
    commit_position = handler.index(".commit_remote_agent_descriptor_evidence(evidence)")
    reverify_position = handler.index(
        ".latest_verified_remote_agent_descriptor_evidence_v1(request.carrier())"
    )
    reply_position = handler.index("return Ok(response_wire)")
    assert commit_position < reverify_position < reply_position

    reverify = endpoint.split(
        "pub(crate) async fn latest_verified_remote_agent_descriptor_evidence_v1", 1
    )[1].split("async fn handle_authenticated_runtime_control_carrier_v1", 1)[0]
    assert "take_remote_agent_descriptor_post_commit_reverify_failure_for_test" in reverify
    assert "verify_remote_agent_descriptor_evidence_v1(" in reverify

    governance = _load_toml(REPO_ROOT / "governance.toml")
    runtime_rows = [
        package
        for package in governance["registry"]["packages"]
        if package.get("cargo_package") == "paraegox-runtime"
    ]
    assert len(runtime_rows) == 1
    runtime_row = runtime_rows[0]
    assert {
        str(REMOTE_AGENT_DESCRIPTOR_EVIDENCE_SOURCE.relative_to(REPO_ROOT)),
        str(RUNTIME_STORE_SOURCE.relative_to(REPO_ROOT)),
        str(RUNTIME_CONTROL_ENDPOINT_SOURCE.relative_to(REPO_ROOT)),
    }.issubset(runtime_row["first_tests"])
    for claim in (
        "crate-private, bounded PXDE v1 latest-slot ledger",
        "same `runtime.lock`",
        "byte-exact authenticated PXAG and PXAH",
        "commit-before-reply",
        "post-commit live reverification",
    ):
        assert claim in runtime_row["responsibility"]
    for limit in (
        "replacement continuity inside the current slot",
        "neither historical authenticity nor anti-rollback",
        "downgrade compatibility is not guaranteed",
        "no idempotent no-write guarantee",
        "not process-abort or power-cut certification",
        "not a PXRA dispatcher",
        "not a descriptor or access capability",
        "not proof of deployed remote-Agent reachability",
    ):
        assert limit in runtime_row["current_capability_limit"]

    for entrypoint in runtime_row["public_entrypoints"]:
        assert "RemoteAgentDescriptorEvidence" not in entrypoint
        assert "PXDE" not in entrypoint
    for api in governance["registry"]["public_apis"]:
        if str(api["module"]).replace("-", "_") == "paraegox_runtime":
            symbols = {str(symbol) for symbol in api["symbols"]}
            assert not any(
                "RemoteAgentDescriptorEvidence" in symbol or "PXDE" in symbol
                for symbol in symbols
            )

    readme = " ".join(_read_required(REPO_ROOT / "README.md").split())
    for phrase in (
        "neither historical authenticity nor anti-rollback",
        "downgrade compatibility is not guaranteed",
        "no idempotent no-write guarantee",
        "not process-abort or power-cut certification",
    ):
        assert phrase in readme
    readme_zh = " ".join(_read_required(REPO_ROOT / "README_zh.md").split())
    for phrase in (
        "不证明历史真实性或 anti-rollback",
        "不保证 downgrade compatibility",
        "不保证 idempotent no-write",
        "不构成 process-abort 或 power-cut 认证",
    ):
        assert phrase in readme_zh


def test_restricted_runtime_apply_send_stays_in_controller_owner_allowlist() -> None:
    allowed_preflight_sources = {
        RESTRICTED_APPLY_OWNER_SOURCE,
        RESTRICTED_APPLY_FABRIC_SOURCE,
        RESTRICTED_APPLY_FABRIC_LIBRARY,
        DEPLOYMENT_PROCESS_SOURCE,
    }
    send_call_counts: dict[Path, int] = {}
    raw_string = re.compile(r'(?s)(?:br|r)(?P<hashes>#+)".*?"(?P=hashes)')
    quoted_string = re.compile(r'(?s)b?"(?:\\.|[^"\\])*"')
    send_call = re.compile(
        r"(?:\.\s*send_once\s*\(|"
        r"\bRestrictedRuntimeApplyPreflightV1\s*::\s*send_once\s*\()"
    )
    for path in CRATES_ROOT.rglob("*.rs"):
        source = path.read_text(encoding="utf-8")
        if "RestrictedRuntimeApplyPreflightV1" in source:
            assert path in allowed_preflight_sources, (
                "restricted Runtime apply preflight escaped its Fabric mechanism and "
                f"Deployment owner: {path.relative_to(REPO_ROOT)}"
            )
        source_without_strings = quoted_string.sub('""', raw_string.sub('""', source))
        count = len(send_call.findall(source_without_strings))
        if count:
            send_call_counts[path] = count

    assert send_call_counts == {
        RESTRICTED_APPLY_OWNER_SOURCE: 2,
        RESTRICTED_APPLY_FABRIC_SOURCE: 2,
        DEPLOYMENT_PROCESS_SOURCE: 9,
    }, (
        "restricted physical send calls must remain limited to the two exact Controller-owner "
        "sources plus Fabric's move-only compile-fail example"
    )


def test_pure_compile_sources_do_not_reimplement_manifest_or_side_effects() -> None:
    forbidden_literals = (
        "PXCM",
        "paraegox.runtime.artifact-compatibility-manifest.sha256.v1",
        "decode_compatibility_manifest",
        "decode_compatibility_projection",
        "build_manifest_wire",
        "build_projection_wire",
        "append_manifest_target_row",
        "decode_manifest_target_row",
    )
    forbidden_patterns = {
        "manifest type definition": re.compile(
            r"\b(?:struct|enum|union|type|trait)\s+"
            r"RuntimeArtifactCompatibilityManifestV1\b"
        ),
        "manifest codec implementation": re.compile(
            r"\bimpl(?:\s*<[^>]*>)?\s+RuntimeArtifactCompatibilityManifestV1\b"
        ),
        "manifest framing constant": re.compile(
            r"\bconst\s+(?:COMPATIBILITY_)?MANIFEST_"
            r"(?:MAGIC|DIGEST_DOMAIN|VERSION|BYTES)\b"
        ),
        "filesystem/network/process/thread access": re.compile(
            r"\bstd\s*::\s*(?:fs|net|process|thread)\s*::"
        ),
        "side-effect module import": re.compile(
            r"(?m)^\s*use\s+std\s*::\s*(?:fs|net|process|thread)\s*;"
        ),
        "grouped side-effect import": re.compile(
            r"(?m)^\s*use\s+std\s*::\s*\{[^}\n]*\b(?:fs|net|process|thread)\b"
        ),
        "async runtime access": re.compile(r"\b(?:tokio|async_std)\s*::"),
        "async function": re.compile(r"(?m)^\s*(?:pub(?:\s*\([^)]*\))?\s+)?async\s+fn\b"),
        "await point": re.compile(r"\.await\b"),
        "mutable static": re.compile(
            r"(?m)^\s*(?:pub(?:\s*\([^)]*\))?\s+)?static\s+mut\s+[A-Za-z_]\w*"
        ),
        "interior-mutable static": re.compile(
            r"(?m)^\s*(?:pub(?:\s*\([^)]*\))?\s+)?static\s+[A-Za-z_]\w*\s*:"
            r"[^=;\n]*\b(?:Atomic\w*|Mutex|OnceLock|RwLock)\b"
        ),
        "thread-local state": re.compile(r"\bthread_local\s*!"),
    }

    for path in PURE_COMPILE_SOURCES:
        source = _read_required(path)
        for literal in forbidden_literals:
            assert literal not in source, (
                f"{path.relative_to(REPO_ROOT)} duplicates manifest authority: {literal}"
            )
        for mechanism, pattern in forbidden_patterns.items():
            assert not pattern.search(source), (
                f"{path.relative_to(REPO_ROOT)} violates pure-compile boundary: {mechanism}"
            )


def test_s7_c_pure_compile_types_remain_private_behind_exact_process_facades() -> None:
    governance = _load_toml(REPO_ROOT / "governance.toml")
    registry = governance["registry"]

    deployment_rows = [
        package
        for package in registry["packages"]
        if package.get("cargo_package") == "paraegox-deployment"
    ]
    assert len(deployment_rows) == 1
    deployment_row = deployment_rows[0]
    assert deployment_row["status"] == "experimental"
    assert deployment_row["public_entrypoints"] == [
        "paraegox_deployment::run_tenure_authority_process",
        "paraegox_deployment::run_deploymentd_process",
        (
            "paraegox-deploymentd initialize-reference-v1/commit-reference-loop-v1/"
            "commit-reference-empty-v1/acquire-tenure-v1/bootstrap-runtime-v1/"
            "apply-reference-v1/reconcile-reference-once-v1/"
            "migrate-controller-journal-v7-to-v8-v1/"
            "initialize-distributed-agent-stack-v1/"
            "observe-distributed-agent-stack-nodes-once-v1 CLI"
        ),
        DEVELOPER_LOCAL_ENTRYPOINT,
        DEVELOPER_DISTRIBUTED_FIXTURE_ENTRYPOINT,
        DEVELOPER_DEPLOYMENT_ENTRYPOINT,
        DEVELOPER_AGENT_BOOTSTRAP_ENTRYPOINT,
    ]
    assert deployment_row["consumers"] == [
        "paraegox-tenure-authority",
        "paraegox-deploymentd",
        "paraegox-local",
    ]

    deployment_api_symbol_groups = []
    for api in registry["public_apis"]:
        module = str(api["module"]).replace("-", "_")
        symbols = {str(symbol) for symbol in api["symbols"]}
        if module == "paraegox_deployment":
            deployment_api_symbol_groups.append(frozenset(symbols))
            continue
        assert not module.startswith(("paraegox_deployment", "paraegox_decks"))
    assert set(deployment_api_symbol_groups) == {
        frozenset(PUBLIC_DEPLOYMENT_PROCESS_SYMBOLS),
        frozenset(PUBLIC_DEVELOPER_LOCAL_SYMBOLS),
        frozenset(PUBLIC_DEVELOPER_DEPLOYMENT_SYMBOLS),
        frozenset(PUBLIC_DEVELOPER_AGENT_BOOTSTRAP_SYMBOLS),
    }

    deployment_manifest = _load_toml(DEPLOYMENT_ROOT / "Cargo.toml")
    assert "bin" not in deployment_manifest
    assert not (DEPLOYMENT_SRC / "main.rs").exists()
    executable_sources = sorted(path.name for path in (DEPLOYMENT_SRC / "bin").glob("*.rs"))
    assert executable_sources == [
        "paraegox-deploymentd.rs",
        "paraegox-tenure-authority.rs",
    ]
