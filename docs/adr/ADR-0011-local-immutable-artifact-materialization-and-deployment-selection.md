# ADR-0011 — 本地不可变 Artifact 物化与 Deployment 选择边界

> 状态：Proposed
> 日期：2026-08-10
> 决策者：待定
> 关联文档：[ADR-0001 — DeploymentController、DeploymentPlan 与 Runtime 边界](ADR-0001-deployment-controller-boundary.md)、[ADR-0004 — Deck 工作负载、DeckLock 与 Application 准入边界](ADR-0004-deck-workload-and-application-admission-boundary.md)、[ADR-0005 — Typed Domain Graph、Graph Foundation 与 Runtime Assembly 边界](ADR-0005-typed-domain-graphs-and-runtime-assembly-boundary.md)、[ADR-0008 — PXTE v4 / PXAR v5 source-only/empty reference target successor](ADR-0008-pxte-v4-pxar-v5-subject-ingress-separation.md)、[CardDefinition、Card 与 Deck](../concepts/card-definition-card-deck.md)、[Local Operator CLI/Ops Program](../plans/local-operator-cli-ops-program.md)

## 一句话结论

提议让未来首个外部单 Artifact 路线只建立以完整 `ArtifactObjectRefV1 { artifact_digest, artifact_manifest_digest }` 为身份的不可变 `ArtifactStore` 物化边界：ArtifactStore 只拥有 canonical manifest 与 payload bytes 的联合验证、发布、重开和物化 Receipt，DeploymentController 继续独占 desired Artifact 选择，RuntimeHost 继续独占 live generation；不建立 Application、ProductRelease、Installation、InstallationId、active pointer、卸载、GC 或安装私有状态。

## 背景

当前 DeveloperLocal deterministic-echo profile 是与现有 Runtime/composition 同构建的 compiled-in fixture，不是一个独立构建、传输或安装的工作负载文件。它可以由真实 DeploymentController 提交 desired state，并由 RuntimeHost 产生 exact apply terminal evidence，但不能被描述为已经存在外部 Artifact 安装能力。

现有 `paraegox-runtime-host release-descriptor-v1/install-v1` 只拥有 exact RuntimeHost executable、singleton compatibility manifest 与 sequence-one Runtime store 初始化。它不是通用 ArtifactStore、产品 Installation、active release pointer 或 workload rollback owner；相同 install 命令在 initializer marker 已消费后也不会成为可查询的通用部署幂等操作。因此未来外部单 Artifact 路线可以复用 strict canonical verification、pinned-file 与 crash-safe publication的设计原则，但不能复用或重解释该 Runtime installation identity。

[ADR-0004](ADR-0004-deck-workload-and-application-admission-boundary.md) 的 A0 只在真实切片需要多 Deck 统一 release/update/uninstall、installation-owned mutable state，或同一 release 多次隔离安装/多 Artifact 共同稳定 owner 时触发。一个 compiled-in deterministic fixture、一个 DeploymentScope、一个 target 且没有安装私有状态，不满足这些条件。另一方面，未来即使只有一个外部 Artifact，公开内容身份、持久物化格式、唯一 writer、崩溃恢复和 Receipt 仍是需要显式裁决的架构边界；这正是本 Proposed ADR 的范围。

当前 [Local Operator CLI/Ops Program](../plans/local-operator-cli-ops-program.md) 仍是交付顺序权威，且它对 A0/D0 的现有表述尚未由本 Proposed ADR 修改。本记录不改变 Program 状态、依赖或完成声明，也不能以 Proposed 状态绕过 Program、治理登记或用户授权。

## 范围与非目标

本 ADR 提议决定：

- compiled-in deterministic D0a 与未来外部单 Artifact 路线的分界；
- 完整 `ArtifactObjectRefV1`、canonical Artifact manifest/payload、ArtifactStore immutable materialization 与物化 operation/Receipt 的唯一 owner；
- ArtifactStore、DeploymentController、RuntimeHost 与 operator CLI 之间的依赖方向；
- 单 Artifact 物化的 crash、幂等、兼容和失败关闭规则；
- 未来 replace/rollback 在没有 Installation active pointer 时必须遵守的长期语义；
- 重新触发 ADR-0004 A0 的 exact gate 与稳定 diagnostic。

