# 分布式身份、作用域与所有权研究

> 状态：Research Complete，结论为 `revise`
> 日期：2026-07-29
> 深度：Deep
> 评审策略：Dual independent review
> 范围：Node 身份与分代、部署与远端 Runtime 所有权、CardDefinition/Card Link 到 DeploymentPlan.bindings/PortBinding 的分层、Site/Zenoh topology/语义导航空间、Authority/Resource/Safety/Enforcement、Fabric 与网络分区
> 实现状态：尚未实现；本文是 Proposed ADR 与最小验证的输入，不是任何分布式能力已经完成的证明

> 术语更新（2026-07-28）：本文涉及的授权、服务调用与传输支持事实分别按新基线使用 `CapabilityGrant`、typed service client/permission-bound handle 与限定 FeatureReport。DeploymentPlan 仍是唯一 desired truth，但 RuntimeHost 只通过 `RuntimeApplyRequest/RuntimePlanSlice` Schema 消费 DeploymentController 产生的目标投影；规范边界见 [Capability、Service Contract 与 Feature Support](../concepts/capability-service-feature-boundaries.md)和[分布式具身 Agent OS 缺口研究](distributed-embodied-agent-os-gap-analysis.md)。

> 后续裁决（2026-07-28）：[ADR-0001](../adr/ADR-0001-deployment-controller-boundary.md) 明确 DeploymentPlan/Revision/DeploymentPlanner/DeploymentController 全部位于 Kernel 外；DeploymentPlanner 只产生 DeploymentPlanCandidate，DeploymentController 原子提交 committed plan/revision，RuntimeSliceProjector 产生 tenure-neutral Slice，writer tenure 由独立 authority proof 与 Runtime fencing 处理。本文中较早的“Compiler/Planner 写入 DeploymentPlan”按 candidate→commit 链解释。HA 与多 DeploymentController 共识后置。

