# ADR-0002 — CardDefinition、Card 与 CardInstance 术语边界

> 状态：Accepted
> 日期：2026-07-29
> 修订：2026-07-29，链接 Proposed ADR-0004 与 Accepted ADR-0006；不改变 CardDefinition 决策
> 决策者：ParaEGOX maintainers
> 关联文档：[CardDefinition、Card 与 Deck](../concepts/card-definition-card-deck.md)、[CardDefinition 输入输出、Port、Link 与运行绑定](../research/card-definition-ports-links-and-bindings.md)、[ADR-0004 — Deck 工作负载、DeckLock 与 Application 准入边界](ADR-0004-deck-workload-and-application-admission-boundary.md)、[ADR-0006 — Rust-first 核心机制与多语言工作负载边界](ADR-0006-rust-first-core-and-polyglot-workloads.md)、[Kernel、RuntimeHost 与 Core Services](../architecture/kernel-runtime-core-services.md)

## 一句话结论

ParaEGOX 保留“可复用、可版本化、无运行身份的能力定义层”，但不再使用 `Module` 作为公共领域名：唯一规范链为 `CardDefinition → Card → CardInstance`，实现代码是 Artifact entrypoint 后的私有对象，不形成万能基类或第二个运行身份。

## 背景

PhanthyMotus 中 `Card` 是已经存在的 Canvas 编排对象。它保存一次 Tool 使用的名称与配置引用，连接通过 Card id 引用它，Card id 又被用作 `instance_id` 来源，因此最自然地对应 Deck 内的一次配置使用，而不是可安装代码类型。PhanthyMotus 当前没有 `Deck` 类型；ParaEGOX 的 Deck 是对 `CanvasLayout + Project` 心智模型的声明式演进。Motus 的 `PerceptionBundle` 则只是一个 MCP endpoint 内聚合多个 Plugin/Tool 的实现概念，不是应用或发行模型。

EAGOS 的 `Module` 同时承担能力实现、Input/Output、运行身份、生命周期、通信、线程/lane、Tool、Probe、TF、配置、安全和健康等职责。ParaEGOX 需要保留其中的行为需求，但不能继续让一个公共基类拥有这些职责。

当前 ParaEGOX 仍然需要一个独立于 Deck Card、CardInstance 和 Artifact 的定义层：DeckLock 必须在尚未选择 live Node、placement 和运行 route 时，锁定稳定 ID、版本、Port/interaction、配置 Schema、Requirement、ExecutionRequirements 和实现 export。删除这一层只会把相同合同隐式塞入 Artifact manifest。

## 范围与非目标

本 ADR 决定：

- 可复用能力定义、Deck 内配置使用、运行身份和实现对象的规范名称与所有权。
- Deck 作者引用能力定义的字段语义。
- 哪些对象可以成为 CardDefinition，以及哪些必须保持 CoreService、Driver、Gateway、Tool、Skill 或普通 library。
- 文档、Schema、包名和未来代码中 `Module` 领域名的迁移规则。

本 ADR 不决定：

- Rust/Python/C++ 等语言 SDK、descriptor、decorator、macro、trait 或 manifest 的最终作者语法。
- Artifact registry、Marketplace 或多 provider Card 合同的最终协议。
- Call、Operation、Tool 和 State 的完整交互 Schema。
- CoreService、Driver 或 Gateway 的运行合同。
- 将实现语言定义成 Card、CoreService、Driver 或 Gateway 的领域身份；语言与进程边界服从 [ADR-0006](ADR-0006-rust-first-core-and-polyglot-workloads.md)。

## 决策

### 1. 唯一规范链

```text
CardDefinition ──referenced by──> Card ──realized as──> CardInstance
       │                             │                       │
       │ immutable contract          │ Deck desired use      │ Runtime identity
       └── Artifact export ref       └── config + Links      └── private implementation
```

