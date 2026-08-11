# ADR-0007 — P2e reference journal 与 crash recovery 基线

> 状态：Accepted
> 日期：2026-07-31
> 决策者：ParaEGOX workspace user（以 `docs/plans/s7-p2e-baseline-v1.authorization-receipt` 为唯一生效证据）
> 关联文档：[ADR-0001 — DeploymentController、DeploymentPlan 与 Runtime 边界](ADR-0001-deployment-controller-boundary.md)、[Kernel Foundation Plan](../plans/kernel-foundation.md)、[CONTRIBUTING](../../CONTRIBUTING.md)

## 一句话结论

提议 P2e 的 `DeploymentController`、`DeploymentTenureAuthority` 与 `RuntimeHost` 各自独占一个版本化、带校验、容量有界、由 OS 排他锁保护的本地 atomic snapshot journal，并以严格 quarantine、operation id/digest 幂等和显式迁移处理受支持 filesystem profile 内的 process crash；该基线不具备外部 anti-rollback anchor，不能识别被完整替换回同一 store 的旧有效 snapshot。当本 ADR header 为`Proposed`且authorization receipt未生效时不得据此实现；即使达到`Accepted`也不表示这些 journal 或恢复能力已经完成。

## 背景

Accepted ADR-0001 已经决定以下不变量：

- `DeploymentController` 按 `DeploymentScope` 单写，原子提交 stable-ID allocation delta、下一 `DeploymentRevision` 与 committed `DeploymentPlan`，并独占 rollout/reconcile 状态。
- `DeploymentTenureAuthority` 独占推进 `DeploymentWriterEpoch` 和签发 `WriterTenureProof`；Tenure Authority signing key 不交给 `DeploymentController`。
- `RuntimeHost` 在任何 prepare 副作用前持久推进 `writer_fence`，并把 `prepared` 与 `active` 分开；只有成功 activate 才能替换 `active`。
- 同一 operation id 与同一 canonical request digest 只能查询或推进同一 operation；同一 id 携带不同 digest 必须 conflict。
- 任一 owner 无法证明最高 epoch、revision 或 operation 状态时必须 fail-closed 或 quarantine，不能从空状态重新计数。

本ADR若被Accepted或由决策包的exact授权短语接受，将**明确、窄幅修订 ADR-0001 §4.1 第162条**，而不是声称两套文字可同时满足：

- `active`在P2e journal中重命名语义为`committed desired head`，与current `live_materialization`和operation terminal分离。`OneSourceLoop`仍只有在readiness成功时原子写desired head+LiveReady+terminal；`EmptyDeactivate`替换current `LiveReady`且exact matching nonzero generation时，必须先原子写canonical empty desired head+`FirstActionIntent`+`HeadCommittedRetiringOld`，再drain，exact-zero后才terminal。已经是`canonical empty + ExactZero`或`RecoveryFailedNotReady + ExactZero`的no-retire fast path则原子写new empty head+`ExactZero`+terminal，不产生action intent。因此ADR-0001“只有activate成功才替换active”的句子只在前述head-first live-retire路径上由本ADR替代。
- `Superseded`只适用于尚未durable `FirstActionIntent`的prepared operation；越过或可能越过副作用边界的旧operation只能进入`SupersededReconcileRequired`，new fence阻止旧writer但new effects保持blocked，直到exact-zero terminal或quarantine。因此ADR-0001把所有不能继续的prepared统一写成`Superseded`的句子由本ADR细化替代。
- 除这两个明确列出的state-machine修订外，ADR-0001的owner/import/single-writer/tenure/CAS/revision/operation-id边界全部保留。若本ADR仍为Proposed且没有exact授权记录，ADR-0001原文继续是当前权威，实现不得提前采用该修订。

Kernel Foundation P2e 进一步要求单 Node reference 完成 plan→commit→project→prepare→ADR-0008 `ReferenceAssemblyProfileV1` activate/deactivate→observe→reconcile，并覆盖 Controller restart、Runtime crash、partial apply、timeout 与重复提交。当前仓库已存在 projection/apply contract 和 private/experimental Runtime mechanisms，但 `governance.toml` 明确记录 durable RuntimeHost journal、DeploymentController，以及该narrow S7 profile的exact one-subject/empty lifecycle vertical与restart reassembly仍未实现；本ADR不据此声称通用RuntimeAssemblyEngine已经存在。

持久格式属于 Architecture/Runtime-state 变更。若没有 Accepted 决策就直接实现，会在以下 crash window 中留下互相冲突的隐式规则：

- allocation 已分配但 revision/plan 未提交；
- tenure proof 已签发但 epoch high-water 未持久，或已持久但调用方未收到回复；
- Runtime 已接受更高 writer tenure，但 incoming revision 尚未 prepare；
- prepare 或 activate 已产生局部副作用，但 `active` 尚未提交；
- owner 已原子替换状态文件，但在 directory fsync 或回复调用方之前 crash；
- active snapshot 损坏、截断、版本未知或被误删，而目录中仍留有 temp/backup 文件。

P2e 需要一个足够小、可故障注入的 reference baseline，而不是提前建设 WAL database、分布式 consensus 或 HA store。本 ADR 因此提议 owner-local full-snapshot journal。它依赖受支持本地文件系统对 same-directory atomic rename、file fsync 和 directory fsync 的明确语义；未验证的 network/overlay filesystem 不在 reference profile 内。

## 范围与非目标

本 ADR 提议决定：

- `DeploymentController`、`DeploymentTenureAuthority` 与 `RuntimeHost` 的持久状态所有权和互斥边界；
- P2e local reference journal 的 envelope、容量上限、atomic replace protocol、首次初始化和启动恢复规则；
- Controller allocation/plan/sign/rollout、Authority tenure/proof、Runtime host/clock generation、AdmissionState、per-source admitted revision high-water、`writer_fence/prepared/active` desired-head、live materialization 与 owned-resource tombstone 的最小持久内容；
- operation id/digest 幂等、corrupt/torn/unknown-version quarantine 和 crash matrix；
- fault injection、格式迁移、owner decommission 与未来 HA backend 的替代条件。

本 ADR 不决定：

- Deck、DeploymentPlan、RuntimePlanSlice、RuntimeApplyRequest 或 WriterTenureProof 的公共 Schema；这些继续由各自 contract owner 和 ADR-0001 管理。
- Deployment planner、placement、rollout policy 或 RuntimeAssemblyEngine 的完整业务算法。
- 多副本 DeploymentController、leader election、consensus、跨 Site failover、replication、backup service 或 network filesystem profile。
- ProcessDomain 的 cgroup/job-object/pidfd、host `SIGKILL` 后 orphan containment、恶意本机 privileged writer 防护，或物理 effect owner 的 durable ledger。
- 通用 storage crate、database abstraction、generic repository、第二个 state writer 或 silent compatibility layer。
- 从当前代码推导出 journal 已经存在；在本 ADR Accepted 且实现证据完成前，所有下述内容都只是候选约束。

## 决策

以下规则只有在本 ADR Accepted 后才成为实现约束。

### 1. 三个 owner，三个排他 journal

| Journal owner | 唯一可写状态 | 明确禁止写入 |
| --- | --- | --- |
| `DeploymentController` | stable-ID allocation/high-water、committed plan/revision/digests、request-auth key/algorithm ref、exact signed apply request、controller operation ledger、per-target rollout/observed/reconcile facts | Tenure Authority signing private key、tenure epoch authority、Runtime `writer_fence/prepared/active`、Runtime lifecycle handle |
| `DeploymentTenureAuthority` | per-scope epoch high-water、tenure-acquire operation、writer claim、已签发 proof envelope/digest 与 key/algorithm ref | committed plan、rollout、Runtime active state；journal payload不得保存 signing private key |
| `RuntimeHost` | RuntimeHost/clock generation high-water、完整 replay/temporal `AdmissionState`、per-source admitted revision high-water 与 `writer_fence`、exact admitted request/Slice、apply operation/prepared phase、active desired head、live materialization、bounded owned-resource recovery facts 与 terminal result refs | editable DeploymentPlan、allocation、rollout decision、authority signing key |

每个 owner 使用独立目录、固定 lock file 和固定 active snapshot path。一个 owner 不写另一个 owner 的目录，也不通过 shared database transaction 合并逻辑 authority。三个 journal 之间不存在跨文件系统事务；跨 owner 的 uncertainty 由 tenure proof、operation identity、digest、CAS、query 和 reconcile 解决。

同进程共置不改变上述边界。测试 fake 可以在进程内运行，但必须使用不同 store instance/lock/state，并通过与真实 owner 相同的 transition invariant。

### 2. 固定 lock 与受支持存储 profile

- active owner 启动后先在固定、永不 rename 的 lock file 上取得 OS exclusive lock，并由唯一 owner handle 持有到 owner 退出。无法取得锁时在读取或产生任何业务副作用前失败；不得退化为只读后继续服务或使用第二个 lock path。
- lock handle 必须 non-inheritable：POSIX reference 至少以 `O_CLOEXEC` 打开，并在所有 spawn/fork/exec file-actions 中显式验证或关闭；任何 ProcessDomain、reference worker、RuntimeHost child或 helper 都不得继承能延长 lock lifetime 的 file description/handle。所选 `fcntl`/`flock`/platform primitive 的 fork、exec、duplicate、close 与 crash release 语义必须按目标 OS 单独证明，不能只称“process-scoped”。
- reference POSIX profile 使用 non-blocking exclusive advisory lock，并同时依赖目录 owner/permission 阻止不遵守锁协议的普通进程。checksum 不提供对 privileged local attacker 的认证。
- reference POSIX deployment必须让TenureAuthority、DeploymentController与RuntimeHost使用不同OS service account/安全主体；各journal/key目录和local control socket使用owner/group ACL隔离。Authority signing key handle只对Authority principal可用，Controller request-auth key handle只对Controller principal可用；平台支持时使用non-exportable handle，文件型reference key也至少由独立account和最小权限保护。若部署把它们放在same uid/root compromise域内，只能声明逻辑owner分离，不能宣称抵抗该principal读取key/store。
- `acquire_tenure` 的local IPC必须versioned、bounded、authenticated且authorized：使用与apply/bootstrap/query不同的protocol version与domain-separated signing transcript，签名覆盖scope、writer ref、operation id/digest、Controller principal/key selector/fingerprint、client nonce、response bound及所有canonical fields；Authority同时验证identity-bound channel的peer credential、socket ACL、request signature与exact scope/writer allowlist。任意普通local client、wrong peer uid/key、wrong scope/writer、replay conflict或oversized request都不能推进epoch。transport connect/ACK不等于tenure commit。
- protocol ownership不得由endpoint实现临时复制：`acquire_tenure` request/response canonical value、framing、auth transcript/digest domain、bounds与stable rejection taxonomy由现有`paraegox-deployment`唯一拥有，Authority executable是server consumer/producer，`paraegox-deploymentd`是唯一production client；其中嵌入的`WriterTenureProof` canonical value继续由`paraegox-runtime-contracts`唯一拥有。authenticated Runtime bootstrap/query request/response、channel-auth transcript/digest domain、bounds与stable rejection taxonomy由`paraegox-runtime-contracts`唯一拥有，RuntimeHost endpoint与DeploymentController client只能消费这些exact contracts，不能各自定义近似wire。S7-D/S7-E/S7-F的internal→public promotion仍必须分别等待真实双端call path和first functional tests。
- lock file、active snapshot 与 temp 必须是预期目录中的 regular file；实现拒绝 symlink、跨目录 rename、路径穿越和 owner identity 不匹配。
- journal 只支持经 fault suite 验证、能提供 same-directory atomic rename、file fsync 和 directory fsync 的本地文件系统。能力无法证明时启动失败；NFS、SMB、未验证 overlay 或 object-store mount 不得以“通常可用”加入支持矩阵。
- 在所有 child 都已证明未继承 lock handle 后，OS 释放 crash owner 的 lock 只表示新进程可以开始恢复，不表示旧状态完整。新 owner 仍须从头严格验证 active snapshot。