> 后续裁决（2026-07-29）：[ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 已接受“Rust-first mechanisms，polyglot workloads”。Rust 是首个 production mechanism reference 的实现选择，不改变 Node、RuntimeHost、DeploymentController、Fabric、Authority、Resource 或 Safety 的身份和 owner；跨 Node/进程边界只依赖语言中立 Schema、canonical encoding、version、digest 与 epoch/fencing 语义。

## 一句话结论

ParaEGOX 应保留短名 `Node`，但将其严格定义为可部署、可管理的计算目标，并把稳定身份、NodeDaemon publication tenure、RuntimeHost、Fabric session 与 binding 分代分开。CardDefinition `In`/`Out` 只生成 transport-neutral `PortSpec`，Deck 的 `Card.Out → Card.In` `Link` 只表达工作负载交付意图；DeckCompiler 产生内嵌 canonical DeckTopology 的 DeckLock，DeploymentPlanner 将 desired binding 写入 DeploymentPlanCandidate，DeploymentController 原子提交后才形成权威 `DeploymentPlan.bindings`。RuntimeHost/Fabric 安装 Slice 后才产生 live `PortBinding`；一个 BindingId 在同一 DeploymentRevision 与活动 BindingEpoch 下只允许一条 Zenoh production route。当前只有非权威 `site_hint`，稳定 `SiteRef` 后置；Zenoh 1.9 的 region 是 Fabric adapter 的拓扑配置与观测事实；语义导航的 `SemanticRegionRef` 是带 `SpatialMapRef` 与 `MapEpoch` 的空间实体。三者可以显式映射，不能合并身份、权限或生命周期。

物理写路径必须是 `Authority → Resource → Safety → Enforcement → Receipt`：CapabilityGrant 只允许请求，具有单调 fencing token 的短期 Lease 才授予当前控制权，Safety 可独立抑制，Enforcement 必须在副作用所在的本地故障域拒绝旧 token、过期 lease 和不安全状态。Zenoh 提供唯一生产 Fabric/数据面，但不充当身份、授权、租约真相或远端 Runtime 的生命周期 owner。

## 1. 研究问题与成功标准

本研究回答：

1. `Node`、NodeDaemon、RuntimeHost、DeploymentTarget 和 Zenoh peer 分别是什么，哪些 ID 在重启与重连后应变化。
2. 是否应合并 `Site`、Zenoh region 与语义导航的 region；若不合并，如何避免产品、Fabric 与空间地图产生同名歧义。
3. 远端 Runtime、DeploymentController、FabricService 和实际执行者各自拥有何种生命周期与故障语义。
4. 网络分区、Authority 缓存、Resource Lease、fencing、Safety 与 physical command 的最小正确链路是什么。
5. Zenoh 1.9 topology、keyspace、liveliness、query 和连接恢复怎样服务 Fabric，而不成为 ParaEGOX Kernel 或全局控制平面。
6. CardDefinition `In`/`Out`、Deck 的 `Card.Out → Card.In` `Link`、`DeploymentPlan.bindings`、live `PortBinding` 和 `BindingEpoch` 如何保持独立 owner。

成功标准不是建立 Node/Site/网络区域/Fleet 大目录，而是能用以下规则实施和验证：

- 单进程、单 Node、多进程和跨 Node placement 使用相同的 ID、deadline、epoch 与 Receipt 契约。
- 任一次 Node 重启、RuntimeHost 重启、Fabric 重连、纯 binding compile 或 PortBinding install/reinstall/reconfigure/revoke 均不会被错误地当成另一种事件。
- `SiteRef`、Zenoh topology metadata、`SemanticRegionRef` 可以同时出现，但不从名称、网络连通性或同一 Site 推导 Authority。
- 两个控制者竞争同一资源时，旧 owner 的迟到 Command 即使在网络恢复后也不能造成物理副作用。
- 远端 Fabric、控制器、OPS 或 Agent 不可用时，本地 Safety 的行为可预测；获准自治仅来自资源级显式 `partition_behavior` 值或小型配置对象，不能来自缓存的 default-allow。
- Zenoh callback 只完成传输边界工作；它不直接执行 CardInstance 私有实现对象的 callback/invocation、设备 SDK 或未验证的物理 Command。

## 2. 范围、假设与非目标

### 2.1 范围

- 当前 ParaEGOX 的 Kernel、RuntimeHost、CoreService、Deck/Deployment 和分布式系统设计输入。
- Zenoh 1.9 Regions、gateway、session、liveliness 与故障诊断资料。
- 经典 lease 的失效模型对物理 Resource owner 的适用边界。
- 未来 Semantic Navigation/SpatialMap 需要的命名保留，但不设计其算法或存储。

### 2.2 假设

- ParaEGOX 需要从离线单进程测试演进到设备—边缘—云、多 Site 的具身系统；分布式是契约目标，不是首版已具备的集群功能。
- Zenoh 是唯一生产 Fabric，覆盖 `session-local`、`host-local` 与 `remote` route locality，ROS2/DDS 等经 Gateway 接入；非 Zenoh 路径只作为确定性 `PortBinding test fixture`，不成为生产 Bus 或部署选项。
- 故障模型是 crash、重启、断链、延迟、重复、乱序、时钟漂移和部分分区，不假定 Byzantine 容错。
- 安全关键抑制链可在下层硬件或本地控制器实现，ParaEGOX 不替代 E-Stop、驱动固件或安全认证设备。

### 2.3 非目标

- 不冻结 Zenoh gateway 具体配置、key expression、QoS 参数、router 数量或跨 Site 共识实现。
- 不建立持久 `Site`、`SemanticRegionRef`、Robot、Embodiment、Fleet、Cluster 或通用 `TopologyManager`。
- 不选择 lease ledger 的数据库、硬件 watchdog 或时间同步产品。
- 不把分布式 Lease 误宣称为安全认证机制；Safety 的硬实时与认证边界需要独立工程论证。

## 3. 术语与命名裁决

### 3.1 保留与拒绝

| 名称 | 裁决 | 限定含义 |
| --- | --- | --- |
| `Node` | 保留短名 | 可独立被部署、管理、观察断连的计算目标；不是 Robot、Site 或 ROS node |
| `RuntimeHost` | 保留 | Node 内拥有 ExecutionDomain、进程、Mailbox 与关闭的运行 owner |
| `NodeDaemon` | 保留为基础设施进程角色 | OS-resident 的 Node presence/status、NodeFeatureReport、Runtime endpoint discovery 与窄 NodeManagementEndpoint owner；不应用 Deployment plan，也不是 Runtime apply gate |
| `site_hint` / `SiteRef` | 当前 hint；稳定 Ref 后置 | 非权威部署提示；未来稳定的物理/行政范围引用，不是网络、安全或故障域 |
| `ZenohTopologyProfile` | 保留为 Deployment/Fabric 配置概念 | Zenoh mode、gateway filters、`region_name`、连接与能力的版本化拓扑配置 |
| `SemanticRegionRef` | 为 SpatialMap 预留 | 某 `SpatialMapRef + MapEpoch` 中的空间/导航实体 |
| `ControlledResourceRef`、`LeaseGrant`、`FencingToken` | Kernel 候选契约 | 受控资源、短期控制权与旧 owner 拒绝规则 |
| `CapabilityScope` | 保留为值 | CapabilityGrant 可以请求的 resource selector + operations；不建独立 owner |
| `SafetyState`/`SafetyDomain` | 按需保留 | 本地抑制与共同安全状态；不自动等于资源组或权限域 |
| `ComputeNode` | 拒绝 | `Node` 已被本项目窄化；改名无额外信息，反而制造与计算图 node 的迁移噪声 |
| `FabricRegion` | 拒绝 | 会把 Zenoh 的 adapter 内部拓扑提升为稳定领域对象，制造错误的 owner 与 ID |
| 裸 `Region` | 拒绝 | 除引用和反例外，Schema、API、架构陈述与运行日志不得把它作为正式名；Fabric 使用私有 region observation，空间域使用 `SemanticRegionRef` |
| 裸 `Topology` | 拒绝 | 必须写 `DeckTopology`、`ServiceDependencyGraph`、`ZenohTopologyProfile` 或其他限定词；不能成为跨层万能图 |

Zenoh 官方的 Region/`region_name` 只允许用于 adapter 配置解释、adminspace/log 观测和测试描述。它不进入 Kernel 契约，也不是外部 API 要求的 ParaEGOX 实体。运行观测留在 Fabric 私有 self-inspection shape，不冻结公共 `ZenohRegionObservation`；若不需要讨论 Zenoh region tree，则改写为 `Fabric connectivity`、`ZenohTopologyProfile` 或具体 Node/link。

### 3.2 三种作用域不可合并

```text
Physical / administrative                 Spatial / semantic
site_hint / future SiteRef                 SpatialMapRef + MapEpoch
   │ optional deployment metadata             │ contains
   ▼                                           ▼
Node / installed resource                 SemanticRegionRef → Place / Opening

Fabric / transport
ZenohTopologyProfile → session / gateway / observed ZenohRegion
```

三者的 owner、更新节奏和失败语义不同：

| 术语 | 回答的问题 | 权威 owner | 典型变化 |
| --- | --- | --- | --- |
| future `SiteRef` | 设备属于哪个物理/行政地点？ | future Site inventory | 资产迁移、组织调整 |
| `ZenohTopologyProfile` | Zenoh 节点怎样形成连接与 gateway 层级？ | Deployment + Fabric adapter | 网络、容量、gateway 配置、重连 |
| `SemanticRegionRef` | 地图中当前位置是厨房、走廊还是充电区？ | SpatialMap/World | 建图、定位、地图 epoch 更新 |

一个 Site 可映射到多个 Zenoh topology profile；一个 cloud/backbone profile 可服务多个 Site；一个 Site 可保存多张真实或仿真地图。即使 UI 给三者使用同一显示名称，也只能由显式 mapping 关联，不能共享 ID，不能由字符串前缀、region name、坐标名称或 keyspace 自动生成权限。

## 4. 证据与强度

### 4.1 本地证据

| 证据 | 类型 | 强度 | 对结论的影响 |
| --- | --- | --- | --- |
| [分布式系统模型](../architecture/distributed-system-model.md) 将 Node/RuntimeHost、各层 epoch、deadline、Receipt、分区行为和 fencing 分开 | local | 高 | 为实现提供统一的身份、恢复与物理控制边界 |
| [Node 与作用域边界](../concepts/node-and-scope-boundaries.md) 将 Site、Zenoh topology、空间地图与物理控制分开 | local | 高 | 禁止裸作用域名，并为每张关系图指定 owner |
| [Kernel/Runtime 基线](../architecture/kernel-runtime-core-services.md) 将 RuntimeHost、Fabric、Authority、ResourceCoordinator、Safety 与 Enforcement 分层 | local | 高 | 本地执行、网络连接和物理写路径不能再次汇聚为大 Runtime |
| [Kernel Foundation 计划](../plans/kernel-foundation.md) 将本地物理闭环放在 Zenoh 和双 Node 前 | local | 高 | Resource owner、lease/fencing 和 simulated Safety 必须先在本地闭环验证 |
| 总体架构中的 Authority Service 正确性不依赖远端 Fabric | local | 高 | 本地 Authority/Resource/Safety/Enforcement 可独立闭合；typed service client/permission-bound handle 跨 Node 时才声明 Fabric 依赖，普通 PortBinding 的生产 route 统一由 Zenoh 承载 |
| 既有研究记录 EAGOS 的中心 Bus 同时拥有 Zenoh session、本地订阅、线程、重连、授权 hook 与诊断 | local | 高 | Fabric、Runtime、Authority 和 Inspection 必须避免重新汇聚为大对象 |

相邻 EAGOS/PhanthyMotus 的材料只用来识别工程失败模式，不被复制为 ParaEGOX 的实现、配置或 schema。

### 4.2 外部一手资料

| 来源 | 类型 | 强度 | 对结论的影响 |
| --- | --- | --- | --- |
| [Zenoh 1.9 Longwang](https://zenoh.io/blog/2026-04-16-zenoh-longwang/) | external | 高 | Region 是拓扑树元素；`region_name` 是可匹配的可选配置名，adminspace 的 `north`/`south:n:*` 是观测标识，不能当 ParaEGOX 领域 ID |
| [Zenoh Deployment](https://zenoh.io/docs/getting-started/deployment/) | external | 高 | gateway 通过 mode、zid、接口和 region name 等 filter 建立 north/south 关系；其职责属于部署/数据面而非物理或授权范围 |
| [Zenoh Abstractions](https://zenoh.io/docs/manual/abstractions/) | external | 高 | Zenoh 同时有 pub/sub、query/queryable、storage 与 key expression；Fabric 必须显式映射语义而非把它误当作单一队列 |
| [Zenoh Troubleshooting](https://zenoh.io/docs/getting-started/troubleshooting/) | external | 中高 | Zenoh 带时间戳数据会受跨主机 clock drift 影响；这是传输事实，不直接定义 ParaEGOX lease 语义 |
| [Zenoh Access Control](https://zenoh.io/docs/manual/access-control/) | external | 高 | ACL 是按消息、主体和 key expression 的传输过滤，且效果依赖拓扑；不能替代业务 Authority 或资源所有权 |
| [Gray & Cheriton, *Leases* PDF](https://web.stanford.edu/class/cs240/readings/leases.pdf) | external | 高 | lease 是带期限的权利合同，可在 crash/通信失败下过期收敛；其物理时钟假设和 cache 场景不能直接替代 fencing 或 Safety |

### 4.3 推断与开放证据

- **inference**：把 `NodeId`、RuntimeHost 进程 ID 和 Zenoh ZID 分开，能把“机器重启”“执行宿主重启”“传输重连”变成可独立测试的状态转换；这是现有抽象边界的直接推论。
- **inference**：针对非 Byzantine 模型，资源 owner 持久保存最大 fencing token 是阻止旧 controller 延迟命令的最小措施；lease expiry 本身不能比较两个仍在有效期内的竞争 owner。
- **inference**：结合 Zenoh 的 clock-drift 风险与 Gray–Cheriton 的物理时钟假设，ParaEGOX 不直接比较跨主机 monotonic timestamp；lease duration 必须由本地执行 owner 安装为可比较的 expiry。
- **inference**：资源级 `partition_behavior` 值或小型配置对象应属于 Resource/Safety 的声明式策略，而不是 Authority Service 不可用时的缓存开关；当前没有证据支持一个大型自治领域对象。
- **open**：目标 MCU/驱动是否能持久比较 fencing token，或必须由本地 actuator proxy 充当唯一执行 owner，需在首个硬件 profile 验证。
- **open**：跨 Site controller 高可用是否需要共识、共识保护哪些 desired state 和 lease issuer 状态，尚无产品规模与故障预算证据。
- **open**：各目标平台的 monotonic clock、PTP/NTP 偏差、睡眠/重启行为和可接受 lease term 尚未实测；不得照搬论文中的期限。

## 5. 当前问题重建

### 5.1 身份与远端所有权混淆的风险

没有明确分代时，以下事件容易被错误合并：

```text
hardware replacement      → Node identity migration
NodeDaemon bootstrap/registration tenure change → NodeIncarnation change
RuntimeHost restart        → RuntimeHostEpoch change
ExecutionDomain rebuild    → DomainEpoch change
Zenoh Session object recreate → FabricSessionEpoch change
transport disconnect/auto-reconnect → connection observation only
Deck/Deployment pure compile → desired DeploymentRevision only
PortBinding install/reinstall/reconfigure/revoke → BindingEpoch change
```

若把它们都写成一个 `epoch`，旧 Fabric callback 可能在新 RuntimeHost 中被接受；反之，短暂网络重连会使控制器错误重建整台 Node。若将 remote Runtime 视为本机对象引用，DeploymentController/OPS 又会绕过 RuntimeHost 的 apply admission、关闭与 observed-state 协议；NodeDaemon 不能被插入这条链成为第二个 admission owner。

### 5.2 裸 `Region` 同名产生错误映射

Zenoh 1.9 region 的边界取决于 gateway configuration，且 region tree 可任意加深；它不表达楼层、房间、地图 epoch、资产责任或控制许可。`region_name` 最多是用于 gateway filter 的短字符串。相反，语义导航 `SemanticRegionRef` 需要携带地图、几何/证据、父子关系和 map epoch。将它们合并会使网络调优改变地图身份，或把“厨房”错误地变成可路由、可授权的网络组。

### 5.3 lease 只有名称、没有拒绝旧 owner 的链

仅有 `Lease(expiry)` 仍会出现：控制者 A 在分区前获得 lease，控制者 B 在资源 owner 看来 lease 已过期并取得新 lease；网络恢复后 A 的旧 Command 迟到。Resource owner 若只检查“命令里有 lease”或由 Runtime 内存表判断 owner，就可能执行 A 的旧副作用。lease 需要与递增 fencing token、执行点持久状态、receipt 查询和安全抑制共同工作。

## 6. 方案比较与淘汰理由

### 6.1 方案 A：统一 `Topology`/`Region`/`Robot` 聚合树

```text
Region or Robot
├── Site
├── Nodes / RuntimeHosts
├── Zenoh peers
├── semantic map regions
├── resources / safety
└── authority
```

**优点**：UI 和初期配置看起来简单。
**淘汰理由**：它把物理归属、网络拓扑、地图语义、权限、故障域和生命周期合并为一个 owner。一个 Node 可服务多个物理单元，一个 Site 可有多张地图和多个 topology，一个资源组又可跨 Node。该树会变成新的 Service Locator，无法正确表达多对多关系与独立变更。

### 6.2 方案 B：以 Zenoh Region 作为统一分区/权限范围

**优点**：可复用 Zenoh gateway 和 keyspace 配置，减少看似重复的分组字段。
**淘汰理由**：Zenoh region 仅描述 gateway 形成的网络拓扑；其 region name 可被 filter 匹配，adminspace region identifier 也会随配置改变。官方说明 ACL 的效果还强依赖网络拓扑，故它不能成为业务授权证明。[Zenoh 1.9](https://zenoh.io/blog/2026-04-16-zenoh-longwang/), [Zenoh ACL](https://zenoh.io/docs/manual/access-control/)

### 6.3 方案 C：每个 Node 自治，依靠时间戳/最后 owner 处理竞争

**优点**：首版无需显式 lease issuer 或持久状态。
**淘汰理由**：网络延迟、重启与时钟漂移会让最后可见消息失真；单纯 TTL 不能拒绝旧 owner 的迟到 command。Zenoh 本身也提示跨主机时间偏差会导致数据时间戳被拒绝。该方案既不能提供排他控制，也不能解释 `uncertain` 结果。

### 6.4 方案 D：分离作用域、分代与执行链（推荐）

**做法**：保留 `Node` 短名；site_hint/future SiteRef、Zenoh topology 与 SemanticRegionRef 使用不同、限定的引用；以 NodeDaemon/RuntimeHost/Resource owner 分层；Authority、Lease、Safety、Enforcement 分离；Zenoh 仅作为 Fabric；每个控制资源都在本地执行点验证单调 fencing token。

**代价**：首版须定义更多小契约、Resource owner 的 durable state 和分区 profile，UI 需要做投影而不能依赖一棵对象树。
**推荐原因**：这是唯一同时满足 local-first、远端 placement、物理安全、语义导航演进和 Zenoh 1.9 真实语义的方案。

## 7. 推荐模型

### 7.1 身份与分代

| 值对象 | 创建者 | 何时改变 | 不可替代的用途 |
| --- | --- | --- | --- |
| `NodeId` | 资产/Bootstrap 注册 | 受控替换或身份迁移；不可因重启变化 | Deployment target、审计和稳定归属 |
| `NodeIncarnation` | NodeDaemon | NodeDaemon bootstrap 或重新取得 current registration tenure | 证明本次 Node facts 发布者代次 |
| `RuntimeHostId` | Bootstrap/Enrollment 配置 | 创建或永久替换 Host 时 | 运行宿主的稳定逻辑身份；Deployment 消费、NodeDaemon 发现/报告，不临时生成 |
| `RuntimeHostEpoch` | RuntimeHost | Host 每次启动 | 隔离旧进程、task、Receipt callback |
| `DomainInstanceId` / `DomainEpoch` | RuntimeHost | Domain 创建/重建 | 隔离旧线程、进程与 IPC 代次 |
| `InvocationId` | ExecutionDomain | 每次工作受理 | 拒绝同一 DomainEpoch 中已超时/取消调用的迟到结果 |
| `FabricSessionEpoch` | FabricService | Zenoh session 建立/重建 | 隔离旧 ingress/egress callback；不是 Node restart |
| `BindingEpoch` | RuntimeHost/Fabric binding owner | 逻辑 `PortBinding` install/reinstall/reconfigure/revoke 时；纯 compile 不变 | 仅在同一 `BindingId` 内拒绝旧 PortBinding 的晚到数据 |
| `DeploymentRevision` | DeploymentController | desired plan 修改 | 将 observed state 与一个明确计划对比 |
| `LeaseIssuerEpoch` | local resource owner/issuer | issuer 或执行 owner 每次重启 | 使重启前签发的 lease 全部失效 |
| `FencingToken` | Resource owner | 每次成功授予排他 lease | 拒绝此前 owner 的写入 |

约束：`NodeId` 不复用；NodeDaemon 与 RuntimeHost 只能报告自己的 epoch。NodeDaemon 每次 bootstrap/重新取得 registration tenure 生成新 NodeIncarnation；RuntimeHost restart、Feature refresh、heartbeat gap、Zenoh disconnect 或同 session reconnect 均不改变它。所有 Node facts 带 NodeId、NodeIncarnation、sequence/freshness，旧 incarnation 不得覆盖当前；双 NodeDaemon 由 enrollment/current-registration record 或本地独占锁 fencing。跨层消息至少携带足以被接收方验证的 owner ID、epoch、deadline 与 causality。Zenoh ZID/peer identity 是 Fabric session 的输入，不等同于 `NodeId` 或 `PrincipalRef`。DeckCompiler 的纯 compile 只产生内嵌 canonical DeckTopology 的 DeckLock，DeploymentPlanner 的纯 compile 只产生 DeploymentPlanCandidate；二者都不安装 live binding，因此不递增 `BindingEpoch`。只有 DeploymentController commit 才推进 DeploymentRevision 并形成 desired `DeploymentPlan.bindings`。首版每条解析后的静态 1:1 Link 对应一个稳定 `BindingId`；`BindingEpoch` 只在同一 `BindingId` 内比较，两个 binding 的 epoch 数值可以相同。

同一 DeploymentRevision 与活动 BindingEpoch 内，一个 BindingId 恰有一条接收新 `Message`/frame 的 active route。route replacement 必须带 revision，按 `prepare → activate → drain → retire` 推进，失败时显式 rollback；`activate` 原子切换新准入、旧 route 只 drain。禁止 local+wire 双投、隐式 fallback，以及用 payload hash 或时间窗掩盖 echo。fan-out 为每个 destination 创建独立 BindingId，不共享 active route。

### 7.2 远端 Runtime 与 Deployment 所有权

```text
DeploymentController (desired state, DeploymentRevision)
                   │ authenticated RuntimeApplyRequest
                   ▼
RuntimeHost / RuntimeApplyEndpoint
  (apply admission, RuntimeHostEpoch, DomainEpoch, processes, Mailboxes, PortBindings)
                   │ starts / drains / terminates
                   ▼
CoreService / CardInstance

NodeDaemon ── NodeIncarnation / NodeStatus / Runtime endpoint discovery ──► DeploymentController
```

- DeploymentController 不能持有远端 RuntimeHost 对象或直接 kill 远端进程；它只提交带 revision、deadline、principal 和 receipt contract 的操作。
- NodeDaemon 拥有 NodeManagementEndpoint，只报告 Node presence/status、FeatureReport 和 Runtime endpoint discovery；它不接收、改写或拒绝 DeploymentPlan/RuntimePlanSlice，也不把转述的 Runtime facts 升级成自己的权威事实。即使 transport 需要 proxy，也只能透明承载。
- RuntimeHost 拥有 RuntimeApplyEndpoint，独占 target/revision/proof/CAS/fencing 的 apply admission、RuntimeApplyRequest Receipt、本地 ExecutionDomain、Mailbox、子进程与关闭；它不 import DeploymentPlan，而是应用 runtime-owned `RuntimePlanSlice` 中的 execution assignment，并报告 observed Domain、PID/TID、loop、capacity 与 epoch；与同一 source revision 不一致时不得 Ready。它不拥有 Site、Zenoh topology、Authority policy 或全局 Deck registry。
- `remote` placement 表示通过协议交互，不表示 DeploymentController 成为远端 Service 的生命周期 owner。
- 一份 DeploymentPlan 所属作用域的破坏性 reconciliation 首版必须只有一个有效 controller owner/epoch。多 controller 或 HA 之前先拒绝并发 writer；不能靠“最后到达的计划”解决。
- RuntimeHost 内的 heartbeat/lag task 不能证明 RuntimeHost 自身没有卡死。生产 profile 中不同故障域的 NodeDaemon 持续观测 bootstrap、heartbeat 与 control responsiveness 并发布 LivenessState；OS service manager 拥有 RuntimeHost 整体进程并执行 TERM/KILL/restart。每个 profile 必须指定唯一的 restart-budget/quarantine ledger 与 mutation owner，禁止 NodeDaemon 和 service manager 双重重启；不能只在启动期检查。

### 7.3 Authority → Resource → Safety → Enforcement

```text
Principal assertion / transport authentication
                    │
                    ▼
Authority Decision ──> CapabilityGrant (request permission)
                    │
                    ▼
Resource owner ──────> LeaseGrant + monotonically increasing FencingToken
                    │
                    ├──> Safety state / partition behavior may inhibit
                    ▼
Local Enforcement Point / actuator proxy
                    │ validates CapabilityGrant, lease, token, expiry, safety,
                    │ idempotency and constraints before effect
                    ▼
Physical effect ─────> staged Receipt / durable handoff / status query
```

最小对象应表达：

```text
CapabilityGrant
├── principal_ref, scope(resource selector + operations)
├── issued_by, policy_version, expires_at
└── attenuation/delegation proof

LeaseGrant
├── resource_ref, holder_ref, lease_id
├── fencing_token, lease_issuer_epoch
├── local_expiry_basis + expiry
└── partition_behavior

Command
├── command_id, idempotency_key, resource_ref, requested operation
├── capability_ref, lease_id, fencing_token, deadline
└── causality/trace references
```

规则如下：

1. CapabilityGrant 允许主体提出请求，不能授予独占资源控制；Authority 缓存必须有期限、scope、policy version 和撤销行为。
2. 对物理写资源，Resource owner 或其同一故障域单写代理是 lease 的唯一 issuer；远端 Authority 只能授权请求。每次改变排他 holder 必须分配严格递增的 fencing token。若未来引入 HA issuer，token 的单调性与单写语义必须由该 ADR 明确保证。
3. Enforcement 在副作用所在的本地故障域保存每个 resource 已接受的最大 token，并拒绝较低 token；在 MCU 无法持久检查时，本地 actuator proxy 必须是唯一可写 owner，不能把检查留在远端 Runtime 内存。
4. expiry 用执行点可比较的本地单调时间语义，不能直接比较两个机器的 monotonic 值；wall/HLC 只用于跨节点展示、关联和审计。远端只请求有界 duration，由 owner 换算为本地 expiry 并写入 Grant/Receipt。若以后拆分 issuer 到不同故障域，必须另行定义时钟不确定性、安装确认和故障恢复协议。
5. issuer/执行 owner 重启时必须生成新的 `LeaseIssuerEpoch`，使旧 epoch 的全部 lease 无条件失效；恢复持久 fencing/idempotency 状态前 fail-closed。首版不跨重启恢复旧 lease，因此不需要把已经失去基准的 monotonic expiry“换算回来”。
6. Safety inhibition 优先于有效 lease。E-Stop 不依赖 Authority、Fabric、Agent、OPS 或普通队列；解除抑制是独立、受审计的安全动作。
7. `partition_behavior` 是资源级显式输入，例如 `stop_on_lease_expiry`、`hold_last_safe_setpoint_until`；Deployment 将其与固定的 revision/digest、时钟/能源/Evidence 门槛和预先衰减的 offline Grant 编译为 `ContinuityProfile`。控制面失联时不能默认继续，断网后的本地身份不能自授新 lease；由 ContinuityController 执行 Profile，但 Authority/Resource/Safety/Device/Evidence 各自保留事实所有权。
8. `accepted` 仅表示准入；执行超时或断连必须产生 `uncertain`，调用者通过权威状态查询或后续 Receipt 恢复，Fabric 不自动重放副作用。

Gray 与 Cheriton 的 lease 说明“有限期限权利”能让 crash/通信失败通过到期收敛，但其论文依赖物理时钟，并讨论 cache consistency。ParaEGOX 采用该失效收敛思想，不把它误用为 physical effect 的 exactly-once、fencing 或硬安全证明。[原论文 PDF](https://web.stanford.edu/class/cs240/readings/leases.pdf)

### 7.4 Fabric 边界

CardDefinition `In`/`Out` 只生成 transport-neutral `PortSpec`，Deck 的 `Card.Out → Card.In` `Link`/`DeliveryProfile` 拥有一次工作负载连接的交付意图。DeckCompiler 只产生内嵌 canonical DeckTopology 的 DeckLock；DeploymentPlanner 结合 target facts 和 DeploymentProfile 将 desired binding 写入 DeploymentPlanCandidate，DeploymentController commit 后才形成 `DeploymentPlan.bindings`，不另建 `BindingPlan`。FabricService 根据 RuntimePlanSlice 中的对应 assignment 拥有 Zenoh session、序列化/解码、keyspace version、连接与 matching/liveliness 状态、ingress validation，以及 `session-local`、`host-local`、`remote` 三种 locality 的 production `PortBinding` route。它可以把语义 DeliveryProfile 映射为 Zenoh 的可靠性、优先级、query、storage 或 QUIC 能力，但不向 Kernel 泄露 Zenoh 类型或参数。

```text
producer PortBinding
    │ Zenoh callback
    ▼
Fabric callback: fixed-cost key/header/size/version/cache/BindingEpoch checks
    → nonblocking try_offer encoded frame/reference
    ▼
bounded Fabric ingress buffer (pre-validation; not a Mailbox)
    → ingress worker
    → decode/decompress/full schema, principal and binding admission
    ▼
validated Message → bounded target Mailbox (only semantic payload queue)
    ▼
ExecutionDomain → CardInstance private implementation invocation / Enforcement
```

FabricService 不拥有：

- `NodeId` 的资产注册或 NodeDaemon lifecycle。
- Authority policy、lease ledger、Safety state、physical driver 和 resource ownership。
- Evidence 的权威 retention/commit；Zenoh storage 只能是一个 transport/storage adapter，不可把“远端可见”误报为本地 durable handoff。
- 业务 SemanticRegionRef、SiteRef 或 `DeckTopology`。

callback 不做完整 payload decode、解压、复杂 schema/authentication，也不调用 CardInstance 的私有实现对象；它只把 encoded frame/reference 交给 items/bytes/age 有界的 Fabric ingress buffer。该 buffer 不是 Mailbox，也不能产生应用 accepted；它和 Zenoh channel 的 retained bytes、overflow 与 age 必须可观测。Ingress worker 验证成功后才构造不可变 Message 并准入唯一 target Mailbox，失败产生 ingress rejection，不额外创建隐藏 backlog。Zenoh liveliness/matching 表示传输可观察性，不等于应用 ready、controller lease 有效或物理安全。短暂 disconnect/同一 Session 对象 auto-reconnect 只更新 stale/partitioned 等连接观测；只有 Session 对象重建才更新 `FabricSessionEpoch`。它不自动撤销 Node identity，也不透明重试 Command。确定性测试可用 `PortBinding test fixture` 直接把已验证 Message offer 到同一 Mailbox 契约，但该 fixture 不进入生产 DeploymentPlan，也不命名为 `MemoryPortBinding` 或另一种 Bus。

## 8. 分期与验证

| 阶段 | 首个交付 | 必须验证 | 明确不做 |
| --- | --- | --- | --- |
| P0 | 术语、边界与 Proposed ADR | 公共 Schema/API/运行日志禁止裸 `Region`、裸 `Topology`、`ComputeNode`、`FabricRegion` | 持久 Site/地图服务 |
| P1 | immutable ID/time/message/command/Receipt | 分别拒绝旧 RuntimeHostEpoch、DomainEpoch/InvocationId、session 和 binding；wall 回拨不改变 local deadline | 网络连接与 HA controller |
| P2a | bounded Mailbox + PortBinding test fixture | item/byte/age 与 queued/inflight/retained-byte 预算守恒；Command 不静默丢失；无 hidden backlog；fixture 不进入生产 API/plan | Zenoh session |
| P2b | LoopDomain + Dispatcher | 阻塞/CPU/unknown-native 工作负载不进主 loop；overload 下可解释的公平与 SLO | 公共 Lane |
| P2c | ThreadDomain + ExecutorBudget | 线程总量有界；timeout 不伪报已杀死；late result 被拒绝 | 每 Card 一线程 |
| P2d | ProcessDomain + Liveness/Recovery | crash/wedge/kill/IPC 污染/进程树清理和 quarantine 可验证；在途 effect 为 Uncertain 且默认不 replay | 只在 bootstrap 监测 |
| P2e | 最小 Deployment control plane | single-writer DeploymentController、独立 tenure authority、candidate atomic commit、Slice/CAS apply、DeploymentController/RuntimeHost journals 与 reconcile-once；restart/partial apply 不产生双 active revision | HA/共识、手写 Slice 生产旁路 |
| P3 | Authority + Resource lease/fencing + simulated Safety/Actuator | 竞争 Command、过期 lease、旧 token、Safety inhibition、`uncertain` 查询 | 跨 Node issuer |
| P4 | FabricService：Zenoh production routes | `session-local`、`host-local`、`remote` 通过同一 conformance；区分纯 compile、自动重连、Session 重建与 PortBinding install/reinstall/reconfigure/revoke；对应旧 ingress 被拒；liveliness 不冒充 ready | Fabric 作为 Resource owner |
| P5 | Node + DeploymentRevision：双主机 | 旧 incarnation/revision 被拒；分区和 reconciliation 可解释 | 多 controller 共识 |
| P6a | local durable Evidence + node-local Inspection | 独立显示本 Node 的 Host/session/binding/lease epoch 与 stale 原因；远端不可用仍可查询 | federated UI 统一拓扑树 |
| P6b | replication + federated Inspection + OpsService | lag/conflict/cursor 与 operation Receipt 可解释；不改写 source facts | 将 projection 或 OpsService 当 source owner |
| P7–P9 | TUI、ROS2Gateway、SpatialMap | 客户端边界、生态映射和地图 epoch 分别验证 | Robot/Fleet 聚合 |

首批硬性 harness：

1. RuntimeHost 重启后，旧 Host epoch 的 Receipt/callback 不能改变新实例状态。
2. ExecutionDomain 重建后，旧 DomainEpoch/InvocationId 的迟到结果不能改变状态或重复副作用；Runtime 依据 RuntimePlanSlice assignment 判断 Ready，DeploymentController/Inspection 再把 observed Domain/PID/TID/loop/capacity/epoch 与 committed `DeploymentPlan.execution` 对比；任一层不一致均不得 Ready。
3. 纯 compile 不改变 `BindingEpoch`，Zenoh 自动重连也不改变 epoch；Session 对象重建后旧 FabricSessionEpoch callback 被拒，PortBinding install/reinstall/reconfigure/revoke 后旧 BindingEpoch 消息只在同一 `BindingId` 内被拒；NodeId 始终不变化。
4. route replacement 的每个观测点最多一条 active route 接收新 Message；`prepare → activate → drain → retire` 可完成或显式 rollback，不出现双投、implicit fallback 或基于内容的 echo 去重。
5. Controller-role CardInstance A 的 lease token 为 7、B 为 8；A 迟到 command 被本地 actuator proxy 拒绝，即使 A 的 CapabilityGrant 还未过期。
6. ResourceCoordinator 与执行 owner 分别及联合重启后，旧 LeaseIssuerEpoch 的 lease/Command 全部被拒绝；持久 fencing/idempotency 状态恢复前不能签发新 lease。
7. lease 到期且没有 `local_only_renewal` 分区行为时，资源进入 configured safe state；有有效本地配置时，只允许其声明的行为。
8. Safety inhibition 在 Authority/Fabric 不可用时仍阻止 simulated actuator；解除抑制产生独立 Receipt。
9. 一个 future SiteRef 的两个 Zenoh topology profile 和两张 `SpatialMapRef` 可并存，且任何 `SemanticRegionRef` 名称都不改变 routing、FeatureReport 或 CapabilityGrant。
10. Zenoh 时间戳漂移、plugin/version 不匹配、multicast/scouting 配置失败均在 Fabric inspection 中显示为连接/配置 finding，不伪造应用 ready。
11. NodeDaemon restart 或 registration tenure 换代后，旧 NodeIncarnation 的 facts、回复和 NodeManagementEndpoint ref 全部被拒绝；RuntimeHost restart 不改变 NodeIncarnation。
12. NodeDaemon 失联只使 Node facts stale/partitioned，不自动宣称 RuntimeHost 已死；NodeStatus 不吞并 RuntimeHostStatus 的 producer/epoch/freshness。
13. 同一 NodeId 的双 NodeDaemon 中，旧 registration tenure 无法覆盖当前 facts；随机 incarnation 不能替代 enrollment/current-registration fencing。
14. NodeDaemon 不能改写、准入或绕过 RuntimeApplyRequest；错误 target/revision/proof/CAS 仍只由目标 RuntimeHost 的 RuntimeApplyEndpoint 拒绝并产生 Receipt。
15. NodeDaemon 与 RuntimeHost 共进程的 development profile 必须在生产 liveness/recovery conformance 中失败，不能冒充跨故障域恢复证据。
16. NodeDaemon 与 OS service manager 同时检测到 RuntimeHost stall 时，只有 profile 指定的 recovery mutation owner 消耗 restart budget 并执行动作，不产生双 restart 或双 quarantine ledger。

## 9. 风险、反例与失效条件

### 9.1 主要风险

- 把 Zenoh 1.9 region name 当作跨系统稳定 ID，未来 gateway 改动导致部署、授权或 UI 事实被意外重写。
- 只在 Runtime 内存保存 fencing token，进程/机器重启后重新接纳旧控制者。
- 用 remote wall clock 或 Zenoh timestamp 判定 lease expiry，遇到漂移、休眠或重连时误拒/误放行。
- 将 Authority 临时不可用解释为允许已缓存 CapabilityGrant 继续无限控制。
- 因 UI 便利把 Site、地图区域、网络区域、SafetyDomain 和 ResourceGroup 收敛为 `Robot` 或 `Topology` 对象。
- FabricService 再次吸收 Storage、Inspection、identity、lease、driver、调度和授权，重演万能 Bus。

### 9.2 反例与应对

- **极小单机设备**：development/constrained profile 中 NodeDaemon、RuntimeHost、FabricService 可以同进程，但 ID、ownership 和 epoch 仍不同；进程共置不是概念合并，也不能通过生产跨故障域恢复 Harness。
- **资源不支持 token 持久化**：由更靠近设备的单写 actuator proxy 保存 token；若也不可行，该资源不能提供分布式排他写入，只能 local-only 或 fail-closed。
- **网络确实与物理 Site 一对一**：DeploymentProfile 可声明 mapping，OPS 可共用展示分组；任一实体改名/重配不影响其他实体的 ID、lease 或 Authority。
- **语义导航暂未实现**：仍为 SpatialMap 保留 `SemanticRegionRef`；不创建空 service/schema，只禁止 Fabric 占用裸 `Region`。
- **将来需要 controller HA**：首先为 deployment 和 lease issuer 分别定义单写权、leader epoch、durability 和 fencing；不能把 Zenoh discovery/liveliness 当作共识。

### 9.3 会推翻或收缩本推荐的证据

- 产品范围正式冻结为单一不可远程控制的本地设备，且不支持 process restart、remote placement 或多地图；此时可把 deployment 身份模型收缩，但仍不应把 Navigation Region 与网络配置混名。
- 首个硬件资源无法在本地执行 token/lease/safety 判定，并且系统不能接受 fail-closed 或 local-only；此时必须先改变硬件/driver ownership，不应发布伪分布式控制。
- 实测证明 Zenoh 的版本或目标平台绑定无法满足 P4 的 reconnect、priority、FeatureReport 要求；此时重新评估 Fabric adapter，但不把实例化传输能力下沉 Kernel。
- 明确采用具有强一致 lease/fencing 服务的外部控制平面；此时可替换 issuer 实现，但本地 Enforcement、Safety 和 Receipt 边界仍保留。

## 10. Proposed ADR 影响

本研究建议分开创建 Proposed ADR，避免用一个“分布式架构 ADR”冻结所有实现细节：

1. **Node、NodeDaemon、RuntimeHost 与 Deployment identity/epoch**：冻结本研究 7.1 的 ID 关系、NodeManagementEndpoint/RuntimeApplyEndpoint 分权、重启规则、NodeId 迁移、远端 observed-state protocol 与 host recovery mutation owner。
2. **作用域与命名**：冻结短名 `Node`，禁止 `ComputeNode`、`FabricRegion`、裸 `Region`、裸 `Topology`；定义 site_hint/SiteRef、`ZenohTopologyProfile`、`SemanticRegionRef` 的 owner 与 mapping 禁令。
3. **Deployment controller ownership**：冻结 desired/observed state、DeploymentRevision、单写 controller 以及未来 HA 需要另行决策的边界。
4. **Physical command、lease、fencing 与 partition behavior**：冻结 Authority/Resource/Safety/Enforcement 链、token 单调性、执行点持久比较、expiry 时间语义、`uncertain` 恢复和资源级分区行为。
5. **Zenoh-native Fabric boundary**：冻结 CardDefinition `In`/`Out`/`PortSpec`、Deck 的 `Card.Out → Card.In` `Link`/`DeliveryProfile`、`DeploymentPlan.bindings`、live `PortBinding`、FabricService 的能力、ZenohTopologyProfile、session/binding epoch、keyspace/version、pre-validation ingress buffer/validation、target Mailbox、single-active-route/route replacement 规则与不拥有的职责。
6. **Evidence/Inspection time model**：冻结 `observed_at`、`reported_at`、sample time 与 local monotonic deadline 的边界，以及 receipt 的权威性。

ADR 应冻结不变量、producer/consumer 与验证条件，而不冻结具体 Rust crate、Python package 或其他语言文件布局、Zenoh gateway JSON、地图存储结构、数据库或 lease term 数值。

## 11. 推荐与置信度

**Verdict：revise。** 保留 ParaEGOX 当前“小 Kernel、RuntimeHost、Zenoh-native Fabric、Authority/Resource 分离”的大方向，但在首段代码前修订术语、身份分代、local-first Authority 依赖和 resource fencing 验收。最优先不是建立 Site/网络区域/Fleet 目录，而是证明一个资源 owner 能在 restart、partition 和迟到 command 下做出可审计且安全的拒绝。

置信度：

- 保留 `Node`、拒绝 `ComputeNode`/`FabricRegion`/裸 `Region`/裸 `Topology`：高。
- Site、Zenoh topology、SemanticRegionRef 不可合并：高。
- Node/RuntimeHost/session/binding 分代与远端 ownership：高。
- Authority、Lease、Safety、Enforcement 分离：高。
- 本地持久 fencing token 的必要性：高，具体硬件代理实现为中。
- 资源级分区行为、lease 期限、时钟误差预算和多 controller 共识：中，须由目标硬件和故障注入验证。

## 12. 后续入口

- [ParaEGOX 分布式系统模型](../architecture/distributed-system-model.md)
- [Node 与作用域边界](../concepts/node-and-scope-boundaries.md)
- [Kernel、RuntimeHost 与 Core Services 架构基线](../architecture/kernel-runtime-core-services.md)
- [Kernel Foundation 实施计划](../plans/kernel-foundation.md)
- [Kernel 消息机制、Fabric、Evidence、Telemetry 与 Security 边界研究](kernel-messaging-fabric-evidence-security.md)
- [Runtime 执行模型、调度与恢复研究](execution-model-scheduling-and-recovery.md)
- [CardDefinition 输入输出、Port、Link 与运行绑定研究](card-definition-ports-links-and-bindings.md)
