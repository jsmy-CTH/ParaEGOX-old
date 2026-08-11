# ADR-0004 — Deck 工作负载、DeckLock 与 Application 准入边界

> 状态：Accepted
> 日期：2026-07-29
> 决策者：ParaEGOX workspace user（以 `docs/plans/s7-p2e-baseline-v1.authorization-receipt` 为唯一生效证据）
> 关联文档：[Application、Deck、Card 与 Service 边界研究](../research/application-deck-card-service-boundaries.md)、[CardDefinition、Card 与 Deck](../concepts/card-definition-card-deck.md)、[Kernel、RuntimeHost 与 Core Services](../architecture/kernel-runtime-core-services.md)、[ADR-0001](ADR-0001-deployment-controller-boundary.md)、[ADR-0002](ADR-0002-card-definition-terminology.md)、[ADR-0006 — Rust-first 核心机制与多语言工作负载边界](ADR-0006-rust-first-core-and-polyglot-workloads.md)、[Kernel Foundation Plan](../plans/kernel-foundation.md)

## 一句话结论

提议将 `Deck` 固定为声明式可执行工作负载并保留该公共名称，不以 `Bundle` 作为同义词；canonical `DeckTopology` 是 `DeckLock` 中受 digest 覆盖的结构子树。当前不建立正式 Application 家族，只有通过条件式 Application Admission Gate 后才允许引入稳定安装与应用私有持久状态模型。

## 背景

ParaEGOX 已经区分 CardDefinition、Card、CardInstance、DeckSpec、DeckLock、DeploymentPlan 和 DeckRun，但“应用”尚无独立 producer、consumer、owner 或生命周期。旧文档把解析后的结构称为 `ApplicationTopology`，却不存在 ApplicationSpec、ApplicationLock 或 Application installation；部分流程还把它与 DeckLock 并列传给 DeploymentPlanner，形成两份可能漂移的解析真相。

另一方面，未来真实产品可能由多个独立 Deck、Gateway/client Artifact 和私有持久服务组成，并需要稳定安装身份、升级、回滚、卸载与数据保留。永久规定“Deck 就是 Application”会迫使 Deck 吞掉这些职责；现在预建完整 Application 家族又会制造没有消费者的兼容负担。

[边界研究](../research/application-deck-card-service-boundaries.md)对照了 OAM/KubeVela、Kubernetes workload/ownership、Helm Chart/Release、ROS 2 launch、Dora dataflow 和 Dapr service/state 模型。共同证据支持分开“可执行工作负载”“交付/安装所有权”“部署期望”和“运行事实”，但不能替 ParaEGOX 提前决定尚未出现的安装 API。

## 范围与非目标

本 ADR 提议决定：

- Deck 的长期语义，以及它与 Product/Application/Installation 的非等价关系。
- Deck 的公共名称；`Bundle` 不成为 Deck 的 Schema、包名或 API 别名。历史来源可保留原名，未来全新窄对象的名称需由自己的 ADR 决定。
- DeckSpec、DeckCompiler、DeckTopology 与 DeckLock 的单一解析真相和字段分区。
- Canvas、DeploymentPlanner 与 DeckLock 的输入边界。
- 当前不引入正式 Application 类型，以及未来准入该类型的证据门槛。
- run-bound、installation-bound 与 platform-bound service/data 的所有权缝隙。

本 ADR 不决定：

- `deck.yaml`、`deck.lock`、ApplicationSpec 或 Installation API 的最终编码。
- Artifact registry、Marketplace、计费、租户或产品 UI。
- DeploymentController 的单写、plan commit 与 Runtime apply 语义；这些继续由 [ADR-0001](ADR-0001-deployment-controller-boundary.md) 拥有。
- `CardDefinition → Card → CardInstance` 的术语决策；该链继续由 [ADR-0002](ADR-0002-card-definition-terminology.md) 拥有。
- Deck、Card、CoreService 或 Application 候选的实现语言与进程边界；语言不能成为这些领域身份，具体边界服从 [ADR-0006](ADR-0006-rust-first-core-and-polyglot-workloads.md)。
- 现在批准 Application、ApplicationService 或新的 reconcile controller。

