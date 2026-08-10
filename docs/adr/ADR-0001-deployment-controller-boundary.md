# ADR-0001 — DeploymentController、DeploymentPlan 与 Runtime 边界

> 状态：Accepted
> 日期：2026-07-29
> 修订：2026-07-29，补充命名纪律、OPS 关系和 writer 类型限定；Deck 内部模型交由 ADR-0004 提议
> 决策者：ParaEGOX maintainers
> 关联文档：[ADR-0003 — OPS、OpsService 与 Inspection 操作边界](ADR-0003-ops-service-operation-boundary.md)、[ADR-0004 — Deck 工作负载、DeckLock 与 Application 准入边界](ADR-0004-deck-workload-and-application-admission-boundary.md)、[Kernel、RuntimeHost 与 Core Services](../architecture/kernel-runtime-core-services.md)、[分布式系统模型](../architecture/distributed-system-model.md)、[Kernel Foundation Plan](../plans/kernel-foundation.md)

## 一句话结论

ParaEGOX 保留 `DeploymentController` 作为 Kernel 之外、按 DeploymentScope 单写的分布式控制面 owner：它拥有全局 desired `DeploymentPlan` 与 `DeploymentRevision`，向目标 RuntimeHost 投影 `RuntimePlanSlice` 并构造 authenticated apply request；P0–P2e 立即冻结、实现并验证这条闭环，复杂 placement、HA、共识和跨 Site 自动恢复后置。

## 背景

EAGOS 的 Runtime、BundleLoader 和 Bus 同时承担应用解析、进程内外放置、启动顺序、消息投递、生命周期恢复和领域服务装配。ParaEGOX 必须把“系统希望运行什么、放在哪里”与“本机如何可靠执行”分开，避免 RuntimeHost 再次成为全局编排器。

一个来源中立、面向单 RuntimeHost 的 `RuntimePlan` 足以启动本地实例，却不能独自拥有以下跨节点不变量：

- 一次 DeckRun/Service rollout 的全局 revision 与唯一 desired state；
- Link 两端、Fabric route、Schema、BindingId 和 target admission 的一致编译；
- 多 Node placement、资源冲突和 Service provider 选择；
- prepare/activate/drain/rollback 在多个 RuntimeHost 之间的协调；
- Node 断连、重启或 observed state 漂移后的 reconciliation；
- OpsService 提交的 deployment operation 与自动恢复之间的单写者、幂等和审计边界。

如果让 CLI、DeckCompiler、OpsService 和各 Node 分别直接签发完整本地计划，这些来源可以各自合法却彼此冲突。冲突最终只能由 Runtime、Fabric 或人工脚本隐式解决，形成新的结构债务。因此，对于明确以 distributed-first 为目标的 ParaEGOX，全局 deployment authority 不能被删除或推迟到契约之外。

另一方面，当前没有证据支持立即建设多副本 DeploymentController、leader election、复杂调度器或强原子跨 Site 事务。必须保留控制面 owner，但不能把 Kubernetes 的全部实现复杂度提前搬入首版。

## 范围与非目标

本 ADR 决定：

- DeploymentPlanner、DeploymentController、RuntimePlanSlice 与 RuntimeHost 的所有权和依赖方向。
- P0–P5 中哪些控制面契约和行为必须立即实现，哪些可以后置。
- DeploymentController 的单写者、故障域和 bootstrap 边界。

本 ADR 不决定：

- 最终 placement 算法、HA 存储、leader election 或跨 Site 共识协议。
- DeploymentController 的产品 UI、API transport 或最终部署拓扑。
- Deck、Artifact、Authority、Safety、Evidence 等领域内部实现。

## 决策

### 1. 权威链

```text
resolved Deck input + ServiceSpec + target Node facts / policy
                                      │
                                      ▼
                          DeploymentPlanner（纯、确定性）
                                      │ DeploymentPlanCandidate
                                      ▼
                           DeploymentController
                 atomic commit: allocation + revision + plan
                                      │ canonical target projection
                                      ▼
                              RuntimePlanSlice
                                      │ authenticated RuntimeApplyRequest
                                      ▼
                                 RuntimeHost
```

