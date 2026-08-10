# ADR-0009 — Agent 对话、类型化客户端与 Console 边界

> 状态：Accepted
> 日期：2026-08-03
> 决策者：ParaEGOX workspace user（以 `docs/plans/agent-conversation-baseline-v1.authorization-receipt` 为生效证据）
> 关联文档：`docs/architecture/distributed-system-model.md`、`docs/architecture/kernel-runtime-core-services.md`、`docs/research/web-console-webrtc-webxr-gateway-boundaries.md`

## 一句话结论

ParaEGOX 首个用户可见 Agent 切片采用 Runtime-managed `AgentService`、provider-neutral 的有界 Model 调用机制、独立 Model provider adapter、`AgentConversationProtocol` 与 `AgentConversationClient`；本地 TUI 直接消费该类型化客户端，Web 客户端未来经 `ConsoleGateway` 消费同一语义边界，旧的全能 `ConsoleBridge` 不再作为目标组件。

## 背景

本 ADR 作出决策时，代码已经具备窄 Linux Runtime/Deployment reference、受限 ProcessDomain、Mailbox 与 PortBinding test fixture，但没有一般 CoreService assembly、Zenoh production Fabric、Agent/Model 服务、Agent 对话协议或 TUI。现有 P7 只规划运维 TUI，不能承担 Agent 对话语义。

Web/Console 边界研究已经指出：把身份、Schema、投影、权限、缓存、操作转换和底层连接塞入一个 Bridge 会形成新的全局 Runtime/BFF owner。另一方面，如果本地 TUI 直接取得 raw Zenoh Session、RuntimeHost 私有对象或模型凭据，只是把旧 Bridge 隐藏进客户端。

用户明确授权实施紧接本 ADR 前给出的完整开发计划。本 ADR 只冻结该计划中首个本地对话切片所需的 owner、依赖方向和非目标；后续分布式、运维和 Web 能力仍须分别取得实现证据。

### 当前实现快照（2026-08-08，r29 Textual replacement closeout 已通过）

当前工作树已经实现并本地验证本 ADR 的 deterministic DeveloperLocal 首聊切片：

- committed Fabric→Agent successor、真实 Authority/Deployment/Runtime apply 链；
- Runtime-managed 唯一 Zenoh Session、两条有界 PXAC binding 与 opaque typed client；
- durable `AgentService` Session/Turn journal、duplicate/conflict/cancel/watch/terminal 语义；
- 内部 Python Textual child 与一条命令 `paraegox-local` composition；Textual 是唯一当前展示路径，
  旧 Rust reference frontend 已在 replacement gate 通过后的本退役批删除；
- 同一 state root 双启动、同 Session 两 Turn、第二次启动新 Turn、旧 handle fencing、joined shutdown 和 UDS/TCP 释放证据。

后续同一工作树已经增加显式 `developer-openai-v1`：committed desired state 精确绑定
`Provisioned` provider/config/opaque Secret ref，Runtime 通过 provider-neutral resolver 在首次启动和
恢复时重建 adapter；fixture 与 OpenAI profile 不互相 fallback。同一 composition 还把当前展示层迁移为
独立 Python Textual child，并给它两个不同的 owner-private bootstrap 路径：PXAI/PXAB 只承载
Runtime-issued Agent typed client，PXIB v2 + PXIQ/PXIP v2 只承载本地只读 Inspection typed client。
Inspection client 在 App 启动前只执行一次无重试 Latest，严格关联 response 并解码完整 PXIS v2；
失败则 UI 不启动，成功则只显示三行只读 startup status。它没有 Watch、retry、cache、
background refresh、持续监控、action、Ops 或 federation authority。Inspection 端只投影已验证 owner
输出，缺失的 liveness/health 保持 Unknown。argv 不携带 raw identity、Zenoh route、token 或 Secret。自动化只
覆盖 loopback provider；尚未运行或宣称 credentialed external provider smoke，包括当前配置的 DeepSeek 验证路径。

