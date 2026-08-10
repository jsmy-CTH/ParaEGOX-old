# ADR-0010 — Remote Agent ingress proxy 与单一 Fabric Session 边界

> 状态：Accepted
> 日期：2026-08-09
> 决策者：ParaEGOX workspace user（按当前 T2 阶段授权与 2026-08-09 架构裁决）
> 关联文档：[ADR-0009 — Agent 对话、类型化客户端与 Console 边界](ADR-0009-agent-conversation-and-client-boundary.md)、[CONTRIBUTING](../../CONTRIBUTING.md)、[T2-B2 P0 transport proof](../../crates/paraegox-fabric/tests/remote_agent_proxy_gateway.rs)

## 一句话结论

保留现有 `S0` 作为唯一 production Fabric、application bus、Agent `PortBinding` 与本地 Agent route
Session owner；远程 Agent ingress 只允许增加一个由 Runtime 治理、TLS-only、非 Fabric 的 `S1`
proxy Session，它在恰好两条 route 上把已准入请求通过 generation/epoch/PXAP-fenced 的窄 `S0`
request capability 转发到 `S0`，且只能按 `fence ingress → bounded drain → close S1` 退役。

## 背景

ADR-0009 冻结了 FabricService 对 production Fabric、application route、Agent binding 和 raw Zenoh
Session 的所有权，目的在于阻止客户端、UI 或第二个 Bus owner 取得 raw Session、route、binding、重试和
Runtime lifecycle 权限。T2-A 随后增加了 asymmetric remote-Agent 的 PXTE v9/PXAR v10/PXAU v1 和
PXRA/PXRR v1 合同；T2-B 又证明了 exact remote mTLS listener/connector configuration 与 default-deny
CN+TLS application ACL。两者都没有决定远程 ingress 如何在不重启本地 Agent 数据面的情况下接入。

现有本地 Agent 数据面记为 `S0`：它是一个已运行的 FabricService-owned Session，拥有 Agent submit 与
control 两个 `PortBinding`、对应 handler、Fabric generation、Agent generation、session epoch 和 binding
epoch。`LocalAgentOnlyDeactivate` 必须只撤销远程访问，不能让本地 Agent 因远程入口退役而换 generation、
重装 binding 或短暂不可用。

把一个新的 plaintext connector Session 连到 `S0` loopback listener 不能满足这个边界。该 listener
没有远程 subject 的 mTLS 身份和 application ACL；Zenoh ZID 是可配置的 topology identity，不是本 ADR
可接受的密码学 principal。该方案会让远程 subject 进入一个不能证明两主体隔离的 link，也不能把
“连接到了 `S0`”解释为授权。

直接把远程 mTLS listener 加到 `S0` 则需要修改或重启已运行 Session，进而改变 base Fabric lifecycle、
session/generation 或 binding continuity。另起第二个 FabricService 并在其中安装 Agent binding 又会形成
第二个 production Fabric/application bus owner。这两种做法都与 `LocalOnly` 的窄撤销语义冲突。

因此本 ADR 区分两个不同角色：

```text
Mac S2 TLS connector
  -> Runtime-owned S1 ingress wrapper
       -> paraegox-fabric TLS-only lower transport capability
            -> exact submit/control forwarders
                 -> generation/epoch/PXAP-fenced narrow S0 request capability
                      -> existing S0 Session local request
                           -> existing Agent PortBindings and handlers
```

`S1` 使用 Zenoh transport 不使它成为 FabricService、application bus 或第二个 Agent binding owner。`S1`
与 `S0` 之间没有 network connector/link；转发发生在同一个 Runtime 进程内，通过 `S0` owner 发出的
Fabric-only route handle 与 Runtime remote-access owner 包装的窄 request capability 完成。

## 范围与非目标

本 ADR 决定：

- `S0` 与 `S1` 的 owner、依赖方向和允许持有的 capability；
- `S1` 的 exact-two-route、TLS-only、default-deny ingress 边界；
- admitted/queued/in-flight request 在退役时的 fence、drain、terminal 和 uncertainty 规则；
- `LocalOnly` 对 `S0` generation、session epoch、binding epoch 与本地 Agent 可用性的非影响；
- 旧 remote-Agent contracts/state 不能被原地改释，后继必须 additive versioning；
- P0 transport proof 能证明与不能证明的范围。