本 ADR 不决定或授权：

- D0a 的 exact public CLI grammar、JSON Schema 或完成状态；
- 当前实现外部 Artifact build、inspect、materialize、deploy、replace、restart、rollback、push 或 remote deploy；
- Application、ProductRelease、Installation、InstallationId、installed-product index 或 ApplicationController；
- active/current release pointer、符号链接选择、安装记录、卸载、GC、retain/delete/transfer、backup/migration 或 installation-owned private state；
- Artifact registry、catalog、marketplace、签名/SBOM、Artifact Trust、远端传输或供应链 policy；
- 新 crate、CoreService、daemon、后台清理任务或通用包管理器；
- 通用 Graph、workflow、saga、retry、compensation 或 rollback engine。

## 决策

### 1. Compiled-in deterministic D0a 不触发 A0，也不依赖本 ADR

一个只选择现有 compiled-in `deterministic-echo-v1` fixture 的本地 D0a：

- 不产生、复制、传输或物化外部 Artifact bytes；
- 不创建 ArtifactStore object、Installation record、release history 或 active pointer；
- 只允许真实 DeploymentController 提交或查询一个 DeploymentScope 的 committed desired state；
- 只允许 RuntimeHost 创建和拥有 live generation，并以绑定 exact operation/revision/Slice 的 owner terminal报告 `ActiveReady`、失败或不确定；
- 不拥有跨 DeckRun/升级的 installation-private mutable state。

因此该窄切片不满足 ADR-0004 §6 的 A0 三项触发条件，也不以本 ADR Accepted 作为前置依赖。D0a 仍必须单独冻结 public CLI/JSON compatibility、登记治理 surface，并取得 exact-ref 行为证据；本条不是 D0a 实现或发布授权。

### 2. 未来单 Artifact 路线只有一个完整对象引用

未来外部单 Artifact v1 只允许两个内容 digest，并且只能通过一个不可拆分的完整对象引用消费：

- `ArtifactDigest`：由 Artifact contract 的唯一 canonical owner 计算，绑定 exact immutable payload bytes；
- `ArtifactManifestDigest`：绑定 exact versioned canonical manifest bytes。

`ArtifactObjectRefV1` 严格且仅由上述 digest pair 组成。request digest、durable admission、store key/lookup、terminal object record、`ArtifactMaterializationReceipt`、Deployment Plan/Slice、Runtime reopen、replace 与 rollback 都必须绑定同一个完整 ref；payload digest 相同但 manifest digest 不同是两个不同对象，不能互相代替、覆盖、去重、激活或回滚。任何只携带 `ArtifactDigest` 或 mutable path 的操作在 ArtifactStore mutation、Deployment commit 或 Runtime effect 前失败关闭。

canonical manifest 至少无歧义覆盖 Artifact contract version、payload byte length、`ArtifactDigest`、target/ABI compatibility、entrypoint/export identity，以及该 profile 实际需要的 bounded immutable execution metadata。字段、编码、digest domain、上限、unknown-field/version behavior 和跨语言 golden vectors必须在首个真实 producer/consumer批次中一次冻结；在冻结前不得以 Rust struct layout、serde 默认值、文件名、目录名或可编辑 TOML/JSON 充当公共身份。

Artifact manifest 不包含 ApplicationId、ProductReleaseId、InstallationId、active flag、DeploymentRevision、RuntimeGeneration、安装路径、mutable status、retain/GC policy 或私有数据 namespace。相同 canonical manifest 与相同 payload bytes 必须产生相同 `ArtifactObjectRefV1`；任何 payload 或执行语义变化必须改变 ref 中对应 digest。公共编码、边界和 golden vectors 必须保证 digest pair 不能被交换、截断或降格为单 digest identity。

