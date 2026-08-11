# ParaEGOX 分布式具身 Agent OS 缺口研究与演进计划

> 状态：Research Complete
> 日期：2026-07-29
> 深度：Deep
> 评审方式：主研究 + Agent 执行面、具身安全面、仓库边界反例三路独立审视
> 结论：`revise`——现有“小 Kernel + RuntimeHost + Core Services + Deck”方向成立，但在首段实现前必须补齐跨层契约脊柱，并把 Agent 执行面、物理时空面、安全供应链和验证面列为 Kernel Foundation 之后的正式工程分支
> 实现状态：本文是研究和计划输入，不是完成声明；长期约束仍需 Proposed ADR

> 后续裁决（2026-07-28）：[ADR-0001](../adr/ADR-0001-deployment-controller-boundary.md) 已保留 Kernel 外、按 DeploymentScope 单写的 DeploymentController，并进一步分开 DeploymentPlanCandidate 与 committed DeploymentPlan、tenure-neutral RuntimeSliceProjector 与 writer-context apply builder、DeploymentTenureAuthority 与 Runtime writer_fence/prepared/active journal。本文中较早的“Planner/Compiler 生成 DeploymentPlan”按该 ADR 解释为生成 candidate；只有 DeploymentController 原子提交后才形成权威 plan/revision。后置的是 HA、共识和复杂自动 placement，不是全局 desired-state owner。
> 后续研究（2026-07-29）：[Application、Deck、Card 与 Service 边界](application-deck-card-service-boundaries.md)将 Deck 收窄为 executable workload，规定 DeckTopology 内嵌并受 DeckLock digest 覆盖；正式 Application/Installation 与 application-owned durable service 等待多 Deck、稳定安装或跨 DeckRun 私有状态证据。
> 后续研究（2026-07-29）：[Graph Foundation、领域图与执行边界](graph-foundation-and-domain-execution-boundaries.md)拒绝 Kernel 通用 Graph Engine，细分领域图 owner，并补齐 RuntimeAssemblyEngine 与 DeploymentPlan activation contract；本节 Graph 结论以该研究为准。
> 后续裁决（2026-07-29）：[ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 已接受“Rust-first mechanisms，polyglot workloads”。首个生产参考以 Rust 实现 Kernel、RuntimeHost、执行/进程治理、NodeDaemon、Zenoh Fabric，以及 ADR 指定的 Deployment 与 Evidence/Inspection 机制；Python、C++、模型、Agent 与设备代码通过版本化 ProcessDomain、独立 Service 或 Gateway 接入。该裁决不改变本文的 owner 边界，也不把 Rust 等同于硬实时或功能安全。

## 一句话结论

ParaEGOX 目前不缺一个更大的 Kernel，也不缺一个通用 Graph/VFS；真正欠缺的是贯穿“身份—意图—工具/动作—授权—资源—安全—执行—回执—验证”的稳定契约脊柱，以及建立在它之上的 durable Agent execution、物理时空 Grounding、设备与安全生命周期、可信制品、可恢复运维和 SIL/HIL 验证闭环。

## 1. 研究问题与成功标准

本研究回答四个问题：

1. VFS、Card、Deck、Capability 以及 EAGOS 已有但 ParaEGOX 尚未显式承接的能力，应该放在哪里？
2. 当前 Kernel Foundation 已经覆盖什么、只覆盖了名词但没有完整协议的是什么、完全缺失的是什么？
3. 面向分布式具身 Agent OS，Kernel 之后最值得建设的系统结构是什么？
4. 哪些“先进结构”应现在纳入路线，哪些只能条件触发，哪些应该明确拒绝？

成功标准不是列出最多的子系统，而是每个候选都能回答：

- 它解决哪条真实失败路径。
- 谁拥有 desired state、observed state、持久状态与最终 Receipt。
- 是否必须进入 Kernel；如果不是，属于 Runtime、CoreService、CardDefinition、Driver、Gateway、Deployment 还是离线工具链。
- 第一个生产者、消费者和可重复验证入口是什么。
- 不实现它会留下什么债务，过早实现它又会制造什么债务。

## 2. 范围、假设与非目标

### 2.1 范围

- 单 Node 多进程、跨 Node、设备—边缘—云与多机器人协同的共同契约。
- Agent loop、harness、session、tool、memory、world、verification 与外部互操作。
- 物理 Command、设备、坐标/时间/不确定性、安全控制与恢复。
- Zenoh-native Fabric、生命周期、资源、Evidence、OPS、制品和升级。
- 从 deterministic fixture、simulation、shadow 到真实硬件的验证顺序。

### 2.2 假设

- Zenoh 是唯一生产 Fabric，ROS2/DDS 通过 Gateway/bridge 接入。
- 生产机制采用 Rust-first，业务工作负载保持多语言；Rust 由 Cargo workspace、`Cargo.lock` 与 `rust-toolchain.toml` 管理，Python SDK、worker、Agent、模型和治理工具继续由 `uv`、`pyproject.toml` 与 `uv.lock` 管理。
- Rust 只改变首个参考实现的语言选择，不改变 ExecutionDomain、owner、fencing、Evidence 或安全边界；硬实时与最后安全闭锁既不依赖普通 Python event loop，也不因使用 Rust/Tokio 自动成立。
- ParaEGOX 基于 PhanthyMotus 的合法代码血缘重新建设；EAGOS 只提供中立工程经验，不移植其源码或领域抽象。
- 当前仓库尚处于 clean-slate 文档阶段，因此可以修正包边界，不能把设计文档当作实现事实。

### 2.3 非目标

- 现在决定数据库、向量库、模型供应商、容器平台或机器人产品形态。
- 现在实现通用工作流引擎、Fleet 平台、Marketplace 或多控制器共识。
- 宣称任何具体工业安全标准合规。
- 用一套 Schema 统一 Agent、World、Memory、Evidence、Telemetry 与 DeckTopology。

## 3. 证据与置信度

### 3.1 本地证据

| 证据 | 用途 | 强度 |
| --- | --- | --- |
| 当前 Kernel/Runtime/CoreService、分布式模型、CardDefinition/Card/Deck、执行模型、Fabric/Evidence/Security 文档 | 重建已经采用的 owner、不变量与计划 | 高；但均为 Draft，不是运行证据 |
| EAGOS 的 Bus/VFS/Runtime、lane、线程/进程、Evidence 与 OPS 失败路径审阅 | 提取“万能 owner、隐藏 backlog、生命周期混合、结果冒充”等反例 | 高；只转化行为需求，不复制实现 |
| PhanthyMotus 的 Card、CanvasLayout/Project、PerceptionBundle、ROS2/设备接入与单体 Agent 结构审阅 | Card 有实际代码血缘；Deck 是 ParaEGOX 对 CanvasLayout + Project 的声明式演进；PerceptionBundle 仅为 MCP endpoint 内的实现聚合，ParaEGOX 当前不把裸 `Bundle` 建成公共总概念或 Deck 别名，同时识别需要拆开的故障域 | 中高 |
| 2026-07-24 public embodied-agent-OS 生态快照（30 个仓库、13 篇论文） | 对照 Agent permissions、durable session、robot data plane、loop、memory 与 sandbox 结构 | 中；项目成熟度与目标不同 |

### 3.2 外部一手资料

- Zenoh 1.9 已增强 async/single-thread 使用方式、advanced pub/sub、连接监测和重连等能力，支持继续走 Zenoh-native，而不是先抽象成最低公分母多 Backend；但是否满足具体 payload 和控制 SLO 仍需目标硬件 benchmark。[Zenoh 1.9 release](https://zenoh.io/blog/2026-04-16-zenoh-longwang/)
- Zenoh ACL 能约束 pub/sub/query/liveliness 等传输操作，但它不表达 ParaEGOX 的物理 Lease、Safety、实例 audience 与应用 Receipt，因此只能作为纵深防御。[Zenoh access control](https://zenoh.io/docs/manual/access-control/)
- MCP 授权强调资源 audience 绑定并禁止 token passthrough；其 Task 是协议能力，不能成为 ParaEGOX 内部 durable AgentSession 的唯一真相。[MCP authorization](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization)、[MCP tasks](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/tasks)
- A2A Agent Card 是外部 Agent 自描述与发现对象，和 ParaEGOX 的应用 `Card` 同名但语义不同；该协议类型必须留在 Gateway 边界或使用限定内部名。[A2A agent discovery](https://a2a-protocol.org/latest/topics/agent-discovery/)
- OpenTelemetry semantic conventions 和 GenAI conventions 会继续演进，适合由 exporter/adapter 映射，不应反向冻结 ParaEGOX Evidence Schema。[OpenTelemetry semantic conventions](https://opentelemetry.io/docs/specs/semconv/)
- SPIFFE 的 workload identity、短期轮换身份和 trust domain 可作为 Node/Workload identity 的参考或 adapter；它要求足够的工作负载隔离，不能只加一个 ID 字段就宣称可信。[SPIFFE concepts](https://spiffe.io/docs/latest/spiffe/concepts/)
- SLSA 与 Sigstore 分别提供构建 provenance 分级与签名验证参考，说明 Artifact admission 需要 digest、provenance、签名与策略，而不只是 URL。[SLSA tracks](https://slsa.dev/spec/v1.2/tracks)、[Sigstore verification](https://docs.sigstore.dev/cosign/verifying/verify/)
- ROS 2 实时设计明确区分 real-time 与 non-real-time 路径，强调非实时线程不能阻塞或抢占实时线程，并需要预分配与 benchmark；这支持把硬安全/高频控制留在独立故障域。[ROS 2 real-time proposal](https://design.ros2.org/articles/realtime_proposal.html)
- ISO 3691-4:2023 覆盖无人驾驶工业车辆及 AMR 的安全要求与验证，但 ParaEGOX 只能把它当作产品安全 case 的输入，不能凭通用 OS 架构宣称合规。[ISO 3691-4:2023](https://www.iso.org/standard/83545.html)
- WASI Component Model 正快速演进并已进入原生 async/stream/future 阶段，适合作为未来不可信插件隔离候选，不适合成为首版 Foundation 的强依赖；Linux seccomp 自身也明确不是完整 sandbox。[WASI Component Model FAQ](https://component-model.bytecodealliance.org/reference/faq.html)、[Linux seccomp](https://docs.kernel.org/6.2/userspace-api/seccomp_filter.html)

### 3.3 推断与开放问题

本文将“某结构能避免已见失败模式”标为架构推断，而不是实现事实。以下问题必须靠 prototype/benchmark 回答：

- Zenoh SHM、普通 payload 与 GPU/device buffer 在目标硬件上的 p99.9、copy 与 lifetime 成本。
- Rust RuntimeHost、Python/C++ ProcessDomain、native control runtime 或独立控制器各自能承诺的 worst-case deadline；语言或内存安全本身不能替代目标平台测量。
- 首个真实机器人产品需要的 Operational Design Domain、风险分析和人工接管流程。
- durable AgentSession、Evidence 与 World State 是否需要独立存储，还是早期可共享同一物理数据库但保持逻辑 owner。

## 4. 当前基线：已覆盖、薄弱与缺失

| 领域 | 当前状态 | 裁决 |
| --- | --- | --- |
| Kernel/Runtime 边界 | 已覆盖：小 Kernel、RuntimeHost、ExecutionDomain、Mailbox、liveness/recovery、revision | 保留；补 import 边界和 apply contract |
| CardDefinition/Card/Deck | 已覆盖：类型、使用、运行身份、executable workload 组合分离 | 保留；全部在 Kernel 外 |
| Fabric | 已覆盖：Zenoh-only production、无 LocalBus、single-active-route、bounded ingress | 保留；补大 payload、Schema/codec 与 support report |
| 分布式 owner | 已覆盖：Node、epoch/revision、local-first safety、reconciliation | 保留；补 workload identity、升级兼容与运行期 dependency loss |
| Authority/Lease/Safety | 结构已覆盖，协议仍薄 | 补 Decision-to-command digest、approval、mode handoff、arbitration、device completion 与 Evidence commit |
| Evidence/OPS | 分层已覆盖，durable commit 与控制操作协议薄 | 拆为 P6a local commit/node-local Inspection 与 P6b federated Inspection/OpsService，不进 Kernel |
| 时间与物理语义 | Deadline/Freshness 有，frame/unit/calibration/uncertainty 太晚 | 在首个物理闭环前增加 physical contracts |
| Agent 执行 | 只有候选 CoreService 名称 | 建立独立 Program：Session、Harness、Tool、Context、Memory、Verifier、Eval |
| 安全与供应链 | Authority 有，workload identity、Secret、sandbox、artifact trust 薄 | 分阶段增加；不把策略与凭证塞入 Kernel |
| 验证 | 并发/Fabric Harness 已较完整，Agent/仿真/HIL/安全 case 缺失 | 扩成 scenario-based system evidence |

## 5. 四个立即裁决

### 5.1 VFS：现在不做，也不进入 Kernel

VFS 会把 Artifact、Evidence、Secret、Workspace、Memory、State 与设备 I/O 再次聚合成 URI 版 Service Locator。ParaEGOX 应使用 owner-specific typed reference：

| 引用 | 唯一 owner |
| --- | --- |
| `ArtifactRef` | ArtifactStore / Release |
| `EvidenceRef` | EvidenceService |
| `SecretRef` | SecretService 或外部 secret provider |
| `WorkspaceRef` | 未来按真实需求引入的 WorkspaceService |
| `BlobRef` / `BufferRef` | payload/transfer owner，带 lifetime、lease、size、digest 与 release 语义 |
| service state ref | 对应 CoreService 的 typed client |

CardInstance 的私有实现只获得依据 CardDefinition 与 active plan 编译的 scoped client/handle，不获得全局 `open(uri)`、scheme registry、万能 `ObjectRef` 或跨 owner traversal。`InspectionService` 可以提供 federated resource explorer projection，但它只是各 owner 的只读查询投影，不能成为数据权限和生命周期的第二真相；OpsService 只消费该投影，不拥有它。

只有至少三个真实 backend 需要相同的 read/list/watch/write/atomic-commit 语义，并且权限与生命周期仍能保持在 owner 处，才重新研究 Workspace/Storage facade；即使出现也属于 owner-specific CoreService，不属于 Kernel、InspectionService 或 OpsService。

### 5.2 Card 与 Deck：一等公共模型，但不在 Kernel

“重要、公共、稳定”不等于 Kernel。建议所有权保持：

```text
kernel/       通用 ID、Message、Port、deadline、Receipt、Grant/Lease 纯契约
cards/        CardDefinition、In/Out、ExecutionRequirements
decks/        Card、CardProfile、Link、DeckSpec、DeckLock、解析与编译
deployment/   DeploymentPlan、placement、reconciliation
runtime/      RuntimeHost、CardInstance、DeckRun、执行与 observed facts
```

Kernel 不知道 Card、Deck、CardDefinition resolution 或 DeckTopology。DeckCompiler 的唯一可持久产物是内嵌 canonical DeckTopology 的 DeckLock；DeploymentPlanner 不接收独立 topology。Card/Deck 可以是产品最核心的用户模型，仍然不应让底层调度和安全机制反向依赖它们。

当前产品语言“应用”不增加公共 identity：Deck 是工作负载，DeckRun/CardInstance/ServiceInstance 是运行事实，DeploymentScope 是写权范围。多 Deck 产品、稳定安装 identity 或应用私有持久状态出现真实消费者后，再研究 Deck 之上的 Application 控制/交付对象；它不能成为第二个 DeploymentController。

### 5.3 Capability：拆成授权、服务与支持事实

正式边界见 [Capability、Service Contract 与 Feature Support](../concepts/capability-service-feature-boundaries.md)：

- `CapabilityGrant` / `PermissionRequirement`：安全授权。
- `ServiceContractId` / `ProvidedService` / `ServiceRequirement`：服务接口和依赖。
- `NodeFeatureReport` / `FabricFeatureReport` / `DeviceFeatureReport`：observed support。

三者不共享基类。Deck/CardDefinition 只声明 Requirement，Authority 为具体实例和 revision 签发 audience-bound Grant；工具可见性、服务可发现性、目标支持情况和真正授权互不替代。

### 5.4 DeploymentPlan 与 Runtime 的反向依赖必须消除

研究时的旧草案曾同时要求 RuntimeHost 直接应用 `DeploymentPlan.execution` 与 `runtime → kernel`，这会让 Runtime 反向 import `deployment/`。ADR-0001 已将它修正为 Runtime-owned apply protocol：RuntimeHost 只消费 target `RuntimePlanSlice`，DeploymentPlan 与投影器留在 Deployment control plane。

冻结后的单向边界：

```text
DeckCompiler → DeploymentPlanner → DeploymentPlanCandidate
              │ DeploymentController atomic commit
              ▼
        immutable committed DeploymentPlan
              │ RuntimeSliceProjector
              │ projects immutable tenure-neutral target slice
              ▼
RuntimeApplyRequest {
  target_runtime_id, source_scope_ref, source_plan_revision,
  expected_active_target_slice_digest, source_plan_digest, target_slice_digest,
  plan_writer_context, operation_id, temporal_constraint, auth_proof,
  RuntimePlanSlice
}
              │ schema/apply protocol owned by runtime/contracts
              ▼
         RuntimeHost → observed facts / Receipt
```

`runtime/contracts` 只拥有 `RuntimePlanSlice/RuntimeApplyRequest` Schema 与 apply state machine，DeploymentController 拥有实际产生的 immutable slice value。Slice 只包含目标 RuntimeHost 必须执行的 Artifact entrypoint、InstanceId、ConfigSnapshot digest、binding/execution assignment、budget、编译后的 LivenessSpec、FailureContainmentSpec、RecoveryPolicy 或 policy ref 与 revision tags；不能复制 Authority/Deployment policy state。它不是第二份 desired truth：必须是某一不可变 DeploymentPlan revision 的 canonical projection，不能被 Runtime 或用户独立编辑。

RuntimeHost 可以重算并验证 `target_slice_digest`，但不能从局部 slice 重算完整 `source_plan_digest`；因此 issuer/auth proof 必须覆盖完整 canonical apply payload，并用 exact expected-active target-slice digest 做 CAS。source revision 单调性另行校验，不能替代 exact CAS。Runtime 不 import DeckSpec、DeckLock、DeploymentPlan、DeckCompiler、DeploymentPlanner 或 DeploymentController。

## 6. 建议的端到端契约脊柱

ParaEGOX 最核心的系统流不应是“Agent 调一个 Tool”，而应是：

```text
Observation / Query result
  → GroundedSnapshot {time, frame, unit, uncertainty, provenance}
  → AgentSession + ContextManifest
  → Intent / Plan / OutcomeClaim
  → ToolInvocation or Physical OperationSpec
  → Authority / human approval / data-flow decision
  → Resource lease + fencing + command arbitration
  → Local Safety + Driver / EnforcementPoint
  → Effect Receipt + Evidence commit
  → independent Verifier
  → Session/Eval outcome + reconciliation
```

其中每一箭头都可能 timeout、cancel、partition、restart、duplicate、become stale 或返回 `Uncertain`。Planner 永远不获得 raw Driver、raw Zenoh、宿主凭证或绕过 EnforcementPoint 的句柄。

## 7. Foundation 层还缺的跨层合同

### 7.1 Failure 与公开安全的错误信息

需要一个小而稳定的 `Failure`/`Problem` 值：code/class、origin、stage、correlation、retry/reconcile hint 与 public-safe detail。内部 exception、敏感 prompt、token、设备地址和 stack trace 不直接跨信任边界。不要建立庞大中央错误注册中心，也不要让每个服务只返回自由文本。

### 7.2 四种不能混的状态

- `Liveness`：进程/loop 是否仍产生可验证进展。
- `Health`：组件是否发现内部故障。
- `Readiness`：是否能为当前 revision 接受工作。
- Feature availability：目标能力是否支持/降级/未知。

它们可以是小值或投影，不需要四个服务；但 OPS、reconciliation 与 dependency policy 不能把 heartbeat 当 Ready。

### 7.3 Cancellation、Timer 与时间域

除 Deadline/Freshness 外，还需 `CancellationScope` 的 parent/child propagation、ack、escalation 和 late-result fencing；Timer 需要 missed-tick、coalesce、skip/catch-up、overrun 与 shutdown 语义。

物理数据必须区分 sample time、receive time、wall/HLC、owner-local monotonic 与 `ClockDomainRef`；跨 Node 不直接比较 monotonic 值，时钟同步误差需要显式 uncertainty。

### 7.4 Config 与 State owner

首版配置权威链应为：

```text
CardDefinition ConfigSchema
→ CardProfile values + SecretRef
→ DeckLock resolution
→ DeploymentPlan ConfigSnapshot digest
→ immutable validated config
→ observed config digest
```

配置变化产生新 `DeploymentRevision`；首版不原地 hot reload。当前 State 只支持三类：CardInstance 私有可重建状态、平台 CoreService 自有持久状态、Driver 投影的外部权威状态。只属于一个稳定产品安装、跨 DeckRun 持久的状态是明确 gap：不能藏入 Card 全局变量或冒充平台 CoreService，需求出现时触发 Application/Installation owner、state schema/migration/retention/GC 研究。不要先建通用 StateStore/checkpoint framework。

### 7.5 Blob/Buffer lifetime

音视频、点云、图像、GPU buffer 与 SHM 不能塞进普通 Message bytes 后忽略所有权。需要 `BlobRef/BufferRef` 的 size、media/schema、location、digest、producer epoch、read/write mode、retain/release、expiry/lease 和 copy/zero-copy observability。自动释放前必须考虑 Mailbox、inflight、IPC、late result 与 recorder retention。

### 7.6 运行期依赖丢失

ServiceDependencyGraph 不能只解决启动顺序。每个 requirement 必须说明 provider 在 Ready 后消失时由谁执行 `degrade / stop / rebind / restart / fail-closed`、如何防抖、如何传播 not-ready，以及恢复时如何防旧 client/epoch 复活。

### 7.7 多资源 claim、Schema 与 recorder

需要多个设备/GPU/buffer 的 Operation 不能逐个拿 lease 后永久等待。`ResourceSetRequirement` 应由单一 owner 采用 canonical ordering、all-or-none local claim 或显式 prepare/abort；跨故障域默认不宣称原子，只提供 deadline、补偿与 deadlock diagnosis。

Schema/codec 还需要 digest、compatibility window、显式 Adapter、cache invalidation 与 binding-time negotiation；是否引入分布式 Schema Registry 等至少两个独立发布方出现后再决定。Recorder/Replay 记录的是带 revision/provenance 的边界输入、Receipt 与 Blob refs，不是 VFS 或全局 event sourcing；replay 默认进入 simulation/shadow，不能重新执行真实 physical effect。

### 7.8 Command 需要窄交互合同

P2a 的静态 1:1 `Signal/Event` Link 足以验证 Mailbox，却不能支撑 P3 物理命令。P1 必须同时冻结一个窄的 `Command` interaction contract，但 P2 不必马上实现通用 RPC 或动态 routing：`Controller-role Card`（控制应用）只拿到 typed `OperationClient`，它向计划安装的 `CommandEndpoint` 提交带 deadline、command digest、idempotency key 与完整 enforcement chain 要求的 Operation。Endpoint 产生阶段 Receipt，并明确 `Rejected/Accepted/Started/Succeeded/Failed/Uncertain`。

这是 Port/Binding 的受约束交互，不是 Controller-role Card→Driver 直接方法调用，也不是新的 Bus。P3 只需静态 1:1、单目标、bounded outstanding、无透明 retry 的实现；fan-out、partial acceptance、动态 routing 和跨资源 transaction 继续保持 unsupported，直到各自 owner 与失败语义被研究。

完整控制链不能由 Endpoint 自选：Operation effect class、resource contract 与 DeploymentPolicy 编译 Authority/approval/Lease/Safety/Enforcement requirement，Endpoint 只能加强，不能 opt-out。非物理 Command 使用明确不同的 effect class；不能靠同一个 Port 上的 `requires_authority=false` 绕过 write/physical enforcement。

## 8. 具身与物理面欠缺

### 8.1 Physical data ABI 应早于 SpatialMap

完整 FrameGraph、地图和语义导航可以留到后期，但 P3 前至少需要独立 `physical/contracts`：

- `FrameRef`、frame epoch 与 transform revision。
- 单位、坐标约定与 axis convention。
- sample timestamp 引用 Kernel `time/` 唯一定义的 `ClockDomainRef`、ClockQuality、ClockMapping/ClockMappingRevision 与同步 uncertainty；`MonotonicInstant`、`WallInstant`、`SimInstant` 是不可互换的类型，physical/contracts 不重复定义时间域。
- pose/measurement uncertainty 或 covariance 的可选表达。
- `CalibrationRef`、calibration revision、source 与有效期。
- sensor/driver provenance 与 freshness。
- `DeviceRef`、`DeviceIncarnation`、channel/sequence 与 simulation/real origin。

该层可依赖 Kernel 的 ID/time 基础值，但不把 TF graph、geometry engine、SpatialMap 或 World Model 塞入 Kernel。

Package 边界必须冻结为：Kernel 只提供通用 ID/time/Grant、ControlledResourceRef/ResourceSet、Lease/Fencing/Receipt 机制；`physical/contracts` 拥有 Device epochs/readiness、Observation、Frame/Unit/Calibration、Operation/PhysicalCommandEnvelope、ControlMode/Safety contracts；`runtime/enforcement` 和 Driver 才实现执行。物理类型引用 Kernel 基础值，不反向把 DeviceIncarnation、SafetyDecision 或 PhysicalCommand 放进 Kernel。

P1 必须冻结 Observation 的不可变可信头部；开放的只能是字段编码和包布局，不能再把语义本身留到真实 Driver 阶段：

```text
ObservationHeader {
  device_ref, device_incarnation, device_session_epoch,
  driver_binding_epoch, channel_ref, sequence,
  measured_at, received_at,
  clock_domain_ref, clock_mapping_revision, time_uncertainty,
  frame_ref, frame_epoch, transform_revision,
  unit, dimension, calibration_ref, calibration_revision,
  quality, validity, covariance?,
  origin, environment_ref?, provenance
}
```

只有受信 Driver/Scenario boundary 能从边界事实生成 physical origin/device/frame/calibration 等头部；普通 PortBinding 只能验证 transport principal/BindingEpoch 或添加独立 transport receive metadata，不能把 CardInstance 自报 payload 提升为真实设备 provenance。`unknown uncertainty` 不得解释成零；`measured_at` 必须位于声明的 clock domain，跨域比较必须通过带 `measured_at/valid_until/source/uncertainty` 的 `ClockMapping` 及其 `ClockMappingRevision`。Lease、deadline 与本地 watchdog 只使用 owner-local monotonic 时间。即使 `FrameRef` 未变，frame/transform/calibration revision 更新也会使旧 Observation 对依赖新 revision 的 consumer 失效。

Kernel 只拥有 ClockMapping Schema 和纯比较规则，不签发运行值。具体 mapping value/revision 由源时钟的受信边界 owner 产生：Node time-sync adapter 拥有 Node/system clock mapping，Device/Driver adapter 拥有 device clock mapping，ScenarioRunner 拥有 simulation clock mapping；每个值携带 producer identity/epoch、freshness 与 uncertainty。Consumer、Deployment 和 OPS 只能验证或投影，不能自造“更精确”的 mapping；暂不建立一个假装拥有所有时钟的全局 ClockService。

### 8.2 Device lifecycle

真实设备前必须定义：设备稳定身份与发现、`DeviceIncarnation`、`DriverBindingEpoch`、Driver binding、hotplug、firmware/ABI compatibility、初始化、校准、ready、reset-required、poisoned/quarantined、断连、device completion 与接管。Process crash 不能证明设备动作已停止；新 owner 在 completion/reset/fencing 可证明前不能接管。

`DeviceRef` 是物理设备/接口身份，`ControlledResourceRef` 是可租赁、可执行和可 fencing 的控制面资源；二者不继承、不互换。版本化 `PhysicalAssemblySpec` 是 Device↔Resource desired mapping 的唯一声明，DeploymentController 只选择并激活其 revision/digest；DeviceService 报告 observed realization/readiness，不修改 desired mapping。一个 Device 可以暴露多个 Resource，一个 Resource 也可以聚合多个 Device，纯传感 Device 可以没有可写 Resource。

不要用一个 `DeviceStatus` 大枚举混合所有事实。至少分别观测 presence、binding、operational、control 与 maintenance 状态，每条事实带 owner、observed_at、valid_until、device incarnation 和 revision；`Ready` 是 DeviceService 基于当前 Requirement 导出的结论，不是 Driver 随手写的布尔值。

事实 owner 与 epoch 不能揉成一个 `device_epoch`：

| Facet | 权威 owner | 至少携带的代次 |
| --- | --- | --- |
| 原始 presence、observed firmware/ABI/config、硬件 ack、Device boot/session | Driver/设备 adapter | DeviceIncarnation、DeviceSessionEpoch、observed fact revision |
| 当前 Driver binding | Driver binding owner | DriverBindingEpoch |
| 归一化 status/readiness | DeviceService | DeviceReadinessSnapshot：全部输入 revision + derived-at/valid-until |
| 期望 binding、firmware/config、assembly | DeploymentController | DeploymentRevision、PhysicalAssemblyRevision、firmware/config revision |
| 校准 | Calibration owner | CalibrationRevision、effective range |
| lease/fence | ResourceCoordinator | LeaseIssuerEpoch、LeaseId、FencingToken |
| safety inhibit/reset | 独立 safety island | SafetyEpoch + safety-function-specific revision |
| 控制模式 | mode owner | ControlModeEpoch |

USB path、序列号甚至 `DeviceRef` 均未变化，也不能证明设备未重启；只要硬件 session 重新建立，就必须推进 `DeviceSessionEpoch`。Firmware/config、calibration、assembly、mode 与 safety 的变化也分别推进自己的 revision/epoch，不能借 DriverBindingEpoch 代替。

DeviceService 只能从一次原子输入快照派生 `DeviceReadinessSnapshot`，不得把旧 session presence、新 calibration 与另一 firmware/ABI 组合成 Ready；任一输入换代立即使旧 snapshot 失效。真实 PhysicalAssembly 与设备的 realization 还需要 commissioning/binding Receipt，证明 identity、firmware/ABI、Driver、frame/resource mapping、calibration 与 safety path；完整 commissioning UI/workflow 可以后置，这份事实证明不能后置。

### 8.3 Control mode 与 ownership transfer

`manual / autonomous / maintenance` 等 control mode 不是 UI 标签；`emergency` 属于具体 safety function/state，不是一个可以与控制者竞争的 mode。Mode owner 需要 `ControlModeEpoch`、进入/退出条件、操作者身份、deadman/enable、lease 转移、进行中 Operation 的处理和 Receipt。安全 handoff 至少执行 `request → stop/quiesce → observed-safe/neutral → release old lease → acquire new lease → activate new epoch`；仅推进 epoch 只能挡住迟到命令，不能停止设备中已经执行的动作。

Resource lease 解决“当前谁控制”，而 command arbitration 还需 `CommandSequence`。Sequence 的作用域固定为 `ControlledResourceRef + LeaseIssuerEpoch/LeaseId + ControlModeEpoch`，并明确 duplicate、gap、supersession 与被替换 Command 的唯一终态 Receipt。Safety 是 clamp/inhibit/fail-safe owner，不作为与 teleop/planner 竞争的“最高数字优先级 writer”；mode 切换后旧 epoch 的迟到命令必须拒绝。

跨多个执行器的动作默认不宣称分布式原子执行；只能由同一物理故障域内可证明的 prepare/commit/barrier 实现，或显式声明非原子并提供安全补偿与 reconciliation。

所有写入使用不可变 `PhysicalCommandEnvelope/EnforcementContext`，绑定 command digest、requested operation digest、ControlledResourceRef/Operation、DeviceIncarnation/DeviceSessionEpoch/DriverBindingEpoch、PhysicalAssemblyRevision、CalibrationRef/CalibrationRevision、ControlModeEpoch、SafetyEpoch、LeaseIssuerEpoch/LeaseId/FencingToken、deadline、sequence、idempotency key，以及 `AuthorityDecisionRef/digest`、`SafetyDecisionRef/digest`。每个 Decision 本身再绑定 command/operation digest、resource、device/session、audience、policy/epoch 与 expiry；同一 policy revision 下 Command A 的 allow 不能用于 Command B。

Safety clamp 后必须产生 permitted/applied operation digest，不能把“较小动作已执行”报告成原请求完全成功。EffectReceipt 至少区分 requested、authorized/permitted 与 applied operation digest，并记录 SafetyEpoch、device-send ack、device completion ack 与 observed-effect evidence level；只有 Operation contract 要求的 completion level已满足才能 `Succeeded`，SDK 返回或 enqueue/accepted 绝不能直接产生该终态。

Driver EnforcementPoint 是 normal-command path 的最后软件准入点：在把 setpoint 写入 safety gate/device interface 前原子比较全部事实。Safety island/设备原生 safety controller 位于其物理下游，拥有优先级更高的 enable/inhibit/clamp/safe-output gate 和最终 applied-output 证明；二者不是平级 writer。Safety trip 后迟到 Driver write 不能覆盖安全输出。若硬件没有这种下游仲裁或等价 device-native safety，H1 不得通过。

### 8.4 Safety island 与产品 safety case

高频 servo 与产品安全功能必须位于 Agent、Rust RuntimeHost/普通 Python worker dispatcher 与远端 Fabric 之外的 local safety island（MCU、PLC、安全控制器或经证明的设备原生故障域）。`SafetyIslandAdapter` 只把它的状态、许可和证据接入 ParaEGOX，绝不拥有实际安全功能，也不能用普通 `SafetyService` 名字冒充硬实时/功能安全边界。

E-Stop、protective stop、deadman/enable、travel/joint limit、collision inhibit 和普通控制器 `Stop` 不是一个可随意 clear 的通用 FSM：前五类由具体产品 hazard 分析决定独立输入、优先级、锁存、复位和安全输出；普通 `Stop` 只是仍需 Authority/Lease/Enforcement 的控制命令。E-Stop 输入不要求先持有 control lease，也不经过 Agent、Mailbox 或 Fabric 才生效。

跨软件边界至少需要带 freshness 的 `SafetySignal`、`SafetyEpoch`、function kind、inhibit state、safe-output/watchdog state、reset request/permit 与硬件 ack。Reset 必须绑定 SafetyEpoch、具体 safety function、Device/ControlMode、已认证操作者和硬件 ack；所有 trip source 都重新观测为 fresh/clear 后才允许复位。陈旧、跨 epoch 或重放的 clear/reset 一律不能解锁。

P3 的模拟 safety island 也必须具有可证明的独立推进与故障域：reference profile 使用独立进程、独立 clock/deadman 和直达 simulated actuator safe-output path；纯确定性测试可以使用单独推进的 state machine，但冻结/kill RuntimeHost loop 后它仍必须继续推进。把 SafetyIslandAdapter 和 safety model 都做成同一 Rust RuntimeHost/Tokio executor 或 Python event loop 内的对象不能通过这项验证。

“直达 safe-output”表示 safety island 控制 Driver normal setpoint 下游、物理上占优的 gate，不表示创建第二个平级 actuator writer。Trip 与迟到 normal Command 并发、随后 Driver 重启时，输出保持 safe，直到完整 reset + re-arm；最终 applied output/ack 来自 safety/device boundary。

每个真实产品还需独立的 Operational Design Domain/operational envelope、hazard analysis、安全状态、人工接管、验证证据与变更影响分析。首版 P3 使用模拟 actuator；第一个高风险硬件 write 必须把 durable Evidence 和 recovery gate 提前。

### 8.5 能源与热预算

电池、充电、热状态、峰值功耗、降频和 Operational Design Domain 会改变 placement、任务可行性与断网自治。Driver/设备 adapter 拥有 raw power/thermal telemetry，DeviceService 原子派生带 revision/freshness 的 `PowerState/ThermalState`；产品域 `OperatingEnvelopeEvaluator` 拥有绑定 ODD/World/state revision 的 `OperatingEnvelopeEvaluation`。Safety island/Adapter 消费这些事实并拥有 inhibit/SafetyDecision，Continuity/Admission 只消费，不接管。只有出现多个明确消费者和独立生命周期后才形成 EnergyService，不进 Kernel。

Operation contract 声明其所需 operating facts；对应 SafetyDecisionRef 与 EnforcementContext 绑定 PowerState/ThermalState/OperatingEnvelopeEvaluation ref、revision 和 freshness。在 Safety decision 后、normal setpoint write 前发生温度超限、ODD 越界或事实过期时，旧 Command 必须拒绝或由下游 gate进入安全输出；该规则在网络正常时同样生效，不只是 Continuity 行为。

### 8.6 PhysicalAssembly 与断网自治

需要一个版本化 `PhysicalAssemblySpec` Artifact，把 DeviceRef、FrameRef、joint/sensor/actuator/resource、碰撞几何、载荷/运动/热限制、CalibrationRef 与 Driver/firmware compatibility 绑定起来。它描述一台机器人、机械臂、固定传感器阵列或实验台的物理装配，但不引入中心化 `Robot` owner，也不拥有生命周期、权限或通信。

每个资源各写一个 `partition_behavior` 仍不足以表达断网自治。Deployment 应编译目标限定且不可变的 `ContinuityProfile`，固定 DeploymentRevision、PhysicalAssemblyRevision、CalibrationRevision、safety policy revision 与所有 model/map/policy/artifact digest，并明确最长断网时间、允许继续的动作/资源、预先衰减的 offline CapabilityGrant、本地时钟/电源/热/Operating Envelope/Evidence 容量门槛、安全停止/返航/人工接管以及重连 reconciliation。

Profile 由本地 `ContinuityController` 执行；它只组合这些约束，不接管 Authority、Resource、Safety、Device 或 Evidence 的事实所有权。所谓“本地 issuer”必须解释为上线期间预签发、scope/期限更窄的 offline authority，而不是断网后凭本地身份自授新权限。若主机重启导致 monotonic baseline 丢失，默认不得恢复离线自治；只有经验证的持久 boot counter/safe clock 或本地重新授权流程可以重新进入。断网不得延长云端授权，重连不得自动重放旧 Command，恢复必须从当前物理观测开始。

Continuity 还需要 episode 语义：RuntimePlanSlice 携带 Profile ref/digest、触发 dependency set、debounce/hysteresis 与 applied revision；首次满足 loss condition 时由 ContinuityController 创建 `ContinuityEpisodeId` 和 owner-local monotonic `offline_since`。多链路状态按编译的 dependency predicate 求值，短暂重连不自动结束 episode或刷新最大离线时长；只有连接稳定、desired/observed/physical state reconciliation 完成并产生 close Receipt 后才结束。ContinuityController 重启必须从可信持久 episode 恢复，否则直接进入停止/人工接管，不从零重计时。

### 8.7 仿真语义同构但信任不等价

Fake、simulation 与 real Driver 可以实现同一 contract，但 origin、clock domain、environment identity 与 trust level 必须不可伪造地进入 Observation/Evidence。sim time 可以暂停、步进、重置和倒跳；truth channel 与模拟 sensor observation 分离。仿真数据默认不能满足真实硬件 readiness 或授予真实硬件执行权。

`ScenarioManifest` 应固定 Runtime Artifact/ABI、DeckLock、DeploymentRevision、PhysicalAssemblySpec revision/digest、world/physics engine/assets、seed/timestep、model/calibration digest、fault plan 与 expected invariants。固定 seed 不等于承诺并行物理和 GPU 的逐位确定性。

P3 必须交付最小 `ScenarioRunner`，而不只是几个 fake：读取 immutable ScenarioManifest，隔离 world truth channel 与经 Sensor contract 生成的 Observation，并让 fake 与 sim Driver 运行同一可复用 conformance suite。origin/environment/provenance 只能由受信 Scenario/Driver boundary 标记；expected invariants/tolerances、采样、seed、timestep 与依赖 digest 都进入可复现证据。Real Driver 只在后续 H1 Hardware Enablement 中对同一 suite 和 HIL 扩展执行，P3 不连接真实 actuator。

P3–P5 的 physical write 结论只对 simulation profile 成立。第一次连接真实 actuator 前必须在 P6b 后进入条件式 H1 Hardware Enablement：Driver 提供 raw observed/commissioning facts，DeviceService 独占派生 readiness，Safety/Evidence 提供 real Driver conformance、HIL/timing、独立 E-Stop/物理隔离、回滚与人工接管证据，Release owner 签发绑定具体 device/assembly/deployment/artifact/ODD/限制/expiry 的 HardwareEnablementReceipt；否则不得把“仿真闭环通过”描述为硬件可安全上线。

目标 Node 的 local HardwareActivationGate 验证该 Receipt 与 RuntimePlanSlice，绑定当前 DeviceReadinessSnapshot、限制和 expiry，安装 `HardwareActivationRef/HardwareActivationEpoch`；real Driver activation 与每个 PhysicalCommandEnvelope 都引用它。Receipt 撤销/到期、readiness/ODD/revision 变化时，本地立即推进 epoch、disarm/inhibit/stop，即使 Fabric 分区也不等待远端。该 activation 只做本地 Release/Deployment admission，不替代每次 Command 的 CapabilityGrant、Lease 或 SafetyDecision。

## 9. Agent 执行面欠缺

### 9.1 Durable AgentSession

Agent 的长期真相不能依附某个 CardInstance、event loop 或 prompt。Agent 层至少需要 `AgentSessionId`、`AgentRunId`、`TurnId`、`StepId`、`AttemptId`、Harness/Context revision、Model/Tool invocation id。

规则：

- Session 是追加式、可恢复的逻辑历史；Context 是某次模型调用的可重建投影，两者不同。
- 每个 Session 分支需要 single-writer lease/epoch 或 CAS。
- retry 创建新 Attempt，不覆盖旧结果。
- restart 优先读取已提交 Tool/Model 结果，不静默重做。
- physical/write/irreversible 调用的未知结果进入 `Uncertain → Reconcile`，不得自动 replay。
- 不持久化隐藏推理；保存经策略允许的输入引用、决策、调用、Receipt 与 verifier evidence。
- Session 不属于 Agent CardInstance、RuntimeHost 或 EvidenceService；CardInstance 可以作为可替换 worker，Session 必须跨实例/进程重启存在。

不要把整个 Session、Run/Turn/Step 与一次外部 Invocation 压成一个总 FSM。Session 是多个 Run/Turn/Step 的 append-only event stream，只需独立的 single-writer epoch/CAS 和 `Open → Sealed` 生命周期；`Idle/Running/Waiting/Suspended/Closing` 是可重建 projection，不是另一个真相。Run/Turn/Step 拥有认知进度、`WaitingApproval/Resource/Tool/Human/Timer` 原因和 `CancelRequested/Cancelled`。一次有副作用的 `InvocationAttempt` 才使用最小 commit protocol：

```text
InvocationPrepared
  → InvocationIntentCommitted
  → Dispatched
      ├→ ResultCommitted
      └→ Uncertain → Reconciled | Abandoned
```

Cancel intent、delivery、ack 与实际 effect terminal state 分阶段记录；取消不能把已 handoff 的 effect 伪装成未发生。只有 `InvocationIntentCommitted` 后才能产生外部调用；`Dispatched` 只表示 client journal 已记录 exact envelope 并交给 transport/send boundary，不等于 Provider durable acceptance 或 effect handoff。concrete Provider 独占 acceptance/dedup record 并在 effect 前按 InvocationId/AttemptId/request digest 持久准入；client 只记录 acceptance ref/ack，effect owner 再单独记录 handoff。恢复时先读取这些记录与已提交结果/Receipt，再决定 reconcile。VerificationAttempt 是对 OutcomeClaim 的独立协议，不是 InvocationAttempt 的终态。各领域自己定义 `AgentRunId`、`DeckRunId`、`EvalTrialId`，Kernel 不提供会混淆 owner 的泛 `RunId`。

每个 AgentRun 在开始时还固定 immutable `RunExecutionSnapshot`：DeploymentRevision、Harness、Model、ToolCatalogSnapshot、ContextPolicy、SandboxPolicy 与相关 Artifact digest。相同 Run 和在途 Invocation 不静默换 revision；旧 snapshot 仍可用且未撤销时继续，不兼容升级必须新建 Run/branch。Artifact/Grant/policy 被撤销或 snapshot 不可解析时，未来及尚未 dispatch 的 Attempt fail-closed；已 dispatch Attempt 不得被伪装为“未发生”，只能依据 acceptance/effect handoff 证据 cancel、query/reconcile 或进入 `Uncertain`。恢复不能用“最新配置”猜测迁移或 Provider 切换。

### 9.2 AgentHarness，而不是第二个重型 Runtime

平台或 tenant 共享的 AgentSession 机制，在出现跨独立 Product/Installation 的真实消费者后，可以候选为 Agent CoreService，并拥有 AgentHarness、LoopPolicy 与 Planner/critic coordination；RuntimeHost 仍拥有执行域、资源 enforcement、取消投递/ack/escalation 和 sandbox instance lifecycle。单一 installation 私有、跨 DeckRun/升级的 conversation history 不能无条件交给平台 Agent CoreService，它仍是 Application Admission Gate 的未建模能力。`LoopSpec` 可声明 trigger/cadence、input view、model profile、step/tool/token/cost/time/effect budget、stop/preemption/failure policy，但 S0/S1/S2 等固定层级不进入 Kernel。

预算与取消不能只写一个 owner：Agent CoreService 拥有 step/model/tool/token/cost 等 semantic budget、逻辑停止原因和 cancellation intent；RuntimeHost 强制 CPU/RSS/wall-time/process/task/handle 限额并执行取消升级；Security/Sandbox owner 决定 sandbox profile、egress 与 Secret proxy policy，RuntimeHost 只实例化；Authority/Resource/Safety 决定物理 effect 的权限、控制权和允许状态。

术语必须分开：`LoopDomain` 是 Runtime 执行域，`AgentLoop` 是认知循环，`AgentHarness` 编排 AgentLoop 与 Tool，`FaultHarness/TestHarness` 是测试设施。

`Intent`、Plan 与 OutcomeClaim 是 Agent/Grounding 层的版本化领域值，可以通过 Port/Operation 传播并进入 Evidence，但不建立 Kernel `IntentBus`，也不让一条自然语言 Intent 直接成为 Driver Command。

### 9.3 Tool 与 Operation

Tool 层需要区分：

- `ToolDefinition`：稳定 ID/version、input/output/progress/terminal schema digest 与作者的 semantic effect/idempotency claim；不拥有 policy-specific effective risk、Card、Service、Driver、endpoint 或 live state。
- `ToolProviderDeclaration`：某个 Artifact/export 能实现哪些 ToolDefinition，以及其 Service/Feature/Resource/egress、minimum isolation、state/stream/reconcile 等 provider-specific 要求。
- Deployment `ToolBinding`：把逻辑定义绑定到 planned Card/已准入 CoreService/Gateway/composite Provider target，并固定 Artifact 与 desired constraints；它不吸收 Runtime observed facts。ToolCatalogSnapshot 再以 authenticated readiness facts 固定具体 ProviderInstanceRef/generation；若后续确需 ToolBindingEpoch，它不复用 PortBinding BindingEpoch。
- `ToolAdmissionDecision`：Trust/Policy 针对 definition/provider/Artifact/policy revision 产生的不可变有效 effect/permission/retry 上界，不修改 ToolDefinition digest。
- `ToolCatalogSnapshot`：Agent execution assembly 从 committed DeploymentRevision/ToolBinding、ToolAdmissionDecision 与指定 authenticated provider fact snapshot 纯投影的本次 Run 值；projection 无权另选 Provider 或改写 source facts。
- `ToolSet`：DeckSpec 中特定 Agent Card 使用的逻辑 Tool 选择；Session/Run 只能向 `ToolView` 收窄，不能增加 Tool 或选择 Provider。
- `ToolInvocation` 与 `InvocationAttempt`：前者记录逻辑意图，后者固定参数 digest、deadline、correlation 与恰好一个 concrete Provider。
- `EffectClass`：pure/read/write/physical/irreversible。
- semantic Permission/effect/idempotency 位于 ToolDefinition 的待准入合同；Provider-specific Service/Feature/Resource/egress/minimum-isolation 要求位于 Provider 声明，不能写回逻辑定义。
- progress、cancel、single terminal Receipt 与 `Uncertain` reconciliation。

Provider 自报的 `read-only`、EffectClass、idempotency、health 或 PermissionRequirement 都不是可信事实。首个 Tool 就需要不可变 `ToolAdmissionDecision` 与 `ToolCatalogSnapshot`：Trust/Policy admission decision owner 消费 Artifact signature/provenance/test evidence，验证 Schema、provider binding 和 effect/permission 上界；DeploymentController 把 definition digest、admission ref 与 desired binding 编译到当前 DeploymentRevision，Runtime 只报告 observed provider facts。未知或无法证明的 effect class 按更危险等级处理。动态 Registry 服务可以后置，静态 Snapshot 不能后置。一个 Provider 可以导出多个 Tool，一个 Tool 可以有多个兼容 Provider，但首个切片在每个 binding scope 内必须显式选择唯一 desired target，Catalog 只能解析出唯一 Ready Provider；缺失、重复或不兼容均 fail-fast。

有效属性按保守组合产生：effect 取更危险上界，permission/egress/Secret/resource 要求不可削弱地合并，retry/cancel/idempotency/reconcile/state-transfer 只保留各层均可证明的能力。外部 MCP 同名/同 Schema descriptor 默认只形成 endpoint/export-qualified provider candidate，不自动合并成同一 canonical ToolDefinition；合并必须有显式、可审计的 mapping 与语义兼容准入。

Agent owner 再把 Catalog Snapshot、该 Agent Card 的 ToolSet、当前 CapabilityGrant、data-flow/egress policy 与 Session context 求交，只可收窄地物化本次 `ToolView`；ToolView 可见性仍不等于调用授权。“模型看见一个 Tool”不等于“有权调用”，“Tool 返回成功文本”不等于物理效果成功。每个 Attempt 固定 Catalog digest、ToolBindingId、ProviderInstanceRef/generation 与 Artifact；若后续确需 ToolBindingEpoch，也只在所属 binding 内比较。write/physical/irreversible 在 dispatch 后未知时先 reconcile，不透明切换 Provider 或 replay。MCP 是 Gateway/SDK，不是内部 definition/provider identity、授权、任务或 durability 的权威。

CardDefinition 不拥有 ToolDefinition。Artifact 可以把 CardDefinition 与 ToolProviderDeclaration 作为 sibling exports，CardInstance Ready 后成为 concrete Provider；已准入 CoreService、Gateway-backed adapter 或单一 composite invocation owner 也可以成为 Provider。client Invocation/Attempt journal 与 provider acceptance/dedup/subcall journal 分属调用方和 concrete Provider。多个组件若没有一个 owner 能拥有 provider-side 子调用 correlation、cancel/reconcile 与 Tool 级终态，就应表达为 Workflow/多个 Operation，不能把 Schema 片段拼成一个 Tool；即使有 composite owner，没有可证明的 prepare/commit/barrier 也只能暴露 partial/`Uncertain` 与 compensation/reconcile，不能宣称跨故障域原子。完整证据、方案比较与切片见 [Tool 定义、Provider 绑定与调用边界研究](tool-definition-provider-binding-and-invocation.md)。

### 9.4 Context、Memory 与 World

- `ContextManifest` 记录选入来源、digest/revision、选择原因、transform/compaction revision、data label、token estimate 与 model/tool catalog revision。
- Memory 是跨 Session 的 Observation/Episode/Entity/SpatialRelation/SkillExperience/SemanticFact；每条记录需要 provenance、confidence、validity、correction/tombstone、访问控制与 consolidation lineage。
- World 是当前可操作状态及其 uncertainty/owner；Memory 可以陈旧、矛盾和被纠正，不能直接授予物理权限。

ContextMaterializer 属于 Agent execution 边界，拥有某次模型调用的选择、顺序、裁剪、转换和最终 output digest；source owner 仍拥有原始事实，AgentSession 只固定不可变 `ContextManifestRef`，不能把物化副本反写成 source truth。每个 context item 还必须标记 instruction authority、content trust、provenance、freshness、data label 以及 data/instruction boundary；Tool、Sensor、Memory 和外部文档输出默认是不可信 data，不能仅凭其中一段文本升级为 system/developer/operator instruction。结构化通道与 policy decision 是边界，字符串“清洗”不是充分防护。

Context、Memory、World 使用不同 typed service contract，不通过 VFS 合并；ContextMaterialization 从第一个模型调用就需要，Memory 和完整 World Service 则按真实跨 Session 纠错与 state-estimation 消费者逐步引入。

### 9.5 独立 Verifier

Task issuer/delegator 拥有 `OutcomeRequirement`/acceptance criteria，Agent 只能据此提出 `OutcomeClaim`，不能降低验收标准。独立 Verifier 对 Requirement 做准入和可验证性解析，产生 immutable `VerificationSpec`；Spec 只能保持或加强 Requirement，无法验证或存在冲突时必须在执行前拒绝/请求澄清。OutcomeRequirement digest 与 VerificationSpec digest 在执行前一起绑定 AgentRun/Operation。

数字任务由测试、服务状态或外部系统验证；物理任务由 Driver、World、传感器或资源 owner 的 Evidence 验证。Verifier 必须由独立 CardDefinition 定义并作为独立 CardInstance 运行，或由独立 CoreService/不同 trust boundary 承担；实际 Verifier owner 必须拥有独立 principal、revision 与 lifecycle，不能只是同一 Harness 调另一个模型自评。VerificationSpec 固定证据源、阈值、时间窗、inconclusive/failure policy 与 verifier identity/revision；执行后不能为了让结果通过而改成功标准。Verifier 据此产生 `VerificationAttempt/VerificationResult`。

四类结果不能合并：`EffectReceipt` 由该 effect protocol 指定的 Receipt owner 组装并引用 Authority、Lease、Safety、Driver submission 与下游 applied/completion proof；物理首切片可以由 Driver EnforcementPoint 承担 assembler，但它不取得其他 source truth 的所有权。`OutcomeClaim` 由 Agent 拥有，VerificationSpec/Attempt/Result 由在线 Verifier owner 拥有，`EvalTask/Trial/Trajectory/EnvironmentOutcome/GraderResult/Suite` 由离线/CI/仿真 Eval owner 拥有。EvalSuite 必须有 revision，跨多个独立 Trial 记录 environment/grader seed、grader revision/calibration 与统计区间；`pass@k`（至少一次成功）和 `pass^k`（连续全部成功）必须写明定义，不能互换。Eval 证据用于版本比较和发布 gate，不是生产 physical effect 的权威终态。

### 9.6 Multi-agent delegation

正式 delegation 至少包含 parent/child session、goal/success criteria、deadline、token/cost/effect budget、attenuated Grant、context/evidence refs、join/cancel policy、partial failure 与 lineage。supervisor-worker、fan-out/fan-in、auction、blackboard 都是 Agent 层策略，不是 Kernel Graph；此处也正是 `Supervisor` 被保留的语义域，不产生同名 Runtime 类型或进程。

A2A 是 Gateway。由于 A2A 也使用 Agent Card，内部 adapter 应使用 `A2AAgentDescriptor` 等限定名，避免与 ParaEGOX `Card` 混淆。

### 9.7 Model plane 与 Improvement Lab

ModelService 需要 ModelRef/revision、request/config/prompt-template/tool-catalog digest、token/cost/deadline budget、fallback/canary 与 usage Receipt。模型路由是服务策略，不是 Kernel。

生产系统不能让 Agent 自行发布、自行授权或在线修改 Skill。改进必须走离线 dataset/eval → simulation → shadow → canary → signed artifact/revision → bounded rollout，并有独立 reviewer/verifier 与 rollback。

## 10. Security、身份与供应链欠缺

### 10.1 Node/Workload identity

需要 enrollment、node/workload identity、attestation evidence、trust domain、短期凭证、轮换与撤销。进程/容器隔离不足时，给每个 workload 一个字符串 Principal 并不能产生可信边界。SPIFFE 可作为 adapter 候选，不强制成为内部模型。

### 10.2 Secret 与数据流

Secret 不进入 Deck、ConfigSnapshot、Trace 或 prompt；Runtime 注入 session/instance-scoped proxy/handle。授权还需和 data label、sink trust、residency 与 egress policy 分开：允许调用一个 Tool，不表示允许把摄像头、语音、位置或 PII 发送给它。

### 10.3 Sandbox

不可信代码/模型工具至少隔离 filesystem、network/egress、process/IPC、device namespace、CPU/memory/time/handles，以及 raw Zenoh/ROS/Driver access。早期优先使用 ProcessDomain + OS user/namespace/cgroup/seccomp 等组合并明确其局限；WASI/component sandbox 只在真实 plugin 需求与 ABI prototype 成立后引入。

### 10.4 Artifact trust 与升级兼容

远端或第三方 Artifact 进入生产前需要 digest、platform/runtime ABI、service/message schema compatibility、signature、provenance、SBOM、publisher trust、revocation 与 admission Receipt。混合版本部署还要分别验证 runtime protocol、ServiceContract、Message Schema、Artifact ABI 与 state schema，不能用一个 `version` 概括。

首版继续 stop-and-replace；rolling upgrade/state migration framework 等首个有持久状态且不能停机的服务出现后再研究。

## 11. Evidence、OPS 与可恢复运维欠缺

### 11.1 Evidence commit protocol

“durable handoff”还需定义 append idempotency、local commit ack、store epoch、owner-local sequence/causality、integrity digest、retention、storage-full/backpressure、replication lag 与 redaction。不要承诺全局总序、区块链或 universal event sourcing。

### 11.2 OpsService control protocol

OpsService 接受的写操作需要 `ControlRequestId`、target、expected revision/epoch、request digest、dry-run、Authority/approval、progress、cancel、compensation/reconcile、single terminal OpsReceipt 与 break-glass 审计。OpsService 只拥有 operation record，并经 typed client 调用真实 owner；TUI 只做 OpsClient/InspectionClient，不能直接 kill PID、改数据库或绕过 DeploymentController。

### 11.3 Incident evidence

需要 revision-tagged snapshot、crash dump/minidump、thread/task/process inventory、Mailbox/ingress/buffer budget、recent Receipt refs、clock quality 与 dependency state。原始 prompt、音视频和 Secret 默认不进入 incident bundle；导出前按数据分类策略脱敏。

## 12. 开发者平台与兼容性欠缺

Agent OS 如果只有内部架构、没有稳定 authoring 与 conformance 工具，复杂度最终会泄漏给每个 CardDefinition/Driver 作者。Foundation 后需要逐步形成：

- CardDefinition、CoreService、Driver、Gateway 的窄 SDK 与模板；其中 CardDefinition API 只表达不可变声明，CardInstance 私有实现 SDK 只暴露 typed CardInstance-scoped handles 和由 RuntimeHost 驱动的 lifecycle callbacks，不暴露 RuntimeHost internals。
- 从 Schema/ServiceContract 生成类型、validator 与 compatibility test 的 codegen；生成代码不是协议真相，digest/Schema 才是。
- Deck validate/resolve/compile/dry-run、plan diff、permission/feature explanation 与本地 simulation profile。
- PortBinding、Driver、Tool、Evidence sink、Gateway 的共享 conformance suite。
- Rust toolchain/target、Python、OS/architecture、Zenoh、ROS2、GPU/device 的 reference profile 与支持矩阵；`unknown` 不冒充 supported。
- Cargo 是 Rust crate、feature、build 与 lockfile 的唯一工程入口，`uv` 是 Python SDK、worker、Agent/模型与治理工具的唯一工程入口；CI 分别验证两条依赖图，再用跨语言 golden/conformance suite 验证 canonical Schema、IPC、错误和终态语义。
- ServiceContract、Message Schema、Artifact ABI、state schema 的版本/弃用窗口和 upgrade guide。
- ADR、Reference、Testing、Runbook 从实现证据自动交叉检查；文档状态不能冒充 compatibility guarantee。

首版不建设 Marketplace。先让第三方作者在不读取 Runtime 私有实现的情况下，完成一个 CardDefinition 与其 CardInstance 私有实现或一个 Driver 的 authoring、validation、simulation、packaging、admission 与诊断，再判断公共生态 API 是否足够稳定。

## 13. “Graph”不能再作为一个泛概念

ParaEGOX 至少会出现下列不同的图或图状关系：

| 图/关系 | owner | 结构与执行裁决 |
| --- | --- | --- |
| `DeckTopology` | DeckCompiler 产生；DeckLock 持有 | canonical directed multigraph dataflow declaration；不是 live execution graph，DataLink 不等于启动依赖 |
| `ServiceDependencyGraph` | ServiceSpec 声明；DeploymentPlanner 编译 | lifecycle/readiness DAG；任何环在副作用前 fail-fast |
| Deployment rollout relation | DeploymentController | revision prepare/activate/drain/rollback 与 reconcile loop；不是 workflow graph executor |
| Runtime assembly relation | RuntimePlanSlice + RuntimeHost apply journal | RuntimeAssemblyEngine 的一次本地 apply 派生关系；不形成第二 desired graph |
| Agent workflow/plan graph | Agent Harness/策略 | 条件触发；首版显式状态机，满足 durable workflow 准入门后另做 Engine ADR |
| OPS operation flow | OpsService + actual owner | ControlRequest/Receipt 状态机；不内置任意 DAG/saga engine |
| Evidence causal graph | Evidence projection | causality refs、redaction 与时间窗投影；不执行 |
| World/scene/spatial graph | World/Spatial services | 自己的时空、frame、epoch 与 uncertainty 语义 |

不建立 `graph/` 万能包、Graph Store/Service/Query Router 或 `GraphKind + metadata` 来承载这些关系。允许复用的上限是纯 immutable multigraph view、SCC/cycle witness、reachability 和对已验证 DAG 的 stable topological batches；也只有 DeckCompiler 与 DeploymentPlanner 等至少两个独立生产消费者证明真实算法交集后，才经 ADR 抽取内部 Graph Foundation，当前不创建包或占位 API。

只有 Agent workflow 至少出现两个独立生产者，并在持久暂停/恢复、effect fencing、补偿、并行 join、approval、Receipt 与可视化编辑上语义一致时，才研究专用 WorkflowEngine。完整证据、RuntimeAssemblyEngine 边界与验证矩阵见 [专项研究](graph-foundation-and-domain-execution-boundaries.md)。

## 14. 方案比较

| 方案 | 优点 | 主要失败 | 裁决 |
| --- | --- | --- | --- |
| A. 扩大 Kernel，纳入 VFS、Graph、Agent、World、Deck | 表面统一、早期 demo 快 | 认知与产品语义反向锁死底层；故障域和权限再次混合 | 拒绝 |
| B. 维持小 Kernel，但直接增加大量 CoreService | 目录边界清楚 | 没有共同 contract spine；服务名很多但失败语义仍断裂 | 不足 |
| C. 小 Kernel + 薄跨层 contracts + 独立 owner + 纵向切片 | 可验证、可替换、允许单机到分布式演进 | 需要严格 Schema/owner 纪律，早期不能只追 UI demo | 推荐 |
| D. 直接采用外部 Agent runtime/workflow/robot OS | 复用成熟能力 | 授权、物理 safety、Zenoh data plane 与 Card/Deck 语义不匹配 | 仅作为 adapter/参考 |

## 15. 依赖有序的演进计划

下面是对现有 P0–P9 的修订输入，不新建一套并行阶段编号。

### 15.1 P0 前必须冻结

1. Capability/Service/Feature 三义与 `requires.services / permissions / features`。
2. Card/Deck/Deployment/Runtime import boundary 与 `RuntimeApplyRequest + RuntimePlanSlice`。
3. 明确拒绝 Kernel VFS、universal URI resolver、万能 ObjectRef 和通用 StateStore。
4. ConfigSnapshot/SecretRef/observed digest 的单一权威链；首版配置变更走新 revision。
5. 包依赖检查和术语 lint，确保 Runtime 不 import Deck/Deployment 上层模型。
6. Agent protocol seam contract probe：ToolCall → ServiceCall/Query/OperationSpec → PhysicalCommand → EffectReceipt → OutcomeClaim → VerificationResult；每层失败、取消、progress、Uncertain 和 owner 分开，不把 Agent 领域类型塞进 Kernel。
7. Command interaction 的最小 `OperationClient/CommandEndpoint` 语义；禁止 Controller-role Card 直接持有 Driver/Actuator 对象。

P0 只冻结跨层 seam 与包边界，不要求完整 Agent durability、Context、Tool Catalog 或 Verifier ADR；这些在分支 A 开始前冻结。

### 15.2 P1 Foundation amendments

1. 最小 Failure/Problem 与 redaction。
2. Liveness/Health/Readiness/Feature 状态边界。
3. CancellationScope、Timer missed-tick/overrun 与 late-result fencing。
4. `BlobRef/BufferRef` ownership/lifetime 与联合 retained-byte accounting。
5. CapabilityGrant 值/纯验证输入；签发仍在 AuthorityService。
6. ConfigSnapshot digest 与 owner-specific refs；ArtifactRef/SecretRef 分别位于 artifact/secret owner 的 contract 包，Kernel 只提供通用 ID/digest，不引入统一 Resolver。
7. Kernel time 把 Monotonic/Wall/Sim Instant 分型，并定义 ClockDomain/ClockQuality/ClockMapping/ClockMappingRevision；`physical/contracts` 冻结可信 ObservationHeader、Frame/unit/uncertainty/Calibration/Device ABI 并引用前者。
8. 冻结静态 1:1、bounded outstanding、无透明 retry 的 Command interaction 与 `OperationClient/CommandEndpoint`；其他 Operation routing 继续 unsupported。
9. 在 agent/contracts/tool contracts 中做最小 seam probe：ContextManifest、ToolCall、OperationSpec、OutcomeClaim、VerificationResult 只验证与 Kernel Invocation/Receipt/Deadline 的组合，不提前实现完整 Agent Service。

### 15.3 P2a–P2d Runtime amendments

1. RuntimeApplyRequest 的 revision/digest/CAS、prepare/activate/rollback。
2. Service dependency 在 Ready 后丢失的 degrade/stop/rebind/restart 语义。
3. service client、port binding、permission-bound access handle 分离的最小 Context。
4. sandbox profile、OS service-manager signals、watchdog 与 exit reason。
5. fake clock/fabric/driver/model/tool 与 fault injector 共享 deterministic Harness。

### 15.3.1 P2e Deployment control-plane amendments

1. 接入 pure DeploymentPlanner、single-writer DeploymentController、OS-service-managed DeploymentTenureAuthority 与 crash-consistent DeploymentController/RuntimeHost journals。
2. candidate → atomic commit → target Slice → authenticated CAS apply → observe → reconcile-once 成为进入 P3 的硬门槛。
3. P2a–P2d fixture 只来自 production projector/builder，不允许 CLI、手写 Slice 或 RuntimeHost 成为第二 desired-state owner。
4. DeploymentController restart 获取更高新 tenure，writer_fence/prepared/active 分离；partial apply、timeout、重复 operation 与 takeover 不得产生 revision 回退或双 active route。

### 15.4 P3 Physical amendments

1. 模拟设备完整 lifecycle、分 facet owner/epoch、control mode、ownership transfer 与 command arbitration。
2. OperationClient/CommandEndpoint → CapabilityGrant → approval → Lease/Fencing → SafetyIslandAdapter → Enforcement → Receipt。
3. 冻结的 ObservationHeader 把 time/frame/transform/unit/calibration/uncertainty/origin/provenance 送入 Sensor→Controller-role Card 数据链。
4. Device/Driver/assembly/calibration/mode/safety 的独立 epoch/revision，以及 crash/reset/poisoned/completion 与 Uncertain reconciliation。
5. ContinuityController 执行固定全部 revision/digest 的 ContinuityProfile；主机重启失去 monotonic baseline 后默认不恢复自治。
6. ScenarioRunner/ScenarioManifest 分离 truth 与 observation，P3 让 fake/sim Driver 执行可复用 conformance；real Driver 只在 H1 执行同一 suite + HIL 扩展。
7. Driver/adapter raw power/thermal → DeviceService PowerState/ThermalState → product OperatingEnvelopeEvaluator → safety island inhibit/Decision 的 owner 链；所需 fact refs/freshness 进入每次 EnforcementContext，Continuity/Admission 只消费。
8. P3–P5 保持 simulation-only；P6b 后通过 H1 Hardware Enablement 才允许具体 real actuator，gate 覆盖 commissioning/readiness、local durable Evidence、real Driver conformance、HIL/timing、独立 E-Stop/物理隔离、产品 hazard/ODD、rollback 与人工接管。local HardwareActivationGate 把 Receipt/RuntimePlanSlice/Readiness 安装成 activation ref/epoch，并在分区中本地失效/disarm。

### 15.5 P4–P6 Distributed/Operations amendments

- P4：Blob/Buffer/SHM lifetime、Schema/codec negotiation/cache、FabricFeatureReport 与 locality/bandwidth adaptation。
- P5：Node/Workload identity、enrollment、attestation adapter、artifact/runtime compatibility、mixed-version refusal；HA/consensus 后置。
- P6a：local Evidence commit、local Inspection/incident、retention/storage-full；可与 P4/P5/Agent slice 并行。
- P6b：P5+P6a 后的 Evidence replication/lag、federated Inspection、OpsService ControlRequest journal、artifact Release/audit、跨 Node incident correlation；汇合测试覆盖 Session takeover 旧 writer fencing、EvidenceRef 与 effect 不重复。

### 15.6 P3 后分叉的三个正式 Program 分支

```text
P0/P1 contracts → P2a–P2d Runtime → P2e minimal Deployment control plane → P3 simulated physical spine
                                      ├── A. local Agent vertical slice
                                      ├── B. P4/P5 distributed Fabric
                                      └── C. P6a local Evidence/Inspection
                          P5 + P6a → P6b federated Inspection + OpsService
                                                   │ converge A + P5 + P6b
                                                   ▼
                                      distributed Agent Execution Plane
                                                   ├── Grounding & World
                                                   ├── ROS2 / MCP / A2A Gateways
                                                   └── H1 conditional hardware gate
```

首个 Agent 垂直切片不等待双主机或完整 OpsService：local durable AgentSession/WAL → ContextMaterializer/Manifest → immutable ToolCatalogSnapshot/ToolView → 一个经 admission 的 read-only Tool → 一个模拟物理 Operation → Authority/approval/lease/safety → EffectReceipt（durable effect 等待 P6a）→ independent Verifier → 最小 Eval Task/Trial/EnvironmentOutcome。它同时验证 Agent、工具与物理脊柱，不先建设通用多 Agent 平台；分布式和生产化能力随后与 P4–P6b 汇合。

分支 A 的进入 gate 才冻结：Session append-only/single-writer 与 Run/Turn/Step/InvocationAttempt 分层；每个 Run 的 immutable RunExecutionSnapshot 与不静默换 revision 规则；semantic vs Runtime enforcement budget；ContextMaterializer 的 instruction/data trust；ToolCatalogSnapshot admission/ToolView；task issuer 的 OutcomeRequirement → pre-committed VerificationSpec 单向加强规则与独立 Verifier/Eval owner。动态 Tool Registry、MemoryService 和多 Agent delegation 不是首切片前置。

## 16. Brainstorm 候选池与路由

### 16.1 Concern map，不是固定数量的 Plane 或 Service

下面只是帮助检查遗漏的 concern map，不是部署拓扑、package 清单或 owner map，也不追求固定数量。一个 concern 可以包含多个必须独立的 owner；早期共进程只减少部署成本，不合并 desired state、持久状态、principal、epoch 或 Receipt。

| Concern group | 必须保持独立的 owner | 不可跨越的边界 |
| --- | --- | --- |
| Kernel contracts | ID/time/message/receipt/epoch 的 Schema 与纯规则 | 不拥有 Card/Deck、Agent、World、I/O 或持久业务状态 |
| Runtime execution | Domain、Mailbox、dispatch、budget、liveness/failure-containment/recovery、sandbox instance | 不拥有 Deployment desired state、AgentSession 或设备真相 |
| Fabric data plane | Zenoh session/route/binding/ingress owner | 不拥有 placement、应用语义或 physical effect |
| Deployment control plane | DeckCompiler/DeploymentPlanner/DeploymentController、DeploymentPlan、RuntimePlanSlice value、reconciliation | 不拥有 live route、Runtime instance 或设备 observed state |
| Physical control | Device/assembly/calibration、mode、Resource、safety island/Adapter、Enforcement 各自独立 | Planner Intent/Memory 不能成为 permission、completion 或 safe state |
| Agent execution | Session、Harness/loop、ToolView、semantic budget | Runtime enforcement、Tool trust admission、Driver 和硬安全另有 owner |
| Context & Memory | ContextMaterializer 与 MemoryService 分开；source owner 保留原始事实 | Context projection 不反写 source truth，Memory 不冒充 World current state |
| Grounding & World | state estimation、Frame/Spatial/World owner | 不授予权限，不伪造 device completion/Evidence |
| Online verification | VerificationSpec/Attempt/Result 的独立 principal/lifecycle | 不属于 Agent self-review、ScenarioRunner 或离线 Eval |
| Trust & Release | identity、Secret/data-flow、Artifact/signature/SBOM、admission、rollout owner | Agent/CardInstance 私有实现不能自签名、自授权、自发布 |
| Evidence | durable Receipt/ref、retention、replication | 不拥有运行对象，也不等同于 Trace/Log/Metric |
| Inspection / OPS / Telemetry | 事实 producer、InspectionService projection、OpsService ControlRequest、Telemetry exporter 各自独立 | 观测或 UI 不能改写 owner；OpsService 不执行领域副作用；Telemetry 不能反推权威成功 |
| Scenario & offline Eval | ScenarioRunner/SIL/HIL 与 EvalSuite/Trial/Grader 各自版本化 | 不拥有生产 effect 或在线 VerificationResult |
| Ecosystem gateways | ROS2、MCP、A2A 等 protocol-specific adapter | 外部类型不成为内部权威，不引入对等多 Fabric |

### 16.2 候选路由表

| 候选 | 路由 | 进入条件 |
| --- | --- | --- |
| Capability 三义、RuntimePlanSlice、ConfigSnapshot | 合并进现有 Foundation | P0 前必需 |
| Physical contracts | 合并进 P1/P3 | 首个 Sensor→Controller-role Card 纵向切片 |
| Durable AgentSession/Harness/Tool/Verifier | 新 Program 候选 | P3 本地 authority/effect spine 可复用后启动 |
| ContextMaterializer/ContextManifest | 首个 Agent slice 必需 | 第一次模型调用即产生；固定 instruction authority、content trust、provenance 与 output digest |
| MemoryService | research-first | 出现跨 Session 纠错、consolidation、tombstone 和访问控制需求 |
| Scenario Lab / SIL / HIL / Eval | 分阶段共同工作流 | P3 建 ScenarioRunner/SIL；HIL 只在 P6b 后 H1；Eval 与在线 Verifier 保持独立 owner |
| Human Supervision / Approval / Teleop Handoff | 合并 P3 与 Agent Program | 第一个 physical/write Tool；明确 approval expiry、operator identity、mode/lease transfer 与 no-response policy |
| Device Commissioning / Calibration | H1 最小 Receipt + 后续 workflow | 第一台真实设备先有 identity/firmware/ABI/assembly/calibration/safety binding Receipt；第二类设备再抽取完整审批/回退/维修 workflow |
| State Estimation / World Model | Grounding Program | 第一个 consumer 需要当前可操作 state；两个传感源融合前冻结 uncertainty、frame/transform revision 与 truth owner |
| Data Governance / Consent / Residency | 合并 Security 与 Deployment | 摄像头、语音、位置或 PII 首次流向远端 Tool/Model 前 |
| Model Serving / Router / Cache | Agent Program 内的 ModelService | 第二个模型后端、fallback/canary 或资源竞争出现；不进 Runtime/Kernel |
| Immutable ToolCatalogSnapshot | 首个 Agent slice 必需 | 第一个 Tool 即经 Trust/Policy admission 并编译到 DeploymentRevision |
| Dynamic Tool/Skill Registry | Trust/Release 后置能力 | 两个独立发布方和稳定 ToolDefinition/Artifact admission；不允许 Agent 自发布 |
| Artifact trust/OTA | 最小 admission 合并 P5，Release/audit 合并 P6b；后续独立 Program | 使用远端/第三方 artifact 前 |
| SDK/codegen/conformance/reference profiles | 合并各纵向切片 | 第二个独立 CardDefinition/Driver 作者出现前 |
| Recorder/Replay | research-first | simulation/shadow 需要复现跨边界输入时；不 replay effect |
| Adaptive QoS / bandwidth-aware placement | P4/P5 research-first | 三类 locality 基准证明固定 profile 无法满足 Observation/Command SLO |
| Formal policy/model checking | 条件触发 | Safety/Authority/continuity 状态机已稳定且事故代价支持投入；不替代 HIL |
| Native Realtime Domain | 条件触发 | 测得 worst-case deadline 无法由 Rust RuntimeHost、Python/C++ worker 或设备 controller 满足；不是“代码是 Rust”或“延迟看起来高” |
| WorkspaceService | park/research-first | 至少三个 backend 共用且 owner 不被吞并 |
| Workflow/Graph Service | park/research-first | 两个生产者需要 durable pause/join/compensation |
| WASI plugin runtime/Marketplace | park | trust、ABI、artifact、sandbox、rollback 全部成熟后 |
| Internal multi-agent delegation | Agent Program 条件分支 | 单 AgentSession、attenuation、join/cancel、partial failure 与 lineage 已稳定 |
| A2A Gateway | gateway-later | 内部 DelegationSpec/Task/Artifact 映射稳定后；外部 Agent Card 不成为内部 Card/Session 权威 |
| Fleet/Site/Cluster | park | 出现不可由 Node/Deployment/标签查询表达的真实 owner |
| 多 DeploymentController 共识/HA | park | single-writer DeploymentController 成为经测量不可接受的可用性风险 |
| EnergyService | park | placement/scheduler 有真实消费者 |
| Federated/Privacy-preserving Learning | park | 数据不可集中且离线 Improvement Lab、consent、artifact lineage 已成熟 |
| Autonomous self-improvement | reject as production authority | Agent 只能提出候选；训练、评测、签名、授权和 rollout 必须由独立 pipeline/owner 完成 |

## 17. 验证设计

### 17.1 Claim-to-evidence matrix

| 声明 | 最小证据 |
| --- | --- |
| 系统有界 | ingress、Mailbox、inflight、executor/IPC、child work、Blob retain 联合守恒与洪峰测试 |
| 分区下安全 | Fabric partition、Authority unavailable、旧 Grant/Lease/epoch、local safety stop 故障注入 |
| Runtime 不反向依赖 | import/layer check；Runtime 使用序列化 RuntimePlanSlice fixture，不安装 deployment 包也可测试 |
| 多语言不产生双 Runtime | Rust RuntimeHost 与 Python/C++ worker 使用同一版本化 ProcessDomain 合同；worker 不能自建第二 RuntimeHost、Mailbox、restart/readiness owner 或 raw Zenoh route |
| 跨语言合同一致 | Rust encode/Python decode、Python encode/Rust decode 的 golden vectors、unknown-field/version negative tests，以及 dispatch 后 `Uncertain → reconcile` 终态矩阵 |
| 物理数据可 grounding | frame/unit/time/calibration/uncertainty 不匹配 fail-fast；stale 数据拒绝 |
| Decision 不能错配 effect | Command A 的 Authority/Safety Decision 用于 B 被拒；clamp 后 requested/applied digest 与 completion evidence 分开 |
| Device Ready 是一致快照 | desired/observed firmware/ABI/session/assembly/calibration 任一换代使 DeviceReadinessSnapshot 失效；commissioning Receipt 不可拼接 |
| Safety 独立于 Runtime | wedge/kill RuntimeHost 后独立 safety process/clock/deadman 仍直接驱动 simulated safe output |
| 断网窗口不可被抖动刷新 | ContinuityEpisodeId/offline_since 跨短暂重连和 ContinuityController restart 保持，只有 reconciliation close 才结束 |
| Tool 不越权 | visibility≠authorization、audience/scope/revision/data-label negative tests |
| Tool/Context 不被自报提升信任 | provider 自报 read-only 被 policy override；Tool/Sensor/Memory 文本不能升级为 instruction authority |
| Agent 可恢复 | Session single-writer/CAS、Run/Step/InvocationAttempt crash matrix、waiting/cancel 恢复、既有结果不重做、Uncertain 不 replay |
| Agent revision 可恢复 | RunExecutionSnapshot 固定 Deployment/Harness/Model/Tool/Context/Sandbox/Artifact revisions；进程重启和新 revision 激活时，同一 Run 不静默换版，撤销后 fail-closed |
| Agent 成功可信 | task-owned OutcomeRequirement 与 VerificationSpec digests 执行前绑定；Agent 降低标准、Verifier 弱化/事后修改标准、Planner claim 与 independent verifier disagreement 均被拒绝或显式失败 |
| 制品可信 | digest/signature/provenance/SBOM/ABI/schema/revocation admission matrix |
| OPS 可控 | stale expected revision、dry-run、approval、cancel、partial failure、break-glass Receipt |
| 物理输出单一权威 | Driver normal setpoint 与 safety trip/重启竞态时，下游 gate 保持 safe output；不存在平级 writer 的 last-write-wins |
| 真实硬件受 gate | 无 H1 Receipt/local activation 时 real write 拒绝；Device/assembly/deployment/artifact/readiness/ODD/revision 变化即使 Fabric 分区也推进 local activation epoch 并 disarm |

### 17.2 场景层级

1. 纯 contracts/property tests：ID、revision、grant attenuation、deadline、schema、state machine。
2. deterministic runtime harness：fake clock/fabric/driver/model/tool、bounded queues、crash/wedge/cancel。
3. single-process simulation：无真实 Zenoh/设备，验证语义；Rust 与 Python 各自的纯实现测试分别由 Cargo 与 `uv` 驱动。
4. same-host cross-language multiprocess：Rust RuntimeHost + Python/C++ worker，验证 IPC、credits、deadline/cancel、process tree、crash 与 uncertain reconciliation。
5. same-host multiprocess + Zenoh：以原生 Rust Zenoh API 为 production primary，验证 SHM、session/binding epoch 与 callback handoff；Python binding 保留为 workload/Gateway 兼容性证据。
6. two-host partition lab：断连、重连、clock uncertainty、old owner fencing。
7. SIL/digital twin：可重置环境、场景 seed、golden trajectory 与 fault injection。
8. HIL：真实 Driver/控制器、模拟危险边界、人工 E-Stop。
9. shadow/canary：真实观测但不写，随后限定资源/速度/区域的最小写入。

Agent/具身 Eval 除任务成功率外，还应测 unsafe attempt、policy bypass、stale-state action、duplicate effect、recovery time、terminal Receipt completeness、人工介入、延迟、成本与能耗。

## 18. Rollout、迁移与回滚

- 所有新 Schema 带 version/digest；首版只支持相邻、显式兼容窗口。
- 配置、binding、execution、Grant 与 Artifact 变化绑定 `DeploymentRevision`，通过 prepare/activate/drain/retire/rollback，不原地混配。
- 首版采用 stop-and-replace；state migration 或 rolling upgrade 未经专门 ADR 不进入生产。
- Agent Harness/Model/Tool/Context/Sandbox policy 通过 shadow/canary revision 推进；每个 AgentRun 用 immutable RunExecutionSnapshot 固定实际 revision/digest，回滚不改写历史，同一 Run 不隐式切到“当前最新”。
- 物理副作用不可随软件回滚而“撤销”；回滚动作是停止新准入、进入安全状态、reconcile 现实、恢复已知安全 revision。
- Artifact/Grant/credential 撤销必须支持快速 fail-closed，同时保留本地最低安全路径。

## 19. 风险与失效条件

| 风险 | 预防/失效条件 |
| --- | --- |
| 文档先设计出过多空服务 | 每个新类型必须有生产者、消费者、failure test；无消费者则 park |
| 小 Kernel 变成 giant contracts dump | 领域 ID/Schema 留在所属 service；Kernel 只保留跨故障域通用值和纯机制 |
| typed refs 演化成 VFS | 禁止通用 open/resolve；每个 ref 只能由单一 owner client 解释 |
| RuntimePlanSlice 变成第二真相 | Schema/apply protocol 归 Runtime，value/projection 归 Deployment；请求携带 source/slice digest、target、revision、exact expected-active target-slice digest 和全 payload 认证，不可独立编辑 |
| CapabilityGrant 被当成服务发现或 feature support | 三类状态独立 reason code、Inspection 与 negative test |
| SafetyIslandAdapter 被误当产品合规 | Adapter 只接入独立安全岛；第一个真实产品必须单独建立 hazard/ODD/safety case 和 HIL evidence |
| durable Session 诱导自动 replay | effect class + Receipt + reconciliation；write/physical/irreversible 默认不 replay |
| 恢复时 Agent 偷换版本 | RunExecutionSnapshot 固定执行依赖；旧 snapshot 不可用或被撤销时 fail-closed 并显式 migration/branch |
| Agent/Verifier 降低任务标准 | issuer-owned OutcomeRequirement 与 VerificationSpec digest 预绑定；Verifier 只能保持或加强，冲突先拒绝 |
| Graph Engine 提前锁死 Agent 模式 | 前两个 workflow 用代码/状态机实现，重复需求成立再抽象 |
| Zenoh-native 变成 transport lock-in 债 | Kernel contracts 保持 transport-neutral；只允许 Gateway，不建设多 Backend 最低公分母 |
| sandbox 名不副实 | threat model + escape/egress/device negative tests；seccomp/WASI 单项不宣称完整隔离 |

如果两个以上 Foundation 纵向切片都必须绕过当前 Kernel 才能表达 deadline、Receipt、Grant、Lease 或 revision，说明 Kernel 过薄，需要 ADR 修订；如果 Kernel 开始知道 Model、Memory、Card、Deck、领域 Graph/Graph Engine 或 URI scheme，则说明边界已经过厚。两个独立消费者后条件抽取的无状态纯 Graph Foundation 不属于这里的领域倒灌。

## 20. 明确拒绝项

- Kernel VFS、scheme registry、万能 ObjectRef 或全局 Service Locator。
- Card、Deck、Agent Loop、IntentBus、通用/领域 Graph Engine、Memory 或 World 进入 Kernel。
- 多生产 Fabric Backend 抽象与默认 LocalBus。
- 三种 Capability 共享基类或用 service discovery 推导授权。
- 一个通用 Graph 表示 Application、Service、Agent、Evidence 与 World。
- 一个通用 StateStore/checkpoint 暗示外部世界可以回滚。
- 自动 replay `Uncertain` 的 Tool 或物理副作用。
- LLM、Rust RuntimeHost、普通 Python worker dispatcher 或远端 Fabric 位于 E-Stop/硬安全闭环。
- MCP/A2A/ROS2 类型成为内部权威模型。
- 无预算、无 CapabilityGrant attenuation、无 join/cancel lineage 的递归 Agent spawn，以及无 owner/CAS 的全局 blackboard。
- 在两个以上真实 durable workflow 出现前自研通用分布式工作流引擎。
- 生产 Agent 自行发布、自行签名、自行授权新的 Skill。
- 在没有产品 hazard evidence 时宣称安全标准合规。

## 21. 开放问题与下一项决策

高置信度、应立即进入 Proposed ADR：

1. Capability/Service/Feature 三义。
2. RuntimeApplyRequest/RuntimePlanSlice 与 package import boundary。
3. VFS/ObjectRef 拒绝和 owner-specific typed refs。
4. ConfigSnapshot/SecretRef/revision activation。

中等置信度、需首个纵向 prototype：

1. `physical/contracts` 的确切包位置、已冻结 ObservationHeader 的字段编码/扩展方式，以及 PhysicalAssembly/Calibration Artifact 的首版最小字段；owner、时间/空间/校准/origin 语义不再开放。
2. Blob/Buffer 在 Rust、Python/C++、Zenoh SHM 与 GPU/device memory 间的 ownership、lifetime 和跨进程引用 API；不得跨边界泄漏语言私有对象或裸指针。
3. Durable AgentSession 的存储与 single-writer 实现。
4. Agent 能力由 CardDefinition 定义并作为 Card 配置，还是由共享 Agent CoreService 提供的首个产品边界。
5. Sandbox 的 production profile 与支持平台。

保持开放：

- 首个真实机器人产品及其 operational envelope。
- 是否需要 Site/Fleet owner、多 controller HA 或专用 workflow engine。
- ROS2/Jetson/Ubuntu、Rust toolchain/target 与 Python 的最终版本矩阵。

## 22. 推荐结果

当前推荐不是马上建设“完整 Agent OS”，而是先按 ADR-0006 用 Rust-first mechanisms 建成一条能证明的仿真物理执行脊柱，并以版本化 ProcessDomain 保留 Python/C++/模型/Agent 的多语言工作负载边界；随后在 P3 后立即用一个极窄 local Agent 垂直切片穿透它。该切片与 P4/P5 分布式化、P6a local Evidence 并行，再经 P6b 汇合：

```text
P0/P1 boundary contracts
→ P2a–P2d deterministic RuntimeHost
→ P2e DeploymentController/tenure/apply/reconcile loop
→ P3 simulated grounded physical operation
├── AgentSession/Harness + ContextManifest + one Tool + independent verifier/eval
├── P4/P5 Zenoh two-process/two-node
└── P6a local Evidence/Inspection
        P5 + P6a → P6b federated Inspection + OpsService
        → distributed Agent convergence → H1/HIL → shadow/canary
```

这条路线保留 ParaEGOX 作为分布式具身 Agent OS 的野心，同时把先进结构放在可验证 owner 之上，而不是提前堆进 Kernel。
