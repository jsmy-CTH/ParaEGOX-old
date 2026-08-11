# ParaEGOX Kernel、RuntimeHost 与 Core Services 架构基线

> 状态：Draft
> 日期：2026-08-03
> 范围：ParaEGOX 基础架构、运行边界、平台服务、执行与运维
> 实现状态：部分实现；已提交基线的 P0/B1、S2/B2、P2a/S3、P2b/S4、P2c/S5 与 P2d/S6 本地纵向切片已落地；P2e/S7 的 Controller SF5 与 Linux SF6 已提交到 `4334a59`，但 CI `30787514013` 仍有 3 个跨 service-account Python probe 失败，当前修正待 fresh Ubuntu CI；当前工作树另有窄 DeveloperLocal Fabric/Agent/TUI 与 embedded/static Model mechanism，一般 managed CoreService、managed Model 与 plugin admission 仍未实现，后续内容仍须由真实代码和证据推进
> 架构裁决：[ADR-0001 — DeploymentController、DeploymentPlan 与 Runtime 边界](../adr/ADR-0001-deployment-controller-boundary.md)
> 术语裁决：[ADR-0002 — CardDefinition、Card 与 CardInstance 术语边界](../adr/ADR-0002-card-definition-terminology.md)
> 实现语言裁决：[ADR-0006 — Rust-first 核心机制与多语言工作负载边界](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md)
> Deck/Application 决策：[ADR-0004 — Deck 工作负载、DeckLock 与 Application 准入边界](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md)，状态为 Accepted
> Graph/assembly 决策：[ADR-0005 — typed domain graphs 与 Runtime assembly 边界](../adr/ADR-0005-typed-domain-graphs-and-runtime-assembly-boundary.md)，状态为 Accepted
> P2e persistence 决策：[ADR-0007 — reference journal 与 crash recovery](../adr/ADR-0007-p2e-reference-journal-and-crash-recovery.md)，状态为 Accepted
> P2e successor 决策：[ADR-0008 — PXTE v4/PXAR v5 source-only/empty reference target](../adr/ADR-0008-pxte-v4-pxar-v5-subject-ingress-separation.md)，状态为 Accepted
> Agent 对话决策：[ADR-0009 — Agent 对话、类型化客户端与 Console 边界](../adr/ADR-0009-agent-conversation-and-client-boundary.md)，状态为 Accepted
> 浏览器边界研究：[Web Console、WebRTC、WebXR 与交互式 Gateway 边界](../research/web-console-webrtc-webxr-gateway-boundaries.md)
> Application 边界研究：[Application、Deck、Card 与 Service 边界](../research/application-deck-card-service-boundaries.md)
> Graph 边界研究：[Graph Foundation、领域图与执行边界](../research/graph-foundation-and-domain-execution-boundaries.md)
> 平台兼容研究：[平台兼容 Port、host-platform support evidence 与 OS Backend 边界](../research/platform-compatibility-ports-and-host-feature-profiles.md)；platform RP/Research Complete 只表示研究输入完成；S7-E executable evidence 已具备，但 PCA 尚未准入，PC0–PC3 仍不代表统一平台层或跨平台 production support 已实现

## 一句话结论

ParaEGOX 采用“小 Kernel、独立 RuntimeHost、Kernel 外 Deployment control plane、声明式 ServiceDependencyGraph 与 DeckLock 内嵌 DeckTopology”分层结构，并以 `Rust-first mechanisms, polyglot workloads` 建设首个生产参考实现：Kernel 只提供稳定机制，领域图各自拥有语义与执行器，DeploymentPlanner 只纯计算非权威 candidate，DeploymentController 独占跨 Node committed desired plan、revision 与 reconciliation，Rust RuntimeHost 通过内部 RuntimeAssemblyEngine 应用本 Node 的 canonical Slice 并负责执行、生命周期和恢复，Python/C++/native 工作负载通过受管 ProcessDomain 或独立 Service/Gateway 接入；系统契约按分布式和语言中立方式设计，本地安全路径不依赖远端系统或普通 Runtime 存活。

## 当前代码快照（2026-08-03）

最后完成并经 CI 验证的基线已推进到 P2e/S7-F Runtime tranche。S7-E 主提交 `14d0012` 及 follow-up 至 `1ed704c` 接通 descriptor/install/initializer、RuntimeHost PXBR/PXAR v5、fixed native Loop→Empty owner、Runtime-signed PXRT 与 `paraegox-deploymentd` one-shot commit/tenure/bootstrap/apply/Empty 路径，Ubuntu CI `30748840399` 验证了 exact Linux ext4 process fixture。S7-F 随后由 `4cbba96`/`fc96534` 与 CI `30753147231` 增加 authenticated PXQR/PXQS endpoint，由 `860d023`/`2494687` 与 CI `30780053169` 增加 Controller owner-private exact query journal/client，再由主提交 `7d7db38` 及 follow-up 至 `f32700f` 将 Runtime normal payload推进到 v5、加入 lossless stopped v4→v5 migration，并在 listener capability 发布前完成 fixed `OneSourceLoop`/`EmptyDeactivate` restart reassembly；Ubuntu CI `30782979187` 已成功。`reconcile-reference-once-v1`、bounded one-shot Controller reconciliation 与 SF6 Linux process/system scenarios 已提交到 `4334a59`。CI `30787514013` 的 Rust job 成功，governance/tests job 为 372 passed/3 failed；失败均位于 Runtime socket readiness probe，当前工作树已有跨 service-account `/usr/bin/python3` 修正，但尚无 fresh Ubuntu CI。因此 SF5/SF6 与整个 S7 仍不能标 complete。三个 store 的 `ProductionReference` 仍只接受 exact Linux ext4；macOS 对 SF6 profile 只 skip，APFS 在 PC1 补齐 FD-anchored extended-ACL 与 crash-durability 证据前 fail closed，Windows 仍 unsupported。

Runtime 已有 private/experimental local POSIX ProcessDomain baseline，覆盖单 domain/单 instance/单 invocation、strict generation/sequence fencing、IPC credit/retained bytes、heartbeat/liveness、failure facts、`Uncertain`/no-replay、bounded cooperative stop→TERM→KILL、same-process-group cleanup、identity-guarded workspace cleanup、exact-zero cleanup proof 与 restart/backoff/quarantine；Linux 另有有界 `/proc` RSS/FD/process-tree/CPU census。Rust 与 Python reference worker 提供 crash、partial frame、ignore cancellation/TERM、stale generation 和 same-group grandchild 证据。

`paraegox-runtime-host` 不再只是 idle composition root：除 S6 的可选 PXHW v1 reactor-progress endpoint 与独立 POSIX service-manager/watchdog reference executable 外，精确 `serve-bootstrap-v1` 已能重开 payload-v5 durable Runtime journal，在发布 listener 前完成 fixed Loop/Empty restart reassembly，并在同一 authenticated channel 服务 PXBR/PXQR/PXAR；已提交的 `paraegox-deploymentd` 能完成 tenure/bootstrap 与 Loop/Empty apply。它仍不是一般 RuntimeAssemblyEngine：fixed restart 不能外推为 Thread/Process recovery，也没有一般 plan assembly、Fabric、NodeDaemon 或 post-idle workload ingress，因此仍不是完整可用的具身 Agent OS。temporal 仍只从首次 target ingress 安装本地 deadline，admission ledger 也无 eviction/compaction/rollover。

