# Node 与作用域边界

> 状态：Draft
> 日期：2026-07-31
> 范围：Node、RuntimeHost、部署作用域、Fabric/空间命名和物理控制边界
> 实现状态：NodeIdentity/NodeSpec/NodeStatus、NodeDaemon 与 scope contracts 尚未实现；S6 仅实现 private local POSIX ProcessDomain、idle RuntimeHost substrate 和独立 service-manager/watchdog reference adapter。本文仍只限制术语、身份和所有权，不冻结具体 Node Schema
> 架构裁决：[ADR-0001 — DeploymentController、DeploymentPlan 与 Runtime 边界](../adr/ADR-0001-deployment-controller-boundary.md)
> Application 边界：[Application、Deck、Card 与 Service 边界研究](../research/application-deck-card-service-boundaries.md)

## 一句话结论

ParaEGOX 正式保留短名 `Node`，但将稳定身份、期望配置和观测状态分开；Kernel 外的 DeploymentController 按 scope 单写期望部署，`NodeDaemon` 产生 Node observed facts，`RuntimeHost` 只拥有本地执行与 Runtime apply admission。NodeDaemon 不位于 DeploymentController→RuntimeHost apply 权威链中。当前只保留非权威 `site_hint`，并禁止裸用 `Region` 与 `Topology`，以免网络、空间、应用和安全边界再次合并成一个万能模型。

## 1. 建模原则

分布式具身系统中，下列边界可能暂时重合，但没有共同身份或生命周期：

- 计算单元与本地运行宿主。
- 期望部署与实际运行状态。
- 物理或行政位置与网络路由结构。
- Deck 工作负载连接关系与平台服务依赖。
- 空间地图区域与 Fabric 网络观测。
- 资源占用、授权范围、安全联锁和坐标关系。

ParaEGOX 使用带限定名的引用和关系，不建立统一 `TopologyManager`、Node 大对象或 Robot 聚合根。简单 DeploymentProfile 可以声明一对一映射，但映射不产生身份等价、权限继承或共同生命周期。

## 2. 术语决策与 owner