- `CardDefinition` 是不可变、可复用、可版本化的能力定义，至少拥有稳定 ID/version、Port/interaction、配置 Schema、Service/Permission/Feature Requirement、ExecutionRequirements 和 Artifact export/entrypoint 引用。
- `Card` 是一个 Deck 中对 CardDefinition 的具名、配置使用；Deck Link 通过 Card 的稳定 key 引用其 Port。Card 本身不拥有 Link、PID、线程、运行状态或 live binding。
- `CardInstance` 是 RuntimeHost 按 committed DeploymentPlan 创建和管理的运行身份。
- Artifact 提供代码、模型或二进制实现及平台变体；Artifact 不是 CardDefinition，CardDefinition 也不是安装包。
- entrypoint 只是 Artifact 中实现 export 的语言中立定位和装载合同，不建立新的 `Service`/`Driver`/`Job` 统一继承树，也不承诺公共 Rust `dylib`/trait-object ABI。

### 2. CardDefinition 是数据，不是业务基类

业务实现不继承一个能注入 Runtime、Bus、Zenoh Session、线程、lane、TF、Tool、Probe、token 或 Service Locator 的 `Module`/`CardDefinition` 基类。语言 SDK 可以用 manifest、decorator、macro 或窄 adapter 将语言内私有实现关联到同一 CardDefinition，但这些只是作者体验和 Artifact binding，不改变 CardDefinition、Card 或 CoreService 的身份。按 ADR-0006，只有受信、与 RuntimeHost 同构建同发布的 Rust 实现可以通过内部 registry/static linkage 进入 Loop/ThreadDomain；Python、C++、未知或不可信 Artifact 默认经版本化 ProcessDomain 协议装载，具体作者 API 在实现切片中验证后再冻结。

RuntimeHost 为每个 CardInstance 创建或请求 subordinate worker 创建 generation-scoped 的私有实现。该实现可以保存 CardInstance-scoped 的领域状态并提供 `on_start`、输入处理、`on_stop` 等回调，但不拥有系统运行身份、生命周期状态机、恢复策略或 placement。`Handler` 与 `Factory` 不成为普通作者必须理解的公共领域实体；ProcessDomain worker 也不是第二 RuntimeHost。

### 3. Deck 引用语法

Deck 作者使用 `uses` 表达 CardDefinitionRef，不使用 `module` 字段：

```yaml
cards:
  near_field_asr:
    uses: para.speech.asr@^2.1
    profile: near-field
```

具体序列化格式仍可演进，但规范 DTO 名为 `CardDefinition` 与 `CardDefinitionRef`；不并存 `ModuleSpec`、`ModuleRef`、`CardType` 或语义相同的兼容别名，也不引入冗余 `CardDefinitionSpec`。

本 ADR 不决定 Deck 与 Product/Application/Installation 的长期关系；该问题由 Proposed [ADR-0004](ADR-0004-deck-workload-and-application-admission-boundary.md) 单独评审。无论该提案是否接受，都不能改写本 ADR 已接受的 `CardDefinition → Card → CardInstance` 定义/配置使用/运行身份链，或把新的产品聚合对象变成 CardInstance 的运行父对象。

### 4. 准入边界

只有能进入 Deck、具有稳定合同、可独立版本/复用/配置、能够形成隔离 CardInstance，且 Deployment 需要解析其执行或资源要求的应用工作负载，才定义 CardDefinition。

- 无独立配置、复用、隔离或观测边界的代码仍是普通 function/library。
- 跨 Deck 共享并长期持有平台权威状态的能力使用 CoreService/ServiceSpec。
- 设备、仿真器或外部协议边界保持 Driver/Gateway。
- MCP/Agent 调用语义保持 Tool，Agent 指令资源保持 Skill。
- 注释、分组、条件展示等纯编辑器对象属于 Canvas View State/CanvasElement，不成为 Card。

采用 Card 家族词汇不能成为“把所有对象 Card 化”的理由。

### 5. `Module` 的保留范围

- ParaEGOX 新的公共文档、Schema、manifest、CLI、包和代码类型不使用大写 `Module` 作为领域名。
- EAGOS `Module`、历史失败模式、源码路径以及“未迁移旧 Module 语义”等 provenance 表述保留原名并明确限定来源。
- Python/Rust module 等普通编程语言词汇不受限制。
- 当前尚无发布兼容负担，因此不提供 `Module = CardDefinition` 兼容别名。

## 备选方案

### 保留窄 Module

它能够表达定义层，但同时与 Python module/`ModuleSpec`、EAGOS 万能基类和私有实现对象产生多重歧义；Card/Deck 用户还要额外学习一套无关词族，因此不采用。

