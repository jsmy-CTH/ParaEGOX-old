# CardDefinition、Card 与 Deck

> 状态：Draft
> 日期：2026-07-29
> 范围：ParaEGOX 可复用能力、工作负载组合、依赖锁定、执行约束与运行身份
> 实现状态：尚未实现；本文定义目标语义，不是 API 或格式完成声明
> 架构裁决：[ADR-0001 — DeploymentController、DeploymentPlan 与 Runtime 边界](../adr/ADR-0001-deployment-controller-boundary.md)
> 术语裁决：[ADR-0002 — CardDefinition、Card 与 CardInstance 术语边界](../adr/ADR-0002-card-definition-terminology.md)
> Deck/Application 提案：[ADR-0004 — Deck 工作负载、DeckLock 与 Application 准入边界](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md)，状态为 Proposed
> 语言与运行边界：[ADR-0006 — Rust-first 核心机制与多语言工作负载边界](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md)
> Gateway 边界研究：[Web Console、WebRTC、WebXR 与交互式 Gateway 边界](../research/web-console-webrtc-webxr-gateway-boundaries.md)
> Application 边界研究：[Application、Deck、Card 与 Service 边界](../research/application-deck-card-service-boundaries.md)
> Graph 边界研究：[Graph Foundation、领域图与执行边界](../research/graph-foundation-and-domain-execution-boundaries.md)
> Tool 边界研究：[Tool 定义、Provider 绑定与调用边界](../research/tool-definition-provider-binding-and-invocation.md)
> 独立开发与探测研究：[Card 独立开发、测试 Harness 与运行探测边界](../research/card-independent-development-testing-and-probe-boundaries.md)

## 一句话结论

ParaEGOX 使用不可变 `CardDefinition` 表达可复用、可版本化的能力定义，使用 `Card` 表达它在 Deck 中的一次具名、配置使用，使用 Deck `Link` 引用 Card 的 Port，使用 `CardInstance` 表达 RuntimeHost 托管的运行身份；`Deck` 是可锁定、可部署的声明式可执行工作负载单元，不永久等同于 Product、Release、Installation 或自然语言中的完整“应用”。

## 1. 背景与证据

PhanthyMotus 中 `Card` 是已经存在的 Canvas 编排对象，但不是发布包或独立代码类型。`CanvasLayout` 保存 Cards、数据连接和 executor 连接；每张 Card 保存 `id`、`mcpId`、`toolName`、输入输出 Topic 等状态，连接则通过 Card id 引用它，启动时再把 `card.id` 作为 `instance_id` 交给对应 Tool。ParaEGOX 继承的是“Card 表示能力的一次具名、配置使用，并可被连接图引用”这一心智模型，不继承 Canvas 把 UI 坐标、动态 Topic、连接求解和运行状态混在同一个浏览器真相结构中的做法。

PhanthyMotus 当前代码没有 `Deck` 领域概念；完整组合由 `CanvasLayout` 与 `Project` 启停状态共同表达。`Deck` 是 ParaEGOX 为 Cards、Links 与工作负载需求引入的声明式可执行组合单元，是对 Motus `CanvasLayout + Project` 心智模型的独立演进，而不是从 Motus 原样继承的类型。

PhanthyMotus 当前代码反而真实使用 `Bundle` 作为实现聚合名：`PerceptionBundle` 把多个 Perception Plugin/Tool 聚合在一个 MCP endpoint 后面，部分 Canvas 逻辑还通过 Tool 数量推断 bundle。ParaEGOX 当前不把裸 `Bundle` 建成公共总概念，也不把它作为 Deck 别名或把 `Deck` 解释为 `PerceptionBundle` 的改名；endpoint 聚合、软件交付和应用组合由不同 owner 表达。

phanthymotus-driver 进一步证明这种基数不是 1:1：R1 的一个 `LocoPlugin` 导出 `loco`、`switch_mode` 与 `arm`，一个 `StatePlugin` 导出多个状态/资源 Tool，DeviceBundle 再把全部 Plugin Tool 拉平到一个 MCP endpoint。反方向的“同一逻辑 Tool 由多个可替换 Provider 实现”则没有正式模型，当前内置/外置同类设备只是不同裸 Tool 名称。ParaEGOX 因而保留“一个运行 owner 可提供多个 Tool”的能力，但不能把 Plugin、Card、Driver 或 endpoint 当成 ToolDefinition owner。

受限的 EAGOS 工程经验表明，`Module` 名称曾跨越定义、配置使用与运行职责；`Bundle` 名称则被多个 source、lock、交付/安装、runtime/deployment 协作表面复用，各表面的完成度和真正动作 owner 并不相同。问题不是“一个类实现了一切”，而是同一个词跨层承载了不同身份和生命周期。ParaEGOX 只把这种边界风险转化为中立需求，不复制相关代码、Schema、测试或领域语义。

本文解决六个问题：

1. 如何保留 Motus Card 的配置使用心智模型，同时分开可复用定义、Deck 内声明和运行身份。
2. 如何把 Motus `CanvasLayout + Project` 演进为独立于 UI 的 DeckSpec/DeckTopology。
3. 如何明确 Motus `PerceptionBundle` 只是一种历史实现聚合，并避免重新聚合 EAGOS Bundle 的职责。
4. 如何保留直观的 In/Out，同时把 Port 声明、Link 交付、PortBinding 与 Zenoh route locality 分开。
5. 如何允许 CardInstance 成为 Tool Provider，同时保持 ToolDefinition、Provider 声明、Deployment 绑定、ToolView 与具体调用彼此独立。
6. 如何让 Card 被快速独立开发和隔离验证，同时不引入 `Card.run()`、第二套 Runtime 入口或万能 Probe。

## 2. 非目标

本文不定义：

- 完整 `deck.yaml`、`deck.lock`、CardDefinition 序列化格式或 Port API Schema。
- Marketplace、在线升级、自动回滚、离线归档或签名发布流程。
- Graph、Agent、Model、Memory 或具体 Driver API。
- 正式 Application/Installation Schema、Marketplace 或应用私有持久服务合同。
- 与 EAGOS `Module`/`Bundle` 配置或生命周期的兼容层。
- 由 Canvas 浏览器状态直接驱动生产 Runtime。
- 公共 CardHarness、standalone runner、Probe registry、开发 CLI 或测试 manifest；其准入边界见专项研究。
- 将 Rust、Python、C++ 或某种装载方式定义成 Card/CoreService 身份，或建立公共 Rust `dylib`/trait-object Card ABI。

## 3. 核心关系

```text
CardDefinition
   │ 不可变合同 + ArtifactExportRef / entrypoint
   ├──────────────────────────────> Artifact
   │ 在应用中被配置使用
   ▼
Card
   │ 通过 Port 与 Link 组成应用
   ▼
DeckSpec ──resolve──> DeckLock
                          │ + ServiceSpec / Node facts / policy
                          ▼
                  DeploymentPlanner
                          │ DeploymentPlanCandidate
                          ▼
                 DeploymentController
                 committed DeploymentPlan
                          │ RuntimeApplyRequest {RuntimePlanSlice + writer context}
                          ▼
                     RuntimeHost
                          │ RuntimeAssemblyEngine
                          │ CardInstance + observed facts / Receipt
                          ▼
       DeploymentController reconcile → DeckRun / Inspection projection
```

可以用近似关系帮助理解：

```text
CardDefinition : Card ≈ Type : Configured Use
Card : CardInstance ≈ Desired Declaration : Running Identity
DeckSpec : DeckRun ≈ Desired Workload : Observed Run
```

这些只是心智模型，不表示代码必须使用继承或传统面向对象结构。

## 4. CardDefinition

`CardDefinition` 是不可变、可复用、可版本化的能力定义。它包含语言中立的合同和 Artifact export/entrypoint 引用；实现代码由 Artifact 提供，但不与定义共同组成一个行为对象。它回答：

> 这是什么能力，提供哪些 In/Out，接受什么配置，需要哪些权限和执行条件，以及由哪个 Artifact 提供实现？

CardDefinition 的目标声明可包含：

- 稳定 ID 与版本。
- language-neutral Artifact export/entrypoint 引用；它描述 runtime kind、目标兼容性和实现 export，但不暴露语言私有类型。可信、与 RuntimeHost 同构建同发布的 Rust 实现通过内部 registry/static linkage 接入，Python、C++、未知或不可信 Artifact 默认由版本化 ProcessDomain worker 承载；不建立公共 Rust `dylib`/trait-object ABI。
- `In`/`Out` Port 及其 Schema、方向、interaction 和不可削弱约束。
- 配置 Schema。
- 三类独立需求：`ServiceRequirement`、`PermissionRequirement` 与 `FeatureRequirement`；不把服务发现、授权和平台支持都写成 Capability。
- 实现内在的 `ExecutionRequirements`，包括调用模型、阻塞/native/device 风险、重入性、并发上限、取消能力、单次非抢占 run bound 及其证据来源、minimum isolation。不知道时显式为 `unknown`，不伪造时序保证。
- 兼容平台和 ResourceClaim，而不是具体 lane、线程、PID 或最终运行位置。
- 对代码、模型或原生二进制 Artifact 的内容寻址引用；一个 Artifact 可以导出一个或多个 CardDefinition。

`ServiceRequirement` 回答依赖哪个版本化服务接口，`PermissionRequirement` 回答希望 Authority 授予哪些最小操作，`FeatureRequirement` 回答目标 Node/Fabric/Device 必须实际支持什么。三者分别由 Deployment 与 Authority 求解，不能共享一个 Capability 基类；完整边界见 [Capability、Service Contract 与 Feature Support](capability-service-feature-boundaries.md)。

