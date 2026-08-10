# ADR-0008 — PXTE v4 / PXAR v5 source-only/empty reference target successor

> 状态：Accepted
> 日期：2026-07-31
> 决策者：ParaEGOX workspace user（以 `docs/plans/s7-p2e-baseline-v1.authorization-receipt` 为唯一生效证据）
> 关联文档：[S7/P2e 准入与交付决策包](../plans/s7-p2e-admission-and-delivery-plan.md)、[ADR-0001 — DeploymentController、DeploymentPlan 与 Runtime 边界](ADR-0001-deployment-controller-boundary.md)、[ADR-0005 — Typed Domain Graph、Graph Foundation 与 Runtime Assembly 边界](ADR-0005-typed-domain-graphs-and-runtime-assembly-boundary.md)、[ADR-0006 — Rust-first 核心机制与多语言工作负载边界](ADR-0006-rust-first-core-and-polyglot-workloads.md)、[ADR-0007 — P2e reference journal 与 crash recovery 基线](ADR-0007-p2e-reference-journal-and-crash-recovery.md)、[CONTRIBUTING](../../CONTRIBUTING.md)

## 一句话结论

提议以additive PXTE v4/PXAR v5只公开真实端到端消费的narrow reference grammar：digest-covered target compatibility manifest projection、`ReferenceAssemblyProfileV1`，以及exact zero/one `ReferenceLoopDomainSpecV1`+`ReferenceLoopSubjectSpecV1`；它只表达`OneSourceLoop`和`EmptyDeactivate`，使无ingress的source-only subject与认证empty desired target可canonical表示而不由Runtime猜默认值。Thread/Process、Thread executor budget、ExecutionIngress与一般activation schema不进入v4 public wire，等待各自真实producer/consumer的新successor；PXTE v1–v3、PXAR v1–v4既有bytes、digest、reason code与严格无fallback行为全部不变。

## 背景

仓库当前实现的执行合同按版本逐层增加 Domain kind：

- PXTE v1 的 `TargetExecutionPlan` 同时要求至少一个 `LoopDomainSpec` 和一个 `MailboxExecutionSpec`。
- PXTE v2 的 `TargetExecutionPlanV2` 可以嵌入 byte-exact PXTE v1，但自身仍同时要求至少一个 `ThreadDomainSpec` 和一个 `ThreadMailboxExecutionSpec`。
- PXTE v3 的 `TargetExecutionPlanV3` 可以嵌入 byte-exact PXTE v2，但自身仍同时要求至少一个 `ProcessDomainSpec` 和一个 `ProcessMailboxExecutionSpec`。
- 三个版本都把 subject/workload requirements 放进对应的 Mailbox execution record；一个 subject 因而不能在没有 inbound Binding/Mailbox 的情况下获得执行授权。
- PXAR v2、v3、v4 分别携带 PXTE v1、v2、v3。各版本 decoder 只接受自己的 exact version，并明确不向旧版本 fallback。

这些是 `paraegox-runtime-contracts` 及其 Rust/Python contract tests 已实现并发布的事实，不是可原地重解释的草稿。

S7/P2e 的最小闭环需要表达两个现有合同无法表达的 target state：

1. revision 1 只有一个 trusted、manifest-pinned compiled-in Rust Loop fixture，没有 In/Out、Link、Mailbox 或 Binding；它仍需要一个明确的执行 Domain、workload selection与lifecycle policy。
2. revision 2 对同一 target 提交 authoritative empty desired state，以经过认证、revision、writer fencing 和 CAS 约束的方式 deactivate、drain 并 retire revision 1。

本ADR中的`source-only`只表示结构上没有inbound ingress/Binding，并不承诺该reference fixture会持续采样或发出output；`OneSourceLoop`的可执行语义仍以第4节的idle lifecycle限制为准。

为 source-only subject 制造 synthetic Binding/Mailbox 会让不存在的 ingress 进入 digest、容量、backpressure、cleanup 和 owner census。把 target 从 rollout 中省略又不能证明“期望为空”：省略可能只是没有对该 target 发起 apply。修改 PXTE v1–v3 的非空规则或既有 record 解释则会破坏 canonical bytes、digest 和跨语言向量。因此需要新版本，而不是修补旧版本。

## 范围与非目标

本 ADR 提议决定：

- PXTE v4 narrow public grammar 的 target compatibility manifest projection、`ReferenceAssemblyProfileV1`、zero/one `ReferenceLoopDomainSpecV1` 与 zero/one `ReferenceLoopSubjectSpecV1`；
- digest-covered `ReferenceAssemblyProfileV1` 的两个精确 shape、readiness/failure/activation/drain/cleanup 语义，以及其他 shape 在 S7 Runtime 的 fail-closed 边界；
- source-only one-subject target 和 zero-domain/zero-subject/zero-binding empty target 的合法形状；
- empty target 与 omitted target 的不同控制语义；
- PXTE v4 / PXAR v5 的 canonical ordering、fixed bounds、strict decoder 和兼容边界；
- PXAR v5 request-level `expected_runtime_store_instance_id`的authenticated binding，使延迟旧request不能跨Runtime store重放；
- Rust internal-first contract implementation、真实 vertical promotion 与独立 Python fixture 的最低 evidence；
- producer、consumer、迁移、downgrade 和 rollback 约束。

本 ADR 不决定：

- 最终 binary header、field offset、record byte size、enum 数值、reason-code 数值或 digest-domain 字符串；
- Thread/Process record、target-level Thread executor budget、ExecutionIngress或一般activation/dependency/egress wire；这些不以zero-only placeholder或未消费branch预留进v4 public API；
- DeckSpec、DeckLock、DeploymentPlan 或 journal 的最终 public Schema；
- RuntimeAssemblyEngine 的完整 prepare/activate/drain 算法；
- 新的 Domain kind、C++/WASM worker、通用 Workflow/Graph engine、Fabric、NodeDaemon 或跨 Node rollout；
- ProcessDomain 的 production containment、物理 effect reconciliation 或硬件 capability；
- 将计划中的类型、endpoint、controller 或持久格式描述为已经实现。

上述 wire 物理细节只能在本 ADR Accepted 后，由 Rust contract implementation、独立 Python encoder/decoder、golden vectors 和 governance admission 在同一个 bounded batch 中冻结。

本ADR的canonical empty live-retire两阶段语义及exact-zero no-retire fast path依赖ADR-0007对ADR-0001 §4.1第162条的明确修订：`active`在P2e表示committed desired head并与live/terminal分离。两份ADR必须由同一次Accepted/explicit authorization共同生效；若ADR-0007修订未获接受，本ADR的`EmptyDeactivate`也不能单独实现。

## 决策提案

以下规则只有在本 ADR Accepted 后才成为实现约束。

### 1. 新增版本，不改变旧版本

PXTE v4 与 PXAR v5 是 additive successor，而不是对旧版本的 alias、宽松 decoder 或隐式升级：