## 决策提案

### 1. Deck 是可执行工作负载，不是万能产品容器

`Deck` 表达一组需要共同验证和部署的 Cards、内部 Links、DeliveryProfile 及 Service/Permission/Feature Requirements。直接连接内部 Card Port 的结构必须位于同一个 Deck；跨 Deck 只能经显式、版本化的 ServiceContract、Gateway endpoint 或未来单独裁决的 export 边界交互。

Deck 不等于 Product、Release、Installation、DeploymentScope、进程、Runtime 或 DeckRun。首个 reference profile 可以用一个 Deck 完整表达一个产品应用，但这只是 profile 映射，不是身份等价不变量。未来是否需要 multi-Deck consistency group、共同 revision 或原子 rollout 由 Application fixture 和后继 ADR 决定，本 ADR 不预先排除。

Deck 也不改名为 Bundle。`Card → Deck` 清楚表达“多次能力使用构成一个工作负载”；EAGOS 的 Bundle 名称曾覆盖组合、解析、交付/安装与 runtime/deployment 等协作 surface，Motus `PerceptionBundle` 则是 endpoint/进程内实现聚合。把这些不同边界复用为一个公共词会产生伪一致性，并给 Deck 重新吸收打包和安装职责留下入口。

### 2. DeckLock 是唯一解析产物

```text
Canvas ──edit──> DeckSpec
                    │ DeckCompiler {resolve + canonical validate}
                    ▼
                 DeckLock
                 ├── DeckTopology
                 └── resolved closure
                    │
                    ▼
        DeploymentPlanner + immutable target inputs
```

- Canvas 只编辑 DeckSpec。它可以只读展示由 DeckCompiler 派生的 DeckTopology 验证投影，但不能直接编辑或持久化 topology。
- DeckCompiler 是纯编译入口；DeckResolver 只是其中无独立状态的纯步骤。
- DeckLock 是唯一可持久、可交给 DeploymentPlanner 的解析产物。Planner 不接收 DeckSpec、独立 DeckTopology 或 Canvas state。
- DeckLock 的 canonical digest 覆盖 DeckTopology 和 resolved closure；坐标、缩放、折叠等 Canvas View State，以及按 Card key 旁置的纯 display metadata 不进入 digest。
- DeckLock 只锁定 ServiceRequirement contract/range；live provider 由 Planner 基于 immutable ServiceSpec inventory、target facts 和 policy 选择。

### 3. DeckTopology 与 resolved closure 不重复事实

字段按以下方式分区：

| DeckLock 区域 | 唯一拥有的内容 |
| --- | --- |
| `DeckTopology` | 稳定 Card closure key、由 closure key 限定的 Port endpoint key、Link、DeliveryProfile key/ref、RequirementRef key，以及纯结构关系 |
| resolved closure | 精确 CardDefinition/version、Port/Schema、Artifact/adapter ref、DeliveryProfile payload、完整 Service/Permission/Feature Requirement payload 与其他锁定元数据 |

Topology 不再重复精确 definition/version、Schema、Artifact 或完整 Requirement payload。canonical validator 必须拒绝悬空 key、同一语义的重复 closure entry、同一 identity 的冲突版本、重复 Requirement 和 key/payload 不匹配。任何影响执行或连接语义的 closure 变化都必须改变 DeckLock digest。

### 4. 当前“应用”不是公共身份

产品文案、示例和 TUI 可以把一个完整 Deck 称为“应用”，但公共 DTO、Receipt、权限与日志必须使用已有真实 identity：DeckLock digest、`DeploymentId@DeploymentRevision`、DeckRunId 或 CardInstanceId。显示分组不产生权限继承、级联删除、数据所有权或生命周期。

当前不建立：

- ApplicationSpec、ApplicationLock、ApplicationInstance 或无 owner 的 `application_id`。
- ApplicationController 或与 DeploymentController 并行的 reconcile loop。
- ApplicationService、application store 或 `applications/` 空包。
- 将 DeckCatalog 当成已安装产品索引。

