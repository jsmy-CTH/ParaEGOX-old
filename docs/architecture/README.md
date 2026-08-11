# Architecture

本目录描述 ParaEGOX 当前或目标系统结构，包括组件所有权、依赖方向、关键数据流、控制流和故障边界。

## 当前文档

- [ADR-0001：DeploymentController、DeploymentPlan 与 Runtime 边界](../adr/ADR-0001-deployment-controller-boundary.md) — 已接受的 control-plane/Kernel/Runtime 所有权裁决。
- [ADR-0002：CardDefinition、Card 与 CardInstance 术语边界](../adr/ADR-0002-card-definition-terminology.md) — 已接受的可复用定义、Deck 配置使用、运行身份与私有实现边界裁决。
- [ADR-0003：OPS、OpsService 与 Inspection 操作边界](../adr/ADR-0003-ops-service-operation-boundary.md) — Proposed；运维领域、ControlRequest owner、只读投影、ConsoleGateway 与真实 action owner 的边界裁决。
- [ADR-0004：Deck 工作负载、DeckLock 与 Application 准入边界](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md) — Accepted；Deck executable workload、DeckLock 单一解析真相、Canvas/Planner 分界与条件式 Application 准入门。
- [ADR-0005：Typed Domain Graph、Graph Foundation 与 Runtime Assembly 边界](../adr/ADR-0005-typed-domain-graphs-and-runtime-assembly-boundary.md) — Accepted；领域图分型、Graph Foundation 抽取门与 Slice/journal-only Runtime assembly 边界。
- [ADR-0006：Rust-first 核心机制与多语言工作负载边界](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) — Accepted；Rust-first production mechanisms、language-neutral contracts、ProcessDomain worker 和 Cargo/uv 工具链边界。
- [ADR-0007：P2e reference journal 与 crash recovery 基线](../adr/ADR-0007-p2e-reference-journal-and-crash-recovery.md) — Accepted；三个 owner 的独立 journal、crash recovery 和 target-platform evidence 边界。
- [ADR-0008：PXTE v4 / PXAR v5 reference target successor](../adr/ADR-0008-pxte-v4-pxar-v5-subject-ingress-separation.md) — Accepted；exact build/store-bound、zero-binding reference assembly successor。
- [Kernel、RuntimeHost 与 Core Services 架构基线](kernel-runtime-core-services.md) — 当前 clean-slate 架构的总入口，状态为 Draft。
- [ParaEGOX 分布式系统模型](distributed-system-model.md) — distributed-first contract、local-first execution、网络分区、一致性和恢复模型，状态为 Draft。
- [分布式具身 Agent OS 缺口研究与演进计划](../research/distributed-embodied-agent-os-gap-analysis.md) — 对 Foundation 跨层合同、具身物理面、Agent 执行面、安全供应链与验证路线的深度缺口审查。
- [Capability、Service Contract 与 Feature Support 边界](../concepts/capability-service-feature-boundaries.md) — 授权、服务接口和 observed platform support 的独立 owner 与失效语义。
- [分布式身份、作用域与所有权研究](../research/distributed-identity-scope-and-ownership.md) — Node、Site/Zenoh/空间作用域、远端所有权与物理控制链的深度研究。
- [Node 与作用域边界](../concepts/node-and-scope-boundaries.md) — 当前采用、后置和禁止的限定术语。
- [CardDefinition 输入输出、Port、Link 与运行绑定研究](../research/card-definition-ports-links-and-bindings.md) — CardDefinition 声明模型、CardInstance 私有实现、Schema/interaction/cardinality 和从 Deck Link 到 live PortBinding 的所有权链。
- [Application、Deck、Card 与 Service 边界研究](../research/application-deck-card-service-boundaries.md) — Product/Application 与 executable Deck、DeckLock/DeckTopology、Card、CoreService、未来安装及应用私有持久状态的深度边界研究。
- [Runtime 执行模型、调度与恢复研究](../research/execution-model-scheduling-and-recovery.md) — Lane 裁决、CardDefinition/Card/Deck 到 DeploymentPlan 的执行编译、Mailbox、Loop/Thread/Process、预算、判活、恢复和故障 Harness。
- [Graph Foundation、领域图与执行边界研究](../research/graph-foundation-and-domain-execution-boundaries.md) — DeckTopology、ServiceDependency、Deployment、Runtime assembly、Agent workflow 与其他图领域的 owner 分离，Graph Foundation 准入门及单 Node Deck 运行闭环。
- [平台兼容 Port、host-platform support evidence 与 OS Backend 边界](../research/platform-compatibility-ports-and-host-feature-profiles.md) — Research Complete；S7-E executable evidence 前置已由 `1ed704c` / CI `30748840399` 满足，但 PCA 尚未准入，PC0–PC3 仍是候选；当前不是统一平台层或 macOS/Windows production support。
- [Kernel 消息、Zenoh-native Fabric 与 ROS2/DDS 边界研究](../research/kernel-messaging-fabric-evidence-security.md) — Message/messaging/Mailbox/PortBinding 术语、pre-validation Fabric ingress、Zenoh-only production route、single-active-route、证据、方案比较和失效条件。
- [Web Console、WebRTC、WebXR 与交互式 Gateway 边界研究](../research/web-console-webrtc-webxr-gateway-boundaries.md) — ConsoleGateway、WebRTC/WebXR 外部腿、媒体/XR/OPS 路径、browser/peer/stream 分代、Gateway managed-workload/exposure 缺口、断线安全与分期验证。
- [Kernel Foundation 实施计划](../plans/kernel-foundation.md) — 按依赖排序的落地与验证路径。

## 内容边界

Architecture 文档回答：

- 系统由哪些层和组件组成。
- 每个组件拥有什么、不拥有什么。
- 数据、Command、Receipt 和生命周期如何跨边界流动。
- 哪些结构是目标设计，哪些已经有实现证据。

Architecture 文档不代替：

- [`adr/`](../adr/README.md) 中的正式长期决策。
- [`plans/`](../plans/README.md) 中的实施顺序。
- [`reference/`](../reference/README.md) 中与版本绑定的接口事实。

## 写作要求

每篇文档必须标注状态和日期，包含范围、非目标、依赖方向、关键失败场景、实施影响和开放问题。描述目标状态时使用 Draft；只有链接到真实实现和验证后才能标记 Current。