本 ADR 只要求 Planner 接收已经解析、验证且不可变的 Deck 输入，不拥有其内部 Schema。DeckSpec、DeckCompiler、DeckLock 与 DeckTopology 的候选关系由 Proposed [ADR-0004](ADR-0004-deck-workload-and-application-admission-boundary.md) 单独评审；无论其最终形式如何，都不能让 source editor 或 resolver 成为第二个 DeploymentRevision 写 owner。

`DeploymentPlanner` 负责确定性计算；为了让同一逻辑 Link/Instance 跨 revision 保持稳定 ID，它只消费 DeploymentController 提供的不可变 previous-plan/allocation snapshot，不自行持久化或从 observed state 猜 ID。`DeploymentController` 负责 desired-state ownership、stable desired ID allocation、revision、发布协调和 reconciliation。两者必须分开，不能让 DeploymentController 同时成为 Deck resolver、Artifact installer、RuntimeHost lifecycle/recovery owner、Authority、Fabric 或 OpsService ControlRequest journal owner。

Planner 的输出与已提交计划必须分型：

- `DeploymentPlanCandidate` 是 Planner 的不可变输出，包含同级字段 `PlanContent`、stable-ID allocation delta、diagnostics 和 `PlanContentDigest`；PlanContentDigest 只覆盖 PlanContent，allocation delta 与 diagnostics 均不进入该 digest。Candidate 不带权威 DeploymentRevision、writer tenure、rollout 或 observed facts。
- DeploymentController 验证 candidate 后，在一个 crash-consistent transaction 中原子提交 stable-ID allocation delta、分配下一 `DeploymentRevision`，并构造不可变的 committed `DeploymentPlan`。
- committed `DeploymentPlan` 至少包含 DeploymentScopeId、DeploymentId、DeploymentRevision、PlanContent 与 PlanContentDigest；`DeploymentPlanDigest` 覆盖这些 canonical 字段，但不覆盖 DeploymentWriterRef/DeploymentWriterEpoch、tenure proof、operation id、deadline、rollout ledger、status 或 observed facts。
- 相同 Planner 输入只要求产生字节稳定的 `DeploymentPlanCandidate/PlanContentDigest`；相同 candidate 加相同已分配 plan header 才产生相同 committed `DeploymentPlanDigest`。DeploymentController 重启只推进 writer tenure，不改变 plan revision 或 digest。

### 2. DeploymentController 的唯一职责

每个 `DeploymentScope` 同一时刻只有一个有效写 owner。DeploymentController：

- 接收已经解析和验证的 DeckLock、ServiceSpec、目标 Node facts 与 policy 输入；
- 向 Planner 提供不可变 previous-plan/stable-ID allocation snapshot，并原子提交新的 desired ID allocation；
- 调用 Planner 生成不可变 DeploymentPlanCandidate；
- 在同一事务中提交 allocation delta、分配并推进 DeploymentRevision，并封装 committed DeploymentPlan；
- 将计划规范投影为每个目标 RuntimeHost 的 RuntimePlanSlice；
- 通过 authenticated CAS apply 协调 prepare/activate/drain/retire/rollback；
- 比较计划与 NodeDaemon/RuntimeHost/Fabric observed facts 并产生 reconciliation decision；
- 输出结构化 Deployment status、decision 与 Receipt。

DeploymentController 不：

- 直接创建线程、进程、Mailbox、Zenoh Session 或 CardInstance；
- 直接打开设备或执行物理 Command；
- 签发 CapabilityGrant、Lease 或 SafetyDecision；
- 保存 Memory、World、Evidence、Artifact 内容或业务状态；
- 通过类名、字符串或运行时猜测改变已经编译的 ExecutionDomain；
- 在失去合法单写者 authority 后继续发布 revision。
- 充当 Product/Application resolver、安装目录或应用私有状态 owner。

未来即使引入 Deck 之上的 Product Application/Installation，它也只能向本权威链提供已解析的不可变输入，不能创建 `ApplicationController` 并行提交 DeploymentRevision、调用 RuntimeApplyEndpoint 或 reconcile 相同资源。

### 2.1 名称与内部实现边界

保留 `DeploymentController` 全称，因为该组件同时拥有每个 DeploymentScope 的单写提交、committed plan/revision、rollout 与 desired/observed reconciliation。`DeploymentReconciler`、`DeploymentCoordinator`、`DeploymentManager` 或 `DeploymentDirector` 都只覆盖其中一部分或弱化唯一 desired-state owner 的含义。

