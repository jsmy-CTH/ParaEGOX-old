# S7/P2e 准入与交付决策包

> 状态：Decision Gate（本地执行材料，不进入 Git）
> 日期：2026-07-31
> 代码基线：`861d436`；GitHub CI `30601156490` 成功
> 前置完成：S6/P2d local POSIX reference slice
> 已接受边界：[ADR-0001](../adr/ADR-0001-deployment-controller-boundary.md)、[ADR-0002](../adr/ADR-0002-card-definition-terminology.md)、[ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md)
> 待决策：[ADR-0004](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md)、[ADR-0005](../adr/ADR-0005-typed-domain-graphs-and-runtime-assembly-boundary.md)、[ADR-0007](../adr/ADR-0007-p2e-reference-journal-and-crash-recovery.md)、[ADR-0008](../adr/ADR-0008-pxte-v4-pxar-v5-subject-ingress-separation.md)

## 1. 当前结论

S7 已越过 S6 代码依赖，但尚未越过架构准入门。`CONTRIBUTING.md` 要求公共协议、新 owner、package boundary 与 persistent format 由 Accepted ADR 或针对具体选择的明确授权支撑。此前笼统的“继续开发”授权阶段推进，不等于接受 Deck、Graph、successor wire 和 journal 的长期兼容选择。

在决策前可以完成 ADR、wire 草图、crash matrix、governance admission 和测试设计；不能创建 public Deck/Graph/Assembly/journal/controller placeholder，也不能把本文件中的 candidate 当作已实现能力。

## 2. 一次性待授权基线

授权短语定义为：**接受 S7/P2e 基线 v1**。

该短语同时表示：

1. 接受 ADR-0004 当前提案：Deck 是 executable workload；DeckLock 是唯一 canonical 解析产物；DeckTopology 是受 DeckLock digest 覆盖的 directed multigraph；P2e 不引入 Application/Installation 或 `Bundle` 别名。
2. 接受 ADR-0005 当前提案：DataLink、ServiceDependency 与 activation constraint 分型；首版 cyclic Deck fail-closed；不建通用 Graph Engine/Store/Schema；Graph Foundation 只有两个真实生产消费者后才按实际交集 internal 抽取；RuntimeAssemblyEngine 只消费 authenticated Slice + Runtime journal，无独立 desired store/identity，不进入 steady hot path。
3. 接受 ADR-0008 当前提案：additive public surfaces 包含 `RuntimeBuildDescriptorV1`、`RuntimeBuildIdentityV1`、singleton `RuntimeArtifactCompatibilityManifestV1`及其target projection、`RuntimeApplyEnvelopeV2`、PXTE v4/PXAR v5、`ReferenceAssemblyProfileV1`以及exact zero/one `ReferenceLoopDomainSpecV1`+`ReferenceLoopSubjectSpecV1`。descriptor、manifest、identity、projection和envelope的canonical Schema/digest domain由`paraegox-runtime-contracts`唯一拥有；release pipeline是descriptor唯一producer，system installer严格验证final RuntimeHost artifact并作为singleton manifest唯一producer，Runtime从编译内嵌actual identity/constants与store-pinned identity双重校验。installer不得接受任意prebuilt manifest bytes作为第二authority。只允许`OneSourceLoop`与`EmptyDeactivate`两种shape，PXTA binding必须为零。Envelope v2以request-level `expected_runtime_store_instance_id`绑定Controller bootstrap-pin的exact Runtime store，并用新domain纳入canonical transcript/complete request digest/signature；它不进入Plan/Slice，旧envelope v1与PXAR v1–v4不变。新record不alias/重解释既有`LoopDomainSpec`/`CardSubjectSpec`；Thread/Process、Thread executor budget、ExecutionIngress与一般activation schema不进入v4 public wire。任何binary变化或v5→v4-only都要求empty terminal、decommission旧RuntimeHostId/store并建立新identity/store；旧decoder、bytes、digest、reason code与严格无fallback行为不变。
4. 接受 ADR-0007 当前提案，并同步接受它对Accepted ADR-0001 §4.1第162条的两个exact修订：P2e `active`改指committed desired head且与live/terminal分离；存在live/nonzero generation时empty先在同一事务写`FirstActionIntent`、commit empty head并进入retire，随后第二事务terminal，已经exact-zero且无action/resource的empty fast path则单事务收敛且不创建intent/callback；`Superseded`只限pre-effects，post-intent旧operation进入`SupersededReconcileRequired`并阻塞new effects直到exact-zero或quarantine。其余ADR-0001边界不变。DeploymentController、DeploymentTenureAuthority、RuntimeHost分别独占versioned/checksummed/bounded atomic snapshot journal与owner-specific one-shot initializer；keys/policy/service principals由operator预置并绑定sequence-1 fingerprint。Authority与Controller使用隔离OS principal/key handle，`acquire_tenure`经versioned/bounded/authenticated+authorized local IPC；其request/response/framing/auth transcript由现有`paraegox-deployment`唯一拥有，`WriterTenureProof` canonical value仍由`paraegox-runtime-contracts`拥有。authenticated Runtime bootstrap/query request+response、channel-auth transcript/digest domain与bounds由`paraegox-runtime-contracts`唯一拥有。Runtime tenure-only fence transaction与full request admission transaction分开，后者原子提交request/temporal state、per-source revision high-water、exact request/Slice和`PreparedNoEffects`；任何将产生资源/callback效果的normal apply与restart recovery都必须在效果前分别durable写`FirstActionIntent`或`StartCallIntent`。active desired、live materialization、recovery action与resource ledger分型且owner-wide只有一个side-effect action。S7-E提供authenticated bootstrap并持久pin RuntimeHostId/store/channel/build；S7-F提供authenticated operation/live query与journal-bound restart reassembly，历史Active不冒充current Ready。invalid/undecodable snapshot、descriptor/manifest或compiled-vs-pinned pre-validation mismatch不能提供authenticated bootstrap/query；只有validated snapshot与成功startup-generation commit之后的active-head compatibility、recovery或ownership quarantine可以携带exact identity返回authenticated `Indeterminate`。journal使用OS exclusive lock、temp write/fsync、atomic rename与directory fsync；本地基线不防完整旧有效snapshot rollback，anti-rollback/HA只能经显式migration与后继ADR替换。

