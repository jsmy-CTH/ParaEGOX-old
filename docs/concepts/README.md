# Concepts

本目录维护 ParaEGOX 的稳定术语和系统心智模型，防止同一概念在 Kernel、Runtime、Services、Agent、Graph 和 OPS 中被重复命名。

## 计划内容

- [CardDefinition、Card 与 Deck](card-definition-card-deck.md)——可复用能力、Card 最小权威内容、executable workload 组合、Deck/Bundle 命名边界、依赖锁定、执行要求编译与运行身份，状态为 Draft。
- [Node 与作用域边界](node-and-scope-boundaries.md)——计算身份、运行宿主、部署、Fabric、空间与物理控制作用域，状态为 Draft。
- [Capability、Service Contract 与 Feature Support 边界](capability-service-feature-boundaries.md)——授权、服务接口与 Node/Fabric/Device 支持事实的三义拆分，状态为 Draft。
- [Application、Deck、Card 与 Service 边界研究](../research/application-deck-card-service-boundaries.md)——产品应用、executable Deck、Card、平台 CoreService 与未来应用私有持久服务的准入边界，状态为 Research Complete。
- [ADR-0003：OPS、OpsService 与 Inspection 操作边界](../adr/ADR-0003-ops-service-operation-boundary.md)——运维领域、ControlRequest、只读 projection、clients/Gateway 与真实 owner 的限定术语，状态为 Proposed。
- [ADR-0004：Deck 工作负载、DeckLock 与 Application 准入边界](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md)——Deck/DeckLock/Application 长期边界的提案，状态为 Proposed。
- Kernel、RuntimeHost、Core Service、CardInstance 与 Driver。
- Message、messaging、Mailbox、In/Out、PortSpec、Link、DeploymentPlan.bindings、PortBinding、DeckTopology、Deployment、Artifact、CardProfile 与 DeploymentProfile。
- Signal、Event、Command、Query 与 Receipt。
- Authority、CapabilityGrant、ServiceContract、FeatureReport、Resource、Lease 与 Constraint。
- Inspection/InspectionService、OPS/OpsService/OpsClient、Readiness、Health、Degraded 与 Fault Domain。

## 内容边界

Concept 文档回答“这个词在 ParaEGOX 中是什么意思，以及不是什么意思”，不描述具体类名、配置字段或实施排期。

新增核心术语前必须检查现有词汇。ParaEGOX 已接受的规范链是 `CardDefinition → Card → CardInstance`：CardDefinition 是带 Artifact export/entrypoint 引用的不可变合同，不是业务基类；Card 是 Deck 内一次具名、配置使用，连接由 Deck Link 引用 Card Port；CardInstance 是运行身份并托管私有实现对象，这部分由 [ADR-0002](../adr/ADR-0002-card-definition-terminology.md) 冻结。`Card` 延续 Motus Canvas 的配置使用心智模型；`Deck` 作为 executable workload、canonical `DeckTopology` 内嵌 DeckLock，以及当前不创建无 owner Application 的方向由 [ADR-0004](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md) 提议，尚未 Accepted。Motus `PerceptionBundle` 的 endpoint 聚合语义不迁入。Port、Link、期望 binding 与 live binding 不得合并成万能通信对象。`Message` 是验证后的不可变逻辑契约值，`messaging` 是子系统名，`Mailbox` 是目标异步边界唯一、只容纳 Message 的 backlog/admission owner，`PortBinding` 是唯一公共 live binding；pre-validation encoded frame 只进入 Fabric 私有有界 ingress buffer，生产 route 统一由 Zenoh 承载，测试 fixture 不另建公共领域名。`CapabilityGrant` 只表示安全授权，服务依赖使用 `ServiceContract`，目标支持事实使用限定 `FeatureReport`；三者不能共享一个 Capability 基类。

术语冻结后，相关架构文档、ADR、代码和 Reference 必须使用相同含义。
