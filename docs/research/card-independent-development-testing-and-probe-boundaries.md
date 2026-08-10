# Card 独立开发、测试 Harness 与运行探测边界研究

> 状态：Research Complete，结论为 `revise`
> 日期：2026-07-29
> 深度：Deep
> 评审策略：ParaEGOX/PhanthyMotus 本地代码审查 + 受限历史失败模式审查 + 外部一手资料 + independent challenge
> 范围：CardDefinition、Card、CardInstance 的独立开发体验、测试 Harness、one-subject Deck、startup/liveness/readiness/health 与物理诊断边界
> 实现状态：尚未实现；本文是目标架构与实施顺序的决策输入，不是已存在的 SDK、CLI、Probe API 或测试能力声明

> 前置裁决：[ADR-0001](../adr/ADR-0001-deployment-controller-boundary.md) 已接受 single-writer DeploymentController、canonical RuntimePlanSlice 与 RuntimeHost apply 边界；[ADR-0002](../adr/ADR-0002-card-definition-terminology.md) 已接受 `CardDefinition → Card → CardInstance` 三分链。本文不修改这两项裁决，只回答如何在不建立第二条生命周期和 desired-state 路径的前提下获得快速开发、独立验证与可信运行探测。

> 后续裁决（2026-07-29）：[ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 已接受 Rust-first mechanisms + polyglot workloads。本文的直接对象测试只表示语言内 workload unit profile；P2b LoopDomain 使用可信同构建 Rust fixture，Python/C++/未知或不可信 Artifact 的生产等价装载证据从 P2d 版本化 ProcessDomain worker 开始。不建立公共 Rust `dylib` ABI或默认嵌入解释器路径。

## 一句话结论

Card 可以被**独立开发和隔离验证**，但 `CardDefinition` 与 Deck 中的 `Card` 都不能直接 `run()`，`CardInstance` 也不能由作者或测试自行构造、启动。ParaEGOX 应分层提供语言内 workload unit profile、Runtime 内部的 canonical Slice 单主体 Card component Harness，以及走 `DeckCompiler → Planner → DeploymentController → RuntimeHost` 正式链路的 one-subject Deck 开发运行；未来的便捷命令只能是生成显式 one-subject Deck 的语法糖。`startup`、`liveness`、`readiness`、`health` 和测试断言必须分型，不建立万能 `probe() -> bool`、公共 Probe registry 或由 Card 自己重启自己的旁路。通用 L0–L3 Harness 强制禁止真实物理副作用，设备自检必须升级为受 Authority、Lease/Fence、Safety 与真实效果证据约束的独立 L4/H1 诊断 Operation。

## 1. 研究问题与成功标准

本研究回答：

1. Card 是否能够像普通组件一样独立运行或通过 Probe 测试。
2. 不经过完整 Deck/Deployment 是否能快速测试 ASR、TTS、感知、Agent 或控制 Card。
3. CardDefinition、Card、CardInstance 和其私有实现对象分别允许什么测试入口。
4. 单 Card Harness 如何复用 production Runtime 契约，而不成为手写 Slice、第二套 desired truth 或 production bypass。
5. startup、liveness、readiness、health、diagnostic 与测试 observation 应由谁产生、判定和采取动作。
6. 对真实设备有影响的自检、commissioning 与 HIL 如何避免伪装成普通 Probe。

成功标准是：

- 作者能在不启动整个分布式系统时快速验证纯领域逻辑。
- Runtime 能在不安装 `decks/`、`deployment/` 的进程中通过 canonical apply fixture 被独立验证。
- production 中只有 DeploymentController 能提交 desired revision，只有 RuntimeHost 能创建和推进 CardInstance 生命周期。
- 同一 CardDefinition 的两个实例不共享语言私有 mutable state、类变量/descriptor、`Arc<Mutex<_>>` 别名、全局 registry、workspace、PortBinding 或运行状态。
- “测试通过”“进程存活”“当前 revision Ready”“业务健康”和“物理效果成功”不被压成同一个布尔值。
- 每一层测试都明确它证明什么、不能外推什么，并能完整清理 Task、Thread、Process、Mailbox、FD、SHM 与 workspace。

## 2. 范围、假设与非目标

### 2.1 范围与假设

- ParaEGOX 处于 clean-slate 文档阶段，尚无兼容负担，也没有已实现的 Card SDK 或 RuntimeHost。
- `CardDefinition` 是不可变能力合同；`Card` 是 Deck 中一次具名、配置使用；`CardInstance` 是 RuntimeHost 托管的运行身份。
- Runtime 只消费序列化 `RuntimeApplyRequest {RuntimePlanSlice + writer context}`，不 import `decks/` 或 `deployment/`。
- Zenoh 是唯一 production Fabric；确定性测试只能注入 `PortBinding test fixture`，不能建立第二套 Memory Bus 或 production route。
- Rust 核心与 Runtime Harness 使用 Cargo workspace、`Cargo.lock` 和 pinned toolchain；Python SDK/worker、测试辅助与当前治理工具继续使用 `uv`，跨语言证据使用同一 canonical contract suite。

### 2.2 非目标

- 不冻结 `card test`、`deck dev` 等最终 CLI 名称和参数。
- 不创建公共 `CardHarness`、`ProbeSpec`、`ReadinessReporter` 或 test manifest Schema。
- 不让 unit/component fixture 取得 production Grant、Secret、真实设备或 raw Zenoh Session。
- 不把测试 fixture、Canvas preview 或 notebook 变成 RuntimeHost 的生产输入源。
- 不声明 Kubernetes、ROS 2 或 PhanthyMotus 的现有行为就是 ParaEGOX API。
- 不用一份 component test 证明现场安全、硬实时、生产 readiness 或真实物理成功。
- 不把 Rust/Python/C++ 定义成 Card 或 CoreService 身份，不把 Python direct-object unit profile 变成生产 runner，也不建立公共 Rust `dylib`/trait-object Card ABI。

## 3. 证据与强度

### 3.1 ParaEGOX 当前硬约束

| 证据 | 强度 | 对本研究的影响 |
| --- | --- | --- |
| ADR-0002 已将定义、Deck 内 desired use 与 Runtime 身份拆成 `CardDefinition → Card → CardInstance`；CardInstance 生命周期归 RuntimeHost | 高，Accepted ADR | 直接 `CardDefinition.run()`、`Card.run()` 或作者自行 new/start CardInstance 会破坏已经接受的 owner 边界 |
| ADR-0001 要求 DeploymentController 是 DeploymentScope 内唯一 desired-state writer，Runtime 只接收 canonical Slice 与 writer fencing | 高，Accepted ADR | production standalone runner 不能直接写 RuntimeHost；测试 fixture 也不能成为第二份可编辑 desired truth |
| ADR-0006 已冻结 Rust-first core、语言中立合同、可信 Rust in-process 与多语言 ProcessDomain 边界 | 高，Accepted ADR | Python direct-object 测试只形成 workload unit evidence；P2b 的 LoopDomain fixture 不得冒充 Python production runner，P2d 必须覆盖 reference Python worker protocol/kill/cleanup |
| Runtime 研究已把 P2a 定义为 PortBinding/Mailbox fixture，明确该阶段不运行 Card callback；P2b 才开始可信 Rust LoopDomain callback 执行 | 高，Research + Plan | “binding fixture 已通过”不能被宣传成 Card 已可运行；首个 in-process callback Harness 属于 P2b，多语言 production-equivalent worker 证据属于 P2d |
| Runtime/Graph 研究要求 RuntimeAssemblyEngine 只消费 Slice，steady Message path 不经过中央 graph loop | 高，Research + Plan | P2b Harness 复用最小 apply/callback seam，P2e 再复用完整 assembly contract；都不能发明 test-only graph executor |
| Kernel/Fabric 研究将 Health/Readiness value/fact 与 probe execution、聚合、告警和展示拆开 | 中高，Research | Kernel 不需要万能 Probe scheduler 或业务自检 registry |
| P3–P5 只允许仿真物理闭环，真实设备必须等待 H1 Hardware Enablement Gate | 高，Plan redline | 通用 Card Harness 必须 effect-denied，不能因为单卡测试方便而绕过真实硬件 gate |

这些约束中只有 Accepted ADR 是现有架构裁决；Research 与 Plan 是本轮设计输入，不能冒充实现事实。

### 3.2 PhanthyMotus 与 phanthymotus-driver

| 证据 | 强度 | 中立结论 |
| --- | --- | --- |
| Canvas Card 保存 `mcpId`、`toolName` 和 `card.id`，并把 `card.id` 作为 `instance_id` 调用聚合 endpoint 后的 start/stop/info Tool | 高，公开代码 | UI Card 的一次使用身份与后端实现聚合已存在，但这不是独立 Card 进程或独立 endpoint |
| Driver/Perception Plugin 通常由 Bundle 装配后共同暴露一个 MCP endpoint；Plugin 没有统一独立 CLI | 高，公开代码 | “能力可单独操作”与“实现单元可独立装载/运行”不是同一件事 |
| Driver 进程可以脱离 Agent Core 继续提供 MCP，但通常仍依赖 ROS、设备 SDK 或硬件；健康检查也混有 MCP ping、容器状态、HTTP endpoint 和设备 health Tool | 高，公开代码 | 存活、协议可达、容器运行和设备健康不能归并为一个通用 Probe |
| start/stop/info 的返回检查、fire-and-forget stop、文档端口与 manifest 端口、host-network 端口之间存在不一致风险 | 中高，公开代码 | 单组件开发入口仍需 canonical config、强终态 Receipt 和端口/资源冲突验证，不能以请求已发出当作启动或停止成功 |

公开行为证据来自 PhanthyMotus 的 `agent-core/web/js/canvas.js`、`agent-core/src/api/mcp_manage.py`、Perception bundle/plugin，以及 phanthymotus-driver 的 bundle、plugin、manifest 与 health 路径。ParaEGOX 可以借鉴“按实例操作”和“聚合实现”的经验，但不继承其 UI 真相、MCP Tool 生命周期或不统一 health 语义。

### 3.3 受限历史工程经验

只读审查显示，允许实现对象自己承担 start/stop、线程/进程、event loop、Bus 绑定、probe 和状态上报时，容易出现以下中立失败模式：

- “独立启动”与正式部署分别创建资源，形成两个生命周期 owner。
- probe 与 start/stop 共享命令平面，检查动作可能写状态、修复或触发重启。
- callback、消息队列、线程和子进程退出的完成语义不一致，stop returned 不代表资源已清理。
- 同故障域 heartbeat 继续更新并不能证明 event loop 或关键工作路径可响应。
- local Bus fixture 与 production Fabric 的路由、背压、Schema 和完成语义漂移。
- test helper 手写运行配置，长期演化为不受 Deployment revision、fencing 和 provenance 约束的旁路。

ParaEGOX 只保留这些失败模式对应的中立需求，不复制私有代码、Schema、测试、路径或领域对象。

### 3.4 外部一手资料

| 来源 | 强度 | 对结论的影响 |
| --- | --- | --- |
| [Kubernetes Pod lifecycle probes](https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/#container-probes) | 高 | startup、liveness 与 readiness 的动作和门控语义必须分开；错误 liveness recovery 会引发级联故障 |
| [ROS 2 Managed nodes](https://design.ros2.org/articles/node_lifecycle.html) | 高 | 生命周期状态和 transition callback 可以由组件参与，但外部 lifecycle manager 才拥有状态推进与协调 |
| [ROS 2 launch_testing](https://docs.ros.org/en/rolling/p/launch_testing/) | 高 | 对真实进程的验证既要覆盖运行期间行为，也要覆盖退出后的结果、输出和清理 |

这些资料支持“分型”和“外部 owner”原则，不意味着 ParaEGOX 复制 Pod、ROS node 或 launch test API。

### 3.5 证据限制

- ParaEGOX 目前没有实现代码、性能基准或目标设备 Harness，因此本文不能证明 API 易用性或运行开销。
- PhanthyMotus/driver 证明了真实使用路径与失败风险，但其 MCP/ROS 架构不是 ParaEGOX 的 production contract。
- 没有两个独立 production 消费者证明需要公共 CardHarness 或通用 Probe framework；首版只能准入 internal fixture。
- 真实设备自检、commissioning 与 HIL 的设备特定合同尚未形成，本文只冻结不得绕过的安全边界。

## 4. 先把“独立”拆成四种语义

| 对象/诉求 | 是否允许 | 精确含义 |
| --- | --- | --- |
| 校验 `CardDefinition` | 允许 | 纯解析、Schema/Port/ExecutionRequirements/entrypoint compatibility 检查；不创建运行身份 |
| 语言内 workload unit profile | 允许 | 在 Python/Rust/C++ 各自单元测试中直接构造 ASR/TTS 私有实现，验证领域逻辑和窄 collaborator；不经过 Artifact/ProcessDomain 装载，不产生 CardInstance、Ready 或 recovery 结论 |
| 独立创建并启动 `CardInstance` | 禁止 | CardInstance 只能由 RuntimeHost 根据 canonical apply 创建和推进生命周期 |
| 只运行一张 subject Card | 允许 | 用 Runtime component Harness 隔离验证，或生成仅含一张 subject Card 的 Deck 走正式链路；不是 `Card.run()` |

“one-subject Deck”不承诺物理上只有一个 CardInstance。如果 subject Card 有必需 In/Out，L2 在已安装 test PortBinding 边界使用非 Card 的 source/sink adapter；L3 若确需 fixture Card，则它必须在编译前进入 ephemeral Deck、committed plan 与 Slice，并由 RuntimeHost 正常创建。无论哪种方式，只有一张 Card 是被验证主体，不能在投影后偷偷追加 fixture desired object。

## 5. 方案比较

### 5.1 方案 A：`CardDefinition.run()` / `Card.run()`

优点是演示和 notebook 看起来最短。真实代价是它必须自行选择 config、instance ID、Port binding、ExecutionDomain、readiness、recovery 和 cleanup；一旦可用于生产，就与 DeckCompiler、Planner、DeploymentController 和 RuntimeHost 形成第二条控制链。

**结论：拒绝。** 同样拒绝公共 `StandaloneRunner`、`RuntimeHost.run_card()`、由作者 new/start CardInstance、可持久化 standalone config/status 和直写 Runtime 的 CLI。

### 5.2 方案 B：只支持语言内 workload unit profile

它能提供最快反馈，也适合纯算法和 callback 逻辑，但无法证明：

- Artifact/entrypoint 到 CardInstance 私有实现的装载。
- PortBinding、Mailbox、ExecutionDomain 与 lifecycle callback 的协同。
- 同 CardDefinition 多实例隔离、generation/revision fencing。
- callback timeout、process crash、drain、recovery 与资源清理。

**结论：保留但不充分。** 语言内私有实现单测是第一层，不是 Artifact compatibility、ProcessDomain 或 Card 运行证明；Python direct-object 测试尤其不能外推为 RuntimeHost 嵌入 CPython。

### 5.3 方案 C：现在建立公共 CardHarness/Probe framework

公共 framework 看似能统一 SDK、CI 和调试，但当前没有两个独立 production 消费者，也没有冻结 Card SDK、Runtime API 或 device diagnostic contract。此时公共化会把测试便利固化成生产 API，并诱发 test package 被 production import、测试 proof 被 production trust 接受、任意 probe method 进入公共 Schema。

**结论：推迟。** 首版只准入 internal composition fixture；公共 CLI、持久 manifest 或 probe contribution Schema 都需要新的准入证据与 Proposed ADR。

### 5.4 方案 D：分层验证 + canonical internal Harness + one-subject Deck

这一方案同时保留纯逻辑反馈速度、Runtime 独立可测性和 production 等价路径：

```text
language-local private implementation
          │ workload unit fixture
          ▼
domain callback evidence

production projector/builder ──> canonical RuntimeApplyRequest
                                          │
                                          ▼
                                RuntimeHost component Harness
                                single subject CardInstance

ephemeral one-subject DeckSpec
          │
          ▼
DeckCompiler → Planner → DeploymentController → RuntimeHost → Inspection
```

**结论：推荐。** “ephemeral”只表示短寿命开发 workload，不表示无 revision、无 digest、无 owner 或无 fencing。

## 6. 推荐的五层开发与验证模型

### L0：CardDefinition 纯合同检查

输入是不可变定义、Artifact manifest/export metadata 和测试配置；输出只包含确定性 diagnostics。至少检查：

- 稳定 ID/version、引用和 digest。
- Port name/direction/interaction/cardinality/Schema compatibility。
- config Schema 与默认值的确定性。
- entrypoint/export 可解析且与目标 runtime profile 兼容。
- Service/Permission/Feature Requirement 分型。
- ExecutionRequirements 未用 `unknown` 冒充可证明 run bound。

L0 不 import/执行不受信实现，不创建 workspace、CardInstance、PortBinding 或网络连接。

### L1：语言内私有实现的 workload unit profile

作者可以在对应语言的普通单元测试中直接构造私有实现并逐个调用 callback。该路径不解析 production Artifact runtime kind、不启动 ProcessDomain，也不声称与 Runtime 装载等价。允许注入：

- virtual monotonic clock 与确定性 random seed。
- cancellation token。
- 录制型窄 In/Out handle。
- typed fake service client、permission-bound access handle 和最小 config。
- 只读 Receipt/Inspection fact sink fixture。

L1 fixture 强制禁止 raw Fabric、Zenoh Session、真实设备、production Secret、持久 host path、任意网络、无 owner Task/Thread/Process 和全局 registry。Python profile 至少构造两个对象，证明没有类变量、descriptor、cache、workspace 或 mutable default 泄漏；Rust/C++ profile 对应验证无共享 mutable singleton、`Arc<Mutex<_>>` 别名、裸指针或语言私有 runtime handle 泄漏。

L1 只能证明该语言内的领域函数、callback 和窄依赖合同；它不能声称 Artifact/entrypoint compatible、CardInstance 已 Live/Ready、ProcessDomain protocol/cleanup 已验证、Runtime 可恢复、production 等价或物理效果成功。

### L2：分阶段的 Runtime 单主体 Card component Harness

L2 是第一层真正创建 CardInstance 并运行 callback 的测试，但仍是 internal test composition：

1. fixture 使用 production `RuntimeSliceProjector` 与 `RuntimeApplyEnvelopeBuilder` 生成 `RuntimeApplyRequest`，或读取由同一路径生成并校验 digest 的 golden fixture。
2. RuntimeHost 始终是 CardInstance 唯一创建与生命周期 owner。P2b 只扩展 P2a 已有的最小 apply seam，以可信、同构建的 Rust fixture 验证 CardInstance/LoopDomain callback；P2d 再用版本化协议启动 Python reference worker，验证 heartbeat、IPC credit、cancel/kill、process-tree cleanup 和 stale generation。二者都不依赖尚未完成的完整 RuntimeAssemblyEngine；P2e 才由 RuntimeHost 通过内部 AssemblyEngine 验证完整 prepare/readiness/activate/drain/rollback。AssemblyEngine 和 ProcessDomain worker 都不形成第二 owner。
3. Harness 只能提交 apply/deactivate、注入已验证 Message、推进受控时间和读取 Receipt/facts；不能自行规划 placement、生成 ID、安装 binding 或调用私有 start/stop。
4. `PortBinding test fixture` 只在测试 composition root 注入；不进入 DeploymentPlan route 枚举、公共配置、provider registry 或 production package 依赖。
5. subject Card 若需要 In/Out，L2 只能在已安装的 test PortBinding 边界使用 source/sink adapter 注入已验证 Message、记录输出；adapter 不是 Card，也不能在投影后追加 desired object。若 fixture 本身需要 Card 语义，则必须在 L3 编译前显式进入 ephemeral Deck，并由 RuntimeHost 正常创建对应 CardInstance。
6. teardown 必须验证 Task、Thread、Process、Mailbox、FD、socket、SHM、workspace、retained payload 和 child process tree 全部归零。

L2 不证明 DeploymentController 的 tenure、journal、commit 或 reconcile；它只证明 Runtime 对 canonical contract 的组件行为。测试 principal/trust root 必须与 production 分离，production Runtime 不接受 test proof。

### L3：one-subject Deck 本地开发运行与 system smoke

需要 production 等价 lifecycle、dependency 和 Inspection 时，使用显式或生成的 one-subject Deck：

```text
DeckSpec {one subject Card + explicit fixture Cards/dependencies if needed}
  → DeckCompiler
  → DeckLock
  → DeploymentPlanner
  → single-writer DeploymentController commit
  → RuntimeSliceProjector / ApplyEnvelopeBuilder
  → RuntimeHost
  → Inspection / DeckRun
```

未来若提供 `dev run-card`、`card dev` 等命令，它只能：

- 读取 CardDefinitionRef 和 config，生成可打印/导出的临时 DeckSpec。
- 在编译前显式列出所需 fixture Card/ServiceRequirement；不得在 DeckLock、Plan 或 Slice 生成后追加 fixture。
- 调用正常 Deck/Deployment API，不直写 RuntimeHost。
- 以 metamorphic test 证明语法糖生成的 canonical DeckSpec 与用户显式 one-subject Deck 等价；在相同 source scope/revision、resolver inputs、previous allocation、target facts、policy 与 committed provenance 下产生相同 DeckLock、PlanContentDigest 和 target Slice digest。独立 deployment/commit 不要求 revision-bound Slice digest 相同。
- 将 stop/replace 请求提交给 DeploymentController，而不是直接停止 CardInstance。

CLI 名称和格式尚未冻结，也没有实现；公共化前必须完成工作项准入，若形成长期外部合同则需要 Proposed ADR。

### L4：Scenario、simulation、HIL 与 Field

L4 证明跨 Card、CoreService、Gateway、Driver、网络与物理 owner 的闭环：

1. **Scenario/simulation**：使用模拟 DeviceService/SafetyIslandAdapter 和 simulation provenance。
2. **HIL**：使用真实协议/设备接口和受控硬件环境；没有完整 H1 条件时仍只能 read-only/effect-denied，不能靠 Harness 开关获得运动/能量权限。
3. **Field/real**：绑定有效 HardwareEnablementReceipt、DeploymentRevision、HardwareActivationRef/Epoch、fresh DeviceReadiness/ODD、下游 safety gate、人工/物理隔离、回滚条件与最终 effect evidence。

L0–L3 成功不能外推到 HIL/Field；simulation Receipt 也不能满足 real/H1 gate。

## 7. Harness 的 canonical 输入、证据与退出条件

### 7.1 唯一输入链

Harness 不允许作者手写 `RuntimePlanSlice` 作为长期测试或开发输入。允许的 source 只有：

- production projector/builder 在测试中即时生成的 request；或
- 由同一 production path 生成、带 source/slice digest 和版本 provenance 的 golden fixture。

这不要求每个 Runtime 单元测试都启动 DeploymentController。Runtime 必须能在独立进程中只安装 runtime contracts 并消费序列化 Slice；但 test fixture 不能被部署 CLI、配置文件或 production API 复用成第二条 source-of-truth。

### 7.2 每次 Harness run 的最小 provenance

- CardDefinition/Artifact/config digest。
- runtime/test harness version 与 target profile。
- source plan revision、source digest、slice digest、target、instance/generation/DomainEpoch。
- seed、clock mode、deadline/fault schedule。
- typed input、expected output/fact、允许的 nondeterminism envelope。
- effect provenance：L0–L3 固定为不可由作者切换的 `denied`，只能记录派生事实；simulation/HIL/real 不进入通用 Card Harness 配置。

### 7.3 活跃期和关闭后证据

活跃期按阶段观察：

- P2b–P2d 观察 RuntimeHost-owned create/start/callback/drain/stop seam、deadline 与资源归零，不宣称完整 revision assembly 已验证。
- P2e 才观察 `prepare → readiness → activate → message/invocation → drain → stop/rollback`、apply 幂等、same-id/different-digest conflict、旧 revision/writer/epoch 与 CAS 拒绝。
- callback deadline、backpressure、late result 和 recovery budget。
- planned 与 observed Domain/PID/TID/capacity/epoch 一致，否则不得 Ready。

关闭后至少观察：

- lifecycle callback 返回不等于清理完成；必须有 RuntimeHost-owned cleanup Receipt/fact。
- 不存在孤儿 Task、Thread、Process、Mailbox、FD、socket、SHM、workspace 或未释放 retained payload。
- process crash/timeout 后在途副作用是 `Uncertain`，默认不 replay。
- 旧 generation 的迟到 output、readiness evidence 或 completion 被 fencing。

## 8. 不建立万能 Probe：五类语义必须分开

| 语义 | 事实/决策 owner | 能证明 | 不能证明 |
| --- | --- | --- | --- |
| startup completion | RuntimeHost 管理 lifecycle callback 与 deadline | 当前 generation 的 bounded prepare/start step 完成 | 持续 Live、业务健康或物理 Ready |
| RuntimeHost liveness | 不同故障域的 NodeDaemon 观测并产生恢复请求；OS service manager 独占整体进程 TERM/KILL/restart | RuntimeHost bootstrap/progress/control responsiveness/process tree 在推进 | 具体 Card 语义正确 |
| Domain/CardInstance liveness | RuntimeHost | 计划中的 invocation/heartbeat/run-bound/DomainEpoch 未失效 | 当前 revision 所有依赖已满足 |
| readiness | RuntimeHost 判定；内部 RuntimeAssemblyEngine 求值 | exact active revision 的 artifact/config/domain/resource/dependency/binding/startup/permission facts 与 Slice 一致 | 长期业务质量、设备效果成功 |
| health | source owner 产生事实，Inspection 只读投影 | 资源压力、overrun、最近 fault、degraded 原因与 freshness | 自动 stop/restart 权限或 desired-state 写权 |
| test observation/assertion | Harness | 该测试刺激下的可复现行为 | production truth、production readiness 或现场安全 |

每个 production profile 还必须指定唯一的 RuntimeHost restart-budget/quarantine ledger mutation owner；NodeDaemon 与 OS service manager 不能形成两个重启循环。NodeDaemon 的 liveness fact/恢复请求不等于已经执行进程动作，OS service manager 的 spawn/TERM/KILL 返回也不等于 CardInstance Ready。

因此首版不建立：

- 每张 Card 必须实现的 `probe() -> bool`。
- 公共 Probe registry、任意 command-string probe 或每张 Card 一个 HTTP `/health`。
- 由 Card 写自己的 lifecycle state、Ready 终态或 restart action。
- 把 Probe 暴露成 Agent Tool，或把 start/stop/info/config/probe 放入同一业务 action plane。

### 8.1 Readiness 的派生规则

RuntimeHost 可以把以下 exact-revision 事实合取为 CardInstance readiness：

- source revision、Slice digest、CardInstance generation 与 DomainEpoch 当前有效。
- Artifact/config 已验证并装载，observed Domain 与 allocation 一致。
- required PortBinding 已安装并处于正确 BindingEpoch。
- required Service/Feature/permission/resource 已满足且未过 freshness。
- `on_start` 在 deadline 内完成。
- implementation-specific semantic evidence 只有被当前 committed Slice 的 readiness contract 明确要求时才参与，且必须来自当前 generation 的窄上报通道并仍有效；CardDefinition 只是 Planner 的上游声明来源，Runtime 不读取它作为第二 desired truth。

Card 实现可以贡献“模型已 warm”“索引已加载”等有界证据，但 RuntimeHost 仍独占最终 readiness 判定。实现自报 `ready=true` 不能覆盖 missing binding、错误 revision、stale dependency、资源失配或已过期 Grant。

### 8.2 运行证据的最小约束

若未来确有实现特定诊断需求，候选的一向 fact 至少要绑定：

- subject 与 source owner。
- SourcePlanRevision、CardInstance generation 和 DomainEpoch。
- sequence、monotonic `observed_at`、`valid_until`/freshness。
- reason code、有限 evidence ref 与可观察值。

旧 revision/generation、乱序或 stale fact 必须降为 `Unknown`/失效，不能继续保持绿色。公共名称、Schema 和 reporter API 等待两个独立消费者与 ADR；当前不预建 `ProbeSpec`。

### 8.3 Probe/检查行为约束

任何被称为检查的动作都必须：

- 有界、可取消、只读或明确声明 effect class。
- 不在内部执行 repair、restart、desired config mutation 或业务操作。
- 不依赖被检查的同一 event loop 来证明该 loop 的 liveness。
- 不以 endpoint 可达、heartbeat、callback return 或 process exit 冒充 exact-revision readiness。
- 将“发现事实 → policy 求值 → RuntimeHost recovery action”分成三个 owner。

Health 可以 degraded 但仍 Ready；也可以 Live 但 NotReady。Readiness 丢失不自动等价于进程死亡，Health 告警也不自动授权 restart。

## 9. 物理设备与有副作用诊断

通用 L0–L3 Harness **强制 effect-denied，且没有可切换为 simulation/HIL/real 的配置开关**：

- 不签发真实 Device Grant、Lease、SafetyDecision 或 HardwareActivationRef。
- 只连接 typed fake owner；simulation Receipt 必须带 simulation provenance。
- `send/enqueue/accepted`、callback return、进程 exit 或 MCP/HTTP 200 都不能生成物理 `Succeeded`。
- handoff 后 crash/timeout 的结果为 `Uncertain`，restart 默认不 replay。
- `on_stop` 不是安全机制；safe output 依赖下游 safety island、deadman 与 fencing。

会移动、加热、写入、reset、calibrate 或改变设备模式的所谓“probe”，实质是受控诊断 Operation。它必须走：

```text
authorized Diagnostic Operation
  → AuthorityDecision
  → Lease / Fence
  → SafetyDecision
  → EnforcementPoint
  → device acceptance
  → applied/effect evidence
  → terminal Receipt or Uncertain
```

DeviceReadinessSnapshot 是设备/safety owner 的原子事实，不等于 CardInstance Ready。真实 HIL/Field Harness 必须与通用 Card component Harness 分开，并同时绑定有效 HardwareEnablementReceipt、当前 DeploymentRevision、HardwareActivationRef/Epoch、fresh DeviceReadiness/ODD、下游 safety gate、现场隔离条件与最终 effect evidence；任一项缺失、过期或换代都必须 fail-closed。

## 10. 最小验证矩阵

| 层 | 最小 fixture | 必须证明 | 明确不能声称 |
| --- | --- | --- | --- |
| L0 Unit | Definition/config/Port/entrypoint metadata | 确定性解析、无副作用 fail-fast | 可运行、Ready |
| L1 Unit | 某语言内两个 ASR/TTS 私有实现 + fake handles | 领域逻辑、状态隔离、虚拟时间与取消 | Artifact/ProcessDomain/Runtime/Deployment 等价 |
| L2 Component | one transform subject Card + canonical Slice + test PortBinding | Runtime lifecycle、callback、binding、epoch、cleanup | DeploymentController/tenure 已验证 |
| L2 fault | on_start failure、callback timeout、process crash、late old-epoch output | callback/Domain 有界失败、fencing、quarantine/no replay、无孤儿资源 | 完整 assembly/CAS、现场安全 |
| L3 System smoke | 显式 one-subject Deck | compile→plan→commit→apply→observe→deactivate 闭环；所有 fixture Card 在编译前声明 | 多 Node/真实设备已验证 |
| L3/P2e assembly fault | prepare/readiness/activate crash、重复 apply、same-id/different-digest、旧 writer/revision、CAS conflict | 旧 active 保持或 bounded rollback/quarantine；无 mixed revision/双 active | 多 Node rollout |
| L3 metamorphic | 便捷入口与显式 DeckSpec，在相同 source/revision/target/provenance 输入下求值 | canonical DeckSpec 与 DeckLock/Plan/Slice digest 等价、无 Runtime bypass | 独立 commit 的 revision-bound Slice digest 相同；CLI 已稳定 |
| L4 Scenario/HIL/Field | simulated → HIL → real profile | owner-specific Receipt、fault/recovery、安全 gate | 其他设备/ODD 自动成立 |

时间测试分为两类：纯 reducer、deadline 和 deterministic callback 优先使用 virtual monotonic clock；进程 wedge、OS scheduling、cleanup 和 watchdog 必须使用真实 monotonic time，并保留可重复的宽限 envelope。

## 11. 与 P2 实施阶段的关系

| 阶段 | 本研究增加的明确边界 |
| --- | --- |
| P1 contracts | 冻结 Card 不可直接 run、canonical fixture provenance、startup/liveness/readiness/health 分型；不冻结公共 Probe API |
| P2a Mailbox/PortBinding | 只验证 binding/admission/隔离；不执行 Card callback，不宣传“单 Card 已运行” |
| P2b LoopDomain | 首次实现可信同构建 Rust callback adapter 与 RuntimeHost-owned CardInstance/LoopDomain component seam；覆盖 startup、deadline、late result 和结构化清理，不依赖或声称完整 AssemblyEngine |
| P2c ThreadDomain | 同一 Harness 扩展 thread capacity、timeout、wedged、late result 与无虚假 kill 语义 |
| P2d ProcessDomain | 用 Python reference worker 冻结语言中立 protocol/runtime version、heartbeat、IPC credit、cancel/terminate/kill、process-tree cleanup、restart budget/quarantine、外部 watchdog 与 stale generation；不嵌入 CPython、不加载公共 Rust `dylib` |
| P2e Deployment control plane | 补齐 RuntimeAssemblyEngine 全阶段，并首次提供显式 one-subject Deck system smoke；所有开发语法糖必须等价走 compiler/planner/controller/apply 链 |
| P3+ | 只先连接 simulated physical owner；真实设备等待 H1，并使用独立 HIL/Field Harness |

P2a 的 test fixture 不能长期替代 P2e。反过来，也不要求所有 Runtime component 测试启动完整控制面；这两类证据证明不同边界。

## 12. 关键风险、反例与失效条件

### 12.1 最强反例

1. **每个 callback 单测都启动 Deck 太重。** 成立，所以 L1 允许在语言内直接构造私有实现；但这只是 workload unit profile，不证明 Artifact 装载、ProcessDomain、CardInstance 或 Ready。
2. **Runtime 需要在没有 deployment package 的环境里测试。** 成立，所以 L2 消费 canonical serialized Slice；但 fixture 不能成为 production source。
3. **模型 warm、索引加载无法完全由 Runtime 推断。** 成立，所以实现可以贡献 epoch/freshness-fenced semantic evidence；最终判定仍归 RuntimeHost。
4. **设备 commissioning/self-test 必须隔离运行。** 成立，但有副作用 self-test 是受权诊断 Operation，不是普通 health probe。
5. **未来可能有 CLI/batch/CoreService/Gateway 也复用同一实现。** 若出现两个真实生产消费者，应研究中性的 `WorkloadDefinition` 或其他合同，并用 superseding ADR 重审 ADR-0002；不能偷偷给 Card 增加 run。

### 12.2 实施风险与防护

| 风险 | 防护 |
| --- | --- |
| internal Harness 演变为生产 runner | production crate/package 不依赖 testing；无 public route/config/status；production trust 拒绝 test proof |
| golden Slice 漂移 | 只由 production projector/builder 生成；版本与 digest conformance test |
| 过度模拟掩盖 Runtime 问题 | L1、L2、L3 分层；每项声明绑定最低充分层级 |
| test PortBinding 语义偏离 Zenoh | P4 由相同 PortBinding/Mailbox conformance suite 覆盖三种 Zenoh locality |
| probe storm/级联重启 | bounded scheduler、freshness、recovery budget；probe 不直接执行 recovery |
| Card 自报永远绿色 | revision/generation/epoch/freshness fencing；RuntimeHost 合取外部依赖事实 |
| 物理自检绕过安全链 | L0–L3 强制 effect-denied 且无配置开关；有动作诊断升级为独立 L4/H1 Operation Harness |

### 12.3 推翻本结论所需证据

以下证据出现时可以重审，但需要 Proposed/superseding ADR：

- 至少两个独立 production 消费者需要同一定义和生命周期合同，而 Deck/Deployment 语义真实不适用。
- public CI、第三方 SDK 和本地开发工具共同需要稳定 CardHarness manifest/API。
- 两类独立 health/readiness contributor 证明需要公共 contribution Schema。
- 目标设备基准证明 current effect-denied → HIL → Field 分层无法满足 commissioning 流程，并能给出不削弱 Authority/Safety 的替代 owner。

## 13. ADR 影响与实施裁决

本研究当前**不需要再修改 Accepted ADR，也不创建新 ADR**，因为推荐路径严格服从 ADR-0001/0002，并按 ADR-0006 收窄了语言与进程边界：

- `CardDefinition`/`Card` 不可直接运行。
- RuntimeHost 仍是 CardInstance 唯一创建与生命周期 owner。
- DeploymentController 仍是 production desired revision 的唯一 writer。
- Runtime component fixture 只消费 production path 生成的 canonical contract。
- 可信 Rust in-process fixture 只使用内部 linkage；Python/C++ production-equivalent workload 只经 RuntimeHost-owned ProcessDomain，语言不产生新的 Card/CoreService 身份。

可以在后续 P2 批次直接实现的只有 internal test infrastructure 与既有 owner 内部状态机。以下任一项进入实施前必须先做工作项准入，若形成长期公共合同则进入 Proposed ADR：

- public `run-card`/`card dev` CLI。
- 可持久化 standalone/test manifest。
- public CardHarness/fixture/provider registry。
- public readiness/health contribution 或 Probe Schema。
- RuntimeHost 的第二种 production desired input。

## 14. 推荐实施顺序

1. 在 P1 定义内部 test provenance、clock/fault fixtures 和 startup/liveness/readiness/health truth table，不创建公共 API。
2. P2a 完成 production projector/builder conformance、PortBinding/Mailbox fixture 和两个 CardInstance 的绑定隔离；明确不执行 callback。
3. P2b 增加可信同构建 Rust adapter、internal single-subject component Harness 和 LoopDomain 生命周期/清理故障矩阵。
4. P2c 扩展 ThreadDomain；P2d 以 Python reference worker 冻结版本化 ProcessDomain 协议，并加入 IPC credit、真实 monotonic watchdog、cancel/terminate/kill 与 process-tree cleanup。
5. P2e 增加显式 one-subject Deck system smoke，证明 compile→commit→apply→observe→deactivate；所有 fixture Card 在编译前声明，先不发布便捷 CLI。
6. P3 仅以 simulation profile 连接受控 Operation 链；H1 以后单独建设设备限定的 HIL/Field Harness。
7. 等待真实独立消费者后，再决定公共 developer command、test manifest 与 diagnostic contribution Schema。

## 15. 最终裁决

**方向性结论：`revise` 后推进。**

- 推进：语言内 workload unit profile、可信 Rust LoopDomain fixture、Python ProcessDomain reference worker、internal canonical Slice 单主体 Harness和 one-subject Deck 正式链路。
- 拒绝：`CardDefinition.run()`、`Card.run()`、作者直接 new/start CardInstance、production StandaloneRunner、万能 Probe 与 test-only production route。
- 分开：startup、liveness、readiness、health、test observation 与 physical diagnostic Operation。
- 延后：公共 CLI、public Harness/Probe Schema、真实设备自检合同，直到出现独立消费者和 ADR 证据。

这样 Card 既能像一个高质量组件被快速开发和隔离验证，又不会因为“方便单独跑一下”而把 Deck、DeploymentController、RuntimeHost、Authority 与 Safety 的唯一所有权重新拆成两套系统。
