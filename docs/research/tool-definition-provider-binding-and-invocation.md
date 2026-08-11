# Tool 定义、Provider 绑定与调用边界研究

> 状态：Research Complete
> 日期：2026-07-29
> 结论：`proceed`，但只接受“Tool 语义合同独立于 Card/Driver/CoreService，Provider 与运行绑定另行建模”的方向；公共名称与 Schema 必须经 ADR 冻结后实施
> 实现状态：尚未实现；本文中的 DTO 名称均为候选名，不代表公共 API 已获准创建
> 范围：Tool 语义、Provider 声明、Deployment 绑定、Catalog/View、调用与物理效果边界
> 关联裁决：[ADR-0002 — CardDefinition、Card 与 CardInstance 术语边界](../adr/ADR-0002-card-definition-terminology.md)
> 关联研究：[分布式具身 Agent OS 缺口研究](distributed-embodied-agent-os-gap-analysis.md)、[CardDefinition 输入输出、Port、Link 与运行绑定](card-definition-ports-links-and-bindings.md)
> 参考实现：PhanthyMotus 与 phanthymotus-driver；EAGOS 仅用于抽取受限的中立失败模式

> 后续裁决（2026-07-29）：[ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 已接受“Rust-first mechanisms，polyglot workloads”。ToolDefinition、Provider identity、Invocation/Attempt 与 EffectReceipt 合同保持语言中立；Rust、Python、C++ 或外部协议只是 Provider Artifact 的实现/部署属性，不能成为 Tool identity 或授权捷径。

## 一句话结论

ToolDefinition 的身份与 Schema 不嵌套从属于 Card、Plugin、Driver 或 CoreService 的运行对象：它只表达稳定的逻辑调用合同，这些 owner 可通过独立 Provider 声明表达实现关系；Deployment 把经过准入的定义绑定到 desired Provider target，ToolCatalogSnapshot 再固定已经 Ready 的具体 Provider，而每次 `InvocationAttempt` 必须固定一个 Provider、版本与代次，物理副作用仍走 Authority、Lease、Safety、Enforcement 与 EffectReceipt 链。

## 1. 问题与成功标准

本研究回答六个问题：

1. 一个 ASR、TTS、导航或设备能力应由 Card 定义 Tool，还是由独立合同定义。
2. 一个实现导出多个 Tool、一个 Tool 存在多个实现，以及多个组件共同完成一个 Tool 时分别如何表达。
3. PhanthyMotus 的 Card、Plugin、Bundle、MCP Tool 与 motus-driver 的 Driver/Plugin 应如何映射到 ParaEGOX。
4. Tool 的发现、配置、生命周期、可见性、授权、Provider 选择和调用终态应由谁拥有。
5. 多 Provider 切换、流式调用、状态迁移和物理副作用如何避免重放、串线和伪成功。
6. 第一阶段怎样验证模型，而不先建立重型动态 Registry 或新的万能 Service。

成功标准是：

- `CardDefinition → Card → CardInstance` 的既有裁决不被 Tool 重新侵入。
- 明确支持 `1 Provider → N Tool` 与 `1 Tool → N Provider`，且不靠名称、列表顺序或字符串前缀消歧。
- 同一 Tool 的语义版本、Provider 实现版本、Artifact digest、运行实例和调用 Attempt 可以独立追踪。
- Agent 看见的 Tool 集合、实际授权和物理效果终态彼此分离。
- Provider crash、超时、重启、hotplug 与切换时，不透明重放 write/physical/irreversible effect。
- 首个实现切片可以静态、确定、可测试，不要求先建设全局动态 Registry。

## 2. 范围与非目标

本文研究：

- Tool 的稳定语义合同与调用粒度。
- Provider 声明、具体 Provider 绑定、ToolSet、Catalog Snapshot 与 ToolView 的所有权。
- CardInstance、CoreService、Driver-backed adapter、Gateway 与组合 Provider 的关系。
- request/response、progress、streaming、cancel、terminal result 与 reconciliation。
- 外部 MCP Tool 进入 ParaEGOX 后的转换与准入边界。

本文不定义：

- 最终 Rust trait/macro、Python class/decorator、manifest、YAML、RPC、wire Schema、C++ ABI 或 PyO3/maturin binding；这些只能是作者体验或窄 adapter，不能成为公共 Tool/Runtime ABI。
- Marketplace、通用 Workflow Service、动态 Tool Registry 或自动负载均衡服务。
- 把现有 PhanthyMotus Plugin/Bundle 类直接移植为 ParaEGOX 公共对象。
- Tool 与 Call/Query/Operation 的全部最终交互协议；本文只给出必须保持的边界。
- 用 Tool 绕过现有 Port、Operation、Authority、Resource 或 Safety 模型。
- EAGOS 兼容层、源码映射或旧领域名称迁移 API。

## 3. 证据来源与强度

### 3.1 PhanthyMotus：强实现证据

本轮只读检查了以下路径：

- `agent-core/src/api/mcp_manage.py:62-102`：MCP endpoint 通过 `initialize → tools/list` 被发现，不依赖 Card。
- `agent-core/src/api/mcp_manage.py:617-619,714-716,798-868`：请求 DTO 与远端转发路径使用 `mcp_id`、Tool 名称和参数，不读取 Card。
- `agent-core/src/mcp_client.py:46-84,276-310`：LLM Tool 使用 `mcp__{mcp_id}__{tool}` 限定名称并直接路由到 MCP endpoint。
- `agent-core/web/js/canvas.js:41-56,1999-2015`：Canvas Card 保存的是一次 `{mcpId, toolName}` 使用；Card 不是 Tool 定义类型。
- `agent-core/src/event/llm.py:222-259`：LLM 路径用 `execConnections` 筛选外部 Tool schema，但没有保留目标 Card 为正式 InstanceRef；它不是数据边、Provider 合同或生命周期依赖。
- `perception/main.py:52-104,238-258`：`PerceptionBundle` 在一个 MCP endpoint 内聚合多个 Plugin 和 Tool。

这些路径足以证明当前真实关系是：Tool 在协议与调用层独立存在；Card 是 Canvas 中对 Tool 的一次编排投影；Bundle 是 endpoint 内部聚合，不是 Deck 或应用。

### 3.2 phanthymotus-driver：强实现证据

本轮重点检查了 `unitree/r1` 与 Driver 开发规范：

- `README_dev.md:27-37,347-362`：Driver 开发规范使用独立 MCP HTTP endpoint，注册后由 Agent Core 发现 Tool。
- `README_dev.md:27-54`：Driver 暴露 `initialize/tools/list/tools/call`；Tool dict 混合类型、Schema、多实例、配置和 Topic。
- `README_dev.md:197-274`：Plugin 同时拥有 host `start/stop`，又在 `dispatch` 中处理 `start/stop/info` 与业务动作。
- `unitree/r1/main.py:55-148`：`R1DeviceBundle` 聚合 Plugin、拉平 Tool，并按列表顺序扫描 Tool 名称后 dispatch。
- `unitree/r1/device.py:603-684`：一个 `LocoPlugin` 导出 `loco`、`switch_mode`、`arm` 三个 Tool。
- `unitree/r1/device.py:1070-1120`：一个 `StatePlugin` 导出五个 Tool。
- `unitree/r1/device.py:1295-1346`：一个 Camera Plugin 导出四个 Tool。
- `unitree/r1/main.py:212-268`：MCP 与心跳注册没有稳定 hardware instance、Artifact digest、Provider generation 或 binding identity。
- `unitree/r1/main.py:219-224,281-306`：Tool advertisement 没有独立 readiness gate；DDS 初始化失败时 MCP server 仍继续启动并返回聚合 Tool。

这组证据明确证明 `Plugin/Driver → Tool` 不是 1:1；同时也证明当前实现没有正式表达 `一个逻辑 Tool → 多 Provider`、Provider readiness、selection、epoch 或 failover。

### 3.3 ParaEGOX：已有决策约束

- [ADR-0002](../adr/ADR-0002-card-definition-terminology.md) 已接受 Tool 不进入 CardDefinition 准入链；CardDefinition 是应用工作负载定义，不是 Tool 容器。
- [ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 已接受 Rust-first mechanisms 与 polyglot workloads；CoreService、Card、Driver、Gateway、Tool Provider 都不是语言分类。
- [CardDefinition、Card 与 Deck](../concepts/card-definition-card-deck.md) 已把 Tool 与 In/Out、Port、Driver、Gateway 和 CoreService 分开。
- [分布式具身 Agent OS 缺口研究](distributed-embodied-agent-os-gap-analysis.md) 已提出 `ToolDefinition`、`ToolView`、`ToolInvocation` 与 immutable `ToolCatalogSnapshot`。
- [Kernel Foundation 计划](../plans/kernel-foundation.md) 已要求首个 AgentRun 固定 ToolCatalogSnapshot，并禁止 write/physical/irreversible Tool 在 `Uncertain` 后自动 replay。

### 3.4 受限历史经验：中等强度

受限工程经验表明，把 Tool 作为某个 live 业务对象的方法并在运行时聚合，会出现定义身份与实例生命周期绑定、同名冲突行为不一致、reload 后旧 bound method 残留、暂停端点仍可见，以及不同调用入口授权不一致等风险。本文只把这些风险转化为中立约束，不保留私有源码路径、配置结构或可还原实现细节。

### 3.5 证据限制

- 本轮是源码与文档复核，没有连接 R1 真机，也没有把 Driver 的返回值当作 HIL 或安全认证。
- 本轮未发现 R1 Driver 自身覆盖 Provider identity、生命周期、故障恢复与物理调用归因的自动化契约测试；`unitree_sdk2py/test` 属于 vendored SDK 示例/测试，不构成 R1 Provider 证据。
- Motus 提供了已实现的原型结构，但本轮没有验证其运行可用性，也没有验证 ParaEGOX 候选合同的分布式恢复语义。

## 4. 当前参考路径重建

### 4.1 Motus Perception 路径

```text
config
  → PerceptionBundle
      → Plugin[]
          → get_tools()
  → one MCP endpoint
      → initialize / tools/list / tools/call
  → Agent Core registry
  → Canvas Card {mcpId, toolName}
  → execConnections filters LLM-visible schemas
  → mcp_client.call_tool()
```

一个 Plugin 可以产生多个 Tool；Card 只选中其中一个 Tool。`execConnections` 在 Canvas layout 中引用 Card id，但 LLM schema 暴露及调用路径没有把目标 Card id 保留为正式 InstanceRef，因此不能形成 CardInstance 级调用绑定；它也没有 Provider generation、Grant 或 action-level 权限。

### 4.2 R1 Driver 路径

```text
Driver package/container
  → device config
  → DeviceBundle
      → Plugin[]
          → one or many Tool dicts
  → flattened tools/list
  → first matching tool name
  → Plugin.dispatch(action, args)
  → vendor SDK / ROS2 / DDS / subprocess
```

R1DeviceBundle 是 endpoint-local implementation aggregator/dispatch host，不是 ParaEGOX Deck；经 ParaEGOX 准入后，它承载的实现才可能映射为一个或多个 Provider candidate。`driver.yaml` 是静态交付元数据，不是具体设备、运行 Provider 或调用 Attempt 的身份。

### 4.3 当前 Tool dict 实际混合的职责

Motus/driver 的 Tool dict 与 dispatch 平面至少混合：

1. LLM 可见的业务调用描述。
2. Tool type 等 Canvas 展示 metadata，以及 Agent Core 由 Topic format、device type 或名称派生的 renderer hint。
3. Port/Topic 输入输出描述。
4. shared/instance 配置 Schema。
5. `multiInstance` UI 与实例策略。
6. host 或实例的 `start/stop`。
7. `info` 承担的 discovery 补充、probe-like status、Topic 推导与 Inspection-like 输入；Motus 没有正式 Probe 类型。
8. 真正业务 action。

这种聚合适合快速原型，但无法给不同 effect、权限、超时、幂等性、生命周期和 owner 分别建立可验证合同。

## 5. 必须区分的四类“多个组件与一个 Tool”

“一个 Tool 由多个 module/card 定义”至少可能表示四种不同需求，不能共用一个含糊数组：

| 真实需求 | 正确表达 | 禁止的表达 |
| --- | --- | --- |
| 多个实现可以任选其一 | 一个 ToolDefinition，多条兼容 Provider 声明；Deployment 选定绑定 | 多个组件各复制一份同名 ToolDefinition，按注册顺序覆盖 |
| 一个实现导出多个逻辑调用 | 一个 Provider 声明引用多个 ToolDefinition | 假定 Card/Plugin 与 Tool 永远 1:1 |
| 一个逻辑调用内部依赖多个组件 | 一个 composite Provider 独占 provider-side journal、correlation 与 Tool 级终态，依赖其他 Card/CoreService/Driver | 由多个组件分别返回局部结果，却宣称存在一个原子 Tool 终态 |
| 只是希望 Agent 一次看到一组 Tool | ToolSet 只引用 ToolDefinition；ToolView 再按 policy 过滤 | 把 Tool Schema 片段散落在多个组件，运行时拼接成定义 |

其中第四类尤其重要：ToolSet 是选择与分组，不是定义 owner；ToolView 是某次 Session/Run 的可见投影，不是权限或 Provider Registry。

## 6. 方案比较

### 方案 A：Tool 继续是 Card/Plugin 的方法或子对象

优点是作者代码短，调用可以直接绑定当前实例。缺点是 Tool identity、Schema 版本、实例生命周期和实现类绑定；同一 Tool 的多个 Provider、CoreService/Gateway Provider、跨版本 Catalog、独立授权与调用恢复都需要旁路补丁。

**结论：不作为公共领域模型。** 方法或 decorator 可以保留为 Provider 内部作者语法，但编译后必须产生独立合同与绑定。

### 方案 B：独立 ToolDefinition + Provider 声明 + Deployment 绑定

它分开“调用是什么意思”“谁声称能实现”“当前部署绑定谁”“本次 Attempt 实际调用谁”，能够覆盖 CardInstance、CoreService、Gateway 和组合 Provider，并允许静态 Catalog 先行。

**结论：推荐。** 这是本文 `proceed` 的方向。

### 方案 C：每个 Tool 都成为 CoreService

它能提供稳定 endpoint 和生命周期，却会把轻量纯函数、Deck-scoped Card 能力和外部 Gateway Tool 全部平台服务化，制造过多长期状态 owner、部署单元和故障域。

**结论：拒绝作为默认。** 只有 Tool 的真实 Provider 本来就是跨 Deck 长期共享状态 owner 时，CoreService 才是 Provider。

### 方案 D：先建设动态 Tool Registry 与自动 Provider failover

它看似一次解决发现和切换，但在 Provider identity、EffectClass、Attempt、reconcile、状态迁移与物理安全尚未冻结时，只会把动态性放大为不可解释行为。

**结论：后置。** 第一阶段使用 committed DeploymentRevision 与 immutable ToolCatalogSnapshot；动态发现只能形成待准入候选。

## 7. 推荐的规范关系

以下名称是研究候选，不是已接受 API：

```text
ToolDefinition
  │ stable semantic contract
  │
  ├──────── referenced by ──────── ToolProviderDeclaration[]
  │                                  │ owner/export + requirements
  │                                  ▼
  │                         Deployment planning/admission
  │                                  │
  │                                  ▼
  └────────────────────────── ToolBinding
                                     │ committed desired provider target
                                     ▼
                         Runtime provider readiness facts
                                     │
                                     ▼
                              ToolCatalogSnapshot
                                     │ resolved provider instance,
                                     │ artifact, generation, optional ToolBindingEpoch
                                     │ ∩ ToolSet ∩ policy/context
                                     ▼
                                  ToolView
                                     │
                                     ▼
                               ToolInvocation
                                     │ one or more Attempts
                                     ▼
                             InvocationAttempt
                                     │ exactly one provider
                                     ▼
                              InvocationResult
                                     └─ physical path references EffectReceipt
```

这个模型不要求所有对象成为独立进程或服务；它要求身份、版本、owner 和失败语义可独立表达。

## 8. 候选对象的最小语义

### 8.1 ToolDefinition

`ToolDefinition` 是不可变的逻辑调用合同，候选内容包括：

- 稳定 logical ID、semantic version 与 canonical digest。
- 面向调用者的 input、output、error、progress/cancel 与 terminal schema。
- interaction kind，例如 bounded call、streaming call 或 Operation submission；不能用几条无关联 In/Out 假装一个调用。
- 作者声明的 semantic EffectClass/effect envelope：pure、read、write、physical 或 irreversible。
- 作者声明的语义级 idempotency、deduplication key、reconcile/query expectation 和超时后不确定性规则。
- permission/action claim、data classification 与结果 provenance 要求。

它明确不拥有：

- CardInstance、CoreService instance、Driver endpoint、MCP id、Node、PID 或网络地址。
- Provider-specific model、vendor、GPU、Service、Feature、Resource、egress 或 minimum isolation 要求。
- live health、readiness、配置值、Topic、PortBinding、Lease、Grant、Secret 或调用状态。
- Provider 的 Artifact 版本和运行代次。

作者提交的 EffectClass、idempotency 与权限声明只是 ToolDefinition 内的 semantic claim。Trust/Policy 不修改 ToolDefinition 或其 digest，而是针对 definition digest、ToolProviderDeclaration/Artifact 与 policy revision 产生独立、不可变的 admitted-effect/permission/retry decision；它可以保持或加强风险等级，也可以拒绝，不能因 Provider 自报而削弱。后文暂称其为 `ToolAdmissionDecision`，最终名称等待 ADR。

### 8.2 ToolProviderDeclaration

候选 `ToolProviderDeclaration` 是 Artifact 或受管 owner 类型对 ToolDefinition 的实现声明，包含：

- 支持的 ToolDefinition ID/version/digest range。
- Provider owner kind 与 export 引用，例如 CardDefinition-backed、已准入 CoreService/ServiceSpec-backed、Gateway-backed 或 Driver-backed adapter；不由此引入 application-owned/DeckRun-scoped Service。
- Artifact/export digest、Provider implementation version 和 state/protocol compatibility。
- language-neutral Artifact entrypoint、runtime kind 与 target protocol/version；语言、crate/package/module 名只作受约束的实现 metadata，不能替代 export digest、Provider identity 或 wire contract。
- Provider-specific Service/Feature/Resource/Permission/egress、minimum isolation 与平台要求。
- concurrency、reentrancy、cancel、stream、reconcile、health/readiness 和 state-transfer 支持能力。
- 一个 Provider 导出的多个 ToolDefinition 引用。

Provider-specific 要求不能写回 ToolDefinition。一个 CPU ASR 与一个 GPU ASR 可以实现同一 ToolDefinition，却拥有不同 Feature/Resource/placement 要求。

准入时不以任一单方声明直接产生“有效安全属性”：`ToolAdmissionDecision` 的 EffectClass 取 definition、provider、Artifact/测试证据与 policy 中最保守的风险上界；permission/egress/Secret/resource 要求取不可削弱的并集；retry、cancel、idempotency、reconcile 和 state-transfer 能力只取各层都能证明的交集。Provider 如果引入 ToolDefinition 未允许的外部数据披露或副作用，必须被拒绝或使用新的语义定义，不能只在实现 metadata 中悄悄增加。policy 变化产生新的 decision/catalog revision，不改写 semantic definition/version。

该对象不意味着 Provider 已经运行、健康或被授权。它只是可供解析与准入的不可变声明。

### 8.3 ToolBinding 与运行解析

候选 `ToolBinding` 是 committed DeploymentPlan 中“一个逻辑 Tool 在指定 scope 期望由哪个 Provider target 实现”的 desired relation。它至少固定：

- 稳定 ToolBindingId。
- ToolDefinition ID/version/digest。
- 选定的 ToolProviderDeclaration，以及 planned Card/已准入 CoreService/Gateway/composite provider target/export 引用。
- Provider implementation version、Artifact digest、desired Node/placement、selection/affinity 与只可加强的约束。
- runtime kind、entrypoint/export、ProcessDomain/Service/Gateway protocol version 与目标平台兼容要求；这些由 Deployment 固定，不由 ToolCatalog 按语言猜测。
- 通过的 compatibility、Trust/Policy 与 resource resolution 引用。
- 对 stateful/streaming provider 的 affinity 与迁移限制。

ToolBinding 不能包含 observed readiness、live endpoint、runtime generation、Secret 或可转移 bearer token，因为 committed DeploymentPlan 是 desired truth，不能吸收 Runtime observed facts。目标 RuntimeHost/managed workload owner 在 apply 后产生 authenticated provider instance/generation/readiness facts；Agent execution assembly 只能以纯确定性 projection 从 committed ToolBinding、ToolAdmissionDecision 与指定的 observed facts 构造 ToolCatalogSnapshot 中的 resolved binding entry，不能另选 Provider 或把结果写回 DeploymentPlan。

resolved binding entry 至少固定 ProviderInstanceRef、provider generation/session、readiness fact revision、Artifact digest 与 Catalog digest。若实现需要独立于 DeploymentRevision 和 provider generation 的 binding activation/fencing，ADR 可以引入仅在 ToolBindingId 内比较的 `ToolBindingEpoch`；它不能复用或跨域比较 Runtime PortBinding 的 BindingEpoch。首个 fixture 应先证明 DeploymentRevision + ToolBindingId + ProviderInstanceRef/generation + Catalog digest 是否已经充分，避免预造冗余 epoch。

ToolBinding 与 PortBinding 是不同关系：前者声明逻辑调用合同的 desired Provider target，后者安装消息 Port 的 live route。

### 8.4 ToolSet

候选 `ToolSet` 是 DeckSpec 中某个 Agent Card 对一组 ToolDefinition 的声明式选择，作用域随该 Card/DeckRun，可包含 logical aliases、版本范围与只可加强的 effect/action 限制。CardDefinition 可以声明允许的 Tool requirement slot，Deck 中的配置只能在其范围内选择或收窄；Session/Run policy 再从 ToolSet 向 ToolView 单向收窄，不能增加 Tool。本文不让尚未准入的 Product/Application 或泛 GraphPlan 共同拥有 ToolSet；未来领域对象若需要复用，只能引用已版本化 ToolSet 或经独立 ADR 定义自己的 selection contract。

ToolSet 不复制 Schema、不选择 desired/live Provider、不授予权限，也不拥有调用状态。

`ToolPalette` 如果未来存在，只能是 UI 对 ToolSet/ToolView 的展示投影，不成为权威领域对象。

### 8.5 ToolCatalogSnapshot

`ToolCatalogSnapshot` 是一个 Run 可复现的不可变目录快照，固定：

- 已准入 ToolDefinition 及 digest。
- committed ToolBinding 与经过 authenticated readiness facts 解析的 Provider instance/Artifact/generation，以及不可切换约束。
- 对 definition/provider combination 的 ToolAdmissionDecision、policy revision 与撤销状态引用。
- 生成它的 DeploymentRevision 与确定性 projection revision。
- 确定性的冲突、缺失与兼容性解析结果。

Catalog Snapshot 是 Run-owned value，不自动要求创建 `ToolCatalogService`。首版由 Agent execution assembly 使用纯确定性 projection，把 committed DeploymentRevision/ToolBinding、ToolAdmissionDecision 与指定的 authenticated provider fact snapshot 求交；它无权另选 Provider 或改写 source facts，只能在 missing/duplicate/stale 时拒绝建立 Run。AgentRun owner 只固定结果及其 source digests 到 `RunExecutionSnapshot`。

### 8.6 ToolView

`ToolView` 是 Catalog Snapshot、Agent Card 的 ToolSet、当前 Session/Run context、Grant、data-flow/egress policy 与 action-level restriction 的交集。它决定模型或明确的调用消费者看见的名称、描述和参数子集。

ToolView 可见性不是授权。每次 Invocation 仍需在调用点检查 audience-bound Grant、approval、数据标签、egress、资源和当前撤销状态；模型不可见也不等于后台调用有权执行。

### 8.7 ToolInvocation、InvocationAttempt 与 Result

- `ToolInvocation` 表达调用者对逻辑 Tool 的一次意图，固定 definition、args digest、deadline、caller/session/run、correlation 与期望结果协议。
- `InvocationAttempt` 表达一次具体派发，必须固定 Catalog digest、ToolBindingId、Provider instance/generation、Artifact digest、适用时的 ToolBindingEpoch、Grant/approval decision 与 attempt number。
- `ToolResult` 是 Provider 产生的 typed business output/error；`InvocationResult` 是 client Attempt journal 提交的 terminal envelope，固定 ToolResult digest、partial/progress lineage、provider acceptance 与下游 Receipt 引用。二者都不能仅凭字符串“success”证明外部或物理效果完成。

同一个 Attempt 恰好只有一个 concrete Provider。切换 Provider 必须产生新的 Attempt，并服从 effect 与 reconciliation 规则。

## 9. 所有权矩阵

| 对象或动作 | 权威 owner | 明确不拥有 |
| --- | --- | --- |
| Tool 语义合同 | ToolDefinition author | effective admitted risk、live Provider、Grant、调用状态 |
| Provider 实现声明 | Provider Artifact/export author；Deployment 验证兼容性 | 运行 readiness、自动授权 |
| ToolAdmissionDecision | Trust/Policy admission decision owner；消费 Artifact provenance/test evidence，并绑定 definition/provider/Artifact/policy revision | 修改 ToolDefinition、选择 live Provider、授予 Invocation |
| admitted Provider target candidate 的解析与排序 | DeploymentPlanner；只消费已准入 candidate | committed revision、observed readiness、live effect |
| desired ToolBinding commit | DeploymentController | observed readiness、live endpoint、runtime generation、调用业务结果 |
| Provider runtime facts | 目标 RuntimeHost 或 managed workload owner | 修改 committed ToolBinding、Catalog policy |
| resolved binding entry | Agent execution owner 从 committed ToolBinding 与 authenticated runtime facts 物化并固定到 Catalog Snapshot | 写回 DeploymentPlan、推进 Provider lifecycle |
| ToolSet | DeckSpec 中特定 Agent Card 使用的 author | live/dedicated Provider、权限、Catalog truth；Session/Run 只能收窄 |
| ToolCatalogSnapshot | Agent execution assembly 纯投影；AgentRun owner 固定 value/source digests | 改写 Deployment/Runtime/Trust facts、二次选择 Provider、动态改变既有 Run |
| ToolView | Agent owner 按 Session/Run context 与 policy 物化 | Invocation authorization、物理权限 |
| client Invocation/Attempt journal | 首个 Agent slice 的 AgentHarness/AgentRun writer | Provider acceptance/dedup/subcall journal、host lifecycle、物理最终事实 |
| Provider acceptance/dedup journal | concrete Provider；composite Provider 另拥有自身 subcall journal | 改写 client Attempt journal、宣称跨故障域原子性 |
| Card/CoreService/Gateway provider state | 对应 CardInstance、CoreService 或 Gateway owner | ToolDefinition 全局身份 |
| Provider host lifecycle | RuntimeHost 或受管 external workload owner | Tool 业务 action |
| Probe/Inspection projection | source owner 产生 fact，InspectionService 投影 | 修改 source truth |
| AuthorityDecision | Authority owner | Lease、SafetyDecision、setpoint、applied output |
| Resource Lease/fencing | ResourceCoordinator/该资源的 Lease owner | Authority/Safety policy、设备完成事实 |
| SafetyDecision | Safety owner | normal-command submission、设备 applied output |
| normal-command admission/setpoint submission | Driver EnforcementPoint | Authority/Lease/Safety 真相、下游 safe-output latch |
| safe-output/applied-output proof | 独立下游 safety gate/device boundary | Tool/Driver 文本自报终态 |
| physical EffectReceipt | 该 effect protocol 指定的 Receipt owner；必须引用上述 decision 与下游 proof | 改写各 source truth、在缺少 applied/completion proof 时宣称成功 |

## 10. Card、CoreService、Driver、Gateway 与 Tool 的关系

### 10.1 CardDefinition/CardInstance

- CardDefinition 不拥有 ToolDefinition，也不内嵌 Tool Schema 列表。
- 同一个 Artifact 可以导出 CardDefinition 和一个或多个 ToolProviderDeclaration；它们是 sibling exports，不是继承关系。
- 一个 CardInstance 可以实现零到多个 Tool Provider；同一个 ToolDefinition 也可由多个不同 CardDefinition-backed Provider 实现。
- CardInstance-scoped state 仍属于 CardInstance 私有实现对象。ToolDefinition 不拥有 ASR decoder、conversation、stream cursor 或缓存。
- Deployment 只有在 Card 实际进入 Deck 后才能提交指向其 planned target 的 ToolBinding；Card-backed Provider 实例化并 Ready 后，才可进入 ToolCatalogSnapshot 的 resolved binding entry。

作者体验可以用 Rust macro/函数、Python decorator/普通方法或其他语言的绑定减少样板，但编译结果必须分离 ToolDefinition 与 Provider 声明。不能把 Python method address、Rust trait/vtable/type identity、C++ symbol 或进程内指针当作稳定 Tool identity。

### 10.2 CoreService

CoreService 可以提供多个 Tool，但只有它本来就需要跨 Deck 长期持有平台权威状态、稳定服务合同和独立生命周期时才成立。Tool 是调用合同，CoreService 是状态与生命周期 owner；两者不能互相替代。

### 10.3 Driver-backed Provider

Driver 负责设备、仿真器或 vendor SDK 边界。Driver adapter 可以声明 read/query Tool Provider，或作为物理 Tool 的下游实现，但：

- Provider identity 不能只等于 vendor、`driver.yaml` id、MCP server name 或网络 endpoint。
- 具体 hardware/resource identity、session、generation、readiness 和 Lease 必须显式关联。
- physical/write Tool 不得给 Agent 一个绕过 Authority/Lease/Safety 的 raw Driver handle。
- 物理 Tool 应产生 typed OperationSpec 或进入等价受控链；最终只接受 EffectReceipt，而不是 Driver 返回文本。

### 10.4 Gateway 与 MCP

MCP 是外部 Tool 发现和调用协议；ParaEGOX 使用 MCPGateway 把外部 descriptor 转换为待准入的 ToolDefinition/Provider candidate。MCP endpoint、`mcp_id` 与 `server_name` 都不是内部 Tool 或 Provider 的权威身份。

默认情况下，每个外部 descriptor 只能形成 endpoint/export-qualified provider candidate；两个 endpoint 的 Tool 名称和 Schema 相同也不能自动合并为同一逻辑 ToolDefinition。只有显式、可审计的 catalog mapping 或受信 Artifact 声明通过语义兼容准入后，Provider 才能绑定到既有 canonical ToolDefinition。

Motus 直接证明一个 MCP endpoint 可以聚合多个实现单元与 Tool；Gateway 准入后，这些实现关系才可能映射为一个或多个 Provider candidate，不能由 endpoint、Plugin 或 Tool 数量直接推断。Gateway 必须：

- 为 definition、provider、endpoint 与实例建立不同 ID。
- 校验名称与 Schema 冲突，不能 first-match 或 last-write-wins。
- 将外部 lifecycle/config/info 语义转换到 ParaEGOX 的独立管理协议。
- 把外部自报 effect、idempotency、health 与权限视为不可信 claim。
- 在 endpoint 断线时更新 readiness/availability，并保持既有 Attempt 的不确定性与 lineage。

### 10.5 实现语言与故障域

- 只有受信任、同版本、静态链接且满足 RuntimeHost admission 的 Rust Provider 才可作为窄 in-process implementation；它仍保留 Provider acceptance、Attempt 与 effect handoff 的逻辑边界。
- Python、C++、未知原生库、模型 runtime 和第三方 SDK 默认进入版本化 ProcessDomain，或作为独立 Service/Gateway；worker 不能自建第二 RuntimeHost、Mailbox、restart/readiness owner 或 raw Zenoh route。
- ProcessDomain 合同至少固定 ProviderInstanceRef/generation、Artifact/export digest、request/Attempt digest、credits、deadline/cancel、progress/terminal、protocol version 与 reconciliation query。语言私有异常、对象身份和 callback address 都不能表达公共终态。
- ToolCatalogSnapshot 不按语言动态选 Provider。Deployment 根据 Artifact、target facts、runtime kind、resource/isolation 与 policy 提交唯一 compatible target，Catalog 只与 authenticated readiness facts 求交。

## 11. 发现、准入与可用性的规范链

```text
discover descriptor
  → verify Artifact/provenance/signature
  → normalize ToolDefinition candidate
  → validate semantic/version/schema compatibility
  → validate Provider-specific requirements
  → Trust/Policy emits immutable ToolAdmissionDecision
  → Deployment resolves desired provider target/resources
  → Deployment commits desired ToolBinding
  → Runtime realizes target and reports provider readiness/generation
  → Catalog builder resolves desired binding against authenticated facts
  → build immutable ToolCatalogSnapshot
  → materialize ToolView
  → authorize Invocation
```

“被发现”“有 Tool Schema”“endpoint 在线”“host 已启动”“Provider Ready”“用户可见”和“本次有权调用”是七个不同状态。任何一步失败都不能用 HTTP 200、heartbeat 或非空 Tool 列表代替后续状态。

首版规则：

- 每个被 ToolSet 选中的 ToolDefinition 在首个 binding scope 内恰有一个显式 desired Provider target，Catalog 中恰有一个 Ready resolved Provider；缺失或重复都 fail-fast。
- 不按 server name、裸 Tool name、Plugin 前缀、列表顺序或最近心跳自动选择。
- 动态发现的 Provider 只影响未来 Deployment/Catalog revision；不会静默改变已经固定的 RunExecutionSnapshot。
- Provider withdrawal/revocation 在 `Dispatched` 前阻止派发并记录明确拒绝；`Dispatched` 后不得把“撤销”解释成 effect 未发生，只能依据 provider acceptance/effect handoff 证据 cancel、query/reconcile 或进入 `Uncertain`。后续 Attempt 被拒绝，同一 Attempt 永不换 Provider。

## 12. 调用、流式与取消

首个 InvocationAttempt 状态建议继续沿用已有研究中的最小协议：

```text
Prepared
  → IntentCommitted
  → Dispatched
       ├→ ResultCommitted
       └→ Uncertain → Reconciled | Abandoned
```

关键规则：

- 外部 effect 只能在 `IntentCommitted` 后派发；恢复先检查已提交 Result/Receipt，再决定是否 reconcile。
- `Dispatched` 只表示 client Attempt journal 已持久记录 exact request envelope，并已交给 transport/send boundary；它不表示 Provider 已 durable accept，也不表示 effect handoff。
- concrete Provider 独占 provider-side acceptance/dedup record，必须在 effect 前以 InvocationId/AttemptId 与 request digest 持久准入；client 只保存其 acceptance ref/ack。ack 丢失时仍可能是 `Uncertain`，不能根据 transport success 猜测。
- effect handoff 由真实 effect owner 另行记录；Provider acceptance、effect handoff 与 terminal result 之间的 crash window 必须逐段故障注入。纯 in-process Provider 也要保留这些逻辑边界，不能用函数返回掩盖。
- timeout、cancel intent、cancel delivered、provider ack 与 effect terminal 是不同事实。
- Provider response、transport ACK、stream chunk、progress 和 ToolResult 都只有各自层级的含义。
- write/physical/irreversible 在 `Dispatched` 后未知时不得透明 retry 或 failover。
- pure/read Tool 只有在合同与 Provider 均证明可安全重试时，才可创建新 Attempt；仍需保留旧 Attempt lineage。

跨 ProcessDomain 调用也使用同一状态机。Rust future 被 cancel、Python task 被取消、worker socket EOF 或进程退出，都不证明 Provider 未 durable accept 或 effect 未 handoff；`Dispatched` 后缺少唯一 terminal proof 时必须进入 `Uncertain → query/reconcile`。PyO3 直调或同进程 Rust Provider 不能绕过该语义。

Streaming Tool 还必须固定：

- request、chunk/progress、cancel 与 final 的 correlation。
- sequence/cursor、bounded buffer、backpressure、deadline 与 consumer abandonment。
- 唯一协议终态；最后一个 chunk 不是天然 terminal Receipt。
- Provider affinity。除非存在版本化 checkpoint/state-transfer 协议，stream 不跨 Provider 恢复。

## 13. 多 Provider、选择、切换与组合 Provider

### 13.1 Alternative providers

多个 Provider 只有在实现同一个 ToolDefinition digest 或经过显式兼容解析时才是 alternatives。CPU/GPU、本地/云端、内置/USB/远端可以有不同 Provider 要求，但不能暗中削弱 input/output、effect、permission、idempotency 或 terminal 语义。

第一阶段不做运行期自动选择。Deployment/Profile 显式绑定一个 Provider；第二阶段才可以研究基于 locality、resource、cost、latency 或 health 的候选排序，并且选择发生在 Attempt 建立前。

### 13.2 Stateful providers

有状态调用默认 sticky。Provider state schema、checkpoint/cursor、migration protocol 与 compatibility version 必须显式存在，才能在 Provider replacement 后继续；否则旧调用进入 `Uncertain` 或终止，新 Provider 只接受新 Invocation/Attempt。

### 13.3 Composite provider

一个逻辑 Tool 若需要 ASR、retrieval、policy 和 TTS 等多个组件，必须有一个 composite Provider 独占：

- provider-side acceptance/dedup 与 subcall journal；client Invocation/Attempt journal 仍由调用方拥有。
- 子调用 correlation 与 deadline budget。
- cancel/compensation/reconcile policy。
- Tool 级 terminal Result 与 Receipt lineage。

子组件仍拥有各自状态与局部 Receipt。单一 composite owner 不自动赋予跨组件或跨故障域原子性：只有同一可证明事务边界，或有版本化 prepare/commit/barrier 与故障注入证据时，才可宣称原子；否则必须暴露 partial/`Uncertain`、compensation/reconcile 语义。若不存在一个 owner 能定义这些语义，该过程应建模为领域 Workflow/多个 Tool/Operation，而不是声称它是一个 Tool。

### 13.4 禁止运行时拼装定义

多个组件可以共同实现 Provider，不能分别提交 Tool Schema 片段再在运行时拼装一个“共同定义”。这种模式没有唯一 semantic version、digest、review owner、权限边界和兼容性判定；组合必须发生在 Provider 实现后面，ToolDefinition 仍由一个 owner 发布。

## 14. Lifecycle、配置、Port 与 Tool 必须分开

Motus/driver 中 `start/stop/info/config` 与业务 action 共用 dispatch。ParaEGOX 应拆分：

| 语义 | owner/路径 | 是否作为普通 Tool action |
| --- | --- | --- |
| Provider host prepare/start/drain/stop/recover | RuntimeHost 或 managed workload owner | 否 |
| Card/CoreService instance 生命周期 | 对应实例 owner，产生 Lifecycle Receipt | 否 |
| Provider readiness/health/probe | source owner 产事实，Inspection 投影 | 否 |
| desired configuration | Deck/Service/Deployment owner；revisioned apply | 否 |
| instance targeting/affinity | ToolBinding 与 InvocationAttempt | 否，不用隐藏 `instance_id` 注入 |
| 业务调用 | ToolInvocation | 是 |
| 物理动作 | Tool → typed Operation/受控执行链 | 不是 raw Driver action |

ToolDefinition 可以定义业务级 `cancel` 或 `stop_speaking`，但不能与 host lifecycle `stop` 同名同义。若一个 action union 内的分支具有不同 EffectClass、权限、幂等性或终态协议，应拆成多个 ToolDefinition；只有语义与安全属性一致时才保留 enum 参数。

Port 与 Tool 也不互相替代：

- 连续音频、图像、Telemetry 与中间处理仍优先使用 typed Port/Observation stream。
- snapshot/query、一次推理、受控操作和 Agent function calling 使用 Tool/Query/Operation。
- Tool Provider 可以内部消费 Port，但不能把 live Topic 塞入 ToolDefinition，或由 Canvas 连线推断调用原子性。
- Motus 的 sensor/processor/actuator/resource 四类 Tool 需要逐项归类，不能整体迁移为 ParaEGOX Tool。

## 15. Security、Effect 与物理边界

### 15.1 ToolView 不授予权限

ToolView 只决定可见 Schema。调用点至少重新验证 caller/audience、Grant revision、action、EffectClass、data labels、egress、approval、resource condition、deadline 和 revocation；Tool 输出默认是不可信 data，不能升级为 system/operator instruction。

### 15.2 Provider 声明不是安全事实

Provider 自报 read-only、idempotent、healthy、Ready 或 permission-free 均不能直接进入权威 Catalog。准入根据 Artifact provenance、测试证据、policy 和保守默认决定有效上界；无法证明的 effect 按更危险等级处理。

### 15.3 物理 Tool 不拥有设备成功事实

```text
ToolInvocation
  → admitted Tool Provider
  → typed OperationSpec / PhysicalCommand intent
  → Authority decision
  → Resource Lease + fencing
  → Safety decision
  → Driver EnforcementPoint normal-command admission/setpoint
  → downstream safety gate / device applied-output proof
  → designated EffectReceipt owner references the chain
  → InvocationResult references receipt
```

这些步骤没有共同的“物理 effect 总 owner”：Authority、ResourceCoordinator、Safety、Driver EnforcementPoint 与下游 safety gate 分别拥有自己的 Decision/Lease/submission/applied-output fact。物理首切片可以让 Driver EnforcementPoint 成为指定 EffectReceipt assembler，但 Receipt 必须引用当前 AuthorityDecision、Lease/fence、SafetyDecision 和下游 applied/completion proof；缺少后者时只能报告 accepted/submitted/unknown，不能自证 applied success。Tool Provider、AgentHarness、MCPGateway、Card 或领域 Graph 都不能生成假的物理成功。对 physical/irreversible effect，Provider crash、网络超时或 ack 丢失默认进入 `Uncertain`，先 query/reconcile，再决定后续动作。Provider 或 Driver 使用 safe Rust 也不会自动取得 Authority、Lease、Safety 或 applied-output ownership，更不构成功能安全或硬实时证明。

## 16. 对 Motus 与 motus-driver 的取舍

### 16.1 可以继承的经验

- MCP 适合作为外部 Tool descriptor/call 协议。
- Tool 可以脱离 Card 被发现和调用。
- 一个 endpoint/Plugin/Driver 可以聚合并导出多个 Tool。
- Card 提供布局持久化的 Card key，并在当前原型中复用为实例 key；这证明了实例定向需求，不构成正式 Runtime identity。
- Agent 对外部 MCP Tool 只看见经 `execConnections` 选中的 schema；系统 Tool 由另一条路径加入。
- shared 与 instance 配置需要分层。
- endpoint-registration-qualified naming 有助于避免当前显示层冲突，但不能成为 Provider identity。

### 16.2 不能照搬的结构

- `Card = one MCP Tool`。
- `Bundle = Deck/Application`；Motus Bundle 只是 endpoint-local 聚合。
- 一个 broad Tool 用 `action` 混合 lifecycle、inspection、configuration 和不同权限的业务动作。
- `mcp_id`、server name、vendor 或 URL 作为长期 Provider identity。
- 同名 Tool first-match、last-write-wins 或按 server name 去重。
- `multiInstance` 主要作为 discovery/Canvas metadata；实际实例管理依赖带外 `instance_id`、动态 Topic 与各 Plugin 私有字典，没有统一实例合同。
- Tool advertisement 不受独立 Provider readiness contract 约束；例如 R1 在 DDS 不可用时仍启动 MCP server，并继续通过 `tools/list` 返回聚合 Tool。
- 当前没有领域级 Invocation/Attempt identity、持久 correlation journal 或 timeout 后 reconciliation 合同，因此超时、迟到完成和重试的业务语义未定义。
- 用动态 Topic、Tool 数量或字符串前缀推断 Bundle、类型和路由。

### 16.3 中立映射

| Motus / motus-driver | ParaEGOX 解释 | 处理方式 |
| --- | --- | --- |
| MCP Tool dict | ToolDefinition、Port/config/view metadata 的混合原型；实现来源由 endpoint/Plugin 带外关联 | 拆分后保留语义，不原样兼容 |
| Plugin | 私有实现/adapter/dispatch 单元；可能支撑一个或多个 Provider 声明，但不等于 Provider identity | 不建立统一公共 Plugin 基类 |
| PerceptionBundle / R1DeviceBundle | endpoint-local implementation aggregator/dispatch host | 不等于 Deck，也不能由 Bundle 直接推导 Provider 基数 |
| Canvas Card | Tool 的一次 UI/编排使用 | ParaEGOX Card 继续由 CardDefinition/Deck 裁决，不等于 Tool |
| execConnections | 非常粗的外部 Tool visibility selection 原型；映射到 ToolSet/ToolView 是 ParaEGOX 设计推断 | 拆成 Tool 选择、Provider binding、Grant 与 InstanceRef |
| driver.yaml | Artifact/Driver 交付 metadata | 不作为 live Provider/device identity |
| multiInstance | UI 与 Plugin 内部实例字典 | 转为显式 Card/CoreService/provider instance、generation、resource lease 与 affinity |
| info/start/stop/config action | discovery、inspection、lifecycle、config 混合 | 迁移到各自 owner |

## 17. 实施顺序

### T0：ADR 与研究 fixture

- 建立 ToolDefinition/ToolProviderDeclaration/ToolAdmissionDecision/ToolBinding/ToolSet/Catalog/View/Invocation 的最小 fixture。
- 用 ASR/TTS、一个 read-only query、一个 stateful stream 和一个 simulated physical Tool 检查对象数量与作者体验。
- 新 ADR 冻结公共名称、版本 owner、Card/已准入 CoreService/Gateway provider relation 和 first-slice 非目标。
- 用 canonical golden vectors 冻结语言中立 Schema/digest；Cargo 与 `uv` 分别承载 Rust/Python fixture，并验证 Rust↔Python 双向 encode/decode。

在 T0 通过前，不创建 `tools/` 公共包、Tool Registry daemon、通用 ToolService 或自动 failover manager。

### T1：静态单 Provider、read-only vertical slice

- 一个 immutable ToolDefinition。
- 一个 Card-backed 或已准入 CoreService/ServiceSpec-backed Provider 声明。
- 一个把 definition/provider/Artifact 与 policy revision 固定在一起的 immutable ToolAdmissionDecision。
- committed DeploymentPlan 中一个显式 desired ToolBinding。
- 一个把该 binding 与 authenticated ready Provider fact 解析后的 immutable ToolCatalogSnapshot，以及按 Session 过滤的 ToolView。
- 一个带 args/result digest、deadline、Attempt、provider generation 的 read-only 调用。
- reference slice 覆盖 Rust host + Python ProcessDomain Provider；若采用 in-process Rust Provider，也必须通过同一 Attempt/terminal conformance。
- duplicate/missing/incompatible provider 均在启动前 fail-fast。

### T2：基数与冲突验证

- 一个 Provider 导出多个 Tool。
- 两个不同 Provider 声明同一 ToolDefinition，但 Profile 必须显式选一。
- 同名不同 digest、同 digest 不兼容 protocol、重复 Provider identity 全部稳定拒绝。
- Card restart 后旧 ProviderInstanceRef/generation，以及适用时旧 ToolBindingEpoch 的 Attempt 不能派发。

### T3：MCPGateway

- 通过 MCP 发现外部 descriptor，标准化为 candidate。
- 完成 descriptor provenance、Schema、effect 与 provider identity 准入；readiness 只消费 Runtime 认证事实，不由 Gateway 自报即生效。
- 验证 endpoint 聚合、断连、重新注册和重复 server name 不改变内部 identity。

### T4：Composite 与 streaming

- 一个 composite read-only Provider，拥有 provider-side acceptance/dedup/subcall journal、deadline、cancel/reconcile 与唯一 Tool-level terminal Result；client Attempt journal 仍由调用方拥有。若无可证明的 prepare/commit/barrier，跨故障域失败必须暴露 partial/`Uncertain`，不能宣称原子完成。
- 一个有界 streaming Tool，验证 sequence/cursor/backpressure、provider stickiness 与 crash 后 `Uncertain`。
- 无 state-transfer 协议时禁止跨 Provider resume。

### T5：simulated physical Tool

- Tool 只生成 typed OperationSpec。
- 运行完整 Authority、Lease/fencing、Safety、Enforcement、EffectReceipt 与 reconciliation 链。
- 注入 timeout、Driver restart、迟到 result、旧 generation 和撤销，证明不透明重放。

动态 Registry、运行期自动 Provider 选路、跨 Provider state transfer 与真实硬件 Tool 均在上述证据之后单独立项。

## 18. 验证矩阵

| 场景 | 必须证明 | 不充分证据 |
| --- | --- | --- |
| 一个 Provider 多 Tool | 每个 definition/version/digest 独立，调用准确路由 | `tools/list` 能返回多个名字 |
| 一个 Tool 多 Provider | 显式兼容与唯一 desired target，Catalog 唯一解析 Ready Provider，Attempt 固定一个 Provider | 同名 endpoint 都在线 |
| definition conflict | 构建/部署稳定 fail-fast | first-match 或覆盖后仍能调用 |
| admission policy change | 新 ToolAdmissionDecision/catalog revision，不改 ToolDefinition digest；风险只加强 | 在线修改 Definition 或信任 Provider 自报 |
| desired/observed split | DeploymentPlan 只含 desired binding；Catalog 纯投影 authenticated facts | 把 readiness/generation 写回 plan |
| Provider restart | 旧 ProviderInstanceRef/generation 被拒绝，inflight 可解释 | 新进程 heartbeat 恢复 |
| missing readiness | Tool 不进入可调用 binding | host start 或 HTTP 200 |
| ToolView/action restriction | 模型只见允许 action，调用点仍重新授权 | UI 隐藏按钮 |
| dispatch/accept crash windows | client Attempt、provider acceptance/dedup、effect handoff 与 terminal records 分权且 correlation 不串线 | transport ACK 或单一日志 |
| timeout/late result | 旧 Attempt 进入 Uncertain，新 Attempt 不接收迟到结果 | 仅有 transport timeout |
| streaming overload | buffer 有界、sequence/cursor 和 terminal 守恒 | 能连续收到 chunk |
| stateful replacement | 无迁移协议不 resume，有协议固定 state/protocol version | 自动重连成功 |
| composite Provider | provider-side journal/terminal owner、子 Receipt lineage、partial/Uncertain/compensation；原子性另有事务证据 | 多个组件都返回 success 或只有一个协调对象 |
| MCP duplicate endpoint | endpoint/provider/definition identity 分离 | 按 server name 合并 |
| physical effect | Authority/Lease/Safety/submission/applied proof 分权，EffectReceipt 引用完整链，旧 lease/fence 拒绝 | ToolResult 或 Driver 文本为 success |
| lifecycle/config | revisioned apply、readiness、drain/stop Receipt 分离 | `action=start/stop/info/config` 都返回 200 |
| deterministic Catalog | 相同 committed/admission/runtime-fact snapshot 产生相同 digest，projection 不选 Provider，Run 内不漂移 | 每次 prompt 动态扫描 registry |
| cross-language Provider | Rust host 与 Python/C++ worker 双向 golden vectors、protocol/version mismatch、error mapping | 能通过 PyO3/import 调到函数 |
| ProcessDomain termination | cancel、EOF、SIGKILL、late reply、restart 后旧 generation | 进程退出或 future aborted 就报告 effect 未发生 |

## 19. 风险、反例与推翻条件

### 风险

- **对象过多**：定义、声明、绑定、Catalog、View 和 Attempt 会增加实现成本。缓解方式是给作者提供简洁语法，而不是合并领域身份。
- **双重版本复杂度**：Tool semantic version 与 Provider implementation version 必须同时记录。它们真实不同，不能用一个版本假装简化。
- **静态 Snapshot 陈旧**：revocation 与 Provider failure 对未来或尚未 dispatch 的 Attempt 必须 fail-closed；已 dispatch Attempt 进入 cancel/query/reconcile 或 `Uncertain`。静态表示“不可静默替换”，不表示忽略安全事件。
- **Provider 组合滥用**：任何复杂流程都包装成一个 composite Tool 会隐藏 partial failure。必须要求 provider-side journal/终态 owner，并明确单一 owner 不等于跨故障域原子；无法定义 partial/compensation/reconcile 时升级为 Workflow/Operation。
- **Card 与 Tool 双入口**：同一业务能力如果同时暴露 Port 与 Tool，可能形成重复 effect 路径。Deployment/Authority 必须识别共同资源与唯一 enforcement。
- **MCP 兼容压力**：外部 Tool descriptor 不足以表达内部 effect、state、stream 与 readiness；Gateway 必须允许保守降级或拒绝，不能伪造字段。
- **语言特例侵蚀合同**：Python decorator、Rust trait/PyO3 或 C++ SDK 的隐式 fast path 会产生多套 identity、错误和终态语义；所有实现必须通过同一 Artifact/export 与 Invocation conformance。

### 会推翻或收缩本方案的新证据

- 如果首个真实产品中所有 Tool 都只在单一 Agent 进程内使用、没有版本、授权、Provider 替换、恢复或外部协议需求，可以把 ToolProviderDeclaration/ToolBinding 收缩为内部编译记录，但 ToolDefinition 与 InvocationAttempt 仍应保留。
- 如果出现多个非 Agent 消费者对相同 ToolCatalog 的长期共享、动态查询与 watch 需求，才有证据研究独立 ToolCatalogService/Registry。
- 如果 Tool 与 Call/Query/Operation 最终证明拥有完全相同的合同、状态机和所有权，可以通过新 ADR 合并中性 InteractionDefinition；不能先以名称相似为由合并。
- 如果 Card 未来退化为纯 UI 投影，Card-backed Provider relation 需要随 CardDefinition ADR 一起重审，但 Tool 不因此回归 UI Card。

## 20. 推荐裁决与开放问题

### 推荐裁决

1. 接受 ToolDefinition 独立于 CardDefinition、Card、Driver、Gateway 与 CoreService 的方向。
2. 接受 `ToolProviderDeclaration + ToolAdmissionDecision → desired Deployment ToolBinding → ready Provider resolution/Catalog Snapshot → ToolView → Invocation/Attempt` 的分层语义。
3. 接受 `1 Provider → N Tool`、`1 Tool → N Provider`；一个 Attempt 只能选择一个 Provider。
4. 多组件共同实现一个 Tool 时必须存在 composite provider-side journal/terminal owner；否则建模为 Workflow/Operation。单一 owner 不自动证明跨故障域原子性。
5. 第一阶段只实现静态、单 desired target 且 Catalog 唯一解析一个 Ready Provider 的 read-only slice；不建设动态 Registry 和透明 failover。
6. MCP 作为 Gateway 协议，Motus Bundle/Plugin 仅作为参考实现，不进入 ParaEGOX 公共领域模型。
7. 物理 Tool 永远不绕过 Authority/Lease/Safety/Enforcement/EffectReceipt。
8. ToolDefinition 与 Provider 公共合同语言中立；Deployment 固定 runtime kind/target，Rust in-process 仅限受信静态同版本实现，Python/C++/未知代码默认 ProcessDomain/Service/Gateway。

### 需要 ADR 冻结的问题

- `ToolProviderDeclaration`、`ToolAdmissionDecision`、desired `ToolBinding`、resolved binding entry 与 `ToolSet` 是否采用这些最终名称。
- ToolDefinition、ToolProviderDeclaration 与 Artifact export 的 version/digest 兼容规则。
- Card-backed/CoreService-backed/Gateway-backed provider reference 的最小中性 Schema。
- Catalog Snapshot 纯 projection 的输入/output Schema、代码放置与 DeploymentRevision/RunExecutionSnapshot 精确引用方式；projection 无 Provider 选择权这一 owner 边界不再开放。
- action granularity、stream protocol、cancel、progress、error 与 terminal Schema。
- desired ToolBinding 与 resolved binding entry 的最终名称，Provider readiness/generation/withdrawal/revocation 的精确状态机，以及是否确有必要引入 ToolBindingEpoch。
- pure/read retry 的最小证明条件，以及 write/physical reconciliation query 协议。
- Tool 与 Query/Operation 是否共享底层 invocation envelope，但保持不同 effect/terminal contract。

这些问题未冻结前，本文只授权继续研究、ADR 与 fixture，不授权创建稳定 SDK、公共 Schema、Registry daemon 或自动 Provider failover。
