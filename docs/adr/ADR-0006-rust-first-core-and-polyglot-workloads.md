# ADR-0006 — Rust-first 核心机制与多语言工作负载边界

> 状态：Accepted
> 日期：2026-07-29
> 决策者：ParaEGOX maintainers
> 关联文档：[Kernel、RuntimeHost 与 Core Services](../architecture/kernel-runtime-core-services.md)、[Runtime 执行模型、调度与恢复](../research/execution-model-scheduling-and-recovery.md)、[Kernel Foundation Plan](../plans/kernel-foundation.md)、[ADR-0001](ADR-0001-deployment-controller-boundary.md)、[ADR-0002](ADR-0002-card-definition-terminology.md)

## 一句话结论

ParaEGOX 采用 `Rust-first mechanisms, polyglot workloads`：首个生产参考实现优先用 Rust 建设 Kernel、RuntimeHost、执行与进程治理、NodeDaemon 和 Zenoh Fabric 路径；Python、C++、模型、Agent 与设备生态通过版本化 ProcessDomain 协议或独立 Service/Gateway 接入，不把解释器、动态库 ABI 或语言私有对象变成核心系统边界。

## 背景

ParaEGOX 当前仍处于 clean-slate 阶段，尚无需要迁移的 Kernel/Runtime 产品实现；现有 Python 只承担仓库治理、测试与文档工具。因此现在选择核心实现语言，成本主要是修改工程基线和执行模型，而不是重写已有产品。

既有研究已经要求 RuntimeHost 对 Mailbox、Task、Thread、Process、IPC credit、payload bytes、generation、deadline、取消、恢复与关闭拥有唯一且可验证的责任。Python 仍然是 Agent、模型、ASR/TTS、VLM、ROS2 和设备 SDK 的重要生态入口，但若解释器、第三方 native extension、用户 callback、Fabric ingress 与系统生命周期长期处在同一故障域，容易重新形成隐藏线程、event-loop stall、不可杀工作和关闭失控。

Rust 能强化内存安全、类型约束、资源所有权、crate 依赖边界和 Zenoh 原生集成，但不能自动解决无界队列、线程卡死、分布式一致性、crash consistency、hard realtime 或功能安全。Tokio channel 也不能替代 ParaEGOX 的 items/bytes/age 全链 admission，协作式 cancellation 也不能证明外部 effect 已取消。因此本决策选择 Rust 作为机制层默认实现，不把 Rust 当作系统语义或安全证明。

## 范围与非目标

本 ADR 决定：

- 首个生产参考实现的核心语言方向。
- Kernel/Runtime/Fabric 与 Python、C++、native workload 的装载和故障边界。
- Rust/Cargo 与 Python/uv 的工具链权威分工。
- 公共合同、Card Artifact、in-process 实现与 ProcessDomain worker 的兼容边界。
- `unsafe`、FFI、共享内存和嵌入解释器的准入原则。

本 ADR 不决定：

- 强迫所有 Card、Agent、CoreService、Driver、Gateway、OPS、TUI 或 ROS2 组件使用 Rust。
- 将组件的领域身份绑定到实现语言；CoreService、Card、Gateway 和 DeploymentController 仍由所有权与生命周期定义。
- 最终 wire codec、IPC/RPC library、Tokio 版本、SHM 实现、Card 作者宏或 Python decorator 语法。
- 公共 Rust `dylib`/trait-object 插件 ABI、WASM ExecutionDomain 或 RealtimeDomain。
- 把 Rust 类型布局、`serde` derive 或 crate-private enum 直接当作稳定协议规范。
- 改变 ADR-0001 至 ADR-0004 已拥有的 Deployment、Card、Deck、OPS 与 Application 边界。
- 从目标目录图预建空 crate、SDK、daemon 或兼容层。

## 决策

### 1. Rust-first 表示机制默认，而不是 Rust-only 产品

首个生产参考实现优先由 Rust 拥有以下路径：

- Kernel 的基础 value contract、time、identity、digest、failure、receipt、admission、grant/lease/fencing 等纯机制。
- Runtime-owned apply contract、RuntimeHost、RuntimeAssemblyEngine、Mailbox、Dispatcher、Loop/Thread/Process Domain、RuntimeOwnershipTree、liveness、recovery 与资源记账。
- NodeDaemon 及其 Runtime endpoint discovery、Node facts 和窄 NodeManagementEndpoint。
- ParaEGOX 侧的 Zenoh session、Fabric ingress、decode/validation、PortBinding 和 session/binding epoch。
- 进入本地物理 effect enforcement 的窄、经证据准入的机制路径；硬件 safety island 仍在独立故障域。

