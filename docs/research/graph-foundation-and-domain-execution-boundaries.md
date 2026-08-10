# Graph Foundation、领域图与执行边界研究

> 状态：Research Complete，结论为 `revise`
> 日期：2026-07-29
> 深度：Deep
> 评审策略：本地受控审计 + 外部一手资料 + 独立工程复核
> 范围：DeckTopology、ServiceDependencyGraph、Deployment、RuntimeHost、Agent Workflow、OPS、Evidence 与 World/Spatial
> 实现状态：尚未实现；本文是 ADR 与实施计划输入，不是代码或功能完成证明

> 后续裁决（2026-07-29）：[ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 已接受“Rust-first mechanisms，polyglot workloads”。它决定首个 RuntimeHost/RuntimeAssemblyEngine 等机制实现的语言与 Cargo 工程 authority，但不接受或改号本文预留的 ADR-0005，也不把领域 Graph Schema、Agent workflow 或工作负载锁进 Rust。

## 一句话结论

ParaEGOX 不应在 Kernel 建设“能运行任意图”的通用 Graph Engine。正确结构是：每个领域拥有自己的权威模型、状态机、失败策略和执行器；只有 DeckCompiler 与 DeploymentPlanner 等至少两个独立真实消费者已经证明存在相同需求后，才把确定性遍历、强连通分量、环路见证和 DAG 拓扑分批等纯算法抽取为极小、无状态、内部使用的 `Graph Foundation`。Deck 的本地启动与替换由 RuntimeHost 内部的 `RuntimeAssemblyEngine` 执行经 DeploymentPlan 编译后的 activation contract，而持续数据流仍直接经过 `PortBinding → Mailbox → ExecutionDomain → CardInstance`，不经过中央图调度循环。

## 1. 研究问题与成功标准

本研究回答：

1. ParaEGOX 为什么不能直接复用一套通用 Graph Engine 运行 Deck、服务依赖、Agent 和 OPS。
2. 哪些图算法可以共享，什么证据出现后才允许进入 Kernel。
3. Deck 如何获得与历史工作负载组合相当的 validate、compile、run、replace、stop 与 inspect 能力。
4. DeploymentController、RuntimeHost 和未来 Agent WorkflowEngine 分别执行什么。
5. `RuntimePlanSlice` 到 DomainInstance、CardInstance、ServiceInstance、Mailbox 和 PortBinding 之间由谁装配、激活、排空与回滚。
6. 有环数据流、并行边、readiness、dependency loss 与跨 Node rollout 应如何处理。

方案成功必须同时满足：

- Kernel 不知道 Card、Deck、Service、Agent、OPS、Evidence 或 World 的领域类型。
- 同一个词 `graph` 不产生一个新的全局 owner、存储、revision、状态机或查询入口。
- DeckLock、DeploymentPlan、Runtime apply journal、Agent run 与 Evidence/World 各自只有一个权威真相。
- `Link`、服务依赖、activation constraint、causal reference 与 spatial transform 不共用边语义。
- 启动、替换和失败可以通过确定性 Harness 证明，而不是只凭拓扑图“看起来正确”。
- 第一版不创建空 `graph/` 包、占位接口或尚无消费者的公共 Schema。

## 2. 范围、假设与非目标

### 2.1 范围

- ParaEGOX 当前 clean-slate 文档、治理规则与尚未实现的 Kernel Foundation 计划。
- 受控本地历史实现中，工作负载启动图、有限执行图、Agent/Task/OPS 图与图查询的真实职责和失败模式。
- 分布式控制循环、流处理、durable workflow、机器人 dataflow 与纯图算法的外部一手资料。
- 单 Node Deck 运行闭环，以及它向多 Node rollout 演进时的 owner 边界。

### 2.2 假设

- Zenoh 是 ParaEGOX 的唯一生产 Fabric；Graph Foundation 不承担 transport。
- Deck 是声明式可执行工作负载，不是 Agent workflow、Deployment transaction 或 Product installation。
- RuntimeHost 只消费 canonical target `RuntimePlanSlice`，不 import DeckSpec、DeckLock 或 DeploymentPlan。
- 首个生产 RuntimeHost 与内部 assembly mechanism 使用 Rust/Cargo；Python SDK、领域 worker、Agent/模型和治理工具继续使用 `uv`。公共 Schema、canonical encoding 与 failure state 必须 language-neutral。
- 当前仓库还没有 Graph、Runtime 或 Deployment 的生产实现，因此所有包名与 API 都必须通过后续 admission。

### 2.3 非目标

- 不在本文冻结公共 `GraphNode`、`GraphEdge`、`GraphDef` 或 YAML/JSON 图格式。
- 不选择 Agent workflow 框架，也不实现 durable workflow。
- 不把 Zenoh Flow、Dora、Flink、Beam、LangGraph 或 Temporal 作为首版依赖。
- 不建立通用 Graph Store、Graph Service、Graph Query Router 或 Graph UI 真相。
- 不通过复制或重构历史私有实现来完成 ParaEGOX。

## 3. 证据与强度

### 3.1 本地证据

| 证据 | 类型 | 强度 | 对结论的影响 |
| --- | --- | --- | --- |
| ParaEGOX 已把 DeckTopology、ServiceDependencyGraph、DeploymentPlan 与 RuntimeOwnershipTree 分给不同 owner | local | 高 | 现有边界基本正确，缺的是执行链细化而不是万能引擎 |
| 当前 Runtime apply 合同已有 writer fence、prepared、active、exact target-slice CAS 和独立 revision 单调性，但没有明确本地装配 owner | local | 高 | 需要 RuntimeHost 内部 assembly mechanism |
| 受控历史实现中的通用执行图固化了 Tool、行为树、审批、checkpoint 和本地消息语义 | local | 高 | 它是领域 workflow engine，不是图论 Kernel |
| 同一历史系统仍分别实现了工作负载 boot order、Task graph、Agent scheduler 与 OPS lowering | local | 高 | “共享引擎”没有消除领域执行器，反而形成双模型和有损 lowering 风险 |
| 工作负载组合的启动顺序实际由独立纯拓扑算法处理，而不是由通用执行图运行 | local | 高 | 可共享的是纯结构算法，不是生命周期状态机 |
| 通用查询投影无法无损表达 Evidence 的时间窗、redaction、ref-only 等约束 | local | 高 | Inspection 可以联合只读投影，但不能成为权威 Graph API |

上述历史证据只用于抽取中立行为需求和反例；公开文档不复制其源码、配置、测试或私有路径。

### 3.2 外部一手资料

| 来源 | 类型 | 强度 | 对结论的影响 |
| --- | --- | --- | --- |
| [Kubernetes Controllers](https://kubernetes.io/docs/concepts/architecture/controller/) | external | 高 | desired/observed reconciliation 由多个 focused controller 完成；支持 DeploymentController，但不支持把控制循环等同于 workflow graph executor |
| [Apache Flink Architecture](https://nightlies.apache.org/flink/flink-docs-release-2.3/docs/deployment/overview/) 与 [Execution Mode](https://nightlies.apache.org/flink/flink-docs-stable/docs/dev/datastream/execution_mode/) | external | 高 | logical graph、physical execution graph 与运行组件分离；无界 streaming 的持续并发执行不同于有限 DAG 逐节点调度 |
| [Apache Beam Programming Guide](https://beam.apache.org/documentation/programming-guide/) 与 [Runner Capability Matrix](https://beam.apache.org/documentation/runners/capability-matrix/) | external | 中高 | 统一中间表示只有在 window、trigger、state、timer 和 runner capability 明确时才成立，不能靠裸 node/edge 泛化语义 |
| [LangGraph Pregel](https://docs.langchain.com/oss/python/langgraph/pregel)、[Graph API](https://docs.langchain.com/oss/python/langgraph/graph-api) 与 [Persistence](https://docs.langchain.com/oss/python/langgraph/persistence) | external | 中高 | Plan/Execute/Update superstep、checkpoint 与 durable state 适合 Agent workflow，不适合持续低延迟 Deck data plane |
| [Temporal Workflow Definition](https://docs.temporal.io/workflow-definition) 与 [Workflow Execution](https://docs.temporal.io/workflow-execution) | external | 高 | durable workflow 依赖确定性 replay 与 Activity 副作用边界；retry/resume 不是普通图算法 |
| [GraphBLAS](https://graphblas.org/) | external | 中 | 可共享图算法原语不等于共享应用生命周期引擎 |
| [Zenoh Flow](https://github.com/eclipse-zenoh-flow/zenoh-flow) | external | 中 | descriptor、runtime mapping 与 source/operator/sink 生命周期值得参考，但不证明其模型应成为 ParaEGOX Kernel |
| [Dora dataflow](https://dora-rs.ai/dora/concepts/dataflow-yaml.html) 与 [Dora repository](https://github.com/dora-rs/dora) | external | 中 | 机器人 dataflow 的声明、daemon/coordinator 与运行命令可作 UX 对照；其对象和协议不自动满足 ParaEGOX Authority、Deployment 与 Runtime fencing |

### 3.3 推断、开放证据与置信度

- **inference，高置信度**：相同的 `A → B` 形状不能证明 edge 语义、状态机或失败策略相同。
- **inference，高置信度**：DeckTopology 必须按 directed multigraph 建模能力考虑，因为同一 Card 对之间可以通过不同 Port 存在多条 Link；把基础结构限制为简单 DAG 会立即产生债务。
- **inference，高置信度**：Runtime 需要显式 assembly owner，否则实例创建、readiness、binding activation、drain 和 rollback 会散落回 RuntimeHost、Card loader 与 Fabric callback。
- **inference，中高置信度**：首版应能表示和诊断 Deck SCC，但在 feedback/delay/seed/backpressure 合同冻结前，生产 admission 应拒绝有环数据流。
- **open**：首个允许的 feedback contract 是显式 delay、initial seed、latest-value、drop/coalesce，还是受限组合。
- **open**：跨 Node activation barrier 的超时、补偿和部分成功 Schema 尚需 ADR 冻结。
- **open**：Graph Foundation 最终是否位于 `kernel/`；只有两个以上真实生产消费者和故障边界证据出现后才能决定。若准入，ADR-0006 使首个参考实现倾向于 Cargo workspace 内的私有 Rust leaf crate，但不会因此自动取得 Kernel/public API 地位。

## 4. 关键区分：图结构不等于图执行模型

图结构只回答：有哪些 vertex、edge 和结构关系。执行模型还必须回答：

- 什么条件使一个工作项 ready；
- edge 传递数据、readiness、控制、回滚、因果还是坐标变换；
- 是否允许环、并行边和动态成员；
- 重试会不会重放副作用；
- timeout、cancel、approval denied 与 dependency loss 是什么状态；
- 谁持久化、谁推进 revision、谁生成 Receipt；
- crash 后从哪里恢复，以及旧结果如何 fencing。

ParaEGOX 中至少存在以下不同图或图状关系：

| 关系 | 权威 owner | edge 的含义 | 环策略 | 执行方式 |
| --- | --- | --- | --- | --- |
| `DeckTopology` | DeckCompiler 产生，DeckLock 持有 | Card.Out 到 Card.In 的 `DataLink` | 结构可检测 SCC；首版无显式 feedback contract 时拒绝 | 部署后为持续消息流，不逐节点跑完 |
| `ServiceDependencyGraph` | ServiceSpec 声明，DeploymentPlanner 编译 | provider readiness 与 dependency-loss 义务 | 必须是 DAG，发现环即计划失败 | Deployment/Runtime lifecycle gate |
| Deployment rollout relation | DeploymentController | target prepare/activate/drain/rollback 协调 | 由 revision transition 决定 | desired/observed reconcile loop |
| Runtime assembly relation | RuntimePlanSlice + RuntimeHost apply journal | 本 Node 的创建、readiness、activation 和 drain 约束 | 只消费已编译 group/barrier | 一次 apply/replace/stop 的局部状态机 |
| Agent workflow/plan | Agent owner | reasoning、tool、wait、join、approval、loop、compensation | 领域预算允许的显式 loop | 未来 durable workflow 或明确状态机 |
| OPS operation flow | OpsService + actual typed owner | consent、请求、进度、补偿和 Receipt chain | 不允许隐式循环 | ControlRequest 状态机与 owner 调用 |
| Evidence causal projection | Evidence owner | provenance/causality reference | 可以有异常引用，按领域规则处理 | refs-only 查询与投影，不执行 |
| Frame/World/Spatial graph | Physical/World owner | 时空 transform、scene relation、uncertainty | 按 frame/world 领域不变量 | 查询、估计与校准，不执行 workflow |
| Fabric/Node observed topology | Fabric/Node owner | session、route、presence 和 reachability 事实 | 动态且带 epoch/freshness | 观察与路由，不成为 desired plan |

因此不建立 `GraphKind` 开关后执行 `execute(graph)`。领域类型在编译期决定语义，不能把差异塞进 `metadata: dict`。

## 5. 受控历史路径的中立结论

历史实现给出了三个重要反例：

1. 一套名为通用的执行图实际绑定了 Tool、行为树、审批、checkpoint、消息和 handler 调用；与此同时，工作负载启动、Agent、Task 与 OPS 仍各自保留调度或领域模型。
2. 领域模型 lowering 到通用 node/edge 后，risk、consent、effect、resource、loop budget 与 compensation 等字段只能进入 params/metadata；权威模型和执行模型可能分别校验、分别持久化并逐渐漂移。
3. retry、timeout、resume、approval、checkpoint 和并发预算如果没有领域 effect/fencing 语义，容易产生“仍有 pending 却成功”“拒绝后后继继续”“副作用被重放”或“恢复能力被高估”等错误。

这不说明永远不能复用代码。它说明正确复用层是纯算法；失败策略仍由调用领域决定。例如拓扑算法只返回 cycle witness，Service planner 决定 fail-fast，Deck compiler 决定 admission profile，RuntimeHost 决定不产生任何副作用。

## 6. 方案比较

### A. Kernel 通用 Graph Engine

把 node/edge、loader、状态、scheduler、retry、checkpoint、query 和 store 放进 Kernel。

- 优点：早期 demo 看似统一，Canvas、Agent、OPS 可以共用一个入口。
- 致命问题：领域语义倒灌 Kernel；需要 `GraphKind`、opaque metadata 和 feature flag；形成第二 desired/runtime truth；副作用恢复无法正确泛化。
- 裁决：**拒绝**。

### B. 每个领域永久复制全部图算法

- 优点：owner 清楚，领域可独立演化。
- 问题：stable ordering、SCC、cycle witness 等纯算法会重复并可能产生不一致结果。
- 裁决：**不足**；适合作为领域模型起点，不是长期目标。

### C. 极小 Graph Foundation + typed domain engines

- 领域先拥有 typed model、validator、状态机和失败策略。
- 两个独立生产消费者出现相同纯算法后，抽取内部 leaf library。
- Foundation 不执行、不持久化、不联网、不拥有 revision。
- 裁决：**推荐**。

### D. 直接采用 Zenoh Flow、Dora、Flink、Beam 或其他 dataflow runtime

- 优点：复用成熟的图解析、调度、分布式或流处理能力。
- 问题：ParaEGOX 的 Card/Deck、RuntimePlanSlice、Authority、physical safety、revision fencing、Zenoh native Fabric 和轻量机器人目标并不等价；引入后仍需完整 adapter 和 owner proof。
- 裁决：**只作参考或未来显式 adapter**，当前不依赖、不 fork 为内核。

### E. 通用 Graph Service / Store / Query Router

- 优点：UI 可以从一个入口看所有图。
- 问题：Evidence redaction、World freshness、Deployment revision 与 Agent run 的鉴权和一致性不同；统一存储会成为第二真相和权限绕过点。
- 裁决：**拒绝**。Inspection 未来可以联合各 owner 的只读、有损 `GraphView` 投影，但投影不能写回、执行或恢复 owner state。

## 7. 推荐架构

```text
DeckSpec ──DeckCompiler──────────────> DeckLock {canonical DeckTopology}
                  │                               │
ServiceSpec ──────┼──DeploymentPlanner────────────┤
NodeFacts/policy ─┘                               ▼
                                      DeploymentPlanCandidate
                                                  │ atomic commit
                                                  ▼
                                      committed DeploymentPlan
                                                  │ target projection
                                                  ▼
                                      RuntimeApplyRequest
                                      {RuntimePlanSlice + CAS}
                                                  │
                                                  ▼
                                      RuntimeHost
                                      └─ RuntimeAssemblyEngine
                                         prepare → ready → activate
                                         drain / retire / rollback
                                                  │
                                                  ▼
                 steady data path: PortBinding → Mailbox → ExecutionDomain
                                                       → CardInstance callback

AgentSession/AgentRun ── future Agent WorkflowEngine（Kernel 外、独立准入）
Evidence/World/Fabric ── owner-specific graph/query semantics

optional internal leaf library after admission:
Graph Foundation = immutable graph view + pure deterministic algorithms only
```

这里没有中央“Deck Graph loop”。Graph Foundation 在编译和校验时运行；RuntimeAssemblyEngine 只在 apply/replace/stop 阶段推进本地生命周期；稳定运行后的每条消息不再绕回它们。

## 8. Graph Foundation 的精确边界

`Graph Foundation` 是研究阶段的能力名，不是已批准包名、CoreService 或公共 API。若 ADR-0005 后续接受并满足双消费者门槛，首个实现可以是内部 Rust leaf crate；调用边界仍是 typed、不可变、language-neutral 的领域 view，不能让 Rust struct/trait、trait object ABI 或 crate serialization 变成 Deck/Deployment/Agent 的公共 Schema。

### 8.1 允许的能力上限

- 调用方提供稳定 node key、edge key、source 与 target 的不可变 directed multigraph view。
- 并行边、自环和确定性迭代。
- endpoint/edge-key 结构校验。
- 强连通分量与 condensation。
- 具体、确定性的 cycle witness。
- 对已经由领域声明为 DAG 的 view 做 stable topological batches 与 reverse batches。
- 按真实消费者需要提供 reachability；structural diff 只有第二个消费者出现后再准入。
- 结构化 diagnostics；reason code 由调用领域映射成自己的公开错误。

算法必须只依赖调用方提供的稳定排序键，不能自行发明全局 canonical identity。

### 8.2 明确禁止

- 公共 `GraphId`、`GraphRevision`、`GraphDomain`、`GraphNodeState` 或 `GraphRunState`。
- 公共 `GraphNode/GraphEdge/GraphDef` 持久 Schema。
- YAML/JSON loader、canonical serialization、digest、registry、store、query router。
- `execute(arbitrary_graph)`、async scheduler、线程、进程、I/O 或后台任务。
- retry、timeout、approval、checkpoint、resume、compensation、Receipt、fallback 或 rollback 策略。
- Card、Deck、Service、Agent、OPS、Evidence、World、Deployment 或 Runtime import。
- `metadata: dict` 作为领域语义逃生口。

DeckLock digest 继续由 Deck owner 定义；DeploymentRevision/PlanContentDigest 继续由 Deployment owner 定义；AgentRun、Evidence 和 World revision 继续由各自领域定义。Graph Foundation 不统一这些 identity。

### 8.3 准入门

当前不创建 `kernel/graph`、`graph_core` 或占位 Protocol。只有同时满足以下条件才允许抽取：

1. 至少两个独立、生产代码消费者；测试 fixture、同一 owner 的 wrapper 或为证明抽象而创建的消费者不计。
2. 两者需要的算法、排序确定性和错误事实真正相同。
3. 提取后不引入领域 import、opaque metadata、第二份 serialization/digest 或执行状态。
4. property test 证明插入顺序不影响结果，并覆盖并行边、自环、SCC 和 cycle witness。
5. 有明确 owner、兼容策略、移除条件和架构依赖检查。

首个可能的同批消费者是 DeckCompiler 的 multigraph/SCC 校验与 DeploymentPlanner 的 ServiceDependency DAG 校验。实现顺序仍是先写各自 typed input/expected behavior，再在同一 bounded batch 中确认并抽取重合算法；不能先做 Foundation 再强迫领域适配。

## 9. DeckTopology 与 Deck 的运行语义

### 9.1 DeckTopology 是 dataflow declaration

DeckTopology 描述 `Card.Out → Card.In` 的 `DataLink`。它回答 endpoint、Schema/interaction、DeliveryProfile 和结构关系，不直接回答服务 readiness、进程启动或失败恢复。

同一对 Card 之间可以有多个不同 Port Link，因此结构必须支持 parallel edge。Link identity 至少受 source Port、target Port 和 Deck-scoped stable key 限定，不能只用 `(source_card, target_card)` 去重。

### 9.2 DataLink 不等于启动依赖

`A.Out → B.In` 不表示“必须先启动 A，再启动 B”。通常恰好需要：

1. 先创建 B 的 Domain、CardInstance、Mailbox 和 ingress binding。
2. 再创建或 ready A。
3. 最后开放 A 的 producer egress。

否则 producer 可能在 consumer admission boundary 存在前发送数据。服务依赖则不同：消费者必须等 provider readiness gate。两种 edge 不能共用一个 topological order。

### 9.3 有环 dataflow

基础结构不能假设 Deck 永远是 DAG，因为反馈控制、状态估计和迭代处理可能需要环。但“能表示环”不等于“首版可以安全运行环”。

P2e 的保守 admission 应为：

- compiler 确定性报告 SCC 与具体 cycle witness；
- 没有显式、已批准 feedback contract 时拒绝 cyclic Deck；
- 不回退到声明顺序，不自动插 buffer，也不把环当成无限 retry；
- 后续只允许带显式 break semantics 的环，例如经 ADR 冻结的 delay/seed/latest-value/non-blocking policy；
- 每个允许的 SCC 必须有 bounded Mailbox、启动条件、无初始 token 行为、overflow/backpressure 和 shutdown 证明。

环上的 `block_until_deadline` 或所有边都要求阻塞 backpressure 会形成分布式等待环，默认拒绝。

## 10. ServiceDependencyGraph

ServiceDependencyGraph 是 typed lifecycle/readiness DAG，不是 dataflow graph。每条 requirement 至少编译出：

- provider identity/selection；
- 何种 readiness 满足 consumer activation；
- provider loss 后的 `degrade / stop / rebind / restart / fail-closed` 行为；
- debounce、freshness、epoch 和恢复条件；
- shutdown 时 consumer-before-provider 的逆序义务。

任何环都在 DeploymentPlanCandidate 产生前 fail-fast，并给出稳定 cycle witness。算法只报告结构事实；失败 reason、用户解释和 remediation 属于 DeploymentPlanner。

## 11. RuntimeAssemblyEngine

### 11.1 为什么需要它

当前架构已有 `RuntimeApplyRequest → RuntimeHost`，但如果没有一个明确的内部 owner，以下动作会分散到 loader、Domain、Fabric 和 Card 回调：

- 谁创建 DomainInstance、ServiceInstance、CardInstance 和私有实现对象；
- 谁安装 Mailbox 与 inactive PortBinding；
- 谁等待 readiness、打开 ingress/egress；
- 谁在 partial failure 时回滚；
- 谁按 revision 排空和回收旧实例。

`RuntimeAssemblyEngine` 是 RuntimeHost 内部的确定性 apply mechanism，用来集中这些动作。依据 ADR-0006，首个参考实现位于 Rust RuntimeHost 内；它不是 CoreService、daemon、公共 graph executor 或第二个 lifecycle owner，RuntimeHost 仍是唯一持有 PID/Domain/Binding 并执行副作用的 owner。Rust/Tokio 不改变 apply 状态机：取消 future 不证明外部副作用已取消，dispatch 后缺少 terminal proof 仍必须进入 `Uncertain → query/reconcile`，`spawn_blocking` 也不等于有界 ThreadDomain。

### 11.2 输入与禁止依赖

它只消费：

- 已认证的 runtime-owned `RuntimeApplyRequest`；
- canonical target `RuntimePlanSlice`；
- RuntimeHost 的 writer fence、prepared/active journal 与 observed facts；
- Runtime-owned artifact/config/resource/Domain/Binding ports。

它不能 import 或查询 DeckSpec、DeckLock、DeckTopology、DeploymentPlan、DeploymentController、ServiceSpec registry 或 editable desired state。它可以从 Slice 派生一次 ephemeral `assembly relation`，但不得持久化成另一份可编辑 Runtime graph，也不得拥有独立 revision/digest。

### 11.3 RuntimePlanSlice 必须携带的 activation contract

最终字段名等待 ADR/Schema 冻结，但 `DeploymentPlan.execution` 及 target Slice 至少要无歧义地表达：

- 实例、Domain、Mailbox、Binding 与资源 assignment；
- typed activation dependency；
- readiness gate 及其 timeout/failure action；
- activation group/barrier，包括经准入的 SCC group；
- consumer ingress 与 producer egress gate；
- provider loss action；
- drain/retire 顺序和 deadline；
- revision/epoch fencing、rollback boundary 和 collateral restart scope。

RuntimeAssemblyEngine 不从普通 Deck Link 猜测这些规则，也不重新运行全局 placement 或 service resolution。

### 11.4 本地 apply 状态机

```text
admit request
  ├── verify target / writer tenure / digest / CAS / deadline
  └── persist writer_fence before side effects
          ↓
prepare
  ├── stage Artifact + validated config + resources
  ├── create DomainInstance + Card/ServiceInstance
  ├── create bounded Mailbox
  ├── install inactive ingress/egress PortBinding
  └── persist prepared operation/revision
          ↓
ready + activate
  ├── satisfy provider/readiness gates
  ├── activate consumer ingress / admitted activation groups
  ├── atomically switch active revision/binding admission
  └── open producer egress last
          ↓
steady state
  └── no central graph loop; normal PortBinding/Mailbox/Domain path
          ↓
replace or stop
  ├── close old producer egress
  ├── drain/cancel within deadline
  ├── retire old Binding/Instance/Domain/resources
  └── emit observed facts and Receipt

failure before active CAS → remove prepared resources, keep old active
failure after partial activation → bounded rollback or quarantine; never report global success
```

跨 RuntimeHost 时，DeploymentController 协调 target prepare 和 rollout；每个 RuntimeAssemblyEngine 只执行本地 slice。它不能把跨 Node 部分成功冒充本地原子事务。

### 11.5 与 DeploymentController 的关系

| 能力 | DeploymentController | RuntimeAssemblyEngine / RuntimeHost |
| --- | --- | --- |
| desired plan 与 revision | 唯一 owner | 只验证 source provenance |
| provider/placement/全局 activation compile | 负责 | 不重算 |
| writer tenure 后的 apply 请求 | 构造并发送 | fencing/CAS/admission |
| 多 Node rollout/补偿/reconcile | 负责 | 返回本地事实与 Receipt |
| 本地 PID/Domain/Binding/Instance | 不持有 | RuntimeHost 独占 |
| 本地 prepare/activate/drain/retire | 协调目标阶段 | 实际执行 |
| steady data/message dispatch | 不参与 | PortBinding/Mailbox/ExecutionDomain |

这也是 Deployment 进入系统架构规划但不进入 Kernel 的原因：它拥有分布式 desired state 和 reconciliation，不拥有本地执行对象。

## 12. Agent 与 OPS 的未来执行器

### 12.1 Agent WorkflowEngine

Agent workflow 才可能需要有限步骤、wait、approval、loop、parallel join、checkpoint、compensation 与 resume。即便未来建立，也应位于 Agent 领域，调用 typed Tool/Operation client，并遵守 Runtime、Authority、Resource、Safety 和 Receipt 边界。它可以由 Python、Rust 或其他语言实现；不得因为 RuntimeHost 是 Rust 就把领域 workflow 状态、checkpoint 或框架塞进 RuntimeAssemblyEngine。

只有至少两个独立生产 workflow 消费者在以下语义上真正一致时才研究共享引擎：

- node lifecycle 和 edge condition；
- pause/resume/checkpoint；
- retry、effect idempotency、fencing 与 `Uncertain`；
- compensation、join、approval 和 cancellation；
- durable state owner、版本迁移和 Receipt。

在此之前使用显式 Agent state machine，比先建通用 engine 更安全。

### 12.2 OPS

OpsService 首版只拥有 `ControlRequest → typed owner call → progress → OpsReceipt` 状态机和幂等 journal。它不内置任意 DAG/saga executor。若未来大量运维操作确实共享 durable compensation 语义，另做 Workflow ADR；不能把 Agent Engine 或 RuntimeAssemblyEngine 直接复用为 OPS owner。

## 13. Inspection 与可视化

Canvas 可以展示 DeckCompiler 产生的 DeckTopology 验证投影；TUI/Console 可以联合展示 service readiness、deployment rollout、runtime assembly、message pressure 与 Agent run。但联合 UI 不产生联合权威模型。

未来若出现通用 `GraphView`，必须满足：

- 只读且明确标记 source owner、revision/epoch、observed_at、freshness 与是否有损；
- 由 owner-specific query/Inspection adapter 产生；
- 不作为执行、恢复、授权、持久化或 write-back 输入；
- 不绕过 Evidence redaction、World time/uncertainty 或 Deployment writer fencing；
- UI node/edge identity 不能反向成为领域 identity。

## 14. 有序实施计划

本计划修订现有 P0–P9，不创建平行阶段体系。

### G0：ADR 与 Schema 冻结，先不写 Graph 包

- 提出 `ADR-0005 — Graph Foundation、领域图与执行引擎边界`。
- 冻结 DataLink、ServiceDependency、activation constraint 三种不同 edge。
- 冻结 P2e 对 cyclic Deck 的保守拒绝策略与稳定 diagnostics。
- 在 RuntimePlanSlice Schema 中冻结 activation/readiness/egress/drain 合同。
- 以 canonical wire/golden vectors 冻结 language-neutral 输入输出；不把 Rust enum/trait 或 Python class identity 当公共合同。

完成证据：ADR Accepted；架构/术语/import checks 不允许 universal Graph Engine、Graph Store、Graph Service 或 Runtime 反向 import。

### G1：P2e 的两个 typed domain consumers

- DeckCompiler 使用 Deck-owned typed model 校验 endpoint、parallel Link、SCC/cycle，并生成 canonical DeckTopology/DeckLock。
- DeploymentPlanner 使用 Service-owned typed model 校验 provider/readiness DAG、cycle 与 reverse shutdown batches。
- 先写领域 golden/property tests和错误语义，再确认算法交集。

完成证据：两者不共享领域 Schema，不通过 metadata lowering；相同输入稳定，cycle witness 可复现；Graph 算法还没有执行副作用。

### G2：有证据时同批抽取最小 Graph Foundation

- 只抽取 G1 已经重复的纯算法。
- 保持 internal/private；不承诺公共 API。首个实现若为 Rust leaf crate，由 Cargo 管理并禁止 `unsafe`、I/O、Tokio task 与领域依赖。
- 加 architecture test 禁止领域 import、I/O、execution state 和 serialization/digest。
- 若实际交集不足，取消抽取，不为了目录美观创建 package。

完成证据：至少两个真实消费者；提取前后领域输出 byte/diagnostic 等价；删除 Foundation 后可在一个 bounded revert 中恢复领域实现。

### G3：编译 activation contract

- DeploymentPlanner 将 service readiness、runtime assignment、consumer ingress、producer egress、activation group、loss action 和 drain order 写入 `DeploymentPlan.execution`。
- RuntimeSliceProjector 只做 canonical target projection，不重算语义。
- PlanContentDigest 与 target slice digest 覆盖相关内容。

完成证据：修改 activation rule 必然改变 plan/slice digest；普通 Link 变化不会被悄悄解释成 service dependency；同一 canonical 输入产生相同 plan。

### G4：RuntimeAssemblyEngine 与单 Node Deck vertical slice

- 在 RuntimeHost 内实现 admit/prepare/ready/activate/drain/retire/rollback。
- 首个实现使用 Rust，但仍显式登记每个 task/thread/process、budget、cancel、deadline 与终态；不以 Rust ownership 或 async cancellation 替代 RuntimeOwnershipTree 和 recovery proof。
- 创建真实 DomainInstance、CardInstance、Mailbox 和 inactive/active PortBinding。
- 打通 `DeckSpec → DeckLock → candidate → commit → Slice → apply → DeckRun Inspection`。
- 实现 run、replace、stop 和 partial failure，不实现中央 steady-state graph scheduler。

完成证据：重复 apply 幂等；旧 revision 被 fencing；prepare/activate crash 保留旧 active；同一 BindingId 不双活；停止后无 Task/Thread/Process/FD/SHM 泄漏。

### G5：跨 Node rollout

- DeploymentController 协调多 RuntimeHost prepare/activate 和 producer-egress opening。
- 记录 per-target applied/uncertain facts，执行 bounded rollback/reconcile/quarantine。
- Zenoh route 继续遵守 single-active BindingEpoch。

完成证据：partition、timeout、controller restart、target restart 和 late reply 不产生 revision rollback、双 active route 或伪原子成功。

### G6：Agent workflow 另行准入

- 收集两个真实 durable workflow 消费者和失败 Harness。
- 对 LangGraph/Temporal/自研显式状态机做独立 research。
- 若语义不能无损统一，继续保持领域 state machine，不建立共享服务。

## 15. 验证矩阵

| 声明 | 最小测试/故障注入 | 通过证据 |
| --- | --- | --- |
| Foundation 结果确定 | 随机打乱 node/edge 插入顺序 | SCC、batches、cycle witness 与 diagnostics 不变 |
| 支持 multigraph | 同 Card pair 多 Port Link、自环、重复 edge key | parallel edge 保留；非法重复稳定拒绝；不按端点误去重 |
| Service graph 必须为 DAG | 注入直接环、长环、自环 | 任何 Runtime 副作用前失败并给具体 witness |
| Deck Link 不是启动依赖 | producer→consumer Link 与反向 service requirement 组合 | readiness/activation order 来自 typed constraints，不来自 Link topo sort |
| cyclic Deck 保守拒绝 | 两 Card feedback 和多 Card SCC | 无显式 feedback contract 时稳定 fail-fast，不回退声明顺序 |
| activation 进入 digest | 逐个修改 gate/group/loss/drain rule | PlanContentDigest 和对应 target slice digest 改变 |
| Runtime 不拥有上层真相 | 不安装/import decks/deployment，读取序列化 Slice fixture | RuntimeAssemblyEngine 可完成本地 apply；没有 Deck/Plan lookup |
| 多语言 Slice 一致 | Rust decode Python golden vector、Python decode Rust golden vector、unknown field/version mismatch | canonical digest、activation groups、错误 reason 与终态完全一致；语言私有类型不越界 |
| Rust async 不吞掉执行责任 | abort future、阻塞 FFI、超时外部 effect、wedged `spawn_blocking` | task/thread/process census 有界；外部 effect 无 terminal proof 时进入 Uncertain/reconcile |
| prepare 不污染 active | artifact stage、instance create、readiness 阶段 crash | restart 后旧 active 保持；prepared 可清理/继续且不双活 |
| activate 原子且幂等 | 重复 operation、旧 writer、旧 revision、exact-slice CAS conflict | 相同请求同结果；冲突拒绝；active slice 精确匹配且 revision 单调 |
| consumer 先于 producer egress | 在各 activation point 注入消息 | ingress 未 ready 前 producer 不发送；切换无丢失假承诺或双投 |
| dependency loss 明确 | provider crash/stale/epoch change/recover | 按编译 action degrade/stop/rebind/restart/fail-closed，旧 client 不复活 |
| replace/stop 可清理 | drain timeout、wedged thread、killed process | honest status/Uncertain；无 orphan 和旧 BindingEpoch |
| steady path无中央引擎 | streaming/command 压力基准 | 消息只走 Binding/Mailbox/Domain，assembly 调度不进入 hot path |
| Inspection 不是第二真相 | source owner restart、projection stale/redaction | UI 标注 revision/freshness/lossy；不能 write-back 或恢复 owner |
| workflow effect 不被重放 | future Agent resume 前后注入副作用 timeout | 无 idempotency/fencing 证明时为 Uncertain，不透明 retry |

Graph Foundation 可使用独立参考算法作测试 oracle，但测试 oracle 不能泄漏成生产公共对象或依赖；是否引入 dev-only library由实现批次决定。

## 16. 发布、回滚与移除策略

- P2e 首版仅启用无环 Deck profile；未来 feedback 能力按独立 ADR 和 feature gate 启用。
- RuntimeAssemblyEngine 先在单 Node、无 Zenoh production route 的 PortBinding fixture 上完成，再接 P4 Zenoh 和 P5 双 Node。
- Rust graph/runtime mechanisms 用 Cargo test/clippy/architecture checks 验证；Python 领域模型、fixtures 与 SDK 用 `uv` 验证；跨语言 golden/conformance suite 同时作为发布 gate。
- activation compile 可先 report-only 比较现有期望，Harness 通过后才允许 apply。
- 新 revision 失败保留旧 active；不能恢复时 quarantine，不回退到手写启动顺序。
- Graph Foundation 必须可以作为一个 bounded change 整体移除；领域 owner 的 Schema、错误语义和测试不依赖其类型。
- 不保留“旧 Graph Engine”和“新领域引擎”双执行路径。

## 17. 风险、反例与失效条件

### 17.1 主要风险

| 风险 | 早期信号 | 控制措施 |
| --- | --- | --- |
| Core creep | Foundation 出现 loader、status、retry 或 async | architecture/import test；ADR 变更；立即退回领域 owner |
| 第二真相 | Runtime 持久化自己的 editable graph/revision | Slice-only 输入；只 journal apply state，不存 desired graph |
| 语义最低公分母 | 大量 metadata、GraphKind、feature flag | typed domain model；禁止 opaque lowering |
| DataLink/依赖混淆 | 用 Deck topo order 决定 service start | typed edge separation 与反例测试 |
| 环路死锁 | SCC 全边阻塞、无 seed | 首版拒绝；后续显式 feedback/buffer policy |
| 副作用重放 | generic retry/resume 处理 Command | effect owner idempotency/fencing；Uncertain 默认不 replay |
| Kernel dependency magnet | 新 graph domain 必须修改 Kernel enum | Foundation 无 domain registry；owner adapter 在上层 |
| UI 变成写入入口 | Canvas/Console 修改 derived graph | DeckSpec/OpsProtocol 是唯一 write path |
| 语言实现反向冻结领域 | Rust struct/enum 或 Python object 被当作公共 Graph Schema | canonical language-neutral contract；crate/package 类型只留在 owner 内部 |
| Rust 被误当实时/安全证明 | Tokio 延迟或 safe Rust 被直接写成 deadline/safety guarantee | 目标平台 worst-case benchmark、独立 RealtimeDomain/Safety island 与故障注入仍是 gate |

### 17.2 最强反例

完全禁止共享会让多个领域各自实现 stable topo、SCC 与 cycle witness，产生不一致 ordering 和缺陷。因此本文不主张“Kernel 永远不能有 graph 算法”；它主张先有两个真实消费者，再抽取无领域语义的最小 leaf library。

另一个反例是：若未来 ParaEGOX 产品被明确收窄成一种固定 dataflow appliance，所有图都只有同一种 data/event-time/checkpoint 语义，采用成熟 dataflow runtime 可能比自研更合理。当前 Agent、物理控制、Deployment、Evidence 与 World 的目标明显不满足此前提。

### 17.3 会推翻本结论的证据

要把通用 Workflow/Graph Engine 提升到共享基础层，至少需要同时证明：

1. 两个以上独立生产 workflow 消费者在生命周期、edge、retry、resume、approval、compensation、join、Receipt 和 effect uncertainty 上一致。
2. domain 到 generic 的 lowering 无损且无需 opaque metadata，不存在双持久化真相。
3. crash/timeout/cancel/resume Harness 证明副作用不重放、late result 被 fencing、并发有界、terminal status 诚实。
4. 缺陷或维护数据证明重复领域执行器造成真实成本。
5. 该能力确实必须位于 Kernel bootstrap/failure boundary；“都长得像图”或“UI 想统一展示”不算证据。

## 18. ADR 影响与决策门

本研究建议新增 Proposed `ADR-0005 — Graph Foundation、领域图与执行引擎边界`，至少冻结：

- 不建设 Kernel 通用 Graph Engine/Service/Store/Query Router。
- Graph Foundation 的准入门、允许项和禁止项。
- DeckTopology、ServiceDependency、Runtime assembly、Agent workflow 和 Inspection projection 的 owner。
- DataLink 不等于 activation dependency。
- P2e cyclic Deck 的保守策略。
- RuntimePlanSlice activation contract 与 RuntimeAssemblyEngine 边界。

本文没有自动创建或接受 ADR。ADR-0005 Accepted 前，不创建 graph package、公共类型或 RuntimeAssemblyEngine 代码；ADR-0004 后续也应明确 DeckTopology 是受 DeckLock digest 覆盖的 directed multigraph declaration，而不是 live execution graph。

ADR-0006 已接受的是实现语言总边界，不是 Graph Foundation 的架构准入。它不占用、替代或重新编号 `ADR-0005`；即使 Rust-first RuntimeHost 开工，Graph package 与 RuntimeAssemblyEngine 仍必须等待本节 ADR-0005 的独立裁决和相应阶段 gate。

## 19. 推荐方向与置信度

**Verdict：revise。** 修订“Kernel 完全不出现任何图能力”的绝对表述，但拒绝“Kernel 建通用 Graph Engine”。采用语言中立的领域模型/领域执行器，真实复用后条件抽取极小 Graph Foundation；若准入，首个 Foundation 与 RuntimeAssemblyEngine mechanism 使用 Rust/Cargo，但领域 workload 保持 polyglot。这样 Deck 获得完整运行闭环而不污染消息热路径，也不把 Rust 实现误升为通用 graph、实时或安全语义。

置信度：

- 通用 Graph Engine 不进入 Kernel：高。
- DeckTopology、ServiceDependency 和 Agent workflow 分开：高。
- DataLink 与 activation dependency 分开：高。
- 需要显式 Runtime assembly owner：高。
- Graph Foundation 只在两个真实消费者后抽取：高。
- 首版 cyclic Deck 默认拒绝：中高，等待首个 feedback control 需求与 ADR。
- `RuntimeAssemblyEngine` 最终名称和内部 API：中；职责边界高，但名称不构成公共兼容承诺。

## 20. 后续入口

- 总体架构：[Kernel、RuntimeHost 与 Core Services](../architecture/kernel-runtime-core-services.md)
- Deck 领域：[CardDefinition、Card 与 Deck](../concepts/card-definition-card-deck.md)
- Runtime 专项：[Runtime 执行模型、调度与恢复](execution-model-scheduling-and-recovery.md)
- 系统缺口：[分布式具身 Agent OS 缺口研究](distributed-embodied-agent-os-gap-analysis.md)
- 实施主计划：[Kernel Foundation](../plans/kernel-foundation.md)
- 验证入口：[Testing](../testing/README.md)
