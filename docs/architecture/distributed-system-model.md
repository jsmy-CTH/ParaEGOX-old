# ParaEGOX 分布式系统模型

> 状态：Draft
> 日期：2026-08-04
> 范围：分布式身份、作用域、所有权、网络分区、一致性与恢复语义
> 实现状态：部分合同、本地 Runtime execution substrate、DeveloperLocal Fabric/Agent/TUI、NodeDaemon observation、Controller 本地发现及 owner-private 受限 apply 机制已实现；本文主体仍定义目标边界，不代表已有生产 provisioning、真实双机证据、一般 workload RuntimeAssemblyEngine、跨平台或集群能力
> 研究依据：[分布式身份、作用域与物理所有权研究](../research/distributed-identity-scope-and-ownership.md)、[CardDefinition 输入输出、Port、Link 与运行绑定研究](../research/card-definition-ports-links-and-bindings.md)、[Runtime 执行模型、调度与恢复研究](../research/execution-model-scheduling-and-recovery.md)、[Graph Foundation、领域图与执行边界研究](../research/graph-foundation-and-domain-execution-boundaries.md)、[Web Console、WebRTC、WebXR 与交互式 Gateway 边界研究](../research/web-console-webrtc-webxr-gateway-boundaries.md)、[Application、Deck、Card 与 Service 边界研究](../research/application-deck-card-service-boundaries.md)
> 架构裁决：[ADR-0001 — DeploymentController、DeploymentPlan 与 Runtime 边界](../adr/ADR-0001-deployment-controller-boundary.md)、[ADR-0003 — OPS、OpsService 与 Inspection 操作边界](../adr/ADR-0003-ops-service-operation-boundary.md)、[ADR-0006 — Rust-first 核心机制与多语言工作负载边界](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md)

## 一句话结论

ParaEGOX 以分布式具身智能系统为目标，采用 **distributed-first contracts、local-first execution、Zenoh-native Fabric、Rust-first mechanisms + polyglot workloads**：契约从第一天处理远端故障和不确定性，Kernel 外的 DeploymentController 提供按 scope 单写的 desired-state/revision owner 并协调跨 Node rollout，每个目标 RuntimeHost 的内部 RuntimeAssemblyEngine 只执行 canonical Slice 的本地装配，Rust reference core 拥有 Runtime、Node 与 Fabric 机制，Python/C++/Agent/模型和设备生态通过受管 ProcessDomain 或独立 Service/Gateway 接入；物理现场优先保持有界的本地自治，Zenoh 是唯一生产数据面，覆盖 `session-local`、`host-local` 与 `remote` route locality，不存在全局 Graph scheduler，硬安全与最低本地自治不依赖远端网络或普通 Rust/Python Runtime 存活。

## 当前实现快照（2026-08-04）

S2/B2 已实现语言中立的 canonical signed apply envelope、真实 Ed25519 exact-trust admission、target clock domain/generation 约束和纯 reducer consumer；P2a–P2c 又增加 PXTA、PXTE v1/v2、PXAR v1–v3、bounded Mailbox/PortBinding、LoopDomain/Dispatcher、ThreadDomain/ExecutorBudget 与可启动但 idle 的 RuntimeHost substrate。S6/P2d 进一步锁定 additive PXTE v3/PXAR v4、strict PXWP v1/PXHW v1，并由 Rust/Python 独立实现固定 canonical bytes/version/error vectors。S7-E 已增加 exact Linux ext4 的 descriptor/install、三个 owner journal、TenureAuthority、one-shot DeploymentController、authenticated Runtime bootstrap/apply 与 fixed Loop→Empty vertical。

Runtime 现在有 private/experimental local POSIX ProcessDomain baseline：generation/sequence fencing、bounded IPC credit/retained bytes、heartbeat/liveness、failure facts/recovery、`Uncertain`/no-replay、cooperative stop→TERM→KILL、same-process-group cleanup、identity-guarded workspace cleanup、exact-zero cleanup proof、restart/quarantine，以及 Linux `/proc` aggregate resource census。Rust/Python reference workers 覆盖 crash、partial frame、ignore cancellation/TERM、stale generation 和 same-group grandchild；独立 POSIX service-manager/watchdog 通过 PXHW v1 持有 RuntimeHost process group、restart/backoff/quarantine reference ledger。

S7-F 已提交 authenticated PXQR/PXQS Runtime query、Controller owner-private exact query journal/client，以及 Runtime payload v5、lossless stopped v4→v5 migration 与 listener-before-publish fixed `OneSourceLoop`/`EmptyDeactivate` restart reassembly。Runtime tranche 的最新基线是 `f32700f`，Ubuntu CI `30782979187` 已成功。当前未提交工作树还存在 `paraegox-deploymentd reconcile-reference-once-v1`、bounded one-shot reconcile 与 SF6 Linux SIGKILL/lost-response/system scenarios，但它们尚未通过 fresh Ubuntu CI，S7 仍为 `in_progress`。

当前工作树又增加了 DeveloperLocal Fabric/Agent/TUI 进程链、PXND/PXNS NodeDaemon 单写快照与受认证
Runtime observation ingress，以及 Controller 从 owner-private PXNC 启动、经 PXNL 观察 NodeDaemon、原子维护
PXDJ/PXDN 的本地可信发现链。PXDN 对 Runtime epoch、observation sequence/digest、endpoint generation、
absolute freshness 和 predecessor completion snapshot floor 做持久 fencing；`NotModified` 不续 freshness。
PXRP v1、PXCB v1、PXRC v1 与 PXDS v2 也已定义受限远端 apply 的精确 TLS profile、carrier、Controller
请求签名与 Runtime receipt 签名边界。Runtime 的具体 pinned-Ed25519 接缝会先验外层 PXRC，再复用既有
PXAR8 单写者，最后复验内层 PXDS1 并签出 PXDS2；纯 contract verifier callback 本身仍只是调用方 TCB。

这些仍主要是单 Node fixed-profile 和本地机制证据，不是完整分布式运行能力：deploymentd 是 one-shot
CLI，不是 daemon 或 continuous reconciler；NodeDaemon 尚无 registration acquisition 或 Zenoh observation
adapter；PXNC 尚无生产 producer。Controller connector-only / Runtime listener-only Zenoh 适配器、Runtime
进程内同一单写者 ingress、PXDJ v3 双目标原子 claim、one-shot dispatch owner 与 PXDS v2 双目标独立验签/
durable terminalization 已落地；predecessor pair 允许不同 target 的 target-specific plan digest，但继续严格
共享 scope/plan/revision/writer tenure/Controller key pins。生产调用方仍未
提供 PXRP producer、credential/profile resolver 或真实证书/双机链路，因此真实跨 Node reconcile 尚未接通。
launch resolution 仍是 trusted caller seam，Linux census 不是 cgroup
containment，RuntimeHost 被不可捕获 SIGKILL 后，独立 process group 的 ProcessDomain worker 仍没有外部
枚举/清理 owner；admission ledger retention/rollover 也未实现。ProductionReference 只接受 exact Linux
ext4，SF6 在 macOS 只 skip，macOS/Windows 均没有 production support。当前远端 GPU23 还没有 Rust
toolchain，因此这些未提交工作树改动尚未取得完整 workspace compile/clippy/test 证据。

## 1. 目标与非目标

同一套契约应覆盖：