- PXTE v1、v2、v3 的 encoder、decoder、canonical bytes、digest domain/output、limits、error variant 和已发布 reason-code 数值不变。
- PXAR v1、v2、v3、v4 的 outer bytes、request digest/signing commitment、decoder、limits、error variant 和已发布 reason-code 数值不变。
- 旧 decoder 收到 v4/v5 version 时继续返回其既有稳定 `unsupported-version` 类 reason；不能尝试按最接近的旧布局解释。
- PXTE v4 decoder 只接受 exact v4；PXAR v5 decoder 只接受 exact v5。即使 prefix 或 length 看起来像旧版本，也不得 fallback。
- 若未来提供统一入口，它只能先读取受界 version discriminator，再一次性分派到 exact decoder；禁止“先试 v5，失败再试 v4/v3”。
- 新版本拥有新的 version-specific execution digest 和 PXTA+PXTE composite digest。它们不得复用旧 digest domain，也不得改变旧 digest output。

PXAR v5携带additive `RuntimeApplyEnvelopeV2`、canonical zero-binding PXTA body和canonical PXTE v4 body。Envelope v2保留v1全部逻辑controls，却增加request-level exact 32-octet `expected_runtime_store_instance_id`，使用新的version/domain-separated canonical encoding、request-signing transcript和complete-request digest；v1 bytes/transcript/digest/signature vectors完全不改。该identity来自Controller已durable pin的authenticated bootstrap response，不属于Plan/Slice desired truth，也不改变target-slice digest。Envelope v2签名同时覆盖expected store与successor composite target digest，使request不能脱离exact Runtime store、writer tenure、revision、operation identity、CAS与request authentication被改写。

Runtime在任何tenure nonce/fence、request nonce/temporal state、source revision high-water、prepared或副作用前，必须验证v5 expected store逐字等于本地journal envelope的`store_instance_id`；mismatch返回stable `RuntimeStoreMismatch`且状态byte-identical。PXAR v1–v4不增加该字段；S7 reference store只执行v5 narrow profile，因此误以同一RuntimeHostId初始化的新store也会拒绝延迟旧v5 signed request，而不依赖Controller恰好先观察到变化。未来一般旧版本target迁移不由本ADR解决。

### 2. Narrow public logical model

PXTE v4 public grammar只有：

```text
Canonical target desired state
├── PXTA binding_count = 0
└── PXTE v4
    ├── RuntimeArtifactCompatibilityManifestProjection
    ├── ReferenceAssemblyProfileV1
    ├── ReferenceLoopDomainSpecV1?   # OneSourceLoop恰好1；EmptyDeactivate恰好0
    └── ReferenceLoopSubjectSpecV1?  # OneSourceLoop恰好1；EmptyDeactivate恰好0
```

| Record | 唯一拥有的逻辑事实 | 不拥有 |
| --- | --- | --- |
| target manifest projection | exact target、manifest digest、`RuntimeBuildIdentityV1`、selected exact PXAR v5、profile v1与一个exact fixture entry（definition/implementation/export refs、definition digest、fixture artifact digest） | CapabilityGrant、live FeatureReport、Runtime协商、bounds revision、version/mode/fixture集合或feature bag |
| `ReferenceAssemblyProfileV1` | exact mode与固定readiness/activation/failure/restart/drain/cleanup规则 | 通用activation graph、metadata bag、任意Rust sandbox保证 |
| `ReferenceLoopDomainSpecV1` | target-local LoopDomain ref与signed start/drain/cleanup lifecycle budgets | capacity、live DomainEpoch、Thread/Process executor或OS handle |
| `ReferenceLoopSubjectSpecV1` | exact InstanceRef、LoopDomainRef、CardDefinitionRef、CardImplementationRef/fixture export identity、definition digest、fixture artifact digest与canonical-empty config digest | Binding/Mailbox/ingress、dispatch/input requirements、live CardInstance或observed health |

S7同时冻结两个有真实producer/consumer的compatibility contracts，不能把“build digest”留成config自报值：

1. `RuntimeBuildDescriptorV1`由`paraegox-runtime-contracts`拥有canonical Schema/digest domain。它只有fixed-order、bounded字段：`descriptor_version = 1`、build pipeline用OS CSPRNG为本次最终artifact build生成并嵌入binary只读数据的nonzero 32-octet `build_instance_id`、final RuntimeHost executable的bounded byte length与SHA-256、canonical bounded target triple，以及`compiled_reference_compatibility_digest`。最后一项domain-separate覆盖exact PXAR v5/PXTE v4/profile-v1 constants和exact single fixture entry，不覆盖operator target。descriptor本身不嵌入binary，因此pipeline可先生成build id并编译binary，再hash final executable、最后生成descriptor，不存在自哈希。
2. `RuntimeArtifactCompatibilityManifestV1`也是该contract owner下的singleton canonical public configuration：`manifest_version = 1`后恰有一个fixed target row，row只含exact RuntimeHost target、`RuntimeBuildIdentityV1 { build_instance_id, build_descriptor_digest, runtime_artifact_sha256, compiled_reference_compatibility_digest }`、selected exact PXAR v5、profile v1与exact single fixture entry。没有record count、集合、unknown field、bounds revision或第二target。`manifest_digest`对不含digest字段的完整canonical manifest bytes使用独立domain计算；projection只携带该digest和同一exact row，所以没有递归自哈希。多target manifest需要后继Schema；S7可以为每个target提供独立singleton。

release pipeline是descriptor的唯一production producer：CSPRNG失败/short fill/all-zero、artifact hash/length读取失败或compatibility table不一致时不发布artifact。system installer/install operation是descriptor+artifact strict consumer，也是singleton manifest唯一production producer：它只从strict-verified descriptor、实际installed executable、operator选择的exact Runtime target/service identity和binary compiled exact fixture/compatibility table生成一份canonical manifest bytes+digest；不得接受任意prebuilt/editable manifest bytes作为第二authority。该同一exact artifact必须byte-identically交给Runtime initializer和operator/Controller/Planner immutable ingress，Planner不得手写或从另一份manifest重建PlanContent。

Runtime initializer是installer output的独立consumer，必须重新strict decode exact descriptor/manifest canonical bytes、验证digests和installed final executable length/SHA-256/target，并从**binary内不可由config/journal覆盖**的compiled `build_instance_id`与exact compatibility table计算actual compatibility digest；全部一致后才把descriptor exact bytes+digest、manifest exact bytes+digest和derived store-pinned identity直接写入sequence-1 snapshot/receipt。每次正常启动严格读取sequence-1延续下来的exact journal bytes并重新计算binary compiled actual进行逐字段比较，但不重新hash executable或读取installer side file；不匹配在startup-generation commit、bootstrap/query-ready/callback/resource create前quarantine。bootstrap分别报告compiled actual identity与store-pinned descriptor/manifest identity，只校验Controller pin与Runtime pinned同一artifact，不能把journal值回显成actual或掩盖双producer。release generator保持internal build tool；含operator target/service selection的install operation是S7-E真实operator CLI/config/install entrypoint，必须与owner、initializer/Planner/Controller consumers和first tests同批治理登记。S7-B先以internal codec/generator/oracle冻结bytes/bounds/golden，S7-E随真实release generator、install operation、initializer、Planner与Runtime consumers一起登记/promote这两个public surfaces。