本 ADR 不决定或实现：

- successor contract 的 canonical fields、wire offsets、bytes、digest domain、signature transcript 或 stable
  reason code；
- Runtime production lifecycle owner 的具体类型、durable store codec、current-latest slot marker wire、
  restart/recovery dispatcher 或 public endpoint 实现；
- macOS APFS outbox、FD-anchored ACL、Secret/credential resolver、真实 Mac connector composition 或双机
  Echo；
- reconnect、透明 replay、自动 retry、continuous reconciliation、partition healing 或 availability SLO；
- TUI/Web Console、Agent protocol、Model、PXCB desired truth 或 Deployment rollout owner 的扩大；
- 把 P0 test gateway、test PKI 或 raw test Session 发布为 public API 或 production component。

## 决策

### 1. `S0` 继续是唯一 production Fabric 与 Agent binding owner

`S0` 唯一拥有：

- production Fabric/application bus Session 与其 lifecycle；
- Agent submit/control 的 exact `PortBinding`、handler、route registration 和 ingress census；
- request correlation、bounded handler dispatch 和 response status；
- 只暴露 exact route/schema/bounds、Fabric session epoch 与 binding epoch 的 Fabric-only route handle。

AgentService 继续唯一拥有 AgentSession/Turn 语义。Runtime lifecycle owner 继续拥有 Fabric/Agent generation，
并把 Fabric-only route handle 包装成逐次重验 PXAP、store/host epoch 与 remote-access generation 的 opaque
typed capability；本 ADR 不把这些 authority 转给 `S0` transport owner，也不让 Runtime 取得 raw `S0`
Session 或 binding mutation API。

`S1` 自身也分成两个不能合并的 owner layer：`paraegox-fabric` 只提供不暴露 raw Zenoh Session 的
TLS-only lower transport capability，并执行 exact ACL/query/reply 机制；Runtime wrapper 独占一个 `S1`
实例的 credential-resolution 调用、Session lifecycle、queryable、queue/worker、forwarding、PXRS/current-live
marker、deadline、drain 与 terminal selection。lower layer 不拥有 desired truth、Runtime state、`S0`
binding、retry 或 recovery。`S0` 仍是唯一 FabricService 与唯一 production Fabric Session；`S1` 的底层
Zenoh Session 只是 Runtime-owned ingress transport，不能被解释成第二个 Fabric Session/application bus。

`S1` 不得创建、替换、undeclare 或复制 `S0` binding，不得取得 raw `S0` Session，不得启动/停止 `S0`
FabricService 或 Agent，不得改变 `S0` generation/session/binding epoch，也不得成为 desired-state、journal、
PXST/PXAP descriptor 或 retry/reconcile owner。

`S1` 只由 Runtime remote-access lifecycle owner 创建和销毁；其唯一 application path 是两个
route-specific forwarder。Mac `S2` connector 只消费远程 TLS query/reply boundary，不取得 Runtime、Agent、
Fabric 或 desired-state 对象。

### 2. `S1` 是一个非 Fabric、TLS-only 的窄 ingress proxy

每个 active remote-access generation 最多有一个 `S1`。它必须：

- 只有一个 non-loopback IPv4 TLS listener、零 plaintext listener、零 connector；
- 只允许从 byte-exact retained PXAP strict decode、固定为 submit/control 顺序且彼此不同的两条
  target-scoped exact route；P0 使用的测试字面 route 不构成 production contract；
- 使用 default-deny application ACL，把 ingress query 和 reply/declare-queryable flow 同时约束到 exact
  Mac data-plane principal 的证书 Common Name 与 TLS link；
- 禁用 scouting、admin space、plugin loading、wildcard route、put/delete/subscriber/liveliness flow；
- 对 session、link、pending ingress、每 route queue、frame bytes 和 absolute operation deadline 保持显式
  hard bound；
