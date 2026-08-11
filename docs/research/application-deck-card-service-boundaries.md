# Application、Deck、Card 与 Service 边界研究

> 状态：Research Complete
> 日期：2026-07-29
> 深度：Deep
> 结论：`revise`
> 范围：ParaEGOX 产品应用、可执行工作负载、Card、平台服务、应用私有持久状态、交付与运行身份
> 实现状态：仅完成研究与文档收敛；没有冻结 Application Schema，也没有对应代码
> 评审：本地契约审计、外部一手资料对照、独立反例审查
> 关联文档：[ADR-0004 — Deck 工作负载、DeckLock 与 Application 准入边界](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md)、[CardDefinition、Card 与 Deck](../concepts/card-definition-card-deck.md)、[Kernel、RuntimeHost 与 Core Services](../architecture/kernel-runtime-core-services.md)、[分布式系统模型](../architecture/distributed-system-model.md)、[ADR-0001](../adr/ADR-0001-deployment-controller-boundary.md)、[ADR-0002](../adr/ADR-0002-card-definition-terminology.md)

## 一句话结论

ParaEGOX 当前不新增一套与 Deck 重叠的正式 `Application` 类型：`Deck` 收窄为一个可验证、可锁定、可部署的**声明式可执行工作负载单元**，`Card` 是其中一次能力使用，`CoreService` 是与普通 DeckRun 生命周期解耦的平台共享能力；产品层仍然可以称“应用”，但只有在多 Deck 聚合、稳定安装身份或应用私有持久状态出现真实消费者后，才研究位于 Deck 之上的 Application 控制/交付对象。

同时建议修正当前模型中的命名与双真相问题：`ApplicationTopology` 改为 `DeckTopology`；它是 `DeckLock` 中由 digest 覆盖的 canonical structure subtree，而不是 DeploymentPlanner 的第二份独立输入。该长期边界已经提交为 Proposed ADR-0004，尚未 Accepted。

## 1. 问题与成功标准

本研究回答：

1. 用户所说的“ParaEGOX 应用”与 Card、Deck、CoreService、Artifact、Release、Deployment、DeploymentScope 和运行实例分别是什么关系。
2. 当前是否应立即引入 `ApplicationSpec`、Application 安装身份、Application service 或新的控制器。
3. 一个只属于某个产品、需要跨 DeckRun 保存状态的服务应放在哪里。
4. 当前 `ApplicationTopology` 是否有真实的 Application owner，以及 DeckLock 和 topology 是否形成双真相。
5. 如何既不重建 EAGOS Bundle 式万能聚合，也不封死未来多 Deck 产品。

成功标准：

- 每个一等对象都有独特语义、producer、consumer、owner 和 failure boundary。
- source spec、解析闭包、部署决策、安装身份与运行身份不能互相冒充。
- 共享平台服务不能因某个应用停止或卸载而被误删。
- 应用私有状态不能被迫藏入 Card 全局变量，也不能为图省事升级成跨应用 CoreService。
- 未来引入 Application 时不产生第二个 DeploymentController、第二份 desired truth 或第二套 Runtime 身份。
- 当前 P0–P2e 可以实现，不要求先创建无消费者的包、Schema、数据库或 controller。

## 2. 范围与非目标

本文研究产品和系统模型边界，不定义：

- 最终 `ApplicationSpec`、安装 API、Marketplace、计费或租户 Schema。
- `deck.yaml`、`deck.lock` 和 ServiceSpec 的完整字段编码。
- Web/TUI 产品形态、商店 UI 或发行渠道。
- 应用私有数据库的具体存储引擎、备份工具或迁移框架。
- 用 Application 取代 DeploymentController、RuntimeHost、Artifact、Gateway 或 CoreService。

“当前不冻结 Application 类型”不是宣称 ParaEGOX 不做应用平台，而是拒绝在应用独有语义出现之前创造 Deck 的同义对象。

## 3. 证据与置信度

### 3.1 本地证据

当前文档已经形成以下真实权威链：

```text
CardDefinition ─referenced by→ Card
Card + Link + Requirement ─authored in→ DeckSpec
DeckSpec ─resolve/compile→ DeckLock
DeckLock + ServiceSpec + target facts + policy ─plan→ DeploymentPlanCandidate
DeploymentController ─commit/reconcile→ DeploymentPlan @ DeploymentRevision
RuntimeHost ─realize→ DeckRun / CardInstance / ServiceInstance
```

但旧文档存在一个不一致：`ApplicationTopology` 由 DeckSpec/DeckCompiler 完全拥有，却没有 ApplicationSpec、ApplicationLock、Application installation 或 Application controller；部分流程又把 `DeckLock + ApplicationTopology` 并列交给 DeploymentPlanner。这样既没有真正的 Application owner，也允许 topology 与 lock 漂移。

已有边界还说明：