CardDefinition 不具有独立系统运行身份，也不是业务基类。RuntimeHost 可以依据其 entrypoint 在受信 in-process profile 中创建私有 Rust 实现，或请求 subordinate ProcessDomain worker 创建语言内私有实现；该实现可以持有 ASR 模型句柄、解码状态或领域缓存，并实现 `on_start`、输入回调和 `on_stop` 等可选钩子，但 CardInstance/RuntimeHost 才拥有并推进生命周期、判活、恢复、超时和取消。实现回调参与生命周期不等于实现对象、worker 或 CardDefinition 拥有系统生命周期，ProcessDomain worker 也不是第二 RuntimeHost。

CardDefinition 也不是默认的代码包装层。无状态 helper 仍是普通 library/function，跨 Deck 共享且长期持有权威状态的能力是 CoreService，硬件或外部协议边界是 Driver/Gateway；一个算法内部步骤如果不需要独立复用、配置、隔离或观测，不应仅为了 Canvas 颗粒度被强行定义为 CardDefinition。

普通作者不需要额外理解 `Handler` 或 `Factory` 领域对象。输入处理可以是语言 SDK 映射到合同的方法，具体构造只属于 Artifact/Runtime adapter 或语言内 workload unit profile，不形成公共领域对象。下面只是候选 Python workload SDK 的作者体验示意，不是规范合同、Runtime 装载方式或 Python-first 声明：

```python
class ASR:
    audio = In[AudioFrame]()
    transcript = Out[Transcript]()

    async def on_audio(self, frame: AudioFrame) -> None:
        result = await self.model.transcribe(frame)
        await self.transcript.send(result)
```

上例只表达一种 Python 语言内实现类和 In/Out 作者体验，不表示 `ASR` 继承 CardDefinition、定义 Card 身份、承诺嵌入 CPython，也不是已冻结 API。Rust 或其他语言 SDK 必须生成/消费相同的语言中立合同。CardDefinition 中的 Port 声明必须不可变；运行时只有 CardInstance-scoped 的 Card 实现上下文能取得对应窄端口句柄、Clock、Cancellation、Receipt/Inspection sink、typed service client、permission-bound access handle 和最小配置。它不能取得完整 Runtime、raw Fabric、原生 Zenoh Session、token/Secret 或新的 Service Locator/VFS。共享模型、设备和 native singleton 必须通过 CoreService、ResourceClaim 或其他显式 owner 建模，不能藏在类变量或全局 registry 中。

首个 reference profile 为每个 CardInstance 提供新 generation 的隔离 ephemeral workspace，并在实例终止后回收；不注入 host persistent path、原始数据库凭证或任意 network/egress。持久存储和外部访问只能来自 CardDefinition 已声明、Deployment 已解析的 typed service/access handle。显式请求非支持 state lifetime 或非平台 provider ownership 时，编译分别返回稳定 reason code `UnsupportedStateLifetime` 或 `UnsupportedProviderOwnership`。未声明的原始文件、数据库和网络访问需要由 ProcessDomain 的受限 sandbox profile 拒绝并记录；Loop/ThreadDomain 只允许受信内置实现，不能宣称是对恶意代码的安全边界。

## 5. Card

`Card` 是某个 CardDefinition 在一个 Deck 中的一次命名和配置使用。它回答：

> 这份 Deck 以哪个稳定 key、什么 role/profile 和配置使用该能力？

Card 只声明“这一次怎么使用”；CardDefinition 声明能力和 Port，DeckSpec 的 Link 声明“谁与谁相连”。因此 Card 不复制 PortSpec，也不把连接列表藏进自身。

一个 CardDefinition 可以在同一个 Deck 中产生多张 Card。例如，同一个 ASR CardDefinition 可以分别形成近场和远场 Card：

```yaml
cards:
  near-field-asr:
    uses: para.speech.asr@^1.2
    profile: near-field

  far-field-asr:
    uses: para.speech.asr@^1.2
    profile: far-field
```

Card 可以被标注为 `Sensor`、`Processor`、`Controller-role`、`Actuator` 或 `Agent` 等角色，但角色只用于约束、检查和展示，不形成统一继承树；`Controller-role` 不等于部署控制面的 `DeploymentController`。

Card 不是进程、线程或服务，也不选择 lane、worker 或具体 ExecutionDomain。Card/CardProfile 可以为本次使用请求资源、把 Permission selector 绑定到更窄的具体资源、删除 optional permission 或加强 CardDefinition 的最低隔离约束，但缩小后仍须满足 required operations，不能扩大权限或削弱要求。DeckCompiler 只解析工作负载语义并产生内嵌 canonical DeckTopology 的 DeckLock；DeploymentPlanner 再结合 CardDefinition 要求、Link 交付语义、DeploymentProfile 和目标 Node facts 生成候选 DomainAssignment，DeploymentController 提交 revision，RuntimeHost 才创建相应 CardInstance。

### 5.1 Card 的最小权威内容

Card 的最终序列化字段尚未冻结，但其最小语义边界已经足够清楚：

| 内容 | 是否属于 Card | 边界 |
| --- | --- | --- |
| Deck-scoped Card key | 必须 | 在当前 Deck 中标识一个逻辑 desired slot；序列化可使用 map key 或 `card_key`，最终编码尚未冻结；它不是全局 ID，也不等于 CardInstanceId |
| `uses: CardDefinitionRef` | 必须 | 引用 CardDefinition 的版本约束；精确版本和 Artifact 在 DeckLock resolved closure 中确定 |
| 本次配置或 `CardProfileRef` | 可选 | 必须通过 CardDefinition 配置 Schema；不得注入未声明字段 |
| 语义 role | 可选 | 只参与验证、策略选择和展示，不建立继承树，也不授予权限或决定运行位置 |
| display label/description | 否（可旁置） | 可由 Deck/Canvas metadata 按 Card key 关联，但不进入 Card 的可执行权威，也不改变 DeckLock digest |
| per-use requirement/resource refinement | 可选 | 只能在 CardDefinition 允许的 envelope 内选择具体值、收窄 selector、删除 optional permission 或加强最低约束 |
| PortSpec、Schema、entrypoint、Artifact | 否 | 归 CardDefinition 与 Artifact 所有；Card 只通过稳定 port name 引用 |
| Link、DeliveryProfile | 否 | 归 DeckSpec 所有，endpoint 使用稳定的 `card-key.port-name` 逻辑引用 |
| x/y、zoom、折叠、颜色 | 否 | 归 Canvas View State；改变它们不得改变 DeckLock digest |
| topic/key expression、route、codec、queue | 否 | 归 DeploymentPlan、PortBinding、Fabric 与 Runtime observed facts |
| CardInstanceId、PID/TID、running/health、start/stop | 否 | 归 CardInstance、RuntimeHost、DeploymentController 与 Inspection |
| placement、ExecutionDomain、lane/worker | 否 | 由 Planner/Controller/Runtime 求解和实现，不是作者真相 |
| Grant、token、Secret、raw device/session | 否 | 归 Authority、Driver/Gateway 或 permission-bound access handle |
| 跨 DeckRun 的可变持久状态 | 否 | 不得藏入 Card；产品安装私有状态触发 A0，真正的平台/租户状态必须由对应 CoreService/tenant ownership ADR 证明 owner 与隔离 |

当 DeploymentController 以某个 active Deck workload 的前一 committed plan 为基线计算其下一 revision 时，未改变的 Card key 表示同一逻辑 desired slot，仅用于 diff、稳定 allocation 和 rollout 关联；改名首版一律按 remove + add 处理，不做启发式 rename。删除后重新使用同一 key 也不恢复旧实例或私有状态，state migration 必须有独立 owner 和显式 Receipt。CardInstanceId 仍包含部署/运行与 incarnation 语义，不能直接复用 Canvas id 或 Card key；不同 Deck workload/DeploymentScope 中的同名 key 没有身份关系。Deck Link 引用 Card 的 Port，但 Port 合同仍只有 CardDefinition 一份权威定义。

### 5.2 In、Out、Port、Link 与运行绑定

`Port` 是方向端口的统称；作者侧使用 `In` 与 `Out`，Kernel 使用一套不可变 `PortSpec(direction=IN|OUT)`。方向回答数据相对该 Card 能力边界是进入还是离开，不决定具体 Topic、队列或传输。

```text
CardDefinition declares In/Out
          │ Card 引用端口
          ▼
Deck Link: Card.Out ─────────> Card.In
          │ DeliveryProfile
          ▼
DeckCompiler → DeckLock {canonical DeckTopology}
                         + DeploymentProfile + immutable target facts
                         │
                         ▼
              DeploymentPlanner → DeploymentPlanCandidate
          │ DeploymentController atomic commit
          ▼
committed DeploymentPlan {bindings + execution}
          │ RuntimeSliceProjector + ApplyEnvelopeBuilder
          ▼
RuntimeApplyRequest {source-scope/source-revision/writer-ref/writer-epoch/target/expected-active/source+slice digests/operation-id/deadline/writer-tenure-proof/auth}
          │ RuntimeHost verify/apply
          ▼
PortBinding: one active Zenoh route
          │ {session-local | host-local | remote}
          ▼
bounded Fabric ingress + validation
          │ produces Message
          ▼
bounded target Mailbox
```