DeckCatalog 若以后出现，只索引可发现的 Deck definition/release metadata；安装目录必须等待稳定 Installation identity。

### 5. Service 工作负载、数据所有权与存储托管分开

“服务由谁运行”不等于“数据归谁”，也不等于“谁保存 bytes”。未来服务模型必须区分：

| 角色 | 回答的问题 |
| --- | --- |
| Service workload owner | 谁声明、启动、升级、停止或替换 ServiceInstance |
| durable data owner | 谁拥有候选 `StateNamespaceRef`、schema/migration、retain/delete/transfer 和唯一 GC authority |
| storage custodian | 哪个共享或专属 provider 实际保存 bytes、执行备份并报告存储事实 |

CardInstance 私有状态默认只到 CardInstance/DeckRun。一次 DeckRun 内的共享计算优先建模为 Card；只有真实多消费者要求版本化 ServiceContract 时，才研究 DeckRun-scoped ServiceSpec。平台 CoreService 必须先证明它由平台拥有、跨独立 Product/Installation 共享或持有平台权威，不能仅因“长期运行、稳定 API、持久状态”而准入。

安装私有、跨 DeckRun/升级的数据当前不支持。未来可以复用同一 scoped `ServiceDependencyGraph` 扩展 run/installation-scoped vertex，不创建第二套 ApplicationServiceGraph，也不预设每个 installation 都需要独立服务进程。两个 installation 可以共享一个平台 storage provider，但各自拥有隔离 namespace；卸载 A 不停止 provider、不删除 B，只能按 A 的权威 Receipt 处理 A 的 namespace。

### 6. A0 Application Admission Gate

任何工作切片满足以下任一条件时，必须进入 A0，而不能继续把需求塞入 Deck、Card 全局状态或 CoreService：

1. 产品用例不能由一个 Deck 表达，并需要多个独立 DeckLock 形成统一发布、更新或卸载闭包。
2. 需要 installation-owned mutable state 跨 DeckRun、升级或重新部署继续存在。
3. 同一 release 需要多次隔离安装，或 Gateway/client/private service 等多 Artifact 需要稳定安装 owner。

提交 A0 证据至少包括一组公共材料和与实际触发条件对应的 fixture；不得为了凑齐模型而伪造另一类需求：

- 公共材料：一个候选 identity producer 和两个独立 consumer；若存在不可逆数据删除，单一 GC authority 本身可以作为强 consumer 证据。只提交本次涉及操作的 owner matrix 和 failure surface，不强迫无状态产品填写 retain/delete。
- 触发条件 1：至少两份可独立演进的 DeckLock、统一 release/update/uninstall 诉求，以及 partial rollout/rollback fixture。
- 触发条件 2：稳定 owner 候选、一个安装私有数据 namespace、schema/migration/backup/retain/delete 语义，以及 crash、迁移失败和 partial GC fixture。
- 触发条件 3：同一 release 的两个隔离安装 fixture，或需要同一 owner 的 Deck/Gateway/client/private-service Artifact closure；覆盖重复请求、partial install/uninstall 和隔离失败。
- 所有 fixture 只覆盖与声明风险相关的旧 writer/split-brain、安装记录与 Deployment commit 间 crash 等故障，不把完整 Application 平台当作准入前提。

A0 只有两个合法出口：形成并接受一份只定义实际最小 owner 的后继 ADR（可能是 ProductRelease、Installation、Application 或更窄对象），或继续以稳定 diagnostic reason code 拒绝对应能力。研究文档、UI 名称或一个自由字符串 `scope` 不能绕过 gate。

### 7. DeckRun 与 AgentSession 生命周期

DeckRun 是运行身份和 Inspection 投影，不是可以直接 stop/delete 的 Runtime 对象。操作方请求 DeploymentController deactivate/replace Deck workload，旧 DeckRun 随后进入 terminal state。DeckRun terminal 本身永不触发 installation data GC；未来只有 installation uninstall owner 可以签发 retain/delete/transfer Receipt。