`RuntimeBuildIdentityV1.runtime_artifact_sha256`与fixture entry的`fixture_artifact_digest`不等价：前者覆盖final RuntimeHost executable bytes，后者沿用Card implementation artifact的独立canonical digest/`BOUND_ARTIFACT_DIGEST`语义。manifest和compiled compatibility digest只把二者约束到同一verified release，不要求或推导两个digest字节相等。该机制防止受支持部署中的误装/换build；能替换binary并伪造compiled identity的privileged attacker仍超出本地reference threat model。

两个`Reference*V1` record都是新version-specific类型，不alias、不嵌入也不重新解释现有public `execution::LoopDomainSpec`/`CardSubjectSpec`；后者的`LoopDomainCapacity`和Mailbox execution语义不进入v4。`ReferenceAssemblyProfileV1`自身固定唯一内部装配常量：`lifecycle_concurrency = 1`，`mailbox_slots = 0`，`dispatch_slots = 0`，`background_task_slots = 0`。Runtime以这些profile-version语义创建idle lifecycle owner，不从local config/default或旧`LoopDomainCapacity`猜参数；任何需要非零dispatch/capacity的target都unsupported并等待新successor。

v4不定义可出现的Thread/Process/Ingress、Thread-executor或通用capacity branch，也不保留“当前必须为零、以后解释”的placeholder tag/count。未来新successor可以在有真实producer/consumer时复用本ADR的subject/ingress分离动机，但不能把其record layout或semantics追溯解释进v4。

### 3. 两个且仅两个合法 target shape

`OneSourceLoop`必须同时满足：zero-binding PXTA、exact target manifest projection、`ReferenceAssemblyProfileV1::OneSourceLoop`、恰好一个`ReferenceLoopDomainSpecV1`、恰好一个引用该Domain的`ReferenceLoopSubjectSpecV1`。subject的definition/implementation/export refs、definition digest与fixture artifact digest必须逐字段等于manifest projection中的single fixture entry，config digest必须是protocol冻结的canonical-empty digest；S7 production Planner拒绝任何per-use config。`RuntimeBuildIdentityV1`则独立匹配Runtime compiled/store identity，绝不能拿fixture artifact digest代替。subject无需也不能携带synthetic Binding/Mailbox/Ingress、`LoopExecutionRequirements`、Mailbox dispatch或input callback policy。

`EmptyDeactivate`必须同时满足：zero-binding PXTA、同一target manifest projection、`ReferenceAssemblyProfileV1::EmptyDeactivate`、零`ReferenceLoopDomainSpecV1`、零`ReferenceLoopSubjectSpecV1`。

缺少/重复manifest或profile、domain-only、subject-only、subject引用错误Domain、任何PXTA binding、任何未知/trailing branch，或partial empty都在allocation/callback/resource side effect前拒绝。除上述两种shape外不存在“structurally valid v4 general target”。

### 4. `ReferenceAssemblyProfileV1` 的唯一可执行语义

profile 的 canonical value 至少固定 `profile_version = 1` 与一个 mode：`OneSourceLoop` 或 `EmptyDeactivate`。profile 不携带字符串 action、opaque field map 或可选 fallback；其以下语义由 version 固定并被 PXTE/PXAR digest覆盖。

`OneSourceLoop` 只允许：

- 一个 LoopDomain、一个引用它的 trusted、manifest-pinned compiled-in Rust Loop fixture；workload selection与definition/implementation/artifact identity必须逐字段等于committed manifest projection中的single fixture entry，不能接受任意同进程Rust callback；
- 零 PXTA Binding；v4 grammar本身没有Thread/Process或ExecutionIngress branch；
- 零 ServiceDependency、activation edge/group/barrier、consumer-ingress/producer-egress gate、provider-loss action、Port、PermissionRequirement 与已授予 effect handle；该 fixture 的实现须经审计和测试证明 reference behavior 不执行外部 effect；
- `on_start`返回后fixture不得spawn/detach task、不得输出或获得周期tick；steady state只保留idle LoopDomain/CardInstance。`OneSourceLoop`证明source-only identity可装配，不证明持续source processing；
- exact fixture的`on_start/on_stop`必须是cooperative async state machine：每次poll有build-audited固定工作上限，不能执行blocking syscall/native call、同步等待、unbounded CPU loop或隐藏thread/task detach；等待只能返回`Pending`并由Runtime owner reactor的deadline/cancellation重新驱动。signed budget只约束这个fixture与owner cleanup，不能宣称可抢占任意Rust future或阻塞callback；
- readiness 唯一解释为：sole LoopDomain 和 CardInstance 已由 Runtime owner创建，exact subject 的 `on_start` 在该 Domain 的 signed `LoopLifecycleBudgets.start_budget` 与 request temporal deadline共同上限内成功返回，且 ownership census 与 incoming Slice identity/generation一致。worker handshake、process liveness或“没有 requirement”本身都不冒充 Ready；
- start返回known typed error或超时只允许销毁 staged generation并保留旧 active head；known error且cleanup/census后的`TerminalOutcomeSelection`仍严格在deadline前时返回non-success `StartFailedBeforeHeadCommitExactZero { reason, raw }`，否则raw timeout或selection到期/相等后exact-zero返回`StartTimedOutBeforeHeadCommitExactZero { raw }`，不能证明 exact-zero时 quarantine；不 retry callback、不把 staged写成 active；
- readiness成功后才 atomic commit incoming desired head；因为没有 ingress/egress或effect，不存在要由 Runtime猜测的其他开闸动作。

这里的“trusted manifest-pinned no-effect fixture”是极窄的 reference evidence，不是 OS security boundary。同进程 Rust 仍可能拥有 ambient filesystem、network、process 与 native-library authority；零 Port/Permission/effect handle 只证明 ParaEGOX 没有向 Slice 授予这些能力，不证明 seccomp、sandbox 或系统调用阻断。任何不受该 exact fixture entry 与 Runtime build identity共同约束的实现、需要强制 effect denial 的 workload，或 ProcessDomain containment 都必须等待后继 capability/profile 与 OS policy。

`EmptyDeactivate` 只允许：