- `DeckRun`、`CardInstance`、`ServiceInstance` 是运行身份；不存在需要由通用 `ApplicationInstance` 再包装的运行对象。
- DeploymentController 已经是每个 DeploymentScope 的唯一 desired-state writer；新 Application controller 会与它争夺 revision、rollout 和 reconcile 权限。
- CoreService 被定义为跨 Deck 共享、长期存在或持有平台权威的服务，普通 Deck 只能声明 ServiceRequirement。
- 当前没有一个合法对象表达“只属于一个产品安装、跨 DeckRun 持久、但不应跨产品共享”的状态。这是必须明确承认的能力缺口，不能靠错分类掩盖。

对两个前身代码库的只读职责审计又补充了三条证据：

- Motus Card 目前是 Canvas 中的轻量配置使用，而不是后端领域类型；Canvas 同时保存语义连接、视图坐标、动态 Topic 和运行启停状态。ParaEGOX 应保留“配置一次能力”的 Card 心智模型，但把视图、解析和运行事实分离出去。
- Motus `PerceptionBundle` 是单 endpoint/进程内对多个 Plugin/Tool 的实现聚合，不是多 Card 工作负载、发布包或安装身份。
- EAGOS 的 Bundle 名称被 source、lock、交付/安装和 runtime/deployment 等多个协作 surface 复用；各表面的成熟度与真正动作 owner 并不相同，不能从同名推导为一个完整生产对象。ParaEGOX Deck 只能承接 source 中的工作负载组合子集；其余职责分别归 DeckLock、Artifact/Release、Deployment、RuntimeHost、Trust、Inspection/Ops 和未来 Installation owner。

因此不存在 `Card ≈ EAGOS Bundle` 的合法映射。Card 是 Deck 内一个叶级配置使用；EAGOS Bundle 是跨多个架构层复用的 surface-family 名称。直接沿用 Bundle 名字会把已拆开的身份重新暗示为一个整体。

### 3.2 外部一手先例

