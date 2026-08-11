# Testing

本目录描述 ParaEGOX 的系统声明由什么验证证据支持，包括测试策略、场景矩阵、确定性 Harness 和人工验收边界。

## 证据层次

| 层次 | 证明什么 |
| --- | --- |
| Unit | 纯契约、状态机、策略和边界条件 |
| Component | RuntimeHost、Core Service、Driver 或 Execution Domain 的独立行为 |
| Integration | Service client/dependency、Fabric、进程和存储之间的连接 |
| Scenario | 一段可复现的机器人或 Agent 行为闭环 |
| System | 启动、降级、恢复、关闭以及 Inspection/OpsService 解释与操作能力 |
| Field | 真实设备、真实网络和长期运行证据 |

每个测试策略必须建立“声明 → 测试入口 → 期望证据”的映射。测试数量、覆盖率或一份 checklist 本身不证明系统行为正确。

首批重点是架构基线中定义的契约、并发与故障 Harness：Port direction/interaction/cardinality/Schema 不兼容 fail-fast，多 CardInstance 的私有实现对象与 BindingId 隔离，PortBinding test fixture 和 Zenoh `session-local`/`host-local`/`remote` conformance，pre-validation Fabric ingress frame 与 validated Message/target Mailbox 的边界，enqueue/remote admission/Card invocation completion/effect 结果分阶段，以及阻塞、线程卡死、进程终止、背压、重连、取消、排空、竞态和安全控制面隔离。

## Rust-first 核心与多语言合同验证

[ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md)选择 Rust-first mechanisms，但不允许语言选择替代系统证据。首个 Rust/Python 纵向切片至少建立以下映射：

| 声明 | 必需证据 | 不能外推 |
| --- | --- | --- |
| Rust core 可独立构建 | 不安装 Python workload、Zenoh、ROS2、模型或硬件依赖时，最小 Kernel/Runtime contract slice 的 Cargo build/test | 已实现完整 RuntimeHost 或支持目标硬件 |
| 公共合同语言中立 | Rust encode→Python decode、Python encode→Rust decode 的 byte-level golden/error vectors；digest、unknown-field、version 与 reason code 一致 | 某个生成 binding 或语言对象是协议真相 |
| RuntimeHost 是唯一运行 owner | Rust RuntimeHost 创建 Python/C++ worker，验证 startup/readiness、持续 heartbeat、bounded credit、deadline、cancellation intent、generation 和 terminal result | worker 可拥有第二套 Runtime、Mailbox、readiness 或 restart policy |
| 进程隔离真实有效 | worker block/crash/ignore cancellation/spawn child、IPC 半写、旧代迟到时，host 仍响应并完成 process-tree、FD、socket、IPC、SHM、workspace 与 retained-byte 清理 | thread cancellation、future drop 或 process exit 证明外部 effect 已取消 |
| in-process Rust 边界受限 | architecture/API scan 只允许同构建、受信、静态关联的内部实现；不存在承诺第三方稳定性的 `dylib + trait object` Card ABI | safe Rust 自动提供插件隔离、硬实时或功能安全 |
| 大 payload 生命周期有界 | Blob/Buffer/SHM handle 的 lease/generation、bytes accounting、consumer crash、迟到访问和恰好一次 release Harness | 裸指针、借用对象或语言私有引用可跨边界 |

Rust 侧的 unit/property/concurrency/fuzz 测试、Python 侧的 unit/contract 测试和跨语言 system Harness 是互补证据。Cargo 是 Rust 构建与测试的权威，`uv` 是 Python 环境与测试的权威；一个生态的测试通过不能代替另一个，也不能用两份 lock 表示同一依赖真相。当前 Cargo workspace、pinned toolchain、Cargo/uv lock 与 CI 已存在，Cargo/uv/governance 命令都是实际强制门禁；某一阶段通过仍不能外推尚未实现的 Domain、目标硬件或 system 能力。

## Card 独立开发、Harness 与运行探测验证

[Card 独立开发、测试 Harness 与运行探测边界研究](../research/card-independent-development-testing-and-probe-boundaries.md)要求把“快速测试”“Runtime 组件验证”和“正式运行链路”分开：

| 层 | 入口 | 必须证明 | 不能外推 |
| --- | --- | --- | --- |
| L0 Unit | CardDefinition/config/Port/entrypoint 纯检查 | 确定性合同、无副作用 fail-fast | Card 可运行或 Ready |
| L1 Unit | 两个语言原生私有实现对象 + virtual clock/typed fake/recording handles | callback 领域逻辑、取消和实例状态隔离 | Runtime、ProcessDomain、Deployment 或 production 等价 |
| L2 Component | production projector/builder 产生 canonical RuntimeApplyRequest，Runtime crate-private Harness 创建单一 subject CardInstance | P2b–P2d 的 callback/Domain seam、binding、epoch fencing、故障与分阶段清理；P2e 才把同一 seam 接入 public apply/assembly | idle executable 消费 apply、DeploymentController tenure/journal 或 production Ready 已验证 |
| L3 System | 显式 one-subject Card Deck | compile→plan→commit→apply→observe→deactivate 正式链路 | 多 Node、真实设备或现场安全 |
| L4 Scenario/Field | simulation → HIL → real profile | owner-specific Receipt、安全 gate 与目标设备/ODD 证据 | 其他设备或 profile 自动成立 |