| 层 | 权威内容 | 明确不拥有 |
| --- | --- | --- |
| CardDefinition `PortSpec` | 名称、方向、Schema/版本约束、支持的 interaction、required/cardinality、实现硬约束 | Topic/key、Publisher、queue、Zenoh QoS、线程、PID、运行指标 |
| Card/CardProfile | 本次参数和只可加强的约束 | 改写 Port 方向、Schema 或硬限制 |
| Link/DeliveryProfile | from/to、消息种类、deadline、freshness、ordering、overflow、ack、workload envelope、routing/merge 意图 | 具体 Mailbox、Zenoh 参数、codec、ExecutionDomain |
| DeckLock | 精确 CardDefinition、Artifact、Schema 与显式 Adapter 解析结果 | live binding 和 observed state |
| `DeploymentPlan.bindings` | endpoint、Zenoh route/locality、Schema/codec、keyspace、Fabric ingress limits、目标 admission boundary 和安装信息 | observed session、queue depth 和 effect result |
| Runtime `PortBinding`/Inspection | BindingId 作用域内的已安装 binding、BindingEpoch、实际 route、queue/drop/reject/latency 与 desired/observed 差异 | 修改 CardDefinition 或 Deck 拓扑 |

这里的 `Message` 是带 MessageId、causality、deadline 与 trace context 的不可变逻辑 Envelope；接收侧只有在 wire decode 与 Schema/principal/binding 准入成功后才构造 Message，pre-validation encoded frame 不是 Message、也不能进入 target Mailbox。`messaging` 只是实现 Message/Port/Delivery/Mailbox/Binding 的子系统名；`Mailbox` 仍是目标异步边界唯一的有界 Message backlog/admission owner，没有改名。公共 live binding 只叫 `PortBinding`：生产 route 由 Zenoh 承载，测试只称 PortBinding test fixture；不引入 `ZenohBinding`，也不使用会与领域 `Memory` 混淆的 `MemoryPortBinding` 或 `MemoryBus`。

不建立与 DeploymentPlan 并列的 `BindingPlan`。首版每条解析后的静态 1:1 Link 对应一个 BindingId；纯编译不改变 `BindingEpoch`，只有 Runtime/Fabric 对该逻辑 Binding 的 install、reinstall、reconfigure 或 revoke 才改变 epoch。BindingEpoch 只在所属 BindingId 内比较，不能用两个 binding 的 epoch 数值是否相同来判断它们是否同一连接。

同一 DeploymentRevision 与活动 BindingEpoch 内，一个 BindingId 恰有一条接收新 Message/frame 的 active route。route replacement 按 revision-tagged `prepare → activate → drain → retire` 执行，失败时显式 rollback；`activate` 原子切换新准入、旧 route 只 drain。禁止 local+wire 双投、隐式 fallback 和 payload hash/time-window echo 去重。fan-out 必须为每个 destination 创建独立 BindingId，再由父 interaction 聚合 per-destination 结果。

`In`/`Out` 适合单向 Signal、Event，以及受完整安全与 Receipt 契约约束的 Command/Receipt。它们不是所有交互的唯一模型：并发 TTS 需要 request/chunk/cancel/final correlation，导航 Operation 需要 goal/feedback/cancel/result，Call/Query、Tool 和权威 State 也有各自完整性要求。这些逻辑 interaction 可以在运行时编译成多条通道，但不能让 Deck 作者用几条无关联 Link 手工拼装后假装原子。

CardDefinition 版本不能代替消息 Schema 版本。Zenoh 的 `session-local`、`host-local`、`remote` 与确定性 PortBinding test fixture 都必须通过同一 Message/Envelope/Schema/Mailbox conformance；转换由显式 Adapter/Card/Gateway 提供，Compiler 不静默猜测。只有目标硬件 benchmark 证明 Zenoh `session-local` 不满足 SLO，才可通过独立 ADR 考虑与 Zenoh route 互斥的同进程 route，且不得跨 Binding 传递任何语言私有可变对象、`Arc<Mutex<_>>` 别名、裸指针或 Runtime handle。一个 In 的 fan-in、一个 Out 的 fan-out、merge/routing 和 partial acceptance 都必须显式声明并在启动前检查；首版未支持的模式应 fail-fast。

`send()`/`enqueue()` 的 accepted 只表示当前 owner 接受了 handoff，不表示远端已准入、CardInstance invocation 已完成或物理效果成功。fan-out 需要 per-destination 结果或明确聚合规则，领域终态由有权判定的 Receipt owner 产生。

完整证据、方案比较、反例和最小切片见 [CardDefinition 输入输出、Port、Link 与运行绑定研究](../research/card-definition-ports-links-and-bindings.md)。

## 6. CardInstance

`CardInstance` 是 Card 被 RuntimeHost 启动后的受管理运行身份。它拥有：

- 稳定 `instance_id` 与所属 `deck_run_id`。
- 当前生命周期、Readiness 和健康状态。
- 实际 Placement 与 ExecutionDomain 引用。
- 已验证的 Binding 和最小权限。
- 一个私有 Card 实现对象及其由 RuntimeHost 调用的可选回调；该对象没有第二份系统身份或生命周期状态机。
- Mailbox、资源使用和最新故障的 Inspection 事实。
- 启动、排空、停止和失败 Receipt。

生命周期、取消、超时、重启预算、进程终止和 wedge 判定只由 RuntimeHost 拥有；RuntimeHost 内部 RecoveryEngine 根据 LivenessState、RuntimeFailureFact 与 RecoveryPolicy 求值，但不是第二 owner。Card 实现对象的 `on_start`/`on_stop` 等回调只在 CardInstance 所属的结构化 ownership scope 中运行。

### 6.1 独立开发、单主体 Harness 与运行探测

Card 可以独立开发和隔离验证，但不能建立独立于 Deck/Deployment/RuntimeHost 的生产生命周期：

| 入口 | 是否允许 | 边界 |
| --- | --- | --- |
| CardDefinition 纯校验 | 允许 | 只检查不可变合同、配置、Port、Requirements、ExecutionRequirements 与 entrypoint；不创建运行身份 |
| 语言内 workload unit profile | 允许 | 在 Python/Rust/C++ 各自单元测试中直接构造私有实现并注入 virtual clock、cancellation、录制型 Port handle 和 typed fake；不经过 Artifact/ProcessDomain 装载，不产生 CardInstance/Ready 结论 |
| internal 单主体 Card component Harness | 允许 | 由 production projector/builder 生成 canonical RuntimeApplyRequest，RuntimeHost 创建唯一 subject CardInstance；fixture 不是 production source |
| one-subject Deck 开发运行 | 允许 | 显式或生成 DeckSpec，完整走 Compiler → Planner → DeploymentController → RuntimeHost；“ephemeral”仍有 revision、digest 和 fencing |
| `CardDefinition.run()`、`Card.run()`、作者直接 new/start CardInstance | 禁止 | 会产生第二份 desired truth 和 lifecycle owner |

单主体 Harness 不承诺物理上恰好只有一个 CardInstance。L2 只允许非 Card 的 source/sink test adapter 在已安装 PortBinding 边界提供刺激和观测；若 fixture 本身需要 Card 语义，必须在 L3 编译前进入 ephemeral Deck、committed plan 与 Slice，再由 RuntimeHost 正常创建。test fixture 只能形成 local/diagnostic 证据；它不证明 DeploymentController tenure、production readiness、真实物理安全或现场可用性。P2a 的 PortBinding/Mailbox fixture 不执行 Card callback；P2b 只验证 RuntimeHost-owned CardInstance/Domain callback seam，完整 RuntimeAssemblyEngine 与 one-subject Deck smoke 属于 P2e。

不建立每张 Card 一个 `probe() -> bool`。startup completion、RuntimeHost/Domain liveness、exact-revision readiness、ongoing health、test observation 与有副作用的设备诊断分别拥有不同事实和动作 owner。实现对象可以通过未来的窄、generation/revision/freshness-fenced 通道贡献“模型已 warm”等语义证据，但只有当前 committed Slice 的 readiness contract 能要求该证据，RuntimeHost 仍独占最终判定；CardDefinition 不是 Runtime 的第二输入，Card 也不能写自己的生命周期终态或直接触发 restart。通用 L0–L3 Harness 强制 effect-denied，会移动、加热、写入、reset 或 calibrate 设备的检查必须进入独立 L4/H1 路径，并受 Authority、Lease/Fence、Safety、Enforcement、Hardware activation 与 Effect Receipt 约束。

完整方案比较、Harness 分层、证据等级、故障矩阵和失效条件见[专项研究](../research/card-independent-development-testing-and-probe-boundaries.md)。

## 7. Deck

`Deck` 是 ParaEGOX 面向用户的声明式可执行工作负载/组合单元：一组 Cards、Links、配置引用和平台需求形成可共同验证、锁定与部署的组合。它是对 PhanthyMotus `CanvasLayout + Project` 心智模型的独立演进，不是 Motus 已有类型。

首个 reference profile 可以把“一个产品应用”完整表达为一个 Deck，但这只是产品映射，不是永久身份等价。Deck 不是 Product、安装包、Release、Installation、DeploymentScope、Runtime、CoreService 容器或任意脚本执行器。为避免一个对象混合期望、解析结果和运行事实，Deck 分为三种权威形态。

### 7.1 DeckSpec

`DeckSpec` 是用户编写的期望状态，通常由 `deck.yaml` 表达。它可以声明：

- Deck 元数据和 Schema 版本。
- Cards 与 Links。
- CardDefinition 版本约束。
- CardProfile 引用和有限覆盖。
- 所需 ServiceContract、Permission scope 与目标 Feature；不保存 Grant、token、Secret 或 observed FeatureReport。
- Link 的 DeliveryProfile，包括消息类型、deadline、freshness、ordering、overflow、ack need、应用 criticality，以及 rate/最小到达间隔、burst、payload bytes 上界和 max inflight 等 workload envelope。
- Placement 提示、资源约束和共置/反共置意图；不指定 worker、lane、线程或 PID。
- 应用入口、启用条件或启动策略。

