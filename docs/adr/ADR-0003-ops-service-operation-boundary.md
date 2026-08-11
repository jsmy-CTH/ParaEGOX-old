# ADR-0003 — OPS、OpsService 与 Inspection 操作边界

> 状态：Proposed
> 日期：2026-07-29
> 决策者：ParaEGOX maintainers
> 关联文档：[ADR-0001 — DeploymentController、DeploymentPlan 与 Runtime 边界](ADR-0001-deployment-controller-boundary.md)、[Kernel、RuntimeHost 与 Core Services](../architecture/kernel-runtime-core-services.md)、[分布式系统模型](../architecture/distributed-system-model.md)、[Kernel Foundation Plan](../plans/kernel-foundation.md)、[Web Console、WebRTC、WebXR 与交互式 Gateway 边界](../research/web-console-webrtc-webxr-gateway-boundaries.md)

## 一句话结论

`OPS` 是 ParaEGOX logical control plane 内的运维领域与产品能力总称，不是一张 Card、每 Node 固定进程或新的独立 plane；其后端运行组件 `OpsService` 是长期共享的 CoreService，只拥有受权运维请求自身的状态机与 journal，不拥有被操作对象的 desired state、observed facts、权限、生命周期或副作用结果。`InspectionService` 保持只读投影边界，ConsoleGateway 保持外部协议边界，真正动作始终由 DeploymentController、NodeManagement、Artifact/Release、Authority/Resource/Safety 或其他领域 owner 执行。

## 背景

运维系统天然需要同时查看状态、发起变更、跟踪进度、解释失败并关联证据。如果把这些需求收进一个“万能 OPS”，它会逐渐同时持有远程传输、任意脚本、安装器、Runtime 生命周期、Deployment desired state、权限、日志和数据库，最终成为新的全局 Runtime 与第二个控制面写者。

另一方面，完全取消统一的 ControlRequest boundary，也会让 CLI、TUI、Web Console、自动化和 runbook 分别实现鉴权、幂等、取消、超时和回执，产生多个行为不一致的运维入口。ParaEGOX 因此保留统一的 `OpsService`，但把它限制为 ControlRequest coordinator，而不是系统状态 owner 或通用执行器。

## 范围与非目标

本 ADR 决定：

- `OPS`、`OpsService`、`OpsClient`、Inspection 与 ConsoleGateway 的组件身份和依赖方向；
- OpsService 唯一拥有的状态、允许的操作协议和 Receipt 边界；
- OpsService 与 DeploymentController、Authority、Evidence、NodeDaemon/NodeManagementEndpoint 及领域 owner 的关系；
- 默认部署粒度、失效行为、bootstrap/rescue 路径和禁止的旁路；
- P6b/P7 前必须具备的最小验证证据。

本 ADR 不决定：

- OpsService journal 的最终数据库、HA 或多副本协议；
- public API 的最终 transport、字段编码和 Web 路由；
- TUI/Web Console 的页面、命令布局或产品交互；
- 各领域 action 的业务参数、Deployment rollout 算法或物理控制策略；
- 通用分布式 workflow/saga engine；除非未来出现多个真实、持久、跨 owner 的工作流消费者并另立 ADR，否则不建设。

## 决策

### 1. 名称与组件身份

| 名称 | 限定含义 |
| --- | --- |
| `OPS` | logical control plane 内的运维领域、产品能力和公开协议集合；不是单个运行对象，也不建立新的全局 plane taxonomy |
| `OpsService` | 实现受权 OpsProtocol/ControlRequest、幂等 journal、进度和终态回执的 CoreService |
| `OpsClient` | CLI、TUI、ConsoleGateway 或自动化使用的公开协议客户端；不内嵌 OpsService 实现或领域 executor |
| `Inspection` | 对各真实 owner 所产事实的只读 snapshot/watch/projection 协议与能力；聚合不转移事实所有权 |
| `InspectionService` | 提供 node-local 或 federated Inspection projection 的 CoreService；只拥有 projection revision/cursor/cache/freshness |
| `ConsoleGateway` | HTTPS/SSE/WS 等外部客户端协议、身份映射、限流与 bounded cache 边界；不托管 OPS 真相或领域状态 |