公共文档、API、status、log 和 metric 不得用裸 `Controller` 指代 DeploymentController。deployment 领域中的 writer identity/tenure 分别叫 `DeploymentWriterRef` 与 `DeploymentWriterEpoch`；Runtime 消费侧继续使用 `PlanWriterRef` 与 `PlanWriterEpoch`，避免 Runtime import deployment 领域类型。候选进程名使用 `paraegox-deploymentd` 或等价带 deployment 限定的名称，不使用 `controllerd`；具体可执行文件名在实现时冻结。

DeploymentController 内部可以调用无独立身份和持久状态的纯组件：

- `DeploymentReconciler`：根据 immutable desired/observed snapshot 产生 reconcile decision；
- `DeploymentRolloutEngine`：根据 rollout state 和 policy 产生下一组有界 action；
- `RuntimeSliceProjector`：产生 tenure-neutral target Slice；
- `RuntimeApplyEnvelopeBuilder`：将 deployment writer tenure 映射为 runtime-owned request。

这些 helper 不拥有进程、ServiceSpec、journal、tenure、I/O、重试循环或第二份 write authority。DeploymentController 仍独占持久 commit、rollout ledger、I/O、查询、重试与最终 reconcile 决策。只有当外部 owner 已拥有 committed plan/revision、ParaEGOX 组件仅 fan-out 时，顶层角色才适合改叫 `DeploymentCoordinator`；若组件只读取既有 desired state 并给出收敛建议，才适合叫 `DeploymentReconciler`。任何这类拆权或改名都需要 superseding ADR，不能静默 alias。

### 3. RuntimePlanSlice 与 RuntimeApplyRequest

committed `DeploymentPlan` 是 deployment scope 的唯一 desired truth；`RuntimePlanSlice` 是其面向一个 RuntimeHost 的不可编辑、可验证 projection，不是第二份 desired state。

`runtime/contracts` 拥有 Slice Schema 和 apply protocol，使 Runtime 无需 import `deployment/`。Slice 至少携带：

- target RuntimeHost；
- consumer-owned `PlanProvenance` wire DTO：opaque source scope/plan refs、source revision 与 source plan digest；
- target slice digest；
- Instance、Artifact entrypoint 与 immutable ConfigSnapshot ref；
- Binding、Domain、Mailbox、budget、LivenessSpec、FailureContainmentSpec、RecoveryPolicy 和 revision transition assignment。

writer tenure 不进入 Slice，也不改变 plan/slice digest。`runtime/contracts` 另定义 `PlanWriterContext`，包含 `PlanWriterRef`、`PlanWriterEpoch` 与 `WriterTenureProof`。`RuntimeApplyRequest` 绑定 Slice、PlanWriterContext、target、exact expected-active target-slice digest、operation id、temporal constraint 和覆盖完整 canonical request 的认证/完整性证明；认证 principal 必须与 `PlanWriterRef` 相符。RuntimeHost 在任何副作用前验证 target、revision、digest、authority 和 CAS；exact slice CAS 与 source revision 单调性是两条独立不变量，它只能执行或拒绝计划，不能改写 placement 和 policy。

Runtime 不直接使用或 import `DeploymentScopeId`、`DeploymentRevision`、`DeploymentWriterRef`、`DeploymentWriterEpoch` 领域类型。`runtime/contracts` 定义消费侧 `SourceScopeRef`、`SourcePlanRevision`、`PlanProvenance`、`PlanWriterRef`、`PlanWriterEpoch`、`WriterTenureProof` 与 `PlanWriterContext` wire value；Runtime 只执行协议规定的作用域、顺序、CAS 与 fencing。纯 `RuntimeSliceProjector(committed_plan, target)` 只生成 tenure-neutral Slice；纯 `RuntimeApplyEnvelopeBuilder(slice, writer_context, expected_active, operation_id, temporal_constraint)` 再把 deployment writer tenure 规范映射为 runtime-owned request。这样 DeploymentController 重启可以签发相同 plan/slice 的新 tenure request，而不建立 `runtime → deployment` 反向依赖。