首个 durable AgentSession 若在稳定的非运行 owner ADR 之前实施，必须是 DeckRun-bound：它可以跨同一 DeckRun 内的 CardInstance/进程重启恢复，但 Deck workload terminal 时必须 seal，不会被新 DeckRun 自动续接，并按明确 retention/GC Receipt 处理。跨 DeckRun、升级或重新安装续接首先必须说明稳定 owner：若会话只属于某个产品安装，则触发 A0；若它确实由平台或租户域拥有并跨独立产品共享，则必须通过相应 CoreService/tenant ownership ADR 与隔离证据，不能借 CoreService 名义把安装私有状态平台化。

## 备选方案

### 永久规定 Deck 等于 Application

短期词汇最少，但会迫使 Deck 吞入多 Deck release、安装记录、客户端 Artifact、私有持久状态、升级/卸载和数据 GC，最终重建万能 Bundle，因此不采用。

### 现在创建完整 Application 家族

它能预留 ProductRelease、Installation 和私有服务模型，但当前这些对象没有独立 consumer，且 ApplicationController/ApplicationInstance 会分别与 DeploymentController/DeckRun 重叠，因此后置到 A0。

### 只给 ServiceSpec 增加 application scope

没有稳定 installation identity、authority 和生命周期时，scope 字符串无法决定迁移、备份、retain/delete、卸载和 GC，也无法防止误删共享 provider，因此不足以解决问题。

### 让 DeploymentController 同时成为安装 owner

DeploymentController 只拥有 DeploymentScope 的 desired plan/revision 和 rollout。让它再拥有产品 release、安装记录和私有数据会把部署收敛与产品生命周期耦合，并使 GC 权威模糊，因此拒绝。

### 将 Deck 改名为 Bundle，或提供 Bundle 别名

这个方案的最强形式不是复活旧聚合，而是定义一个严格收窄的 `BundleSpec` 只表达 Cards/Links/Requirements，并把 BundleLock、Artifact、Installation 和 Deployment 明确拆开。它在技术上可行，也可能降低既有用户、文档和 source asset 的迁移成本，不能只用“表面连续性”否定。

本提案仍选择 Deck，因为 ParaEGOX 当前没有既有 Bundle source 的兼容承诺、已发布 Schema 或实现迁移负担，Motus 同名类型又表示 provider/endpoint 内实现聚合；此时 `Card → Deck` 的边界更直接。若本 ADR 接受前出现必须无损导入既有 Bundle source 的真实需求，应提交代表性资产、转换成本与用户词汇证据并重新评审。无此证据时不提供别名，以免公共 Schema 永久保留错误等价。历史文档只写“旧 Bundle source 的工作负载组合子集对应 Deck”；未来离线交付物可研究 `DeckArchive` 等限定名称，最终术语由交付 ADR 决定。

## 后果

收益：

- Deck、解析锁、部署与运行事实各有唯一 owner，删除了 `DeckLock + topology` 双输入。
- 首版能直接实现 ASR/TTS/Agent/Controller 等单 Deck 产品切片，而不预造应用平台。
- 给未来多 Deck 产品和安装私有状态留下明确、可验收的演进门。
- 共享平台服务不会因某个 DeckRun terminal 或某个 installation 卸载被误停、误删。
- Card/Deck 保持一组语义一致的公共词，同时避免 Bundle 的打包、安装与实现聚合歧义。

成本与限制：

- 当前不能宣称支持一等 Application 安装、多 Deck release 或 application-owned durable service。
- DeckLock Schema 必须明确 topology/closure 分区并做 canonical validation。
- 未知、不可信或多语言 reference Card 需要受限 ProcessDomain、ephemeral workspace 和 typed access handle，才能使“未声明持久状态不支持”成为可执行边界；受信、与 RuntimeHost 同构建同发布的 Rust 实现可以进入内部 in-process profile，但语言和装载方式都不改变 Card 身份。opaque Card code 的业务意图不能由编译器猜测。
- A0 fixture 和数据所有权 Harness 会增加未来 Application 准入成本，但这是安全卸载和升级所必需的证据。