不随该授权冻结：

- `deck.yaml`/`deck.lock` 最终文件语法和公共 CLI 名称；
- Application/Installation、Marketplace、通用 Workflow/Graph Engine；
- Graph Foundation 是否最终抽取；
- HA/共识、跨 Node rollout、Zenoh transport；
- C++ worker、production ProcessDomain containment 或硬件能力。

## 3. 最小可执行闭环

首个 S7 reference profile 只包含：

- 一个 DeploymentScope、一个 target RuntimeHost、一个 release-produced `RuntimeBuildDescriptorV1`、一个由system installer从verified descriptor+actual executable+operator选择的exact target/service identity+binary compiled fixture table唯一生成的singleton `RuntimeArtifactCompatibilityManifestV1`，以及一个逐字段匹配其single fixture entry的trusted compiled-in Rust Loop subject；manifest分别绑定final RuntimeHost executable identity与Card fixture artifact identity，二者不是同一digest；
- subject 无 In/Out、Link、ServiceRequirement、PermissionRequirement、已授予 effect handle 或物理 effect contract；fixture 的 reference behavior 经审计与测试为 no-effect；
- 零 Port/Permission/effect handle 不是 sandbox：同进程 Rust 仍有 ambient OS authority，本阶段不宣称对任意 trusted implementation实施系统调用阻断；
- fixture的`on_start`返回后不得spawn/detach task、不得输出或获得tick；steady state只保留idle LoopDomain/CardInstance，不宣称持续source processing；
- `source-only`在该profile中只是“零inbound ingress”的结构分类，不表示reference fixture实际产生source stream；
- fixture的`on_start/on_stop`必须cooperative async、每次poll有固定审计上限且不做blocking/native wait、同步unbounded work或detach；Runtime reactor能对永远`Pending`的fixture执行deadline cancellation，但本阶段不宣称可抢占任意阻塞Rust callback；
- revision 1 编译、commit、project、authenticated apply、prepare、Ready、activate、observe；
- revision 2 是同 target 的 empty desired target，用于 deactivate、drain、retire；
- revision 2 的 canonical empty Slice 仍作为 durable active desired head/revision/CAS high-water，资源 census 为零不把 active 清成 `None`；
- DeploymentController、TenureAuthority 与 RuntimeHost 分别重启，并从各自 journal 恢复；RuntimeHost restart先将 durable desired active 与 live readiness分开，在新 host/clock generation下以 internal recovery action分配新 resource generations、重新执行 bounded `on_start`，成功后才发布 `LiveReady`；
- Controller query 分别观察 durable desired head/apply operation 与 current RuntimeHostEpoch/live materialization；历史 Active Receipt不冒充当前 Ready，同 revision新 apply不用于重建资源；
- 最终 live Task/Thread/Process/Mailbox/Binding/retained bytes 与 staged/live-resource owner census 精确为零；journal 仍保留一个 canonical empty active desired head。

```text
DeckSpec(one subject)
  → DeckCompiler
  → DeckLock
  → DeploymentPlanner
  → DeploymentPlanCandidate
  → DeploymentController atomic commit
  → RuntimeSliceProjector
  → RuntimeApplyEnvelopeBuilder
  → DeploymentController request-auth sign + rollout journal commit
  → RuntimeApplyEndpoint
  → RuntimeAssemblyEngine PreparedNoEffects/FirstActionIntent/readiness
  → observed facts
  → RuntimeHost restart: Recovering/NotReady
  → journal-bound internal reassembly → LiveReady
  → revision 2 empty target
  → deactivate/drain/retire/exact zero
```

该闭环不经过 public `Card.run()`、standalone Runtime route、test-only production proof、OpsService、Fabric 或 NodeDaemon。

## 4. PXTE v4 / PXAR v5 successor 约束

### 4.1 为什么必须 successor

当前 PXTE v1–v3 把执行 subject 与 inbound Binding/Mailbox execution record 绑定；PXTE v3 还要求至少一个 ProcessDomain 和 ProcessMailbox execution。当前 PXAR v4 的 target assignments/execution validation 因而不能表达：

- 没有 inbound Mailbox 的 source-only subject；
- 同 target 上无 subject、无 binding 的 authoritative empty desired state；
- 仅为 deactivate/retire 旧 active revision 而提交的 empty target。

修改旧版本会破坏已经锁定的 canonical bytes、digest 和严格 decoder，所以只能新增版本。

### 4.2 Narrow public grammar

PXTE v4只有四个fixed-order组成：

- exact target `RuntimeArtifactCompatibilityManifestV1` projection：fixed tuple绑定target、manifest digest、`RuntimeBuildIdentityV1`、selected exact PXAR v5、profile v1，以及一个exact fixture entry（definition/implementation/export refs、definition digest、fixture artifact digest）；
- `ReferenceAssemblyProfileV1` exact singleton；
- optional `ReferenceLoopDomainSpecV1`：`OneSourceLoop`恰好一个且只含Domain ref与signed start/drain/cleanup lifecycle budgets，`EmptyDeactivate`恰好零个；
- optional `ReferenceLoopSubjectSpecV1`：`OneSourceLoop`恰好一个并携带exact Instance/Domain、CardDefinition/Implementation/export refs、definition/artifact/canonical-empty-config digests；`EmptyDeactivate`恰好零个。