示例仅表达目标语义，不是已冻结 Schema：

```yaml
schema: paraegox.deck/v1
name: voice-robot

requires:
  services:
    - authority.v1
    - fabric.v1
    - model.v1
  permissions:
    - resource: microphone/*
      operations: [read]
  features:
    - node.audio-input

cards:
  microphone:
    uses: para.audio.microphone@^1.0
    profile: robot-mic

  asr:
    uses: para.speech.asr@^2.1

links:
  - from: microphone.audio
    to: asr.audio
```

### 7.2 DeckLock

`DeckLock` 是纯 DeckCompiler 调用内部 DeckResolver，对 DeckSpec、CardDefinition Catalog 和 Artifact Metadata 求解、规范化并验证后的唯一可持久解析产物；它独立于 live Node facts，通常由 `deck.lock` 表达。DeckResolver 可以作为纯步骤单独测试，但不产生第二份权威中间对象。ADR-0004 提议把内容严格分为结构子树和解析闭包：

| DeckLock 区域 | 唯一拥有的内容 |
| --- | --- |
| canonical `DeckTopology` | 稳定 Card closure key、以 closure key 限定的 Port endpoint key、Link、DeliveryProfile key/ref、RequirementRef key 及其 canonical directed-multigraph 结构关系；同一 Card pair 的不同 Port Link 作为 parallel edge 保留 |
| resolved closure | 精确 CardDefinition/version、Port/Schema、Artifact/adapter ref 与 manifest/candidate digest、DeliveryProfile payload、完整依赖闭包、Schema/协议版本、完整 Service/Permission/Feature Requirement contract/payload/兼容范围/声明约束、声明平台/ABI 约束，以及影响解析结果的 CardProfile/Feature 选择 |

目标 Node 的精确 artifact variant 由 DeploymentPlanner 选择并写入 DeploymentPlanCandidate，DeploymentController commit 后才进入 committed DeploymentPlan。live Grant、provider、observed FeatureReport 与某个 live Node 的匹配结果均不进入 DeckLock。

DeckLock digest 覆盖 canonical DeckTopology 与上述全部 resolved closure。Topology 不能重复精确 definition/version、Schema、Artifact 或完整 Requirement payload，也不能成为旁路文件、Canvas state 或 DeploymentPlanner 的第二个可漂移输入。canonical validator 必须拒绝悬空 key、重复语义 entry、同一 identity 的冲突版本、重复 Requirement 和 key/payload 不匹配。DeckLock 不负责安装或启动；生产运行必须能关联到确定的 DeckLock digest，未锁定的开发运行必须明确标记为非可重复状态。

DeckLock 不保存 live provider、NodeStatus/FeatureReport、placement、Zenoh route、DomainAssignment 或 target admission 结果；这些 target-specific 决策由 DeploymentPlanner 独占计算并进入 DeploymentPlanCandidate，原子 commit 后才成为 committed DeploymentPlan 的内容。

DeckTopology 是 dataflow declaration，不是 live execution graph。`A.Out → B.In` 只表示 `DataLink`，不表示 A 必须先于 B 启动；Runtime 通常先准备 B 的 ingress/Mailbox，再开放 A 的 producer egress。Service readiness、activation dependency、dependency-loss 与 drain order由 DeploymentPlanner 独立编译。基础结构可报告 SCC，但在 feedback/delay/seed/backpressure contract 通过 ADR 前，P2e 对 cyclic Deck fail-fast，不按声明顺序降级运行。

### 7.3 DeckRun

`DeckRun` 是一次真实运行的身份和观测入口，而不是对 RuntimeHost 的对象引用。它至少关联：

- `run_id`、DeckSpec digest 和 DeckLock digest。
- Deployment 与目标节点。
- CardInstance 集合。
- 当前状态、开始和结束时间。
- Readiness、故障、恢复和关闭 Receipt。

OpsService 与 TUI 通过 InspectionProtocol 查询 DeckRun，不能直接读取 RuntimeHost、CardInstance 私有实现对象或 Driver 的内部字段。

DeckRun 不是直接的 stop/delete target。经授权的操作请求 DeploymentController deactivate/replace Deck workload，旧 DeckRun 随后进入 terminal state；terminal 只结束本次运行，不推导安装私有数据 GC。

### 7.4 产品 Application 边界

“应用”目前是产品和自然语言概念，不是已经冻结的公共领域类型。文案、示例和 TUI 可以把一个完整 Deck 称为应用，但公共 DTO、Receipt、权限和日志必须使用真实的 DeckLock digest、`DeploymentId@DeploymentRevision`、DeckRunId 或 CardInstanceId；禁止为了 UI 分组加入没有 owner 的 `application_id`。

当前不建立 `ApplicationSpec`、`ApplicationLock`、`ApplicationInstance`、`ApplicationController` 或 `applications/` 空包。若未来出现多 Deck 产品、跨 DeckRun 的私有持久状态、同一产品多次隔离安装、或 Deck/Gateway/client/service 共同签名发布闭包，再依据 [Application 边界研究](../research/application-deck-card-service-boundaries.md)进入 Proposed ADR。

未来 Application 即使成立，也只能是 Deck 之上的控制/交付/所有权聚合：它不能拥有 placement、进程、PortBinding、CoreService 生命周期或 reconcile 写权，DeploymentController 仍是 DeploymentScope 的唯一 desired-state owner，运行事实仍是 DeckRun、CardInstance、ServiceInstance 和各 Gateway owner 的 facts。

### 7.5 应用私有持久状态

当前模型明确存在一个后置能力：只属于一个稳定产品安装、需要跨 DeckRun/升级保存、但不应跨产品共享的领域状态。

- 不能把它藏入 Card 类变量、进程全局 registry 或 CardInstance 私有状态。
- 不能仅因其长期运行就提升为平台 CoreService。
- 在稳定 Application/Installation owner 出现前，不能只给 ServiceSpec 增加一个自由字符串 scope 假装解决生命周期。

首版不支持一等的 application-owned durable service。真实需求出现后，应优先研究复用 `ServiceContract → ServiceSpec → ServiceInstance` 机制，并分开 service workload owner、durable data owner 与 storage custodian，再补足稳定 owner ref、lifetime、state namespace/schema、migration、backup/retention、retain/delete/transfer、GC authority 和 Receipt；在这些语义验证前不冻结 `ApplicationService` 类型，也不预设每个 installation 都需要独立服务进程。

如果一个临时能力只随 DeckRun 存活并被多张 Card 使用，先判断它是否就是一张拓扑可见的 Card；只有真实消费者必须通过版本化 ServiceContract 共享它时，才单独研究 DeckRun-scoped ServiceSpec。这种 run-bound seam 不需要 Application identity，也不等于平台 CoreService。

## 8. Canvas

`Canvas` 只可视化编辑 DeckSpec，并只读展示由 DeckCompiler 派生的 DeckTopology 验证投影。它可以创建 Card、编辑 Link、选择 CardProfile 并展示验证错误，但不能直接编辑或持久化 topology，也不是生产运行时的权威状态来源。

```text
Canvas editing state
        │ save / validate
        ▼
     DeckSpec
        │ resolve
        ▼
     DeckLock
        │ DeploymentPlanner
        ▼
DeploymentPlanCandidate
        │ DeploymentController atomic commit
        ▼
committed DeploymentPlan
        │ RuntimeSliceProjector + ApplyEnvelopeBuilder
        ▼
RuntimeApplyRequest {target RuntimePlanSlice + writer context + CAS controls}
        │ RuntimeHost verify/apply
        ▼
     RuntimeHost
        │ observed facts / Receipt
        ▼
DeploymentController reconcile → DeckRun / Inspection projection
```

Card 的屏幕坐标、缩放和折叠状态属于 Canvas View State，不应影响 Deck 的运行语义或 digest。

## 9. CardDefinition、Card 与 CoreService

| 属性 | CardDefinition | Card / CardInstance | CoreService |
| --- | --- | --- | --- |
| 含义 | 不可变的可复用能力合同，含 Artifact export/entrypoint 引用 | Deck 中的配置节点及运行身份 | 平台长期共享能力 |
| 定义/期望 owner | CardDefinition 作者；DeckCompiler 解析引用；Artifact/ArtifactStore 分别拥有内容身份与存取，不因此成为产品安装 owner | DeckSpec 作者声明 Card；DeploymentController 提交实例计划 | ServiceSpec owner 声明；DeploymentController 提交实例计划 |
| 本地实例生命周期 owner | 实现对象没有独立系统生命周期；由所属 CardInstance 托管 | 目标 RuntimeHost | 目标 RuntimeHost |
| 典型寿命 | 跨多个 Deck 版本存在 | 随 DeckRun 存在 | 随节点或平台存在 |
| 运行状态 | 定义无运行状态；私有领域状态位于 CardInstance 内的实现对象 | CardInstance 持有系统状态 | Service 自己持有 |
| 依赖获取 | 声明 Required Binding | 获得已验证的最小 Binding | 获得 ServiceContext 中的声明依赖 |
| 能否拥有平台权威 | 否 | 默认否 | Authority、Resource 等特定服务可以 |
| 能否被普通 Deck 停止 | 不适用 | 可以按授权停止 | 默认不可以 |

ASR、目标检测、导航处理和 Agent Reasoner 可以是 CardDefinition。Authority、Resource、Fabric 和 InspectionService 是 CoreService，不因在 Canvas 中显示为系统卡片而变成普通 CardDefinition。