- 不向任何 caller 暴露 raw Session、Queryable、Zenoh ZID、route builder 或 credential bytes。

Production route、BindingId、BindingEpoch、request/response schema 与 ingress limits 的唯一权威都是当前
byte-exact retained PXAP 及其两个 nested PXBD。Runtime wrapper、Fabric lower capability、local config 与
test constant 都不得重建、归一化或覆盖这些字段。当前 owner 产生的是 target-scoped
`paraegox/agent/v1/.../{submit,control}` family，但该字符串只是说明现状，不是本 ADR 冻结的 route；每个
production `S1` 必须逐字段消费其 exact retained PXAP。

证书握手、一个 live TLS link 或 ZID 本身都不授予 Agent access。只有 ACL 后被 exact route callback 观察、
通过 framing/binding/deadline admission 并成功进入 bounded queue 的 query 才是 admitted work。未匹配、
wrong-CN、forbidden route、malformed、stale binding、over-capacity 或 deadline-expired 输入都不能产生 `S0`
request effect。

### 3. Forwarder 只持有窄 `S0` request capability

capability 必须分成两个不能互相替代的层次：

1. `S0`/Fabric owner 只发放 route-specific、不可变且不暴露 raw Session 的 lower handle；它绑定 exact
   route、schema、bounds、当前 `S0` session epoch 与该 `PortBinding` 的 binding epoch，但不知道或声称
   Agent generation、Runtime store/host epoch、remote-access generation；
2. Runtime remote-access owner 把 lower handle 包装成不可扩权的 upper request capability，并在每次 send 前
   重验其余 Runtime-authoritative scope。

每个 forwarder 只持有一条 route 对应的 upper capability。两层合起来必须同时绑定：

- 当前 Fabric generation 与 Agent generation；
- 当前 `S0` session epoch 和该 `PortBinding` 的 binding epoch；
- exact retained PXAP descriptor/root 及从中 strict decode 的 BindingId、BindingEpoch、route、schema 与
  limits；
- 当前 Runtime store/host epoch 和 remote-access generation 所要求的 scope。

任一 lower-handle 或 Runtime scope 维度 stale 都必须在 `S0` callback 前 fail closed。调用只能是“把一个已
strict decode、关联且仍在 admission absolute deadline 内的 request 发送一次到 exact
`S0` binding，并返回一个 typed terminal”。Forwarder 不得取得 raw `FabricService`/Session、动态查找
route、声明 queryable、安装 handler、修改 desired truth、刷新 descriptor、重连 `S0` 或重试 request。

每个 admitted query 最多调用 `S0` 一次。queue wait 消耗同一个 admission-time absolute deadline；出队时
已过期的请求必须产生明确 no-effect terminal 或有界 error reply，并保持 `S0` callback count 为零。不能在
出队时重新生成 budget，也不能因 timeout、disconnect、reply failure 或 shutdown 猜测 replay。

### 4. 退役顺序固定为 fence、drain、close

正常 `LocalOnly` 或 owner shutdown 必须按以下顺序执行：

1. 在 Controller apply deadline 前 durable commit 并完成 exact first ingress fence：undeclare 两个 `S1`
   queryable、等待其 callback 退出并关闭 ingress admission；
2. 保留 admitted queue 与已经进入 `S0` 的 response receiver，在独立 bounded cleanup budget 内让每个
   forwarder 顺序 drain 自己的 receiver；
3. 每个 admitted query 取得一次 typed no-effect/effect-completed/uncertain terminal，且不产生 retry；
4. join 两个 forwarder 后关闭 `S1` Session，并验证 listener capability 已释放；
5. 只有所有 admitted work 均得到可证明的 terminal、没有残余 `S0` effect，且 `S1` 已 joined/closed 时，
   才能报告 `Drained` 并完成 `LocalOnlyReady`。

在 drain 中已经进入 `S0` handler 的 request 不能被“取消 future”冒充 no effect。shutdown 必须等待 known
handler terminal；如果 handler/deadline、reply delivery、join 或 cleanup 使 effect outcome 无法证明，结果
必须是 `OutcomeUncertain`，remote-access owner 进入 quarantine/reconciliation-required 状态，且不得报告
`Drained`、`LocalOnlyReady` 或自动重试。关闭 `S1` 只完成 ingress fencing，不会把一个尚在 `S0` 执行的
effect 变成 no effect。