历史 r22 源码快照 `ff2d8109` 的 Ubuntu 证据包括 workspace format、locked metadata 和 locked
all-targets check 通过，Inspection 39/39、非 root DeveloperLocal 89/89 以及非 root Deployment
364/364 通过。该 r22 workspace Clippy 运行暴露了约 30 个历史结构 lint；修正已进入后续源码快照，
但 r29 macOS artifact workflow 没有运行 workspace Clippy，因此本快照不把它冒充为 fresh Clippy
pass。原生 Intel macOS r29 commit `944ce332`、run `31238285076` 已通过 locked Textual tests 和完整
governance checker，构建并验证公开 native CLI，组装可搬移 bundle，并真实走通 PTY 下的 typed
Inspection markers、Runtime ready、Textual→Runtime Echo、priority Ctrl-C、terminal restoration 和
父进程 joined shutdown；bundle checksum、executable mode、archive 和 artifact upload 也已通过。
这项 replacement evidence 允许在本退役批删除旧 Rust reference frontend。macOS bundle 仍明确依赖
宿主 `PATH` 中的外部 Python 3.11+ `python3`，不内置 Python runtime；credentialed external provider
smoke 仍未执行。

上述快照早于本 ADR 后面的统一 Adapter amendment。当前 A1 已完成 stronger integration：OpenAI 与
production deterministic fixture 都由同一个 Runtime provider-neutral resolver 经静态 registry 构造
`ModelServiceV1`，旧 fixture bypass 已移除并取得本地 focused evidence。这个新增事实不改变
credentialed external OpenAI smoke 尚未执行，也不把 embedded mechanism 提升为 production service。

因此当前实现已经超过本 ADR 最初的 deterministic 同进程快照，但仍不是 production profile。
authenticated 双 Node、continuous reconciliation、federated Inspection/Ops 和 ConsoleGateway 仍是后继
工作。当前 Runtime successor 是固定的
Fabric→Agent 两阶段生命周期，不是一般 CoreService DAG/placement engine。

### 2026-08-07 Accepted amendment — 单一 Chat 配置入口与 Adapter 收敛

workspace user 明确否决 provider/model 出现在公开 CLI taxonomy 中，并授权按 ParaEGOX 自身 owner
边界收敛实现。本修订取代本 ADR 早期快照中的 `developer-fixture-v1` / `developer-openai-v1` 公开
命令描述；这些旧名称仅保留为历史实施上下文，不再是当前目标或兼容入口。

当前公开 grammar 固定为：

```text
paraegox chat --config <absolute-paraegox.toml>
```

CLI 只表达对话动作。versioned config 是 state root、Fabric listen、provider、model 与 SecretRef 的
唯一公开权威；不得同时保留 provider 子命令、`--model`/`--state-root`/`--fabric-listen` override 或
任意环境变量配置覆盖。环境只作为一个被配置精确引用的 Secret resolver source，Secret value 不进入
配置、argv、manifest 或 journal。未知 schema/字段/provider、缺失或错误 SecretRef 均 fail closed。

DeepSeek 是当前 credentialed smoke 的可替换验证配置，不是 CLI 名词、默认 provider 或自动选择策略。
`deterministic-echo-v1`、`openai-responses-v1` 与 `deepseek-chat-completions-v1` 都只能由同一配置 schema
显式选择，并继续走同一 composition → Runtime resolver → exact static registry → ModelService 路径；
不存在 fixture bypass、provider discovery、fallback 或每轮重新选模型。

反碎片规则同时把 provider-specific HTTP leaf 收敛到一个 `paraegox-model-adapters` crate。当前 OpenAI
Responses 与 DeepSeek Chat Completions adapter 在该边界内保持各自精确 endpoint、request/response、
Secret lifetime 和 failure mapping；收敛 crate 不合并协议实现、不把 provider 依赖推入
`paraegox-model`，也不表示动态 plugin、路由器或独立 CoreService。若未来某个 adapter 获得独立
deployment/trust/dependency isolation 需求，可按新的准入证据重新拆分，不能按 provider 名机械增 crate。

本修订记录的是公开边界与当前工作树实现方向，不是验证结果。只有 fresh Rust gates 和显式
credentialed external smoke 才能证明相应 provider 可用；DeepSeek smoke 尚未完成时不得声明外部服务
可用或 production ready。

### 2026-08-05 Accepted amendment — Model CoreService 提前准入

