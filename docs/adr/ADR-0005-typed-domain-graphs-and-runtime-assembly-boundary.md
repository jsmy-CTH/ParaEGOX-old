# ADR-0005 — Typed Domain Graph、Graph Foundation 与 Runtime Assembly 边界

> 状态：Accepted
> 日期：2026-07-31
> 决策者：ParaEGOX workspace user（以 `docs/plans/s7-p2e-baseline-v1.authorization-receipt` 为唯一生效证据）
> 关联文档：[Graph Foundation、领域图与执行边界研究](../research/graph-foundation-and-domain-execution-boundaries.md)、[Kernel Foundation Plan](../plans/kernel-foundation.md)、[Kernel、RuntimeHost 与 Core Services](../architecture/kernel-runtime-core-services.md)、[ADR-0001 — DeploymentController、DeploymentPlan 与 Runtime 边界](ADR-0001-deployment-controller-boundary.md)、[ADR-0004 — Deck 工作负载、DeckLock 与 Application 准入边界](ADR-0004-deck-workload-and-application-admission-boundary.md)、[ADR-0006 — Rust-first 核心机制与多语言工作负载边界](ADR-0006-rust-first-core-and-polyglot-workloads.md)、[ADR-0008 — PXTE v4 / PXAR v5 source-only/empty reference target successor](ADR-0008-pxte-v4-pxar-v5-subject-ingress-separation.md)

## 一句话结论

提议将 `DeckTopology/DataLink`、`ServiceDependency` 与 `activation constraint` 固定为三种不可互换的 typed domain graph；P2e 首版对 cyclic Deck fail-closed，不建设通用 Graph Engine、Store 或持久 Graph Schema，只有两个独立真实生产消费者证明了相同算法需求后才按实际交集抽取 internal `Graph Foundation`；RuntimeHost 内部的 `RuntimeAssemblyEngine` 只消费 authenticated Runtime apply request 中的 canonical target Slice 和 Runtime apply journal，不拥有独立 desired state、identity 或 store，也不进入 steady-state 消息热路径。

## 背景

ParaEGOX 已由 [ADR-0001](ADR-0001-deployment-controller-boundary.md) 将全局 desired plan、revision、rollout 与 reconciliation 交给 Kernel 外、按 DeploymentScope 单写的 DeploymentController，并将本机 PID、Domain、Binding、Instance 与 apply 副作用留给 RuntimeHost。[ADR-0004](ADR-0004-deck-workload-and-application-admission-boundary.md) 冻结了由 DeckCompiler 产生唯一 canonical DeckLock、由受 digest 覆盖的 DeckTopology 表达 Deck 内部结构的候选决策；其是否已生效只由该 ADR 的exact header与authorization receipt决定，本文不另行假定。[ADR-0006](ADR-0006-rust-first-core-and-polyglot-workloads.md) 已接受 Rust-first mechanisms 与 polyglot workloads，但没有接受任何通用 Graph 抽象，也没有授权 RuntimeHost 重建 Deck 或 Deployment 真相。

当前源码已经有版本化 Runtime apply contract、canonical target Slice 的纯 projection/request builder，以及 Loop、Thread、Process 等本地执行机制证据；`governance.toml` 与 Runtime 源码同时明确：这些路径尚未被公开 apply endpoint 消费，durable RuntimeHost journal、DeploymentController 和完整 `RuntimeAssemblyEngine` 仍未实现。仓库也没有已准入的 Deck/Graph package、公共 Graph Schema 或通用 Graph runtime。因此本文冻结的是 P2e 候选边界，不是对现有能力的完成声明。

[Graph Foundation 研究](../research/graph-foundation-and-domain-execution-boundaries.md)发现，相同的 `A → B` 形状不能证明 edge、生命周期、环路、失败或恢复语义相同：

- `DataLink` 表达持续消息流的 Card Port 连接；
- service dependency 表达 provider readiness 和 dependency-loss 义务；
- activation constraint 表达已编译到 target Slice 的本地创建、readiness、开闸与排空顺序；
- Agent workflow、Deployment rollout、Evidence causal ref、World transform 和 Fabric observed topology 又分别有自己的 owner 与状态机。

