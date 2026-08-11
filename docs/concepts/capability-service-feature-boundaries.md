# Capability、Service Contract 与 Feature Support 边界

> 状态：Draft
> 日期：2026-07-29
> 范围：授权、服务依赖、平台支持事实及其在 CardDefinition、Card、Deck、Deployment 与 Runtime 中的所有权
> 实现状态：尚未实现；本文冻结候选术语，仍需 Proposed ADR 评审
> Deployment 边界：[ADR-0001 — DeploymentController、DeploymentPlan 与 Runtime 边界](../adr/ADR-0001-deployment-controller-boundary.md)
> Application 边界：[Application、Deck、Card 与 Service 边界研究](../research/application-deck-card-service-boundaries.md)

## 一句话结论

ParaEGOX 不提供裸 `Capability` 公共类型；capability 词根只保留在安全授权族 `CapabilityScope/CapabilityGrant` 中。`CapabilityGrant` 回答“某个 Principal 当前被允许请求什么”；服务提供关系使用 `ServiceContract`，Node、Fabric 与 Device 的实际支持情况使用限定的 `FeatureReport`。三者不能共享基类、状态机、版本号或缓存。

## 1. 为什么必须拆开

“能力”在自然语言里至少包含三件完全不同的事：

1. 某个主体是否被授权执行一个操作。
2. 某个服务是否提供消费者需要的接口。
3. 某个目标平台、传输或设备是否真正支持一种特性。

如果把三者都叫 `Capability`，就会出现危险的错误推断：服务可发现不等于调用者被授权；目标 Node 支持 SHM 不等于当前 binding 已启用 SHM；Deck 声明需要设备写权限也不等于它已经拿到可执行的 Grant。

因此，“服务存在”“平台支持”和“主体获权”必须分别求解、分别失效、分别观测。

## 2. 三类正式语义

| 语义 | 正式术语 | owner | 典型失效事件 |
| --- | --- | --- | --- |
| 谁可请求哪些操作 | `PermissionRequirement`、`CapabilityScope`、`CapabilityGrant` | AuthorityService；执行点负责本地验证 | 过期、撤销、策略换代、Principal/实例换代 |
| 谁提供或需要什么接口 | `ServiceContractId`、`ProvidedService`、`ServiceRequirement` | ServiceSpec 声明；DeploymentPlanner 解析候选；DeploymentController 提交 revision | provider 消失、版本不兼容、依赖不 Ready |
| 目标实际支持什么 | `NodeFeatureReport`、`FabricFeatureReport`、`DeviceFeatureReport` | NodeDaemon、FabricService、Driver | 重启、重连、设备更换、驱动/固件变化 |

这些名字属于三个独立命名族，不暗示继承关系。禁止创建通用 `CapabilityBase`、统一 `capability_version` 或同时缓存三类状态的 registry。

## 3. CapabilityGrant

`CapabilityGrant` 是可传递但必须受约束的授权值。它至少需要表达：

- `principal`：被授权主体。
- 通用 workload subject：`InstanceId` 与 owner-issued subject incarnation/epoch；不引用 Card、Deck、Service、Gateway 或 Driver 领域类型。
- `scope`：resource selector 与允许的 operations。
- `audience`：可在哪个执行点或服务验证，防止 token 被转交给错误消费者。
- `constraints`：时间、位置、数据标签、控制模式、审批或其他限制。
- `issued_by`：Authority owner。
- `policy_version` 与 `revocation_epoch`。
- `expires_at` 或可由本地验证者可靠判断的期限语义。
- 可选的 delegation/attenuation proof；子 Grant 只能缩小，不能放大父 Grant。

Kernel 可以承载不可变值、纯验证输入和 reason code，但签发、撤销、策略持久化、凭证轮换与审计属于 AuthorityService。RuntimeHost、Gateway、Driver 和资源 owner 是 enforcement points，不因为进程位置或组件类型自动获得权限。

`CapabilityGrant` 只表示“允许请求”，不表示：

- provider 存在或 Ready。
- Node/Device 支持目标操作。
- 当前拥有物理资源控制权。
- Safety 已允许本次动作。
- 副作用已经执行成功。

物理写入仍需经过 `CapabilityGrant → AuthorityDecisionRef → Lease/Fencing → SafetyIslandAdapter/SafetyDecisionRef → Enforcement → EffectReceipt`；Decision 必须绑定具体 command/operation/resource/device/audience/expiry，不能只凭 policy revision 复用。Adapter 只接入独立 safety island，不是安全功能 owner。