workspace user 明确授权：不再要求 Model CoreService 必须先取得独立的真实运行消费者才能建立。
`paraegox-model` 可作为 Accepted 战略 CoreService foundation 在消费路径完整前提前准入，但必须保持
`experimental`/`enabler`，并满足仓库对 owner、非 owner、非 placeholder 机制、focused semantic
evidence、近期集成批次和批次末撤并复审的全部约束。ADR 授权本身不是实现或运行证据。

本修订冻结如下边界：

- `paraegox-model` 的目标是 provider-neutral、有界的 invocation/admission mechanism；它可以拥有
  调用准入、请求边界、deadline/cancellation 传播和 provider-neutral outcome，但不拥有
  AgentSession/Turn journal；
- `paraegox-model-openai` 仍是 provider adapter，只拥有 OpenAI 协议、Secret 使用和 provider-specific
  timeout/error 映射；普通 provider adapter 不适用战略 CoreService 提前准入例外；
- `AgentService` 继续唯一拥有 AgentSession/Turn journal、durable cancel intent 和唯一 terminal
  记录；Model mechanism 接收取消信号或返回 outcome 不会转移这些持久语义；
- 本 ADR 的下一 Model integration batch 必须把 `paraegox-model` 的机制接入 Agent adapter/use path，
  并由 DeveloperLocal composition 产生 focused semantic evidence。即使该路径接通，当前阶段也只
  声明 embedded mechanism，不声明独立共享进程、managed Model desired plan、跨进程合同、router、
  fallback 或 production Model CoreService。

复审点固定在该 Model integration batch closeout：如果届时没有 Agent adapter/use path 与实际语义
证据，必须把 foundation 折回现有 owner、移除或合并。以后若要声明真正独立、共享的 Model
CoreService，必须另行增加 additive managed Model plan，以及 lifecycle、readiness、recovery 和
必要的跨进程 contract；本修订不预先授权这些能力。

### 2026-08-05 Accepted amendment — transport fold-back 与 provider leaf

`paraegox-agent-fabric` 在本地闭环后仍只有 Runtime 一个生产消费者，也没有独立进程、生命周期、
安全或依赖隔离边界，因此按反碎片规则折回 Runtime owner-private `managed_agent_transport`。PXAP
双 lane、descriptor、严格相关性、无重试、取消进度和 mutation disposition 语义继续保留，但不再
作为独立 package 或公共 Rust API。未来只有在第二个独立宿主真实需要复用时才重新提取。

`paraegox-model-openai` 的判断不同：HTTP/TLS、JSON、Secret 与 provider-specific failure 形成真实
依赖和安全隔离，继续作为 ModelBackend leaf adapter；它不得解释 Runtime/Deployment desired
selection。selection 的 exact profile/provider/config/SecretRef 校验属于 composition owner，通用
`paraegox-model` 继续不依赖任何具体 provider。

### 2026-08-05 Accepted amendment — Model Adapter 装配与 Plugin 分期

workspace user 接受先补齐统一 Model Adapter 装配层、再推进 managed Model 与 plugin 准入的三批路线。
本修订借鉴 Adapter/Plugin/Module 分工思想，但不声明兼容或达到任何既有插件系统的能力；ParaEGOX 继续以
自身的 Deployment、RuntimeHost、CoreService 和精确 desired-state 边界为权威。

四个角色固定如下：

| 角色 | 拥有 | 明确不拥有 | 当前状态 |
| --- | --- | --- | --- |
| Model CoreService / `ModelService` mechanism | provider-neutral 的有界调用准入、deadline/cancellation 传播、容量和 provider-neutral outcome | provider 协议、Secret、AgentSession/Turn journal、provider 选择策略 | 只有 embedded、in-process mechanism；不是独立受管服务进程 |
| `ModelAdapter` | 一个精确 provider/protocol implementation 的请求编码、transport、响应校验、Secret 使用和 provider-specific failure 映射；当前 Rust seam 由 `ModelBackendV1` 承载 | Model 调用准入、Agent 语义、Runtime lifecycle、自动路由、fallback | 静态 registry core 与 fixture/OpenAI 统一接线已实现并完成本地 focused validation |
| Model plugin profile | 未来声明 adapter identity/version/capability、非 Secret 配置约束与摘要、SecretRef、网络/沙箱/Artifact/信任要求 | adapter 实现代码、Secret value、运行调用、默认选择或 fallback | 后继 plugin-admission 批次规划，尚未实现 |
| composition | 校验外部 selection 的 exact profile/provider/config/SecretRef binding，把已准入 profile 显式映射到一个编译内 adapter ID，并组装 registry 与 Model mechanism | committed desired state、通用 Service Locator、installer、动态 loader、retry/reconcile owner；也不能声称外部 selection 已绑定 adapter ID/version/capability | 当前仅为 owner-private DeveloperLocal 静态组合 |