### 3. versioned/checksummed bounded snapshot envelope

P2e 不采用 append-only WAL；每次持久 mutation 都重写一个完整、容量有界的 owner snapshot。共同 envelope 至少包含：

- 固定 magic 和 journal envelope version；
- owner kind 与 owner-specific payload version；
- 随首次显式初始化生成且不复用的 `store_instance_id`；
- expected owner/scope/target identity fingerprint；
- 单 journal 单调递增、不得回绕的 `snapshot_sequence`；
- exact payload length；
- checksum algorithm/version；
- domain-separated SHA-256 checksum，覆盖除 checksum 字段自身外的完整 header 和 exact payload。

payload 使用严格、确定性的 big-endian length-delimited encoding。每个 version 固定字段顺序、必填字段、enum 值、count 与 byte bounds；duplicate、trailing bytes、unknown enum/field、非 canonical ordering、长度溢出和 owner mismatch 均拒绝。实现必须先验证 envelope 和声明长度上限，再分配 payload buffer。

checksum 只用于检测 torn write、bit corruption 和错误文件，不代替 request authentication、WriterTenureProof 或文件系统权限。`snapshot_sequence` 也不是 DeploymentRevision、writer epoch 或跨 owner clock，不能用它推导业务 authority。

active snapshot 本身没有独立、受信的 sequence/epoch high-water 可供重启时比较。因此 checksum、`store_instance_id` 和 `snapshot_sequence` 能发现 torn/corrupt/mismatched 文件以及当前进程内的错误 transition，但不能识别 privileged operator、旧 backup、device rollback 或其他外部动作把 active path 完整替换成同一 store identity 的较旧、仍 canonical 且 checksum 正确的 snapshot。P2e reference threat model 明确排除这种完整旧快照回放；需要覆盖该威胁的部署必须增加 TPM/secure counter、受信远端 high-water 或等价 anti-rollback authority，并由后继 ADR 定义，不能把 checksum 冒充该能力。

### 4. 初始 reference limits

初版常量是兼容性与故障边界的一部分；实现不得从不受信输入或环境变量无上限放大它们。

| Bound | P2e reference candidate |
| --- | --- |
| Controller active snapshot | 16 MiB |
| Tenure Authority active snapshot | 1 MiB |
| RuntimeHost active snapshot | 16 MiB |
| active source scope per RuntimeHost | 1，与 ADR-0001 的 P0–P5 reference profile 一致 |
| target/rollout records per Controller scope | 1，与 P2e single-Node reference 范围一致 |
| stable allocation records per Controller scope | 4096 |
| retained Controller operation records | 256 |
| managed scopes per local Tenure Authority | 1，与 P2e single-scope reference 范围一致 |
| retained tenure-acquire records per scope | 64 |
| nonterminal Runtime apply operation per source scope + target | 1 |
| retained Runtime terminal operation records | 256 |
| retained Runtime tenure nonce records | 256 |
| retained Runtime request nonce records | 256 |
| retained Runtime temporal lineage records | 256 |
| Runtime owned-resource recovery records | 4096 |
| recognized orphan temp files scanned per owner directory | 32 |

所有嵌入的 canonical request/plan body还必须满足其 contract 自身更小的 byte/count limit。达到任一 journal limit 时，owner 在 mutation 或副作用前返回稳定的 `CapacityExceeded` 类错误，保留旧 snapshot，不静默 eviction、截断、覆盖未 reconciled operation 或创建隐藏 side store。

P2e 不以无界运行作为目标。初版没有自动 compaction；达到 retained-operation 上限后必须显式停写并由后继格式/工具安全迁移。任何 compaction 设计都必须证明 operation-id conflict 与 epoch/revision high-water 不会因删除历史而失效。

### 5. atomic replace protocol

每次 mutation 在持有 owner lock 的同一进程内严格执行：

1. 读取并完整验证当前 active snapshot；基于内存中这个 exact validated state 检查 expected sequence、operation id/digest、epoch/revision、CAS 和容量。
2. 构造下一完整 snapshot，`snapshot_sequence + 1`；在任何文件 mutation 前完成 canonical encoding、bounds 和 checksum。
3. 在同一目录用 `create-new/O_EXCL` 创建 owner 限定、不可预测名称的 temp regular file；权限至少拒绝其他普通用户写入。
4. 写完 exact bytes，检测 short write，并对 temp file 执行 fsync。任何错误都不得 rename。
5. 用同目录 atomic rename 把 temp 替换为固定 active path。
6. 对包含目录执行 fsync。
7. 只有 directory fsync 成功后，才能向调用方确认 durable commit 或发出依赖该 mutation 的外部副作用。

不允许原地覆盖 active、先 truncate active、只 flush 用户态 buffer、跨文件系统 rename、在 directory fsync 前回复成功，或把 `.tmp`/`.bak` 当作隐式权威。

temp 在 rename 前从来不是 authority。启动发现 temp 时，只有 active snapshot 完整有效后，才可在 owner lock 下按严格名称/regular-file/数量规则清理或隔离 temp；超过 orphan-temp scan 上限时 quarantine，不做无界目录扫描。不得因 temp 的 sequence 较高、checksum 正确或修改时间较新而自动 promote。active 缺失或无效时，即使 temp/backup 看起来完整，也必须 quarantine，等待显式 repair/migration 决策。

失败必须按是否可能越过 publish point 分型：

- temp create/write/temp-file-fsync 失败，或 rename 明确返回且目标平台契约能证明 namespace 没有变化时，结果是 `RejectedBeforePublish`：old active 唯一权威，operation 未提交。owner 可以因 I/O health 停服并保留 temp 证据，但不得把确定未提交冒充 `Uncertain`；恢复后可在重新验证 old state 后安全重试同一 intent。
- rename 已成功、rename 结果在目标平台上可能已发布但调用方无法判断，或之后 directory fsync 失败/结果未知时，结果是 `UncertainAfterPublish`。当前进程不得继续签发 proof、发送 apply、激活 revision 或把内存状态报告为 durable；它停止 owner 服务并进入 quarantine。重启后只按 active snapshot 和 operation id/digest 重新判断 old/new commit。
- directory fsync 成功后才是 durable commit；回复调用方前 crash由新 snapshot中的 operation identity提供幂等查询。

### 6. 显式初始化，不从“文件不存在”猜空状态

正常 owner 启动要求 active snapshot 已存在且可验证。首次建立 store 必须使用显式 initialization operation：

- 调用方只提供 expected owner kind、scope/target identity与storage path；initializer必须从目标OS认可的CSPRNG一次取得exact 32 octets（256 bits）并生成nonzero `store_instance_id`，在sequence 1与receipt中一次性持久化后绝不复用。RNG不可用、short fill、返回all-zero或API无法证明cryptographic quality时必须在任何journal file mutation前fail-closed；不能降级为timestamp、PID、path hash、PRNG或caller input。生产接口不接受caller指定identity；测试专用dependency injection可以提供deterministic exact-32-octet generator，但生成结果仍必须经过nonzero/fresh-store校验并明确标为fixture；
- initializer 要求目录处于经验证的全新/已授权状态并取得同一exclusive lock。它使用唯一initialization variant：确认active path不存在且目录除允许的fresh lock/install metadata外为空，直接构造sequence 1 → `O_EXCL` temp → exact write/file fsync → atomic rename → directory fsync；若active已存在则拒绝。publish point后的ambiguous rename/directory-fsync仍按`UncertainAfterPublish`停服并由strict restart验证，不能重跑initializer；
- initialization 产生可审计 receipt；owner 之后只接受配置中 exact store/owner identity；
- 已初始化目录中 active snapshot 缺失、被清空或 identity 不匹配时 quarantine，绝不重新 initialize；
- Controller/Authority 的 revision/epoch high-water 和 Runtime writer fence 绝不从默认 0 推断为“此前没有状态”。

“删除文件后重启”不是 reset API。测试若需要新状态，必须创建新的临时目录和新的 store instance，而不是复用生产 identity。

reference首次安装还必须由operator/system installer在initializer之前显式预置：Authority signing key handle及public verification manifest、Controller request-auth key handle及public key、Runtime verification/admission policy与local-channel ACL、三个service principal和expected fingerprint，以及ADR-0008 strict-verified exact canonical `RuntimeBuildDescriptorV1` bytes+digest。system install operation严格验证descriptor与实际installed executable，并作为singleton `RuntimeArtifactCompatibilityManifestV1`唯一production producer，只从该verified descriptor/artifact、operator选择的exact Runtime target/service identity和binary compiled exact fixture/compatibility table生成一份canonical manifest artifact；它不得接受任意prebuilt/editable manifest bytes。该同一exact artifact必须byte-identically交给Runtime initializer和operator/Controller/Planner immutable ingress，Planner不得另行重建。