Controller apply deadline、per-query admission deadline 与 safety cleanup budget 是三个不同边界：

- Active apply 只有在 Controller deadline 前 durable commit 首个有副作用 intent，并在同一 deadline 前选择
  `ActiveReady`，才可以成功。effect may have started 而 deadline 前不能证明 Active success 时，只能进入
  uncertain/quarantined；不能在 deadline 后补报 Active success 或重放 intent。
- LocalOnly 只有在 Controller deadline 前 durable commit fence intent 并证明 first ingress fence 已完成，
  才有资格最终报告 `LocalOnlyReady`。deadline 前没有 intent/effect 是 no-effect rejection；intent 或 fence
  may have started、但 deadline 前不能证明 fence 完成时进入 uncertain/quarantined，并继续不可取消的
  containment。已经按时完成 first fence 后，drain admitted work、join worker、close Session 与释放
  capability 都是不可取消的 safety cleanup，可以跨过 Controller deadline，并且只能由同一 operation
  继续，不能因 caller timeout、supersede 或 shutdown 再次入队。
- 每个 query 仍使用 admission-time absolute deadline；它只界定该 query 的一次 send/reply，不得取消已经
  durable fenced 的 LocalOnly cleanup，也不得重置成 Controller 或 cleanup budget。

cleanup budget 到达只会让 owner 选择 `OutcomeUncertain`/quarantine 并保留唯一 cleanup authority，不得
cancel/drop worker 或 response receiver。LocalOnly terminal 可以由同一个 durable operation 在 Controller
deadline 后选出；调用方若先失去 reply，只能 query/reconcile 该 operation，不能据此重试 fence。
post-deadline cleanup 失败或 effect outcome 不明仍不能伪造 `Drained`。

`LocalOnly` 只撤销 `S1` 和 remote-access generation。成功或 uncertain 的 remote retirement 都不得为了
收尾而重启/重配 `S0`，不得推进 Fabric/Agent generation、`S0` session epoch 或 binding epoch，也不得撤销
本地 typed handle。成功后必须以 local request 证明原有 `S0` Agent route 仍可用；uncertain 时继续按
quarantine 处理远程 operation，而不是把不确定性转嫁为 `S0` 生命周期 mutation。

### 5. 版本与状态迁移只能 additive

已冻结的 PXTE v9、PXAR v10、PXAU v1、PXRA/PXRR v1 与 owner-private PXRS v1 仍按原有 bytes、digest、
signature、decoder、outcome/state 和 rejection 语义存在。本 ADR 不允许：

- 把旧版的“remote listener/client ACL”字段追溯解释为 `S1` proxy owner、forwarder 或窄 `S0` capability；
- 让 PXAU v1 `LocalOnlyReady` 在旧 decoder 下获得新的 drain/quarantine 含义；
- 从 PXRS v1 raw bytes 恢复 authorization、Session、queryable、task、capability 或可执行 transition；
- 用 alias、宽松 decoder、fallback 或 offline rewrite 把旧 state 伪装成新拓扑。

后继 contract batch 必须使用显式 additive versions。当前预留的连续编号是 PXTE v10、PXAR v11、PXAU
v2、PXRA/PXRR v2 与 private PXRS v2；其物理字段、canonical bytes、digest/signature domains、migration 和
reason taxonomy 由该后继批次独立冻结，本 ADR 不把架构图当成 wire contract。每对 exact decoder 必须
双向 cross-reject：PXTE v9 ↔ PXTE v10、PXAR v10 ↔ PXAR v11、PXAU v1 ↔ PXAU v2、PXRA v1 ↔ PXRA
v2、PXRR v1 ↔ PXRR v2、PXRS v1 ↔ PXRS v2。统一入口只能在有界读取 version 后分派一次，不能
try-new-then-old fallback。PXRS v1 只保留为 fresh-only、未部署 predecessor evidence，不迁移为 PXRS v2
authority。