单主体 Card Harness 不承诺物理上只有一个 CardInstance。L2 只能在已安装 test PortBinding 边界使用非 Card 的 source/sink adapter；若 fixture 需要 Card 语义，必须在 L3 编译前进入 ephemeral Deck/committed plan/Slice，并由 RuntimeHost 正常创建，禁止投影后追加。L2 canonical request 只能来自 production Slice projector/builder，不允许长期手写 Slice，也不能进入 production route/config/provider registry。test package 不得被 production import，test principal/proof 不得被 production trust 接受。

未来的便捷入口必须先导出 canonical one-subject DeckSpec。metamorphic test 只在相同 source scope/revision、resolver inputs、previous allocation、target facts、policy 与 committed provenance 下比较 DeckLock、PlanContentDigest 和 Slice digest；两个独立 deployment/commit 的 revision-bound Slice digest 不要求相同。

P2b 当前覆盖 Rust in-process create/start/callback/drain/stop seam、startup failure、callback deadline、panic/timeout、late old-generation output，以及 Task/Mailbox/permit/retained-payload exact-zero。P2c 再覆盖 ThreadDomain queue/admission/execution、stuck call、late result、poison 与 thread budget；P2d 再覆盖 Python/Rust worker 相同合同投影、process crash、recovery budget/quarantine 和 process-tree/FD/socket/IPC/SHM/workspace 清理。P2e 才覆盖 `prepare → readiness → activate → drain/rollback`、重复 apply、same-id/different-digest 与 CAS conflict。纯 reducer/deadline 优先使用 virtual monotonic clock；thread/process wedge、OS watchdog 与清理使用真实 monotonic time。

### S4/P2b 当前证据与限制（2026-07-30）

- canonical PXTE/PXAR v2 由 Rust contracts、Deployment production builder 和独立 Python fixture 双向校验；提交 `451f5e2` 全仓通过 212 个 Rust unit、1 个 compile-fail doctest 和 63 个 Python test，GitHub CI `30534371196` 成功。
- deterministic scheduler-tick direct/hierarchical A/B 证明 reference hierarchical dispatcher 的尾部延迟/公平性收益；本机 current-thread 真实单调时钟诊断以 8,192 次 Stream/Background offer 对 4,096 次 dispatch start 形成精确 2× 压力，采集 1,024 个 Control enqueue→callback-start 样本，background 最大服务间隔不超过 64，结束后可跟踪资源归零。
- 上述观测固定为 `local/diagnostic`：不是 target-platform、并发 L2 ingress、post-idle wakeup、硬实时或功能安全证据。Control start bound 还依赖 producer 遵守 signed arrival envelope；Runtime 尚无 sliding-window violation observer。
- `paraegox-runtime-host` 可启动并经 Ctrl-C 正常退出，但 binary 保持 idle、不消费 apply。P2b 只证明 startup 不等于 Ready；exact-revision readiness、dependency/resource/permission 合取和 activation truth table 归 P2e。

运行探测不能只断言 `ok`：

| 信号 | 最小测试 |
| --- | --- |
| startup completion | S4 已证明当前 generation callback 在 deadline 内完成，失败/timeout 不会被标为 started；由于还没有 Ready surface，这不能外推为 exact-revision Ready |
| RuntimeHost liveness | NodeDaemon 在不同故障域观测并发出恢复请求，OS service manager 独占进程 TERM/KILL/restart；profile 指定唯一 restart-budget/quarantine mutation owner，同一 async runtime/executor 的 heartbeat 不能证明自身 progress |
| Domain/CardInstance liveness | heartbeat/invocation progress/run bound 绑定 DomainEpoch，旧代次拒绝 |
| readiness | exact revision 的 artifact/config/domain/resource/dependency/binding/startup/permission facts 合取；任一 stale/mismatch 为 NotReady/Unknown |
| health | degraded/recovered/freshness truth table；可 degraded 但 Ready，也可 Live 但 NotReady |
| test observation | 固定 local/diagnostic evidence level，不升级为 live/system admission |