- 上述 canonical empty target shape，并对当前 active head使用 exact CAS；S7 不用它做 `ExpectedActive::None` 的空 bootstrap；
- 只有current `LiveReady`且resource ledger精确匹配nonzero active generation时才走两阶段live-retire：第一次deadline check通过后，同一atomic owner transition写`FirstActionIntent`、把exact current live generation标为`NoNewAdmission/Draining`，并commit canonical empty desired head和`HeadCommittedRetiringOld`；两者不存在可服务旧generation的新admission窗口。S7无inbound ingress或background source task，但lifecycle callback/domain action仍受该state gate约束；
- `canonical empty + ExactZero`或`RecoveryFailedNotReady + ExactZero`且没有nonterminal action/staged/orphan generation时走no-retire fast path：full admission仍先写`PreparedNoEffects`，随后deadline/exact-CAS/census复核通过的single completion transaction直接写new empty head+`ExactZero`+terminal；它不写`FirstActionIntent`/`HeadCommittedRetiringOld`，不调用`on_stop`且不执行cancel/drain/cleanup。deadline已到则以`StopTimedOutBeforeHeadCommitNoEffects`终结并保留old head；
- 使用exact old active `OneSourceLoop` Slice中signed lifecycle budgets执行`on_stop → task/domain join → ownership cleanup`。graceful stop/drain deadline为`min(now + old signed drain_budget, incoming installed deadline)`；post-intent/pre-effect check或`TerminalOutcomeSelection`观察到incoming deadline到期或相等后不得选择ordinary apply success；
- 为释放Runtime已拥有资源，owner cleanup可在graceful deadline后继续，但只受old signed cleanup budget与fingerprinted local recovery cap约束。随后exact-zero的terminal是non-success `TimedOutButExactZero { raw }`；cleanup无法证明zero则quarantine，绝不后台补ordinary success；
- `on_stop`返回known error时仍必须先durable raw fact再运行owner cleanup；只有cleanup/census后的`TerminalOutcomeSelection`仍严格早于deadline时，exact-zero terminal才是`StopFailedButExactZero { reason, raw }`，其中`reason`必须是profile冻结、容量有界的typed reason而非任意字符串；selection到期或相等则timeout primary但raw仍保留known error。callback panic、cleanup error或ownership uncertainty不能被该结果遮蔽，仍按下条quarantine；
- drain/cleanup成功才返回 terminal exact-zero；panic、旧 active不是受支持 reference profile或ownership无法证明时 quarantine，不清除 empty desired head、不猜成功、不 fallback到kill-as-proof；
- 已经 terminal empty的 same-operation replay只查询同一结果；新的更高 revision必须对 empty head digest做 exact CAS。

所有将产生资源或lifecycle effect的normal apply都先durable `PreparedNoEffects`。intent/empty-head transaction构造前的pre-intent check到期或相等时，normal start=`StartTimedOutBeforeIntentNoEffects`，empty=`StopTimedOutBeforeHeadCommitNoEffects`并保留old head；未到期才atomic写`FirstActionIntent`。该intent/head directory fsync成功后、任何Domain/Instance/resource/handle create或callback前必须做独立post-intent/pre-effect check：normal到期时不创建effect；由于没有资源且census已经exact-zero，它在同一个atomic terminal snapshot中写`RawActionOutcomeLatch { callback=NotInvoked, deadline=TimedOut }`、`TerminalOutcomeSelection`与`StartTimedOutBeforeHeadCommitExactZero { effect_started=false, raw }`，不产生raw-only中间状态。live-retire empty保留empty head、跳过`on_stop`、先durable raw timeout/NotInvoked再cleanup到`TimedOutButExactZero { raw }`。callback/readiness/cancel raw fact必须在cleanup前durable；cleanup/census后、发布desired/LiveReady/terminal前再做`TerminalOutcomeSelection`的单次deadline sample。`now == deadline`按timeout。check fact进入journal transaction input；selection后fsync跨deadline不重分类，publish uncertain按ADR-0007 `UncertainAfterPublish`停止，不能以内存结果补写。

`active` 只表示 durable desired head，不表示当前进程仍有 live-ready materialization。RuntimeHost crash 后，旧进程内 LoopDomain/Card/Task 已消失；新进程不得只读取 active Slice 就报告 Ready，也不得用同 revision 的新 apply operation 绕过 revision/CAS state machine。只有“该exact head在上一host epoch曾durable `LiveReady`，且没有既存failure/unknown latch”才有一次new-host recovery资格；已经`RecoveryFailedNotReady`或`StartCallIntent` outcome unknown的head在后续process restart继续保持failure latch，不得重新进入callback。启动前先完成snapshot/store-pinned truth与binary compiled actual的pre-validation，再atomic推进RuntimeHostEpoch/ClockGeneration并invalidate旧live state；该事务durable后才逐字段比较已经strict-valid的active Slice target manifest projection/profile/fixture（包括独立的fixture artifact digest）与独立取得的compiled actual及store-pinned supported row。post-start compatibility不匹配进入可认证的validated operational quarantine；匹配且eligible时才持久标为`Recovering/NotReady`并创建journal-bound internal recovery action：

1. 绑定 exact active Slice/manifest digest、validated store-pinned complete `RuntimeBuildIdentityV1`，并分别绑定本次启动从binary独立取得且已与pinned truth逐字段核验的compiled actual `build_instance_id`与compatibility digest/table、exact profile v1、new RuntimeHostEpoch、new Domain/Instance/resource generations 与 current ClockGeneration；这些facts在创建资源前 durable commit，不能把journal identity回显成compiled actual；
2. 不转换旧 request deadline，而以 active Slice 中 signed `start_budget` 在当前 clock generation 安装一次性 recovery deadline，同时受 fingerprinted Runtime maximum budget约束；
3. 先在current ClockGeneration做pre-intent deadline check；到期或相等时durable写`RecoveryFailedNotReady { TimedOutBeforeIntentNoEffects, raw.callback=NotInvoked } + ExactZero`永久latch，不写intent。未到期才可durable写`StartCallIntent`；其directory fsync后、创建sole LoopDomain/CardInstance或调用`on_start`前再做post-intent/pre-effect check，到期时因没有资源且census exact-zero而以一个atomic snapshot同时durable raw timeout/NotInvoked与permanent TimedOut `RecoveryFailedNotReady + ExactZero` latch，不产生raw-only中间状态且不callback；
4. callback/readiness/cancel raw fact在任何cleanup前durable；cleanup/census后、发布`LiveReady`前用`TerminalOutcomeSelection`再采样一次deadline。只有success+census一致+未到期才durable Ready。start失败、timeout、panic或post-intent unknown后不得由当前或后续restart再次调用；cleanup证明exact-zero时发布保留raw facts的`RecoveryFailedNotReady`，cleanup/ownership无法证明时quarantine。

该 recovery action 不是新的 desired revision、Runtime apply operation 或旧 apply replay。`RecoveryPlannedNoEffects`中若host在intent和deadline observation均未durable前crash，下一host可证明零效果，终结旧action为`AbortedBeforeIntentNoEffects { raw.callback=NotInvoked }`并用new action identity/generations重新plan，不设置failure latch；已经durable的pre-intent timeout不能这样重试。`StartCallIntent` durable后的crash绝不replay callback，只在缺少durable callback fact时补`UnknownAfterIntent`，已经known success/error/deadline fact必须保留。若 active head 已是 canonical empty，启动只按 resource tombstone reconcile exact-zero，不创建 subject。若旧 empty drain 在 crash 时未完成，已消失的 in-process object不能收到伪造的 `on_stop` replay；Runtime只能依据 exact resource ledger和本进程边界证明清理。纯in-process reference资源若能证明now exact-zero，旧operation终结为non-success `InterruptedButNowExactZero { raw }`；任何可能跨RuntimeHost crash存活的资源都quarantine。新的`EmptyDeactivate`遇到`RecoveryFailedNotReady + exact-zero`时可以在exact CAS后走no-retire fast path；只有存在current live materialization时才调用其`on_stop`。