| 术语 | 决策 | 限定含义 | 权威 owner |
| --- | --- | --- | --- |
| `Node` | 保留 | 可独立识别、管理和断连的 ParaEGOX 计算单元 | Enrollment/Deployment；Node facts 由 NodeDaemon 产生 |
| `NodeDaemon` | 保留 | OS-resident 节点管理进程角色；拥有 Node presence/status、feature report、Runtime endpoint discovery 与窄 NodeManagementEndpoint | NodeDaemon；进程候选名 `paraegox-noded` |
| `NodeIdentity` | 保留 | 稳定 `NodeId`、身份主体和身份材料引用 | Identity/Enrollment |
| `NodeSpec` | 保留 | 期望标签、约束和允许的 DeploymentProfile | Deployment |
| `NodeStatus` | 保留 | 当前 incarnation、NodeFeatureReport、容量、端点和 freshness | NodeDaemon 产生，InspectionService 投影 |
| `RuntimeHost` | 保留 | Node 内本地执行、生命周期、恢复和关闭的 owner | 目标 Node 上的 RuntimeHost |
| `DeploymentScope` | 保留 | committed DeploymentPlan/Revision 与单写 DeploymentController 的 write-authority 边界 | DeploymentController + DeploymentTenureAuthority |
| Product “Application” | 暂不建公共类型 | 产品/自然语言聚合；首个 profile 可由一个 Deck 完整表达 | 当前无系统 identity/owner；等待多 Deck/安装/私有状态证据 |
| `site_hint` | 保留 | 非权威部署与展示提示 | 声明它的 DeploymentProfile/DeploymentSpec |
| `SiteRef` | 后置 | 经注册的站点身份 | 未来 Site inventory |
| `ZenohTopologyProfile` | 保留 | Zenoh router/client、连接、gateway 和 Region 的期望配置 | Deployment/Fabric 配置 |
| Zenoh region observation | Fabric 私有投影 | 运行时相对 region 标识和连接事实；不冻结公共类型，也不是跨重连稳定 ID | FabricService self-inspection |
| `SemanticRegionRef` | 后置 | 特定 SpatialMap 与 MapEpoch 下的语义空间实体 | 未来 SpatialMap/World 服务 |
| `DeckTopology` | 保留 | DeckLock 中由 digest 覆盖的 canonical resolved Card、Port、Link 与 DeliveryProfile 结构；源意图来自 DeckSpec | DeckSpec 声明；DeckCompiler 解析、验证并内嵌到 DeckLock |
| `DeploymentPlan.bindings` | 保留 | DeploymentPlanner 先将 candidate desired endpoint、Zenoh route/locality、Schema/codec、Fabric ingress limits、target admission boundary 与 FeatureMismatch/FeatureLoss 结果写入 DeploymentPlanCandidate；DeploymentController 原子提交后才成为权威 revision | DeploymentController / committed DeploymentPlan |
| `PortBinding` | 保留 | RuntimeHost/Fabric 根据 DeploymentPlan 投影到 RuntimePlanSlice 的 binding assignment 安装的唯一公共 live binding 及 observed endpoint/active route/session/epoch；不拥有第二份队列 | RuntimeHost/Fabric binding owner |
| `ServiceDependencyGraph` | 保留 | CoreService 的 provides/requires 依赖 DAG | ServiceSpec 声明；DeploymentPlanner 编译；DeploymentController 提交 revision |
| `ControlledResourceRef` | 保留 | 可以被占用或产生物理副作用的受控资源引用 | Resource registry/coordinator |
| `CapabilityScope` | 保留为值 | CapabilityGrant 能作用的 resource selector + operations；不建立独立 owner | AuthorityService |
| `SafetyDomain` | 后置 | 必须共同停止或受同一联锁约束的资源范围 | 未来本地 Safety subsystem |
| `SpatialMapRef` / `FrameGraphRef` | 后置 | 地图 epoch 与坐标变换关系 | 未来 Spatial/Geometry 服务 |
| `Robot` / `Embodiment` | 非核心 | 仅允许产品语言或后续领域研究 | 不设基础 owner |
| `ComputeNode` | 禁止 | 不作为 `Node` 的平行别名 | — |
| 裸 `Region` | 禁止 | 无法区分 Zenoh 与语义空间 | — |
| `FabricRegion` | 禁止 | 当前只有 Zenoh Fabric，没有第二层抽象的证据 | — |
| 裸 `Topology` | 禁止 | 无法区分应用、服务、网络与空间关系 | — |
| `RemoteDomain` | 禁止 | 远端执行不由调用侧 RuntimeHost 拥有 | — |

InspectionService 只能投影权威 owner 的事实，OpsService/TUI 只能消费该投影，不能因为负责查询或展示就成为注册、拓扑、资源或运行状态的真相来源。

`DeploymentScope` 不得兼作 Application/Installation identity。它只解决“一组 desired state 由谁提交 revision”；未来一个产品可能跨多个 scope，一个 scope 也可能承载多个产品工作负载。当前 UI 可以把 Deck 展示为应用，但 API、Receipt、权限和生命周期必须继续使用 DeckLock/Deployment/DeckRun/Instance 的真实 identity，不得引入无 owner 的 `application_id`。

`Agent` 和 `Supervisor` 保留给 Agent 层，不作为节点基础设施命名。节点驻留管理进程不得叫 `NodeAgent` 或 `NodeSupervisor`，统一叫 `NodeDaemon`；daemon 表示 OS-resident 角色，不限定 Unix 进程形态，在 Windows service 或容器中仍沿用同一逻辑名。

## 3. Node 的三个权威形态

`Node` 是实现 ParaEGOX 节点管理与 Inspection 协议的计算单元。硬件形态不是判据：设备侧计算机、边缘服务器、云实例或受控嵌入式目标都可以成为 Node；只通过 Driver 被管理、不能报告身份和 incarnation 的 MCU 或设备不是 Node。

### 3.1 NodeIdentity

`NodeIdentity` 只表达跨重启稳定的身份：

- 稳定 `NodeId`。
- `PrincipalRef` 或身份材料引用。
- enrollment issuer 与必要的信任元数据。

它不包含 IP、Zenoh Session、readiness、CPU/GPU 容量、Site、Deck 或服务列表。

### 3.2 NodeSpec

`NodeSpec` 表达 Deployment 的期望事实：

- 允许的 platform/DeploymentProfile 与调度标签。
- 资源或数据驻留约束，而不是瞬时容量。
- 允许启动的 RuntimeHost 类型和权限上限。
- 关联的期望状态由 DeploymentController 通过 `DeploymentRevision` 标识。