```text
单进程开发
    ↓
单 Node 多进程
    ↓
设备 Node + 边缘 Node + 云端 Node
    ↓
多物理站点协同
```

系统需要允许 CoreService、CardInstance 和 Gateway 跨 RuntimeHost、进程与 Node 部署；在断连、重启、重复、乱序和版本不一致时给出明确结果；重连后通过 revision、epoch、Receipt 和 desired/observed state 恢复。

这套契约也必须跨实现语言成立。Rust struct/enum layout、trait object、Tokio handle、Python object、任意 pickle 或语言默认 serializer 都不能成为分布式协议；稳定边界由唯一 Schema、canonical encoding、digest、版本、unknown-field 和 reason-code 规则定义。当前语言选择不改变任何领域 owner，也不允许 worker 建立第二 Deployment truth、第二 RuntimeHost 或第二语义 Mailbox。

首阶段不建设 Kubernetes 替代品、通用容器编排器、Fleet/Cluster 大对象、全局强一致数据库或多控制器共识，也不承诺端到端 exactly-once。云端、Agent、Graph、OPS 与普通网络队列不得成为本地 Safety 路径的必要依赖。

## 2. 五套不能合并的作用域

ParaEGOX 不用一棵万能层级树描述所有分布式关系：

| 作用域 | 权威 owner | 当前表达 | 不能替代 |
| --- | --- | --- | --- |
| 计算身份 | enrollment / Deployment | `NodeId`、`NodeIdentity`、`NodeSpec`、`NodeStatus` | Robot、ROS node、Zenoh peer |
| 执行生命周期 | 目标 Node 的 RuntimeHost | `RuntimeHostId`、`RuntimeHostStatus`、Execution Domain | 远端代理、DeploymentPlan |
| 工作负载连接 | DeckSpec / DeckCompiler | DeckLock 内嵌的 `DeckTopology`、Card 对 CardDefinition `In`/`Out` `PortSpec` 的解析引用，以及 `Card.Out → Card.In` Deck `Link` | 安装后的 `PortBinding`、服务依赖、网络或空间图 |
| 网络连接 | FabricService + DeploymentProfile | `ZenohTopologyProfile`、session/binding status、Fabric 私有 region observation | Site、TrustDomain、业务身份 |
| 物理与空间 | Resource/Safety；未来 SpatialMap | `ControlledResourceRef`；未来 `SpatialMapRef`、`MapEpoch`、`SemanticRegionRef` | Node 或 Zenoh 拓扑 |

因此：

- ParaEGOX 顶层的短名 `Node` 只有“可独立识别、部署、管理和断连的计算单元”这一种含义，不改为冗长的 `ComputeNode`。
- ROS2、Zenoh 和 Graph 上下文分别使用 `RosNodeRef`、`ZenohSession`/`ZenohPeer`、Card 或 Step，不能复用 ParaEGOX `Node`。
- 当前只允许非权威部署元数据 `site_hint`；有多站点 OPS、数据驻留或站点自治的真实消费者后，才通过 Proposed ADR 引入 `SiteRef` 及其 owner。
- 裸 `Region` 和裸 `Topology` 禁止进入公共契约。Zenoh 厂商概念、Deck 工作负载连接、服务依赖和空间导航必须使用限定名称。
- CardDefinition 使用 `In`/`Out` 声明 transport-neutral `PortSpec`，Deck 的 `Card.Out → Card.In` `Link` 表达一次工作负载连接与 `DeliveryProfile`；DeckCompiler 只产生内嵌 canonical DeckTopology 的 DeckLock，DeploymentPlanner 将绑定结果写入 DeploymentPlanCandidate.plan_content.bindings，DeploymentController 原子提交后才形成权威 `DeploymentPlan.bindings`，RuntimeHost/Fabric 安装后才产生 live `PortBinding`。

当前“应用”只作为产品语言，不增加一套分布式 identity。Deck 是 executable workload；DeckLock 是自包含的 resolved closure；DeckRun/CardInstance/ServiceInstance 是运行事实。`DeploymentScope` 只表示 desired-state 单写与 revision 的权威范围，不是 Product/Application identity：未来一个产品可能跨多个 scope，同一 scope 也可能承载多个产品工作负载，因此不能通过复用 scope ID 提前伪造 Application/Installation。

只有多 Deck 聚合、稳定安装身份或应用私有持久状态出现真实 producer/consumer 后，才研究 Deck 之上的 Application 控制/交付对象；它仍必须通过 DeploymentController 提交 intent，不能直接 apply Runtime 或成为第二个 reconcile owner。

一个未来 `SiteRef` 可以映射零到多个 Zenoh 拓扑片段和零到多个空间地图；一对一只能是某个 DeploymentProfile 的配置，不能成为类型等价关系。若空间位置影响网络选路，应建立显式 `RoutingPolicy`，不能共享或推导两种区域 ID。

详细词义见 [Node 与作用域边界](../concepts/node-and-scope-boundaries.md)。

## 3. 分层与所有权

```text
Logical control plane (outside Kernel)
DeploymentPlanner → DeploymentController / AuthorityService / ResourceCoordinator
            Rust-first reference mechanisms; ownership remains language-neutral
InspectionService（read-only projection） ← owner facts
Ops clients → OpsService → Authority → typed owner API
                │ committed DeploymentPlan.bindings / execution / decisions
                │ RuntimeApplyRequest directly to RuntimeApplyEndpoint
                ▼
Execution plane
Rust RuntimeHost.RuntimeAssemblyEngine → CoreService / CardInstance / Mailbox / PortBinding
RuntimeHost → trusted Rust implementation or managed polyglot worker / local ExecutionDomain
     ▲
     └── NodeDaemon: Node facts + Runtime endpoint discovery（not an apply gate）
                │ installed PortBinding
                ▼
Data plane
Zenoh-native Fabric
{session-local | host-local | remote}
                │ explicit semantic and trust boundary
                ▼
Ecology plane
ROS2Gateway / Web Console & media/XR Gateway roles / Drivers / external systems
```

### 3.1 Control plane

- DeploymentPlanner 是无副作用、确定性的求解组件：消费已解析 DeckLock/ServiceSpec、不可变 Node facts、policy 与 stable-ID allocation snapshot，生成 `DeploymentPlanCandidate {plan_content, allocation_delta, diagnostics, plan_content_digest}` 或结构化拒绝；PlanContentDigest 只覆盖 PlanContent。它不监听 Node、不持久化、不分发、不重试。
- DeploymentController 按 `DeploymentScope` 单写，在一个 crash-consistent transaction 中原子提交 allocation delta、下一 DeploymentRevision 与 committed DeploymentPlan，拥有 rollout 与 reconciliation 决策；committed plan 的 `bindings` 子树是绑定的 desired truth，不另建并行 `BindingPlan`。DeploymentController 不解析 Deck、不启动 Runtime 实例、不持有 Zenoh Session，也不签发 Grant/Lease/SafetyDecision。
- DeploymentTenureAuthority 是 writer tenure proof 的唯一签发 owner；它由 OS service manager 独立托管、原子推进 DeploymentWriterEpoch，signing key 不暴露给 DeploymentController。RuntimeHost 只信 bootstrap 配置的 authority verification root。
- AuthorityService 回答某主体是否有权请求某操作。
- ResourceCoordinator 回答当前谁持有受控资源并分配 lease/fence；Driver EnforcementPoint 拥有 normal-command 的最后软件比较、idempotency 与 setpoint submission，物理下游 safety island/设备原生 controller 独占 enable/inhibit/clamp/safe-output gate 与最终 applied-output ack。三者可同故障域但逻辑 owner 分离；远端控制面只能请求，不能越过本地 issuer/EnforcementPoint/safety gate。
- InspectionService 只拥有 read-only projection revision/cursor/cache/freshness，不拥有 source facts、desired state 或运行对象。P6a 提供 node-local role，P6b 可以增加 federated role。
- `OPS` 是运维领域/产品标签；OpsService 是 CoreService，只拥有 ControlRequest identity/digest、幂等 journal、进度、取消/reconcile intent 和 terminal OpsReceipt。它消费 Inspection、Authority 与 Evidence contracts，并经 typed client 调用 DeploymentController、NodeManagementEndpoint 或其他真实 owner；不直接写 Runtime、Deployment store 或领域数据库。