`OpsController`、`ControlPlaneService`、`OperatorService` 和 `ConsoleService` 不作为替代名：它们分别会混淆 desired-state controller、扩大职责范围、排除自动化主体或把 UI transport 误当运维 owner。公共文档可以使用 `OPS` 表示领域，具体类型、协议和客户端必须分别写作 `OpsService`、`OpsProtocol` 与 `OpsClient`。

### 1.1 与 EAGOS OPS 的关系

`DeploymentController` 不是 EAGOS OPS 的改名版本，ParaEGOX 的 `OpsService` 也不是对 `eagos/ops/` 包边界的继承。EAGOS 的 OPS surface 曾同时覆盖 system context、Bundle/Plugin lifecycle、部署与回滚、资产和依赖安装、Runtime/Module 控制、debug/incident、sandbox/remediation、graph execution 以及 operator tooling；这些能力说明真实运维需求存在，但把它们放在同一个代码命名空间会弱化 desired state、执行副作用、观测投影与客户端入口之间的 owner 边界。

ParaEGOX 只提取行为需求、故障教训和可验证不变量，不复制 EAGOS 的包结构、Schema、实现或 `Module`/`Bundle` 语义。迁移映射固定为：

| EAGOS OPS 中混合出现的职责 | ParaEGOX owner | 关系 |
| --- | --- | --- |
| 状态、why、debug、system context、incident view | InspectionService + EvidenceService/Telemetry | 只读投影和持久证据；不进入 OpsService 私有查询 |
| 运维请求、审批、进度、取消、终态汇总 | OpsService | 只拥有 ControlRequest/operation record |
| deploy、rollback、rollout、desired/observed reconcile | DeploymentController | 从 OPS surface 中抽出的唯一 deployment authority，不是 OpsService 子组件 |
| Node/Runtime 维护与本机诊断动作 | NodeManagementEndpoint / RuntimeHost 的公开管理契约 | OpsService 只调用，不直接操作进程或 Runtime 私有对象 |
| Artifact、Release、依赖与资产物化 | Artifact/Release/Transfer owner | 不在 OpsService 中保留安装器、包管理器或 active pointer |
| Console/HTTP/WebSocket bridge | ConsoleGateway + OpsClient/InspectionClient | 只负责外部协议和 presentation boundary |
| Agent remediation、sandbox tool 与通用 graph execution | Agent/Tool/Scenario 等未来专属 owner，或明确不保留 | 不把 Agent 或 workflow engine 藏回 OpsService |

因此二者不是 `EAGOS OPS → DeploymentController` 的一对一迁移，而是一次职责拆分：DeploymentController 获得 deployment desired-state 单写权，OpsService 获得 operation-record 单写权，InspectionService 只拥有其限定 scope 的 projection state，各 action owner 保留实际副作用与领域 Receipt。三者可以协作，不能相互代替。

### 2. 读取与写入链路分开

```text
CLI / TUI ───────────────────────────────┐
                                         ├──> InspectionClient ──> InspectionService projection
Web Console ──> ConsoleGateway ──────────┤
                                         └──> OpsClient ──> OpsService
                                                                  │
                                                                  ▼
                                                               Authority
                                                                  │
                                     ┌────────────────────────────┼───────────────────────────┐
                                     ▼                            ▼                           ▼
                           DeploymentController       NodeManagementEndpoint       other typed owner API
                                     │                            │                           │
                                     └──────────── owner Receipt / EvidenceRef ──────────────┘
                                                                  │
                                                                  ▼
                                                    Ops progress / terminal OpsReceipt
```