Runtime initializer是installed artifact与installer output的独立consumer，也是唯一在store initialization时重新读取并验证final RuntimeHost executable length/SHA-256/target的阶段；它还必须从binary内不可由config/journal覆盖的compiled `build_instance_id`与exact compatibility table计算actual compatibility digest，逐字段验证descriptor、installer-generated manifest、installed artifact与target/store binding。全部验证成功后，initializer才把descriptor canonical bytes+digest、manifest canonical bytes+digest、derived `RuntimeBuildIdentityV1`及exact key/policy/channel fingerprint直接写入sequence-1 snapshot和receipt；不能只存identity/fingerprint，也不能从普通config或待写journal把expected值回显成actual。sequence-1 pin后normal startup只读journal exact bytes，不再读取installer side file；bootstrap只校验Controller pin与Runtime pinned artifact一致，不能掩盖第二producer。private key不进journal、不从普通环境变量或通用配置导出，也不允许owner每次启动随机生成新key。测试key/build identity必须标为fixture且不能进入deployment profile。key/build rotation在P2e不是普通restart能力，只能走显式migration/decommission。release descriptor generator保持internal build tool；含operator target/service selection的install operation是S7-E真实operator CLI/config/install entrypoint，必须与owner、consumers和first tests同批治理登记。

三个initializer都是owner-specific、one-shot install/admin operation，不是正常服务的reset endpoint：S7-D为Authority提供fresh-directory+lock+identity+key-fingerprint初始化，S7-E为Controller/Runtime提供对应初始化；每次都产生auditable receipt。若实现暴露CLI/config/public protocol，必须与initializer、owner和first functional tests同批治理登记；若只存在system-install/test harness mechanism，文档必须说明常规binary在未初始化store上不可启动，不能把Harness冒充通用运维API。

### 7. owner-specific atomic state

#### DeploymentController

Controller payload 至少分为以下 owner-local section，但它们仍由同一个 Controller snapshot transaction 提交：

- `allocation`：stable logical key→ID、allocation high-water 和防止旧 identity 意外复用所需 tombstone；
- `plan`：当前 committed scope/plan identity、DeploymentRevision、PlanContent/PlanContentDigest、DeploymentPlanDigest，以及生成 exact projection 所需的 canonical committed content；
- `controller_operations`：operation id、canonical intent digest、expected revision、phase、terminal/uncertain result ref；
- `request_auth`：Controller 自己的 request-auth key ref、algorithm/version、验证 key fingerprint 与 rotation generation；OS-protected signing private key/handle 不写入 journal，更不得复用 Tenure Authority signing key；
- `target_binding`：每target的exact RuntimeHostId、expected Runtime `store_instance_id`、endpoint/channel-auth profile fingerprint、committed target `RuntimeArtifactCompatibilityManifest` digest、first/last accepted RuntimeHostEpoch以及latest bootstrap response bytes/digest；
- `rollout`：每 target 的 projected source/slice digest、apply operation id/digest、exact canonical signed PXAR request bytes、request-auth key ref、exact canonical authenticated query response bytes/digest与channel peer identity、prepared/active/retired/uncertain observed fact、Receipt ref 和引用query id/snapshot sequence的reconcile decision input。

验证 `DeploymentPlanCandidate` 后，allocation delta、下一 revision、完整 committed plan 和 plan digest 必须在一次 snapshot mutation 中同时出现。不得先占用 ID、再单独推进 revision，或在 plan commit 失败后从 observed Runtime state反推 allocation。

plan commit 与后续 per-target rollout 是不同 transaction。Controller在首次sign/send前先通过S7-E authenticated bootstrap把target binding和response digest durable commit；后续bootstrap若`store_instance_id`或endpoint-auth fingerprint改变，返回`OwnershipTransferRequired/Indeterminate`，绝不把同一RuntimeHostId的新空store当fresh target。P2e不允许same RuntimeHostId reinitialize到new store：decommission必须永久封存旧target+store pair并分配全新RuntimeHostId。ADR-0008 PXAR v5把该durable pinned `expected_runtime_store_instance_id`加入canonical request、complete request digest与Controller signing transcript；Runtime在任何AdmissionState/fence/revision mutation前exact比较local store identity，所以即使误复用RuntimeHostId，延迟旧v5 request也由store mismatch拒绝。PXAR v1–v4保持不变，未来若要让旧profile支持same-target store transfer，仍需successor control。

纯 RuntimeApplyEnvelopeBuilder 保持无 key；DeploymentController 使用自己的 OS-protected request-auth key handle 签署 exact transcript，把 operation id/digest、nonce、完整 signed request 与 key ref durable commit 后才可发送。该 signer 是 Controller 内的窄 mechanism，不是第二 desired-state owner或独立 signer service。Controller 在 Runtime timeout 或自身 crash 后先以同一 operation/query 或 byte-identical signed request reconcile，把exact canonical authenticated query response bytes（或足以byte-identically重建的全部immutable fields）、digest、channel peer identity、epoch/sequence先commit，再以引用该query id/snapshot sequence的transaction更新rollout；重新query得到的新snapshot不能被悄悄替换为旧decision input。send/transport ACK 不能直接写成 target active。Controller restart 只取得新 tenure，不能因此修改 committed plan revision/source digest/slice digest。

request-auth key rotation 必须与 Runtime trust-policy migration协调，并在旧 key 仍可验证历史 operation 时显式切换；不能在 timeout 后换 key/nonce重签同一 operation，也不能把 Tenure Authority key交给 Controller。key handle/ref 不匹配、历史 request 无法 byte-identically恢复或 signer 输出无法验证时，Controller停止 rollout并 quarantine。

#### DeploymentTenureAuthority

Authority payload 至少保存：

- 每个 scope 的最高已提交 `DeploymentWriterEpoch`；
- tenure-acquire operation id 与 canonical request digest；
- exact writer ref、`supersedes_through_epoch`、nonce、authority/key/algorithm version；
- 已签发 proof envelope或足以 byte-identically 返回它的 canonical bytes，以及 proof-envelope digest；
- terminal issuance status。

signing private key 位于 Authority 独占的 OS-protected key handle/store，不写入 journal，也不暴露给 Controller。`acquire_tenure` 在锁内构造 next epoch/claim/proof，先把 epoch 和 exact proof atomic commit，再返回 proof。crash 发生在 commit 之后、回复之前时，相同 acquire operation id/digest 返回已持久的同一 proof；不得为同一 operation 再推进 epoch，也不得在同一 epoch 签发不同 proof。

无法读取 epoch high-water、key ref 不匹配、签名算法/version 未知或 proof record checksum 失败时，Authority quarantine；它不能从 epoch 1 重新开始。

#### RuntimeHost

Runtime payload 至少严格分开：

- `host/clock/admission`：已经 durable 使用的非零 `RuntimeHostEpoch` high-water、固定 owner-local `ClockDomainRef`、已经 durable 使用的非零 `ClockGeneration` high-water、sequence-1起直接持久且后续byte-identical保留的strict-verified exact canonical `RuntimeBuildDescriptorV1` bytes+digest与singleton `RuntimeArtifactCompatibilityManifestV1` canonical bytes+digest、由它们导出的store-pinned `RuntimeBuildIdentityV1`、每次进程从binary只读compiled `build_instance_id`与exact compatibility table得到并验证的actual identity（Runtime executable SHA-256/build identity与fixture artifact digest保持独立）、exact admission/trust-policy fingerprint，以及完整、canonical、容量有界的 replay/temporal `AdmissionState`；后者包含 tenure nonce identity→proof digest、request nonce identity→complete request digest 和 temporal constraint identity→scope/target/original+remaining budget/installed deadline lineage，不能逐 map 合并或从空状态重建；
- `writer_fence`：每个 accepted source scope 的最高 PlanWriterRef、PlanWriterEpoch、proof-envelope digest 和 authenticated principal；
- `source_revision_high_water`：每个 accepted source scope 已通过完整 auth/target/temporal/exact-active CAS 与 semantic admission、并被 durable admitted 的最高 source revision；它独立于当前 active revision，prepared 被 supersede/rollback、empty activate、writer turnover 或 Runtime restart 都不能降低；
- `prepared`：至多一个 target-local nonterminal apply operation，保存 operation id、canonical request digest、exact canonical PXAR request bytes（其中唯一携带 exact incoming RuntimePlanSlice bytes）、incoming source revision/plan/slice digests、expected-active CAS、authenticated temporal constraint、installed local deadline lineage、`PreparedNoEffects/FirstActionIntent/HeadCommittedRetiringOld`等phase或intent marker、下文定义的bounded `RawActionOutcomeLatch`、resource-ownership/cleanup facts和uncertainty；只有retire current live/nonzero generation的empty apply进入`HeadCommittedRetiringOld`，此时还必须保留被替换的exact old active Slice、其signed lifecycle budgets、old live generation与callback/cleanup phase，直到terminal exact-zero，不得另存可独立变化的第二份incoming或old Slice；
- `active`：最后一次已 durable 选择的 desired head，只保存 source revision/plan/slice/target-manifest digests、exact canonical active Slice 与产生它的 operation/result ref；canonical empty target也必须保留，资源exact-zero后不能把它改成`None`。current/historical ownership generation、draining refs和result payload不属于active；
- `recovery_action`：至多一个 journal-bound internal restart action，绑定 exact active Slice/digest、target `RuntimeArtifactCompatibilityManifest` projection、new RuntimeHostEpoch/ClockGeneration、new Domain/Instance/resource generations、deadline lineage、`RecoveryPlannedNoEffects/StartCallIntent`等phase、同一bounded `RawActionOutcomeLatch`与永久failure latch；它不是apply operation或desired revision；
- `live_materialization`：active desired head在当前RuntimeHostEpoch下唯一的materialization index，保存 `Recovering/NotReady`、`LiveReady`、`RecoveryFailedNotReady`、`Draining`、`ExactZero`或`Quarantined`、freshness、current resource generation以及所引用的prepared/recovery action ref；它不是第二desired head，也不能从历史apply Receipt推导；
- `owned_resources`：RuntimeHostEpoch 下每个 Domain/Instance/resource slot 的 exact generation、tombstone、OS identity、workspace/containment/cleanup phase 与 terminal ownership evidence；不能只保存当前 live generation 后在 empty retire 或 restart 时复用旧 slot identity；
- `terminal_operations`：同 operation 的 canonical terminal result body，或足以 byte-identically 重建它的全部 immutable fields，至少绑定 outcome、desired-head digest、resource-census digest、completion RuntimeHostEpoch/snapshot sequence与Receipt digest；历史 terminal result与current live readiness始终分开查询。

`active`、`prepared/recovery_action`、`live_materialization` 与 `owned_resources` 不得复制可独立修改的 ownership truth：live index只能引用exact action/resource generation，详细phase和census只由被引用record拥有；snapshot validator必须检查所有digest/ref/generation双向一致，悬空、重复authority或不一致即quarantine。