### 3. ArtifactStore 只拥有不可变物化

`ArtifactStore` 是逻辑 owner 名，不自动批准新 package、进程或 CoreService。首个实现应与真实 build/inspect producer、Deployment projection consumer 和 Runtime materialization consumer 同批准入，并在没有独立部署、隔离或复用证据时保持最小、可共置实现。

ArtifactStore 唯一拥有：

- strict decode manifest、重算并验证 manifest/payload digest、长度和目标兼容前置事实；
- 在 owner-private store root 中以完整 `ArtifactObjectRefV1` 联合发布和重开 exact canonical manifest 与 payload immutable object；
- 单个物化 operation 的 request identity/digest、bounded durable state、terminal outcome 与 `ArtifactMaterializationReceipt`；
- 对自己精确创建且仍可证明身份的临时对象做有界清理；
- 报告 `Materialized`、`AlreadyMaterialized`、`Failed` 或 `Uncertain`，而不把任何一项升级成 Activated/Ready。

ArtifactStore 不：

- 选择哪个 Artifact 应运行，保存 active/current pointer，或写 DeploymentPlan；
- 创建线程、进程、Runtime generation、CardInstance 或 ServiceInstance；
- 推导 Deployment Ready、Runtime健康或 rollout成功；
- 拥有 Application/Installation 生命周期、卸载、GC、retain/delete、数据迁移或级联删除；
- 从目录枚举、mtime、文件名、符号链接、进程存在或调用方自报 metadata重建权威身份；
- 启动后台 reconcile、retry、watch、cleanup daemon 或第二 writer。

没有 GC owner 时，已完整发布但尚未被任何 Deployment引用的 immutable object只允许保留并计入有界容量；不得由 CLI、RuntimeHost或目录扫描自行删除。

### 4. DeploymentController 是唯一 desired Artifact 选择 owner

未来单 Artifact 路线的权威链是：

```text
build producer
  → canonical manifest + exact ArtifactObjectRefV1
  → ArtifactStore immutable materialization + owner Receipt
  → strictly verified immutable manifest projection
  → DeploymentPlanner candidate
  → DeploymentController committed PlanContent/DeploymentRevision
  → RuntimePlanSlice {exact Artifact/manifest commitment + entrypoint}
  → RuntimeHost read-only reopen/reverify through Runtime-owned Artifact access port
  → Runtime owner terminal Receipt
```

DeploymentPlanner/DeploymentController 只消费严格验证的 immutable manifest projection和绑定完整 `ArtifactObjectRefV1` 的物化 Receipt reference，不读取或保存 payload bytes，不接受可变路径，也不从 ArtifactStore对象存在推导 active。完整 object ref、entrypoint和compatibility facts必须进入 PlanContentDigest及受影响的 target Slice digest。

DeploymentController 继续独占 committed desired head、DeploymentRevision、replace/rollout/rollback协调与 Deployment Receipt。任何 ArtifactStore `active` 字段、`current` symlink、CLI pointer、Runtime-local preference 或 last-write-wins 文件都不得成为第二 desired selection。

### 5. RuntimeHost 是唯一 live generation owner

RuntimeHost 只消费 authenticated Runtime apply request、canonical target Slice、自己的 durable journal以及Runtime-owned只读 Artifact access port。该 port 只能从 ArtifactStore 已 terminal-commit 的完整 `ArtifactObjectRefV1` 重开 exact canonical manifest 与 payload；它不发布 ArtifactStore object、不签发物化 Receipt、不选择 desired/current object。若实际执行需要 Runtime-private staging，该 staging 只能是当前 generation-owned ephemeral state，并随 generation fence/cleanup；它不是共享内容 store、Installation、desired/current pointer或可被其他 generation复用的物化权威。RuntimeHost 必须在任何 workload副作用前：

