# CardDefinition 输入输出、Port、Link 与运行绑定研究

> 状态：Research Complete，结论为 `revise`
> 日期：2026-07-29
> 深度：Deep
> 评审策略：PhanthyMotus/EAGOS 本地代码审查 + independent challenge；尚待 Proposed ADR 评审
> 范围：CardDefinition 开发模型、In/Out、PortSpec、Deck Link、DeliveryProfile、DeploymentPlan.bindings 与 Runtime PortBinding
> 实现状态：尚未实现；本文是目标架构的决策输入，不是 API 或功能完成声明

> 术语更新（2026-07-28）：本文早期的 capability requirement、capability loss 与 scoped Fabric capability，分别按新基线解释为 Service/Permission/Feature Requirement、FeatureLoss 与 scope 指向 Fabric resource 的 `CapabilityGrant`。DeploymentPlan 仍是唯一 desired truth，RuntimeHost 只消费其 target RuntimePlanSlice；见 [Capability、Service Contract 与 Feature Support](../concepts/capability-service-feature-boundaries.md)和[分布式具身 Agent OS 缺口研究](distributed-embodied-agent-os-gap-analysis.md)。

> 后续裁决（2026-07-28）：[ADR-0001](../adr/ADR-0001-deployment-controller-boundary.md) 将编译链明确为 DeckCompiler → pure DeploymentPlanner → DeploymentPlanCandidate → single-writer DeploymentController atomic commit → tenure-neutral RuntimePlanSlice + writer-context apply；本文中较早的“Compiler 写入 DeploymentPlan”按 candidate→commit 链解释。本研究的 Port/Link/Binding 与 single-active-route 结论不变。

> 后续裁决（2026-07-29）：[ADR-0002](../adr/ADR-0002-card-definition-terminology.md) 保留本研究证明必要的可复用定义层，但移除早期公共领域名。本文按 `CardDefinition → Card → CardInstance` 解释；CardDefinition 是不可变数据，Artifact entrypoint 后的 CardInstance 私有实现对象才执行回调。该裁决替代早期“保留窄定义对象”建议，不改变 Port/Link/Binding 结论。