### 6. 首个 slice 的 durable state 与 recovery 固定 fail-closed

首个 production slice 不为 admitted query、queued query 或 in-handler effect 创建 per-query durable sidecar。
PXRS v2 只持久化 remote-access operation/phase、generation、fence/drain/terminal 与 exact owner binding；
query-level correlation、queue item、response receiver 和 effect observer 都只存在于当前 Runtime epoch 的
live owner 内。持久 snapshot 不能重建、重试或推断任何单个 query outcome。

因此 RuntimeHost restart 看到 old-epoch `ActiveReady`、任何 in-progress phase、`OutcomeUncertain` 或
`Quarantined` 时，一律保持 `ReconcileRequired`/quarantine。restart path 必须调用零 credential resolver，
创建零 `S1`/listener/queryable/worker，发布零 current-live marker，提供零 remote-access Describe，并且不能
自行选择 `LocalOnlyReady` 或 fresh `ActiveReady`。raw PXRS v2 decode 永远是 inert evidence，不带
authorization、capability、task handle 或 transition method。

Recovery 顺序固定为：

1. 在任何 Runtime owner reassembly 前 strict decode/adjudicate current PXRS v2，并验证外部 current-slot
   marker 对 snapshot digest、Runtime store instance、RuntimeHost epoch、slot revision 与 monotonic latest
   high-water 的 exact binding；self-checksum、previous-record digest 或可替换 latest file 单独不构成
   anti-rollback；
2. mismatch、older slot、old epoch、missing marker 或 remote-present uncertain phase 在任何 resolver、listener、
   queryable、worker、Describe 或 remote effect 前 fail closed；
3. 只有完成该 adjudication 后才独立 reassemble 原有 `S0` Fabric/Agent stack，且 reassembly 不读取 PXRS
   作为 `S0` desired truth；
4. `S0` reassembly 后，只有 byte-exact 匹配 current PXRS slot、current Runtime epoch 与 current owner 的
   non-durable same-epoch live marker 才可继续已经存在的 `S1` operation。该 marker 不可 decode、clone 或
   跨 restart 重建；restart recovery 本身永不激活 `S1`。

任何 fresh Active 只能来自 adjudication/reconcile 后的新 authenticated successor Apply，并重新取得当前
PXAP、credential 与 move-only owner capability；它不是 recovery continuation。

## 备选方案

### 用 plaintext `S1` connector 接入 `S0` loopback listener

拒绝。`S0` loopback listener 没有远程 subject 的 mTLS identity/application ACL；Zenoh ZID 不能替代
密码学 principal。该方案无法证明 Mac client 与本机其他 subject 的 default-deny 隔离。

### 把 remote mTLS listener 直接加入 `S0`

拒绝。它把远程 access lifecycle 与 base Fabric/Agent lifecycle 耦合，`LocalOnly` 会要求重配或重启
`S0`，从而改变 generation/session/binding continuity，并扩大 `S0` ingress authority。

### 启动第二个 FabricService 并复制 Agent bindings

拒绝。它形成第二个 production application bus、binding/handler 和潜在 retry/lifecycle owner，无法保持
ADR-0009 的单一 Fabric 与 AgentSession/PortBinding ownership。

### 让 Mac client 直接持有或发现 `S0`

拒绝。它泄漏 raw Session、route/discovery 与 reconnect 语义，绕过 Runtime-issued typed capability 和
remote-access lifecycle，也把客户端变成第二个 Fabric owner。

### shutdown 时取消所有 forwarder 并立即关闭 `S1`

拒绝。一个已经进入 `S0` handler 的 effect 可能在 shutdown 返回后继续完成。取消等待 future 只丢失
outcome observer，不能撤销 effect；此时声称 `Drained` 会制造错误的 exact-zero/LocalOnly 证据。

## 后果

收益：

- 远程 access 可以独立启停，而本地 Agent route、handle 和 generation 保持稳定；
- `S0` 仍只有一个 Fabric/application truth 与 binding owner，不增加网络回环 link 或第二份 Agent handler；
- CN+TLS ACL、exact route、bounded admission 与 no-retry 形成可单独审查的最小远程入口；
- shutdown 的 known drain 与 uncertain/quarantine 分型防止把后台 effect 冒充成功清理。

