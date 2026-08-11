# ParaEGOX 文档中心

ParaEGOX 的文档与代码一起演进。文档用于区分已经决定的架构、仍在比较的方案、准备实施的计划和已经验证的行为；一份计划或架构图本身不代表对应能力已经实现。

## 当前阅读入口

1. [ADR-0001：DeploymentController、DeploymentPlan 与 Runtime 边界](adr/ADR-0001-deployment-controller-boundary.md)
2. [ADR-0002：CardDefinition、Card 与 CardInstance 术语边界](adr/ADR-0002-card-definition-terminology.md)
3. [ADR-0003：OPS、OpsService 与 Inspection 操作边界](adr/ADR-0003-ops-service-operation-boundary.md)
4. [ADR-0004：Deck 工作负载、DeckLock 与 Application 准入边界](adr/ADR-0004-deck-workload-and-application-admission-boundary.md)
5. [ADR-0006：Rust-first 核心机制与多语言工作负载边界](adr/ADR-0006-rust-first-core-and-polyglot-workloads.md)
6. [Kernel、RuntimeHost 与 Core Services 架构基线](architecture/kernel-runtime-core-services.md)
7. [分布式系统模型](architecture/distributed-system-model.md)
8. [Node 与作用域边界](concepts/node-and-scope-boundaries.md)
9. [CardDefinition、Card 与 Deck](concepts/card-definition-card-deck.md)
10. [Capability、Service Contract 与 Feature Support 边界](concepts/capability-service-feature-boundaries.md)
11. [Agent OS 参考特性采纳矩阵](research/agent-os-reference-feature-adoption.md)
12. [Application、Deck、Card 与 Service 边界研究](research/application-deck-card-service-boundaries.md)
13. [分布式具身 Agent OS 缺口研究与演进计划](research/distributed-embodied-agent-os-gap-analysis.md)
14. [CardDefinition 输入输出、Port、Link 与运行绑定研究](research/card-definition-ports-links-and-bindings.md)
15. [Card 独立开发、测试 Harness 与运行探测边界研究](research/card-independent-development-testing-and-probe-boundaries.md)
16. [分布式身份、作用域与物理所有权研究](research/distributed-identity-scope-and-ownership.md)
17. [Runtime 执行模型、调度与恢复研究](research/execution-model-scheduling-and-recovery.md)
18. [Graph Foundation、领域图与执行边界研究](research/graph-foundation-and-domain-execution-boundaries.md)
19. [Tool 定义、Provider 绑定与调用边界研究](research/tool-definition-provider-binding-and-invocation.md)
20. [Kernel 消息、Zenoh-native Fabric 与 ROS2/DDS 边界研究](research/kernel-messaging-fabric-evidence-security.md)
21. [Web Console、WebRTC、WebXR 与交互式 Gateway 边界研究](research/web-console-webrtc-webxr-gateway-boundaries.md)
22. [Local Operator CLI/Ops Active Program](plans/local-operator-cli-ops-program.md)
23. [Kernel Foundation 实施计划](plans/kernel-foundation.md)
24. [文档目录地图](#目录结构)
25. [项目中文简介](../README_zh.md)
26. [项目英文简介](../README.md)

当前仓库已经进入实现阶段。什么是“下一步获授权工作”由 Accepted ADR 与 Active Program 决定；什么是“已经实现”仍必须回到代码、治理登记和测试/运行证据。生产核心机制选择 Rust-first，Python/C++ 保留为受管工作负载与生态语言。

## 目录结构

```text
docs/
├── README.md          # 总导航、状态和写作约定
├── architecture/      # 当前系统结构、边界和关键流程
├── concepts/          # 稳定术语和心智模型
├── adr/               # 长期架构决策记录
├── research/          # 决策前的证据、比较与开放问题
├── plans/             # 已确定方向的实施顺序
├── guides/            # 面向任务的开发和使用指南
├── reference/         # API、配置、协议和术语参考
├── testing/           # 声明到验证证据的映射
├── runbooks/          # 运行诊断、恢复和回滚手册
└── workbench/         # 唯一允许不进入 Git 的本地草稿区
```

每个目录都有自己的 `README.md`，用于约束内容和提供局部导航。目录下只有说明而没有真实专题文档时，表示该文档面已经预留，但相关产品能力尚未形成。

## 文档类型

ParaEGOX 借鉴 EAGOS 文档工程中“按问题类型区分文档”的经验，但不复制其目录规模。目录只在出现真实内容时创建。

| 目录 | 回答的问题 | 状态要求 |
| --- | --- | --- |
| [`architecture/`](architecture/README.md) | 系统现在如何划分，关键数据和控制流是什么 | 必须标注 Draft 或 Current，并链接相关 ADR |
| [`concepts/`](concepts/README.md) | 一个核心名词在 ParaEGOX 中严格表示什么 | 不承载实现计划或 API 细节 |
| [`adr/`](adr/README.md) | 为什么正式选择某个长期约束 | Proposed、Accepted、Superseded 或 Rejected |
| [`research/`](research/README.md) | 比较过哪些方案，证据和不确定性是什么 | 必须区分事实、推断和开放问题 |
| [`plans/`](plans/README.md) | 已确定方向准备按什么顺序实现 | 必须包含依赖、验证和完成证据 |
| [`guides/`](guides/README.md) | 开发者或使用者如何完成一项工作 | 命令和示例必须经过实际验证 |
| [`reference/`](reference/README.md) | 稳定 API、配置、协议和术语是什么 | 必须与实现版本对应 |
| [`testing/`](testing/README.md) | 一个系统声明由什么测试和场景证明 | 必须链接到真实测试入口 |
| [`runbooks/`](runbooks/README.md) | 运行故障如何诊断、恢复和回滚 | 必须声明权限、风险和恢复证据 |

暂不创建 `progress/`、`checklist/`、`discussion/` 等目录。实现规模足以需要持久化协作状态时，再定义唯一的状态来源，避免文档数量先于产品复杂度增长。

## 文档演进路径

```text
Research ──形成证据──> ADR ──冻结决策──> Plan
                                           │
                                           ▼
                                    Code + Testing
                                           │
                         ┌─────────────────┼─────────────────┐
                         ▼                 ▼                 ▼
                      Guide           Reference          Runbook
```

不是每项工作都需要走完整链路。可逆的小改动可以直接实现并更新 Guide 或 Reference；跨层、长期或难以回滚的决策必须先进入 ADR。

## 权威层级

不同文档回答不同问题，不能用一份 Draft 覆盖另一份已经接受的决策：

1. **Accepted ADR** 冻结长期架构约束；Active Program 不得覆盖它，只能在约束内安排交付顺序。
2. **Active Program** 是当前跨阶段工作的交付权威，记录授权范围、依赖 DAG、冻结项和完成证据。
3. **Current 文档、`governance.toml`、代码与测试/运行证据** 共同回答当前实现事实；其中机器可检查的 owner、公共 surface 和证据入口以 `governance.toml` 为准。
4. **Draft、Proposed、Research Complete 与 Decision Gate** 是候选、待决或准入材料，不能自行成为实现授权，也不能被写成已实现事实。

发生冲突时，先区分是在争论“以后做什么”还是“现在有什么”。前者按 Accepted ADR → Active Program 排序；后者以可复核的实现与验证证据为准。ADR-0003 当前仍是 Proposed，因此它可以指导边界设计，但不能被描述为已经接受或已经实现了完整 OpsService。

## 文档状态

- **Draft**：探索中的架构说明，可以被后续研究修改。
- **Proposed**：已经形成明确决策，等待评审或实现证据。
- **Accepted**：已被项目采用，并有实现或明确的实施授权。
- **Active**：已经获得当前交付授权的 Program，必须维护依赖、状态、冻结项和完成证据。
- **Current**：描述当前代码真实行为，必须能链接到实现和验证。
- **Decision Gate**：用于一次明确准入/停止判断的材料；通过前不授权后续阶段。
- **Superseded**：历史文档，由新文档替代，必须链接替代项。
- **Rejected**：已经明确不采用，保留原因和替代方向。

任何标为 Draft 或 Proposed 的内容都不能被当成已实现能力。代码、测试和运行证据高于计划与示意图。

## 版本化规则

`docs/` 中除 `docs/workbench/` 外的正式文档、manifest 和 authorization receipt 都必须进入 Git，并由治理检查器拒绝未追踪文件。`docs/workbench/` 只用于本地临时推演，既不得被 Git 追踪，也不得被正式文档当作长期权威来源。

旧文档状态行中可能仍含“本地执行视图，不进入 Git”等历史说明；这类括号后缀只描述当时的用途，不再定义版本控制策略。版本控制策略只由 `.gitignore`、`governance.toml` 与治理检查器定义。

## 核心文档写作约定

新的核心文档至少包含：

1. 一句话结论。
2. 状态、日期、范围和非目标。
3. 使用了哪些本地或外部证据。
4. 所有权边界以及允许和禁止的依赖方向。
5. 关键失败场景与反例。
6. 实施顺序和验证边界。
7. 尚未决定的问题。
8. 被替代时的后继文档链接。

术语必须在首次出现时给出限定含义。不同领域不得为同一个概念重复创建名称，也不得用改名掩盖旧抽象。

## 来源与知识产权边界

ParaEGOX 基于 [PhanthyMotus](https://github.com/4paradigm/phanthymotus)，保留其 Git 历史和 Apache-2.0 许可证归属。ParaEGOX 可以复用许可证允许的 PhanthyMotus 代码并保留归属，也可以选择少量适合新系统的名词。具体到应用组合，`Card` 有 PhanthyMotus 的实际代码血缘；`Deck` 是 ParaEGOX 对 `CanvasLayout + Project` 的新形式化，不是 Motus 现存类型；Motus 的 `PerceptionBundle` 只是 MCP endpoint 内的实现聚合。ParaEGOX 当前不把裸 `Bundle` 建成公共总概念，也不把它作为 Deck 别名；未来全新窄对象仍需独立 ADR。

EAGOS 只作为工程经验来源，用于识别已经验证的能力、失败模式和架构风险。ParaEGOX 不复制 EAGOS 的源代码、测试、配置、注释、ADR 正文或领域抽象；相关经验必须先转化为中立的行为需求，再由 ParaEGOX 独立设计和实现。公开文档也不应记录 EAGOS 私有源码路径、逐段映射或可还原内部实现的材料；需要保留的 provenance 审计记录应访问受控并与公开仓库分离。

特别地，ParaEGOX 不使用 `Module` 作为公共领域名。不可变 `CardDefinition` 拥有稳定 ID/version、Port、配置 Schema、Requirements、ExecutionRequirements 与 Artifact export/entrypoint 引用；Deck 中的 `Card` 通过 `uses: CardDefinitionRef` 表达一次配置使用，`CardInstance` 才是运行身份并托管私有实现对象。Deck 是声明式可执行工作负载，不永久等同 Product/Application/Installation；正式 Application 等待多 Deck、稳定安装或应用私有持久状态证据。CardDefinition 不是业务基类，普通实现对象不通过继承获得 Runtime、Bus、线程、Zenoh Session 或权限。EAGOS `Module` 只在历史失败模式和 provenance 中保留原名；完整裁决见 [ADR-0002](adr/ADR-0002-card-definition-terminology.md)与 [Application 边界研究](research/application-deck-card-service-boundaries.md)。

## 后续文档工程顺序

文档工程随真实实现逐步建设：

1. 架构基线稳定后，为不可轻易逆转的决策建立 ADR。
2. 首个 RuntimeHost 和 Core Service 运行后，建立开发指南与生命周期参考。
3. Fabric、Authority 和 ProcessDomain 出现后，建立协议参考与测试矩阵。
4. OPS/TUI 能读取真实 Inspection 数据后，建立运行手册。
5. 公共文档达到可发布状态后，再引入 MkDocs、链接检查和 CI 构建。

不在当前阶段搭建静态站点，避免导航结构和工具配置先于内容定型。