只服务一个稳定产品安装、需要跨 DeckRun 持久的领域能力并不会因“长期运行”自动成为 CoreService。当前尚无正式 application-owned service 模型；这类需求必须触发 Application/Installation owner 研究，不能塞入 Card 全局状态，也不能扩大成跨产品平台权威。

Gateway 是“外部生态语义、安全和故障边界”这一角色，不因需要运行就自动成为 CardDefinition、Card 或 CoreService。长期 Gateway workload 的 Runtime/external-workload-manager envelope、以及非 Card Gateway endpoint 如何参与 Deployment-owned connection 仍需 Proposed ADR。Deck 只声明 Port/Service/Permission/Feature 等应用需求，不拥有浏览器 session、WebRTC peer、ICE/TURN、外部 token 或 Gateway lifecycle，也不直接选择外部 transport。

### 9.1 ToolDefinition 与 Provider 关系

Tool 是调用语义，不是 CardDefinition 的组成字段，也不是 CoreService、Driver 或 Gateway 的别名。目标关系为：

```text
ToolDefinition
      │ referenced by one or many provider declarations
      ▼
ToolProviderDeclaration
      │ Trust/Policy admits definition/provider/artifact claims
      ▼
ToolAdmissionDecision
      │ Deployment commits desired provider target
      ▼
ToolBinding
      │ Runtime reports authenticated readiness/generation
      ▼
ToolCatalogSnapshot {resolved provider}
      │ fixed into RunExecutionSnapshot
      ▼
ToolView → ToolInvocation → InvocationAttempt(one concrete provider)
```

这些名称除 `ToolDefinition`、`ToolCatalogSnapshot`、`ToolView` 与 `ToolInvocation` 的既有研究用法外仍是候选，最终公共 Schema 等待专项 ADR。稳定边界如下：

- CardDefinition 不定义或复制 ToolDefinition。一个 Artifact 可以把 CardDefinition 与 ToolProviderDeclaration 作为 sibling exports；CardInstance Ready 后才可能成为具体 Provider。
- 一个 CardInstance/已准入 CoreService/Gateway-backed adapter 可以提供零到多个 Tool；一个 ToolDefinition 也可以有多个兼容 Provider。ToolAdmissionDecision 独立固定 definition/provider/Artifact/policy 的有效风险与权限边界；committed ToolBinding 只保存 desired provider target，不吸收 Runtime observed facts；ToolCatalogSnapshot 再以 authenticated readiness facts 固定具体 ProviderInstanceRef/generation。每个 InvocationAttempt 只能固定一个 Provider 与 Artifact；若后续证明需要 ToolBindingEpoch，它与 PortBinding BindingEpoch 必须分域。
- 多个组件共同完成一个 Tool 时，必须有一个 composite Provider 独占 provider-side acceptance/dedup/subcall journal、子调用 correlation、cancel/reconcile 和 Tool 级终态；client Attempt journal 仍归调用方。单一 owner 也不自动提供跨故障域原子性；没有事务/barrier 证据时必须暴露 partial/`Uncertain`。
- ToolSet 只属于 DeckSpec 中特定 Agent Card 使用并引用逻辑 Tool，Session/Run 只能收窄；ToolCatalogSnapshot 由 committed binding、admission decision 与 authenticated runtime facts 纯投影，ToolView 是某次 Session/Run 的可见投影。三者都不选择新 Provider 或授予调用权限。
- Provider 的 state、model handle、stream cursor 和 cache 属于 CardInstance/CoreService 等实际 owner；ToolDefinition 无运行状态。
- host `start/stop`、desired config、Probe/Inspection 与业务 Tool action 走不同管理路径。不同 EffectClass、权限、幂等性或终态语义的 action 应拆成不同 ToolDefinition。
- MCPGateway 可以把外部 MCP Tool 转换为待准入的定义与 Provider candidate，但 `mcp_id`、server name 和 endpoint 不是内部权威身份。
- physical/write Tool 不取得 raw Driver handle；它只能进入 typed Operation 与 Authority、Lease、Safety、Enforcement、EffectReceipt 链。

完整证据、Provider 选择/failover、streaming、组合 Provider 与最小切片见 [Tool 定义、Provider 绑定与调用边界研究](../research/tool-definition-provider-binding-and-invocation.md)。

## 10. Card 实现、Driver 与 ExecutionDomain

- CardInstance 私有的 Card 实现对象承载领域计算和私有状态；它的方法是普通回调，不建立独立 `Handler` 领域类型，也不形成第二个系统身份。
- `Driver` 只负责设备、仿真器或具体硬件/SDK 边界，可以服务某张 Card，也可以由平台作为受保护 workload 托管。ROS2、MCP、A2A、浏览器、WebRTC/WebXR 等外部生态与协议语义属于 Gateway；Bridge 只解决无状态连通，不能替代 Gateway 的类型、身份、Authority 和失败转换。
- `ExecutionDomain` 是 RuntimeHost 对本地 Loop、Thread 或 Process 执行边界的所有权对象。

Camera/Audio Driver 可以产生 `MediaSample`/`EncodedVideoSample` 等 typed payload，但不自行启动公网 HTTP/MJPEG/WebRTC/static server 或 PeerConnection。`FrameRef` 保留给物理坐标系引用；媒体大载荷通过 BlobRef/BufferRef 表达。WebXR 是浏览器 API，不是 CardDefinition；可复用 encoder、format converter 或 pose mapper 只有满足独立复用、配置、实例隔离、ExecutionRequirements 和观测边界时才值得定义 CardDefinition。

CardDefinition 只声明 ExecutionRequirements、平台能力和 minimum isolation，不能自行选择或创建 ExecutionDomain。DeckCompiler 解析声明，DeploymentPlanner 根据这些要求、应用 SLO、policy 与目标 Node facts 计算候选 Placement/Domain，DeploymentController 验证并提交 revision。需要硬终止、未知原生库或可能永久卡死的实现必须进入 ProcessDomain；ThreadDomain 卡死时只能报告 wedged 并升级，不能声称线程已经被杀死。

远端 placement 不创建 Remote ExecutionDomain：DeploymentController 拥有 desired placement，调用侧只拥有 typed service client/permission-bound access handle，目标 Node 的 RuntimeHost 拥有目标实例生命周期。

### 10.1 Lane 不是 CardDefinition、Card 或 Deck 概念

过往实现经验表明，把 admission、调度、执行隔离和 placement 合并成公共 lane-like 对象会造成跨层耦合。ParaEGOX 只保留行为需求，不保留这种公共对象：

- `Mailbox` 负责积压、顺序、freshness 和 overflow。
- `ExecutionDomain` 负责 Loop、Thread 或 Process 执行与故障隔离。
- Runtime dispatcher 负责多个 ready Mailbox 的优先级、公平性、deadline 和最大 burst。
- RuntimeHost 是 Domain/child 卡死、退出、重启、quarantine 和 epoch fencing 的唯一生命周期动作 owner；内部 RecoveryEngine 只求值 RecoveryDecision。RuntimeHost 自身 stall 由不同故障域的 NodeDaemon 观测，并由 profile 指定的 OS service-manager recovery owner 恢复。

CardDefinition、Card、DeckSpec、DeckLock 和公共 Kernel Schema 都不出现 `lane_id`、`thread_lane` 或 `process_lane`。Runtime 内部可以按编译结果建立多级 ready queue，但它没有公共身份、独立生命周期或第二份 payload queue。

### 10.2 声明、编译与观测的唯一链路

```text
CardDefinition.ExecutionRequirements
              +
DeckSpec {Cards + Links + DeliveryProfile}
              +
DeploymentProfile + NodeFacts
              │
              ▼
         DeckCompiler
              │ DeckLock {canonical DeckTopology}
              ▼
      DeploymentPlanner (pure)
              │ DeploymentPlanCandidate
              ▼
      DeploymentController atomic commit
              │
              ▼
DeploymentPlan
├── bindings
│   ├── endpoints / route / Schema / codec
│   └── admission boundary / Binding install information
└── execution
    ├── DomainAssignment / MailboxSpec / DispatchPolicy
    ├── AdmissionBudget / OutstandingBudget
    ├── ExecutorBudget / IPC credits / retained-byte budget
    ├── LivenessSpec / FailureContainmentSpec / RecoveryPolicy
    ├── typed activation dependency / readiness / activation group
    ├── consumer ingress / producer egress / dependency-loss / drain order
    └── RevisionTransition
              │
              ▼ project immutable target slice
RuntimeApplyRequest {source-scope/source-revision/writer-ref/writer-epoch/target/expected-active/source+slice digests/operation-id/deadline/writer-tenure-proof/auth + RuntimePlanSlice}
              │
              ▼
RuntimeHost.RuntimeAssemblyEngine
              │ prepare / ready / activate / drain / rollback
              ▼
RuntimeHost → DomainInstance / CardInstance / observed facts
```

