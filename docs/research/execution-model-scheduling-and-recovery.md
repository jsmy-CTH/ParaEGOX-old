# Runtime 执行模型、调度与恢复研究

> 状态：Research Complete，结论为 `revise`
> 日期：2026-07-29
> 深度：Deep
> 评审策略：Dual independent review + EAGOS failure-path audit；尚待 Proposed ADR 评审
> 范围：CardDefinition、Card、Deck、DeploymentPlan、RuntimeHost、Mailbox、调度、线程、进程、Watchdog 与 Zenoh ingress
> 实现状态：尚未实现；本文是决策输入，不是功能完成证明

> 术语更新（2026-07-28）：本文的目标支持条件与损失分别按新基线使用限定 FeatureRequirement/FeatureReport 与 FeatureLoss；授权只使用 `CapabilityGrant`。DeploymentPlan 是唯一 desired truth，RuntimeHost 通过 runtime/contracts Schema 应用 DeploymentController 产生的 RuntimePlanSlice value，避免反向 import；见 [Capability、Service Contract 与 Feature Support](../concepts/capability-service-feature-boundaries.md)。

> 后续裁决（2026-07-29）：[ADR-0001](../adr/ADR-0001-deployment-controller-boundary.md) 保留 DeploymentController，并将本文中笼统的“DeckCompiler/DeploymentController 编译”细分为 DeckCompiler 解析工作负载语义并产生内嵌 DeckTopology 的 DeckLock、DeploymentPlanner 纯计算 DeploymentPlanCandidate binding/execution、DeploymentController 原子提交 committed plan/revision 并协调 rollout；tenure-neutral Slice 与 writer-context apply 分开，Runtime journal 分开 writer_fence/prepared/active。Runtime 调度结论不变。

> 术语裁决（2026-07-29）：`Agent` 与 `Supervisor` 保留给 Agent 层。Runtime 不建立同名 actor；原本混在该词下的职责拆为 RuntimeOwnershipTree、LivenessSpec/State、FailureContainmentSpec、RecoveryPolicy/Engine、RuntimeFailureFact 与 RuntimeHost-owned RecoveryAction。节点驻留管理进程称 NodeDaemon，整机进程 owner 称 OS service manager。

> 术语裁决（2026-07-29）：[ADR-0002](../adr/ADR-0002-card-definition-terminology.md) 将 `CardDefinition` 限定为不可变能力定义；Artifact export/entrypoint 只是实现定位与装载合同。运行回调、私有状态、窄 handles 和 invocation 均属于 CardInstance 私有实现与 RuntimeHost-owned scope，不属于 CardDefinition。

> 后续研究（2026-07-29）：[Graph Foundation、领域图与执行边界](graph-foundation-and-domain-execution-boundaries.md)明确 DataLink 不等于 activation dependency，并补齐 RuntimeHost 内部 RuntimeAssemblyEngine。本文 P2e、DeploymentPlan.execution 与验证矩阵按该研究修订。