如果把这些关系 lowering 到一个 `GraphKind + metadata` 模型，再由通用 engine 执行，领域 validator、持久状态和副作用恢复会形成第二份真相。反过来，如果每个领域永久复制 stable ordering、SCC、cycle witness 和 topological batches，也可能积累重复缺陷。P2e 因而需要同时确定 typed owner、保守环策略、条件式纯算法复用门，以及 Runtime 本地装配 owner。

## 范围与非目标

本 ADR 提议决定：

- `DeckTopology/DataLink`、`ServiceDependency` 和 `activation constraint` 三种 typed domain graph 的 owner、用途与禁止混用规则。
- P2e v1 对 cyclic Deck 的 fail-closed admission。
- `Graph Foundation` 的双真实消费者准入门、能力上限和 internal 抽取方式。
- `RuntimeAssemblyEngine` 与 DeploymentController、RuntimePlanSlice、Runtime apply journal、ExecutionDomain 和 steady data path 的边界。
- P2e 中证明上述边界所需的最小实现与故障证据。

本 ADR 不决定：

- `deck.yaml`、DeckLock、DeploymentPlan 或 RuntimePlanSlice 的最终字段名、wire tag、codec 和版本号。
- 接受 ADR-0004，或引入 Application、Installation、ProductRelease、application-owned durable state。
- 通用 Agent WorkflowEngine、OPS saga engine、Graph Service、Graph Store、Graph Query Router 或可写 Graph UI。
- feedback、delay、seed、latest-value、drop/coalesce 或 cyclic backpressure 的公共合同。
- DeploymentController 的多副本共识、跨 Site 原子 rollout 或 P5 的跨 Node activation barrier 协议。
- 将 Rust struct、trait、crate serialization 或 Python object 固定为公共领域 Schema。
- 现在创建 `graph` crate/module、Deck/Service 公共类型、Runtime apply endpoint、durable journal 或 `RuntimeAssemblyEngine` 实现。

## 决策

### 1. 三种 typed domain graph 不共享 edge 语义

P2e candidate baseline 固定以下分型：

| typed relation | 权威输入与 owner | edge/constraint 的含义 | 环策略 | 消费方式 |
| --- | --- | --- | --- | --- |
| `DeckTopology/DataLink` | DeckCompiler 从 DeckSpec 与 resolver inputs 产生，DeckLock 持有 canonical 结构 | 一个 Deck 内 `Card.Out → Card.In` 的 Port、Schema/interaction、DeliveryProfile 和结构关系 | 结构必须能表示并诊断 SCC；v1 没有已接受 feedback contract 时拒绝任何 cyclic Deck | 激活后消息持续经过 PortBinding、Mailbox 和 ExecutionDomain；不是逐 vertex 完成的 workflow |
| `ServiceDependency` | ServiceSpec 声明，DeploymentPlanner 在 immutable plan inputs 上解析与编译 | provider selection、readiness gate、dependency loss、恢复与 consumer-before-provider shutdown 义务 | 必须为 DAG；直接环、长环和自环都在产生可执行 candidate 前拒绝 | Planner 编译 lifecycle obligation，不传递业务 payload |
| `activation constraint` | DeploymentPlanner 从已验证的 Deck/Service/target/policy inputs 编译进 PlanContent.execution，再由 RuntimeSliceProjector 做 canonical target projection | 本 target 的 instance/Domain/Mailbox/Binding 创建、readiness、activation group/barrier、consumer ingress、producer egress、loss action、drain/retire 和 rollback boundary | Runtime 只接受 Slice 已显式表达且版本支持的 group/barrier；不得从 DataLink 或本地观察猜环 | RuntimeAssemblyEngine 仅在一次 apply/replace/stop 中执行本地状态机 |

三类关系必须使用各自的 typed model、validator、diagnostic 和公开 reason code。不得用公共 `GraphEdge {kind, metadata}`、字符串 edge type、opaque parameter map 或共享 mutable node state 表达领域差异。

DeckTopology 是 directed multigraph declaration：同一 Card pair 之间经不同 Port 的 parallel DataLink 必须保留，edge identity 不能只由 source/target Card pair 推导。结构层可以表示 self-loop 和 SCC，以便给出确定性拒绝证据；能表示不代表可运行。