| 层 | 权威内容 | 不拥有 |
| --- | --- | --- |
| CardDefinition | 实现的内在 ExecutionRequirements | 应用级优先级和具体 executor |
| Card/CardProfile | 本次配置、资源请求和更严格约束 | 削弱 minimum isolation、创建线程/进程 |
| Link/DeliveryProfile | deadline、freshness、ordering、overflow、ack、criticality 和 workload envelope | Zenoh 参数与 Runtime queue |
| DeckSpec | 可编辑的 Cards、Links、Requirement 和端到端工作负载意图 | resolved refs、Domain、worker、CPU affinity |
| DeckTopology（DeckLock 结构子树） | Card/Port endpoint/Link/DeliveryProfile/Requirement 的结构 key/ref | 精确定义、Schema、Artifact、payload、依赖闭包与 live facts |
| DeckLock resolved closure/digest | 精确定义、Schema、Artifact、payload、依赖闭包和锁定兼容约束；digest 同时覆盖 closure 与 DeckTopology | live provider、target facts、placement 和 observed state |
| DeploymentProfile | 目标平台、placement、资源和经验证的专家 override | 业务消息语义和 Authority |
| DeploymentPlan.bindings | 编译后的 endpoint、route、Schema/codec、Fabric ingress limits 和 binding 安装要求 | observed session、queue 和 effect result |
| DeploymentPlan.execution | 编译后的 Domain、Mailbox、admission/outstanding budget、dispatch、liveness/failure-containment/recovery、activation/readiness/egress/drain 和 revision transition | observed 运行事实 |
| RuntimePlanSlice | Schema/apply protocol 归 Runtime；value 是 DeploymentController 对唯一 DeploymentPlan 的 canonical target projection，带 source/slice digest | Card/Deck/Deployment ownership 或第二份 desired truth |
| RuntimeHost/Inspection | 应用计划并证明 PID/TID/loop/epoch 等实际事实 | 静默重分类或降低隔离 |

完整执行真相不再单独建立一个可由用户编辑的 `ExecutionContract`。CardDefinition 的要求和 Deck 的 workload SLO 只有经 DeploymentPlanner 在目标平台 facts 上形成 DeploymentPlanCandidate、再由 DeploymentController 原子提交 allocation delta/revision/committed plan 后，才成为可执行的 `DeploymentPlan.execution`。`runtime/contracts` 拥有 apply Schema/protocol，DeploymentController 拥有唯一 committed DeploymentPlan，并通过 tenure-neutral Slice projector 与 writer-context apply builder 交给 RuntimeHost；RuntimeHost 内部 RuntimeAssemblyEngine 只执行 Slice 中已编译的本地装配约束，不从 Deck Link 猜测启动顺序，也不持久化第二份 graph。声明与 observed Domain 不一致时不能 Ready。

Deck 只能请求 control/high criticality，不能通过把所有 Link 标成 control 自行获得高优先级。DeploymentPolicy 必须授权、保留容量并验证 arrival/run bound 可行性；不可行时拒绝计划，不静默降级。

完整证据、方案比较和失效条件见 [Runtime 执行模型、调度与恢复研究](../research/execution-model-scheduling-and-recovery.md)。

## 11. 历史 Module 与 Bundle 名称的中立拆分

本节只记录跨层名称暴露出的中立边界，不是 EAGOS 私有实现清单或逐字段迁移表。ParaEGOX 不使用 `Module` 作为公共领域名；相关责任族按独立 owner 拆分：

| 历史 Module-like 责任族 | ParaEGOX 所有者 |
| --- | --- |
| 可复用能力合同、Port、配置 Schema 与 Requirements | CardDefinition |
| Deck 中的一次具名配置使用 | Card / CardProfile |
| 代码、模型、二进制与 entrypoint | Artifact；CardInstance 私有实现对象执行领域逻辑 |
| 运行身份、本地执行、生命周期与恢复 | CardInstance / ExecutionDomain / RuntimeHost |
| 连接、交付和运行传输 | Deck Link / DeliveryProfile / PortBinding / Mailbox / Fabric |
| 外部协议或硬件边界 | Gateway / Driver |
| 诊断和运维解释 | InspectionService projection / OpsService transaction explanation |

`Deck` 是 ParaEGOX 独立定义的声明式可执行工作负载/组合单元，其 Card/Canvas 心智模型来自 PhanthyMotus，但 Deck 并不是 Motus 既有类型。EAGOS 的 Bundle 不是单一对象，而是同名的 source、lock、交付/安装与 runtime/deployment 协作表面族；各表面的成熟度和动作 owner 不相同，不能因共用名称就视为一个完整生产对象。Deck 只承接其中 source 表面的工作负载组合子集：

| 历史 Bundle surface 的责任族 | ParaEGOX 所有者 |
| --- | --- |
| Module-like 实例组合、连接和符合 Card/Deck Schema 的 per-use workload 配置 | DeckSpec |
| 解析、规范化和版本锁定 | DeckCompiler 产生唯一 DeckLock；内部 DeckResolver 只是纯步骤，不是 owner |
| 软件、模型与二进制内容/身份 | Artifact；ArtifactStore 只负责保存和取回内容 |
| 目标 placement、启动与 rollout | DeploymentPlanner 计算候选；DeploymentController 提交 revision 并协调 rollout |
| 本地线程、进程与运行生命周期 | RuntimeHost |
| 安装、激活、升级、回滚与卸载 | 当前没有稳定 Installation owner；真实需求触发 A0，DeploymentController 不兼任安装 owner |
| 产品体验、策略、客户端资产、私有数据和其他非 workload 配置 | 不整体进入 DeckSpec；分别归领域 policy/profile、Artifact 或未来 Product/Application/Installation owner |
| 签名、SBOM 与可信校验 | Artifact Trust；不等于安装或运行 owner |
| 可发现 Deck definition/release 元数据 | DeckCatalog 候选；不承担已安装产品索引 |
| 离线交换/交付格式 | 名称和 owner 尚未冻结；`DeckArchive` 仅为研究候选 |
| 运行诊断和解释 | InspectionService projection / OpsService transaction explanation |

这意味着 Card 与 EAGOS Bundle 的比较本身就是层级错误：Card 是一份工作负载内部的单次能力使用，EAGOS Bundle 一词覆盖跨层 surface family。更准确的迁移关系是：

| 既有概念 | 实际粒度 | ParaEGOX 去向 |
| --- | --- | --- |
| Motus Card | Canvas 中的一次 Tool/能力配置使用 | 保留为 Card，但剥离 UI、动态 Topic 与运行身份 |
| Motus Canvas 的语义连接 | 多张 Card 之间的图 | 进入 DeckSpec 的 Cards 与 Links；视图状态继续独立 |
| Motus `PerceptionBundle` | 单 endpoint/进程内的 Plugin/Tool 实现聚合 | 作为 provider 或 Artifact 内部实现细节，不成为公共工作负载类型 |
| EAGOS Module | 定义、配置使用、实现、运行身份与 Runtime 能力的混合 | 拆到 CardDefinition、Card、Artifact、CardInstance、RuntimeHost 等 owner |
| EAGOS Bundle surface family | 同名覆盖工作负载 source、lock、交付/安装和 runtime/deployment 表面 | 只将 source 中的工作负载组合子集交给 Deck；其余按上表拆分 |

### 11.1 为什么 Deck 不改名为 Bundle

ParaEGOX 当前保留 `Deck`，不新增泛化 `Bundle` 领域类型，也不提供 `Bundle = Deck` 的 Schema、包名或 API 别名。理由不是刻意避开旧名，而是两个词指向不同边界：

1. `Card → Deck` 是一致的组合隐喻：Deck 包含 Cards；它准确表达工作负载组合，不暗示交付格式或安装身份。
2. EAGOS 的 Bundle 名称已覆盖远超组合图的多种 surface；复用该词会制造“其他旧 surface 以后也应回到这里”的错误兼容预期。
3. Motus 的 `PerceptionBundle` 又表示 endpoint/进程内插件聚合，与 EAGOS Bundle 和 ParaEGOX Deck 都不同；同名只会增加第三种语义。
4. 软件工程中的 bundle 通常还会被理解为打包或分发物，而 Deck 明确不是 Artifact、Release、Installation 或离线包。
5. 真正的名词一致性应是“一个词只有一个 owner 和生命周期”，不是让新旧系统表面同名。

最强反例不是保留旧式万能聚合，而是把 source 层收窄成 `BundleSpec`，同时把 Lock、Artifact、Installation 与 Deployment 全部拆开。这在技术上完全可行，并能降低旧用户和资产的迁移成本。当前提案仍选择 Deck，是因为 ParaEGOX 尚无 Bundle source 兼容承诺、公共 Schema 和实现负担，Motus 又已把 Bundle 用于另一种 provider 聚合；在这个干净起点上，Card/Deck 的认知成本更低。若 ADR-0004 接受前出现必须直接导入既有 Bundle source 的真实产品要求，应以迁移样本和转换成本重新打开命名裁决，不能拿本文结论压过新证据。

迁移文档可以写“EAGOS Bundle source 的工作负载组合子集迁移到 ParaEGOX Deck”，但不能宣称两者等价。未来若需要离线单文件交付，优先研究 `DeckArchive` 等限定名称；若出现多 Deck 产品发布和稳定安装身份，则在 admission gate 后研究 `ProductRelease`/`Installation` 等真实对象，而不是重新扩张 Deck。本文只拒绝 Bundle 作为 Deck 别名，不永久禁止后继 ADR 为全新、窄边界对象采用带限定词的名称。

## 12. 强制不变量