DeploymentPlanner、DeploymentController、Evidence/Inspection 与其他控制面组件的首个 production reference 同样优先 Rust，以减少核心节点的解释器依赖并共享合同实现；但它们的正确性仍来自确定性、单写、journal、revision、epoch、fencing 和恢复协议。未来某个组件使用 Python/C++ 不改变其领域身份，也不能建立第二 writer 或第二 Runtime owner。

平台 CoreService 按真实工作负载选择语言：Fabric、节点和 effect enforcement 更适合 Rust；Agent、Model、ASR/TTS、VLM、Memory/World 算法和研究型服务可以优先 Python/C++。`CoreService` 不是语言类别。

### 2. 公共合同保持语言中立

- CardDefinition、DeckLock、DeploymentPlan、RuntimePlanSlice、Message、Receipt、Grant/Lease 与 ServiceContract 不暴露 Rust memory layout、trait object、Tokio handle、channel、Python object 或 crate-private type。
- 跨进程/跨语言数据具有唯一 Schema authority、canonical encoding、版本与 unknown-field 规则；Rust 与 Python/C++ binding 只能实现同一合同，不能各自成为真相。
- digest 不依赖 map iteration、编译器布局、platform endianness 或语言默认序列化行为。
- 首个跨语言消费者出现时必须提供 byte-level golden vectors、双向 encode/decode、错误向量与版本兼容测试。
- RuntimeHost 只理解自己消费的 RuntimePlanSlice/apply/wire contract，不在 Rust 中重建 DeckCompiler 或 Deployment 的第二份领域真相。

### 3. Card 和 Service Artifact 不等于动态语言插件

CardDefinition 的 Artifact export/entrypoint 保持 language-neutral。Artifact resolution 至少区分 target、architecture、runtime kind、protocol version、entrypoint、dependency closure 与 digest，但这些字段只有真实 producer/consumer 出现时才冻结。

- 只有受信、随 RuntimeHost 同构建和同发布的 Rust 实现可以进入 in-process Loop/ThreadDomain；它通过内部 registry/static linkage 接入，不形成第三方稳定 ABI。
- Python Card/Agent/模型、第三方 C/C++、未知或不可信 Artifact、可能 wedge/panic/native crash、需要独立 GPU/SDK 环境或强制终止的工作默认进入 ProcessDomain或独立 Service/Gateway。
- 不把 Rust `dylib + trait object` 作为公共 Card 插件协议；Rust 没有稳定通用 ABI，动态加载也不提供故障隔离或强制终止。
- PyO3/maturin 可以用于经过基准证明的窄 binding、codec 或纯计算优化，但不作为 RuntimeHost 托管 Card callback、lifecycle、取消和恢复的默认边界。
- WASM 可以在生态、GPU/设备和高带宽能力成熟后作为新的受管 execution profile 研究，当前不预建公共 Domain。

### 4. RuntimeHost 是多语言执行的唯一运行 owner

Python/C++ worker 只是 RuntimeHost 的 subordinate executor，不是第二 RuntimeHost。它不得自行拥有 Deployment truth、语义 Mailbox、restart policy、readiness authority、raw Zenoh session 或平行 lifecycle state machine。

ProcessDomain protocol 至少需要表达：

- Artifact/entrypoint、protocol/runtime version 与 target compatibility。
- instance、source revision、RuntimeHost/Domain generation 和 Invocation identity。
- startup/readiness handshake、持续 heartbeat 和 liveness sequence。
- bounded ingress/egress、IPC credit、payload byte accounting、deadline 与 cancellation intent。
- `stop accepting → drain → cooperative stop → terminate → kill → process-tree cleanup → join`。
- terminal result、RuntimeFailureFact、Receipt 引用与 `Uncertain`。
- workspace、Secret/egress/access handle、resource budget 与 sandbox profile。

禁止跨边界传递 Python/Rust 私有对象、Future、Task、Queue、Lock、裸指针、任意 pickle 或未版本化对象句柄。控制 metadata 使用版本化协议；大 payload 只有在 lifetime、lease/generation、释放和 bytes accounting 合同通过 crash Harness 后，才可使用 Blob/Buffer/SHM handle。

### 5. Rust 不改变 ExecutionDomain 和失败语义