## 4. ServiceContract

`ServiceContractId` 标识一个稳定、版本化的服务接口；`ProvidedService` 与 `ServiceRequirement` 构成 ServiceDependencyGraph 的声明。它们回答“需要什么接口、由谁提供、版本是否兼容”，不携带授权 token。

一个 `ServiceRequirement` 可以包含：

- contract id 与兼容版本范围。
- required/optional。
- locality、latency、availability 或数据驻留约束。
- dependency-loss policy 的选择范围。

DeploymentPlanner 根据已冻结 ServiceSpec/DeckLock 与 Node facts 纯解析候选 provider，DeploymentController 验证并提交该 revision；RuntimeHost 只接收已解析、目标限定的 typed client/handle。消费者仍需独立的 `PermissionRequirement` 才能执行受保护操作。

Core Service 的公共接口称 `ServiceContract API`，不能再称 `Capability API`。DeckSpec/DeckTopology 只能声明 `ServiceRequirement`，不能创建或停止平台关键服务。

ServiceRequirement 只表达接口依赖，不表达 provider ownership。当前 ServiceSpec/CoreService 模型覆盖平台级、跨相互独立 Product/Installation 共享或持有平台权威的服务；同一产品内跨两个 Deck 使用并不足以准入 CoreService。“只属于一个稳定产品安装、跨 DeckRun 持久”的领域服务是明确后置的模型缺口，不能仅靠 `scope: application-name`、Card 全局状态或把它提升为 CoreService 解决。稳定 Application/Installation owner 出现后，再研究复用 ServiceContract/ServiceSpec/ServiceInstance 并增加 lifetime、state migration、retention/GC authority 等字段。

只随一次 DeckRun 存活的共享能力是不同问题：优先用 Card 表达拓扑可见领域计算；若确有多个消费者需要 ServiceContract，再研究以 DeckRun 为 owner 的 run-bound ServiceSpec。它不要求 Application identity，也不产生平台 CoreService 权威。

## 5. FeatureReport

Feature report 是带观测时间、来源、版本和作用域的 observed fact，例如：

- Node 是否存在 GPU、IOMMU、特定 accelerator 或可信执行能力。
- Fabric 当前是否支持所需的 SHM、优先级、多流、编码或路由特性。
- Device/Driver 当前是否支持某一操作、控制模式、固件 ABI 或校准版本。

Feature report 不是永久标签。它至少要带 owner、observed_at、source epoch/incarnation、feature version 与 support/degraded/unsupported/unknown 状态。Deployment 可以用它验证 placement 与 binding，但不能据此授予权限。

报告不满足 Requirement 时使用 `FeatureMismatch` 或 `FeatureLoss`，不用 `capability-loss`。例如 Zenoh route 的可用特性由 `FabricFeatureReport` 表达；允许某个 Card 使用 raw Fabric 则由 scope 指向 Fabric resource 的 `CapabilityGrant` 表达。

## 6. CardDefinition、Card 与 Deck 的声明

CardDefinition 的需求必须拆成三个命名空间：

```yaml
requires:
  services:
    - contract: model.inference.v1
  permissions:
    - resource: microphone/*
      operations: [read]
  features:
    - kind: node.gpu
      version: ">=1"
```

这里的 YAML 只表达心智模型，不是已冻结格式。

规则如下：

1. CardDefinition 声明内在最小 Requirement。
2. CardProfile 可以把通配 resource selector 绑定到更窄的具体资源或删除 optional permission，但缩小后仍须满足 CardDefinition required operations；不能扩大未声明权限或削弱必需要求。
3. DeckSpec 聚合 Requirement，但不保存 Grant、token、secret 或 observed FeatureReport。
4. DeckLock 内嵌受 digest 覆盖的 canonical DeckTopology，并固定 CardDefinition/Artifact candidate、Schema/Adapter、ServiceRequirement/FeatureRequirement contract/range 与声明的平台兼容约束；它不固定 live provider、目标 Node Feature 匹配、placement 或运行 route，也不签发权限。这些 target-specific 结果只进入 DeploymentPlan。
5. DeploymentPlanner 根据目标 facts 计算 Service/Feature Requirement 的候选匹配，DeploymentController 在提交/激活 revision 前验证结果，并通过窄 Authority API 请求实例限定 Grant；DeploymentController 不能自己签发、缓存或扩大 Grant。
6. AuthorityService 将 Grant 绑定到 `PrincipalRef`、通用 `InstanceId`/subject incarnation、可选 DeploymentRevision、audience 和期限；CardInstance、ServiceInstance、Gateway、Driver 等领域身份到这些通用字段的映射留在各自上层。
7. RuntimeHost 向 CardInstance 私有实现上下文注入窄 typed client/handle；默认不交付原始 token、Zenoh Session 或全局 Service Locator。