RuntimeHost 每次进程启动都必须先完整验证snapshot envelope/payload以及其中descriptor/manifest的exact canonical bytes、各自digest、derived store-pinned identity与所有结构性cross-ref，再从当前binary只读compiled `build_instance_id`/exact compatibility table逐字段比较descriptor、manifest和store-pinned `RuntimeBuildIdentityV1`。这里的pre-validation包含active Slice自身的strict decode/canonical bytes/digest、内部manifest projection与record refs一致性，但不把“内部完整的active head是否仍等于store-pinned supported row”混成结构损坏。normal startup不重新hash executable；final executable length/SHA-256/target的证据只来自initializer对installed artifact的一次性验证和sequence-1持久bytes。替换binary若带distinct compiled build id/table会在startup generation mutation前稳定拒绝；能替换binary且伪造相同compiled id/table的privileged attacker属于本reference既定out-of-model威胁。missing/corrupt/undecodable descriptor或manifest、digest/field mismatch、compiled-vs-pinned mismatch、active Slice malformed/noncanonical/internal-ref mismatch都必须在startup generation mutation、bootstrap/query-ready、callback或resource create前fail-closed。

只有上述验证全部成功，RuntimeHost才可在接收 ingress 前，通过一次 atomic snapshot mutation同时把RuntimeHostEpoch与clock generation high-water推进到从未durable使用过的新值，并把旧epoch的`LiveReady`无条件invalidate为neutral `NotReady`，把prepared deadline、nonterminal action与recovery eligibility转换为新snapshot的明确`Expired/RecoveryRequired/failure-latched`状态；该事务不得提前写`Recovering`，只有下文post-start active compatibility检查通过且head eligible后才可进入`Recovering/NotReady`。任一overflow、丢失、ref不一致或中间结果无法证明时quarantine。任何bootstrap、apply或query-ready fact都只能在该事务durable后发布，绝不能出现“new host epoch + old LiveReady”。当前clock domain/generation必须经受信bootstrap/query fact提供给request producer，不能继续接受写给旧generation的temporal constraint。在新的RuntimeHostEpoch下，纯进程内Domain/Instance generation可以从其初值开始，但任何可能跨host crash存活的Process、workspace、containment或外部句柄都必须由持久resource slot generation/tombstone区分并先reconcile；不能因host epoch改变就假设旧资源消失。

S7-E 首个apply之前必须同批提供最小versioned/bounded/authenticated bootstrap read。request使用独立protocol version/domain-separated transcript，绑定target/source scope、Controller principal/key selector、client nonce和response bound；response在identity-bound channel中回显nonce/request digest并绑定channel peer、exact target、store instance、RuntimeHostEpoch、ClockDomainRef/current ClockGeneration、binary-compiled actual `build_instance_id`/compatibility digest、store-pinned complete `RuntimeBuildIdentityV1`、manifest/profile fingerprint与admission-policy fingerprint。Controller先验证nonce/transcript/channel peer、compiled-vs-pinned一致性与committed facts，再journal exact response bytes+digest。bootstrap只提供构造request所需current fact，不查询operation/live state或成为协商authority；stale clock/host/store/build fingerprint生成的request在request nonce、temporal lineage、revision high-water或prepared被消费前稳定拒绝。S7-F才增加完整operation/live query。

bootstrap/query protocol有独立于apply的service-readiness gate。只有active snapshot已经完整strict decode/validate、descriptor/manifest canonical bytes与digests验证成功、binary compiled identity与store-pinned truth匹配，且本次startup generation/live-invalidation transaction已经durable commit，Runtime才可绑定listener或产生authenticated bootstrap/query response。missing/corrupt/undecodable/unknown-version snapshot或任何无法建立exact store/host/epoch/sequence的pre-validation quarantine都不得回答authenticated `Indeterminate`，也不得回显外部config中的expected identity；调用方只能观察到fail-closed service unavailable。只有建立在validated snapshot之上、startup transaction之后产生的active-head compatibility quarantine、recovery failure latch或ownership uncertainty，才可保持apply disabled并以pinned identity返回authenticated bootstrap NotReady/quarantine fact和query `Indeterminate { stable reason }`。

旧 clock generation 的 temporal lineage 继续保留用于冲突检测与审计，但其 MonotonicDeadline 绝不能换算、刷新或延长到新 generation。旧 generation 下未 terminal 的 prepared operation 不得继续 activate，只能进入 typed expired/recovery 状态，按 exact ownership facts bounded cleanup、reconcile 或 quarantine；已 terminal operation仍可查询。

更高 tenure 使用一个明确拆分的 typed transition，而不是复用“完整 request 已 admitted”的现有状态：

1. tenure preflight先完成 exact canonical envelope、local target/source-scope、ADR-0008 v5 expected Runtime store identity、trusted selector、request signature/principal binding、WriterTenureProof、tenure nonce identity/capacity与proof-envelope digest验证；unauthenticated、wrong-target、wrong-store或wrong-scope input绝不能推进fence。
2. 一个 tenure-only atomic snapshot写入完整next `AdmissionState`（只新增该tenure nonce，request nonce/temporal lineage保持byte-identical）、exact proof/principal与new `writer_fence`。若旧prepared尚未durable `FirstActionIntent`，可在同事务终结为`SupersededBeforeEffects`；一旦intent可能越过callback/resource边界，只能标`SupersededReconcileRequired`，并阻塞任何新prepare/assembly effect，直到bounded cleanup产生exact-zero terminal或quarantine。
3. tenure-only transition不保存incoming request为admitted、不消费request nonce/temporal constraint、不推进`source_revision_high_water`。crash后同一proof可幂等查询/重试request；旧writer已被new fence拒绝。
4. Runtime重读该exact snapshot，完整验证request nonce/temporal deadline、exact-active CAS、revision、frame/ledger bounds、contract/profile、lifecycle budgets和owner-wide action slot后，才以第二个atomic transaction同时提交request/temporal `AdmissionState`变化、new `source_revision_high_water`、exact canonical request/Slice与`PreparedNoEffects`。不能只写high-water而丢request，也不能只消费nonce/deadline而不创建prepared。

full admission durable后，normal apply的唯一副作用边界固定为`PreparedNoEffects → FirstActionIntent`。`PreparedNoEffects`必须证明尚未分配Domain/Instance/resource generation、尚未调用lifecycle callback、尚未执行cancel/drain/cleanup。Runtime在构造intent/empty-head transaction前以该operation安装时的ClockGeneration执行pre-intent deadline check；`now >= installed_deadline`（含相等）时不写intent/head、不创建resource/callback：`OneSourceLoop`原子终结为`StartTimedOutBeforeIntentNoEffects`，live/nonzero或no-retire `EmptyDeactivate`原子终结为`StopTimedOutBeforeHeadCommitNoEffects`，两者都保留old desired/live。

pre-intent check通过后，`OneSourceLoop`才可在下一次atomic snapshot中写`FirstActionIntent`、exact reserved generations、action kind与deadline lineage。retire current live/nonzero generation的`EmptyDeactivate`必须在同一个atomic snapshot中写`FirstActionIntent` marker、canonical empty desired head、`HeadCommittedRetiringOld`、`NoNewAdmission/Draining`及exact old Slice/budgets/generation。该empty transaction一旦durable就不得再按pre-effect operation supersede，也不得回滚已经committed的empty head；higher tenure只能走`SupersededReconcileRequired`并保留head-first cleanup语义。

intent/empty-head snapshot完成directory fsync后、创建任何对象/handle/resource或调用callback前，Runtime必须在同一ClockGeneration执行独立的post-intent/pre-effect deadline check。此时到期或相等：normal start不创建resource/callback；由于该分支没有资源且census已经exact-zero，它在同一个atomic terminal snapshot中写`RawActionOutcomeLatch { callback=NotInvoked, deadline=TimedOut }`、`TerminalOutcomeSelection`并以`StartTimedOutBeforeHeadCommitExactZero { effect_started=false, raw }`终结，不产生raw-only中间状态。live-retire empty则保留已经committed的empty head，跳过`on_stop`，先durable raw timeout/NotInvoked再只做owner cleanup，exact-zero后选择`TimedOutButExactZero { raw }`。intent fsync后、该check前crash按post-intent interruption处理，不能推导timeout。

callback/readiness/cancel raw fact必须在任何owner cleanup前atomic durable；随后cleanup/census完成，Runtime在构造terminal或success head/live snapshot前只采样一次`terminal_selection_observed_at`形成`TerminalOutcomeSelection`。`now >= installed_deadline`同样选择timeout，不能发布ordinary success。no-retire empty没有intent/effect，其completion前deadline sample到期时直接`StopTimedOutBeforeHeadCommitNoEffects`并保留old head。每次check的ClockGeneration、deadline、measured ordering与结果必须进入将要提交的snapshot input；selection确定后atomic writer/fsync或回复才跨deadline不会追溯改写，未durable则restart按old snapshot，publish不确定则`UncertainAfterPublish`停服。Controller transport/receive deadline独立于该Runtime terminal选择。

除 exact same-operation query/reconcile 外，incoming source revision必须严格大于per-source high-water；active=1、曾durable admitted/prepared=3、随后supersede后的revision2仍fail-closed。合法“回滚到旧内容”只能以更高的新revision表达。CAS/temporal/profile失败的高revision不会烧掉high-water，但已经通过tenure preflight的更高合法proof仍可完成独立fence takeover。该拆分需要S7 typed API/state-machine变更；当前monolithic admission/evaluate-writer-fence路径不能不经修改就声称满足。

admission/trust policy 的 fingerprint 覆盖 exact scope/target/key selector、验证 key、algorithm/version、maximum budget 与各 ledger bound。S7 Runtime compatibility truth是sequence-1持久的exact descriptor/manifest canonical bytes+digests、binary compiled actual与其共同导出的store-pinned `RuntimeBuildIdentityV1` fixed tuple，以及selected exact PXAR v5、profile v1与exact single fixture entry；它不携带supported-version/mode/fixture集合或separate hard-limit revision，所有bounds由v5/v4/profile-v1 protocol constants唯一拥有。active Slice携带PlanContent中exact singleton `RuntimeArtifactCompatibilityManifestV1` projection/digest。Runtime executable SHA-256/build identity与fixture artifact digest不得相等比较或互相替代。