成本与限制：

- Runtime 必须拥有第二个 Zenoh transport Session 的 task、deadline、credential 与 cleanup，但不得把它
  泛化为第二个 FabricService；
- 每条 route 的 bounded queue 和顺序 drain 限制吞吐；扩容需要证明 ordering、capacity、backpressure 与
  shutdown semantics 后再提出 successor；
- `S1` 关闭不自动解决已进入 `S0` handler 的 uncertain effect，Runtime 需要 durable quarantine/reconcile
  状态和 current-latest evidence；
- exact Zenoh 1.9 local-dispatch/ACL 行为是实现依赖，升级 transport 前必须重跑同等级真实网络证据；
- Common Name pinning 不是最终 SAN/fingerprint/rotation 方案，也不提供拒绝 wrong-CN TLS link 建立所需的
  availability/DoS 保证。

## 失败场景与反例

- 如果 production wrapper 无法在不暴露 raw `S0` Session/FabricService 的前提下实现 generation/epoch/
  PXAP-fenced request capability，应停止 Runtime integration 并重新评审 owner seam；不得把 P0 中直接持有
  `Arc<FabricService>` 的 test helper 提升为生产 API。
- 如果新的 Zenoh 版本不再保证同 Session local request、ACL exact-route flow 或 callback undeclare/join
  语义，应 fail closed，并以 fresh pinned-version transport proof 决定是否修改或替换 `S1`。
- 如果真实 workload 需要多于两条 route、并行同 route handler、server push、subscription 或 liveliness，
  本 ADR 的 exact-two-query-route proxy 不足；需要新的 producer/consumer、ACL、ordering、backpressure、
  drain 和 compatibility 决策，不能用 wildcard 扩权。
- 如果 drain deadline 内无法取得所有 admitted request 的确定 terminal，必须保持 `OutcomeUncertain` 与
  quarantine；业务压力或 shutdown SLO 不能成为伪造 `Drained` 的理由。

## 实施与验证

### P0 transport proof（已完成，非 production）

[Linux real-network test](../../crates/paraegox-fabric/tests/remote_agent_proxy_gateway.rs) 中的
`remote_agent_proxy_gateway_forwards_exact_routes_without_a_second_fabric_link` 已在 immutable ref
`refs/heads/build/mac-source-snapshot-20260809-r215-t2-b2-proxy-gateway-typed-drain-outcome-fmt`
（commit `315672a92f2b4bd20fa2834c7834d6cfa2137457`，test-file SHA-256
`2e953377c4a06ef360c46ec11747c7682c7ab205711068865481e3ccf0f7094a`）冻结以下 P0 证据：

- `S0` 保留原两个 binding/handler，`S1` 没有到 `S0` 的 connector/link；正确 CN client 经 `S1` 对
  submit/control 各得到一次 typed Echo，forward/callback count 均为 exact one；
- sentinel、parent、child、wildcard 和 same-CA wrong-CN query 不能到达 `S1` callback 或 `S0` handler；
- 已进入 handler 与已 admission/queued 的 query 在 ingress fence 后被 drain 一次，shutdown 等待它们的
  typed responses，不 cancel、不 retry；
- handler timeout 被分型为 `OutcomeUncertain`，关闭 downstream observer 后的迟到 effect 不被冒充
  `Drained`；
- `S1` joined shutdown 后 listener port 可复用，原 `S0` local request 继续成功，最终单独 shutdown `S0`。

P0 特意使用 test-only 字面 route `paraegox/agent/submit` 与 `paraegox/agent/control`，只验证 topology、ACL、
single-send 和 drain causality；它没有消费 target-scoped production PXAP，因而不是 production route、
BindingId、BindingEpoch、schema 或 limits 的证据。