1. CardDefinition 是不可变数据，不是业务基类；实现对象不能通过继承自动获得 Bus、线程、进程、TF、Tool、配置、Probe 或完整 Runtime。
2. Card 实现对象可以提供生命周期回调，但不拥有、推进或恢复系统生命周期；CardInstance/RuntimeHost 才是 owner。
3. Card 不直接持有 RuntimeHost、CoreService 或 Driver 内部对象。
4. DeckSpec 只声明 Service、Permission 与 Feature Requirement，普通 Deck 无权创建、停止或替换平台关键服务，也不保存 Grant、token、Secret 或 observed FeatureReport。
5. Deck 引用 Artifact，不把 Artifact 安装、发布和可信校验实现吞入自身。
6. DeckLock 只描述解析结果，不执行安装或启动。
7. DeckRun 只提供稳定身份和 Inspection 投影，不成为新的全局 Runtime。
8. Canvas 不是持久化和运行时真相来源。
9. CardProfile 只承载 Card 的参数、资源与环境配置，不偷偷改变 DeckTopology 或扩大权限；DeploymentProfile 和 ZenohTopologyProfile 是不同类型。
10. 所有 EAGOS 经验必须转化为中立行为需求后独立实现，不建立兼容层。
11. CardDefinition、Card、Deck 和 Kernel 公共 Schema 不声明 Lane、线程、PID 或 Runtime ready queue。
12. `DeploymentPlan.execution` 是 desired execution 的唯一权威；Runtime observed facts 不一致时不得 Ready。
13. workload envelope、run bound 或其 provenance 为 unknown 时，Deck 不得凭 priority 声明获得未证明的 control SLO。
14. Mailbox 容量不代替 inflight、executor/IPC credit、child work 和 retained-byte 预算；这些都由执行计划统一限界。
15. ProcessDomain crash 不证明副作用失败或设备已取消，CardInstance 及其私有实现对象不得绕过 RecoveryPolicy 自动 replay。
16. `In`/`Out` 声明不包含 Topic、Publisher、Subscriber、Session、queue、thread、PID、metrics 或 live binding。
17. CardDefinition 定义 Port，Card 只引用 Port、声明本次配置并强化允许的约束；Deck Link 连接 Card Port 并拥有 DeliveryProfile，实际 route 只存在于 DeploymentPlan 与 Runtime PortBinding。
18. 每个 CardInstance 默认独占一个私有 Card 实现对象；共享模型、设备和状态必须有显式 owner。
19. Schema 兼容、required/cardinality、routing/merge 在启动前验证；Zenoh 各 route locality、PortBinding test fixture 与任何经 ADR 准入的优化都不能绕过契约。
20. enqueue/handoff、remote admission、Invocation Receipt 和 effect Receipt 不合并成一个 ack。
21. 原生 Zenoh/raw Fabric 权限按 scope 指向 Fabric resource 的 `CapabilityGrant` 授予，不按 CardInstance、CoreService、Driver 或 Gateway 名称自动授予。
22. 同一 DeploymentRevision 与活动 BindingEpoch 内，每个 BindingId 只有一条 active route；禁止 local+wire 双投、隐式 fallback 和内容/时间窗 echo 去重。
23. RuntimeHost 只通过 runtime-owned Schema/protocol 消费 DeploymentController 产生的 canonical RuntimePlanSlice value；请求必须带 runtime-owned `SourceScopeRef`/`PlanWriterRef`/`PlanWriterEpoch`/target/`SourcePlanRevision`/expected-active/source+slice digests/operation id/deadline/WriterTenureProof/auth，不得 import Card、Deck、Compiler、DeploymentController 或可编辑 DeploymentPlan。
24. Artifact、Evidence、Secret、Workspace、Blob/Buffer 与服务状态使用 owner-specific typed reference/client，不通过 Kernel VFS、万能 ObjectRef 或 URI Service Locator 聚合。
25. DeckLock 是 DeckCompiler 唯一可持久解析产物；canonical DeckTopology 必须内嵌并受 DeckLock digest 覆盖，DeploymentPlanner 不接收独立 topology。
26. Deck 不是 Product、Release、Installation 或 DeploymentScope 的身份别名；当前不建立无 owner 的 application_id 或 ApplicationInstance。
27. 未来 Application 不得创建第二个 reconcile owner；DeploymentController 继续独占 committed DeploymentPlan/Revision 与 Runtime apply 写权。
28. 应用私有、跨 DeckRun 的持久状态在首版明确 unsupported；不能藏入 Card 全局状态或仅因长期运行就升级为平台 CoreService。
29. CoreService 的平台作用域由共享范围、权威和生命周期决定，不能由某个 Deck 的引用关系推导；DeploymentController deactivate/replace Deck workload 并使 DeckRun terminal，不停止其平台依赖。
30. DeckTopology 是受 DeckLock digest 覆盖的 directed-multigraph declaration，不是通用 Graph Engine 的输入或 Runtime 真相；DataLink、ServiceDependency 与 activation constraint 不合并。
31. 没有显式 feedback contract 时 cyclic Deck 在任何 Runtime 副作用前失败；不回退声明顺序、不自动插 buffer。
32. CardDefinition/Card 不拥有或复制 ToolDefinition；Artifact 可以导出二者，但它们是 sibling contracts。
33. 一个 Tool Provider 可以提供多个 Tool，一个 Tool 可以有多个 Provider；同一 InvocationAttempt 只能固定一个 concrete Provider、Artifact 与运行代次。
34. ToolView 可见性不等于授权，Provider 的 ToolResult 与 client journal 的 InvocationResult 都不等于外部或物理 EffectReceipt。
35. Tool Provider host lifecycle、配置、Probe/Inspection 与业务 Invocation 不共用一个字符串 action 平面。
36. 首个 Tool slice 使用 ToolAdmissionDecision 与 committed desired ToolBinding，并以 authenticated runtime readiness facts 纯投影 immutable ToolCatalogSnapshot 中的 resolved provider entry；projection 不另选 Provider，不把 observed facts 写回 DeploymentPlan，也不按 MCP server name、裸 Tool 名、Plugin 前缀或注册顺序动态选路。
37. CardDefinition/Card 没有 `run()`；CardInstance 只能由 RuntimeHost 根据 canonical RuntimeApplyRequest 创建，internal Harness 不成为 production source。
38. 语言内 workload unit profile、Runtime component Harness 与 one-subject Deck system smoke 是不同证据层；前一层成功不能自动升级为后一层 Artifact compatibility、readiness 或 production 等价声明。
39. startup、liveness、readiness、health 与 test observation 分型；实现可贡献 fenced semantic evidence，但不能写自己的生命周期终态、Ready 终态或 recovery action。
40. 通用 L0–L3 Card Harness 强制 effect-denied 且无切换开关；有物理副作用的检查必须进入独立 L4/H1，并受 Authority/Lease/Safety/Enforcement/Hardware activation/Receipt 约束。

## 13. 第一阶段边界

首版只需要形成：

1. CardDefinition 的最小字段、不可变 In/Out PortSpec、ExecutionRequirements（含 run-bound/provenance）和 language-neutral entrypoint；可信 Rust in-process 实现只经内部 registry/static linkage 接入，Python reference workload 经版本化 ProcessDomain 协议接入，不建立公共 Factory/Handler 体系、嵌入解释器默认路径或 Rust `dylib` ABI。
2. DeckSpec 的 Card、静态 1:1 Signal/Event Link、P3 受限的 1:1 CommandEndpoint Link、DeliveryProfile（含 workload envelope/max inflight）、Requirement 和 CardProfile 引用；复杂 fan-in/fan-out、Call、通用/多目标 Operation、Tool 和动态 binding 首版 fail-fast。
3. DeckParser、DeckValidator 与纯 DeckCompiler。
4. 可重复的 DeckResolver 与 DeckLock。
5. DeckCompiler 产生确定性、内嵌 canonical DeckTopology 的 DeckLock，保留 parallel Link 并报告稳定 SCC/cycle witness；没有显式 feedback contract 时拒绝 cyclic Deck。DeploymentPlanner 只将该 DeckLock 与 ServiceSpec、DeploymentProfile、policy、stable-ID allocation snapshot 和不可变目标 Node facts 编译为 `DeploymentPlanCandidate`，不接收可独立漂移的 topology 输入，并把 typed activation/readiness/consumer-ingress/producer-egress/dependency-loss/drain contract 写入 execution。其中 PlanContent 只包含候选 bindings/execution 等 desired content；allocation delta、diagnostics 与 PlanContentDigest 是 candidate 内与 PlanContent 同级的字段，三者都不进入 PlanContentDigest 所覆盖的 content。Planner 相同输入必须产生相同 candidate/digest，不持久化、不分发、不管理 Runtime。
6. DeploymentController 作为每 DeploymentScope 单写 owner，在一个 crash-consistent transaction 中原子提交 allocation delta、下一 DeploymentRevision 与 committed DeploymentPlan。纯 RuntimeSliceProjector 从 committed plan + target 生成 tenure-neutral Slice；纯 RuntimeApplyEnvelopeBuilder 再把 `DeploymentWriterRef/DeploymentWriterEpoch/WriterTenureProof` 映射为 runtime-owned `PlanWriterContext` 并绑定 CAS/request auth。DeploymentController restart 不改变 plan revision、source digest 或 slice digest；Runtime 不 import deployment/decks。
7. RuntimeHost 内部 RuntimeAssemblyEngine 从 canonical RuntimePlanSlice 创建 Domain/CardInstance/Mailbox/inactive PortBinding，按 readiness/activation gate 开放 consumer ingress 与 producer egress，并执行 drain/rollback；它不进入 steady Message hot path。RuntimeHost 产生 DeckRun Receipt。
8. Inspection 能解释 desired Domain/Mailbox/dispatch 与 observed CardInstance、PID/TID/loop/epoch 的差异。
9. P2 使用不依赖 Zenoh 的 PortBinding test fixture 验证 CardInstance-scoped binding 与 target Mailbox；P4 安装 Zenoh `session-local`、`host-local`、`remote` route 实现并复用同一 conformance suite，其中 P4 实测同 session/同主机，P5 才以双主机验证 remote。
10. 不创建 Application DTO、ApplicationController、ApplicationService、application store 或 `applications/` 空包；首个产品应用以一个完整 Deck reference profile 验证现有边界。
11. reference Card 只有 CardInstance-scoped ephemeral workspace；只有 Schema 显式声明 `owner=application`、`lifetime=installation` 或未来等价字段时，Compiler 才能以稳定 reason code 结构化拒绝。opaque code 的业务意图无法由 Compiler 推断，但未声明持久文件、数据库和 egress 在受限 ProcessDomain 中拒绝并记录 fact。Loop/ThreadDomain 不承载不可信 Card。
12. P2b 建立 internal single-subject Card component Harness：只消费 production projector/builder 生成的 canonical request，由 RuntimeHost 创建 CardInstance，只验证 callback/Domain seam，并在退出后验证所有 Task/Thread/Process/Mailbox/FD/SHM/workspace 已回收；不声称完整 RuntimeAssemblyEngine 已完成。
13. P2e 补齐 RuntimeAssemblyEngine 并建立显式 one-subject Deck system smoke；首版不发布 StandaloneRunner。任何未来便捷入口都必须可导出 canonical DeckSpec，并在相同 source scope/revision、resolver/target/policy/committed provenance 下证明与显式 Deck 的 DeckLock/Plan/Slice digest 等价；独立 commit 不要求 revision-bound Slice digest 相同。