两个shape都使用zero-binding PXTA，且v5 outer request必须携带Controller已journal的exact 32-octet expected Runtime store identity；Runtime在tenure/request AdmissionState、fence、revision或副作用前比较本地store，mismatch状态不变。两个`Reference*V1` record不复用旧public capacity-bearing type；profile v1固定`lifecycle_concurrency=1`以及zero mailbox/dispatch/background-task slots，Runtime不得从local default或旧`LoopDomainCapacity`补值。v4没有Thread/Process/ExecutionIngress、Thread-executor/general-capacity或一般activation branch，也不预留zero-only placeholder。profile v1语义已经固定两个mode和全部protocol bounds，manifest projection不重复mode mask/bounds revision，也没有version/mode/fixture set，不存在集合排序歧义。`OneSourceLoop`的definition/implementation/export refs、definition digest与fixture artifact digest逐字段匹配manifest中的single fixture entry，config固定canonical-empty且不接受per-use config；Runtime build identity独立匹配manifest，不能与fixture digest混用。该profile没有input/tick/dispatch语义。`EmptyDeactivate`仍是真实authenticated desired head，不删除RuntimeHost或journal。missing/duplicate manifest/profile、partial shape、任意PXTA binding、unknown branch或manifest/runtime-build/fixture-entry mismatch均在副作用前拒绝。

`RuntimeBuildDescriptorV1`、`RuntimeBuildIdentityV1`、`RuntimeArtifactCompatibilityManifestV1`及projection都由`paraegox-runtime-contracts`拥有canonical encoding、digest domain、bounds和strict decoder：

1. `RuntimeBuildDescriptorV1`只有`descriptor_version = 1`、build pipeline以OS CSPRNG生成并嵌入binary只读数据的nonzero exact 32-octet `build_instance_id`、final RuntimeHost executable的bounded byte length与SHA-256、canonical bounded target triple、`compiled_reference_compatibility_digest`。最后一项只覆盖exact PXAR v5/PXTE v4/profile-v1 constants与exact fixture entry，不覆盖operator target。pipeline先生成build id并编译，再hash final executable并生成外部descriptor，因此没有binary自哈希。
2. singleton `RuntimeArtifactCompatibilityManifestV1`只有`manifest_version = 1`和恰好一个fixed target row；row只含exact RuntimeHost target、`RuntimeBuildIdentityV1 { build_instance_id, build_descriptor_digest, runtime_artifact_sha256, compiled_reference_compatibility_digest }`、selected exact PXAR v5、profile v1与exact single fixture entry。没有record count、集合、unknown field、bounds revision或第二target；多target只能使用每target独立singleton或后继Schema。
3. `manifest_digest`对不含digest字段的完整canonical manifest bytes使用独立domain；projection只携带该digest和同一exact row，不递归自哈希。projection/digest同时进入committed PlanContent、canonical Runtime Slice和target-slice digest，并绑定active/journal restart preflight；它不是CapabilityGrant或live FeatureReport。
4. release pipeline是descriptor唯一producer；system installer/install operation是descriptor+artifact的strict consumer，也是singleton manifest唯一producer。它验证descriptor digest、final executable length/SHA-256/target与binary compiled table，只从这些verified facts和operator选择的exact Runtime target/service identity一次生成一个canonical manifest artifact，拒绝任意prebuilt manifest bytes。该同一artifact必须byte-identically交给Runtime initializer和operator/Controller/Planner immutable ingress；Planner不能手写、重建或接受另一份manifest产生PlanContent，bootstrap只校验两端同一artifact而不掩盖双producer。initializer再次验证installed artifact与binary compiled id/table，并把exact canonical descriptor bytes+digest、singleton manifest bytes+digest作为sequence-1 snapshot的一部分持久pin。每次startup先验证snapshot canonical bytes/digests，再从binary内嵌build id/compatibility table取得compiled actual并逐字段比较descriptor、manifest与store-pinned identity；startup不重新hash executable或读取installer side file，artifact length/SHA证据由initializer拥有，distinct replacement build由compiled id/table mismatch拒绝，能伪造同一compiled identity的privileged attacker不在本地reference threat model。bootstrap把compiled actual与store-pinned `RuntimeBuildIdentityV1`分开报告，Controller验证并pin exact response；不能从可编辑config或manifest反向声明compiled actual。

`RuntimeBuildIdentityV1.runtime_artifact_sha256`覆盖final RuntimeHost executable bytes；fixture entry的`fixture_artifact_digest`沿用Card implementation artifact的独立canonical digest/`BOUND_ARTIFACT_DIGEST`语义。fixed manifest只把二者约束到同一verified release，不声明、推导或比较二者字节相等。S7 reference store/build immutable：任何binary变化或v5→v4-only都必须empty terminal→decommission old target/store→new RuntimeHostId/store，不能原地upgrade/downgrade。

最终 binary field layout、record byte size 和新增 reason-code 数值只在授权后由 contract implementation + independent Python fixture 一起冻结，不能先由本计划假装确定。

### 4.3 兼容与向量

- 保留所有现有 PXTE v1–v3 与 PXAR v1–v4 golden fixtures逐字不变。
- S7-B Rust internal canonical builder 与独立 Python encoder双向生成Envelope v2、descriptor、identity、singleton manifest/projection、PXTE v4/PXAR v5 golden fixtures；到 S7-E release/installer/Planner producer→Runtime executable consumer接通前都只称internal enabler。
- 每个header/presence/descriptor/identity/manifest/profile/`ReferenceLoopDomainSpecV1`/`ReferenceLoopSubjectSpecV1`/PXTA/envelope/digest/signature field都有单字段扰动。
- legacy capacity-bearing `LoopDomainSpec`/`CardSubjectSpec` bytes与任意capacity/dispatch field稳定拒绝；assembly测试证明profile固定one-lifecycle/zero-dispatch常量不从Runtime local default补值。
- expected store identity的missing/zero/wrong-width/bit-flip和old-store signed request→same-target fresh-store均在AdmissionState/fence/revision mutation前稳定拒绝。
- descriptor的RNG failure/short-fill/all-zero、installer final executable length/SHA/target/compiled-table mismatch、任意prebuilt manifest输入、initializer复验失败，以及sequence-1 exact descriptor/manifest bytes缺失、corrupt、non-canonical、digest/cross-ref不一致或startup compiled actual与store-pinned identity不匹配，都在startup-generation mutation、bootstrap/query-ready或副作用前稳定拒绝且不提供authenticated response；runtime artifact SHA与fixture artifact digest互换也拒绝。只有完整store snapshot与pinned build truth已strict验证、startup-generation/live invalidation已经durable后才检查的active Slice manifest projection/profile/fixture compatibility失败，进入不接收apply但可携带exact pinned identity回答authenticated `Indeterminate`的validated compatibility quarantine。
- 随机输入顺序不改变 canonical bytes。
- malformed/oversized/duplicate/orphan/manifest mismatch/PXTA binding/unknown-branch/unknown-version在Runtime副作用前拒绝。
- empty target 与 omitted target 不等价；前者是 authenticated desired state，后者没有该 target 的 apply。

