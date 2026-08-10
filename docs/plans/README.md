# Plans

本目录保存已经确定方向的实施计划与跨阶段 Program。Plan 回答“按照什么依赖顺序落地，以及如何证明完成”，Program 还负责维护长期授权范围、冻结项、Step DAG 和当前状态；二者都不能重新讨论或覆盖已经由 Accepted ADR 冻结的方向。

## 每份计划至少包含

- 目标结果与明确非目标。
- 前置 ADR、Research 和现有实现证据。
- 按依赖排序的任务。
- 每项任务的生产者、消费者和首次验证入口。
- 单元、集成、系统和人工验证边界。
- 上线、降级和回滚方式。
- 可由评审者复核的完成证据。

计划不得仅按目录或文件数量拆分，也不得把“创建接口”“添加配置”本身当作用户可见交付。

## 当前计划

- [Local Operator CLI/Ops Program](local-operator-cli-ops-program.md) — 当前状态为 **Active**。它把用户可部署、可检查、可进入 TUI 的本地 operator golden path 提升为当前交付优先级，先交付严格离线、只读的 CLI 切片，再按 M0–M6 推进生命周期、Inspection、Evidence/日志、可附着 TUI 与打包部署；Remote Agent 新能力冻结到 M6 完成并再次明确解冻。它不接受 ADR-0003，亦不声称完整 OpsService、federated Inspection 或 Web Console 已实现。
- [Kernel Foundation](kernel-foundation.md) — 在 Accepted [ADR-0004](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md) 的 Deck/DeckLock/Application admission 边界与 Accepted [ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 的 Rust-first mechanisms、polyglot workloads、Cargo/uv 分权和 language-neutral wire boundary 下，从 Capability/Service/Feature、deterministic DeckCompiler→DeckLock（DeckTopology/closure 受 digest 覆盖、Canvas state 排除、Planner 无独立 topology 输入）、Kernel 外 DeploymentPlanCandidate→committed DeploymentPlan→tenure-neutral RuntimePlanSlice 边界、CardDefinition/Port/Link、Mailbox/ExecutionDomain、P2e 最小单写 DeploymentController/tenure authority与本地仿真物理闭环，到 P2e 后须经 PCA admission 的 [PC0–PC3 平台兼容候选工作池](../research/platform-compatibility-ports-and-host-feature-profiles.md)、Zenoh 双 Node、node-local/federated Inspection、OpsService、TUI、Operator/Web Interaction、ROS2Gateway、空间语义和条件式 H1 硬件 gate；状态为 Draft，候选工作池不代表已授权阶段，计划与研究内容也不代表目标 backend 或生产能力已经实现。

Active Program 可以在 Accepted ADR 允许的范围内重排交付优先级；它不能把 Proposed ADR 升级为 Accepted，也不能把 Draft 候选写成已实现。若 Active Program 与 Draft 计划的先后顺序冲突，以 Active Program 为当前执行顺序；若与 Accepted ADR 冲突，必须暂停并先完成 ADR 变更。

实现完成后，稳定用法进入 [`guides/`](../guides/README.md) 或 [`reference/`](../reference/README.md)，系统声明的证据进入 [`testing/`](../testing/README.md)。