DataLink 不产生启动 topological order。对于 `A.Out → B.In`，安全 activation 通常先建立 B 的 Domain、CardInstance、Mailbox 与 inactive/ready ingress，再开放 A 的 producer egress。ServiceDependency 也不能从 Link 方向推断。activation constraint 是 Planner 对所有 typed inputs 的编译结果，RuntimeHost 不重新解释 DeckTopology。

### 2. P2e v1 对 cyclic Deck fail-closed

在 feedback 合同由后继 ADR 接受前：

- DeckCompiler 必须确定性计算 SCC，并返回具体、稳定的 cycle witness。
- 任何 cyclic Deck 都在 DeploymentPlanner、DeploymentController 和 Runtime 副作用前被拒绝。
- 不回退到声明顺序，不自动插入 buffer/delay/initial token，不把环解释为 retry，也不靠超时碰运气启动。
- 不允许通过 `allow_cycle` 布尔值、feature flag、metadata 或不同实现语言绕过。
- ServiceDependency 环始终在 candidate 产生前 fail-closed；未来 Deck feedback ADR 不自动放宽 service lifecycle DAG。
- RuntimeAssemblyEngine 对 malformed、unsupported 或与 Slice digest 不一致的 activation group/barrier 在 prepare 副作用前拒绝，不自行修图。

未来若允许 cyclic Deck，后继 ADR 至少必须冻结 explicit break semantics、initial seed/no-token 行为、每条边的 bounded Mailbox、overflow/backpressure、deadline、shutdown、failure propagation 和可复现 Harness。

### 3. 不建设通用 Graph Engine、Store 或持久 Graph Schema

Kernel、Runtime、Deployment、Deck、Agent、OPS、Evidence、World 和 Inspection 都不得引入以下共享 owner：

- `execute(arbitrary_graph)`、通用 async scheduler、Graph runtime 或中央 lifecycle loop；
- 公共 `GraphId`、`GraphRevision`、`GraphRunState`、`GraphNodeState` 或 Graph registry；
- 公共 `GraphNode/GraphEdge/GraphDef` 持久 Schema、通用 YAML/JSON loader 或 canonical Graph digest；
- Graph Store、Graph Service、Graph Query Router 或可以 write-back 的联合 GraphView；
- 通用 retry、checkpoint、resume、approval、compensation、fallback、Receipt 或 rollback 策略。

DeckLock digest、DeploymentPlan/Revision、Runtime apply journal、AgentRun、Evidence 和 World revision 继续由各自 owner 定义。Inspection 未来可以联合 owner-specific、只读、有 freshness/redaction 标记的有损投影，但投影不能成为执行、恢复、授权或写回输入。

### 4. Graph Foundation 只能从两个真实消费者的实际交集中抽取

`Graph Foundation` 是条件式内部纯算法能力名，不是本 ADR 自动批准的包名、公共 API、CoreService 或 Schema。即使本 ADR 后续 Accepted，也只有同时满足以下条件才允许在对应 bounded implementation batch 中抽取：

1. 至少两个独立真实生产代码消费者已经各自存在；测试、示例、同一 owner 的 wrapper、只为证明抽象而创建的第二调用点不计。
2. 两个消费者已经用自己的 typed input、validator、golden/property tests 和公开错误语义证明相同算法、stable ordering 与结构事实需求。
3. 只抽取实测重合的纯算法，不先创建 Foundation 再迫使领域适配。
4. 抽取后没有领域 import、opaque metadata、serialization、digest、I/O、线程、Tokio task、执行状态或持久化。
5. property tests 覆盖输入插入顺序、parallel edge、self-loop、SCC、cycle witness，以及被两个消费者共同需要的 topo/reverse batches。
6. 有明确 owner、internal 兼容策略、架构依赖检查和 bounded removal condition。

P2e 首批候选消费者是 DeckCompiler 的 multigraph/SCC/cycle validation 与 DeploymentPlanner 的 ServiceDependency DAG validation。二者必须先落下 typed 行为；若实际交集不足，就不抽取 Foundation。若交集成立，首个参考实现可按 ADR-0006 进入 private/internal Rust leaf crate，但 Rust 类型、trait 或 serialization 不成为 Deck、Service 或 Runtime 的公共合同。

允许抽取的能力上限是调用方提供 stable node/edge/source/target key 的 immutable directed-multigraph view，以及两个真实消费者共同需要的 deterministic iteration、SCC/condensation、cycle witness、DAG topological/reverse batches 或 reachability。算法只报告结构事实；领域 owner 将其映射成自己的 error、policy 与 remediation。