- async runtime 是 RuntimeHost 私有实现，公共合同只表达 structured ownership、deadline、budget、cancellation 和 observed facts；不暴露 Tokio `Handle`、`JoinHandle` 或 channel。
- LoopDomain 仍拒绝阻塞 syscall、长 CPU、未知 native/device work 和不主动 yield 的实现。
- ThreadDomain 提交前必须获得显式 permit；不能把 Tokio `spawn_blocking` 或默认 blocking pool 直接当作有界 ThreadDomain。运行中的线程仍不可安全硬杀，wedge/late-result 语义不变。
- ProcessDomain 继续承担强制终止、依赖隔离、Python/native/GPU worker 和 process-tree cleanup；Rust 不能把它删除。
- drop/cancel future 只表示 Runtime 的取消意图，不证明外部 effect 未发生；无权威终态时继续使用 `Uncertain → query/reconcile`。
- Rust core 不宣称 hard realtime 或功能安全；RealtimeDomain、MCU/PLC/设备 safety island、E-Stop 与 local fail-safe 路径保持独立。

### 6. Cargo 与 uv 各有唯一权威范围

- 根 Cargo workspace、`Cargo.lock` 和 `rust-toolchain.toml` 是 Rust 核心构建、依赖与 toolchain 的权威。
- `uv`、Python `pyproject.toml` 与 `uv.lock` 继续管理 Python SDK、worker、Agent/模型服务、测试辅助和当前治理工具；uv 不再是所有 ParaEGOX 工程的唯一完成判据。
- CI 分开验证 `rust-core` 与 `python-tooling-sdk`，再运行跨语言 contract/system suite。把 `cargo` 包在 `uv run` 后面不消除双工具链，也不能建立第二份 Rust lock authority。
- governance 需要通过 `cargo metadata --locked` 等真实 crate graph 验证依赖方向，并继续检查 Python import；不能只靠目录名或文档声明边界。
- crate、feature、binary 和 Python package 仍须满足现有 owner/consumer/admission 规则，不能因为采用 Rust 就预建空 workspace 成员。

### 7. `unsafe`、FFI 与原生依赖必须收窄

- 普通 contract、planner/state-machine 和 Runtime mechanism crate 默认禁止 `unsafe`。
- Zenoh SHM、FFI、设备 SDK、GPU 或 OS-specific syscall 的 `unsafe` 只进入窄 adapter crate/module，并记录 owner、调用前条件、lifetime、线程模型和故障边界。
- 第三方 native panic/crash、ABI drift、library-owned thread pool 与 shutdown 必须进入 target compatibility matrix 和故障注入；safe Rust 不能替它们背书。
- 跨 FFI unwind、共享内存释放、consumer crash 与迟到访问必须 fail-closed 或隔离到可终止进程，不以代码 review 代替 Harness。

## 备选方案

### 保持 Python-first，只把热点改写为 Rust extension

它能减少初期工具链成本，但 RuntimeHost、callback、进程治理和 Zenoh owner 仍由解释器承担，Rust extension 又引入 FFI/lifetime 边界。它适合算法热点，不作为 production mechanism baseline。

### 全系统 Rust-only

它能减少语言种类，却会人为切断 Agent、模型、ROS2、GPU 与设备生态，并迫使项目维护大量薄弱 wrapper。ParaEGOX 的 Artifact 和 ProcessDomain 本来就需要多语言，因此拒绝把语言纯度提升为领域限制。

### 在 Rust RuntimeHost 中嵌入 CPython

调用看似直接，但 interpreter、GIL/native extension、Rust lock、shutdown 和 packaging 会回到同一进程故障域，无法替代 ProcessDomain 的 kill、cleanup 与 generation fencing。它只允许作为经过独立基准与故障审查的窄优化，不作为默认 Card runner。

### 公共 Rust 动态插件 ABI

部署形式灵活，但 Rust ABI、panic/FFI、依赖版本和 allocator 不构成稳定兼容合同，动态库也不能隔离 wedge 或内存破坏，因此拒绝。

### C++ production core

技术上可行，也可能适配少数硬件 SDK，但 clean-slate 核心在内存安全、并发所有权和 Zenoh 原生集成上没有足够理由优先 C++。设备或 Realtime 窄组件仍可使用 C++，不改变整体 Rust-first 决策。

## 后果

收益：