Digest 覆盖面固定如下：

- `PlanContentDigest`：只覆盖 candidate 的 canonical desired content。
- `DeploymentPlanDigest` / wire `source_plan_digest`：覆盖 committed plan 的 scope、plan identity、revision、content digest 与 exact content；排除 writer tenure 和 rollout/observed state。
- `target_slice_digest`：覆盖 target、PlanProvenance 和该目标的 canonical assignment；排除 PlanWriterContext、expected-active、operation id、temporal constraint、proof 和 request authentication。
- request authentication：签名/认证输入覆盖以上 digest、完整 Slice、完整 PlanWriterContext、exact expected-active target-slice digest、operation id 与 temporal constraint，但排除认证值自身；防止把合法 proof 或 Slice 拼接到另一请求。`WriterTenureProof` 的 envelope fingerprint 包含 signature bytes，只用于标识完整 proof envelope；签名转录必须是另一份不包含 signature value 的 canonical contract。

### 4. Kernel 边界

Deployment 不进入 Kernel：

```text
deployment/ ───────> runtime/contracts ───────> kernel
runtime/     ────────────────────────────────> kernel
deployment/planner ───────> decks/contracts
```

- `DeploymentPlan`、`DeploymentRevision`、placement 和 reconciliation 属于 `deployment/`。
- `RuntimePlanSlice`、`PlanProvenance`、`PlanWriterContext`/`WriterTenureProof` wire DTO、`RuntimeApplyRequest` 和 apply state machine 属于 `runtime/contracts`。
- Kernel 只拥有通用 ID/digest、time、Failure、Message、Receipt、Grant、Lease 与 fencing 原语；不定义 DeploymentPlan、DeploymentController 或 DeploymentRevision。
- Runtime 不 import Deck、DeploymentPlan、DeploymentPlanner 或 DeploymentController；DeploymentController 作为 Slice producer 依赖 Runtime 的消费契约。

### 4.1 DeploymentScope 与 writer fencing

- `DeploymentScope` 是 deployment desired-state 的写权边界，不是 Site、TrustDomain、SafetyDomain 或 Robot。P0–P5 reference profile 限制为一个 RuntimeHost 只接受一个 active DeploymentScope；多 tenant/scope 共用同一 RuntimeHost 需要新 ADR 与 namespace/resource-conflict 证明。
- `DeploymentRevision` 在一个 scope 内单调推进；DeploymentController 重启本身不伪造新的 plan revision。
- `DeploymentWriterEpoch` 表示该 scope 的 writer tenure。每次 DeploymentController 进程启动、重新取得写权、人工 takeover 或恢复成为 active writer 前，都必须从 `DeploymentTenureAuthority` 取得新 tenure；authority 在 crash-consistent store 中原子推进 epoch，不能回退或复用旧 tenure。
- `DeploymentTenureAuthority` 是 writer-tenure proof 的唯一签发 owner，不是 Kernel、RuntimeHost、DeploymentController 或 OpsService。P2e reference profile 用 OS service manager 独立托管、OS lock 保护的本地 authority adapter：其 signing key 不暴露给 DeploymentController，原子 `acquire_tenure(scope, deployment_writer_ref)` 后签发 `WriterTenureProof`。RuntimeHost bootstrap trust 配置只包含 authority ref、验证 key 与允许 scope，不包含 signing key。
- `WriterTenureProof` 至少绑定 authority/key/algorithm version、SourceScopeRef、PlanWriterRef、PlanWriterEpoch、`supersedes_through_epoch`、nonce 和 signature；不依赖跨主机 wall-clock 排序。正常重启也必须取得新 proof。RuntimeHost 可接受任何由受信 authority 签发且 epoch 高于本地 durable fence 的 proof，即使它离线期间跳过多个 epoch。
- RuntimeHost 在验证 request principal、proof 和 takeover authority 后，于任何 prepare 副作用前持久保存每个 `SourceScopeRef` 的最高 `PlanWriterRef + PlanWriterEpoch + proof-envelope digest`。低 epoch 请求拒绝；同 epoch 只允许相同 `PlanWriterRef`、proof-envelope digest 和认证 principal，并遵守 exact-active-slice CAS；高 epoch 请求必须携带合法 proof。
- RuntimeHost journal 必须分开三类状态：`writer_fence` 保存最高 writer tenure；`prepared` 保存 operation id、incoming source revision/digests、expected-active 与阶段；`active` 只保存已成功 activate 的 source revision/digests。接受新 tenure 先推进 writer_fence，并将不能继续的旧 prepared operation 原子写为 `Superseded`；prepare 只写 prepared，只有 exact target-slice CAS 与 revision 单调性均通过时 activate 才原子替换 active。prepare 失败或 DeploymentController crash 绝不能提前把 incoming revision 标成 active。
- `operation_id` 在 SourceScopeRef + target 内唯一。同一 operation id 与相同 canonical request digest 只能查询或推进同一 durable journal operation；相同 id 携带不同 plan/slice/CAS/temporal constraint 必须拒绝为 conflict。writer turnover 后旧 operation 的 durable terminal/prepared history 仍可按 operation identity 查询；新 DeploymentController 必须先 reconcile，再用新 operation id 发起新的副作用尝试，不能靠换 epoch 隐式重放旧 operation。
- DeploymentController 与 RuntimeHost journal 任一无法证明最高 epoch/revision 时 fail-closed 或进入 quarantine，不能从零计数。旧 DeploymentController 的晚到 apply、Receipt 与 observed reply 都由 writer epoch/revision fencing。