> 后续裁决（2026-07-29）：[ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 已接受 Rust-first mechanisms + polyglot workloads。本文所有 CardDefinition、Port、Message 与 entrypoint 均按语言中立合同解释；可信同构建 Rust 实现只经内部 registry/static linkage 进入 in-process profile，Python/C++/未知或不可信 Artifact 默认经版本化 ProcessDomain 协议运行，不建立公共 Rust `dylib` ABI。早期 Python 示例只保留为 workload SDK 作者体验证据。

## 一句话结论

ParaEGOX 应使用不可变 `CardDefinition` 表达可复用能力合同，并保留 `In`/`Out` 作为最直观的单向端口作者语法；Kernel 使用 transport-neutral 的 `PortSpec(direction=...)`，Deck `Link` 表达本次连接与交付意图。CardDefinition 不是业务基类，CardInstance 私有实现对象才接收运行句柄和执行回调。DeckCompiler 解析工作负载拓扑并产生内嵌 canonical DeckTopology 的 DeckLock，DeploymentPlanner 生成含 binding assignment 的 DeploymentPlanCandidate，DeploymentController 原子提交唯一权威 DeploymentPlan；RuntimeHost 只消费 target RuntimePlanSlice，并为每个 CardInstance 安装 `PortBinding`。生产 `PortBinding` 在任一时刻只激活一条 Zenoh 路由，按 `session-local`、`host-local` 或 `remote` locality 到达目标 `Mailbox`。不得迁移 EAGOS 的可变 Input/Output 描述符，也不得把 Call、Operation、Tool、配置和生命周期强行伪装成普通 In/Out 数据流。

## 1. 研究问题与成功标准

本研究回答：

1. ParaEGOX 保留 CardDefinition 后，是否还需要显式输入和输出。
2. EAGOS `Input`/`Output` 中哪些需求成立，哪些耦合必须删除。
3. Port、Link、Mailbox、Zenoh key、QoS 和运行指标分别由谁拥有。
4. 普通开发者是否需要理解独立的 Handler 与 Factory。
5. In/Out 能否覆盖 ASR、TTS、导航操作、Tool 和权威状态等不同交互。
6. 同一契约如何在单进程、跨进程和跨 Node 部署中保持语义一致。

成功标准是：

- CardDefinition 作者能直接看出能力接收和产生什么数据。
- Deck 在启动前检查方向、Schema、cardinality、连接完整性和交付可行性。
- CardDefinition 与 Card 不写 Zenoh key、Publisher、队列、线程或 PID。
- 同一 CardDefinition 的多个 CardInstance 拥有独立绑定和运行身份。
- Zenoh 的 `session-local`、`host-local` 与 `remote` locality 只改变编译后的路由事实，不改变应用契约；确定性测试 fixture 也通过同一 PortBinding/Mailbox 契约。
- source accepted、encoded frame staged、Message/target Mailbox 准入、领域执行和物理效果不会被压缩成一个虚假“成功”。
- 不为首版尚未支持的动态拓扑、双向流和复杂操作留下 silent fallback。

## 2. 范围、假设与非目标

### 2.1 范围与假设

- ParaEGOX 当前处于 clean-slate 文档阶段，尚无必须兼容的公共 Port API。
- `CardDefinition` 是不可变的可复用能力定义；`Card` 是一次配置使用；`CardInstance` 是运行身份。
- Zenoh 是唯一生产 Fabric，覆盖同 session、同 host 与远端通信；Kernel 和 CardDefinition 不依赖 Zenoh 类型。
- 非 Zenoh 的确定性路径只作为 `PortBinding test fixture` 存在，不成为生产类型、第二套 Bus 或部署选项；`Memory` 保留给领域/平台能力，不命名 `MemoryPortBinding`、`MemoryBus`。
- DeckSpec/DeckTopology 需要支持静态验证、依赖锁定、跨进程和跨 Node 编译。
- Rust 核心工程以 Cargo workspace、`Cargo.lock` 与 pinned toolchain 为权威；Python SDK、worker、测试辅助和当前治理工具继续使用 `uv`，两者通过跨语言 contract suite 汇合。

### 2.2 非目标

- 不冻结 Rust/Python/C++ 等语言 SDK、descriptor、decorator、macro、trait 或 YAML 的最终字段名。
- 不在本文设计完整 RPC、Action、Tool 或 State API。
- 不承诺端到端 exactly-once、全局顺序或透明重放。
- 不迁移 EAGOS `Input`/`Output`、Bus、Lane、Module 或 Bundle 实现。
- 不允许文档中的伪代码被误解为已经可导入的 SDK。
- 不把实现语言定义成 Card/CoreService 身份，不建立公共 Rust `dylib`/trait-object Card ABI，也不以嵌入 CPython 代替 ProcessDomain。

## 3. 证据与强度

### 3.1 PhanthyMotus

| 证据 | 强度 | 含义 |
| --- | --- | --- |
| MCP `tools/list` 提供 `topic_in`、`topic_out`、`format`、配置 Schema 与多实例元数据 | 高 | Motus 已经需要能力定义层和方向端口，只是契约较松散 |
| Canvas Card 保存 `mcpId`、`toolName`、输入输出 Topic 和连接；类型匹配主要比较 `format` 字符串 | 高 | In/Out 对 Canvas 很直观，但字符串相等不足以成为长期 Schema 兼容规则 |
| 多实例 processor 的 Topic 会在连线或启动后动态推导，并写回 Card/UI 状态 | 高 | Topic 是运行绑定事实，不应成为 CardDefinition Port 的稳定身份 |
| Canvas 使用 BFS 传播 Topic，运行布局与类型、绑定和 UI 状态仍有混合 | 中高 | ParaEGOX 需要新建独立 DeckSpec、编译计划和运行观测，不能把 Deck 误写成 Motus 已有 Schema，也不能让 Canvas 计算生产 binding |
| 当前整体组合使用 `CanvasLayout` 与 Project 启停状态；代码中不存在 Deck 类型或 Deck Schema | 高 | DeckSpec 是 ParaEGOX 的形式化演进，不是 Motus 现状声明 |
| `PerceptionBundle` 在一个 MCP endpoint 后聚合多个 Plugin/Tool，Canvas 又以 Tool 数量启发式判断 bundle | 高 | Bundle 是实现聚合且边界不稳，不应升级为 ParaEGOX 应用、发行或部署总概念 |

相关本地实现位于 `phanthymotus/agent-core/src/api/mcp_manage.py`、`agent-core/web/js/canvas.js`、`perception/plugins/asr.py` 与 `perception/plugins/tts.py`。这些路径只作为现有行为证据。

### 3.2 EAGOS

| 证据 | 强度 | 含义 |
| --- | --- | --- |
| EAGOS Module 类级 `Input`/`Output` 支持方向反射，启动时把输入绑定到对应回调、把输出绑定到 publisher | 高 | 显式方向显著改善作者体验、拓扑发现和启动前检查 |
| 描述符同时保存 type/schema、Topic、Zenoh QoS、buffer、freshness、Publisher、EAGOS Module 引用和运行计数 | 高 | 声明、交付、传输和 observed state 混入一个可变对象 |
| `Input.connect(Output)` 保存进程内对象引用，真实 Bundle 又使用 Topic/key 映射 | 高 | 进程内对象图不能成为跨进程或离线编译的拓扑真相 |
| Input 的类型参与 decode，而 Output 类型主要作为元数据，publish 路径没有对称验证 | 高 | 生产者与消费者必须使用同一 Schema 兼容规则 |
| ring/fifo、阻塞和 wire pressure 的声明与多层 queue/pump 行为并不总是一致 | 高 | DeliveryProfile 和 Mailbox 必须有独立 owner 与 conformance test |
| 同一描述符实例化时被复制并注入运行状态 | 中高 | CardDefinition 必须不可变；每个 CardInstance 的绑定必须独立创建 |

以上结论来自相邻私有工作区的只读行为审查。ParaEGOX 只抽取中立需求，不在公开文档中记录其文件路径、代码片段、配置、测试或注释，也不把它作为实现规范。

### 3.3 ParaEGOX 当前约束

ParaEGOX 已经选择 DeckLock、DeploymentPlan、bounded Mailbox、ExecutionDomain、PortBinding 与 Zenoh-native Fabric。若 CardDefinition 直接持有 Topic、Publisher 或 queue，会同时破坏：

- 同一 CardDefinition 多 Card 复用；
- Zenoh route locality 与部署位置的可替换性；
- `DeploymentPlan.execution` 的唯一 desired truth；
- Mailbox 和 outstanding budget 的单 owner；
- 最小权限 PortBinding；
- DeckLock 的可重复解析。

### 3.4 证据发布边界

PhanthyMotus 是 ParaEGOX 的公开、许可代码血缘，因此可以在仓库中保留公开路径和许可证归属。EAGOS 只作为受限的工程经验输入：公开文档记录“观察到的失败模式 → 中立需求 → ParaEGOX 独立裁决”，不记录私有源码路径、逐段映射、复制性伪代码或可还原内部实现的细节。若未来确有法务或 provenance 审计需求，应在访问受控、与公开仓库分离的记录中保存 reviewer、时间和证据摘要，而不是把私有材料提交到 ParaEGOX。

## 4. 方案比较

### 4.1 方案 A：没有 Port，Card 实现直接使用 Bus/Zenoh

优点是原型代码少，动态 Topic 灵活。代价是 Deck 无法可靠检查 source/target、Schema、悬空依赖、fan-out、权限和跨进程绑定；多实例会重新依赖字符串命名规则。ParaEGOX 会退化为 Zenoh 的薄封装。

**结论：拒绝作为普通 Card 默认模型。** 原生 Zenoh Session 只属于 Fabric 实现；其他主体的动态消息需求必须通过 scope 指向 Fabric resource 的显式、可审计 `CapabilityGrant` 获得。

### 4.2 方案 B：迁移 EAGOS Input/Output 描述符

优点是作者 API 成熟，方向反射和 `on_<input>` 约定直观。代价是把 Topic、Zenoh、QoS、buffer、运行 publisher、指标和 EAGOS Module 对象再次绑定，复制已经观察到的多层排队、实例状态污染和跨进程困难。

**结论：拒绝实现，仅保留行为需求。**

### 4.3 方案 C：只公开通用 `Port(direction=...)`

它能让 Kernel 只有一套模型，也便于 compiler 统一处理；但开发者在 ASR/TTS 等数据流代码中每次阅读方向字段，作者体验弱于 `In[T]`/`Out[T]`，Canvas 也仍需要投影成输入和输出。

**结论：适合作为 Kernel Schema，不单独作为开发者 API。**

### 4.4 方案 D：PortSpec 内核模型 + In/Out 作者语法 + Link + 编译绑定

```text
SDK In[T] / Out[T]
          │ 生成或声明
          ▼
CardDefinition.ports: PortSpec(direction, schema, interaction, constraints)
          │ Card 引用，Deck Link 连接
          ▼
DeckCompiler → DeckLock {canonical DeckTopology}
          │ + target facts + DeploymentProfile
          ▼
DeploymentPlanner → DeploymentPlanCandidate
          │ DeploymentController atomic commit
          ▼
committed DeploymentPlan {bindings + execution}
          │ RuntimeSliceProjector + ApplyEnvelopeBuilder
          ▼
RuntimeApplyRequest {RuntimePlanSlice + writer context + CAS controls}
          │ RuntimeHost verify/apply
          ▼
CardInstance-scoped PortBinding
          │ exactly one active production route
          ▼
Zenoh {session-local | host-local | remote}
          │ bounded Fabric ingress + validation
          ▼
validated Message → target Mailbox
```

这同时保留开发者可读性、静态验证、传输解耦和运行时单 owner。

**结论：推荐。**

### 4.5 方案 E：所有交互都用几条 In/Out Link 表达

ASR 的 `audio In → transcript Out` 是自然数据流，但并发 TTS 还需要 request id、音频 chunk 关联、取消、部分结果和唯一终态；导航操作还需要 goal、feedback、cancel、result 与 Receipt；Tool 需要发现、授权、超时和流式返回。把它们手工拆成几条互不相关的 Link 会丢失逻辑原子性。

**结论：拒绝。** In/Out 是方向端点，不是所有交互的完整协议。

## 5. 推荐模型

### 5.1 CardDefinition 与私有实现对象不依赖 Handler/Factory 心智模型

一个发布的 `CardDefinition` 本身就是不可变合同，包含：

- ID、契约版本、Port、配置 Schema、Service/Permission/Feature Requirement 和 ExecutionRequirements。
- 指向 Artifact export/entrypoint 的 language-neutral 引用；代码、模型和二进制仍属于 Artifact。

entrypoint 由 Artifact runtime kind 解释，不形成语言身份：可信、与 RuntimeHost 同构建同发布的 Rust 实现只经内部 registry/static linkage 接入 Loop/ThreadDomain；Python、C++、未知或不可信 Artifact 默认由版本化 ProcessDomain worker 构造语言内私有实现。直接构造只属于各语言 workload unit profile，不是 production 装载证据；`Factory` 不成为普通作者必须理解的公共概念，输入回调只是私有实现的方法，`Handler` 不建立为独立领域实体。

每个 CardInstance 默认拥有一个私有 Card 实现对象。实现对象可以持有 ASR 模型句柄、解码状态和领域缓存，也可以实现 `on_start`、`on_audio`、`on_stop` 等回调；但 CardInstance/RuntimeHost 才拥有并推进生命周期、判活、恢复、超时、取消和运行身份。回调参与生命周期不等于拥有生命周期。

共享模型、设备、native singleton 或跨 Card 状态必须通过 CoreService、ResourceClaim 或其他显式 owner 建模，不能藏在类变量、全局 registry 或 descriptor 中。

### 5.2 CardDefinition 准入条件与反例

使用 CardDefinition 不等于所有代码都要定义成 Card。一个能力只有在同时满足以下大部分条件时才值得拥有 CardDefinition：

- 它是可被多个 Deck 复用和独立版本化的领域能力；
- 它有稳定的配置、Port/interaction 或 Service/Permission/Feature 需求；
- 同一类型可能在一个 Deck 中形成多个隔离的 CardInstance；
- 它的私有状态可以随 CardInstance 创建、排空、销毁或重建；
- Deployment 需要根据其 ExecutionRequirements 决定 placement 或 isolation。

反例：无状态 helper 属于普通 library/function；跨 Deck 共享、长期持有权威状态的能力属于 CoreService；硬件或外部协议边界属于 Driver/Gateway；应用连接关系属于 Deck；算法内部步骤若不需要独立配置、复用、隔离或观测，不应为了 Canvas 颗粒度强行定义 CardDefinition。每增加一个 CardDefinition 都会引入版本、Schema、Artifact、CardInstance、binding、liveness/recovery 和文档成本，粒度过细会让拓扑和运维复杂度超过复用收益。

### 5.3 作者侧示意

以下只是候选 Python workload SDK 的目标心智模型，不是冻结 API、Python-first 声明或 Runtime 装载协议：

```python
class ASR:
    audio = In[AudioFrame]()
    transcript = Out[Transcript]()

    async def on_audio(self, frame: AudioFrame) -> None:
        result = await self.model.transcribe(frame)
        await self.transcript.send(result)
```

`In[T]` 与 `Out[T]` 是 `PortSpec(direction=IN|OUT, schema=T)` 的一种语言 SDK 语法糖。上例只表示 Python 语言内实现类，不表示继承 CardDefinition、定义 Card 身份或承诺嵌入 CPython；Rust/C++ binding 必须生成/消费相同语言中立合同。定义中的声明保持不可变，运行实例访问得到 CardInstance-scoped 的窄输出句柄，而不是保存 Zenoh Publisher 的共享 descriptor。

变量名无需强制重复 `_in`/`_out` 后缀。`audio = In[...]` 已经表达方向；领域名称应优先稳定。

### 5.4 权威字段归属

| 层 | 拥有 | 明确不拥有 |
| --- | --- | --- |
| `CardDefinition.PortSpec` | name、direction、SchemaId/版本约束、支持的 interaction、required/optional、cardinality、实现不可削弱的硬约束 | Topic/key、Publisher、queue、Zenoh QoS、PID、线程、运行指标 |
| `Card` / `CardProfile` | 本次配置、有限资源请求和只可加强的约束 | 改写 direction、Schema 或 CardDefinition 硬限制 |
| `Deck Link` / `DeliveryProfile` | from/to、应用消息种类、deadline/freshness、ordering、overflow、ack need、rate/burst/payload/max_inflight、routing/merge 意图 | Zenoh 参数、具体 mailbox、codec、线程和 placement |
| `DeckLock` | 精确 CardDefinition/Artifact/Schema/adapter 解析结果和 digest | 运行 binding、queue 和 observed state |
| `DeploymentPlan.bindings` | binding identity、endpoint identity、Zenoh route/locality、Schema/codec、Fabric ingress limits、目标 admission boundary、keyspace、安装规范、FeatureMismatch/FeatureLoss | BindingEpoch、observed session、实时 queue depth 和 effect 结果 |
| `DeploymentPlan.execution` | MailboxSpec、DomainAssignment、dispatch、budget、liveness/failure-containment/recovery | observed PID/TID/session/epoch |
| Runtime `PortBinding` / Inspection | BindingId 作用域内的实际 endpoint、active route、session、BindingEpoch、queue/age/drop/reject/latency 与 desired/observed 差异 | 静默改写 CardDefinition、Deck 或 DeploymentPlan |

不新建与 DeploymentPlan 并列的顶层 `BindingPlan`。Binding 是 DeploymentPlan 的一部分，避免 placement、revision、transport 和执行计划形成两份 desired truth。

## 6. Port 与交互种类

方向和交互种类彼此正交：

- `direction` 回答数据相对该 Card 能力边界是进入还是离开。
- `interaction_kind` 回答这条逻辑交互如何关联、取消和完成。

首版可稳定支持的单向类型包括 `Signal` 与 `Event`。`Command` 与 `Receipt` 也可以沿方向端口传递，但物理或软件副作用必须保持 Authority、deadline、idempotency、effect owner 和阶段 Receipt。

下列交互不能由几条无关联的普通 Link 冒充：

- `Call/Query`：request/reply correlation、deadline、取消和结果版本；
- streaming call：一个请求对应多个 chunk、部分结果和唯一终态；
- `Operation`：goal、feedback、cancel、result/Receipt；
- Tool：发现、授权、参数 Schema、调用关联和可能的流式结果；
- authority-owned State：snapshot、revision、watch、一致性与写 owner。

后续可以把 Call/Operation 编译为多条物理通道，但在 Deck 和 Receipt 中必须保持一个逻辑 interaction identity。Tool 是上层 Service/Permission/call contract，不是新的 transport primitive；配置、生命周期和 CoreService 依赖不通过 In/Out 发送。

## 7. Schema、cardinality 与路由

CardDefinition 版本不能代替消息 Schema 版本。Port 至少需要：

- 稳定 `SchemaId`；
- producer schema version/content hash；
- consumer accepted range 与兼容性结果；
- 需要时的 payload/encoding 约束。

DeckLock 内嵌并以 digest 覆盖 canonical DeckTopology，同时固定选择的 CardDefinition、Artifact、Schema 和显式 Adapter；`DeploymentPlan.bindings` 固定实际 codec 与 FeatureMismatch/FeatureLoss。生产路径统一经过 Zenoh。只有目标硬件上的 p99.9 延迟、CPU 与 copy profile 证明 Zenoh `session-local` 不足时，才可通过独立 ADR 引入同进程 route；该 route 必须与同一 BindingId 的 Zenoh route 互斥，并通过相同 Message/Envelope/Schema/Mailbox conformance，不能传递任何语言私有可变对象、`Arc<Mutex<_>>` 别名、裸指针或 Runtime handle。转换必须由显式 Adapter/Card/Gateway 提供，Compiler 不静默猜测。

方向不等于连接数量。PortSpec 必须声明可接受的 cardinality，Deck 对以下行为显式建模并在启动前验证：

- 一个 In 是否允许零、一个或多个上游；
- 多上游时的 merge、ordering 和冲突规则；
- 一个 Out 是 single-target、broadcast、select 还是 partition；
- fan-out 部分下游拒绝时的 partial acceptance；
- Command 默认是否只有一个 effect owner。

首版不必实现全部模式，但未支持的 fan-in/fan-out 必须 fail-fast，不能退化为偶然顺序。

首版静态 1:1 模型中，每条解析后的 Link 对应一个稳定 BindingId，Runtime 每次 install/reinstall/reconfigure/revoke 该逻辑 binding 时推进其 BindingEpoch。在同一 DeploymentRevision 和活动 BindingEpoch 下，一个 BindingId 恰有一条接收新 Message/frame 的 active route；禁止 `local_and_wire` 双投，也禁止依赖内容哈希或时间窗做 echo 去重。路由切换必须带 revision，按 `prepare → activate → drain → retire` 执行；`activate` 原子切换新准入、旧 route 只 drain，失败时显式 rollback，不能隐式 fallback 或双写。

未来 fan-out 必须为每个目标生成独立 BindingId；父 interaction 可以聚合 per-destination 结果，但不能让多个目标共享一条活动路由、一个 epoch 或模糊的共同成败。

## 8. 交付、背压与结果阶段

PortSpec 只声明实现硬约束；每条 Link 的 DeliveryProfile 声明本次使用的交付意图；DeploymentPlanner 根据所有上下游约束、immutable target facts 和预算生成候选 Mailbox/binding assignment，DeploymentController commit 后才成为权威计划。`Message` 是 transport-neutral 的不可变逻辑 Envelope：发送侧只从通过 Schema/Port 校验的 payload 构造，接收侧只在 decode 与 Schema/principal/binding 准入成功后构造；它携带 MessageId、causality、deadline 与 trace context。`messaging` 是实现这一消息平面的子系统名；`Mailbox` 是目标异步边界唯一的有界 Message backlog/admission owner，名称不改，也不属于任何语言 SDK 的 In descriptor、Fabric 或每次调用。`PortBinding` 只负责把一个已编译 Link 的 Message 交给目标 Mailbox，不拥有第二份语义队列。

接收侧在完整 decode/validation 前拿到的 bytes/SHM reference 只是 Fabric 私有 encoded ingress frame，不是 Message，也不能进入 target Mailbox。它只能短暂停留在 items/bytes/age/retained-byte 有界且可观测的 Fabric ingress buffer；验证成功后才构造 Message 并 offer 到 target Mailbox，失败记录 ingress rejection。该 buffer 是 transport staging，不是第二个 Mailbox，不能产生应用 accepted。确定性 PortBinding test fixture 直接使用已验证 Message；wire malformed/validator 故障属于 P4 Fabric conformance。

同一 Out 的不同 Link 可以有不同交付策略。把 QoS 固定在 Output 上会使一个慢消费者决定所有消费者行为，也无法表达 per-link deadline、drop 或 durable handoff。

`send()` 或 `enqueue()` 的结果必须分阶段：

```text
source Message accepted
    ≠ fabric egress accepted
    ≠ encoded frame staged in Fabric ingress buffer
    ≠ Message validated and admitted to target Mailbox
    ≠ CardInstance invocation accepted/completed
    ≠ physical effect succeeded
```

结构化 `SendResult`/`EnqueueResult` 描述当前 owner 的 handoff 结果；领域 `Receipt` 描述有权判定的执行阶段。fan-out 需要 per-destination 结果或明确聚合规则。Zenoh publish、encoded frame staged、target Mailbox admitted 与领域执行必须是不同 stage，进程 crash 或断连后的未知副作用不能被伪造为失败，也不能透明 replay。

## 9. Runtime 与权限边界

```text
CardInstance-scoped Out handle
      │ send / enqueue
      ▼
compiled PortBinding
      │ exactly one active route
      ▼
Zenoh {session-local | host-local | remote}
      │ fixed-cost callback handoff
      ▼
bounded Fabric ingress buffer (encoded frame; not Mailbox)
      │ decode / schema / principal / binding admission
      ▼
validated Message
      │ semantic admission
      ▼
bounded target Mailbox
      │
      ▼
ExecutionDomain
      │
      ▼
CardInstance private implementation callback
```

确定性单元/属性测试可以把 `PortBinding test fixture` 接到相同 Mailbox 契约，但它不进入 DeploymentPlan 的生产 transport 选项，也不获得公共产品名。普通 CardInstance 的私有实现上下文默认只获得为 CardDefinition 声明端口编译的窄 handle，不获得 raw Fabric 或原生 Zenoh Session。权限不能按“Card/CoreService/Driver/Gateway”名词自动发放：只有 Fabric 实现拥有原生 Session；其他动态订阅、Recorder 或协议适配需求必须获得限定 key/schema/operation/rate/scope、指向 Fabric resource 的 `CapabilityGrant`，并进入审计与撤销链。不能通过把普通代码改名为 Gateway 绕过权限。

scope 指向 Fabric resource 的 CapabilityGrant 是显式逃生口，不是第二套隐形应用拓扑。其流量在 Inspection 中标记为 dynamic/opaque，不能满足 Deck required Port、静态 readiness 或端到端交付证明；任何物理 Command 仍经过完整 Authority、Lease/Fencing、Safety 和 EnforcementPoint。

## 10. 强制不变量

1. `In[T]`/`Out[T]` 只是不可变 `PortSpec(direction=...)` 的作者侧表达；类级声明不保存运行连接。
2. CardDefinition 定义 Port；Card 只能配置、连接和加强约束，不能修改 direction、Schema 或硬限制。
3. PortSpec 不包含 Topic、Publisher、Subscriber、Session、queue、thread、PID、metrics 或运行 binding。
4. Link 拥有本次交付意图；实际 transport、codec、key 和 Mailbox 只存在于 DeploymentPlan 与 Runtime observed state。
5. 每个目标异步边界恰有一个语义 admission owner；queued、inflight、IPC/executor credit 和 retained bytes 分别有界。
6. required/optional、cardinality、routing、merge 和 Schema 兼容在启动前验证；无隐式 converter。
7. 每个 CardInstance 默认独占一个私有 Card 实现对象；共享领域状态和资源必须显式建模。
8. 生命周期回调由私有实现对象提供、由 CardInstance/RuntimeHost 调用和管理；实现代码不得自建无 owner 的线程、进程、event loop 或后台 task。
9. enqueue、fabric handoff、remote admission、Invocation Receipt 和 effect Receipt 不合并成一个 ack。
10. Call/Operation 的 request/reply/cancel/feedback/result 保持 correlation、deadline、epoch 和唯一终态，不能用无关联 Link 拼装。
11. Zenoh 的所有 route locality 与 PortBinding test fixture 使用同一 Message/Envelope/Schema/Mailbox conformance；任何经 ADR 准入的优化也不能绕过契约。
12. 同一 DeploymentRevision 与活动 BindingEpoch 内，每个 BindingId 只有一条 active route；禁止 local+wire 双投、基于内容的 echo 去重和隐式 transport fallback。
13. 原生 Zenoh/raw Fabric 权限按 scope 指向 Fabric resource 的 CapabilityGrant 授予，不按组件类别自动授予。
14. 动态 Card、开放 provider set 或运行时任意 Topic 首版显式拒绝，不能绕过 DeckLock 和 DeploymentRevision。

## 11. 最小实施与验证切片

为避免一开始构造万能 Port，第一切片只实现：

- 静态可枚举的 CardDefinition、Card 和 Link；
- 1:1 typed `In`/`Out`；
- `Signal`/`Event` 单向交互；
- Schema/version compatibility；
- bounded target Mailbox；
- 结构化 `SendResult`/`EnqueueResult`；
- 不依赖 Zenoh 的确定性 `PortBinding test fixture`，只用于内核契约测试；
- 对 fan-in、复杂 fan-out、Call、Operation、Tool 和动态 binding 的 fail-fast。

随后用两个垂直场景扩展，而不是先发明全能协议：

1. 并发 TTS streaming call：验证 request/chunk/cancel/final correlation。
2. 导航 Operation：验证 goal/feedback/cancel/result、Authority 与阶段 Receipt。

首次验证至少包括：

- `Out[A] → In[B]` 不兼容时编译失败；
- 同一 ASR CardDefinition 的两张 Card 获得不同 CardInstance、BindingId 和私有实现对象，不共享运行 descriptor；各自 BindingEpoch 只在自己的 BindingId 内单调，数值允许相同；
- PortBinding test fixture 与 Zenoh `session-local`、`host-local`、`remote` 路由通过同一 Port/Delivery/Mailbox conformance；
- 同一 BindingId 切换 route 时不会双 active、双投或依赖内容哈希去重，失败切换可显式 rollback；
- queue 满、过期、关闭和 fan-out 部分拒绝产生可区分结果；
- CardInstance 私有实现上下文无法取得未声明 Port 或原生 Fabric Session；
- Card 实现 callback 卡死由 RuntimeHost 根据 ExecutionDomain、LivenessSpec、FailureContainmentSpec 与 RecoveryPolicy 处置，不污染 PortSpec。

## 12. 风险、反例与失效条件

### 12.1 主要风险

- PortSpec 过早吸收 RPC、Action、Tool 和 State，重新成为万能接口。
- 为了作者 API 简洁，把运行 handle 藏进可变 class descriptor，重现 EAGOS 实例污染。
- DeploymentPlan.bindings 与 execution 分别修改却没有共同 revision，形成新旧计划混配。
- Schema 只比较字符串名称，无法处理版本兼容与显式 adapter。
- raw Fabric 例外按类名授权，最终所有组件都自称 Gateway。
- 为了“本地更快”并行启用 local 与 Zenoh route，造成双投、乱序和无法证明的 echo 去重。
- `.send()` 返回值被误读为领域效果成功。

### 12.2 会推翻或收缩本推荐的证据

- 产品正式收缩为单一进程、单一实例、没有 Deck 编译和静态验证需求的 appliance。
- 实测证明在目标控制路径中，Port/Envelope conformance 带来无法优化且不可接受的延迟，同时直接 transport API 能提供等价的类型、安全、版本和观测证明。
- 真实 workload 主要是动态 open-world Tool routing，静态 Card/Link 几乎没有消费者；此时应收缩静态 Port 范围，而不是让 runtime Topic 绕过契约。

目前没有这些证据。

## 13. ADR 影响与置信度

[ADR-0002](../adr/ADR-0002-card-definition-terminology.md) 已接受并冻结：

1. CardDefinition、CardInstance 私有实现对象与 CardInstance 生命周期边界。
2. CardDefinition 的准入条件，以及 library/CoreService/Driver/Gateway/Deck 不被强行定义成 Card 的反向边界。
3. `PortSpec + In/Out + Link/DeliveryProfile + DeploymentPlan.bindings + PortBinding + target Mailbox` 的唯一 owner 链，以及 Zenoh-only production route 与 single-active-route 不变量。
4. Schema/version、cardinality、SendResult/Receipt 阶段与 Fabric-scoped CapabilityGrant。
5. In/Out 只覆盖单向端点，Call/Operation 保留逻辑完整性的边界。

[ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 进一步冻结 Rust-first mechanism、语言中立合同、可信 Rust in-process 与多语言 ProcessDomain 边界。它不改变上述 Port/Link/Binding owner 链。不在 ADR 中冻结具体语言 SDK/descriptor 语法、Zenoh key 格式、codec、队列实现或完整交互种类集合。

置信度：

- 保留显式方向 Port：高。
- 不迁移 EAGOS 可变描述符：高。
- DeliveryProfile 归 Link、transport 归 Deployment/Runtime：高。
- Handler/Factory 不作为用户核心术语：高。
- P2 数据链从静态 1:1 Signal/Event 开始，P3 仅增加 bounded、无透明 retry 的 1:1 OperationClient/CommandEndpoint：中高，仍需首个 ASR/TTS 与模拟物理 Command 垂直原型验证。
- Call/Operation 的具体公共 Schema：中低，需并发 TTS 与导航场景研究。

## 14. 后续入口

- 应用概念：[CardDefinition、Card 与 Deck](../concepts/card-definition-card-deck.md)
- 总体架构：[Kernel、RuntimeHost 与 Core Services](../architecture/kernel-runtime-core-services.md)
- 消息与 Fabric：[Kernel 消息机制、Fabric、Evidence、Telemetry 与 Security 边界](kernel-messaging-fabric-evidence-security.md)
- 执行模型：[Runtime 执行模型、调度与恢复](execution-model-scheduling-and-recovery.md)
- 实施顺序：[Kernel Foundation 实施计划](../plans/kernel-foundation.md)
- 正式决策入口：[ADR 目录](../adr/README.md)