> 实现语言裁决（2026-07-29）：Accepted [ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 将首个 production Runtime mechanism reference 改为 Rust，并保留 Python/C++/native 为受管 ProcessDomain workload。本文保留 Python/asyncio/multiprocessing 作为历史证据和 worker profile 风险，不再把它们作为 RuntimeHost 核心实现基线。

## 一句话结论

ParaEGOX 不把 `Lane` 建立为 CardDefinition、Card、Deck 或 Kernel 的公共一等概念；首个 production mechanism reference 使用 Rust RuntimeHost，但稳定合同仍是窄 `CardDefinition {In/Out PortSpec + ArtifactExportRef/entrypoint reference + ExecutionRequirements}`、Deck `Link/DeliveryProfile`、bounded Mailbox、ExecutionDomain、`LivenessSpec`、`FailureContainmentSpec`、`RecoveryPolicy` 和版本化 worker protocol，而不是 Tokio、Rust ABI 或 Python object。DeckCompiler 只产生内嵌 canonical DeckTopology 的 DeckLock，DeploymentPlanner 将 binding、execution 与 activation contract 写入 DeploymentPlanCandidate，DeploymentController 原子提交后才形成同一 revision 的 committed `DeploymentPlan.bindings/execution`。RuntimeHost 内部 RuntimeAssemblyEngine 只在 apply/replace/stop 阶段装配和激活本地对象；steady path 仍由 PortBinding、Mailbox 与 ExecutionDomain 承载。Runtime 内部可以使用多级 ready queue 或类似 lane 的数据结构，但这些只是可替换实现，不拥有公共身份、配置格式或生命周期。

## 1. 研究问题与成功标准

本研究回答：

1. EAGOS 曾依赖不同 lane 避免 event-loop 延迟，ParaEGOX 是否也必须保留 Lane。
2. 执行、调度与隔离要求分别应该由 CardDefinition、Card、Deck、Deployment 还是 Runtime 声明。
3. 如何避免线程、私有 event loop、隐式 executor、进程重启和消息队列再次形成债务。
4. 如何让 Zenoh 1.9 的网络优先级能力与应用调度协同，而不把 Transport 语义写入 Kernel。
5. 第一轮实现应按什么顺序建立可验证的执行闭环。

成功标准不是找到一个更好听的 Lane 名称，而是形成以下可执行边界：

- Deck 作者表达应用意图，不手工编排线程、PID 或 Runtime queue。
- CardDefinition 以不可变 `ExecutionRequirements` 声明实现固有的调用模型、非抢占 run bound、取消和隔离要求，不为具体应用自封优先级；应用 priority 只能由 Deck Link 请求、DeploymentPolicy 授权并由编译计划落地。
- 编译结果与实际 Runtime 执行位置只有一个权威来源，不能声明在进程、实际仍在主 loop。
- Transport callback、主 event loop、阻塞库和可杀进程具有不同预算与故障边界。
- 线程超时、进程超时、消息过期、积压丢弃和取消都产生真实而不同的结果。
- 调度算法和内部 ready queue 可以演进，而不修改 CardDefinition、Card 或 Deck 的稳定 Schema。

## 2. 范围、假设与非目标

### 2.1 范围

- ParaEGOX 当前 clean-slate 架构、CardDefinition/Card/Deck 概念和 Kernel Foundation 计划。
- EAGOS 中 Bus callback、lane-style execution、event-loop lag、thread/process placement 和生命周期/恢复经验。
- Rust async/task/thread/process ownership 与 Tokio 有界性、取消和关闭行为。
- Python asyncio、multiprocessing 和 native extension 作为受管 worker profile 的当前行为。
- Zenoh Rust production callback/ingress 与 Zenoh 1.9 priority multistream 的边界；zenoh-python 只作为 Python Gateway/worker 兼容证据。
- 机器人本地控制、软实时与真正实时执行的分界。

### 2.2 假设

- ParaEGOX 需要同时覆盖小型单进程开发、单 Node 多进程和分布式部署。
- Rust 是首个 production Runtime mechanism reference；Python/C++/native 仍是 Card、Agent、模型、Gateway 和设备生态实现语言，但不形成第二 Runtime owner。
- Zenoh 是唯一生产 Fabric，覆盖 session-local、host-local 和 remote route；确定性本地测试由 `PortBinding` test fixture 直接验证同一 Mailbox 契约，硬安全路径不依赖普通 Runtime 或 Fabric。
- CardDefinition、Card 和 Deck 要跨目标平台复用，部署细节不能成为应用语义。
- Rust core 使用 Cargo/`Cargo.lock`/pinned toolchain，Python SDK、worker、Agent/模型服务和治理工具使用 `uv`/`uv.lock`；两者都不能替代对方的依赖权威。

### 2.3 非目标

- 不在本文冻结首版 YAML/wire、Rust author API 或 Python author API 的全部字段名。
- 不预先承诺某个 weighted scheduler、线程池实现或 IPC 库。
- 不用文档宣称功能安全、硬实时或生产 readiness。
- 不迁移 EAGOS Lane、Module、Bundle、Bus 或 Runtime 实现。
- 不让通用 Runtime 调度替代硬件 E-Stop、本地 Safety Controller 或 OS 实时能力。

## 3. 证据与强度

### 3.1 本地证据

| 证据 | 类型 | 强度 | 对结论的影响 |
| --- | --- | --- | --- |
| ParaEGOX 已将 `LoopDomain`、`ThreadDomain`、`ProcessDomain` 定义为不同执行边界 | local | 高 | 隔离边界已有正确 owner，不需要 Lane 再拥有线程或进程 |
| ParaEGOX 已定义 Signal/Event/Command/Query/Receipt 和有界 Mailbox | local | 高 | 积压与溢出应归消息语义，不应归某条 Lane 的偶然实现 |
| 当前 Harness 要求“CardInstance 私有实现 callback 永久阻塞时 LoopDomain 仍响应” | local | 高 | 同一 event loop 内无法保证，需要改为准入拒绝或故障域外恢复 |
| EAGOS 的 lane-style execution 将 bounded queue、线程、私有 event loop、FIFO、溢出和指标组合在一个对象中 | local | 高 | Lane 解决过主 loop 卡顿，但混合了四类所有权 |
| EAGOS 的输入桥接、执行队列、输出队列和集中 publish 路径可形成多层排队 | local | 高 | ParaEGOX 必须定义唯一语义积压点并禁止隐藏 backlog；不应误读为每个 Input 固定穿过两层 queue |
| EAGOS 的跨 event-loop asyncio primitive 曾需要专门检查 | local | 高 | 每 lane 私有 loop 会把对象亲和性变成应用开发负担 |
| EAGOS 的 thread execution 不能硬终止；process 判活/恢复中“存活但不响应”和“已经退出”需要不同恢复链 | local | 高 | ThreadDomain 与 ProcessDomain 必须拥有不同、真实的失败状态 |
| EAGOS 的自动 placement 分类与实际子进程启动曾存在不同配置来源 | local | 高 | 声明与 observed executor 必须由一个编译计划和启动回执闭环 |

上述 EAGOS 证据只用于抽取中立的行为需求和失败模式。本文不复制其代码、测试、配置、注释或领域抽象。

### 3.2 外部一手资料

| 来源 | 类型 | 强度 | 对结论的影响 |
| --- | --- | --- | --- |
| [Tokio bounded `mpsc`](https://docs.rs/tokio/latest/tokio/sync/mpsc/) | external | 高 | channel capacity 只约束消息数量，不能替代 ParaEGOX 的 payload bytes、age、inflight 和 retained-byte admission |
| [Tokio `spawn_blocking`](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html) | external | 高 | 已开始的 blocking task 不能被 abort，shutdown 可能等待；默认 blocking pool 不能直接冒充有界 ThreadDomain |
| [Tokio graceful shutdown](https://tokio.rs/tokio/topics/shutdown) | external | 高 | cancellation 与 task tracking 是协作式结构化关闭机制，不证明外部 effect 已被撤销 |
| [Rust Reference：ABI](https://doc.rust-lang.org/reference/items/external-blocks.html) | external | 高 | Rust 原生 ABI 没有稳定保证，Card 扩展边界不能依赖任意 Rust dylib/trait object |
| [Zenoh installation](https://zenoh.io/docs/getting-started/installation/) | external | 高 | Rust 是 Zenoh 原始且功能最完整的 API，支持 Fabric owner 采用 Rust；不改变 Mailbox 和 callback budget 要求 |
| [Python：Developing with asyncio](https://docs.python.org/3.14/library/asyncio-dev.html) | external | 高 | 同一 event-loop 线程中，一个 Task 不主动让出时其他 Task 无法运行；阻塞代码会延迟全部 Task 和 I/O |
| [Python：TaskGroup 与 `to_thread`](https://docs.python.org/3.14/library/asyncio-task.html) | external | 高 | TaskGroup 可建立结构化 Task owner；`to_thread` 主要解决阻塞 I/O，不能被当作通用故障隔离 |
| [Python：concurrent.futures](https://docs.python.org/3.14/library/concurrent.futures.html) | external | 高 | 已经运行的 Future 不能通过 cancel 强制停止，ThreadDomain 不能产生虚假“已杀死”结果 |
| [Python：multiprocessing](https://docs.python.org/3/library/multiprocessing.html) | external | 高 | Python 3.14 POSIX 默认已改为 forkserver，所以 Runtime 必须显式选择/记录 start method；terminate/kill 可能破坏 Queue/Pipe/Lock，且不会自动清理后代进程 |
| [Python：asyncio 与 free-threading](https://docs.python.org/3.14/library/asyncio-threading.html) | external | 中高 | Python 3.14 允许 free-threaded build 的每线程一 loop，因此“单 loop”只是 reference profile；Task/Future/asyncio primitive 仍有 loop/thread 亲和性，多 loop 不自动提供硬终止或故障隔离 |
| [zenoh-python 1.9 API](https://zenoh-python.readthedocs.io/_/downloads/en/latest/pdf/) | external | 高 | 1.9.0 文档保留 indirect callback、bounded FifoChannel 和 drop-oldest RingChannel；FIFO 满时可阻塞，因此 ParaEGOX 仍需 fixed-cost callback 和自己的语义 Mailbox |
| [Zenoh 1.9 Longwang](https://zenoh.io/blog/2026-04-16-zenoh-longwang/) | external | 高 | priority multistream 减少网络连接内的 head-of-line blocking，但不能解决 Python callback、Mailbox 或 Card invocation 的应用侧阻塞 |
| [SEDA 原始论文](https://www.cs.cmu.edu/afs/cs/academic/class/15712-f08/www/readings/Welsh01.pdf) | external | 中高 | 显式 stage、queue、admission 和 load conditioning 是可借鉴结构；不支持每个 stage 必须独占线程 |
| [Deficit Round Robin 原始研究](https://openscholarship.wustl.edu/cse_research/339/) | external | 中 | 带权公平调度可作为 Runtime 参考实现；基础 DRR 本身不自动提供控制路径 deadline 保证 |
| [Linux PREEMPT_RT](https://docs.kernel.org/core-api/real-time/theory.html) 与 [ROS 2 实时背景](https://design.ros2.org/articles/realtime_background.html) | external | 高 | 真正实时需要 OS、内存、锁、调度与执行路径共同约束，不能由 Python Lane 或线程优先级单独提供 |

### 3.3 推断与开放证据

- **inference**：ParaEGOX 首版需要至少两个逻辑调度等级，但不需要把调度等级做成可命名、可安装或有生命周期的 Lane 对象。
- **inference**：同一语义 `DeliveryProfile` 应分别编译为 Runtime dispatch policy 和 Zenoh QoS；二者必须独立报告 FeatureLoss。
- **inference**：固定硬件上的人工调优仍有价值，但它应属于 DeploymentProfile override，而不是写回 Deck 或 CardDefinition。
- **open**：目标平台上 control/stream/background 的具体权重、burst、CPU affinity 和进程粒度尚无基准证据。
- **open**：Zenoh Rust API 是 production reference，但具体 crate version、feature、callback threading、SHM ownership 与 target compatibility 尚未冻结；P4 仍必须锁定版本并跑 ingress/channel conformance。zenoh-python 的兼容性只在需要 raw Zenoh 的受控 Gateway/worker profile 中评估，普通 Card 不取得 Session。
- **open**：free-threaded CPython、InterpreterPoolExecutor 与目标原生依赖的兼容矩阵尚未形成，只能作为 Python ProcessDomain 内部优化，不进入 RuntimeHost 基础假设。

## 4. EAGOS 失败模式的中立重建

EAGOS 的简化路径可表示为：

```text
Zenoh / local callback
          │
          ├── main loop bridge → asyncio.Queue → EAGOS Module callback
          │
          └── direct lane → queue.Queue → worker thread
                                        → private asyncio loop
                                        → EAGOS Module callback
```

Lane 的真实收益是：

- 将慢 I/O、同步 callback 和部分 EAGOS Module invocation 从 daemon loop 移走。
- 为单个工作流提供 FIFO、有界容量和丢弃计数。
- 让慢 EAGOS Module callback 不必立即拖住同一主 loop 上的控制协程。

但 Lane 没有提供：

- Python CPU 工作的稳定多核并行保证。
- 线程硬终止和 native deadlock 隔离。
- end-to-end backpressure；上游和下游仍可能各自积压。
- 真正实时或正在执行 EAGOS Module callback 的抢占。
- `publish returned` 等于 `EAGOS Module invocation applied` 的完成语义。

当 Lane 同时等于 queue、thread、event loop 和 placement 时，后续必然出现：

1. 每增加一个 lane 就增加线程与 loop，缺少全局预算。
2. asyncio object 在创建 loop 与使用 loop 不同时产生亲和性错误。
3. callback queue、lane queue、output queue 和 transport queue 叠加，消息年龄不可解释。
4. thread timeout 只能停止等待，不能停止执行体。
5. placement label、真实 PID/TID 和生命周期 owner 可能不一致。
6. 通过扩大 queue “缓解”压力，实际提高排队延迟并隐藏容量不足。

ParaEGOX 应保留这些需求，不保留解决方案形态。

## 5. 架构裁决：不建立公共 Lane

### 5.1 必须分开的六个问题

| 问题 | 权威对象 | 不由谁拥有 |
| --- | --- | --- |
| 实现能否阻塞、重入、取消或被杀 | `ExecutionRequirements` | Lane 名称 |
| 消息能否等待、丢弃、合并或过期 | `DeliveryProfile` / `MailboxSpec` | 线程池 |
| 工作在哪里运行和隔离 | `ExecutionDomain` / DomainAssignment | Card 自建线程 |
| 多个 ready workload 谁先运行 | Runtime dispatch policy | Transport callback |
| 如何判活 | `LivenessSpec` / `LivenessState` | 业务实现基类或实现自建心跳线程 |
| 卡死、崩溃和重启如何处理 | `RecoveryPolicy` / RuntimeHost 的 `RecoveryEngine` | CardInstance 私有实现对象或独立 Runtime 管理 actor |

`Lane` 不拥有其中任何一项，因此没有必要成为公共身份。Runtime 内部若使用 `control_ready`、`stream_ready` 等队列，它们没有：

- 稳定公共 ID；
- CardDefinition/Deck Schema；
- 独立生命周期；
- 自己的线程、event loop 或进程；
- 跨版本兼容承诺。

### 5.2 “没有 Lane”不等于“只有一个 FIFO”

ParaEGOX 仍必须从第一版区分控制、交互、流数据和后台工作。区别在于：

- Deck/Link 声明 deadline、freshness、priority class、ordering 和失败策略。
- CardDefinition 声明执行实现的内在约束。
- Deployment compiler 结合目标 Node facts 产生 dispatch policy。
- Runtime dispatcher 在各个 Domain 内仲裁 ready Mailbox。

调度分类是派生属性，不是用户创建的对象。

### 5.3 何时才需要内部 lane-like 结构

Runtime 只有同时满足以下条件，才应在某个 ExecutionDomain 内实例化类似 lane 的 ready-mailbox group：

1. 该 Domain 内至少有两个持续活跃的 Mailbox。
2. 它们拥有不同的 deadline、freshness 或最小服务目标。
3. CardInstance 私有实现 callback 的单次非抢占执行时间已有可验证上界。
4. 受资源约束需要有意共置，不能简单拆到另一个 Domain。
5. lane-like dispatch 开启/关闭 A/B 基准证明尾延迟或饥饿得到实际改善。

不满足这些条件时，使用 `Mailbox → ExecutionDomain` 的直接消费路径。内部 group 即使存在，也只维护 ready Mailbox，不再拥有第二份 payload queue。

## 6. CardDefinition、Card、Deck、Deployment 与 Runtime 的职责

```text
CardDefinition
├── In / Out → immutable PortSpec
├── ArtifactExportRef / entrypoint reference
└── intrinsic ExecutionRequirements
              +
DeckSpec {Cards + Links + DeliveryProfile}
              │ DeckCompiler
              ▼
DeckLock {canonical DeckTopology}
              +
DeploymentProfile + immutable NodeFacts
              │
              ▼
      DeploymentPlanner
              │ DeploymentPlanCandidate
              ▼
DeploymentController atomic commit
              │
              ▼
committed DeploymentPlan          # one revision / one desired truth
├── bindings
│   ├── endpoint + schema/codec
│   ├── Zenoh route + session-local / host-local / remote locality
│   └── target admission boundary
└── execution
    ├── DomainAssignment
    ├── MailboxSpec
    ├── DispatchPolicy
    ├── AdmissionBudget / OutstandingBudget
    ├── ExecutorBudget / IPC credits / retained-byte budget
    ├── LivenessSpec / FailureContainmentSpec / RecoveryPolicy
    └── RevisionTransition
              │ RuntimeSliceProjector + ApplyEnvelopeBuilder
              ▼
RuntimeApplyRequest {target RuntimePlanSlice + writer context + CAS controls}
              │ RuntimeHost verify/apply
              ▼
         RuntimeHost
              │
              ▼
live PortBinding + DomainInstance + CardInstance
                         │
                         └── CardInstance-private implementation object
```

| 层 | 负责声明或产生 | 明确禁止 |
| --- | --- | --- |
| `CardDefinition` | 不可变 In/Out `PortSpec`、ArtifactExportRef/entrypoint reference、内在 ExecutionRequirements、ResourceClaim | runtime binding、实现对象的共享运行状态、lane 名、PID、线程名、应用级优先级 |
| `Card` / `CardProfile` | 一次使用的配置、资源请求、允许范围内的强化约束 | 自建 executor、削弱 CardDefinition minimum isolation |
| `Link` / `DeliveryProfile` | 已声明 Port 间的连接、消息类型、deadline、freshness、ordering、overflow、ack need、请求的 criticality 与 workload envelope | 改写 Port direction/schema、Zenoh 参数、线程池和进程布局 |
| `DeckSpec` | Cards、Links、端到端工作负载意图和 `ServiceRequirement` | Runtime queue、worker 数和 CPU affinity |
| `DeploymentProfile` | 目标平台、placement、权限、资源和经过验证的专家 override | 改写业务消息语义或授予 Authority |
| `DeploymentPlan.bindings` | 稳定 BindingId、endpoint、Schema/codec、Zenoh route/locality、Fabric ingress limits、目标 admission boundary 与安装意图 | live BindingEpoch、observed Session、实时 queue depth 和 effect 结果 |
| `DeploymentPlan.execution` | 编译后的 Domain、Mailbox、admission/outstanding budget、dispatch、liveness/recovery 和 revision transition | 由用户长期手工维护 |
| Runtime `PortBinding` / `RuntimeHost` | 消费同 `SourcePlanRevision` 的 target `RuntimePlanSlice`、创建实例和 live binding、执行生命周期/恢复动作并报告 observed facts | import 或擅自改写 CardDefinition/Deck/DeploymentPlan，或降低隔离、容量和安全边界 |

### 6.1 窄 CardDefinition 与 CardInstance 私有实现对象

发布的 `CardDefinition` 是不可变能力定义，声明 In/Out `PortSpec`、配置契约、Requirements、ExecutionRequirements 和 Artifact export/entrypoint 引用；Artifact 另行提供代码、模型或二进制实现。`In[T]`/`Out[T]` 只是作者侧生成 `PortSpec(direction=...)` 的语法，不保存 Topic、Publisher、Mailbox、线程或运行指标。`entrypoint` 只是实现 export 的定位与装载合同：受信、同构建 Rust implementation 可以由 Runtime 内部静态 registry 构造，Python/C++/第三方 implementation 默认经版本化 ProcessDomain worker protocol 构造；两者都不建立公共 `Factory`、`Handler`、Rust dylib ABI 或其他实现对象领域体系。

RuntimeHost 解析 committed plan 中的 Artifact export/entrypoint，为每个 CardInstance 默认创建一个私有实现对象。该对象可以持有 ASR 模型句柄、解码状态和领域缓存，获得已验证的窄 Port/Clock/Cancellation/service/access handles，并实现 `on_start`、`on_audio`、`on_stop` 等 callback；CardInstance/RuntimeHost 才拥有运行身份、生命周期推进、超时、取消、恢复和 PortBinding。实现 callback 参与生命周期，不使 CardDefinition 成为生命周期 owner。

两个 CardInstance 不能通过类变量、descriptor、全局 registry 或隐式 singleton 共享运行状态。共享模型、设备、native singleton 或跨 Card 状态必须由 CoreService、ResourceClaim 或其他显式 owner 承担。`max_concurrency` 和不可重入声明必须说明作用域是 per-instance、per-process/artifact 还是 per-resource；没有作用域的数字不能用于容量证明。

完整 Port、Link 与 binding 裁决见 [CardDefinition 输入输出、Port、Link 与运行绑定研究](card-definition-ports-links-and-bindings.md)。

### 6.2 CardDefinition 的 ExecutionRequirements 与 run bound 来源

ExecutionRequirements 表达实现的内在事实，候选维度包括：

```text
call_model          cooperative_async | sync
workload_kind       io | cpu | native | device | mixed | unknown
blocking_risk       none | bounded | unknown
reentrant           true | false
max_concurrency
max_nonpreemptive_run duration | unknown
run_bound_provenance declared | measured | certified | unknown
cancellation        cooperative | source_timeout | not_guaranteed
minimum_isolation   loop | thread | process
kill_required       true | false
resource_claims
```

字段名仍需 Proposed ADR 冻结，但语义边界现在即可确定：

- `unknown` 不能乐观落入 LoopDomain。
- Card 或 Deployment 可以加强 isolation，不能低于 CardDefinition 的 minimum。
- CPU、native 和 device 只描述风险，不自动等同于某个线程或进程数量。
- `CardDefinition.ExecutionRequirements` 必须按声明的 invocation kind 表达私有实现单次非抢占运行片段的上界或 `unknown`；开发者声明只是一条带来源的输入，不因写进 Manifest 自动成为可信 worst-case 保证。
- 目标平台测量、认证报告或 Artifact 证据可以验证、收紧或否决该声明；普通 p99/p99.9 样本只能作为统计观测，不能冒充硬上界。DeploymentPlanner 将适用于目标平台的保守值和证据引用写入 `DeploymentPlanCandidate.plan_content.execution.effective_run_bound`，不回写 CardDefinition；DeploymentController 原子提交后它才成为 committed DeploymentPlan 的一部分，RuntimeHost 只消费其 target Slice 投影。
- Card/Deck 只声明 arrival envelope 和应用 deadline，不能为实现自报更短 run bound。Runtime observed overrun 可以使实例 degraded、阻止下一 revision 或触发重新编译，不能在运行中静默放宽计划。
- `max_concurrency` 只约束已声明作用域内的同时执行，不代替 arrival envelope、payload 上界、in-flight 和 retained-byte 预算。
- 同一个 CardDefinition 在不同 Deck 中可以拥有不同应用 criticality，因此 CardDefinition 不声明 `control_lane`。

### 6.3 Deck 声明应用意图与 priority 来源

Deck 不规定 Runtime 如何实现，但必须给 compiler 足够的语义：

- 哪条 Link 是 Command、Signal、Event、Query 或 Receipt。
- 端到端 deadline 和最大 freshness。
- workload arrival envelope：最大 rate/最小到达间隔、burst、payload bytes 上界和 `max_inflight`；不能得知时显式为 `unknown`。
- 是否允许 latest/coalesce/drop-oldest。
- 是否要求有序、持久交接或明确拒绝。
- 路径在当前应用中的 priority class 或 criticality。
- 哪些资源请求需要 Authority、Lease 和 Safety。

`PortSpec` 的 interaction 与硬约束不授予调度优先级。Deck Link/DeliveryProfile 只能**请求** priority class 或 criticality；DeploymentPolicy 根据主体、应用、资源和目标平台授权，并保留独立容量；DeploymentPlanner 把有效 class 写入 DeploymentPlanCandidate，DeploymentController commit 后才进入 `DeploymentPlan.bindings/execution`。Runtime 只能执行 Slice 中的该编译值，不能相信 producer 在消息 header 中自报“最高优先级”，Gateway 也不能把外部 transport priority 直接升级为 ParaEGOX control 权限。

高 criticality/control 的请求若未获授权、容量 reservation 或可行性证明必须失败；不允许通过把全部 Link 标成 control 规避准入，也不允许静默降级后仍声称满足原 SLO。

同一图像 CardDefinition 可以在控制 Deck 中位于低延迟路径，在录像 Deck 中属于后台路径。把 lane 固化在 CardDefinition 会破坏这种复用。

### 6.4 Committed DeploymentPlan 是唯一 desired truth

DeploymentPlanner 必须在同一 DeploymentPlanCandidate 中生成机器可验证的 `bindings` 与 `execution`；DeploymentController 在原子 commit 时为二者共同分配 DeploymentRevision 并形成 committed DeploymentPlan。不建立与 committed plan 并列的顶层 `BindingPlan`；否则 transport、placement、Mailbox 和 revision 会形成两份 desired truth。启动前至少检查：

首版每条解析后的静态 1:1 Link 对应稳定 `BindingId`。Pure compile 只产生或复现该身份，不读取、推进或比较 live `BindingEpoch`；BindingEpoch 由 Runtime PortBinding 在同一 BindingId 内安装、重装、重配或撤销时推进，且禁止跨 BindingId 比较大小。同一 BindingId/active BindingEpoch 只有一条 route 接收新 Message/frame；route 切换通过 `prepare → activate → drain → retire/rollback` 推进，`activate` 原子切换新准入、旧 route 只 drain，禁止 local/wire 双投递和内容 hash 回声去重。

- 每条 Link 的 Port direction、interaction、Schema/version、required/cardinality 和 routing 是否兼容；首版未支持的 fan-in/fan-out、Call/Operation 或动态 binding 必须 fail-fast。
- 每个 endpoint 的 Zenoh route/locality、codec、目标 admission boundary 与 `PortBinding` 权限是否完整，且不靠运行时 Topic 猜测补齐。
- CardDefinition minimum isolation 是否被满足。
- 目标 Node 的 FeatureReport 是否满足所需 CPU/GPU/device FeatureRequirement。
- Mailbox item、byte、age 和 overflow 是否完整。
- arrival envelope、单次 run bound、`max_inflight` 与 outstanding/retained-byte 预算是否在目标平台可行；`unknown` 对 control SLO 默认失败准入，对 best-effort 工作也必须使用有界保守 profile。
- 端到端 deadline 是否被分解为 ingress/queue/run/effect/cleanup 阶段预算，且每段都有 overrun action。
- Command 是否可能进入 silent-drop 路径。
- Thread/Process budget 是否越界。
- 共享设备或不可重入实现是否具有唯一 Resource owner。
- ProcessDomain 内的 side-effect 是否具有 restart-safe/idempotency/recovery owner；未声明时禁止自动 replay。
- 共置实例的 collateral restart 范围、IPC credits 和 device reset/fencing 是否可解释。
- hard safety 路径是否错误依赖普通 Runtime dispatcher。

RuntimeHost 启动后必须用 live `PortBinding`、BindingEpoch、`DomainInstanceId`、PID/TID、loop identity、executor capacity 和 epoch 证明实际计划。声明与 observed 不一致是 startup failure，不是 warning。

P2 不允许对活跃 PortBinding/Domain/Mailbox/Policy 做原地可变修补。新 DeploymentRevision 必须使用 `prepare → activate → drain → retire/rollback` 语义并携带 revision；首版可保守地实现为完整替换受影响 Binding/Domain/实例，但不能混用两版配置或留下就地修改通道。

### 6.5 RuntimeAssemblyEngine：Slice 到本地实例的唯一装配机制

`RuntimeApplyRequest → RuntimeHost` 之间必须有明确的内部 mechanism 负责把 target Slice 变成真实 DomainInstance、Card/ServiceInstance、Mailbox 和 PortBinding；否则装载、readiness、binding activation、drain 和 rollback 会重新散落在 loader、Fabric callback 与 Card 实现中。

该 mechanism 称 `RuntimeAssemblyEngine`，但不是公共 Graph Engine、CoreService、daemon 或第二生命周期 owner。RuntimeHost 仍持有所有本地副作用，AssemblyEngine 只按 RuntimePlanSlice 中已编译的 assignment、typed activation dependency、readiness/activation group、consumer ingress、producer egress、dependency-loss 与 drain contract 推进：

```text
verify + writer fence
        ↓
stage Artifact/config/resource
        ↓
create Domain/Instance/Mailbox + inactive Binding
        ↓
provider/readiness gates
        ↓
activate consumer ingress → CAS active revision → open producer egress
        ↓
steady PortBinding → Mailbox → ExecutionDomain path
        ↓
close egress → drain/cancel → retire or rollback
```

它不 import DeckSpec、DeckLock、DeckTopology、DeploymentPlan 或 DeploymentController，不从普通 Link 猜测启动顺序，也不把派生关系持久化为 editable runtime graph。prepare/readiness 失败保留旧 active；跨 RuntimeHost rollout 仍由 DeploymentController 协调。

## 7. Mailbox：唯一语义积压点

每个目标异步 admission boundary 只能有一个系统拥有的语义 Mailbox。CardDefinition In/Out 只声明端口硬约束，Deck Link/DeliveryProfile 声明本次交付意图；DeploymentPlanner 结合 fan-in/fan-out、目标容量和平台事实生成 candidate bindings 与 MailboxSpec，DeploymentController commit 后才进入权威 DeploymentPlan，Runtime PortBinding 只执行对应 RuntimePlanSlice。Mailbox 不属于任何作者 SDK 的 `In` descriptor、Transport 或某次 callback。多个 Link 汇入同一 In 时必须先编译 merge、ordering、cardinality 和公平规则；同一 Out fan-out 时每个目标的准入结果独立，除非契约显式定义聚合语义。

Transport、OS pipe 和第三方库可能仍有内部 buffer，但它们必须被测量，不能被当作 ParaEGOX 的交付承诺。source Message accepted、fabric egress accepted、encoded frame staged、Message validated/target Mailbox admitted、Card invocation completed 和 physical effect succeeded 是不同阶段；本节的 enqueue result 只描述当前 admission owner 的 handoff，不替代领域或物理 Receipt。

`MailboxSpec` 至少需要表达：

```text
capacity_items
capacity_bytes
ordering
max_queue_age
freshness
overflow
ack_mode
priority_class
```

enqueue 必须返回结构化结果，而不是 `True/False`：

```text
accepted
rejected(reason)
evicted(displaced_message_ref)
expired
closed
```

默认压力语义：

| 消息 | 默认行为 |
| --- | --- |
| `Signal` | latest、coalesce 或显式 drop-oldest，保留 drop/age 证据 |
| `Event` | 有界 FIFO；需要重放时显式 durable handoff |
| `Command` | 满或过期时显式拒绝并产生 Receipt，禁止静默丢弃 |
| `Query` | deadline、取消传播和结果版本 |
| `Receipt` | durable handoff 或按风险 fail-closed |

`block_until_deadline` 只能由明确支持 backpressure 且经过无环检查的 producer 使用。Transport callback、Runtime control channel、Safety path、event-loop dispatcher、shutdown path、持有 exclusive ResourceClaim 的调用和位于应用有环 Link 中的 producer 禁止阻塞等待容量。

不能通过扩大 queue 实现“boost”。容量应由 rate、burst、payload bytes、freshness 和可接受等待时间推导；处理能力不足时应拒绝、降级、改变 placement 或增加经过预算的 executor capacity。

### 7.1 Mailbox 有界不等于系统有界

ParaEGOX 必须同时限制五类状态，否则只是把 backlog 从 Mailbox 移到 Fabric、Task、executor 或 IPC：

1. pre-validation encoded ingress frame：由 Fabric ingress buffer 的 items/bytes/age 边界约束；它不是 Message/Mailbox，不能产生应用 accepted。
2. `queued` Message：由 target Mailbox 的 items/bytes/age 边界约束。
3. `dispatched + running`：由 Domain/Invocation `OutstandingBudget` 与 `max_inflight` 约束。
4. executor/IPC 中已提交、已发送但未完成的工作：由 permit/credit window 约束，不得使用库内部无界 queue。
5. payload reference/SHM/buffer 的 retained bytes：从 callback handoff 到终态/释放全程记账，且不与前四项混淆 cohort 数量。

Dispatcher 只在成功获得 execution permit 后才能将消息从 `queued` 原子转移到 `inflight`；无 permit 时消息仍留在唯一语义 Mailbox，不能先创建等待中的 Task/Future。Invocation 创建的 child task 隶属同一 scope 和预算，不得 detach 或越过 InvocationId/DomainEpoch 存活。

IPC 是有界 credit window，不是第二个交付 queue。Payload handle 必须有单一 owner、不可变视图、已知 size、终态释放和跨异常/取消清理规则；callback 返回后不得持有无 owner 的 borrowed buffer。copy、zero-copy、SHM 和 IPC 库是后续基准选择，这些所有权和记账语义不后置。

### 7.2 守恒与可观测语义

offer 结果和已准入消息的生命周期分开记账，禁止把中间转移与终态加在同一等式里：

```text
offered = admitted + rejected + closed + expired_before_admission

admitted cohort = queued + inflight + terminal
terminal = succeeded + failed + cancelled + expired_after_admission
         + evicted + coalesced + uncertain
```

`accepted_with_eviction/coalesce` 要同时为新 MessageId 记录 offer outcome，为被替换 MessageId 记录独立终态。enqueue/dequeue/start/finish 使用 owner 的 monotonic measurement point。Metric label 必须有界，exporter 丢失另有 counter，snapshot 带 epoch/revision 和 partial/stale 标记。

## 8. Dispatcher：调度 Mailbox，不调度 Lane 对象

每个 ExecutionDomain 拥有自己的 dispatcher 或受控 shard，不建立跨全 Runtime 的万能 dispatcher。Dispatcher 只从 ready Mailbox 选择下一项工作。

调度契约至少需要：

- control 类工作有可验证的等待上界或 SLO。
- 非安全等级之间存在最小服务份额，避免严格优先级永久饥饿。
- 每类工作有 `max_burst`，避免一个 ready queue 长时间独占 loop。
- dequeue 前再次检查 deadline/freshness，过期工作不进入 CardInstance 私有实现 callback。
- 同一 binding 内保持声明的 FIFO 或 keyed ordering。
- fairness 按实际 cost 或显式 token 记账，不能只按消息数量假装公平。
- priority class 来自 Deck Link 的请求、DeploymentPolicy 的授权和 `DeploymentPlan.bindings/execution` 的编译结果，不能相信不受信任 producer 在消息中自报“最高优先级”。
- control/high-criticality class 必须经 DeploymentPolicy 授权并保留容量；总 utilization 不可行时拒绝计划，不把所有流都静默降级。

weighted round-robin、deficit round-robin 或 deadline-aware arbitration 都是候选实现。首版 ADR 应冻结行为和测量，不冻结算法名称。基础 DRR 不天然提供 deadline 保证，因此控制路径仍需独立 SLO、admission 和最大 burst 验证。

硬件 E-Stop、本地 Safety inhibition 和需要证明的硬实时路径不进入普通 dispatcher。最高 priority class 也无法抢占当前正在执行且不主动让出的 Rust future、Python callback 或其他非协作 invocation。

CardInstance 私有实现 callback 的最长非抢占执行时间必须进入高优先级工作的 worst-case wait 分析。若低优先级 callback 可以 CPU spin 250 ms，任何 queue 排序都无法承诺新到达 Command 在 250 ms 内开始；正确修复是缩短 callback、拒绝 LoopDomain 或拆分故障域。

deadline 不只在 dequeue 前检查。`DeploymentPlan.execution` 需要为 ingress/queue/run/effect/cleanup 分配当地 monotonic budget，并为运行中超时编译 `continue | cooperative_cancel | escalate | uncertain` 动作。cancellation 必须传播到 child scope，cleanup 另有有界预算。跨进程/Node 只传递 remaining budget 或经明确 clock-uncertainty 换算的期限，接收 owner 在本地 monotonic clock 上安装；远端 monotonic timestamp 只做 Evidence，不直接驱动本地 expiry。

物理 Stop/Enable、mode switch 等同一资源的语义顺序归 resource owner 的 ordering key、fencing 和 supersession rule，不得由 priority queue 重排。优先级只影响何时尝试处理，不改写物理操作的先后关系。

## 9. ExecutionDomain

### 9.1 LoopDomain

用途：短小、可信、主动让出的异步状态机和路由。

硬约束：

- P2 reference profile 在每 RuntimeHost process 使用一个 runtime-owned Rust async runtime/reactor。未来额外 LoopDomain/shard 必须由 RuntimeHost 计划、预算和观测，经 A/B 证明，且不得把同进程多 reactor 误报为故障隔离；Card 不得私建 Tokio runtime、Python loop 或其他 executor。
- 只接受 `cooperative_async`，禁止阻塞 syscall、长 CPU loop、未知 native call、未证明会让出的 future 和阻塞 logging/exporter。
- CardInstance 私有实现不直接创建 detached task；Runtime scope 使用结构化 task registry、cancellation tree 和 join/cleanup owner。Tokio `Handle`、`JoinHandle`、channel 不进入 Card context 或公共合同。
- slow callback/run-slice 只能被测量、拒绝和升级，不能在 async runtime 内部被硬抢占；drop future 也不证明已经发生的外部 effect 被取消。
- CardInstance 私有实现永久不 yield/阻塞时，同一 reactor 的 control 与 Inspection 也会停；正确行为是 admission 不让它进入 LoopDomain，外部 watchdog 负责恢复错误放置造成的 host stall。

### 9.2 ThreadDomain

用途：有来源级 timeout 的有界同步 I/O，以及经证明不会永久卡死、可以在共享地址空间运行的原生工作。

硬约束：

- worker 只运行同步 callable，不创建私有 Rust async runtime 或 Python event loop。
- 使用 RuntimeHost 拥有的少量有界 executor class，不为每个 Card/Input 创建线程。
- 不使用隐式 default executor/Tokio default blocking backlog；submission 之前先占有有界 permit，不能把 backlog 隐藏进 executor 内部 queue。`spawn_blocking` 只有包在同一 admission/budget/census 之后才可能作为内部 mechanism。
- 线程开始运行后不能被强制取消；deadline 只能停止等待或请求 cooperative cancellation。
- 超时结果只能是 `cancellation_requested`、`uncertain` 或 `wedged`，不能是虚假 `cancelled/success`。
- 迟到结果携带 InvocationId 与 DomainEpoch；scope 已结束或 epoch 已变化时不得落地。
- 可能永久卡死、持有独占设备、需要硬终止或未知 native stability 的工作进入 ProcessDomain。
- worker 被判定 wedged 后其 permit/capacity 继续被占用，不得超出 ExecutorBudget 偷偷补线程。Domain/executor 进入 degraded/poisoned，后续工作拒绝或迁移；只有 callable 真实返回或 RuntimeHost process 重启后才能宣称容量恢复。

是否存在 GIL 只影响 Python worker 内部 CPU 并行策略，不改变线程不可硬杀、共享地址空间和 native crash 污染整个进程的事实。Rust 的 `Send/Sync` 与 ownership 能减少部分错误，也不能把 thread wedge 变成可强杀。free-threaded Python 以后可以成为 worker Artifact/Deployment Feature，不改变公共 ExecutionRequirements。

### 9.3 ProcessDomain

用途：Python/C++ Card、Agent、模型、GPU/native、设备 SDK、第三方/未知扩展和需要硬隔离或强制终止的工作。

硬约束：

- Rust RuntimeHost 使用显式 executable/exec-style launch profile，并按平台记录 process group/cgroup/job-object/pidfd 等进程树治理事实。Python worker 内部若使用 multiprocessing，`spawn`/`forkserver` 只能作为经验证的 adapter profile，禁止在线程已启动后依赖隐式 fork；这些参数都不是公共应用语义。
- child 使用版本化最小启动契约、明确的序列化/引用边界和 readiness handshake，不继承完整 RuntimeHost；Python runner 只 construct→invoke→terminal frame，不拥有 raw Zenoh、语义 Mailbox、自重启或 readiness authority。
- heartbeat 必须持续覆盖运行期，不只覆盖 bootstrap。
- shutdown 依次为 stop accepting、drain、cooperative stop、TERM、KILL、join、resource cleanup。
- kill 必须覆盖 process group/tree；不能假定终止父进程会清理后代。
- IPC 具有 item/byte cap、credit window 和独立 epoch；进程被强杀后旧 Queue/Pipe/Lock 不再被视为可靠，重启建立新 channel。IPC 未发送或未回收 credit 的工作继续占用 outstanding budget，不成为第二 backlog。
- restart policy 包含 window、attempt budget、backoff、jitter 和 quarantine。
- Ready 前应用 CPU/RSS/FD/process/device 资源限制；实际限制失败时不报告 ready。
- `RuntimeHostEpoch + DomainEpoch + InvocationId` 用于拒绝旧进程迟到结果。
- child crash/SIGKILL 后，RuntimeHost 只产生 `RuntimeFailureFact`（如 `ProcessExitFact`）：未 handoff 的工作可明确 rejected，已 handoff 但没有副作用终态证明的 Invocation 必须是 `uncertain`。`RecoveryEngine` 不得伪造 effect `Failed` Receipt。
- 自动 restart 默认不 replay 在途工作。RecoveryPolicy 明确区分进程级 RestartPolicy（window/attempt/backoff/jitter/quarantine）与副作用级 InvocationRecoveryPolicy（side-effect class、restart-safe 条件、idempotency/recovery-state owner 和 replay）；后者缺失时 quarantine 或人工 reconcile。
- ProcessDomain 只是地址空间故障边界，不自动拥有 GPU/device 生命周期。`Succeeded` 需要 device/resource owner 的 completion fence/ack；child crash 后设备可能是 `unknown/poisoned/reset-required`，新 owner 接管前必须完成 fencing、健康检查或 reset。

### 9.4 RealtimeDomain

只有目标场景提出可证明的 worst-case deadline 后才引入 RealtimeDomain。它应是独立 Rust/C++ process 或受 OS 实时策略、CPU affinity、memory locking 和受控分配约束的窄执行器。

Rust RuntimeHost 可以负责配置、生命周期/恢复和 Receipt，但不宣称自身是硬实时或功能安全执行器。对普通低延迟路径使用 `control` priority class，不等于建立 RealtimeDomain。

## 10. Resource 与 ThreadBudget

RuntimeHost 必须拥有全局 `ExecutorBudget`，而不是让每张 Card 局部合理、整体过量。预算至少覆盖：

- Runtime framework threads。
- ThreadDomain workers。
- Zenoh callback/runtime threads。
- OpenMP、MKL、OpenCV、ONNX Runtime、Torch 等原生内部 pool。
- Driver/device SDK 自建线程。
- 子进程及其内部线程。

CardInstance 私有实现不直接创建 Thread、Process、event loop 或无 owner background Task。确实需要专用串行执行的设备在 CardDefinition 中声明 `ResourceClaim`、不可重入约束或 exclusive concurrency key，由 Deployment compiler 创建唯一 resource owner；不声明 Lane。

启动后 Inspection 必须比较 planned 与 observed thread/process 数。未知新增线程、oversubscription、affinity 漂移或 native pool 超预算是 degraded/admission failure，不能只写 debug log。

## 11. 结构化生命周期与结果状态

每个 Task、Thread work item、Process、Mailbox 和 Binding 都只有一个 lifecycle owner。父 scope 结束时，取消只有在以下条件之一成立后才算完成：

- cooperative Task 已执行 cleanup 并 join；
- 尚未开始的 work item 已从 Mailbox/queue 移除；
- thread work 已真实返回；
- process 已退出、join 且资源清理完成。

执行状态不能压缩成 `success/failed/cancelled`。至少要区分：

```text
accepted
started
succeeded
failed
rejected
expired
evicted
cancellation_requested
cancelled_cooperatively
uncertain
wedged
terminated
killed
```

`accepted` 只表示取得了明确 owner，不表示应用完成。Transport publish 返回、Mailbox enqueue 成功、Card invocation completed 和 physical effect succeeded 是不同阶段，分别产生 Receipt 或 Inspection fact。

### 11.1 RuntimeOwnershipTree 与故障传播

```text
RuntimeHost
└── DomainInstance          # kill/restart/resource-accounting unit
    ├── CardInstance / ServiceInstance
    │   └── InvocationScope
    │       └── owned child tasks/work
    └── shared executor/IPC/resource handles declared by the plan
```

- 普通 Invocation 失败默认只结束自己 scope，不取消无关 CardInstance。
- DomainInstance 是最小可杀死/重启的运行边界。多个实例共置时，Domain crash 的 collateral restart 集合必须写入 `DeploymentPlan.execution.FailureContainmentSpec` 并由 Inspection 展示。
- child task/work 不得越过 InvocationScope、DomainEpoch 或 DeploymentRevision 存活。TaskGroup fail-fast 是某个 scope 的实现选择，不能未经计划便扩大故障传播。
- device/resource owner 与 process owner 是两个边界。Process exit 不是设备操作取消证明，`RuntimeFailureFact` 不是 effect Receipt。

## 12. 判活、恢复与外部 Watchdog

### 12.1 同进程监测只负责诊断

event-loop lag monitor、slow callback、queue wait 和 Card invocation duration 可以定位问题，但它们与主 loop 同故障域，不能恢复永久 stall。它们的职责是提供最近证据，不是承诺 liveness。

### 12.2 NodeDaemon 观测，OS service manager 拥有 RuntimeHost 进程

生产 profile 需要 RuntimeHost 之外的 `NodeDaemon` 与 OS service manager 形成跨故障域闭环：NodeDaemon 负责节点级探测、事实发布和受限管理请求，OS service manager 拥有 RuntimeHost 进程并执行 start/stop/restart。二者持续覆盖：

- bootstrap 阶段进度。
- 运行期连续 heartbeat。
- control channel responsiveness。
- process tree、resource usage 和 restart budget。

恢复顺序是先采集有界证据，再根据 committed `RecoveryPolicy` 预算化 restart；同一阶段反复失败后进入 quarantine，禁止无限重启风暴。NodeDaemon 不能通过 RuntimeHost event loop 自己证明 RuntimeHost 仍活着，也不因此获得 Deployment desired-state ownership。

## 13. Zenoh route ingress 与端到端优先级

session-local、host-local 和 remote 三种 Zenoh route 的 callback 都只能执行固定成本、非阻塞工作：

- key/header/长度和版本的有界检查。
- 缓存命中的 transport principal assertion。
- 生成不可变 encoded frame reference 或复制受上限约束的小 encoded envelope。
- 验证 BindingId，并只在该 BindingId 内比较 BindingEpoch，随后非阻塞 `try_offer` 到有界 Fabric ingress buffer。

Fabric ingress buffer 不是 Mailbox，不拥有应用 Delivery backlog，也不能报告 Message accepted；其 items/bytes/age、retained bytes 和 overflow 必须进入 Inspection。完整 payload decode、复杂 schema/principal/binding validation 进入有界 ingress worker；只有成功后才构造不可变 Message 并 offer 到 target Mailbox，失败产生 ingress rejection。领域执行 admission 和 CardInstance 私有实现 callback 再由 ExecutionDomain 负责。恶意大 payload 或高成本 validator 不能占用 Zenoh callback thread，也不能污染 target Mailbox。

同一个语义 DeliveryProfile 分别编译为：

```text
Runtime dispatch policy
        +
Zenoh priority / reliability / congestion mapping
```

Zenoh 1.9 multistream 能隔离不同 priority 的网络流，但如果所有消息最终进入同一个 Fabric callback/ingress worker、Mailbox 或 Card invocation reactor，应用侧 head-of-line blocking 仍然存在。P4 production reference 使用 Zenoh Rust API，并联合验证 wire priority、transport callback duration、Mailbox wait 和 Card invocation start latency；Python Card 不持有 raw Session。

## 14. 方案比较

### A. Deck/Card 显式声明 Lane

- 优点：固定机器人上直观，人工调优快，TUI 容易展示。
- 成本：Deck 与线程/queue 实现绑定；不同 Deck 组合产生全局命名和容量冲突；CardDefinition 复用时优先级语义错误。
- 适用反例：封闭 appliance、硬件和工作负载长期固定，且明确接受配置与 Runtime 强耦合。
- 结论：不作为公共默认；只允许 DeploymentProfile 中经过基准的专家 override。

### B. 单 event loop + 单 FIFO，不区分调度等级

- 优点：实现最少，开发态容易理解。
- 成本：stream/bulk 可以阻塞 control，过载只靠扩大 queue，无法证明公平性和延迟。
- 结论：拒绝。

### C. 每个 Card 独立 thread/event loop

- 优点：局部串行和隔离直观。
- 成本：线程/loop 随 Card 增长；跨 loop primitive、关闭、GIL、内部 library threads 和共享状态复杂度迅速上升；仍不能硬杀。
- 结论：拒绝。

### D. 每个 Card 默认独立 process

- 优点：隔离和 CPU 并行清晰，强杀更真实。
- 成本：启动、内存、IPC、序列化和判活/恢复开销高；小设备与高带宽路径不一定可接受；process kill 仍需处理 IPC 和后代进程。
- 结论：作为 minimum isolation 或 DeploymentProfile，不作为所有 Card 的固定默认。

### E. 语义声明 → 编译执行计划 → Runtime 内部调度

- 优点：CardDefinition/Deck 可移植；调度与隔离可随平台演进；desired/observed 可闭环；无需公共 Lane。
- 成本：DeploymentPlanner 与 admission 更严格；Planner 需要 immutable target facts 和真实 benchmark。
- 结论：**推荐**。

### F. free-threaded Python、subinterpreter 或 actor-per-Card

这些都是 Python ProcessDomain worker 内部的候选优化，不是 Rust RuntimeHost 或公共语义替代品。它们仍需 IPC credit、budget、liveness/recovery、deadline、generation 和 observed truth。只有目标 Python、native wheels、debugging 和 shutdown conformance 通过后，才能作为 Artifact/Deployment Feature；首版不预建独立公共 Domain 类型。

## 15. 风险、反例与失效条件

### 15.1 主要风险

- Compiler 过度自动化但缺少 workload facts，产生看似智能的错误 placement。
- `ExecutionRequirements` 字段过多、未经证据冻结，变成另一套配置语言。
- Runtime 内部 dispatch policy 缺少稳定 Inspection，问题只能靠猜。
- 一个语义 Mailbox 之外仍存在未计量 transport/executor buffer。
- 只限制 queued items，但 detached Task、in-flight invocation、IPC credit 或 retained payload 仍无界。
- 为降低延迟使用严格 priority，造成 background/telemetry 永久饥饿。
- Deck 将所有 Link 标记为 control，绕过 criticality 授权和容量保留。
- 把 ThreadDomain deadline 当成线程终止，迟到结果继续写状态。
- ThreadDomain wedged 后无预算补 worker，导致线程与资源逐步泄漏。
- ProcessDomain kill 破坏共享 IPC 或遗留后代进程、SHM、FD。
- RecoveryEngine 把 process exit 伪造为 effect failed，或 restart 后自动 replay 已可能生效的物理 Command。
- DeploymentRevision 就地修改活跃对象，使新旧 Mailbox、Domain 和 policy 混用。
- 将 ProcessDomain crash 误当作 GPU/device 已取消，未 reset/fence 就让新 owner 接管。
- 把 Zenoh network priority 当作应用执行 priority。
- 为追求“自动恢复”形成 restart storm 或反复执行不确定物理副作用。

### 15.2 反例与应对

- 固定硬件上需要人工线程/CPU 调优：放入 DeploymentProfile override，记录目标平台、基准、原因和回退值，不写入 CardDefinition/Deck。
- 多个 Deck 争用同一资源：由 ResourceCoordinator、quota/tenant policy 和 DeploymentPlan 处理，不创建跨 Deck 全局 Lane 名。
- 设备 SDK 要求所有调用在同一线程：声明不可重入 ResourceClaim，由 Runtime 创建受预算的串行 resource owner；若可能卡死则放进 ProcessDomain。
- 单进程设备资源极紧：可以共置 CoreService 和 CardInstance，但逻辑 Mailbox、ExecutionRequirements、owner 和失败状态不合并。
- control CardInstance 私有实现 callback 确实需要长计算：拆成短控制状态机和 Process/Realtime worker，不能靠提高 Rust task、Python worker 或 OS thread priority 掩盖。

### 15.3 会推翻推荐方案的证据

- 实测表明编译层带来的延迟或复杂度不可接受，并且显式公共 Lane 能在多个平台、多个 Deck 组合中保持稳定语义和更低维护成本。
- CardDefinition 的内在执行约束无法与 Deck 的应用语义分离，导致 Deployment compiler 无法产生一致计划。
- 目标产品正式冻结为单一 appliance，不需要 CardDefinition/Deck 跨平台复用，团队明确接受 Runtime 配置成为应用公共 API。

目前没有这些证据。

## 16. ADR 影响

进入实现前建议形成 Proposed ADR：

1. 不可变 CardDefinition、Artifact export/entrypoint 引用、CardInstance 私有实现对象与生命周期 owner。
2. `In/Out PortSpec → Deck Link/DeliveryProfile → DeploymentPlan.bindings → live PortBinding` 的 owner chain。
3. `ExecutionRequirements` 的最小维度、unknown 行为、run-bound 来源和强化/削弱规则。
4. `DeliveryProfile`、Mailbox、enqueue result 与不同消息类型的压力语义。
5. `DeploymentPlan.bindings/execution`、DomainAssignment 和 desired/observed 一致性。
6. Loop/Thread/Process Domain 的准入、取消、关闭和 late-result fencing。
7. Runtime dispatch 行为、priority 授权/fairness/deadline 目标与 Inspection。
8. Process start method、IPC epoch、process-tree cleanup 和 restart quarantine。
9. Zenoh ingress callback budget 与 Runtime/Fabric QoS 编译边界。
10. Workload envelope、run-bound provenance、OutstandingBudget、IPC credit 和 payload-handle ownership。
11. RuntimeOwnershipTree、effect Receipt vs RuntimeFailureFact、restart-safe/replay 与 device completion/reset。
12. DeploymentRevision 的 prepare/activate/drain/retire/rollback 与 revision fencing。
13. Deadline stage budget、运行中 overrun/cancellation 以及跨 Node remaining-budget 安装。
14. Offer/lifecycle 守恒、bounded labels、snapshot epoch 与 benchmark evidence schema。
15. ADR-0006 已接受的 Rust-first/polyglot、Cargo/uv、language-neutral wire、禁止公共 Rust dylib ABI与 Python worker subordinate-owner 边界。

ADR 冻结行为和所有权，不冻结线程数量、调度算法、具体 IPC 库或未经目标平台测试的权重。

## 17. 有序实施计划

### P0：并发宪法与工程门禁

- 落实 Accepted ADR-0006 与本文其余 Proposed execution 不变量。
- 建立 cargo metadata + Python import architecture check，禁止 CardInstance 私有实现直接创建 thread、process、async runtime/event loop 和无 owner task。
- 冻结最小 Rust toolchain/core CI 与 Python tooling/worker profile；不以 free-threaded、InterpreterPool 或嵌入 CPython 为基础假设。

完成证据：违规 fixture 在 CI 中确定失败；未安装 Zenoh/模型/硬件依赖时 Kernel contracts 可测试。

### P1：纯契约与虚拟时间

- 不可变 CardDefinition/PortSpec、ArtifactExportRef/entrypoint reference、Card/Link 约束，以及 `DeploymentPlan.bindings` 的纯编译模型；不创建 runtime descriptor 或独立 BindingPlan。
- `ExecutionRequirements`、`DeliveryProfile`、`MailboxSpec`、`EnqueueResult`。
- workload arrival/payload envelope、run-bound provenance、`max_inflight`、OutstandingBudget 和 deadline stage/overrun 语义。
- side-effect/restart-safe/recovery 声明、RuntimeFailureFact 与 effect Receipt 的 owner 分离。
- RuntimeHostEpoch、DomainEpoch、InvocationId 与 late-result 比较规则。
- Deadline/Freshness 使用虚拟 monotonic clock。
- 为 RuntimeApplyRequest、Message、Receipt 和 worker protocol 建立 Rust↔Python canonical byte/digest/error golden vectors。

完成证据：无真实 sleep、线程、进程或网络的 schema/state-machine suite。

### P2a：Mailbox 与 PortBinding test fixture

- item + byte + age 三重有界 Mailbox。
- queued/inflight/terminal 状态机、Domain outstanding permit、payload-handle 生命周期与 retained-byte 记账。
- Signal/Event/Command/Query/Receipt 的压力 conformance。
- RuntimeHost 通过 production projector/builder 生成的确定性 test fixture 消费 target `RuntimePlanSlice.bindings`，安装 CardInstance-scoped PortBinding，并为每个 CardInstance 创建独立私有实现对象；它不 import 或直接应用 DeploymentPlan。
- Fixture 只按 PortBinding 契约向目标 Mailbox offer，不运行 CardInstance 私有实现 callback，不进入生产路径，也不建立 `MemoryPortBinding` 或第二套 Bus 公共概念。
- 从第一天产生 raw queue/age/drop/reject Inspection facts。

完成证据：相同 CardDefinition 的两张 Card 不共享实现对象或 runtime descriptor；offer/lifecycle 分层守恒，queued + inflight + retained bytes 同时有界，Command 不 silent-drop，关闭后 enqueue 明确拒绝。

### P2b：LoopDomain 与 Dispatcher

- reference Rust async runtime/reactor、结构化 Task owner、ready Mailbox arbitration；公共合同不暴露 Tokio 类型。
- priority、fairness、max burst、expiry-before-run 和 control SLO harness。
- admission 拒绝 sync blocker、CPU spin 和 unknown native work。
- 取得 permit 后原子地 queued→inflight；运行中 deadline overrun/cancellation/cleanup 按阶段预算处理。
- control criticality 经 policy 授权与 reservation，不可行 utilization 在编译/准入阶段失败。

完成证据：错误 workload 无法进入 LoopDomain；stream overload 下 control wait 满足目标平台门槛。

### P2c：ThreadDomain 与 ExecutorBudget

- 有界 submission、少量 executor class、无 worker 私有 runtime/loop；不把 default blocking pool或 `spawn_blocking` queue 当作有界 ThreadDomain。
- honest cancellation/wedged/late-result fencing。
- wedged worker 永久占用容量，Domain 进入 degraded/poisoned；不无预算补线程。
- planned/observed thread inventory，包括 native pools。

完成证据：stuck call 不产生假 cancelled；旧 InvocationId/DomainEpoch 结果不落地；线程总量不随 Card 无界增长。

### P2d：ProcessDomain、判活与恢复

- explicit executable launch、versioned minimal child、readiness/heartbeat、credit/epoch IPC；Python spawn/forkserver 只属于 worker adapter profile。
- TERM/KILL/process-tree cleanup、restart budget/quarantine、resource limits。
- RuntimeHost 外的持续 NodeDaemon/OS watchdog。
- `RuntimeHost → DomainInstance → Card/ServiceInstance → InvocationScope` RuntimeOwnershipTree 与 collateral restart 范围。
- IPC credit/retained-byte budget，crash 后 RuntimeFailureFact vs effect `Uncertain`，默认不 replay。
- device completion fence/ack、crash 后 unknown/poisoned/reset-required 与新 owner 接管门。
- revision-tagged prepare/activate/drain/retire/rollback；首版整体替换受影响 scope，不原地混配。

完成证据：Rust RuntimeHost 分别启动可信 Rust child 与 Python reference worker；blocking、native crash、SIGKILL、ignore TERM、OOM、grandchild、protocol mismatch 和 stale IPC 故障注入全部有真实 RuntimeFailureFact，在途副作用是 `Uncertain` 而非伪造 `Failed`，restart 不 replay，device 接管与 revision 替换可验证。

### P2e：最小 Deployment control plane

- 接入 typed Deck multigraph/SCC validator、typed ServiceDependency DAG validator、pure DeploymentPlanner、single-writer DeploymentController、OS-managed DeploymentTenureAuthority、crash-consistent DeploymentController/RuntimeHost journals 和 direct Runtime apply adapter；没有显式 feedback contract 的 cyclic Deck fail-fast。
- 将 readiness、activation group、consumer ingress、producer egress、dependency-loss 与 drain order编译进 `DeploymentPlan.execution` 和 target Slice，相关变化进入 digest；DataLink 不冒充启动依赖。
- 在 RuntimeHost 内实现 RuntimeAssemblyEngine，完成 candidate → atomic commit → target Slice → authenticated CAS apply → prepare/readiness/activate/drain/rollback → observe → `reconcile_once`，并分开 `writer_fence`、`prepared` 与 `active`。
- 只有 DeckCompiler 与 DeploymentPlanner 等两个独立生产消费者已经证明相同纯算法后，才在同一 bounded batch 抽取内部 Graph Foundation；不先创建 package。
- P2a–P2d 的 fixture 只是 production projector/builder 的 conformance 输入，不是 CLI 或手写 Slice 的第二 desired-state owner。

完成证据：parallel Link 与 SCC/cycle witness 确定；Service cycle 和无 feedback contract 的 Deck cycle 在副作用前失败；consumer ingress 先于 producer egress；Runtime 不安装/import decks/deployment 仍可只靠 Slice apply；DeploymentController restart 必须获取更高新 tenure，prepare/activate crash、partial apply、timeout、重复 operation 与 writer takeover 不产生 revision 回退、双 active route 或旧操作隐式 replay。P3 仿真物理闭环虽不在本文展开，但 Foundation 的硬顺序仍为 P2e → P3 → P4；不得由 P2d 直接进入 Fabric。

### P4：PortBinding 的 Zenoh 生产 route

- 固定 Zenoh Rust crate/version/features 和 callback/ingress conformance；只有受控 Python Gateway/worker 确需 raw Zenoh 时才另测 zenoh-python compatibility，且不形成第二 Fabric owner。
- callback fixed-cost handoff、BindingId-scoped BindingEpoch，以及 Fabric ingress buffer 的 item/byte/age/retained-byte limits；pre-validation frame 不进入 target Mailbox。
- committed `DeploymentPlan.bindings` 固定 endpoint/schema/codec/keyspace/route locality、Fabric ingress limits 与 target admission；RuntimeHost 只消费对应 `RuntimePlanSlice.bindings`。DeliveryProfile 同时编译 Runtime dispatch 与 Zenoh QoS，并报告 FeatureLoss。
- 至少验证 session-local 与同主机双进程 host-local；remote 在分布式里程中继续复用同一 conformance。

完成证据：wire priority、Fabric ingress wait 与 application start latency 联合基准；malformed/large/expensive-validator payload 不拖住 control path、不进入 target Mailbox，也不伪造 Message accepted。

## 18. 验证矩阵

| 类别 | 必须注入 | 通过条件 |
| --- | --- | --- |
| CardInstance isolation | 同一 CardDefinition 的两张 Card 并行启动、分别重启 | 每个 CardInstance 拥有私有实现对象和 PortBinding；共享资源只能经显式 owner；一方重启不污染另一方状态 |
| Binding lifecycle | direction/schema/cardinality 不兼容、同一 BindingId 的旧 BindingEpoch、route prepare/activate/drain 失败、未支持动态 binding | 不兼容在启动前 fail-fast；运行事实与同 revision `DeploymentPlan.bindings` 一致；`activate` 原子切换准入、旧 route 只 drain；同一 BindingId 任何时刻只有一条 route 接收新 Message/frame 且无双投递；旧 binding 消息被拒绝；不同 BindingId 的 epoch 不比较 |
| Deck multigraph | 同一 Card pair 多 Port Link、输入顺序随机化、自环与多节点 SCC | parallel edge 不丢失；DeckLock/SCC/cycle witness 稳定；无 feedback contract 时在副作用前 fail-fast |
| Service dependency | 直接环、长环、自环、provider loss/recover | 环有稳定 witness 并在计划阶段拒绝；loss action 与 reverse shutdown order来自 typed contract |
| Activation semantics | DataLink 与反向 service requirement、各 readiness/activation 点注入消息 | Link 不决定启动 topo；consumer ingress Ready 后才开放 producer egress；规则变化进入 plan/slice digest |
| Runtime assembly | prepare/readiness/activate crash、重复 operation、旧 writer/revision、CAS conflict | 旧 active 保持或 bounded rollback/quarantine；无双活/mixed revision；Runtime 不 import decks/deployment，steady path 不经过 assembly loop |
| Loop admission | sync sleep、CPU spin、unknown native call | 启动/绑定前拒绝或编译到非 LoopDomain |
| Loop latency | stream/bulk 持续超额负载 | 记录 p50/p95/p99/p99.9/max lag；control SLO 来自最小业务 deadline 而非固定魔法数 |
| Mailbox | item、byte、age 分别打满 | 容量不越界；offer outcome 与 admitted lifecycle 分层守恒；evict/coalesce 的被替换 MessageId 有独立终态 |
| System bound | Fabric ingress、Mailbox、executor/IPC/inflight 分别打满，payload handle 迟延释放 | pre-validation frame 不进入 Mailbox；无 permit 不 dequeue/不创建 Task；ingress+queued+inflight+retained bytes 不越计划且终态全释放 |
| Fairness | control + interactive + stream + background 同时 ready | control 满足门槛，非安全等级不永久饥饿，max burst 生效 |
| Dispatch A/B | 同一负载分别使用 direct dispatch 与内部多级 ready queue | 只有 p99/p99.9、饥饿或资源利用有可重复改善时才保留 lane-like 实现 |
| Thread | pool saturation、永不返回、取消后迟到 | 不假装杀线程；domain degraded/wedged；迟到结果被 epoch 拒绝 |
| Thread recovery | worker 永久 wedged 后继续提交 | 不超预算补线程；容量扣减；新工作拒绝/迁移；不伪报 recovered |
| Process | crash、ignore TERM、SIGKILL、OOM、grandchild | Runtime/Fabric/OPS 保持可用；进程树和资源被清理；restart 超预算进入 quarantine |
| Crash result | effect 执行中 SIGKILL、restart 后收到重复工作 | RuntimeHost 只记录 RuntimeFailureFact；无终态证明的 Invocation 为 Uncertain；默认不 replay |
| IPC | kill 正在写消息的 child | 旧 channel 作废，新 epoch 重建；不复用可能损坏的 Queue/Pipe/Lock |
| Revision apply | activate 中失败、旧 revision 迟到、drain 超时 | 不存在新旧 Domain/Mailbox/Policy 混配；rollback 或保守替换可解释；旧 revision 被拒绝 |
| Device | child crash 时 GPU/device 工作在途 | 不因 process exit 报 Succeeded/Cancelled；设备进入 unknown/poisoned/reset-required；新 owner 经 fence/health/reset 后才接管 |
| Deadline | queue/run/effect/cleanup 各阶段超时与跨 Node 传递 | overrun action 与 cancellation 传播可验证；远端 monotonic 不直接驱动本地 expiry |
| Shutdown | startup/drain/cancel 各阶段竞态 | 无 orphan Task/Thread/Process/SHM/FD；最终状态与 Receipt 一致 |
| Zenoh | slow callback、malformed/large payload、expensive validator、断连重连、多 priority | callback budget 与 ingress cap 生效；只有验证成功才构造 Message/准入 target Mailbox；transport、ingress 与 semantic rejection 可区分 |
| Safety | data flood、RuntimeHost stall、Fabric down | hard E-Stop/LocalSafety 不依赖普通 dispatcher 或远端 Fabric |
| Priority inversion | 低优先级持有共享资源、高优先级请求 | 通过 resource owner/arbitration 解决，不依赖跨 class 共享锁 |
| Resource ordering | 同一执行器的 Stop/Enable 从不同 priority class 到达 | ordering key、resource owner 和 supersession rule 保证正确，不由调度优先级重排物理语义 |

基准同时记录：Card invocation run、queue wait、message age、loop/control tick lag、queued/inflight/IPC credits/retained bytes、actual threads/processes、CPU、RSS slope、FD/SHM、drop/reject、restart 和 cleanup time。每份基准证据记录 workload/profile、硬件/OS、Rust toolchain、target triple、libc/CPU features、核心 dependency、worker protocol，以及适用时的 Python/worker runtime 版本，并保存预热、重复次数、原始样本、统计/置信区间和可复现命令。P2 即产生原始事实，P6 只负责 durable aggregation/export，不能到 P6 才第一次发现缺指标。

## 19. 发布、回滚与完成证据

- 首版只启用 development/local profile，不宣称功能安全或硬实时。
- P2a、P2b、P2c、P2d 分别可禁用后回退到上一个已验证 profile，不保留双 executor 或双队列兼容路径。
- 自动编译策略先以 report-only 输出计划，经过 Harness 后才允许 apply；失败回退到明确的保守 isolation，不回退到 main-loop optimistic placement。
- DeploymentRevision 只通过 prepare/activate/drain/retire/rollback 转换；首版回滚是重建受影响 scope，不保留原地可变对象或两版双写。
- DeploymentProfile override 必须记录平台、理由、基准、有效范围和回滚值。
- 进程反复失败进入 quarantine，不因回滚重新执行 `uncertain` 物理 Command。

专项执行模型只有在以下证据存在后才可宣称完成：

1. CardDefinition/Deck Schema 不包含公共 Lane 或线程/PID 编排。
2. 相同 Deck 在至少两个目标 profile 上可以编译为不同执行计划而不改变应用语义。
3. Runtime observed PortBinding/BindingEpoch 与 `DeploymentPlan.bindings` 一致，PID/TID/loop/capacity 与 `DeploymentPlan.execution` 一致。
4. overload、wedge、crash、kill、shutdown 和 stale-result Harness 可重复通过。
5. OpsService/运维客户端仅通过 InspectionService 投影解释消息在哪个 Mailbox 等待、由哪个 Domain 执行、为何拒绝或恢复；不得反向读取 Runtime 私有状态。
6. 目标硬件上的 control SLO、thread/process budget 和资源清理有可复现基准。
7. Fabric ingress frames、queued Messages、inflight、executor/IPC credits 和 retained payload bytes 在联合压力下仍有界，不存在 detached work 或隐藏 backlog。
8. process crash 不伪造 effect Receipt、不自动 replay；device 和 DeploymentRevision 的恢复/替换门可通过故障注入证明。

## 20. 推荐方向与置信度

**Verdict：revise。** 保留 ParaEGOX 当前 Mailbox、ExecutionDomain、RuntimeHost 和 Deployment 分层；删除或改写把 Lane 当作公共调度对象的表述，并把 P2 拆为 Mailbox、Loop、Thread、Process 四个可独立验收的阶段。

置信度：

- Lane 不应成为 CardDefinition/Card/Deck 公共概念：高。
- ExecutionRequirements 与 DeliveryProfile 分离：高。
- 同一 revision 的 DeploymentPlan.bindings/execution 作为唯一 desired binding/execution truth：高。
- 首版使用多级 ready queue 与带权公平策略：中高，算法仍需 benchmark。
- P2 reference ProcessDomain 由 Rust host 通过显式 executable/进程树协议启动：中高；Python worker adapter 内部选择 `spawn`、`forkserver` 或独立解释器仍需在目标 Linux/Jetson profile 上验证启动性能与原生库兼容性，但 start method 不成为公共应用语义。
- RealtimeDomain 具体实现：中低，等待真实 worst-case deadline 和硬件需求。

残余风险集中在 target facts 质量、自动编译可解释性、目标硬件预算和第三方 native runtime 行为。这些风险不阻塞 P0，但 P1/P2a 开始前必须先通过 Proposed ADR 冻结本文新增的 workload/run bound、全链路有界、Receipt/recovery 和 revision-transition 语义；只有调度数值、库选型和平台参数留给基准。

### 20.1 独立评审分歧与综合

三路复核一致认为公共 Lane 应删除，但对 `ExecutionContract` 的命名与 owner 有一项有意义的分歧：

- 一种意见建议由 Artifact 实现作者通过 CardDefinition 直接声明 `ExecutionContract`，Runtime 将其编译为 LanePlan。
- 另一种意见认为完整 Contract 必须同时包含 CardDefinition 内在要求、Card/Link workload SLO、目标 Node facts 和 Deployment policy；若 CardDefinition 自己拥有同名 Contract，会形成新的并行配置真相。

本文采用第二种边界：CardDefinition 只声明 `ExecutionRequirements`，Link 声明 `DeliveryProfile`，完整且可执行的 binding/execution 权威结果只存在于同一 revision 的 `DeploymentPlan.bindings/execution`。Runtime 内部不建立稳定 `LanePlan` 类型；若实现需要 lane-like group，它只是该计划的一次派生数据结构和 Inspection detail。

## 21. 后续入口

- 总体架构：[Kernel、RuntimeHost 与 Core Services](../architecture/kernel-runtime-core-services.md)
- 应用概念：[CardDefinition、Card 与 Deck](../concepts/card-definition-card-deck.md)
- CardDefinition 与端口专项研究：[CardDefinition 输入输出、Port、Link 与运行绑定](card-definition-ports-links-and-bindings.md)
- 消息与 Fabric：[Kernel 消息机制、Fabric、Evidence、Telemetry 与 Security 边界](kernel-messaging-fabric-evidence-security.md)
- 实施顺序：[Kernel Foundation 实施计划](../plans/kernel-foundation.md)
- 正式决策入口：[ADR 目录](../adr/README.md)