### 5. RuntimeAssemblyEngine 是 RuntimeHost 内部 apply mechanism

`RuntimeAssemblyEngine` 只作为 RuntimeHost 内部、无独立身份的确定性 apply mechanism。RuntimeHost 仍是 PID、Domain、Card/ServiceInstance、Mailbox、Binding、task、thread、process、resource 和本地 lifecycle 副作用的唯一 owner。

它唯一允许消费：

- 已验证 authentication、target、writer tenure proof、temporal constraint、request digest 和 exact-active CAS 的 `RuntimeApplyRequest`；
- request 中携带的 canonical target `RuntimePlanSlice`；
- RuntimeHost 自己的 durable AdmissionState、per-source revision high-water、`writer_fence`、`prepared`、active desired head、live materialization、compatibility binding、recovery action与owned-resource facts；
- Runtime-owned Artifact/config/resource/Domain/Binding ports。

它不得：

- import、查询或缓存 DeckSpec、DeckLock、DeckTopology、ServiceSpec、DeploymentPlan、DeploymentController 或 editable desired state；
- 拥有独立 `AssemblyId`、revision、digest、store、writer、journal 或 recovery loop；
- 从 DataLink、进程发现、Fabric route 或 Card callback 重新计算 placement、provider selection、readiness policy 或 activation order；
- 把一次 ephemeral assembly relation 持久化成可编辑 Runtime graph；
- 成为第二个 Runtime lifecycle owner、daemon、CoreService 或公共 graph executor。

Runtime apply journal 必须至少分离：

- `source_revision_high_water`：记录最高durable admitted source revision，supersede/restart/rollback不降低；
- `writer_fence`：在任何 prepare 副作用前持久记录已接受的最高 writer tenure；
- `prepared`：记录operation、exact request/incoming Slice、expected-active CAS、phase；empty head commit后仍保留exact old Slice/budgets直到retire terminal；
- `active`：只记录canonical committed desired head与operation/result ref，不保存current resource generation；
- `live_materialization/recovery_action/owned_resources`：分别保存current live状态索引、唯一nonterminal internal action和exact resource ledger，并由strict cross-ref invariant防止第二真相；
- `compatibility binding`：保存sequence-1 strict-verified exact canonical `RuntimeBuildDescriptorV1`/system-installer-produced singleton manifest bytes+digests、store-pinned `RuntimeBuildIdentityV1`、active target manifest projection，以及binary只读compiled build/profile/exact single-fixture-entry actual fingerprint；同一installer manifest artifact必须byte-identically进入Runtime initializer和Planner/Controller ingress，Planner不得创建第二manifest truth。Runtime executable digest与Card fixture artifact digest是两个独立identity。restart先在startup-generation mutation前strict验证snapshot/descriptor/manifest/store-pinned identity、binary compiled actual以及active projection的canonical/cross-ref完整性；任一结构损坏或compiled-vs-pinned不匹配都fail-closed且不提供authenticated response。只有这些prevalidation全部成功并durable完成startup-generation/live invalidation后才检查的active-head supported-row/profile/fixture compatibility失败，可在callback前进入apply-disabled的validated operational quarantine并携带exact pinned identity回答authenticated `Indeterminate`。

prepare 失败不得提前改变 active；crash/restart、重复 operation、writer turnover 和 partial activation 必须从 journal 查询并 bounded reconcile、rollback 或 quarantine，不能从 Deck/Deployment store 重建，也不能把 timeout 当作未执行后透明 replay。

### 6. RuntimePlanSlice 携带语义，Runtime 不补写语义

未来任何一般 executable assembly successor的PlanContent.execution和target Slice至少需要无歧义覆盖：

- Instance、Domain、Mailbox、Binding、Artifact/config/resource assignment；
- typed activation dependency 与 readiness gate；
- readiness timeout/failure action；
- activation group/barrier；
- consumer ingress 和 producer egress gate；
- provider loss action；
- drain/retire 顺序、deadline、rollback boundary 与 collateral restart scope；
- source revision/digest、target slice digest、generation/epoch fencing。