### 5. 分阶段实现

- **P0/P1**：冻结 deployment/runtime import boundary、DeploymentPlan/Revision provenance、RuntimePlanSlice/apply Schema；不实现 HA。
- **P2**：RuntimeHost 使用由真实 projector conformance fixture 产生的 Slice，证明事务 apply、bounded execution 和 revision fencing。
- **P2e**：实现最小单 Node DeploymentPlanner + single-writer DeploymentController reference，至少完成 plan、project、apply、observe、reconcile-once；开发模式可以由 CLI 驱动，但不能绕过 DeploymentController 语义。
- **P3/P4**：仿真物理闭环与 Zenoh Fabric 使用同一 DeploymentRevision/Binding projection，不建立第二条手工配置路径。
- **P5**：加入 NodeDaemon facts、Runtime endpoint discovery 和双主机持续 reconciliation；验证分区、重连、stale revision、部分 apply 与 DeploymentController restart。NodeDaemon 不是 RuntimeApplyRequest 的 admission gate。
- **后续**：只有真实 availability/Site 需求出现后才选择持久 HA store、leader election、scope sharding 和跨 Site 协调。

P2a–P2d 先用 production `RuntimeSliceProjector`/`RuntimeApplyEnvelopeBuilder` 生成的 conformance fixture 建立可被 apply 的执行目标，不是建立临时 desired-state owner。P2e 必须在 P3 前把 fixture producer 接回真实 DeploymentController journal 闭环；这只是依赖排序，不是搁置 DeploymentController，也不允许留下 CLI/手写 Slice 的生产旁路。

### 6. Bootstrap 与失联

生产 DeploymentController 不能作为由同一个目标 RuntimeHost 托管管理的普通 CoreService，否则形成自举和共同故障域。它由 OS service manager、独立 control-plane Runtime 或外部平台进程物理托管；物理托管不把 plan/revision/reconcile 的语义所有权转移给 OpsService。单机开发可使用独立 CLI/process reference profile。

DeploymentController 暂时不可用时，RuntimeHost 继续执行最后一次已激活且未过期的 Slice，具体允许动作受 Continuity/Safety policy 约束；它不得自行生成新 placement 或 revision。DeploymentController 恢复后先读取 observed facts并 reconcile，不能把 timeout 直接解释为 apply 未发生。

OpsService 只能向 DeploymentController 提交带 request identity、expected revision、authority/approval reference 与 deadline 的 deployment operation，并通过公开 status/Receipt 观察结果；它不能写 plan store、生成 RuntimePlanSlice、调用 RuntimeApplyEndpoint 或复制 reconcile loop。OpsService、ConsoleGateway 或 TUI 故障时，DeploymentController 继续运行。P2e 的窄开发/救援 CLI 可以绕过尚未实现或不可用的 OpsService，但不能绕过 DeploymentController、tenure proof 或 Runtime fencing。完整运维边界见 [ADR-0003](ADR-0003-ops-service-operation-boundary.md)。

