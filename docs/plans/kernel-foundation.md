# ParaEGOX Kernel Foundation 实施计划

> 状态：Draft
> 日期：2026-08-03
> 实现状态：P0/B1、B2/S2、P2a/S3、P2b/S4、P2c/S5 与 P2d/S6 local POSIX reference slice 已完成；P2e/S7 已获基线授权，S7-B internal successor foundation 已由 `fb1547d` + `f2c6593`、CI `30617157810` 完成，S7-C pure compile 已由 `54ebed8`、CI `30622634625` 完成，S7-D real Authority/journal foundation 已由 `1207f4e` + `47da71e` + `08389f3`、CI `30729690319` 完成，S7-E W1 owner-local store/client foundation 已由 `600838d` + `cc15fc8` + `399970c`、CI `30733163162` 完成；S7-E minimum executable vertical 已收口于 `1ed704c`、Ubuntu CI `30748840399`。S7-F authenticated Runtime query、payload v5、lossless v4→v5 migration 与 fixed Loop/Empty restart reassembly 已提交至 `f32700f` 并由 Ubuntu CI `30782979187` 验证；bounded Controller reconcile 与 SF6 Linux scenarios 仅存在于当前未提交工作树，仍待提交及 fresh Ubuntu CI，S7 继续为 in progress；其余内容仍为目标计划
> 范围：Kernel 契约、RuntimeHost、本地仿真物理闭环、Zenoh Fabric、Node/Deployment、Evidence、OPS/TUI、Operator/Web Gateway 与生态边界
> 实现授权：阶段推进已授权；P2e的public successor、Deck/Graph boundary、新owner与persistent format已由ADR-0004/0005/0007/0008 Accepted基线授权，仍须按阶段落下真实实现与证据。每阶段必须先通过自己的完成证据并形成独立提交，再进入下一阶段
> 前置裁决与研究：[ADR-0002 — CardDefinition、Card 与 CardInstance 术语边界](../adr/ADR-0002-card-definition-terminology.md)、[ADR-0004 — Deck 工作负载、DeckLock 与 Application 准入边界](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md)（Accepted）、[ADR-0005 — typed domain graphs 与 Runtime assembly 边界](../adr/ADR-0005-typed-domain-graphs-and-runtime-assembly-boundary.md)（Accepted）、[ADR-0006 — Rust-first 核心机制与多语言工作负载边界](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md)、[ADR-0007 — P2e reference journal 与 crash recovery](../adr/ADR-0007-p2e-reference-journal-and-crash-recovery.md)（Accepted）、[ADR-0008 — PXTE v4/PXAR v5 source-only/empty successor](../adr/ADR-0008-pxte-v4-pxar-v5-subject-ingress-separation.md)（Accepted）、[Agent OS 参考特性采纳矩阵](../research/agent-os-reference-feature-adoption.md)、[分布式具身 Agent OS 缺口研究](../research/distributed-embodied-agent-os-gap-analysis.md)、[CardDefinition 输入输出、Port、Link 与运行绑定](../research/card-definition-ports-links-and-bindings.md)、[Card 独立开发、测试 Harness 与运行探测边界](../research/card-independent-development-testing-and-probe-boundaries.md)、[Runtime 执行模型、调度与恢复](../research/execution-model-scheduling-and-recovery.md)、[Graph Foundation、领域图与执行边界](../research/graph-foundation-and-domain-execution-boundaries.md)、[Tool 定义、Provider 绑定与调用边界](../research/tool-definition-provider-binding-and-invocation.md)、[Kernel 消息机制与 Fabric 边界](../research/kernel-messaging-fabric-evidence-security.md)、[分布式身份、作用域与物理所有权](../research/distributed-identity-scope-and-ownership.md)、[Web Console、WebRTC、WebXR 与交互式 Gateway 边界](../research/web-console-webrtc-webxr-gateway-boundaries.md)、[Application、Deck、Card 与 Service 边界](../research/application-deck-card-service-boundaries.md)
> 目标架构：[分布式系统模型](../architecture/distributed-system-model.md)
> 架构裁决：[ADR-0001 — DeploymentController、DeploymentPlan 与 Runtime 边界](../adr/ADR-0001-deployment-controller-boundary.md)、[ADR-0003 — OPS、OpsService 与 Inspection 操作边界](../adr/ADR-0003-ops-service-operation-boundary.md)

## 一句话结论

先按 ADR-0006 建立 Rust-first mechanism、Cargo/uv 双工具链和语言中立 wire/process boundary，再冻结 Capability/Service/Feature、Kernel 外的 DeploymentPlan/Revision、RuntimePlanSlice/apply、配置与 physical ABI，完成 bounded Rust RuntimeHost 和最小单写 DeploymentController；P2e 只以 typed Deck/Planner、三个 owner journal 和 RuntimeAssemblyEngine 打通 exact manifest-pinned compiled-in idle Loop fixture 的 `OneSourceLoop → EmptyDeactivate` 单 Node 闭环，不实现 Loop→Loop replacement、通用 activation/ingress、Thread/Process assembly 或 streaming。它不建设通用 Graph Engine，只有两个独立真实消费者出现后才条件抽取纯 Graph Foundation。随后用仿真资源证明 `CommandEndpoint → CapabilityGrant → Lease/Fencing → SafetyIslandAdapter → Enforcement → Receipt`。P3 后并行推进 local Agent 垂直切片、P4/P5 Zenoh 分布式化和 P6a local Evidence/node-local Inspection，再由 P5+P6a 进入 P6b federated Inspection + OpsService 并汇合为 distributed Agent execution；TUI、Operator/Web Interaction、ROS2 Gateway、空间语义与条件式 H1 硬件 gate 只建立在已验证的 owner/contract 上。Rust 由 Cargo/locked toolchain 管理，Python SDK、worker、Agent/模型服务与治理工具继续统一使用 `uv`。

## 当前实现快照（2026-08-03）

- P2a/S3 已在 B2 纵向链上增加 canonical PXTA assignments 与完整 PXAR request；S4/P2b 继续增加 canonical PXTE execution plan 与 PXAR v2 composite digest。Deployment production builder 精确匹配独立 Python fixture，Runtime 只接受完整请求，再贯通 exact-trust admission、writer fence/prepare、PortBinding/Mailbox 和 crate-private component execution。B1/S2 摘要与历史 envelope ABI 保持不变。
- Mailbox 已证明 items/bytes/age/inflight/retained bounded、offer/admitted cohort 守恒、显式压力拒绝、drain/close 与 released-token `Uncertain` cleanup；S4 增加 permit-before-dequeue、单 owner LoopDomain/Dispatcher、公平与 max-burst、deadline-before-run、同构建 trusted Card callback、独立 late-generation fence 和 exact-zero local cleanup。
- S5 增加 additive PXTE v2/PXAR v3 Thread execution contract、RuntimeHost 全局 ExecutorBudget、固定 worker ThreadDomain、提交前容量保留、cancellation/uncertain/wedged 与 late-result fence、真实 JoinHandle census/线性 proof，以及 crate-private trusted synchronous Card composition。fenced payload 与最终 Card 均由同一计费 worker 析构；blocking/panic cleanup 不会制造假容量或假 proof。
- S6 增加 additive PXTE v3/PXAR v4 Process execution contract、严格 PXWP v1 与 PXHW v1 协议、RuntimeOwnershipTree、Liveness/Recovery reducer、crate-private RuntimeHost-owned local POSIX ProcessDomain、generation-scoped ephemeral workspace、bounded IPC/credit/retained bytes、late-result fencing、restart budget/backoff/quarantine，以及 Rust/Python reference worker、外部 watchdog 和 POSIX service-manager Harness。Linux `/proc` census 对 RSS/FD/process-tree/CPU fail-closed，并在 exec/exit 竞态下使用固定 work/retry budget。
- S7-B 已在 `paraegox-runtime-contracts` internal surface冻结 exact manifest/build/store-bound PXTE v4/PXAR v5、`RuntimeApplyEnvelopeV2`、authenticated bootstrap/query 与固定 reference assembly grammar；共享 fixture SHA-256 为 `2a446b15e1e8c2f7af4b812dd94e0120666e70f875af992d2e8854fa24815f4b`，Rust 96 tests、Python 354 tests、独立终审和 GitHub CI `30617157810` 均通过。它仍不是 Runtime endpoint、installer、journal 或可启动的 deployment control plane。
- S7-C 已在现有 `paraegox-deployment` 内以 crate-private `deck`/`planner` module 实现 deterministic DeckLock、bounded typed validation、stable cycle evidence、narrow one-subject/empty transition与stable allocation delta；提交 `54ebed8` 的 deployment 61 tests、全量 Python 359 tests、本地完整门禁、独立复核和 CI `30622634625` 均通过。DeliveryProfile/Requirement/refinement 仅为 opaque commitment 且任何非空 wider shape均由Planner拒绝；manifest ingress没有production constructor，因此仍不构成installer、Controller或可启动进程。
- S7-D 已由主提交 `1207f4e` 与 Linux evidence/teardown follow-up `47da71e`、`08389f3` 完成：`paraegox-tenure-authority` 是真实 POSIX process，具备 one-shot initializer、owner-local crash-consistent store、独立 service identity/ACL、Controller Ed25519 request auth、bounded local IPC、CSPRNG store identity、commit-before-reply、exact replay/conflict/restart/crash evidence；Controller/Runtime 各自的 crate-private versioned/checksummed/bounded journal codec、state reducer与successor validator也已落地。CI `30729690319` 两个job全绿，Linux Python 369 tests无skip，Rust workspace/真实worker/doctest门禁通过。该完成范围没有 Authority client、Controller/Runtime store+initializer、Runtime endpoint 或 DeploymentController executable。
- S7-E W1 已由主提交 `600838d` 与 Linux compile/clippy follow-up `cc15fc8`、`399970c` 完成：Controller one-shot initializer/store、authenticated Authority client 与 Runtime store 均为 owner-private，并具备 exact recovery binding、strict proof/peer verification、bounded transport/read、crash-consistent publish、显式 normal-drop unlock 与 stopped-state fail-closed。CI `30733163162` 两个 job 全绿。该完成范围没有 Runtime initializer、installer、Runtime endpoint、DeploymentController executable 或 workload lifecycle。
- workspace 现在有 `paraegox-runtime-host` executable：它拥有 current-thread Tokio reactor、固定本地 clock generation、bounded structured task/cancellation tree，并可在 Ctrl-C 后完整 join；已提交的`serve-bootstrap-v1`还拥有durable Runtime journal、authenticated PXBR/PXAR/PXQR endpoint、fixed native Loop/Empty owner和listener发布前的fixed-profile restart reassembly。已提交的`paraegox-deploymentd`能够沿真实command path完成committed plan、tenure、bootstrap、Loop apply及更高revision Empty commit/apply；S7-E累计基线由`1ed704c`与Ubuntu CI `30748840399`验证，Runtime query/reassembly后续基线由`f32700f`与Ubuntu CI `30782979187`验证。该窄路径仍没有post-idle ingress、一般Card/CoreService/Thread/Process assembly或continuous reconciliation。
- S4 的静态 utilization/control-start admission 使用 `arrivals × (run + cleanup)` 和跨 class 最坏顺序；保证以 producer 遵守 signed arrival envelope 为前提，Runtime 尚未观测 sliding-window violation。本机 2× offer 诊断和 scheduler-tick A/B 不是目标平台、硬实时或 live ingress 证明。
- P2e 只补固定 `ReferenceAssemblyProfileV1::OneSourceLoop` 的 exact-revision idle readiness：manifest/build/fixture、revision/CAS、LoopDomain/Instance、bounded cooperative `on_start` 与 live generation全部匹配后才 `LiveReady`。P2b 只证明 startup 不等于 Ready；一般 dependency/resource/permission readiness 合取仍等待有真实 producer/consumer 的后继 assembly contract。
- P2d/S6 的完成范围是本地 POSIX reference slice，不是完整 production isolation：C++ worker、production program resolver、cgroup/pidfd/full sandbox、GPU/device reset、跨重启 durable recovery journal 和正式 Runtime assembly 尚未实现；非 Linux Unix 只运行受信 local harness，不宣称 live resource enforcement。drop fallback reaper 不注册/不 join，不能签发 cleanup proof；watchdog 在 host 被不可捕获 SIGKILL 后也没有共享 containment/journal 去清理另一个 owner 的 ProcessDomain group。
- 当前阶段仍是 P2e 最小 Deployment control plane，活动工作位于 S7-F 收口：S7-E executable vertical 与 Linux-only process fixture 已由提交`1ed704c`、Ubuntu CI `30748840399`和独立复核收口，pytest 372项全部通过且该fixture实际执行、无skip；authenticated Runtime query、Controller exact query request/response journal foundation、payload v5、lossless v4→v5 migration 与 fixed Loop/Empty restart reassembly 也已提交，其中 Runtime 最新基线`f32700f`由Ubuntu CI `30782979187`验证。当前未提交工作树已有bounded one-shot `reconcile-reference-once-v1`与SF6 Linux lost-response/SIGKILL scenarios，但在独立复核、提交及fresh Ubuntu CI前不算完成；deploymentd仍不是daemon或continuous reconciler。一般 readiness/activation/Ingress、Thread/Process 和 streaming 不属于该阶段。

## 1. 最终最小证据链

```text
Simulated Sensor
      │ Grounded Signal {device/frame/unit/time/calibration/uncertainty}
      ▼
Card.Out → Deck Link → DeploymentPlan.bindings
      │ target RuntimePlanSlice / RuntimeApplyRequest
      ▼
PortBinding
      │
      ├── P2/P3: fixture → validated Message
      └── P4+: one active Zenoh route
          {session-local | host-local | remote}
               → bounded Fabric ingress buffer
               → decode / validate → Message
      │
      ▼
bounded target Mailbox
      │
      ▼
Controller-role CardInstance → Command
      │
      ▼
IdentityResolver → AuthorityService
      │ CapabilityGrant + AuthorityDecision
      ▼
ResourceCoordinator → LeaseGrant + FencingToken
      │
      ▼
SafetyIslandAdapter → SafetyDecision
      │
      ▼
Driver EnforcementPoint → downstream safety gate → Simulated Actuator
      │ staged Receipt
      ▼
P3 Receipt/Evidence test sink
      └── P6a local durable handoff/node-local Inspection
              └── P6b federated Inspection + OpsService → TUI
```

同一 Port/Message/Mailbox 契约随后通过 Zenoh Fabric 的 `session-local`、`host-local` 与 `remote` route 运行。Kernel 测试不安装或启动 Zenoh、ROS2、OTel、数据库、模型或硬件 SDK；测试 fixture 不成为生产 Bus 或 Deployment route。

## 2. 计划红线

- 不复制 EAGOS 源码、配置、测试、文档正文或 `Module`/`Bundle` 语义。
- 不实现全局 `Runtime`、万能 Bus、通用 `TopologyManager` 或 `RemoteDomain`。
- 不实现 Kernel 通用 Graph Engine、Graph Store/Service/Query Router、`GraphKind + metadata` 或 `execute(arbitrary_graph)`；Graph Foundation 只有两个独立生产消费者后才经 ADR 条件抽取纯算法，当前不创建包或占位 API。
- RuntimeHost 只拥有本 Node 内的 Loop/Thread/Process execution；远端生命周期归目标 RuntimeHost。
- Rust 是首个 production mechanism reference，不是公共领域类型；CoreService、Card、Driver、Gateway 与 DeploymentController 的身份和 authority 不由语言、crate、binary 或共进程决定。
- 公共合同不暴露 Rust memory layout/trait/Tokio handle、Python object 或语言私有 queue/lock；Rust/Python/C++ 只实现同一 versioned Schema、canonical encoding、digest 和 reason code。
- 受信同构建 Rust implementation 才可作为 internal in-process candidate；Python/C++/第三方/native/GPU workload 默认通过受管 ProcessDomain 或独立 Service/Gateway 接入，不建立公共 Rust dylib Card ABI或嵌入 CPython 的默认 Runtime 路径。
- 不建立 `ComputeNode`、`FabricRegion`、Robot、Embodiment、Fleet 或 Cluster 基础类型。
- 裸 `Region` 与裸 `Topology` 不进入公共 API；应用、服务、Zenoh 和空间关系使用限定名称。
- 当前只有非权威 `site_hint`；`SiteRef` 等待真实消费者与 Proposed ADR。
- 不承诺端到端 exactly-once、全局顺序或跨服务分布式事务。
- 不建立公共 Lane、`thread_lane`、`process_lane` 或让 Card 实现/Deck 手工编排线程、PID 和 Runtime ready queue。
- `CardDefinition` 是开发者侧不可变的可复用能力合同，包含 Artifact export/entrypoint 引用；它不是业务基类、安装包或运行对象。公共领域模型不建立 `Handler`、`Factory`，也不让私有实现对象成为独立运行身份。
- 不提供 `CardDefinition.run()`、`Card.run()`、作者 new/start CardInstance、production StandaloneRunner 或 RuntimeHost 第二种 desired input；普通实现对象单测、internal canonical Slice Harness 与 one-subject Deck 是三种不同证据层。
- 不建立万能 `probe() -> bool` 或公共 Probe registry；startup、liveness、readiness、health、test observation 和有副作用 physical diagnostic 分型，Card 实现不能写自己的 lifecycle/Ready 终态或直接触发 recovery。
- Card、Deck、CardDefinition resolution 与 DeckTopology 不进入 Kernel；`runtime/` 不 import `deployment/` 或 `decks/`，只消费由唯一 DeploymentPlan 投影的 `RuntimeApplyRequest/RuntimePlanSlice`。
- Deck DataLink、ServiceDependency 与 activation constraint 是不同 typed edge；RuntimeHost 不从 Link 猜测启动顺序。未来一般 assembly successor 若允许这些形状，必须让 DeploymentPlan.execution/Slice 完整携带 readiness、activation group、consumer ingress、producer egress、dependency-loss 与 drain contract；S7 的 PXTE v4 不编码这些 branch，Planner 在 candidate/commit 前拒绝相应 workload。
- Deck 是 executable workload，不是 Product、Release、Installation 或 DeploymentScope 的别名。首版不创建 Application DTO/controller/store、application-owned service 或 `applications/` 空包；多 Deck、稳定安装 identity 或跨 DeckRun 私有状态出现真实证据后再进入 Proposed ADR。
- reference CardInstance 只有隔离 ephemeral workspace；默认不注入 host persistent path、数据库 credential 或任意 egress。外部状态只能经声明的 typed service/access handle；不可信 Card 只能进入受限 ProcessDomain，Loop/ThreadDomain 不是 sandbox。
- `DeploymentPlan`、`DeploymentRevision`、DeploymentPlanner/DeploymentController、placement 和 reconciliation 只属于顶层 `deployment/`；Kernel 不定义这些类型。把 Deployment 纳入 Foundation 计划是为了冻结跨层契约，不是把它沉入 Kernel。
- 每个 DeploymentScope 只有一个有效 DeploymentController 写 owner；CLI、OpsService、GitOps 和 DeckCompiler 只能提交 intent 或调用 DeploymentController，不能并列绕过 DeploymentController 直接写 RuntimeHost。
- 不建立 Kernel VFS、全局 URI scheme registry、万能 ObjectRef 或跨 owner `open(uri)`；Artifact、Evidence、Secret、Workspace、Blob/Buffer 与服务状态使用 owner-specific typed reference/client。
- `CapabilityGrant` 只表示安全授权；服务接口使用 `ServiceContract`，Node/Fabric/Device observed support 使用限定 FeatureReport，三者不共享基类、epoch、缓存或 registry。
- 配置通过 ConfigSchema → CardProfile/SecretRef → DeckLock → ConfigSnapshot digest → immutable validated config 单向解析；首版变更配置必须产生新 DeploymentRevision。
- 不为 helper、每个算法函数、CoreService、Driver/Gateway 或 Deck 拓扑强行定义 CardDefinition；只有确有独立复用、配置、实例隔离、执行要求和观测边界的应用能力才准入。
- `In`/`Out` 只声明 transport-neutral 单向 Port；CardDefinition/Card 不持有 Topic、queue、Publisher、Zenoh Session 或 live binding。
- Port 内在契约、Deck Link 的本次交付意图、`DeploymentPlan.bindings` 的期望安装信息和 Runtime `PortBinding` 的运行事实各有唯一 owner；不创建并行 `BindingPlan`。
- Zenoh 是唯一生产 Fabric；公共 live binding 只叫 `PortBinding`。非 Zenoh 路径只作为确定性 `PortBinding test fixture`，不引入生产 `LocalBus`、`ZenohBinding`、`MemoryPortBinding` 或 `MemoryBus`。
- 不恢复宽泛 `runtime/io`，不让浏览器直连 raw Zenoh/Runtime/Driver。Web Console 经 ConsoleGateway 使用 InspectionProtocol/OpsProtocol，TUI/CLI 使用对应 typed clients；WebRTC/WebSocket/HTTP 是 Gateway 外部腿，WebXR 是浏览器 XR API，不是 Transport 或 Fabric backend。
- 不建立每 Node 一个 OpsService 或 ConsoleGateway 的不变量。InspectionService 与 OpsService 分离：前者只拥有只读 projection，后者只拥有 ControlRequest journal；二者都不接管 source/desired/effect truth。
- 不让 Camera/Driver/Card 实现代码自行启动公网 Web 服务、PeerConnection、私有 event loop 或 daemon thread；Gateway managed-workload/exposure contract 未冻结前，不伪装成 CardDefinition、Card、CoreService 或空 `GatewayInstance`。
- 不建立泛 `ExternalSessionId`；browser auth session、WebRTC peer、XR input stream、未来 teleoperation session 与 Fabric/Agent/Device session 分型并各自分代。短期 peer/session 不产生 DeploymentRevision。
- 遥操作断线不依赖最后一个 neutral/stop packet；本地 lease/deadman/fencing/Safety 必须独立收敛，DataChannel/WebSocket ACK 不能成为物理 EffectReceipt。
- 同一 DeploymentRevision 与活动 BindingEpoch 内，一个 BindingId 只有一条接收新 Message 的 active route；禁止 local+wire 双投、隐式 fallback 与内容/时间窗 echo 去重。
- `In`/`Out` 不冒充所有交互模型；P2 数据链首版仅执行静态 1:1 Signal/Event，P3 只增加静态 1:1、bounded outstanding、无透明 retry 的 `OperationClient/CommandEndpoint`。未实现的 Call/复杂 Operation、fan-in/fan-out 和动态绑定必须 fail-fast。
- 不把 `ExecutionRequirements`、DeliveryProfile、DeploymentPlan.execution 和 observed Runtime facts 合并为一个可被多方改写的配置对象。
- 不用“Mailbox 有界”冒充“系统有界”；pre-validation Fabric ingress frame、queued Message、inflight、executor/IPC credit、child work 和 retained payload bytes 全部必须有 owner 与预算。
- RuntimeHost 只记录真实 RuntimeFailureFact；RecoveryEngine 不伪造副作用 Receipt，process/device crash 后无权威终态证明的工作为 `Uncertain`，自动 restart 不自动 replay。
- `Agent` 与 `Supervisor` 保留给 Agent 层；Runtime/Node 基础设施不声明 `NodeAgent`、`NodeSupervisor`、`RuntimeSupervisor`、`SupervisionSpec` 或 `runtime/supervision`。每 Node 的驻留管理进程称 `NodeDaemon`。
- DeploymentRevision 不原地修改活跃 Domain/Mailbox/Policy；使用 revision-tagged prepare/activate/drain/retire/rollback。
- P3–P5 的 physical write 证据只覆盖 simulation profile；未通过 local durable Evidence、HIL、独立 E-Stop/物理隔离和产品 hazard/ODD gate 时，不连接真实 actuator。
- 不用文档、空目录、mock-only 测试或人工看 log 证明功能完成。

## 3. 分阶段 Architecture Decision inventory

下面是决策库存，不是要求 P0 一次写完 18 份文档。只把当前纵向切片将依赖、跨层且难回滚的不变量提升为 Proposed ADR；尚无消费者的部分保留 Research。建议每个 gate 合并为少量内聚 ADR：