第一批只建立 **exact static registration/selection**：在 `paraegox-model` owner 内提供固定、进程内、
编译期链接的 Adapter 注册与精确选择机制；重复 identity 和未知 selection 必须 fail-closed，不允许默认
adapter、自动探测、按内容路由或 fallback。deterministic fixture 与 OpenAI 必须统一经过 exact
provider selection → `RuntimeAgentProviderResolverV1` → composition-owned profile-to-adapter mapping →
`ModelAdapterRegistryV1` → `ModelServiceV1` → AgentService adapter；production fixture 不得在 Runtime
resolver 之外直接构造 provider。Agent 只消费 provider-neutral Model 边界；fixture 继续只作为
DeveloperLocal/测试证据，不冒充 production provider。
`paraegox-model-openai` 的独立 crate 只表示 provider 依赖和 Secret 安全边界，不表示独立 CoreService、
进程或已安装 plugin。

当前工作树已实现本批的 registry core：`ModelAdapterIdV1`、`ModelAdapterMetadataV1`、
`ModelAdapterSelectionV1`、`ModelAdapterFactoryV1` 与 `ModelAdapterRegistryV1`。这些类型只做完整 identity
精确匹配与 factory 构造，拒绝 duplicate、unknown、selection drift、factory rejection 和 built-backend
identity drift；它们的存在不把本批标为完成。只有 fixture 与 OpenAI 均接入同一个 Runtime resolver →
registry → `ModelServiceV1` 路径、production fixture direct bypass 被移除并取得 composition evidence 后，
第一批才可 closeout。

当前工作树已经满足上述 A1 closeout：Model、OpenAI、AgentService adapter、Runtime resolver/live
Fabric/restart/empty、Local selection drift、fixture 双启动对话与 provisioned loopback rebuild/restart
focused evidence 全部通过，Local all-targets locked/offline check、workspace fmt、governance checker 与
locked Cargo metadata 也通过。A1 因而只标为 implemented / local-validated；没有 fresh Ubuntu CI、
credentialed external OpenAI 或 production readiness 声明。

这里的 `ModelAdapterSelectionV1` 是 composition 内构造的 registry-local 值，不是 signed desired-state
contract。当前 `ManagedAgentProviderSelectionV1` 只精确绑定 profile、provider ref、config digest 与
Secret ref，不含 adapter ID、adapter version 或 capability；A1 由 owner-private、编译内 profile mapping
补出 `ModelAdapterIdV1`。因此 A1 证明的是无 fallback 的静态装配，不是端到端 adapter Artifact 身份
准入。后继 contract successor/plugin-admission 批次必须把 adapter identity/version/capability 纳入受
认证的选择或 profile binding，不能把本地映射反向冒充已提交事实。

第一批不创建 `plugins/` 目录、PluginManager、市场或第二个 Model 服务；不加载 Rust `.so`/`.dylib`、
WASM 或远端代码；不增加自动模型路由、fallback/canary、透明 retry；也不增加独立 Model 进程、
跨进程 Model contract 或 Runtime-managed Model desired plan。只有真实第二个 production adapter、
外部 Artifact 或隔离需求出现并通过准入后，才允许扩大 plugin 实现面。

后继第二批才把 Model 提升为真正的 managed CoreService：Deployment committed plan 绑定 Model
provider profile/config/SecretRef selection，RuntimeHost 拥有 lifecycle/readiness/recovery/generation
fencing，AgentService 通过显式 ServiceDependency 消费它。在声明 production managed Model 的 exact
adapter identity 前，必须先有 additive contract successor 绑定 adapter ID/version/capability。后继第三批
才实现 plugin profile 的 Artifact/provenance、能力与 contract 兼容、Secret/网络/沙箱准入及 activation
evidence。两批目前都只是已接受的顺序与边界，不是已经实现的能力。

## 范围与非目标

本 ADR 决定：