## 备选方案

### 来源中立的独立 RuntimePlan

本地开发最简单，但多个 target plan 之间没有 owner 保证 Binding 两端、全局 revision、资源和 rollout 一致性。未来补 DeploymentController 时还要迁移 provenance、CAS 和 plan ownership，因此不采用为系统权威模型。

### RuntimeHost 直接读取 Deck

会迫使 Runtime 理解 Deck/Card resolution、Artifact、placement 和 policy，重新形成 EAGOS 式大 Runtime，因此拒绝。

### 立即建设高可用分布式 DeploymentController

能够提前处理 DeploymentController 故障，却会在没有 workload 和 availability 证据时引入共识、状态存储和运维负担。保留单写者协议和 DeploymentWriterEpoch seam，但后置 HA 实现。

### 完全依赖外部 Kubernetes/GitOps

可作为未来 DeploymentController backend 或 DeploymentIntent 输入源，但不能假设其原生理解机器人 Binding、ExecutionDomain、Safety、Continuity 和 physical Receipt，也不能绕过 DeploymentController 直接写 RuntimeHost。外部平台必须通过本 ADR 的 deployment/runtime 契约接入，不能替代语义 owner。

## 后果

收益：

- 从第一版就有全局 desired-state、revision 和跨 RuntimeHost 协调 owner。
- RuntimeHost 保持本地、机械、可验证，不需要理解 Deck 或全局 placement。
- P2 fixture 与 P5 分布式 DeploymentController 共用同一 Slice/apply 契约，减少后期破坏性迁移。
- 可以先实现单写者 reference，再按证据扩展 HA，而不改变职责边界。

成本：

- P0/P1 需要同时冻结 deployment provenance 和 runtime apply contract。
- P2e 增加最小 DeploymentPlanner/DeploymentController/RuntimeSliceProjector 的真实生产者，不能只靠长期手写 fixture。
- 必须处理 DeploymentController authority、epoch、幂等、partial apply 和 `Uncertain`。

## 失败场景与反例

如果实测产品永远只有单 Node、静态启动且不需要自动 rollout/reconcile，则持久 DeploymentController 可能比 CLI 带来更多成本；此时可以使用 single-shot DeploymentController profile，但仍保留 Plan/Revision/Slice 语义，以免 Runtime 接管上层职责。

如果一个 DeploymentController scope 成为瓶颈或单点，优先按 DeploymentScope 分片并引入显式 ownership transfer；不能让多个无协调写者同时向同一 RuntimeHost 发布 revision。

## 实施与验证

- Planner 对相同 canonical 输入产生字节稳定的 DeploymentPlanCandidate 与 PlanContentDigest；DeploymentController 的 commit transaction 原子保存 allocation delta、revision、committed plan 和 DeploymentPlanDigest。
- RuntimeSliceProjector 对相同 committed plan + target 产生稳定、tenure-neutral 的 Slice；RuntimeApplyEnvelopeBuilder 对相同 Slice + WriterContext + CAS inputs 产生稳定 request，所有跨 target Binding/Service dependency 都能回溯到同一 DeploymentRevision。
- RuntimeHost 在未安装/import `deployment` 和 `decks` 时仍能消费序列化 Slice。
- 错误 target、source revision/digest、slice digest、writer ref/epoch/proof、auth、deadline 和 expected-active CAS 在副作用前被拒绝。
- DeploymentController 正常重启取得新 tenure 后，plan revision/source digest/slice digest 保持不变；RuntimeHost 先持久推进 writer_fence，active 只在 activate 成功后改变。
- 双 RuntimeHost 的 prepare/activate 部分失败产生可解释状态，DeploymentController 不报告全局 Ready，并能安全 rollback 或 quarantine。
- DeploymentController 重启、重复 apply、timeout 后查询和 stale observed report 不产生双 active route 或 revision 回退。

## 后继与替代

未来若引入多 DeploymentController 共识、外部 GitOps authority、仅 fan-out 的 DeploymentCoordinator 或只读 desired state 的独立 DeploymentReconciler，需要 superseding ADR，但不得破坏本 ADR 的 Kernel/Deployment/Runtime 所有权边界，也不得以 alias 静默转移 plan/revision 写权。