服务解析、特性匹配和授权签发中的任一步失败，都必须在启动前或运行期产生各自的 reason code，不能归并成一个 `capability_error`。

## 7. 运行期状态变化

三类状态的变化顺序也不同：

```text
ServiceRequirement ──resolve──> provider / typed client
FeatureRequirement ──match───> target support fact
PermissionRequirement ─issue─> CapabilityGrant
                                      │
                                      ▼
                          local enforcement + Receipt
```

- provider 在 Ready 后丢失：按 dependency-loss policy 进入 degraded、stop、rebind 或 restart；不能只撤销 Grant 后假装依赖仍可用。
- Feature 在运行期丢失：受影响 binding/instance 变为 degraded 或 not-ready，触发 reconciliation；不能把旧 report 永久缓存。
- Grant 过期或撤销：新的受保护请求必须拒绝；是否终止已开始 Operation 由明确策略与资源 owner 决定。

Inspection projection 与 OpsClient/TUI 必须能分别显示 provider 状态、Feature support 与授权状态，不能用一个绿色“Capability available”覆盖三者。

## 8. 与外部协议的边界

- Zenoh ACL 可以作为传输层防御，但不替代 ParaEGOX `CapabilityGrant`、Lease 与 Safety。
- MCP 的 protocol capabilities 表示协议特性协商，不映射为 ParaEGOX 授权；MCP Gateway 必须重新做身份、scope 与 audience 绑定。
- A2A Agent Card 中的 skills/capabilities 是外部自描述，不等于 ParaEGOX `Card`、`ServiceContract` 或 `CapabilityGrant`。
- ROS2 graph 中可发现的 topic/service/action 不自动成为 ProvidedService，也不自动获得权限。

外部类型在 Gateway 内保留协议限定名称；进入内部模型后必须转换成对应 Requirement、Grant、FeatureReport 或 Port/Operation 契约。

## 9. 强制不变量

1. 公共 Schema、API、配置和新代码完全禁止裸 `Capability`；自然语言与历史说明也应优先写全 `CapabilityGrant`，不得用缩写重新引入歧义。
2. Kernel Grant 不引用 Card、Deck、CardDefinition、CoreService、Gateway 或 Driver 领域类型，只使用通用 Principal/Instance/incarnation/audience/revision 约束。
3. `CapabilityGrant`、`ServiceContract` 和 Feature report 不共享基类、epoch、缓存或 registry。
4. 工具可见、服务可发现、Fabric 可连接和 Component 同进程都不产生隐式授权。
5. Deck/Card/CardDefinition 只能声明 Requirement，不能内嵌 Grant、token、Secret 或旧的 observed report。
6. CardProfile 只能缩小权限和加强约束。
7. 远端调用使用 audience-bound、实例限定的 typed client/handle；禁止 token passthrough。
8. FeatureReport 必须带 owner、时间和 epoch；unknown 不得解释为 supported。
9. Lease、Safety 与 Receipt 不由 CapabilityGrant 替代。

## 10. 首批验证

- provider Ready 但调用者无 Grant：拒绝受保护操作。
- Grant 有效但 provider 不存在或版本不兼容：Deployment 不 Ready。
- Grant 与 provider 均有效但目标 Feature 缺失：计划在 apply 前失败或按显式策略降级。
- CardProfile 试图扩大 Permission scope：编译失败。
- Grant audience、DeploymentRevision、通用 InstanceId/subject incarnation 或 revocation epoch 不匹配：本地 enforcement 拒绝；CardInstance 等领域身份只由上层映射。
- FabricFeatureReport 在重连后变化：旧 BindingEpoch 与旧 report 不能继续支撑 Ready。
- InspectionService 能投影 Service、Feature 和 Grant 的不同状态与原因，OpsClient/TUI 能忠实展示。

详细证据、替代方案与实施顺序见 [分布式具身 Agent OS 缺口研究](../research/distributed-embodied-agent-os-gap-analysis.md)。