NodeSpec 不保存 heartbeat、当前进程、实际端点或运行健康。

### 3.3 NodeStatus

`NodeStatus` 是带 freshness 的观测结果：

- 当前 `NodeIncarnation`。
- 已验证的 platform、architecture 与 `NodeFeatureReport`。
- 当前容量和可绑定设备事实。
- RuntimeHost endpoint/ref inventory；详细 RuntimeHostEpoch、readiness、failure 与 execution facts 仍由各 RuntimeHost 产生，聚合时保留原 producer/epoch/freshness。
- `NodeManagementEndpoint`、RuntimeApplyEndpoint discovery、last-seen、staleness 和 partition 原因。
- 当前 Zenoh 连接的只读关联，不复制 Fabric 内部对象。

NodeDaemon 产生 NodeStatus，DeploymentController 消费状态，InspectionService 投影状态，OpsService/TUI 消费投影。NodeStatus 不反向修改 NodeSpec；NodeDaemon 失联只使 Node facts 变为 stale/partitioned，不能据此断言 RuntimeHost 已死亡或把整个 Node 报为应用 Ready。

`NodeManagementEndpoint` 与 `RuntimeApplyEndpoint` 必须分开：前者由 NodeDaemon 拥有，只处理 presence/status、FeatureReport、Runtime endpoint discovery，以及有明确授权和 Receipt 的窄 node-maintenance 操作；后者由 RuntimeHost 拥有，独占 RuntimeApplyRequest 的 target/revision/proof/CAS/fencing 校验和 apply Receipt。即使底层 transport 需要代理，NodeDaemon 也只能透明承载，不能接收、改写、准入或拒绝 DeploymentPlan/RuntimePlanSlice。

## 4. Epoch 与 revision 必须分离

| 名称 | 变化时机 | 作用域 | owner |
| --- | --- | --- | --- |
| `NodeIncarnation` | NodeDaemon bootstrap 或重新取得当前 registration tenure | 一个 Node 当前 Node facts 发布者代次 | NodeDaemon |
| `RuntimeHostEpoch` | RuntimeHost 每次重新启动 | 一个 RuntimeHost 运行实例 | RuntimeHost |
| `DomainEpoch` | ExecutionDomain 每次创建或重建 | 一个本地执行/故障边界 | RuntimeHost / ExecutionDomain owner |
| `FabricSessionEpoch` | Zenoh Session 重新创建 | 一个 Fabric Session | FabricService |
| `BindingEpoch` | 逻辑 `PortBinding` install/reinstall/reconfigure/revoke | 一个稳定 `BindingId` 标识的 live binding | RuntimeHost/Fabric binding owner |
| `DeploymentRevision` | 期望部署发生变更 | 一份 desired state | DeploymentController |
| `LeaseIssuerEpoch` | 本地 ResourceCoordinator/issuer 重启 | 一个受控资源的 lease 签发代次 | ResourceCoordinator；Driver/actuator 另拥有最终 enforcement/completion |

这些值不能互相代替，也不能从时间戳或字符串前缀推导。NodeDaemon 每次 bootstrap 或重新取得 registration tenure 都生成新 NodeIncarnation；RuntimeHost 重启、Feature 刷新、普通 heartbeat gap、Zenoh disconnect 或同一 session auto-reconnect 均不改变它。所有 NodeDaemon facts 携带 `NodeId + NodeIncarnation + sequence/freshness`，旧 incarnation 的事实与回复不得覆盖当前；同一 NodeId 的双 NodeDaemon 由 enrollment/current-registration record 或本地独占锁 fencing，不能靠随机 ID 猜 current owner。Resource Lease 的 fencing token 还具有独立的资源作用域，不属于上述任何 epoch；owner 重启会使旧 LeaseIssuerEpoch 的 lease 全部失效，但不会允许 fencing token 计数回退。