Runtime query 必须把 `(durable desired head, apply operation terminal/phase)` 与 `(current RuntimeHostEpoch, live materialization state, resource generation, readiness freshness)` 分开返回。历史 `Active` Receipt只证明当时完成过 apply；Controller restart/reconcile 不能据此推断当前 `LiveReady`，也不能把 `Recovering`、`RecoveryFailedNotReady` 或 `Quarantined` 写成 Ready。

合法mode transition只允许：

| Current desired/live | Incoming `OneSourceLoop` | Incoming `EmptyDeactivate` |
| --- | --- | --- |
| uninitialized `None` + no resources | 仅`ExpectedActive::None`允许 | 拒绝empty bootstrap |
| canonical empty + `ExactZero` | exact CAS、更高revision后允许 | exact CAS、更高revision后直接terminal empty |
| nonempty + `LiveReady`且resource ledger精确匹配active generation | S7拒绝Loop→Loop replacement | exact CAS后进入`HeadCommittedRetiringOld` |
| nonempty + `RecoveryFailedNotReady` + exact-zero | 拒绝直接restart；先提交empty | exact CAS后直接收敛empty，不调用`on_stop` |
| `Recovering`、`Draining`、`Uncertain`或`SupersededReconcileRequired` | `RuntimeBusy`，query/cleanup only | `RuntimeBusy`，query/cleanup only |
| `Quarantined`、manifest/build mismatch或ownership indeterminate | 拒绝 | 拒绝 |

owner-wide至多一个side-effecting apply/recovery/drain action。稳定`LiveReady`的非零resources是合法active materialization，不触发busy；只有存在nonterminal action，或resource ledger不能证明恰好等于current stable active generation（extra/staged/orphan generation）时阻塞新full admission。Loop→Loop replacement没有old-generation drain语义，因此v1必须先以更高revisionempty→terminal exact-zero，再以另一更高revisionstart。

higher tenure若在旧operation已经durable `FirstActionIntent`后接管，只推进writer fence而不允许第二action；旧operation进入`SupersededReconcileRequired`。cleanup exact-zero后它只能terminal为non-success `SupersededAfterIntentExactZero { raw }`，由bounded canonical raw summary保留known success/error/timeout/unknown及其组合；ownership无法证明则quarantine。该接管不回滚已经committed的empty desired head，也不把旧operation改写为ordinary success。

ADR-0007定义的`RawActionOutcomeLatch`与`TerminalOutcomeSelection`同样是profile-v1 runtime semantics：callback、deadline、host interruption、higher-tenure takeover与cleanup/census是可同时存在且单调持久的raw facts，known fact不能被crash降为unknown；最终primary outcome按`quarantine/ownership uncertainty > post-intent supersede > host interruption > deadline（equality included） > typed callback error > success`唯一选择，同时在terminal保留raw summary/digest。因而known stop error后crash仍以interrupted为primary但保留known error，known error后cleanup跨deadline以timeout为primary但保留error，不能由线程完成顺序选择不同Receipt。

任何缺少profile/manifest、mode与0/1 Loop shape不匹配、携带PXTA binding或未知branch的v4 request都是malformed/unsupported，并在S7 Planner candidate/commit前拒绝；Runtime再做零副作用defense-in-depth。v4没有可供Planner提交的第三种shape。

### 5. 引用、重复与 cross-body validation

builder 与 decoder 重建验证至少必须 fail-closed 拒绝：

- missing/zero/wrong-width `expected_runtime_store_instance_id`、request signature未覆盖该字段，或它与local journal store identity不一致；
- missing/duplicate/unknown target manifest projection或`ReferenceAssemblyProfileV1`；
- manifest target/digest、`RuntimeBuildIdentityV1`任一字段、selected exact PXAR v5、profile v1或exact single fixture entry与Slice/Runtime compiled/store identity不一致；
- profile mode与`ReferenceLoopDomainSpecV1`/`ReferenceLoopSubjectSpecV1`的0/1 cardinality不匹配；
- duplicate Domain/Subject、subject引用不存在/错误的DomainRef，或Domain未被sole subject使用；
- subject的definition/implementation/export refs或definition/fixture-artifact/config digests缺失/不匹配，任一fixture字段不等于manifest single entry、把Runtime executable SHA-256/build identity与fixture artifact digest混用、config不是canonical-empty，或signed lifecycle budgets不满足reference bounds；
- 旧`execution::LoopDomainSpec`/`CardSubjectSpec` bytes、任何capacity/dispatch field，或Runtime local配置试图覆盖profile固定的one-lifecycle/zero-dispatch constants；
- PXTA binding count非零，或出现任何Binding/Mailbox/Ingress、Thread/Process、dependency/egress/effect-grant/unknown branch bytes；
- malformed enum、unknown kind、unknown version、长度/count 溢出、oversized frame、trailing bytes、非 canonical ordering 和 canonical rebuild mismatch。

重复的 byte-identical record 也必须拒绝，不能静默 deduplicate。未知 kind/enum 不能降级为 `Unknown` 后执行；只有 contract 明确列出的、语义本身就是 `Unknown` 且 admission policy 明确拒绝或限制的既有值，才能保留其既有含义。

所有 cross-body 检查在 Runtime 创建 Mailbox、Task、Thread、Process、workspace 或任何其他副作用前完成。

### 6. Bounds 与 canonical ordering

PXTE v4 必须为以下维度分别发布固定 hard bound：

- frame 总 bytes；
- exact 32-octet `expected_runtime_store_instance_id`；
- `RuntimeBuildDescriptorV1`与singleton `RuntimeArtifactCompatibilityManifestV1`各自的frame/target-triple/field exact bound；
- target manifest projection与single fixture identity的exact bytes；
- `ReferenceAssemblyProfileV1` singleton 的 presence、mode 与 exact bytes；
- `ReferenceLoopDomainSpecV1`与`ReferenceLoopSubjectSpecV1`各自只能0或1；
- DomainRef、InstanceRef、definition/implementation/export refs、各独立digest与signed lifecycle budget的exact byte/count limit。

PXAR v5 必须同时受自己的 frame 上限、PXTA 上限、PXTE v4 上限和 authenticated envelope 上限约束。所有 length/count 使用 checked arithmetic，并在按声明长度分配或复制前验证；环境变量、Deck input、manifest或worker input不能放大或缩小 hard bound。所有bounds是PXTE v4/PXAR v5 + `ReferenceAssemblyProfileV1`的protocol constants，不存在operator-selectable `hard-limit revision/profile`字段；最终数值必须由真实 record sizing、现有 limits和property tests在implementation batch冻结。任何后续bound变化都需要新的contract version/profile successor，不能在同一v4/v5下改manifest。