- Agent 对话语义 owner、Session v1 生命周期和 Model 边界；
- `AgentConversationProtocol` / `AgentConversationClient` 的角色；
- 本地 TUI、FabricService、AgentService 与未来 ConsoleGateway 的依赖方向；
- 首个可启动、可对话切片的最小能力和完成证据；
- 旧 `ConsoleBridge` 的替代边界。

本 ADR 不决定或授权：

- Tool、Memory、多 Agent coordination、通用 Workflow/Graph Service；
- MCP/A2A/ROS2、WebRTC、WebXR、媒体或遥操作；
- 真实物理 effect、设备、Lease、Safety 或 Hardware Enablement；
- 跨 DeckRun、跨安装或租户级永久会话；
- 完整 OpsService、InspectionService、Web Console 页面或外部公共 transport；
- macOS/Windows production support；
- 多 DeploymentController HA、共识或自动迁移。

## 决策

### 1. 组件与所有权

| 组件 | 拥有 | 不拥有 |
| --- | --- | --- |
| `AgentService` | AgentSession/Turn 的语义状态机、request digest 幂等、会话事件 journal、取消意图与 terminal result | Runtime/进程生命周期、raw Zenoh、模型进程、模型 Secret、Tool/物理权限 |
| `ModelService` | provider-neutral 的有界 invocation/admission、请求边界、deadline/cancellation 传播与 provider-neutral outcome | AgentSession、Turn journal、durable cancel intent、模型 Secret、provider 协议、Runtime restart policy |
| `ModelAdapter`（当前 Rust seam：`ModelBackendV1`） | provider 协议、provider-specific timeout/error 映射与 Secret 使用 | AgentSession、Turn journal、调用准入、TUI 状态、自动路由/fallback 或 Runtime restart policy |
| `AgentConversationProtocol` | 语言中立、版本化、有界的 create/open、submit、get/watch、cancel 语义 | transport session、UI、模型私有请求、Tool 或运维操作 |
| `AgentConversationClient` | 调用协议、关联 request/session/turn、暴露明确 unavailable/uncertain/terminal 结果 | raw Zenoh Session、服务发现真相、重试/reconcile owner、Session journal |
| `FabricService` | 唯一 production Zenoh Session、keyspace、route、binding install/revoke、bounded pre-validation ingress 与 Fabric self-inspection | Agent/Model 语义、Deployment placement、TUI 状态 |
| 本地 TUI | 用户输入、消息展示、连接/错误状态和显式取消 | raw Fabric、RuntimeHost、数据库/journal、模型 Secret、透明 replay |
| `ConsoleGateway` | 未来 Web 的 HTTPS/SSE/WS、外部身份映射、限流和有界 projection/cache | AgentSession、Ops/Inspection truth、Deployment 或 Runtime 生命周期 |

`AgentService` 是 CoreService 角色，不进入 Kernel。其进程、readiness、restart、drain 和 cleanup 由 RuntimeHost 根据 committed plan 管理。逻辑 owner 不要求每个组件立即成为独立 crate 或进程；只有准入记录和真实故障/消费者证据支持时才拆分部署边界。

### 2. AgentSession v1 生命周期

首个 `AgentSession` 必须绑定一个明确 `DeckRunRef`：

- 同一 DeckRun 内，Agent 进程重启后可从 Agent-owned append-only journal 恢复；
- TUI 退出或重启不结束 Session；
- DeckRun terminal 时 Session 必须 seal，并产生可查询的 retention/GC 结果；
- 新 DeckRun 不自动继承旧 Session；
- 跨 DeckRun、安装或租户继续会话需要后继 ADR 冻结稳定 owner、隔离和保留策略。

存储 provider 可以承载 bytes，但不因此成为 AgentSession 语义 owner。Model provider 不能写 AgentSession journal。

### 3. 对话协议 v1

首版至少包含有界、可 canonical encode 的：

- `SessionId`、`TurnId`、`RequestId` 与 DeckRun 关联；
- UTF-8 用户输入、content digest 与 deadline；
- request id + digest 幂等；相同 id 不同 digest fail-closed；
- accepted/terminal failure/terminal success；
- 显式 get/watch/cancel；
- event sequence/cursor 与唯一 terminal event，流式 delta 可以在同一 v1 兼容边界内分阶段接通；
- timeout、disconnect 或 owner crash 后的 `Uncertain`/query 语义，不透明 replay 用户输入或模型调用。