compatibility failure严格分两类。snapshot/descriptor/manifest/active Slice无法strict decode、canonical/digest/internal cross-ref无效，或binary compiled actual不等于store-pinned descriptor/manifest时属于pre-validation `StoreUnreadableQuarantine`：不推进startup generation、不绑定listener且不回答authenticated state。active Slice若自身已经完整validated，而其exact target manifest/profile/fixture row在本次startup generation/live-invalidation durable之后的supported-compatibility/recovery-eligibility check中不等于store-pinned supported row，则进入`ValidatedOperationalQuarantine`：保持NotReady、禁用apply/callback/resource create，但可用pinned identity回答authenticated bootstrap quarantine fact和query `Indeterminate`。两类都不能用config覆盖actual，也不能用重启/升级换一套build、trust或fixture后继续reassembly。

`OneSourceLoop`只有在exact CAS、revision、bounded readiness、本地activation switch和`TerminalOutcomeSelection` deadline sample均通过后，才以一次atomic commit同时写new `active`、`LiveReady`与operation terminal；durable commit前不得回复成功。存在current `LiveReady`且resource ledger精确匹配nonzero active generation时，`EmptyDeactivate`使用两阶段live-retire：第一事务就是上述durable `FirstActionIntent` transaction，atomic写canonical empty `active`、prepared=`HeadCommittedRetiringOld`、live=`Draining`并保留exact old Slice/budgets/generation，operation仍nonterminal；第二事务只有在old ownership exact-zero并完成terminal selection后才写terminal result、清nonterminal slot并把live置`ExactZero`。

`canonical empty + ExactZero`和`nonempty + RecoveryFailedNotReady + ExactZero`是仅有的no-retire fast path。它们仍先完成共同的full-admission/`PreparedNoEffects` transaction；随后在再次验证exact CAS、revision、failure latch、deadline与owner census仍为exact-zero后，以一个atomic completion transaction同时写incoming canonical empty `active`、`ExactZero`、operation terminal并清prepared。deadline已到则只终结`StopTimedOutBeforeHeadCommitNoEffects`并保留old head。成功completion transaction不写`FirstActionIntent`或`HeadCommittedRetiringOld`，不创建nonterminal action/generation，不调用`on_stop`或任何callback，也不执行cancel/drain/cleanup。old failure/recovery evidence保留为历史，不因fast path伪装成从未失败。

`Draining/Uncertain/SupersededReconcileRequired/Recovering`、任一nonterminal action，或resource ledger不能证明恰好等于current stable active materialization（存在staged/orphan/extra generation）时，除same-operation query/reconcile/cleanup与tenure-only fence外，所有full request admission和assembly副作用都被owner-wide action gate阻塞。稳定`LiveReady`与其exact matching非零resource generation必须允许exact-CAS `EmptyDeactivate`。

每个normal apply或recovery action内部都有一个canonical、容量有界、monotonic的`RawActionOutcomeLatch`。它分别保存callback raw outcome（`NotInvoked | KnownSuccess | KnownError { reason } | Panicked | UnknownAfterIntent`）、deadline/cancel observation（`NotObserved | TimedOut | Cancelled`及`raw_outcome_observed_at`、exact ClockGeneration/deadline lineage）、host interruption、higher-tenure takeover和cleanup/census evidence/ref。callback/error/timeout/cancel任一fact一旦可证明，必须在进入owner cleanup前atomic durable；start success若无需cleanup，可以与active+LiveReady+terminal同一atomic commit，若该commit前crash则仍无durable known-success fact并按post-intent unknown处理。各维度只能单调补充；已经durable的known success/error/deadline fact永不因后续crash改写为`Unknown`，只有intent已durable而snapshot中从未出现callback result时，restart才能填`UnknownAfterIntent`。raw latch不是operation terminal，也不能由某个较晚事实删除较早事实。

`TerminalOutcomeSelection`是Runtime journal state-machine拥有的纯确定函数：它只读取exact phase、`RawActionOutcomeLatch`、desired/live state与ownership census，并在cleanup/census后采样一次`terminal_selection_observed_at`，在一个terminal snapshot中选择primary outcome，同时保留raw latch的canonical summary/digest。endpoint线程、callback完成顺序或response code不能直接挑选terminal。empty graceful `on_stop`返回known error且selection时exact-zero、`now < deadline`，terminal是non-success `StopFailedButExactZero { reason, raw }`；raw timeout或selection到期/相等则是`TimedOutButExactZero { raw }`。两者都与ordinary success分离；callback panic、cleanup/ownership uncertainty仍quarantine，不能因最终看似无资源而改写结果。

non-empty head commit前的terminal同样不能只写成泛`Failed`：`on_start`返回known typed error且selection时cleanup exact-zero、`now < deadline`才是non-success `StartFailedBeforeHeadCommitExactZero { reason, raw }`，raw timeout或selection到期/相等且随后exact-zero时是`StartTimedOutBeforeHeadCommitExactZero { raw }`。host crash发生在`FirstActionIntent`前时，incoming action outcome可证明为`NotInvoked`，终结为`AbortedBeforeIntentNoEffects { raw }`并保留old desired/live；发生在intent durable后、head commit前时，只有缺少durable callback fact的维度才是`UnknownAfterIntent`，进程边界证明incoming generation exact-zero后终结为`AbortedBeforeHeadCommitExactZero { raw }`。两类都不得replay callback。post-`FirstActionIntent` operation被higher tenure接管后，只有cleanup exact-zero才可terminal为non-success `SupersededAfterIntentExactZero { raw }`；它不能冒充原operation成功。cleanup/ownership无法证明时operation保持`Indeterminate/OwnerQuarantined`，不发布上述terminal。

若empty drain中host crash，canonical empty desired head保持权威；新进程不得 replay `on_stop`。纯in-process/no-effect reference Loop在进程边界和resource ledger能证明exact-zero时，terminal result必须是non-success `InterruptedButNowExactZero { raw }`；crash前durable的known callback/deadline fact原样保留，只有未durable的维度才补`UnknownAfterIntent`。它只证明current convergence，不能映射为ordinary success。任何可能跨host存活的进程、handle或effect都quarantine。若crash发生在new non-empty start callback可能执行但head尚未commit，incoming operation只能在cleanup exact-zero后终结`AbortedBeforeHeadCommitExactZero { raw }`，旧active保持；不能重放callback或把incoming猜成active。

所有normal start/stop operation使用同一canonical primary-outcome全序，不能按线程竞速选择结果；较高优先级只选择primary outcome，不删除较低优先级的durable raw fact：

1. callback panic、owner cleanup error或ownership/census无法证明时，`OwnerQuarantined/Indeterminate`最高优先，不能被后续看似exact-zero遮蔽；
2. durable post-intent higher-tenure takeover在exact-zero后选择`SupersededAfterIntentExactZero { raw }`；后来的host crash、deadline或callback结果只补raw facts，不能覆盖superseded primary；
3. RuntimeHost crash/interruption发生在terminal前时，exact-zero后选择对应`AbortedBeforeHeadCommitExactZero { raw }`或`InterruptedButNowExactZero { raw }`；不能因restart后旧clock deadline看似已过改写为timeout；
4. 没有上述条件时，任一owner-clock check观察到`now >= installed_deadline`就按action phase选择`StartTimedOutBeforeIntentNoEffects`、`StopTimedOutBeforeHeadCommitNoEffects`、`StartTimedOutBeforeHeadCommitExactZero { raw }`或`TimedOutButExactZero { raw }`；equality属于timeout。callback success/error恰在deadline返回，或known error虽在deadline前返回但cleanup/exact-zero直到terminal selection到期时，都由timeout primary胜出；
5. 只有known typed error被严格观察于deadline之前，且cleanup/exact-zero evidence在terminal selection仍严格早于deadline，才选择`StartFailedBeforeHeadCommitExactZero { reason, raw }`或`StopFailedButExactZero { reason, raw }`；
6. 只有callback success、readiness或exact-zero evidence以及terminal selection都严格早于deadline，才选择ordinary success。

`active`只证明durable desired head，不证明当前进程仍`LiveReady`。RuntimeHost restart先按上文完成snapshot/compiled-vs-pinned pre-validation与新的host/clock generation/live-invalidation atomic commit；随后才把已经strict-valid的active target manifest projection/profile/fixture逐字段对照独立取得的compiled actual与store-pinned supported row，并收敛任何nonterminal old-clock action。该post-start compatibility不匹配进入可认证的validated operational quarantine；匹配且eligible时才把non-empty active head的live materialization durable标记为`Recovering/NotReady`。对于ADR-0008 `OneSourceLoop`，Runtime随后在创建对象前commit phase=`RecoveryPlannedNoEffects`的internal recovery action，绑定exact active Slice/manifest digest、new RuntimeHostEpoch/current ClockGeneration与从不复用的新generations，并证明尚未创建任何对象/resource或调用callback；它以signed start budget在new clock generation安装一次性deadline，不换算旧apply deadline，也不创建desired revision或apply operation。

`RecoveryPlannedNoEffects → StartCallIntent`是recovery唯一副作用边界。Runtime先在current ClockGeneration执行pre-intent deadline check；若`now >= recovery_deadline`，它不写intent、不创建资源、不调用callback，而是durable记录`RecoveryFailedNotReady { TimedOutBeforeIntentNoEffects, raw.callback=NotInvoked } + ExactZero`永久failure latch。check通过后才可durable写`StartCallIntent`。该snapshot directory fsync成功后、创建Domain/Instance/resource/handle或调用callback前还必须做独立post-intent/pre-effect check；若此时到期或相等，因为尚无资源且census exact-zero，Runtime以一个atomic snapshot同时durable raw timeout/NotInvoked与permanent TimedOut `RecoveryFailedNotReady + ExactZero` failure latch，不产生raw-only中间状态且不callback。callback/readiness fact在cleanup前durable；cleanup/census后、发布`LiveReady`前再执行terminal selection deadline sample，到期或相等时不得发布Ready，必须先durable raw timeout、再cleanup，exact-zero后以保留raw facts的timeout failure latch发布`RecoveryFailedNotReady`。

若host在`RecoveryPlannedNoEffects`且`StartCallIntent`和pre-intent deadline observation都未durable时crash，next startup可从validated snapshot证明零效果，把旧internal action终结为`AbortedBeforeIntentNoEffects { raw.callback=NotInvoked }`，并在new host epoch以new action identity/generations创建新的`RecoveryPlannedNoEffects`；这不是旧action/callback replay，也不设置failure latch。即使旧进程的wall time可能已越过旧deadline，新clock generation也不得推导或伪造timeout observation。已经durable的pre-intent timeout则保持failure latch，不能以crash转成可重试。若`StartCallIntent`已经durable，crash后不得重放；仅当没有durable callback raw fact时才补`UnknownAfterIntent`，known success/error/timeout必须保留。known start failure、deadline timeout、panic或unknown-after-intent都会设置persistent failure latch，禁止当前或后续RuntimeHost进程为同一active head再次自动调用`on_start`。cleanup能证明exact-zero时发布`RecoveryFailedNotReady`，等待更高desired revision；ownership无法证明时quarantine。只有callback成功、ownership census一致且terminal selection deadline sample通过后才可atomic发布`LiveReady`。