## 5. Journal owner 与 crash matrix

### 5.1 分 owner snapshot

| Owner | 最小持久事实 | 不能持有 |
| --- | --- | --- |
| DeploymentTenureAuthority | current writer epoch、proof/signing lineage、authorized peer/scope/key fingerprint、snapshot generation | committed plan、request-auth key、Runtime active state |
| DeploymentController | stable-ID allocation、next/committed revision、committed plan digest/content、request-auth key ref/rotation、exact signed apply request、pinned RuntimeHostId/store/channel/manifest、bootstrap分别报告的compiled actual与store-pinned `RuntimeBuildIdentityV1`、exact bootstrap/query response bytes+digest、per-target rollout fact | Tenure Authority private key、Runtime lifecycle |
| RuntimeHost | RuntimeHost/clock generation high-water、sequence-1 strict-verified exact canonical `RuntimeBuildDescriptorV1`/singleton manifest bytes+digests、binary只读compiled actual identity/table、store-pinned `RuntimeBuildIdentityV1`、admission/trust-policy fingerprint、完整 AdmissionState、per-source revision high-water、writer fence、exact request/Slice、prepared/retiring operation、active desired head、live materialization、recovery action、resource ledger/tombstone、terminal result、quarantine | DeckSpec、DeckLock、global plan truth |

每份 snapshot：

- magic/version、owner identity、snapshot generation、bounded payload length、payload digest/checksum；
- single writer + OS exclusive lock；lock handle non-inheritable（POSIX 至少 `O_CLOEXEC` 并在 child spawn/exec 路径验证关闭），owner crash后即使其 child仍存活也不能替它持锁；
- create-new temp in same directory → bounded write → file fsync → atomic rename → directory fsync；
- previous committed file remains authoritative until rename；
- no field-by-field multi-file merge and no last-write-wins map recovery；
- unknown version/digest mismatch/truncation/impossible invariant produces typed quarantine fact。

三个store都由显式one-shot initializer在fresh directory写sequence 1并产生receipt；各initializer用目标OS认可的CSPRNG一次填充exact 32 octets生成nonzero `store_instance_id`，RNG错误/short-fill/all-zero在文件mutation前fail-closed，production caller不能指定，测试只能注入明确标记的deterministic exact-width generator。missing file不是empty/reset。Authority initializer在S7-D落地，Controller/Runtime initializer在S7-E落地。Runtime initializer还必须消费release pipeline生成的descriptor与system installer唯一生成、同时byte-identically供Planner/Controller ingress使用的singleton manifest，先验证installed final executable length/SHA/target以及binary compiled actual，再把exact canonical descriptor/manifest bytes+digests与store-pinned identity写入sequence 1；每次startup只从validated snapshot取得expected，并重新读取binary compiled id/table作逐字段校验，不从side file/config取第二authority，也不重新hash executable。任何可编辑config都不能覆盖compiled actual。Authority signing key、Controller request-auth key、Runtime verification policy、service account/socket ACL由外部installer预置并绑定fingerprint，private key不进journal或普通env；若initializer/CLI/config形成public surface，随真实entrypoint同批治理登记。

POSIX reference要求Authority、Controller、Runtime使用不同service account/ACL。Authority acquire transcript绑定scope/writer/operation/principal/nonce并同时验证peer credential、socket ACL、request signature与allowlist；unauthorized local client不能推进epoch。same-uid/root compromise不在其安全保证内。

checksum、store identity 与 snapshot sequence 只覆盖 torn/corrupt/mismatch 和受支持 crash model；没有外部 high-water 时不能识别同一 store 被完整替换成旧但有效的 snapshot，S7 reference 不声明 hostile rollback/backup rollback protection。

### 5.2 必测 crash window

