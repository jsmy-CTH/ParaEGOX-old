from __future__ import annotations

import hashlib
import importlib.util
import json
import struct
from pathlib import Path
from types import ModuleType
from typing import Any

from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
    Ed25519PublicKey,
)

ROOT = Path(__file__).resolve().parents[2]
WIRE = ROOT / "tests" / "fixtures" / "wire"
LEDGER = json.loads((WIRE / "artifact_f0_semantic_ledger_v1.json").read_text())
AGENT_ORACLE_PATH = ROOT / "tests" / "contract" / "test_s7_managed_agent_stack_successor.py"

PXTA_ZERO = bytes.fromhex("50585441000100000000")
MODEL_PROJECTION_BYTES = 270
MODEL_PXTE_FIXED_BYTES = 286
MAX_MODEL_PXTE_BYTES = 1_994
MODEL_COMPATIBILITY_DOMAIN = (
    b"paraegox.runtime.compiled-managed-model-agent-stack-compatibility.sha256.v1"
)
MODEL_EXECUTION_DIGEST_DOMAIN = b"paraegox.runtime.target-execution.sha256.v8"
MODEL_ASSIGNMENT_DIGEST_DOMAIN = b"paraegox.runtime.target-plan-assignments.sha256.v9"
ARTIFACT_COMPATIBILITY_DOMAIN = (
    b"paraegox.runtime.compiled-artifact-bound-managed-model-agent-stack-compatibility.sha256.v1"
)
ARTIFACT_EXECUTION_DIGEST_DOMAIN = b"paraegox.runtime.target-execution.sha256.v11"
ARTIFACT_ASSIGNMENT_DIGEST_DOMAIN = b"paraegox.runtime.target-plan-assignments.sha256.v12"
ARTIFACT_BINDING_DIGEST_DOMAIN = b"paraegox.runtime.artifact-execution-binding.sha256.v1"
ARTIFACT_SOURCE_PLAN_DIGEST_DOMAIN = (
    b"paraegox.deployment.artifact-bound-managed-model-agent-stack-desired.sha256.v1"
)
ARTIFACT_CUTOVER_MARKER_DIGEST_DOMAIN = (
    b"paraegox.deployment.artifact-external-cutover-marker.sha256.v1"
)
PLAN_CONTENT_DIGEST_DOMAIN = b"paraegox.deployment.plan-content.sha256.v2"
LOCAL_CONTROL_CHANNEL_BINDING_DIGEST_DOMAIN = (
    b"paraegox.runtime.local-control-channel-binding.sha256.v1"
)
TERMINAL_RESULT_REF_DOMAIN = (
    b"paraegox.runtime.managed-model-agent-stack-terminal-result.sha256.v1"
)
TERMINAL_RECEIPT_SIGNING_MAGIC = (
    b"ParaEGOX\0managed-model-agent-stack-terminal-signing"
)
TERMINAL_RECEIPT_DIGEST_DOMAIN = (
    b"paraegox.runtime.managed-model-agent-stack-terminal-receipt.sha256.v1"
)
STACK_RESOURCE_CENSUS_DIGEST_DOMAIN = (
    b"paraegox.runtime.managed-model-agent-stack-resource-census.sha256.v1"
)
STACK_RAW_OUTCOME_DIGEST_DOMAIN = (
    b"paraegox.runtime.managed-model-agent-stack-raw-outcome.sha256.v1"
)
STACK_QUARANTINE_DIGEST_DOMAIN = (
    b"paraegox.runtime.managed-model-agent-stack-quarantine.sha256.v1"
)