S7-F 只剩当前 service-account probe 修正的 fresh Ubuntu CI 门。与该外部证据并行，ADR-0009 已准入下一条用户可见主线：一般 managed CoreService → FabricService/Zenoh host-local → Agent/Model + AgentConversationProtocol → 本机聊天 TUI；P5 双 Node 在 Fabric 合同稳定后并行，不阻塞第一次本机对话。详细依赖见[当前阶段开发执行看板 §4.1](../plans/current-stage-development.md#41-s7-f-current-dag当前活动阶段部分实现)。

2026-08-05 工作树中的 Model 只具备 embedded、in-process、statically linked mechanism：
`paraegox-model` 提供 provider-neutral 的有界调用，`paraegox-model-openai` 隔离 OpenAI HTTP/TLS、JSON、
Secret 与 provider-specific failure，DeveloperLocal composition 做精确配置/SecretRef binding 和
resolved Secret ownership。ADR-0009 已接受并在当前工作树本地验证 exact static Adapter
registration/selection，使 deterministic fixture 与 OpenAI 进入同一
provider selection → Runtime resolver → compiled profile mapping → registry → `ModelService` 路径，并
移除 production fixture 的 resolver/registry bypass；该批仍不构成独立 Model 进程、Runtime-managed
Model desired plan、跨进程合同、自动路由/fallback 或 plugin admission。后续 managed Model 与 plugin
profile 只能按
[ADR-0009 的分期修订](../adr/ADR-0009-agent-conversation-and-client-boundary.md)
分别取得实现证据。

该静态 registry core 已在工作树实现为 `ModelAdapterIdV1`、`ModelAdapterMetadataV1`、
`ModelAdapterSelectionV1`、`ModelAdapterFactoryV1` 与 `ModelAdapterRegistryV1`；fixture/OpenAI 的统一
Runtime resolver → compiled profile mapping → registry → `ModelService` composition 接线及 fixture
bypass 移除已完成本地 focused validation，A1 状态为 implemented / local-validated，不是
production-ready。当前 signed provider selection 不含 adapter ID/version/capability，不能把
registry-local exact match 外推为端到端 adapter identity binding。

S6 不是 production containment 证明：launch resolution 是 trusted caller seam，signed sandbox/target profile 没有 production adapter 强制执行；Linux census 不是 cgroup containment，非 Linux 不宣称 live resource enforcement；只证明同 process group descendant。RuntimeHost 被不可捕获 SIGKILL 后，独立 process group 的 ProcessDomain worker 没有外部枚举/清理 owner；unexpected Drop fallback reaper 未注册、未 join、不能签发 cleanup proof。C++、cgroup/job-object/pidfd/full sandbox 与一般 assembly/recovery 仍未实现。S7-F 已补 fixed in-process Loop/Empty restart recovery，但不扩大到一般 Ingress/Thread/Process/streaming assembly，也不解决上述 ProcessDomain orphan/containment 边界；公共 Deck/Graph/persistence/owner 变更仍先经过架构决策门。

## 1. 背景与来源

ParaEGOX 是面向机器人、具身智能体和物理 Grounding 的操作系统式基础设施。项目基于 PhanthyMotus 的公开代码历史和 Apache-2.0 许可证重新建设，但目标不是对 PhanthyMotus 做原样扩展，也不是对 EAGOS 做改名重构。

三类输入承担不同作用：

| 输入 | ParaEGOX 使用什么 | 不使用什么 |
| --- | --- | --- |
| PhanthyMotus | 合法代码血缘、ROS2 和设备接入经验，以及 Card/Canvas 的配置使用心智模型 | 单体 Agent Core、全局事件队列、Canvas 中心架构、MCP 内核化；Deck 不是其既有类型 |
| EAGOS | 内核运行经验、并发与进程故障、准入与回执、OPS、物理安全和资源治理的经验 | 源代码、测试、配置、文档正文、EAGOS `Module`/`Bundle` 语义和全局 Runtime 模型 |
| ParaEGOX | 独立的物理语义、执行模型、服务边界、消息契约和运维协议 | 对任一旧项目做一对一对象映射 |

EAGOS 的重要教训不是“功能不够”，而是能力不断沉入 kernel 后，Kernel、Runtime、Bus 和通用组件基类逐渐承担了整个系统的职责。PhanthyMotus 的重要教训是，把 ROS2、Agent Loop、Web API、Channel、配置和驱动管理集中在一个进程中虽然启动简单，但会把生命周期和故障域绑定在一起。

ParaEGOX 因此从边界、失败语义和可验证性开始，而不是从移植功能开始。

## 2. 目标

本架构需要支持：

- 机器人传感、处理、控制和执行的确定性数据路径。
- Agent、Graph、Memory、Model、World 等能力按需作为平台服务启动。
- 从单进程开发逐步扩展到多进程、远端计算和多机器人节点。
- 对阻塞、卡死、崩溃、断网、积压、重复和乱序做明确处理。
- 物理写操作的身份、权限、期限、资源占用和执行结果可审计。
- OPS 和 TUI 能回答“当前是什么状态”“为什么失败”“下一步安全操作是什么”。
- 核心契约保持小而稳定，上层能力可以独立替换。

### 2.1 分布式目标

ParaEGOX 明确以分布式具身智能系统为目标，而不是把远端通信作为后加功能。采用 `distributed-first contracts、local-first execution、Zenoh-native Fabric`：同一套契约覆盖单进程、单 Node 多进程、设备—边缘—云和多 Site 协同；网络分区时本地 Safety 与最低自治路径仍有明确行为。完整模型见 [ParaEGOX 分布式系统模型](distributed-system-model.md)。

## 3. 非目标

第一阶段不尝试：

- 建设完整 Agent 框架、World Model 或通用 Graph 引擎。
- 把所有逻辑服务拆成独立进程或网络微服务。
- 把 MCP、ROS2、Zenoh 或特定传输协议定义为 Kernel 原语。
- 用一个通用类型表示平台服务、算法处理、设备驱动和 Agent 工作流。
- 建立兼容 EAGOS `Module` 或 `Bundle` 的迁移层。
- 在没有运行证据前设计大规模插件市场、集群调度或自动恢复策略。

## 4. 系统级不变量

后续实现必须遵守以下红线：

1. 不存在能够访问全系统服务的全局 `Runtime` Service Locator。
2. 产品级运行实现位于顶层 `runtime/`，不建立 `kernel/runtime/`。
3. Kernel 不加载 Model、Memory、Agent、World、任何领域 Graph/Graph Engine、OPS 或 TUI；只有两个独立真实消费者证明复用后，才可按 ADR 准入无状态的纯图结构算法。
4. Core Service 不通过继承获得 Bus、线程、Probe、Tool、TF 等混合能力。
5. Transport 回调不直接执行用户或领域逻辑。
6. 所有工作队列有界，溢出策略必须显式并与消息语义匹配。
7. 线程卡死不能被报告为已经终止；需要硬隔离的工作必须进入进程边界。
8. 物理 Command 不得静默丢弃、隐式重试或绕过 Authority Gate。
9. `ServiceDependencyGraph` 和 `DeckTopology` 是语义、owner 与环策略都不同的领域图；不能 lowering 成一套持久化或可执行的通用 GraphDef。
10. Agent、Graph 和普通应用无权停止或替换平台关键 Core Service。
11. OpsService/TUI/Console 读取只通过稳定 InspectionProtocol；变更只通过 OpsProtocol ControlRequest 与真实 owner typed API，不读取运行对象内部字段。
12. 文档、配置或计划不构成功能完成证据。
13. 分布式契约必须显式处理 partial failure、epoch、deadline、重复、迟到和 `uncertain`，不能把 reliable transport 当作 exactly-once。
14. Node、Deck、`site_hint`、Zenoh 拓扑位置或“本机”都不产生隐式信任；物理写入始终经过 Authority、Resource 与 Safety 边界。
15. `Robot` 不作为 Kernel、Runtime 或 Deployment 的基础 owner；产品需要时只能由独立权威关系投影。
16. `Lane` 不作为 CardDefinition、Card、Deck、Kernel 或公共 Runtime Schema 的一等概念；调度分类由 DeploymentPlanner 编译进 `DeploymentPlan.execution`，内部 ready queue 不拥有线程、进程、第二份 payload queue 或生命周期。
17. Card 实现代码不自行创建 Thread、Process、event loop 或无 owner background Task；所有执行资源和预算归 RuntimeHost。
18. `DeploymentPlan.execution` 是 desired execution 的唯一权威，RuntimeHost 必须报告实际 Domain、PID/TID、loop、capacity 和 epoch；不一致时不能 Ready。
19. 每个异步边界只有一个系统拥有的语义 Mailbox，并同时限制 items、bytes 和 max age；第三方/Transport buffer 必须观测但不能冒充交付契约。
20. 系统有界性覆盖 pre-validation Fabric ingress frames、queued Messages、dispatched/running、executor/IPC credits、child work 和 retained payload bytes；无 permit 不 dequeue、不创建 detached Task/Future。
21. RuntimeHost 与跨故障域 NodeDaemon 只能在各自观测范围记录真实 `RuntimeFailureFact`；RecoveryEngine 不能把进程/执行域事实冒充物理副作用的 `Failed` Receipt。已 handoff 且无权威终态证明的调用为 `Uncertain`，restart 默认不 replay。
22. DeploymentRevision 不原地修改活跃 Domain、Mailbox 或 Policy。S7只允许`OneSourceLoop`或`EmptyDeactivate`，拒绝Loop→Loop：先以更高revision提交empty并取得terminal exact-zero，再以另一更高revision启动；一般revision-tagged prepare/activate/drain/retire/rollback等待后继assembly contract。
23. ProcessDomain 只是地址空间故障边界，不自动拥有 GPU/device 生命周期；设备成功、重置与接管由真实 resource owner 的 fence/ack/health 证明。
24. CardDefinition 的 `In`/`Out` 只声明 transport-neutral Port 契约；CardDefinition/Card 不持有 Topic、Publisher、Session、queue 或 live binding。
25. Link 拥有本次交付意图，route/codec/key/admission boundary 编译进同一 revision 的 `DeploymentPlan.bindings`，Runtime 安装 `PortBinding`；不建立并列 BindingPlan。
26. 每个 CardInstance 默认托管一个私有 Card 实现对象；实现可以持有领域状态和提供生命周期回调，但生命周期身份、推进与恢复仍归 CardInstance/RuntimeHost。
27. Schema、required/cardinality 与 interaction compatibility 在 bind 前验证；Zenoh 的 `session-local`、`host-local`、`remote` route locality 与确定性 PortBinding test fixture 共享同一契约，未支持的动态或复杂交互 fail-fast。
28. 原生 Zenoh/raw Fabric 权限按 scope 指向 Fabric resource 的 `CapabilityGrant` 授予，不按 CardInstance、CoreService、Driver 或 Gateway 类别自动授予。
29. 同一 DeploymentRevision 与活动 BindingEpoch 内，一个 BindingId 只有一条接收新 Message 的 active route；route replacement 必须显式 prepare/activate/drain/retire 或 rollback，禁止 local+wire 双投、隐式 fallback 和基于内容的 echo 去重。
30. Zenoh callback 收到的 pre-validation encoded frame 不是 `Message`，也不得进入 target Mailbox；它只能进入 items/bytes/age 有界、可观测的 Fabric ingress buffer，验证成功后才构造 Message 并准入唯一语义 Mailbox。
31. 不建立 Kernel VFS、全局 URI scheme registry、万能 `ObjectRef` 或跨 owner `open(uri)`；Artifact、Evidence、Secret、Workspace、Blob/Buffer 与服务状态分别由 typed owner 解释。
32. `CapabilityGrant` 只表示授权；`ServiceContract` 表示服务接口；限定的 Feature report 表示 Node/Fabric/Device observed support，三者不共享基类、epoch、缓存或 registry。
33. Card、Deck、CardDefinition resolution、DeckTopology、DeploymentPlan、DeploymentRevision 与 DeploymentController 全部位于 Kernel 外；`deployment/` 拥有全局 plan/revision/placement/reconciliation，`runtime/contracts` 只拥有 `PlanProvenance` wire DTO、`RuntimePlanSlice/RuntimeApplyRequest` Schema 与 apply protocol。DeploymentController 签发由唯一 DeploymentPlan 规范投影出的不可变 slice value；RuntimeHost 不得反向 import deployment/decks。
34. 配置通过 ConfigSchema → CardProfile/SecretRef → DeckLock → DeploymentPlan ConfigSnapshot digest → immutable validated config 单向解析；首版配置变化产生新 DeploymentRevision，不原地热改。
35. Kernel `time/` 唯一拥有通用 ClockDomainRef、不可互换的 Monotonic/Wall/Sim Instant、ClockQuality 与 ClockMapping/ClockMappingRevision Schema/纯规则，不拥有运行 mapping value。Node time-sync adapter、Device/Driver adapter、ScenarioRunner 分别拥有其源时钟 mapping value/revision，并携带 producer identity/epoch、freshness 与 uncertainty；完整 FrameGraph/SpatialMap 不进入 Kernel，但首个物理闭环前必须在独立 `physical/contracts` 中冻结可信 ObservationHeader、Frame/transform revision、单位、measurement uncertainty、CalibrationRef、DeviceRef/incarnation/session 等领域契约并引用 Kernel time 值。
36. `Controller-role Card`（控制应用）只能经计划安装的 typed `OperationClient/CommandEndpoint` 提交有界、无透明 retry 的 Command，不能直接持有 Driver/Actuator 对象；该合同不是第二条 Bus，也不等于部署控制面的 DeploymentController。
37. `SafetyIslandAdapter` 只接入独立 MCU/PLC/设备安全岛的状态、许可与证据；E-Stop、protective stop、deadman、limit 和 collision inhibit 不依赖 control lease、Agent、Mailbox 或 Fabric 生效，不能由普通 Rust/Python 用户态 Service、ProcessDomain 或容器冒充。
38. P3–P5 的物理写验证只声明 simulation profile；首个真实 actuator 还需 local durable Evidence、HIL、独立 E-Stop/物理隔离与产品 hazard/ODD gate。
39. 浏览器、Console 和 XR 客户端不得直接获得 RuntimeHost 对象、Driver 句柄、原生 Zenoh Session、内部 keyspace 或隐式 raw Fabric 权限；HTTP/SSE/WebSocket/WebRTC 必须终止于显式 Gateway。
40. WebXR 是浏览器 XR API 与交互来源，不是 Transport；WebRTC 是外部 media/DataChannel 协议腿，不是与 Zenoh 并列的生产 Fabric backend，也不产生第二种 PortBinding。
41. Web Console 只通过 `ConsoleGateway → InspectionClient/OpsClient` 工作：读取是带 revision/observed-at/freshness 的非权威投影，写入形成受权 ControlRequest 与 terminal OpsReceipt，不能直接写 Runtime、PID、Zenoh key 或 Deployment store；TUI/CLI 使用相同 typed clients，不强制经过 Web Gateway。
42. Driver 与 Card 实现代码不自行启动公网 HTTP/WebSocket/WebRTC 服务、私有 event loop、daemon thread 或 PeerConnection lifecycle；协议 adapter 的执行资源仍需 Runtime managed-workload 或明确 external service-manager owner。
43. browser auth session、WebRTC peer、XR input stream 和未来 teleoperation session 使用各自限定的 identity/epoch；建连、TLS/DTLS 成功或 DataChannel ACK 不产生 Node/Card/Instance identity、CapabilityGrant 或物理成功 Receipt。
44. 遥操作断线安全依赖目标 Node 本地 lease/deadman/fencing/Safety 与下游 safe-output gate；disconnect callback 的 neutral/stop 只能是补充动作。WebSocket 与 DataChannel 对同一活动控制输入不得双活或隐式 fallback。
45. 长期 Gateway workload/exposure/config/placement 属于 Deployment desired state 的目标方向，短期 browser/PeerConnection/XR session 不产生 DeploymentRevision；Gateway 的 managed-instance envelope 与 Card Port 到 Gateway endpoint 的 binding contract 尚待 Proposed ADR，不能先伪装成 CardDefinition、Card 或 CoreService。
46. `Agent` 与 `Supervisor` 保留给 Agent 层语义；Runtime 与 Node 基础设施禁止声明 `NodeAgent`、`NodeSupervisor`、`RuntimeSupervisor`、`SupervisionSpec` 或 `runtime/supervision`。节点驻留管理进程只称 `NodeDaemon`。
47. 图结构与执行模型分离：不建立 `GraphKind + metadata`、通用 Graph Store/Service/Query Router 或 `execute(arbitrary_graph)`；DeckLock、DeploymentPlan、AgentRun、Evidence 与 World revision 继续由各 owner 定义。
48. Deck `DataLink` 不等于 ServiceDependency 或 activation dependency。S7 Planner在candidate/commit前拒绝DataLink、ServiceDependency、Ingress与一般activation shape；未来successor若开放它们，consumer ingress、provider readiness、producer egress、dependency-loss与drain order必须完整编译进`DeploymentPlan.execution`/Slice，RuntimeHost不从Link猜测。
49. RuntimeAssemblyEngine 只是 RuntimeHost 内部的 Slice apply mechanism，不是 CoreService、daemon、公共 Graph Engine 或第二 desired-state owner；S7只执行fixed idle Loop/empty profile，steady state只有idle LoopDomain/CardInstance且不经过assembly loop，未来steady-state Message也不得经过它。
50. Rust crate/process layout 服从既有 owner 边界，不反向定义领域结构；共同语言、同一 Cargo workspace、同一 binary image 或 development 共进程不产生共享 authority、生命周期或故障域。RuntimeHost、NodeDaemon 与 DeploymentController 即使都使用 Rust，也不能因此合并 owner。
51. 公共合同不得暴露 Rust memory layout/trait object/Tokio handle、Python object 或语言私有 queue/lock；Rust/Python/C++ 只实现同一版本化 Schema、canonical encoding、digest 与错误语义。
52. CoreService、Card、Gateway、Driver 和 DeploymentController 的身份不由语言决定；Rust 是 production mechanism reference 的默认实现，Python/C++ 仍是受管 workload 和生态实现语言。

## 5. 核心术语

### 5.1 采用的术语

| 术语 | 限定含义 |
| --- | --- |
| `Kernel` | 稳定、可组合、尽量纯的基础机制与契约集合 |
| `Node` | 可独立识别、管理和断连的计算单元；不是 Robot 或 Site |
| `NodeDaemon` | 每个 Node 的 OS-resident 管理进程角色；拥有 Node presence、NodeIncarnation、NodeStatus/NodeFeatureReport 与窄 NodeManagementEndpoint，不拥有 Deployment plan 或 Runtime apply admission；候选进程名为 `paraegox-noded` |
| `RuntimeHost` | Node 内的执行宿主，拥有执行域、生命周期、恢复动作和关闭协调；与 Node 不要求一对一 |
| `CoreService` | 长期运行、可共享、具有稳定 ServiceContract 接口的平台服务 |
| `Inspection` | 各真实 owner 运行事实的只读 snapshot/watch/projection 协议；不是事实、健康、Deployment 或 registry owner |
| `InspectionService` | 提供 node-local 或 federated Inspection projection 的 CoreService；只拥有 projection revision/cursor/cache/freshness，不拥有 source facts |
| `OPS` | logical control plane 内的运维领域与产品能力总称；不是单个运行对象、Card、每 Node daemon 或新的独立 plane |
| `OpsService` | 只拥有受权 ControlRequest 状态机、幂等 journal、进度与 terminal OpsReceipt 的 CoreService；不拥有被操作对象 |
| `OpsClient` | CLI、TUI、ConsoleGateway 或自动化使用的公开 OpsProtocol 客户端；不内嵌领域 executor |
| `CardDefinition` | 不可变、可复用、可版本化的能力合同，包含 Port、配置 Schema、Requirements、ExecutionRequirements 与 Artifact export/entrypoint 引用；不是业务基类、安装包或运行对象 |
| `Card` | 一个 CardDefinition 在 Deck 中的一次具名、配置使用；连接由 Deck Link 拥有 |
| `CardInstance` | Card 被 RuntimeHost 启动后的受管理运行身份和句柄 |
| `Deck` | Cards、Links 与 Requirements 组成的声明式可执行工作负载/组合单元；不等同 Product、Release 或 Installation |
| `DeckSpec` | 用户声明的 Deck 期望状态，不包含运行事实 |
| `DeckLock` | CardDefinition、Artifact candidate、Schema/Adapter、协议与声明平台兼容约束的精确解析结果；不含 live provider、Node Feature 匹配或 placement |
| `DeckRun` | 一次真实运行的稳定身份、CardInstance 集合和 Inspection 入口 |
| `Gateway` | 外部生态与 ParaEGOX 之间的显式语义、安全和故障边界；例如 ROS2Gateway |
| `ConsoleGateway` | 面向 Web Console 的窄 Gateway/BFF，只暴露公开 InspectionProtocol/OpsProtocol 与外部身份映射；不是每 Node 必备组件、全局 Runtime、运维或观测真值 owner |
| `Driver` | 设备、仿真器或具体硬件/SDK 的边界适配器 |
| `Message` | transport-neutral 的不可变逻辑 Envelope；发送侧只从通过 Schema/Port 校验的 payload 构造，接收侧只在 decode 与 Schema/principal/binding 准入成功后构造；带 MessageId、causality、deadline 与 trace context，不是 wire frame、queue、Bus 或子系统 |
| `messaging` | 实现 Message、Port、Delivery、Mailbox 与 binding 机制的包/子系统名；不是运行对象类型 |
| `In` / `Out` | CardDefinition 作者侧对单向输入/输出 Port 的声明；不是 Topic、queue 或运行连接 |
| `PortSpec` | transport-neutral 的端口定义，包含方向、Schema/版本、interaction、cardinality 和不可削弱约束 |
| `Link` | Deck 中从 Card.Out 到 Card.In 的连接，以及本次使用的 DeliveryProfile |
| `BindingId` | 编译后逻辑 binding 的稳定身份；首版一条解析后的静态 1:1 Link 对应一个 BindingId |
| `PortBinding` | Runtime 根据 RuntimePlanSlice 中的 binding assignment 安装的 live endpoint 关联和 observed route；拥有 BindingId-scoped BindingEpoch，但不拥有 DeckTopology 或第二份语义队列 |
| `ExecutionRequirements` | CardDefinition 声明的实现内在执行要求，包括调用模型、阻塞/native/device 风险、重入、取消和 minimum isolation；不是完整运行计划 |
| `Mailbox` | 一个目标异步边界的唯一语义 Message backlog/admission owner，拥有容量、顺序、freshness、overflow 和 enqueue result；只容纳已验证 Message，名称不改 |
| `OutstandingBudget` | 同时限制 queued 之外的 dispatched/running、child work、executor/IPC credit 和 retained bytes；不是另一个 queue |
| `ExecutionDomain` | RuntimeHost 拥有的本地 Loop、Thread 或 Process 执行与故障边界 |
| `RuntimeAssemblyEngine` | RuntimeHost 内部、无独立 identity 或 desired state 的 profile-specific Slice apply mechanism；S7只执行fixed idle Loop start/reassembly，以及live/nonzero empty head-first retire或already-exact-zero fast path，未来successor才可增加一般prepare/readiness/activate/drain/rollback；永不参与稳定数据面调度 |
| `RuntimeOwnershipTree` | `RuntimeHost → DomainInstance → CardInstance/ServiceInstance → InvocationScope` 的结构化所有权层级；不是独立 actor |
| `LivenessSpec` / `LivenessState` | 计划中的 bootstrap/heartbeat/control-responsiveness 判活合同，以及 Runtime/Node 实际观测到的活性事实 |
| `RecoveryPolicy` | 编译后的恢复规则；明确区分进程级 RestartPolicy 与副作用级 InvocationRecoveryPolicy，后者由 side-effect/restart-safe、idempotency/recovery owner 决定，默认不 replay |
| `FailureContainmentSpec` | 计划中允许的故障传播与 collateral restart 边界；必须与 DomainAssignment 一致 |
| `RecoveryEngine` | RuntimeHost 内无独立身份和生命周期的确定性状态机；根据 LivenessState、RuntimeFailureFact、RecoveryPolicy 与预算产生 RecoveryAction，由 RuntimeHost 执行 |
| `RuntimeFailureFact` | 带 observer/subject/epoch 的 crash、exit、heartbeat-missed、wedge、kill 或 cleanup 运行事实；child/Domain 事实由 RuntimeHost 记录，RuntimeHost 整体活性事实可由跨故障域 NodeDaemon 记录；不能冒充物理 Effect Receipt |
| `LoopDomain` | RuntimeHost 的 event-loop ExecutionDomain；不是 Agent 的认知循环 |
| `AgentLoop` | Agent 层的认知/决策循环；不拥有线程、进程或 Runtime 生命周期 |
| `AgentHarness` | Agent 层对 Session、AgentLoop、Model 与 Tool 的编排；不是 RuntimeHost 或测试 Harness |
| `FaultHarness` / `TestHarness` | 用 fake clock/fabric/model/tool/driver 与 fault injector 产生验证证据的测试设施 |
| `DeckTopology` | DeckSpec 的 Card、Port、Link 与 DeliveryProfile 经解析后的 canonical directed-multigraph 连接结构；内嵌于 DeckLock 并受其 digest 覆盖，不是 live execution graph |
| Product “Application” | 当前只作为产品/自然语言聚合，不是公共 identity；首个 profile 可由一个 Deck 完整表达，正式 Application 等待多 Deck/安装/私有持久状态证据 |
| `ServiceDependencyGraph` | CoreService 的 provides/requires、readiness 与依赖 DAG |
| `Graph Foundation` | 尚未准入的纯结构算法候选能力；只有至少两个独立生产消费者后才可抽取，不是 Graph Engine、CoreService、公共 Schema 或当前包名 |
| `Deployment` | 将 DeckLock 与 ServiceSpec 编译为 placement 与期望状态 |
| `DeploymentPlanner` | 纯、确定性的 deployment 计算组件；根据已解析声明、Node facts、policy 和 allocation snapshot 生成 `DeploymentPlanCandidate`，不持久化、不分发、不 reconcile |
| `DeploymentPlanCandidate` | Planner 的不可变非权威输出；包含同级字段 PlanContent、stable-ID allocation delta、diagnostics 与 PlanContentDigest，其中 digest 只覆盖 PlanContent；不含 DeploymentRevision、writer tenure、rollout 或 observed facts |
| `DeploymentController` | Kernel 外、按 DeploymentScope 单写的 desired-state owner；原子提交 allocation delta/DeploymentRevision/committed DeploymentPlan，投影 RuntimePlanSlice、协调 rollout 并 reconcile observed facts |
| `DeploymentReconciler` / `DeploymentRolloutEngine` | DeploymentController 内无独立身份、存储、tenure、I/O 或生命周期的纯决策组件；前者求值 desired/observed 差异，后者求值 rollout state transition |
| `DeploymentTenureAuthority` | Kernel 外 writer-tenure proof 的唯一签发 owner；原子推进 DeploymentWriterEpoch，tenure signing key 不交给 DeploymentController |
| `DeploymentPlan.bindings` | 编译后的 BindingId、endpoint、Zenoh route/locality、Schema/codec/keyspace、Fabric ingress limits 与 target Mailbox admission boundary；不是 live observed state |
| `DeploymentPlan.execution` | 根据 CardDefinition 要求、Deck/Link SLO、目标 Node facts 和 DeploymentProfile 编译出的 Domain、Mailbox、dispatch、budget、liveness、failure-containment 与 recovery 计划 |
| `Artifact` | 可安装、分发和校验的软件或模型产物 |
| `CardProfile` | Card 的参数、资源和环境配置；不承载 DeckTopology 或分发语义 |
| `DeploymentProfile` | placement、进程/容器、平台和 Gateway 组合；不能授予 Authority |
| `ZenohTopologyProfile` | Zenoh router/client、gateway 与连接的期望配置；不是业务区域身份 |
| `Receipt` | 对准入、拒绝、接受、执行、失败或恢复决策的结构化证据 |
| `CapabilityGrant` | Authority 签发的 audience-bound 授权值，只回答某 Principal 可请求哪些 resource operations |
| `ServiceContract` | CoreService 提供/需要的稳定版本化接口；由 ProvidedService 与 ServiceRequirement 建立依赖 |
| `FeatureReport` | NodeDaemon、FabricService 或 Driver 对实际支持特性的限定 observed report；公共类型使用 Node/Fabric/Device 前缀 |
| `RuntimePlanSlice` | Schema 属于 `runtime/contracts`，具体不可变 value 由 DeploymentController 通过纯 projector 产生；是某一 committed DeploymentPlan revision 面向目标 RuntimeHost 的 tenure-neutral canonical digest projection，不是第二份期望真相 |

`Agent` 与 `Supervisor` 保留给 Agent 层的认知主体、委派策略和人机协作语义；Runtime、Node 管理进程、包名、DTO 与进程名不得复用二者。`NodeDaemon` 的 daemon 表示 OS-resident 管理角色，不限定 Unix 部署方式；Windows service 或容器中仍使用同一逻辑名。`Topic` 可以用于命名发布通道，但不是 Port 的稳定身份，也不是所有通信的统一抽象。`Memory` 保留给领域/平台 Memory 能力，不使用 `MemoryPortBinding`、`MemoryBus` 等测试命名；生产绑定也不另命名为 `ZenohBinding`，公共 live 概念只有 `PortBinding`。确定性测试实现称 `PortBinding test fixture`，代码内部可叫 `FakePortBinding`，但不成为公共架构实体或生产部署选项。`Sensor`、`Processor`、`Controller-role`、`Actuator` 可以作为 Card 的角色描述，不形成继承层次；`Controller-role` 不等于 DeploymentController。部署 owner 在公共文档、API、status、log 和 metric 中永远写全 `DeploymentController`，不使用裸 `Controller`；deployment 领域 writer 类型使用 `DeploymentWriterRef/DeploymentWriterEpoch`，Runtime wire 使用 `PlanWriterRef/PlanWriterEpoch`。`Robot` 与 `Embodiment` 不建立基础类型。裸 `Region` 与裸 `Topology` 禁止进入公共契约。完整术语边界见 [CardDefinition、Card 与 Deck](../concepts/card-definition-card-deck.md)、[CardDefinition 输入输出、Port、Link 与运行绑定研究](../research/card-definition-ports-links-and-bindings.md)、[Node 与作用域边界](../concepts/node-and-scope-boundaries.md)以及 [ADR-0003 — OPS、OpsService 与 Inspection 操作边界](../adr/ADR-0003-ops-service-operation-boundary.md)。

`WebRealtimeGateway` 目前只在研究中作为 Web 侧低延迟媒体/XR adapters 的候选部署组合名，不是已冻结的公共类型，也不表示硬实时或 `RealtimeDomain`。逻辑上仍需区分 Media Gateway role 与 XR Input Gateway role。`ExternalSessionId` 不建立为跨领域泛化身份；browser auth session、WebRTC peer、XR input stream、AgentSession、FabricSession、DeviceSession 和未来 TeleoperationSession 不能复用一个 ID/epoch。

Zenoh callback 尚未完成 decode、Schema、principal 与 binding 准入时，手中的 bytes/SHM reference 只称 encoded ingress frame；它是 Fabric 私有传输事实，不建立新的 Kernel 公共消息类型。Fabric ingress buffer 只暂存这种 frame，不是 Mailbox，也不能产生应用层 accepted。它和 Zenoh 自身 channel 都必须有 items/bytes/age/retained-byte 预算与 Inspection，不能成为隐藏 backlog。

### 5.2 禁止的换皮映射

- ParaEGOX 不使用 `Module` 作为公共领域名，也不把 EAGOS Module 类改名迁移。能力合同属于 CardDefinition，领域计算与私有状态属于 CardInstance 托管的普通实现对象；运行身份、外部适配、通信/交付、生命周期/恢复和诊断分别属于 CardInstance、Driver/Gateway、Port/Link/Binding、RuntimeHost 及其 RecoveryEngine、Inspection。`Handler`、`Factory` 不建立为独立领域实体。
- `Deck` 是 ParaEGOX 新定义的声明式可执行工作负载/组合单元，不是 PhanthyMotus 既有类型。它从 Motus Card 与 `CanvasLayout + Project` 的交互模型演进而来；未来 Canvas 只编辑 DeckSpec，并只读展示 DeckCompiler 派生的 DeckTopology 验证投影，不能直接编辑或持久化 topology，也不能成为 Kernel、DeckRun 或生产运行真相。Deck 只承接 EAGOS Bundle 的用户侧组合切面，不承接 Product、安装、发布、部署、生命周期和运维全部职责。
- PhanthyMotus `PerceptionBundle` 是一个 MCP endpoint 内聚合多个 Plugin/Tool 的实现名；ParaEGOX 当前不把裸 `Bundle` 建成公共总概念，也不把它作为 Deck 别名。
- EAGOS `Runtime` 不改名为 `KernelContext` 后继续注入所有组件。
- EAGOS `Bus` 不改名为 `Fabric` 后继续承担传输、调度、RPC、权限、诊断和生命周期全部职责。
- PhanthyMotus `Canvas` 在 ParaEGOX 中演进为 DeckSpec 的 UI 编辑器和 DeckTopology 的只读验证投影，但不是 topology 持久化 owner，也不是 Kernel、DeckRun 或生产运行的真相来源。

## 6. 总体结构

```text
OS service manager / external control-plane process
                    │
                    ▼
DeckLock + ServiceSpec + Node facts + policy
                    │
                    ▼
            DeploymentPlanner
       DeploymentPlanCandidate
                    │
                    ▼
           DeploymentController
 atomic committed plan/revision + reconcile
                    │ RuntimePlanSlice + writer-context apply
        ┌───────────┴───────────┐
        ▼                       ▼
 RuntimeHost A             RuntimeHost B
        │                       │
 Core Services/Cards      Core Services/Cards
        └───────────┬───────────┘
                    ▼
                  Kernel
```

依赖方向只能向下：

```text
application / agent / workflow / operator clients
              ↓
           services
              ↓
           runtime
              ↓
            kernel
```

Kernel 不能反向导入上层。RuntimeHost 不持有领域服务字段，Core Service 不持有完整 RuntimeHost。

精确的包依赖还必须满足：`deployment/` 可以依赖 `runtime/contracts` 的 apply protocol，`runtime/` 不得 import `deployment/`、`decks/` 或具体 Service/Card 实现；`kernel/` 不定义 DeploymentPlan、DeploymentRevision、DeploymentController 或 RuntimePlanSlice。上层实现通过 Artifact entrypoint 与窄 SDK 被加载，而不是让 Runtime 建立反向源码依赖。

## 7. Kernel 边界

Kernel 提供机制，不提供平台产品能力。候选组成包括：

```text
kernel/
├── contracts/      # Message、ID、Deadline、Delivery、Receipt
├── time/           # monotonic/wall clock protocol、Deadline、Freshness
├── lifecycle/      # 纯状态机和转换规则
├── messaging/      # Port、Mailbox、有界队列、顺序和溢出语义
├── admission/      # 纯准入策略与决策模型
├── authority/      # Principal、CapabilityGrant、scope 与纯授权验证原语
└── resources/      # Resource、Lease 的契约和纯冲突判断
```

Kernel 中允许出现纯策略函数和状态机，但不直接启动进程、连接数据库、加载模型、打开设备或创建 Web 服务。需要操作系统副作用的实现属于 Runtime 或 Driver。
Receipt 是 `contracts/` 中的权威结果契约，不建立一个持有存储、线程或发布行为的 Kernel 子系统。

当前不创建 `kernel/graph`。若 DeckCompiler 与 DeploymentPlanner 等至少两个独立真实消费者在同一实施批次证明需要相同的 immutable directed-multigraph view、SCC、cycle witness 或 stable topological batches，才通过独立 ADR 抽取内部 leaf library；它不得拥有 serialization/digest、领域 metadata、状态、I/O 或执行器。

### 7.1 VFS、typed reference 与数据所有权

ParaEGOX 当前明确不建设 VFS。统一 URI resolver 会把 Artifact、Evidence、Secret、Workspace、Memory、State 和设备 I/O 再次聚合成 Service Locator。系统使用 owner-specific `ArtifactRef`、`EvidenceRef`、`SecretRef`、未来的 `WorkspaceRef` 以及带 lifetime/lease 的 `BlobRef`/`BufferRef`；每种引用只能由其 typed service/client 解释。ArtifactRef/SecretRef/EvidenceRef 的 Schema 位于各 owner contract 包，Kernel 只提供可组合的通用 ID/digest 基础值，不建立统一 Ref 基类或 Resolver。

InspectionService 可以联合查询这些 owner 形成资源浏览投影，OpsService/clients 只消费该投影；二者都不拥有数据权限、retention 或生命周期。只有至少三个真实 backend 确实共享 read/list/watch/write/atomic-commit 语义时，才研究 Workspace/Storage CoreService；即使出现也不进入 Kernel，不引入万能 `ObjectRef` 或 `open("anything://...")`。

### 7.2 Card、Deck 与 Kernel

Card/Deck 是一等公共产品模型，但全部位于 Kernel 外：CardDefinition/In/Out/ExecutionRequirements 属于 `cards/`，Card/CardProfile/Link/DeckSpec/DeckLock 属于 `decks/`，DeploymentPlan/DeploymentRevision/DeploymentPlanner/DeploymentController/placement/reconciliation 属于顶层 `deployment/`，CardInstance/DeckRun/RuntimeHost 属于 `runtime/`。Kernel 只提供它们可组合使用的通用 ID、Message/Port、时间、Receipt、Grant/Lease 与准入原语。文档把 Deployment 纳入系统基础架构规划，不等于把它放入 Kernel 包或 Kernel ABI。

### 7.3 Capability、Service 与 Feature

`Capability` 不再同时表示三种“能力”：

| 问题 | 正式类型 | owner |
| --- | --- | --- |
| 谁被允许做什么 | `PermissionRequirement`、`CapabilityScope`、`CapabilityGrant` | AuthorityService + local enforcement |
| 哪个服务提供什么接口 | `ServiceContractId`、`ProvidedService`、`ServiceRequirement` | ServiceSpec 声明；DeploymentPlanner 解析候选 provider，DeploymentController 提交结果 |
| 目标实际上支持什么 | `NodeFeatureReport`、`FabricFeatureReport`、`DeviceFeatureReport` | NodeDaemon、FabricService、Driver |

三者不共享基类。CardDefinition/Card/Deck 只声明 `requires.services`、`requires.permissions` 与 `requires.features`；DeckLock 固定 Requirement 与静态兼容约束，DeploymentPlanner 独占解析 live provider、target support 和 placement，Authority 只用通用 PrincipalRef、InstanceId/subject incarnation、可选 DeploymentRevision、audience 与期限约束签发 Grant，Kernel 不引用 Card/Deck/Service/Gateway/Driver 领域身份。CardProfile 可以把 selector 绑定到更窄的具体资源或删除 optional permission，但仍须满足 required operations，不能扩大权限。完整语义见 [Capability、Service Contract 与 Feature Support 边界](../concepts/capability-service-feature-boundaries.md)。

## 8. RuntimeHost

RuntimeHost 是 Node 内的执行宿主，不是 Node 本身，也不是领域服务容器。小型设备的 development/constrained profile 可以让 NodeDaemon 与 RuntimeHost 共进程部署，但不能据此宣称具备跨故障域恢复；较大主机可以运行多个具有不同权限和故障域的 RuntimeHost。具体 Placement 不改变两者的逻辑身份。RuntimeHost 负责：

- 建立一个受控的主事件循环和本地控制通道。
- 创建、管理和关闭 Execution Domain。
- 通过 runtime-owned apply protocol 验证并应用 DeploymentController 产生的 `RuntimePlanSlice` value；S7/PXAR v5先验证`RuntimeApplyEnvelopeV2.expected_runtime_store_instance_id`逐字等于local journal的`store_instance_id`，mismatch在任何tenure/request/revision/prepared mutation或副作用前以状态byte-identical的`RuntimeStoreMismatch`拒绝。随后才校验target、runtime-owned `SourceScopeRef`/`SourcePlanRevision`/`PlanWriterRef`/`PlanWriterEpoch`、exact expected-active target-slice digest、source_plan_digest、target_slice_digest、operation_id、temporal constraint、WriterTenureProof与请求认证，并在Ready前证明observed execution与该source revision一致。
- 通过内部 `RuntimeAssemblyEngine` 按 Slice 中显式选择的profile执行本地生命周期，不从Deck Link重算全局语义。S7只接受exact manifest-pinned `ReferenceAssemblyProfileV1::{OneSourceLoop, EmptyDeactivate}`、zero/one `ReferenceLoopDomainSpecV1`+`ReferenceLoopSubjectSpecV1`与zero-binding PXTA；两个v4 record不alias旧capacity-bearing public types，profile固定`lifecycle_concurrency=1`、mailbox/dispatch/background-task slots全为0。没有Mailbox/PortBinding、Ingress、Thread/Process或一般activation；zero Port/Grant也不构成对同进程Rust ambient OS authority的sandbox。未来successor若允许一般assignment/readiness/ingress/egress/dependency-loss，必须在独立contract中完整digest-cover。
- 维护全局 Executor/Thread/Process budget，包括 Zenoh、原生库和设备 SDK 的内部线程事实。
- 根据 Placement 启动本地 embedded/process 服务与 CardInstance。
- 执行 `stop accepting → drain → cancel → terminate` 关闭协议。
- 维护进程、线程、队列和资源的运行事实。
- 将生命周期和故障结果写为 Receipt 与 Inspection 数据。
- 以 active RuntimePlanSlice 中的 `LivenessSpec`、`FailureContainmentSpec` 与 `RecoveryPolicy` 为约束，通过内部 `RecoveryEngine` 求值 `RecoveryDecision`，并由 RuntimeHost 执行 `RecoveryAction`；RecoveryEngine 不持有 PID/Domain、不成为第二生命周期 owner，也不产生领域策略。

DeploymentController 是 Kernel 外按 DeploymentScope 单写的控制面 owner，拥有唯一 committed `DeploymentPlan`、DeploymentRevision、远端 desired placement、source plan digest 及其 canonical target projection，并为每个目标产生不可独立编辑的 `RuntimePlanSlice` value。它调用纯 DeploymentPlanner、DeploymentReconciler、DeploymentRolloutEngine、RuntimeSliceProjector 与 RuntimeApplyEnvelopeBuilder，但不解析 Deck、不启动进程、不持有 Zenoh Session，也不签发 Grant/Lease/SafetyDecision。DeploymentReconciler/DeploymentRolloutEngine 只求值 immutable snapshot 和 state transition，不获得身份、journal、tenure、I/O 或重试循环；持久 commit、rollout ledger、查询、重试与最终 reconcile decision 仍归 DeploymentController。Slice projector 只把 committed plan 的 scope/plan/revision/content 投影为 runtime-owned `PlanProvenance` 与 target assignment；apply builder 才把 deployment-owned `DeploymentWriterRef/DeploymentWriterEpoch` 与 authority proof 映射为 runtime-owned `PlanWriterContext`并构造additive `RuntimeApplyEnvelopeV2`。其exact 32-octet `expected_runtime_store_instance_id`来自Controller已durable pin的authenticated bootstrap response，只进入v2 canonical encoding、request-signing transcript与complete-request digest，不属于Plan/Slice desired truth，也不改变target-slice digest。`RuntimeApplyRequest` 至少携带 target RuntimeHost、Slice、writer context、exact expected-active target-slice digest、operation id、temporal constraint 和覆盖完整 canonical request 的认证/完整性证明，且认证 principal 必须与 `PlanWriterRef` 相符。RuntimeHost 可重算 target slice digest，但不需要也无法从局部 slice 重算完整 plan digest；DeploymentController restart 只更新 writer context，不改变 plan revision/source digest/slice digest。

P2e reference profile 中，DeploymentController、DeploymentTenureAuthority、RuntimeHost各自独占versioned/checksummed/bounded snapshot journal和one-shot initializer；keys/policy/service principals由operator预置并绑定sequence-1 fingerprint，missing/corrupt state不能reset。由OS service manager以独立service account/ACL托管的`DeploymentTenureAuthority`经crash-consistent transaction原子推进每个scope的DeploymentWriterEpoch，并签发Runtime可验证的`WriterTenureProof`；`acquire_tenure` local IPC必须versioned/bounded/authenticated+authorized，验证peer credential、request signature与scope/writer allowlist。该IPC的request/response/framing/auth transcript只由`paraegox-deployment`拥有，嵌入的`WriterTenureProof` canonical value继续由`paraegox-runtime-contracts`拥有；Runtime bootstrap/query的canonical request/response、channel-auth transcript、digest domain与bounds也只由`paraegox-runtime-contracts`拥有。same-uid/root compromise不在该POSIX reference保证内。tenure signing key不交给DeploymentController；Controller另以隔离的OS-protected request-auth key handle签署exact apply transcript，把signed request durable commit后才发送。

S7 production compatibility由`RuntimeBuildDescriptorV1`与singleton canonical `RuntimeArtifactCompatibilityManifestV1`共同冻结，它们连同`RuntimeBuildIdentityV1`和projection的canonical Schema、digest domains、strict bytes与bounds都只由`paraegox-runtime-contracts`拥有。release pipeline是descriptor唯一production producer。system installer/install operation严格消费descriptor+installed executable，并且是singleton manifest唯一production producer：它只从verified descriptor/executable、operator exact target/service identity与binary compiled fixture table一次生成同一canonical manifest artifact，再byte-identically交给Runtime initializer以及operator/Controller/Planner immutable ingress；拒绝任意prebuilt manifest或side file成为第二authority，Planner不得手写/重建第二manifest，bootstrap只校验。manifest在`manifest_version = 1`后恰有一个fixed target row，row只含exact target、`RuntimeBuildIdentityV1 {build_instance_id, build_descriptor_digest, runtime_artifact_sha256, compiled_reference_compatibility_digest}`、selected exact PXAR v5、profile v1与exact single fixture entry，projection携带独立domain的manifest digest和同一row；没有record count、集合、第二target或operator-selectable limit selector，所有bounds由PXTE v4/PXAR v5/profile v1 protocol constants拥有，变化必须使用successor。initializer再次验证final executable length/SHA-256/target和binary compiled id/table，并在Runtime sequence-1 snapshot直接持久化exact descriptor bytes+digest与installer-produced manifest bytes+digest。每次后续startup只验证snapshot里的pinned canonical bytes/digests，并从binary内不可由config/journal覆盖的compiled build id与exact compatibility table重算actual identity逐字段比较；不重新hash executable，也不读取side file/config作为第二权威。authenticated bootstrap分别报告compiled actual与store-pinned descriptor/manifest identity，不能把journal回显冒充actual。

RuntimeHost启动先原子、crash-consistently推进RuntimeHostEpoch/clock generation并invalidate旧live facts，完成前不发布bootstrap/query-ready或接收apply。tenure-only transaction只提交完整next AdmissionState tenure nonce+proof/principal与`writer_fence`，不消费request nonce、temporal lineage或revision；未跨`FirstActionIntent`的旧prepared才可`SupersededBeforeEffects`，post-intent旧operation必须`SupersededReconcileRequired`并由owner-wide action gate阻塞new effects直到exact-zero terminal或quarantine。独立full-admission transaction才原子提交request/temporal AdmissionState、per-source revision high-water、exact request/Slice和`PreparedNoEffects`；normal apply随后只以durable `FirstActionIntent`越过effect boundary，restart recovery另走`RecoveryPlannedNoEffects → StartCallIntent`，不得把两套intent混成一个phase。

Runtime journal分开保存host/clock generation、admission、sequence-1 strict-verified exact canonical descriptor/manifest bytes+digests、build-policy fingerprint、`writer_fence`、`prepared`、canonical `active` desired head、`live_materialization`、`recovery_action`、`owned_resources`、`RawActionOutcomeLatch`、`TerminalOutcomeSelection`和terminal-operation ledger。`OneSourceLoop`只在bounded readiness成功后一次提交desired head+`LiveReady`+terminal。`EmptyDeactivate`仅在current live/nonzero generation时以第一事务同时写`FirstActionIntent`、`NoNewAdmission`、canonical empty head、`HeadCommittedRetiringOld`、exact old Slice/budgets/generation与`Draining`，第二事务在old ownership exact-zero后才terminal；canonical empty或`RecoveryFailedNotReady`且ledger exact-zero、无action/resource时走deadline-prechecked、无intent/callback的单事务fast path。两条路径都持续保留empty revision/CAS head。

normal/recovery/empty intent或head transaction在构造前检查owner clock，durable publish后、首个resource/callback effect前再次检查；publish前`now >= deadline`不写intent/head，publish后到期不启动effect，已经committed的empty head不回滚。pre-intent crash证明no-effects；normal apply保留old head，recovery pre-intent crash可在new host/clock epoch用fresh action/generations重建且不置failure latch，recovery pre-intent timeout则消费唯一attempt并置permanent failure latch。需要cleanup的callback/deadline/cancel产生raw结果后、进入cleanup前先atomic durable写bounded monotonic `RawActionOutcomeLatch`；后续host interruption、higher-tenure takeover与cleanup/census evidence作为独立维度单调补入，durable known fact不因crash降为Unknown。invariant/panic/cleanup/ownership uncertainty→quarantine、post-intent supersede、host crash的优先级都高于普通terminal选择；没有这些高优先级结果且cleanup+exact-zero完成后，owner才在构造terminal前以一次`terminal_selection_observed_at`采样持久形成`TerminalOutcomeSelection`，`now >= deadline`含相等选timeout，否则按raw error/success。selection后fsync/回复跨deadline不重分类，raw fact在timeout/interrupted/superseded terminal中也保留。`OneSourceLoop` start success可与active+terminal同一atomic commit而不写中间latch；该commit未durable即crash时outcome仍为Unknown。

active singleton manifest projection中的`RuntimeBuildIdentityV1`绑定initializer验证的Runtime executable identity，fixture entry另绑Card fixture artifact digest；两者不同域且不得相等比较或互相替代。任一build/store变化要求empty terminal→decommission old RuntimeHostId/store→new identity/store，不能原地upgrade/downgrade。missing/corrupt/undecodable/unknown-version snapshot不能建立可信host/store/epoch/sequence，不绑定bootstrap/query-ready，也不返回authenticated `Indeterminate`；只有validated snapshot且startup generation transaction已durable后出现的compatibility/recovery/ownership quarantine，才可携带exact identity返回authenticated `Indeterminate { stable reason }`。ADR-0007/0008 已 Accepted，S7 处于 `in_progress（S7-F）`；S7-B/C/D 与 S7-E executable vertical 已依次形成提交和 CI 证据，S7-F Runtime query/payload-v5/fixed-reassembly tranche 又由 `7d7db38` 至 `f32700f` 和 Ubuntu CI `30782979187` 验证。当前未提交工作树虽已有 Controller reconcile command/implementation 与 SF6 scenarios，但尚无 fresh Ubuntu CI，因此完整 S7 闭环仍未完成。参考 profile 中一个 RuntimeHost 同时只接受一个 active source scope。目标 Node 的 RuntimeHost 拥有目标实例生命周期；external workload manager 托管的工作负载只能经明确 `ExternalWorkloadAdapter` 请求或观察。

RuntimeHost 不允许提供以下访问方式：

```python
host.memory
host.model_manager
host.agent
host.world
host.task_service
host.ops
```

Runtime 可以继续作为状态类名的一部分，例如 `RuntimeSnapshot`、`RuntimeState` 和 `RuntimeInspection`，但不创建表示全系统依赖集合的 `Runtime` 对象。

## 9. ServiceContext

Core Service 不获得 RuntimeHost 本身，只获得最小、能力受限的 `ServiceContext`：

```text
ServiceContext
├── service_identity
├── clock
├── cancellation
├── service_clients       # 已解析的 ServiceRequirement
├── port_bindings         # 显式声明的数据 Port
├── access_handles        # 受 CapabilityGrant 约束的窄句柄
├── receipt_sink
├── inspection_sink
└── validated_config
```

三类入口不能合并成一个 `declared_bindings` 或动态 registry。服务不能通过字符串、URI 或动态属性查找未声明依赖，也不直接取得 token/Secret；这样可以防止 ServiceContext 再次成长为 Service Locator。

## 10. Core Service

Core Service 是长期运行的**平台所有**能力，采用两段式准入。首先必须至少满足一个平台作用域条件：

- 持有 Authority、Resource、Fabric、Inspection 等平台权威或基础设施真相。
- 被两个以上相互独立的 Product/Installation 共享，而不是仅在同一产品的多个 Deck 间共享。
- 是普通 Deck workload 启动前必须存在、且其生命周期不能由任何一个 Deck/Application 支配的平台 substrate。

通过平台作用域门后，下列特征才是把它实现为独立 CoreService 的辅助信号，任何一项都不能单独把 application/run-bound service 提升为 CoreService：

- 提供稳定、版本化的 ServiceContract。
- 需要跨 CardInstance 生命周期保存平台状态。
- 需要独立权限边界、资源预算、故障域、部署或升级。
- 被多个 CardInstance 或其他平台 Service 使用。
- 普通 Agent、Graph 或 Deck 不应有权停止它。

候选 Core Service 包括 Identity/Enrollment、Fabric、Authority、Resource、Evidence、Artifact/Release、Secret、Device、SafetyIslandAdapter、Continuity、Calibration/FrameGraph、Model、Memory、World、Agent、InspectionService 和 OpsService，但首版只实现有真实消费者和验证入口的最小集合。SafetyIslandAdapter 不是 safety function owner，Continuity 也不接管 Authority/Resource/Device/Evidence 事实；InspectionService 与 OpsService 必须保持 projection owner 和 operation owner 分离。候选名不代表立即建包：P0–P3 只落地 Contract spine 与首个本地仿真物理切片需要的 owner。当前不建立通用 Topology、VFS、StateStore、Graph 或 Workflow CoreService；一个算法步骤即使长期运行，也不自动成为 Core Service。

CoreService 的“平台”作用域由独立产品间共享、平台权威与生命周期决定，不由实现是否像 daemon 决定。只属于一个产品安装、需要跨 DeckRun 保留的领域服务不应自动成为 CoreService；当前对此显式后置，待稳定 Application/Installation owner、数据迁移与 retain/delete 语义成立后，再研究 application-owned ServiceSpec/ServiceInstance，而不是把状态藏入 Card 全局变量。只随 DeckRun 存活的共享能力优先建模为 Card；若多个消费者必须使用 ServiceContract，另行研究 DeckRun-scoped ServiceSpec，不要求 Application identity。

Core Service 使用小型 Protocol，而不是功能丰富的基类：

```python
class CoreService(Protocol):
    async def prepare(self, context: ServiceContext) -> None: ...
    async def start(self) -> None: ...
    async def readiness(self) -> Readiness: ...
    async def drain(self, deadline: Deadline) -> None: ...
    async def stop(self) -> None: ...
```

超时、取消、进程终止、重启预算、健康发布和证据记录由 RuntimeHost 负责。Service 自己负责能力实现与服务运行状态；持久领域数据还必须单独声明 durable data owner 和 storage custodian，不能从 ServiceInstance 的进程 owner 自动推导。

### 10.1 Model CoreService、ModelAdapter、plugin profile 与 composition

Model 采用四层责任，而不是把每个 provider crate、adapter、plugin profile 都提升成 CoreService：

| 层 | 责任 | 当前实现边界 |
| --- | --- | --- |
| Model CoreService / `ModelService` mechanism | provider-neutral 的 bounded admission、deadline/cancellation、容量与 outcome | 当前只有 embedded、in-process mechanism；managed/shared service 尚未实现 |
| `ModelAdapter` | 一个精确 provider/protocol 的编码、transport、响应校验、Secret 使用与 provider-specific failure 映射；当前由 `ModelBackendV1` 作为 Rust seam | fixture 与 OpenAI 是静态链接实现；adapter crate 本身不是 Service/process/plugin activation |
| Model plugin profile | 声明 adapter identity/version/capability、配置约束/摘要、SecretRef、Artifact/provenance、网络/沙箱和兼容要求 | planned，尚无 manifest、installer、admission 或 activation 实现 |
| composition | 校验外部 selection 的 exact profile/provider/config/SecretRef binding，以 owner-private 编译内 mapping 选择 adapter ID，并组装 registry、adapter 与 Model mechanism | 当前仅有 DeveloperLocal 静态组合；不拥有 committed desired state 或 Runtime lifecycle，外部 selection 也尚未绑定 adapter ID/version/capability |

第一批只在 `paraegox-model` 既有 owner 内建立 exact static registration/selection。当前工作树已经
实现 `ModelAdapterIdV1`、`ModelAdapterMetadataV1`、`ModelAdapterSelectionV1`、
`ModelAdapterFactoryV1` 与 `ModelAdapterRegistryV1` 的 registry core；fixture/OpenAI 同路径接线和
production fixture bypass 移除也已通过本地 focused validation。registry 是显式、composition-owned 的进程内装配机制，
不是全局 Service Locator、Artifact registry 或动态 loader；它只接受编译期链接且由
composition 显式提供的 factory。重复 identity、未知 selection、metadata/config 不一致必须
fail-closed，不提供 default adapter、自动发现、基于请求的路由、fallback/canary 或透明 retry。
deterministic fixture 与 OpenAI 必须统一走 provider selection → `RuntimeAgentProviderResolverV1` →
composition-owned compiled profile mapping → `ModelAdapterRegistryV1` → `ModelServiceV1` → AgentService
adapter；production fixture 不能保留 resolver/registry direct bypass，AgentService 也不感知具体
provider。`paraegox-model-openai` 可以因 HTTP/TLS 与 Secret 的真实依赖/安全隔离继续独立成 crate，
但不能因此取得独立生命周期或 plugin 身份。

`ModelAdapterSelectionV1` 只是 registry-local exact request。当前 signed
`ManagedAgentProviderSelectionV1` 只绑定 profile、provider ref、config digest 与 Secret ref，不含
adapter ID/version/capability；composition 的编译内 mapping 不能冒充 authenticated adapter Artifact
identity。后继 additive contract successor/plugin admission 必须补齐这个端到端 binding。

第二批才允许由 committed Deployment plan 选择 Model provider profile/config/SecretRef，RuntimeHost 管理
lifecycle、readiness、recovery 和 generation fencing，AgentService 通过 ServiceDependency 消费真正的
managed Model CoreService；在声称 production exact adapter selection 前，additive contract successor
必须绑定 adapter ID/version/capability。第三批才允许实现 plugin profile admission/activation；它必须
绑定 Artifact/provenance、
capability/contract compatibility、SecretRef、网络/沙箱与 activation evidence。两批都尚未实现。

当前不创建 `plugins/`、PluginManager、插件市场或新的 Model 服务进程，不加载 Rust `.so`/`.dylib`、
WASM 或远端代码，也不声称与 EAGOS 插件体系兼容或等价。未来只有真实第二个 production adapter、
外部 Artifact 或隔离需求满足准入门时，才扩张这些边界；目录对称或 provider 数量本身不是理由。

## 11. Core Service、CardDefinition、CardInstance 与 Driver

| 属性 | Core Service | CardDefinition | Card / CardInstance | Driver |
| --- | --- | --- | --- | --- |
| 期望状态 owner | ServiceSpec 声明；DeploymentController 提交实例计划 | CardDefinition 作者；解析由 Resolver、安装由 Artifact owner | Deck Compiler 声明工作负载意图；DeploymentController 提交实例计划 | Service 或 Card 的声明者 |
| 本地生命周期 owner | 目标 RuntimeHost | 不适用；定义没有运行身份 | 目标 RuntimeHost | RuntimeHost 或 external workload manager |
| 领域状态 owner | 平台权威状态可由 CoreService 拥有；托管数据的 owner 由独立 namespace/数据合同决定 | 无；定义是不可变数据 | CardInstance 托管的私有实现对象可持有 run-bound 状态 | 外部系统；Driver 只投影或适配 |
| 典型寿命 | 节点或平台长期寿命 | 跨多个 Deck 版本存在 | DeckRun 寿命 | 外部连接或设备会话寿命 |
| 状态 | 可持久、可共享 | 无运行状态 | CardInstance 持有系统状态；私有实现对象默认可重建 | 以外部系统状态为主 |
| 接口 | 稳定 ServiceContract API | CardDefinition、In/Out、配置、requirements 与 entrypoint ref | 已编译 PortBinding、窄实现上下文与 RuntimeHost-owned 调用 | 外部协议与内部契约转换 |
| 权限 | 可能拥有平台权威 | 只声明所需权限 | 获得经验证的最小应用权限 | 仅拥有声明过的设备或协议权限 |
| 可否被 Agent 停止 | 默认不可以 | 不适用 | 在授权 Deck 操作中可以 | 取决于是否承载安全关键设备 |

一个共享的模型服务器适合成为 Model Core Service；目标检测算法适合成为 CardDefinition，在 Deck 中形成 Processor Card，运行后成为 CardInstance。一个底盘硬件守护进程可能是 Driver Core Service；一个只在某个 Deck 中使用的模拟相机可以由 Driver 支撑一张 Sensor Card。

CardInstance 私有实现对象可以提供 `on_start`、输入处理和 `on_stop` 等回调，但不能拥有或自行推进系统生命周期，也不能通过继承自动获得 Bus、线程、进程、TF、Tool、配置和 Probe。DeckCompiler 产生内嵌 canonical DeckTopology 的 DeckLock，DeploymentPlanner 结合 Node facts 和 policy 生成 DeploymentPlanCandidate，DeploymentController 原子提交 allocation delta/revision/committed DeploymentPlan 并投影 RuntimePlanSlice；RuntimeHost 只按该 slice 创建 CardInstance、按 locked CardDefinition 的 entrypoint 构造私有实现对象，并提供已解析的 Port、Clock、Cancellation、typed service client、permission-bound handle、Receipt/Inspection sink 和最小配置。详细约束见 [CardDefinition、Card 与 Deck](../concepts/card-definition-card-deck.md)。

## 12. 领域图、结构投影与执行关系

ParaEGOX 不只有“两棵图”，而是多个由不同 owner 解释的图或图状关系。DeckTopology 是 dataflow declaration，ServiceDependencyGraph 是 readiness DAG，Deployment rollout 是 revision control loop，Runtime assembly 是从 target Slice 派生的本地执行约束，Agent workflow、Evidence causal projection 与 World/Spatial graph 又拥有各自的 durable/effect/time/uncertainty 语义。相同的 node/edge 形状只允许复用纯算法，不允许共享权威 Schema 或执行状态机。

### 12.1 ServiceDependencyGraph

ServiceDependencyGraph 描述平台服务的 `provides`、`requires`、Placement、重启策略和 readiness。ServiceSpec 是声明 owner；DeploymentPlanner 把已解析依赖编译进 DeploymentPlanCandidate，DeploymentController 原子提交 committed plan/revision 并协调 rollout，不能成为服务 registry 或运行时依赖容器。

若未来准入 DeckRun-scoped 或 installation-scoped ServiceSpec，应扩展同一 scoped ServiceDependencyGraph 的 vertex 与 owner/lifetime 约束，而不是创建 `ApplicationServiceGraph` 或把它们重新归类为 CoreService。Service workload owner、durable data owner 和 storage custodian 必须分别可追踪。

```text
Authority ──requires──> Identity
Resource  ──requires──> Identity
Agent     ──requires──> Model / Memory / World
OpsService ─requires─> InspectionService / Authority / Evidence contracts
cross-Node service client ─requires─> Fabric
```

具体依赖必须由实现阶段验证，上图只表示结构示例。

### 12.2 Deck 与 DeckTopology

DeckSpec 以 Cards 和 Links 描述具身工作负载；Card 通过已解析 CardDefinition 引用 Port，Deck 不复制或改写 CardDefinition 的 Port 定义。`DeckTopology` 是 DeckCompiler 解析后内嵌于 DeckLock、受其 digest 覆盖的 canonical directed-multigraph 结构核心；同一 Card pair 的不同 Port Link 必须作为 parallel edge 保留：

```text
Sensor Driver → Processor → Controller-role Card → OperationClient/CommandEndpoint
                                              ↓
                       Authority → Lease/Fence → Safety → Enforcement → Actuator
```

DeckSpec/DeckTopology 不能包含或停止 Authority、Fabric、Ops 等平台关键服务，只能声明 `ServiceRequirement`、`PermissionRequirement` 和 `FeatureRequirement`。Canvas 或其他 UI 只能编辑 DeckSpec，并只读展示 DeckCompiler 派生的 DeckTopology 验证投影；它不能直接编辑或持久化 topology，也不能成为 DeckLock、DeploymentPlan 或 DeckRun 的运行时真相来源。DeploymentPlanner 只消费自包含的 DeckLock，不接收可独立漂移的 DeckTopology。

`CardA.Out → CardB.In` 是 DataLink，不表示“先启动 A 再启动 B”。S7/P2e production candidate不含Port、Link、Mailbox、Ingress或producer egress，任何此类Deck在candidate/commit前拒绝。未来一般assembly successor若开放DataLink，Runtime必须先准备B的ingress/Mailbox再开放A的producer egress，Service readiness、activation constraint和drain order由DeploymentPlanner完整编译并digest-cover。基础结构能够检测SCC，但在显式feedback/delay/seed/backpressure contract通过ADR前拒绝cyclic Deck，不回退声明顺序或自动插buffer。

### 12.3 Graph Foundation 与领域执行器

当前不建设通用 Graph Engine。DeckCompiler、DeploymentPlanner、DeploymentController、RuntimeHost、Agent、Evidence 与 World 各自拥有 typed model、失败策略和状态机。只有两个独立生产消费者证明相同的纯结构需求后，才条件抽取 `Graph Foundation`，其上限是无状态的 multigraph view、SCC/cycle witness、reachability 和对已验证 DAG 的 stable topological/reverse batches；不包含 loader、digest、store、query、retry、checkpoint、approval、Receipt 或 `execute()`。

RuntimeHost 内部的 `RuntimeAssemblyEngine` 也不是 Graph Foundation 的“执行模式”。它只消费authenticated target RuntimePlanSlice与Runtime journal并执行profile-specific lifecycle。S7只做fixed idle Loop的bounded start/restart reassembly与empty-head-first drain/retire；没有一般replacement/rollback、PortBinding、Mailbox或稳定消息。未来profile的稳定消息仍必须直接经过PortBinding、Mailbox和ExecutionDomain，不能进入assembly loop。完整裁决与准入门见 [ADR-0005](../adr/ADR-0005-typed-domain-graphs-and-runtime-assembly-boundary.md)。

### 12.4 Deck 编译与运行

Deck 作为用户侧概念分为三种不同权威形态：

```text
Canvas ──save──> DeckSpec
                   │ validate + resolve
                   ▼
                DeckLock
                   │ + ServiceSpec / target facts / policy
                   ▼
          DeploymentPlanner (pure)
                   │ DeploymentPlanCandidate
                   ▼
          DeploymentController atomic commit
                   │ committed DeploymentPlan
                   ▼
          RuntimeSliceProjector + ApplyEnvelopeBuilder
                   │ RuntimeApplyRequest {RuntimePlanSlice + writer context + CAS controls}
                   ▼
                RuntimeHost
                   │ RuntimeAssemblyEngine
                   │ profile-specific apply / live query / reconcile
                   │ observed facts / Receipt
                   ▼
          DeploymentController reconcile
                   │
                   ▼
          DeckRun / Inspection projection
```

- `DeckSpec` 的长期模型保存Cards、Links、CardDefinition版本约束、CardProfile引用、`ServiceRequirement`和Placement提示；S7 executable subset只允许一个Deck-scoped instance引用manifest single fixture、canonical-empty config、零Port/Link/Requirement，其他输入只能被typed validator稳定拒绝，不能进入candidate/commit。
- `DeckLock` 的 canonical DeckTopology 只保存稳定 Card closure key、以 closure key 限定的 Port endpoint key、Link、DeliveryProfile key/ref、RequirementRef key 和结构关系；resolved closure 才唯一保存精确 CardDefinition/version、Port/Schema、Artifact/Adapter、DeliveryProfile/Requirement payload、依赖闭包、协议版本、CardProfile/SecretRef 解析结果和声明平台兼容约束。两部分都受 digest 覆盖，validator 拒绝悬空 key、重复语义 entry、冲突版本和 key/payload mismatch。DeckLock 不保存 Secret 内容、live provider、Node Feature 匹配或 placement；目标 artifact variant、provider 与 target-specific 结果由 DeploymentPlanner 写入 DeploymentPlanCandidate。
- DeploymentPlanner 是纯、确定性计算：相同 canonical DeckLock/ServiceSpec/Node facts/policy/allocation-snapshot 输入必须生成相同 `DeploymentPlanCandidate` 与 PlanContentDigest；它不监听 Node、不分发、不重试、不管理 Runtime。
- DeploymentController 在一个crash-consistent transaction中原子提交candidate的stable-ID allocation delta、下一DeploymentRevision与committed `DeploymentPlan`。S7 committed PlanContent只保存exact target singleton `RuntimeArtifactCompatibilityManifestV1` projection、`ReferenceAssemblyProfileV1`、一个fixed idle subject/domain或显式plan-side `EmptyTargetDesiredEntry`；manifest在`manifest_version = 1`后恰有一个row，只含exact target、`RuntimeBuildIdentityV1`、selected exact PXAR v5、profile v1与exact single fixture entry，projection携带独立manifest digest与同一row。没有record count、第二target、version/mode/fixture set、mode mask或operator-selectable limit selector；所有bounds由v4/v5/profile protocol constants拥有，变化必须使用successor。`RuntimeBuildIdentityV1.runtime_artifact_sha256`与fixture artifact digest分域，不得混用。S7没有`.bindings`、general capacity、Ingress、Thread/Process或一般activation fields。未来successor若增加ConfigSnapshot、Binding和一般execution语义，endpoint/route/Schema/Fabric ingress limits与activation/readiness/egress/drain必须在同一revision和target Slice中完整digest-cover，不能独立漂移。committed plan不保存writer tenure、rollout、PID、queue depth或BindingEpoch。
- `RuntimeSliceProjector(committed_plan, target)` 生成 tenure-neutral RuntimePlanSlice；`RuntimeApplyEnvelopeBuilder` 再把 DeploymentWriterRef/DeploymentWriterEpoch 映射为 runtime-owned PlanWriterContext，并构造含exact `expected_runtime_store_instance_id`的`RuntimeApplyEnvelopeV2`，绑定expected-active、operation id、deadline、tenure proof与request auth。expected store来自Controller durable-pinned authenticated bootstrap，只属于request/auth transcript，不改变DeploymentRevision、source plan digest或target slice digest。DeploymentController restart只改变request writer context；S7 projector只能产生`OneSourceLoop`或从`EmptyTargetDesiredEntry`产生`EmptyDeactivate`，omitted target绝不推导为empty。RuntimeAssemblyEngine只执行Slice显式profile，不import、查询或保存可编辑的Deck/Deployment上层模型。
- `DeckRun` 关联实际 Deployment、CardInstance、状态和 Receipt，只通过 Inspection Protocol 暴露。

DeckSpec 不执行安装或启动，DeckLock 不产生副作用，DeckRun 不持有全局 Runtime 对象。Artifact 安装与可信校验、Deployment 应用和 Runtime 执行分别由独立所有者完成。

### 12.5 Product Application 与安装边界

当前架构不新增正式 `Application` 领域对象。一个产品应用可以在首个 reference profile 中由一个 Deck 完整表达，但 Deck 的规范含义保持“可执行工作负载单元”，不与 Product、Release、Installation、DeploymentScope 或源码仓库等价。

```text
当前产品语言“应用”（无公共 identity）
       ├── DeckSpec → DeckLock {DeckTopology}
       ├── client/Gateway Artifact（由各自 owner 管理）
       └── ServiceRequirement → 平台 CoreService
                              │
                              ▼
                    DeploymentController
                              │
                              ▼
           DeckRun / CardInstance / ServiceInstance / Gateway facts
```

当前不建立 ApplicationSpec/Lock/Instance/Controller、application store 或 `applications/` 包，也不允许 UI 分组字段产生权限、级联回收或数据所有权。运行聚合健康由 Inspection 根据真实 DeploymentRevision、DeckRun、instance identity、epoch 和 freshness 投影。

一个产品聚合多个独立 DeckLock、需要跨 DeckRun/升级但不跨产品的持久状态、同一 release 多次隔离安装，或 Deck/Gateway/client/private service 必须形成统一签名/升级/卸载闭包时，触发 Proposed [ADR-0004](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md) 的 A0 gate。只有定义实际最小 ProductRelease/Installation/Application 或其他窄 owner 的后继 ADR 被接受后，才能新增相应公共类型。Application 即使成立也只能位于 Planner 上游作为控制/交付/所有权输入；不能创建第二个 reconcile loop，不能拥有 Runtime/placement/live binding，也不能停止平台 CoreService。

## 13. 声明式服务启动

服务通过 ServiceSpec 声明，而不是在 Rust/Python module import、constructor 或 Web lifespan 中自行创建后台任务：

```yaml
services:
  authority:
    entrypoint: paraegox.services.authority:create
    provides:
      - command.authorization.v1
    requires:
      - identity.v1
    placement: process
    restart: bounded
```

`resource.lease.v1` 由独立的 ResourceCoordinator ServiceSpec 提供；它可以与 Authority 使用同一 process placement，但不能借同进程合并 API、状态和 Receipt owner。本地 Authority 示例不要求 Fabric ServiceRequirement；typed service client 跨 Node 访问时才显式声明 Fabric 依赖，受保护的 raw Fabric 访问另需 scope 指向 Fabric resource 的 `CapabilityGrant`。这是服务依赖调用，不是第二种 PortBinding。

DeploymentPlanner 根据 `provides/requires` 纯编译依赖 DAG，在 DeploymentController 提交计划前拒绝缺失依赖和循环。DeploymentController 只拥有 committed revision 和 rollout，不重复实现图求解。不要使用脆弱的全局 `startup_order` 数字。

依赖 DAG 也必须覆盖运行期：每个 `ServiceRequirement` 声明 provider 在 Ready 后消失时允许的 `degrade / stop / rebind / restart / fail-closed` 范围，Deployment policy 选择具体动作并负责防抖与传播。旧 provider client/epoch 在恢复后不能复活；Liveness、Health、Readiness 与 Feature support 分别报告，heartbeat 不等于当前 revision Ready。

逻辑 Service 不等于独立进程。Placement 可以是：

| Placement | 适用范围 |
| --- | --- |
| `embedded` | 轻量、可信、无阻塞的平台能力 |
| `process` | Model、Memory、Agent、设备 SDK 和可能卡死的工作 |
| `remote` | 期望状态指向另一个 Node；调用侧只创建 typed service client/permission-bound handle，目标 RuntimeHost 拥有实例 |
| `external` | 已由 systemd、容器或其他管理器托管的服务 |

服务契约不依赖 Placement，因此可以先共用 RuntimeHost，再在不改变消费者的情况下拆分进程。Placement 也不等于 ExecutionDomain：`embedded` 仍需根据 Service/CardDefinition 的 ExecutionRequirements、workload SLO 和目标 Node facts 编译为 LoopDomain 或 ThreadDomain；`process` 才要求本地 ProcessDomain。`remote` 不是本地 ExecutionDomain；`external` 的 readiness 和 shutdown 必须来自实际 `ExternalWorkloadAdapter`，不能由 RuntimeHost 猜测。

## 14. Execution Domain

RuntimeHost 通过明确的 Execution Domain 约束并发，但 Domain 不是 CardDefinition/Card 手工选择的运行标签。执行计划来自唯一编译链：

```text
CardDefinition/Service ExecutionRequirements
              +
Deck Links / DeliveryProfile
              +
DeploymentProfile + NodeFacts
              │
              ▼
DeploymentPlanner → DeploymentPlanCandidate.plan_content.execution
├── DomainAssignment
├── MailboxSpec
├── DispatchPolicy
├── AdmissionBudget / OutstandingBudget
├── ExecutorBudget / IPC credits / retained-byte budget
├── LivenessSpec / FailureContainmentSpec / RecoveryPolicy
└── RevisionTransition
              │
              ▼
RuntimeHost → observed DomainInstance / PID / TID / loop / epoch
```

| Domain | 用途 | 关键约束 |
| --- | --- | --- |
| `LoopDomain` | 短小、可信、主动让出的 Rust async 状态机和轻量路由 | P2 reference profile 每 RuntimeHost process 一个 runtime-owned async reactor；额外 reactor 由 RuntimeHost 计划/预算/观测且不冒充故障隔离；禁止阻塞 syscall、长 CPU、未知 native call、未证明可让出的 future 和无 owner Task；Tokio 类型不进入公共合同 |
| `ThreadDomain` | 有来源级 timeout 的有界同步 I/O，以及已知安全的不可异步化库 | 受全局 ExecutorBudget 约束；提交前取得显式 permit，不使用无界/default blocking backlog；worker 不创建私有 async runtime；不能硬杀，不承载可能永久卡死的关键工作 |
| `ProcessDomain` | Python/C++ Card、Agent、模型、GPU/native、设备 SDK、第三方 Artifact 和未知稳定性代码 | 版本化 child protocol、最小 child、readiness/持续 heartbeat、credit/bytes 有界 IPC、cooperative stop→TERM→KILL、进程树清理、资源限制、restart quarantine 和 generation fencing；语言本身不直接决定 Domain |

Domain 之间只通过有界 Mailbox 或稳定协议交互，不直接跨 async runtime/process 共享 Future、Task、Queue、Lock、`Arc<Mutex<_>>` 领域别名、Python object 或可变领域对象。一个 Domain 只有一个生命周期 Owner。ThreadDomain timeout 只能停止等待或请求 cooperative cancellation；已经运行的线程只能报告 `cancellation_requested`、`uncertain` 或 `wedged`，不能报告已经被杀死。ProcessDomain 重启后生成新的 DomainEpoch，旧 InvocationId 的迟到结果不得落地。

Mailbox 只限制 queued 不足以证明系统有界。Dispatcher 只在获得 OutstandingBudget permit 后才原子地将消息从 queued 转为 inflight；executor submission、IPC credit、child work 和 payload/SHM retained bytes 一并记账。无 permit 时不 dequeue、不创建等待 Task/Future，IPC 不成为第二个隐藏 backlog。

`RuntimeOwnershipTree` 是 `RuntimeHost → DomainInstance → CardInstance/ServiceInstance → InvocationScope`。它是从 assignment 与 observed ownership 构建的结构，不是第三棵 desired graph。DomainInstance 是 kill/restart/resource-accounting 单元；普通 Invocation 失败不取消无关实例，child work 不得越过 scope/epoch/revision 存活。共置实例的 collateral restart 集合必须进入 `FailureContainmentSpec` 和 Inspection。

当前 S6 实现注记：已落地的是 private local POSIX ProcessDomain baseline，不是上表完整 production profile。它有 Rust/Python worker、PXWP v1、same-process-group cleanup、workspace identity cleanup、exact-zero proof、restart/quarantine 和 Linux `/proc` census；没有 C++ worker、production Artifact/profile resolver、cgroup/job-object/pidfd/full sandbox、跨平台资源 enforcement、host SIGKILL 后独立 process-group orphan ownership 或 durable journal。

ThreadDomain worker wedged 后容量继续扣减，不超预算补线程；Domain 进入 degraded/poisoned，需硬恢复的工作从一开始进 ProcessDomain。Process crash 后 RuntimeHost 只记录 `RuntimeFailureFact`，RecoveryEngine 只消费事实并求值；在途 effect 无终态证明时是 `Uncertain`，默认不 replay。GPU/device 状态由真实 resource owner 的 completion/reset/fencing 证明。

`Lane` 不建立为公共对象。多个具有不同 deadline/freshness 的 Mailbox 需要共置时，Domain dispatcher 可以内部维护多级 ready queue；它只仲裁 ready Mailbox，不拥有第二份 payload queue、线程、event loop、进程或生命周期。CardDefinition、Card、Deck 和 Kernel Schema 禁止声明 `lane_id`、`thread_lane` 或 `process_lane`。

硬件 E-Stop、本地 Safety inhibition 和需要证明 worst-case deadline 的路径不进入普通 Rust async dispatcher 或 Python worker。后续若有真实需求，建立独立 RealtimeDomain 或安全执行器；Rust、线程优先级或 memory safety 都不等于硬实时与功能安全。

完整证据、替代方案、线程/进程规则和验证矩阵见 [Runtime 执行模型、调度与恢复研究](../research/execution-model-scheduling-and-recovery.md)。

## 15. Fabric 与消息语义

ParaEGOX 不建设万能 Bus，也不把 Zenoh、DDS 和 ROS2 设计成三个等价 Backend。生产消息面分为三层：

| 层 | 所有者 | 职责 |
| --- | --- | --- |
| Kernel messaging | Kernel | Message、Port、Delivery、Mailbox 和失败语义 |
| Zenoh-native Fabric | Fabric CoreService | `session-local`、`host-local`、`remote` production route 的 bind、pub/sub、query/queryable、liveliness/matching、session/reconnect 与自身连接观测 |
| Ecology Gateway | Gateway | ROS2、浏览器、WebRTC/XR 等外部协议生态的类型、身份、权限和故障语义转换 |
| Device boundary | Driver | 设备、仿真器或具体硬件/SDK 的身份、数据、命令和故障适配 |

Zenoh 是 ParaEGOX 唯一生产 Fabric。FabricService 可以直接利用 Zenoh 的 query/queryable、liveliness、SHM、Regions、优先级多流和 mixed reliability，不为假想的 DDS/MQTT Backend 收缩成最小公分母接口。Zenoh storage 只能通过显式 ServiceContract/adapter 使用，调用者另需相应 `CapabilityGrant`；它不能接管 Evidence、World 或 Memory 的数据所有权。Fabric 自检只描述自身 session/binding，不替代全局 Inspection。Kernel 仍不依赖 Zenoh，这一隔离用于保持契约纯度、确定性测试和生命周期所有权，不代表生产路径可以任意替换主数据面。P2 可用不依赖 Zenoh 的 `PortBinding test fixture` 把已验证 Message 直接 offer 到同一 target Mailbox；它不是 LocalBus、不是生产 transport，也不进入 DeploymentPlan 的 route 枚举。Fixture Slice 必须由生产 `RuntimeSliceProjector` 的 conformance 路径生成，不能长期形成绕过 DeploymentController 契约的第二种手写配置。

```text
Card.Out ── Deck Link ──> Card.In
     │              compile
     └──────────> DeploymentPlan.bindings
                            │ install
                            ▼
                      PortBinding
                            │ exactly one active route
                            ▼
       Zenoh {session-local | host-local | remote}
                            │ callback: fixed-cost checks
                            ▼
       bounded Fabric ingress buffer (encoded frame)
                            │ decode / schema / principal / binding admission
                            ▼
                         Message
                            │ semantic admission
                            ▼
                  bounded target Mailbox
                            │
                            ▼
                    Execution Domain
```

Zenoh callback 只做固定成本的 key/header/size/version、缓存命中的 transport principal、BindingEpoch 检查，并把不可变 encoded frame/reference 非阻塞 `try_offer` 到有界 Fabric ingress buffer；不做完整 payload decode、解压、复杂 schema validation、耗时认证、Card 领域调用或物理业务 Authority。Ingress worker 完成 decode、Schema/principal/binding 准入，成功后才构造 `Message` 并 offer 到 target Mailbox；失败产生 transport/ingress rejection fact，不能算 Message accepted。Fabric ingress buffer 不是 Mailbox，不能拥有应用 Delivery backlog；但它与 Zenoh channel 同样必须限制 items/bytes/age/retained bytes、暴露 overflow，并纳入系统总预算。Mailbox 负责已验证 Message 的压力语义，Execution Domain 负责执行，Inspector 负责观测。普通 CardInstance 私有实现上下文只获得为 CardDefinition 声明端口编译的 `PortBinding`；确有动态发现或桥接需求的实现必须显式申请 scope 指向 Fabric resource 的 `CapabilityGrant`，只有 Fabric 实现持有原生 Zenoh Session。权限由 Grant 决定，不由 CardInstance、CoreService、Driver 或 Gateway 类别决定。

同一 DeploymentRevision 和活动 BindingEpoch 下，一个 BindingId 恰有一条接收新 Message 的 active route。route replacement 使用 revision-tagged `prepare → activate → drain → retire`，失败时显式 rollback；`activate` 原子切换新 Message/frame 的准入，旧 route 随即只允许排空已准入项。绝不同时走“本地直达 + Zenoh”，也不靠 payload hash、Message 内容或时间窗消除 echo。fan-out 为每个 destination 编译独立 BindingId。只有目标硬件 p99.9 延迟、CPU 与 copy profile 证明 Zenoh `session-local` 不足时，才通过独立 ADR 考虑同进程 route；它必须与 Zenoh route 互斥，并通过相同 Message/Envelope/Schema/Mailbox conformance，不能传递可变进程内对象别名、Rust runtime handle 或 Python object。

### 15.1 ROS2/DDS 边界

ParaEGOX 不自研 DDS client、discovery 或 QoS stack。存量 ROS2 系统通过 standalone `zenoh-bridge-ros2dds` 进入 Zenoh，再由窄 `ROS2Gateway` 负责 message type、topic/service/action、TF、parameter、lifecycle、Authority、Command 和 Receipt 映射。Bridge 解决连通，Gateway 解决 ParaEGOX 语义与信任边界；完整 ROS graph 不能自动变成可信 `ProvidedService` 或 `CapabilityGrant`。

对我方可控制的新 ROS2 节点，可以另设 `rmw_zenoh` DeploymentProfile。但它使用 ROS2 RMW 自己的 key expression、CDR attachment 和 graph 映射，不等于 ParaEGOX 原生 keyspace，也不能与 ros2dds bridge 作为一个默认混合路径。两种 DeploymentProfile 必须互斥、可验证、可回滚。

非 ROS2 的原生 DDS 是另一个按需边界：只有具体设备、IDL、QoS 和验收场景出现后，才以官方 standalone DDS bridge 配合窄 DDSGateway 接入；不能因为“以后可能需要”就预建第二套分布式 Backend。

### 15.2 Web Console、WebRTC 与 WebXR 边界

浏览器协议只形成 Gateway 外部腿，不扩展 Zenoh locality 或 production binding 枚举：

```text
Web Console ── HTTPS/SSE/WS ──> ConsoleGateway ──> InspectionClient / OpsClient
TUI / CLI ──────────────────────────────────────> InspectionClient / OpsClient

Browser/XR ── HTTPS signaling + WebRTC ──> Web Gateway roles
                                                │ typed seam（待 ADR）
                                                ▼
                                    media Card / controller-role Card
                                                │ internal PortBinding/Operation
                                                ▼
                                          Zenoh-native Fabric
```

- `ConsoleGateway` 的 cache 只是带 source revision、observed time 和 freshness 的投影；行政写操作只以 OpsProtocol `ControlRequest` 进入 OpsService，不能直写 RuntimeHost、PID、Zenoh key 或 Deployment store。ConsoleGateway 只按 external exposure profile 部署，不是每 Node 必备组件；TUI/CLI 不需要绕行 Web BFF。
- WebRTC 终止 signaling、ICE/STUN/TURN、DTLS/SRTP、PeerConnection、media track 和 DataChannel；它不是内部 Bus、RPC 或 Fabric backend。
- WebXR 位于浏览器前端与 XR semantic adapter，产生带限定 session/stream epoch、sequence、time、`FrameRef`、CalibrationRef、uncertainty 和 freshness 的输入；它不是 Transport。
- `FrameRef` 只表示物理坐标系引用，媒体载荷使用 `MediaSample`/`EncodedVideoSample` 等领域类型，并通过 BlobRef/BufferRef 表达大载荷所有权。
- Camera/Audio Driver 只拥有设备/SDK 边界，不自托管公网 `/offer`、MJPEG、静态网页或 PeerConnection。编码 transform 可由满足独立复用、配置、执行和观测边界的 CardDefinition/Card 承担。
- XR 连续输入不逐帧经过 OpsService；Gateway admission 后走待冻结的 typed endpoint，再由 Controller-role Card/Teleoperation owner 产生 Command。离散 arm/mode/operation 与行政操作仍走可授权、可回执的可靠 API。
- 同一活动控制输入在 WebSocket/DataChannel 外部腿上只能有一条 accepted route；其 peer/stream epoch 不能复用内部 BindingEpoch，也不能双投后按内容去重。

当前 RuntimeOwnershipTree 只有 CardInstance/ServiceInstance，Deck Link 也只连接 Card Port，因此 managed Gateway workload 与非 Card Gateway endpoint 的计划/运行合同尚未完整。进入实现前必须裁决 Runtime 中性 managed-instance envelope、`ExternalWorkloadAdapter`、Deployment-owned exposure/binding 或窄 ServiceContract 的边界；不能为填空把 Gateway 强制包装成 CardDefinition、Card 或 CoreService。完整证据、协议矩阵和阶段计划见 [Web Console、WebRTC、WebXR 与交互式 Gateway 边界研究](../research/web-console-webrtc-webxr-gateway-boundaries.md)。

### 15.3 消息类型

| 类型 | 含义 | 默认交付关注点 |
| --- | --- | --- |
| `Signal` | 采样、状态流、控制参考值 | freshness、latest-wins、允许显式合并 |
| `Event` | 已经发生的不可变事实 | 有序、可选持久和可重放 |
| `Command` | 请求物理或软件状态变化 | 权限、期限、幂等、不得静默丢失 |
| `Query` | 只读请求 | 超时、取消传播和结果版本 |
| `Receipt` | 接受、拒绝、执行和失败证据 | 与原请求、身份和 Trace 关联 |

系统只承诺一个 Link 或 Mailbox 内声明过的顺序，不承诺所有订阅者之间的全局完成顺序。

### 15.4 逻辑平面

- Safety/local control 不依赖远端 Fabric 存活。
- Control/Command 可靠、有界、可授权、可回执。
- Observation/Data 优先 freshness 和吞吐，可以按声明丢弃。
- operations projection、Console fan-out 与 Telemetry 使用后台优先级，不反压控制或 Safety 路径。

这些平面是否需要独立 Session、连接或进程由目标平台基准决定；语义隔离必须先于物理调优存在。

证据、备选方案、Zenoh 1.9 能力与 ROS2 DeploymentProfile 限制见 [Kernel 消息机制、Fabric、Evidence、Telemetry 与 Security 边界研究](../research/kernel-messaging-fabric-evidence-security.md)。

## 16. 调度与所有权

ParaEGOX 将过去容易混在一起的调度职责拆开：

| 组件 | 唯一职责 |
| --- | --- |
| `AdmissionPolicy` | 判断工作是否允许进入 |
| `Dispatcher` | 每个 ExecutionDomain 内唯一拥有 ready Mailbox 派发权；不建立全系统中央 Dispatcher |
| `TimerService` | 在确定时间唤醒工作 |
| `RecoveryEngine` | 在 RuntimeHost 内根据 LivenessState、RuntimeFailureFact、RecoveryPolicy 与预算求值 RecoveryDecision；不持有运行对象或执行动作 |
| `RuntimeHost` | 作为唯一生命周期 owner 执行 start、stop、terminate、restart、quarantine 与 cleanup，并记录 RecoveryAction/Receipt |
| `AuthorityGate` | 应用 AuthorityDecision，判断请求者是否有权提出操作 |
| `ResourceCoordinator` | 签发 LeaseGrant/FencingToken，判断当前控制权 |
| `SafetyIslandAdapter` | 接入独立 safety island 的当前状态、许可与证据；不拥有实际安全功能 |
| `EnforcementPoint` | 在副作用前验证全部判定并产生阶段 Receipt |

调度分类来自已验证的 Link/DeliveryProfile 与 `DeploymentPlan.execution`，不信任 producer 在消息中自报最高优先级。Dispatcher 至少执行 deadline/freshness-before-run、priority/fairness、最小服务份额和 max burst；具体 weighted/deficit/deadline-aware 算法由基准选择，不写进 Deck Schema。严格优先级不能让非安全工作永久饥饿，最高优先级也不能抢占一个已经开始且不主动让出的 Rust future、Python callback 或其他非协作 invocation。

Deck 只能请求 control/high criticality，DeploymentPolicy 必须授权并保留容量；arrival envelope、payload 上界、max inflight 和 Card invocation run bound 不能支撑 SLO 时拒绝计划，不静默降级。deadline 分配 ingress/queue/run/effect/cleanup 当地 monotonic budget，并为运行中 overrun 编译 continue/cooperative_cancel/escalate/uncertain；跨 Node 不直接比较远端 monotonic timestamp。

当前 S4 实现注记：plan-time utilization 与 Control start bound 使用完整 `run + invocation cleanup` 占用，并按全部 signed class/capacity/arrival envelope 计算串行最坏顺序，要求严格早于 queue-age/delivery deadline。Runtime ingress 尚无 sliding-window arrival observation/enforcement，effect budget/production Receipt 也未落地；所以这是一项在 producer 遵守 signed envelope 时成立的 fail-closed 准入合同，不是无条件运行期或硬实时保证。

共享设备或不可重入实现使用 ResourceClaim、唯一 resource owner 和 ordering/supersession rule，不用跨调度等级共享锁或创建专用 Lane。策略对象返回 Decision 和原因，不直接操作进程或设备。Runtime 中的副作用执行者只能应用经过授权的 Decision，并写出 Receipt。

`block_until_deadline` 不允许出现在 Transport callback、Runtime control/Safety/shutdown、持有 exclusive ResourceClaim 或 Deck 有环 Link 的路径。物理 Stop/Enable 等先后语义由 resource owner 的 ordering key、fencing 和 supersession 保证，不由 priority queue 重排。

## 17. 生命周期与启动

### 17.1 启动链

```text
independent MCU / PLC / device safety island ──already enforcing safe outputs──┐
                                                                              │ adapter boundary
OS service manager
├── DeploymentTenureAuthority（OS lock + signing key）
├── RuntimeHost bootstrap
│   └── Clock + Identity + apply control endpoint + apply journal + Inspection
├── NodeDaemon（P5；不同故障域 watchdog / Node observed facts / endpoint discovery）
└── DeploymentController process（不由目标 RuntimeHost 部署）
                │ acquire new writer tenure
                ▼
        DeploymentTenureAuthority

DeckSpec → DeckCompiler → DeckLock {canonical DeckTopology}
                           │
                           ▼
                  DeploymentPlanner
                           │ DeploymentPlanCandidate
                           ▼
                  DeploymentController atomic commit
                           │ committed plan + RuntimeApplyRequest
                           ▼
                      RuntimeHost
                           │ RuntimeAssemblyEngine
                           │ S7: fixed idle Loop apply/query/reconcile
                           │ future successor: general prepare/ready/activate
                           ├── Authority / Resource / SafetyIslandAdapter ◄─────┘
                           ├── FabricService + other planned Core Services
                           └── Cards → CardInstances
```

OS bootstrap 只拉起 tenure authority、RuntimeHost 的最小 apply/control substrate、P5 NodeDaemon 和独立 DeploymentController。S7 committed request只装配manifest-pinned compiled-in idle Loop fixture或canonical empty，不装配Authority/Resource/Safety、FabricService、其他CoreService、PortBinding、Thread/Process或streaming Card；图中这些分支需要后继一般assembly successor。未来managed workload仍必须来自committed plan的RuntimeApplyRequest，不能由手写启动顺序形成第二份desired state。NodeDaemon只发布Node facts与RuntimeApplyEndpoint discovery，不接收、改写或拒绝RuntimeApplyRequest；DeploymentController仍直接调用RuntimeHost拥有的RuntimeApplyEndpoint。P2 direct adapter与P4/P5 authenticated bootstrap control adapter只是同一apply protocol的承载方式；后者可以使用受限Zenoh session，但不能安装应用PortBinding、开放通用pub/sub或冒充已由计划启动的FabricService。DeploymentController必须先取得新tenure才能commit/apply，正常重启也不能复用旧DeploymentWriterEpoch。OpsService是后续普通CoreService，DeploymentController bootstrap/reconcile不依赖它；窄开发/救援CLI只能调用DeploymentController API，不能直写RuntimeHost。

当前实现注记：上图的一般 assembly 分支仍是目标态。S7-E 窄路径已由 Ubuntu CI `30748840399` 验证；S7-F 又提交 authenticated Runtime query、payload v5、v4→v5 migration 与 fixed Loop/Empty listener-before-publish restart reassembly，并由 Ubuntu CI `30782979187` 验证。当前工作树中的 Controller `reconcile-reference-once-v1`/bounded `reconcile_once` 和 SF6 Linux scenarios 仍待提交及 fresh Ubuntu CI，macOS 对该 Linux ext4 profile 只 skip，所以还不能宣称完整 S7 闭环。S6 的独立 POSIX service-manager adapter 继续拥有 RuntimeHost child/process-group、restart/backoff/quarantine ledger 与 TERM/KILL/reap，但不因此成为 Deployment desired-state owner；一般 plan-owned CoreService/Card/ExecutionDomain assembly 仍未实现。

RuntimeHost 只能启动/管理 Adapter，不能启动、停止或声称拥有实际 safety island；Adapter 未 Ready 时物理写入 fail-closed，但 safety island 仍独立工作。每个阶段等待依赖的 readiness，而不是只检查进程存在。依赖暂时不可用时，消费者进入明确的 waiting、degraded 或 blocked 状态，不通过隐式循环假装启动成功。

### 17.2 关闭链

```text
stop accepting
      ↓
drain in-flight work until deadline
      ↓
cancel cooperative work
      ↓
terminate isolated process if required
      ↓
release resources and publish final Receipt
```

关闭必须幂等，并在进程路径中完成 TERM、KILL、process-tree cleanup、join 和 IPC/SHM/FD 清理。ThreadDomain 卡死后只能报告 wedged，并升级到进程或节点级恢复；不能声称线程已经被杀死。父 scope 只有在 Task join、thread work 真实返回或 process 真实退出并清理后，才能报告关闭完成。

### 17.3 Out-of-band Watchdog

主 RuntimeHost 可能在启动或运行期因同步设备打开、模型加载、CPU spin、原生库调用或 event-loop stall 卡死。同进程 lag monitor 只能提供诊断，不能证明或恢复自身 failure domain 的 liveness。因此生产部署需要由不同进程/故障域中的 NodeDaemon 与 OS service manager 持续覆盖 bootstrap progress、运行期 heartbeat、control-channel responsiveness、process tree 和 restart budget。NodeDaemon 产生节点级 LivenessState、请求受限恢复并发布事实；OS service manager 是 RuntimeHost 整体进程的实际生命周期 owner。每个 profile 必须指定唯一 restart-budget/quarantine ledger 与 mutation owner，禁止两者形成双重重启循环；NodeDaemon 自身也由 OS service manager 托管。

Watchdog 先收集有界诊断证据，再执行预算化 TERM/KILL 与重启；相同阶段在时间窗口内反复失败时进入 quarantine，禁止无限 restart storm。development profile 可以共进程简化启动，但不能据此宣称具备生产恢复能力。

S6 已实现 PXHW v1 与独立 POSIX OS service-manager/watchdog reference adapter，但尚无 NodeDaemon。该 adapter 的 cleanup proof 只覆盖 RuntimeHost exact process group；RuntimeHost 被不可捕获 SIGKILL 后，独立 process group 的 ProcessDomain worker 仍需要 shared cgroup/job object 或 durable external ownership journal 才有外部清理 owner。PXHW 的 exact-generation reactor progress 也不等于 Runtime/ProcessDomain readiness 或 recovery Receipt。

### 17.4 DeploymentRevision 转换

下图是未来一般assembly successor的目标顺序，不是S7/P2e已经批准的shape：

```text
prepare(new revision) → validate/capacity reserve
                      ↓
activate → route new work to new revision
                      ↓
drain old → retire old resources
                      └── failure: rollback or quarantine
```

S7没有Binding、Invocation、一般replacement或rollback。其合法转换只包括`None/terminal empty → OneSourceLoop`、`LiveReady OneSourceLoop → EmptyDeactivate`，以及canonical empty或nonempty `RecoveryFailedNotReady + ExactZero`在exact CAS、更高revision下到`EmptyDeactivate`的no-retire收敛；Loop→Loop必须先完成empty terminal exact-zero，再以更高revision启动。`OneSourceLoop`在readiness成功时原子提交desired+live+terminal；`EmptyDeactivate`遇到live/nonzero generation时先在同一事务写`FirstActionIntent`、`NoNewAdmission`、empty desired head+`HeadCommittedRetiringOld`，再drain到exact-zero terminal，already-exact-zero且无action/resource时则走无intent/callback的单事务fast path。旧revision/recovery action由revision+RuntimeHostEpoch+resource generation fencing，callback不得在crash后replay。

未来一般profile的每目标prepare/activate/drain/retire/rollback仍由内部RuntimeAssemblyEngine执行，DeploymentController只协调阶段和解释跨目标结果；consumer ingress先于producer egress且DataLink不自动变成启动依赖。S7只协调fixed Loop/empty apply、query与reconcile，没有Ingress/egress。RuntimeAssemblyEngine从Slice派生的关系只在apply内有效，不持久化为第二份desired graph。

跨RuntimeHost的未来切换不能伪称瞬时原子事务；一般successor才可协调多目标prepare、consumer ingress/provider与producer egress。S7 reference是单target，但timeout/crash仍必须保留operation/live的Known/Conflict/Unknown/Indeterminate与per-target uncertain facts，通过authenticated query/reconcile裁决，不能写全局布尔成功或猜测重发。

## 18. 物理 Authority 与 Grounding

物理世界的写操作具有独立于 Agent 推理的安全边界。Command 至少携带：

- 调用身份与能力引用。
- `ControlledResourceRef` 与 `Operation`。
- Deadline。
- Idempotency Key。
- LeaseId 与 FencingToken。
- 期望状态变化或约束。
- Trace 与因果关联。

`Controller-role Card` 不直接调用 Driver。Deployment 安装静态 typed `OperationClient/CommandEndpoint`，首版只支持 1:1、bounded outstanding、deadline、command digest、idempotency key、阶段 Receipt 与无透明 retry；fan-out、动态 routing、partial acceptance 和跨资源 transaction 未经独立合同研究均 fail-fast。

物理写路径按以下 owner 链工作：

```text
IdentityResolver
      ↓
AuthorityService → AuthorityDecision
      ↓
ResourceCoordinator → LeaseGrant + FencingToken
      ↓
SafetyIslandAdapter → SafetyDecision
      ↓
Driver EnforcementPoint     # normal-command 最后软件准入
      ↓ setpoint
Safety island / device-native gate
      ↓ applied output
Actuator → staged Receipts
```

`CapabilityGrant` 只表示“允许请求”，Lease 才表示“当前控制者”。Authority 与 ResourceCoordinator 首版可以同进程，但 API、状态和 Receipt owner 必须分开。ResourceCoordinator 只拥有 lease/fence 分配状态；Driver EnforcementPoint 拥有 normal-command 的最后软件比较、idempotency 与 setpoint submission；物理下游 safety island/设备原生 controller 独占 enable/inhibit/clamp/safe-output gate 与最终 applied-output ack。它们可以同故障域部署，不能借共置合并逻辑 owner，更不能成为两个平级 writer；远端 Authority 只能授权请求。没有这种下游仲裁或经证明的等价 device-native safety，H1 不得通过。Lease expiry 使用 issuer/EnforcementPoint 可共同解释的本地 monotonic 语义，不能直接比较远端 monotonic timestamp。

每种设备必须声明 fencing support：`device-native` 由硬件持久比较 token；`driver-proxy` 由 Driver 持久比较，但 Driver/主机 crash 后必须 stop/drain/reset/observe，不能只恢复 ledger 就接管；`unsupported` 不允许透明 takeover/retry，无法证明安全终态时 quarantine。SDK 调用返回不等于 device completion。

不可变 `PhysicalCommandEnvelope/EnforcementContext` 绑定 command/requested-operation digest、ControlledResourceRef/Operation、DeviceIncarnation/DeviceSessionEpoch/DriverBindingEpoch、PhysicalAssemblyRevision、CalibrationRef/CalibrationRevision、ControlModeEpoch、SafetyEpoch、LeaseIssuerEpoch/LeaseId/FencingToken、deadline、CommandSequence、IdempotencyKey、AuthorityDecisionRef/digest、SafetyDecisionRef/digest，以及 Operation 声明要求的 PowerState/ThermalState/OperatingEnvelopeEvaluation ref、revision 和 freshness；real profile 还绑定 local `HardwareActivationRef/Epoch`。每个 Decision 反向绑定 command/operation digest、resource、device/session、audience、policy/epoch 和 expiry；不能只比较 policy revision。Driver EnforcementPoint 在提交 setpoint 前原子读取当前事实并全量比较；之后若 safety trip 与迟到 normal command 竞态，物理下游 gate 必须保持或进入 safe output，不能让迟到 setpoint 覆盖。

Observation 由受信 Driver/Scenario boundary 生成不可变头部，至少绑定 device/incarnation/session、driver binding、channel/sequence、measured/received time、clock domain/mapping revision/uncertainty、frame/frame epoch/transform revision、unit/dimension、CalibrationRef/CalibrationRevision、quality/validity/covariance、origin/environment/provenance；不相信 payload 自报这些字段。普通 PortBinding 只验证 transport principal/BindingEpoch 或添加 transport receive metadata，不能把 CardInstance 所发 payload 提升为真实设备 provenance。`unknown uncertainty` 不等于零，跨时钟域只经带 measured-at/valid-until/source 的 mapping 转换，lease/deadline 只使用 owner-local monotonic。Frame/transform/calibration revision 改变后，即使 Ref 字符串不变，旧 Observation 也不能满足新 consumer。

设备事实必须分 owner 和代次：Driver/设备 adapter 拥有 raw presence、observed firmware/ABI/config、hardware ack、raw power/thermal telemetry 与 DeviceSessionEpoch；binding owner 拥有 DriverBindingEpoch；Deployment 拥有期望 firmware/config/assembly；Calibration owner、mode owner、ResourceCoordinator 与 safety island 分别拥有 CalibrationRevision、ControlModeEpoch、LeaseIssuerEpoch/FencingToken 与 SafetyEpoch。DeviceService 只从同一组输入 revision 原子派生带 derived-at/valid-until 的 DeviceReadinessSnapshot 和 PowerState/ThermalState；产品域 `OperatingEnvelopeEvaluator` 独占产生绑定 ODD/World/state revision 的 OperatingEnvelopeEvaluation；safety island/Adapter 消费这些事实并拥有 inhibit/SafetyDecision，Continuity/Admission 只消费。任一输入换代立即使相应派生事实失效，不能拼接跨 session 事实。PhysicalAssemblySpec 是 desired Device↔Resource mapping，真实设备 realization 需 commissioning/binding Receipt。USB path 或 DeviceRef 未变也不能阻止 device reboot 推进 DeviceSessionEpoch。

Control mode handoff 必须执行 `request → stop/quiesce → observed-safe/neutral → release old lease → acquire new lease → activate new epoch`，不能只推进 epoch。CommandSequence 的比较 scope 是 ControlledResourceRef + LeaseIssuerEpoch/LeaseId + ControlModeEpoch，duplicate、gap、supersession 与被替换命令各有唯一终态 Receipt；emergency 是 safety function/state，不是普通 control mode。

issuer 或 EnforcementPoint 重启时生成/要求新的相应 epoch，此前 lease 全部失效；在恢复 fencing/idempotency、停止状态与 device completion 前物理写入 fail-closed。首版不恢复跨重启旧 lease，避免把已经失去可比较 monotonic 基准的 expiry 当作仍有效。

Safety clamp 必须产生 permitted/applied operation digest。EffectReceipt 区分 requested、authorized/permitted 与 applied operation，并记录 SafetyEpoch、device-send ack、device completion ack 与 observed-effect evidence level；只有 Operation contract 要求的 completion level满足才能 `Succeeded`，Driver SDK 返回、enqueue 或 accepted 不能直接产生该终态。

`accepted` 不等于 `succeeded`。超时或断连产生的 `uncertain` 必须可查询，Fabric 不得透明重放。网络分区下继续、降级或安全停止由 Deployment 编译的 immutable `ContinuityProfile` 和本地 `ContinuityController` 共同执行，不是 Authority cache 的默认值。Profile 固定 Deployment/assembly/calibration/safety revision 与所有 artifact digest，使用预先衰减的 offline Grant，并检查 clock、Power/Thermal/Operating Envelope 与 Evidence 容量；ContinuityController 不接管这些事实 owner。主机重启丢失 monotonic baseline 后默认停止离线自治，除非持久 boot counter/safe clock 或本地重新授权已被验证。

E-Stop、protective stop、deadman/enable、limit、collision inhibit 与普通控制器 Stop 分开建模。前五者位于 MCU/PLC/设备原生 safety island，不要求 control lease，也不依赖 Agent、Graph、远端 Fabric 或普通消息队列；Reset 必须绑定 SafetyEpoch、具体 safety function、Device/ControlMode、已认证操作者、全部 fresh/clear trip source 与硬件 ack。网络断开和控制面拥塞时，安全路径必须保持本地可用或明确 fail-closed。

P3 的模拟 safety island reference profile 也在独立进程/clock/deadman 中运行并拥有直达 simulated actuator 的 safe-output path；wedge/kill RuntimeHost 后仍须推进。一个与 Adapter 同进程的 Rust task、Python 对象或普通 ProcessDomain 不能证明独立 safety fault domain。

P3 还必须提供 immutable `ScenarioManifest` 与 `ScenarioRunner`，固定 runtime/deployment/assembly/world/engine/assets/seed/timestep/calibration/fault-plan digest 和 expected invariants/tolerances，隔离 truth 与 Observation，并让 fake/sim Driver 复用 contract conformance。P3–P5 的 write 证据限定 simulation profile；P6b 后的条件式 H1 才允许 real Driver/HIL 执行同一 suite，并由 Release owner 依据 Device/Safety/Evidence/hazard/ODD、rollback 与人工接管证据签发受限 HardwareEnablementReceipt。目标 Node 的 local `HardwareActivationGate` 验证 Receipt、RuntimePlanSlice 与当前 DeviceReadinessSnapshot，安装 HardwareActivationRef/Epoch；real Driver activation 和每个真实 Command 都必须引用它。Receipt 撤销/到期、readiness、ODD/Operating Envelope 或绑定 revision 变化时，本地立即推进 epoch 并 disarm/inhibit/stop，不等待 Fabric、远端控制应用或 DeploymentController。

## 19. Inspection、OPS、TUI 与 Web Console

Kernel 和 RuntimeHost 只产生结构化运行事实，不内置 Dashboard、OPS 或 TUI。事实 producer、Inspection projection、Ops control、Evidence 和 Telemetry exporter 必须保持独立 owner。`OPS` 是运维领域/产品标签；具体运行组件为 `OpsService` CoreService。完整裁决见 [ADR-0003](../adr/ADR-0003-ops-service-operation-boundary.md)。

这不是把 EAGOS 的 OPS surface 整体迁移或改名：其中的 deployment authority、只读 inspection、artifact/release、node/runtime management、Gateway 和 Agent/tooling 在 ParaEGOX 分属不同 owner；OpsService 只保留跨客户端一致的 ControlRequest lifecycle。`DeploymentController` 是独立 deployment owner，不是 OpsService 的内部 manager，也不是旧 OPS 的同义词。

### 19.1 InspectionService

InspectionProtocol 至少支持：

- RuntimeHost、Core Service、DeckRun、CardInstance 和 Execution Domain 状态，以及 `DeploymentPlan.execution` 与 observed instance 的差异。
- readiness、degraded 原因和最后故障。
- Fabric ingress buffer 与 Mailbox 各自的 items/bytes/age、容量、丢弃、拒绝、过期和等待时间；二者不可合并成一个“queue depth”。
- Dispatcher priority/fairness/max-burst、enqueue-to-start 和 Card invocation duration。
- event-loop/control-tick lag 与 slow-callback 证据。
- planned/observed Thread、Process、native pool、PID/TID、loop、resource budget、DomainEpoch 和 wedge 证据。
- Command、Admission 和 Execution Receipt。
- 一次 Transaction 的关键路径、缺失 span 和瓶颈。
- 当前 Authority、Resource Lease 和安全状态的只读视图。
- ConsoleGateway projection 的 source revision/epoch、observed time、freshness、watch lag、慢消费者 drop/resync 和 connected client 数；cache 不拥有 truth。
- Web Gateway 的限定 browser/peer/XR stream identity 与 epoch、signaling/ICE/DTLS/track/DataChannel state、selected path class、codec/profile、RTT、jitter、loss、bitrate、frame/input age、拒绝、drop 和 codec worker health。

P6a 的 node-local InspectionService role 聚合本 Node 的公开 facts；P6b 可增加 federated InspectionService role，关联多个 Node 的 snapshot/watch。InspectionService 只拥有 projection revision、cursor、cache、freshness、watch gap 与 staleness，不拥有 producer 的 raw facts、Deployment desired state、Node enrollment、Lease、Safety 或 Evidence 真相。node-local 与 federated role 可以共进程或拆分，但不能通过聚合转移事实所有权。

### 19.2 OpsService

OpsService 只拥有受权 `ControlRequest` 自身的生命周期：request id/digest、principal/approval refs、target/action、expected revision/epoch、deadline、dry-run、ordered progress、cancel/reconcile intent、owner Receipt/Evidence refs 与唯一 terminal OpsReceipt。最小 OpsProtocol 为 `preview/submit/get/watch/cancel-if-supported/reconcile-uncertain`。

同一 request id + digest 重试必须返回或推进同一 operation；同一 ID 携带不同 digest 必须 conflict。timeout、断连和 OpsService restart 后先进入 `Uncertain` 并向实际 owner 查询/reconcile，不能透明 replay。OpsReceipt 只能总结请求生命周期并引用 owner-issued Receipt，不能用 log、probe、exit code、transport ACK 或聚合结果推断 Deployment、Runtime apply、Artifact、Node maintenance 或物理 effect 成功。

OpsService 通过 typed client 把动作交给真实 owner：

| 操作 | 真实 owner |
| --- | --- |
| deploy、rollback、reconcile | DeploymentController |
| Node drain、维护、诊断采集 | NodeManagementEndpoint |
| Artifact/Release | Artifact/Release Service |
| Secret | capability-bound Secret Service client |
| 物理/不可逆操作 | Authority → Resource/Lease → Safety → CommandEndpoint/Enforcement |
| query/status/why | InspectionService + Evidence refs |

OpsService 不直接写 DeploymentPlan store、RuntimeHost、PID、Zenoh key、Artifact active pointer、领域数据库或审批状态，不持有 raw Fabric session、明文 Secret、任意 shell/SSH/package/container/systemd executor，也不内置通用 DAG/saga engine。Deployment rollout/reconciliation 完全属于 DeploymentController；XR/media/teleoperation 连续数据不经过 OpsService。

### 19.3 客户端、部署与失效

```text
Web Console ──> ConsoleGateway ──┬──> InspectionClient ──> InspectionService
                                 └──> OpsClient ────────> OpsService
TUI / CLI ───────────────────────┬──> InspectionClient
                                 └──> OpsClient

OpsService ──> Authority ──> DeploymentController / NodeManagement / typed owner
```

TUI、CLI 与 Web Console 都是客户端，不能直接 import RuntimeHost、领域服务内部对象或原生 Fabric。只有 Web Console 必须经过 ConsoleGateway；TUI/CLI 可以直接使用公开 typed clients。Gateway 的本地 cache 不能改变 desired/observed truth，也不能因已经登录或建连自动获得物理控制 Capability。

ParaEGOX 不建立每 Node 一个 OpsService 或 ConsoleGateway 的不变量。首个 distributed reference profile 使用一个独立管理侧 OpsService 服务多个 Node；constrained/local profile 可以共置，但 operation owner 仍唯一。OpsService 作为普通 CoreService 可由 DeploymentController 管理；DeploymentController 本身由独立 bootstrap 拉起且 reconcile 不依赖 OpsService。OpsService/ConsoleGateway 故障只影响新运维请求和可见性，不停止 RuntimeHost、Deployment reconciliation、Continuity 或本地 Safety。P2e 的受限开发/救援 CLI 可以直达 DeploymentController API，但不能绕过 plan commit、tenure proof、Authority 或 Runtime fencing。

一次可诊断事务应该能回答：

1. 请求从哪里进入。
2. 经过哪些 Service、Port、Mailbox 和 Domain。
3. 在哪里等待或失败。
4. 哪个主体做出了准入或恢复决策。
5. 哪些证据缺失，因此当前结论仍不确定。

## 20. 建议 workspace 与逻辑 owner 结构

物理 workspace 服从逻辑 owner，不把所有架构名词机械拆成 crate。P0/P1 只创建当前纵向切片已有真实 producer、consumer 和验证入口的 workspace member；下图是阶段性目标，不是立即创建所有目录的要求：

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml

crates/
├── paraegox-kernel/              # 纯基础 value/mechanism；不依赖 Runtime/Deployment/Zenoh
├── paraegox-runtime-contracts/   # 已有 Slice/apply/assignment/execution consumer-owned wire contract
├── paraegox-runtime/             # 已有 admission/Mailbox/Binding/Loop/Thread 与 private local POSIX ProcessDomain/Liveness/Recovery baseline；P2e 才接 assembly
├── paraegox-runtime-host/        # 已有 idle process/reactor root、PXHW endpoint 与独立 POSIX service-manager/watchdog adapter；P2e 才接 public apply/assembly
├── paraegox-deployment/          # 已有 private projector/builder；P2e 才扩展 single-writer control plane
├── paraegox-fabric-zenoh/        # P4 才建立；唯一 production Fabric owner
├── paraegox-node/                # P5 才建立；NodeDaemon
└── ...                           # cards/decks/physical/services 只随真实切片准入

pyproject.toml                    # 仓库根 uv 项目；已有 Python contract/reference worker
uv.lock
src/paraegox_sdk/
└── worker/                       # 已有 PXWP v1 Python reference worker/binding；不是 Python RuntimeHost

tests/
├── fixtures/wire/                # canonical bytes、digest、valid/invalid/version vectors
├── contract/                     # Rust↔Python/C++ conformance
├── scenario/
└── system/

apps/
└── console/                      # 条件式 Web Console/WebXR client Artifact；不是 CardDefinition
```

`paraegox-kernel`、`paraegox-runtime-contracts` 和首个真实 consumer 可以在同一 bounded batch 建立；不得为了目录对称预建 deployment/fabric/node/service 空 crate。逻辑 Kernel/Runtime/Deployment/Card/Deck/CoreService 边界仍由 owner 与依赖规则定义，而不是由 Cargo workspace 自动推导。共同 workspace、共同 binary 或开发期共进程不允许 Runtime import Deck/Deployment，也不允许 DeploymentController、NodeDaemon 与 RuntimeHost 合并写 authority。

Python SDK、worker 和 Agent/模型服务是一级支持对象，但不建立与 Rust core 平行的 Python RuntimeHost。`runtime/assembly` 只在 P2e 的真实 target Slice producer 和单 Node apply vertical slice 同批出现；Gateway、OPS、TUI、Agent、Memory、World、ROS2 与 Web 目录只在各自 admission gate 和真实调用链成立后创建。当前不创建顶层通用 `graph/` 或 `kernel/graph/`；Graph Foundation 仍等待两个独立生产消费者和 ADR 准入。

P2e public/persistent surface也必须随真实调用链出现：S7-B已把`RuntimeApplyEnvelopeV2`、descriptor/identity/singleton manifest/projection、Runtime bootstrap/query request+response及channel-auth transcript、successor codec/builder保持在`paraegox-runtime-contracts` internal，并由提交`fb1547d`、Linux test-fixture follow-up `f2c6593`和CI `30617157810`闭环；这本身不构成public endpoint或可启动control plane。S7-C提交`54ebed8`只接crate-private DeckSpec→DeckLock、typed validation、pure Planner candidate、stable allocation delta与opaque manifest selection，未promote S7-B类型；本地61个deployment tests、359个Python tests、完整门禁、独立复核和CI `30622634625`均通过。S7-D提交`1207f4e`、`47da71e`、`08389f3`已实现`acquire_tenure` request/response/framing/auth transcript、real Authority process/initializer/store和Controller/Runtime internal journal model，CI `30729690319`中Linux Python 369 tests无skip且Rust完整门禁通过；proof value仍复用runtime-contracts。S7-E W1 随后提交 owner-local store、Authority client 和 initializer foundation；S7-E 主提交 `14d0012` 及 follow-up 至 `1ed704c` 已把 descriptor/install artifact、Runtime initializer、compiled-identity check、operator Deck/manifest ingress、`paraegox-deploymentd` Authority client 以及 authenticated Runtime bootstrap/apply 接成真实 producer/consumer，并由 Ubuntu CI `30748840399` 验证 Linux-only process fixture。S7-F 后续提交了 authenticated Runtime query endpoint/client foundation，以及 payload v5、v4→v5 migration 和 journal-bound fixed Loop/Empty restart reassembly，最新由 `f32700f` / Ubuntu CI `30782979187` 验证。当前未提交工作树中的 deploymentd reconcile facade与 SF6 tests 已形成真实 producer/consumer 候选，但在提交和 fresh Ubuntu CI 前仍不能登记为完成证据；macOS 只 skip Linux ext4 profile，也不能据此宣称跨平台或 S7 complete。

### 20.1 Rust/Cargo 与 Python/uv

Rust core 使用根 Cargo workspace、committed `Cargo.lock` 和 pinned `rust-toolchain.toml` 作为唯一构建与依赖权威。Python 环境继续统一使用 `uv`：Python `pyproject.toml` 和 `uv.lock` 管理 SDK、worker、Agent/模型服务、测试辅助和治理工具，不维护并行 `requirements.txt`。uv 不能替代 Cargo，Cargo 也不管理 Python 环境；把 `cargo` 包在 `uv run` 后面不消除第二工具链。

首个 Cargo workspace 落地后，核心目标工作流至少包括：

```bash
cargo fmt --all --check
cargo metadata --format-version 1 --locked
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

当前仓库的 Python 治理工具仍使用：

```bash
uv sync --locked
uv run --frozen ruff check .
uv run --frozen python scripts/check_governance.py
uv run --frozen pytest
```

CI 分开 `rust-core`、`python-tooling-sdk` 和跨语言 contract/system suite，并分别验证两个 lockfile 未漂移。Rust core 的最小 feature profile 不安装 Python、Zenoh、ROS2、数据库、模型或硬件 SDK；Python 重依赖通过 uv group/marker/required-environment 显式表达。发布兼容矩阵记录 Rust toolchain、target triple、libc、CPU feature、crate features、wire/workload protocol，以及相应 Python ABI/dependency、ROS2、GPU 与设备版本。

## 21. 实施顺序

| 阶段 | 交付结果 | 主要验证 |
| --- | --- | --- |
| P0 边界与工程冻结 | ADR-0006 的 Rust-first/polyglot 边界、Cargo/uv 双工具链、Capability/Service/Feature 三义、VFS 拒绝、Card/Deck/Deployment Kernel 外、DeploymentPlanner/DeploymentController/RuntimeHost owner、RuntimePlanSlice import 边界、ConfigSnapshot 链和知识产权边界 | 术语与 crate/import graph 检查；Rust core 与当前 Python governance 环境分别可重建；Runtime 不安装 deployment/decks 仍可消费 plan fixture；Kernel 不出现 Deployment/Zenoh 类型；两个 lock authority 不漂移 |
| P1 Kernel/Deployment-Runtime/Physical Contracts 与 Time | Rust owner-specific value/state machine、language-neutral wire Schema、ID/epoch/revision、Failure、Message/Receipt、Grant/Lease、Port/Schema、Runtime PlanProvenance/PlanWriterContext/Slice/apply、pure RuntimeSliceProjector/ApplyEnvelopeBuilder、Cancellation/Timer、Blob/Buffer lifetime、分型 Instant、可信 ObservationHeader、最小 Command interaction 与 Frame/Unit/Calibration/Device refs | Rust unit/property test + Rust↔Python golden/error vectors；Runtime 不 import deployment；相同 committed plan/target 产生稳定 tenure-neutral Slice；writer context 不改变 plan/slice digest；跨时钟/单位/frame/transform/calibration mismatch fail-fast；Controller-role Card 无法直连 Driver；不依赖真实 sleep |
| P2a Mailbox 与 PortBinding fixture | item/byte/age 有界 Mailbox、OutstandingBudget、payload ownership、EnqueueResult、`DeploymentPlan.bindings` fixture 与 live `PortBinding` contract | offer/lifecycle 守恒；多实例不串线；fixture 不进入生产 plan/API；queued/inflight/retained bytes 有界；Command 不静默丢失 |
| P2b LoopDomain 与 Dispatcher | reference 单业务 loop、结构化 Task owner、ready Mailbox 仲裁、execution admission 与 late-generation fence | 阻塞/CPU workload 被拒绝；criticality 授权、run+cleanup deadline、条件式 control start bound、本地 structured shutdown；P2e只接fixed idle Loop→empty lifecycle，一般revision替换后置 |
| P2c ThreadDomain 与 ExecutorBudget | 有界 sync executor、真实取消/wedged、late-result fencing、planned/observed thread inventory | stuck call 扣减容量、不超预算补 worker、native pool 上限和关闭竞态 |
| P2d ProcessDomain、Liveness 与 Recovery（S6 local POSIX baseline 已落地；production containment/journal deferred） | explicit start、持续 heartbeat、credit/epoch IPC、TERM/KILL/tree cleanup、LivenessSpec、FailureContainmentSpec、RecoveryPolicy/Engine、restart quarantine | crash 的 Uncertain/不 replay、device reset/fencing、grandchild/stale IPC 和外部 watchdog |
| P2e 最小 Deployment control plane 与单 Node reference apply | deterministic DeckCompiler→DeckLock与typed graph rejection、deterministic DeploymentPlanner、`RuntimeBuildDescriptorV1`/singleton `RuntimeArtifactCompatibilityManifestV1`/`RuntimeBuildIdentityV1`、PXTE v4/PXAR v5 `ReferenceAssemblyProfileV1::{OneSourceLoop, EmptyDeactivate}`、原子candidate commit、`RuntimeApplyEnvelopeV2` exact store binding、single-writer DeploymentController、OS-service-managed DeploymentTenureAuthority、三个owner initializer/journal、authenticated Runtime bootstrap/apply/query、idle Loop restart reassembly，以及live/nonzero empty head-first retire或exact-zero fast path；只有两个真实消费者证明交集后才抽取internal Graph Foundation | release pipeline唯一产descriptor；registered operator install surface严格验descriptor/artifact、一次唯一产singleton manifest并byte-identically交Runtime initializer与operator/Controller/Planner，拒绝prebuilt/rebuilt manifest；只允许zero-binding、zero/one reference Loop records，profile固定1 lifecycle slot和零mailbox/dispatch/background slots；sequence-1持久exact descriptor/manifest bytes+digests，startup只验snapshot与binary compiled actual；expected store mismatch在任何mutation前拒绝；normal `FirstActionIntent`与recovery `RecoveryPlannedNoEffects → StartCallIntent`分型；intent/head构造前与publish后/effect前检查deadline；raw outcome先latch、exact-zero后再terminal selection；invalid snapshot不提供authenticated state、validated quarantine才可`Indeterminate`；一般activation/Ingress/Thread/Process/Link在candidate前拒绝；build/store不可原地upgrade/downgrade；restart/timeout/重复提交不产生双action/live generation |
| P3 本地仿真物理闭环 | Authority、Lease/Fencing、SafetyIslandAdapter、CommandEndpoint、ContinuityController、ScenarioRunner、Driver normal-command admission、下游模拟 safety gate 与 actuator | 无权、过期、竞争、重启、设备/空间/校准/mode/safety/operating-fact 换代、迟到 command 与 safety trip 竞态、断网自治、truth/observation 隔离和 uncertain；不宣称真实硬件安全 |
| P4 Zenoh-native Fabric | `session-local`、`host-local`、`remote` route 实现、bounded Fabric ingress/validation、session/binding epoch、keyspace、query、liveliness；本阶段实测同 session/同主机，P5 才做双主机 remote | 与 P2 fixture 运行同一 conformance；malformed frame 不成为 Message/不进 Mailbox；断连恢复、优先级隔离、single-active route 和本地基准 |
| P5 Node 与持续 Deployment reconciliation | 双主机、Node facts、把 P2e 已持久的 DeploymentController/RuntimeHost journals 扩展为 per-target cross-node rollout facts、分阶段 rollout 与 bounded reconcile；仍为 single-writer DeploymentController | 分区、重连、DeploymentController restart、partial apply、stale 与旧代次隔离；不宣称 HA/共识或自动迁移物理 workload |
| P6a local Evidence/Inspection | local durable Receipt、每 Node 的 node-local InspectionService role/incident；聚合 P2 起已有 raw facts | 进程/NodeDaemon 重启与 storage-full；node-local query 不依赖 federated service，不在此阶段首次补运行指标 |
| P6b distributed Evidence/OPS | P5+P6a 后的 replication/lag、federated InspectionService role、单实例 OpsService/ControlRequest journal、跨 Node correlation、Release/audit | 分区、重复上传、same-id/different-digest conflict、OpsService restart/Uncertain 不重放、owner Receipt chain 与 why 闭环 |
| H1 Hardware Enablement | 具体设备 commissioning/readiness、real Driver conformance、HIL/timing、独立下游 safety gate、hazard/ODD、HardwareEnablementReceipt 与 local HardwareActivationGate | fake/sim 不代签；每次真实 Command 绑定 activation ref/epoch，Receipt/revision/readiness/ODD 即使分区也可本地失效 |
| P7 TUI | OpsClient + InspectionClient | 读取只走 InspectionProtocol、写入只走 OpsProtocol；不访问内部对象，stale/unknown 正确展示 |
| P8 ROS2Gateway | bridge 与 `rmw_zenoh` 互斥 DeploymentProfile、ROS 语义映射 | 不自研 DDS；物理命令不绕过完整 EnforcementPoint |
| P9 SpatialMap 与语义导航 | FrameGraphRef、SpatialMapRef、MapEpoch、SemanticRegionRef | 地图换代与作用域隔离，不进入 Kernel |

纯 Contract/Time 与 P2a→P2b→P2c→P2d 的 Mailbox、Loop、Thread、Process reference slices、P2e/S7-E executable vertical，以及 S7-F 已提交的 Runtime query/payload-v5/fixed restart reassembly 已依次形成 CI 证据；当前仍须把工作树中的 bounded Controller reconcile 和 SF6 Linux system closure 完成独立复核、提交并通过 fresh Ubuntu CI，才能把 S7 标为 complete 并进入 P3。Authority/Resource/Safety 的本地仿真闭环必须早于 Zenoh；跨主机 Node/Deployment 验证必须晚于同主机双进程 Fabric。ROS2Gateway 与空间语义都不阻塞 Kernel Foundation。

Web Console 与交互式媒体不重排 P0–P9，也不成为 Kernel Foundation 的完成前置。它们在 P3 后形成独立 Operator & Web Interaction workstream：P6a 后可做 local read-only Console，P5+P6b 后增加 federated Inspection/OpsService；P4 与高带宽 payload ownership 后做 view-only WebRTC media，随后做 WebXR view-only；只有 P3/P6a 的模拟 Authority/Lease/Safety/Evidence 链通过后才做 simulated teleoperation，真实执行器始终等待具体设备 H1。详细依赖和 Harness 见 [专项研究](../research/web-console-webrtc-webxr-gateway-boundaries.md)。

## 22. 首批验证 Harness

在 Agent workflow、任何条件式 Graph Foundation 和真实硬件之前，建立独立并发与故障 Harness：

当前 S6 已以 local POSIX reference profile 覆盖 Thread/Process generation fencing、PXWP IPC credit/retained bytes、crash/partial frame/ignore cancellation/TERM/KILL、same-group child tree、workspace/stdio FD cleanup、`Uncertain`/no-replay、restart/quarantine 和独立 host watchdog 的子集。真实 OOM、SHM/socket、hostile escaped descendant、shared containment、durable journal、target-platform live A/B 与 exact-revision readiness 仍未满足，因此以下 Harness 不能整体勾选。

1. sync sleep、CPU spin 和 unknown native Card invocation 在启动/绑定前被拒绝进入 LoopDomain，或编译到更强隔离；不再承诺同一 loop 被永久阻塞时仍响应。
2. 2× stream/bulk 持续洪峰下，control enqueue-to-start 的 p99/p99.9 满足由最小业务 deadline 推导的目标，非安全等级仍获得声明的最小服务份额。
3. Fabric ingress buffer 与 target Mailbox 的 items、bytes 和 max age 均分别不越界；pre-validation frame 不计 Message accepted，offer outcome 与 admitted lifecycle 分层守恒，ingress/queued/inflight/executor/IPC credits/retained bytes 联合有界，无 detached work。
4. Signal 可按声明合并；Command 满或过期时明确拒绝并产生 Receipt，Transport/control/Safety 路径禁止 `block_until_deadline`。
5. ThreadDomain 永不返回或取消后迟到时，结果明确为 cancellation_requested/uncertain/wedged，旧 InvocationId/DomainEpoch 不能落地；wedged worker 不无预算补线程。
6. ProcessDomain 在 crash、ignore TERM、SIGKILL、OOM 和产生 grandchild 时仍可完成进程树/IPC/SHM/FD 清理，并按预算 restart 或 quarantine；在途 effect 为 Uncertain、默认不 replay。
7. RuntimeHost发现observed execution与current active `RuntimePlanSlice.execution`不一致时不能Ready。S7只核对exact Runtime build/fixture fingerprint、source revision、RuntimeHostEpoch/resource generation、一个reference LoopDomain/CardInstance以及profile固定的1 lifecycle slot和零mailbox/dispatch/background slots；没有PID/TID/Thread/Process或general capacity。未来profile再按其contract对账相应observed facts。
8. 启动、排空、取消和关闭竞态不遗留无 owner Task、Thread、Process、Mailbox、SHM 或 FD。
9. 资源竞争和执行 owner 重启后，旧 fencing token 仍不能产生副作用；不同 priority class 的 Stop/Enable 由 resource owner ordering 处理，不被调度重排破坏。
10. RuntimeHost stall 时，同故障域外的 NodeDaemon/OS watchdog 可以收集证据，由唯一恢复动作 owner 预算化恢复并阻止 restart storm。
11. 数据面洪峰不阻塞 control；硬件 safety island 的 E-Stop/protective stop/deadman/limit 不依赖普通 Rust async dispatcher、Python worker、Agent 或远端 Fabric，SafetyIslandAdapter 失效也不能解除 inhibit。
12. Rust RuntimeHost 与 Python reference worker 对同一 RuntimeApplyRequest/Message/Receipt fixture 具有 byte-identical canonical digest 和一致的 version/error 行为；worker 阻塞、crash、忽略 cancellation、产生 child 或返回旧 generation 结果时，RuntimeHost 仍可响应并清理 process tree/FD/IPC/SHM，旧结果不得落地。
13. Zenoh 自动重连保持 epoch；Session 对象重建后旧 `FabricSessionEpoch` callback 被隔离，Binding 重装后旧 `BindingEpoch` 消息被隔离；`session-local`、`host-local`、`remote` 与 P2 fixture 通过同一 conformance。
14. S7的`OneSourceLoop → EmptyDeactivate → OneSourceLoop`不混用新旧resource generation，旧revision被fencing且callback不replay；未来一般activate/drain/rollback同样不得混用Domain/Mailbox/Policy。
15. GPU/device 工作在 child crash 后不被伪报为已取消，经 completion/reset/fencing 后才允许成功或新 owner 接管。
16. Port bind 在 direction、interaction、cardinality 或 Schema/version 不兼容时 fail-fast；两个 CardInstance 不共享私有实现状态或 live binding。
17. source Message accepted、fabric egress、encoded frame staged、Message validated/target Mailbox admitted、Card invocation completion 与物理 effect 分阶段记录；任何层都不能把前一阶段成功冒充后一阶段成功。
18. route replacement 在任一观测点最多一条 active route 接收新 Message；完成或显式 rollback 后无双投、implicit fallback、hash/time-window echo 去重和旧 route 泄漏。
19. Safety trip 与迟到 normal command、Driver restart 竞争时，下游 gate 的 safe output 持续锁存到受权 reset/rearm；Power/Thermal/ODD fact 过期或换代后旧 Decision/Command 被拒绝。
20. H1 activation 安装后切断 Fabric，再撤销/过期 Receipt 或改变 readiness/ODD/revision；目标 Node 本地推进 HardwareActivationEpoch、disarm 并拒绝后续真实 Command。
21. ConsoleGateway 只能通过公开 InspectionProtocol/OpsProtocol fixtures 工作；TUI/CLI 只使用对应 typed clients。缓存 stale、watch gap、慢消费者和重启 resync 不会改写 desired/observed truth。
22. OpsService 对同一 ControlRequestId/digest 幂等、对同 ID/不同 digest 冲突；timeout/restart 后先 `Uncertain → query/reconcile` 而不重放。kill OpsService 不停止 DeploymentController reconcile、RuntimeHost 或本地 Safety，OpsReceipt 不冒充 owner Receipt。
23. WebRTC direct/TURN、packet loss/reorder、peer restart、Gateway kill 和 codec worker wedge 均有有界资源与可解释状态；关闭后无残留 PeerConnection、Task、Thread、Process、FD、socket、SHM 或 retained media payload。
24. browser auth session、WebRTC PeerEpoch 和 XR StreamEpoch 换代后，旧 pose/command/callback 被拒绝；不得复用 BindingEpoch、RuntimeHostEpoch 或 FabricSessionEpoch。
25. WebSocket 与 DataChannel 不能同时成为同一活动控制输入的 accepted route；切换必须显式推进 Gateway-owned stream generation，不能双投后按内容去重。
26. 即使 disconnect callback 的 neutral/stop packet 丢失，目标 Node 的 lease/deadman/fencing/Safety 仍在本地收敛；DataChannel ACK 不产生物理 `Succeeded`。
27. `MediaSample`/`EncodedVideoSample`、BlobRef/BufferRef、codec/peer queues 与 XR ingress 在媒体、日志和输入联合洪峰下保持 items/bytes/age/credit 有界，离散 control 不被反压。
28. Deck multigraph/ServiceDependency validators对parallel Link、SCC、自环/长环与随机输入顺序产生稳定结果；S7只允许zero-Port/Link/Requirement的single fixture Deck进入candidate，其他shape在任何Runtime副作用前稳定拒绝。
29. Rust/Python vectors证明`RuntimeBuildDescriptorV1`、singleton `RuntimeArtifactCompatibilityManifestV1`、`RuntimeBuildIdentityV1`、PXTE v4/PXAR v5 exact manifest projection、`ReferenceAssemblyProfileV1`、zero/one `ReferenceLoopDomainSpecV1`+`ReferenceLoopSubjectSpecV1`与zero-binding PXTA canonical；旧capacity-bearing type不能被alias，Runtime executable digest与fixture artifact digest不能互换。
30. malformed/duplicate/unknown branch、任何Binding/Ingress/Thread/Process/general capacity、nonempty per-use config、manifest/build/fixture mismatch和第三shape都由Planner在candidate/commit前拒绝，Runtime再做zero-side-effect defense-in-depth；missing/zero/wrong-width/wrong-store `RuntimeApplyEnvelopeV2.expected_runtime_store_instance_id`在任何mutation前拒绝。release pipeline唯一产生descriptor；registered system install operation严格消费descriptor/artifact、一次唯一生成singleton manifest并byte-identically交Runtime initializer与operator/Controller/Planner immutable ingress，prebuilt或Planner重建manifest都拒绝，bootstrap只校验。startup验证sequence-1 exact canonical descriptor/manifest bytes+digests并证明binary-derived compiled actual与store-pinned identity逐字段一致，不重新hash executable或读取side file/config。
31. RuntimeHost在不安装/import`decks`与`deployment`时，仅从canonical Slice完成fixed idle fixture的bounded start/restart reassembly，以及live/nonzero empty head-first drain或already-exact-zero fast path；fixture返回后无task/tick/output，RuntimeAssemblyEngine不持久化editable graph或进入steady dispatcher。
32. tenure-only/full-admission、normal `PreparedNoEffects → FirstActionIntent`、recovery `RecoveryPlannedNoEffects → StartCallIntent`、desired/live/action/resource分型、post-intent supersede、OneSource head commit、conditional empty head-first retire/zero fast path、restart/timeout/query/replay的fault matrix证明callback不replay、empty head不丢失、unknown ownership quarantine，且不出现第二side-effect action或双live generation。intent/head构造前与durable publish后/effect前都检查deadline；callback/deadline/cancel raw fact在cleanup前先写`RawActionOutcomeLatch`，无quarantine/supersede/crash时cleanup+exact-zero后才以一次`terminal_selection_observed_at`采样形成`TerminalOutcomeSelection`，`now == deadline`为timeout，selection后fsync/回复跨deadline不重分类且raw fact保留。
33. 若抽取 Graph Foundation，architecture/property tests 证明其无领域 import、I/O、loader、serialization/digest、execution state、retry/checkpoint/Receipt，并至少被两个独立生产消费者直接使用。

第一个端到端闭环为：

```text
Simulated Driver
      │ Signal
      ▼
Processor CardInstance
      │ Signal
      ▼
Controller-role CardInstance
      │ Command
      ▼
CommandEndpoint → Authority → Resource Lease/Fencing → SafetyIslandAdapter
      │ admitted normal command
      ▼
Driver EnforcementPoint → downstream safety gate → Simulated Actuator
      │ Receipt
      ▼
owner facts / Receipt
      ├──> InspectionService ──> InspectionClient ──> TUI read model
      └──> Evidence refs <────── OpsService <─────── TUI ControlRequest
```

## 23. 文档工程

本文件是架构基线，不代替后续 ADR。下面这些决策在接口稳定前应分别形成 ADR：

- Rust-first production mechanisms、多语言 ProcessDomain、language-neutral wire contract 与 Cargo/uv 权威边界已经由 [ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 接受；PXTE v3/PXAR v4/PXWP v1 已在 S6 锁定并有 Rust/Python conformance，C++、production IPC transport/platform profile、进一步 crate 切分和作者 API 仍按后续切片验证。
- Kernel 依赖红线与允许的基础依赖。
- Core Service 生命周期和 ServiceSpec Schema。
- CardDefinition、Artifact export/entrypoint ref、CardInstance 私有实现对象、In/Out、PortSpec、Deck Link、`DeploymentPlan.bindings` 与 live PortBinding 的所有权和兼容规则。
- Graph Foundation 准入门、DeckTopology/ServiceDependency/Runtime assembly/Agent workflow owner、DataLink 与 activation dependency 分型，以及 P2e cyclic Deck 保守策略已由[ADR-0005](../adr/ADR-0005-typed-domain-graphs-and-runtime-assembly-boundary.md)接受；相应实现仍按真实消费者分阶段落下。
- P2e三个owner snapshot journal、initializer、tenure-only/full-admission、normal/recovery intent、desired/live/recovery/resource/outcome分型、conditional empty head-first retire/zero fast path与authenticated query/reassembly由已接受的[ADR-0007](../adr/ADR-0007-p2e-reference-journal-and-crash-recovery.md)约束；PXTE v4/PXAR v5只允许fixed manifest/profile、zero/one reference Loop records与zero-binding PXTA，由已接受的[ADR-0008](../adr/ADR-0008-pxte-v4-pxar-v5-subject-ingress-separation.md)约束。两者均处于分阶段实现中，不能视为完整能力。
- ExecutionRequirements、DeliveryProfile、DeploymentPlan.execution 与 desired/observed 一致性。
- Workload envelope、run-bound provenance、OutstandingBudget、payload ownership、deadline/recovery 与 revision transition。
- 本地 Execution Domain、Dispatcher、ExecutorBudget、RuntimeOwnershipTree、LivenessSpec/State、FailureContainmentSpec、RecoveryPolicy/Engine、跨 Node service client 与 Placement 所有权规则。
- Fabric 消息与交付语义。
- Zenoh-native Fabric、keyspace/版本治理与 ROS2/DDS Gateway 边界。
- Node/RuntimeHost 身份、各层 epoch/revision、网络分区与 reconciliation。
- Authority、Resource Lease 和物理写路径。
- InspectionProtocol、OpsProtocol、OpsService operation journal、ConsoleGateway 与真实 owner 的权限/Receipt 边界（见 ADR-0003）。
- Web Gateway managed-workload、external exposure/internal typed endpoint、ConsoleGateway/OpsService、RuntimeHost 与 Zenoh Fabric 的所有权和禁止依赖。
- browser auth session、WebRTC peer、XR input stream、未来 teleoperation session 的限定 identity/epoch、Principal 映射、reconnect fencing 与 single-active external route。

实现出现后，文档必须链接实际代码与测试。架构图描述目标结构，Current 文档描述真实状态，两者不能混写。

## 24. 开放问题

以下问题尚未冻结：

- Authority 与 ResourceCoordinator 首版是否同进程；二者逻辑接口和状态保持分离。
- 只有目标硬件上的 p99.9 延迟、CPU 与 copy profile 证明 Zenoh `session-local` 不满足已声明 SLO 时，是否为部分非安全 binding 引入互斥的同进程 route；P2 只有确定性 PortBinding test fixture，不预设生产 LocalBus。
- typed service client 与 permission-bound access handle 采用生成式 client，还是显式构造函数注入。
- 已冻结静态 1:1 `Command`/`OperationClient` 之外，`Call`、`Query`、复杂 `Operation` 和 `Tool` 如何映射到一个或多个物理 PortBinding；不能退化成无关联的 In/Out 消息流，也不能重新开放 Controller-role Card→Driver 直连。
- fan-in/fan-out 的 cardinality、routing、partial acceptance、merge owner 与 per-destination/aggregate result 语义。
- 首个允许 cyclic Deck 的 feedback/delay/seed/latest-value/non-blocking backpressure contract，以及其 liveness、初始条件和 shutdown 证明；在此之前生产 admission fail-fast。
- `RuntimeAssemblyEngine` 最终内部名称与 API shape；其 Slice-only 输入、无第二 desired graph、RuntimeHost lifecycle owner 和不进入 steady hot path 的边界不是开放问题。
- Schema registry、兼容窗口和显式 Adapter/Converter 的解析与锁定方式；禁止运行时隐式猜测转换。
- NodeDaemon 与 RuntimeHost 在 development profile 是否共进程，以及一个 Node 是否允许多个 RuntimeHost；production watchdog 必须保持不同故障域，且必须冻结 NodeDaemon/OS service manager 中唯一的 host restart-budget/quarantine owner。
- S4 已有 reference hierarchical deficit/direct fast path、signed class weight/max burst 与 conditional Control bound；最终目标平台 dispatch 参数、live A/B、Thread/Process budget 和 CPU affinity仍由目标 benchmark 决定。
- WorkloadEnvelope/RunBound/OutstandingBudget/RecoveryPolicy 的具体 Schema 字段名和版本策略；其所有权、unknown 保守准入、全链路有界性和默认不 replay 语义不是开放问题。
- S6 已验证 POSIX exec/process-group 与 Linux `/proc` census reference profile；生产目标仍需决定 cgroup/job-object/pidfd、跨平台等价 cleanup/resource profile，以及 Python worker 是否需要经过验证的 spawn/forkserver adapter。禁止依赖平台隐式默认或在线程启动后 fork。
- 何种可证明的 worst-case deadline 足以引入 native RealtimeDomain；普通低延迟不自动升级为硬实时。
- 引入稳定 SiteRef 的真实消费者、owner 与映射格式；当前仅有非权威 site_hint。
- fencing/idempotency ledger 的持久化实现与各类资源的分区行为。
- 安全关键 Driver 默认独立进程还是由 DeploymentProfile 决定。
- Zenoh keyspace、Zenoh Region 配置、schema registry、版本窗口和节点发现的具体参数。
- 存量 ROS2/DDS bridge DeploymentProfile 与受控 `rmw_zenoh` DeploymentProfile 的目标平台支持矩阵。
- P4/P8/硬件里程碑所需的 Rust toolchain/target triple/libc/CPU feature、Python ABI、ROS2、Ubuntu、Jetson、GPU 与设备 SDK 完整支持矩阵；P0 只冻结最小 Rust core 和 Python 治理/SDK 开发 CI 基线。
- Gateway 是复用 Runtime 中性 managed-instance envelope、使用 ServiceInstance 外壳还是需要新的限定 contract；在裁决前不新增万能 `GatewayInstance`。
- Card Port 与非 Card Gateway endpoint 通过 Deployment-owned exposure/binding、窄 ServiceContract 还是限定 Gateway endpoint contract 连接；Deck 不直接选择 WebRTC、TURN 或 external route。
- `WebRealtimeGateway` 候选组合名是否拆为 MediaGateway 与 XRInputGateway；无论命名如何都不表示硬实时或 RealtimeDomain。
- custom JSEP、WHIP/WHEP、P2P/SFU、encoder placement、TURN topology/credential issuer、browser identity provider 与 media recording owner 的目标 profile 和证据门槛。
- DataChannel 与 WebSocket 在目标 XR 浏览器上的 latency、loss、background throttling 和功耗；同一活动输入 single-active 与本地 deadman 不是开放问题。
- `MediaSample`/`EncodedVideoSample` 的 BlobRef/BufferRef、Zenoh SHM、codec negotiation、zero-copy 与 retained payload lifecycle。

这些开放问题不阻塞 P0 开始。P0 完成要求 Cargo/uv 各自锁定的最小开发 CI 基线和跨语言 wire fixture；完整目标平台矩阵分别在 P4、P8 或首个真实硬件里程碑前冻结。P2 不等待未来同进程 route 研究。实现必须用最小垂直闭环收集其余证据。

## 25. 完成判据

这份架构基线只有在以下条件满足后才能从 Draft 升为 Current：

- Kernel 和 Runtime 已形成实际包边界，依赖检查可以自动验证。
- Rust core 可以通过 pinned toolchain + `Cargo.lock` 重建，Python 治理/SDK 环境可以通过 `uv sync --locked` 重建；Kernel 最小构建不依赖 Python、Zenoh、ROS2、模型或硬件 SDK。
- Rust/Python 对公共 wire fixture 的 canonical bytes、digest、unknown-field、version 和 error behavior 一致，Python worker crash/kill/late result 不会产生第二 Runtime owner或资源泄漏。
- 至少两个 Core Service 能按依赖图启动、就绪、排空和停止。
- Deck multigraph 的 parallel Link/SCC/cycle witness 与 ServiceDependency DAG 验证确定；DataLink 不冒充启动依赖，无 feedback contract 的 cyclic Deck 在副作用前失败。
- P2e RuntimeAssemblyEngine只靠canonical Slice完成manifest-pinned idle Loop的bounded normal start/restart reassembly，以及canonical empty的live/nonzero head-first retire或exact-zero fast path；`RuntimeApplyEnvelopeV2.expected_runtime_store_instance_id`在任何mutation前绑定exact journal store，sequence-1持久exact canonical descriptor/manifest bytes+digests，startup只验证snapshot与binary-derived compiled actual/store-pinned identity。normal apply以`FirstActionIntent`、recovery以`RecoveryPlannedNoEffects → StartCallIntent`分开越过effect boundary；intent/head构造前与publish后/effect前都检查deadline。raw outcome在cleanup前先写`RawActionOutcomeLatch`，无更高优先级terminal时cleanup+exact-zero后才以一次owner-clock采样形成`TerminalOutcomeSelection`，`now >= deadline`为timeout，selection后fsync/回复不重分类。invalid snapshot不提供authenticated bootstrap/query，validated startup后的quarantine才可携带exact identity返回`Indeterminate`。crash不replay callback、不丢empty desired head、不产生第二action或双live generation。一般prepare/readiness/Ingress/egress/Thread/Process/replacement/rollback、双active Binding与steady Message path需要后继assembly successor的独立完成证据，不能由S7外推。
- 至少一个 ProcessDomain 故障可以被隔离和解释。
- 联合过载下 Fabric ingress buffer、Mailbox、inflight、executor/IPC credits 和 retained payload bytes 均不越计划，不存在 detached work 或隐藏 backlog。
- thread wedge 不无预算增员；process/device crash 不伪造 effect Receipt、不默认 replay，device reset/fencing 和 revision 替换可验证。
- 最小 Signal → Command → Receipt 闭环通过系统测试。
- 同一闭环在 Zenoh `session-local`、`host-local` 与 `remote` route 下通过，并能证明断连后本地安全行为、stale/连接观测恢复、single-active route，以及发生 Session 重建时的 epoch 隔离。
- TUI 能仅通过 InspectionProtocol 解释一次成功与一次失败事务，并只通过 OpsProtocol 发起 ControlRequest；OpsService 的 terminal Receipt 可追溯到真实 owner Receipt/EvidenceRef。
- 文档中的主要接口均链接到实现与测试证据。

在此之前，本文只表示 ParaEGOX 当前选择的建设方向。