这些逻辑 concern 与物理部署正交。首个分布式 reference profile 中，每个 Node 提供 node-local Inspection/Evidence producers，一个独立管理侧 OpsService 与 federated Inspection role 服务多个 Node；ConsoleGateway 只按 Web exposure 需要部署，不是每 Node 必备。OpsService 可以由 committed ServiceSpec 管理，但 DeploymentController 的 bootstrap、writer tenure 和 reconcile 不依赖它。OpsService 故障只影响新的运维请求与聚合可见性，不停止 RuntimeHost、Continuity 或本地 Safety。

首个 production reference 的 DeploymentPlanner、DeploymentController 及相关机制优先使用 Rust，但单写、journal、revision、tenure 和 fencing 才是正确性来源；实现语言不产生新的 control-plane 身份。Python/C++ 实现只有通过同一语言中立 contract、writer proof 与故障 Harness 才能替换组件，不能并行成为第二 writer。

### 3.2 Execution plane

- NodeDaemon 产生本 Node 的 presence、NodeIncarnation、NodeFeatureReport、NodeManagementEndpoint 与 Runtime endpoint discovery facts；它不把 Inspection 变成注册表 owner，也不接收、改写或准入 RuntimeApplyRequest。
- 首个 production reference 的 NodeDaemon、RuntimeHost、Runtime-owned contracts、Mailbox/Dispatcher/ExecutionDomain 与 Zenoh Fabric path 优先使用 Rust；async runtime、task/channel 和 crate-private 类型只是内部实现，不进入公共 RuntimePlanSlice、Message、Receipt 或 Inspection contract。
- RuntimeHost 只拥有本 Node 内的 `LoopDomain`、`ThreadDomain`、`ProcessDomain`、Mailbox、PortBinding、RuntimeOwnershipTree、RecoveryAction 和关闭；RuntimeApplyEndpoint 及其 target/revision/proof/CAS/fencing 校验也只属于 RuntimeHost。
- RuntimeHost 通过 `RuntimeApplyRequest` 应用唯一 DeploymentPlan 投影出的 target `RuntimePlanSlice`；内部 RuntimeAssemblyEngine 只按已编译的 assignment、readiness/activation group、consumer ingress、producer egress、dependency-loss 与 drain contract 创建或替换 Domain/Instance/Mailbox/PortBinding。它没有独立 identity/revision/store，不从 Deck Link 重算启动语义，也不进入 steady Message hot path；Runtime 不 import deployment/decks。
- 稳定运行后，live `PortBinding` 将 Message 准入 bounded Mailbox，所属 ExecutionDomain 才调用 CardInstance 私有实现 callback/invocation；不存在逐 Message 的中央 Graph Engine。
- 只有随 RuntimeHost 同构建、同发布且受信的 Rust implementation 可以进入 in-process Loop/ThreadDomain。Python/C++、模型、第三方 native、未知或需要强制终止的工作默认由 ProcessDomain 以受版本控制的 worker protocol 托管；worker 只拥有分配给它的 invocation 和窄 handles，不拥有 Deployment、Mailbox、readiness、restart policy 或 raw Zenoh Session。
- ProcessDomain control metadata 携带 Artifact/protocol/runtime compatibility、source revision、instance/generation、InvocationId、heartbeat sequence、credit、deadline/cancellation 和 terminal result。跨边界不得传递语言私有对象、Future/Task/Queue/Lock、裸指针或未版本化对象句柄；Blob/Buffer/SHM 只有在 lease/generation、bytes accounting、consumer crash 和释放合同通过后才能使用。
- 目标 Node 的 RuntimeHost 拥有目标实例生命周期；调用侧只拥有 typed service client/permission-bound access handle，不产生第二种 live Binding 类型。
- `external` 工作负载由 systemd、容器或 external workload manager 拥有；ParaEGOX 通过明确 `ExternalWorkloadAdapter` 请求或观察，不虚构生命周期所有权。

当前实现注记：上述 execution plane 仍有大量目标态。现有代码除 private local POSIX ProcessDomain、idle
RuntimeHost substrate 和 external service-manager/watchdog adapter 外，已落地 exact Linux ext4 的 durable
fixed Loop/Empty plan/apply/query owner、payload-v5 restart reassembly、DeveloperLocal Fabric/Agent/TUI，以及
NodeDaemon 的 durable observation owner/本地 UDS ingress 和 Controller PXNC→PXNL→PXDN 本地发现链；这些仍
不是一般 live plan-driven instance owner。`paraegox-fabric` 已有同一 Zenoh Session 的 secured-hybrid 配置
和 DeveloperLocal typed binding，但远端受限 apply adapter、Runtime concrete PXRC signature ingress、双目标
原子 claim/receipt 与真实跨 Node execution 尚未完成。PXHW adapter 只是 OS-level RuntimeHost lifecycle
reference owner，不产生 NodeStatus/NodeIncarnation，也不进入 DeploymentController→RuntimeApplyEndpoint
权威链。当前工作树尚待完整远端 workspace compile/clippy/test 和 fresh Ubuntu CI，不能把类型或本地单元证据
当作已验证的 distributed capability。

### 3.3 Data 与 ecology plane

FabricService 根据 desired binding 处理 `session-local`、`host-local` 与 `remote` production route 的 bind、pub/sub、query/queryable、liveliness/matching、session/reconnect、keyspace/schema/feature mapping，以及自身连接、`FabricFeatureReport` 和 binding 的 Inspection。首个 production reference 使用 Zenoh Rust API，但 `Message`、PortBinding 与 Fabric contract 不暴露 Zenoh 或 Rust 私有类型。Zenoh callback 收到的 pre-validation encoded frame 只能进入 items/bytes/age 有界、可观测的 Fabric ingress buffer；该 buffer 不是 Mailbox，不能产生应用 accepted。Ingress worker 完成 decode/Schema/principal/binding 准入后才构造不可变 Message，RuntimeHost 再将它准入唯一 target Mailbox 并调度 ExecutionDomain。CardDefinition 只声明 `In`/`Out` 与访问需求；CardInstance 的私有实现上下文只获得对应的已编译窄绑定能力，不获得原生 Zenoh Session。非 Zenoh 路径只作为确定性 `PortBinding test fixture` 把已验证 Message 连接到相同 Mailbox 契约，不进入生产 DeploymentPlan，也不形成 LocalBus、MemoryPortBinding 或第二种 Backend。