字段名、wire 编号和版本迁移仍需在 Accepted 决策后的 contract batch 中冻结。语义一旦进入公共 Slice contract，canonical encoding、unknown-field/version behavior、digest coverage 和 Rust/Python golden vectors 必须保持 language-neutral。

RuntimeSliceProjector 只做 committed plan 到 target Slice 的 canonical projection，不重算领域规则。activation/readiness/loss/drain 规则的任何变化必须改变 PlanContentDigest 和受影响 target slice digest；普通 DataLink 不被静默转换为 ServiceDependency 或 activation topo。

上述列表是一般assembly contract的长期完整性规则，不表示S7已经拥有相应wire字段。S7唯一production candidate/committed Slice由ADR-0008 digest-covered `ReferenceAssemblyProfileV1`和新version-specific `ReferenceLoopDomainSpecV1`/`ReferenceLoopSubjectSpecV1`明确选择；它们不alias旧capacity-bearing `LoopDomainSpec`/`CardSubjectSpec`。`OneSourceLoop`固定一个manifest-pinned compiled-in Loop fixture、零ingress/dependency/egress/effect grant，并由profile固定one lifecycle action以及zero mailbox/dispatch/background-task slots；`EmptyDeactivate`固定canonical empty head、exact CAS、drain/cleanup和exact-zero语义。这些语义是Slice中显式versioned contract，不是Runtime local default或从空字段推导的隐式行为。

PXTE v4没有一般activation、Ingress、Thread/Process或多subject branch；S7的typed graph/planner测试可以验证这些未来需求为什么unsupported，但不能把它们编码成Runtime Slice或committed candidate。未来要commit/执行这些形状，必须由新的digest-covered assembly contract successor同时承载完整target语义并提供真实producer/consumer、failure和migration证据，不能让PlanContent变化却无对应Slice commitment，也不能扩展`ReferenceAssemblyProfileV1` metadata或让Runtime补默认值。

### 7. Assembly 不进入 steady-state 热路径

一般本地状态机候选顺序为：

```text
verify/fence
  → prepare Artifact/config/resource/Domain/Instance/Mailbox/inactive Binding
  → readiness
  → activate consumer ingress
  → active CAS
  → open producer egress last
  → steady state
  → close producer egress
  → drain/retire/rollback
```

进入 steady state 后，Signal、Event、Command 与其他 admitted message 只经过：

```text
PortBinding → Mailbox → ExecutionDomain → CardInstance
```

每条消息不得回到 RuntimeAssemblyEngine、Graph Foundation、DeckCompiler 或 DeploymentController 做 graph traversal、readiness 查询或 scheduling。Assembly 只在 apply、replace、stop、dependency transition 或明确 reconcile action 时推进有界状态。

跨 RuntimeHost rollout 继续由 DeploymentController 协调；每个 RuntimeAssemblyEngine 只执行本 target Slice 并返回本地 facts/Receipt。它不能把多 target partial success 冒充本地原子事务。Rust async cancellation 也不证明外部 effect 已取消；缺少 terminal proof 时仍进入 `Uncertain → query/reconcile`。

S7 reference vertical 不执行上图中的 Mailbox/Binding、consumer-ingress或producer-egress步骤：`OneSourceLoop` 只有 `PreparedNoEffects → FirstActionIntent` durable后才可create LoopDomain/Instance → bounded `on_start` readiness → active commit；`EmptyDeactivate`遇到live/nonzero generation时在empty-head first commit同时写`FirstActionIntent`/`NoNewAdmission`，再bounded stop/join/cleanup → exact-zero，already exact-zero且无action/resource时只走无intent/callback的单事务terminal fast path。RuntimeHost restart时 durable desired active 与 current live readiness分离；journal-bound internal recovery action在新 RuntimeHostEpoch以`RecoveryPlannedNoEffects → StartCallIntent`边界重建reference Loop，不能以同 revision apply retry冒充恢复。该窄路径通过不等于一般 assembly graph已经实现。

### 8. Proposed 状态不授权实现

当本 ADR header 为`Proposed`且authorization receipt未生效时，本文仅记录候选决策。`Proposed` 不满足 `CONTRIBUTING.md` 对公共合同、持久格式、新 owner 或架构边界的 Accepted/明确授权门槛。因此在本 ADR 达到冻结manifest预计算的`Accepted` bytes且对应authorization receipt有效前，不得基于本文：