## 失败场景与反例

本提案最强反例是：首发产品已经必须由多个独立 Deck 构成，拥有跨 DeckRun/升级的私有对话历史，同一 release 可安装多次，并要求可审计的升级、回滚、卸载和数据保留。若这是当前承诺，A0 已经触发；不能接受本 ADR 后继续把 Application 当作遥远假设，而应先完成后继 Application/Installation ADR。

另一个反例是 future Planner 需要对多个独立 DeckLock 实施一致性 rollout。它不推翻“Deck 是 workload”，但会要求后继 ADR 定义 consistency group、release compatibility 和 rollback ownership；本 ADR 不强制把所有共同 revision 的内容合并成一个 Deck。

## 实施与验证

在本 ADR header 为`Proposed`且authorization receipt未生效期间，以下内容只能作为target/candidate；达到冻结manifest预计算的`Accepted` bytes也只授权进入实现，任何一项都必须取得对应完成证据后才可标记为已实现：

1. 原子迁移旧 `ApplicationTopology` 名称，并保证活跃模型只使用 `DeckTopology`。
2. 建立 DeckTopology/closure 字段分区和 canonical validator。
3. 验证相同 DeckSpec/resolver inputs 产生 byte-identical DeckLock；结构或 closure 语义变化改变 digest，Canvas View State 和旁置 display metadata 不改变 digest。
4. 验证 Planner API 不存在独立 topology 输入；同一 byte-identical DeckLock 配不同 immutable ServiceSpec inventory/target facts 得到不同、可解释 candidate，而 lock/digest 不变。
5. unknown、opaque、不可信或多语言 reference Card 只能进入受限 ProcessDomain profile：每个 generation 获得 isolated ephemeral workspace 和声明的 typed handles，未声明 raw persistent file/database/egress 由该 profile 的 sandbox/enforcement 拒绝并记录 fact。S7/P2e 的 exact manifest/build-pinned compiled-in Rust fixture 是独立、极窄的 trusted in-process profile：它不建立公共 `dylib`/trait-object ABI，不获得 workspace/typed handle 或系统调用 sandbox 证明，仍保留 ambient OS authority；它只以 fixed implementation、zero grant、静态审计和 Harness 约束，不能据此宣称任意 Rust 实现满足访问隔离。两类 profile 遇到 Schema 明示 `owner=application`、`lifetime=installation` 或未来等价字段时都返回稳定 `UnsupportedProviderOwnership`/`UnsupportedStateLifetime`。
6. DeploymentController deactivate/replace Deck workload 后，旧 DeckRun terminal，而 Authority/Fabric/Inspection/共享 Model CoreService 继续运行。
7. 两个 installation 共享一个平台 state provider 的未来 fixture 中，卸载 A 不能停止 provider、删除 B 或越权处理 B namespace。
8. AgentSession 若先实现，验证 DeckRun terminal 后 seal、新 DeckRun 不自动继承，以及 retention/GC Receipt。
9. 公共 Schema、任意语言的 package/crate、CLI/API 和 Receipt 不出现 `Bundle` 作为 Deck 别名；历史迁移说明和外部来源原名除外。
10. 跨 revision fixture 验证未变 Card key 只表示同一 desired slot；rename 为 remove + add，删除后复用 key 不恢复旧 CardInstance 或私有状态。

实现计划见 [Kernel Foundation Plan](../plans/kernel-foundation.md)，声明到证据的映射见 [Testing](../testing/README.md)。

## 后继与替代

本 ADR 不替代 ADR-0001、ADR-0002 或 ADR-0006；它只补充这些记录没有拥有的 Deck/Application 长期边界。若 A0 通过，后继 ADR 必须定义 ProductRelease/Installation/Application 中实际成立的最小对象、authority、迁移和 GC 语义，并链接或 supersede 本 ADR 的相应部分。