| Gate | 本阶段需要冻结的决策簇 |
| --- | --- |
| P0/P1 前 | 已接受 ADR-0001 的 DeploymentPlanner/DeploymentController/Plan/Slice/Runtime owner 边界；已接受 ADR-0006 的 Rust-first/polyglot、Cargo/uv、ProcessDomain 与 language-neutral wire boundary；Kernel import/crate 依赖红线；CapabilityGrant/ServiceContract/FeatureReport 三义；VFS/ObjectRef 拒绝和 owner-specific refs；RuntimeApplyRequest/RuntimePlanSlice 单向边界；ConfigSnapshot；通用 ID/epoch/time/message/receipt 与 Agent protocol seam |
| P2 前 | CardDefinition/In/Out/Port/Link/Binding/Mailbox；ExecutionRequirements/DeliveryProfile/DeploymentPlan.execution；Card 不可直接 run、internal canonical Slice Harness 与 startup/liveness/readiness/health 分型；提出并接受 ADR-0005 的 Graph Foundation 准入门、领域图 owner、DataLink/activation 分型、cyclic Deck 保守策略与 RuntimeAssemblyEngine 边界；有界性、deadline、Loop/Thread/Process Domain、RuntimeOwnershipTree、LivenessSpec/State、FailureContainmentSpec、RecoveryPolicy/Engine、external watchdog、revision transition 与 Evidence/Telemetry 可靠性分层 |
| P3 前 | trusted Observation/physical package、OperationClient/CommandEndpoint、Device/assembly/calibration/mode owner、Authority/Lease/Fencing、Safety island/Adapter、Decision-to-Effect binding、Continuity、Scenario 与 simulation-only gate |
| P4/P5 前 | Zenoh keyspace/schema/session/route、Node/workload identity、cross-Node client、artifact/runtime compatibility、partition/reconciliation；ROS2/DDS 只冻结 Gateway 边界，不提前冻结实现参数 |
| Agent slice 前 | DeckRun-bound AgentSession 的 writer/storage/seal/retention 边界；Run/Turn/Step/InvocationAttempt durability、RunExecutionSnapshot、ContextMaterializer、ToolCatalogSnapshot/ToolView、semantic vs enforcement budget、sandbox/data-flow、OutcomeRequirement→VerificationSpec authority 与 independent Verifier/Eval 边界 |
| A0 条件触发 | 多 Deck 统一闭包、installation-owned mutable state、同 release 多次隔离安装或多 Artifact 共同 owner 任一出现时，提交 identity producer/independent consumers、与触发条件对应的最小 owner matrix/fixture/failure Harness；不强迫无关的第二份 DeckLock 或私有 namespace。出口只能是 Accepted 的最小 ProductRelease/Installation/Application 或其他窄 owner ADR，或稳定 reason code 拒绝 |
| Operator/Web slice 前 | ADR-0003 的 InspectionService/OpsService/ConsoleGateway/clients owner 边界；managed Gateway/external workload manager/exposure 与内部 typed endpoint；browser auth、WebRTC peer、XR input stream 的限定 identity/epoch、Principal 映射、single-active external route 与断线收敛 |
| H1 Hardware Enablement 前 | commissioning/readiness snapshot、real Driver conformance、HIL fault/timing matrix、独立下游 safety gate、Evidence、hazard/ODD、rollback/人工接管；Release owner 签发 HardwareEnablementReceipt，目标 Node 的 local HardwareActivationGate 安装并逐命令强制 activation ref/epoch |

ADR 冻结不变量，不冻结首版目录细节、数据库选型或尚未基准测试的 Zenoh 参数。一个决策簇只有在进入对应 gate 时才必须 Proposed；P0 不以 ADR 数量作为完成指标。

## 4. 阶段 DAG

```text
P0 Boundary + Cargo/uv + wire/process boundary
        │
        ▼
P1 Kernel + Deployment/Runtime + Physical Contracts + Time
        │
        ▼
B2 Runtime apply wire + authentication + temporal admission（COMPLETE）
        │
        ▼
P2a Mailbox + PortBinding test fixture
        │
        ▼
P2b LoopDomain + Dispatcher
        │
        ▼
P2c ThreadDomain + ExecutorBudget
        │
        ▼
P2d ProcessDomain + Liveness + Recovery
        │
        ▼
P2e Minimal Deployment control plane
        │
        ▼
P3 Simulated Physical Control Spine
        ├── A. local Agent vertical slice（durable effect 等待 P6a）
        ├── P4 Zenoh routes ──> P5 two hosts
        └── P6a local Evidence commit + node-local Inspection

P5 + P6a ──> P6b replication / federated Inspection / OpsService ──> P7 TUI
       A + P5 + P6b ──> Distributed Agent Execution
                              ├── P8 ROS2Gateway
                              ├── P9 SpatialMap + Semantic Navigation
                              └── H1 Hardware Enablement（条件式，不是 P3 结论）

Platform Compatibility candidate pool（不改变 P 编号，也不阻塞 Linux reference P3）
P2e executable vertical ──> PCA explicit admission decision ──> PC0 Linux semantics/conformance
PC0 ──> PC1 macOS backend/CI
PC0 ──> PC2 Windows backend research/implementation
PC0 +（PC1 或 PC2 任一第二真实 OS backend）+ 两个独立生产消费者
    ──> PC3 shared extraction evaluation/admission gate

Operator & Web Interaction（独立 workstream，不改变 P 编号）
P6a ───────────────────────────> O1 local read-only Console
P5 + P6b + O1 ────────────────> O2 distributed Console operations
P4 + high-bandwidth payload ──> R1 view-only WebRTC media
R1 + physical frame/time ABI ─> R2 WebXR view-only
P3 + P6a + R2 ────────────────> R3 simulated teleoperation
R3 + per-device H1 ───────────> R4 constrained real teleoperation
```

每一阶段只有在自己的失败注入和完成证据通过后，才成为下一阶段的可信依赖。

## 5. P0：术语、语言边界与工程基线

### 结果

- 建立最小根 Cargo workspace、`Cargo.lock`、pinned `rust-toolchain.toml`、首个有真实消费者的 Rust crate 和 crate dependency boundary check；保留当前 `pyproject.toml`/`uv.lock` 作为 Python 治理工具基线，不把它误写成 production Runtime 实现。
- 冻结 Rust-first/polyglot contract：Rust production mechanisms、Python/C++/native ProcessDomain worker、唯一 Runtime lifecycle owner、language-neutral wire Schema/canonical digest，以及禁止公共 Rust dylib ABI和默认嵌入 CPython。
- 冻结 `partial failure`、`partitioned`、`stale`、`wedged`、`uncertain`、epoch 与 revision 的词义。
- 冻结“无公共 Lane”边界：CardDefinition/Card/Deck 不出现 `lane_id`、`thread_lane`、`process_lane`、线程、PID 或 Runtime ready queue 编排。
- 冻结 `CardDefinition → Card → CardInstance` 的定义、配置使用与运行身份边界；CardDefinition 持有 Artifact export/entrypoint 引用，每个 CardInstance 默认托管一个私有实现对象。
- 冻结 `PermissionRequirement/CapabilityGrant`、`ServiceRequirement/ProvidedService` 与 Node/Fabric/Device FeatureReport 三类语义；CardDefinition 的 `requires.services/permissions/features` 分开求解。
- 冻结 Kernel VFS/ObjectRef 拒绝与 owner-specific typed reference；不以 URI 重建 Service Locator。
- 冻结 `deployment/contracts` 拥有 DeploymentPlanCandidate、committed DeploymentPlan/Revision、DeploymentWriterRef/DeploymentWriterEpoch，纯 DeploymentPlanner 产生 candidate，DeploymentController 原子提交 allocation delta/revision/plan；`runtime/contracts` 拥有 source-only PlanProvenance、PlanWriterContext/WriterTenureProof、Slice/apply Schema。纯 Slice projector 生成 tenure-neutral Slice，纯 apply builder 映射 writer tenure。RuntimeHost 不 import deployment 类型，只消费带 source-scope/source-revision/writer-ref/writer-epoch/target/exact-active-slice CAS/source+slice digests/operation-id/temporal-constraint/tenure-proof/auth 的请求。
- 冻结 ConfigSnapshot/SecretRef/observed config digest 权威链；首版不做原地 hot reload。
- 冻结 `CardDefinition In/Out → PortSpec`、`Deck Link → DeliveryProfile`、`DeploymentPlan.bindings → desired install`、`PortBinding → observed runtime` 的单向编译链。
- 冻结短名 `Node` 的唯一含义，以及 `ServiceDependencyGraph`、`ZenohTopologyProfile` 和未来 `SemanticRegionRef` 等限定图名；`DeckTopology` 长期模型等待 ADR-0004 接受。
- 评审 Proposed ADR-0004；接受后冻结 `DeckCompiler → DeckLock {canonical DeckTopology + resolved closure}` 单一解析真相、字段分区和 validator：DeckResolver 是纯内部步骤，两部分受 digest 覆盖，DeploymentPlanner 不接收独立 topology。
- 在 ADR-0004 评审中同时冻结 `Deck` 公共名称和 Card 最小语义边界：Card 至少拥有 Deck-scoped Card key 与 CardDefinitionRef，并可携带经 Schema 验证的 per-use config/profile；可选 refinement 只能在定义允许的 envelope 内收窄或加强。Port/Link、Canvas view、live route、CardInstance identity、placement、Grant/Secret 和跨 DeckRun 状态均不进入 Card。禁止在公共 Schema、package、CLI/API 与 Receipt 中引入 `Bundle` 作为 Deck 别名。
- 冻结 Card key 的 revision 语义：对同一 active Deck workload 的 previous-plan diff，未改 key 只表示同一 desired slot；rename 是 remove + add，删除后复用 key 不复活旧 CardInstance 或私有状态，任何 state migration 都需独立 owner/Receipt。
- 按 ADR-0004 提案验证产品“应用”当前不作为公共 identity，DeckRun/CardInstance/ServiceInstance 才是运行事实；应用私有、跨 DeckRun 的持久状态首版明确 unsupported，不得藏入 Card 全局状态或冒充平台 CoreService。
- 冻结 `UnsupportedStateLifetime` 与 `UnsupportedProviderOwnership` reason code；声明非支持 state lifetime/provider ownership 时在编译期拒绝，未声明 raw storage/egress 则由受限 ProcessDomain enforcement 拒绝并产生可观测事实。
- 落实已接受的 ADR-0001，并为其余尚未冻结的决策建立最小 Proposed ADR，保留评审状态和证据链接。
- 冻结 P0 最小 Rust toolchain/target + Python 治理/SDK + 开发/CI OS 基线；为 target triple/libc/CPU features、Python ABI、ROS2、Jetson、GPU 和目标 Ubuntu 建立有 owner 的兼容性研究，其结果分别作为 P2/P4/P8/硬件里程碑 gate。

### 约束

- Rust Kernel 的最小 feature profile 不依赖 Python、Zenoh、ROS2、OTel、数据库、TUI、模型和硬件；这些依赖只随真实 adapter/workload crate 或 Python uv group 准入。
- Cargo 与 uv 各有唯一 lock/toolchain authority，不相互替代，也不维护第三份依赖清单。
- Kernel 禁止导入 `runtime`、`services`、`deployment`、`gateways`、`drivers` 和上层领域。
- `deployment/` 可以依赖 `runtime/contracts`，`runtime/` 不得 import `deployment/`；DeploymentPlanner/DeploymentController 不进入 Kernel，DeploymentController 也不作为由同一目标 RuntimeHost 自部署的普通 CoreService。
- Card 实现代码禁止直接创建 Thread、Process、event loop 和无 owner background Task；例外只能来自有 owner、可验证的 Runtime/Driver adapter。
- CardInstance 私有实现对象可以保存领域状态并提供回调，但 lifecycle identity、推进、恢复和关闭仍由 CardInstance/RuntimeHost 拥有。
- CardInstance ephemeral workspace 每个 instance generation 新建并回收；未声明 host filesystem、database 和 network/egress 不进入实现上下文。不可信实现不得 placement 到 Loop/ThreadDomain。
- 原生 Zenoh/raw Fabric 访问不按组件类别自动授予；普通 CardInstance 私有实现上下文只获得为 CardDefinition 声明端口编译的 PortBinding，例外必须有 scope 指向 Fabric resource 的可审计 `CapabilityGrant`。
- 不创建 `application_id`、ApplicationSpec/Lock/Instance/Controller、ApplicationService 或 `applications/`；UI 的产品分组不产生权限、GC 或数据所有权。
- 不为未来设想创建无生产者、消费者和测试入口的空包。

### 验证与证据

```bash
cargo fmt --all --check
cargo metadata --format-version 1 --locked
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo test --workspace --doc --locked

uv sync --locked
uv run --frozen ruff check .
uv run --frozen python scripts/check_governance.py
uv run --frozen pytest
```

根 Cargo workspace、pinned Rust toolchain、`Cargo.lock` 与五个 Rust crate 现已建立，因此上面的 Cargo/uv 命令都是强制门禁。B2 证明 canonical apply wire、真实 Ed25519 exact-trust admission、首次 target-ingress local deadline、bounded replay state 与现有 reducer 的纯闭环；P2a 进一步证明 canonical assignments/complete request、bounded Mailbox 与 deterministic single-route PortBinding；P2b 增加受控 RuntimeHost reactor、structured task owner 和有界 Loop/Card component seam；P2c 增加全局线程预算、固定 worker 与真实 join/cleanup proof；P2d local reference slice 增加 versioned Process execution/worker/watchdog contracts、POSIX ProcessDomain、真实 Rust/Python child、process-group cleanup 与 deterministic recovery。它们仍不证明公开 apply/assembly、持久 journal、完整 readiness、ledger retention/rollover、clock recovery 或 production DeploymentController 已实现。CI 必须同时证明 Rust Kernel 最小 profile 不依赖 Python/Zenoh/ROS2/OTel，Cargo/uv lockfile 未漂移，Kernel/Runtime 反向依赖会使构建失败。当前证据不伪称尚未实机验证的 ROS2/Jetson/GPU 组合受支持。

截至 2026-07-31，B1 合同收口提交 `485d335` 与 GitHub CI `30509055586`、S2 提交 `dd3c950` 与 CI `30512762357`、S3 提交 `521cf8d` 与 CI `30522872591` 仍是历史基线。S4 提交 `451f5e2` 已推送，并通过 212 个 Rust 单元测试、1 个 compile-fail doctest、63 个 Python 测试、完整 Cargo/uv/governance 门禁、RuntimeHost Ctrl-C 进程烟测与独立终审（无 blocker/high）；GitHub CI `30534371196` 的 `rust-core` 与 `governance-and-tests` 均成功。S5 提交 `f6cf31a` 已推送，并通过 276 个 Rust 单元测试、1 个 compile-fail doctest、103 个 Python 测试、完整门禁、Ctrl-C 进程烟测、析构/registry 故障注入与独立终审（无 blocker/high）；GitHub CI `30542046220` 的 `rust-core` 与 `governance-and-tests` 均成功。S6 主提交 `b89f283` 与 Linux race follow-up `97f6e2b`、`9baa422`、`861d436` 已推送；Linux CI 常规 all-targets 发现 394 个 Rust tests，结果为 392 passed/2 environment-gated ignored，两项 ignored 又在正确 worker 环境分别显式运行通过；另有 1 个 compile-fail doctest、178 个 Python tests、Rust/Python cross-process system Harness 与独立复核（无 HIGH/MEDIUM）。GitHub CI `30601156490` 的 `rust-core` 与 `governance-and-tests` 均成功。该证据只把 P2d 的 local POSIX reference slice 标记为 complete，不把 public apply endpoint、目标平台 SLO、C++/production sandbox、durable journal 或 P2e 能力前置计入完成度。

### 5.1 目标文件目录

当前仓库已有 Python 3.11/uv 治理、独立 S2–S6 contract/worker 工具，以及由 Cargo 管理的 B1/B2/P2a–P2d local reference Rust 纵向切片。下列五个 crate 已建立；`paraegox-runtime-host` 是可启动但保持 idle 的 process root，canonical Loop/Thread/Process component 与 CoreService 仍是 crate-private Harness，不是公开 apply/assembly 路径。后续目录仍按真实消费者逐批准入，不因目标树而预建空包。

P0/P1 首批文件：

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml

crates/
├── paraegox-kernel/              # identity/digest 与 owner-local monotonic time 纯值；不读取 clock
├── paraegox-runtime-contracts/   # Slice/apply、PXTA/PXTE、PXAR complete request、signing/temporal/execution contracts
├── paraegox-runtime/             # private admission/reducer + Mailbox/Binding + Loop/Thread/Process/Liveness/Recovery；无持久化/apply endpoint
├── paraegox-runtime-host/        # idle executable process root + opt-in PXHW endpoint/POSIX reference service-manager；无 workload assembly
└── paraegox-deployment/          # private complete Slice/request projection；无 key/Controller/I/O

tests/
├── architecture/                 # cargo metadata crate-DAG rules
├── unit/
├── contract/
└── fixtures/wire/                # canonical valid/invalid/version vectors

pyproject.toml                    # governance + Python SDK/PXWP reference worker；不是 Python RuntimeHost
uv.lock
scripts/
└── check_governance.py
```

首批 crate 必须在同一个有界纵向切片中形成真实 producer/consumer 链：`paraegox-deployment` 从 committed plan identity 与 target assignment/execution commitment 投影 Slice，并构造完整 request；`paraegox-runtime` 消费经过 admission 的同一控制合同并拥有本地执行机制，`paraegox-runtime-host` 只提供 process/reactor root。它们只通过 `paraegox-runtime-contracts` 与 `paraegox-kernel` 共享的值协作。

该批称为 **B1 apply-control spine**，不冒充完整 P1，也没有提前发布字段残缺的 `RuntimePlanSlice` 或 `RuntimeApplyRequest`。B1 已冻结 exact target-slice CAS、revision 单调性、writer turnover/replay/recovery 不变量、proof-envelope fingerprint 与 commitment golden vectors。B2 已围绕现有 commitments 增加正式签名转录、canonical wire、authentication 与 temporal admission，并让 producer→verifier/clock admission→现有 reducer consumer 在同批闭环。P2a 在真实 Mailbox/Binding assignment consumer 出现后，现已组合 `RuntimePlanSlice { commitment, assignments }` 与完整 `RuntimeApplyRequest { envelope, slice }`；assignment digest 由 header 承诺、slice digest 覆盖 header，PXAR 不创建第二签名或 request digest。后续扩展不得绕开这条覆盖链。B1 的 Header、Commitment、Control 与 writer-fence/prepared/active reducer 必须复用或显式迁移，不建立兼容别名或第二状态机。

B1 明确不把 producer 侧绝对 `Deadline` 放入 apply control：不同机器或重启代次的 monotonic origin 不可直接比较。B2 已冻结认证的 original/remaining-budget temporal constraint，并由首次目标 ingress 安装到本地 clock domain；同 lineage 的后续 admission 不能延长已安装 deadline，generation mismatch fail-closed。这个合同没有 producer timestamp、target challenge 或受认证 clock mapping，因此不扣除签发至首次 ingress 的驻留时间，也不证明端到端 freshness。创建和持久推进 clock generation 的 RuntimeHost lifecycle owner 仍待后续阶段实现。

截至 S6 收口，`paraegox-runtime` 与 `paraegox-deployment` 仍作为 internal/enabler 准入；新增 RuntimeHost process/reactor、structured task、private ProcessDomain 与 external reference service-manager/watchdog 直接消费或验证现有 complete-request/ExecutionDomain seam，没有平行 desired state、第二 payload queue 或第二 Runtime。production DeploymentController、durable journal、public apply/assembly、Fabric/network route、NodeDaemon/production OS integration、allow-all verifier 与未来目录占位仍未建立。

P2e最多在真实DeckCompiler切片中新增一个`paraegox-decks` crate，Deck contract与Compiler合一；不预建`paraegox-cards`，Card fixture identity继续由现有contract/manifest引用。Physical contracts只随P3真实producer/consumer准入。任何逻辑owner在独立consumer、生命周期或发布边界成立前都不预建crate，也不建立`common/shared/core-utils`。

P2 按可运行纵向切片增加：

```text
crates/
└── paraegox-decks/               # P2e最多新增此一crate：DeckSpec/Lock/Compiler；无live Runtime owner

crates/paraegox-runtime/          # P2a–P2d：在既有纯 ApplyState 上扩展 RuntimeHost/Mailbox/Domain/liveness/recovery
crates/paraegox-runtime-host/     # P2b 起：唯一 process/reactor root；P2e 才接公开 apply/assembly
crates/paraegox-deployment/       # P2e：在既有 projector/builder 上扩展 Planner/Controller/tenure/ports/adapters

pyproject.toml
uv.lock
src/paraegox_sdk/
├── worker/                       # S6 已有 subordinate PXWP reference worker；无 lifecycle/retry/Zenoh owner
└── contracts/                    # 仅在 generated/validated binding 有真实消费者时准入；不成为第二 Schema authority

tests/
├── fixtures/wire/
├── contract/polyglot/
├── integration/runtime/
├── integration/deployment/
└── system/process_domain/
```

上图仍是 target slice，不是 P2 一次建立四个空 crate 的要求。P2a 已让 `paraegox-runtime` 消费 production builder 精确匹配的独立 runtime contract fixture；Python SDK/PXWP reference worker 已在 S6 建立，但只作为 subordinate reference worker，尚未进入 public apply/assembly。P2e 才准入 cards/decks/deployment 的实际 owner；若 colocated module 已足够，不为目录对称强拆 crate。

P4 才增加真实 `paraegox-fabric-zenoh` crate/adapter；P5 才增加 Node contracts、NodeDaemon 与双主机 partition test。P2e 开发/救援 CLI 只向 DeploymentController 提交 intent、触发 reconcile 或查询状态，不能绕过 DeploymentController 直接写 RuntimeHost；它是 bootstrap seam，不是完整 OpsService。

上图不预建 `kernel/graph` crate/module。P2e 先让 DeckCompiler 与 DeploymentPlanner 分别拥有 typed model、validator 和测试；只有同一 bounded batch 中证明存在两个独立生产消费者的纯算法交集后，才按 Accepted ADR 增加最小 internal Graph Foundation。若交集不足，计划必须取消提取，而不是为了目录对称创建空 package/crate。

P6a 有真实 node-local producer/consumer 时再增加 Inspection service owner；Runtime inspection module 只产生 Runtime-owned raw facts/snapshot，不成为第二个 federated projection owner。P6b 再增加 OpsService、OpsProtocol/clients 与 crash-consistent ControlRequest journal，不在 P0 创建空包或 crate。

Operator/Web workstream 只有在 W0 冻结 managed Gateway/external exposure/internal typed endpoint 合同，且出现对应生产者、消费者与测试入口后，才按切片增加以下目标目录；不在 P0 创建空框架：

```text
apps/
└── console/                       # Web Console + WebXR client Artifact

gateways/web/                     # 物理 crate/package 由切片语言与 owner 决定
├── console/                       # HTTP/SSE/WS → typed InspectionClient/OpsClient
└── realtime/                      # 候选组合；media 与 XR input roles 逻辑分离
    ├── sessions/                  # 限定 browser/peer/XR refs，不建立泛 SessionId
    ├── signaling/
    ├── webrtc/
    └── xr_adapter/