| 注入点 | 重启后唯一合法结果 |
| --- | --- |
| temp create 前 | old snapshot remains authoritative |
| partial temp write | temp ignored/removed only after identity-safe inspection；old snapshot remains |
| temp create/write/file-fsync 明确失败 | `RejectedBeforePublish`；old snapshot authoritative，operation 未提交 |
| temp file fsync 成功、rename 前 | old snapshot authoritative；完整 temp仍非权威 |
| rename 前 | old authoritative |
| rename 后、directory fsync 前 | recover exact old or exact new according to filesystem evidence；never synthesize |
| rename 已发布/结果未知或 directory fsync 失败 | `UncertainAfterPublish`；owner停服，重启按 active+operation identity裁决 |
| directory fsync 成功后 | new authoritative |
| tenure-only transaction 中 | old，或完整next AdmissionState tenure nonce+proof/principal+fence+supersede/recovery-required tuple；不消费request/temporal/revision |
| higher tenure遇到旧side-effect intent | new fence拒绝旧writer，但new prepare保持blocked；旧action先cleanup到exact-zero，否则quarantine |
| RuntimeHost startup epoch/live invalidation 中 | 不发布bootstrap/query-ready、不接收apply；host/clock generation与old live/deadline/action状态原子换代 |
| full request admission transaction 中 | old，或request/temporal AdmissionState+revision high-water+exact request/Slice+PreparedNoEffects tuple |
| intent/empty-head transaction构造前deadline已到（`now >= deadline`） | 不发布intent/head、不创建resource/callback；normal start=`StartTimedOutBeforeIntentNoEffects`，empty=`StopTimedOutBeforeHeadCommitNoEffects`且old desired head保留。recovery消费唯一attempt并写`RecoveryFailedNotReady { TimedOutBeforeIntentNoEffects, raw.callback=NotInvoked } + ExactZero` permanent latch |
| intent/empty-head transaction构造时未到期，但durable publish后effect前已到期 | normal start没有resource且census exact-zero，以同一个atomic snapshot写raw+selection并terminal `StartTimedOutBeforeHeadCommitExactZero { effect_started=false, raw }`，不存在raw-only中间状态；recovery同样以一个atomic snapshot写raw TimedOut/NotInvoked与permanent `RecoveryFailedNotReady + ExactZero` latch且不callback；live empty已经commit empty head，跳过`on_stop`、durable raw TimedOut/NotInvoked latch后只做owner cleanup，exact-zero为`TimedOutButExactZero { raw }`且head不回滚 |
| active=revision 1、revision 3 已 admitted/prepared 后 writer turnover | revision high-water 保持 3；revision 2即使大于 active也拒绝，revision 4可在 exact CAS下表达合法旧内容 |
| `PreparedNoEffects`或`RecoveryPlannedNoEffects` durable、对应intent前 host crash | resource/callback尚被协议禁止；进程边界+ledger证明zero后旧operation/action终结`AbortedBeforeIntentNoEffects { raw }`，其中`raw.callback=NotInvoked`。normal apply保留old active；recovery可在new host/clock epoch创建fresh action/generations，不设置failure latch |
| `FirstActionIntent`或recovery `StartCallIntent` durable后 host crash | callback/effect不replay；normal apply保留old active并在exact-zero后`AbortedBeforeHeadCommitExactZero { raw }`，recovery发布`RecoveryFailedNotReady` permanent latch；crash前已durable的KnownSuccess/KnownError/TimedOut raw fact必须保留，只有raw latch前crash才是Unknown；ownership不确定则quarantine |
| non-empty start raw known error/timeout、cleanup exact-zero | terminal selection早于deadline且raw为error时是`StartFailedBeforeHeadCommitExactZero { reason, raw }`；raw timeout或selection `now >= deadline`时是`StartTimedOutBeforeHeadCommitExactZero { raw }`；old head不变 |
| higher tenure接管post-intent旧operation、cleanup exact-zero | non-success `SupersededAfterIntentExactZero { raw }`；new effects此前保持blocked，不能把旧operation写success |
| OneSource active+LiveReady terminal commit、reply前 | new desired/live权威；same operation返回byte-identical terminal |
| non-empty active 后 RuntimeHost restart | desired active保留；eligible head仅一次`Recovering/NotReady`→manifest-bound internal reassembly→`LiveReady`；failure latch跨再次restart阻止重复callback |
| empty apply遇到canonical empty/`RecoveryFailedNotReady`且ledger `ExactZero`、无nonterminal action/resource | 单事务构造前先check deadline；未到期才写incoming empty desired head+terminal+`ExactZero`，不创建`FirstActionIntent`/`HeadCommittedRetiringOld`、不调用callback；已到期按`StopTimedOutBeforeHeadCommitNoEffects`保留old head |
| empty apply遇到live/nonzero generation | 第一事务同时写`FirstActionIntent`、`NoNewAdmission`、empty active+`HeadCommittedRetiringOld`+exact old Slice/budgets+`Draining`；从此为post-intent，head不回滚，operation nonterminal且new full apply blocked |
| empty `on_stop` raw known error、cleanup exact-zero | terminal selection早于deadline时是non-success `StopFailedButExactZero { reason, raw }`；selection到/越deadline时是`TimedOutButExactZero { raw }`并保留raw error；不能因最终zero改写为graceful success |
| empty drain 中 host crash | 不replay`on_stop`；纯in-process resources若证明now zero则non-success `InterruptedButNowExactZero { raw }`并保留crash前durable raw fact，只有raw latch前crash才用Unknown；否则quarantine |
| callback/deadline/cancel outcome → cleanup | owner在进入cleanup前atomic durable写bounded、monotonic `RawActionOutcomeLatch`；每个已durable raw fact均immutable，latch包含raw KnownSuccess/KnownError/TimedOut/NotInvoked、`raw_outcome_observed_at`与clock/deadline lineage。它保存事实但不提前选择最终primary terminal。publish uncertain按old/new snapshot裁决，不猜测。start success可以直接与active terminal同一atomic commit；未commit即crash时仍是Unknown |
| error/timeout/crash/supersede组合或deadline equality | terminal全序为invalid invariant/panic/cleanup/ownership uncertainty→quarantine；已durable post-intent supersede→`SupersededAfterIntentExactZero { raw }`；host crash→相应interrupted/aborted exact-zero并携带raw latch；无前述高优先级且cleanup+exact-zero evidence完成后，owner在构造terminal前只采样一次`terminal_selection_observed_at`形成`TerminalOutcomeSelection`，若`now >= deadline`则timeout exact-zero，否则raw KnownError，最后才是success。selection持久后不因fsync/回复跨deadline重分类，raw fact始终保留 |
| controller plan commit、first target apply 前 | committed plan retained；rollout resumes |
| target apply timeout/ack loss | query by operation id/digest；never assume not applied |
| same operation id/different digest | conflict, zero side effects |
| invalid/undecodable/torn/unknown-version snapshot | owner不进入bootstrap/query-ready，不发送authenticated response；仅本地无authority诊断。Controller把连接/服务不可用记为`Indeterminate`，不能从config或corrupt header猜identity |
| validated snapshot + startup-generation commit成功，随后active-head compatibility/recovery/ownership quarantine | 可在exact authenticated host/store/channel identity下返回`Indeterminate { stable reason }`；不能返回`Unknown`或历史Ready。descriptor/manifest/compiled-vs-pinned pre-validation mismatch不属于此类 |

## 6. Governance admission

授权后按真实 surface 出现的同一变更登记；S7-A 的 registry delta 必须是 0 package、0 public API、0 executable、0 waiver，Accepted ADR 只批准后续 admission 模板，不能把未来 package/API/daemon 先写进 `governance.toml`。

