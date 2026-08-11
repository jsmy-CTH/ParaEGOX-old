# Kernel 消息机制、Fabric、Evidence、Telemetry 与 Security 边界研究

> 状态：Research Complete，结论为 `revise`
> 日期：2026-07-29
> 深度：Deep
> 评审策略：Dual independent review + EAGOS failure-path audit；尚待 Proposed ADR 评审
> 范围：Kernel、RuntimeHost、消息面、Zenoh、网络、Evidence、Telemetry、Intent、Security、OPS
> 实现状态：尚未实现；本文是决策输入，不是功能完成证明

> 术语更新（2026-07-28）：本文早期同时使用 capability 表示授权、服务和传输支持；新基线只用 `CapabilityGrant` 表示授权，服务接口使用 `ServiceContract`，Node/Fabric/Device 支持事实使用限定 FeatureReport。见 [Capability、Service Contract 与 Feature Support](../concepts/capability-service-feature-boundaries.md)。

> 后续裁决（2026-07-28）：[ADR-0001](../adr/ADR-0001-deployment-controller-boundary.md) 确认 DeploymentController 是 Kernel 外的 plan/binding desired-state owner；Planner 只产生 DeploymentPlanCandidate，DeploymentController 原子提交 plan/revision，Fabric 与 Runtime 只实现 tenure-neutral Slice + writer-context apply。DeploymentController 不持有 Zenoh Session，也不能让 reconcile 形成 local+wire 双路径。

> 后续架构细化（2026-07-29）：[Web Console、WebRTC、WebXR 与交互式 Gateway 边界研究](web-console-webrtc-webxr-gateway-boundaries.md)进一步确认 HTTP/SSE/WebSocket/WebRTC 只属于 Gateway 外部腿，WebXR 不是 Transport，浏览器不能形成第二个 Fabric backend；managed Gateway workload/exposure 与内部 typed endpoint 合同仍待 Proposed ADR。