- 新建 Graph Foundation、Deck/Graph contract 或 RuntimeAssemblyEngine 生产代码；
- 修改公共 RuntimePlanSlice/DeploymentPlan Schema 或 durable journal format；
- 在 `governance.toml` 注册尚不存在的 package/API；
- 把 P2e、Deck run/replace/stop、durable recovery 或 Graph 能力标记为已实现。

接受本 ADR 也只授权按本节边界进入具体 admission；Graph Foundation 仍需双真实消费者门，公共 contract 和持久 journal 仍需同批 producer、consumer、compatibility、migration/removal 与故障证据。

## 备选方案

### 在 Kernel 建设通用 Graph Engine

统一 node/edge、scheduler、retry、checkpoint、store 和 query 的 demo 路径较短，但会让领域语义进入 `GraphKind/metadata`，并与 DeckLock、DeploymentPlan、Runtime journal 和 Agent state 形成平行真相，因此拒绝。

### 各领域永久复制所有图算法

它保留清楚 owner，也适合作为 P2e typed model 的起点，但长期可能复制 deterministic ordering、SCC、cycle witness 和 topo batching 缺陷。本文选择有证据后的条件式 internal 抽取，而不是永久禁止共享。

### 现在先建 Graph Foundation，再寻找消费者

它可能减少首批代码重复，却会由抽象形状反向决定 Deck 和 Service 模型，且没有真实交集与移除证据。本文要求 typed consumer 先行，交集不足时取消抽取。

### 用 DataLink topological order 驱动启动

实现简单，但 producer-to-consumer 数据方向通常与“consumer ingress 先 ready、producer egress 后开放”的安全顺序不同，也不能表达 provider readiness 和 dependency loss，因此拒绝。

### 让 RuntimeHost 查询 Deck/Deployment store 并重算 assembly

这可以减小 Slice，却会建立第二 desired-state reader/编译器，使同一 revision 在不同 RuntimeHost 上被不同观察输入解释。本文选择 canonical Slice-only consumption 和 journal recovery。

### 让 RuntimeAssemblyEngine 成为 steady-state graph scheduler

中央调度看似便于统一观察，但会把每条消息重新串行化到控制路径，重复 Mailbox/Dispatcher/Domain 的 admission 与 ownership，并扩大一个故障域。本文只保留 apply-time assembly。

### 采用 Zenoh Flow、Dora、Flink、Beam 或现成 workflow runtime

它们可以作为 UX、dataflow 或 durable workflow 参考，但原生对象不拥有 ParaEGOX 的 Deployment writer fencing、RuntimePlanSlice、physical effect uncertainty 和 Runtime ownership。当前不引入，也不 fork 为 Kernel；未来 adapter 需要独立 owner 和 contract evidence。

## 后果

收益：

- Deck dataflow、service lifecycle 和 Runtime activation 各有唯一 typed owner，结构相似不再制造语义等价。
- RuntimeHost 保持 Slice-only、本地、机械且可恢复，不反向依赖 Deck 或 Deployment。
- P2e 能先证明 ADR-0008 reference profile 的 one-source-Loop run/recover/empty-stop；一般 replace、ingress/egress/dependency-loss 仍等待后继 executable profile，同时 steady data path 不承担中央 graph traversal 成本。
- 纯算法只有在真实复用后进入 internal Foundation，避免空框架和公共兼容债务。
- cyclic Deck 在 feedback 合同成熟前有稳定、可解释且无副作用的拒绝行为。

成本与限制：

- DeckCompiler 与 DeploymentPlanner 必须先分别实现 typed validator 和测试，短期允许少量算法重复。
- P2e v1 不能运行反馈控制、迭代处理或其他需要 dataflow cycle 的 Deck。
- RuntimePlanSlice 的每个可执行 profile都需要显式、digest-covered activation/readiness/loss/drain语义；S7 只承担两个 fixed reference modes，一般 contract 的兼容和跨语言成本后置到其真实 successor。
- durable apply journal、crash recovery、partial activation 和 quarantine 需要真实故障 Harness，不能只靠 Rust ownership 或单元状态机证明。
- Graph Foundation 的包位置、API 和算法集合在真实消费者出现前不稳定，也不对外提供兼容承诺。

迁移影响：