- S7-B 不新建 crate、不新增 public API row：只在现有`paraegox-runtime-contracts`内以private/internal module冻结`RuntimeApplyEnvelopeV2`、`RuntimeBuildDescriptorV1`、`RuntimeBuildIdentityV1`、singleton `RuntimeArtifactCompatibilityManifestV1`/projection、Runtime bootstrap/query request+response与channel-auth transcript、narrow profile/zero-or-one Loop grammar、codec、digest domains、bounds、reason taxonomy与Rust/Python vectors，明确保持enabler/incomplete；不实现Thread/Process/Ingress placeholder branch。跨crate producer/consumer与public promotion留到S7-E/F真实两端call path。
- S7-C 最多新增一个`paraegox-decks` crate，把Deck contract与DeckCompiler合一；package只有真实crate出现时登记，但DeckSpec/DeckLock API、Planner/PlanCandidate/allocation delta都保持internal/enabler，不因Compiler→Planner模块调用或test fixture宣称public/implemented。到S7-E出现真实operator DeckSpec ingress和`paraegox-deploymentd` executable call path时，才按实际需要登记/promote DeckSpec input；DeckLock与committed DeploymentPlan若仍只有同owner内部消费者则保持internal。
- Graph Foundation 在 DeckCompiler 与 Planner 两个生产 consumer 证明实际算法交集前保持 0 row；若条件成立，只能抽取无领域/I/O/serialization/digest/state 的 internal leaf crate。
- S7-D可以登记真实TenureAuthority process、owner-local journal/signing和Authority one-shot initializer/provisioning vertical；`acquire_tenure` request/response/framing/auth transcript由现有`paraegox-deployment`拥有，在`deploymentd`真实client尚未出现前保持internal enabler，不登记public API；其中嵌入的`WriterTenureProof` canonical value继续复用`paraegox-runtime-contracts`唯一owner。Controller/Runtime journal codec/state machine同样internal。
- S7-E是首个minimum executable vertical：同批落下release-pipeline descriptor generator、strict system installer和singleton manifest唯一generator、operator target/DeckSpec ingress、`paraegox-deploymentd` committed-plan transaction、Controller/Runtime one-shot initializer与key/policy/principal provisioning、Controller journal/request-auth signer、Authority acquire IPC真实client/public promotion、unique projector/envelope producer，以及现有RuntimeHost binary内嵌compiled identity/table和authenticated bootstrap + strict apply endpoint/journal/assembly consumer。首次apply之前必须具备exact store/build dual binding、per-source revision high-water、active-desired/live分型、single-action gate、normal `FirstActionIntent`、conditional empty two-phase/zero fast path和restart fail-closed NotReady/quarantine；此时才promote/register descriptor/manifest/envelope/narrow successor与真实persistent/public surfaces。
- S7-F在同一Controller owner内补齐Reconciler/RolloutEngine，并在真实Runtime endpoint/call path与first functional tests同批promote/register由`paraegox-runtime-contracts`拥有的versioned/bounded/end-to-end authenticated operation/live query request/response与transcript，以及journal-bound restart reassembly、failure latch、rollout/partial-recovery；分型/high-water不是到F才首次实现。
- TenureAuthority process、journal rows、`paraegox-deploymentd` 与 Runtime endpoint 只在各自真实 implementation/entrypoint/first functional tests 同批登记；A/B/C 不提前建 executable，不新增 waiver，现有 GOV-WAIVER-0001/0002 只在 E/F 的真实连接点收窄或移除。

release descriptor generator默认是internal build-pipeline tool；若它在仓库形成独立executable/package，S7-E仍须登记entrypoint、owner、descriptor output contract和first golden test。system install operation因接收operator exact target/service identity并发布public canonical manifest，必定是S7-E真实operator install/CLI/config surface，必须同批登记Runtime installation transaction owner、strict descriptor consumer、manifest producer、Runtime initializer+Planner/Controller独立consumers、unknown-input fail-closed规则与first functional test。三个owner journal codec/state、one-shot initializer调用与initializer receipt默认是各自owner-internal system-install surface，不形成第二public protocol或CLI；若另行暴露，也必须在真实entrypoint与首个独立consumer同批admit/register，不能借本基线预授权隐藏surface。

单元测试、ADR、wrapper、registration 或为了消费新抽象而同时创建的第二抽象，不独立证明 consumer。

## 7. 授权后的阶段 DAG

1. **S7-A decisions**：ADR-0004/0005/0007/0008 Accepted，governance admission 模板固定，registry 不写 placeholder。
2. **S7-B internal successor foundation**：在`paraegox-runtime-contracts`内private/internal冻结Envelope v2、build descriptor/identity、singleton manifest/projection、Runtime bootstrap/query request+response/transcript、narrow PXTE v4/PXAR v5 codec、Rust/Python vectors与strict compatibility；不public promotion，不实现未消费branch。
3. **S7-C pure compile**：internal最小DeckSpec→DeckLock、typed validation、pure Planner candidate、stable allocation delta与manifest selection；不public promotion。
4. **S7-D tenure vertical + internal persistence foundations**：real Ed25519 TenureAuthority process/journal/initializer/provisioning；在`paraegox-deployment`内实现internal acquire IPC codec/transcript，Controller/Runtime codecs保持internal并先跑crash/corruption/authz Harness。
5. **S7-E minimum executable vertical + narrow contract promotion**：release descriptor generator、strict system installer+singleton manifest唯一generator、operator target/DeckSpec ingress、`paraegox-deploymentd` committed-plan/journal/signer、Authority真实client及acquire IPC promotion、Controller/Runtime initializer、Runtime compiled-actual/store-pinned双校验、authenticated bootstrap/apply、per-source high-water、active/live/single-action/intent/conditional-empty-two-phase journal，以及public Envelope v2/descriptor/manifest/bootstrap/PXTE v4/PXAR v5端到端接通。Runtime restart在F前至少fail-closed NotReady/quarantine。
6. **S7-F query/reconcile + restart reassembly completion**：补齐bounded reconcile_once、timeout、由`paraegox-runtime-contracts`拥有的end-to-end authenticated operation/live query promotion、response journaling/idempotency、manifest-bound single-attempt internal reassembly与partial-rollout recovery。
7. **S7-G system smoke**：revision 1 one-subject activate → owner restarts → revision 2 empty deactivate → exact zero。
8. **S7-H independent review/CI**：security、persistence、ownership 与 Linux process evidence 全绿后才标记 complete。