Zenoh storage 只能作为显式 ServiceContract/adapter，调用者另需 scope 指向 Fabric resource 的 `CapabilityGrant`；它不能拥有 Evidence、World 或 Memory 的保留策略，Fabric 自检也不能替代全局 Inspection。ROS2/DDS 与浏览器 HTTP/SSE/WebSocket/WebRTC 都是 Gateway 外部生态腿，不是与 Zenoh 并列的数据面 Backend。

浏览器边界内部需要区分三条子路径，但不新建另一套全局 plane taxonomy：

```text
Web Console ── HTTPS/SSE/WS ──> ConsoleGateway ──> InspectionClient / OpsClient
TUI / CLI ───────────────────────────────────────> InspectionClient / OpsClient
Browser media ── signaling/WebRTC ──> Media Gateway role ── typed seam ──> media producer
WebXR input ── DataChannel/WS ──> XR Input Gateway role ── typed seam ──> controller/teleop owner
```

第一、二行分别读取 InspectionProtocol 或向 OpsService 提交受权 ControlRequest；只有 Web Console 必须经过 ConsoleGateway。第三条终止 peer/media 协议；第四条在进入内部前校验限定 session/stream epoch、Schema、sequence、time、`FrameRef`、CalibrationRef、uncertainty 与 freshness。连续 XR input 不逐帧经过 OpsService，物理 Command 仍必须经过 Authority、Lease/Fencing、Safety 和 Driver EnforcementPoint。当前 Card Port 到非 Card Gateway endpoint 的 authoritative binding/exposure contract 尚待 Proposed ADR，不能用示意图声称已经实现。

## 4. 身份、状态与分代

Node 不是一个把所有事实塞进去的大对象：

| 模型 | 内容 | owner |
| --- | --- | --- |
| `NodeIdentity` | 稳定 `NodeId` 与 `PrincipalRef` | enrollment / identity |
| `NodeSpec` | 期望标签、平台约束和允许的 RuntimeHost | Deployment desired state |
| `NodeStatus` | incarnation、`NodeFeatureReport`、last_seen、NodeManagementEndpoint、Runtime endpoint discovery 与 node-scoped liveness | NodeDaemon observation |
| `RuntimeHostStatus` | RuntimeHostEpoch、本地进程、Domain、binding、readiness 与 RuntimeFailureFact | RuntimeHost |

不同故障域必须使用不同代次：

| 代次 | 变化条件 | 防止什么 |
| --- | --- | --- |
| `NodeIncarnation` | NodeDaemon bootstrap 或重新取得 current registration tenure | 旧 Node facts/回复冒充当前事实 |
| `RuntimeHostEpoch` | RuntimeHost 重启 | 旧进程回调进入新宿主 |
| `DomainEpoch` | ExecutionDomain 创建或重建 | 旧线程/进程/IPC 的迟到结果落入新 Domain |
| `FabricSessionEpoch` | Zenoh Session 对象创建或重建 | 旧 session callback 晚到 |
| `BindingEpoch` | 逻辑 `PortBinding` install/reinstall/reconfigure/revoke | 同一 `BindingId` 的旧 binding 消息进入新连接 |
| `DeploymentRevision` | DeploymentController 生成新期望状态 | 两版 placement 相互覆盖 |
| `LeaseIssuerEpoch` | 本地 resource owner/issuer 重启 | 旧进程签发的 lease 在重启后复活 |

这些字段不能压缩成一个泛 `epoch`。NodeDaemon 每次 bootstrap 或重新取得 registration tenure 都生成新 NodeIncarnation；RuntimeHost 重启、Feature refresh、heartbeat gap、Zenoh disconnect/同 session auto-reconnect 都不改变它。所有 NodeDaemon facts 带 NodeId、NodeIncarnation、sequence/freshness，旧代次不得覆盖新代次。Zenoh 重连不是 Node 重启，RuntimeHost 重启也不等于 Deployment 发生变化。纯 DeckCompiler 只产生内嵌 canonical DeckTopology 的 DeckLock；纯 DeploymentPlanner 只产生 DeploymentPlanCandidate。二者都不安装 live binding，因此不递增 `BindingEpoch`；只有 DeploymentController commit 后才形成带 revision 的权威 `DeploymentPlan.bindings`。首版每条解析后的静态 1:1 Link 对应一个稳定 `BindingId`；`BindingEpoch` 只在同一 `BindingId` 内比较，不是全局身份，两个 binding 的 epoch 数值可以相同。

browser auth session、WebRTC peer、XR input stream 与未来 teleoperation session 同样不能压成一个泛 `ExternalSessionId`。它们使用 Gateway/teleoperation owner 限定的 ref 与 epoch，并且不能复用 NodeIncarnation、RuntimeHostEpoch、FabricSessionEpoch、BindingEpoch、AgentSession 或 DeviceSession。PeerConnection/ICE replacement 只推进对应 peer/stream generation，不自动产生 DeploymentRevision；长期 Gateway workload/exposure/config/placement 变化才属于 Deployment desired-state 候选。

同一 DeploymentRevision 与活动 BindingEpoch 内，一个 BindingId 恰有一条接收新 Message/frame 的 active route。route replacement 按 revision-tagged `prepare → activate → drain → retire` 执行，失败时显式 rollback；`activate` 原子切换新准入，旧 route 立即变为 drain-only。禁止 local+wire 双投、隐式 fallback 与 payload hash/time-window echo 去重。fan-out 为每个 destination 编译独立 BindingId。

只有实现 node-management 与 Inspection 协议的 MCU 才是 Node；普通 MCU、PLC 或外设是 `Device`/`ExternalTarget`，由 Driver 或 Gateway 接入。

## 5. 分布式契约基线

Kernel 不依赖 Zenoh，但首批稳定值必须允许跨故障域使用：

| 契约 | 解决的问题 |
| --- | --- |
| `NodeId`、`RuntimeHostId`、`DomainInstanceId`、`InstanceId`、`InvocationId` | 跨进程、执行边界和重启后的中性身份；DeckRunId/AgentRunId/EvalTrialId 由各领域定义 |
| 各层 incarnation/epoch/revision | 拒绝旧事实和旧控制权 |
| `MessageId`、`CausalityRef` | 去重、因果关联和诊断 |
| `Deadline`、`Freshness` | 拒绝延迟后已失效的数据或请求 |
| `SchemaId`、版本与兼容性结果 | 绑定前显式失败或降级 |
| `PrincipalRef`、`CapabilityGrant` | 谁可请求哪些资源操作 |
| `ControlledResourceRef`、`Operation` | 物理作用对象与动作 |
| `LeaseId`、`FencingToken` | 当前控制权与旧 owner 隔离 |
| `IdempotencyKey` | 不确定结果下查询和去重 |
| `Receipt` | 谁在何时接受、拒绝、执行或无法确认 |

`CapabilityScope` 是 `CapabilityGrant` 内的 `resource selector + operations` 值，不建立独立 owner；Grant 与 `ServiceContract`、`Node/Fabric/DeviceFeatureReport` 严格分离。`ResourceGroup` 只可作为选择器或展示分组，不能拥有 lease、Safety 或生命周期语义。完整术语见 [Capability、Service Contract 与 Feature Support](../concepts/capability-service-feature-boundaries.md)。