晚到消息必须按它实际依赖的身份检查。ThreadDomain/ProcessDomain 重建后由 `DomainEpoch + InvocationId` 拒绝旧结果，不能只依赖 RuntimeHostEpoch。短暂断线及同一 Zenoh Session 对象的自动重连只更新带 freshness 的连接观测；只有 Session 对象被重建时才改变 `FabricSessionEpoch`。DeckCompiler 的纯 compile 只产生内嵌 canonical DeckTopology 的 DeckLock，DeploymentPlanner 的纯 compile 产生 `DeploymentPlanCandidate.plan_content.bindings`；二者都不安装 live binding，不递增 `BindingEpoch`。`BindingEpoch` 只在逻辑 `PortBinding` install/reinstall/reconfigure/revoke 时改变。Session 重建本身不隐式推进所有 BindingEpoch；若恢复过程实际 reinstall 某个 live PortBinding，则该独立事件必须推进该 BindingId 的 BindingEpoch。首版每条解析后的静态 1:1 Link 对应一个稳定 `BindingId`；`BindingEpoch` 只在同一 `BindingId` 内比较，不是全局身份，两个 binding 的 epoch 数值可以相同。只有 RuntimeHost 真正重启时才改变 `RuntimeHostEpoch`。

同一 DeploymentRevision 与活动 BindingEpoch 内，一个 BindingId 恰有一条接收新 Message/frame 的 active route。route replacement 只能按 revision-tagged `prepare → activate → drain → retire` 完成或显式 rollback；`activate` 原子切换新准入，旧 route 立即变为 drain-only。不能同时启用 local 与 wire，也不能依赖 payload hash/time-window echo 去重。fan-out 必须为每个 destination 创建独立 BindingId。

## 5. RuntimeHost 只拥有本地执行

RuntimeHost 负责目标 Node 内的：

- 主事件循环、本地控制通道、PortBinding install 与 Mailbox handoff。
- Loop、Thread 和 Process ExecutionDomain。
- Mailbox、Task、线程、本地子进程和关闭流程。
- 本地 CardInstance/CoreService 的 readiness、故障与 Receipt。
- 通过 `runtime/contracts` 拥有的 apply protocol 应用 DeploymentController 产生的 canonical `RuntimePlanSlice` value，校验 runtime-owned `SourceScopeRef`/`PlanWriterRef`/`PlanWriterEpoch`/target/`SourcePlanRevision`/expected-active/source+slice digests/operation id/deadline/WriterTenureProof/auth 后安装 live `PortBinding`，并报告 observed `BindingId`/endpoint/active route/session/binding epoch、Domain、PID/TID、loop 与 capacity；与同一 source plan revision 不一致时不得 Ready。RuntimeHost 不 import deployment/decks。

RuntimeHost 不拥有：

- 远端 Node 或远端 RuntimeHost 的生命周期。
- 由 systemd、容器平台或外部控制器托管的进程。
- 整个 Node registry、Deployment desired state 或 Zenoh topology。
- 任意领域 Service Locator。

跨 Node Placement 由 DeploymentPlanner 计算 DeploymentPlanCandidate，DeploymentController 原子提交 revision/committed plan，并由 Slice projector + apply builder 生成带 runtime-owned source-scope/source-revision/writer-ref/writer-epoch/target/expected-active/source+slice digests/operation id/deadline/WriterTenureProof/auth 的 RuntimeApplyRequest，直接发送到目标 RuntimeHost 拥有的 RuntimeApplyEndpoint；目标 RuntimeHost 才是 apply admission 与本地实例 owner。NodeDaemon 只提供 Node facts 和 endpoint discovery，不是第二个 apply gate。调用侧只拥有 typed service client/permission-bound access handle，不产生第二种 live Binding 类型；外部工作负载只通过受控 adapter 被观察或请求操作。因此不建立 `RemoteDomain`。

RuntimeHost 只拥有自己内部 Domain/child 的 lifecycle、RuntimeOwnershipTree 与 RecoveryAction，不能用同一 event loop 内的 heartbeat 证明自身存活。生产 profile 中 NodeDaemon 位于不同故障域，持续观测 RuntimeHost bootstrap/heartbeat/control responsiveness 并产生 LivenessState；OS service manager 拥有 RuntimeHost 整体进程并执行 TERM/KILL/restart。每个 profile 必须指定 NodeDaemon 或 OS service manager 中唯一的 host restart-budget/quarantine ledger 与 mutation owner，禁止双重重启循环。共进程只允许 development/constrained profile，且不能通过生产恢复 Harness。