每步形成独立可回滚提交，只有自己的验证通过后才进入下一步；不能把后续类型提前建成空 package。

## 8. 完成判据

S7 complete 必须同时满足：

- 四份 ADR 已 Accepted 或记录等价的明确授权；
- canonical Deck/Plan/Slice/request 链只有一个 producer truth；
- Runtime 不 import `deployment`/`decks`，DeploymentController 不持有 Runtime object；
- 三个owner store由显式initializer+预置key/policy/principal启动，missing/corrupt不是reset；unauthorized Authority peer/wrong scope/key无法推进epoch；
- journal crash/corruption/unknown-version fault matrix 全绿；
- exact replay 幂等、same-id/different-digest 拒绝、writer turnover 与 CAS 无双 active；
- tenure-only与full admission两个atomic transaction各有old/new crash证据；side-effecting old action未cleanup前不启动new action；
- per-source admitted revision high-water跨 supersede/restart不回退，active 1/prepared 3后 revision 2拒绝、revision 4合法回滚内容可接受；
- `OneSourceLoop`只从None/terminal empty启动且Loop→Loop拒绝；normal apply在任何resource/callback前durable `FirstActionIntent`，intent构造前与durable publish后/effect前各做owner-clock deadline check，fsync跨deadline不启动effect；pre-intent crash证明no-effects，post-intent start-before-head crash不replay callback，old head保留或quarantine；
- start/stop known error、deadline equality、crash与post-intent writer supersede遵守唯一failure precedence；callback/deadline/cancel到cleanup之间先durable写只保存raw fact的immutable `RawActionOutcomeLatch`。无quarantine/supersede/crash时，cleanup+exact-zero后、terminal构造前以owner clock只采样一次`terminal_selection_observed_at`形成`TerminalOutcomeSelection`，`now >= deadline`含等于，之后的fsync/回复不重分类；raw known fact在Interrupted/Superseded/timeout terminal中也不丢失。cleanup exact-zero后各返回明确non-success terminal；ownership不确定时quarantine，不使用泛Failed或普通success遮蔽；
- `EmptyDeactivate`遇到live/nonzero generation时first commit同时写`FirstActionIntent`、`NoNewAdmission`、`HeadCommittedRetiringOld`与old Slice/budgets，exact-zero后才terminal；head/intent构造前过期保留old head，publish后/effect前过期则empty head保留、跳过`on_stop`并cleanup到timeout exact-zero。already exact-zero且无action/resource时deadline precheck通过才走无intent/callback的单事务terminal fast path；interrupted drain不replay`on_stop`，post-intent empty head不rollback；
- empty graceful stop known error/deadline/crash分别保留`StopFailedButExactZero { reason, raw }`、`TimedOutButExactZero { raw }`、`InterruptedButNowExactZero { raw }` non-success taxonomy，只有完整graceful stop+cleanup才是success；
- PXTE v4只有`ReferenceAssemblyProfileV1`两个exact mode并走production-equivalent chain；未消费的Thread/Process/Ingress schema没有借此成为public API；
- release descriptor generator、strict system installer/manifest唯一generator、Runtime initializer和startup形成唯一build evidence链：installer拒绝prebuilt manifest并把同一canonical artifact byte-identically送入Runtime initializer与Planner/Controller，Planner没有第二manifest producer；Runtime sequence-1持久保存exact descriptor/manifest bytes+digests，compiled actual不能被config/store覆盖；bootstrap pin exact RuntimeHostId/store/channel、compiled actual与store-pinned build并在sign/send前journal；store/build变化要求new RuntimeHostId/store；
- PXAR v5 exact expected store进入canonical complete request digest/signature，Runtime以local journal identity在任何admission mutation前比较；Controller-side pin不是唯一防线；
- RuntimeHost restart后desired active与live readiness分离；recovery在资源/callback前先durable `RecoveryPlannedNoEffects`再`StartCallIntent`，pre-intent crash可用fresh action/generations重试，但recovery deadline在intent前或publish后/effect前到期都会消费唯一attempt并写permanent TimedOut failure latch；post-intent failure latch跨再次restart阻止callback replay；
- query绑定client nonce/channel peer/host epoch/snapshot sequence并journal exact response bytes；invalid/undecodable snapshot或descriptor/manifest/compiled-vs-pinned pre-validation mismatch不提供authenticated query/bootstrap，validated startup后的active-head compatibility/recovery/ownership quarantine才可返回authenticated `Indeterminate`；Unknown与Indeterminate分型，历史Active不冒充live Ready；
- Runtime bootstrap/query协议的canonical Schema/transcript/bounds只由`paraegox-runtime-contracts`拥有；acquire-tenure IPC只由`paraegox-deployment`拥有而proof value继续复用runtime-contracts owner；internal codec必须等真实双端call path才promote/register，initializer/receipt没有被暗中变成public surface；
- public RuntimeHost 进程确实拥有 apply/assembly entrypoint，而不是只在 crate-private Harness 调用；
- final live-resource owner census exact zero，同时 canonical empty active desired head 仍可用于下一次 revision/CAS；
- 本地完整门禁、独立复核和 GitHub Linux CI 成功。

在这些证据出现前，S7 状态保持 `pending` 或 `in_progress`，不能写成 implemented/complete。

## 9. 冻结、授权与回执

冻结后的`docs/plans/s7-p2e-baseline-v1.manifest`使用UTF-8、LF、无BOM/尾随空格的TOML。为避免不存在标准定义的“canonical TOML serializer”产生多组合法bytes，本基线进一步冻结exact逐行序列化：ASCII key后恰为一个space、`=`、一个space和值；整数使用无正号、无非必要前导零的最短十进制；hex一律lowercase；所有string都用TOML basic双引号，当前值必须不含quote、backslash、control character或需要escape的code point，否则本次freeze失败而不是选择另一种escape；不允许comment或empty line；每个`[[adrs]]` header独占一行并紧跟前一字段；文件末尾恰有一个LF。下面模板中的`<...>`只表示由verifier替换的raw metavariable，实际文件不含angle bracket，quoted placeholder替换后仍保留模板中的双引号：