上述值跨语言时共享同一 Schema authority 与 canonical digest。Rust/Python/C++ binding 只是生成或手写的视图；任何 binding 都不得接受另一语言会拒绝的 unknown value、重算不同 digest，或把本地类型布局升级为 wire compatibility 承诺。首个独立跨语言 consumer 出现时必须提供双向 byte-level golden/error vectors。

## 6. 按状态选择一致性

| 状态或消息 | 首选语义 | 分区时 |
| --- | --- | --- |
| Observation / Signal | freshness、latest-wins、允许声明式丢弃 | 只用仍有效的本地数据 |
| WebRTC media / media sample | frame/sample age、codec/track generation、允许显式 drop | track unavailable/degraded；不自动产生或撤销物理成功，teleop 依显式 policy 收敛 |
| XR continuous input | peer/stream generation、sequence、frame/calibration、freshness、latest-wins | 旧/迟到输入拒绝；本地 deadman/lease 按策略到期，不等待 disconnect callback |
| Event | 生产者局部有序、消费者去重 | 有界语义 Mailbox 或显式 durable handoff；否则记录明确丢失窗口 |
| Query | deadline、cancellation、结果版本 | `unavailable`/`partial`/`stale` |
| 物理 Command | Authority、lease、fencing、deadline、idempotency、阶段 Receipt | 新远端写入默认拒绝；本地按资源策略继续、降级或停机 |
| Deployment desired state | 声明式 reconciliation | 冻结破坏性变更，恢复后比较 observed state |
| Inspection / Telemetry | 最终一致、带观测时间 | `stale`/`partitioned`，不显示 healthy |
| Evidence | 副作用故障域内 durable handoff，随后复制 | 保留本地权威记录，恢复后同步 |
| E-Stop / Safety inhibition | 本地硬路径或 fail-closed | 不依赖 Agent、云端或普通消息队列 |

“reliable transport”不等于物理效果 exactly-once。Command 超时或断连后可以是 `uncertain`；调用者只能查询副作用 owner 的权威状态，Fabric 不得透明重放。

## 7. Authority、Resource 与 Safety 闭环

三种判定必须分开：

```text
IdentityResolver
      ↓ PrincipalRef
AuthorityService             # 该主体是否有权请求该操作？
      ↓ CapabilityGrant + AuthorityDecision
ResourceCoordinator          # 当前是否持有该资源？
      ↓ LeaseGrant + FencingToken
SafetyIslandAdapter          # 独立 safety island 当前是否许可？
      ↓ SafetyDecision
Driver EnforcementPoint     # normal-command 最后软件准入
      ↓ setpoint
Safety island / device-native gate
      ↓ applied output
PhysicalEffect + staged Receipts
```

AuthorityService 与 ResourceCoordinator 首版可以同进程，但 API、状态、Decision/Receipt owner 必须分开。关键规则：

1. `CapabilityGrant` 只解决“可以请求”，Lease 才解决“当前谁控制”。
2. Fencing token 对每个受控资源单调递增，并由真实 Actuator/资源 owner 持久比较；只存在 Runtime 内存中不够。
3. ResourceCoordinator 是本地单写 lease/fence issuer；Driver EnforcementPoint 是 normal-command 的最终软件准入 owner；物理下游 safety island/设备原生 controller 是最终输出 gate。它们不是平级 writer；远端 Authority 只能授权 lease 请求，不能在分区中代替本地 issuer 签发写 lease。若没有这种下游仲裁或经证明的等价 device-native safety，H1 不得通过。
4. Lease 失效由资源 owner 的本地单调时钟判断；远端 monotonic timestamp 不能直接比较。远端请求只携带有界 duration，由 owner 换算并回执本地 expiry。
5. issuer/EnforcementPoint 重启后先推进相应 epoch、使旧 lease/command 失效，并在恢复 fencing/idempotency、stop/device completion 前 fail-closed；首版不跨重启恢复仍有效的旧 lease。device-native、driver-proxy 与 unsupported fencing 必须分别声明，后两者不能仅靠恢复 ledger 自动接管。
6. Idempotency 去重状态归副作用 owner；`accepted` 不等于 `succeeded`，`uncertain` 必须可查询。
7. 分区行为按资源显式声明为继续、降级或安全停止；它不是 Authority cache 的默认行为。
8. 本地 Authority/Resource/Safety/Enforcement 的正确性不依赖远端 Fabric 连通；普通应用 PortBinding 的生产传递仍统一走 Zenoh，硬安全与最低本地自治路径是独立、预先部署的执行链，不通过隐式 transport fallback 获得。

### 7.1 物理身份、控制模式与断网自治

完整 SpatialMap 可以后置，但物理链在 P3 前必须冻结可信 ObservationHeader：`DeviceRef/DeviceIncarnation/DeviceSessionEpoch`、`DriverBindingEpoch`、channel/sequence、measured/received time、`ClockDomainRef`/mapping revision/uncertainty、`FrameRef`/frame epoch/transform revision、单位/维度、CalibrationRef/CalibrationRevision、quality/validity/covariance、origin/environment/provenance。只有受信 Driver/Scenario boundary 可标记 physical origin；普通 PortBinding 只验证 transport metadata，不能提升 payload provenance。unknown uncertainty 不等于零。设备、Driver、frame/transform、calibration 或 mode 换代后，旧 binding/Observation/lease/command 即使迟到也不能生效。

物理写入的 `PhysicalCommandEnvelope/EnforcementContext` 还必须绑定 DeviceSessionEpoch、PhysicalAssemblyRevision、CalibrationRef/CalibrationRevision、SafetyEpoch、LeaseIssuerEpoch/LeaseId/FencingToken、deadline、idempotency key、command/requested-operation digest、AuthorityDecisionRef/digest、SafetyDecisionRef/digest，以及 Operation 声明要求的 PowerState/ThermalState/OperatingEnvelopeEvaluation ref、revision 和 freshness；real profile 还绑定 local `HardwareActivationRef/Epoch`。Decision 反向绑定 command/operation、resource、device/session、audience、epoch/expiry；Driver EnforcementPoint 在提交 setpoint 前原子比较当前事实，不能把同 policy revision 的其他 Decision 或较早许可复用。若 safety trip 与迟到 normal command 已越过该点发生竞态，物理下游 gate 必须保持或进入 safe output。

`Controller-role Card`（控制应用）只能经 Deployment 安装的 typed `OperationClient/CommandEndpoint` 提交静态 1:1、有界、无透明 retry 的 Command，不得直接持有 Driver/Actuator。Safety 负责 clamp/inhibit/fail-safe，不作为和 teleop/planner 竞争的最高数字优先级 writer。E-Stop、protective stop、deadman、limit、collision inhibit 与控制应用的普通 Stop 分开建模；前五者的 freshness、SafetyEpoch、锁存/reset、safe output 与硬件 ack 位于 MCU/PLC/设备原生 safety island，不要求 control lease。`SafetyIslandAdapter` 只接入许可和证据，普通 event loop、Agent 或远端 Fabric 失效时仍需收敛到产品定义的安全状态。