owner-wide至多一个side-effecting apply/recovery/drain action。启动recovery或drain未terminal时，Runtime只服务bootstrap/query和已有action的bounded cleanup；mutating apply返回typed busy且不消费request nonce/deadline/revision。`RecoveryFailedNotReady + exact-zero`时reference profile只允许更高revision的exact-CAS `EmptyDeactivate`；若要再次启动Loop，必须先提交empty并terminal exact-zero，再以另一更高revision启动。active已是canonical empty且没有nonterminal drain时，restart只验证resource ledger/tombstone为`ExactZero`，不创建subject。历史terminal`Active` Receipt不能冒充当前Ready。

本 journal 可以让重启后的 RuntimeHost 按持久 resource identity 尝试恢复/清理，但不自动证明当前 S6 reference service manager 已能清理 host `SIGKILL` 后所有独立 process group。缺少共同 containment 或 exact ownership proof时仍须 quarantine。

### 8. operation id/digest 幂等

三个 owner 均使用同一原则，但 operation namespace 不混用：

- Controller operation id 在 `DeploymentScope` 内唯一，绑定 canonical controller intent digest。
- tenure-acquire operation id 在 Authority scope 内唯一，绑定 canonical acquire request digest。
- Runtime apply operation id 在 `SourceScopeRef + target RuntimeHost` 内唯一，绑定完整 canonical RuntimeApplyRequest digest。

相同 id + 相同 digest只允许：

- 返回已持久的 terminal result；
- 返回当前 durable phase供调用方 query/reconcile；
- 在状态机明确允许、且不会重放 unknown effect 的条件下推进同一 operation。

相同 id + 不同 digest在任何副作用前返回 stable conflict。更换 writer epoch、进程 generation、deadline、CAS、plan/slice 或 auth 导致 digest 变化时，不能沿用旧 operation id。

timeout、lost reply 或 process crash不授权透明创建新 operation。调用方先查询旧 id；只有 durable state和实际 owner evidence允许，才以新 id发起新的副作用尝试。terminal/prepared history 在 retained limit 内不得因 restart、writer turnover 或新 plan commit 被覆盖。

S7 query 是 versioned、bounded、authenticated read contract。request transcript使用独立domain separation，至少绑定target/source scope、expected `store_instance_id`、query id、requested apply operation id、可选expected request digest、client nonce与response byte/count bound；它复用Controller request-auth ACL和同一identity-bound local control channel，但不携带或推进writer tenure，不写AdmissionState/nonce/deadline/fence/action。response回显query/request digest与client nonce，并至少包含store identity、snapshot sequence、serving RuntimeHostEpoch、ClockDomainRef/ClockGeneration、owner quarantine/compatibility状态、operation lookup、durable desired head、source revision high-water、live state/resource generation、target-clock measured-at诊断值与census digest。response在该end-to-end authenticated channel内绑定Runtime peer identity，不是可转移authority Receipt；Controller以自己的receive deadline、exact nonce/query、live channel binding、pinned host/store、RuntimeHostEpoch和snapshot sequence判断可用性，拒绝旧epoch/旧query/倒退sequence。没有authenticated ClockMapping时不得把Runtime monotonic measured-at/valid-until与Controller clock直接比较。Controller先durable记录exact canonical response bytes/digest/channel peer identity再据此创建新的operation decision。

operation lookup只有四类：`Known { exact request digest, durable phase, canonical terminal result/ref }`、`Conflict { requested id exists with other digest }`、`Unknown`、`Indeterminate { stable reason }`。`Unknown`仅在exact store有效、snapshot完整、P2e no-eviction history与request-nonce ledger都能证明该id不存在时成立。`Indeterminate`也只能由已经通过上述service-readiness gate的validated snapshot返回，例如startup后active-head compatibility/recovery quarantine、ownership uncertainty、history unavailable或未来compaction缺少tombstone；corrupt/undecodable/unknown-format snapshot根本不提供query response，不能把无法验证的bytes包装成authenticated `Indeterminate`。store identity与Controller pin不一致时Controller同样视为`Indeterminate/OwnershipTransferRequired`，绝不能把“查不到”当作未执行。query不创建recovery attempt；Controller不能以同revision/new apply重建丢失的live resources。

### 9. corruption、unknown version 与 quarantine

以下任一条件都在业务副作用前进入 quarantine：

- active snapshot missing、empty、truncated、oversized、checksum mismatch 或 trailing bytes；
- magic、envelope/payload/checksum version、owner kind、store/owner identity、enum 或必填字段未知；
- count/length 超限、duplicate/non-canonical record、sequence 0/overflow；
- plan/revision/digest、epoch/proof、RuntimeHost/clock generation、admission-policy fingerprint、AdmissionState、source-revision high-water、writer fence/prepared/active desired-head、live materialization 或 resource-generation/tombstone 内部不变量不成立；
- lock/path/regular-file/permission 或受支持 filesystem capability 无法证明；
- active 无效但存在一个看似可用的 temp、backup、旧 version 或另一个目录。

quarantine 不是单一protocol可见性级别。snapshot missing/corrupt/undecodable、unknown version、descriptor/manifest canonical bytes或digest无效、compiled-vs-pinned mismatch等pre-validation `StoreUnreadableQuarantine`无法建立可信store/host/epoch/sequence，因此不执行startup generation mutation、不绑定bootstrap/query-ready listener，也不返回任何authenticated state。只有snapshot本身完整validated且startup generation transaction成功后，由active-head compatibility、recovery或ownership事实产生的`ValidatedOperationalQuarantine`才可query-only返回authenticated NotReady/`Indeterminate`。两类都拒绝继续mutation/签发/apply，且都不是重写一个empty snapshot。实现只向本地诊断保留最小、无敏感payload的错误分类与observed checksum/path fact；不得自动修checksum、忽略未知字段、回退到旧parser、选择修改时间最新文件或删除损坏证据后继续。

人工 repair 也不能凭文件内容猜测最高 epoch/revision。无法由权威外部证据证明单调 high-water 时，只能保持 quarantine，或执行后继 ADR 定义的 ownership transfer/reinitialization；不能称为普通恢复。

### 10. crash matrix

#### 单次 atomic snapshot mutation

| Crash/failure point | 重启后的唯一允许解释 |
| --- | --- |
| 构造 next snapshot 前 | old active 仍权威；operation 未提交 |
| temp create/write 中 | old active 仍权威；partial temp 非权威 |
| temp create/write/fsync 明确失败 | `RejectedBeforePublish`；old active 仍权威，operation 未提交，不得 rename |
| temp fsync 后、rename 前 | old active 仍权威；完整 temp 仍不得 promote |
| rename 后、directory fsync 前 crash | 调用方尚未收到 durable success；重启只接受路径上通过校验的 old 或 new active，并按 operation id查询；missing/invalid 则 quarantine |
| rename 明确未发布 | `RejectedBeforePublish`；old active仍权威，operation未提交 |
| rename 已发布/结果未知或 directory fsync 失败 | current process 标记 `UncertainAfterPublish`并停止；重启按 active+operation identity裁决 |
| directory fsync 成功、回复前 crash | new active 权威；相同 id/digest 返回已提交状态，不重复 mutation/effect |
| durable success 回复后 crash | new active 权威；回复与 journal 一致 |
| lock owner 任意点被 `SIGKILL` | non-inheritable handle使 OS 释放 lock；即使 owner spawn 的 child仍存活也不得持锁。replacement 必须完整重读/验证，不能沿用旧进程内存 |

#### 跨 owner/阶段