def _load_agent_oracle() -> ModuleType:
    spec = importlib.util.spec_from_file_location(
        "_paraegox_artifact_f0_agent_oracle", AGENT_ORACLE_PATH
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


AGENT = _load_agent_oracle()


def _fixture(name: str, size: int) -> bytes:
    text = (WIRE / name).read_text()
    assert text.endswith("\n")
    assert text.count("\n") == 1
    assert text[:-1] == text[:-1].lower()
    frame = bytes.fromhex(text)
    assert len(frame) == size
    return frame


def _raw_digest(domain: bytes, body: bytes) -> bytes:
    return hashlib.sha256(domain + body).digest()


def _canonical_digest(domain: bytes, *fields: bytes) -> bytes:
    digest = hashlib.sha256()
    digest.update(b"ParaEGOX\0canonical-digest")
    digest.update(struct.pack(">H", 1))
    digest.update(struct.pack(">I", len(domain)))
    digest.update(domain)
    for ordinal, field in enumerate(fields, 1):
        digest.update(b"\x01")
        digest.update(struct.pack(">I", ordinal))
        digest.update(struct.pack(">Q", len(field)))
        digest.update(field)
    digest.update(b"\xff")
    digest.update(struct.pack(">I", len(fields)))
    return digest.digest()


def _execution_profile_commitment() -> bytes:
    fields = (
        (b"developer-local-echo-prefix-v1", 30),
        (b"managed_model_data_v1", 21),
        (b"bounded-text-model-data-v1", 26),
        (b"developer-local-managed-model-v1", 32),
        (b"literal-prefix-v1", 17),
    )
    body = b"".join(struct.pack(">H", length) + value for value, length in fields)
    body += struct.pack(">III", 16_384, 32_768, 0)
    return _raw_digest(b"paraegox.artifact.execution-profile.sha256.v1", body)


def _model_compatibility_digest() -> bytes:
    return _canonical_digest(
        MODEL_COMPATIBILITY_DOMAIN,
        b"PXMM",
        struct.pack(">H", 1),
        struct.pack(">H", MODEL_PROJECTION_BYTES),
        b"PXAR",
        struct.pack(">H", 9),
        b"PXTE",
        struct.pack(">H", 8),
        struct.pack(">I", MAX_MODEL_PXTE_BYTES),
        struct.pack(">H", 1),
        struct.pack(">H", 1),
        struct.pack(">H", 1),
        struct.pack(">H", 1),
        struct.pack(">H", 256),
        b"px-bounded-text1",
        MODEL_EXECUTION_DIGEST_DOMAIN,
        struct.pack(">H", 1),
        struct.pack(">H", 2),
        struct.pack(">H", 1),
        struct.pack(">H", 2),
        struct.pack(">H", 1),
        struct.pack(">H", 1),
        b"PXMT",
        struct.pack(">H", 1),
        struct.pack(">H", 1),
        struct.pack(">H", 2_048),
        struct.pack(">H", 512),
        b"paraegox.runtime.managed-model-agent-stack-terminal-result.sha256.v1",
        b"ParaEGOX\0managed-model-agent-stack-terminal-signing",
        b"paraegox.runtime.managed-model-agent-stack-terminal-receipt.sha256.v1",
        MODEL_ASSIGNMENT_DIGEST_DOMAIN,
        PXTA_ZERO,
    )


def _artifact_compatibility_digest() -> bytes:
    return _canonical_digest(
        ARTIFACT_COMPATIBILITY_DOMAIN,
        _model_compatibility_digest(),
        b"PXTE",
        struct.pack(">H", 11),
        struct.pack(">I", 2_506),
        b"PXAR",
        struct.pack(">H", 12),
        struct.pack(">I", 6_630),
        struct.pack(">H", 1),
        struct.pack(">H", 1),
        struct.pack(">I", 192),
        b"px-art-prefix-v1",
        struct.pack(">I", 1),
        b"px-bounded-text1",
        _execution_profile_commitment(),
        ARTIFACT_EXECUTION_DIGEST_DOMAIN,
        ARTIFACT_ASSIGNMENT_DIGEST_DOMAIN,
        ARTIFACT_BINDING_DIGEST_DOMAIN,
        b"PXMT",
        struct.pack(">H", 1),
        PXTA_ZERO,
    )


def _ledger_agent_plan() -> dict[str, Any]:
    agent = LEDGER["agent_plan"]
    return {
        "service_id_hex": agent["service_id_hex"],
        "prepare_budget_nanos": agent["prepare_budget_nanos"],
        "start_budget_nanos": agent["start_budget_nanos"],
        "readiness_budget_nanos": agent["readiness_budget_nanos"],
        "drain_budget_nanos": agent["drain_budget_nanos"],
        "stop_budget_nanos": agent["stop_budget_nanos"],
        "max_sessions": agent["max_sessions"],
        "max_turns_per_session": agent["max_turns_per_session"],
        "max_requests_per_session": agent["max_requests_per_session"],
        "max_event_batch": agent["max_event_batch"],
        "submit_binding_id_hex": agent["submit_binding_id_hex"],
        "control_binding_id_hex": agent["control_binding_id_hex"],
        "submit_key_expression": agent["submit_key_expression"],
        "control_key_expression": agent["control_key_expression"],
        "max_items": agent["max_items"],
        "max_bytes": agent["max_bytes"],
        "max_frame_bytes": agent["max_frame_bytes"],
        "max_response_body_bytes": agent["max_response_body_bytes"],
        "handler_timeout_nanos": agent["handler_timeout_nanos"],
        "provider_profile": agent["provider_profile"],
        "provider_ref_hex": agent["provider_ref_hex"],
        "config_digest_hex": agent["provider_config_digest_hex"],
        "secret_ref_hex": "00" * 16,
    }


def _encode_provider(plan: dict[str, Any]) -> bytes:
    secret = bytes(16)
    return (
        struct.pack(">HBB", 1, plan["provider_profile"], 0)
        + bytes.fromhex(plan["provider_ref_hex"])
        + bytes.fromhex(plan["provider_config_digest_hex"])
        + bytes([int(plan["provider_secret_present"])])
        + secret
    )


def _encode_model_plan() -> bytes:
    model = LEDGER["model_plan"]
    budgets = (
        model["prepare_budget_nanos"],
        model["start_budget_nanos"],
        model["readiness_budget_nanos"],
        model["drain_budget_nanos"],
        model["stop_budget_nanos"],
    )
    wire = bytearray(struct.pack(">H", 1) + bytes.fromhex(model["service_id_hex"]))
    wire += b"".join(struct.pack(">Q", value) for value in budgets)
    wire += struct.pack(">H", model["max_in_flight"])
    wire += _encode_provider(LEDGER["agent_plan"])
    wire += struct.pack(">H", 1)
    wire += model["artifact_adapter_id"].encode("ascii")
    wire += struct.pack(">I", model["adapter_version"])
    wire += model["capability_id"].encode("ascii")
    assert len(wire) == 167
    return bytes(wire)


def _encode_dependency(kind: int, provider: bytes, consumer: bytes) -> bytes:
    frame = struct.pack(">HBBBB", 1, kind, 1, 1, 0) + provider + consumer
    assert len(frame) == 38
    return frame


def _encode_artifact_execution_stack() -> dict[str, bytes]:
    fabric = AGENT.FABRIC._build_vectors()
    base_projection = fabric["projection"]
    agent_projection = AGENT._encode_projection(base_projection)
    agent_pxte = AGENT._encode_pxte(
        agent_projection,
        AGENT.MODE_FABRIC_AND_AGENT,
        fabric["one"]["pxte"],
        _ledger_agent_plan(),
    )
    model_projection = (
        b"PXMM"
        + struct.pack(">H", 1)
        + agent_projection
        + _model_compatibility_digest()
        + struct.pack(">HH", 9, 1)
    )
    assert len(model_projection) == MODEL_PROJECTION_BYTES
    model = LEDGER["model_plan"]
    agent = LEDGER["agent_plan"]
    predecessor = LEDGER["predecessor"]
    model_pxte = bytearray(b"PXTE" + struct.pack(">H", 8) + model_projection)
    model_pxte += struct.pack(">HBBBBI", 1, 1, 1, 2, 0, len(agent_pxte))
    model_pxte += agent_pxte + _encode_model_plan()
    model_pxte += _encode_dependency(
        1,
        bytes.fromhex(predecessor["fabric_service_id_hex"]),
        bytes.fromhex(agent["service_id_hex"]),
    )
    model_pxte += _encode_dependency(
        2,
        bytes.fromhex(model["service_id_hex"]),
        bytes.fromhex(agent["service_id_hex"]),
    )
    binding = _fixture("artifact_f0_binding_v1.hex", 192)
    artifact_pxte = bytearray(b"PXTE" + struct.pack(">H", 11) + model_projection)
    artifact_pxte += _artifact_compatibility_digest()
    artifact_pxte += struct.pack(">HHII", 1, 1, len(binding), len(model_pxte))
    artifact_pxte += binding + model_pxte
    plan_content = bytearray(b"ParaEGOX\0deployment-plan-content")
    plan_content += struct.pack(">HBBI", 2, 3, 0, 252 + len(artifact_pxte))
    plan_content += bytes.fromhex(predecessor["target_hex"])
    plan_content += binding + struct.pack(">I", len(artifact_pxte)) + artifact_pxte
    return {
        "agent_pxte": agent_pxte,
        "model_projection": model_projection,
        "model_pxte": bytes(model_pxte),
        "artifact_pxte": bytes(artifact_pxte),
        "plan_content": bytes(plan_content),
    }


def _encode_artifact_runtime_request() -> dict[str, Any]:
    execution = _encode_artifact_execution_stack()
    pxte = execution["artifact_pxte"]
    plan_content = execution["plan_content"]
    pxdq = _fixture("artifact_f0_pxdq_v1.hex", 288)
    pxdk = _fixture("artifact_f0_pxdk_v1.hex", 240)
    predecessor = LEDGER["predecessor"]
    deployment = LEDGER["deployment"]
    runtime = LEDGER["runtime_apply"]
    authority = LEDGER["authority"]
    signing = LEDGER["signing"]
    legacy = AGENT.FABRIC.LEGACY

    cutover_marker = _canonical_digest(
        ARTIFACT_CUTOVER_MARKER_DIGEST_DOMAIN,
        pxdk[16:48],
        struct.pack(">Q", deployment["admission_sequence"]),
        pxdq[256:288],
        pxdk[208:240],
    )
    plan_content_digest = _canonical_digest(PLAN_CONTENT_DIGEST_DOMAIN, plan_content)
    source_revision = predecessor["legacy_successor_revision"]
    target = bytes.fromhex(predecessor["target_hex"])
    scope = bytes.fromhex(authority["source_scope_hex"])
    source_plan = bytes.fromhex(authority["source_plan_ref_hex"])
    predecessor_slice = bytes.fromhex(predecessor["active_target_slice_digest_hex"])
    source_plan_digest = _canonical_digest(
        ARTIFACT_SOURCE_PLAN_DIGEST_DOMAIN,
        cutover_marker,
        target,
        scope,
        source_plan,
        struct.pack(">Q", source_revision),
        predecessor_slice,
        pxdq[256:288],
        pxdk[208:240],
        plan_content_digest,
        pxte,
    )
    pxta_digest = _canonical_digest(
        b"paraegox.runtime.target-assignments.sha256.v1", PXTA_ZERO
    )
    pxte_digest = _canonical_digest(ARTIFACT_EXECUTION_DIGEST_DOMAIN, pxte)
    assignment_digest = _canonical_digest(
        ARTIFACT_ASSIGNMENT_DIGEST_DOMAIN, pxta_digest, pxte_digest
    )

    writer = bytes.fromhex(authority["writer_hex"])
    writer_epoch = struct.pack(">Q", authority["writer_epoch"])
    tenure_authority = bytes.fromhex(authority["tenure_authority_hex"])
    tenure_key = bytes.fromhex(authority["tenure_key_ref_hex"])
    tenure_algorithm = struct.pack(">H", authority["tenure_algorithm"])
    tenure_algorithm_version = struct.pack(
        ">H", authority["tenure_algorithm_version"]
    )
    supersedes = struct.pack(">Q", authority["tenure_supersedes_through_epoch"])
    tenure_nonce = authority["tenure_nonce_utf8"].encode("utf-8")
    tenure_fields = [
        (1, tenure_authority),
        (2, tenure_key),
        (3, tenure_algorithm),
        (4, tenure_algorithm_version),
        (5, scope),
        (6, writer),
        (7, writer_epoch),
        (8, supersedes),
        (9, tenure_nonce),
    ]
    tenure_keypair = Ed25519PrivateKey.from_private_bytes(
        bytes.fromhex(signing["tenure_seed_hex"])
    )
    tenure_signature = tenure_keypair.sign(
        legacy._signing_transcript(1, legacy.TENURE_SIGNING_DOMAIN, tenure_fields)
    )
    tenure_digest = _canonical_digest(
        legacy.TENURE_PROOF_DIGEST_DOMAIN,
        tenure_authority,
        tenure_key,
        tenure_algorithm,
        tenure_algorithm_version,
        scope,
        writer,
        writer_epoch,
        supersedes,
        tenure_nonce,
        tenure_signature,
    )
    operation_id = bytes.fromhex(runtime["artifact_operation_id_hex"])
    expected_tag = struct.pack(">H", 1)
    control_digest = _canonical_digest(
        legacy.APPLY_CONTROL_DIGEST_DOMAIN,
        _canonical_digest(
            legacy.TARGET_SLICE_DIGEST_DOMAIN,
            struct.pack(">H", 1),
            target,
            scope,
            source_plan,
            struct.pack(">Q", source_revision),
            source_plan_digest,
            assignment_digest,
        ),
        struct.pack(">H", 1),
        target,
        scope,
        source_plan,
        struct.pack(">Q", source_revision),
        source_plan_digest,
        assignment_digest,
        writer,
        writer_epoch,
        tenure_digest,
        expected_tag,
        predecessor_slice,
        operation_id,
    )
    target_slice_digest = _canonical_digest(
        legacy.TARGET_SLICE_DIGEST_DOMAIN,
        struct.pack(">H", 1),
        target,
        scope,
        source_plan,
        struct.pack(">Q", source_revision),
        source_plan_digest,
        assignment_digest,
    )
    unsigned_fields = [
        (1, struct.pack(">H", 1)),
        (2, target),
        (3, scope),
        (4, source_plan),
        (5, struct.pack(">Q", source_revision)),
        (6, source_plan_digest),
        (7, assignment_digest),
        (8, target_slice_digest),
        (9, writer),
        (10, writer_epoch),
        (11, tenure_authority),
        (12, tenure_key),
        (13, tenure_algorithm),
        (14, tenure_algorithm_version),
        (15, scope),
        (16, writer),
        (17, writer_epoch),
        (18, supersedes),
        (19, tenure_nonce),
        (20, tenure_signature),
        (21, tenure_digest),
        (22, expected_tag),
        (23, predecessor_slice),
        (24, operation_id),
        (25, control_digest),
        (26, struct.pack(">H", 1)),
        (27, bytes.fromhex(runtime["artifact_temporal_constraint_id_hex"])),
        (28, bytes.fromhex(runtime["clock_domain_hex"])),
        (29, struct.pack(">Q", runtime["clock_generation"])),
        (30, struct.pack(">Q", runtime["original_budget_nanos"])),
        (31, struct.pack(">Q", runtime["remaining_budget_nanos"])),
        (32, bytes.fromhex(runtime["runtime_store_instance_hex"])),
        (33, bytes.fromhex(authority["request_principal_hex"])),
        (34, bytes.fromhex(authority["request_key_ref_hex"])),
        (35, struct.pack(">H", authority["request_algorithm"])),
        (36, struct.pack(">H", authority["request_algorithm_version"])),
        (37, bytes.fromhex(runtime["artifact_authentication_nonce_hex"])),
    ]
    request_keypair = Ed25519PrivateKey.from_private_bytes(
        bytes.fromhex(signing["controller_seed_hex"])
    )
    request_signature = request_keypair.sign(
        legacy._signing_transcript(2, legacy.AUTH_SIGNING_DOMAIN, unsigned_fields)
    )
    envelope = legacy._encode_envelope_fields([*unsigned_fields, (38, request_signature)])
    values = legacy._decode_envelope(envelope)
    legacy._verify_envelope_signatures(
        values,
        bytes.fromhex(signing["tenure_public_key_hex"]),
        bytes.fromhex(signing["controller_public_key_hex"]),
    )
    outer = (
        b"PXAR"
        + struct.pack(">HIII", 12, len(envelope), len(PXTA_ZERO), len(pxte))
        + envelope
        + PXTA_ZERO
        + pxte
    )
    return {
        "wire": outer,
        "envelope": envelope,
        "envelope_values": values,
        "runtime_slice": PXTA_ZERO + pxte,
        "cutover_marker_digest": cutover_marker,
        "plan_content_digest": plan_content_digest,
        "source_plan_digest": source_plan_digest,
        "assignment_digest": assignment_digest,
        "target_slice_digest": target_slice_digest,
        "request_digest": _canonical_digest(legacy.REQUEST_DIGEST_DOMAIN, envelope),
    }


def _terminal_generation(value: int | None) -> bytes:
    return bytes([value is not None]) + struct.pack(">Q", value or 0)


def _resource_census_digest(variant: dict[str, Any]) -> bytes:
    return _canonical_digest(
        STACK_RESOURCE_CENSUS_DIGEST_DOMAIN,
        struct.pack(">H", variant["physical_binding_census"]),
        struct.pack(">H", int(variant["census_complete"])),
        struct.pack(">H", int(variant["fabric_ready"])),
        struct.pack(">H", int(variant["model_ready"])),
        struct.pack(">H", int(variant["agent_ready"])),
        struct.pack(
            ">H", int(variant["fabric_to_agent_dependency_ready"])
        ),
        struct.pack(">H", int(variant["model_to_agent_dependency_ready"])),
        struct.pack(">Q", variant["fabric_generation"] or 0),
        struct.pack(">Q", variant["model_generation"] or 0),
        struct.pack(">Q", variant["agent_generation"] or 0),
    )


def _quarantine_reason_digest(variant: dict[str, Any], request_digest: bytes) -> bytes:
    assert variant["raw_context"] == "derived_quarantine_reason"
    assert variant["model_cleanup_exact_zero"] == "some_true"
    return _canonical_digest(
        STACK_QUARANTINE_DIGEST_DOMAIN,
        struct.pack(">H", variant["raw_code"]),
        struct.pack(">H", 2),
        request_digest,
    )


def _raw_outcome_digest(
    variant: dict[str, Any], request_digest: bytes
) -> tuple[bytes, bytes | None]:
    context = None
    if variant["raw_context"] == "derived_quarantine_reason":
        context = _quarantine_reason_digest(variant, request_digest)
    else:
        assert variant["raw_context"] == "none"
    fields = [
        struct.pack(">H", variant["raw_code"]),
        struct.pack(">H", int(context is not None)),
    ]
    if context is not None:
        fields.append(context)
    fields.append(request_digest)
    return _canonical_digest(STACK_RAW_OUTCOME_DIGEST_DOMAIN, *fields), context


def _terminal_evidence_flags(variant: dict[str, Any]) -> int:
    names = (
        "census_complete",
        "fabric_ready",
        "model_ready",
        "agent_ready",
        "fabric_to_agent_dependency_ready",
        "model_to_agent_dependency_ready",
        "exact_zero",
        "quarantined",
    )
    return sum(int(variant[name]) << bit for bit, name in enumerate(names))


def _terminal_body(
    variant: dict[str, Any],
    request: dict[str, Any],
) -> dict[str, bytes]:
    values = request["envelope_values"]
    common = LEDGER["runtime_terminal"]["common"]
    target = values[2]
    runtime_store = values[32]
    source_scope = values[3]
    operation_id = values[24]
    request_digest = request["request_digest"]
    target_slice_digest = request["target_slice_digest"]
    assignment_digest = request["assignment_digest"]
    terminal_result_ref = _canonical_digest(
        TERMINAL_RESULT_REF_DOMAIN,
        b"PXMT",
        struct.pack(">H", 1),
        target,
        runtime_store,
        source_scope,
        operation_id,
        request_digest,
    )[:16]
    outcome = {
        "active_ready": 1,
        "no_effect_rejected": 3,
        "uncertain": 4,
        "quarantined": 5,
    }[variant["outcome"]]
    lifecycle = {
        "proven_not_started": 1,
        "may_have_started": 2,
    }[variant["lifecycle_effect"]]
    head = {
        "preserved_none": 1,
        "committed_incoming": 3,
    }[variant["head"]]
    desired = bytes(32) if head == 1 else target_slice_digest
    resource_digest = _resource_census_digest(variant)
    raw_digest, quarantine_reason = _raw_outcome_digest(variant, request_digest)
    channel_target = target
    runtime_peer = bytes.fromhex(common["runtime_peer_hex"])
    local_endpoint = bytes.fromhex(common["local_endpoint_identity_digest_hex"])
    peer_credentials = bytes.fromhex(common["peer_credentials_digest_hex"])
    channel_digest = _canonical_digest(
        LOCAL_CONTROL_CHANNEL_BINDING_DIGEST_DOMAIN,
        struct.pack(">H", 1),
        channel_target,
        runtime_peer,
        local_endpoint,
        peer_credentials,
    )
    response_key = bytes.fromhex(common["response_key_ref_hex"])
    body = bytearray()
    body += target + runtime_store + source_scope + operation_id
    body += request_digest + target_slice_digest + assignment_digest
    body += terminal_result_ref
    body += bytes([1, outcome, lifecycle, head, int(head != 1)]) + desired
    body += _terminal_generation(variant["fabric_generation"])
    body += _terminal_generation(variant["model_generation"])
    body += _terminal_generation(variant["agent_generation"])
    body += struct.pack(">H", variant["physical_binding_census"])
    body += bytes([_terminal_evidence_flags(variant)])
    body += resource_digest + raw_digest
    body += struct.pack(
        ">QQQQ",
        common["completion_runtime_host_epoch"],
        variant["completion_snapshot_sequence"],
        common["selection_clock_generation"],
        variant["selection_observed_at_nanos"],
    )
    body += channel_target + runtime_peer + local_endpoint + peer_credentials
    body += runtime_peer + channel_digest + response_key
    body += struct.pack(
        ">HH", common["response_algorithm"], common["response_algorithm_version"]
    )
    assert len(body) == 519
    return {
        "body": bytes(body),
        "terminal_result_ref": terminal_result_ref,
        "resource_census_digest": resource_digest,
        "raw_outcome_digest": raw_digest,
        "quarantine_reason": quarantine_reason or bytes(32),
        "channel_binding_digest": channel_digest,
    }


def _encode_artifact_terminals() -> dict[str, dict[str, bytes]]:
    request = _encode_artifact_runtime_request()
    signing_key = Ed25519PrivateKey.from_private_bytes(
        bytes.fromhex(LEDGER["signing"]["runtime_seed_hex"])
    )
    terminals: dict[str, dict[str, bytes]] = {}
    for name, variant in LEDGER["runtime_terminal"]["variants"].items():
        encoded = _terminal_body(variant, request)
        transcript = (
            TERMINAL_RECEIPT_SIGNING_MAGIC
            + struct.pack(">H", 1)
            + encoded["body"]
        )
        signature = signing_key.sign(transcript)
        wire = b"PXMT" + struct.pack(">H", 1) + encoded["body"]
        wire += struct.pack(">H", len(signature)) + signature
        assert len(wire) == 591
        terminals[name] = {
            **encoded,
            "transcript": transcript,
            "signature": signature,
            "wire": wire,
            "receipt_digest": _canonical_digest(
                TERMINAL_RECEIPT_DIGEST_DOMAIN, wire
            ),
        }
    return terminals


def _decode_artifact_terminal(
    frame: bytes,
    variant_name: str,
    request: dict[str, Any],
) -> dict[str, bytes | int | None]:
    assert len(frame) == 591
    assert frame[:6] == b"PXMT" + struct.pack(">H", 1)
    expected = _terminal_body(
        LEDGER["runtime_terminal"]["variants"][variant_name], request
    )
    cursor = 6

    def take(length: int) -> bytes:
        nonlocal cursor
        value = frame[cursor : cursor + length]
        assert len(value) == length
        cursor += length
        return value

    values = request["envelope_values"]
    assert take(16) == values[2]
    assert take(32) == values[32]
    assert take(16) == values[3]
    assert take(16) == values[24]
    assert take(32) == request["request_digest"]
    assert take(32) == request["target_slice_digest"]
    assert take(32) == request["assignment_digest"]
    assert take(16) == expected["terminal_result_ref"]
    mode, outcome, lifecycle, head, desired_present = take(5)
    variant = LEDGER["runtime_terminal"]["variants"][variant_name]
    assert mode == 1
    assert outcome == {
        "active_ready": 1,
        "no_effect_rejected": 3,
        "uncertain": 4,
        "quarantined": 5,
    }[variant["outcome"]]
    assert lifecycle == {
        "proven_not_started": 1,
        "may_have_started": 2,
    }[variant["lifecycle_effect"]]
    assert head == {"preserved_none": 1, "committed_incoming": 3}[variant["head"]]
    assert desired_present == int(head != 1)
    assert take(32) == (bytes(32) if head == 1 else request["target_slice_digest"])
    generations: list[int | None] = []
    for generation_name in (
        "fabric_generation",
        "model_generation",
        "agent_generation",
    ):
        present = take(1)[0]
        value = struct.unpack(">Q", take(8))[0]
        expected_generation = variant[generation_name]
        assert (present, value) == (
            int(expected_generation is not None),
            expected_generation or 0,
        )
        generations.append(expected_generation)
    assert struct.unpack(">H", take(2))[0] == variant["physical_binding_census"]
    assert take(1)[0] == _terminal_evidence_flags(variant)
    assert take(32) == expected["resource_census_digest"]
    assert take(32) == expected["raw_outcome_digest"]
    common = LEDGER["runtime_terminal"]["common"]
    assert struct.unpack(">QQQQ", take(32)) == (
        common["completion_runtime_host_epoch"],
        variant["completion_snapshot_sequence"],
        common["selection_clock_generation"],
        variant["selection_observed_at_nanos"],
    )
    target = values[2]
    runtime_peer = bytes.fromhex(common["runtime_peer_hex"])
    assert take(16) == target
    assert take(16) == runtime_peer
    assert take(32) == bytes.fromhex(common["local_endpoint_identity_digest_hex"])
    assert take(32) == bytes.fromhex(common["peer_credentials_digest_hex"])
    assert take(16) == runtime_peer
    assert take(32) == expected["channel_binding_digest"]
    assert take(16) == bytes.fromhex(common["response_key_ref_hex"])
    assert struct.unpack(">HH", take(4)) == (
        common["response_algorithm"],
        common["response_algorithm_version"],
    )
    signature_length = struct.unpack(">H", take(2))[0]
    assert signature_length == 64
    signature = take(signature_length)
    assert cursor == len(frame)
    transcript = TERMINAL_RECEIPT_SIGNING_MAGIC + struct.pack(">H", 1) + frame[6:-66]
    Ed25519PublicKey.from_public_bytes(
        bytes.fromhex(LEDGER["signing"]["runtime_public_key_hex"])
    ).verify(signature, transcript)
    return {
        "outcome": outcome,
        "fabric_generation": generations[0],
        "model_generation": generations[1],
        "agent_generation": generations[2],
        "receipt_digest": _canonical_digest(TERMINAL_RECEIPT_DIGEST_DOMAIN, frame),
        "quarantine_reason": expected["quarantine_reason"],
    }


def _decode_artifact_binding(frame: bytes) -> dict[str, bytes | int]:
    assert len(frame) == 192
    object_ref = frame[:72]
    assert object_ref[:4] == b"PXAK"
    assert struct.unpack(">H", object_ref[4:6])[0] == 1
    assert struct.unpack(">H", object_ref[6:8])[0] == 72
    assert frame[160:192] == _execution_profile_commitment()
    return {
        "object_ref": object_ref,
        "materialization_store_instance": frame[72:104],
        "materialization_sequence": struct.unpack(">Q", frame[104:112])[0],
        "materialization_operation_id": frame[112:128],
        "materialization_receipt_digest": frame[128:160],
    }


def _decode_pxdq(frame: bytes) -> dict[str, bytes | int]:
    assert frame[:4] == b"PXDQ"
    assert struct.unpack(">H", frame[4:6])[0] == 1
    assert frame[6:8] == b"D\x00"
    assert struct.unpack(">H", frame[8:10])[0] == 288
    assert frame[10:12] == b"\x00\x00"
    assert struct.unpack(">I", frame[12:16])[0] == 288
    assert frame[64:68] == b"PXAK"
    assert frame[224:256] == _execution_profile_commitment()
    assert frame[256:288] == _raw_digest(
        b"paraegox.deployment.external-request.sha256.v1", frame[:256]
    )
    return {
        "operation_id": frame[16:32],
        "config_commitment": frame[32:64],
        "object_ref": frame[64:136],
        "materialization_store_instance": frame[136:168],
        "materialization_sequence": struct.unpack(">Q", frame[168:176])[0],
        "materialization_operation_id": frame[176:192],
        "materialization_receipt_digest": frame[192:224],
        "request_digest": frame[256:288],
    }


def _decode_pxdk(frame: bytes, request: dict[str, bytes | int]) -> dict[str, bytes | int]:
    assert frame[:4] == b"PXDK"
    assert struct.unpack(">H", frame[4:6])[0] == 1
    assert frame[6:8] == b"DA"
    assert struct.unpack(">H", frame[8:10])[0] == 240
    assert frame[10:12] == b"\x00\x00"
    assert struct.unpack(">I", frame[12:16])[0] == 240
    assert frame[56:72] == request["operation_id"]
    assert frame[72:104] == request["request_digest"]
    assert frame[104:176] == request["object_ref"]
    assert frame[176:208] == bytes(32)
    assert frame[208:240] == _raw_digest(
        b"paraegox.deployment.external-admission.sha256.v1", frame[:208]
    )
    return {
        "controller_store_instance": frame[16:48],
        "admission_sequence": struct.unpack(">Q", frame[48:56])[0],
        "admission_digest": frame[208:240],
    }


def _encode_pre_c_terminal_record(
    state: bytes,
    pxdq: bytes,
    pxdk: bytes,
) -> bytes:
    assert state in (b"F", b"U")
    lifecycle_generation = bytes.fromhex(
        LEDGER["deployment"]["lifecycle_generation_hex"]
    )
    frame = bytearray()
    frame += b"PXDM" + struct.pack(">H", 1) + state + state
    frame += struct.pack(">H", 496) + bytes(2) + struct.pack(">I", 496)
    frame += pxdk[16:48] + struct.pack(">Q", 1)
    frame += pxdq[16:32] + pxdq[256:288] + pxdk[208:240]
    frame += pxdq[64:136] + pxdq[192:224]
    frame += struct.pack(">QQ", 0, 0) + bytes(96)
    frame += lifecycle_generation + pxdq[224:256] + bytes(64)
    assert len(frame) == 464
    frame += _raw_digest(
        b"paraegox.deployment.external-operation-record.sha256.v1", frame
    )
    assert len(frame) == 496
    return bytes(frame)


def _encode_pre_c_terminal_receipt(
    state: bytes,
    record: bytes,
    pxdq: bytes,
    pxdk: bytes,
) -> bytes:
    lifecycle_generation = bytes.fromhex(
        LEDGER["deployment"]["lifecycle_generation_hex"]
    )
    frame = bytearray()
    frame += b"PXDO" + struct.pack(">H", 1) + b"D" + state
    frame += struct.pack(">H", 432) + bytes(2) + struct.pack(">I", 432)
    frame += pxdk[16:48] + struct.pack(">Q", 1)
    frame += pxdq[16:32] + pxdq[256:288] + record[464:496]
    frame += pxdq[64:136] + pxdq[192:224]
    frame += struct.pack(">QQ", 0, 0) + bytes(96)
    frame += lifecycle_generation + bytes(32)
    assert len(frame) == 400
    frame += _raw_digest(
        b"paraegox.deployment.external-receipt.sha256.v1", frame
    )
    assert len(frame) == 432
    return bytes(frame)


def _encode_pxmj2_prefix(
    phase: bytes,
    pxdq: bytes,
    pxdk: bytes,
) -> bytes:
    assert phase in (b"A", b"F", b"U")
    record = b""
    receipt = b""
    if phase != b"A":
        record = _encode_pre_c_terminal_record(phase, pxdq, pxdk)
        receipt = _encode_pre_c_terminal_receipt(phase, record, pxdq, pxdk)
    body = pxdq + pxdk + record + receipt
    frame_length = 192 + len(body) + 32
    header = bytearray()
    header += b"PXMJ" + struct.pack(">HHI", 2, 192, frame_length)
    header += phase + bytes(3)
    header += struct.pack(">Q", 1 if phase == b"A" else 2)
    header += pxdk[16:48]
    header += struct.pack(">Q", struct.unpack(">Q", pxdk[48:56])[0])
    header += struct.pack(">Q", 0 if phase == b"A" else 1)
    header += struct.pack(">Q", 0) + bytes(64)
    header += struct.pack(">IIIIII", 288, 240, 0, 0, 0, 0)
    header += struct.pack(">HHI", 0 if phase == b"A" else 1, 0 if phase == b"A" else 1, len(body))
    header += bytes(16)
    assert len(header) == 192
    frame = bytes(header) + body
    frame += _canonical_digest(
        b"paraegox.deployment.artifact-bound-managed-model-agent-stack-state.sha256.v2",
        frame,
    )
    assert len(frame) == frame_length
    return frame


def _decode_pxmj2_prefix(
    frame: bytes,
    phase: bytes,
    pxdq: bytes,
    pxdk: bytes,
) -> None:
    assert frame[:4] == b"PXMJ"
    assert struct.unpack(">H", frame[4:6])[0] == 2
    assert struct.unpack(">H", frame[6:8])[0] == 192
    assert struct.unpack(">I", frame[8:12])[0] == len(frame)
    assert frame[12:13] == phase
    assert frame[13:16] == bytes(3)
    assert struct.unpack(">Q", frame[16:24])[0] == (1 if phase == b"A" else 2)
    assert frame[24:56] == pxdk[16:48]
    assert struct.unpack(">Q", frame[56:64])[0] == 1
    assert struct.unpack(">Q", frame[64:72])[0] == (0 if phase == b"A" else 1)
    assert frame[72:144] == bytes(72)
    assert struct.unpack(">IIIIII", frame[144:168]) == (288, 240, 0, 0, 0, 0)
    assert struct.unpack(">HH", frame[168:172]) == (
        0 if phase == b"A" else 1,
        0 if phase == b"A" else 1,
    )
    body_length = struct.unpack(">I", frame[172:176])[0]
    assert frame[176:192] == bytes(16)
    assert body_length == len(frame) - 224
    assert frame[192:480] == pxdq
    assert frame[480:720] == pxdk
    assert frame[-32:] == _canonical_digest(
        b"paraegox.deployment.artifact-bound-managed-model-agent-stack-state.sha256.v2",
        frame[:-32],
    )
    if phase == b"A":
        assert len(frame) == 752
        assert frame[720:-32] == b""
    else:
        assert len(frame) == 1680
        record = frame[720:1216]
        receipt = frame[1216:1648]
        assert record == _encode_pre_c_terminal_record(phase, pxdq, pxdk)
        assert receipt == _encode_pre_c_terminal_receipt(phase, record, pxdq, pxdk)


def test_artifact_execution_binding_shared_golden_is_independently_derived() -> None:
    binding_wire = _fixture("artifact_f0_binding_v1.hex", 192)
    binding = _decode_artifact_binding(binding_wire)
    pxdq = _fixture("artifact_f0_pxdq_v1.hex", 288)
    request = _decode_pxdq(pxdq)
    receipt = _fixture("artifact_f0_pxax_materialized_v1.hex", 240)
    artifact = LEDGER["artifact_store"]

    assert binding_wire == pxdq[64:256]
    assert binding["object_ref"] == _fixture("artifact_f0_pxak_v1.hex", 72)
    assert binding["materialization_store_instance"] == receipt[16:48]
    assert binding["materialization_sequence"] == struct.unpack(">Q", receipt[48:56])[0]
    assert binding["materialization_operation_id"] == receipt[56:72]
    assert binding["materialization_receipt_digest"] == receipt[208:240]
    assert binding["materialization_store_instance"] == bytes.fromhex(
        artifact["store_instance_hex"]
    )
    assert binding["materialization_sequence"] == artifact["primary_operation_sequence"]
    assert binding["materialization_operation_id"] == bytes.fromhex(
        artifact["primary_operation_id_hex"]
    )
    assert binding["object_ref"] == request["object_ref"]
    assert binding["materialization_receipt_digest"] == request[
        "materialization_receipt_digest"
    ]


def test_artifact_pxte11_and_plan_content_goldens_are_independently_derived() -> None:
    expected = _encode_artifact_execution_stack()
    pxte = _fixture("artifact_f0_pxte_v11.hex", 1_805)
    plan = _fixture("artifact_f0_plan_content_v2.hex", 2_057)
    binding = _fixture("artifact_f0_binding_v1.hex", 192)

    assert pxte == expected["artifact_pxte"]
    assert pxte[:6] == b"PXTE" + struct.pack(">H", 11)
    assert pxte[6 : 6 + MODEL_PROJECTION_BYTES] == expected["model_projection"]
    cursor = 6 + MODEL_PROJECTION_BYTES
    assert pxte[cursor : cursor + 32] == _artifact_compatibility_digest()
    cursor += 32
    assert struct.unpack(">HHII", pxte[cursor : cursor + 12]) == (
        1,
        1,
        len(binding),
        len(expected["model_pxte"]),
    )
    cursor += 12
    assert pxte[cursor : cursor + len(binding)] == binding
    cursor += len(binding)
    assert pxte[cursor:] == expected["model_pxte"]
    assert expected["model_pxte"][:6] == b"PXTE" + struct.pack(">H", 8)
    model_tail = 6 + MODEL_PROJECTION_BYTES
    profile, mode, present, dependency_count, reserved, embedded_length = struct.unpack(
        ">HBBBBI", expected["model_pxte"][model_tail:MODEL_PXTE_FIXED_BYTES]
    )
    assert (profile, mode, present, dependency_count, reserved) == (1, 1, 1, 2, 0)
    agent_end = MODEL_PXTE_FIXED_BYTES + embedded_length
    assert expected["model_pxte"][MODEL_PXTE_FIXED_BYTES:agent_end] == expected[
        "agent_pxte"
    ]
    AGENT._decode_pxte(expected["agent_pxte"])

    assert plan == expected["plan_content"]
    assert plan[:32] == b"ParaEGOX\0deployment-plan-content"
    assert struct.unpack(">HBBI", plan[32:40]) == (2, 3, 0, len(plan))
    assert plan[40:56] == bytes.fromhex(LEDGER["predecessor"]["target_hex"])
    assert plan[56:248] == binding
    assert struct.unpack(">I", plan[248:252])[0] == len(pxte)
    assert plan[252:] == pxte


def test_artifact_pxar12_and_runtime_slice_goldens_are_independently_derived() -> None:
    expected = _encode_artifact_runtime_request()
    pxar = _fixture("artifact_f0_pxar_v12.hex", 2_780)
    runtime_slice = _fixture("artifact_f0_runtime_slice_v11.hex", 1_815)
    pxte = _fixture("artifact_f0_pxte_v11.hex", 1_805)
    runtime = LEDGER["runtime_apply"]
    predecessor = LEDGER["predecessor"]
    values = expected["envelope_values"]

    assert pxar == expected["wire"]
    assert runtime_slice == expected["runtime_slice"] == PXTA_ZERO + pxte
    assert pxar[:6] == b"PXAR" + struct.pack(">H", 12)
    envelope_length, binding_length, execution_length = struct.unpack(">III", pxar[6:18])
    assert (envelope_length, binding_length, execution_length) == (
        len(expected["envelope"]),
        len(PXTA_ZERO),
        len(pxte),
    )
    assert pxar[18 : 18 + envelope_length] == expected["envelope"]
    assert pxar[18 + envelope_length :] == runtime_slice
    assert values[2] == bytes.fromhex(predecessor["target_hex"])
    assert values[5] == struct.pack(">Q", predecessor["legacy_successor_revision"])
    assert values[6] == expected["source_plan_digest"]
    assert values[7] == expected["assignment_digest"]
    assert values[8] == expected["target_slice_digest"]
    assert values[22] == struct.pack(">H", 1)
    assert values[23] == bytes.fromhex(predecessor["active_target_slice_digest_hex"])
    assert values[24] == bytes.fromhex(runtime["artifact_operation_id_hex"])
    assert values[27] == bytes.fromhex(runtime["artifact_temporal_constraint_id_hex"])
    assert values[30] == struct.pack(">Q", runtime["original_budget_nanos"])
    assert values[31] == struct.pack(">Q", runtime["remaining_budget_nanos"])
    assert values[32] == bytes.fromhex(runtime["runtime_store_instance_hex"])
    assert values[37] == bytes.fromhex(runtime["artifact_authentication_nonce_hex"])


def test_artifact_pxmt_terminal_goldens_are_independently_derived_and_signed() -> None:
    request = _encode_artifact_runtime_request()
    expected = _encode_artifact_terminals()
    fixtures = {
        "active_ready": "artifact_f0_pxmt_artifact_v1.hex",
        "no_effect_rejected": "artifact_f0_pxmt_artifact_no_effect_rejected_v1.hex",
        "uncertain": "artifact_f0_pxmt_artifact_uncertain_v1.hex",
        "quarantined": "artifact_f0_pxmt_artifact_quarantined_v1.hex",
        "quarantined_after_agent_intent": (
            "artifact_f0_pxmt_artifact_quarantined_after_agent_intent_v1.hex"
        ),
    }
    receipt_digests: set[bytes] = set()
    signatures: set[bytes] = set()
    for variant_name, filename in fixtures.items():
        fixture = _fixture(filename, 591)
        assert fixture == expected[variant_name]["wire"]
        decoded = _decode_artifact_terminal(fixture, variant_name, request)
        assert decoded["receipt_digest"] == expected[variant_name]["receipt_digest"]
        receipt_digests.add(expected[variant_name]["receipt_digest"])
        signatures.add(expected[variant_name]["signature"])

    assert len(receipt_digests) == len(fixtures)
    assert len(signatures) == len(fixtures)
    assert expected["quarantined"]["quarantine_reason"] != bytes(32)
    assert expected["quarantined_after_agent_intent"]["quarantine_reason"] != bytes(
        32
    )
    assert (
        expected["quarantined"]["quarantine_reason"]
        != expected["quarantined_after_agent_intent"]["quarantine_reason"]
    )


def test_artifact_pxmt_signature_drift_is_rejected_by_independent_decoder() -> None:
    request = _encode_artifact_runtime_request()
    fixture = bytearray(_fixture("artifact_f0_pxmt_artifact_v1.hex", 591))
    fixture[-1] ^= 1
    try:
        _decode_artifact_terminal(bytes(fixture), "active_ready", request)
    except Exception as error:
        assert error.__class__.__name__ == "InvalidSignature"
    else:
        raise AssertionError("PXMT signature drift was accepted")


def test_pxdq_pxdk_shared_goldens_have_independent_exact_layout_and_correlation() -> None:
    request = _decode_pxdq(_fixture("artifact_f0_pxdq_v1.hex", 288))
    admission = _decode_pxdk(_fixture("artifact_f0_pxdk_v1.hex", 240), request)
    artifact = LEDGER["artifact_store"]
    deployment = LEDGER["deployment"]
    assert request["operation_id"] == bytes.fromhex(deployment["operation_id_hex"])
    assert request["config_commitment"] == bytes.fromhex(
        artifact["config_commitment_hex"]
    )
    assert request["materialization_store_instance"] == bytes.fromhex(
        artifact["store_instance_hex"]
    )
    assert request["materialization_sequence"] == artifact["primary_operation_sequence"]
    assert request["materialization_operation_id"] == bytes.fromhex(
        artifact["primary_operation_id_hex"]
    )
    assert admission["controller_store_instance"] == bytes.fromhex(
        deployment["controller_store_instance_hex"]
    )
    assert admission["admission_sequence"] == deployment["admission_sequence"]


def test_pxdq_pxdk_reserved_or_digest_mutation_fails_independent_decoder() -> None:
    pxdq = bytearray(_fixture("artifact_f0_pxdq_v1.hex", 288))
    pxdq[7] = 1
    try:
        _decode_pxdq(bytes(pxdq))
    except AssertionError:
        pass
    else:
        raise AssertionError("reserved PXDQ byte was accepted")

    pxdq = bytearray(_fixture("artifact_f0_pxdq_v1.hex", 288))
    pxdq[-1] ^= 1
    try:
        _decode_pxdq(bytes(pxdq))
    except AssertionError:
        pass
    else:
        raise AssertionError("PXDQ digest drift was accepted")

    request = _decode_pxdq(_fixture("artifact_f0_pxdq_v1.hex", 288))
    pxdk = bytearray(_fixture("artifact_f0_pxdk_v1.hex", 240))
    pxdk[176] = 1
    try:
        _decode_pxdk(bytes(pxdk), request)
    except AssertionError:
        pass
    else:
        raise AssertionError("fresh-only previous desired head was accepted")


def test_pxmj2_admitted_and_pre_c_terminal_goldens_are_independently_derived() -> None:
    pxdq = _fixture("artifact_f0_pxdq_v1.hex", 288)
    pxdk = _fixture("artifact_f0_pxdk_v1.hex", 240)
    for name, phase, size in (
        ("artifact_f0_pxmj_v2_admitted.hex", b"A", 752),
        ("artifact_f0_pxmj_v2_failed_pre_c.hex", b"F", 1680),
        ("artifact_f0_pxmj_v2_uncertain_pre_c.hex", b"U", 1680),
    ):
        fixture = _fixture(name, size)
        assert fixture == _encode_pxmj2_prefix(phase, pxdq, pxdk)
        _decode_pxmj2_prefix(fixture, phase, pxdq, pxdk)


def test_pxmj2_pre_c_outer_checksum_and_terminal_pin_drift_are_rejected() -> None:
    pxdq = _fixture("artifact_f0_pxdq_v1.hex", 288)
    pxdk = _fixture("artifact_f0_pxdk_v1.hex", 240)
    admitted = bytearray(_fixture("artifact_f0_pxmj_v2_admitted.hex", 752))
    admitted[-1] ^= 1
    try:
        _decode_pxmj2_prefix(bytes(admitted), b"A", pxdq, pxdk)
    except AssertionError:
        pass
    else:
        raise AssertionError("PXMJ2 outer checksum drift was accepted")

    failed = bytearray(_fixture("artifact_f0_pxmj_v2_failed_pre_c.hex", 1680))
    failed[720 + 248 : 720 + 256] = struct.pack(">Q", 2)
    failed[720 + 464 : 720 + 496] = _raw_digest(
        b"paraegox.deployment.external-operation-record.sha256.v1",
        failed[720 : 720 + 464],
    )
    failed[1216 + 104 : 1216 + 136] = failed[720 + 464 : 720 + 496]
    failed[1216 + 248 : 1216 + 256] = struct.pack(">Q", 2)
    failed[1216 + 400 : 1216 + 432] = _raw_digest(
        b"paraegox.deployment.external-receipt.sha256.v1",
        failed[1216 : 1216 + 400],
    )
    failed[-32:] = _canonical_digest(
        b"paraegox.deployment.artifact-bound-managed-model-agent-stack-state.sha256.v2",
        failed[:-32],
    )
    try:
        _decode_pxmj2_prefix(bytes(failed), b"F", pxdq, pxdk)
    except AssertionError:
        pass
    else:
        raise AssertionError("pre-C committed snapshot pin drift was accepted")