WebXR/remote teleop 只是在这条链之前增加一个不可信外部输入边界。Media/XR Gateway role 校验 Principal/session/stream、Schema、sequence、frame/calibration 和 freshness 后，将连续信号交给 Controller-role CardInstance 或 Teleoperation owner；它不能签发控制 Capability/Lease、调用 Driver 或把 transport ACK 变成 EffectReceipt。浏览器或 Gateway 断线时，disconnect callback 的 neutral/stop 是 best-effort 补充，本地 lease/deadman/fencing/Safety 必须独立到期和收敛。视频断流是否暂停控制由显式 TeleoperationPolicy/ODD 决定，不由 WebRTC transport 硬编码。

Control mode handoff 执行 request→stop/quiesce→observed-safe/neutral→release old lease→acquire new lease→activate new epoch；emergency 是 safety state，不是 control mode。CommandSequence 只在 resource + lease issuer/lease + ControlModeEpoch 内比较。Safety clamp 后 EffectReceipt 分别记录 requested、authorized/permitted、applied operation digest、SafetyEpoch、device-send/completion ack 与 observed-effect level；SDK 返回不能直接产生 Succeeded。

Deployment 还需把资源级 partition policy 编译成目标限定的 immutable `ContinuityProfile`：固定 Deployment/assembly/calibration/safety revision 与全部 artifact digest，约束最长断网时长、允许动作/资源、预先衰减的 offline Grant、时钟/Power/Thermal/Operating Envelope/Evidence 容量、安全停止/返航/人工接管与重连 reconciliation。本地 `ContinuityController` 只组合这些门槛，不接管事实 owner；主机重启失去 monotonic baseline 后默认不恢复自治。断网不延长云端授权，重连不 replay 旧 Command，恢复以当前物理观测为准。

RuntimePlanSlice 还携带 Profile ref/digest、触发 dependency predicate、debounce/hysteresis 与 applied revision。ContinuityController 在首次 loss 时创建 ContinuityEpisodeId/offline_since；短暂重连不结束 episode或刷新最大离线窗口，只有稳定连接和 reconciliation close Receipt 才结束。ContinuityController 重启无法恢复可信 episode 时 fail-closed。

Device/Driver binding/firmware-config/calibration/assembly/control-mode/safety 分别拥有自己的 owner 和 epoch/revision，不能折叠成一个 device epoch。Driver/adapter 拥有 observed firmware/ABI/config/session 与 raw power/thermal telemetry，Deployment 拥有 desired facts；DeviceService 只从同一输入 revision 原子派生 DeviceReadinessSnapshot 和 PowerState/ThermalState，产品域 `OperatingEnvelopeEvaluator` 独占产生绑定 ODD/World/state revision 的 OperatingEnvelopeEvaluation，safety island/Adapter 消费并拥有 inhibit/SafetyDecision，Continuity/Admission 只消费。真实 realization 另需 commissioning Receipt。P3 用 immutable ScenarioManifest/ScenarioRunner 隔离 world truth 与 Observation，只让 fake/sim Driver 执行可复用 conformance；P3–P5 的 write 证据只覆盖 simulation profile。P6b 后 H1 才让 real Driver/HIL 执行该 suite，并由 Release owner 基于 Device/Safety/Evidence/hazard 证据签发受限 HardwareEnablementReceipt。目标 Node 的 local HardwareActivationGate 验证 Receipt、RuntimePlanSlice 与当前 DeviceReadinessSnapshot 后安装 HardwareActivationRef/Epoch；Receipt 撤销/到期、readiness、ODD/Operating Envelope 或绑定 revision 变化时，本地推进 epoch 并 disarm/inhibit/stop，即使 Fabric 分区也不等待远端。

## 8. Placement 与 reconciliation

```text
DeckLock {canonical DeckTopology} + ServiceSpec
+ CardDefinition/Service ExecutionRequirements
+ Link DeliveryProfile + immutable Node facts + DeploymentProfile/policy
                              │
                              ▼
                    DeploymentPlanner (pure)
                              │ DeploymentPlanCandidate
                              ▼
                    DeploymentController
          atomic commit → DeploymentPlan @ DeploymentRevision
          owns rollout ledger
                              │ canonical target RuntimePlanSlice
                              │ + writer-context RuntimeApplyRequest
                              ▼
                         RuntimeHost
                              │ RuntimeAssemblyEngine
                              │ local prepare / ready / activate / drain / rollback
                              │ local observed facts / Receipt
                              ├──────────────────────────────┐
NodeDaemon ── Node observed facts / endpoint discovery ─────┤
FabricService ── binding observed facts ─────────────────────┘
                              │
                              ▼
                    DeploymentController reconcile
```

Placement 至少考虑 CPU/GPU/内存/设备、平台兼容性、TrustDomain、FailureDomain、数据驻留、本地控制延迟、Artifact digest、runtime kind、target triple/architecture、libc/CPU feature、worker protocol/runtime compatibility、协议版本、`ServiceRequirement`、`PermissionRequirement` 与目标 `FeatureReport`。这些 target facts 只描述兼容性，不把语言变成 Card/CoreService 身份。DeploymentPlanner 将 CardDefinition `In`/`Out` 生成的 `PortSpec`、Deck 的 `Card.Out → Card.In` `Link`/`DeliveryProfile`、DeckLock 解析结果、目标 Node facts 和 DeploymentProfile 编译进 `DeploymentPlanCandidate.plan_content.bindings`，包括 endpoint identity、Zenoh route/locality、Schema/codec、Fabric ingress limits、target admission boundary、keyspace 与 `FeatureMismatch`/`FeatureLoss` 结果；同时把 CardDefinition/Service 的内在 `ExecutionRequirements`、typed ServiceDependency、Link 的交付意图、目标 Node facts 和资源预算编译进 `DeploymentPlanCandidate.plan_content.execution`，包括 DomainAssignment、MailboxSpec、DispatchPolicy、Admission/OutstandingBudget、Executor/IPC/retained-byte budget、LivenessSpec、FailureContainmentSpec、RecoveryPolicy、readiness/activation group、consumer ingress/producer egress、dependency-loss/drain 和 RevisionTransition。DataLink 不自动成为启动依赖。DeploymentController 原子提交后，它们才成为同一 committed `DeploymentPlan` revision 的子树；不另建并行 `BindingPlan`。CardDefinition、Card 和 Deck 不得自行选择 transport、Zenoh key、线程、PID、Lane 或 Runtime ready queue。

Web Gateway placement 还需考虑媒体源与操作者距离、编码/GPU、NAT/TURN relay、数据驻留、TrustDomain、XR/control latency 和网络故障域。Deck 只声明 Port/Service/Permission/Feature 需求，不选择 WebRTC、TURN、P2P/SFU 或 Gateway placement。长期 managed Gateway workload、external exposure 和静态 policy 属于 Deployment desired-state 的目标方向；browser login、viewer、PeerConnection、ICE restart 和 XR stream 是 Gateway-owned ephemeral state，不为每个 peer 产生 DeploymentRevision。由于当前 RuntimeOwnershipTree 和非 Card Gateway endpoint binding 尚未冻结，实现前必须先完成 managed-workload/exposure ADR；externally service-managed Gateway 只能通过 ExternalWorkloadAdapter 请求/观察，ParaEGOX 不虚构其进程生命周期。

