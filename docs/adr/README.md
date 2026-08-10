# Architecture Decision Records

本目录记录影响多个组件、长期存在或难以回滚的架构决策。ADR 冻结“以后必须遵守什么”，不负责跟踪每日进度。

## 状态

- `Proposed`：方案明确，等待评审或实施授权。
- `Accepted`：项目正式采用。
- `Rejected`：已评估但不采用，保留原因。
- `Superseded`：已被后续 ADR 替代，必须链接后继文档。

## 命名

```text
ADR-0001-short-decision-name.md
ADR-0002-another-decision.md
```

编号一经使用不重排、不复用。第一份正式 ADR 从 `ADR-0001` 开始。

## 何时需要 ADR

- 修改 Kernel 或 Runtime 的所有权边界。
- 引入公共协议、持久格式或权限模型。
- 选择难以替换的基础依赖。
- 改变物理安全、Command、Resource Lease 或执行语义。
- 改变 ServiceDependencyGraph、DeckTopology 或 Deployment 的长期模型。
- 引入正式 Product Application/Installation identity、application-owned durable service 或跨 Deck release/GC ownership。

小型、局部、容易回滚的实现选择不需要 ADR。

创建新记录时从 [ADR 模板](ADR-template.md) 开始。

## 记录

- [ADR-0001 — DeploymentController、DeploymentPlan 与 Runtime 边界](ADR-0001-deployment-controller-boundary.md) — Accepted；保留 DeploymentController 作为 Kernel 外的分布式 desired-state owner，分开 candidate/committed plan、tenure-neutral Slice、writer proof 和 Runtime apply journal，复杂 HA 与共识后置。
- [ADR-0002 — CardDefinition、Card 与 CardInstance 术语边界](ADR-0002-card-definition-terminology.md) — Accepted；保留可复用定义层但移除旧公共领域名，冻结 `CardDefinition → Card → CardInstance`、`uses` 引用和非万能实现对象边界。
- [ADR-0003 — OPS、OpsService 与 Inspection 操作边界](ADR-0003-ops-service-operation-boundary.md) — Proposed；OPS 是运维领域标签，OpsService 只拥有 ControlRequest journal，InspectionService 只拥有只读投影，ConsoleGateway/clients 与真实 action owner 保持分离。
- [ADR-0004 — Deck 工作负载、DeckLock 与 Application 准入边界](ADR-0004-deck-workload-and-application-admission-boundary.md) — Accepted；固定 Deck 的 executable workload 语义、拒绝 `Bundle = Deck` 伪别名、建立 DeckLock 单一解析真相、Canvas/Planner 边界和条件式 Application Admission Gate。
- [ADR-0005 — Typed Domain Graph、Graph Foundation 与 Runtime Assembly 边界](ADR-0005-typed-domain-graphs-and-runtime-assembly-boundary.md) — Accepted；将 DataLink、ServiceDependency 与 activation constraint 分型，cyclic Deck 首版 fail-closed，仅在两个真实生产消费者证明交集后抽取 internal Graph Foundation，并限制 RuntimeAssemblyEngine 为 Slice/journal-only Runtime mechanism。
- [ADR-0006 — Rust-first 核心机制与多语言工作负载边界](ADR-0006-rust-first-core-and-polyglot-workloads.md) — Accepted；生产核心机制优先 Rust，Python/C++/模型与设备生态经版本化 ProcessDomain 或独立 Service/Gateway 接入，Cargo 与 uv 各自拥有唯一工具链权威范围。
- [ADR-0007 — P2e reference journal 与 crash recovery 基线](ADR-0007-p2e-reference-journal-and-crash-recovery.md) — Accepted；三个 owner 分别使用有界、版本化、严格 quarantine 的 local atomic snapshot journal，冻结 exact store/build binding、admission/fencing/generation、desired/live/action/resource分型、intent/outcome latch、conditional empty retire、restart/query与crash规则，并明确 reference profile 不具备完整旧快照 anti-rollback 能力。
- [ADR-0008 — PXTE v4 / PXAR v5 source-only/empty reference target successor](ADR-0008-pxte-v4-pxar-v5-subject-ingress-separation.md) — Accepted；以`RuntimeBuildDescriptorV1`、singleton manifest、`RuntimeApplyEnvelopeV2`和additive narrow PXTE/PXAR successor只表达exact build/store-bound、zero-binding的`OneSourceLoop`与`EmptyDeactivate`，不把Thread/Process/Ingress/general-capacity placeholder写入v4 public wire，同时保持全部旧版本 bytes/digest/reason 行为不变。
- [ADR-0009 — Agent 对话、类型化客户端与 Console 边界](ADR-0009-agent-conversation-and-client-boundary.md) — Accepted；首个用户可见切片采用 Runtime-managed AgentService、独立 Model provider、AgentConversationProtocol/Client 与 FabricService-owned Zenoh typed binding，本地 TUI 直接使用类型化客户端，未来 Web 经 ConsoleGateway，旧全能 ConsoleBridge 不再作为目标组件。
- [ADR-0010 — Remote Agent ingress proxy 与单一 Fabric Session 边界](ADR-0010-remote-agent-ingress-proxy-boundary.md) — Accepted；保留 `S0` 为唯一 production Fabric/application bus/Agent PortBinding Session，只允许一个 Runtime-governed、TLS-only、非 Fabric 的 `S1` exact-two-route ingress proxy，经 generation/epoch/PXAP-fenced 窄 capability 转发到 `S0`，并以 fence→drain→close 与 `Drained`/`OutcomeUncertain` 分型退役。

当前不创建 Application 类型；ADR-0004 只冻结 Deck 与 Application 的准入边界，不代表 Application/Installation 已获准实施。

实现注记（2026-08-03）：S7-B internal successor foundation 已由提交 `fb1547d`、Linux test-fixture follow-up `f2c6593` 和 CI `30617157810` 完成；S7-C internal DeckCompiler/Planner 已由提交 `54ebed8` 与 CI `30622634625` 完成；S7-D real TenureAuthority process/initializer/store、authenticated IPC server以及Controller/Runtime internal journal model已由提交`1207f4e`、Linux evidence follow-up `47da71e`、service-account teardown follow-up `08389f3`和CI `30729690319`完成。S7-E W1由`600838d`、`cc15fc8`、`399970c`和CI `30733163162`完成；S7-E executable vertical 又由主提交 `14d0012` 及 follow-up 至 `1ed704c`、Ubuntu CI `30748840399` 完成，形成 descriptor/install/Runtime initializer、authenticated PXBR/PXAR endpoint、fixed Loop/Empty owner与 `paraegox-deploymentd` one-shot commit/tenure/bootstrap/apply/Empty真实路径。S7-F 后续已提交 authenticated Runtime query、Controller owner-private query journal/client，以及 Runtime payload v5、lossless v4→v5 migration 与 fixed Loop/Empty restart reassembly；Runtime 最新基线 `f32700f` 已由 Ubuntu CI `30782979187` 验证。当前未提交工作树已有 `reconcile-reference-once-v1`/bounded one-shot reconcile 和 SF6 Linux scenarios，但尚无 fresh Ubuntu CI，因此 SF5/SF6 与 S7 均不标 complete。已验证证据仍只覆盖 exact Linux ext4 reference profile；macOS 对 SF6 只 skip，APFS 在 PC1 提供 FD-anchored extended-ACL 与 crash-durability 证据前 fail closed，Windows仍unsupported。平台兼容仍是 [Research/PCA 前候选](../research/platform-compatibility-ports-and-host-feature-profiles.md)，没有 Accepted platform ADR，也未准入 PC0–PC3。