1. 通过只读 port 打开 Slice 指定的 exact `ArtifactObjectRefV1`，而不是解析 mutable current path；
2. 重新验证 manifest/payload canonical bytes、digests、length、target和支持的 profile；
3. 验证 request、writer tenure、revision high-water、operation id/digest 与 exact-active CAS；
4. 只在上述验证全部成功后创建、替换或退役 Runtime generation。

Artifact `Materialized` 不蕴含 Runtime `Activated`；Runtime `Activated` 不蕴含 Deployment rollout `Ready`。只有 Runtime owner 对 exact operation/revision/Slice 返回 `ActiveReady` terminal，且 DeploymentController 将其关联到当前未被 supersede 的 committed revision后，operator surface才可报告该次 deployment 的 point-in-time `Ready`。持续 freshness、stale/unknown和当前健康仍属于 Inspection，不得由历史 Receipt推断。

### 6. CLI 只聚合 owner reference

operator CLI 可以调用 ArtifactStore、DeploymentController 与查询 seam，但不能直接写它们的 store。CLI 输出只能摘要并引用：

- `ArtifactMaterializationReceipt`；
- DeploymentController 的 commit/operation Receipt；
- RuntimeHost 的 authenticated terminal Receipt；
- 可选 EvidenceRef/Inspection freshness。

CLI、日志、stdout、transport ACK、文件存在、进程存在或 exit 0 都不能签发新的权威“总 Receipt”，也不能在 owner evidence缺失时合成 Installed、Activated、Ready或RolledBack。

### 7. Materialization crash 与幂等边界

每个物化请求必须携带非零 bounded operation identity；ArtifactStore 计算覆盖 exact operation输入与完整 `ArtifactObjectRefV1` 的 canonical request digest。在创建 temp、发布 content bytes 或执行任何其他 store mutation 前，必须先 durable commit 并 exact-readback `{ operation_id, request_digest, artifact_object_ref, state = Admitted }`；没有这份 admission 就不能拥有 temp，也不能在崩溃后把同 operation id 的不同请求误认成可继续。规则固定为：

- 相同 operation id + 相同 request digest 只能返回、查询或推进同一个 durable operation；
- 相同 operation id + 不同 request digest 必须在 payload/store mutation前返回 conflict；
- canonical manifest 与 payload bytes 先写入同一 admitted operation拥有的 owner-private staging，分别完成 length/digest验证与 file sync；再以 no-overwrite/identity-checked方式发布由完整 `ArtifactObjectRefV1` 寻址的两份 content bytes，同步相应父目录并同时 exact reopen验证；只有两者都durable且exact后才能 commit terminal object record与Receipt。ArtifactStore lookup/consumer只能看见 terminal record绑定的完整pair，任一半发布、旧manifest/新payload或相反组合都不是可物化对象；
- crash发生在 final publish前时，旧 final object和Deployment desired state不变；只清理可证明由同 operation拥有的临时对象；
- crash发生在一份或两份 content bytes durable publish后、operation terminal commit前时，重启只能从durable admission按完整ref同时重新验证manifest与payload，再完成同一operation；不能从单份文件、目录枚举或payload digest创建另一个identity，未完成pair只是不对consumer可见的orphan bytes；
- final object durable但尚无 Deployment引用时只是安全 orphan，不能解释为 Activated或Ready；
- operation terminal commit后CLI输出失败或断连时，调用方必须按同一 operation查询并取得原 terminal Receipt，不能透明创建新 operation；
- 无法证明临时对象身份、final durability、operation high-water或Receipt关联时返回 `Uncertain`或quarantine，不能覆盖、删除、reset或从空状态继续。

ArtifactStore 的 snapshot/journal、临时文件、rename/no-replace、file sync、directory sync和readback必须服从 admitted platform profile；普通单元测试、进程退出或文件系统调用成功不能冒充 power-cut/durability认证。

### 8. 未来 replace/rollback 没有 Installation active pointer

本 ADR 不授权当前实现 replace、restart或rollback。未来若对应 public contract和故障证据另行获准：