- 读取路径只查询 InspectionService 的 InspectionProtocol snapshot/watch；每份投影必须携带 source owner、revision/epoch、`observed_at`、freshness 与 `stale/unknown/partitioned` 状态。
- 写入路径只通过 OpsService 接受受权 `ControlRequest`。OpsService 调用真实 owner 的 typed API，不直接 import 其私有实现。
- ConsoleGateway 只是外部 BFF/Gateway。TUI 和受信本地 CLI 可以直接使用 OpsClient/InspectionClient；这种 transport 差异不改变权限和 owner。
- 连续 WebRTC media、XR input、teleoperation sample 与高频控制流不经过 OpsService；它们进入各自 typed Gateway/Command boundary，并继续经过 Authority、Lease/Fencing、Safety 和本地 Enforcement。

### 3. OpsService 只拥有 ControlRequest 自身

OpsService 的最小公开能力为 `preview/submit/get/watch/cancel-if-supported/reconcile-uncertain`。它拥有并持久化：

- `ControlRequestId` 与 canonical request digest；
- caller principal、target/action、expected revision/epoch、deadline、dry-run 与 approval/authority references；
- request admission、ordered progress、cancel/compensation/reconcile intent；
- owner request/Receipt/Evidence references；
- 一个且只有一个 terminal `OpsReceipt`。

同一 `ControlRequestId + digest` 重试只能返回或推进同一 operation；相同 ID 携带不同 digest 必须拒绝为 conflict。deadline、断连或 transport timeout 不能推断底层动作未发生，operation 必须进入可查询的 `Uncertain`，先向真实 owner 查询或 reconcile，不透明 replay。

`OpsReceipt` 只总结运维请求生命周期并引用 owner-issued Receipt。它不能重写 Deployment、Runtime apply、Artifact、Node maintenance 或物理 EffectReceipt 的结果；证据不足时只能报告 `Uncertain/Unknown`，不能用日志、probe、进程退出码或 transport ACK 推断成功。

### 4. 动作路由与真实 owner

| ControlRequest family | OpsService 调用的 owner | OpsService 不得做的事 |
| --- | --- | --- |
| deploy、rollback、reconcile | DeploymentController | 写 DeploymentPlan store、生成 RuntimePlanSlice、直接调用 RuntimeApplyEndpoint 或复制 reconcile loop |
| Node drain、维护、诊断采集 | NodeManagementEndpoint | 直接 kill PID、改 Node 状态或拼 raw Fabric key |
| Artifact/Release | Artifact/Release owner | 保存 artifact bytes、执行包管理、改 active pointer 或隐式 rsync/SSH |
| Secret rotation/access | Secret owner 的 capability-bound client | 在 journal、环境快照或 process global 中保存明文 Secret |
| 物理/不可逆操作 | typed CommandEndpoint，经 Authority、Resource、Safety 与 Enforcement | 把 UI approval 当 CapabilityGrant、跳过 lease/fencing/safety 或把 accepted 当 effect success |
| query/status/why | InspectionService 与 Evidence references | 回写 observed facts、把 projection 命名为 desired truth 或把 Evidence 降为日志 |

Deployment-specific rollout、partial apply、rollback 和 reconciliation 完全属于 DeploymentController。OpsService 只提交带 expected revision 的 ControlRequest、观察进度并关联最终 Receipt。即使 OpsService、TUI 和 Console 全部不可用，DeploymentController 仍必须继续 reconcile。

### 5. Inspection、Evidence 与 Authority 保持独立

- RuntimeHost、NodeDaemon、FabricService、Driver 与各领域服务继续产生自身 observed facts；Inspection 只读聚合，不成为 registry、Deployment、lease 或 health truth owner。
- node-local/federated InspectionService role 可以与 OpsService 共进程，但 ServiceContract、持久状态、权限、budget、failure 和 Receipt owner 必须分离。后续拆进程不改变协议语义。
- EvidenceService 保存 owner-issued durable Receipt/Evidence；OpsService journal 保存 operation orchestration state，并只引用 EvidenceRef。
- Authority 决定某 principal 是否可以请求目标 operation，并把 decision 绑定 request digest、target/resource、期限和 approval。OpsService 不签发 Grant，不持有 Authority signing key；实际副作用 owner 仍必须在本地执行最终校验。

### 6. 部署粒度、bootstrap 与失效