通用 L0–L3 Harness 强制 effect-denied，且没有 simulation/HIL/real 配置开关：不签发真实设备 Grant、Lease、SafetyDecision 或 HardwareActivationRef。simulation Receipt 不能满足 H1；会移动、加热、写入、reset 或 calibrate 设备的检查必须作为独立 L4/H1 受权诊断 Operation，通过 Authority → Lease/Fence → Safety → Enforcement，并同时绑定有效 HardwareEnablementReceipt、HardwareActivationRef/Epoch、fresh DeviceReadiness/ODD、下游 safety gate 与最终 device/effect evidence。

## Deck 与 Product Application 边界验证

[Application、Deck、Card 与 Service 边界研究](../research/application-deck-card-service-boundaries.md)和 Proposed [ADR-0004](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md)要求首个 Deck/Deployment 切片至少证明：

| 声明 | 必需证据 |
| --- | --- |
| DeckLock 是唯一解析产物 | 相同 DeckSpec/resolver inputs 的 byte-stable golden/property test；Planner 没有独立 DeckTopology 输入 |
| DeckTopology 受 lock digest 覆盖 | Card key/ref/config/role/refinement、resolved Port、Link、DeliveryProfile 或 locked ref mutation 改变 digest；旁置 display metadata 不改变 DeckLock digest |
| Card key revision 语义明确 | previous-plan diff 中同 key 表示同一 desired slot；rename 是 remove + add；删除后复用 key 不恢复旧 CardInstance 或私有状态 |
| Canvas 不是运行真相 | 坐标、缩放、折叠变化不改变 DeckLock digest |
| Requirement 不锁 live provider | 同一 byte-identical DeckLock 在不同 immutable ServiceSpec inventory/target facts 下产生不同、可解释 candidate，DeckLock/digest 不变 |
| Deck 不拥有平台 CoreService | DeploymentController deactivate/replace Deck workload、旧 DeckRun terminal 后 Authority、Fabric、Inspection 和共享 Model service 继续 |
| 没有隐式 Application | P0/P2e 的 DTO/Receipt/权限中无无 owner `application_id`；UI 分组不触发 GC/权限继承/数据删除推迟到首个 TUI/Console 展示切片验证 |
| 首版不伪装应用私有持久状态 | CardInstance replacement 不继承未声明全局/文件状态；显式 `owner=application`/`lifetime=installation` 或等价字段返回稳定 reason code，opaque code 不做语义猜测，未声明 raw persistence/database/egress 由受限 ProcessDomain 拒绝并记录 fact |
| 首个 durable AgentSession 不偷渡安装状态 | 只在同一 DeckRun 内跨 CardInstance/进程重启恢复；Deck workload terminal 后 seal 并产生 retention/GC Receipt；新 DeckRun 不自动继承 |

未来 A0 Application gate 触发后，必须按实际触发条件选择故障 fixture：多 Deck 闭包使用至少两份独立 DeckLock 与 partial rollout/rollback；安装私有状态使用私有 namespace、迁移和 partial GC；多次安装或多 Artifact 闭包使用两个隔离安装或共同 release owner。只覆盖相关的重复请求、安装记录与 Deployment commit 间 crash、旧 writer/split-brain 等风险，不强迫每种场景同时具备所有对象；所有场景仍须证明只有 DeploymentController 能提交 DeploymentRevision/apply Runtime，涉及共享 provider/私有 namespace 时才额外证明卸载 A 不停止 provider、不删除 B，且只能按 A 的 Receipt 处理 A namespace。

## Graph、Activation 与 Runtime Assembly 验证面

[Graph Foundation、领域图与执行边界研究](../research/graph-foundation-and-domain-execution-boundaries.md)要求先证明领域语义，再条件抽取纯算法。P2e 至少建立以下映射：

| 声明 | 注入或检查 | 通过证据 |
| --- | --- | --- |
| DeckTopology 是 directed multigraph | 同一 Card pair 多 Port Link、自环、重复 edge-key fixtures | parallel edge 保留，非法重复稳定拒绝 |
| 纯算法结果确定 | 随机打乱 node/edge 插入顺序 | SCC、cycle witness、topological batches 与 diagnostics 不变 |
| Deck 与 Service 环策略不同 | ServiceDependency 的直接环/长环/自环，以及无 feedback contract 的 Deck SCC | 两类 reason/owner 分开，且都在副作用前 fail-fast |
| DataLink 不是启动依赖 | `A.Out → B.In` 与正反向 ServiceRequirement 组合 | 实际 activation 只服从 typed gate，consumer ingress Ready 后才开放 producer egress |
| activation contract 是 desired truth 的一部分 | 逐一修改 typed dependency、readiness timeout/failure、activation group/barrier、consumer ingress、producer egress、dependency-loss、drain deadline/order、fencing、rollback boundary 与 collateral scope | PlanContentDigest 与所有受影响 target slice digest 必须变化；未受影响 target 保持稳定 |
| Runtime 不重算 Deck/Deployment | 不安装/import `decks`/`deployment` | RuntimeAssemblyEngine 只消费序列化 Slice 完成本地 apply |
| prepare 与 active 分离 | Artifact stage、instance create、readiness、activate 各点 crash | 旧 active 保留或 bounded rollback/quarantine，不出现 mixed revision |
| apply 幂等且 fenced | 重复 operation、同 ID 不同 digest、旧 writer/revision、exact-slice CAS conflict、迟到 callback | 同请求同结果，冲突拒绝，active target-slice 精确匹配、revision 单调且 Binding single-active |
| dependency loss 有行为 | provider crash/stale/epoch change/recover | 严格执行编译后的 degrade/stop/rebind/restart/fail-closed，旧 client/epoch 不复活 |
| assembly 不进入 hot path | steady Signal/Command/stream 压力与调用路径检查 | Message 只走 PortBinding/Mailbox/ExecutionDomain；无中央 graph loop |
| Foundation 不吸收领域语义 | architecture/import/API scan | 至少两个独立生产消费者；无领域 import、I/O、loader、serialization/digest、state、retry/checkpoint/Receipt 或 metadata escape hatch |