- replace 选择新 `ArtifactObjectRefV1`，改变 PlanContent并提交更高 DeploymentRevision；
- same-artifact restart保持同一 desired Artifact commitment，只有在旧 generation 已有 joined/retired terminal后才由Runtime owner推进新 RuntimeGeneration；它不能通过重写 DeploymentRevision或active pointer冒充restart；
- rollback 选择已知、仍物化且重新验证兼容的历史 `ArtifactObjectRefV1`，作为新的 forward DeploymentRevision执行完整 commit/apply/Ready流程；
- rollback 不降低 DeploymentRevision、writer epoch、Runtime generation high-water或store sequence，不 checkout Git、不 rename backup、不切 symlink，也不产生 Installation revision；
- 只有新的 forward revision取得 exact `ActiveReady`并被DeploymentController关联后，CLI才可摘要为 `RolledBack`。

缺少旧 bytes、兼容证据、joined stop、exact terminal或owner query时必须返回 Failed/Uncertain，不得回退到当前目录、上一个文件或透明重试。

### 9. ADR-0004 A0 gate 在任何副作用前执行

当请求或 fixture 出现 ADR-0004 §6 任一事实时，本窄边界不再适用：

1. 多个可独立演进 DeckLock 需要统一 release/update/uninstall闭包；
2. mutable state需要由稳定安装 owner跨 DeckRun、升级或重新部署继续持有；
3. 同一 release需要多次隔离安装，或Deck/Gateway/client/private-service等多个 Artifact需要共同稳定安装 owner。

稳定 InstallationId、产品级 active pointer、卸载、retain/delete/transfer、backup/migration、GC或安装私有数据 namespace的需求也必须映射回上述真实触发条件和fixture，不能只增加自由字符串 scope。

在最小后继 ADR Accepted 前，Artifact build/materialization、Deployment commit、Runtime apply和任何数据 mutation都必须以稳定 public diagnostic `A0_APPLICATION_ADMISSION_REQUIRED` 失败关闭。无法证明仍处于“单 Artifact、无 installation-owned state、无多次隔离安装”边界时同样失败关闭。

多个 DeploymentRevision引用不同 immutable digest、rollback以新 revision选择历史 digest，或多个 deployment只读复用一个无安装身份/私有状态的 Artifact object，本身不创建 Installation identity，也不自动触发 A0。

### 10. 禁止通用 Graph/workflow engine

Artifact materialization、Deployment commit、Runtime apply与未来rollback保持各自 owner-specific state machine和Receipt。不得把这些步骤 lowering 到通用 Graph node、workflow/saga engine、Graph Store、通用 retry/compensation框架或中央恢复 owner。

纯确定性步骤表或局部算法可以保留在实际 owner内部；任何跨领域 Graph Foundation仍服从 [ADR-0005](ADR-0005-typed-domain-graphs-and-runtime-assembly-boundary.md) 的双真实生产消费者门，且不得拥有I/O、持久状态、operation、Receipt、retry或rollback。

### 11. Proposed 状态不授权实现

本 ADR 处于 `Proposed` 时仅是候选边界。它不授权：

- 创建 Artifact crate/package/service、公共 manifest/API/CLI或持久 store；
- 修改 DeploymentPlan/RuntimePlanSlice/public Receipt Schema；
- 实现或宣称 artifact build/inspect/materialize、D0、replace、restart或rollback；
- 修改 Local Operator Program、`governance.toml` 或任何 milestone状态；
- 创建 authorization receipt或把本记录视为用户接受证据。

即使未来 Accepted，也只授权按本边界进入具体 implementation admission；每个公共 contract、持久格式、package和CLI仍需同批登记真实 producer、独立 consumer、compatibility、migration/removal与first functional evidence。

## 备选方案

### 直接产品化 compiled-in deterministic D0a

这是当前最快的用户可见 deployment切片，也是本 ADR 明确允许保持独立的路线。它能证明真实 DeploymentController/Runtime Ready/Receipt，但没有外部 Artifact bytes，因此不能承担 build/inspect/materialize/push。本文不阻塞它，也不把它伪装成本 ADR 的消费者。