ParaEGOX 不建立“每个 Node 一个 OPS”的不变量。Node 侧的基础运行角色是 NodeDaemon 与一个或多个 RuntimeHost；P6a 增加 node-local Inspection/Evidence producer，但不增加 node-local OpsService。OpsService 按实际管理范围部署，首个分布式 reference profile 使用一个独立管理侧 OpsService 服务多个 Node。

OpsService 作为普通 CoreService 可以由 DeploymentController 通过 ServiceSpec/committed plan 管理，并放置在独立 management RuntimeHost；constrained/local profile 可以与其他服务共置，但这只是 placement，不产生多个并列 operation owner。未来若运行多个 OpsService，必须先冻结 request routing、journal ownership、idempotency/fencing 和 terminal Receipt 唯一性，不能让多个实例同时接受同一 operation。

OpsService 故障只影响新运维请求、查询聚合和在途 operation 的协调可见性，不得停止 RuntimeHost、Deployment reconciliation、本地 Continuity 或 Safety。已经 accepted 的 operation 不依赖 CLI、TUI 或 ConsoleGateway 进程继续存活。

P2e 保留经过认证和审计的窄开发/救援 CLI，可直接向 DeploymentController 提交 intent、触发 reconcile 或查询状态；它绕过的是尚未实现或不可用的 OpsService，不绕过 DeploymentController、writer tenure、Authority 或 Runtime apply fencing。生产中不得提供 CLI/SSH 直写 RuntimeHost、Deployment store 或领域数据库的隐式 fallback。

### 7. 明确禁止的膨胀路径

OpsService 不得：

- 成为容纳所有领域命令和实现代码的全局 command namespace；
- 持有 raw Zenoh/Fabric session、拼接内部 key expression 或发送未公开的领域 DTO；
- 内置任意 shell、SSH、sudo、rsync、package manager、container/systemd executor 或子进程 registry；
- 直接安装 Artifact、启动/停止 Runtime 实例、修改 active pointer 或领域数据库；
- 隐藏一个通用 DAG/saga/workflow engine，并让各领域操作退化为其 graph node；
- 保存明文凭证、process-global 密码或把 Secret 写入 operation journal；
- 把 Inspection projection、cache、log、probe 或聚合 Receipt 提升为领域 desired/observed truth；
- 仅靠进程内 idempotency，或在 timeout、重启、writer turnover 后透明重放副作用；
- 因共置、来自受信 Console 或拥有一个 CapabilityGrant 就绕过实际 owner 的本地准入。

若未来某个跨 owner runbook 确实需要持久协调，只能建立明确版本、有限步骤、显式补偿和 owner Receipt 的专用 coordinator；在至少两个独立真实 workflow 证明共享语义前，不抽象通用 workflow engine。

### 8. 与 DeploymentController 的命名边界

保留 `DeploymentController` 全称。它拥有 committed desired state、Revision、rollout 和 reconciliation；OpsService 只拥有 operation record。公共文档、Schema、日志和进程名不得用裸 `Controller` 指代它，也不得把 OpsService 叫作 `OpsController`。

DeploymentController 内部可以有无独立身份的 `DeploymentReconciler`、`DeploymentRolloutEngine` 与 `RuntimeSliceProjector`。只有未来 committed DeploymentPlan/Revision 由外部平台拥有、ParaEGOX 只负责 fan-out 时，顶层组件才适合改叫 `DeploymentCoordinator`；若只读取既有 desired state 并产生收敛建议，才适合叫 `DeploymentReconciler`。当前 ADR-0001 定义的职责同时包含单写提交、rollout 与 reconcile，因此不改名。

候选部署进程名使用 `paraegox-deploymentd` 或等价带 deployment 限定的名称，不使用 `controllerd`。具体可执行文件名在实现时冻结，不属于本 ADR 的 wire compatibility 承诺。

## 分阶段实施