tests/
├── contract/gateways/
├── component/gateways/
├── integration/gateways/
├── scenario/teleoperation/
├── system/browser/
└── bench/realtime/
```

没有多 viewer、共享转码/录制或跨 Deck teleoperation state 的真实消费者前，不创建 Media/Teleoperation/Session Service、`runtime/io` 或 `IORegistryService`。Web/API、WebRTC/codec 与 browser test 依赖按实现语言进入明确的 Cargo feature 或 uv dependency group，不污染 Kernel/Runtime 默认环境。

## 6. P1：Kernel、Deployment/Runtime、Physical Contracts 与 Time

### 结果

实现小型、不可变、无 I/O 的值与纯规则：

- 身份：`NodeId`、`RuntimeHostId`、`DomainInstanceId`、`InvocationId`、`InstanceId`、`MessageId`、`BindingId`、`PrincipalRef`。`DeckRunId`、`AgentRunId`、`EvalTrialId` 由各领域定义，不建立泛 `RunId`。
- 基础/运行分代按 owner 分包：Kernel 只提供必要的通用 ID/digest/time/fencing 构件；`DeploymentScopeId`、`DeploymentId`、`DeploymentRevision`、`DeploymentWriterRef` 与 `DeploymentWriterEpoch` 属于 `deployment/contracts`；`RuntimeHostEpoch`、`DomainEpoch`、`BindingEpoch` 与 Invocation fencing 属于 `runtime/contracts`；`NodeIncarnation` 属于 `node/contracts`；`FabricSessionEpoch` 属于 Fabric contract；`LeaseIssuerEpoch` 属于 resource contract；DeviceIncarnation/DeviceSessionEpoch/DriverBindingEpoch 等属于 `physical/contracts`。任何领域 revision 都不因 Foundation 计划而下沉 Kernel。
- 失败：最小 `Failure`/`Problem`，包含稳定 code/class、origin、stage、correlation、retry/reconcile hint 与 public-safe detail；不建立中央巨型错误注册表。
- 关联：`TraceContext`、`CausalityRef`、`SchemaId`、Schema version/range/hash 与兼容性结果。
- 时间：`MonotonicInstant`、`WallInstant`、`SimInstant` 不可互换；Clock protocol、`ClockDomainRef`、ClockQuality/uncertainty、`ClockMapping/ClockMappingRevision`（含 producer identity/epoch、measured-at/valid-until/source/uncertainty）、`Deadline`、`Freshness` 和测试虚拟时钟。Kernel 只拥有 Schema/纯规则；Node time-sync、Device/Driver 或 ScenarioRunner 分别拥有其源时钟 mapping value。
- 消息：`Message` 是 transport-neutral 的不可变逻辑 Envelope；发送侧只从通过 Schema/Port 校验的 payload 构造，接收侧只在 decode 与 Schema/principal/binding 准入成功后构造；它带 MessageId、causality、deadline、trace context，pre-validation encoded frame 不是 Message；`Signal`、`Event`、`Command`、`Query`、`Receipt` 使用公共头部；`messaging` 只是实现子系统名。
- 端口与交互：`PortSpec` 的 name/direction/schema/interaction/cardinality/required 与不可削弱约束；P2 首版执行静态 1:1 Signal/Event，P1 同时冻结供 P3 使用的静态 1:1、bounded outstanding、无透明 retry 的 Command interaction、typed `OperationClient/CommandEndpoint`，其他 interaction 明确 unsupported。
- 执行与交付：`ExecutionRequirements`、`DeliveryProfile`、`MailboxSpec` 和结构化 `EnqueueResult` 的最小不可变语义；`Mailbox` 是目标异步边界唯一的有界 Message backlog/admission owner，只容纳已验证 Message，名称不改。
- 准入与有界性：workload arrival/payload envelope、run-bound provenance、`max_inflight`、OutstandingBudget、IPC credit 和 retained-byte 语义。
- 大 payload：`BlobRef/BufferRef` 的 size/schema/media、producer epoch、digest、retain/release、expiry/lease、copy/zero-copy 与 ownership；不引入 VFS/ObjectRef。
- 运行超时：ingress/queue/run/effect/cleanup 阶段预算、overrun action、`CancellationScope` propagation/ack/escalation、Timer missed-tick policy 和跨故障域 remaining-budget 安装。
- 判活、故障约束与恢复：`LivenessSpec`、`FailureContainmentSpec`、`RecoveryPolicy`（区分 RestartPolicy 与 InvocationRecoveryPolicy）、side-effect class、restart-safe 条件、RuntimeFailureFact/effect Receipt owner 分离，以及 `prepare → activate → drain → retire/rollback` 纯状态机。
- Deployment/Runtime seam：`deployment/contracts` 定义 DeploymentPlanCandidate、committed DeploymentPlan/Revision 与 DeploymentWriterRef/DeploymentWriterEpoch 领域值；`runtime/contracts` 独立定义 source-only `PlanProvenance`、`PlanWriterRef/PlanWriterEpoch/WriterTenureProof/PlanWriterContext` DTO，以及分阶段完成的 RuntimePlanSlice/RuntimeApplyRequest/apply Receipt。source plan digest、target slice digest、writer fencing、exact-active-slice CAS、operation id、temporal constraint 和 tenure proof 的比较规则在无 I/O 测试中冻结；source revision 单调性不能替代 exact CAS。
- Digest 覆盖面：PlanContentDigest 只覆盖 candidate desired content；DeploymentPlanDigest/source_plan_digest 覆盖 committed scope/plan/revision/content；target_slice_digest 覆盖 PlanProvenance/target assignment。三者都排除 writer tenure、rollout/observed facts 与 request controls；request auth 覆盖完整 Slice、PlanWriterContext、exact expected-active slice、operation id 和 temporal constraint。proof-envelope fingerprint 包含 signature bytes，只标识完整 envelope；B2 的签名转录是独立 canonical contract。
- 最小 producer 链分批落地：B1 的 `RuntimeSliceProjector` 只产生 tenure-neutral commitment，apply-control builder 映射 writer context、exact CAS 与 operation id；B2 增加 canonical signed envelope、正式 verifier 与目标 clock admission；P2a 在真实 assignment consumer 出现时才组合完整 RuntimePlanSlice/RuntimeApplyRequest。相同输入字节稳定，未知或缺失 assignment fail-fast；DeploymentController restart 的新 writer tenure 不改变 plan/slice digest。P2a fixture 必须调用这条 production projection/build path，不得手写第二种 Slice Schema。
- 授权、服务与支持：`CapabilityScope/CapabilityGrant` 纯值和验证输入、`ServiceContractId/Requirement`、限定 FeatureRequirement/Report；三者不共享基类，签发和 observed state 不在 Kernel。
- 配置引用：Deployment/config contract 的 `ConfigSnapshot` digest，以及分别位于 artifact/secret owner contract 包的 `ArtifactRef`、`SecretRef`；Kernel 只提供通用 ID/digest，不解释内容或聚合这些 ref。
- 物理基础：Kernel `time/` 唯一定义 ClockDomainRef、分型 Instant、ClockQuality、ClockMapping/ClockMappingRevision；独立 `physical/contracts` 定义 `DeviceRef`、`FrameRef/frame epoch/transform revision`、单位/维度、measurement uncertainty、`CalibrationRef`、simulation/real origin并引用 Kernel time，不包含 FrameGraph/World/Map engine。
- Observation ABI：冻结 immutable trusted header，至少含 device/incarnation/session、driver binding、channel/sequence、measured/received time、clock domain/mapping revision/uncertainty、frame/frame epoch/transform revision、unit/dimension、CalibrationRef/CalibrationRevision、quality/validity/covariance、origin/environment/provenance；只有受信 Driver/Scenario boundary 可以标记 physical origin。PortBinding 只能添加/验证 transport metadata，不能把 CardInstance 所发 payload 提升为设备 provenance。
- 资源基础：Kernel `resources/` 拥有通用 `ControlledResourceRef`、`ResourceSetRequirement`、Lease/Fencing 纯契约；不认识 Device、Operation 或 Safety。
- 物理控制（位于 `physical/contracts`）：`Operation`、`PhysicalCommandEnvelope/EnforcementContext`、`DeviceIncarnation/DeviceSessionEpoch/DriverBindingEpoch`、`PhysicalAssemblyRevision`、`CalibrationRef/CalibrationRevision`、`ControlModeEpoch`、`CommandSequence`、`SafetyEpoch`、operating-fact refs；H1 扩展包含 `HardwareActivationRef/Epoch`。这些类型只引用 Kernel 的 resource/Lease/Fencing/Receipt 基础值。
- 判定：`AuthorityDecisionRef/digest`、`SafetyDecisionRef/digest`、`LeaseGrant`、reset/inhibit 与稳定 reason code。Decision 绑定 command/operation digest、resource、device/session、audience、policy/epoch 和 expiry；不能只绑定 policy revision。
- 设备 readiness：`DeviceReadinessSnapshot` 原子绑定 desired assembly/firmware/config 与 observed device session/firmware/ABI/config、Driver binding、calibration、safety/mode 的全部输入 revision，并带 derived-at/valid-until；任一输入换代使旧 snapshot 失效。
- Agent protocol seam fixture：在 `agent/contracts`/tool contracts 中区分 ToolCall、ServiceCall/Query、OperationSpec、OutcomeRequirement、OutcomeClaim、VerificationSpec/Result，与 Kernel Invocation/Deadline/Receipt 组合；不把 AgentSession/Harness/Verifier 实现放进 Kernel。
- Receipt 阶段：`Accepted`、`Rejected`、`Started`、`Succeeded`、`Failed`、`Uncertain`；physical EffectReceipt 另记录 requested/authorized-permitted/applied operation digest、SafetyEpoch、device-send ack、device completion ack 与 observed-effect evidence level。
- 执行终态与中间态：`Expired`、`Evicted`、`CancellationRequested`、`CancelledCooperatively`、`Wedged`、`Terminated`、`Killed`。

### 关键规则

- 公共 Envelope 与领域 payload 分离。
- CardDefinition 声明 Port；Card 只引用并配置 CardDefinition；Link 连接 `Card.Out → Card.In` 并拥有本次 DeliveryProfile。Topic/key、queue、codec、route 和 live endpoint 不进入 PortSpec。
- bind 前验证 direction、interaction、cardinality、required 与 Schema compatibility；转换只能来自显式、版本锁定的 Adapter，任何 route locality、test fixture 或经 ADR 准入的优化都不能绕过契约。
- `CapabilityScope` 是 CapabilityGrant 内的 resource selector + operations 值；它与 ServiceContract、FeatureReport 分离。
- Command 必须携带 deadline、idempotency key、目标资源与操作。是否要求 Authority/approval/Lease/Safety/Enforcement 由 Operation effect class、resource contract 与 DeploymentPolicy 编译，目标 Port/Endpoint 只能加强、不能 opt-out；write/physical Command 必须经过完整链，非物理 Command 使用明确的 effect class。
- 每种 epoch/revision 对应一个 owner 和失效事件，不能用泛 `epoch` 互相替代。
- Receipt 的 owner 是做出判定或执行副作用的组件，不是中转 Bus。
- `send()`/enqueue accepted、remote admission、Card invocation completion 与 physical effect 是不同阶段；API 与 Receipt 不得把前一阶段成功冒充后一阶段成功。
- RuntimeHost 只记录 Domain/Process 的 RuntimeFailureFact；内部 RecoveryEngine 消费事实并求值 RecoveryDecision，不成为第二 owner。已 handoff 但缺少副作用终态证明的 Invocation 必须是 `Uncertain`，不能伪造 `Failed`。
- CardDefinition 只声明内在 ExecutionRequirements，Link 声明 DeliveryProfile；完整执行真相只存在于编译后的 `DeploymentPlan.execution`，不建立可由用户并行编辑的完整 ExecutionContract。
- `unknown` blocking/native risk 不得乐观进入 LoopDomain；Card/Deployment 只能加强 minimum isolation，不能削弱。
- control/high criticality 需要 DeploymentPolicy 授权与容量 reservation；arrival/run bound 为 unknown 时不得承诺 control SLO。
- TraceContext 不携带 secret、token、原始 prompt、音视频或 CapabilityGrant 内容。
- Liveness、Health、Readiness 与 Feature availability 分开；heartbeat 不能证明当前 revision Ready。
- 配置变化产生新 DeploymentRevision；首版不在活跃实例上原地 hot reload。
- 不同 clock domain/type 的 timestamp 不能直接相减；lease/deadline 只使用 owner-local monotonic。`unknown uncertainty` 不等于零；单位、frame/transform revision、calibration 或 uncertainty 不满足消费者要求时 fail-fast 或显式 degraded。
- 多资源 Operation 使用 canonical ordering、同 owner all-or-none claim 或显式 prepare/abort；跨故障域不宣称原子事务，必须有 deadline、补偿与 deadlock diagnosis。
- DeviceRef 标识物理设备/接口，ControlledResourceRef 标识可 lease/fence/write 的控制资源；二者不继承、不互换。PhysicalAssemblySpec 是 desired Device↔Resource mapping 的唯一声明，DeploymentController 选择/激活其 revision，DeviceService 只报告 observed realization/readiness。
- `Controller-role Card`（控制应用）只能通过计划安装的 `OperationClient/CommandEndpoint` 提交 Command；它不得持有 Driver/Actuator 对象或绕过 Authority/Lease/Safety/Enforcement。P3 之前 fan-out、partial acceptance、动态 routing 和跨资源 transaction 均明确 unsupported。
- Mode handoff 必须执行 request→stop/quiesce→observed-safe/neutral→release old lease→acquire new lease→activate new ControlModeEpoch；CommandSequence 只在 resource + lease issuer/lease + mode epoch 内比较，并为 duplicate/gap/superseded Command 产生唯一终态。

### 验证与证据

- frozen/immutable、schema round-trip、未知字段与版本不兼容行为；Rust encode→Python decode、Python encode→Rust decode 的 canonical bytes/digest/error golden vectors 必须一致，生成类型不成为第二 Schema authority。
- Port direction/interaction/cardinality/required/schema compatibility 的表驱动测试；不支持的 Call/Operation/fan-in/fan-out 在编译或 bind 阶段 fail-fast。
- monotonic deadline 不受 wall clock 回拨影响。
- sim clock 暂停/步进/倒跳、device clock 漂移与同步质量下降时，freshness/uncertainty 结果可确定且可观测。
- 远端 monotonic timestamp 不被直接用于本地 lease expiry 比较。
- ID/epoch/revision mismatch、旧 DomainEpoch/InvocationId late-result 与 fencing-token total order 的纯比较规则；真正的重复去重、旧 lease/token 拒绝和重启恢复留到 P2/P3/P5 的 stateful Harness。
- offer outcome 与 admitted lifecycle 的分层守恒；queued/inflight/terminal 是互斥状态，evict/coalesce 为被替换 MessageId 产生独立终态。
- revision transition 的 activate 失败、rollback、旧 revision 迟到和终态比较使用纯 state-machine 验证。
- DeploymentPlanCandidate 与 committed DeploymentPlan 不能互相反序列化冒充；candidate/content digest 稳定，commit header/revision 改变只影响 DeploymentPlanDigest，writer tenure 改变不影响 plan/slice digest。
- PlanWriterContext/proof 的 scope、writer ref、epoch、authority/key/version/signature mismatch fail-fast；proof-envelope fingerprint 与签名转录不能混用；Runtime apply 纯状态机分开 writer_fence、prepared 和 active，prepare 不能提前改 active。
- CapabilityGrant audience/scope/revision/expiry/delegation 的负向测试；Service/Feature 不兼容分别产生稳定错误。
- Blob/Buffer 在 queue/inflight/IPC/late-result/release 的 property test 中不泄漏、不 double release，retained bytes 守恒。
- Observation trusted header 的 device/session/binding、frame/transform、unit、clock mapping、calibration、origin 与 uncertainty 不匹配时失败；unknown uncertainty 不作零处理，simulation origin 不能满足真实硬件 readiness。
- Command contract probe 证明 bounded outstanding、deadline、idempotency、阶段 Receipt 与无透明 retry；任何 Controller-role Card→Driver 直接调用由依赖/接口测试阻止。
- Command A 的 Authority/Safety Decision 不能用于 Command B；Safety clamp 后 requested/applied digest 不同且结果不冒充原请求完全成功，SDK return 不能直接产生 `Succeeded`。
- DeviceReadinessSnapshot 不允许把旧 session presence、新 calibration 和错误 firmware/ABI 拼成 Ready；任一输入 revision 换代立即 invalid。
- 对声明需要 ODD/thermal/power 的 Operation，SafetyDecision/EnforcementContext 中任一 operating fact 过期或换代时拒绝；real profile 的 HardwareActivation/Readiness 在 decision 后失效也必须在本地写入前拒绝。
- contract probe 证明 ToolResult、EffectReceipt、OutcomeRequirement、OutcomeClaim、VerificationSpec/Result 和 Eval outcome 不能互相冒充；ContextManifest 从第一次 fake model invocation 即存在，VerificationSpec 不能弱化 Requirement。
- 纯单元测试不真实 sleep、开线程、连网或写磁盘。

## 7. P2：RuntimeHost、ExecutionDomains 与 PortBinding fixture

P2 是一个阶段组，不是一批同时落地的并发设施。进入 P3 前的硬顺序固定为 P2a Mailbox → P2b Loop → P2c Thread → P2d Process → P2e 最小 Deployment control plane；每一步只有通过自己的故障 Harness 后才能成为下一步依赖，不能以 Runtime fixture 已通过为由跳过 P2e 的 DeploymentController/tenure/journal 闭环。

### 7.1 P2 共同所有权规则

- `RuntimeHost`、`ServiceSpec`、最小 `ServiceContext` 和 `CoreService` Protocol 不持有领域 Service Locator。
- Rust RuntimeHost 是 production execution 的唯一 lifecycle/resource owner；Python/C++ worker 只是 subordinate executor，不能拥有第二语义 Mailbox、raw Zenoh session、self-restart、readiness authority 或并行 recovery loop。
- ServiceContext 分开提供 resolved `service_clients`、declared `port_bindings` 与 permission-bound `access_handles`，不建立一个模糊 `declared_bindings` registry。
- RuntimeHost 不启动或拥有远端实例，也不把 external service 当成本地子进程；调用远端服务只产生 typed service client/permission-bound handle，不创建第二种 live Binding 类型。
- `runtime/contracts` 拥有 source-only PlanProvenance、PlanWriterContext/WriterTenureProof wire DTO、RuntimeApplyRequest/RuntimePlanSlice Schema 与 apply state machine；DeploymentController 拥有唯一 committed DeploymentPlan/revision，并通过纯 Slice projector + apply envelope builder 生成请求。RuntimeHost 只应用带 opaque source-scope/source-revision/writer-ref/writer-epoch/target/expected-active/source+slice digests/operation id/deadline/tenure proof/auth proof 的请求，不能 import `deployment/`、`decks/` 或具体 Service/Card 实现。
- RuntimeHost journal 的 `writer_fence`、`prepared`、`active` desired head、`live_materialization`、`recovery_action` 与 `owned_resources` 必须分开。接受更高 writer tenure 使用独立 tenure-only transaction：未跨 `FirstActionIntent` 的旧 prepared 才可 `SupersededBeforeEffects`；已跨或可能跨副作用边界的旧 operation 进入 `SupersededReconcileRequired`，owner-wide action gate 阻塞新 effect 直到 exact-zero terminal 或 quarantine。full request admission 另行原子提交 request/temporal state、per-source revision high-water、exact request/Slice 与 `PreparedNoEffects`。normal apply随后只能以durable `FirstActionIntent`越过effect boundary；restart recovery必须先写`RecoveryPlannedNoEffects`，再以独立durable `StartCallIntent`越界。`OneSourceLoop`仅在readiness成功时同时提交desired head、`LiveReady`与terminal；`EmptyDeactivate`只有在live/nonzero generation时先以同一事务提交`FirstActionIntent`、`NoNewAdmission`、canonical empty head与`HeadCommittedRetiringOld`，再drain到exact-zero terminal，already-exact-zero且无action/resource则走无intent/callback的单事务fast path。
- PortBinding handoff/producer 只 bind 和 enqueue；Card 实现 callback/invocation 只由所属 ExecutionDomain 调用。
- 每个异步边界只有一个系统语义 Mailbox；Runtime 内部 dispatch group 不建立第二份 payload queue。
- 系统有界覆盖 queued、dispatched/running、executor/IPC credits、child work 和 retained payload bytes；无 execution permit 不 dequeue、不创建等待 Task/Future。
- `Lane` 不进入 CardDefinition、Card、Deck、Kernel 或公共 Runtime Schema；禁止 `thread_lane`、`process_lane` 和一 Card 一线程。
- 一个 Mailbox、Task、Thread、Process 和 live PortBinding 只有一个 lifecycle owner。
- 一个 CardInstance 默认拥有一个私有 Card 实现对象；除非未来通过显式 SharedService 契约设计，不允许跨实例共享可变实现状态。
- RuntimeOwnershipTree 固定为 `RuntimeHost → DomainInstance → Card/ServiceInstance → InvocationScope`；它是 assignment/observed ownership 的结构而非第三份 desired graph。child work 不得越过 scope/epoch/revision 存活，Domain 共置的 collateral restart 范围进入 FailureContainmentSpec 与 Inspection。
- `DeploymentPlan.execution` 是 desired execution 的唯一权威。S7 只允许 digest-covered `ReferenceAssemblyProfileV1::{OneSourceLoop, EmptyDeactivate}`，exact target manifest projection、zero/one LoopDomain+LoopSubject 和 zero-binding PXTA进入 PlanContent/Slice digest；没有一般 Mailbox/capacity/activation/Ingress branch。未来 successor 若允许一般 assembly，才必须把 typed activation dependency、readiness/activation group、consumer ingress、producer egress、dependency-loss 与 drain contract完整写进Plan/Slice；Runtime observed facts与其不一致时不能 Ready。
- Deck DataLink 不等于 ServiceDependency 或 activation dependency。RuntimeHost 内部 RuntimeAssemblyEngine 只执行 authenticated target Slice 的 profile-specific lifecycle，不从 Link 重算全局语义、不持有第二 desired graph，也不进入 steady Message hot path。S7 只有 idle Loop 的 bounded start/recovery 和 empty-head-first drain/retire；一般 prepare/readiness/ingress/egress/rollback图留给后继 contract。
- P2 即产生 raw Mailbox、dispatch、loop、thread、process 和 resource facts；P6a/P6b 只做聚合、持久化和导出。
- P2 不启动 Zenoh；确定性 PortBinding test fixture 只把已验证 Message 直接 offer 到相同 target Mailbox，其 conformance suite 在 P4 被 Zenoh production routes 复用。fixture 不进入公共 API 或 DeploymentPlan production route；encoded frame、malformed wire payload 与 ingress validator 属于 P4 Fabric conformance。
- P2 的测试入口分层：普通实现对象单测不产生运行身份；internal single-subject Harness 只消费 production projector/builder 生成的 canonical request 并由 RuntimeHost 创建 CardInstance；P2e 的 one-subject Deck 才证明正式 desired-state 链。fixture/golden Slice 不能成为 production source、公共 route 或可编辑运行配置。
- startup completion、RuntimeHost/Domain liveness、exact-revision readiness、ongoing health 与 test observation 使用不同 facts/truth table。test evidence 固定为 local/diagnostic；同 event loop 自报不能证明自身 liveness，endpoint/heartbeat 不能单独证明 Ready。
- P2 Harness 强制 effect-denied 且没有 simulation/HIL/real 配置开关，不取得真实 Device Grant、Lease、SafetyDecision 或 HardwareActivationRef；simulation proof 不能升级为 H1/real evidence。
- RuntimeHost 只依据当前 committed Slice 中编译好的 readiness contract 判定 CardInstance Ready；CardDefinition 只是 Planner 的上游声明来源，不能被 Runtime 读取为第二 desired truth。
- P2 不原地修改活跃执行对象。S7 不支持 Loop→Loop replacement：必须以更高 revision 提交 `EmptyDeactivate` 并取得 terminal exact-zero，再以另一更高 revision启动 `OneSourceLoop`；一般 replacement/rollback需要后继 assembly contract。
- Service dependency 在 Ready 后丢失时的 degrade/stop/rebind/restart/fail-closed 与防抖属于后继一般 assembly profile；S7 fixed idle fixture没有 ServiceDependency 或 client。
- P2 使用 fake Agent/fake Tool contract probe 验证 semantic step/tool/token/cost budget 归 Agent 层、CPU/RSS/wall/task/process 限额和 cancellation delivery/escalation 归 Runtime；Sandbox/Security owner 提供 profile/egress/Secret proxy policy，Runtime 只实例化执行。

### 7.2 P2a：Mailbox 与 PortBinding test fixture

> 实现状态：S3 提交 `521cf8d` 已完成本节的 bounded Mailbox、canonical complete request 与 deterministic PortBinding gate；CI `30522872591` 成功。它没有 RuntimeHost、Card callback、Receipt owner 或可启动进程。

#### 结果

- Typed `PortSpec`、`SchemaRef`、`DeliveryProfile`、`MailboxSpec` 和结构化 `EnqueueResult`。
- 两个 opaque Instance fixture、静态 1:1 Signal/Event assignment、同 revision 的 canonical target assignments、投影后的 RuntimePlanSlice 与 live `PortBinding`；最小 CardDefinition/Artifact 与真实 CardInstance 由 P2b 首个 internal single-subject Harness 同批引入，不在 P2a 用假作者 API 占位。
- items、bytes、max age 三重有界 Mailbox。
- `queued → inflight → terminal` 状态机、OutstandingBudget permit 和 payload-handle 单 owner/不可变/size/release 规则。
- `latest`、`coalesce`、`drop_oldest`、`reject_new` 和受限 `block_until_deadline` 等显式压力策略。
- 不依赖 Zenoh 的确定性 PortBinding test fixture：使用已验证 Message 验证 install、enqueue、关闭传播和只读 Inspection；它不运行 Card 实现 callback、不拥有第二份 payload queue，内部实现可叫 `FakePortBinding`，但不形成产品术语。
- `stop accepting → drain/expire → close` 的 Mailbox 生命周期。

#### 验证与证据

- Signal 洪峰按声明合并；合成 Command Message 满或过期时明确拒绝并返回测试夹具 outcome，但不产生 Receipt。该测试只证明 Message/Mailbox 压力语义，不代表 P3 `OperationClient/CommandEndpoint` 或 Receipt owner 已实现。
- Transport callback、Runtime control 和 Safety producer 不能使用阻塞 enqueue。
- items、bytes 和 age 不越界；`offered = admitted + rejected + closed + expired_before_admission`，已准入 cohort 另以 `queued + inflight + terminal` 守恒，不重复计数中间转移。
- executor/IPC 暂停接收时，无 permit 的消息仍留在 Mailbox，不出现 detached Task、第二 backlog 或 retained-byte 泄漏。
- 关闭后 enqueue 明确返回 closed；Mailbox conformance 可被后续 Runtime/Fabric 复用。
- direction/schema/interaction/cardinality 不兼容时不安装 binding；两个相同 CardDefinition 的 CardInstance 不串消息、不共享实现对象或 BindingId，各自 BindingEpoch 只在所属 BindingId 内推进且数值允许相同。
- pure compile 不递增 BindingEpoch；只有 Runtime/Fabric install、reinstall、reconfigure 或 revoke live PortBinding 才递增。
- 不安装/import deployment/decks 包时，Runtime 仍能应用序列化 RuntimeApplyRequest fixture；错误 target/revision/expected-active/source digest/slice digest/operation id/deadline/writer tenure/auth 必须在 prepare 前拒绝，Runtime 能重算 slice digest，但不假装由局部 slice 重算完整 plan digest。
- 同一 DeploymentRevision 与活动 BindingEpoch 下，每个 BindingId 最多一个 fixture route 接收新 Message；替换按 `prepare → activate → drain → retire` 完成或 rollback，不出现双 active、双投、implicit fallback 或 hash/time-window echo 去重。
- P2a 明确没有执行 Card callback、startup/readiness 或 recovery；测试报告不能把 binding/Mailbox fixture 通过标记成“Card 已独立运行”。

实际完成证据：PXTA/PXAR production builder 与独立 Python fixture exact match，Runtime 反向消费同一完整请求并贯通 admission/fence/prepare/two-binding offer；118 个 Rust unit、1 个 compile-fail doctest、43 个 Python test 与三路独立终审全绿。`InflightToken` 的 abandoned cleanup 只在 payload owner 已释放且 Mailbox 停止接收后产生 `Uncertain`；不持有原 payload 的 late-generation completion fence 已由 S4 独立实现。

### 7.3 P2b：LoopDomain 与 Dispatcher

#### 结果

- P2 reference profile 每 RuntimeHost process 一个受控 Rust async runtime/reactor；Runtime scope 使用结构化 task registry/cancellation tree 拥有并 join 所有 Task。这不是永久公共数量限制，Tokio Handle/JoinHandle/channel 不进入公共合同。
- 每个 LoopDomain 一个 dispatcher owner，从 ready Mailbox 仲裁，不建立全系统中央 Dispatcher。
- deadline/freshness-before-run、priority/fairness、最小服务份额和 max burst。
- admission 将 sync blocker、CPU work、unknown native/device 实现拒绝出 LoopDomain。
- dispatcher 先取得 outstanding permit 再原子地 queued→inflight；S4 实现 run 与 invocation-cleanup budget、cooperative cancel/escalation/uncertain。production effect stage 与 Receipt 尚未实现，output proposal 当前只在 Harness 中被丢弃。
- control/high criticality 必须经 policy 授权和容量 reservation；不可行 utilization 在启动前失败。
- `block_until_deadline` 禁用于 Transport/control/safety/shutdown、持有 exclusive ResourceClaim 和 Deck 有环 Link 的路径。
- `RuntimeHost` process/reactor root 与固定两 CoreService 的预验证 provider→consumer 启动、ready、排空和逆序关闭；通用 ServiceSpec DAG、Deck 生命周期、Inspection snapshot 与 public apply/assembly 尚未实现。
- 首个 internal single-subject Card component Harness：从 production projector/builder 获得 canonical RuntimeApplyRequest，由 RuntimeHost 通过 P2a 已有的最小 apply seam 创建 subject CardInstance；P2b 只验证 CardInstance/LoopDomain callback，不依赖或声称完整 RuntimeAssemblyEngine。source/sink test adapter 只在已安装 PortBinding 边界负责 typed stimulus/observation，不是 Card，也不形成公共 runner 或第二 desired owner。
- 普通实现对象 callback adapter 与 L1 unit fixture；作者可直接构造两个实现对象并注入 virtual clock、cancellation、录制型 Port handle 和 strict typed fake，但该层不产生 CardInstance/Ready 结论。

#### 验证与证据

- sync sleep、CPU spin 和 unknown native fixture 无法进入 LoopDomain。
- 2× stream/bulk 洪峰下，control enqueue-to-start p99/p99.9 满足由最小业务 deadline 推导的门槛，background 不永久饥饿。S4 当前证据是本机 current-thread/manual-owner-turn 真实单调时钟诊断，不是目标平台、并发 ingress、post-idle wakeup 或硬实时证明。
- direct dispatch 与内部多级 ready queue 做 A/B；S4 已有 deterministic scheduler-tick A/B 与本地真实链路诊断，但目标平台 live A/B 仍须在生产 ingress/profile 出现后补证。
- shutdown race 不遗留 Task 或 Mailbox；同 loop 永久阻塞不被错误宣称仍可 Inspection。
- Harness 覆盖 RuntimeHost-owned create/start/callback/drain/stop seam、on_start failure、callback timeout、late old-generation output；P2e 只补固定 idle fixture 的 authenticated apply、bounded `on_start` readiness、desired/live atomic commit、restart reassembly，以及 empty-head-first drain/exact-zero。一般 `prepare → readiness → ingress/egress → replacement/rollback`、生产输入与持续 dispatch不属于S7。S4 关闭后可跟踪 Task/Mailbox/permit/retained payload 全部归零；workspace/process-tree 清理属于 P2d。
- S4 只证明 `startup != Ready`，并保留/fence 当前 source revision、slice digest、generation/DomainEpoch、Artifact/config 与 binding facts。P2e 的 exact-revision readiness 仅对 manifest-pinned、canonical-empty-config、zero-binding idle fixture成立；required dependency/resource/permission 的一般合取与 truth table仍等待后继 contract，不得以空列表或一个布尔值伪造完成。

S4 实现证据：提交 `451f5e2` 与 GitHub CI `30534371196`；canonical PXTE/PXAR v2 跨 Rust/Deployment/Python fixture；212 个 Rust unit、1 个 compile-fail doctest、63 个 Python test；完整 Cargo/uv/governance 门禁；idle RuntimeHost 启动后 Ctrl-C exit 0；独立终审无 blocker/high。静态 Control start bound 以 producer 遵守 signed arrival envelope 为条件，Runtime 尚无 sliding-window observer；该限制与目标平台 live A/B 一并作为后续证据项保留，不冒充 P2b 的无条件硬实时保证。

### 7.4 P2c：ThreadDomain 与 ExecutorBudget

#### 结果

- 少量 RuntimeHost-owned、有界同步 executor class；提交前取得 permit，不使用隐式 default executor backlog。
- worker 不创建私有 async runtime/event loop；Card 实现代码不直接创建线程。ThreadDomain 使用显式有界 pool/permit，不把 Tokio default blocking pool 或 `spawn_blocking` queue 当作系统有界性。
- `cancellation_requested`、`uncertain`、`wedged` 与 late-result fencing。
- planned/observed ThreadBudget，包括 Zenoh、OpenMP/MKL/OpenCV/Torch 和设备 SDK 内部线程。

#### 验证与证据

- 永不返回、pool saturation、caller timeout 和取消后迟到均不产生假 `cancelled/success`。
- 旧 InvocationId/DomainEpoch 结果不能写状态或产生副作用。
- 线程总量不随 Card/Input 数无界增长；observed native pool 超预算时 admission/degraded 可解释。
- ThreadDomain 卡死只能报告 `wedged`；需要硬终止的工作必须在 ProcessDomain。
- wedged worker 继续占用 permit/capacity，不得无预算补线程；Domain/executor 进入 degraded/poisoned，直到 callable 真实返回或 RuntimeHost process 重启。
- L2 Harness 复用同一 subject fixture 覆盖 thread queue/admission/execution/teardown 分阶段 deadline；timeout 后仍运行的 worker 不得冒充已取消，所有可跟踪工作最终 join 或进入可解释 wedged state。

S5 实现证据：提交 `f6cf31a`；canonical PXTE v2/PXAR v3 跨 Rust/Deployment/Python fixture；276 个 Rust unit、1 个 compile-fail doctest、103 个 Python test；完整 Cargo/uv/governance 门禁；idle RuntimeHost Ctrl-C exit 0；两路独立终审无 blocker/high；GitHub CI `30542046220` 的两个 job 均成功。当前可执行 component profile 仅支持单 subject、单 worker、零 native thread；更宽 worker 与非零 native pool 只存在于旧 desired-state contract，Runtime admission fail-closed。S7 的 PXTE v4 不携带 Thread executor budget或general capacity；它必须让exact allowlisted fixture的构造、销毁和cooperative `on_start/on_stop`都由LoopDomain owner计费/持有，且返回后不得spawn/detach、输出或获得tick。异常无法交接给 owner 的 Drop 显式保留泄漏至进程恢复，不释放或伪造 proof。

### 7.5 P2d：ProcessDomain、Liveness 与 Recovery

#### 当前实现落点（2026-07-31）

S6 已完成本节的 **local POSIX reference slice**：PXTE v3/PXAR v4 Process desired/wire contract、PXWP v1、Rust/Python byte-level conformance 与 reference worker、private/experimental Runtime-boundary sole-owner ProcessDomain mechanism、generation/sequence fencing、单 invocation credit/retained-byte ownership、worker Ready/Constructed startup handshake、heartbeat/liveness、RuntimeFailureFact/RecoveryEngine、`Uncertain`/no-replay、cooperative stop→TERM→KILL、same-process-group descendant cleanup、workspace identity cleanup、exact-zero cleanup proof、Linux `/proc` 聚合资源 census，以及 PXHW v1 + 独立 POSIX service-manager/watchdog 的 RuntimeHost process-group/restart/backoff/quarantine reference owner。ProcessDomain 尚未由公开 RuntimeHost executable assembly 消费。主提交为 `b89f283`，Linux 收敛到 `861d436`，CI `30601156490` 成功。

以下“结果/验证”仍是完整 P2d 目标，不能整体当作已实现。当前 launch resolution 是 trusted caller seam，signed sandbox/target profile 尚无 production adapter 强制执行；Linux census 不是 cgroup containment，非 Linux 不宣称 live resource enforcement；只证明同 process group descendant。RuntimeHost 被不可捕获 SIGKILL 后，独立 process group 的 ProcessDomain worker 没有外部枚举/清理 owner；unexpected Drop fallback reaper 未注册、未 join、不能签发 cleanup proof；PXHW 只证明 exact-generation reactor progress，不是 readiness、ProcessDomain receipt 或 recovery receipt。C++、cgroup/job-object/pidfd/full sandbox、GPU/device reset、durable recovery journal 与正式 assembly 均未实现。

#### 结果

- Rust host 使用显式 executable/exec-style launch profile、版本化 minimal child contract、worker startup handshake（不是 Deployment readiness）、持续 heartbeat 和 epoch IPC；process group/cgroup/job-object/pidfd 等 OS 机制按 target profile 冻结。
- Python/C++ reference worker 只实现 construct→invoke→terminal frame 的受管协议，不创建第二 Runtime；Python multiprocessing 的 spawn/forkserver 只属于特定 worker adapter/依赖兼容矩阵，不是 ProcessDomain 公共语义。
- `stop accepting → drain → cooperative stop → TERM → KILL → process-tree cleanup → join`。
- restart window、attempt budget、backoff、jitter、quarantine 和 CPU/RSS/FD/process/device limits。
- `RuntimeHostEpoch + DomainEpoch + InvocationId` late-result fencing。
- `RuntimeOwnershipTree` 与 `FailureContainmentSpec`；前者表达实际 ownership，后者冻结 Domain 共置和 collateral restart 的计划边界。
- `LivenessSpec`/`LivenessState` 和具体 RuntimeFailureFact（至少覆盖 ProcessExit、HeartbeatMissed、WedgeDetected、CleanupCompleted）；Liveness 不等于 Health 或 Readiness。
- `RecoveryPolicy` 区分进程级 `RestartPolicy` 与副作用级 `InvocationRecoveryPolicy`，默认 no replay。
- RuntimeHost 内部的确定性 `RecoveryEngine` 只产生 RecoveryDecision/RecoveryAction；RuntimeHost 是持有 Domain/PID 并执行 start/stop/terminate/restart/quarantine/cleanup 的唯一 owner。
- 不同故障域中的 watchdog 持续监测 bootstrap、heartbeat 和 control responsiveness；P2d 只实现 watchdog contract 与外部测试进程/OS service-manager adapter，完整 NodeDaemon 到 P5 才实现。
- bounded IPC credit/retained-byte budget，side-effect/restart-safe/RecoveryPolicy，以及 device completion/reset/fencing 门。
- reference ProcessDomain 的受限 Card profile：每个 CardInstance generation 使用新的 ephemeral workspace，默认无 host persistent path、database credential、raw Zenoh 或任意 egress；只安装计划声明的 typed handles。该 profile 必须明确 OS sandbox 的实际覆盖与缺口，不能用单一 seccomp/容器标签宣称完整隔离；Loop/ThreadDomain 仅运行通过 run-bound/native-risk 审查的受信同构建实现，Rust 语言本身不自动获得准入。

#### 验证与证据

- child crash、ignore TERM、SIGKILL、OOM、写 IPC 时死亡和产生 grandchild 均能清理并产生真实 RuntimeFailureFact；已 handoff 但无 effect 终态证明的 Invocation 为 `Uncertain`，不伪造 `Failed` Receipt。
- 强杀后旧 Queue/Pipe/Lock/channel 作废，新 DomainEpoch 建立新 IPC；迟到结果被拒绝。
- restart 超预算进入 quarantine，不形成重启风暴或重放 `uncertain` 物理 Command。
- 默认 restart 不 replay 任何在途工作；只有具备权威 recovery owner 和已验证 idempotency 的 RecoveryPolicy 可例外。
- child crash 不表示 GPU/device 已停止；设备进入 unknown/poisoned/reset-required，新 owner 在 fence/health/reset 前不得 Ready，`Succeeded` 需要 device completion ack。
- revision activate/drain 失败时不混用新旧 Domain/Mailbox/Policy，旧 revision 的 Invocation 被 fencing。
- 在干净 process + 新 workspace replacement 后，旧 ephemeral 文件不可见；未声明 raw file/database/network access 被 reference sandbox/enforcement 拒绝并产生 reason/fact，受限 profile 不能通过环境变量、继承 FD 或 raw credential 绕过 typed handle。
- RuntimeHost/Fabric 不因一个 ProcessDomain 故障共同退出；OpsService 尚未在 P2d 实现，其故障隔离在 P6b 验证。
- L2 Harness 增加 Rust RuntimeHost → Python reference worker 的真实本地 child smoke，贯通 launch/readiness handshake/heartbeat/credit/callback/cancel/stop/process-tree cleanup；heartbeat 只有在 protocol/schema、instance/generation/sequence 校验后才刷新 liveness，旧代或 malformed payload 不续绿。另以可信 Rust child 验证协议不绑定 Python。

### 7.6 P2e：最小 Deployment control plane

P2e 不建设 Kubernetes 替代品，而是让 P0/P1 冻结的分布式 owner 有第一个真实 producer 和执行闭环。P3 以后不得长期依赖手写 Slice fixture。

#### 结果

- ADR-0004/0005/0007/0008 Accepted 或取得针对其具体选择的等价明确授权后，才实现最小纯 `DeckCompiler` 及后续 P2e 闭环。Compiler 把 DeckSpec 确定性解析为唯一 DeckLock，DeckResolver 是无 I/O 的纯步骤，Compiler 不读取 live Node、Runtime 或 Fabric 状态，Planner 不接收独立 topology 输入。typed Deck multigraph/SCC 与 ServiceDependency validator可以对更宽输入给出稳定结构化拒绝，但S7唯一能进入candidate/commit的非空 workload是一个Deck-scoped instance引用exact manifest single fixture、canonical-empty config、零Port/Link/ServiceRequirement/dependency/effect grant的idle Loop subject；per-use config/profile、Signal/Event、provider和一般graph shape都不能借DeckLock进入S7执行。
- 纯、确定性的 `DeploymentPlanner`：输入 canonical DeckLock、固定 target facts、DeploymentProfile/policy snapshot、system installer/install operation一次生成并byte-identically交给operator/Controller/Planner immutable ingress的singleton `RuntimeArtifactCompatibilityManifestV1` artifact，以及 DeploymentController 提供的 immutable previous-plan/stable-ID allocation snapshot，输出不可变 `DeploymentPlanCandidate {plan_content, allocation_delta, diagnostics, plan_content_digest}` 或结构化拒绝；PlanContentDigest 只覆盖 PlanContent。manifest在`manifest_version = 1`后恰有一个target row：exact target、`RuntimeBuildIdentityV1 {build_instance_id, build_descriptor_digest, runtime_artifact_sha256, compiled_reference_compatibility_digest}`、selected exact PXAR v5、profile v1与exact single fixture entry；没有record count、第二target、supported-version/mode/fixture set、mode mask或operator-selectable limit selector。projection携带独立domain的manifest digest和同一exact row，并进入PlanContent、canonical target Slice与target-slice digest。所有bounds由v4/v5/profile protocol constants拥有，变化必须使用successor，不能通过manifest配置。Planner拒绝operator预制manifest或其他side-file authority，不手写/重建第二manifest，只消费与Runtime initializer相同的exact installer artifact；bootstrap也只校验，不产生manifest。Runtime executable SHA-256/build identity与fixture artifact digest不得混用。Planner只可提交`OneSourceLoop`或显式plan-side `EmptyTargetDesiredEntry`生成的`EmptyDeactivate`；任何一般activation、Ingress、Thread/Process、binding、per-use config或第三shape都在candidate/commit前拒绝。
- Graph 算法提取门：先完成上述两个领域的 typed model、validator、golden/property test；只有它们作为两个独立生产消费者证明相同 immutable multigraph/SCC/cycle/topological 算法后，才在本批次按 Accepted ADR 抽取 internal Graph Foundation。不得包含领域 Schema/metadata、Graph identity/revision/digest/store/query、I/O 或执行状态；交集不足就不提取。
- 按 DeploymentScope 单写的最小 `DeploymentController`：验证 candidate，并在同一 crash-consistent transaction 中原子提交 stable-ID allocation delta、下一 DeploymentRevision、committed DeploymentPlan/DeploymentPlanDigest；随后只协调该窄profile的apply/query/reconcile、保存per-target rollout fact。Loop→Loop replacement在S7拒绝，必须经更高revision `EmptyDeactivate` terminal exact-zero后再以另一更高revision启动；一般prepare/activate/ingress/egress/rollback rollout不属于S7。本地副作用只由目标RuntimeHost执行。reference进程候选名为`paraegox-deploymentd`；不使用裸`controllerd`。
- 首个 production reference 优先以 Rust 实现 DeploymentPlanner/DeploymentController，但语言不是 authority 属性：它仍位于 Kernel/RuntimeHost 外，独立进程和 journal，不能因同一 Cargo workspace 或 binary image 下沉 Kernel、合并 Runtime owner或与 Python reference 形成并列 writer。
- DeploymentController 内部使用无独立身份、store、tenure、I/O 和生命周期的纯 `DeploymentReconciler` 与 `DeploymentRolloutEngine` 求值 desired/observed diff 和 rollout transition；持久 commit、journal、I/O、retry/query 与最终 decision 仍归 DeploymentController，不新增 daemon/CoreService。
- P2e 集成 P1 已实现的纯 `RuntimeSliceProjector` 和 `RuntimeApplyEnvelopeBuilder`：前者从 committed plan + target 生成 tenure-neutral Slice/PlanProvenance；后者构造additive `RuntimeApplyEnvelopeV2`，把DeploymentWriterRef/DeploymentWriterEpoch/WriterTenureProof映射为runtime-owned PlanWriterContext并绑定CAS/request controls，同时加入exact 32-octet `expected_runtime_store_instance_id`。该store identity来自Controller已durable pin的authenticated bootstrap response，只进入v2 canonical encoding、request-signing transcript与complete-request digest，不属于Plan/Slice desired truth，也不改变target-slice digest。builder无key；DeploymentController使用独占OS-protected request-auth key handle签署并commit-before-send，且与Tenure Authority private key隔离。Runtime不importdeployment领域类型。
- additive PXTE v4/PXAR v5 的public grammar固定为：exact target manifest projection、`ReferenceAssemblyProfileV1`、optional `ReferenceLoopDomainSpecV1`、optional `ReferenceLoopSubjectSpecV1`，PXTA binding count恒为零。这两个v4 record不alias旧的capacity-bearing public types；profile固定`lifecycle_concurrency=1`、mailbox/dispatch/background-task slots全为0。`OneSourceLoop`恰有一个reference domain（仅Domain ref与signed start/drain/cleanup budgets，无capacity）和一个reference subject（exact Instance/Domain、definition/implementation/export refs及definition/fixture-artifact/canonical-empty-config digests）；`EmptyDeactivate`二者均为零。不存在Thread/Process、Thread executor/general capacity、ExecutionIngress、input/tick/dispatch或一般activation branch，也不保留zero-only placeholder。旧版本bytes/digest/reason/no-fallback不变。
- RuntimeHost 内部 `RuntimeAssemblyEngine` 只消费已认证 RuntimeApplyRequest、canonical target Slice 与 Runtime apply journal。PXAR v5先在任何tenure nonce/fence、request/temporal state、revision high-water、prepared或副作用前验证`RuntimeApplyEnvelopeV2.expected_runtime_store_instance_id`逐字等于local journal envelope的`store_instance_id`；mismatch以`RuntimeStoreMismatch`零状态变化拒绝。随后normal `OneSourceLoop`才按`verify/fence/full-admission → PreparedNoEffects → FirstActionIntent → create LoopDomain/Instance → bounded cooperative on_start → desired head+LiveReady+terminal atomic commit`推进；restart reassembly另走`RecoveryPlannedNoEffects → StartCallIntent`，两套intent不得混用。live/nonzero `EmptyDeactivate`的第一事务必须同时写`FirstActionIntent`、`NoNewAdmission`、canonical empty head、`HeadCommittedRetiringOld`、exact old Slice/budgets与`Draining`，再bounded stop/join/cleanup到exact-zero terminal；canonical empty或`RecoveryFailedNotReady`且ledger exact-zero、无action/resource时只走deadline-prechecked、无intent/callback的单事务terminal fast path。intent/head transaction构造前和durable publish后、首个effect前各检查一次owner clock：publish前`now >= deadline`不写intent/head，publish后到期不创建resource/callback，已提交的empty head不得回滚。
- 需要cleanup的callback/deadline/cancel产生raw结果后、进入cleanup前，Runtime先atomic durable写bounded monotonic `RawActionOutcomeLatch`，保存raw KnownSuccess/KnownError/TimedOut、`raw_outcome_observed_at`及clock/deadline lineage但不提前选primary terminal；后续host interruption、higher-tenure takeover与cleanup/census evidence继续作为独立维度单调补入，durable known fact永不因crash降为Unknown。failure precedence固定为invariant/panic/cleanup/ownership uncertainty→quarantine，post-intent supersede，host crash，最后才是普通timeout/error/success；只有没有前三类高优先级结果且cleanup+exact-zero完成后，owner才在构造terminal前采样一次`terminal_selection_observed_at`并持久形成`TerminalOutcomeSelection`，`now >= deadline`含相等选timeout，否则按raw error/success选择。selection持久后不因fsync/回复跨deadline重分类，raw fact在timeout/interrupted/superseded terminal中也保留。`OneSourceLoop` start success可与active+terminal同一atomic commit而不写中间latch；该commit未durable即crash时outcome仍为Unknown。pre-intent crash证明no-effects；normal apply保留old head，recovery pre-intent crash可用fresh action/generations重建且不置failure latch，recovery pre-intent timeout则消费唯一attempt并置permanent failure latch；post-intent callback不因crash重放。
- `RuntimeBuildDescriptorV1`、`RuntimeBuildIdentityV1`与singleton `RuntimeArtifactCompatibilityManifestV1`的canonical Schema、digest domains、strict bytes与bounds都由`paraegox-runtime-contracts`唯一拥有。release pipeline是descriptor唯一production producer：canonical descriptor绑定nonzero CSPRNG `build_instance_id`、final RuntimeHost executable length/SHA-256、target triple与`compiled_reference_compatibility_digest`。system installer/install operation是descriptor+installed executable的strict consumer和singleton manifest唯一production producer；它只从verified descriptor/executable、operator exact target/service identity与binary compiled fixture table一次生成同一canonical manifest artifact，再byte-identically交给Runtime initializer与operator/Controller/Planner immutable ingress，拒绝任意prebuilt manifest或side file成为第二authority。initializer再次验证final executable length/SHA-256/target和binary compiled id/table，并在Runtime sequence-1 snapshot直接持久化exact descriptor bytes+digest与installer-produced manifest bytes+digest；Planner不重建manifest，bootstrap只校验。后续startup只验证snapshot里的pinned canonical bytes/digests，并从binary内不可由config/journal覆盖的compiled build id与compatibility table重算actual identity逐字段比较；不重新hash executable，也不读取side file/config作为第二权威。bootstrap分别报告compiled actual与store-pinned descriptor/manifest identity，不能把journal值回显为actual；任一不一致在bootstrap/query-ready/callback/resource create前quarantine。
- 窄 `DeploymentStore`、`DeploymentTenureAuthorityPort`、`NodeInventoryPort` 与 `RuntimeApplyPort`。三个owner各有versioned/checksummed/bounded snapshot store与owner-specific one-shot initializer；keys/policy/service principals由operator预置并绑定sequence-1 fingerprint，missing/corrupt state不能reset。P2e reference `DeploymentTenureAuthority` 由OS service manager以独立service account/ACL托管，用OS lock+crash-consistent store原子推进epoch并签署proof；`acquire_tenure` local IPC必须versioned/bounded/authenticated+authorized，验证peer credential、request signature、scope/writer allowlist。Tenure Authority signing key不交给DeploymentController；Controller request-auth key与Runtime verification policy隔离。same-uid/root compromise不在该POSIX reference保证内。单元测试可使用in-process fake，阶段完成证据必须使用真实Authority process、真实IPC client、release descriptor/installer chain、两条签名/验证路径、Controller/Runtime journals和authenticated Runtime bootstrap/apply endpoint。
- surface按真实调用链分阶段准入：S7-B将`RuntimeApplyEnvelopeV2`、descriptor/identity/singleton manifest/projection、Runtime bootstrap/query request+response及其channel-auth transcript、successor codec/builder保持`paraegox-runtime-contracts` private/internal；`acquire_tenure` IPC request/response/framing/auth transcript只由`paraegox-deployment`拥有，其中嵌入的`WriterTenureProof` canonical value继续复用`paraegox-runtime-contracts`唯一owner。S7-C DeckSpec/DeckLock/Planner保持internal enabler；S7-D只登记真实Authority process/initializer/persistent surface，`acquire_tenure` IPC在没有`deploymentd` client前仍internal。release descriptor generator默认保持internal build tool，若实现为repository executable则按真实entrypoint登记；system install operation因接收operator exact target/service identity并输出public singleton manifest，必须在S7-E同批register owner、consumers、compatibility与first functional test。S7-E还同批接通installer artifact byte-identical进入Runtime initializer与operator/Controller/Planner immutable ingress、binary compiled-identity check、operator DeckSpec ingress、`paraegox-deploymentd`、Controller/Runtime journal、Authority真实client/public promotion、authenticated Runtime bootstrap/apply与唯一request producer/consumer；S7-F才随真实两端call path登记runtime-contracts拥有的authenticated operation/live query并完成journal-bound restart reassembly。不得以prebuilt manifest、测试、wrapper或registration提前宣称public/implemented。
- DeploymentController 由 OS service manager 拉起，不由它控制的 RuntimeHost 自部署；开发/救援 CLI 是窄 bootstrap client，只提交 intent、触发 reconcile 或查询状态。它可以绕过尚未实现或不可用的 OpsService，但不能绕过 DeploymentController、tenure proof、Authority 或 Runtime fencing，也不提供 SSH/数据库直写 fallback。
- 显式 one-subject Card Deck development/system-smoke profile：仍产生 DeckSpec/DeckLock、committed DeploymentRevision、target Slice 与 DeckRun，并走完整 compiler→planner→controller→runtime query/reconcile 链；它只证明allowlisted compiled-in fixture的idle lifecycle与restart reassembly，不证明source processing、post-idle ingress或streaming；不发布`Card.run()`或standalone Runtime route。

#### 明确后置

- leader election、多 DeploymentController 共识、CRDT、跨 Site 自动 failover；这些后置不取消单写 tenure authority 和 proof。
- 通用容器调度、复杂 bin-packing、自动迁移物理设备 owner。
- 把 timeout 当作 apply 未发生后直接重试；先按 operation id/target revision 查询并 reconcile。
- 正式 Application/Installation、Marketplace、application-owned durable service、跨 Deck release closure 与 uninstall data policy；出现真实多 Deck/稳定安装/私有持久状态 fixture 后先 research 和 Proposed ADR，不在 P2e 留空类型。
- 允许 cyclic Deck 的 feedback/delay/seed/latest-value/backpressure 公共合同；在其 ADR 前 P2e 只报告 SCC/witness 并拒绝运行。
- 通用 Workflow/Graph Engine、Graph Service/Store/Query Router；未来 Agent durable workflow 需两个真实消费者和独立 ADR，不扩大 RuntimeAssemblyEngine 或 Graph Foundation。
- 跨 owner 的公共 platform crate、巨型 `Platform` trait、运行时 backend registry，以及 macOS/Windows production support 声明。S7-E 只要求新增 OS 调用留在 owner-private seam；共享抽取和跨平台 backend 按 [平台兼容研究](../research/platform-compatibility-ports-and-host-feature-profiles.md) 的 PC0–PC3 gate 后置。

#### 验证与证据

- 相同 Planner 输入产生字节稳定的 DeploymentPlanCandidate/PlanContentDigest；相同 candidate + committed header 产生稳定 DeploymentPlanDigest；相同 committed plan + target 产生稳定 tenure-neutral RuntimePlanSlice。Planner/Projector/Builder property test 不做 I/O。
- 相同 DeckSpec + resolver inputs 产生字节稳定的 DeckLock/digest；修改 Card key/ref/config/role/refinement、resolved Port、Link、DeliveryProfile 或 locked ref 必须改变 digest，只修改 Canvas View State 或旁置 display metadata 不改变 DeckLock digest。跨 revision fixture 证明同 key 参与 previous-plan diff，rename 产生 remove + add，删除后复用不继承旧 CardInstance/私有状态。测试与生产 Planner API 都不存在独立 DeckTopology 参数或旁路文件。
- 同一 Card pair 的不同 Port Link 作为 parallel edge 保留；随机打乱 node/edge 输入顺序不改变 SCC、cycle witness、DeckLock 或 diagnostics。ServiceDependency 的直接环、长环、自环和没有显式 feedback contract 的 Deck cycle 均在任何 Runtime 副作用前稳定拒绝，不回退声明顺序。
- S7 production candidate没有DataLink、Mailbox、consumer ingress或producer egress；相应Deck/Service graph输入只产生稳定拒绝，不能进入PlanContent/Slice。未来一般assembly successor若开放这些形状，DataLink仍不得生成启动topo，完整activation/readiness/loss/drain rule必须进入PlanContentDigest和对应target-slice digest，Runtime不能自行重算或弱化。
- 同一 byte-identical DeckLock 配不同 immutable ServiceSpec inventory/target facts 产生不同且可解释的 provider candidate，同时 DeckLock 内容和 digest 保持不变。
- RuntimeHost 未安装/import `deployment` 与 `decks` 仍能消费序列化 Slice；DeploymentController 不持有 RuntimeHost、Zenoh Session、Driver 或领域 Service 对象。
- `RuntimeApplyEnvelopeV2.expected_runtime_store_instance_id` missing/zero/wrong-width、签名未覆盖、或与local journal store identity不等时，在任何AdmissionState/fence/revision mutation前以`RuntimeStoreMismatch`拒绝；错误 PlanWriterRef/PlanWriterEpoch、target、source revision/digest、slice digest、operation id、temporal constraint、auth、tenure proof 或 exact-active-slice CAS 同样在副作用前拒绝。RuntimeHost 启动先原子、crash-consistently推进RuntimeHostEpoch/clock generation并invalidate旧live facts，验证sequence-1 snapshot中的exact canonical descriptor/manifest bytes+digests，并从binary重读compiled id/table逐字段匹配store-pinned identity；不重新hash executable或读取side file/config。完成前不发布bootstrap/query-ready也不接收apply。tenure-only transaction只持久化完整next AdmissionState tenure nonce+proof/principal与writer fence，不消费request/temporal/revision；第二个full-admission transaction才原子提交request/temporal state、source-revision high-water、exact request/Slice与`PreparedNoEffects`。
- 同 epoch 的不同 writer/proof、低 epoch 的迟到请求，以及没有受信 tenure proof 的高 epoch 均被 fencing；参考 profile 的一个 RuntimeHost 同时只接受一个 active source scope。
- journal 明确区分 host/clock generation、AdmissionState、source-revision high-water、`writer_fence`、`prepared`、`active` canonical desired head、`live_materialization`、`recovery_action`、`owned_resources`、`RawActionOutcomeLatch`、`TerminalOutcomeSelection`与terminal-operation ledger。normal apply的`PreparedNoEffects → FirstActionIntent`和recovery的`RecoveryPlannedNoEffects → StartCallIntent`分别证明副作用边界；pre-intent crash证明no-effects，post-intent不replay callback并须cleanup到exact-zero terminal或quarantine。`OneSourceLoop`只在bounded readiness成功时原子提交new desired head+`LiveReady`+terminal；`EmptyDeactivate`仅在live/nonzero时第一事务同时写intent、`NoNewAdmission`、empty head+`HeadCommittedRetiringOld`并保留exact old Slice/budgets/generation，第二事务在old ownership exact-zero后才terminal；already-exact-zero且无action/resource时为无intent/callback单事务fast path。empty head持续保留，旧clock generation deadline不换算。
- intent/head构造前与durable publish后、effect前分别做deadline check；publish前到期不写intent/head，publish后到期不启动resource/callback，已提交empty head不回滚。需要cleanup的callback/deadline/cancel raw事实在cleanup前先写immutable `RawActionOutcomeLatch`；无quarantine、post-intent supersede或host crash时，cleanup+exact-zero后才用一次`terminal_selection_observed_at`采样写`TerminalOutcomeSelection`，`now >= deadline`选timeout，否则按raw error/success，selection后fsync/回复延迟不改变分类且raw fact不丢失。`OneSourceLoop` start success只在active+terminal同一commit内成立，未durable即crash仍为Unknown。
- RuntimeAssemblyEngine 在不安装/import `decks` 与 `deployment` 的环境中只靠 Slice 完成 `OneSourceLoop` idle start/restart reassembly，以及`EmptyDeactivate`的live/nonzero head-first retire或exact-zero fast path；各phase crash、重复request与旧revision不产生第二action、双active generation或mixed revision。该证据不包含Binding、streaming/Command、一般ingress/egress、Thread/Process或Loop→Loop rollback。
- 相同 operation id + canonical request digest 的重试只返回/推进同一 journal operation；相同 id 不同 expected store、plan/slice/CAS/deadline 被拒绝。same-target旧store签名request送到fresh store在任何mutation前拒绝；writer turnover后先查询旧operation，再以新operation id发起新尝试，不因epoch变化隐式replay。
- `PreparedNoEffects`后start/readiness失败、DeploymentController timeout、重复apply、DeploymentController/RuntimeHost crash/restart和stale observed report均不产生revision回退、callback replay或双live generation；normal pre-intent crash保留old head，recovery pre-intent crash可在new host/clock epoch用fresh action/generations重建而不设置failure latch，recovery pre-intent timeout则消费唯一attempt并置permanent failure latch；unknown ownership只能quarantine。
- DeploymentController 正常重启从 DeploymentTenureAuthority 取得新 epoch/proof，但复用同一 committed plan revision/source digest/slice digest；journal恢复authenticated bootstrap exact bytes/digest及其pinned RuntimeHostId/store/channel、exact signed request、active/staged revision、operation id与per-target rollout fact。无法恢复权威store/build binding时quarantine/fail-closed，不从零猜测；timeout后不能换store identity、nonce或key重签同一operation。
- missing/corrupt/undecodable/unknown-version snapshot无法建立可信host/store/epoch/sequence，不绑定bootstrap/query-ready、不返回authenticated `Indeterminate`，Controller只能把服务不可用记为自己的`Indeterminate`且不得从config/corrupt header猜identity。只有snapshot完整validated、startup generation transaction已durable后出现的compatibility/recovery/ownership quarantine，才能携带exact pinned identity返回authenticated `Indeterminate { stable reason }`；它不能返回`Unknown`或历史Ready。
- 单 Node reference 只证明 owner、projection、apply 与恢复；不宣称 P5 之前已经完成跨 Node reconciliation。
- DeploymentController以`EmptyDeactivate`结束reference Deck workload并使旧DeckRun terminal，不会停止Authority、RuntimeHost journal或其他平台owner；S7没有Fabric/Inspection/共享Model CoreService的assembly，也不支持直接Loop→Loop replace。后续若需新fixture，必须先empty terminal exact-zero，再以更高revision启动；Application/Installation ownership、跨run持久状态、raw storage/egress和ProcessDomain都在S7 candidate前结构化拒绝，不能靠opaque code推断。
- 显式 one-subject Deck 的所有 fixture Card 在编译前声明，并完成 plan→commit→project→apply→observe→deactivate；任何未来便捷入口必须可导出 canonical DeckSpec。metamorphic test 只在相同 source scope/revision、resolver inputs、previous allocation、target facts、policy 与 committed provenance 下比较 DeckLock、PlanContentDigest 与 Slice digest；独立 deployment/commit 不要求 revision-bound Slice digest 相同，且调用图中不存在直写 RuntimeHost。

### 7.7 平台兼容候选工作池（P2e 后，须重新 admission）

平台兼容候选方向由 [平台兼容 Port、host-platform support evidence 与 OS Backend 边界研究](../research/platform-compatibility-ports-and-host-feature-profiles.md) 给出。该文档是platform RP（research plan/研究输入），Research Complete不是架构或实现授权；程序顺序固定为`platform RP → S7-E executable vertical evidence → PCA explicit admission → PC0 → PC1/PC2 → PC3`。S7-E evidence prerequisite 已由提交`1ed704c`和Ubuntu CI `30748840399`满足，但这只为未来PCA提供真实Linux-only候选证据；PCA仍未准入，macOS/Windows仍无production support，不能自行触发PC0–PC3或把它们改成当前ready set。PCA必须由用户明确接受范围或通过Proposed ADR/topic admission；这条路线不在P2e前增加横向重构，也不把“可编译”或“skip”冒充“平台支持”。统一对象是 requirement、result/evidence 与 conformance；Authority、DeploymentController、Runtime 和 OS service manager 继续拥有各自状态、journal、授权、retry/reconcile/restart budget 与 terminal outcome。

- **PC-G — S7-E 已执行、后续持续有效的治理 guardrail（不是新阶段）**：S7-E 已将 Controller/Runtime store、Authority client、Runtime endpoint 和 installer 的新增 OS syscall 限制在 owner-private adapter/module；S7-F 及后续同样不得新增顶层 platform crate、公共 trait、动态 registry、环境变量 fallback 或 generic journal。
- **PCA — platform-workstream admission decision（尚未准入）**：P2e 最小 executable vertical 的技术前置已满足；仍须冻结目标 OS、owner、完成证据、预算和与 P3 的优先级。未获得用户明确接受或 Proposed ADR/topic admission 时，PC0–PC3 保持候选，不自动执行。
- **PC0 — Linux semantics/conformance（候选）**：依赖 PCA。以现有真实 consumer 冻结窄 internal feature vocabulary 与 evidence level，分别建立 owned-process-tree、resource observation、authenticated local channel、secure filesystem object、crash-consistent publish、service supervision 与 entropy conformance；抽象后不得削弱当前 Linux/POSIX 证明，并单独裁决 cgroup v2/pidfd production profile。
- **PC1 — macOS backend/CI（候选）**：依赖 PC0。增加 pinned macOS compile/lint 与可运行 POSIX conformance CI；先为 secure filesystem object 补齐不经 path reopen、绑定已验证 file descriptor/object identity 的 extended-ACL 读取与拒绝证据，再证明 APFS file/directory durable publish 与 crash recovery，并分别证明 peer identity、service identity 和 process cleanup；在此前 `ProductionReference` 对 APFS 保持 unsupported，缺少 Linux `/proc` 等价 enforcement 时显式 unsupported/degraded。
- **PC2 — Windows backend（候选）**：依赖 PC0，可与 PC1 研究并行。Named Pipe + token/SID、Security Descriptor、Job Object、Windows service、file lock/replace/flush 与 CSPRNG 只是待一手资料和实机验证的候选；production 证据出现前保持 unsupported，不以 compile-only 宣称支持。
- **PC3 — shared extraction evaluation/admission（候选）**：治理要求的至少两个独立生产消费者，加上本文建议的保守附加门槛“至少两个真实 OS backend”，共同证明稳定交集后，才评估是否需要 internal leaf crate；涉及新 package/public Schema/descriptor-manifest persistent bytes时先 Proposed ADR 与 governance admission。交集不足就保留 owner adapter，不为目录对称抽取。

PC0–PC2 只有通过 PCA 后才可与 P3 的 Linux reference 实现并行；只有目标 milestone 明确要求某个 backend 时，该 backend 才成为硬依赖。`host-platform support evidence` 只是工作称谓，不是已冻结的新 Schema；它只表达平台支持，不是 `CapabilityGrant`、service readiness、未来 `NodeFeatureReport` 的替代品或永久机器标签，任何 `unknown/degraded/unsupported` 都不能静默满足 exact requirement。

验证至少包含同一语义 conformance、真实 OS component/system Harness 和 Authority/Controller/Runtime owner vertical；service wrapper/真实 child 身份不一致、peer identity/ACL不一致、逃逸 descendant、继承 lock、durability uncertain、resource census不完整、feature report stale以及fallback强制启用都必须 fail closed。Rollout 只替换唯一 adapter wiring，不 dual-run/dual-write；回滚保留 owner journal 与 canonical contract。

## 8. P3：本地仿真物理控制闭环

### 结果

- `IdentityResolver` 测试实现。
- AuthorityService：CapabilityGrant、策略版本、revocation epoch、audience，以及绑定 command/operation/resource/device-session/audience/expiry 的 AuthorityDecisionRef/digest。
- ResourceCoordinator：每资源 LeaseGrant、LeaseId、LeaseIssuerEpoch、单调 FencingToken 与冲突判断。
- `SafetyIslandAdapter` + 模拟独立 safety island：reference profile 位于独立进程/独立 clock 与 deadman，拥有直达 Simulated Actuator 的 safe-output path；输出带 freshness/SafetyEpoch/function kind、inhibit、watchdog、reset request/permit、硬件 ack 和绑定本次 command 的 SafetyDecisionRef/digest。Adapter 不拥有实际安全功能。
- Simulated Driver 拥有 presence、observed firmware/ABI/config、device session/hardware ack；Deployment 拥有 desired firmware/config/PhysicalAssembly revision；DeviceService 从同一组输入 revision 原子派生 `DeviceReadinessSnapshot`，含 derived-at/valid-until，任一输入换代立即失效。
- `OperationClient/CommandEndpoint`：静态 1:1、bounded outstanding、无透明 retry 的唯一 Command 入口；Controller-role Card 无法直接取得 Driver/Actuator 对象。
- Driver EnforcementPoint 是 normal-command path 的最后软件准入点；它原子校验后只把 setpoint 提交给物理下游的模拟 safety gate，后者拥有 enable/inhibit/clamp/safe-output 的支配权与最终 applied-output ack。`PhysicalCommandEnvelope` 绑定 command/requested-operation digest、resource/Operation、device/session/driver、assembly、CalibrationRef/revision、mode/safety、lease/fence、deadline/sequence/idempotency、AuthorityDecisionRef/digest、SafetyDecisionRef/digest，以及 Operation 声明要求的 Power/Thermal/Operating Envelope fact refs；real profile 还必须绑定 H1 安装的 `HardwareActivationRef/Epoch`。正常 Driver 与 safety gate 不是两个平级 writer，也不存在绕过安全链的 fallback。
- 版本化 `PhysicalAssemblySpec` fixture：Device/Frame/Resource/Calibration 与约束的 Artifact，不引入 Robot owner。
- `ContinuityController` + 编译后的最小 `ContinuityProfile`：固定 Deployment/assembly/calibration/safety/artifact revision/digest，约束触发 dependency set、debounce/hysteresis、断网时长、允许动作、预先衰减的 offline Grant、时钟/电源/热/Operating Envelope/Evidence 下限、安全停止与重连 reconciliation；RuntimePlanSlice 携带 profile ref/digest/applied revision。
- Driver/设备 adapter 拥有 raw power/thermal telemetry；DeviceService 从同一输入 revision 原子派生有 freshness 的 `PowerState/ThermalState`；产品域 `OperatingEnvelopeEvaluator` 独占产生绑定 ODD/World/state revision 的 `OperatingEnvelopeEvaluation`；safety island/Adapter 消费这些事实并拥有 inhibit/SafetyDecision，Continuity/Admission 只消费。不提前建立 EnergyService。
- 最小 `ScenarioRunner/ScenarioManifest`：固定 runtime/deployment/assembly/world/engine/assets/seed/timestep/calibration/fault-plan digest 和 expected invariants/tolerances，隔离 truth channel 与 Observation；P3 只运行 fake+sim Driver，可复用 suite 留给 H1 real Driver/HIL。
- 副作用 owner 的 idempotency ledger、最高 fencing token 与可查询执行状态。
- Authority、lease、Safety、execution 分阶段 Receipt；EffectReceipt 区分 requested、authorized/permitted、applied operation digest、SafetyEpoch、device-send ack、device completion ack 与 observed-effect evidence level。

### 硬性语义

- CapabilityGrant 只表示“允许请求”，Lease 才表示“当前控制者”。
- Authority 与 ResourceCoordinator 可以同进程，但 API、状态和 Receipt owner 分开。
- ResourceCoordinator 只拥有本地单写 lease/fence 分配；Driver EnforcementPoint 拥有 normal-command 的最终软件比较、idempotency 和 setpoint submission，物理下游 safety island/设备原生 controller 独占 enable/inhibit/clamp/safe-output gate 与最终 applied-output ack。三者可共置但逻辑 owner 分离，远端 Authority 不能直接签发本地写 lease；若设备没有这种下游仲裁或经证明的等价 device-native safety，H1 不得通过。
- 每资源 fencing token 单调递增，并在真实执行 owner 处跨 RuntimeHost 重启保持；只写 Runtime 内存不通过验收。
- Lease expiry 由资源 owner 的本地 monotonic clock 判断；P3 的 issuer 与执行 owner 同故障域，有界 duration 由 owner 换算为本地 expiry。
- issuer/EnforcementPoint 重启时推进相应 epoch、使此前 lease/command 失效，并在恢复 fencing/idempotency、stop/device completion 前 fail-closed；首版不恢复跨重启旧 lease。
- 每类设备声明 `device-native / driver-proxy / unsupported` fencing support；proxy/unsupported 在 Driver/主机 crash 后必须 stop/drain/reset/observe 或 quarantine，不能只恢复 ledger 自动接管。
- `accepted` 不等于 `succeeded`；超时后返回 `uncertain` 并查询 owner，禁止自动重放。
- E-Stop、protective stop、deadman、limit、collision inhibit 与控制应用的普通 Stop 分开建模；前五者由独立 safety island 直接驱动安全输出，不要求 control lease，也不经过 Agent/Mailbox/Fabric。Reset 绑定 SafetyEpoch、具体 safety function、Device/ControlMode、已认证操作者与硬件 ack，并要求所有 trip source fresh/clear。
- Safety 负责 clamp/inhibit/fail-safe，不作为和 planner/teleop 竞争的最高数字优先级 writer；clamp 后 applied digest 与 requested digest 分开，不能冒充原请求完全成功。
- Control mode handoff 固定为 request→stop/quiesce→observed-safe/neutral→release old lease→acquire new lease→activate new epoch；CommandSequence 作用域为 resource + lease issuer/lease + ControlModeEpoch，并定义 duplicate/gap/supersession Receipt。Emergency 是 safety function/state，不是 control mode。
- `ContinuityController` 只组合 Deployment 编译的约束，不拥有 Authority/Resource/Safety/Device/Evidence 事实；所谓本地 issuer 是预先衰减的 offline Grant，不是断网后自授权限。首次满足 loss predicate 时创建 ContinuityEpisodeId/offline_since；短暂重连不结束 episode或刷新窗口，只有稳定连接 + reconciliation + close Receipt 才结束。ContinuityController 重启无法恢复可信 episode或 monotonic baseline 时默认停止自治，除非持久 boot counter/safe clock 或本地重新授权已被验证。
- 分区行为由资源规则与 Deployment 编译的 `ContinuityProfile` 表达；断网不延长远端 Grant，不自动 replay Command，重连从当前物理观测 reconciliation。
- 跨多个执行器默认不宣称分布式原子 effect；只有同一物理故障域中经验证的 barrier/prepare-commit 可以声明原子，否则提供安全补偿。
- 本地 Authority/Resource/Safety/Enforcement 的正确性不依赖远端 Fabric 连通；P3 用 test fixture 闭合契约，不建立生产 fallback route。
- P3–P5 physical write 验证均限定 simulation profile；第一个真实 actuator 只能在 P6b 后通过 H1 Hardware Enablement：Device/Safety/Evidence 提供 commissioning、readiness、HIL/timing、独立 E-Stop/物理隔离、rollback/人工接管证据，由 Release owner 签发绑定 device/assembly/deployment/artifact/ODD/限制/expiry 的 HardwareEnablementReceipt。

### 验证与证据

- 未识别 Principal、缺失/过期/错误 audience 的 CapabilityGrant、未知策略版本、过期 Lease 和旧 fencing token 默认拒绝。
- delegation 只能缩小资源、操作和期限。
- 两个 Controller-role CardInstance 竞争同一资源时，旧 owner 即使迟到也不能执行。
- ResourceCoordinator、Driver EnforcementPoint、下游模拟 safety gate 与 RuntimeHost 分别及联合重启后，旧 LeaseIssuerEpoch、旧 fencing token 和重复 Command 均被拒绝；恢复完成前不签发新 lease，safe output 不因 Driver 重启被解除。
- 设备仍在执行缓存 Command 时注入 Driver crash；无 device-native completion/fencing 证明时，新 owner 不能接管并进入 quarantine。
- Safety input 进入 inhibited 状态时，已有授权和 Lease 仍不能绕过。
- 在 Authority/Safety decision 后、Driver setpoint submission 前分别注入 E-Stop、hotplug、calibration/assembly/mode、Power/Thermal/ODD fact 换代或过期；旧 `PhysicalCommandEnvelope` 在 Driver EnforcementPoint 被拒绝，若竞态已经越过该点则由物理下游 safety gate 保持或进入 safe output。
- 让 safety trip 与迟到 normal command、Driver restart 竞争；下游安全输出必须持续锁存到受权 reset/rearm，迟到 setpoint 不能覆盖它，两个路径也不能表现为平级 writer 的 last-write-wins。
- Command A 的 Authority/Safety Decision 用于 Command B 时拒绝；Safety clamp 后 Receipt 明确 requested/applied 差异，SDK return 不能直接产生 Succeeded。
- 真实 wedge/kill RuntimeHost 进程、断开 Adapter、Fabric 断连、Evidence 写满或 SafetySignal 过期时，独立进程的模拟 safety island/clock/deadman 仍能经 direct path 收敛；重放 reset/clear 不能解锁。
- 分别注入 E-Stop、protective stop、deadman loss、limit、collision inhibit 与普通 Stop，证明其 owner、锁存/reset、lease 要求和 Receipt 不被一个通用 FSM 混淆。
- hotplug、同路径设备 reboot/替换、双 Driver bind、observed/desired firmware/config、calibration、assembly、mode 与 safety 变化后，相应旧 DeviceReadinessSnapshot/session/epoch/revision/lease/command 全部失效；不能拼接跨 session facts 得出 Ready。
- 只有 trusted Driver/Scenario boundary 对 Observation physical origin 与 CalibrationRef/revision 盖章；普通 PortBinding/payload 伪造 origin/device/frame/calibration/time quality 无效，frame/transform/calibration revision 更新和 unknown uncertainty 均使不满足要求的 Observation 被拒绝。
- Continuity Harness 覆盖断网时长、clock quality、Power/Thermal/Operating Envelope、Evidence 容量、主机重启/monotonic baseline 丢失、重连不 replay 与当前状态 reconciliation；接近最大离线时长反复短暂重连不能无限刷新窗口。
- ScenarioRunner 固定 manifest 后在声明 tolerance 内复现；truth 不得通过普通 Observation 泄漏，fake/sim Driver 通过共同 contract conformance。Real Driver/HIL 只在 H1 运行，不能用 P3 mock 冒充。
- 未经完整 EnforcementPoint 无法调用 Simulated Actuator。
- fake Agent → admitted fake Tool → simulated Operation → EffectReceipt → OutcomeClaim → independent VerificationResult 贯通；各阶段 owner/terminal state 分开，InvocationIntent 在 dispatch 前已提交，Uncertain 不 replay。

## 9. P4：Zenoh-native production Fabric

### 结果

- FabricService 窄能力：bind、pub/sub、query/queryable、liveliness/matching、session/reconnect。
- production Fabric owner 使用 Zenoh Rust API；zenoh-python 只允许出现在经准入的 Python Gateway/worker 内，不能形成第二 FabricService、第二 session owner 或 Card 的 raw Fabric 旁路。
- keyspace versioning、schema/content type、boundary principal 与 DeliveryProfile mapping。
- items/bytes/age/retained-byte 有界、可观测的 Fabric ingress buffer 与 ingress worker；buffer 只暂存 pre-validation encoded frame，不是 Mailbox。
- `FabricSessionEpoch`、`BindingEpoch`、连接/binding 自检和 `FabricFeatureReport`。
- `ZenohTopologyProfile` 作为部署配置；Zenoh 1.9 Region 只保留为 adapter 配置或运行观测。
- `session-local`、`host-local`、`remote` 三种 route locality 使用同一 PortBinding/Message/Mailbox contract；P4 实测同 session 与同主机双进程，P5 在双主机实测 remote。

### 约束

- 普通 CardInstance 私有实现上下文不获得原生 Zenoh Session，只获得为 CardDefinition 声明端口编译的 PortBinding；动态发现或桥接的例外走 scope 指向 Fabric resource 的最小 `CapabilityGrant`，只有 Fabric 实现拥有原生 Session。
- callback 只做固定成本的 key/header/size/version、缓存命中的 transport principal、BindingEpoch 检查，并非阻塞 `try_offer` immutable encoded frame/reference 到 Fabric ingress buffer；完整 decode、解压、复杂 schema/principal/binding validation 进入有界 ingress worker。成功后才构造 Message 并 offer 到 target Mailbox，失败记录 ingress rejection，不能报告 Message accepted；物理业务授权仍由本地 EnforcementPoint 完整执行。
- FabricService 不拥有 Evidence/World/Memory retention；Zenoh storage 只能是显式 ServiceContract/adapter，调用者另需 Grant。
- Fabric inspection 只报告自身 session、route、matching 和 binding，不充当全系统 Inspection。
- 短暂断线/同 Session 自动重连只更新连接观测；Session 对象重建才改变 FabricSessionEpoch，逻辑 Binding install/reinstall/reconfigure/revoke 才改变 BindingEpoch。
- 同一 DeploymentRevision 与活动 BindingEpoch 内，一个 BindingId 只有一条 active production route；route replacement 按 `prepare → activate → drain → retire` 完成或显式 rollback，`activate` 原子切换新 frame/Message 的准入、旧 route 立即转为 drain-only，fan-out 为每个 destination 使用独立 BindingId。
- 禁止 local+wire 双投、隐式 transport fallback 与 payload hash/time-window echo 去重；只有目标硬件证据证明 Zenoh `session-local` 不满足 SLO，才通过独立 ADR 评估与 Zenoh 互斥、通过同一 conformance 且不传递可变进程内引用、裸指针、Runtime handle 或 Python object 的同进程 route。
- Transport 重试不自动重放物理 Command。
- 不为 DDS、ROS2 或 MQTT 创建对等 Fabric Backend。

### 验证与证据

- 断线/同 Session 自动重连保持 epoch；强制 Session 重建后旧 session callback 被拒绝；逻辑 Binding 重装后旧 binding callback 被拒绝。
- Observation 洪峰不阻塞 Command 的 Runtime priority class 和端到端 control path；不建立公共 control lane。
- Zenoh priority/multistream 与 Runtime dispatch policy 分别从同一 DeliveryProfile 编译并报告 `FeatureLoss`；网络优先级不能冒充应用执行优先级。
- `FabricFeatureReport` 无法满足 DeliveryProfile 时，binding 在运行前失败。
- 无 Zenoh 依赖的 Kernel/PortBinding fixture suite 仍通过，Zenoh 各 locality 复用同一 conformance。
- malformed/oversized/高成本 validator 输入不会进入 target Mailbox；Fabric ingress buffer 的 items/bytes/age/retained bytes 不越界，overflow/rejection 可解释且不伪造应用 accepted。
- route replacement 的故障注入证明任一观测点最多一条 route 接收新 Message，完成或 rollback 后无旧 route 泄漏、双投或基于内容的 echo 去重。
- 记录 PortBinding test fixture、Zenoh `session-local` 与 `host-local` 的延迟、抖动、CPU、copy/serialization、队列和恢复时间；P5 补 remote 数据。

## 10. P5：Node、持续 Deployment reconciliation 与双主机

### 结果

- `NodeIdentity`、`NodeSpec`、`NodeStatus`、`RuntimeHostStatus`。
- NodeDaemon 发布带 NodeId/NodeIncarnation/sequence/freshness 的 Node observed facts，拥有 NodeManagementEndpoint 和 Runtime endpoint discovery；Identity/Enrollment 拥有计算身份，DeploymentController 只消费 Node facts并拥有 desired placement，不能反向修改 observed status。
- RuntimeApplyEndpoint 属于 RuntimeHost。NodeDaemon 不接收、改写、准入或拒绝 RuntimeApplyRequest；即使 transport 需要 proxy，也只能透明承载。RuntimeHostId 来自 Bootstrap/Enrollment 配置，NodeDaemon 只发现/报告。
- 扩展 P2e 已存在的 DeploymentPlanCandidate、committed DeploymentPlan/Revision、DeploymentPlanner/RuntimeSliceProjector/RuntimeApplyEnvelopeBuilder/DeploymentController/DeploymentTenureAuthority 和 journals，加入双 RuntimeHost 分阶段 rollout、持续 bounded reconciliation 与 cross-node Binding 两端一致性；不建立第二套控制面。
- DeploymentController 仍为每 DeploymentScope 单写；P5 不宣称 leader election、多 DeploymentController HA、跨 Site 共识或自动迁移物理 workload。
- Node/workload enrollment、短期 identity/credential、trust domain 与可选 attestation adapter；字符串 Principal 不冒充可信隔离。
- Artifact digest、platform/runtime ABI、ServiceContract/Message Schema compatibility 与 publisher/provenance policy 的最小 admission。
- 目标 Node 的 RuntimeHost 拥有实例；调用方只拥有 typed service client/permission-bound handle。
- ExternalWorkloadAdapter 只观察或请求 external workload manager，不产生虚假本地 ownership。

### 验证与证据

- 双主机启动、断连、分区、重连、NodeDaemon 重启与 RuntimeHost 重启。
- NodeDaemon restart/registration tenure 换代后旧 NodeIncarnation facts/replies 被拒；RuntimeHost restart、Feature refresh、heartbeat gap 或同 session reconnect 不改变 NodeIncarnation。
- NodeDaemon 失联只使 Node facts stale/partitioned，不自动断言 RuntimeHost 已死；双 NodeDaemon 的旧 tenure 不能覆盖 current registration。
- NodeDaemon 无法修改或绕过 RuntimeApplyRequest；RuntimeHost 仍独占 target/revision/proof/CAS/fencing admission 与 apply Receipt。
- production profile 中 NodeDaemon 与 RuntimeHost 位于不同故障域；NodeDaemon/OS service manager 对 host restart-budget/quarantine 只有一个 ledger/mutation owner，不形成双 restart loop。
- 只有在P5之前另有被接受的一般assembly successor后，consumer ingress/provider才按其digest-covered规则先prepare/ready、再激活producer egress；S7/P2e本身没有Ingress/egress。任一目标partial apply时全局状态保持staged/degraded/uncertain，而不是误报Deployment Ready。
- DeploymentController crash/restart 后从 journal 恢复 active/staged revision、operation id 和 rollout facts，再向 DeploymentTenureAuthority 获取更高的新 DeploymentWriterEpoch/WriterTenureProof；旧 epoch 只能作为审计事实，不能恢复、复用或继续签发 apply。随后查询各目标 observed facts 再 reconcile，不盲目重复 apply。
- 旧 NodeIncarnation、RuntimeHostEpoch、FabricSessionEpoch、BindingEpoch 和 DeploymentRevision 分别被正确拒绝。
- 启动中失联的 Node 不被报告为 ready；P5 产生带 source epoch/freshness 的 stale/partitioned Inspection fact，P6b/P7 才由 federated Inspection/OpsClient/TUI 展示。
- 分区时本地物理路径按资源策略继续、降级或停机；新的远端 Command 默认拒绝。
- 重连只 reconciliation desired/observed state，不透明重放物理副作用。
- 非权威 `site_hint` 不参与授权、lease、信任或网络身份推导。
- 混合版本的 runtime protocol、ServiceContract、Message Schema、Artifact ABI 与 state schema 任一不兼容时拒绝；首版只做 stop-and-replace。

## 11. P6：Evidence 与 Operations 阶段组

P6 必须拆成本地 durability 与分布式 operations 两段，不能让尚未存在的 P4/P5 Fabric/identity 成为本地 effect commit 的隐藏前提。

### 11.1 P6a：local Evidence commit 与 local Inspection

- EvidenceService local commit protocol：append idempotency、local commit ack、store epoch、owner-local sequence/causality、integrity digest、retention、storage-full/backpressure 与 redaction。
- 聚合并持久化本 Node 自 P2 起就存在的 Runtime instrumentation；补充本地 TraceContext/logging adapter，但不到 P6a 才首次实现 queue/lag/thread/process 指标。
- 每个 Node 建立逻辑上的 node-local InspectionService role 与公开 InspectionProtocol：投影 RuntimeHost、Service、Domain、Mailbox、Binding、Authority、Lease、Safety、Device、Continuity 与 Evidence 状态；它只拥有 projection revision/cursor/cache/freshness，不拥有 source facts。
- local incident snapshot 与 Receipt/Evidence refs；远端 Collector 不可用不阻止权威本地查询。

### 11.2 P6b：distributed aggregation、federated Inspection 与 OpsService

前置条件是 P5 双 Node identity/partition/reconciliation 与 P6a local commit。交付：

- Evidence replication、replication lag、跨 Node causality/ref resolution、retention policy 与 conflict/duplicate handling；不宣称全局总序。
- federated InspectionService role：关联多个 node-local snapshot/watch，保留 source owner/revision/epoch、observed_at、freshness、watch gap 与 stale/unknown/partitioned；不回写 source truth。首版可与 OpsService 共进程，但 contracts、state、budget、failure 和 Receipt owner 分离。
- 单实例 OpsService CoreService 与 crash-consistent operation journal。最小 OpsProtocol 为 `preview/submit/get/watch/cancel-if-supported/reconcile-uncertain`；记录 ControlRequestId、canonical digest、principal/approval refs、target/action、expected revision/epoch、deadline、dry-run、ordered progress、owner Receipt/Evidence refs 与唯一 terminal OpsReceipt。
- action 只路由到 typed owner：deployment → DeploymentController，Node maintenance → NodeManagementEndpoint，Artifact/Release → 对应 Service，物理/不可逆操作 → Authority/Resource/Safety/CommandEndpoint。OpsService 不写 RuntimeHost、Deployment store、Artifact active pointer、领域数据库或 raw Zenoh key，也不内置任意 shell/SSH/package/container/systemd executor 或通用 DAG/saga engine。
- 同 ID/同 digest 重试幂等，同 ID/不同 digest conflict；timeout/断连/restart 进入 `Uncertain` 并先查询实际 owner，不透明 replay。OpsReceipt 只引用 owner Receipt，不能用 log/probe/exit code/transport ACK 推断成功。
- 首版一个独立管理侧 OpsService 服务多个 Node，不建立每 Node 一个 OpsService/ConsoleGateway。OpsService 是普通 plan-managed CoreService；DeploymentController bootstrap/reconcile 不依赖它，P2e 救援 CLI 仍可使用窄 DeploymentController API。
- Artifact signature/provenance/SBOM/revocation admission、跨 Node revision-tagged incident correlation 与 exporter；最小 Artifact compatibility admission 已在 P5，P6b 补 Release/audit 闭环。
- OTel-compatible exporter 与标准 logging；Exporter/Collector 是诊断消费者，不是 Evidence owner。

### 分离规则

- Receipt/Evidence 是权威记录；Trace、Log、Metric 是可采样诊断信号。
- P2 raw Inspection facts 的 owner 仍是 RuntimeHost/Domain/Mailbox；P6a/P6b 只聚合、关联、持久化和导出，不接管运行所有权。
- exporter 失败不伪造 Evidence 成功，不阻塞本地 Safety。
- InspectionService 是只读投影 owner，不成为 Node enrollment、Deployment、lease 或 source-health owner；federated role 故障不阻止 node-local query。
- Evidence 不承诺全局总序、区块链或所有系统状态的 universal event sourcing；storage-full 行为必须在安全运行包络中显式定义。
- OpsService 只拥有 operation record，不拥有被操作系统；它不 import RuntimeHost/Driver/Artifact installer 私有实现，不持有 raw Fabric/Secret，所有操作经 Authority 与 actual owner 的本地准入并产生分层 Receipt。
- TUI、CLI 与 Web Console 都是 clients。只有 Web Console 必经 ConsoleGateway；TUI/CLI 直接使用 InspectionClient/OpsClient。连续 media/XR/teleop 流量不经过 OpsService。

### 验证与证据

- P6a local durable handoff 后进程/NodeDaemon 重启仍可查询。
- Evidence 远端不可用时先本地提交；高风险路径按明确策略 fail-closed。
- 能解释成功、拒绝、超时、`uncertain`、断连重连和缺失证据。
- 并发 snapshot 不触发 iterate-while-mutate。
- log/trace exporter 阻塞不污染控制路径。
- P6b 在分区、重复上传、远端 store epoch 换代和 replication lag 下不丢失本地权威 ref，也不合并两个 Session writer。
- distributed convergence gate 覆盖跨 Node AgentSession takeover 的旧 writer fencing/CAS、EvidenceRef 可解析性，以及既有 physical effect 不因接管而重复。
- kill federated Inspection role 后 node-local query 继续；恢复后按 source revision/epoch/cursor 重建，不把旧 cache 报为 fresh。
- kill/restart OpsService 后 DeploymentController、RuntimeHost、Continuity 与 Safety 继续；同 ControlRequestId/digest 不重复 side effect，同 ID/不同 digest 被拒绝。
- owner timeout 形成 `Uncertain → query/reconcile`，OpsReceipt 可追溯到 actual-owner Receipt/EvidenceRef；缺 owner terminal evidence 不报告 success。
- contract/import test 证明 OpsService 无 RuntimeHost 私有实现、raw Fabric、Driver、Artifact installer、明文 Secret 或隐式 SSH/shell fallback。

## 12. P7：TUI

### 结果

- TUI 只作为 OpsClient + InspectionClient。
- 展示 desired/observed 差异、Node/RuntimeHost 状态、Mailbox 压力、binding epoch、Authority/Lease/Safety 和 Receipt 时间线。
- 所有变更操作先展示目标、权限需求和预期影响，再通过 OpsProtocol 提交 ControlRequest；物理 Command 仍由实际 CommandEndpoint 与 Authority/Resource/Safety 链处理。

### 验证与证据

- TUI 不 import RuntimeHost、Service 或数据库内部对象，不经 ConsoleGateway 绕行 Web BFF。
- 断网与数据缺失显示 unknown/stale，不沿用绿色状态。
- 一次成功与一次失败事务可完全由公开协议解释。
- 非交互测试覆盖 screen model；至少一个真实终端 smoke test 使用 TUI 实际实现语言的 locked build/run 路径，Rust TUI 不经 uv，Python TUI 继续经 uv。

## 13. P8：ROS2Gateway

P8 是生态接入，不引入 ParaEGOX 自研 DDS stack。

### 结果

- standalone `zenoh-bridge-ros2dds` 接入存量 ROS2/DDS；窄 ROS2Gateway 转换声明的 topic/service/action/type/TF/lifecycle/parameter。
- 受控新 ROS2 节点另行评估 `rmw_zenoh`；它与 ros2dds bridge 是互斥 DeploymentProfile。
- ROS ingress 的 Principal、Command、deadline、idempotency、Authority、Lease、Safety 与 Receipt 映射。
- 原生非 ROS2 DDS 只有出现具体设备、IDL、QoS 和验收需求后才建立 DDSGateway。

### 验证与证据

- topic、service、action、TF 和 lifecycle 分别有契约测试。
- 未声明 ROS endpoint 不会自动成为 ParaEGOX Port、ProvidedService 或 CapabilityGrant。
- ROS 发起的物理 Command 不能绕过完整 EnforcementPoint。
- 不形成 DDS 直连与 bridge 重复路径或环路；两种 DeploymentProfile 明确拒绝混用。

## 14. P9：SpatialMap 与 Semantic Navigation

P9 不扩大 Kernel；它建立在 P1 已验证的 FrameRef/Unit/ClockDomain/Calibration/Device contracts 上，只增加空间域自己的 owner 与引用：

- `FrameGraphRef`、`SpatialMapRef`、`MapEpoch`、`SemanticRegionRef`。
- `SemanticRegionRef` 的身份始终作用于 `SpatialMapRef + MapEpoch`。
- 空间位置影响网络或部署时，通过显式 RoutingPolicy/Placement constraint 连接，不复用 Zenoh 标识。
- 不创建 Robot、RobotView、Embodiment、FrameDomain 或统一 Grounding 大对象。
- 只有出现真实 Agent/Driver 消费者后，才研究短生命周期 `GroundingSession` 或显式 grounding relation；它不是 transport/live Binding，也不拥有资源、地图、Node 或 Agent 生命周期。

验证至少覆盖地图换代、旧 SemanticRegionRef 失效、空间观测 freshness，以及跨地图同名区域不能误绑定。

## 15. H1：条件式 Hardware Enablement Gate

H1 不是 P3 的自然延伸，也不是“把 simulator URL 换成设备地址”。它只能在 P6b 完成后、针对具体设备与受限 ODD 单独执行：

- Driver/adapter 只产生 raw observed identity/session/firmware/ABI/config/hardware facts 与 commissioning/binding Receipt；DeviceService 独占地把这些事实与 Deployment desired binding/PhysicalAssembly、CalibrationRef/revision、mode/safety 输入原子派生为 DeviceReadinessSnapshot，Driver 不能自证 Ready。
- real Driver 执行 P3 已定义的 contract conformance，再执行 HIL fault/timing matrix；fake/sim 结果不能代签。
- Safety owner 证明独立 E-Stop/kill/deadman/limit/safe output、Runtime/Fabric wedge 和电源/网络故障路径。
- Evidence owner 证明 local durable handoff、incident snapshot 与 storage-full 行为；Operator 演练 rollback、人工接管和 physical isolation。
- Product owner 提供 hazard analysis、ODD/Operating Envelope 与变更影响；Release owner 是最终 gate owner，签发 `HardwareEnablementReceipt`，绑定 DeviceSession/PhysicalAssembly/Deployment/Artifact/Safety policy revision、允许资源/动作/速度/区域、证据 digest、expiry 和撤销条件。
- 目标 Node 的 local `HardwareActivationGate` 验证 Receipt、当前 `RuntimePlanSlice` 与 `DeviceReadinessSnapshot`，再安装 `HardwareActivationRef/HardwareActivationEpoch`；real Driver activation 和每个真实 `PhysicalCommandEnvelope` 都必须引用它，不能只在部署时检查一次。

Receipt 到期/撤销、readiness、ODD/Operating Envelope、Deployment/Artifact/Safety revision 或限制变化时，本地 gate 立即推进 activation epoch 并 disarm/inhibit/stop；该动作即使 Fabric 分区也不等待远端。首个 H1 只允许最小资源、最小速度/力和明确区域内的 canary write；它不是通用“real hardware enabled”布尔值。
`HardwareEnablementReceipt` 只是 Deployment/Release admission 条件，不是 CapabilityGrant、Lease 或 SafetyDecision；H1 通过后每一条真实 Command 仍走完整 Authority/Resource/Safety/Enforcement 链。

## 16. P3 后的正式 Program 分支

Kernel Foundation 不把 Agent、World 或外部协议继续塞入 Kernel。完成 P3 的本地模拟物理 spine 后立即分叉：local Agent vertical slice、P4/P5 distributed Fabric 与 P6a local Evidence/node-local Inspection 可以并行；P5 + P6a 再进入 P6b federated Inspection + OpsService，随后汇合为 distributed Agent execution。不等待双主机和完整 OpsService 后才第一次验证 Agent 契约，但 Agent 的 durable physical effect 必须等待 P6a local commit。

1. **Agent Execution Plane**：DeckRun-bound local durable AgentSession/WAL、AgentHarness、每个 Run 固定的 immutable `RunExecutionSnapshot`、首次模型调用即生成的 ContextManifest、独立 ToolDefinition/ToolProviderDeclaration/ToolAdmissionDecision、committed desired ToolBinding、以 authenticated runtime readiness facts 解析 Provider 的 immutable ToolCatalogSnapshot/ToolView、Tool/Operation、task issuer 的 `OutcomeRequirement`、independent Verifier 与最小 Eval。首个切片只包含一个 Agent Card、一个 Model adapter、一个经 Trust/Policy admission 且显式绑定唯一 Provider 的 read-only Tool、一个模拟物理 Operation 和完整 Authority/EffectReceipt/Verification 链；Memory 等真实跨 Session 纠错需求出现后再加。
2. **Grounding & World Plane**：从 P1 physical contracts 发展 Calibration/FrameGraph/PhysicalAssembly、World/SpatialMap 与 semantic navigation；Memory 不冒充当前 World truth。
3. **Ecosystem Gateways**：依次建设 ROS2、MCP、A2A adapter；外部 Task/Agent Card/capabilities 不成为内部 Session、Card 或 CapabilityGrant 的权威。
4. **Operator & Web Interaction**：W0 先冻结 managed Gateway/exposure/typed endpoint 与限定 browser/peer/XR session contract；O1/O2 按 local read-only Console→federated Inspection/OpsService 推进，R1/R2 按 view-only WebRTC media→WebXR view-only 推进，R3 只连接模拟 Controller-role CardInstance/Actuator。WebRTC 是 Gateway 外部腿，WebXR 不是 Transport；媒体、XR input、administrative ControlRequest 和 physical teleoperation 使用不同 owner/Receipt。R4 真实遥操作仍逐设备经过 H1，不因浏览器链路可用自动解锁。

### 16.1 Agent Tool vertical slice gates

Tool 分支按以下顺序推进；完整证据和反例见 [Tool 定义、Provider 绑定与调用边界研究](../research/tool-definition-provider-binding-and-invocation.md)：

1. **AT0 — ADR/fixture gate**：用 ASR/TTS、read-only query、stateful stream 与 simulated physical Tool fixture 冻结 ToolDefinition、ToolProviderDeclaration、ToolAdmissionDecision、ToolBinding、ToolSet、Catalog/View 与 Invocation/Attempt 的公共名称和版本 owner。ADR 接受前不创建公共 `tools/` 包、Tool Registry daemon、通用 ToolService 或自动 failover manager。
2. **AT1 — static read-only**：一个 immutable ToolDefinition、一个 Card-backed 或当前已准入 CoreService/ServiceSpec-backed Provider、一个绑定 definition/provider/Artifact/policy revision 的 ToolAdmissionDecision、一个 committed desired ToolBinding、一个以 authenticated ready Provider fact 解析的 ToolCatalogSnapshot/ToolView 和一个可恢复的 read-only Attempt。client Attempt journal 与 provider acceptance/dedup journal 分离；`Dispatched` 只表示 exact envelope 已交给 transport/send boundary，Provider 在 effect 前持久准入并返回 acceptance ref。缺失、重复或不兼容 Provider 在任何调用副作用前 fail-fast；observed facts 不写回 DeploymentPlan，不引入 application-owned 或 DeckRun-scoped Service。
3. **AT2 — cardinality/conflict**：验证一个 Provider 导出多个 Tool、两个 Provider 实现同一 Tool 但 Profile 显式选一、同名不同 digest 和旧 ProviderInstanceRef/generation 均被稳定拒绝；同时用 fixture 判定是否还需要独立 ToolBindingEpoch，若需要则与 PortBinding BindingEpoch 分域。运行期不按名称、注册顺序或 heartbeat 选路。
4. **AT3 — MCPGateway**：把外部 MCP descriptor/endpoint 转为待准入 definition/provider candidate，分离 MCP endpoint、Provider 与 Tool identity，并验证重复 server name、断连、重注册和 endpoint 聚合。
5. **AT4 — composite/streaming**：一个 composite read-only Provider 独占 provider-side acceptance/dedup/subcall journal、子调用 correlation、cancel/reconcile 与 Tool 级终态，client Attempt journal 仍归调用方；没有 prepare/commit/barrier 证据时暴露 partial/`Uncertain` 而不宣称跨故障域原子。一个有界 streaming Tool 验证 sequence/cursor/backpressure 和 provider stickiness；无 state-transfer 协议不跨 Provider resume。
6. **AT5 — simulated physical**：Tool 只提交 typed OperationSpec，走完整 Authority、Lease/fencing、Safety、Enforcement、EffectReceipt 与 `Uncertain → reconcile`；Driver restart、timeout、迟到 result、旧 generation 和撤销均不触发透明 replay。

动态 Registry、运行期自动 Provider 排序/failover、跨 Provider state transfer 和真实硬件 Tool 不属于 AT1；它们只能在 AT2–AT5 证据后分别立项。

首个 AgentSession 是 DeckRun-bound 的 append-only event stream：它可以跨同一 DeckRun 内的 CardInstance/进程重启恢复，但 Deck workload terminal 时必须 seal，产生 retention/GC Receipt，并且新 DeckRun 不自动续接。跨 DeckRun、升级或重新安装继续会话必须先证明稳定 owner：产品安装私有 owner 触发 A0；真正的平台/租户 owner 需先通过独立 CoreService/tenant ownership ADR 与隔离 Harness。Context 是某次模型调用的可重建投影，DeckRun 是一次 Deck 工作负载运行，三者不能合并。每个 AgentRun 开始时固定 `RunExecutionSnapshot`，绑定 DeploymentRevision、Harness、Model、ToolCatalogSnapshot、ContextPolicy、SandboxPolicy 与 Artifact digests；每个 InvocationAttempt 再固定 Catalog digest、ToolBindingId、ProviderInstanceRef/generation 与 Artifact digest，以及经 AT2 证明确有必要时的 ToolBindingEpoch。同一 Run 的后续 Attempt 不静默切换 revision 或 Provider；撤销/无法解析发生在 dispatch 前时 fail-closed，发生在 dispatch 后时不得宣称 effect 未发生，而是进入 cancel/query/reconciliation 或 `Uncertain`。ContextMaterializer 拥有选择/顺序/裁剪/转换/output digest，Context item 标记 instruction authority/content trust/provenance/freshness/data boundary；Tool/Sensor/Memory 文本默认是 untrusted data。AgentHarness 不成为第二个 RuntimeHost：Agent owner 管 semantic budget/cancellation intent，Runtime 强制 OS/resource budget 和取消升级，Security/Sandbox owner 管 profile/egress/Secret policy，Authority/Resource/Safety 管物理 effect。任何 write/physical/irreversible Tool 的 `Uncertain` 结果默认 reconcile，不 replay。

不要建立一个 AgentSession 总 FSM：Session 只有 writer epoch/CAS 和 Open→Sealed，Run/Turn/Step 记录 waiting/cancel/progress，InvocationAttempt 使用 `Prepared → IntentCommitted → Dispatched → (ResultCommitted | Uncertain → Reconciled/Abandoned)`。Task issuer/delegator 拥有不可被 Agent 降低的 `OutcomeRequirement`；Verifier 在执行前准入并解析为只能保持或加强 Requirement 的 immutable `VerificationSpec`，把两者 digest 一起绑定 AgentRun/Operation，冲突或不可验证时先拒绝/澄清。physical EffectReceipt 由指定 effect protocol owner 组装；首切片可以是 Driver EnforcementPoint，但必须引用 AuthorityDecision、Lease/fence、SafetyDecision 与下游 applied/completion proof，不能用 Tool/Driver 文本自证。OutcomeClaim 由 Agent 拥有；VerificationSpec 固定 evidence source/threshold/window/failure policy，Spec/Attempt/Result 由独立 principal/lifecycle 的在线 Verifier 拥有。Eval Task/Trial/Trajectory/EnvironmentOutcome/GraderResult/Suite 由离线/CI/仿真 owner 拥有并固定 suite/grader revision、calibration 与统计定义；同一 Harness 自评不能冒充独立验证。

通用 Workflow/Graph Service、多 Agent coordination、Marketplace、WASI plugin runtime、Fleet/Cluster、rolling migration 与多 DeploymentController HA 只有满足 [分布式具身 Agent OS 缺口研究](../research/distributed-embodied-agent-os-gap-analysis.md)中的触发条件后才单独立项。

## 17. 测试分层与基准

| 层级 | 证明什么 | 不能替代 |
| --- | --- | --- |
| Unit | 纯契约、时钟、状态机、策略与 Mailbox | mock integration |
| Contract | Rust/Python wire golden/error vectors，以及 PortBinding test fixture 与 Zenoh route 对稳定 Message/Port/Delivery/Mailbox suite 的一致性 | 强迫 Gateway 冒充等价 Backend或把生成类型当 Schema authority |
| Integration | RuntimeHost、ProcessDomain、Zenoh、Evidence 生命周期 | 人工看 log |
| Gateway/Browser | typed InspectionClient/OpsClient、HTTPS/session/signaling、WebRTC/XR、bounded media/input、真实 browser smoke | 浏览器 mock、Transport ACK 或单次 happy path |
| Scenario | trusted Observation→CommandEndpoint→Decision refs→Lease→SafetyIsland→EffectReceipt/Verifier | 跳过故障注入或把 fake/sim 外推为 H1 |
| Manual/Bench | Jetson/机器人上的延迟、Wi-Fi、SHM 和恢复 | 每次 CI 的唯一证据 |

基准至少记录 p50/p95/p99/p99.9/max latency、async-reactor/control-tick lag、queue wait、message age、items/bytes、queued/inflight/IPC credits/retained bytes、drop/reject/evict/expire、copy/serialization、Card invocation duration、actual threads/processes/native pools、CPU、RSS slope、FD/SHM、reconnect、restart/quarantine、reconciliation 和 cleanup/shutdown time，并区分 PortBinding test fixture、Zenoh `session-local`、`host-local` 与 `remote`。Operator/Web 切片另记录 Console snapshot freshness/watch backlog/OPS Receipt latency、WebRTC setup/ICE/RTT/jitter/loss/bitrate/media sample age/codec queue、XR input age/rate/reject/sequence gap/stream generation/deadman expiry，以及 Peer/Task/FD 在关闭后的回收。每份证据固定 workload/profile、硬件/OS、Rust toolchain/target triple/libc/CPU features/crate features、wire/worker protocol、Python/runtime dependencies、browser 版本、预热、重复次数、原始样本、统计/置信区间和可复现命令。控制门槛从最小业务 deadline 和目标硬件基准推导，不先写一个跨平台“魔法毫秒数”。

## 18. 发布、降级与回滚

- 初期只有 development/local profile，不宣称生产功能安全。
- 每阶段可回退到前一已验证 profile；不保留半启用双写或隐式 compatibility shim。S7 reference RuntimeHost/store额外绑定exact `RuntimeBuildIdentityV1`、store-pinned `RuntimeBuildDescriptorV1`/singleton manifest与独立fixture artifact digest，不能把一般“阶段回退”解释成同store binary downgrade。
- P2a/P2b/P2c/P2d/P2e 分别启用；DeploymentController rollout/reconcile 先 report-only，再经过 Harness apply。S7 workload内容回到旧逻辑只能使用更高revision并遵守exact CAS/high-water；同一build上的Loop变化必须先empty terminal exact-zero，再启动后继revision。任何binary变化或v5→v4-only必须empty terminal→decommission/封存旧RuntimeHostId/store→初始化new identity/store，不能原地upgrade/downgrade或隐藏fallback。
- 一般 DeploymentRevision rollback 通过 retire 新 scope/重建旧已验证 scope，不保留原地可变配置、双执行计划或双写兼容通道；S7的mandatory canonical empty head、operation history、writer fence和revision high-water不能映射为`None`或由旧binary读取，除非后继ADR定义显式migrator/ownership transfer。
- DeploymentProfile 的 executor/CPU/dispatch 专家 override 必须绑定目标平台、基准、原因、有效范围和回滚值，不写回 CardDefinition 或 Deck。
- Fabric 失败时，受影响 Binding 明确进入 unavailable/degraded；独立、预先部署的本地 Safety/最低自治链按资源策略继续或 fail-closed，不把同一 Binding 隐式切换到 local-only route，也不双写。
- Telemetry exporter 可关闭或降采样；Evidence 只能切换到已验证 durable sink，不能退化为 log。
- Authority、Lease、Safety 和物理写路径只能显式、限时、可审计地切换 development policy，禁止隐藏 default-allow。
- P8 Gateway 停止后，原生 Zenoh Fabric 和本地安全闭环继续工作。
- Console/Web media/XR Gateway 停止后，本地 Runtime、安全闭环和最低自治继续；外部 session 明确 unavailable，旧 peer/stream generation 被 fence，不隐式回退到 MJPEG、raw Zenoh 或双 transport。

## 19. 评审清单

- [ ] Kernel 最小环境不依赖 Zenoh、ROS2、OTel、数据库、模型或硬件。
- [ ] Kernel 不包含 VFS、URI scheme registry、万能 ObjectRef、Card、Deck、DeploymentPlan/Revision/DeploymentController、Agent、领域 Graph/Graph Engine、Memory 或 World；若条件抽取 Graph Foundation，它只有两个独立生产消费者且仅含无状态纯结构算法。
- [ ] 未建立 Graph Store/Service/Query Router、GraphKind/opaque metadata、公共 Graph identity/revision/digest/schema 或 `execute(arbitrary_graph)`；Deck、Service、Runtime、Agent、Evidence、World 各自拥有 typed semantics。
- [ ] DeploymentPlanner 是纯确定性计算，DeploymentController 是每 scope 唯一 desired-state 写 owner；DeploymentController 不兼任 Deck resolver、RuntimeHost lifecycle/recovery owner、Fabric、Authority、Safety、Artifact 或 OpsService ControlRequest store。
- [ ] DeploymentController 全称不缩写为裸 Controller；进程候选名为 `paraegox-deploymentd`，DeploymentReconciler/DeploymentRolloutEngine 无独立 identity/store/tenure/I/O/lifecycle/write authority。
- [ ] RuntimeSliceProjector 只产生 tenure-neutral canonical target Slice；RuntimeApplyEnvelopeBuilder 将 DeploymentWriterRef/DeploymentWriterEpoch/WriterTenureProof 映射为 runtime-owned PlanWriterContext，且 writer tenure 变化不改变 plan/slice digest；Controller request-auth signer 把 exact request commit-before-send，CLI/OpsService/GitOps 不绕过 DeploymentController 直接写 RuntimeHost。
- [ ] DeploymentTenureAuthority 是 proof 唯一签发 owner，其 private key 不交给 DeploymentController；Controller request-auth key 与之隔离。RuntimeHost 在副作用前持久化最高 PlanWriterRef/PlanWriterEpoch/proof-envelope digest。低 epoch、同 epoch 异 writer/proof、无受信 proof 的高 epoch 及第二个 active source scope均被拒绝。
- [ ] RuntimeHost journal 原子保存RuntimeHost/clock generation、完整AdmissionState/fingerprint、source-revision high-water、writer_fence、exact request/Slice、prepared、active desired head、live materialization、recovery action、owned-resource ledger、`RawActionOutcomeLatch`、`TerminalOutcomeSelection`与terminal operations；tenure-only/full-admission分事务，post-intent supersede阻塞新effect。normal apply只用`FirstActionIntent`，restart recovery只用`RecoveryPlannedNoEffects → StartCallIntent`。`OneSourceLoop`只在readiness成功时原子写desired+live+terminal；`EmptyDeactivate`仅在live/nonzero时first commit同时写intent+`NoNewAdmission`+empty head+retiring-old，再于exact-zero写terminal，already-exact-zero且无action/resource时走无intent/callback fast path，empty head不因资源为零丢失。
- [ ] CapabilityGrant、ServiceContract 与 Node/Fabric/Device FeatureReport 分离；CardDefinition/Card/Deck 只声明 Requirement，不保存 Grant/token/Secret/旧 report。
- [ ] RuntimeHost 只消费 authenticated canonical RuntimePlanSlice request；Schema/protocol owner 与 value owner 分离，target/revision/expected-active/source+slice digest/operation id/deadline/writer tenure 验证及 import check 阻止第二真相和反向依赖。
- [ ] Deck DataLink、ServiceDependency和activation constraint分型；S7 Planner在candidate/commit前拒绝它们及所有一般activation/Ingress/Thread/Process shape，只提交fixed manifest/profile的idle Loop或empty。未来successor若开放一般assembly，DeploymentPlan.execution/Slice必须完整覆盖readiness/activation/consumer-ingress/producer-egress/dependency-loss/drain；RuntimeAssemblyEngine永不从Link猜测或建立第二desired graph。
- [ ] parallel Link、SCC/cycle witness 与 Service DAG 验证确定；无 feedback contract 的 cyclic Deck 在副作用前失败；Graph Foundation 若存在则无领域 import、I/O、loader、digest、execution state、retry/checkpoint/Receipt。
- [ ] 配置通过 ConfigSnapshot digest 与新 revision 激活；owner-specific Artifact/Evidence/Secret/Blob refs 未演化成 Service Locator。
- [ ] RuntimeHost 只拥有本地 ExecutionDomain；实现与公共 Schema 不定义 `RemoteDomain`。
- [ ] CardDefinition/Card/Deck/Kernel 公共 Schema 不定义 Lane、`thread_lane`、`process_lane`、线程或 PID 编排。
- [ ] CardDefinition 是带 Artifact export/entrypoint 引用的不可变可复用能力合同，不是业务基类；公共领域模型不引入 Handler/Factory，CardInstance 默认托管私有实现对象。
- [ ] CardDefinition/Card 没有直接 `run()`，作者不能 new/start CardInstance；internal Harness 只消费 production path 生成的 canonical request，未形成 StandaloneRunner 或第二种 Runtime desired input。
- [ ] L1 普通实现对象单测、L2 Runtime component Harness 与 P2e one-subject idle Deck→empty smoke 分层；每层 evidence level/limitations 明确，前一层成功不冒充后一层 Ready 或 production 等价，P2e通过也不冒充source processing/streaming。
- [ ] In/Out 只声明 PortSpec；Port、Link、`DeploymentPlan.bindings` 和 live PortBinding 的 owner 分离，未建立并行 BindingPlan。
- [ ] bind 前验证 direction、interaction、cardinality 与 Schema；PortBinding test fixture 和 Zenoh 各 locality 共享契约，首版之外的交互 fail-fast。
- [ ] 同一 DeploymentRevision/活动 BindingEpoch 下每个 BindingId 只有一条 active route；route replacement 可完成或 rollback，无双投、implicit fallback 或内容 echo 去重。
- [ ] CardDefinition ExecutionRequirements、Link DeliveryProfile、DeploymentPlan.execution 和 Runtime observed facts 只有各自唯一 owner。
- [ ] Runtime observed Domain、PID/TID、loop、capacity 和 epoch 与计划不一致时不能 Ready。
- [ ] 每个异步边界只有一个 items/bytes/age 有界的语义 Mailbox；内部 dispatch 不复制 payload queue。
- [ ] Fabric ingress frames、queued Messages、inflight、executor/IPC credits、child work 和 retained payload bytes 同时有界，无 permit 不 dequeue/不创建 detached work。
- [ ] LoopDomain admission 拒绝 sync blocker、CPU spin 和 unknown native/device workload。
- [x] ThreadDomain timeout 不被报告为线程已取消或终止；late result 受 DomainEpoch/InvocationId fencing。
- [x] ThreadDomain wedged 不无预算补 worker，容量扣减和 degraded/poisoned 状态可观测。
- [ ] ProcessDomain 具有持续 heartbeat、TERM/KILL、process-tree cleanup、restart budget/quarantine 和不同故障域 watchdog。
- [ ] Process crash 只产生 RuntimeFailureFact，在途副作用为 Uncertain 且默认不 replay；device completion/reset/fencing 与 process lifecycle 分离。
- [ ] RuntimeOwnershipTree、FailureContainmentSpec、collateral restart 范围和 revision prepare/activate/drain/retire/rollback 进入计划与 Inspection；RecoveryEngine 只求值，RuntimeHost 是唯一 lifecycle action owner。
- [ ] LivenessSpec/State 与 Health/Readiness 分型；RecoveryPolicy 区分 RestartPolicy 与 InvocationRecoveryPolicy，默认 no replay。
- [ ] startup/liveness/readiness/health/test observation 不使用万能 `probe() -> bool`；事实绑定 source revision、instance/generation/epoch/freshness，Card 不能写 Ready 终态或直接执行 restart。
- [ ] `Agent`/`Supervisor` 仅用于 Agent 层；Runtime/Node 包、Schema、类型和进程名没有 `NodeAgent`、`NodeSupervisor`、`RuntimeSupervisor`、`SupervisionSpec` 或 `runtime/supervision`。
- [ ] CardDefinition/Card 不拥有或复制 ToolDefinition；Artifact 可导出 sibling CardDefinition 与 ToolProviderDeclaration，一个 Provider 可导出多个 Tool，一个 Tool 可有多个兼容 Provider。
- [ ] ToolDefinition author claim 与 ToolAdmissionDecision effective risk 分离；policy 变化产生新 decision/catalog revision，不改写 ToolDefinition/version/digest。
- [ ] ToolCatalogSnapshot 从 committed desired ToolBinding、ToolAdmissionDecision 与 authenticated runtime readiness facts 解析具体 Provider；observed facts 不写回 DeploymentPlan，projection 不二次选择 Provider，ToolView 只控制可见性，每次 Invocation 在调用点重新授权。
- [ ] 每个 InvocationAttempt 固定 Catalog digest、ToolBindingId、恰好一个 ProviderInstanceRef/generation 与 Artifact；只有 AT2 证明 DeploymentRevision/provider generation 不足时才引入 ToolBindingEpoch，且不复用 PortBinding BindingEpoch。write/physical/irreversible 在 dispatch 后未知时先 reconcile，不透明 failover/replay。
- [ ] client Attempt journal 与 provider acceptance/dedup/subcall journal 分离；`Dispatched`、Provider durable acceptance、effect handoff、terminal Result/Receipt 的每个 crash window 均有故障注入，ack/timeout 不串层。
- [ ] Provider host lifecycle、desired config、Probe/Inspection 与业务 Tool action 分离；MCP endpoint/server name 不充当内部 Tool/Provider identity。
- [ ] 通用 L0–L3 Card Harness 强制 effect-denied 且无切换开关；有设备副作用的诊断进入独立 L4/H1 Operation，并同时要求有效 HardwareEnablementReceipt、HardwareActivationRef/Epoch、fresh DeviceReadiness/ODD、Authority/Lease/Safety/Enforcement、下游 safety gate 与最终 effect evidence；simulation proof 不能满足 H1。
- [ ] control criticality 经 policy 授权/容量 reservation，deadline 的 queue/run/effect/cleanup 预算与 overrun action 可验证。
- [ ] Node identity/spec/status 与各层 epoch/revision 已分离。
- [ ] NodeDaemon 只拥有 Node facts、NodeManagementEndpoint 与 Runtime endpoint discovery；RuntimeHost 独占 RuntimeApplyEndpoint/admission/Receipt，NodeDaemon 不能形成第二 apply gate。
- [ ] NodeIncarnation 表示 NodeDaemon publication/registration tenure；RuntimeHost restart 不推进它，旧 NodeDaemon facts/replies 被 fencing，NodeDaemon loss 只产生 stale/partitioned。
- [ ] DeploymentController bootstrap 不依赖其控制的 RuntimeHost 自部署；DeploymentController 失联不使本地 Safety 失效，也不授权 RuntimeHost 自行生成 revision。
- [ ] 裸 `Region`、裸 `Topology`、`ComputeNode` 和 `FabricRegion` 未进入公共契约。
- [ ] Site 当前只是非权威 `site_hint`，不参与权限、路由或身份推导。
- [ ] Transport callback 只做固定成本检查与非阻塞 handoff，不直接完整 decode/validate 或运行 Card 领域调用；pre-validation encoded frame 只进入有界 Fabric ingress buffer，不冒充 Message/Mailbox/accepted，验证成功后才进入 target Mailbox。
- [ ] source Message accepted、fabric egress、encoded frame staged、Message validated/target Mailbox admitted、Card invocation completion 和 physical effect 的结果分阶段；原生 Fabric 权限只按 scope 指向 Fabric resource 的 CapabilityGrant 授予。
- [ ] CapabilityGrant、Lease/Fencing、Safety 与执行判定有不同 owner/Receipt。
- [ ] AuthorityDecision/SafetyDecision 绑定具体 command/operation/resource/device/audience/expiry；Command A 的 Decision 不能用于 B，requested/applied operation 与 completion evidence 分开。
- [ ] Device/Frame/Unit/ClockDomain/CalibrationRef+revision/uncertainty 进入首个物理链；DeviceReadinessSnapshot 原子绑定 desired+observed 输入，device/mode/safety epoch 换代后旧 Ready/Command 被拒绝。
- [ ] Power/Thermal/Operating Envelope 的 raw、derived、evaluation、inhibit owner 分离，所需 fact refs/freshness 绑定 Command；变更后旧 Decision 不可继续写。
- [ ] Driver 只拥有 normal-command 准入与 setpoint submission；下游 safety gate 独占 enable/inhibit/clamp/safe-output 与 applied-output ack，不形成平级双 writer。独立进程/clock/deadman 的模拟 safety island 在 Runtime/Fabric wedge/kill 时仍可经 direct path 收敛。
- [ ] ContinuityEpisode 的 trigger/offline_since/debounce/close/restart 语义阻止网络抖动无限刷新自治窗口。
- [ ] fencing 和 idempotency 在真实副作用 owner 处跨 RuntimeHost 重启有效。
- [ ] Command 没有透明 transport retry；`uncertain` 可查询。
- [ ] Fabric storage/self-inspection 未吞并 Evidence、World、Memory 或全局 Inspection。
- [ ] Trace/Log/Metric 未被当作权威 Evidence。
- [ ] InspectionService 与 OpsService 分离：前者只拥有 projection，后者只拥有 ControlRequest journal；二者都不接管 source/desired/effect truth，也不建立每 Node 一个 OpsService/ConsoleGateway 的不变量。
- [ ] TUI/CLI/Web Console 读取只通过 InspectionProtocol，写入只通过 OpsProtocol；只有 Web Console 经 ConsoleGateway。Gateway cache 不拥有 truth，客户端不能访问 RuntimeHost 私有对象或 raw Fabric。
- [ ] OpsService 同 ID/同 digest 幂等、同 ID/不同 digest conflict，timeout/restart 后 `Uncertain → query/reconcile`；无 raw Fabric、Runtime 私有 import、明文 Secret、shell/SSH/install fallback，terminal OpsReceipt 引用 actual-owner Receipt。
- [ ] WebRTC/WebSocket/HTTP 只作为 Gateway 外部腿，WebXR 不作为 Transport；Zenoh 仍是唯一内部 production Fabric。
- [ ] Camera/Driver/Card 实现代码不自行启动公网 WebRTC/MJPEG/static server、private event loop 或 daemon thread；Gateway workload 有明确 Runtime/external workload manager owner。
- [ ] browser auth、WebRTC peer、XR input stream 和 teleoperation session 分型；旧 peer/stream generation 被 fence，短期连接不生成 DeploymentRevision。
- [ ] 同一活动 XR/control input 不同时接受 WebSocket 与 DataChannel；disconnect stop packet 丢失时本地 lease/deadman/Safety 仍收敛，Transport ACK 不冒充 EffectReceipt。
- [ ] `FrameRef` 只表示物理坐标系；媒体使用 `MediaSample`/`EncodedVideoSample` 与 BlobRef/BufferRef，不建立视频 `FrameRef`。
- [ ] ROS2/DDS 只通过声明的 Gateway/DeploymentProfile 接入。
- [ ] P3–P5 没有连接 real actuator；若启用真实硬件，存在 P6b 后、具体设备限定且可失效的 H1 HardwareEnablementReceipt，并由 local HardwareActivationGate 在每次真实 Command 强制执行。
- [ ] Robot/Embodiment 未成为 Kernel、Runtime 或 Deployment owner。
- [ ] Rust 工程由 pinned toolchain + Cargo/Cargo.lock 管理，Python 工程由 uv/uv.lock 管理；CI 分开验证并运行跨语言 conformance，不存在第三份 lock authority。

## 20. 开放问题

以下问题需要实现或目标平台证据，不能在 Kernel 中凭偏好写死：

- P4/P8/硬件里程碑所需的 Rust toolchain/target triple/libc/CPU feature、Python ABI、ROS2、Ubuntu、Jetson、GPU 与设备 SDK 完整版本矩阵；P0 已冻结最小 Rust core + Python tooling/SDK 开发 CI 基线。
- Zenoh `session-local`/SHM 的高带宽 payload ownership 与 reference 方案；只有目标硬件 p99.9/CPU/copy 证据证明其不满足 SLO，才研究互斥同进程 production route 的数据结构。
- `physical/contracts` 的最终包边界、已冻结 ObservationHeader 的字段编码/扩展规则，以及 PhysicalAssemblySpec/Calibration Artifact 的最小字段；Observation 的 owner、时间/空间/校准/origin 语义不再开放。
- Call/Query/Operation/Tool 的公共 invocation envelope 与各自 effect/terminal contract，以及 fan-in/fan-out、routing、partial acceptance 与 merge owner；首版不以无关联 In/Out 提前模拟。
- Schema registry、兼容窗口和显式 Adapter/Converter 的锁定与部署方式。
- ExecutionRequirements/DeliveryProfile 的具体字段名与版本策略；arrival/payload envelope、run-bound provenance、max_inflight 和 unknown 的保守准入语义在 P1 前必须冻结。
- 目标平台的 dispatch algorithm、priority weight、max burst、control SLO 和 Thread/Process/native-pool budget。
- [平台兼容研究](../research/platform-compatibility-ports-and-host-feature-profiles.md) 的 PC0–PC3 具体准入：Linux production containment 采用何种 cgroup v2/pidfd 组合，macOS 能提供何种 aggregate resource/escaped-descendant 证明，Windows Named Pipe/SID/Job Object/service/durable publish backend 的证据边界，以及何时达到共享 crate 抽取门；Python worker 内部采用 spawn 还是经验证的 forkserver只属于其adapter compatibility matrix，禁止依赖隐式默认或在线程启动后fork。
- one-subject 开发入口的最终 CLI 名称、临时 DeckSpec/DeckLock 导出策略，以及是否有两个独立消费者足以准入 public CardHarness 或 readiness/health contribution Schema；在此之前不创建公共 runner、test manifest 或 Probe registry。
- 何种真实 worst-case deadline 足以引入 native RealtimeDomain。
- Authority 与 ResourceCoordinator 首版是否同进程。
- fencing/idempotency ledger 的最小持久实现。
- 每类受控资源的 partition behavior、Deployment 编译的 ContinuityProfile 与本地安全输入。
- Zenoh keyspace、schema registry、版本窗口、SHM 与多优先级流参数。
- Evidence durable handoff 使用 WAL、SQLite 还是独立进程。
- NodeDaemon 与 RuntimeHost 在 development profile 是否共进程，以及单 Node 是否允许多个 RuntimeHost；production watchdog 保持不同故障域，且冻结唯一 host restart-budget/quarantine mutation owner。
- 何时出现足以引入 `SiteRef`、多 DeploymentController 共识/leader election/scope sharding 或原生 DDSGateway 的真实需求；single-writer DeploymentController 本身不再是开放问题。
- DeckRun-bound durable AgentSession 的 single-writer/storage、seal 后 retention/GC 实现、Agent 作为 Card 还是平台/tenant 共享 CoreService，以及首个 Tool/Verifier vertical slice 的产品边界；跨 DeckRun/升级/重新安装续接不是无 owner 的实现选项，产品安装私有 owner 触发 A0，平台/tenant owner 需单独裁决。
- ToolProviderDeclaration/ToolAdmissionDecision/ToolBinding/ToolSet 的最终名称与 Schema、Card/CoreService/Gateway-backed provider reference、Provider readiness/generation/withdrawal、stream/state transfer、pure/read retry 证明与 physical reconciliation query；AT0 ADR 前不建立公共动态 Registry。
- 首个真实产品能否始终由一个 Deck 完整表达；多 Deck 统一 release、稳定 installation identity、同一产品多次隔离安装或跨 DeckRun 私有状态任一出现时，触发 Application/Installation research fixture 与 Proposed ADR，不在 Foundation 中预造答案。
- 同一 DeckRun 内多张 Card 共用的临时能力是否仍可由一张 Card 表达；只有真实 ServiceContract consumer 出现后才研究 DeckRun-scoped ServiceSpec，它不自动触发 Application。
- 跨 Deck interaction 的第一个真实消费者应使用 ServiceContract、限定 Gateway endpoint 还是未来 Port export；不得直接寻址另一 Deck 的内部 Card。
- managed Gateway 是复用 Runtime 中性 instance envelope、使用 ServiceInstance 外壳，还是需要新的限定 contract；不因角色名直接新增万能 `GatewayInstance`。
- Card Port 与非 Card Gateway endpoint 使用 Deployment-owned exposure/binding、窄 ServiceContract 还是限定 Gateway endpoint contract；Deck 不直接选择 WebRTC、TURN 或 placement。
- `WebRealtimeGateway` 候选组合是否拆成 MediaGateway 与 XRInputGateway；无论名字如何都不表示硬实时或 `RealtimeDomain`。
- browser auth、WebRTC peer、XR input stream 和未来 teleoperation session 的最终 Schema/epoch 命名与恢复边界；泛 `ExternalSessionId` 不作为答案。
- custom JSEP/WHIP/WHEP、P2P/SFU、DataChannel/WebSocket、encoder placement、TURN topology/credential issuer、identity provider 和 recording owner 的目标设备基准与产品 profile。
- `MediaSample`/`EncodedVideoSample` 的 BlobRef/BufferRef、Zenoh SHM、codec negotiation、zero-copy 与 retained payload 生命周期。

## 21. 计划完成判据

只有以下证据全部可复现，本计划才可由 Draft 进入完成状态：

- pinned Rust toolchain + `Cargo.lock` 可重建 Rust core，`uv sync --locked` 可重建 Python tooling/SDK 环境；cargo metadata + Python import governance 自动验证 Kernel/Runtime/Deployment 依赖方向。
- Rust/Python 对 RuntimeApplyRequest（包括additive `RuntimeApplyEnvelopeV2`、PXTE v4/PXAR v5 source-only/empty successor）、`RuntimeBuildDescriptorV1`、singleton `RuntimeArtifactCompatibilityManifestV1`、`RuntimeBuildIdentityV1`、Message、Receipt和最小worker protocol的canonical bytes/digest/version/unknown-field/error vectors一致；v4只含exact manifest projection、fixed profile、zero/one reference Loop records与zero-binding PXTA。旧PXTE/PXAR vectors保持不变。
- `paraegox-runtime-contracts`是Envelope v2、descriptor/identity/manifest/projection及Runtime bootstrap/query Schema、digest/transcript/bounds的唯一owner；`paraegox-deployment`是`acquire_tenure` IPC framing/auth transcript的唯一owner，而`WriterTenureProof` value仍复用runtime-contracts。release pipeline唯一产生descriptor；system installer/install operation严格消费descriptor+artifact且一次唯一产生singleton manifest，拒绝prebuilt manifest，并byte-identically交Runtime initializer与operator/Controller/Planner immutable ingress，Planner不重建、bootstrap只校验。internal codec只在该operator install surface已登记owner/consumers/first test、`deploymentd` producer与Runtime executable consumer同批接通并register后promotion；release generator若只是internal build tool不虚假public，若是repository executable则据实登记，initializer/receipt也不被暗中扩大成public surface。
- Runtime不安装/import deployment/decks包时仍可应用RuntimeApplyRequest fixture；missing/zero/wrong `expected_runtime_store_instance_id`或same-target old-store replay在任何mutation前失败，corrupt/noncanonical sequence-1 descriptor/manifest bytes或digest mismatch失败，binary-derived compiled actual与store-pinned identity mismatch在bootstrap/query-ready前quarantine；startup不重新hash executable或读取side file/config。错误target/revision/expected-active/source/slice/operation/deadline/writer/auth同样零副作用失败。
- invalid/corrupt/undecodable/unknown-version snapshot不绑定bootstrap/query-ready且不返回authenticated state；只有validated snapshot与durable startup generation commit后的compatibility/recovery/ownership quarantine可携带exact identity返回authenticated `Indeterminate`。Controller将服务不可用记为自己的`Indeterminate`，但不从config/corrupt bytes猜identity，也不把历史Active或`Unknown`冒充current Ready。
- P2e DeploymentPlanner对相同canonical输入产生稳定DeploymentPlanCandidate/PlanContentDigest；DeploymentController原子提交allocation/revision/plan，durable pin authenticated bootstrap的RuntimeHostId/store/channel与compiled/store identities，再以隔离request-auth key签署`RuntimeApplyEnvelopeV2`并commit-before-send。RuntimeSliceProjector产生tenure-neutral Slice，expected store不改变plan/slice digest。single-writer Controller通过plan→commit→project→sign→apply/query→reconcile闭环，crash/restart、fresh-store重放、timeout和重复提交不产生revision回退、第二action或双live generation。
- DeckCompiler 对相同 DeckSpec/resolver inputs 产生自包含、字节稳定的 DeckLock；canonical directed-multigraph DeckTopology 与 resolved closure 都受 digest 覆盖，parallel Link 不丢失，Canvas View State 不影响 digest，DeploymentPlanner 没有独立 topology 输入。SCC/cycle witness 对输入顺序稳定，无 feedback contract 的 cyclic Deck 在副作用前失败。
- ServiceDependencyGraph的环与所有DataLink/Port/Binding/Ingress输入在S7 candidate/commit前以稳定reason拒绝；S7 Plan/Slice只承载exact manifest projection、fixed profile、reference Loop records或canonical empty。一般activation/readiness/consumer-ingress/producer-egress/dependency-loss/drain必须等待新successor并完整进入plan/slice digest。
- RuntimeAssemblyEngine 对fixed idle fixture的prepare/start/readiness/desired+live commit、restart reassembly与条件式empty head-first drain/zero-fast-path故障注入证明callback不replay、old desired或canonical empty head正确保留、无mixed revision/双live generation。normal apply在resource/callback前durable `FirstActionIntent`，recovery另走`RecoveryPlannedNoEffects → StartCallIntent`；intent/head构造前与publish后/effect前都检查deadline。raw callback/deadline/cancel fact在cleanup前先以`RawActionOutcomeLatch`持久，cleanup+exact-zero后再以一次`terminal_selection_observed_at`采样形成`TerminalOutcomeSelection`；`now == deadline`为timeout，selection后fsync/回复跨deadline不重分类。Runtime不import decks/deployment。S7没有Binding、steady Message/streaming或一般rollback路径。
- DeploymentController restart取得更高tenure时，committed plan revision/source digest/slice digest不变；Runtime先原子durable推进host/clock generation并invalidate old live，再以tenure-only transaction推进AdmissionState tenure/fence，以独立full-admission transaction原子提交request/temporal state、source-revision high-water、exact request/Slice和`PreparedNoEffects`。post-intent旧operation进入`SupersededReconcileRequired`并阻塞new effects；pre-intent crash证明no-effects，normal apply保留old head，recovery可用fresh action/generations重建；`EmptyDeactivate`只有live/nonzero路径先提交intent+`NoNewAdmission`+head再retire，exact-zero fast path不创建intent，revision/CAS high-water都不丢失。
- 相同 DeckSpec/resolver inputs 先产生同一 byte-identical DeckLock；同一 DeckLock 在至少两个 DeploymentProfile/target facts 上产生不同但可解释的 `DeploymentPlan.execution`，不修改 DeckLock 或应用消息语义。
- 同一 DeckLock 的 Port/Link 编译为同 revision 的 `DeploymentPlan.bindings`，PortBinding test fixture 与 Zenoh `session-local`、`host-local`、`remote` 安装通过同一 conformance suite，BindingEpoch 只随 live binding 生命周期变化。
- P2a/P2b/P2c/P2d/P2e 分别通过 overload、blocking、wedge、crash、kill、stale-result、resource cleanup、DeploymentController restart 和 desired/observed Harness。
- L1 两个普通实现对象不共享状态；P2a 明确只证明 binding/Mailbox；P2b–P2d internal single-subject Harness 只消费 canonical request，覆盖 callback/Domain seam、startup failure、deadline、旧代次、真实 child cleanup 与 local/diagnostic evidence 限制，不宣称正式 AssemblyEngine；P2e只补fixed idle Loop/empty profile并让显式one-subject Deck走完整正式链，不外推为一般assembly、source processing或streaming。所有fixture Card在编译前声明，没有`Card.run()`、StandaloneRunner或test proof进入production trust。
- startup、RuntimeHost/Domain liveness、exact-revision readiness、ongoing health 与 test observation 的 truth table 拒绝 stale revision/generation/epoch/freshness；Card semantic evidence 不能覆盖缺失依赖或直接触发 recovery。
- 联合过载下 Fabric ingress frames/queued Messages/inflight/IPC credits/retained bytes 不越界，pre-validation frame 不计 Message accepted，offer outcome 与 admitted lifecycle 守恒，无 detached Task 或隐藏 backlog。
- process/device crash 不伪造 effect Receipt、不重放 Uncertain 工作，设备重置/接管和 DeploymentRevision 原子替换通过故障注入。
- CardDefinition/Card/Deck 不含公共 Lane，Runtime 内部 lane-like dispatch 只有在 A/B 基准证明收益时存在。
- 首版没有无 owner 的 Application identity/controller/store/service；deactivate/replace Deck workload 并使 DeckRun terminal 不停止平台 CoreService，CardInstance replacement 不继承未声明全局状态。显式非支持 owner/lifetime 字段返回稳定 reason code，opaque code 不做虚假的语义推断，未声明 raw persistence/egress 由 sandbox 拒绝；跨 DeckRun 私有状态触发 A0。
- 同一 byte-identical DeckLock 在不同 immutable ServiceSpec inventory/target facts 下产生不同且可解释的 provider candidate，而 DeckLock/digest 保持不变。
- 本地仿真物理闭环通过 Decision-to-command binding、clamp requested/applied、独立 safety process、control-mode safe handoff、原子 DeviceReadiness、device hotplug/换代、ContinuityEpisode flapping/restart 与 uncertain reconciliation；不把该证据外推为真实硬件安全。
- 同一契约在同主机双进程和双主机 Zenoh placement 运行，并通过分区/重连/旧代次测试。
- fencing 与 idempotency 在执行 owner 重启后仍能阻止旧副作用。
- P6a 在远端不可用时完成 local durable Evidence/node-local Inspection；P6b 在 P5 后完成 replication/lag、federated Inspection、OpsService journal 与旧 Session writer fencing，并能解释成功、拒绝、证据缺失和接管不重复 effect。OpsService/ConsoleGateway 故障不停止 DeploymentController reconcile、RuntimeHost 或本地 Safety。
- 首个 Agent slice 使用独立 ToolDefinition/ToolProviderDeclaration/ToolAdmissionDecision、committed desired ToolBinding、由 authenticated readiness facts 解析 Provider 的 immutable RunExecutionSnapshot/ToolCatalogSnapshot、ContextMaterializer、分层 Session/Run/Turn/Step/InvocationAttempt、task-owned OutcomeRequirement、预提交 VerificationSpec 和独立 Verifier；同一 Run 不静默换 revision/Provider，每个 Attempt 固定 Catalog/ToolBinding/ProviderInstanceRef/generation/Artifact，dispatch 前撤销 fail-closed、dispatch 后 `Uncertain → reconcile`，Tool/Sensor/Memory 数据不能升级成指令。
- 未产生 H1 Receipt/本地 HardwareActivation 时没有真实 actuator write；若执行 H1，Receipt 与 commissioning/readiness/HIL/ODD/限制绑定，每条 Command 绑定 activation ref/epoch，任一 revision/epoch/fact 变化即使分区也在本地失效并 disarm。
- ROS2Gateway 不绕过 Authority/Lease/Safety，也不与原生 Fabric 混成对等 Backend。
- 文档中的公共接口都链接真实实现和测试证据。

在此之前，本文件只表示建设顺序，不是功能完成声明。