- 当前没有已发布 Graph Schema、Graph Engine 或 RuntimeAssemblyEngine 持久状态需要迁移。
- 已实现的 P0–P2d contract/mechanism 证据保持有效，但不因此升级为 P2e 完成证据。
- 未来若冻结公共 Slice 或 journal format，替换实现语言仍必须保持同一 canonical contract、authority、fencing 与恢复语义。

## 失败场景与反例

最强反例是：未来 ParaEGOX 产品被明确收窄为一种固定 dataflow appliance，所有独立生产领域在 event-time、state、checkpoint、retry、effect 和 recovery 上都具有同一无损语义，并且采用成熟 dataflow runtime 的 crash/latency/资源证据显著优于 typed engines。只有这种证据足以重新评估通用 engine；“都长得像图”或 UI 想统一展示不够。

第二个反例是 DeckCompiler 与 DeploymentPlanner 落地后，实际需要的 ordering、parallel-edge handling、diagnostic 或 cycle semantics 不同。此时即使两份实现都使用 SCC，也不得为了完成计划而抽取 Foundation；正确结果是保留领域实现，缩小或取消复用。

第三个反例是首个真实产品在 P2e 就必须运行 feedback Deck，并能提供明确 delay/seed、bounded buffer、overflow/backpressure、startup、shutdown 和 fault Harness。此证据不会授权 `allow_cycle=true`；它会触发后继 feedback ADR，并可能将 v1 fail-closed profile 扩展为显式版本化 profile。

第四个反例是 crash Harness 证明 canonical Slice 无法携带安全恢复所需的某个事实。此时应通过后继 contract decision 扩展 language-neutral Slice/journal，而不是让 RuntimeAssemblyEngine 查询 editable Deck/Deployment store 或创建独立 desired state。

若 steady-state benchmark 证明现有 PortBinding/Mailbox/Domain 路径无法满足某个已承诺 SLO，也不能直接把中央 graph loop 放入热路径；必须先定位 admission、routing 或 execution bottleneck，并用后继 ADR 证明新的 owner 和故障边界。

## 实施与验证

以下均是本 ADR 授权后仍需建立的候选实施证据，不是现有功能；`Proposed`期间不得启动受门控实现，达到`Accepted`也不等于这些能力已经完成：