- **P2e**：先实现 DeploymentController 与窄开发/救援 CLI；不提前伪造完整 OpsService。
- **P6a**：实现 local Inspection 和 local durable Evidence，建立真实 read-only producer/consumer。
- **P6b**：实现 distributed Inspection projection、单实例 OpsService、crash-consistent operation journal、Authority/owner typed client 与 `Uncertain → query/reconcile`。
- **P7**：TUI 只使用 OpsClient + InspectionClient；Web Console 通过 ConsoleGateway 使用同一公开协议。
- **后续**：只有真实 availability 和并发入口需求出现后，才研究 OpsService HA、journal sharding 或多个 management scope；不因多 Node 自动建立 per-Node OPS。

## 最小验证

1. 同 ID/同 digest 重试只产生一个 operation；同 ID/不同 digest 被拒绝。
2. OpsService 在 owner 超时后记录 `Uncertain`，重启后先查询/reconcile，不重复副作用。
3. deploy 类 ControlRequest 只能经过 DeploymentController；CLI、OpsService 和 ConsoleGateway 都无法直接调用 RuntimeApplyEndpoint。
4. ConsoleGateway/CLI 崩溃后，已接受 operation 和 Deployment reconciliation 继续；OpsService 崩溃不影响 Runtime/Safety。
5. Owner Receipt 与 OpsReceipt 分离；缺少 owner terminal evidence 时不能报告成功。
6. Inspection projection 的 stale/unknown/partitioned、source revision/epoch 和 freshness 在断连与重启后保持可解释。
7. import/contract test 证明 OpsService 不依赖 RuntimeHost 私有实现、raw Fabric、Driver 或 Artifact installer。
8. 没有 Secret、raw credential、任意 shell/SSH fallback 或 process-global password 进入生产 OpsService 路径。
9. 连续 media/XR/teleop 流量不经过 OpsService，也不因 OpsService backlog 反压本地控制和 Safety。

## 备选方案

### 一个全能 OPS 服务

部署简单，但会把查询、执行、安装、控制、证据和领域状态聚成新的 god object，形成第二个 Runtime 与多写者，因此拒绝。

### 每个 Node 固定一个 OPS

本地访问方便，但会让全局操作存在多个接收者、journal 和终态 Receipt owner。Node-local management 已由 NodeDaemon/typed endpoint 承担，不采用这一不变量。

### 完全取消 OpsService，让所有客户端直连领域 owner

减少一跳，却复制 request identity、审批、幂等、取消、进度、`Uncertain` 和审计语义。保留窄 OpsService，同时保留 P2e rescue CLI 的受限 bootstrap seam。

### 让 ConsoleGateway 同时成为 OpsService

短期进程数更少，但会把 browser session/cache 与 durable operation journal、Authority 调用和 owner routing 混为一体。development profile 可以共进程，逻辑 contract 和状态 owner 不得合并。

## 后果

收益：

- CLI、TUI、Web Console 和自动化共享统一、可审计、可恢复的 operation 语义；
- DeploymentController、RuntimeHost、Artifact、Node、Authority、Evidence 与 Safety 保持唯一 owner；
- Console、OPS 或网络入口故障不改变系统 desired state，也不破坏本地安全闭环；
- 可以先做单实例 OpsService，再根据证据扩展 availability，而不预建 per-Node 多写控制面。

成本：

- 必须维护 OpsReceipt 与领域 Receipt、operation journal 与 Evidence store 的清晰关联；
- 每个领域写操作需要稳定 typed client/owner endpoint，不能靠临时 shell 快速接入；
- timeout 和 partial failure 必须显式建模为 `Uncertain` 并实现查询/reconcile；
- development 共进程也要保留逻辑权限、budget 和故障边界。

## 开放问题

- P6b reference journal 使用哪种 crash-consistent backend，以及与 EvidenceRef 的事务边界；
- public OpsProtocol 首版采用何种内部 transport，watch cursor 与 backpressure 如何编码；
- 多 tenant 或多个管理域出现时，operation routing/ownership 的正式 scope 名称与 fencing；
- 哪些少量跨 owner runbook 值得专用 coordinator，而不是由客户端显式串联。

这些开放问题不改变核心边界：**OpsService owns the operation record, not the operated system.**