首版不建设 Marketplace、DeckCatalog、产品级热升级/跨 Node 自动回滚、离线归档、签名发布、插件沙箱或多节点 Release Index。本地 P2 仍必须验证 revision-tagged 整体替换与失败回滚/quarantine，以禁止新旧执行计划混配。

## 14. 验证要求

- 同一 CardDefinition 能在同一 Deck 中创建两张不同配置的 Card。
- 两张 Card 默认创建不同私有实现对象、BindingId 与 PortBinding，不共享 descriptor 或领域状态；各自 BindingEpoch 独立推进，数值可以相同。
- Card 的 Canvas 坐标变化不改变运行语义 digest。
- 缺失 CardDefinition、Port 不兼容、依赖循环和未满足 Service/Permission/Feature Requirement 在启动前失败。
- 同一 Card pair 的多条 Port Link 被保留为 parallel edge；随机输入顺序不改变 SCC/cycle witness 或 DeckLock。没有显式 feedback contract 的 cyclic Deck 在任何 Runtime 副作用前失败。
- `Out[A] → In[B]` 不自动产生 A-before-B 启动顺序；consumer ingress Ready 后才开放 producer egress，service readiness 与 dependency-loss 只来自编译后的 typed activation contract。
- `Out[A] → In[B]` Schema/版本不兼容、required/cardinality 不满足、隐式 converter 或首版未支持的 interaction 在编译阶段失败。
- 相同 DeckSpec 与解析输入生成相同 DeckLock。
- 修改 resolved Card/Port/Link/DeliveryProfile/locked ref 会改变 DeckLock digest；只修改 Canvas View State 不改变它。
- DeploymentPlanner 只接收自包含 DeckLock，不存在独立 DeckTopology 文件、参数或可编辑副本。
- ProcessDomain 中的 CardInstance 崩溃不会创建完整全局 Runtime。
- 相同 DeckSpec/resolver inputs 先生成同一个 byte-identical DeckLock；同一 DeckLock 再配两个 DeploymentProfile/target facts 可以生成不同且可解释的 DomainAssignment，而不改变 DeckLock 或应用消息语义。
- 同一 byte-identical DeckLock 配不同 immutable ServiceSpec inventory/target facts 可以选择不同且可解释的 provider candidate，DeckLock/digest 保持不变。
- CardDefinition、Card 和 Deck 中出现 lane/thread/PID 编排时在验证阶段失败。
- control/high criticality 未经 DeploymentPolicy 授权/容量 reservation，或 arrival/run bound 不可行时，计划在启动前失败。
- Runtime 实际 Domain、PID/TID、loop 或 capacity 与 `DeploymentPlan.execution` 不一致时不能 Ready。
- Domain/Mailbox/Policy 替换时不出现新旧 revision 混配，旧 Invocation 不能越过 revision/epoch 落地。
- DeploymentController deactivate/replace Deck workload 并使旧 DeckRun 进入 terminal state 后，其 Authority、Fabric 或 InspectionService CoreService 仍保持独立生命周期。
- CardInstance 在干净进程和新 ephemeral workspace 中 replacement，不继承未声明的全局或文件状态；原始持久文件、数据库与 egress 在受限 profile 中被拒绝。显式 application-owned durable state 请求返回 `UnsupportedStateLifetime`/`UnsupportedProviderOwnership` 并触发研究，而不是静默写入 Card/CoreService。
- OpsService/TUI 只通过 InspectionProtocol 观察 DeckRun。
- Kernel 和 Runtime 的依赖检查能阻止它们反向导入具体 CardDefinition、Deck、Agent 或 Graph 实现。
- PortBinding test fixture 与 Zenoh `session-local`、`host-local`、`remote` 使用相同 Message/Envelope/Schema/Delivery/Mailbox conformance；`send accepted` 不被报告为领域或物理 effect succeeded。
- route replacement 在任何观测点最多一条 active route 接收新 Message；完成或显式 rollback 后不存在双投、implicit fallback、hash/time-window echo 去重或旧 route 泄漏。
- 未声明端口不能收发；普通 CardInstance 私有实现上下文无法取得原生 Zenoh Session，scope 指向 Fabric resource 的 CapabilityGrant 可审计和撤销。
- Runtime 在不安装/import `decks` 与 `deployment` 包时，仍能从 `RuntimeApplyRequest` fixture 应用 plan slice 并报告 observed facts。
- RuntimeAssemblyEngine 在 prepare/readiness/activate 任一阶段 crash 或收到重复/旧 revision 请求时不覆盖旧 active、不双活 Binding；steady streaming/Command 路径不经过 assembly graph loop。
- Python workload unit profile 同时构造两个私有对象，证明没有 mutable descriptor、类变量、全局 registry、workspace 或窄 handle 串用；该结果只证明语言内领域逻辑，不证明 Artifact/ProcessDomain compatibility，也不标记 CardInstance Ready。Rust/C++ 等语言内单测适用同一证据上限。
- P2b component Harness 以可信 Rust in-process fixture 覆盖 startup failure、callback timeout、late old-generation output 和幂等 teardown；P2d 再以 Python ProcessDomain reference worker 覆盖 protocol version、heartbeat、credit、cancel/kill、process-tree cleanup 与 stale generation；P2e 覆盖完整 prepare/readiness/activate、重复 apply/CAS conflict。test evidence 固定为 local/diagnostic，production trust 不接受 test proof。
- startup/liveness/readiness/health/test observation 的 truth table 拒绝 stale revision/generation/epoch/freshness；同 event loop 的自报不能证明该 loop liveness，endpoint/heartbeat 也不能单独证明 exact-revision Ready。
- 显式 one-subject Deck 的所有 fixture Card 在编译前声明，并走 plan→commit→project→apply→observe→deactivate；若未来出现便捷入口，它在同一 fixed provenance 下与显式 Deck 的 canonical digests 等价且不直写 RuntimeHost，独立 commit 不比较 revision-bound Slice digest。
- 通用 Harness 无真实硬件权限；simulation provenance 不能满足 H1，任何写设备的诊断必须产生 owner-specific Receipt 或 `Uncertain`，不能以 enqueue/callback/HTTP/MCP 成功冒充 effect success。

## 15. 开放问题

- CardDefinition 与各语言私有实现的关联方式，以及 Python/Rust 等 SDK 的最终作者语法；这些 SDK 必须消费同一语言中立合同，不能把 Python direct-object profile、Rust类型布局或公共 `dylib` ABI升级为 Card identity。DeckSpec 只引用已解析 Port 名称，不复制或改写 CardDefinition PortSpec。
- Call/Query、streaming call 与 Operation 的逻辑契约、correlation、取消、反馈和唯一终态 Schema。
- fan-in merge、fan-out routing、partial acceptance 与 `send()` 聚合结果的公共表达。
- Schema compatibility range、显式 Adapter 选择与 `FeatureMismatch`/`FeatureLoss` report 的具体格式。
- ExecutionRequirements 与 DeliveryProfile 的具体字段名和 Schema 版本；unknown 保守准入、arrival/run bound 和 max inflight 语义在 P1 前冻结。
- `DeckArchive` 是否需要成为正式离线交换格式，或只作为 Release 工具的临时输出。
- Canvas View State 单独存储在哪里，以及如何与 DeckSpec 关联而不污染其 digest。
- one-subject 开发入口的最终 CLI 名称、是否需要持久 test manifest，以及开发态临时 DeckLock 的保存/导出策略；公共化前不创建 StandaloneRunner 或第二种 Runtime input。
- 何时出现两个独立消费者，足以准入 public CardHarness 或 readiness/health contribution Schema；在此之前只保留 internal fixture 和精确语义 facts。
- 首个需要多 Deck 聚合、稳定安装 identity 或跨 DeckRun 私有状态的真实产品场景；它将触发 Application/Installation Proposed ADR。
- 跨 Deck 交互的首个真实消费者应使用 ServiceContract、限定 Gateway endpoint 还是未来 Port export；在此之前不允许直接寻址另一 Deck 的内部 Card。
- ToolProviderDeclaration/ToolAdmissionDecision/desired ToolBinding/resolved provider entry/ToolSet 的最终名称、Card/CoreService/Gateway-backed provider reference、action 粒度、stream/cancel/terminal Schema 与 Provider readiness/failover；在专项 ADR 前不创建公共 Tool Registry 或透明自动切换。

这些问题需要后续 Research 与 ADR 冻结；在此之前实现应保持最小、显式和可替换。