- [OAM Application specification](https://github.com/oam-dev/spec/blob/master/7.application.md)把 Application 作为组件实例和发布配置的聚合，而真正运行的是 component workload。它证明 Application 可以是交付面对象，也说明 ownership 会带来删除和回收语义。
- [KubeVela core concepts](https://kubevela.io/docs/getting-started/core-concept/)把 Application 表达为 components、traits、policies 与 workflow 的声明；[其架构](https://kubevela.io/docs/getting-started/architecture/)明确这是 application delivery/operation control plane。它支持“Application 不直接执行”，但其宽泛 workflow/policy 模型也展示了 god object 风险。
- Kubernetes 的 [workload 概念](https://kubernetes.io/docs/concepts/workloads/)与 [Workload API](https://kubernetes.io/docs/concepts/workloads/workload-api/)分开静态 workload policy 和运行期对象；Kubernetes 核心长期没有统一 Application 对象，而用 [推荐 labels](https://kubernetes.io/docs/concepts/overview/working-with-objects/common-labels/)组织资源。它说明系统不必为了 UI 分组立即创造运行实体。
- [Kubernetes owners and dependents](https://kubernetes.io/docs/concepts/overview/working-with-objects/owners-dependents/)区分关联标签与会触发级联回收的 owner reference。对 ParaEGOX 而言，Application ownership 不能只是命名；它必须回答停止、卸载、保留和删除。
- [Helm 的 Chart 与 Release](https://helm.sh/docs/intro/introduction/)分开可分发包和一次安装；同一 Chart 可有多次 Release。它证明 Product/Artifact/Release/Installation/Run 不能压成一个对象。
- ROS 2 的 [Node](https://docs.ros.org/en/lyrical/Concepts/Basic/About-Nodes.html)、[Launch](https://docs.ros.org/en/lyrical/Tutorials/Intermediate/Launch/Creating-Launch-Files.html)和 [Composable nodes](https://docs.ros.org/en/ros2_documentation/lyrical/How-To-Guides/Launching-composable-nodes.html)提供可复用执行单元与启动组合，但不提供稳定的产品安装和数据所有权聚合。它接近 Card + Deck 的执行视角，不足以解决 Application 交付语义。
- [Dora dataflow YAML](https://dora-rs.ai/dora/concepts/dataflow-yaml.html)以有向 dataflow 组织 node、input、output 与 execution，支持 Deck 作为可执行图的方向；但 ParaEGOX 仍应把 source/build artifact、解析锁和目标部署从 topology 中分离。
- [Dapr terminology](https://docs.dapr.io/concepts/terminology/)与 [state management](https://docs.dapr.io/developing-applications/building-blocks/state-management/state-management-overview/)把 application、平台 building blocks 和可替换 state store 分开。它支持“平台服务是应用依赖，不等于应用拥有平台服务”。

### 3.3 推断与置信度

高置信结论：

- Application 即使未来成为一等对象，也应位于控制/交付面，不是运行执行器。
- Deck、Artifact、Release、Deployment、DeploymentScope、Run 必须保持不同身份。
- 现在的 `ApplicationTopology` 实际是 Deck topology，应改名并锁入 DeckLock；长期 Schema 需由 ADR-0004 接受后冻结。
- 不能新增与 DeploymentController 并行 reconcile 相同资源的 ApplicationController。

中等置信结论：

- ParaEGOX 未来大概率会需要某种稳定的产品安装/所有权身份，但当前没有足够 producer/consumer 证据冻结其名称和 Schema。
- 应用私有持久服务很可能复用 ServiceContract/ServiceSpec/ServiceInstance 的大部分机制，但其 owner/lifetime/GC 字段必须由真实用例验证。

## 4. 不能混用的七层对象

| 层 | 当前或候选对象 | 回答的问题 | 不是 |
| --- | --- | --- | --- |
| 产品/体验 | 自然语言“应用” | 用户购买、理解或操作的产品是什么 | 当前不是公共 API identity |
| 可复用能力定义 | CardDefinition | 一个可版本化能力提供什么合同 | 配置使用、包、进程 |
| 可执行组合 | DeckSpec | 哪些 Card、Link、Requirement 构成一份工作负载意图 | 安装包、平台服务容器 |
| 可重复解析闭包 | DeckLock | 这份 Deck 精确解析成什么 | live provider、placement、运行状态 |
| 交付物 | Artifact / 未来具体 Release 类型 | 代码、模型、客户端和元数据如何版本化、签名、分发 | 运行实例 |
| 部署期望 | DeploymentPlan @ DeploymentRevision | 在一个 DeploymentScope 中运行什么、放在哪里、如何绑定 | 产品身份、源码工程 |
| 运行事实 | DeckRun、CardInstance、ServiceInstance | 这次实际运行发生了什么 | spec、release、installation |

`DeploymentScope` 只定义 desired-state 的写权与 revision 边界。一个应用可以将来跨多个 scope 部署，一个 scope 也可能承载多个应用；因此二者不能等价。

源码仓库或 IDE project 也不是 Application identity。一个仓库可以产生多个 Deck/Artifact，一个产品也可以引用多个仓库产物。

## 5. 方案比较

### 方案 A：永久规定 Deck 就是 Application

优点是词最少，当前一个语音机器人 Deck 也确实可以直接作为一个“应用”展示。

拒绝把这种映射永久化，原因是它会迫使 Deck 后续吞入：多 Deck 产品、安装记录、客户端 Artifact、私有持久状态、升级/卸载、数据保留和产品权限。最终 Deck 会重新长成 Bundle 式万能聚合。

允许的表述是：**首个 reference profile 中，一个产品应用可以由一个 Deck 完整表达。** 这是 profile 映射，不是身份等价不变量。

### 方案 B：现在创建完整 Application 家族

可能的家族包括 Application source spec、resolved lock/release、installation、status 和 application-owned service。

现在拒绝实施，原因是每一项都与现有对象重叠且没有独立消费者：

- source spec 与 DeckSpec 重叠。
- resolved lock 与 DeckLock 重叠。
- release 与 Artifact/Release 方向重叠。
- installation 尚无稳定身份、存储、CRUD 或数据生命周期需求。
- status 会与 Deployment/Inspection 重叠。
- 新 controller 会与 DeploymentController 冲突。

一次性创建整套目录和 DTO 只会把未知问题固化成公共兼容负担。

### 方案 C：Deck 收窄为 executable workload，Application 证据门后置

这是本研究的推荐方案，并已提交为 Proposed ADR-0004；在 ADR 接受前不是正式架构裁决。

- 立即明确 Deck 的独特语义和锁定边界。
- 允许产品 UI 用“应用”描述一个 Deck，但 API/Receipt 使用真实 Deck、Deployment 和 Run identity。
- 为未来多 Deck、安装和私有持久状态保留明确的 admission gate。
- 在 gate 触发前不创建 Application 类型、包、controller、数据库或伪 ID。

代价是必须诚实声明当前不支持一等的应用安装和应用私有持久服务，不能用命名假装已经支持。

### 方案 D：只给 ServiceSpec 增加一个 application scope

单独采用不足。没有稳定 Application/Installation identity 时，scope 无处引用；一个字符串 scope 也回答不了 cardinality、所有权、备份、迁移、GC、retain/delete、卸载授权和 Receipt。

未来 owner identity 成立后，明确 scope/lifetime 可以成为 ServiceSpec 的一部分；它不是当前绕过 Application 建模的捷径。

### 方案 E：将 Deck 改名为 Bundle

最强版本是把 `BundleSpec` 严格收窄为 Cards/Links/Requirements，同时拆开 Lock、Artifact、Installation 与 Deployment。它在技术上成立，也可能为既有 source asset、文档与用户词汇降低迁移成本，而不只是“表面连续性”。

当前仍不推荐改名：ParaEGOX 没有已发布 Bundle Schema 或兼容承诺，Motus 又把 Bundle 用于 provider/endpoint 内插件聚合；在干净起点上，`Card → Deck` 更清楚，也更难误读为 Artifact、Release 或 Installation。若 ADR 接受前出现必须直接导入既有 Bundle source 的真实要求，应以代表性资产、转换成本和用户词汇证据重开裁决。

迁移说明可以保留“EAGOS Bundle source 的工作负载组合子集迁移到 Deck”这句话，但不应建立 `Bundle = Deck` alias。若未来需要单文件离线分发，可研究 `DeckArchive` 等限定名称；若需要多 Deck 产品与稳定安装 owner，则通过 A0 研究 ProductRelease/Installation，而不是扩大 Deck。本文不替未来窄对象预先冻结名字。

## 6. 推荐模型（等待 ADR-0004 评审）

### 6.1 Deck 的精确定义

ADR-0004 提议将 `Deck` 定义为 ParaEGOX 的声明式可执行工作负载/组合单元：它表达一组需要共同验证和部署的 Cards、Links、DeliveryProfile 及 Service/Permission/Feature Requirements。

Deck 不等于：

- Product 或品牌应用。
- Artifact、Package、Release 或离线 Archive。
- 一次安装。
- DeploymentScope。
- Runtime、进程、容器或 DeckRun。
- CoreService 容器。
- 任意 workflow/hook 执行器。

直接连接内部 Card Port 的 Cards 必须位于同一个 Deck。共同 SLO、failure containment 或 revision 是否必然要求合并，需要由实际部署和 future multi-Deck consistency fixture 验证；当前不为 UI 分组任意拆分紧耦合 Card，也不预先禁止由 DeploymentController 实施跨 Deck 一致性 rollout。

### 6.2 唯一解析产物

```text
DeckSpec
   │ parse + source validation
   ▼
DeckCompiler
   ├── DeckResolver（内部纯步骤：版本、definition、schema、adapter、artifact candidate）
   └── canonical validation
   ▼
DeckLock
   ├── DeckTopology {structural keys/refs only}
   ├── resolved closure {exact definitions/schemas/artifacts/payloads}
   ├── locked requirement contracts/ranges in closure
   └── canonical digest covers all above
```

规则：

1. `DeckCompiler` 是纯编译入口；DeckResolver 是其中可测试的纯步骤，不拥有独立持久状态。
2. 唯一可持久、可传给 DeploymentPlanner 的解析产物是 `DeckLock`。
3. `DeckTopology` 是 DeckLock 内嵌的 canonical structure subtree：只保存稳定 Card closure key、以 closure key 限定的 Port endpoint key、Links、DeliveryProfile key/ref 和 RequirementRef key。
4. DeckLock digest 覆盖 DeckTopology；不能把 topology 作为旁路文件、UI state 或第二个 Planner 输入。
5. 精确 CardDefinition/version、Port/Schema、Artifact/adapter、DeliveryProfile payload 和完整 Requirement payload 只在 resolved closure 保存一次；validator 拒绝悬空 key、重复语义 entry、冲突版本和 key/payload mismatch。
6. Canvas 只编辑 DeckSpec，并只读展示 DeckCompiler 派生的 DeckTopology 验证投影；它不直接编辑或持久化 topology，坐标、缩放、折叠等 View State 不进入 DeckLock digest。
7. DeckLock 锁定 ServiceRequirement 的 contract/version/constraint，不锁定 live provider。DeploymentPlanner 根据 immutable ServiceSpec inventory、target facts 和 policy 选择 provider。

因此 Planner 的输入应写成：

```text
DeckLock { canonical DeckTopology + resolved closure }
+ ServiceSpec inventory
+ immutable target facts
+ DeploymentProfile / policy
+ previous-plan and stable-ID allocation snapshot
```

不再写 `DeckLock + DeckTopology`。

### 6.3 当前“应用”的产品表达

在文案、示例和 TUI 分组中可以称一个语音机器人 Deck 为“应用”。但在公共 DTO、Receipt、日志字段和权限目标中必须使用其真实 identity：DeckLock digest、`DeploymentId@DeploymentRevision`、DeckRunId 或 CardInstanceId。

禁止为了显示方便偷偷加入没有 owner 的 `application_id`。显示分组不产生级联删除、权限继承、数据归属或生命周期。

### 6.4 当前运行身份不增加一层

```text
DeckLock ─deployed as→ DeploymentRevision
                         ├── DeckRun
                         │    └── CardInstance*
                         └── ServiceInstance*
```

不建立 `ApplicationInstance`：它既不是进程，也不应复制 DeckRun、Deployment status 或 ServiceInstance 状态。聚合健康由 Inspection 按明确 revision、run、instance 和 freshness 投影。

一个 DeploymentRevision 可以替换 DeckRun；一个 Deck 也可以有历史或并行 DeckRun。不要把“当前运行”写回 DeckSpec/DeckLock。

## 7. Card、CoreService 与未来应用私有服务

### 7.1 分类原则

分类首先看所有权、寿命、共享范围、权威和故障边界，不看它是不是 Python 类、daemon、模型进程或 UI 方框。

| 问题 | Card / CardInstance | CoreService | Gateway / Driver | 当前未建模的应用私有服务 |
| --- | --- | --- | --- | --- |
| 主要用途 | Deck 内领域计算或控制能力 | 跨独立 Product/Installation 共享或持有平台权威的能力 | 外部协议、浏览器或硬件边界 | 只属于一个稳定产品安装的长期领域能力 |
| 典型寿命 | 随 DeckRun | 随 Node/平台，独立于普通 DeckRun | 随受管 workload 或外部 session | 跨 DeckRun/升级，随安装保留或删除 |
| 状态 | 私有、默认可重建 | 可持久、共享或平台权威 | 以外部状态/连接为主 | 私有持久状态与明确迁移/保留策略 |
| 谁可停止 | 经授权的 Deck operation | 普通 Deck 不可停止 | 各自 workload/session owner | 未来安装 owner，不能由任意 DeckRun 决定 |
| 当前是否有正式模型 | 有 | 有 | 边界研究中 | 没有；显式 gap |

“长期运行”本身不是 CoreService 判据。只为一个产品保存对话历史的服务，即使是 daemon，也不应自动成为平台 Memory CoreService；反过来，被多个 Deck 共享的模型推理服务不应因为某个应用引用它就变成 application-owned。

还要分开一个更窄的情形：能力只在一次 DeckRun 内被多张 Card 使用，在 DeploymentController 使对应 Deck workload terminal 后即可回收，也没有跨运行持久状态。若它是拓扑可见、可配置、可隔离的领域计算，优先建模为 Card；若真实消费者必须通过版本化 ServiceContract 共享它，才研究 `DeckRun`-scoped ServiceSpec/ServiceInstance。这不需要 Application identity，也不能因为 API 长得像 service 就升级成平台 CoreService。

### 7.2 典型例子

| 需求 | 建议对象 | 理由 |
| --- | --- | --- |
| ASR/TTS 算法节点 | CardDefinition → Card → CardInstance | 可配置、可连接、随 DeckRun 替换的领域计算 |
| 多个独立 Product/Installation 共享的大模型推理池 | Model CoreService | 独立资源预算、版本化服务合同、平台作用域明确 |
| 一个 CardInstance 的短期 VAD/解码缓存 | CardInstance 私有实现状态 | 与实例同寿命、默认可重建 |
| 同一 DeckRun 内多张 Card 共用的临时协调/缓存 | 优先 Card；必要时研究 DeckRun-scoped ServiceSpec | 随 DeckRun 回收，不产生安装级数据 owner |
| 一个产品安装的长期对话历史 | 当前 gap；未来 application-owned domain service | 应跨 DeckRun，但不应跨产品共享或藏入 Card 全局变量 |
| 摄像头 SDK/ROS2 协议适配 | Driver/Gateway | 外部协议与设备 session 边界 |
| 视觉检测算法 | CardDefinition/Card | 可复用领域计算，不拥有摄像头协议 |
| Console/WebRTC/WebXR 接入 | Gateway managed workload | 不可信客户端、媒体/session 和 external exposure 边界 |
| Web/TUI 客户端静态资源 | Artifact/产品交付物 | 不是 Runtime Card，仅因属于产品也不应进入 Deck |

### 7.3 应用私有持久服务的演进缝隙

当前版本明确不把这种服务伪装成 Card 或 CoreService。未来真实用例出现时，优先研究复用现有 `ServiceContract → ServiceSpec → ServiceInstance` 运行机制，并先把三个 owner 拆开：

| Owner | 最小职责 |
| --- | --- |
| Service workload owner | 声明、启动、升级、停止和替换 ServiceInstance |
| durable data owner | 拥有候选 `StateNamespaceRef`、schema/migration、retain/delete/transfer 与唯一 GC authority |
| storage custodian | 实际保存 bytes、执行 provider 级备份并报告存储事实；不因此取得 namespace 删除权 |

在此基础上再补足以下正交字段：

- 稳定 owner/installation reference。
- owner scope 与 cardinality。
- lifetime：run-bound、installation-bound 或 platform-bound。
- state namespace、schema/version 与 storage custodian reference。
- upgrade/migration/rollback protocol。
- backup、retention、retain/delete/transfer policy。
- stop、uninstall、GC 的授权 owner 和 Receipt。
- 多副本、故障转移和 split-brain fencing。

在这些语义有证据前不冻结 `ApplicationService` 这一公共类型，也不建立 CoreService 子类。实现形态相似不等于所有权相同；application-owned data 也不意味着每个 installation 必须拥有独立服务进程。

其中 run-bound service 可以引用 DeckRun owner，不要求先引入 Application；只有 installation-bound/private durable service 才需要稳定 Application/Installation owner。未来两者应扩展同一 scoped ServiceDependencyGraph 的 vertex，不创建第二套 `ApplicationServiceGraph`。不能用一个模糊 `scope` 字段同时替代这两套生命周期。

首版边界应写成：

- CardInstance 私有状态默认只保证 CardInstance/DeckRun 范围。
- 平台持久状态必须由明确 CoreService owner 管理。
- 应用私有、跨 DeckRun 的持久状态暂不支持；需求出现即触发本研究的 Application admission gate。

## 8. 未来何时引入正式 Application

### 8.1 A0 Application Admission Gate

出现下列任一不可规避场景，必须进入 ADR-0004 提议的 A0 gate，不能继续把需求塞入 Deck、Card 全局状态或 CoreService：

1. 一个命名产品需要引用两个以上可独立演进的 DeckLock，并统一 install/update/uninstall 或版本闭包。
2. 状态必须跨 DeckRun 和升级存在，但不能成为跨产品 CoreService。
3. 同一产品 release 需要安装多次，每次拥有隔离配置、权限、数据和生命周期。
4. Deck、Web/TUI 客户端、Gateway workload、私有 service 等多种 Artifact 需要一个签名发布闭包。
5. 产品级配置、授权、数据驻留、保留、计费或 GC 需要跨 DeploymentRevision 的稳定 owner。
6. API 有真实 consumer 需要稳定的 install/list/update/uninstall/watch/status identity，而 Deck 仍然只代表可复用工作负载蓝图。

只有 catalog/marketplace 展示元数据不是充分证据；此时应引入更具体的 ProductRelease/CatalogEntry，而不是万能 Application。

一个只随 DeckRun 存活的临时共享 service 也不是 Application 触发条件；它应先验证 Card 是否足够，或单独研究 DeckRun-scoped ServiceSpec。

A0 的准入材料由公共证据和按触发条件选择的 fixture 组成：

1. 公共证据：一个候选稳定 identity producer 和两个独立 consumer；若存在不可逆删除，单一 GC authority 本身可以构成强 consumer。owner matrix 和 failure surface 只覆盖本次真实操作。
2. 多 Deck 闭包：至少两份可独立演进的 DeckLock、统一 release/update/uninstall 诉求和 partial rollout/rollback fixture。
3. 安装私有状态：稳定 owner 候选、一个私有 namespace、schema/migration/backup/retain/delete 语义和 crash/migration/partial-GC fixture。
4. 多次安装或多 Artifact 闭包：两个隔离安装，或 Deck/Gateway/client/private-service Artifact 的共同 owner fixture；覆盖重复请求、partial install/uninstall 和隔离失败。
5. 旧 writer/split-brain、安装记录与 Deployment commit 间 crash 等 Harness 只在对应风险存在时要求；不能强迫纯多 Deck 场景制造私有 namespace，也不能强迫单 Deck 私有状态场景制造第二份 DeckLock。

A0 只有两个合法出口：形成并接受一份只定义实际最小 owner 的后继 ADR（可能是 ProductRelease、Installation、Application 或更窄对象），或继续用稳定 diagnostic reason code 拒绝该能力。研究结论、UI 分组或自由字符串 `scope` 不能自行越过 gate。

### 8.2 未来 Application 的允许职责

若触发，Application 最多是 Deck 之上的控制/交付/所有权聚合，可以：

- 引用一个或多个 immutable DeckLock。
- 引用产品客户端、Gateway 和其他 Artifact release。
- 声明 application-owned service 与数据保留策略。
- 提供稳定 installation identity 和 release history。
- 向 DeploymentPlanner/DeploymentController 提交不可变、已解析的输入。
- 由 Inspection 聚合状态。

候选关系仅用于约束未来研究，不是当前 Schema：

```text
Product release
   └── stable installation identity
          ├── DeckLock*
          ├── application-owned ServiceSpec*
          ├── Gateway exposure refs*
          └── client/artifact refs*
                    │
                    ▼
             DeploymentController
                    │
                    ▼
      DeckRun / CardInstance / ServiceInstance / Gateway facts
```

### 8.3 未来 Application 的禁止职责

即使引入，也不得：

- 创建第二个 ApplicationController 去 reconcile Runtime resources。
- 直接决定 placement、线程、PID、ExecutionDomain、Zenoh route 或 live binding。
- 直接启动/停止 CardInstance、ServiceInstance、Driver 或 Gateway process。
- 拥有 raw RuntimeHost、Zenoh Session、设备或 credential。
- 停止、升级或 GC 平台 CoreService。
- 把 source spec、resolved lock、release、installation、deployment 和 observed status 混进一个可变大对象。
- 通过任意 hooks/workflow 绕过 committed DeploymentPlan 和 Authority。

DeploymentController 继续是 DeploymentScope 的唯一 desired-state writer；Application resolver/catalog 若存在，只能位于 Planner 上游。Application status 是 Inspection projection，不是新的运行状态机。

## 9. 多 Deck 应用的组合纪律

未来一个 Application 如果包含多个 Deck：

- 每个 Deck 仍有自己的 DeckLock 和清晰可部署边界。
- 直接内部 Card link 必须保留在同一个 Deck；不得从另一个 Deck 按内部 Card 名称寻址。
- multi-Deck consistency group、共同 revision、原子 rollout 与跨 Deck rollback 由 A0 fixture 和后继 ADR 决定；当前不强制合并，也不提前承诺支持。
- 跨 Deck 交互使用显式导出的 typed ServiceContract、Port/Gateway endpoint 或其他经 ADR 冻结的边界；禁止按内部 Card 名称直接寻址。
- 应用 release 可以约束多个 DeckLock 的兼容集合，但不能修改它们的内容。
- Application upgrade 产生新的发布/安装期望，再由 DeploymentController 生成 DeploymentRevision；它不直接修改 live instance。
- 经 DeploymentController deactivate/replace Deck workload 后，旧 DeckRun 进入 terminal state；该事件不应删除 application-owned durable data。未来只有 installation uninstall owner 可以为 retain/delete/transfer 签发独立 Receipt。

## 10. `DeckCatalog` 与产品发布术语

现有文档曾把 `DeckCatalog` 写成“已安装应用索引”，这会混合模板发现与安装状态。

当前约束：

- 若未来出现 `DeckCatalog`，它只索引可发现的 Deck definition/release metadata，不拥有安装、运行或升级状态。
- 已安装产品索引必须等待稳定 installation identity；不能用 DeckCatalog、ArtifactStore 或 Deployment status 伪装。
- `DeckArchive` 若出现，只是离线交换/交付格式，不是 Application 或 installation。
- 一个 release 只需要打包一个 Deck 时，不必因此引入 Application。

## 11. 风险、反例与失效方式

### 11.1 过早引入 Application

失败模式：

- ApplicationSpec 与 DeckSpec 两份源真相。
- ApplicationLock 与 DeckLock 两份解析闭包。
- ApplicationController 与 DeploymentController 争夺 rollout。
- ApplicationInstance 与 DeckRun/Deployment status 重复。
- ApplicationService 同时吞 catalog、状态存储、部署和运行控制。
- 应用删除误删共享 CoreService。

### 11.2 永久不引入 Application

最强反例是：一个产品由多个独立 Deck 组成，拥有跨 DeckRun 私有状态，同一 release 可安装多次，并要求可审计的升级、卸载和数据保留。此时只用 Deck 会产生 owner vacuum：状态要么藏进 Card，要么被错误提升为平台 CoreService。

如果这一场景已成为首发承诺，就不能继续按当前方案直接实现；应将 Application 研究前置，并用“两份 DeckLock + 一个私有持久 service + 一个 Gateway/client artifact + 升级/卸载/retain-delete 故障 Harness”作为最小模型 fixture。

### 11.3 名义 scope 代替所有权

给 ServiceSpec、label 或 DeploymentScope 增加 `application: foo` 只建立关联，不建立合法 owner。没有稳定 identity、authority、lifecycle 和 GC Receipt 时，任何自动删除都不安全。

### 11.4 Application 变成 workflow engine

若任意 hook 能绕过 DeploymentPlan 直接创建资源，它会产生不在 release/revision 跟踪中的孤儿副作用。产品安装流程必须分解为有 owner、可查询、幂等或显式 uncertain 的 operation，而不是脚本成功即视为系统成功。

## 12. 分阶段行动计划

### P0：文档与裁决准备

1. 全文将 `ApplicationTopology` 原子迁移为 `DeckTopology`。
2. 提交 Proposed ADR-0004，提议 Deck 是 executable workload，而非 Product/Release/Installation。
3. 在 ADR-0004 评审通过后冻结 `DeckCompiler → DeckLock {canonical DeckTopology + resolved closure}`；Planner 不接收独立 topology。
4. 明确 DeckLock 锁 requirement contract/range，Planner 才根据 ServiceSpec 与 target facts 选择 provider。
5. 明确当前没有正式 Application identity，也没有 application-owned durable service。
6. 不创建 `applications/`、Application DTO、ApplicationController 或 ApplicationService 空包。
7. 保留 `Deck` 公共名称；禁止在 Schema/package/API 中把 `Bundle` 作为 Deck 同义词。历史来源保留原名，未来全新窄对象的命名另行裁决。

### P2e：首个实现证据

1. 同一 DeckSpec 与相同 resolver inputs 产生字节稳定的 DeckLock/digest。
2. 修改 Cards、structural Port endpoint key、Links、DeliveryProfile、closure payload 或 locked refs 必须改变 DeckLock digest；validator 拒绝悬空 key、重复语义 entry 和 key/payload mismatch。
3. 只修改 Canvas View State 不改变 digest。
4. DeploymentPlanner 的函数签名只消费 DeckLock，不消费独立 DeckTopology。
5. 同一 byte-identical DeckLock 配不同 immutable ServiceSpec inventory/target facts 产生不同且可解释的 provider candidate，DeckLock/digest 不变。
6. DeploymentController deactivate/replace Deck workload 并使旧 DeckRun terminal，不停止其依赖的 Authority、Fabric、Inspection 或共享 Model CoreService。
7. 两张 ASR Card 各有私有实例状态；不得用进程全局变量模拟应用持久状态。

### 第一个 reference application

选择一个可由单 Deck 完整表达的语音/具身闭环作为应用示例：Sensor/Input → ASR → Agent/Reasoner → TTS/Controller-role Card，并通过 ServiceRequirement 使用 Model/Memory/Authority/Fabric 等平台服务。它验证 Card/Deck/CoreService 分类，但不借示例提前声明 Marketplace 或 installation 已实现。

### Application gate 触发后

1. 先建立 research fixture，不先命名全套 DTO。
2. 验证多 Deck release、稳定安装 identity、application-owned state、升级/回滚、卸载 retain/delete 和 controller ownership。
3. 明确 ProductRelease、Installation、Service owner scope 中究竟哪些有独立 consumer。
4. 形成 Proposed ADR；只有 Accepted 后才新增公共类型和目录。

## 13. 验证矩阵

| 声明 | 必需证据 |
| --- | --- |
| DeckLock 是唯一解析真相 | compiler golden/property test；无独立 topology 输入或持久文件 |
| DeckTopology 被 digest 覆盖 | Card key/ref/config/role/refinement、Port/Link/Delivery/locked-ref mutation test；旁置 display metadata 不改变 DeckLock digest |
| Card key 不暗示运行或状态连续性 | previous-plan diff 中同 key 表示同一 desired slot；rename 为 remove + add；删除后复用不恢复旧实例/状态 |
| Canvas 不是真相 | 坐标、缩放、折叠变化不改变 DeckLock digest |
| Requirement 与 provider 分层 | 相同 DeckLock 在不同 ServiceSpec/target facts 下得到不同、可解释的 candidate；DeckLock 本身不变 |
| CoreService 不受 Deck 生命周期支配 | deactivate/replace Deck workload、旧 DeckRun terminal 后共享服务保持 Ready，引用与权限正确回收 |
| Card 状态不越界 | CardInstance replacement 不继承未声明全局状态；旧 incarnation callback 被 fencing |
| 没有隐式 Application | API/Receipt/权限中不存在无 owner application_id；UI 分组不触发 GC |
| Deck/Bundle 无伪别名 | 工作负载 Schema、包、API 与 Receipt 使用 Deck；Bundle 不成为 Deck 兼容类型，未来全新窄对象需独立裁决 |
| 未来 Application 不成为第二控制器 | proposed fixture 中只有 DeploymentController 能 commit revision/apply Runtime |
| 应用私有数据生命周期明确 | 两个 installation 共用一个平台 provider；卸载 A 不停止 provider、不删除 B，只按 A 的权威 Receipt 处理 A namespace；另覆盖 upgrade/migration/rollback/backup failure |

## 14. 决策记录影响

- 本文不改变 [ADR-0002](../adr/ADR-0002-card-definition-terminology.md) 已接受的 `CardDefinition → Card → CardInstance` 决策。
- `ApplicationTopology → DeckTopology` 是对现有 owner 的术语纠正；Topology/closure 的长期 Schema 由 Proposed ADR-0004 评审，不新增运行或部署 authority。
- [ADR-0001](../adr/ADR-0001-deployment-controller-boundary.md) 的单写 DeploymentController 决策继续成立；未来 Application 也不能绕过它。
- 已创建 Proposed [ADR-0004](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md)承载 Deck/Application 准入边界，但它不定义或批准 Application 类型。触发 A0 后仍需建立并接受只覆盖真实需求的最小 ProductRelease/Installation/Application 或其他窄 owner ADR。
- 如果未来只需要产品发布/目录元数据，应为 ProductRelease/CatalogEntry 建立更窄决策，不自动触发万能 Application。

## 15. 当前开放问题

- 首个 reference application 是否确实可以由一个 Deck 完整表达。
- 第一个需要跨 DeckRun 保存、但不能由平台 Memory/World CoreService拥有的状态是什么。
- 首个实现固定为 DeckRun-bound；第一个跨 DeckRun/升级/重新安装续接的真实需求由谁稳定拥有。产品安装私有 owner 触发 A0；真正的平台/租户 owner 则需要独立 CoreService/tenant ownership ADR 与隔离证据。
- Web/TUI 客户端和 managed Gateway 是否需要与 Deck 形成统一签名 release closure。
- 同一产品是否有多次隔离安装的真实需求，以及 isolation key 由谁签发和持久化。
- 应用卸载的数据保留、迁移和删除是否有法规或安全要求。
- 跨 Deck interaction 的第一个真实消费者更适合 ServiceContract、Gateway endpoint 还是未来限定 Port export。

这些问题不会阻塞 Kernel、RuntimeHost、DeckCompiler 与 P2e Deployment control plane；它们会决定 ParaEGOX 何时需要比 Deck 更高的一层产品/安装模型。