committed `DeploymentPlan.bindings` 和 `DeploymentPlan.execution` 分别是 desired binding 与 desired execution 的唯一权威。`runtime/contracts` 拥有 PlanProvenance/PlanWriterContext wire DTO、RuntimePlanSlice/RuntimeApplyRequest Schema 与 apply protocol，DeploymentController 拥有实际 canonical projection value。RuntimeSliceProjector 将 committed plan 的 scope/plan/revision/content 投影为 tenure-neutral Slice；RuntimeApplyEnvelopeBuilder 再映射 DeploymentWriterRef/DeploymentWriterEpoch/WriterTenureProof，并绑定 target、exact active target-slice digest、operation id、temporal constraint 和覆盖完整 request 的认证/完整性证明。PlanContentDigest 只覆盖 desired content，source plan digest 覆盖 committed header/content，target slice digest 覆盖 PlanProvenance/target assignment；三者均排除 writer tenure，request authentication 则覆盖 writer context 和完整 request。slice 不可独立编辑，RuntimeHost 不 import `deployment/` 或 `decks/`。RuntimeHost journal 分开 `writer_fence`、`prepared` 和 `active`，只有 exact target-slice CAS 与 source revision 单调性均通过时 activate 才替换 active；RuntimeHost/Fabric 回报的实际 binding/execution facts 与同一 source revision 不一致时，实例不得进入 Ready。远程 placement 只改变 owner 所在 Node，不在调用侧构造 `RemoteDomain`。

首版由单一 DeploymentController 完成 reconciliation。每个 deployment scope 必须只有一个明确的写 owner/revision；Inspection 只投影 desired/observed 差异，不修改它们。DeploymentController 由 OS service manager 拉起，不作为由同一目标 RuntimeHost 部署和管理的普通 CoreService。多控制器共识、leader election、自动跨 Site failover 与物理 workload 自动迁移在真实高可用需求出现前后置。

## 9. Inspection、Evidence 与时间

- `measured_at`：Observation 对应的物理时间，类型是不可互换的 Monotonic/Wall/Sim Instant 之一并绑定 ClockDomainRef。
- `received_at`：受信 Driver/Scenario source boundary 接收到/生成样本的本地时间；Fabric 可另记 transport_received_at，但不能冒充 measured_at 或 physical provenance。
- `reported_at`：Node 或服务发布事实的墙钟/HLC 时间。
- `observed_at`：Inspection projection 或其他明确 consumer 观察到该事实的时间。
- 本地 deadline、freshness 与 lease expiry：各 owner 的 monotonic 语义。

跨 clock domain 只能使用带 producer identity/epoch、measured-at/valid-until/source/uncertainty 的 ClockMapping 及其 ClockMappingRevision。Kernel 只拥有 Schema/纯规则；Node time-sync、Device/Driver 或 ScenarioRunner 分别拥有运行 mapping value，Consumer、Deployment、InspectionService 与 OpsService 不能自行改写。HLC 可以帮助排序和判断 happened-before，但不能替代 lease 的本地失效判定。跨节点墙钟漂移需要 NTP/PTP 与显式容差。Inspection 缺少事实时必须显示 unknown/stale，不能从最后快照推断健康。

Receipt/Evidence 是权威执行记录；Trace、Log、Metric 只用于关联与诊断。exporter 或 Collector 故障不得阻塞安全控制，也不得伪造 Evidence 已提交。

## 10. 首批 Harness

1. Node 启动中断连时，Deployment 不将其报告为 ready。
2. RuntimeHost 重启后，旧 RuntimeHostEpoch 的 callback、Receipt 和 Command 被隔离。
3. ThreadDomain/ProcessDomain 重建后，旧 DomainEpoch 或 InvocationId 的迟到结果不能改变新实例状态或产生副作用。
4. RuntimeHost/Fabric 先按 active `RuntimePlanSlice` 检查 PortBinding endpoint/session/epoch/Feature support 与 Domain、PID/TID、loop、capacity/epoch；DeploymentController/Inspection 将 reported observed facts 与 committed `DeploymentPlan.bindings/execution` 对账，任一不一致时都不得报告 ready。
5. Fabric ingress buffer、target Mailbox、inflight、executor/IPC 与 retained-byte 预算分别有界、可观测；Mailbox 仍有空间但 execution permit 已满时，工作不 dequeue/不创建 Task，系统不出现隐藏 backlog。
6. 纯 compile 不改变 `BindingEpoch`；短暂断线/同 Session 自动重连也不改变 epoch。Session 对象重建后旧 FabricSessionEpoch callback 被拒绝，逻辑 PortBinding install/reinstall/reconfigure/revoke 后旧 BindingEpoch 消息只在同一 `BindingId` 内被拒绝。
7. route replacement 在任一观测点最多一条 active route 接收新 Message，完成或显式 rollback 后无双投、implicit fallback、hash/time-window echo 去重和旧 route 泄漏。
8. Fabric 断开期间，本地 trusted Observation → Controller-role Card → CommandEndpoint → Authority → Lease → SafetyIslandAdapter → Driver EnforcementPoint → downstream safety gate → Actuator → Receipt 行为保持声明。
9. 两个控制者竞争资源时，较旧 fencing token 无法执行；执行 owner 重启后规则仍成立。
10. Lease 到期与跨节点时钟漂移不会造成双 owner；issuer/EnforcementPoint 重启后旧 LeaseIssuerEpoch/driver/device epoch 的 Command 仍被拒绝。
11. Schema/ServiceContract/Feature 不兼容或 Permission 无法签发时，在 binding/deployment 阶段分别失败。
12. Evidence 远端不可用时，本地 durable handoff 和高风险 fail-closed 可验证。
13. P5 为失联 Node 产生带 source epoch/freshness 的 partitioned/stale Inspection fact，而不是 healthy；P6b/P7 的 federated Inspection/OpsClient/TUI 只能忠实展示该状态。
14. 单进程测试在没有 Zenoh、数据库和云服务时仍可运行；使用 PortBinding test fixture，但公共契约与生产路径不新增测试专有类型。
15. Command A 的 Authority/Safety Decision 不能用于 Command B；Safety clamp 后 requested/applied digest、device ack/completion 与 observed effect 分开。
16. wedge/kill RuntimeHost 后，独立模拟 safety process/clock/deadman 仍能直达 safe output；同 loop mock 不算证据。
17. observed/desired firmware/ABI/session/assembly/calibration 任一换代使 DeviceReadinessSnapshot 失效，跨 session facts 不能拼成 Ready。
18. ContinuityEpisode 在接近上限时反复短暂重连不能刷新 offline_since；ContinuityController restart 无可信 episode 时 fail-closed。
19. Safety trip 与迟到 normal command、Driver restart 竞争时，下游 safe output 持续锁存到受权 reset/rearm；Power/Thermal/ODD fact 过期或换代后旧 Decision/Command 被拒绝。
20. P3–P5 real actuator write 必须拒绝；H1 Receipt 或 local activation 缺失时拒绝。H1 激活后即使切断 Fabric，Receipt 到期/撤销或 readiness/revision/ODD 变化仍使 HardwareActivationEpoch 本地推进、设备 disarm 并拒绝旧 Command。
21. Console/Media/XR Gateway 任一崩溃时，本地 RuntimeHost、Safety 与最低自治仍保持声明；Console cache 与 media track 明确显示 unavailable/stale，不沿用健康状态。
22. WebRTC peer 或 XR input stream replacement 推进 Gateway-owned generation，旧 sequence/callback 被拒绝；它不改变 FabricSessionEpoch/BindingEpoch，也不为每个 peer 创建 DeploymentRevision。
23. 同一控制输入的 WebSocket/DataChannel 外部 route 不能双活；ICE/TURN failure、video loss、Gateway restart 和 packet loss/reorder 不形成隐式 MJPEG/raw Zenoh fallback。
24. 即使最后一个 neutral/stop packet 丢失，本地 deadman/lease/fencing/Safety 仍收敛；DataChannel ACK、Gateway accepted 与 media connected 都不能冒充 EffectReceipt。
25. 同一 ControlRequestId/digest 在 OpsService 重启和 client retry 后仍幂等；同 ID/不同 digest 被拒绝，timeout 进入 `Uncertain` 并先查询真实 owner，不能透明重放。
26. kill OpsService 或 federated Inspection role 后，DeploymentController reconcile、RuntimeHost、node-local Inspection、Continuity 与 Safety 继续；恢复后 projection 通过 source revision/epoch/cursor 重建而不改写 source facts。
27. deploy 类 ControlRequest 只能经 OpsService → DeploymentController；ConsoleGateway、TUI/CLI 和 OpsService 都不能直接写 plan store 或调用 RuntimeApplyEndpoint，terminal OpsReceipt 可追溯到 owner Receipt/EvidenceRef。
28. 相同 canonical contract vectors 在 Rust encode→Python decode 与 Python encode→Rust decode 下得到相同值、digest、unknown-field/version 结果；malformed、oversized 和 incompatible protocol 在产生 Runtime 副作用前失败。
29. Python worker 阻塞、crash、忽略 cancellation、半写 IPC、产生后代进程或在旧 generation 迟到返回时，Rust RuntimeHost 仍响应；旧结果被 fencing，process tree、FD/socket/IPC/SHM/workspace/retained bytes 最终清零，未知 effect 不被伪造为 Failed 或 Cancelled。
30. architecture/API scan 证明没有第三方稳定 Rust `dylib + trait object` Card ABI、默认嵌入 CPython Runtime 路径或跨 ProcessDomain 传递语言私有对象；trusted in-process Rust 与 polyglot worker 仍由同一 RuntimePlanSlice 和 Runtime ownership tree 驱动。