canonical body使用固定唯一顺序：manifest projection → profile → `ReferenceLoopDomainSpecV1` presence/record → `ReferenceLoopSubjectSpecV1` presence/record。不存在需要排序的集合；builder仍须拒绝重复input，decoder通过decode→rebuild byte equality拒绝alternate encoding、duplicate、padding、unknown branch和trailing bytes。

Domain/subject presence必须显式编码；empty PXTE v4仍携带exact target manifest和`EmptyDeactivate` profile，具有唯一、非省略的canonical body与version-specific digest。不得用“缺少PXTE section”、zero-length outer body、null、默认字段或EOF表示empty target。

### 7. Empty target 不等于 omitted target

empty target 是一份真实的 PXAR v5 apply：

- 指向 exact RuntimeHost target、source scope与已bootstrap-pin的Runtime store instance；
- 携带 canonical zero-binding PXTA，以及含exact target manifest projection、`ReferenceAssemblyProfileV1::EmptyDeactivate`、zero-domain/zero-subject的PXTE v4；
- 由 exact successor composite digest、signed/authenticated apply envelope、writer tenure、revision、expected-active CAS、operation id 和 temporal constraints共同约束；
- canonical empty target 本身就是“该 target 的 desired execution 为空”的认证 deactivate intent；
- 只允许进入 deactivate/drain/retire 路径，不能被当作普通 activate 来创建 placeholder owner；
- 不表示删除 RuntimeHost、DeploymentScope、journal、writer fence 或 target identity。

Runtime 接受 empty target 后，仍需按 Runtime journal 和 assembly lifecycle 记录 prepare/terminal result，停止新 admission，drain/retire 旧 active generation，并证明本次 target 所有 Task、Thread、Process、Mailbox、Binding、retained payload 和 staged/live-resource owner census exact zero。已经为空时，相同 operation id/digest 的 replay只能查询或返回同一 terminal result；same id/different digest 仍然 conflict。

资源 exact-zero 不等于删除 desired-state head。empty target 必须像非空 target 一样成为 durable `active` canonical Slice：保存其 source revision、target-slice/composite/target-manifest digest、exact canonical empty body与committing operation/result ref，作为下一次 exact-active CAS 和 revision high-water；canonical terminal result payload只由terminal-operation ledger拥有。旧非空generation在该empty head下进入drain/retire；terminal exact-zero后仍保留empty active head。把`active`清为`None`会使旧revision重新满足bootstrap/CAS，因此禁止。

omitted target 则没有发送给该 RuntimeHost 的 apply request、canonical target bytes、operation identity 或 authenticated deactivate intent。Controller 未选择 target、rollout 尚未到达 target、transport 未发送和 target 被错误漏掉都可能表现为 omitted。Runtime 必须保持当前 active desired state 不变，不能把沉默、timeout、连接关闭、Controller restart 或 target 不在某次消息中解释为 empty。

Controller 若要移除一个已active target，removal revision的committed PlanContent/RevisionTransition必须仍显式保留唯一plan-side `EmptyTargetDesiredEntry`，固定target、source provenance、target compatibility-manifest projection与exact `EmptyDeactivate` profile。它不是owned-resource tombstone，也不是未来downgrade tombstone；old signed budgets来自Runtime当前exact active Slice，incoming deadline来自authenticated apply controls，不再引入含糊的第二deactivation-policy字段。pure projector只从该entry生成empty PXAR v5；rollout/observed history只决定何时推进，不得临时合成empty desired truth。Controller观察terminal retire/exact-zero evidence并完成该revision后，下一committed revision才可真正omit该target。

### 8. Owner、producer 与 consumer

contract owner 保持 `paraegox-runtime-contracts`。它只拥有 canonical values、validation、wire、digest 和 stable rejection taxonomy，不拥有 Deployment desired truth、Runtime lifecycle 或 journal。

唯一 production producer 候选是 `paraegox-deployment` 中由 committed Deployment target projection 驱动的 successor projector/request builder。它必须：

- 只从 committed plan content、stable allocation 与 immutable target inputs 构造完整 PXTA/PXTE；
- 对 source-only subject 产生真实Loop subject/domain与zero-binding PXTA，而不是synthetic ingress；
- 只从 removal revision 中显式 committed 的 `EmptyTargetDesiredEntry`产生 empty target，不从 observed active history或 target omission反推；
- 生成 signature-independent draft；DeploymentController 再以自己独占的 OS-protected request-auth key handle 调用窄 signer mechanism 完成现有签名流程。该 signer 不成为第二 desired-state owner或独立 service，且绝不持有 Tenure Authority private key；
- request builder只从Controller durable target binding取得expected Runtime store identity并放入PXAR v5 request control；projector不能把store写入Plan/Slice，CLI也不能覆盖；
- 不保存第二份 desired state，不根据 Runtime observed object 反推计划。

production consumer 候选是 `paraegox-runtime` 中的 executable Runtime apply admission/assembly path。它必须从 PXAR v5 bytes 独立 strict decode、认证、验证 composite commitment，并只基于 accepted canonical Slice 与 Runtime journal执行 `ReferenceAssemblyProfileV1` 的 prepare/activate/deactivate。没有 profile或 shape不匹配时在副作用前拒绝；Runtime 不得 import Deployment/Deck mutable truth，也不得自行补造 subject、binding、activation rule或 omitted-target intent。

当前仓库的 `paraegox-deployment` 仍是 enabler，public RuntimeHost 也尚没有上述 apply/assembly endpoint。因此这两个候选在真实 executable call path 和 governance registration 完成前，不得被文档或测试称为已接通的 producer/consumer。ADR、unit test、Python fixture、wrapper 或为消费 contract 而同批创建的第二 abstraction 都不算独立 consumer。

### 9. Rust/Python contract vectors

S7-B 至少需要以下双向证据：

