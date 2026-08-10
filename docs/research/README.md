# Research

本目录保存会改变架构或实施选择的研究结果，包括仓库证据、外部一手资料、参考实现分析、方案比较和未解决问题。

## 每篇研究至少包含

1. 明确的问题和成功标准。
2. 范围、假设与非目标。
3. 证据来源及强度。
4. 当前行为或参考路径的重建。
5. 可行方案和真实权衡。
6. 风险、反例和开放问题。
7. 推荐方向，以及需要 ADR 还是可以直接实施。

研究结论不是架构决策。需要长期冻结的结论应进入 [`adr/`](../adr/README.md)，已经确定的实施顺序应进入 [`plans/`](../plans/README.md)。

## 当前研究

- [平台兼容 Port、host-platform support evidence 与 OS Backend 边界](platform-compatibility-ports-and-host-feature-profiles.md) — 状态为 Research Complete；结论为 `proceed`（进入后续 admission，未授权实现），采用窄 owner port + exact support evidence + OS backend，拒绝最低公分母、巨型 `Platform` trait、静默降级与 generic journal；PC-G 已由 S7-E 落实，S7-E executable evidence 也已由 `1ed704c` / CI `30748840399` 满足，但 PCA 尚未准入，PC0–PC3 仍是候选工作池且不会自动抢占 S7-F/P3。
- [Agent OS 参考特性采纳矩阵](agent-os-reference-feature-adoption.md) — 状态为 Research Complete；结论为 `revise`，将公开 Agent OS/具身 Runtime 的可借鉴内容按特性而非项目归并到 ParaEGOX owner 与 P0–P9 gate，覆盖 invariant、effect/authority、bounded Runtime、Mailbox/Fabric、恢复、physical ABI、Agent/Memory/Ops 后续能力及明确反例，不引入上游整套架构或名词。
- [Application、Deck、Card 与 Service 边界](application-deck-card-service-boundaries.md) — 状态为 Research Complete；结论为 `revise`，建议将 Deck 收窄为 executable workload、拒绝 `Bundle = Deck` 伪别名、将 `DeckTopology` 内嵌并纳入 DeckLock digest，当前不新增 Application 家族，并定义 A0 admission gate；长期边界已提交为 Proposed [ADR-0004](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md)。
- [分布式具身 Agent OS 缺口研究与演进计划](distributed-embodied-agent-os-gap-analysis.md) — 状态为 Research Complete；结论为 `revise`，系统梳理 VFS/Card/Deck/Capability、RuntimePlanSlice、跨 concern owner map、可信 Observation/Command ABI、安全岛与断网自治、Agent execution、security/supply-chain、P6a/P6b Evidence/OPS 与 H1 SIL/HIL 路线。
- [CardDefinition 输入输出、Port、Link 与运行绑定](card-definition-ports-links-and-bindings.md) — 状态为 Research Complete；结论为 `revise`，建议采用不可变 CardDefinition 和 In/Out 作者语法，严格拆分定义、私有实现、Port、Link、DeploymentPlan.bindings、live PortBinding 与 target Mailbox，并冻结 single-active-route；命名结论已由 ADR-0002 接受。
- [Card 独立开发、测试 Harness 与运行探测边界](card-independent-development-testing-and-probe-boundaries.md) — 状态为 Research Complete；结论为 `revise`，建议采用普通实现对象单测、分阶段 internal canonical Slice 单主体 Harness 与 one-subject Deck 正式链路三层入口，拒绝 `Card.run()`/StandaloneRunner/万能 Probe，并分开 startup、liveness、readiness、health、测试观察与独立 L4/H1 物理诊断 Operation。
- [Runtime 执行模型、调度与恢复](execution-model-scheduling-and-recovery.md) — 状态为 Research Complete；结论为 `revise`，已完成 Dual independent review 与 EAGOS failure-path audit；其执行语言结论已由 [ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 更新为 Rust-first mechanisms、language-neutral contracts 与 polyglot ProcessDomain workloads。
- [Graph Foundation、领域图与执行边界](graph-foundation-and-domain-execution-boundaries.md) — 状态为 Research Complete；结论为 `revise`，拒绝 Kernel 通用 Graph Engine，采用领域权威模型/执行器，并仅在两个独立真实消费者出现后条件抽取纯 Graph Foundation；同时补齐 RuntimeHost 内部 RuntimeAssemblyEngine 与 activation contract。
- [Tool 定义、Provider 绑定与调用边界](tool-definition-provider-binding-and-invocation.md) — 状态为 Research Complete；结论为 `proceed`（方向性），建议将 ToolDefinition 与 Card/Driver/CoreService 解耦，以 ToolProviderDeclaration、ToolAdmissionDecision、desired Deployment ToolBinding、Runtime readiness resolution、immutable ToolCatalogSnapshot、ToolView 和 InvocationAttempt 分离定义、实现、准入、期望绑定、运行事实、可见性与具体调用；首版只做静态单 active Provider，公共名称与 Schema 等待 ADR。
- [Kernel 消息机制、Zenoh-native Fabric、ROS2/DDS、Evidence、Telemetry 与 Security 边界](kernel-messaging-fabric-evidence-security.md) — 状态为 Research Complete；结论为 `revise`，明确 Message/messaging/Mailbox/PortBinding、pre-validation ingress、Zenoh 唯一生产 Fabric 与确定性 test fixture 边界；待 Proposed ADR 评审。
- [分布式身份、作用域与所有权](distributed-identity-scope-and-ownership.md) — 状态为 Research Complete；结论为 `revise`，已完成 Dual independent review。
- [Web Console、WebRTC、WebXR 与交互式 Gateway 边界](web-console-webrtc-webxr-gateway-boundaries.md) — 状态为 Research Complete；结论为 `revise`，明确 Console/WebXR/WebRTC/Zenoh 分类、Console 与媒体/XR/遥操作路径、断线安全与阶段计划，同时保留 managed Gateway/exposure/typed endpoint 合同为 Proposed ADR 前置。

针对 EAGOS 的分析只能抽取中立的工程经验与失败模式，不在本目录复制其代码、测试、配置或文档正文。

研究文档中的 Python、asyncio、multiprocessing 或既有实现描述可以作为历史证据与特定 worker profile 继续存在；若它们与当前实现基线冲突，以 Accepted [ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 的 Rust-first/polyglot 边界为准。