1. DeckCompiler 与 DeploymentPlanner 先分别建立 typed model、validator、稳定 diagnostic 和 golden/property tests；在此之前不创建 Graph Foundation。
2. DeckTopology fixtures 覆盖同一 Card pair 的不同 Port parallel Link、self-loop、duplicate edge key、悬空 endpoint 和随机输入顺序；相同 canonical 输入产生相同 DeckLock、SCC、cycle witness 和 diagnostics。
3. v1 cyclic Deck 的直接环、长环和 self-loop 在任何 Deployment/Runtime 副作用前返回稳定 reason/witness；不回退到声明顺序或隐式 buffer。
4. ServiceDependency 的直接环、长环和 self-loop 在 DeploymentPlanCandidate 产生前失败，并验证 shutdown reverse batches；其公开错误不复用 Deck cycle reason。
5. Planner research/validator反例测试组合`producer → consumer` DataLink与反向service requirement，证明一般activation order只能来自typed activation constraint；S7对此返回stable unsupported而不产生committed candidate或Runtime Slice。
6. activation/readiness/loss/drain rule 的任何变化进入 PlanContentDigest 和对应 target slice digest；S7 `ReferenceAssemblyProfileV1` 的 mode/shape也进入 digest，RuntimeSliceProjector 不做 provider selection、placement 或 graph inference。
7. runtime-contract Rust/Python golden vectors覆盖`RuntimeBuildDescriptorV1`、`RuntimeBuildIdentityV1`、singleton `RuntimeArtifactCompatibilityManifestV1`/projection、`RuntimeApplyEnvelopeV2`与PXTE v4两个exact shape及其manifest/profile/`Reference*V1` shape mismatch、digest和错误reason；legacy capacity-bearing record不能alias/嵌入，语言私有enum/type不越过wire boundary，unknown branch在Runtime副作用前稳定拒绝。
8. RuntimeHost 在没有安装或 import Deck/Deployment domain implementation 时，仅靠 authenticated request、target Slice、journal 和 Runtime-owned ports完成 `OneSourceLoop` prepare/readiness/activate/restart reassembly与 `EmptyDeactivate` drain/cleanup；不将其声明为一般 assembly。
9. 在 writer-fence/revision-high-water persist、Instance/LoopDomain create、readiness、active CAS、RuntimeHost restart recovery action和empty drain各点注入 crash；旧 desired active或canonical empty head按状态机保留，live readiness单独恢复，重复 request/旧 writer/旧 revision不产生 mixed revision或双 active generation。Artifact、Mailbox/Binding、producer-egress等一般阶段等待其可执行 successor后再补对应故障证据。
10. 相同 operation id + canonical request digest 的 retry 只查询或推进同一 journal operation；相同 id 不同内容拒绝，timeout 与 writer turnover 不透明 replay。
11. prepare 或 partial activation 失败后能 bounded rollback 或 quarantine；停止后 Task、Thread、Process、FD、workspace、Mailbox、Binding 和 retained bytes 的 ownership/census 与实际 profile 相符，不伪造 cleanup proof。
12. S7以dependency/call-graph guard证明idle `OneSourceLoop` lifecycle没有把RuntimeAssemblyEngine、Graph Foundation、DeckCompiler或DeploymentController放进任何steady callback loop；该profile没有message、Command、Binding、input或tick，不能把它冒充streaming压力证据。只有未来binding-bearing executable successor拥有真实production producer/consumer后，才要求steady streaming/Command压力测试证明消息只经过其PortBinding/Mailbox/ExecutionDomain且assembly/control-plane不进入per-message调用图。
13. 只有 DeckCompiler 与 DeploymentPlanner 都成为独立生产消费者且算法交集成立时，才同批抽取 internal Foundation；抽取前后领域 canonical output/diagnostic 等价，architecture checks 禁止领域 import、I/O、serialization/digest、Tokio task 和 execution state。
14. Foundation property tests 覆盖实际抽取的 stable ordering、parallel edge、self-loop、SCC、cycle witness 和 topo/reverse batches；删除 Foundation 可在一个 bounded change 内恢复领域实现，不迁移产品数据。
15. one-subject Deck system Harness 走完整 `DeckSpec → DeckLock → candidate → commit → Slice → authenticated apply → observe → deactivate`，但只声明单 Node P2e reference evidence，不冒充 P5 跨 Node rollout、production sandbox 或 physical safety。

受影响的候选组件包括 DeckCompiler/DeckLock owner、DeploymentPlanner/PlanContent.execution、RuntimeSliceProjector、runtime contracts、RuntimeHost apply journal 与内部 RuntimeAssemblyEngine。每个公共 contract、持久 journal、package 和 executable 仍需按 `CONTRIBUTING.md` 在实现批次登记 owner、producer、独立 consumer、compatibility、migration/removal 和 first functional test。

回滚策略是移除尚未冻结的 internal Foundation、停用未通过 Harness 的 P2e apply profile，并保留上一 verified Runtime contract/mechanism；不得回退到手写 Slice、声明顺序启动、Runtime 查询 Deck store 或 local/wire 双执行路径。

## 后继与替代

本 ADR 不替代 [ADR-0001](ADR-0001-deployment-controller-boundary.md)、[ADR-0004](ADR-0004-deck-workload-and-application-admission-boundary.md) 或 [ADR-0006](ADR-0006-rust-first-core-and-polyglot-workloads.md)。它补充三者尚未共同冻结的 typed graph、条件式算法复用和 Runtime local assembly 边界；ADR-0004 仍须独立接受，才能把其 Deck/DeckLock 提案作为 P2e 公共领域合同实施。

首个 cyclic Deck profile 必须由 feedback/delay/seed/backpressure ADR 后继；Graph Foundation 若要成为公共 API、Kernel owner 或持久 Schema，需要新的 superseding ADR 和真实外部消费者；Agent durable workflow、OPS compensation engine、Inspection GraphView 与跨 Node activation barrier 分别等待自己的领域证据和决策。

若未来证据满足“失败场景与反例”中通用 dataflow/workflow engine 的严格条件，后继 ADR 可以 supersede 本文相应禁止项，但不得静默转移 DeckLock、DeploymentPlan、Runtime journal、AgentRun、Evidence 或 World 的现有 owner。