- S7-B Rust internal canonical builder 生成 PXTE v4/PXAR v5，独立 Python decoder 验证 exact fields、canonical bytes、digests 和 rejection；它到 S7-E committed-plan vertical接入前不称 production producer；
- Rust/Python独立冻结`RuntimeBuildDescriptorV1`、singleton `RuntimeArtifactCompatibilityManifestV1`、`RuntimeBuildIdentityV1`与compiled-compatibility digest的canonical bytes/domain/bounds；descriptor/manifest的duplicate/trailing/unknown version/第二row/target mismatch稳定拒绝；
- install-operation tests证明manifest只能从strict-verified descriptor/installed artifact、operator exact target/service identity和binary compiled fixture table生成，任意prebuilt/editable manifest input稳定拒绝；Runtime initializer与Controller/Planner ingress收到同一byte-identical output，sequence-1后startup不读取installer side file；
- 独立 Python encoder 从逻辑 fixture 生成 bytes，Rust strict decoder 接受后重编码为 byte-exact 相同结果；
- `OneSourceLoop`、`EmptyDeactivate`、manifest/profile/shape mismatch、missing/duplicate manifest/profile与unknown branch golden fixtures；
- cooperative lifecycle Harness覆盖立即完成、跨多次yield完成与永远返回`Pending`后被deadline取消；code/architecture review证明exact fixture不含blocking/native wait、同步unbounded work或detach。该证据只适用于固定fixture，不能推广为通用callback抢占保证；
- 旧 PXTE v1–v3/PXAR v1–v4 golden fixtures、digest 和 reason-code tables逐字不变；
- builder输入field/record排列不影响唯一canonical bytes；
- legacy capacity-bearing `LoopDomainSpec`不能构造、嵌入或被v4 decoder接受；Runtime assembly测试证明profile固定one-lifecycle/zero-mailbox/zero-dispatch/zero-background-task常量是唯一构造输入且不读取local default；
- 对每个header/presence/manifest/profile/domain/subject/PXTA/digest/signature field的单字段扰动；
- expected store identity的missing/zero/wrong-width/bit-flip、signature/request-digest mutation，以及old-store signed request送到same-target fresh-store时在任何AdmissionState/fence/revision mutation前拒绝；
- duplicate domain/subject、orphan subject/domain、manifest/runtime-build/fixture-entry mismatch、Runtime/fixture digest混用、PXTA binding、unknown branch、non-canonical order、trailing bytes、oversized count/length和unknown-version rejection；
- empty target、omitted target、binding-bearing target、domain-only、subject-only、profile absent/unknown/mismatch的区分测试；
- profile state-machine vectors逐点覆盖normal/recovery的pre-intent deadline before/equal/after、intent publish old/new、durable intent后的post-intent/pre-effect check before/equal/after及raw-timeout mutation old/new、首个effect前crash、raw callback result old/new、known result后crash、cleanup跨deadline、`TerminalOutcomeSelection` before/equal/after、head/live/terminal或failure-latch publish old/new；两次pre-intent recovery crash不得调用callback，pre-intent timeout与任一post-intent crash必须永久禁止同head recovery replay；
- raw/terminal cross-product覆盖panic/cleanup uncertainty、post-intent supersede、host interruption、deadline、typed error与success，证明known raw facts原样保留且primary outcome固定为`quarantine > supersede > interruption > deadline > typed error > success`；
- 随机 malformed bytes/property tests证明 checked bounds 先于 allocation，且任何 reject 都发生在 Runtime 副作用前。

Python fixture 是独立 contract oracle，不是 Runtime owner，也不自动成为 production workload API。golden fixture 只能由明确的生成入口更新；旧 fixture 不能因新版本测试方便而重写。

### 10. Migration、downgrade 与 rollback

该版本迁移是 artifact compatibility rollout，不是对旧 bytes 的原地转换：

1. 先部署同时保留 strict v1–v4 decoder并增加 exact v5 support 的 Runtime reader；旧 decoder 本身不修改。
2. P2e compatibility唯一来源是strict-verified `RuntimeBuildDescriptorV1`和system install operation唯一生成、operator-installed的immutable singleton `RuntimeArtifactCompatibilityManifestV1`：exact RuntimeHost target、manifest digest、`RuntimeBuildIdentityV1`、selected exact PXAR v5、profile v1，以及一个exact fixture entry。profile v1的语义已经固定只含`OneSourceLoop|EmptyDeactivate`和本ADR的protocol bounds，projection不重复携带mode mask或bounds revision，也没有version/mode/fixture set，因此不引入集合排序。Runtime executable SHA-256/build identity与fixture artifact digest绝不相等比较或互相推导。该同一exact artifact的projection进入committed PlanContent、canonical Runtime Slice与target-slice digest，Runtime active/journal也持久绑定。它不是`CapabilityGrant`或observed`FeatureReport`。Runtime bootstrap分别报告binary compiled actual与store-pinned descriptor/manifest identity和current clock供exact一致性校验；mismatch停止rollout。只会strict decode v5但compiled profile/fixture不匹配的binary不满足manifest。
3. 同一 apply operation id/digest 只发送一个 exact version并绑定一个exact Runtime store instance。禁止对同一 target/revision dual-write v4 与 v5、先发 v5 timeout后猜测发 v4，或把两个版本/两个store的结果 last-write-wins 合并。
4. 仍能由旧合同完整表达的既有 target 可以继续使用其原版本；version selection 必须是 committed plan/projection事实，不是 Runtime 隐式 fallback。
5. source-only subject 与 empty target没有合法旧版本表示。目标不支持 v5 时，Controller 在 commit/rollout 的明确边界 fail-closed；不得注入 synthetic Mailbox，亦不得把 omitted target 当作降级表示。

wire fixture 和 request history 是 immutable evidence，不做运行时 rewrite。若 Runtime journal 持久了 accepted/prepared/active v5 canonical state，其 journal payload version 与 migration 服从 ADR-0007：旧 binary 看到无法理解的新状态必须停止并 quarantine，不能丢弃 v5 fields、重新解码为旧 target 或从空状态启动。

同一 RuntimeHost identity/store 不存在直接降级到 v4-only binary的路径：即使 v5 empty target已经 exact-zero，mandatory canonical empty active head/revision/CAS high-water仍没有 v1–v4表示。停止新 v5 rollout但继续运行能解析 v5 journal/head的 binary不是旧版 downgrade。

S7 reference store identity还与exact `RuntimeBuildIdentityV1`绑定，因此也不支持同store原地A→B binary upgrade，即使B声称兼容v5。compiled build A无法接受manifest B，build B也不得对active manifest A自动reassembly；authenticated active Slice不能由offline journal tool静默改写。任意build identity变化都必须先在A上提交empty并取得terminal exact-zero、显式decommission/封存旧target/store identity，再以new target/store identity和manifest初始化B；一般in-place compatible upgrade需要后继ADR的two-phase ownership/compatibility transfer。

安全退出 v5 identity 只有：

- 显式 decommission/ownership-transfer 流程封存旧 RuntimeHost/target/store identity、最后 v5 empty head、writer fence、operation history与 exact-zero receipt，再以全新 target/store identity初始化一个旧 profile；旧 identity永不复用；
- 后继 ADR + offline migrator定义目标 binary确实可读的 tombstone/high-water representation，并证明无损保留 revision、writer fence、operation history、CAS和owner census。没有该新 parser/format时不能把 v5 empty映射为 `None`。

直接回滚 producer 而保留 source-only/v5 active target、让旧 Runtime尝试读取新 journal、从 backup 恢复较旧 target state或自动用 v4重发都不安全。新版本出现问题时可以停止新的 v5 rollout并保留旧版本 target不变；已接受 v5 的 target必须通过上述显式路径收敛，不能靠隐藏 fallback 回滚。

## 备选方案

### 原地放宽 PXTE v1–v3 的非空规则

代码改动可能较小，但会改变既有 builder/decoder接受集合、canonical bytes解释、digest commitment和稳定 rejection behavior。跨语言 fixture即使保留相同 happy-path bytes，也无法保留旧 malformed input 的合同，因此拒绝。

### 为 source-only subject 制造 synthetic Mailbox/Binding