S6 已为第 3 条的 ProcessDomain generation/invocation fencing、第 28 条的 PXTE/PXAR/PXWP Rust/Python wire compatibility，以及第 29 条中的 crash/block/partial IPC/same-process-group descendant/workspace cleanup 提供 local reference evidence；socket/SHM、真实 OOM、host SIGKILL 后独立 process-group orphan、production resolver/containment 和完整 plan-driven assembly 仍未证明，因此本表不能整体勾选。

性能基准区分 PortBinding test fixture、Zenoh `session-local`、`host-local` 与 `remote`，至少记录 p50/p95/p99/p99.9/max latency、jitter、async-runtime/control-tick lag、queue wait/message age/items/bytes、drop/reject/evict/expire、copy/serialization cost、Card invocation duration、实际 thread/process/native pool 数、CPU、RSS slope、FD/SHM、重连、重启/隔离和 cleanup/shutdown 时间。每份证据还固定 Rust toolchain/target triple/libc/CPU feature、Python worker runtime/protocol、依赖版本和 Artifact digest。只有该目标硬件证据证明 `session-local` 不满足 SLO，才启动互斥同进程 production route 的 ADR；Zenoh Region 只作为具体实验配置维度，不成为 ParaEGOX 业务身份。

## 11. 实施顺序

```text
P0 术语、边界、Accepted ADR-0006 与 Rust/Cargo + Python/uv 工程基线
P1 Kernel IDs / time / message / command / receipt
P2a bounded Mailbox / PortBinding test fixture
P2b LoopDomain / Dispatcher
P2c ThreadDomain / ExecutorBudget
P2d ProcessDomain / Liveness / Recovery / external watchdog（S6 local POSIX baseline 已落地；production containment/journal 未完成）
P2e deterministic DeckCompiler→DeckLock + 最小单写 DeploymentController + RuntimeAssemblyEngine：Deck multigraph/SCC、ServiceDependency DAG、activation contract/digest、Canvas state 排除、Planner 无独立 topology 输入、candidate atomic commit / tenure authority / Slice projector + apply builder / local prepare-activate-drain-rollback / reconcile-once
P3 本地仿真物理闭环：Authority + lease/fencing + SafetyIslandAdapter + ScenarioRunner/actuator
P4 Zenoh Fabric：session-local / host-local 实测，remote route 实现与 conformance
P5 Node + 持续 Deployment reconciliation：双主机 remote、分区、重连、DeploymentController restart、partial apply、旧代次
P6a local Evidence/node-local Inspection → P6b replication/federated Inspection/OpsService
P7 TUI
P8 ROS2Gateway
P9 SpatialMap / Semantic Navigation
H1 conditional Hardware Enablement after P6b
```

这不是要求每阶段一次性冻结全部 Schema，而是要求每个新故障域先有 owner、失败词汇和可运行 Harness。详细任务见 [Kernel Foundation 实施计划](../plans/kernel-foundation.md)。

当前下一阶段为 P2e；其 public Deck/Graph/persistence/Deployment owner 变更必须先通过 Accepted 或明确授权的架构决策门。

Operator & Web Interaction 是 P3 后的独立 workstream，不重排上述 Foundation 编号：P6a 后可做 local read-only Console，P5+P6b 后增加 federated Inspection/OpsService；P4 和高带宽 payload ownership 后做 view-only WebRTC media，再做 WebXR view-only；P3/P6a 后只允许 simulated teleoperation，真实执行器必须等待具体设备 H1。详细切片见 [专项研究](../research/web-console-webrtc-webxr-gateway-boundaries.md)。

## 12. Proposed ADR 入口

进入对应代码前，应分别评审：

1. Node/RuntimeHost 身份、各层 epoch 与 desired/observed owner。
2. 禁止裸作用域名，以及 SiteRef 的引入触发条件。
3. RuntimeHost、本地实例、跨 Node typed service client/permission-bound handle 与 external workload manager 的生命周期所有权。
4. Authority、Resource lease、fencing、Safety 与 Receipt 的执行语义。
5. Zenoh-native Fabric 边界、keyspace/schema 与连接恢复。
6. Evidence 本地提交、复制和恢复语义。
7. CardDefinition `In`/`Out`/`PortSpec`、`Card.Out → Card.In` Link `DeliveryProfile`、`DeploymentPlan.bindings`、Runtime `PortBinding`、ExecutionRequirements、`DeploymentPlan.execution` 和 observed facts 的分层 owner 与一致性。
8. Web Gateway managed-workload、external workload manager、external exposure/internal typed endpoint 与 DeploymentPlan/Runtime observed state 的 owner；不预设公共 `GatewayInstance`。
9. browser auth session、WebRTC peer、XR input stream 与未来 teleoperation session 的限定 identity/epoch、Principal 映射、single-active ingress、reconnect fencing 和断线收敛。

Deployment/Runtime 的 owner 与依赖边界已由 [ADR-0001](../adr/ADR-0001-deployment-controller-boundary.md) 接受，核心实现语言与多语言边界已由 [ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 接受；其余条目仍需分别评审。当前可以声明已注册 PXTE/PXAR/PXWP contract 的 Rust/Python byte-level conformance 和 local POSIX reference-worker interoperability；不得外推为 C++/任意 workload 兼容、Rust production core、生产级一致性、容错或集群能力。
