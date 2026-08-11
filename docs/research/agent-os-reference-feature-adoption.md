# Agent OS 参考特性采纳矩阵

> 状态：Research Complete，结论为 `revise`
> 日期：2026-07-29
> 深度：Deep
> 范围：公开 Agent OS、具身 Agent OS、机器人 Runtime 与相关论文中，可影响 ParaEGOX P0–P9 的可验证特性
> 实现状态：本文是研究与计划输入，不表示相关能力已经实现
> 关联文档：[Kernel Foundation 实施计划](../plans/kernel-foundation.md)、[Runtime 执行模型、调度与恢复](execution-model-scheduling-and-recovery.md)、[Kernel 消息机制与 Fabric 边界](kernel-messaging-fabric-evidence-security.md)、[分布式具身 Agent OS 缺口研究](distributed-embodied-agent-os-gap-analysis.md)

> 后续裁决（2026-07-29）：[ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 已接受“Rust-first mechanisms，polyglot workloads”。本文的项目采纳矩阵继续按机制、owner 与 failure evidence 切片；参考项目使用 Rust、Python 或其他语言，不是采纳或拒绝该特性的充分条件。

## 一句话结论

ParaEGOX 不选择某个 Agent OS 作为母版，也不迁移其整套 Kernel、Runtime、Module、Bus、Graph、VFS 或产品词汇；它按 owner 拆取可验证特性：P0–P2 吸收可执行架构不变量、精确副作用语义、分层预算、有界 Mailbox、公平调度、代际判活、进程树清理、恢复与 Deployment reconciliation；P1 先冻结 physical ABI，P3 再实现 Permit/Lease/Safety/Receipt/Verifier；AgentSession、Memory、ROS2、Ops 与受控 Skill 演进分别留在后续领域 owner。研究项目中的全局单例、无界队列、双生产 Bus、可绕过安全 wrapper、自动 Skill 晋升和 Kernel 万能 Graph 只作为反例。

## 1. 研究问题与成功标准

本文不回答“哪个项目最好”，而回答：

1. 每个参考项目中，哪些具体特性解决了 ParaEGOX 已知的失败路径。
2. 特性应由 Kernel contract、RuntimeHost、Deployment、Fabric、Physical、Agent、Memory 还是 Ops owner 承接。
3. 哪些特性现在实现，哪些只冻结 seam，哪些后置，哪些明确拒绝。
4. 吸收特性时如何避免同时带回原项目的命名、隐式 owner、第二真相或不适合物理系统的恢复语义。

成功标准是每项候选都有：

- 明确来源与可检查证据；
- ParaEGOX 内唯一 owner；
- 对应阶段和首个验证入口；
- 采用、改造、后置或拒绝结论；
- 不照搬的限制。

## 2. 范围、方法与证据强度

### 2.1 研究范围

研究基线是 2026-07-24 固定的公开生态快照，包含 30 个外部代码仓库和 13 篇论文。对高价值项目检查了入口、核心执行路径、状态与生命周期、安全/权限、存储与恢复、分布式边界和测试资产；README 主张不自动视为运行时保证。

本文主要使用以下固定 revision：

| 项目 | Revision | 主要证据用途 |
| --- | --- | --- |
| [Agent libOS](https://github.com/yingqi-z20/Agent-libOS/tree/e35d8eac1906) | `e35d8eac1906` | invariant、授权事务、effect binding、unknown/reconcile |
| [Microsoft Agent Governance Toolkit](https://github.com/microsoft/agent-governance-toolkit/tree/ff23ddcd76c7) | `ff23ddcd76c7` | canonical action、policy obligation、sandbox/fail-closed |
| [ROSClaw](https://github.com/ros-claw/rosclaw/tree/3356a1cf564b) | `3356a1cf564b` | physical Permit、Receipt、E-stop、worker generation |
| [Rivet agentOS](https://github.com/rivet-dev/agent-os/tree/ef88310f0576) | `ef88310f0576` | bounded Runtime、资源账本、公平性、task census、sandbox |
| [Dora](https://github.com/dora-rs/dora/tree/117dfd63d583) | `117dfd63d583` | bounded channel、Zenoh/SHM、restart、reconnect、record/replay |
| [DimOS](https://github.com/dimensionalOS/dimos/tree/1dbb6c27a0da) | `1dbb6c27a0da` | process-tree watchdog、forkserver、invocation token、异步 physical effect 反例 |
| [RoboNeuron](https://github.com/guanweifan/RoboNeuron/tree/5b792d59b8e8) | `5b792d59b8e8` | typed physical action lowering 与 Core/Edge 边界 |
| [SkillOS Robot](https://github.com/EvolvingAgentsLabs/skillos_robot/tree/19ef52d8e4c7) | `19ef52d8e4c7` | 慢语义环、快反应环、MCU fail-safe |
| [PhyAgentOS](https://github.com/PhyAgentOS/PhyAgentOS/tree/e68f399b3536) | `e68f399b3536` | Session、preflight、physical compatibility、Verifier |
| [Google AX](https://github.com/google/ax/tree/cbd2c564376d) | `cbd2c564376d` | Agent Harness、event stream、resume 与 single-writer 反例 |
| [Automatika EMOS](https://github.com/automatika-robotics/emos/tree/31b82958e7a1) | `31b82958e7a1` | ROS2 lifecycle、preflight、Preparing/Running 分型 |
| [eMEM](https://github.com/automatika-robotics/emem/tree/82e3da61cf71) | `82e3da61cf71` | observation/episode/entity、时空索引与 provenance |
| [MoFA](https://github.com/mofa-org/mofa/tree/a83bc9720c67) | `a83bc9720c67` | admission/scheduling 候选与多 Bus、安全旁路反例 |
| [OM1](https://github.com/OpenMind/OM1/tree/b4f6df8f6e81) | `b4f6df8f6e81` | Input/Action/Background、mode lifecycle、Prometheus |
| [RoboClaw MINT](https://github.com/MINT-SJTU/RoboClaw/tree/e9b28f9a7bca) | `e9b28f9a7bca` | Driver manifest、binding、preflight 与诊断 session |
| [ROS MCP Server](https://github.com/robotmcp/ros-mcp-server/tree/f1c023bb5570) | `f1c023bb5570` | ROS adapter schema、错误映射与测试 Harness |
| [NASA ROSA](https://github.com/nasa-jpl/rosa/tree/e7e53754bed6) | `e7e53754bed6` | read-only ROS 查询与诊断 Agent |
| [OpenFang](https://github.com/RightNow-AI/openfang/tree/acf2587e46be) | `acf2587e46be` | WASM host-call choke point、安全配置与 Kernel 膨胀反例 |
| [TypeGo](https://arxiv.org/abs/2607.05482) | arXiv `2607.05482` | 多时间尺度 workload、资源模式、抢占和 bounded stop |

ASPIRE、ENPIRE、CaP-X、RoboOS、EMOS Habitat 等研究用于后置能力与反例；论文主张在没有可核验实现时不升级为当前基础建设要求。

### 2.2 证据分级

- **高**：机制位于强制运行路径，并有对应测试或故障用例。
- **中**：源码存在且路径可重建，但分布式、故障或实机验证不足。
- **低**：论文、README 或原型主张，仅用于问题定义和测试负载。

“采用特性”只表示 ParaEGOX 接受该行为需求或验证方法，不表示引入上游依赖、复制代码、接受上游名词或继承上游安全声明。

语言同样不是证据捷径。Rivet、Dora 的 Rust 实现是 Rust-first Runtime/Fabric 的重要 mechanism 参考，Agent libOS、DimOS 等 Python 实现仍是副作用、worker 与恢复语义的重要证据；ParaEGOX 只移植可独立证明的行为，不把 safe Rust 误当硬实时/功能安全证明，也不把历史 Python 缺陷误归因于语言本身。

## 3. P0–P2 当前基础建设应吸收的特性

### 3.1 Kernel contract、Authority 与副作用

| 特性 | 来源与证据 | ParaEGOX owner / 阶段 | 采用结论与首次验证 |
| --- | --- | --- | --- |
| 可执行架构不变量 | Agent libOS [invariants.yaml](https://github.com/yingqi-z20/Agent-libOS/blob/e35d8eac1906/tests/invariants.yaml)、Rivet architecture guards | P0 repository governance/testing | 建立 claim→test manifest；import boundary、隐藏 runtime、无界队列、越权进程创建和禁用术语由自动检查证明 |
| Generation、Epoch、Revision、Fencing 分离 | Rivet readiness、Dora reconnect、AX event/controller | P1 Kernel contracts；P2e Deployment/Runtime | 旧实例、旧 writer、旧 ack 和迟到 result 必须在副作用或 commit 前被拒绝；不同领域不复用一个万能 epoch |
| 外部副作用阶段化 | Agent libOS [external_effects.py](https://github.com/yingqi-z20/Agent-libOS/blob/e35d8eac1906/agent_libos/evidence/external_effects.py)、ROSClaw Receipt | P1 Receipt/effect contracts | 至少区分 Prepared、Dispatched、Committed、Failed、Uncertain、Reconciled；在每个 crash window 注入故障 |
| 精确 Decision-to-Effect 绑定 | Agent libOS [effect_binding.py](https://github.com/yingqi-z20/Agent-libOS/blob/e35d8eac1906/agent_libos/capability/effect_binding.py)、Governance Toolkit、ROSClaw | P1 Authority contracts；P3 enforcement | Grant/Approval/Decision 绑定 principal、operation、canonical arguments digest、target revision、policy/catalog revision、audience 和 expiry；参数漂移必须拒绝 |
| 有限次授权的原子预留与结算 | Agent libOS [transaction.py](https://github.com/yingqi-z20/Agent-libOS/blob/e35d8eac1906/agent_libos/capability/transaction.py) | P1 Authority/Admission | reserve→dispatch/commit→settle；只有证明未 dispatch 才能 restore，禁止 check-then-act |
| 资源预算先预留、后结算 | Agent libOS resource manager、Rivet accounting | P1 admission；P2c ExecutorBudget | 外部 provider 或本地 executor 在执行前预留上限，terminal/unknown 分别结算；并发竞争测试不能超卖 |
| Policy obligation fail-closed | Governance Toolkit、OpenFang 反例 | P1 Security contract；P2d enforcement | allow-with-obligation 只有在 obligation executor 可用且成功时生效；unknown tool、unknown schema、policy error 和 backend unavailable 默认拒绝 |
| Tool visibility 不等于 authority | Agent libOS、Governance Toolkit、MoFA 旁路反例 | P1 Agent protocol seam；Agent slice | ToolCatalog/ToolView 只决定模型可见性；每个 InvocationAttempt 在调用点重新授权，catalog 变化不静默扩权 |

Kernel 只保留稳定 value/decision/receipt contract；effect journal、policy state、审批队列、Artifact catalog 和物理执行状态仍由各自领域 owner 持久化，不能因这些特性重要就全部沉入 Kernel。

### 3.2 Runtime ownership、预算与调度

| 特性 | 来源与证据 | ParaEGOX owner / 阶段 | 采用结论与首次验证 |
| --- | --- | --- | --- |
| 一个 ExecutionDomain 一个明确 owner | Rivet Runtime 规则与 architecture guards | P0 rule；P2 RuntimeHost | RuntimeHost 独占 task/thread/process 创建；CardInstance 私有实现不得自行创建 loop、线程池、进程或 detached task |
| 分层资源账本 | Rivet [accounting.rs](https://github.com/rivet-dev/agent-os/blob/ef88310f0576/crates/runtime/src/accounting.rs) | P2c ExecutorBudget | `RuntimeHost → ExecutionDomain → CardInstance/Port` 分层原子预留并用 RAII/等价机制归还；子级不能突破父级总预算 |
| operations + bytes 二维公平性 | Rivet [fairness.rs](https://github.com/rivet-dev/agent-os/blob/ef88310f0576/crates/runtime/src/fairness.rs) | P2b Dispatcher；P2c ExecutorBudget | 按 CardInstance 与 Port/ResourceClass 分层 DRR 或等价算法，同时计操作数和字节；基准覆盖小消息洪峰与大消息饥饿 |
| `Accept / Defer / Reject` admission | MoFA scheduler | P2c Admission/ExecutorBudget | 永久不满足与暂时资源不足分开；evaluate 与 reserve 必须原子，不能复制上游 TOCTOU |
| 资源模式与抢占结果分型 | TypeGo、DimOS | P1 ExecutionRequirements seam；P2b Dispatcher | 表达 exclusive/shared、serial/parallel，以及 interrupt-and-return、replace-without-return；不以一个 priority 数字代替恢复语义 |
| 有界停止能力 | TypeGo | P1 contracts；P2d/P3 | 使用 StopMode、MaxStopLatency、SafeTerminalState 和 terminal Receipt 表达；cancel flag 或函数 return 不算已经停止 |
| Readiness 状态与 wake 提示分离 | Rivet [readiness.rs](https://github.com/rivet-dev/agent-os/blob/ef88310f0576/crates/runtime/src/readiness.rs) | P2b LoopDomain/Dispatcher | generation/revision-tagged state 是真相，容量 1 wake 只表示状态变化；旧 wake/ack 被 fencing，不形成第二 Message Bus |
| 有 owner 的 task/resource census | Rivet [supervision.rs](https://github.com/rivet-dev/agent-os/blob/ef88310f0576/crates/runtime/src/supervision.rs) | P2c/P2d RuntimeHost | 采用 `close admission → cancel/drain → census=0 → exit`；所有 spawn 走同一 admission gate，不能证明清零则 quarantine；ParaEGOX 不采用 Supervisor 名称 |
| 独立故障域的进程树 watchdog | DimOS [process_lifecycle.py](https://github.com/dimensionalOS/dimos/blob/1dbb6c27a0da/dimos/core/coordination/process_lifecycle.py) | P2d ProcessDomain/Liveness | 主进程 SIGKILL 后仍能发现并清理子孙进程；正式实现结合 process group/cgroup、PID start time 或 pidfd，避免只依赖环境变量和 PID |
| Native runtime/start-method 兼容矩阵 | DimOS Python worker | P2d ProcessDomain validation | Rust RuntimeHost 通过版本化协议显式托管 Python/C++ worker；Python adapter 测试 spawn/forkserver 与 CUDA/ROCm、Zenoh、ROS2、厂商 SDK，C++/native worker 测试 explicit executable/process-tree profile；不依赖平台隐式默认或在线程启动后 fork |
| TERM→grace→KILL→reap | DimOS、EMOS process group | P2d ProcessDomain | 允许协作退出但保证最终有界；KILL 后仍需 census，不能把“信号已发送”报告为“资源已释放” |
| restart budget、backoff 与 restart window | Dora [prepared.rs](https://github.com/dora-rs/dora/blob/117dfd63d583/binaries/daemon/src/spawn/prepared.rs)、ROSClaw worker manager | P2d RecoveryPolicy | restart 受 failure class、budget、Deployment revision 和 effect uncertainty 约束；自动 restart 默认不 replay invocation |

Runtime 只吸收行为不变量和验证方法，不迁入 Rivet 的 VM/WASI/VFS/POSIX 栈，不复制 TypeGo 的 S0–S3 公共层次，也不恢复 Lane 为 Card、Deck 或 Kernel 概念。首个 RuntimeHost/执行与进程治理实现使用 Rust/Cargo，但仍必须显式建模 LoopDomain、ThreadDomain、ProcessDomain、task/thread/process census 和 bounded cancellation；`spawn_blocking` 不等于 ThreadDomain，abort future 不证明外部副作用已取消。Python/C++/模型/Agent worker 默认走版本化 ProcessDomain，不拥有第二 RuntimeHost、Mailbox、restart/readiness 或 raw Fabric。

### 3.3 Mailbox、PortBinding 与 Fabric

| 特性 | 来源与证据 | ParaEGOX owner / 阶段 | 采用结论与首次验证 |
| --- | --- | --- | --- |
| 每个输入独立 DeliveryPolicy | Dora [channel.rs](https://github.com/dora-rs/dora/blob/117dfd63d583/binaries/runtime/src/operator/channel.rs) | P2a Mailbox/DeliveryProfile | 按 Link 明确 backpressure、drop-oldest、replace-latest、deadline；不让传感器和 Command/Receipt 共用隐式策略 |
| count、bytes、age、outstanding 四维有界 | Rivet accounting、Dora channel 的局限 | P2a Mailbox；P2c ExecutorBudget | 逻辑 queue length 与实际 retained bytes 都受限；测试必须覆盖 tombstone/allocator 未压缩导致的物理 OOM |
| 可靠性与合并语义分类 | Dora、Rivet | P2a DeliveryProfile | 只有明确声明的 telemetry/latest-state 可 drop/coalesce/replace；Command、Approval、Receipt、lifecycle transition 永不静默丢弃 |
| 控制、取消与结算保留容量 | Rivet resource classes/readiness | P2a/P2b Mailbox/Dispatcher | 数据洪峰下 cancel、shutdown、terminal Receipt 仍可推进；这是内部 admission class，不是公共 Lane |
| Transport callback 固定成本 handoff | Dora/Zenoh 路径、现有 Fabric 研究 | P2a fixture；P4 Fabric | callback 不执行完整 decode、Card callback 或阻塞操作；pre-validation frame 与 validated Message 分开计数和报告 |
| Zenoh SHM/网络统一数据面 | Dora Zenoh/SHM 路径 | P4 Fabric | production primary 使用 Zenoh 原生 Rust API，以同一 PortBinding contract benchmark session-local/SHM/remote；不把 Dora wrapper 或 Transport 类型引入 Kernel |
| control plane 与 data plane 分离 | Dora Coordinator/Daemon | P2e Deployment；P4 Fabric | DeploymentController 只拥有 desired/reconcile，RuntimeHost/Fabric 拥有本地执行和 route；二者不能互相接管 |
| reconnect/reclaim | Dora daemon/coordinator | P2e、P5 | 控制连接短暂中断不自动摧毁 data-plane instance；reclaim 必须重新证明 Node identity、generation、revision 和 writer tenure |
| record/replay 与 node substitution | Dora | P2–P4 Validation Harness | 重放 Observation/Message 和时序故障；physical effect 必须 stub/simulate，禁止 replay 到真实执行器 |

MoFA 同时维护 Local AgentBus、NativeChannel 和 DoraChannel 的结构作为反例：ParaEGOX 生产只认 Zenoh Fabric；不安装 Zenoh 的 PortBinding test fixture 只验证相同 Mailbox contract，不成为第二套 Bus。`zenoh-python` 仍可用于 Python workload/Gateway 兼容性验证，但 production Session、route、reconnect 与 FeatureReport owner 只有 Rust Fabric implementation。

### 3.4 Deployment、生命周期与恢复

| 特性 | 来源与证据 | ParaEGOX owner / 阶段 | 采用结论与首次验证 |
| --- | --- | --- | --- |
| Preparing 与 Running 分开取消 | EMOS Runtime | P2e Runtime assembly/Deployment attempt | 尚未创建 effectful process 时可 abort；attach/dispatch 后必须走 cancel、Receipt 和 reconcile，不共用一个 bool |
| startup/liveness/readiness/health 分型 | EMOS、Dora、Rivet、ROSClaw | P2d RuntimeHost；P2e Deployment | 事实绑定 source revision、generation、epoch 和 freshness；Ready、Live、Healthy、Degraded 不互相覆盖 |
| input timeout/circuit breaker/recovery event | Dora | P2a PortBinding health；P2e Inspection projection | timeout 表示 freshness/liveness 丢失，不自动推断进程死亡；恢复产生新 generation/revision-aware fact |
| persisted Recovering + reconnect window | Dora coordinator store | P2e DeploymentController | 控制面重启后先进入 Recovering 并等待 authenticated RuntimeHost reports；不立刻宣称全部实例死亡或重复创建 |
| desired/observed/reconcile 单写者 | Dora/AX 的部分模式与局限 | P2e DeploymentController | 保持 ADR-0001 的 DeploymentWriterEpoch、tenure proof、writer_fence/prepared/active journal、authenticated CAS；上游只能提供故障 fixture，不能替代 ParaEGOX owner |
| 声明式 compatibility preflight | PhyAgentOS、EMOS、RoboClaw MINT | P1 contract fields；P2e/P3 preflight | 在启动前检查 schema、representation、frame、frequency、feature、permission 和 target readiness；不只检查 topic 字符串是否存在 |
| Artifact fingerprint/lockfile 与 revision 绑定 | Dora reproducible builds | P2e 后 Artifact/Deck revision | fingerprint 进入 DeckLock/Deployment provenance 与高风险 Permit binding；lockfile 本身不等于供应链信任 |

## 4. P1 先冻结、P3 再实现的 physical 特性

| 特性 | 来源与证据 | ParaEGOX owner | 裁决与验证 |
| --- | --- | --- | --- |
| typed action lowering | RoboNeuron [action_semantics.py](https://github.com/guanweifan/RoboNeuron/blob/5b792d59b8e8/src/roboneuron_core/kernel/action_semantics.py) | Physical contracts / Driver Gateway | 采用 `StateSnapshot → ActionContract → ActionChunk → MotionIntent → ActuationCommand` 的分层思想；自由 tool arguments 不直达 Driver |
| Observation freshness 与 state version | RoboNeuron、PhyAgentOS、ROSClaw | Physical contracts | source、frame、captured time、state revision、freshness bound、calibration/mode revision 和 digest 进入 ABI；stale command 在执行 owner 处拒绝 |
| physical compatibility schema | PhyAgentOS runtime contract | Physical contracts / preflight | representation、shape、dtype、frame、control mode、frequency、component、chunk 和 safety profile 在 P1 冻结 seam |
| Permit 绑定 immutable physical snapshot | ROSClaw [permits.py](https://github.com/ros-claw/rosclaw/blob/3356a1cf564b/src/rosclaw/daemon/permits.py) | Authority + physical action owner | 绑定 caller、session、body/calibration/mode snapshot、capability/operation、canonical intent、deadline、次数与 execution mode |
| Resource Lease 与 stale-release fencing | ROSClaw、DimOS invocation token | ResourceCoordinator / effect owner | Lease 具有 owner、generation、TTL、renewal、fencing；旧 invocation teardown 不能释放新 owner，函数 return 不能释放仍在后台运行的动作 |
| confirmation stage 分级 | ROSClaw contracts | Receipt / Evidence | RequestReceived、ControllerAccepted、ExecutionCompleted、PhysicallyVerified 分型；Transport ACK 不升级为 EffectReceipt |
| EvidenceDomain 分离 | ROSClaw、PhyAgentOS | Evidence owner / Verifier | 软件确认、synthetic simulation、controller feedback、独立物理观测分开；任何降级必须显式保留 |
| 慢规划、快反应、固件 safety floor | TypeGo、SkillOS Robot | Agent/Physical/firmware owners | LLM 不进入实时安全关键路径；edge/firmware 执行 clamp、heartbeat/deadman、watchdog、E-stop 和 safe output |
| independent Verifier | PhyAgentOS | Agent task/Verifier service | 执行进程结束不等于任务成功；Verifier 消费 initial/final observation、Receipt、environment fact 和预提交 VerificationSpec |
| SIM/SHADOW/REAL 分型 | ROSClaw | Deployment/Physical profile | 三种模式的 authority、route 和 EvidenceLevel 不同；fixture/simulation 不能满足 H1 Hardware Enablement |

上述特性现在只形成 P1 contract seam 和失败 Harness；P3 才建立 simulation-only `Authority → Lease/Fencing → Safety → Enforcement → Receipt → Verifier`，H1 前不外推为真实硬件安全证据。

## 5. 后续 Agent、Memory、ROS2、Ops 与 Improvement 特性

这些能力不进入 Kernel，也不因为参考项目称其为 kernel/runtime 就改变 ParaEGOX owner。

| 特性 | 主要来源 | ParaEGOX 后续 owner / gate | 采用结论 |
| --- | --- | --- | --- |
| durable AgentSession 状态机 | PhyAgentOS、Google AX | P3 后 Agent CoreService | Claimed、Preflight、Running、Finalizing、Verifying、Terminal 分型；严格 transition 与 timeout/replan lineage |
| Session/Run/Turn/Step/Invocation/Attempt identity | AX、Agent libOS | Agent slice 前 contract seam | 各层 ID 不压成通用 RunId，不复用 Deployment operation identity |
| command/event 分离和 terminal closure | Google AX | Agent CoreService journal | command 请求变化；append-only event 记录已提交事实；stream EOF 前缺 terminal event 视为协议失败 |
| checkpoint 是 projection cache | AX、Agent libOS effect/event 模式 | Agent CoreService | event journal 是恢复真相；checkpoint 绑定 cursor、schema/code revision 和 digest，不能持久化任意对象图冒充协议 |
| Agent definition、Session truth、Workspace/Process 分离 | AX、Rivet | Agent CoreService + RuntimeHost | definition 可复用，Session 持久，执行环境可销毁重建；RuntimeHost 不拥有 AgentSession truth |
| ToolDefinition 与 Card 解耦 | Agent libOS、MoFA 反例 | Tool Catalog / Agent CoreService | Artifact 可并列导出 CardDefinition 与 ToolProviderDeclaration；一个 Tool 可有多个 Provider，一个 Provider 可导出多个 Tool |
| typed HITL continuation | Governance Toolkit、MoFA schema | Agent/Ops owner | Approval 绑定 exact invocation/request digest、principal、policy revision、expiry；resume 重新呈现 pending request，不形成永久授权 |
| preflight + Verifier + Replan lineage | PhyAgentOS | Agent task owner | 父 Run、子 Replan、VerificationClaim 和 EvidenceBundle 显式关联；`depends_on` 必须真实 enforce 后才可宣称 DAG |
| 具身结构化记忆 | eMEM | Memory CoreService | Observation、Episode、Gist、Entity、时间/空间索引、provenance、confidence、retention 和 consolidation 分层 |
| Receipt/Evidence 驱动记忆 | eMEM | Memory + Evidence owner | Memory 可以从可信 Evidence 派生；LLM 生成文本不能直接升级为系统事实 |
| ROS2 component lifecycle Gateway | Automatika EMOS | P8 ROS2Gateway | 借 discovery、health、restart、topic/service/action adapter 与测试；ROS2 不成为第二 Fabric 或绕过 Authority/Lease/Safety |
| Driver manifest/binding/preflight | RoboClaw MINT | Driver/Artifact/Ops | 用于安装、兼容、诊断和受权运维 session；本机 `flock` 不升级为分布式 Lease |
| ROS tool schema/error mapping | ROS MCP Server | ROS2Gateway / Tool Provider | 借 adapter schema 与 Harness；原始 ROS topic/service/action 不直接暴露给 Agent |
| read-only diagnostic Agent | NASA ROSA | Inspection/Ops 上层 Agent | 诊断 assistant 只读 Inspection/ROS diagnostics；不能拥有控制 authority |
| Input/Action/Background 与 mode lifecycle | OM1 | 产品/交互 Agent | 用于 ASR、TTS、表情、对话和 background behavior；不把这些角色固化为 Kernel Card 类型 |
| multi-robot decomposition/delegation | RoboOS、EMOS Habitat | Agent/Application 层 | 只借 subtask、leader、resume 和协作算法；DeploymentController 不成为 multi-agent planner |
| 受控 Skill 演进 | ASPIRE、ENPIRE、CaP-X | Improvement Lab | candidate→sandbox→sim→shadow→canary→签名→发布→rollback；生成次数只是候选信号，不产生 CapabilityGrant |
| branch/budget/evaluation/artifact lineage | ENPIRE | Improvement Lab / Artifact release | 每个实验分支绑定环境、安全配置、预算、验证、父版本和淘汰原因；不允许 production Agent 在线自改 |

## 6. 明确只作为反例的模式

| 反例 | 来源 | ParaEGOX 红线 |
| --- | --- | --- |
| Local Bus 与 Zenoh/Dora 双生产数据面 | MoFA | 生产只认 Zenoh Fabric；test fixture 不形成 Bus、Registry 或第二 route owner |
| 公共 Lane、S0–S3 或“一 Lane 一线程/进程” | TypeGo/EAGOS 类模式 | 时间尺度只形成 benchmark、ExecutionRequirements 和 Runtime 内部调度策略，不进入 Card/Deck Schema |
| 全局 singleton、无界 queue、轮询 request/reply | 多个轻量 Agent OS 原型 | 所有 backlog、task、thread、process、polling budget 都必须有 owner 和上限 |
| Module/Blueprint 统一 stream、RPC、tool、lifecycle、deployment | DimOS/MoFA 等 | 只借 typed assembly 和开发体验；继续遵守 `CardDefinition → Card → CardInstance`，不恢复 Module/Bundle |
| Kernel 通用 Graph/Workflow Engine | OpenFang/MoFA/HoloAgent 类设计 | DeckTopology、ServiceDependency、Deployment、Agent workflow 各有 owner；未满足双消费者 gate 不抽取 Graph Foundation |
| Kernel VFS/WASM/POSIX 栈提前进入 Foundation | Rivet/OpenFang | 首版使用 owner-specific typed references；sandbox backend 与 VFS 不是 Kernel 公共模型 |
| Tool/MCP wrapper 自称 Authority | MoFA/OpenFang/ROS MCP Server | 可绕过的 review、guardrail、tool filter 不构成强制边界；真实 effect owner 必须 enforcement |
| raw ROS topic/service/action 暴露给 LLM | ROS MCP Server | 必须经 Tool admission、Authority、Permit/Lease 与 Driver Gateway |
| 本机 `flock`、Redis lock、进程内 registry 充当分布式 Lease | RoboClaw MINT、DimOS、原型项目 | Lease 必须有 generation、TTL/renewal、fencing 和 effect-owner enforcement |
| timeout/EOF/函数 return 推断 physical effect 未发生 | AX/DimOS 等局限 | dispatch 后无 terminal proof 一律 `Uncertain → query/reconcile`，不能自动 replay 或释放冲突资源 |
| checkpoint、trace、Memory、UI projection 冒充权威状态 | 多个 Agent runtime | 权威 journal/store 与 projection 分开，投影可重建且不能反向写 owner truth |
| generated Skill 自动晋升或 raw in-process `exec` | CaP-X/自进化原型 | 生成、评测、授权、签名和发布分开；production Agent 不自行升级权限 |
| Card/recipe 自己选择 thread/process 并创建后台线程 | EMOS/DimOS 类实现 | ExecutionDomain 由 DeploymentPlan 编译、RuntimeHost 分配和登记，Card 实现不能建立隐藏执行面 |
| 把上游 README 的功能声明直接当实现保证 | 早期/论文项目 | 只有强制代码路径、测试与目标平台证据能升级 ParaEGOX completion claim |

## 7. 对现有阶段计划的归并

本文不新增阶段，也不建立新的公共组件族。特性按现有 Kernel Foundation DAG 归并：

ADR-0006 使首个生产参考中的 Kernel、RuntimeHost、执行/进程治理、NodeDaemon、DeploymentPlanner/DeploymentController、Zenoh Fabric 与 Evidence/Inspection mechanisms 优先使用 Rust；这只是 implementation allocation，不合并下述阶段和 owner。Cargo workspace/Cargo.lock/rust-toolchain 管 Rust，`uv`/pyproject/uv.lock 管 Python SDK、worker、Agent/模型和治理工具。

```text
P0   executable invariants + architecture guards
P1   identity/revision/effect/authority/physical ABI seams
P2a  bounded Mailbox + explicit DeliveryProfile + control reserve
P2b  readiness broker + fairness + cancellation/resume semantics
P2c  hierarchical resource ledger + atomic admission + ExecutorBudget
P2d  process isolation + external watchdog + census + bounded recovery
P2e  writer tenure + desired/observed reconcile + reconnect/reclaim
P3   Permit/Lease/Safety/Receipt/Verifier（simulation only）
P4   Zenoh SHM/network routes + record/replay conformance
P5   authenticated reclaim、partition、two-host reconciliation
A    durable AgentSession、ToolCatalog/HITL、Verifier
P6   local durable Evidence、Inspection、Ops journal
P8   ROS2Gateway lifecycle/adapters
后续  Memory、multi-agent、Improvement Lab 与受控 Skill release
```

### 7.1 现在应加入完成证据的检查

- P0：每条红线有 claim→test mapping，文档声明不被误报为实现。
- P1：exact effect binding、grant reservation、Unknown/Uncertain 与 late-result fencing 有纯 contract test。
- P2a：count/bytes/age/outstanding 联合过载，控制/结算路径仍可推进，逻辑有界同时证明 retained memory 有界。
- P2b/P2c：CardInstance 与 Port/ResourceClass 的公平性、deadline、Defer/Reject、atomic reserve 和 starvation 行为可重复。
- P2d：SIGKILL、孙进程、native fork/start-method、TERM/KILL、restart storm、shutdown census 和 quarantine 有真实子进程故障注入。
- P2e：旧 writer 在外部 effect 后、Receipt commit 前恢复；Coordinator restart、Runtime reconnect、相同 operation 冲突和 prepared/active crash window 可解释且不产生双 active。
- P3：ControllerAccepted 与 PhysicallyVerified 不混报；stale Observation、Permit mismatch、lease loss、Safety process crash、effect uncertainty 和 independent verifier 均有 simulation fixture。
- Agent slice：两个 execution instance 不共享状态；pending HITL、checkpoint incompatibility、committed tool result no-reinvoke 和 irreversible effect no-transparent-replay 可恢复验证。
- 工程与跨语言：Cargo 与 `uv` 各自 locked validation 通过；Rust encode/Python decode 与反向 golden vectors、ProcessDomain version mismatch、cancel/deadline、terminal/Uncertain 和 buffer lifetime conformance 通过。

## 8. 决策影响与边界

### 8.1 不改变的既有裁决

- 不改变 ADR-0001 的 DeploymentController 单写 owner、tenure authority 与 RuntimeHost apply 边界。
- 不改变 ADR-0002 的 `CardDefinition → Card → CardInstance`，也不恢复 Module/Bundle/Blueprint 公共名词。
- 不建立 Local Bus、MemoryBus、ZenohBinding 或其他第二生产通信抽象。
- 不建立 Kernel 通用 Graph Engine、Kernel VFS 或万能 Runtime。
- 不将 AgentSession、Memory、ROS2、Ops、Evidence store 或 Improvement Lab 纳入 Kernel。
- 遵守 ADR-0006 的 Rust-first mechanisms/polyglot workloads；CoreService、Card、Gateway 和 Tool Provider 都不是语言类别，Rust 实现也不能合并 ADR-0001/0002 已分开的 owner。

### 8.2 需要在对应 gate 前冻结的内容

- P2 前：bounded resources、DeliveryProfile、readiness state/wake、cancellation/resume、task census、external watchdog 与 quarantine 语义。
- P3 前：EffectAttempt、exact Decision binding、physical snapshot/freshness、Permit/Lease/Fencing、confirmation stage、EvidenceDomain 与 Verifier 输入。
- Agent slice 前：Session writer/storage、event journal/checkpoint、ID hierarchy、ToolCatalogSnapshot、HITL continuation 与 irreversible effect recovery。
- P8 前：ROS2Gateway 的 lifecycle、schema/QoS compatibility、raw ROS 权限收敛和 conformance suite。

具体公共名称、Schema 和持久格式仍按现有 ADR admission gate 冻结；本文不能单独把候选字段变成公共 API。

## 9. 风险、反证与开放问题

- Rivet、Agent libOS 和 ROSClaw 的高价值机制来自不同信任模型；将行为合并到 ParaEGOX 仍需自己的端到端 failure Harness，不能拼接上游测试数量冒充系统证明。
- Dora 的 Coordinator/Daemon 与 Zenoh/SHM 是重要实现参考，但其 reclaim、writer tenure 和 physical safety 不满足 ParaEGOX Deployment authority，不能替代 ADR-0001。
- TypeGo 的多时间尺度结果支持内部调度分型，不证明某组固定 priority、S0–S3 或 thread topology 适用于目标硬件。
- Rivet/Dora 使用 Rust 不证明 Tokio 调度具有硬实时上界，也不证明 RuntimeHost 具备功能安全等级；RealtimeDomain、safety island、HIL 和下游 applied-output proof 仍需独立证据。
- ROSClaw、SkillOS Robot 和 PhyAgentOS 的实机或验证覆盖有限；P3 只能声明 simulation spine，H1 仍需具体设备 SIL/HIL、ODD、E-stop 与独立下游 safety gate。
- Agent workflow、Memory 和自进化特性依赖产品场景；在首个真实 producer、consumer 和 retention/authority owner 出现前，不创建空 CoreService 或公共 Schema。
- 如果 P2 基准证明简单 FIFO 在全部目标负载下满足 deadline、公平性和 boundedness，可不实现复杂 DRR；不变量和观测仍保留。
- 如果 Zenoh session-local/SHM 在目标平台不能满足特定高带宽 binding 的 SLO，可以研究互斥的优化 route，但不得恢复 local+wire 双投或第二 source of truth。

## 10. 最终裁决

结论为 `revise`：ParaEGOX 当前总体架构和阶段 DAG 不需要重写，但 Kernel Foundation 的实施与验证必须显式吸收本文列出的 feature-level invariants，并按 ADR-0006 落成 Rust-first mechanisms 与 polyglot workloads。采纳单位是“语义 + owner + failure test”，不是“项目 + 语言 + 依赖 + 名词”。任何特性只有进入 ParaEGOX 自己的强制路径、跨语言 conformance、失败注入和完成证据后，才可以从研究输入升级为实现事实。