首版 cyclic Deck 只验证稳定拒绝与 cycle witness。只有后继 ADR 冻结 feedback/delay/seed/latest-value/backpressure 合同后，才能新增允许环路的 liveness、bounded buffer、初始条件、deadlock 和 shutdown Harness。

## Operator/Web Gateway 后续验证面

[Web Console、WebRTC、WebXR 与交互式 Gateway 边界研究](../research/web-console-webrtc-webxr-gateway-boundaries.md)定义了独立于 Kernel Foundation 完成门槛的浏览器验证面。进入对应切片后，至少建立以下“声明 → 证据”映射：

| 声明 | 必需证据 |
| --- | --- |
| InspectionService 不拥有 source truth | producer fact 与 node-local/federated projection 分离；source revision/epoch/freshness、watch gap、慢消费者、partition、restart/resync |
| OpsService 只拥有 ControlRequest | same-id/same-digest 幂等、same-id/different-digest conflict、timeout→Uncertain→query/reconcile、terminal OpsReceipt 引用 actual-owner Receipt |
| OPS 故障不停止系统 owner | kill OpsService/ConsoleGateway/federated Inspection 后 DeploymentController reconcile、RuntimeHost、node-local Inspection、Continuity 与 Safety 继续 |
| Console 不拥有系统真值 | 仅用公开 InspectionClient/OpsClient 的 contract test；读取只走 InspectionProtocol，写入只走 OpsProtocol，Gateway cache 不恢复 owner state |
| WebRTC 只是 Gateway 外部腿 | simulated `MediaSample`/`EncodedVideoSample` → typed seam → WebRTC → 真实浏览器；无 raw Zenoh/browser direct path |
| WebXR 不是 Transport | secure-context browser test；XR payload 的 session/stream generation、sequence、time、Frame/Calibration/uncertainty/freshness admission |
| 外部输入 single-active | WebSocket/DataChannel 切换推进 Gateway-owned stream generation；双活、旧 generation、迟到/重复输入均被拒绝 |
| 断线不依赖最后一个 stop packet | 丢弃 disconnect neutral/stop，目标 Node 的 local lease/deadman/fencing/Safety 仍按 profile 收敛 |
| Transport success 不是物理成功 | DataChannel ACK、Gateway accepted、media connected 均不能生成 EffectReceipt；模拟执行器 owner 独占 terminal effect 证据 |
| Gateway lifecycle 有界 | ICE/TURN failure、codec worker wedge、Gateway kill/restart/rollout 后 PeerConnection、Task、Thread、Process、FD、socket、SHM 和 retained payload 全部回收 |
| 媒体不会饿死控制 | media/log/XR 联合洪峰下，所有 ingress/codec/peer buffers 有界，discrete control latency 和 deadman 仍满足声明 |

Gateway/Browser 测试分层包括 unit、contract、component、integration、scenario、真实 browser system smoke 和 target-device benchmark。至少覆盖 direct ICE、TURN-only、packet loss/reorder、bandwidth drop、视频断流、peer replacement、Gateway crash、Fabric/OpsService/federated Inspection partition、旧 peer/XR stream generation 和 reconnect fencing。H1 之前的 browser/仿真结果不能外推为真实遥操作安全证据。

Python Web/API、WebRTC/codec 和 browser runner 使用明确的 `uv` 可选依赖组；Rust Kernel/Runtime 的最小 Cargo 测试环境不安装这些 Python 依赖也必须继续通过。若 Gateway 后续使用 Rust，其依赖和测试由 Cargo profile/feature 管理，不得写回 Python lock；跨语言 Gateway contract 继续复用同一 Schema/golden suite。