当前 S6 注记：已实现的 PXHW v1/service-manager adapter 是 OS-level RuntimeHost lifecycle reference owner，不是 NodeDaemon，不产生 NodeStatus、NodeIncarnation 或节点级 LivenessState，也不进入 DeploymentController→RuntimeApplyEndpoint 权威链。RuntimeHost binary 仍无 public apply endpoint、assembly 或 durable epoch/journal；该 adapter 未裁决未来 NodeDaemon 的部署形态，也不能替代它。

## 6. Site、Zenoh 与语义空间

当前阶段不建立 Site 模型，只允许非权威 `site_hint`。它可以帮助开发 DeploymentProfile、日志展示和候选 Placement，但不得用于：

- 生成 CapabilityGrant 或信任结论。
- 证明数据驻留或法规合规。
- 推导 FailureDomain、SafetyDomain 或网络连通性。
- 作为资源控制和生命周期 owner。

当数据驻留、站点自治或多 Site OPS 出现可验收的真实消费者后，再研究稳定 `SiteRef`、SiteSpec 和 owner。在此之前不得让自由字符串逐步变成隐式主键。

Zenoh 网络结构使用两个不同形态：

- `ZenohTopologyProfile` 是期望配置，可声明 router/client、连接、gateway 和 Zenoh Region 设置。
- Zenoh region observation 是 FabricService 的私有 self-inspection shape，必须带 freshness 和来源；当前不冻结 `ZenohRegionObservation` 公共类型。

未来语义导航使用 `SemanticRegionRef`。它必须由 `SpatialMapRef + MapEpoch` 定位，表示房间、走廊、工作区等空间认知实体，不表示 Zenoh 路由范围。

```text
site_hint ──optional hint──> Deployment constraints

ZenohTopologyProfile ──apply──> Fabric self-inspection (private region observation)

SpatialMapRef @ MapEpoch ──contains──> SemanticRegionRef
```

三者可以由显式策略关联，但不共享 ID。若未来需要根据空间位置选择网络路径，应由 RoutingPolicy 显式映射 `SemanticRegionRef` 到 Zenoh 配置，不能按名称自动关联。

## 7. 所有关系图必须使用限定名

- `DeckTopology`：DeckSpec 的连接意图经解析后形成、内嵌于 DeckLock 并受其 digest 覆盖的 canonical workload topology。
- `ServiceDependencyGraph`：平台服务依赖与 readiness 顺序。
- `DeploymentPlan`：工作负载到 Node/RuntimeHost 的 Placement 决策；其 `bindings` 子树是 `BindingId`、endpoint、Zenoh route/locality、Schema/codec 与 admission boundary 的 desired truth，`execution` 子树是 Domain、Mailbox、dispatch、budget、liveness、failure-containment 和 recovery 的 desired truth。不另建并行 `BindingPlan`。
- `ZenohTopologyProfile`：Fabric 的期望网络配置。
- `NavigationGraph`：未来地图中的可达和路径关系。

不得创建同时管理上述关系的 `TopologyController` 或 `TopologyService`。共享图算法只有出现至少两个真实消费者后，才可提取为无领域 owner 的纯基础库。

## 8. 物理 Scope 保持正交

```text
CapabilityScope ─selects──> ControlledResourceRef + Operation
SafetyDomain     ─groups───> ControlledResourceRef
FrameGraphRef   ──relates──> frames / sensors / actuators
```

`ResourceGroup` 当前不作为正式基础类型；普通标签或查询结果足以满足展示需求。若以后引入，它也不能自动成为 CapabilityScope、SafetyDomain、FrameGraph 或生命周期边界。

Authority 回答“主体是否有权做”，Resource lease/fencing 回答“当前是否拥有”，Safety 回答“物理状态是否允许”。三类决定可以同进程部署，但必须有不同的逻辑 owner 和 Receipt。

## 9. Robot 与 Embodiment 红线

`Robot` 和 `Embodiment` 都不进入 Kernel、RuntimeHost、Deployment 或 Fabric 的基础模型。产品可以使用 robot、vehicle、arm、drone 等展示标签，但这些词不能成为 Node 上级、权限来源、资源 owner 或生命周期聚合根。

当前不定义 `RobotView`、`EmbodimentSpec` 或相关 Schema。未来 Grounding 出现明确生产者、消费者和验证场景时，应优先研究只关联引用、不会拥有底层对象的短生命周期 Binding，而不是重新建立万能物理主体对象。