该测试使用一个进程、测试 PKI、test-only raw `S1` gateway 和直接的 test `FabricService` handle；它没有
Runtime production owner/store/endpoint、successor wire、APFS connector/outbox、真实 Secret resolver、两台
机器或 deployable marker。因此它只回答“所选拓扑在 pinned transport 上能否成立”，不能证明 remote
Agent capability 已实现、可部署或 production ready。

### 后继实施门槛

1. additive contract batch 冻结 successor desired/apply/terminal/access/state bytes、compatibility、golden，
   并逐对锁定 PXTE v9↔v10、PXAR v10↔v11、PXAU v1↔v2、PXRA/PXRR v1↔v2、private PXRS v1↔v2 的
   双向 cross-reject；
2. Fabric owner 提供不暴露 raw Session 的 `S1` lower transport capability 与 `S0` lower route handle；
   Runtime remote-access owner 提供 generation/epoch/PXAP-fenced upper request capability，并独占 `S1`
   wrapper、queue/worker 与 cleanup；
3. Runtime owner 实现 PXRS v2、durable current-slot/latest marker、access-generation high-water、first-fence、
   drain/close 与 `Drained`/`OutcomeUncertain`/quarantine，不增加 per-query durable sidecar；
4. Controller producer 与 Runtime consumer/dispatcher 同时接入 successor carrier；restart 时先 adjudicate
   current PXRS v2/current-slot，再按 predecessor owner 规则 reassemble `S0`；之后只有 existing same-epoch
   live marker 可以继续，old-epoch state 不调用 resolver、不创建或激活 `S1`；
5. macOS owner 完成 FD-anchored ACL、APFS durable outbox 与 TLS-only connector，再进行真实双机 one Echo；
6. 只有 exact immutable-source Rust gates、focused lifecycle/failure evidence、独立安全复核和双机结果都
   通过后，才能更新 governance、ADR/index、runbook 与运行状态；ADR Accepted 与 P0 test 都不能代替这些
   证据。

### 首次 production remote-Agent ingress 证据

首次可以更新实现状态的证据至少包括：

- successor public wire 由 Rust 与独立 Python oracle 锁定 exact bytes、digest、signature transcript，并与
  上述各自的 exact predecessor version 双向 fail closed；
- `ActiveReady` 与 `LocalOnlyReady` 都证明 retained PXFT/PXST/PXDE/PXAH/PXAP、Fabric/Agent generation、
  `S0` session epoch 和两路 binding epoch 前后完全不变，只允许 remote access generation 前进或消失；
- durable current-latest marker、slot revision、snapshot digest 与 access high-water 能拒绝复制、回滚、旧
  epoch 和错误 operation 的合法重签 state；
- partial queryable declare/fence、queue expiry/overflow、handler/response timeout、worker join、Session close
  与 crash/restart failpoint 都不会产生 retry，也不会把未知结果提升为 Ready；
- old-epoch ActiveReady/in-progress/uncertain/quarantined snapshots 在没有 per-query sidecar 的前提下只产生
  `ReconcileRequired`/quarantine，并证明 resolver、`S1`、listener、queryable、worker、live marker 与
  remote Describe count 全为零；
- production credential resolver、PXAP-derived target-scoped routes、真实 Runtime producer/consumer、Mac
  connector 与双机 one Echo system gate 全部使用同一 successor contract 和 owner chain。

首次 production remote-Agent ingress 证据明确不包括 TUI 或透明 reconnect/partition recovery；这些能力
需要各自的后继 owner、contract 与系统门。

## 后继与替代

本 ADR 仅在 remote-Agent ingress 场景下，窄化替代 ADR-0009 中“唯一 production Zenoh Session”的绝对
数量表述：`S0` 仍是唯一 production **Fabric/application bus/Agent PortBinding Session**，只额外允许一个
由 Runtime 治理的非 Fabric `S1` TLS ingress Session。ADR-0009 的其余 ownership、typed-client、protocol
与 raw-Session containment 规则全部不变；本 ADR 本身不构成 production `S1` 准入或运行证据。

本 ADR 不替代 PXTE v9、PXAR v10、PXAU v1、PXRA/PXRR v1 或 PXRS v1。后继合同与 Runtime state 只能通过
新的 additive version 和显式迁移/拒绝规则实现。