```toml
manifest_version = 1
authorization_phrase = "接受 S7/P2e 基线 v1"
code_baseline = "<40-lowercase-hex>"
decision_plan_path = "docs/plans/s7-p2e-admission-and-delivery-plan.md"
decision_plan_bytes = <minimal-decimal>
decision_plan_sha256 = "<64-lowercase-hex>"
accepted_status_transform = "replace-first-exact-proposed-header-v1"
[[adrs]]
path = "docs/adr/ADR-0004-deck-workload-and-application-admission-boundary.md"
proposal_bytes = <minimal-decimal>
proposal_sha256 = "<64-lowercase-hex>"
accepted_bytes = <minimal-decimal>
accepted_sha256 = "<64-lowercase-hex>"
[[adrs]]
path = "docs/adr/ADR-0005-typed-domain-graphs-and-runtime-assembly-boundary.md"
proposal_bytes = <minimal-decimal>
proposal_sha256 = "<64-lowercase-hex>"
accepted_bytes = <minimal-decimal>
accepted_sha256 = "<64-lowercase-hex>"
[[adrs]]
path = "docs/adr/ADR-0007-p2e-reference-journal-and-crash-recovery.md"
proposal_bytes = <minimal-decimal>
proposal_sha256 = "<64-lowercase-hex>"
accepted_bytes = <minimal-decimal>
accepted_sha256 = "<64-lowercase-hex>"
[[adrs]]
path = "docs/adr/ADR-0008-pxte-v4-pxar-v5-subject-ingress-separation.md"
proposal_bytes = <minimal-decimal>
proposal_sha256 = "<64-lowercase-hex>"
accepted_bytes = <minimal-decimal>
accepted_sha256 = "<64-lowercase-hex>"
```

manifest列出本文和四份ADR的exact repository-relative path、byte length与SHA-256；每份expected accepted bytes只把首个exact header line `> 状态：Proposed`替换为`> 状态：Accepted`，这是授权后唯一允许的不重新授权mutation，正文、日期和其他metadata一字节不变。manifest自身不嵌入自哈希或生成时间；授权请求在对话中给出完整manifest的外部SHA-256。三份架构总览与ADR README是派生说明，不替代manifest中的决策输入。

授权短语“接受 S7/P2e 基线 v1”只适用于请求消息所列exact manifest SHA-256以及manifest内的exact文档hash。任何material ADR/plan修改、代码基线变化、路径变化或hash不匹配都使尚未使用的短语失效，必须生成新manifest并重新请求；不能把一次泛化的“继续开发”解释成未来版本的永久授权。

收到有效短语后，唯一持久回执写入`docs/plans/s7-p2e-baseline-v1.authorization-receipt`。它复用manifest相同的UTF-8/LF、key/space/value、integer、hex、basic-string、no-comment/no-empty-line与exact-final-LF规则；`authorization_received_at`额外固定为UTC、无fractional second的`YYYY-MM-DDTHH:MM:SSZ`。exact模板如下：

```toml
receipt_version = 1
authorization_status = "accepted"
decision_maker = "ParaEGOX workspace user"
session_reference = "current-codex-thread"
authorization_received_at = "<UTC-YYYY-MM-DDTHH:MM:SSZ>"
authorization_phrase = "接受 S7/P2e 基线 v1"
authorization_message_sha256 = "<64-lowercase-hex>"
code_baseline = "<40-lowercase-hex>"
manifest_path = "docs/plans/s7-p2e-baseline-v1.manifest"
manifest_sha256 = "<64-lowercase-hex>"
decision_plan_path = "docs/plans/s7-p2e-admission-and-delivery-plan.md"
decision_plan_bytes = <minimal-decimal>
decision_plan_sha256 = "<64-lowercase-hex>"
[[adrs]]
path = "docs/adr/ADR-0004-deck-workload-and-application-admission-boundary.md"
proposal_bytes = <minimal-decimal>
proposal_sha256 = "<64-lowercase-hex>"
accepted_bytes = <minimal-decimal>
accepted_sha256 = "<64-lowercase-hex>"
[[adrs]]
path = "docs/adr/ADR-0005-typed-domain-graphs-and-runtime-assembly-boundary.md"
proposal_bytes = <minimal-decimal>
proposal_sha256 = "<64-lowercase-hex>"
accepted_bytes = <minimal-decimal>
accepted_sha256 = "<64-lowercase-hex>"
[[adrs]]
path = "docs/adr/ADR-0007-p2e-reference-journal-and-crash-recovery.md"
proposal_bytes = <minimal-decimal>
proposal_sha256 = "<64-lowercase-hex>"
accepted_bytes = <minimal-decimal>
accepted_sha256 = "<64-lowercase-hex>"
[[adrs]]
path = "docs/adr/ADR-0008-pxte-v4-pxar-v5-subject-ingress-separation.md"
proposal_bytes = <minimal-decimal>
proposal_sha256 = "<64-lowercase-hex>"
accepted_bytes = <minimal-decimal>
accepted_sha256 = "<64-lowercase-hex>"
```

`authorization_message_sha256`对用户消息可见内容的exact UTF-8 bytes计算，不含传输framing或隐式换行。先写并fsync receipt，再只执行manifest预先计算的四处exact status-line substitution，验证全部current accepted hashes等于expected；partial mutation或任一mismatch都fail-closed，不开始实现。未来验证者可把current Accepted header反向替换为Proposed并重算proposal hash，不依赖ignored docs的Git历史。

receipt不参与已经授权的manifest自哈希，decision plan bytes也不因授权改写；四份ADR全部达到expected accepted bytes且receipt存在后才开始S7公共/持久化实现。回执只证明准入，不证明任何S7代码、测试或运行能力已经完成；任何其他material/metadata变更必须生成新manifest/version和新receipt，不能覆写v1。