Rust 与 Python 必须通过独立 codec 和 golden vectors 验证相同 canonical bytes、bounds、version 与 unknown-field 拒绝行为。Rust 内存布局、Serde 默认格式或 Python 对象不是跨进程合同。

### 4. TUI 与 Web 路径

本地路径固定为：

```text
Python Textual child
  -> AgentConversationClient
  -> authenticated/no-retry PXAB + PXAI/PXAO private Runtime IPC
  -> Runtime-owned conversation handle
  -> FabricService-owned Zenoh route
  -> AgentConversationProtocol endpoint
  -> AgentService
  -> ModelService
  -> selected ModelAdapter

Python Textual child
  -> separate DeveloperLocalInspectionClientV2
  -> authenticated/no-retry PXIB + PXIQ/PXIP v2 private Inspection IPC
  -> one strict PXIS v2 startup snapshot
```

Model service 在首次启动或恢复时按另一条 owner 明确的 build path 构造，不在每次 invocation 中重新选择：

```text
signed provider selection
  -> RuntimeAgentProviderResolverV1
  -> composition-owned compiled profile-to-adapter mapping
  -> ModelAdapterRegistryV1 exact resolution
  -> selected ModelBackendV1
  -> ModelServiceV1 construction
```

这里的 signed provider selection 尚不携带 adapter ID/version/capability；exact adapter resolution 只发生
在 composition owner 内，不能当作 Deployment 已认证 adapter Artifact identity 的证据。

“TUI 直接使用客户端”不表示 TUI 直接创建 Zenoh Session、拼接 key expression 或实现发现/reconnect。Fabric bootstrap 和 binding owner 必须位于 Runtime/Fabric 边界。

未来 Web 路径为：

```text
Web Console
  -> ConsoleGateway
  -> AgentConversationClient / InspectionClient / OpsClient
```

`ConsoleGateway` 不在首个本地聊天切片的进程清单中。Agent 对话不进入 OpsProtocol。当前
DeveloperLocal Textual child 通过独立的本地 Inspection typed client 只在 App 前读取一份 Latest
startup snapshot，并显示三行只读状态；这不是独立 Status 页面、持续监控、federated Inspection
或 Operations action。以后增加 Operations 页面时仍必须使用独立 OpsClient，不能把权威混入
AgentConversationClient 或这条单次 Inspection 读路径。

### 5. Fabric 自举与唯一生产数据面

Zenoh 继续是唯一 production Fabric。FabricService 是首个受管 Fabric owner，但其自身启动不能依赖尚未存在的 Fabric requirement：RuntimeHost 根据 committed local plan 和受验证本地配置启动 Fabric owner；Fabric Ready 后才安装其他 Service 的 typed bindings。

不得引入 LocalBus、MemoryBus、通用 Backend、浏览器直连 Zenoh或 TUI 私有 transport fallback。测试 fixture 只能验证相同 PortBinding/Message/Mailbox contract，不能进入 production route 或形成第二套公共名词。

### 6. 一条命令启动的边界

Developer-local 启动入口可以依次拉起 Authority、DeploymentController、RuntimeHost、FabricService、AgentService、Model adapter 与 TUI，并等待公开 readiness、转发终止信号和执行显式清理。它不得：

- 保存或改写 desired state；
- 成为第二 restart/retry/reconcile owner；
- 直写 Controller/Runtime/Agent journal；
- 绕过 DeploymentController 或 RuntimeHost 启动 Agent；
- 把 fixture、未启动组件或 skipped check 显示为 ready。

首个正式声明限定为 Ubuntu 本机、Zenoh host-local、Runtime-managed Agent CoreService 与 TUI 对话基线。其他平台只能按单独准入的 DeveloperLocal/Production profile 声明。

## 备选方案

### 恢复全能 ConsoleBridge

拒绝。它会混合外部协议、身份、缓存、Agent、运维、Runtime 和 Fabric owner，并重新形成第二个全局 Runtime。

### TUI 直接连接模型

拒绝。它绕过 AgentSession、Runtime lifecycle、request 幂等和未来分布式 binding，也会把 Secret 与 provider 细节泄漏到客户端。

### TUI 直接持有 Zenoh Session