- ownership、lifetime、Send/Sync 和 crate dependency 可以在编译阶段暴露更多错误。
- RuntimeHost、Mailbox、process tree、FD/SHM 与 Zenoh resource 更容易形成单一 owner 和自动释放边界。
- 减少解释器、GIL、Python object dispatch 与 Python/native callback 对核心热路径和长期 daemon 的影响。
- 更直接使用 Zenoh Rust API、buffer/SHM 和新能力，同时保留 Python/C++ 工作负载生态。
- 核心节点可逐步交付为固定 toolchain 的原生 binary，减少生产环境 package/import 漂移。

成本与限制：

- Cargo 与 uv、Rust 与 Python/C++ binding、跨语言合同和 target matrix 成为长期成本。
- ProcessDomain 引入 IPC、序列化、进程启动和高带宽 payload ownership 问题。
- Rust async、unsafe、cross-compilation、feature 组合和 native linking 需要专门工程能力。
- Rust 不消除 deadlock、starvation、无界配置、逻辑错误、native wedge、journal 损坏或分布式 partial failure。
- 首个实现必须先证明语言边界，不能同时建设完整 Agent/Graph/ROS2 产品面。

## 失败场景与反例

以下证据会触发重新评审，而不是静默破坏边界：

1. 目标硬件证明 ProcessDomain 的 copy/latency 无法满足某个可信 workload SLO：允许通过独立 ADR 准入静态 in-process Rust adapter 或受控 SHM handle，但不能因此开放任意动态库。
2. Rust 在必需 ROS2 发行版、Jetson/architecture 或设备 SDK 上无法形成可维护实现：对应 Gateway/Driver 可以保持 C++/Python 独立进程；只有核心机制本身无法满足目标时才重开本 ADR。
3. 跨语言 Schema/IPC 的故障率和维护成本持续高于 Runtime 收益：必须以真实 benchmark、incident 和团队成本比较 Python-only reference，不能让 Python worker 悄悄获得第二 Runtime owner。
4. 未来稳定 WASM component model 能覆盖所需 GPU、设备、高带宽和调试能力：可提出后继 ADR增加新的受管 execution profile。

## 实施与验证

实施顺序：

1. 更新文档、计划和工程治理，将 Python-only/uv-only 假设改为 Rust-first + polyglot。
2. 建立最小 Cargo workspace、锁文件和 pinned toolchain；只创建当前 P0/P1 切片有真实 producer/consumer 的 crate。
3. 让 governance 同时验证 crate graph 与 Python import graph，CI 分开核心和 Python 子域。
4. 实现最小 Rust contracts、canonical wire vectors 和 Python reference binding。
5. 在正式 P2 前完成一个 Rust vertical spike：有界 Mailbox、一个可信 Rust implementation、一个 Python child worker、heartbeat/credit/cancel/kill/cleanup/generation fencing。
6. spike 通过后由 Rust RuntimeHost 成为唯一 production execution owner，再按 P2a→P2d 实施。
7. P2e、Zenoh、Agent/模型与 ROS2 仍按既有依赖 DAG 进入，不因语言选择提前并行展开。

最低证据：

- Rust core 可以在不安装 Python、Zenoh、ROS2、数据库和模型依赖时构建并测试最小 contract slice。
- architecture test 能拒绝 Kernel → Runtime/Deployment/Zenoh 和 Runtime → Deck/Deployment 非法依赖。
- Rust encode/Python decode 与 Python encode/Rust decode 的 golden vectors、digest、unknown-field 和错误行为一致。
- malformed、oversize、version mismatch、stale generation 和 exhausted credit 在副作用前 fail-fast。
- Python worker 阻塞、crash、忽略 cancellation、产生 child 后，RuntimeHost 仍可响应并完成 process-tree/FD/IPC/SHM cleanup。
- overload 下 items、bytes、age、inflight、IPC credit 与 retained bytes 同时有界。
- kill、timeout 或断连不会伪造 effect Receipt；未知终态进入 `Uncertain`，默认不 replay。
- 仓库不存在对第三方承诺稳定性的 Rust `dylib` Card ABI，也不存在嵌入 CPython 的默认 Runtime 路径。

在 wire contract 冻结前，可以撤回 Rust workspace 而不迁移产品数据；冻结后即使替换实现语言，也必须保持同一 Schema、digest、authority 和 ProcessDomain 语义，不能借语言回滚重写协议。

## 后继与替代

本 ADR 不替代 ADR-0001、ADR-0002、ADR-0003 或 ADR-0004。Graph Foundation、RealtimeDomain、WASM execution、稳定 external component ABI、具体 ROS2Gateway 语言和高带宽 SHM handle 分别等待自己的准入证据与后继决策。