## 10. 第一阶段不变量

1. 公共 API 中 `Node` 只有本文定义，不并存 `ComputeNode`。
2. NodeIdentity、NodeSpec 和 NodeStatus 不合并成可变大对象。
3. InspectionService 只投影状态，不拥有 Node registry 或 heartbeat。
4. RuntimeHost 不启动、停止或宣称拥有远端执行实例。
5. 除术语禁令和反例说明外，文档、Schema 和测试不使用无领域限定的 `Region` 或 `Topology`。
6. site_hint 不参与授权、信任、驻留证明或资源 fencing。
7. Fabric 私有 region observation 与 SemanticRegionRef 不共享 ID 或生命周期。
8. Robot/Embodiment 不成为 Kernel、Runtime、Deployment 或 Fabric owner。
9. Resource、Authority、Safety 与 Frame 关系不互相冒充。
10. CardDefinition `In`/`Out`/`PortSpec`、Deck `Link`/`DeliveryProfile`、`DeploymentPlan.bindings` 和 live `PortBinding` 分属声明、工作负载意图、desired plan 与 observed runtime owner；后一层不得回写前一层。
11. 首版每条解析后的静态 1:1 Link 只生成一个稳定 `BindingId`；`BindingEpoch` 只在该 ID 内比较，纯 compile 不递增它。
12. `DeploymentPlan.execution` 与 Runtime observed execution facts 分属 desired/observed owner，不得由 CardDefinition、Card、Deck 或 Inspection 回写。
13. 同一 DeploymentRevision 与活动 BindingEpoch 内，每个 BindingId 只有一条 active route；切换必须完成或 rollback，禁止双投、implicit fallback 和基于内容的 echo 去重。
14. CapabilityGrant、ServiceContract 与 Node/Fabric/Device FeatureReport 分离；Node feature 不产生权限，Grant 不证明目标支持或 Ready。
15. RuntimePlanSlice Schema/apply protocol 属于 Runtime，具体 canonical projection value 属于 DeploymentController；它带 source/slice digest 与 authenticated CAS request，不成为第二份 desired truth。
16. DeploymentPlan/Revision/Planner/DeploymentController 全部位于顶层 deployment control plane，不进入 Kernel；每个 DeploymentScope 只有一个有效写 owner，CLI/OpsService/DeckCompiler 不得并列直写 RuntimeHost。
17. DeploymentScope、Deck、Product/Application 和 Installation 不互为身份别名；正式 Application 需在多 Deck、稳定安装或应用私有持久状态出现后另行决策。

## 11. 后续决策入口

以下问题不阻塞 Kernel P0–P2，但在对应功能实现前需要 Research 或 Proposed ADR：

- NodeDaemon 与首个 RuntimeHost 是否在 development/constrained profile 共进程部署；production 必须保持不同故障域。
- 一个 Node 是否允许多个 RuntimeHost，以及它们的权限隔离方式。
- SiteRef 的首批真实消费者、注册 owner 与迁移路径。
- Product Application/Installation 的首批真实消费者；只有多 Deck release、稳定安装 identity 或跨 DeckRun 私有状态出现后才引入，且不能复用 DeploymentScope。
- SpatialMapRef、MapEpoch、SemanticRegionRef 与 FrameGraphRef 的身份规则。
- SafetyDomain 的本地权威来源和硬件联锁边界。
- typed service client/permission-bound handle 的断连、staleness、Grant 撤销与 FeatureLoss 语义。

## 12. 相关文档

- [ParaEGOX 分布式系统模型](../architecture/distributed-system-model.md)
- [Kernel、RuntimeHost 与 Core Services 架构基线](../architecture/kernel-runtime-core-services.md)
- [CardDefinition、Card 与 Deck](card-definition-card-deck.md)
- [Capability、Service Contract 与 Feature Support](capability-service-feature-boundaries.md)
- [分布式具身 Agent OS 缺口研究](../research/distributed-embodied-agent-os-gap-analysis.md)
- [CardDefinition 输入输出、Port、Link 与运行绑定研究](../research/card-definition-ports-links-and-bindings.md)
- [Runtime 执行模型、调度与恢复研究](../research/execution-model-scheduling-and-recovery.md)
- [Kernel Foundation 实施计划](../plans/kernel-foundation.md)