| Crash window | 必须保持的不变量与恢复动作 |
| --- | --- |
| Controller allocation/plan commit 中 | old 或 new 的 allocation+revision+plan完整 tuple；不存在只分配 ID 或只推进 revision |
| Authority 已签 proof、尚未 durable commit | proof 未出 owner，旧 epoch仍权威；retry 可产生新的完整 transaction |
| Authority durable commit、回复前 | new epoch/proof权威；同 acquire id/digest byte-identically返回该 proof |
| RuntimeHost snapshot/build pre-validation 中 | missing/corrupt/undecodable descriptor/manifest、digest或compiled-vs-pinned mismatch均不推进startup generation，不绑定bootstrap/query listener，不返回authenticated `Indeterminate` |
| RuntimeHost 启动 generation/live invalidation 中 | 未发布bootstrap/query-ready且不接收apply；重启再次从validated snapshot原子推进，绝不服务new host epoch + old LiveReady组合 |
| Runtime tenure-only transaction 中 | 完整old，或完整next AdmissionState tenure nonce+proof/principal+fence+旧action supersede/recovery-required tuple；不消费request nonce/temporal/revision，也不形成prepared |
| higher tenure 遇到已跨 FirstActionIntent 的旧 prepared | new fence拒绝旧writer，但owner-wide action gate继续阻塞新prepare/effect；先cleanup到exact-zero terminal，否则quarantine |
| Runtime full request admission transaction 中 | 完整old，或request/temporal AdmissionState+source revision high-water+exact request/Slice+PreparedNoEffects tuple；不存在只消费nonce/deadline、只推进revision或丢失assembly input |
| revision 3 admitted/prepared、随后被新 tenure supersede | per-source revision high-water 仍为 3；revision 2 即使大于 active revision 1 也拒绝，revision 4 可按 exact CAS表达合法旧内容回滚 |
| `PreparedNoEffects` durable、pre-intent deadline observation/`FirstActionIntent` 前 crash | phase证明没有incoming generation/resource/callback；restart验证incoming census后终结`AbortedBeforeIntentNoEffects { raw.callback=NotInvoked }`并保留old desired/live，不执行cleanup/action且不调用callback |
| pre-intent deadline check通过、intent atomic mutation 中 crash | active path只能是old `PreparedNoEffects`或完整new `FirstActionIntent`；old走`AbortedBeforeIntentNoEffects { raw }`，new走post-intent no-replay cleanup，不从进程内check结果猜测 |
| `FirstActionIntent` durable、post-intent/pre-effect deadline check mutation 中 crash | old intent或完整raw timeout+typed timeout terminal二选一；old按post-intent interruption且缺失callback fact才补`UnknownAfterIntent`，new证明`effect_started=false`，两者都不创建resource/callback |
| `FirstActionIntent` durable、post-intent check通过后首个实际effect前 crash | 按post-intent interruption处理；仅因没有durable callback fact而补`UnknownAfterIntent`，不replay，bounded cleanup证明incoming exact-zero后终结`AbortedBeforeHeadCommitExactZero { raw }`，否则quarantine |
| callback/readiness raw-outcome latch mutation 中 crash | old latch或包含完整known result的new latch二选一；restart不得把durable known result降为unknown，也不得根据外部callback猜补known result |
| start/readiness effect 后、`TerminalOutcomeSelection`或non-empty active commit 前 crash | incoming不成为active；不replay callback，resource ledger exact-zero后按interruption primary终结`AbortedBeforeHeadCommitExactZero { raw }`，保留已经durable的success/error/deadline fact，否则quarantine |
| `TerminalOutcomeSelection`/head+LiveReady+terminal atomic mutation 中 crash | old nonterminal action或完整new desired+LiveReady+terminal二选一；old在restart按interruption收敛，new幂等查询，绝不存在head/LiveReady/terminal部分组合 |
| OneSourceLoop active+LiveReady terminal commit、Receipt 回复前 | new desired/live权威；相同Runtime operation byte-identically返回同terminal result |
| non-empty active 后 RuntimeHost crash/restart | desired active保留但live先`Recovering/NotReady`；manifest/build preflight后先写`RecoveryPlannedNoEffects`，再以单独durable `StartCallIntent`越过effect boundary，成功才`LiveReady` |
| `RecoveryPlannedNoEffects` 中、deadline observation/intent前再次crash | old internal recovery action可证明无effect并终结`AbortedBeforeIntentNoEffects { raw.callback=NotInvoked }`；new host epoch可用new action identity/generations重新plan，不把旧clock deadline推导为timeout，不产生apply terminal且不设置failure latch |
| recovery pre-intent deadline/failure-latch atomic mutation 中 crash | old plan可在next host按pre-intent crash重新plan，或完整new `RecoveryFailedNotReady { TimedOutBeforeIntentNoEffects, raw.callback=NotInvoked } + ExactZero`永久latch；不能出现timeout fact与可重试资格并存 |
| recovery `StartCallIntent` atomic mutation 中 crash | old `RecoveryPlannedNoEffects`可replan，或完整new post-intent state永久禁止callback replay；intent绝不与generation reservation/deadline lineage部分出现 |
| recovery post-intent/pre-effect deadline check mutation 中 crash | old intent按interruption永久failure且缺失callback fact才补unknown，或完整raw timeout/NotInvoked+permanent failure latch；两者都禁止callback与后续replay |
| recovery callback raw-outcome latch/`TerminalOutcomeSelection` mutation 中 crash | old或完整new raw fact；durable known success/error/timeout不降为unknown，缺失fact才补`UnknownAfterIntent`，且均禁止callback replay |
| recovery `LiveReady`或`RecoveryFailedNotReady` publish 中 crash | old nonterminal recovery action或完整new live/failure-latch+census state；success/failure、live generation与action ref不部分发布 |
| recovery `StartCallIntent` durable后crash | callback outcome只在没有durable raw action-outcome fact时为`UnknownAfterIntent`，且绝不replay；failure latch跨后续restart保留，exact-zero后`RecoveryFailedNotReady`，否则quarantine |
| live-retire empty head/intent transaction 中 | old snapshot完全保留，或new snapshot同时含`FirstActionIntent`、empty active、`HeadCommittedRetiringOld`、exact old Slice/budgets/generation和`Draining`；new状态不可pre-effect supersede或回滚head |
| live-retire empty post-intent/pre-effect deadline check mutation 中 | old committed empty head+intent按interruption收敛，或完整raw timeout/NotInvoked后跳过`on_stop`进入cleanup；empty head在两者中都不回滚 |
| empty callback/cleanup raw-outcome与`TerminalOutcomeSelection` mutation 中 | old或完整new raw facts；known stop error、deadline、crash/takeover与cleanup evidence按latch单调保留，terminal primary只由固定全序选择 |
| exact-zero empty fast-path deadline/completion 中 | deadline到期则old head+完整`StopTimedOutBeforeHeadCommitNoEffects` terminal；成功则完整new empty active+`ExactZero`+terminal；两者都没有`FirstActionIntent`、`HeadCommittedRetiringOld`、callback或cleanup action |
| empty drain 中 host crash | 不replay`on_stop`；exact-zero后的`InterruptedButNowExactZero { raw }`必须保留crash前已durable的callback/deadline outcome fields，只有对应fact从未durable时才为`Unknown`；否则quarantine |
| Runtime success、Controller rollout update 前 | Runtime query是实际 owner evidence；Controller恢复后 reconcile，再提交 rollout fact |
| Controller timeout/partition | timeout不等于 apply 未发生；先按 target+operation id查询，不使用新 epoch隐式重放 |
| journal capacity exhausted | 旧 snapshot保持；新 operation在副作用前拒绝，不 eviction |

### 11. fault injection 与验证门

每个 owner 的 first implementation 必须提供 deterministic failpoint，并由 subprocess/system harness 在真实目录和真实 process kill 下至少覆盖：

- temp create、header/payload/checksum partial write、temp fsync、rename、directory fsync、durable commit 后回复前的 before/after crash；
- header/payload每个结构边界的 truncation、single-bit corruption、length bomb、checksum mismatch、duplicate、trailing bytes、unknown envelope/payload/checksum version 和 owner identity swap；
- orphan temp、invalid active + valid temp、valid active + higher-sequence temp、symlink/path replacement、第二 writer lock contention，以及 owner spawn 的 child保持存活时 `SIGKILL` owner 后 child未持 lock、replacement可竞争并重读；
- initialization-only no-active variant在CSPRNG unavailable/error/short-fill/all-zero、temp/rename/dir-fsync各点old/new/uncertain、active-already-exists拒绝、missing-after-initialize、key/policy/principal fingerprint mismatch、sequence overflow和每个capacity boundary；production generator必须填充exact 32 octets，deterministic generator只在fixture build/path可注入；
- 相同 id/same digest、相同 id/different digest、lost reply、writer turnover和 stale request；
- Controller allocation/revision/plan atomicity、request-auth key隔离、bootstrap exact bytes/digest commit、RuntimeHostId/store/channel/compiled+store-pinned build identity pin-before-sign/send、v5 expected-store进入complete request digest/signature、same target+new store在AdmissionState/fence/revision前拒绝、signed request commit-before-send/byte-identical retry、exact query response bytes commit-before-decision，以及rollout update不改写committed plan；
- Authority epoch严格单调、commit-before-return、同acquire operation返回同proof、signing key不出Authority；wrong peer credential/socket ACL/request key/scope/writer/replay/oversize均不推进epoch；
- RuntimeHostEpoch/clock generation与旧live invalidation原子推进；tenure-only完整AdmissionState tenure nonce+proof/principal+fence+supersede/recovery-required，与full request/temporal+source-revision-high-water+exact request/Slice+PreparedNoEffects两类transaction分别做old/new crash evidence；
- build-pipeline descriptor golden、install operation仅从verified descriptor/artifact+operator exact target/service identity+binary compiled fixture table生成唯一canonical manifest、拒绝任意prebuilt/editable manifest、向Runtime initializer与Controller/Planner ingress交付byte-identical artifact、sequence-1 exact bytes留存、initializer installed-artifact hash/length/target验证、normal startup不读installer side file且只核binary compiled actual、compiled-vs-store/manifest mismatch、旧generation deadline不换算、fence-before-prepare、prepared/active/live-materialization分离、owner-wide single action gate和无双active resource；
- 对normal OneSourceLoop、live-retire empty、no-retire empty与restart recovery分别在`Prepared/RecoveryPlannedNoEffects`、pre-intent check before/at/after equality、intent transaction old/new、intent durable后的post-intent/pre-effect check before/at/after equality及raw-timeout mutation old/new、首个effect前、callback raw fact transaction old/new、known error/success后crash、cleanup跨deadline、`TerminalOutcomeSelection` before/at/after equality、head/live/terminal或failure-latch transaction old/new、durable commit后reply前逐点kill；两次连续pre-intent recovery crash必须每次只终结old action并使用new identity，pre-intent timeout必须永久latch，post-intent任意restart必须零callback replay；
- `RawActionOutcomeLatch` mutation/model tests覆盖known error/success后crash、known error+cleanup跨deadline、deadline+crash、post-intent takeover+deadline/crash及panic/cleanup uncertainty，证明known raw fact不降为unknown且`TerminalOutcomeSelection`恒按`quarantine > supersede > interruption > deadline > typed error > success`选同一primary；start/stop/recovery typed terminal、empty head两阶段retire与exact-zero fast path均做byte-exact golden；
- invalid/missing/corrupt/undecodable/unknown-version snapshot、invalid descriptor/manifest和compiled-vs-pinned mismatch必须证明没有startup generation mutation、没有bootstrap/query listener且没有authenticated `Indeterminate`；另以完整validated snapshot+successful startup transaction注入active-head compatibility/recovery/ownership quarantine，证明只返回pinned identity的authenticated NotReady/`Indeterminate`且apply保持disabled；
- active revision 1 → admit/prepare revision 3 → writer turnover/supersede → cross-restart revision 2 reject，以及 revision 4携带旧内容但合法 CAS时接受；CAS/auth/deadline失败的高 revision不得烧掉 high-water；
- minimal bootstrap stale clock/build/profile fact在request nonce/deadline/revision消耗前拒绝；query request/response bounds/auth/channel binding、store/snapshot/epoch freshness、Known/Conflict/Unknown/Indeterminate分型与Controller exact response bytes/digest commit-before-decision；
- migration在每个 I/O boundary crash后只产生可验证 old/new state，unknown/newer version绝不由旧 binary降级打开。

编码层需要 golden bytes、checksum vector、round-trip、non-canonical rejection和随机输入 bounds property tests。状态层需要 model/property test证明 epoch/revision/sequence不回退、operation conflict不分叉、Controller plan tuple原子、Runtime owner-wide side-effecting action至多一个；每个durable admitted request恰有匹配的operation row、request nonce、temporal lineage与revision-high-water关系，terminalization不删除replay identity。任一悬空/冲突ref、callback/resource uncertainty丢失或quarantine latch不一致都使snapshot quarantine；corrupt/quarantined owner不能回答terminal或Unknown。

system evidence 必须在声明支持的目标 filesystem/OS 上执行 kill/restart suite。仅在 mock filesystem 或 unit test中通过，不能声明 durable crash recovery；CI 无法提供的目标平台 evidence 必须明确标为未验证，而不是 soft pass。