### 直接把 Card 定义为可复用类型

名称较短，但会破坏 PhanthyMotus 中 Card 已经表示 Canvas 配置使用的心智模型，并让“Deck 中的 Card”和“Card 类型”同名。为保持定义、期望和运行三态分离，不采用。

### CardType 或 CardSpec

`CardType` 容易与 Sensor/Processor/Agent 等角色分类和 payload type 混淆；`CardSpec` 容易被理解为 Deck 内某张 Card 的 desired 配置。它们不建立为并列公共对象。

### CardTemplate、CardBlueprint 或 CardPack

Template/Blueprint 更像默认配置复制件，不能准确表达版本化合同；Pack 若未来存在，只能表示发行或展示聚合，不能替代定义层。

### WorkloadDefinition

它适合真正被 Deck、CoreService、Gateway 和 batch 同等消费的中性合同，但当前没有这类共享消费者证据。提前引入会掩盖尚未裁决的 managed Gateway/workload 边界，因此后置。

## 后果

收益：

- Card/Deck 的产品词汇与定义、期望、运行三态一致。
- 从名称和结构上切断 EAGOS 万能 Module 基类的回流路径。
- CardDefinition 的准入条件自然排除 CoreService、Driver、Gateway、Tool 和 helper。
- Deck 用户主要只接触 `Card` 与 `uses`；较长的 CardDefinition 名称集中在合同、Catalog、Resolver 和 SDK 层。

成本：

- 现有架构与研究文档中的 `Module`、`ModuleSpec`、`modules/` 和示例需要一次原子迁移。
- 未来实现必须明确 Artifact entrypoint 如何按 runtime kind 关联可信 Rust 私有实现或启动多语言 ProcessDomain worker，而不能依赖万能父类、嵌入解释器或公共 Rust 动态库 ABI 注入上下文。
- Card 家族词汇可能诱导 UI 将非执行元素伪装成 Card，必须由准入验证阻止。

## 失败场景与反例

以下新证据会触发重新评估：

- Card 退化为纯 UI 投影，不再是 Deck 的持久领域对象；此时底层定义应考虑中性的 `WorkloadDefinition`。
- 同一可复用定义需要脱离 Deck，被 CoreService、Gateway、CLI/batch 以相同合同和生命周期消费；应先证明共同抽象，再拆出中性定义层。
- 产品正式收缩为无多实例、无 DeckLock、无跨平台 Artifact variant 的单进程 appliance；此时 CardDefinition 可能收缩为 Artifact export metadata。
- 一个语义合同需要多个互换 provider；届时应评估 `CardContract` 与 provider release/implementation 的拆分，不能让 CardDefinition 同时模糊表示接口和 provider。

## 实施与验证

- 将目标模型中的 `ModuleSpec`、`ModuleRef`、`Module` 与 `modules/` 分别迁移为 `CardDefinition`、`CardDefinitionRef`、明确的 CardInstance 私有实现对象表述与 `cards/`。
- 将 Deck 示例中的 `module:` 迁移为 `uses:`。
- 重命名概念与研究文档文件，并更新全部相对链接。
- 术语检查只允许受限定的 EAGOS `Module` 历史引用和普通编程语言用法。
- 用两个相同 CardDefinition、不同配置的 ASR Card 验证 Card/CardInstance 隔离；用 CoreService、Driver、Gateway、Tool 和 CanvasElement 反例验证准入边界。
- 用同一语言中立 CardDefinition 分别验证可信 Rust in-process fixture 与 Python ProcessDomain reference worker；两者必须产生相同合同语义，但不得共享语言私有对象、Runtime handle 或未版本化 ABI。直接构造 Python 实现对象只形成 workload unit profile，不形成 CardInstance、Runtime readiness 或生产装载证据。

文档迁移完成不代表 SDK、Schema 或 Runtime 已实现。

## 后继与替代

本 ADR 替代此前研究草案中“保留窄 Module 领域名”的建议；ADR-0006 补充实现语言与进程边界，但不替代本记录的术语裁决。若未来引入 `WorkloadDefinition` 或拆分 `CardContract`，必须由新 ADR 明确替代本记录的对应部分。