它可以复用 PXTE v1–v3，却会虚构 ingress、Schema、delivery、capacity和cleanup owner，使 digest、资源预算、backpressure和最终 exact-zero census都包含不存在的对象。fixture不是独立生产事实，因此拒绝。

### 用 omitted target 表示 deactivate

没有 request、operation id、writer tenure、revision、CAS或authentication，Runtime无法区分期望删除、rollout未到达、网络丢失与Controller crash，会把沉默变成破坏性写操作，因此拒绝。

### 新增独立 stop/delete endpoint

一个完整认证、带 revision/CAS、operation id、journal和reconcile的deactivate endpoint在技术上可行，但会复制apply authority和idempotency路径；一个较弱的stop endpoint又不能修改authoritative desired state。canonical empty target可在同一 apply contract内表达相同意图，因此首版不增加第二控制入口。

### 每种 Domain kind 继续维护独立 subject+mailbox record

这能延续v1–v3结构，却会继续把workload identity复制到每个ingress，使零ingress subject仍无法表达，并保留同一subject多Mailbox之间的subject-mismatch问题，因此不采用。

### 建立无类型通用 execution node/edge 图

统一 node/edge 表面上更灵活，但会把Loop/Thread/Process的capacity、lifecycle、launch、resource与recovery约束降为可选字段，并与ADR-0005拒绝通用Graph Engine/Schema的边界冲突。PXTE是typed target execution contract，不是通用图存储。

## 后果

收益：

- source-only subject获得真实、最小的execution authorization，不再伪造Mailbox或Binding。
- v4只公开真实producer/consumer覆盖的Loop domain/subject与empty grammar，不让未消费的Thread/Process/Ingress branch借reference path获得公共兼容承诺。
- explicit empty target在现有authenticated apply、revision、CAS和operation id边界内提供可恢复的deactivate语义。
- 旧版本的bytes、digests、reason codes和strict rejection保持稳定，新能力通过清晰版本边界加入。
- explicit profile为exact S7 one-subject/empty Runtime state machine提供唯一prepare/recovery/drain顺序，而不创建通用Graph owner。

成本与限制：

- contract crate、Deployment producer、Runtime consumer和独立Python vectors都需要新版本实现与长期compatibility coverage。
- Runtime在迁移期需要同时维护多个exact decoder，但不能共享模糊fallback路径。
- empty target把deactivate纳入journal/crash recovery测试面；收到成功回复前后的每个crash window都必须query/reconcile。
- 首个S7 profile只证明一个trusted Rust Loop source-only subject和empty retire，不证明多ingress吞吐、Thread/Process production assembly、effectful workload或跨Node rollout。
- reference fixture 的零授予 Port/Permission/effect handle与审计测试不是 sandbox；它不证明任意同进程 Rust 不能通过 ambient OS authority产生副作用。
- 新逻辑模型本身不提供public Runtime apply endpoint、DeploymentController、journal或exact-zero实现证据。

## 失败场景与反例

如果真实target需要在没有subject时预创建共享Domain以满足可测的startup SLO，当前“每个domain至少一个subject、empty必须全零”会拒绝该需求。重新评审需要提交真实producer/consumer、资源owner、失败清理和为何不能随第一个subject原子prepare的证据；不能先把unused domain放宽为合法。

如果未来需要任意PXTA Binding/ExecutionIngress，后继设计必须先以真实producer/consumer明确一对一或fan-out的delivery、credit、ordering、backpressure、schema和cleanup owner，并使用新版本；不能把它们追溯解释进v4或通过多个Runtime本地订阅绕过canonical contract。

如果未来deactivate需要保留预热Domain、durableMailbox或installation-owned state，zero-all empty target不能表达分层retention。这不会授权部分empty；需要先决定稳定state owner、retain/delete Receipt与migration，再提出typed retained-resource successor。

如果PXTE v4的narrow manifest/profile/Loop record sizing在现有request/envelope限制下仍无法容纳reference fixture，应以representative fixtures和benchmarks比较更紧凑的v4布局。该证据可以在发布前改变v4物理编码，但不能修改v1–v3或偷偷加入未消费branch。

## 实施与验证

当本 ADR header 为`Proposed`且authorization receipt未生效时，本文只新增决策草案，不授权创建或注册PXTE v4/PXAR v5 public API，不授权修改旧decoder，也不授权新增RuntimeApplyEndpoint、RuntimeAssemblyEngine、DeploymentController、journal、daemon或generic contract framework。达到`Accepted`也只授权按下述依赖顺序进入实现，不表示source-only/empty-target能力已经完成。

若本 ADR 被明确接受，建议分成 internal-first 与真实 vertical promotion 两步实施：

1. S7-B 先冻结两个逻辑fixtures、`RuntimeApplyEnvelopeV2`、`RuntimeBuildDescriptorV1`、singleton `RuntimeArtifactCompatibilityManifestV1`、hard bounds、fixed field order、新digest domains和新增reason taxonomy，再由Rust/Python共同冻结exact bytes；不改旧常量或fixtures，不实现Thread/Process/Ingress placeholder branch。
2. S7-B 在`paraegox-runtime-contracts`中以 private/internal module 实现上述values、builder、strict decoder、cross-body validation和decode→rebuild equality，完成Rust/Python双向golden、mutation/property与old-version regression。此时 registry 不新增 public API，不能称 production successor capability。
3. 若 S7-E 不能形成真实 vertical slice，删除或继续保持该 internal enabler；不得为通过治理而制造 wrapper/第二 abstraction。
4. S7-E 同一 executable vertical 变更中落下真实release descriptor generator、installer/initializer verification和binary compiled-identity check；再由 `paraegox-deploymentd` 的最小 committed-plan transaction产生权威 target projection，接入 `paraegox-deployment` 的唯一 pure successor producer和现有 RuntimeHost binary 的 strict apply endpoint consumer。各端只传 canonical contract bytes/value，不共享 mutable object。没有真实release/installer或committed-plan owner驱动时，descriptor/manifest/projector都仍只是enabler，不满足promotion。
5. 只有第4步真实 call path、first functional tests 与 no-side-effect rejection evidence 同时存在时，才在governance登记contract owner、producer、executable consumer、compatibility和removal/migration rule，并把 module 提升为 public contract；不为 request signing 新建第二 owner/service。
6. 在S7-E/S7-F/S7-G以production-equivalent chain分别证明one-subject activate、RuntimeHost restart后的 `Recovering/NotReady → LiveReady` internal reassembly与query分型、empty-target deactivate/drain/retire和final exact-zero；contract tests本身不能完成该声明。
7. 通过独立工程/安全复核、完整本地门禁和目标Linux CI后，才允许把相应 internal foundation/Runtime vertical slice 分别标记为implemented/validated；ADR Accepted也不能替代代码与system evidence。

## 后继与替代

本 ADR 不替代PXTE v1–v3或PXAR v1–v4；这些版本继续按各自compatibility contract存在。任何新增Domain kind、retained partial-empty semantics、fan-out ingress、incompatiblebounds/layout变化或旧版本retirement都必须通过后继ADR与新version处理。