> 后续裁决（2026-07-29）：[ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 已接受“Rust-first mechanisms，polyglot workloads”。首个 production Fabric 由 Rust 机制层通过 Zenoh 原生 Rust API 实现；Python/C++/模型/Agent/设备代码仍可作为 ProcessDomain、Service 或 Gateway 工作负载。该裁决不把 Fabric 放入 Kernel，也不改变 Message、Mailbox、PortBinding、Evidence、Security 与 Deployment 的 owner 边界。

## 一句话结论

ParaEGOX 应采用“窄 CardDefinition In/Out/Port 契约、Deck Link、`DeploymentPlan.bindings`、live `PortBinding`、目标 `Mailbox`、Zenoh-native Fabric CoreService、生态 Gateway”的单一 owner chain。Zenoh 是唯一生产 Fabric，覆盖同 Session、同主机和远端 route；不再建立与它并列的生产 `LocalBus`，更不允许同一 `BindingId` 同时 local/wire 双投递。`Message` 是被传递的契约值，`messaging` 是子系统，`Mailbox` 是目标侧唯一语义积压点，`PortBinding` 是安装后的运行连接；四者不互相改名。Receipt、TraceContext 和授权判定等稳定契约进入 Kernel，Evidence 存储、Tracing SDK、日志管线、网络会话、Intent 编排和安全状态服务留在 Kernel 之外。

## 1. 研究问题与成功标准

本研究回答以下问题：

1. 为什么 Kernel 需要 Mailbox，是否还应保留 Bus。
2. Zenoh 是 Kernel、CoreService 还是 Adapter。
3. `net` 是否应该从 Kernel 剥离，网络能力由谁拥有。
4. Evidence、Tracing、Log、Metric、Audit 如何区分。
5. Intent 和 Security 哪些部分可以进入 Kernel。
6. ParaEGOX 第一阶段应按什么依赖顺序实现并验证这些边界。

成功标准不是画出更多目录，而是形成可以被代码和测试执行的所有权规则：

- Kernel 在没有网络、文件系统、数据库、ROS2、Zenoh 和 OTel SDK 时可以导入和测试。
- Transport 回调只做固定成本、非阻塞的 header/key/size/epoch/cache 检查与 handoff，不直接完整 decode/validate、触发 CardInstance 私有实现回调或执行 Card invocation。
- pre-validation encoded frame 只进入有界、可观测的 Fabric ingress buffer；验证成功后才构造 Message 并准入 target Mailbox，两个阶段不混报 accepted。
- 物理 Command 的授权、期限、幂等和 Receipt 语义不由 Transport 暗中决定。
- Trace、Log 或 Metric 丢失不会被误报为 Evidence 完整。
- Zenoh 作为唯一生产 Fabric，同时 Kernel/Runtime 通过不启动 Zenoh 的确定性 `PortBinding` 测试 fixture 验证稳定契约；fixture 不进入生产路径。
- OpsService/TUI 只消费稳定 InspectionProtocol，不反向进入 Runtime 内部对象。

## 2. 范围、假设与非目标

### 2.1 范围

- ParaEGOX 当前 clean-slate 架构文档。
- PhanthyMotus 的全局事件队列、ROS2 bridge、Agent Core lifespan 和部署方式。
- EAGOS 的 Bus、Net、Runtime、Intent、Evidence、Log、Tracing 与 Security 实现经验。
- Zenoh、ROS2 QoS、OpenTelemetry、W3C Trace Context 和 NIST Zero Trust 的当前一手资料。

### 2.2 假设

- ParaEGOX 需要从单进程开发扩展到多进程、远端节点和多机器人。
- Zenoh 是 ParaEGOX 唯一生产 Fabric，可承担同 Session、同主机与跨主机路由；ROS2/DDS/MQTT 等是边界生态，不与 Zenoh 建立虚假的等价 Backend 关系。
- `Memory` 保留给 Agent/平台的 Memory 能力；不用 `MemoryPortBinding` 表示测试替身，也不用“生产/测试”建立两种公共 Binding 类型。
- Linux/systemd/容器负责真正的进程和节点级隔离，ParaEGOX Kernel 是稳定机制库，不是假装替代 Linux 的特权内核。
- Rust Kernel/Runtime/Fabric 机制由 Cargo workspace、`Cargo.lock` 与 `rust-toolchain.toml` 管理；Python SDK、worker、Agent/模型与治理工具由 `uv`、`pyproject.toml` 与 `uv.lock` 管理。两条依赖图分别验证，并以跨语言 conformance suite 汇合。

### 2.3 非目标

- 不在本研究中选择具体日志后端、Evidence 数据库或 OTel Collector 部署方式。
- 不冻结 Zenoh keyspace、Wire Schema 和所有 QoS 数值。
- 不设计 Agent Loop、Graph Engine、Memory 或 World Model。
- 不迁移或复制 EAGOS 源码、测试、配置和 ADR 正文。
- 不创建 Accepted ADR，也不声称任何目标结构已实现。

## 3. 证据与强度

### 3.1 本地证据

| 证据 | 类型 | 强度 | 对结论的影响 |
| --- | --- | --- | --- |
| PhanthyMotus `agent-core/src/event_bus.py` 使用模块级单队列和最近事件列表 | local | 高 | 单队列适合早期 Agent Loop，但没有按消息语义隔离背压和故障域 |
| PhanthyMotus `agent-core/src/start.py` 在一个 lifespan 中启动 ROS2、MCP、Scheduler、Channel 和 Agent Loop | local | 高 | 简单部署换来了生命周期和故障所有权聚合 |
| PhanthyMotus `agent-core/src/ros2_bridge.py` 从 ROS2 线程直接向 asyncio loop 提交协程 | local | 高 | Transport callback 与应用执行之间需要明确的有界 Mailbox |
| EAGOS 的 Runtime 与网络职责长期沉入 Kernel，代码规模和依赖方向显示 Kernel 已承担平台主体 | local | 高 | “Kernel” 名称边界失真，ParaEGOX 必须用 import 与 owner 规则限制增长 |
| EAGOS 的中心 Bus 同时拥有 Zenoh Session、本地订阅、线程、重连、查询、诊断、授权 hook 和 SHM | local | 高 | Bus 抽象、局部调度和具体传输不能继续由一个对象拥有 |
| EAGOS 网络聚合层反向依赖 OPS、Fleet、Console 等上层能力 | local | 高 | 泛化 Net 子系统已成为跨层聚合点 |
| EAGOS 的 Intent 编排总线持有完整 Runtime、路由、Future、状态、fallback 和领域 callback | local | 高 | Intent 是 Agent/Grounding 领域编排，不是 Kernel 原语 |
| EAGOS logger 拥有全局 `logging.Handler`、文件路径、有界队列和写线程 | local | 高 | Kernel 可以记录诊断，但不能拥有日志运行系统 |
| EAGOS Evidence 子系统位于 Kernel 外，Ledger 又持有索引和持久化 sink | local | 高 | Evidence 引用和 Receipt 可入 Kernel，Ledger/Replay/Publication 不入 |
| EAGOS 的纯策略判定值可以脱离状态服务独立测试 | local | 中高 | Security 的稳定判定原语可以留在 Kernel，状态与执行必须拆分 |

EAGOS 证据来自相邻受限工作区；公开文档只保留中立失败模式和行为需求，不披露或依赖其私有源码路径，也不复制其实现。

### 3.2 外部一手资料

| 来源 | 类型 | 强度 | 对结论的影响 |
| --- | --- | --- | --- |
| [Zenoh Abstractions](https://zenoh.io/docs/manual/abstractions/) | external | 高 | Zenoh 同时提供 pub/sub、query/queryable、storage、keyspace 和时间戳，不只是一个队列库 |
| [Zenoh Deployment](https://zenoh.io/docs/getting-started/deployment/) | external | 高 | router/client/peer、region 和 gateway 属于部署与数据面拓扑，不属于纯 Kernel |
| [Zenoh Access Control](https://zenoh.io/docs/manual/access-control/) | external | 高 | Transport ACL 受网络拓扑和 key expression 影响，不能替代应用级 Authority |
| [Zenoh 1.8 release notes](https://zenoh.io/blog/2026-03-18-zenoh-kiyohime/) | external | 中高 | Query/reply QoS 行为会随版本变化，厂商参数不应成为 Kernel 公共契约 |
| [Zenoh 1.9 Longwang](https://zenoh.io/blog/2026-04-16-zenoh-longwang/) | external | 高 | Regions、QUIC priority multistream、mixed reliability 和连接能力使 Zenoh 足以承担 ParaEGOX 主数据面，避免最小公分母式多 Backend |
| [zenoh-python 1.9 Channels and callbacks](https://zenoh-python.readthedocs.io/en/1.9.0/concepts.html#channels-and-callbacks) | external | 高 | FIFO 满时可阻塞 Zenoh 线程，Ring 满时丢最旧项，callback 不得做阻塞或 Zenoh 操作；因此 Zenoh 不能替代目标 Mailbox 和 Runtime dispatch |
| [Zenoh 1.9 same-session source path](https://docs.rs/zenoh/latest/src/zenoh/api/session.rs.html#2479-2552) | external | 中高 | 同 Session publication 可直接投递 local callback，降低为本地性能再建一套生产 Bus 的必要性；具体 Python 尾延迟仍需目标平台基准 |
| [zenoh-bridge-ros2dds](https://github.com/eclipse-zenoh/zenoh-plugin-ros2dds) | external | 高 | 官方 bridge 可以映射 ROS topic/service/action 和 ROS graph，但保留 DDS/CycloneDDS 部署与回路约束，适合作为生态边界 |
| [zenoh-plugin-dds](https://github.com/eclipse-zenoh/zenoh-plugin-dds) | external | 高 | Zenoh 官方另有面向非 ROS2 DDS 应用的透明 bridge，并明确建议 ROS2 使用 ros2dds；证明 DDS Gateway 与 ROS2Gateway 是两个按需边界 |
| [rmw_zenoh](https://github.com/ros2/rmw_zenoh) 与其 [Design](https://github.com/ros2/rmw_zenoh/blob/rolling/docs/design.md) | external | 高 | ROS2 可直接运行在 Zenoh 上，但其 key expression、CDR attachment、graph cache 和 QoS 是 RMW 私有映射；官方明确不保证普通 Zenoh 应用互操作，并且不与 ros2dds bridge 互操作 |
| [ROS2 QoS](https://docs.ros.org/en/humble/Concepts/Intermediate/About-Quality-of-Service-Settings.html) | external | 高 | history、depth、reliability、durability、deadline、lifespan 和 liveliness 是组合语义，端点不兼容时可能完全不通信 |
| [ROS2 RMW implementation guidance](https://docs.ros.org/en/ros2_documentation/rolling/Tutorials/Advanced/Creating-An-RMW-Implementation.html) | external | 中高 | 不同 RMW 对 QoS 的实现并不等价；ROS2Gateway 需要 FeatureReport/FeatureLoss |
| [OpenTelemetry Context](https://opentelemetry.io/docs/specs/otel/context/) 与 [Propagators](https://opentelemetry.io/docs/specs/otel/context/api-propagators/) | external | 高 | 执行上下文与跨传输 inject/extract 可以分离 |
| [OpenTelemetry Logs](https://opentelemetry.io/docs/specs/otel/logs/) | external | 高 | 现有 logging、LogRecord 处理和 exporter 是不同层，可通过 TraceId/SpanId 关联 |
| [W3C Trace Context](https://www.w3.org/TR/trace-context/) | external | 高 | `traceparent`/`tracestate` 提供跨实现的最小传播格式，也明确存在隐私与安全风险 |
| [NIST SP 800-207](https://csrc.nist.gov/pubs/sp/800/207/final) | external | 高 | 不能仅因本地网络位置或资产归属授予隐式信任；策略判定与执行点需要明确分离 |

### 3.3 推断与开放证据

- **inference**：生产路径首版应只实现 Zenoh-backed `PortBinding`；不安装 Zenoh 的 Kernel/Runtime 测试使用确定性 `PortBinding` fixture 直接向目标 Mailbox offer，它不是第二套 Bus。
- **inference**：若目标硬件证明 Zenoh same-session 对特定 ultra-hot binding 的 p99.9、CPU 或 copy 成本不可接受，Compiler 可在后续版本为该 binding 选择互斥的 in-process route；不得恢复 local-and-wire 双投递。
- **inference**：高带宽 Observation 最终应优先使用 Zenoh SHM；ROS2 loaned message 只在 ROS Gateway 的本地 DDS/RMW 侧考虑。Port 契约必须允许 payload reference，而不能假定所有消息复制进 Python Queue、Rust-owned buffer 或任一语言私有对象。
- **inference**：production Fabric 的基准和故障注入以 Zenoh 原生 Rust API 为 primary；`zenoh-python` 证据继续用于解释 callback 风险、Python workload/Gateway 兼容性和历史行为，不再定义生产 Fabric owner。
- **open**：目标机器人、Jetson、服务器和 Wi-Fi 环境下，Zenoh Rust primary 的延迟、抖动、重连和 SHM 表现，以及 Python/C++ workload 经 ProcessDomain/typed seam 接入后的端到端开销，尚未基准测试。
- **open**：首版 Evidence durable handoff 采用本地 WAL、SQLite 还是独立进程，尚无负载证据。
- **open**：Authority 与 Resource Lease 是同一 CoreService 还是两个服务，需要最小物理闭环验证。

## 4. 参考路径重建

### 4.1 PhanthyMotus

```text
ROS2 callback / MCP / Scheduler / User
                  │
                  ▼
          global asyncio.Queue
                  │
                  ▼
          single Agent Loop
                  │
        tool / ROS2 / channel side effect
```

它的优势是启动路径短、概念少、部署直观。主要风险是：

- 不同语义共享一个 `maxsize=1024` 队列；Signal、Command、用户输入没有不同溢出策略。
- 队列满时 producer 阻塞，Transport 线程或上游回调可能被间接拖住。
- ROS2 executor、asyncio、Web lifespan 和后台 task 的 owner 不完全一致。
- Agent Core、Web API、驱动注册、Channel 和调度器同进程，故障和关闭相互影响。
- `privileged`、host network 和 `/dev` 全量挂载适合快速硬件接入，不适合作为长期最小权限边界。

ParaEGOX 应保留它的声明式连接和易用性，不保留全局队列与单体生命周期。

### 4.2 EAGOS 失败模式的中立重建

```text
EAGOS Module / Runtime / Intent / OPS
             │
             ▼
    central Bus + local callbacks
             │
   Zenoh session / reconnect / SHM
             │
 node RPC / discovery / transfer / log stream
```

EAGOS 补齐了 QoS、Receipt、重连、诊断、权限和跨节点能力，但这些需求持续沉入同一 Kernel：

- `Runtime` 成为 model、memory、task、world、agent、fleet、security 和 evidence 的 Service Locator。
- `EAGOS Module` 基类通过 Runtime 自动获得 Bus、Logger、Config、生命周期、Probe 和 Tool 等能力。
- Bus 同时拥有本地同步回调、线程 lane、Zenoh callback、重连资源恢复和诊断锁。
- Net 同时拥有 router 进程、RPC、文件传输、日志流、Topology、Fleet 和 OPS 控制。
- Intent 状态机又建立第二套“Bus”，并直接依赖完整 Runtime。
- Log、Trace、Evidence、Audit 虽然用途不同，仍有多条相互投影和双写路径。

测试中出现的重连后 callback 恢复、诊断遍历竞态和跨订阅者顺序问题不是偶然 bug，而是所有权过宽后的必然复杂度。

## 5. Kernel 准入规则

一个候选能力只有同时满足大部分条件，才应进入 Kernel：

1. 语义稳定，预期跨多个 Runtime、Service、CardDefinition 和 Adapter 使用。
2. 可表达为不可变值、纯函数、有限状态机或有界内存机制。
3. 不打开网络、文件、数据库、设备、线程、进程和 Web 服务。
4. 不依赖 Zenoh、ROS2、OTel、数据库驱动、模型框架或平台专用 wheel。
5. 使用虚拟时钟和内存 fixture 即可确定性测试。
6. 失败是局部、显式、可表示的，不通过全局状态或后台重试隐藏。
7. 不包含 Agent、World、Memory、Graph、OPS 等快速演化领域语义。

归属公式如下：

```text
稳定语义 / 有界机制       → Kernel
进程内执行与副作用应用     → RuntimeHost
长期共享状态与平台能力     → CoreService
厂商、协议和 OS 集成       → Gateway / Adapter / Driver
Agent、机器人和产品语义    → Domain / CardDefinition
```

## 6. 消息面结论

### 6.1 `Message`、`messaging`、`Mailbox` 与 `PortBinding`

这四个名称表达不同层次，不能互相改名：

| 名称 | 含义 | 不是什么 |
| --- | --- | --- |
| `Message` | transport-neutral 的不可变逻辑 Envelope；发送侧只从通过 Schema/Port 校验的 payload 构造，接收侧只在 decode 与 Schema/principal/binding 准入成功后构造；带 `MessageId`、Causality、Deadline、Trace 等稳定元数据 | pre-validation wire frame、queue、Bus 或服务 |
| `messaging` | Kernel/Runtime 中容纳 Message、Port、Delivery、Mailbox 与 Binding handoff 的子系统/包名 | 一个运行对象 |
| `Mailbox` | 目标异步 admission boundary 的唯一语义 Message 积压点，拥有 items/bytes/age、ordering、freshness、overflow 和 enqueue result；只容纳已验证 Message | Message 的新名字，或 Transport 内部 buffer |
| `PortBinding` | RuntimeHost/Fabric 根据 active `RuntimePlanSlice.bindings` 安装的 live endpoint 关联，拥有 `BindingId` 作用域内的 `BindingEpoch` 和 observed route；DeploymentController/Inspection 再与 committed `DeploymentPlan.bindings` 对账 | CardDefinition/Deck 拓扑或某个具体 transport 的别名 |

`Memory` 保留给 Agent/平台的 Memory 能力。测试替身只称为“确定性 `PortBinding` test fixture”：代码内可以有 `FakePortBinding` 或类似 fixture 名，但不建立 `MemoryPortBinding`、`MemoryBus`、`ZenohBinding` 这些公共领域类型。实现需要区分时使用“Zenoh-backed `PortBinding`”或“`PortBinding` test fixture”这类形容表达。

### 6.2 Port、Link、计划与运行绑定的唯一 owner chain

CardDefinition 作者使用 `In[T]`/`Out[T]` 声明方向端口，但这些只是不可变 `PortSpec` 的作者侧表达，不保存 Topic、Publisher、queue、Session 或运行指标。

```text
CardDefinition In/Out → immutable PortSpec
                           │ Card 引用；Deck Link 连接
                           ▼
DeckCompiler → DeckLock {canonical DeckTopology}
                           │ + DeploymentProfile + immutable NodeFacts
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
CardInstance-scoped PortBinding (one active production route per BindingId/BindingEpoch)
   └── Zenoh-backed route: session-local / host-local / remote
                         │ fixed-cost callback handoff
                         ▼
              bounded Fabric ingress buffer (encoded frame; not a Mailbox)
                         │ decode / schema / principal / binding admission
                         ▼
                      Message ──offer──> bounded target Mailbox
                                                   │
                                                   ▼
                                 Domain Dispatcher → CardInstance 私有实现回调 / Card invocation

tests only: deterministic PortBinding fixture ──validated Message──> the same Mailbox contract
```

各层只能拥有自己的事实：

| 层 | 权威内容 | 明确不拥有 |
| --- | --- | --- |
| `CardDefinition.PortSpec` | name、direction、Schema/version、interaction、required/cardinality 和不可削弱硬约束 | Topic/key、Publisher、queue、Zenoh QoS 和运行状态 |
| Deck `Link/DeliveryProfile` | from/to、应用消息种类、deadline/freshness、ordering、overflow、ack、workload 与 routing/merge 意图 | codec、具体 Mailbox、Zenoh 参数、PID/线程 |
| `DeploymentPlan.bindings` | 稳定 BindingId、解析后的 endpoint/schema/codec、Zenoh route/locality、keyspace、Fabric ingress limits 和目标 admission boundary | live Session、当前 BindingEpoch、queue depth 和 effect 结果 |
| Runtime `PortBinding` | 实际 endpoint、当前 BindingEpoch、active route/session 与 desired/observed 差异 | 静默改写 CardDefinition、Deck 或 DeploymentPlan |
| Fabric ingress buffer | pre-validation encoded frame/reference 的短暂、有界 transport staging，及其 items/bytes/age/retained-byte/overflow 事实 | Message、Mailbox、应用 accepted 或第二份 Delivery backlog |
| Mailbox / ExecutionDomain | 目标 admission、积压、dispatch 和 Card invocation | 远端 effect 成功或跨系统 exactly-once |

首版每条解析后的静态 1:1 Link 对应一个稳定 `BindingId`。同一 `BindingId` 在一个 active `BindingEpoch`/部署 revision 内只能安装一条 route；禁止 local-and-wire 双投递，也禁止依赖 payload hash + 时间窗压制回声。一个 Out 面向多个 destination 是多个独立 `BindingId` 的显式 fan-out，不是同一 Input 的双路投递。Route 变更使用 revision-tagged `prepare → activate → drain → retire/rollback`；`activate` 原子切换新 Message/frame 的准入，旧 route 随即只排空已经准入的项，不在一个 active binding 中隐式 fallback。

`BindingEpoch` 只在所属 BindingId 内由 live PortBinding 安装、重装、重配或撤销时推进和比较；pure compile 不读取或递增 epoch，也不能跨 BindingId 比较大小。DeploymentPlanner 不新建与 DeploymentPlanCandidate 并列的 `BindingPlan`；DeploymentController 提交后的 immutable DeploymentPlan 是唯一权威 desired truth，避免连接、placement、revision 和执行形成两份 desired truth。

Kernel 定义 Message、PortSpec、Delivery、Mailbox、Deadline、Ordering 和最小 Sender/Receiver Protocol；RuntimeHost 拥有已安装 PortBinding 的本地侧、Mailbox、dispatch 和 ExecutionDomain；FabricService 拥有 Zenoh session、同 Session/同主机/远端 routing、discovery、reconnect、remote query，以及 encoded ingress frame 的有界 staging/validation。Zenoh callback 尚未完整 decode/validate 的 bytes/SHM reference 不是 Message，不能进入 target Mailbox；它只可进入 Fabric 私有 ingress buffer。Ingress worker 验证成功后才构造 Message 并 offer 到 target Mailbox，失败记录 ingress rejection。Mailbox 是每个目标异步 admission boundary 唯一的系统语义积压点，必须同时限制 items、bytes 和 max age；Fabric ingress buffer、Zenoh channel、IPC credit 和 executor 仍需独立可观测预算，但不能冒充第二份 ParaEGOX Delivery backlog 或应用 accepted。

Runtime dispatcher 可以内部维护多级 ready queue 来仲裁不同 priority class，但 `Lane` 不成为 CardDefinition、Card、Deck、Kernel 或公共 Runtime Schema；内部调度结构不拥有第二份 payload queue、线程、event loop、进程或生命周期。完整执行裁决见 [Runtime 执行模型、调度与恢复研究](execution-model-scheduling-and-recovery.md)，完整端口裁决见 [CardDefinition 输入输出、Port、Link 与运行绑定研究](card-definition-ports-links-and-bindings.md)。

### 6.3 Raw Fabric 例外按 CapabilityGrant 授权

普通 CardInstance 的私有实现默认只得到依据其 CardDefinition 与 active plan 编译的窄 PortBinding。CoreService、Driver 或 Gateway 也不会因为类别名称自动获得 raw Fabric：

- 只有 Fabric 实现拥有原生 Zenoh Session；RuntimeHost messaging 只拥有已编译 PortBinding、目标 Mailbox 和 dispatch 的窄安装/交接权限，不拥有第二套全局 Bus。
- Recorder、动态路由或协议适配若确有 wildcard/query/storage 等需求，必须获得 scope 指向 Fabric resource 的显式 `CapabilityGrant`，限定 operation、keyspace、Schema、principal/tenant、rate/bytes、期限和可撤销范围。
- Fabric-scoped CapabilityGrant 的授予、衰减、使用和撤销进入 Authority/Inspection/Audit；不能把普通代码改名成 Gateway 绕过检查。
- 动态 Fabric access Grant 不能反向修改 CardDefinition、DeckLock 或已编译静态 Link，也不能把任意 Topic 当作未声明 Port 注入 CardInstance。
- 这类动态流量在 Inspection 中必须标记为 dynamic/opaque，不能用来证明静态 DeckTopology 完整、required Port ready 或端到端交付成立；物理 Command 仍须经过完整 Authority/Lease/Safety/EnforcementPoint。

这允许 ParaEGOX 使用 Zenoh 的 query/queryable、liveliness、storage、SHM 和 Regions，而不把原生 Session 变成新的 Service Locator。

### 6.4 Handoff、准入与效果结果分阶段

一次发送至少可能经过以下不同 owner：

```text
source Message accepted
    ≠ fabric egress accepted
    ≠ encoded frame staged in Fabric ingress buffer
    ≠ Message validated and admitted to target Mailbox
    ≠ Card invocation accepted/completed
    ≠ physical effect succeeded
```

结构化 `SendResult`/`EnqueueResult` 只描述当前 owner 的 handoff；领域 `Receipt` 由有权判定的 invocation/effect owner 产生。Zenoh publish、encoded frame staged、target Mailbox admitted 与领域执行必须是不同 reason/stage，任一前段都不能冒充后一段。fan-out 必须返回 per-destination 结果或使用显式聚合规则；断连、进程 crash 或 timeout 后缺少权威终态时结果是 `uncertain`，不能透明 replay。

### 6.5 消息种类先于 Transport QoS

| 类型 | 核心语义 | 默认压力策略 | 不能由 Transport 暗中决定的内容 |
| --- | --- | --- | --- |
| `Signal` | 当前采样或参考值 | latest-wins 或显式 drop-oldest | freshness、sample time、最大陈旧度 |
| `Event` | 已发生的不可变事实 | 有界 FIFO；按声明持久化 | 领域顺序、重放边界、去重 |
| `Command` | 请求状态变化 | 满时显式拒绝，禁止静默 drop | Authority、deadline、idempotency、effect result |
| `Query` | 只读请求 | deadline + cancellation | 一致性级别、结果版本、部分结果 |
| `Receipt` | 对判定和执行的权威记录 | durable handoff 或 fail-closed | owner、causality、integrity、retention |

Transport 的 reliable/best-effort 不等于物理效果 exactly-once。ParaEGOX 首版不宣称端到端 exactly-once；物理 Command 通过 Idempotency Key、Authority Decision、执行端去重和阶段 Receipt 约束副作用。网络结果不确定时返回 `uncertain`，由权威查询恢复事实，不能透明重试。

In/Out 适合 Signal/Event 等单向数据流，但方向不是完整交互协议。Call/Query 必须保持 request/reply/cancel correlation，Operation 必须保持 goal/feedback/cancel/result/Receipt 的逻辑身份；Tool 是上层 Service/Permission/call contract，权威 State 由明确 owner 提供 snapshot/revision/watch 语义。它们以后可以编译为多条物理通道，但不能由几条互不相关的普通 Link 冒充一个原子交互。

### 6.6 QoS 分两步

```text
DeliveryProfile
        │ compile
        ▼
FabricFeatureReport + FabricMapping
        │ validate
        ▼
Zenoh concrete settings
```

Kernel 只表达 freshness、deadline、ordering、durability need、overflow、priority class 和 acknowledgement need。Deck Link 只能请求应用 priority，DeploymentPolicy 授权并保留容量，DeploymentPlanner 才把有效值写入 `DeploymentPlanCandidate.plan_content.bindings/execution`；DeploymentController 原子提交后形成同一 revision 的 committed DeploymentPlan，RuntimeHost 只消费对应 target Slice。CardDefinition 不声明有效的应用 priority，消息 producer 和外部 transport header 也不能自我升级。Zenoh Fabric 必须声明其实际支持能力；无法满足时在 plan compile/apply 阶段失败，而不是静默降级。ROS2/DDS Gateway 在边界上另做语义映射，不反向限制 ParaEGOX 原生 Fabric 的能力。

## 7. Zenoh-native Fabric 与 ROS2/DDS 边界

### 7.1 选择：不做多协议对等 Backend

ParaEGOX 选择 Zenoh 作为唯一生产 Fabric，但仍不把 Zenoh 放进 Kernel。同 Session、同主机跨进程、跨主机、跨机器人和云边协同都是 Zenoh-backed PortBinding 的 route locality，而不是多个公共 Binding 类型。按照 ADR-0006，首个生产 Fabric owner 使用 Zenoh 原生 Rust API；只有这个 Fabric owner 持有 production Zenoh Session，Python binding 保留给受控 workload/Gateway/兼容性场景，不能形成第二 Fabric。Kernel 的离线确定性由纯 Message/Port/Mailbox 契约和 `PortBinding` test fixture 保证；硬件 E-Stop、LocalSafety 与需证明 worst-case deadline 的路径属于独立原生安全执行边界，不依赖 Rust RuntimeHost、普通 Python worker、test fixture 或远端 Fabric。Rust 的内存安全和 Tokio 调度不构成 worst-case deadline 或功能安全证明。

这不是为了未来随意替换 Zenoh，而是为了防止 Zenoh Session、版本、线程、重连和部署状态污染稳定消息契约。FabricService 可以主动使用 Zenoh 特有能力，而不被“所有 Backend 都必须支持”的最小公分母接口限制，包括：

- pub/sub、query/queryable、liveliness、显式 storage adapter ServiceContract 和 advanced pub/sub。
- Zenoh 1.9 Regions 的机器人—站点—云拓扑。
- QUIC priority multistream，隔离不同优先级流的 head-of-line blocking。
- QUIC mixed reliability，在同一连接中组合可靠 stream 与 best-effort datagram。
- SHM、Fabric 自身的 connectivity inspection、matching listener 和 admin space。

因此不建立 `DDSFabric`、`ROS2Fabric` 或 `MQTTFabric`。ROS2/DDS/MQTT 等外部协议生态通过 Gateway 进入内部 typed seam，设备/仿真器/具体硬件 SDK 通过 Driver 接入，并显式报告能力损失；二者不能因共置而合并语义。Zenoh storage 只是可选的数据面能力，不拥有 Evidence、World 或 Memory retention；connectivity inspection 也只描述 Fabric 自身，不替代全系统 Inspection。

### 7.2 ROS2 与 DDS 不是同一适配问题

- DDS 是 ROS2 常用中间件和 wire/discovery 生态。
- ROS2 还包含 message type、node graph、service、action、parameter、lifecycle、TF、rosbag 和工具链语义。
- `zenoh-bridge-ros2dds` 能桥接 DDS 上的 ROS topic/service/action，并保留 ROS graph/tooling，但不能替 ParaEGOX 决定类型归一化、Authority、Resource、Command、Receipt 和 TF 所有权。

所以 ParaEGOX **不自己实现 DDS 传输**，但仍需要一个窄 `ROS2Gateway` 负责生态语义：

```text
existing ROS2 nodes
        │ DDS / ROS graph
        ▼
official zenoh-bridge-ros2dds
        │ bridged Zenoh keys + CDR payload
        ▼
ParaEGOX ROS2Gateway
        │ type / TF / action / authority mapping
        ▼
ParaEGOX native Ports and Zenoh keyspace
```

Gateway 只路由声明过的 topic/service/action，不能把整个 ROS graph 自动变成可信 ParaEGOX ProvidedService 或 CapabilityGrant。

### 7.3 两种 ROS2 部署模式不能混成一个默认路径

| 模式 | 用途 | 优势 | 主要限制 |
| --- | --- | --- | --- |
| DDS ROS2 + `zenoh-bridge-ros2dds` | 接入现有机器人、驱动和第三方 ROS2 系统 | 不改已有节点；ROS tooling 与 graph 保留 | 需管理 DDS discovery、bridge 路由和环路；主要验证 CycloneDDS |
| ROS2 + `rmw_zenoh` | 我方可控制的新 ROS2 节点，希望通信直接运行在 Zenoh | Zenoh P2P、SHM、buffer pool 和 ROS2 API 结合 | 使用 RMW 私有 key/CDR/attachment/graph 映射；普通 Zenoh app 互操作不在官方支持范围；不与 ros2dds bridge 互操作 |

这两个模式必须作为互斥 DeploymentProfile。不能在同一机器人上默认混用，再假设它们共享一个 ROS2/Zenoh graph。

对 ParaEGOX 首版的推荐是：

1. 原生 CardInstance、CoreService、OPS、Agent 和跨节点通信通过已编译 PortBinding 或显式 Fabric-scoped CapabilityGrant 使用 ParaEGOX Zenoh Fabric，不取得原生 Session。
2. 接入存量 ROS2/DDS 设备时使用官方 `zenoh-bridge-ros2dds`，外加窄 ROS2Gateway 做语义和权限转换。
3. `rmw_zenoh` 作为后续受控 DeploymentProfile 研究，不作为 ParaEGOX native keyspace 的直接替代，也不与 bridge DeploymentProfile 混用。
4. 不维护 ParaEGOX 自研 DDS client、discovery 或 QoS stack。

### 7.4 Bridge 的能力边界

Bridge 可以帮助 ParaEGOX 利用 Zenoh，但不是“零适配”：

- 它解决 DDS/ROS graph 到 Zenoh 的路由，不解决 ParaEGOX schema 和领域语义。
- DDS QoS 与 Zenoh 特性不完全等价，边界必须产生限定 FeatureReport。
- bridge 官方警告跨主机同时存在直接 DDS 与 bridge 路径会形成重复或环路，因此 DDS 应限制在机器人本地区域。
- plugin 构建要求与 zenohd 的 Zenoh/Rust 版本一致；首版更适合把 standalone bridge 作为由 OS service manager 独立托管的进程，而不是动态加载进 ParaEGOX router。
- 从 ROS2 进入的 Command 仍必须经过 ParaEGOX Authority Gate；ROS namespace、Domain ID 或 DDS participant 不是授权证明。

### 7.5 非 ROS2 的原生 DDS 是独立例外

如果真实设备或行业系统直接暴露 DDS/IDL，而不是 ROS2 graph，`zenoh-bridge-ros2dds` 不适用。Zenoh 官方提供独立 `zenoh-plugin-dds`/`zenoh-bridge-dds` 来透明路由 DDS 数据，并明确让 ROS2 场景使用 ros2dds。

ParaEGOX 对此采用“需求触发”策略：出现已确认的原生 DDS 设备、Topic/IDL、QoS 与验收场景后，再建立窄 `DDSGateway` 和独立 DeploymentProfile；优先复用官方 standalone bridge，不把 DDS Session、Discovery 或 QoS 实现成 ParaEGOX 的第二套 Fabric。没有这一证据前不建设通用 DDS 适配层。

### 7.6 不保留泛化 Net 聚合层

`net` 不是单一所有者，建议按职责拆分：

| 内容 | 所有者 |
| --- | --- |
| `NodeId`、`EndpointRef`、`PeerRef`、`ConnectionState` | 公共 contracts，仅在有跨层消费者时建立 |
| 本地进程通信和 control channel | RuntimeHost |
| routing、discovery、liveliness、session、reconnect | FabricService |
| Zenoh keyspace、publisher/subscriber/queryable、QoS、Regions 和 QUIC 映射 | Zenoh Fabric |
| zenohd 进程、配置和版本生命周期管理 | DeploymentController / OS service manager |
| mTLS、Zenoh ACL、peer authentication | Transport Security Adapter |
| ROS2 topic/service/action、type、TF 和 lifecycle 映射 | ROS2Gateway |
| DDS discovery 和 wire transport | 官方 ROS2 RMW / `zenoh-bridge-ros2dds`，ParaEGOX 不自研 |
| 文件/Artifact 传输 | Artifact/Transfer Service |
| 远程运维控制 | OpsService，经 typed owner client 提交 ControlRequest |
| Service placement 和工作负载连接期望 | `DeploymentPlan.bindings` / DeckTopology |

### 7.7 逻辑平面

首版至少区分以下逻辑平面，是否使用独立 Zenoh Session、端口或进程由后续基准决定：

- Safety/local control：不得依赖远端 Fabric 存活。
- Control/Command：可靠、有界、可授权、可回执。
- Observation/Data：高吞吐、允许按 freshness 丢弃。
- Ops/Telemetry：后台优先级，不得反压控制面。

## 8. Evidence、Tracing、Log、Metric 与 Audit

这些信号可以通过同一 Correlation Context 关联，但不能共享可靠性承诺：

| 信号 | 权威性 | 允许采样/丢失 | Kernel 内容 | Kernel 外所有者 |
| --- | --- | --- | --- | --- |
| Receipt | 权威 | 关键路径不允许静默丢失 | schema、owner、phase、causality、integrity ref | Runtime emitter + EvidenceService |
| Evidence | 权威 | 按策略持久化 | `EvidenceRef` 和最小 sink ServiceContract | EvidenceService、Ledger、Store、Replay |
| Trace | 非权威诊断 | 允许 | `TraceContext`/carrier contract | Runtime instrumentation、OTel SDK/Collector |
| Log | 非权威诊断 | 通常允许 | 无日志子系统；Kernel 可调用普通 logger | Runtime config、Observability pipeline |
| Metric | 聚合运行事实 | 允许短暂丢失 | 仅必要的 health/readiness value | Observability/OPS |
| Security Audit | 权威安全记录 | 由策略决定，关键判定不可丢 | Decision/AuditRef | Authority + EvidenceService |

### 8.1 TraceContext

Kernel 使用 W3C 兼容的最小值对象和 carrier protocol，不自己实现 Tracer：

- `trace_id`、`span_id`、`trace_flags`。
- 可选 `tracestate`，但必须限制大小并跨信任边界清洗。
- `run_id`、`deck_run_id`、`card_instance_id` 等 ParaEGOX correlation ref 独立于 OTel baggage。
- Fabric/Gateway 边界负责 inject/extract；Runtime instrumentation 负责 span 生命周期。
- Principal、CapabilityGrant、Token、原始 prompt、音视频内容不得自动进入 baggage。

### 8.2 Log

Kernel 代码可以使用标准 logging，但不得拥有：

- 全局日志配置和动态 level registry。
- 文件路径、rotation、写线程和网络 `logging.Handler`。
- OTel exporter 或 Zenoh log stream。
- 将 log 成功写出作为 Command/Evidence 成功的条件。

Runtime 在 composition root 配置 logging 和 TraceId/SpanId 关联；Observability Service/Collector 处理输出；OpsService 接受授权后的 ControlRequest，并把实际动作交给对应领域 owner。

### 8.3 Evidence

Kernel 只定义 Receipt/Evidence 引用和 emission result。EvidenceService 拥有 durable handoff、WAL/数据库、索引、retention、redaction、query 和 replay。

物理 Command 的关键阶段必须定义 Evidence 不可用时的行为。首选原则是：无法记录授权和执行事实时，对新的高风险写操作 fail-closed；低风险或恢复动作是否允许进入降级模式由独立 Safety/Authority ADR 决定。

## 9. Intent 与 Grounding

Intent 不进入 Kernel。它属于 Agent/Grounding/Orchestration：

```text
User / Agent Intent
        │
        ▼
Grounding / Planner / Graph
        │
        ▼
Command / Query / PlanStep
        │
        ▼
Admission + Authority + Runtime
```

原因：

- Intent 的 taxonomy、fallback、planner 和模型路由会快速变化。
- 不同 Agent 或应用可能完全不使用 Intent。
- Kernel 已有 Command、Query、Decision 和 Receipt 足以承载执行边界。
- 创建 `IntentBus` 会建立第二套生命周期、路由和状态流。

共享、长期运行的 Grounding 可以成为 CoreService；单个应用的 Intent 处理逻辑可以是 CardDefinition/Card。二者都复用统一消息面。

## 10. Security 分层

Security 是纵向约束，不是一个万能 `kernel/security` 包。

### 10.1 Kernel Authority 原语

- `PrincipalRef`、`IdentityRef`。
- `CapabilityGrant`、`CapabilityScope`、期限和委托链。
- `AuthorizationContext`。
- `PolicyOutcome` 与不可变 `PolicyDecision`。
- 权限衰减、scope 交集、期限判断等纯函数。
- digest/signature/key reference 和 signer/verifier Protocol；不自制密码算法。

### 10.2 Authority CoreService

- 身份解析、策略集和版本。
- Grant 签发、撤销和缓存失效。
- Credential/Secret 生命周期与 trust anchor。
- 高风险 approval workflow。
- 安全审计 Receipt。

### 10.3 Runtime Enforcement

- Side effect 前的 Policy Enforcement Point。
- 进程身份、CapabilityGrant attenuation、资源限制。
- seccomp/cgroup/container profile 的应用与失败报告。
- Driver 和 Actuator binding 的最小权限。

### 10.4 Transport Security

- mTLS、Zenoh peer authentication 和 ACL。
- 入站 Principal/Node assertion 到 ParaEGOX IdentityRef 的映射。
- 跨信任边界清理 tracing baggage 和非必要 metadata。

本地网络、同一主机或同一 Deck 不构成隐式信任。Transport ACL 只是一层 enforcement，不能替代 Command Authority。

## 11. 其他容易误入 Kernel 的能力

| 能力 | Kernel 中允许 | Kernel 外 |
| --- | --- | --- |
| Health/Readiness | 值对象、状态机和原因码 | probe 执行、聚合、告警、Dashboard |
| Discovery | Service/Endpoint 引用契约 | scanning、heartbeat、liveliness 和 registry 状态 |
| Codec/Schema | schema id、content type、兼容性结果 | msgpack/protobuf/JSON 实现和 schema registry 服务 |
| Clock | monotonic/wall 接口、deadline/freshness 计算 | NTP/PTP/chrony、跨节点健康与校准服务 |
| Graph | 当前不建包；至少两个独立生产消费者证明相同需求后，才经 ADR 抽取无状态 immutable directed-multigraph view、SCC/cycle witness 与对已验证 DAG 的纯拓扑算法 | 领域 Graph Schema/identity/digest/store/query、Agent graph、Behavior Tree、workflow/runtime assembly engine、调度与失败策略 |
| Config | 组件自己的 typed value | 全局配置 loader、env、文件监听、远程配置 |
| E-Stop | stop state/ref 和不可绕过规则 | 独立硬件 safety island、watchdog、设备安全输出；SafetyIslandAdapter 只接入状态与证据 |
| TF/Geometry | 独立基础库，按消费者决定 | 不因机器人常用就自动进入 Kernel |
| Serialization | Envelope 边界和 codec protocol | 具体库、压缩、SHM buffer 管理 |

## 12. 方案比较

### A. Zenoh-native Kernel

Kernel 直接暴露 Zenoh Session、key expression 和 QoS。

- 优点：代码路径短，能快速利用 Zenoh 全能力。
- 缺点：测试、部署、版本和安全边界与 Zenoh 绑定；Local delivery 与远端 transport 难分；重演 EAGOS 聚合。
- 适用反例：产品被明确限定为不可扩展的 Zenoh appliance，所有进程始终依赖同一协议。

### B. 大而全 Bus Protocol，多种等价 Backend

定义 publish/subscribe/query/storage/discovery/security 的统一 Bus 接口。

- 优点：表面上可替换。
- 缺点：不同 transport 能力并不等价，接口最终变成最小公分母或大量 feature flag；容易制造虚假可移植性。

### C. Kernel Message/messaging + PortBinding/Mailbox + Zenoh-native Fabric + 生态 Gateway

- 优点：稳定语义与具体 Session 解耦；测试 fixture 可不启动 Zenoh 直接验证同一 Mailbox 契约；生产 Fabric 能直接使用 Zenoh same-session、Regions、query、liveliness、SHM 和多优先级链路，不受多 Backend 最小公分母限制。
- 成本：多一层 binding/compile；项目明确承担 Zenoh 版本、keyspace 和部署治理责任，生态协议必须经过 Gateway。
- 结论：**推荐**。

### D. 全部交给 ROS2/DDS

- 优点：机器人生态成熟，QoS 和工具完整。
- 缺点：Agent/Cloud/OPS/Evidence/多语言服务不都适合 ROS2；RMW 能力差异依然存在；无法消除 ParaEGOX 自己的 Command/Authority 语义。
- 结论：ROS2 是重要 Gateway 生态，不是全系统唯一消息语义，也不是与 Zenoh 并列的 Fabric Backend；Driver 只负责设备/仿真器/具体硬件 SDK 边界。

### E. 只部署 ROS2 bridge，不建设 ROS2Gateway

- 优点：首期代码最少，现有 ROS2 topic/service/action 很快可跨 Zenoh 互通。
- 缺点：桥接后的 ROS graph、CDR payload 和 namespace 会直接渗入原生 keyspace；TF、lifecycle、action、Authority、Command 和 Receipt 没有明确 owner。
- 结论：只适合临时连通性实验，不作为 ParaEGOX 产品边界。

## 13. 推荐目标结构

```text
paraegox/
├── kernel/
│   ├── contracts/       # ID、Message、Causality、Receipt、Health
│   ├── time/            # Clock、Deadline、Freshness
│   ├── lifecycle/       # 纯状态机
│   ├── messaging/       # Port、Mailbox、Delivery、Ordering
│   ├── admission/       # 纯准入结果
│   ├── authority/       # Principal、CapabilityGrant、PolicyDecision
│   └── resources/       # ResourceRef、Lease 与纯冲突判断
├── runtime/
│   ├── host/
│   ├── lifecycle/        # RuntimeOwnershipTree / structured scope ownership
│   ├── execution/
│   ├── messaging/       # PortBinding 安装、Mailbox handoff
│   ├── liveness/        # LivenessSpec/State monitoring；不等于 Health/Readiness
│   ├── recovery/        # RecoveryPolicy/Engine；RuntimeHost 仍是唯一 action owner
│   ├── enforcement/
│   ├── inspection/
│   └── telemetry/       # instrumentation，不含 exporter 产品状态
├── services/
│   ├── fabric/         # Zenoh-native FabricService、bounded ingress/validation 与受控能力接口
│   ├── authority/
│   ├── evidence/
│   ├── observability/
│   ├── resources/
│   └── ops/
├── adapters/
│   ├── otel/
│   └── logging/
├── gateways/
│   └── ros2/           # ROS graph/type/TF/action/authority 语义边界
├── drivers/
├── cards/
├── decks/
└── deployment/
```

这是一张所有权图，不要求第一批提交创建所有空目录。

它也不是语言包地图。首个生产参考中，Kernel、RuntimeHost、本地执行/进程治理和 Fabric 是 Cargo workspace 中的 Rust mechanisms；Python/C++/模型/Agent/设备实现通过版本化 ProcessDomain、独立 Service 或 Gateway 接入，公共合同保持 language-neutral。受信任、同版本、静态链接的 Rust 实现才可在明确准入后 in-process；未知代码、Python 和 C++ 默认不取得 RuntimeHost 内部对象、Mailbox owner、restart/readiness owner 或 raw Zenoh Session。`unsafe`/FFI 只能位于窄 adapter，PyO3/maturin 只用于经测量的局部优化，不能成为默认 Runtime ABI。

## 14. 风险、反例与失效条件

### 14.1 主要风险

- 抽象过早：在没有第二个 transport 前设计过宽 Fabric Protocol。
- 重新引入生产 LocalBus，或让同一 BindingId 同时激活 local/wire route，导致双投递、顺序和完成语义漂移。
- 按 CoreService/Driver/Gateway 类别自动授予 raw Bus/Fabric，促使组件通过改名绕过最小权限。
- 把 BindingEpoch 当全局序号或由 pure compile 递增，导致不同 BindingId 的迟到消息比较失真。
- 把 pre-validation encoded frame 冒充 Message 塞进 target Mailbox，或把 Fabric ingress buffer 变成未计量的第二 Delivery backlog。
- 把 `.send()`、Zenoh publish 或 remote admission 误报为 Card invocation/physical effect 成功。
- 让 PortSpec 吞入 Call、Operation、Tool 和 State 的全部语义，重新形成万能消息接口。
- Zenoh 版本、keyspace 或部署能力变化成为平台集中风险。
- 在同一机器人误混 `rmw_zenoh` 与 `zenoh-bridge-ros2dds`，造成不可互操作的 ROS graph 或重复路径。
- 为追求“自动接入”而把完整 ROS graph 直接暴露为可信 ProvidedService/CapabilityGrant。
- 高带宽数据被统一 Envelope、跨语言 copy 或 Python validation 拖慢。
- Evidence fail-closed 造成机器人在存储故障时不可用。
- Trace baggage 泄露身份、prompt 或环境信息。
- Authority Service 不可用时，缓存策略被错误地 default-allow。
- OPS 为了方便重新绕过 Inspection/Authority 直接操作 Runtime。

### 14.2 反例与应对

- 若目标平台基准证明 Zenoh same-session 无法满足某个 ultra-hot binding 的尾延迟、CPU 或 copy 预算，可以研究一条 Runtime 内部 in-process route；它必须与 Zenoh route 对同一 BindingId 互斥、经过同一 Message/Envelope/Schema/Mailbox conformance，且不得传递 Python/Rust 语言私有对象别名、裸指针或未受 lifetime 约束的 buffer handle。
- 若单进程最小设备资源极紧，可以把 FabricService embedded placement 启动；逻辑 owner 不因此合并进 Kernel。
- 若某个受控 ROS2 工作负载确实适合 `rmw_zenoh`，应建立独立 DeploymentProfile，并用 ROS2Gateway 保持 ParaEGOX 语义边界；不能同时开启 ros2dds bridge 假定自动互通。
- 若 Evidence durable handoff 延迟无法接受，可以采用本地预分配 WAL 或双阶段 Receipt；不能退化为“写一条 log 就算证据”。
- 若 OTel SDK 开销过高，Kernel TraceContext 仍保持兼容，Runtime 可以切换为 no-op 或轻量 exporter。

### 14.3 会推翻推荐方案的证据

- 产品范围正式冻结为单一 Zenoh appliance，明确不支持离线 Kernel 测试、其他 transport、独立安全路径或协议演进。
- 实测表明分层引入不可接受且无法优化的控制延迟，并且直接耦合 Zenoh 能给出可证明的安全与维护收益。
- 后续验证证明同一 Port/Delivery/Mailbox 语义无法覆盖 Zenoh 三种 locality，或确定性 fixture 无法对该契约提供有效测试证据，需要重新限定契约范围。ROS2Gateway 的损失映射失败不应反向扩大 Kernel 契约。

目前没有这些证据。

## 15. ADR 影响

研究建议后续分别建立 Proposed ADR，而不是用一个总 ADR 冻结全部细节：

1. Kernel 准入规则与依赖红线。
2. CardDefinition In/Out/PortSpec、Deck Link、`DeploymentPlan.bindings` 与 live PortBinding 的 owner chain。
3. 稳定 BindingId、BindingId-scoped BindingEpoch、pure compile 与 runtime install/reinstall 边界。
4. Message/messaging/Mailbox/PortBinding 语义、single-active route 不变量与分阶段 SendResult/Receipt。
5. ExecutionRequirements、DeliveryProfile、`DeploymentPlan.bindings/execution` 与 Runtime dispatch/Domain 边界。
6. Fabric-scoped CapabilityGrant、Zenoh-native Fabric Service、keyspace/版本治理与 ROS2/DDS Gateway 边界。
7. Receipt/Evidence 与 Trace/Log/Metric 分离。
8. Authority Decision、Enforcement Point 与 Transport Security 分层。
9. 物理 Command、Idempotency 和 `uncertain` 恢复语义。

ADR-0006 已经冻结 Rust-first mechanisms、polyglot workload、Cargo/`uv` 工程 authority 与 Zenoh Rust production primary；以上 Proposed ADR 只能细化消息、Fabric、Evidence 和 Security 不变量，不能重新开放语言总边界，也不能借 Rust 实现合并既有 owner。

ADR 应冻结不变量，不冻结首版目录名、Zenoh 具体参数或数据库选择。

## 16. 推荐方向与置信度

**Verdict：revise。** 修订现有架构基线中把 LocalBus 与 Zenoh 写成并列生产路径的部分，并冻结 `Message` 契约、`messaging` 子系统、`Mailbox` 唯一语义积压点、`PortBinding` 唯一公共 live binding 和 Zenoh 唯一生产 Fabric 的分层；Mailbox 没有改名为 Message，Message 也不是 Mailbox。首个 production Fabric 使用原生 Rust Zenoh API，但 Zenoh 仍在 Kernel 外，跨语言工作负载仍只通过版本化合同接入。继续保持“小 Kernel”方向。

置信度：

- Kernel/Runtime/Service/Gateway/Driver 分层：高。
- In/Out/Port → Link → DeploymentPlan.bindings → PortBinding owner chain：高。
- raw Fabric 按 Fabric-scoped CapabilityGrant 而非组件类别授权：高。
- Intent 不入 Kernel、Evidence Store 不入 Kernel、Log runtime 不入 Kernel：高。
- Zenoh 作为唯一生产 Fabric，覆盖 session-local/host-local/remote：高，具体 QoS、SHM 和尾延迟门槛仍需目标平台基准和版本治理原型。
- `PortBinding` test fixture 与 Zenoh-backed production route 共用 conformance：中高，需在 P2/P4 用故障 Harness 证明。

残余风险集中在性能、目标平台支持矩阵、Evidence 故障策略和 Authority/Resource 服务粒度，不阻塞 Kernel P0–P2。

## 17. 后续入口

- 总体架构：[Kernel、RuntimeHost 与 Core Services](../architecture/kernel-runtime-core-services.md)
- CardDefinition 与端口专项研究：[CardDefinition 输入输出、Port、Link 与运行绑定](card-definition-ports-links-and-bindings.md)
- 执行模型研究：[Runtime 执行模型、调度与恢复](execution-model-scheduling-and-recovery.md)
- 分布式作用域研究：[分布式身份、作用域与所有权](distributed-identity-scope-and-ownership.md)
- 实施顺序：[Kernel Foundation 实施计划](../plans/kernel-foundation.md)
- 正式决策入口：[ADR 目录](../adr/README.md)