### 建立带 active pointer 的 LocalReleaseInstallation owner

它能表达 release history、installation record和rollback，但当前单 Artifact fixture没有多安装、多 Artifact闭包、安装私有状态、卸载或GC consumer。active pointer还会与DeploymentController committed desired head形成第二真相。因此当前不采用；真实 A0 fixture出现后由最小 ProductRelease/Installation或更窄后继 ADR重新裁决。

### 让 DeploymentController 保存 Artifact bytes或安装目录

它减少一个表面 owner，却违反 ADR-0001 对Deployment desired state与Artifact内容/Installation目录的分离，使 plan/reconcile故障域同时承担内容存取和清理。本文选择严格的 immutable projection输入和Runtime Artifact port。

### 让 RuntimeHost 从目录扫描或 current symlink选择 Artifact

实现简单，但会让Runtime从本地可变观察重建desired state，绕过DeploymentRevision、writer fencing和exact Slice digest。本文要求Runtime只消费Slice中的exact commitment。

### 复用 `paraegox-runtime-host install-v1` 作为通用 Artifact安装器

该命令绑定final RuntimeHost executable、singleton manifest和sequence-one store，重解释它会混淆平台Runtime安装与工作负载内容身份，并制造错误兼容承诺。因此只允许复用设计原则，不复用公共身份或命令语义。

## 后果

收益：

- compiled-in D0a 可以先证明可见 deployment闭环，不等待不存在的Installation模型。
- 外部 Artifact bytes、desired selection和live generation各有唯一 owner，不需要active pointer。
- content-addressed immutable object使重复物化、传输校验和未来forward rollback可共享同一内容身份。
- A0 在真实复杂度出现前保持可执行，避免把Application/Installation预建成空平台。
- owner Receipt链能区分Materialized、Committed、ActiveReady与持续Inspection freshness。

成本与限制：

- 首个真实外部 Artifact批次必须建立canonical manifest、store、operation journal/Receipt与Runtime复验，不能只复制文件。
- 没有GC时orphan object占用有界容量，需要显式报告容量不足；当前不能提供卸载或空间回收承诺。
- ArtifactStore、DeploymentController和RuntimeHost之间存在多owner partial-success，需要Uncertain/query/reconcile证据。
- 当前不能声称支持replace、restart、rollback、push、remote deploy或任意用户代码加载。
- compiled-in D0a和未来external-artifact profile必须显式分型，不能用同一字段静默推断来源。

迁移影响：

- 当前没有已接受的通用 Artifact manifest、ArtifactStore持久格式、Installation record或active pointer需要迁移。
- 现有 RuntimeHost descriptor/install/manifest bytes与语义保持不变，不被本 ADR alias或重解释。
- 若候选 owner未获得真实producer/consumer，本记录可以Rejected而不迁移产品数据；任何已实现但未准入的临时目录不能升级成公共store。

## 失败场景与反例

最强反例是首个真实产品已经需要同一release两次隔离安装、多Deck或Deck+Gateway/client共同升级/卸载，或拥有跨DeckRun/升级的私有状态。该证据会立即触发A0并使本单Artifact边界不足；正确动作是停止mutation并接受只覆盖真实fixture的ProductRelease/Installation或更窄后继ADR，而不是向ArtifactStore追加scope、active pointer和GC。

第二个反例是目标平台无法在已接受filesystem profile内提供安全的no-overwrite publication、file/directory durability和identity recheck。此时必须先建立平台端口/存储profile决策，不能把一次rename测试解释为crash consistency，也不能通过覆盖final object降低要求。

第三个反例是实际Runtime consumer需要的entrypoint、dependency closure、signature/SBOM或sandbox commitment不能由单一canonical manifest无歧义表达。此时应在首个真实consumer批次提出显式manifest successor或Artifact Trust决策，不能让Runtime读取可编辑sidecar/default补语义。

## 实施与验证