拒绝。它把 route、重连、keyspace 和 binding generation owner 藏进 UI，破坏 FabricService 单一 owner。

### 先完成双节点、完整 OPS 和 Web Console再做对话

拒绝。双节点和运维/Web 是本地 typed contract 稳定后的独立消费者，不应阻塞第一次 Agent 对话；但在真实双节点证据前不得宣称分布式 Agent 执行完成。

## 后果

收益：

- 首个用户可见切片沿正式 Runtime/Fabric 路径实现，不产生以后推倒的旁路；
- Agent、Model、TUI、Web 和 OPS owner 保持可独立故障和演进；
- 本地对话不等待仿真、硬件、Web 或完整运维平面；
- TUI 与未来 Web 可以复用同一语义合同。

成本：

- 首聊前必须先完成一般 managed-service assembly 与本机 Zenoh typed binding；
- AgentSession journal、跨语言 codec、取消/不重复和重连必须从首版设计；
- 跨 DeckRun 长期历史、Tool 和物理 effect 需要后续独立决策，不能由 v1 偷带。

## 失败场景与反例

- 如果真实 ProcessDomain/Zenoh 集成证明类型化 client binding 无法在不泄漏 raw Session 的情况下提供所需吞吐或恢复语义，应暂停 public promotion，保留 owner 边界并研究窄 adapter，而不是恢复通用 Bus。
- 如果真实产品要求首次会话即跨 DeckRun/安装永久保留，本 ADR 的 v1 生命周期不足；必须先冻结稳定 owner、隔离、migration、retention 和 delete Receipt。
- 如果 Agent 的第一个真实消费者不需要共享长期服务，而一个 Deck 内 Agent Card 更能满足生命周期和隔离，应通过后继 ADR 调整部署角色；ConversationProtocol、TUI/Fabric 边界和 Model 不拥有 Session 的规则仍保留。

## 实施与验证

依赖顺序：

1. 收口当前 S7/P2e fixed reference；
2. 建立 committed-plan-owned single managed Service/ProcessDomain successor；
3. Runtime bootstrap FabricService，验证 Zenoh session-local/host-local typed binding；
4. 接通 AgentConversationProtocol、AgentService 与一个 Model provider；
5. 接通本地 TUI 和 developer-local 启动入口；
6. 在此基础上并行推进双 Node remote、Inspection/Ops 和未来 Web。

首个本地对话完成证据至少包括：

- committed plan 使 FabricService 与 AgentService 达到真实 Ready；
- TUI 仅通过 AgentConversationClient 完成同 Session 两个 Turn；
- CI 使用确定性 Model fixture，另有凭据外真实 Model provider smoke；
- duplicate request、different-digest conflict、timeout、cancel、worker crash、TUI restart、Fabric reconnect 与 stale generation 均有正确终态且不透明 replay；
- Ctrl-C 后 binding、child process、workspace、FD 和 retained bytes exact-zero，或明确进入 quarantine；
- ConsoleGateway、OpsService、InspectionService、NodeDaemon、仿真和物理 Driver 不在该里程碑进程清单中，也不被启动日志冒充。

Accepted 只授权按上述边界实施，不表示任何组件已经完成。只有真实代码、测试和运行证据可以更新实现状态。

## 后继与替代

本 ADR 将 Web/Console 研究中的 `ConsoleBridge` 拆分结论提升为 Agent 对话切片的实施约束，但不接受尚未实现的完整 WebRealtimeGateway、OpsService 或 InspectionService。ADR-0003 仍保持 Proposed，未来 Ops/Inspection 实现需单独接受或取得明确授权。

[ADR-0010 — Remote Agent ingress proxy 与单一 Fabric Session 边界](ADR-0010-remote-agent-ingress-proxy-boundary.md)
只在 remote-Agent ingress 场景下 supersede-in-part 本 ADR 当前快照与“组件与所有权”表中“唯一
production Zenoh Session”的绝对数量表述：原 Session 作为 `S0` 继续是唯一 production
Fabric/application bus/Agent PortBinding owner，只额外允许一个 Runtime-governed、TLS-only、非 Fabric
的 `S1` ingress Session。AgentSession、ConversationClient、TUI/Web、Model、raw Session 不向客户端泄漏
以及单一 Fabric owner 的其余规则不变。