### 12. migration、rollback 与 removal

任何 incompatible envelope/payload 变化都增加 version，提供显式 offline migrator和双向不可混淆的 golden fixtures。自动“尝试旧 parser”、dual-write old/new、运行时原地猜字段或 unknown-version fallback 均禁止。

local format migration流程为：

1. 停止唯一 owner并取得同一 exclusive lock；
2. 严格验证 source snapshot及全部业务不变量；
3. 生成 canonical target snapshot，记录 source version/checksum/store identity和 migration id；
4. 通过同一 temp→file fsync→atomic rename→directory fsync protocol切换；
5. 用 target parser重新验证后才允许新 owner启动；
6. 保留 operator-controlled、只读 source evidence用于显式 rollback/审计，但运行时不得自动回退读取。

migration crash 后，active path上的完整 old 或 new version决定下一步；old binary看到 new version、new binary看到未迁移 old version时均停止并要求显式操作。rollback也是一项显式 migration，必须证明不会降低 Controller revision/allocation high-water、Authority epoch或Runtime writer fence/active state；直接把旧 backup rename回来不被允许。

owner journal 只能在显式 decommission 后 removal：

- Controller scope已经 retire，所有 target rollout/uncertain operation已 reconcile并产生 terminal evidence；
- Authority scope已通过后继 ownership-transfer/revocation流程封存，不能通过删除文件复用旧 epoch；
- Runtime prepared为空、active desired head 已显式提交 canonical empty target、owned resources exact-zero，或全部不确定 ownership 已经由后继 authority 显式 quarantine 移交；
- removal receipt绑定 store instance、最后 snapshot checksum/sequence和授权主体。

未来迁移到 SQLite、remote database、replicated log或 HA backend 必须先有 superseding ADR，定义新 persistence owner、fencing/consensus、availability/partition语义、backup/restore和 local→new backend cutover。迁移必须导入并验证最高 allocation/revision、epoch/proof、writer fence、active/prepared和 operation identity；不得 dual-write形成两个 authority，也不得在 HA backend不可用时 silent fallback到 stale local journal。

## 备选方案

### 一个共享 database 和跨 owner transaction

它可以让三个状态看起来原子，但会把 Controller、Tenure Authority 与 RuntimeHost 合并为一个存储/故障/权限 owner，并让 Runtime读取 editable Deployment truth或让 Controller写 Runtime active state。跨 owner uncertainty本来就必须通过协议处理，因此不采用。

### append-only WAL 或自建 replicated log

WAL适合高 mutation rate，但首版必须同时决定 replay、checkpoint、compaction、torn-tail、retention和operation tombstone，测试面显著大于单 Node P2e所需。保留为 full-snapshot写放大或容量证据推翻本方案后的候选。

### SQLite

SQLite可提供成熟 transaction和WAL，但不会自动解决 owner分离、schema migration、fsync profile、operation idempotency或corruption policy。P2e bounded low-churn state可先用更小的固定格式；若真实 churn/容量/维护成本证明 full snapshot不足，再以 superseding ADR引入每-owner独立 SQLite store。

### 仅内存状态，restart时从 observed state重建

observed Runtime facts不能恢复最高 allocation/revision、Authority epoch或旧 operation conflict，且可能把旧 writer重新授权，因此拒绝。

### active损坏时读取 temp、backup或空状态

该方案会把未 durable commit、stale epoch/revision或部分写入提升为 authority，直接破坏 fencing。所有此类自动 fallback均拒绝。

### 立即使用 HA/consensus backend

目前没有多副本 availability、跨 Site或scope-sharding证据，会提前引入 leader/fencing和运维复杂度。P2e只保留显式迁移 seam，不实现 HA。

## 后果

收益：

- 每个逻辑 owner只有一个持久 writer，Controller、Authority与Runtime职责不因存储共置而混合。
- full snapshot、fixed bounds和atomic replace产生有限、可枚举的 crash matrix，便于真实 kill/fault injection。
- 在声明的 process-crash/filesystem profile 内，commit-before-effect/return、operation id/digest和严格 quarantine阻止已确认 mutation 因正常 crash回退、重复副作用和“丢文件即重置”。
- format/version/migration seam为未来后端替换保留证据链，而不提前建立第二 authority。

成本与限制：

- 每次 mutation都写完整 snapshot并执行两次 fsync，写放大和latency高于 WAL；只适合P2e低churn reference。
- 三个 owner之间没有分布式事务，timeout/partition必须显式 query/reconcile，调用者会看到 `Uncertain`。
- 初版operation history有硬上限且无自动 compaction；达到容量会停写，需要显式迁移。
- local disk丢失、filesystem违反fsync/rename假设或无法证明单调high-water时不能自动恢复，只能quarantine。
- checksum与OS权限不防 root/磁盘firmware级恶意篡改；这不是安全审计日志。
- 单 active snapshot 没有独立 anti-rollback anchor；同 store 的完整旧有效 snapshot、旧 backup restore 或 block-device rollback 可能通过校验，不能宣称防止这种 revision/epoch/generation 回退。
- 该基线本身不解决 host `SIGKILL` 后escaped ProcessDomain、物理effect reconciliation、HA或跨Node rollout。

## 失败场景与反例

可能推翻 full-snapshot选择的主要反例是：真实P2e/P5 workload在声明硬件上产生持续高频rollout/observed更新，使16 MiB snapshot的p99 fsync latency、写放大或介质寿命无法满足控制面SLO，或256条operation history在合理维护周期内经常耗尽。出现该证据时，应比较per-owner SQLite/WAL或其他bounded log，而不是放宽为无界内存/文件。

如果目标部署只能使用不提供可验证directory fsync和atomic rename语义的filesystem，本reference profile也不可用；需要为该backend提交独立durability模型、crash harness和superseding ADR。

如果P5以后要求多个Controller副本在同一scope自动接管，本地OS lock不再足以证明single writer。届时必须引入带term/lease/fencing和partition语义的HA authority，并显式迁移最高epoch/revision/operation状态；不能把shared filesystem lock包装成consensus。

整盘丢失、完整旧快照回放或 backup restore 而没有可证明不回退的外部 authoritative high-water 时，任何本地格式都无法安全恢复最高 tenure/fence/generation。新证据若表明该 availability 或 hostile-rollback 目标是产品硬要求，应先决定 backup/restore authority 和 anti-rollback 机制。

## 实施与验证

当本 ADR header 为`Proposed`且authorization receipt未生效时，本文只新增决策草案，不授权或声称任何实现。达到`Accepted`只授权按本文边界进入bounded implementation，仍不构成公共journal API、persistent format、daemon、generic storage package或production recovery能力的完成声明。

若 Accepted，建议按以下依赖顺序形成一个bounded P2e batch：

1. 冻结共同 envelope、owner-specific payload version、常量、stable error taxonomy和golden vectors；保持实现internal，不创建generic storage framework。
2. 实现owner-specific显式initializer、provisioning fingerprint、fixed lock、strict reader、atomic snapshot writer和quarantine result；先以独立process failpoint harness验证文件级crash matrix。
3. S7-D实现`DeploymentTenureAuthority` store/initializer/独立service principal和真实tenure-proof签名路径，证明epoch/proof commit-before-return、same-operation byte-identical reply和key隔离；acquire IPC先internal，S7-E有真实deploymentd client后才public promotion并验证peer auth/authz。
4. 先以internal owner-specific codec/state machine实现Runtime host/clock generation、AdmissionState/trust/build fingerprint、source-revision high-water、tenure-only/full-admission split、`writer_fence/prepared/active/live_materialization/recovery_action`与resource journal；S7-E与Runtime executable同批落地initializer、authenticated bootstrap/apply、empty two-phase与restart fail-closed，S7-F再登记full authenticated query和manifest-bound single-attempt reassembly。
5. 先以internal owner-specific codec/state machine实现Controller allocation/plan/target-binding/rollout journal；S7-E落下`paraegox-deploymentd` committed plan、request-auth signer、exact bootstrap response pin和signed request commit-before-send，接回unique projector并登记真实producer；S7-F补exact query response journaling、rollout/restart reconcile。全程不建立手写Slice旁路。
6. 运行完整fault matrix、capacity/golden/property tests和one-subject plan→commit→project→apply→observe→deactivate system flow，再由独立review确认owner/write-set、corruption和crash evidence。

Accepted本ADR不允许提前在`governance.toml`写placeholder。S7-D只登记真实Authority process/initializer/persistent surface，acquire IPC在deploymentd真实client出现前保持internal；S7-E同批登记acquire promotion、Controller/Runtime initializer与journal、authenticated bootstrap/apply及真实producer/consumer；S7-F query public contract/reassembly/reconcile随真实endpoint/call path和first tests登记。internal helper不能冒充public capability。

实现评审必须逐项提供：

- exact format constants、canonical bytes与checksum vectors；
- 受支持OS/filesystem列表和真实file/directory fsync证据；
- 每个failpoint的old/new snapshot、returned status和restart result；
- Controller plan tuple、Authority epoch/proof、Runtime host/clock/admission/source-revision-high-water/fence/prepared/active desired-head/live-materialization/resource-generation 的状态机模型与反例测试；
- same-id/same-digest和same-id/different-digest的cross-restart evidence；
- corruption/unknown-version/quarantine、capacity exhaustion和migration interruption evidence；
- 明确的未实现项，包括HA、backup restore、cross-node reconciliation、production containment和effect-owner ledger。

上线只允许在explicitly initialized的新store上进行。通用未来格式的binary rollback必须先验证parser/semantic compatibility并走显式migration；ADR-0008 S7 reference target更严格绑定exact build，任何build变化都必须empty terminal→decommission old RuntimeHostId/store→new identity初始化，不能原地reverse-migrate authenticated active Slice。任何journal错误都使owner停止新mutation，不能删除state、`--force`空启动或fallback。

## 后继与替代

本 ADR 不替代 ADR-0001的整体owner边界；它仅按“背景”中列出的两个exact state-machine点修订ADR-0001 §4.1 第162条，并细化Kernel Foundation P2e已经要求的local journal与crash recovery机制。该修订和本ADR其余规则必须由同一次Accepted/explicit authorization生效，不能挑选其中一半实施。

未来任何HA store、replicated log、external control-plane database、SQLite/WAL替换、online compaction、backup/restore或多writer takeover方案，都必须通过显式migration和superseding ADR替代本reference persistence选择，同时保留ADR-0001的Deployment/Authority/Runtime owner边界、epoch/revision单调性、operation id/digest幂等和fail-closed fencing。