若本 ADR 后续 Accepted，仍须按依赖分批实施，且文档或代码存在都不构成完成证据：

1. D0a 单独证明 compiled-in deterministic profile不创建ArtifactStore/Installation/active pointer，并通过真实DeploymentController commit与Runtime terminal返回point-in-time Ready；该证据不声明外部Artifact能力。
2. 首个external-artifact batch同时冻结canonical manifest、digest pair、`ArtifactObjectRefV1`编码与bounds、strict decoder、unknown version/field behavior、Rust/Python golden vectors和reproducible build input/output；pair交换/截断、payload相同但manifest不同、tamper、truncation、oversize、unsupported target与entrypoint drift在store mutation前失败。
3. ArtifactStore file tests覆盖absolute/private root、owner/mode、symlink/hardlink、path traversal、inode replacement、no-overwrite、manifest/payload联合temp ownership、short write、file sync、directory sync、terminal object record与两份bytes exact readback；平台不满足admitted profile时fail closed。
4. deterministic fault harness在durable admission、manifest/payload temp create/write/sync、各自final publish、directory sync、联合object reverify、terminal object/operation snapshot commit和Receipt output各点注入crash；重启只能得到旧完整状态、新完整pair或Uncertain/quarantine，不得得到可选择的partial object。
5. operation tests证明same id+same request+same完整ref只推进同一operation，same id+different request/ref pre-effect conflict，terminal output loss后可查询绑定原完整ref的Receipt，orphan或半发布bytes不成为Materialized/Activated/Ready。
6. Planner/Controller contract tests证明只消费strict immutable projection，完整`ArtifactObjectRefV1`进入PlanContentDigest和target Slice digest，且源码/依赖guard禁止读取ArtifactStore目录、current symlink或payload bytes。
7. Runtime tests证明只通过read-only access port打开terminal完整ref，并在任何create/start副作用前重新验证manifest/payload与Slice commitment；materialized-only、wrong pair/digest/target/entrypoint、missing object和identity race稳定拒绝且不改变active head，Runtime不会签发第二份materialization Receipt。
8. Receipt correlation tests证明CLI只引用Artifact、Deployment和Runtime owner Receipts；file/process/ACK/log无法合成Ready，timeout/output loss保留Uncertain并按operation查询。
9. A0 negative fixtures覆盖三项触发条件和InstallationId/active pointer/uninstall/GC/private-state伪装；全部在ArtifactStore、Deployment和Runtime mutation前返回exact `A0_APPLICATION_ADMISSION_REQUIRED`。
10. architecture checks禁止Application/ProductRelease/Installation/InstallationId/active pointer和通用Graph/workflow owner进入本切片；禁止将existing RuntimeHost `install-v1` alias成Artifact materialization。
11. 未来replace/restart/rollback只有在独立public contract获准后才能实现；其Harness必须证明forward revision、generation fencing、joined stop、no-double-active、历史digest复验、partial failure和Ready/Uncertain Receipt关联。本ADR文本不满足该授权或证据。

远端transfer/push、签名供应链、多个平台以及process/power-cut认证分别需要自己的evidence；单机mock、`--no-run`、截图、一次marker或目录列表不能替代。

回退策略是在没有committed Deployment引用时停用未通过Harness的external-artifact profile并保留compiled-in D0a；不得删除未知object、重写Deployment desired head、把未验证目录切成current，或从build server回传源码修补Mac权威工作树。

## 后继与替代

本 ADR 不替代 ADR-0001、ADR-0004、ADR-0005或ADR-0008；它只提议补充这些记录之间尚未拥有的单external-Artifact immutable materialization与Deployment selection边界。

任何满足ADR-0004 A0触发条件的后继ProductRelease/Installation/Application或更窄owner ADR必须明确本记录哪些单Artifact规则继续保留、哪些被supersede，以及object、operation、Receipt和Deployment引用如何迁移。若本提案未获得真实producer/consumer或平台durability证据，应Rejected或缩回internal implementation，不保留orphan公共合同、package或store。
