# Web Console、WebRTC、WebXR 与交互式 Gateway 边界研究

> 状态：Research Complete，结论为 `revise`
> 日期：2026-07-29
> 深度：Deep
> 评审策略：受限实现 failure-path audit + 外部一手规范核对 + ParaEGOX 架构一致性复核；尚待 Proposed ADR 评审
> 范围：Web Console、HTTP/SSE/WebSocket、WebRTC、WebXR、媒体与遥操作入口、RuntimeHost、Deployment、OPS、Authority、Zenoh Fabric
> 实现状态：尚未实现；Gateway managed-workload、外部 exposure 到内部 typed endpoint、浏览器/peer/XR stream 分代合同仍需 Proposed ADR
> 架构依据：[ADR-0003 — OPS、OpsService 与 Inspection 操作边界](../adr/ADR-0003-ops-service-operation-boundary.md)、[Kernel、RuntimeHost 与 Core Services 架构基线](../architecture/kernel-runtime-core-services.md)、[分布式系统模型](../architecture/distributed-system-model.md)、[Kernel 消息机制与 Fabric 边界](kernel-messaging-fabric-evidence-security.md)、[Runtime 执行模型、调度与恢复](execution-model-scheduling-and-recovery.md)

> 后续裁决（2026-07-29）：[ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md) 已接受“Rust-first mechanisms，polyglot workloads”。Rust-first Runtime/Fabric 不要求 Console/WebRTC/WebXR Gateway 全部改写为 Rust；Gateway 仍按 Artifact、ProcessDomain、独立 Service/Gateway 的信任、故障与性能证据选择 Rust、Python、C++ 或外部进程。

## 一句话结论

ParaEGOX 不恢复宽泛的 `runtime/io`，也不把 Console、WebRTC 和 WebXR 压成一个全能 Bridge：Console 是客户端产品，WebXR 是浏览器 XR 交互 API，WebRTC/HTTP/WebSocket 是外部连接机制；浏览器边界由窄 `ConsoleGateway` 与候选 `WebRealtimeGateway` 终止，内部只暴露 InspectionClient/OpsClient 与待冻结的 typed Port/Service/Operation seam；InspectionService、OpsService 和真实 action owner 保持分离，Zenoh 仍是唯一内部生产 Fabric，Gateway 被 ParaEGOX 管理时所使用的通用 workload/lifecycle envelope 仍需独立裁决。

## 1. 研究问题与成功标准

本研究回答：

1. 旧系统中的 Console Bridge、WebRTC camera stream、WebXR input/server 和 Runtime I/O 能力应如何在 ParaEGOX 中重新归属。
2. Console、WebRTC、WebXR、HTTP、WebSocket、Zenoh 分别属于客户端、协议、语义还是内部数据面。
3. 浏览器观测、媒体播放、XR 输入、行政运维和物理遥操作如何走不同的授权与失败路径。
4. RuntimeHost、DeploymentController、Gateway、CardDefinition、Card、Deck、OPS、Media 与 Teleoperation 分别拥有什么。
5. 如何避免私有 event loop、重复 WebRTC endpoint、无界媒体队列、断线 best-effort stop 和 Console 缓存冒充真值再次形成债务。

成功标准是形成可实施和验证的 owner chain：

- 浏览器不能直接获得原生 Zenoh Session、RuntimeHost 对象或 Driver 句柄。
- WebRTC 不成为与 Zenoh 并列的 ParaEGOX Fabric backend。
- WebXR payload 在进入内部前完成身份、Schema、时间、坐标、校准、freshness 和权限转换。
- Console 的只读投影与受权写操作具有不同 API、owner 和 Receipt。
- PeerConnection/DataChannel 的 transport success 不会被误报为物理 effect success。
- Gateway 或公网断开时，本地 Safety、deadman、lease expiry 和最低自治不依赖最后一个网络包到达。
- 视频编码、实时输入、Console 日志和后台观测不共享无界队列或单一故障域。

## 2. 范围、假设与非目标

### 2.1 范围

- 浏览器 Console 与可选的 WebXR 前端。
- HTTPS、REST、SSE、WebSocket、WebRTC media track 与 DataChannel。
- signaling、ICE/STUN/TURN、PeerConnection、codec/track 和浏览器会话。
- Camera/Audio/XR input 到 ParaEGOX typed Port，以及遥操作到 Authority/Lease/Safety/Driver 的链路。
- Gateway 的 deployment、runtime lifecycle/liveness/recovery、inspection、security 与 failure convergence。

### 2.2 假设

- Zenoh 仍是 ParaEGOX 唯一生产 Fabric，覆盖 `session-local`、`host-local` 和 `remote` route。
- 浏览器不原生参与 ParaEGOX Node management、Deck/Card lifecycle 或内部 Fabric trust domain。
- 首版主要面向单操作者、少量 viewer 和单机器人/设备会话；大规模 SFU、CDN 和全球媒体编排没有当前证据。
- 物理写继续经过 `CommandEndpoint → Authority → Lease/Fencing → Safety → Driver EnforcementPoint → Receipt`。
- Rust native Gateway mechanism 如被采用，由 Cargo 管理；Python SDK、Gateway/worker、浏览器辅助工具和验证由 `uv` 管理。公网媒体和编解码依赖必须进入明确的可选依赖组，跨语言合同由共同 conformance suite 验证。

### 2.3 非目标

- 不在本文冻结具体 Web 框架、WebRTC 库、SFU 产品、OIDC provider 或 TURN 实现。
- 不把 Dashboard、Web UI、媒体服务器或 XR 渲染代码放入 Kernel。
- 不把所有 camera frame 强制复制进 Python/Rust 语言私有对象或强制跨主机 Zenoh 传输。
- 不用浏览器按钮冒充硬件 E-Stop。
- 不创建全局 `IORegistryService`、万能 endpoint registry 或第二套 Bus。
- 不复制受限相邻项目的源码、配置、测试、网页资源或内部文档。

## 3. 证据与强度

### 3.1 本地证据

| 证据 | 类型 | 强度 | 对结论的影响 |
| --- | --- | --- | --- |
| ParaEGOX 已把 Gateway 定义为外部生态与内部系统之间的语义、安全和故障边界 | local | 高 | 浏览器协议应止于 Gateway，不进入 Kernel/Runtime 公共原语 |
| ParaEGOX 已冻结 Zenoh-only production Fabric、typed PortBinding、InspectionService/OpsService 与 Authority 边界 | local | 高 | WebRTC 不能成为第二个 Fabric；Console 不能直读 Runtime |
| ParaEGOX 禁止 Card 实现私建 thread、process、event loop 和无 owner task | local | 高 | HTTP/WebRTC/XR server 生命周期必须由 RuntimeHost 或 external workload manager 托管 |
| 相邻系统的 Console 入口会启动全局 Runtime，并在同一聚合器中混合观测缓存、命令、审批和底层网络 | local | 高 | Console Bridge 已越过 BFF 边界，应拆为纯客户端、ConsoleGateway、InspectionService 与 OpsService |
| 相邻系统的 XR 入口同时托管静态网页、HTTP/WebSocket、鉴权、XR 坐标、视频 SDP proxy、WebRTC peer 和物理输入映射 | local | 高 | 单一 XR Card 实现已成为半个 Runtime；协议终止与 XR 语义转换必须分层 |
| 相邻系统的 camera 组件同时承担采集、MJPEG、WebRTC signaling、PeerConnection 和 endpoint advertisement | local | 高 | Driver/Camera Card 不应拥有公网服务和 peer lifecycle |
| 断线 callback 发送 neutral/stop 只能 best-effort，无法证明 packet 到达或执行器进入安全状态 | local | 高 | 断线收敛必须依赖本地 lease/deadman/fencing，而非最后一条消息 |
| 视频编码、Web server、XR input 和 Console stream 共享 event loop/thread 时会互相放大延迟与关闭竞态 | local | 高 | execution requirements、Domain、队列、liveness/recovery 必须分开计划和观测 |

以上相邻系统证据只转化为中立失败模式和行为需求；公开文档不记录其私有源码路径或可还原实现的逐段映射。

### 3.2 外部一手资料

| 来源 | 类型 | 强度 | 对结论的影响 |
| --- | --- | --- | --- |
| [W3C WebRTC](https://www.w3.org/TR/webrtc/) | external | 高 | WebRTC 提供浏览器 media/transceiver、PeerConnection 和 DataChannel API，不定义 ParaEGOX 业务权限与 Receipt |
| [RFC 9429：JSEP](https://www.rfc-editor.org/rfc/rfc9429.html) | external | 高 | WebRTC 不标准化完整应用 signaling；地址发现、鉴权、重试和会话 API 仍需应用 owner |
| [RFC 8831：WebRTC Data Channels](https://www.rfc-editor.org/rfc/rfc8831.html) | external | 高 | DataChannel 是 SCTP over DTLS 的有序/无序、可靠/部分可靠消息通道，不是内部 Bus 或 RPC 语义 |
| [RFC 8445：ICE](https://www.rfc-editor.org/rfc/rfc8445.html)、[RFC 8489：STUN](https://www.rfc-editor.org/rfc/rfc8489.html)、[RFC 8656：TURN](https://www.rfc-editor.org/rfc/rfc8656.html) | external | 高 | 公网 WebRTC 需要正式的路径发现、中继、凭证、容量和连接观测；Zenoh 不能代替浏览器 ICE |
| [W3C WebXR Device API](https://www.w3.org/TR/webxr/) | external | 高 | WebXR 是 secure-context 内的设备、空间跟踪、输入与渲染 session API，不是网络传输协议 |
| [RFC 9725：WHIP](https://www.rfc-editor.org/rfc/rfc9725.html) | external | 高 | 单向 WebRTC ingest 已有 Standards Track HTTP signaling profile，可作为媒体源到中心服务的可选 adapter |
| [WHEP Internet-Draft](https://datatracker.ietf.org/doc/draft-ietf-wish-whep/) | external | 中高 | viewer egress profile 截至 2026-07-29 仍不是 RFC，只能放在可替换 adapter 后面 |

### 3.3 推断与开放证据

- **inference**：首版浏览器运维使用 HTTPS + SSE/WebSocket 比 WebRTC DataChannel 更容易做鉴权、游标、审计、代理和故障诊断。
- **inference**：连续 XR pose/摇杆若经目标设备基准证明 WebSocket head-of-line 或 latency 不满足 SLO，可使用 unordered/partially-reliable DataChannel；离散 arm/mode/operation 仍使用可靠、可回执路径。
- **inference**：单机器人少 viewer 的首版无需自建 SFU；需要多 viewer、共享转码、录制或 track authority 后再建立 MediaService/SFU 边界。
- **open**：Quest/Pico/目标浏览器上 WebSocket 与 DataChannel 的真实延迟、抖动、后台 throttling 和断线行为尚未测量。
- **open**：目标 camera/Jetson 上 raw frame、hardware encoded frame、Zenoh SHM 与 WebRTC packetization 的 copy/CPU/GPU profile 尚未测量。
- **open**：远程操作中视频 freshness 是否作为控制 lease 的必要条件，需要具体 ODD/hazard policy 决定。

## 4. 先纠正分类

| 名称 | 正确类别 | 拥有什么 | 明确不是什么 |
| --- | --- | --- | --- |
| Console Web App / TUI | 客户端产品 | 页面/终端交互、local presentation state | Runtime、OPS truth、Transport owner |
| `ConsoleGateway` | 外部 Gateway/BFF | HTTP/SSE/WS、外部身份映射、公开 InspectionProtocol/OpsProtocol、bounded projection cache | 每 Node 必备组件、全局 Runtime、Evidence/OPS owner、原生 Fabric backend |
| `InspectionService` | 只读 CoreService | node-local/federated projection revision、cursor、cache、freshness | source facts、desired state、health truth owner |
| `OpsService` | 运维 CoreService | ControlRequest lifecycle、幂等 journal、progress、terminal OpsReceipt | 被操作对象、Authority、Deployment、Runtime 或 physical effect owner |
| WebRTC | 外部实时协议栈 | media track、DataChannel、PeerConnection、ICE/DTLS/SRTP | WebXR、Zenoh、权限、物理成功语义 |
| WebXR | 浏览器 XR API/交互来源 | headset/controller pose、input、space、render session | Transport、Server、Node identity |
| `WebRealtimeGateway`（候选组合名） | 外部交互 Gateway 的部署组合 | WebRTC signaling/media 与 XR input adapters、peer stats | 硬实时 `RealtimeDomain`、第二个 Fabric、Teleoperation policy owner、Safety owner |
| Zenoh-native Fabric | 内部生产数据面 | ParaEGOX PortBinding 的 session/host/remote route | 浏览器会话或 WebRTC signaling |
| `MediaService` | 条件式共享服务 | 多消费者 track、共享转码、录制或 SFU 状态 | 首版必建服务、Evidence store |
| `TeleoperationService` | 条件式领域服务 | 跨 Deck 会话、控制权仲裁、共享 teleop policy | Transport terminator、Authority/Lease/Safety 替代品 |

`ConsoleBridge` 只有在它确实只是无状态字节转发时才适合继续叫 Bridge。一旦它负责身份、Schema、投影、权限、失败语义或操作转换，就必须叫 Gateway。`WebXR transport` 是错误名词；服务端看到的是外部会话和经过转换的 XR observation/input，不是一个叫 WebXR 的 transport backend。

`WebRealtimeGateway` 目前只是“Web 侧低延迟交互组合”的候选限定名，不表示硬实时，也不等于未来的 `RealtimeDomain`。逻辑上仍应区分 `MediaGateway` 与 `XRInputGateway` 两个角色；首版若共享 PeerConnection/session owner，可以在一个部署组合中共置。命名、managed workload 身份和是否需要独立实例类型必须在 ADR 前复核，不能因本文示意图直接进入 Kernel Schema。

这些角色也不是语言类别。ConsoleGateway 可以从 Python 起步，codec/media worker 可以是 C++/Rust/硬件进程，受信任且与 RuntimeHost 同版本静态链接的窄 Rust implementation 才可能经准入 in-process；Python、C++、未知原生库和第三方 codec 默认进入 ProcessDomain 或 external workload。PyO3/maturin 或 Rust trait object 不是 Gateway/Runtime 的公共 ABI。

## 5. 方案比较

### 5.1 方案 A：恢复宽泛 `runtime/io`

把 HTTP、WebSocket、WebRTC、camera、recording、Console 和 XR 都放进 Runtime I/O。

优点是短期目录集中、启动路径看似简单。问题是 Runtime 很快需要理解 endpoint URL、TLS、CORS、SDP、ICE、codec、browser session、coordinate frame、录制和权限；它会重新成为全局 Service Locator 和协议聚合点。**拒绝。**

### 5.2 方案 B：浏览器直接接 Zenoh

浏览器直接订阅 key expression 并发布控制消息。

它减少一跳，但把内部 keyspace、Schema、重连、权限和 topology 暴露给不可信客户端；浏览器 transport session 被误当成 Principal/Capability，Console 也会与内部 wire format 锁死。**拒绝。**

### 5.3 方案 C：一个全能 Web/Console/XR Bridge

一个进程同时托管静态网页、OPS、日志、WebSocket、WebRTC、XR 坐标、camera proxy、录制和遥操作。

初期部署方便，但媒体 CPU 峰值、日志慢消费者和遥操作输入共享故障域；同一对象既是 cache、session owner、transport terminator 又是 command ingress，无法建立清晰的 failure-containment/recovery 与权限边界。**拒绝作为逻辑架构。** development profile 可以共置，但 contracts、budgets、domains 和 observed state 仍必须分离。

### 5.4 方案 D：ConsoleGateway + WebRealtimeGateway + typed internal boundary

ConsoleGateway 只暴露 InspectionProtocol/OpsProtocol；WebRealtimeGateway 终止 WebRTC 与 XR 外部会话；InspectionService、OpsService、媒体、遥操作、Authority 和内部 Fabric 保持独立 owner。组件可以按 DeploymentProfile 共置或拆分。**推荐。**

### 5.5 方案 E：为 HTTP、WebSocket、WebRTC 各建一个内部 Backend

它看似传输可插拔，实质会把不同协议压到最小公分母，并复制 routing、retry、backpressure、identity 和 telemetry。ParaEGOX 内部只保留 Zenoh production Fabric；外部协议用 Gateway adapter。**拒绝。**

## 6. 目标结构

```text
Browser Console / Mobile / Quest / WebXR Client
        │
        ├── HTTPS snapshot/query/operation + SSE/WS watch
        │                         │
        │                         ▼
        │                  ConsoleGateway
        │                    ├── InspectionClient ──> InspectionService
        │                    └── OpsClient ─────────> OpsService ──> Authority / typed owner
        │
        └── authenticated HTTPS signaling + WebRTC
                                  │
                                  ▼
                           WebRealtimeGateway
                             ├── WebSessionCoordinator
                             ├── JSEP/WHIP/WHEP adapters
                             ├── WebRTC media adapter
                             ├── WebRTC data adapter
                             ├── XR semantic adapter
                             └── IceCredentialProvider
                                  │
          typed Port / Service / Operation boundary（合同待 W0 冻结）
                                  │
          ┌───────────────────────┴────────────────────────┐
          ▼                                                ▼
Camera/Audio/Encoder Card                    Teleoperation owner / Controller-role Card
          │                                                │
          └── compiled PortBinding ── Zenoh Fabric         ▼
                                              CommandEndpoint
                                                   ▼
                                  Authority → Lease/Fencing → Safety
                                                   ▼
                                          Driver EnforcementPoint
                                                   ▼
                                                Receipt
```

这是逻辑结构，不要求每个框都是单独进程。推荐生产起点是：

- ConsoleGateway 单独故障域；
- ConsoleGateway 只按 Web exposure 部署，不建立每 Node 一个 Gateway；TUI/CLI 可直接使用 InspectionClient/OpsClient；
- 首个分布式 profile 使用一个管理侧 OpsService 与 federated Inspection role 服务多个 Node；node-local Inspection 在 federated role 故障时仍可查询；
- WebRealtimeGateway 的 signaling/control ingress 与 codec worker 分开 ExecutionDomain；
- 编码/native media pipeline 在 ProcessDomain、外部 worker 或硬件编码器中运行；
- XR high-rate ingress 不与日志、SSE fan-out 或视频编码共享无界 backlog；
- development profile 可以共进程，但必须报告实际 Domain、queue、peer、codec、CPU 和 drop facts。

内部 production Zenoh route 由 Rust Fabric owner 通过原生 Zenoh API 持有。无论 Gateway 自身使用什么语言，它默认只获得编译后的 typed Port/Service/Operation client；不能持有 raw Zenoh Session、复制 route/reconnect owner，或因使用 `zenoh-python`/C++ API 形成第二 Fabric。只有显式 Fabric-scoped CapabilityGrant 和独立审计场景才允许受控原生访问。

## 7. 五条不能混合的数据与控制路径

### 7.1 Console 读取路径

```text
Web Console → ConsoleGateway → InspectionClient → InspectionService snapshot/watch
TUI / CLI ────────────────────→ InspectionClient → InspectionService snapshot/watch
```

- Snapshot、delta、log stream 都是投影，不是 Runtime/Deployment/Evidence 的真值 owner。
- 每份缓存值携带 source revision/epoch、`observed_at`、freshness 和 stale/unknown。
- SSE 适合单向增量；确需双向订阅控制时才使用 WebSocket。
- 慢消费者使用 cursor、bounded buffer、drop/resync 或重新取 snapshot，不能反压控制面。

### 7.2 行政运维写路径

```text
Web Console → ConsoleGateway → OpsClient → OpsService ControlRequest
TUI / CLI ───────────────────────────────→ OpsClient → OpsService ControlRequest
        → Authority → DeploymentController / actual typed owner
        → owner Receipt/EvidenceRef → progress / terminal OpsReceipt
```

ConsoleGateway 不直接写 RuntimeHost、PID、Zenoh key、DeploymentPlan store 或审批状态。ControlRequest 必须携带 request id、target/action、expected revision/epoch、canonical digest、deadline、principal/approval refs 与 dry-run；OpsService 只拥有 request lifecycle，实际 owner 仍在本地准入。timeout/断连进入 `Uncertain` 并 query/reconcile，不能透明 replay。

### 7.3 视频与音频路径

```text
Camera/Audio Driver
  → `MediaSample` / `EncodedVideoSample` typed Port
  → optional Encoder Card backed by CardDefinition
  → compiled PortBinding
  → session-local/SHM/host-local/remote Zenoh route
  → WebRealtimeGateway
  → WebRTC SRTP media track
  → Browser
```

- `FrameRef` 已保留给物理坐标系引用，不能表示视频帧；媒体 payload 使用 `MediaSample`/`EncodedVideoSample` 等领域类型，并通过 `BlobRef`/`BufferRef` 表达大载荷所有权和生命周期。
- Driver 负责设备/SDK 边界，不负责公网 HTTP server、PeerConnection 或浏览器 auth。
- Encoder 负责 codec transform；硬件直接输出 encoded stream 时，Driver 可以暴露受版本约束的 encoded Port，但 peer adaptation 仍归 Gateway。
- WebRealtimeGateway 负责 peer-specific track、packetization、bitrate/keyframe、congestion 和 session stats。
- 媒体 bytes 不默认进入 Evidence；Evidence 只保存授权、会话、录制策略、内容 digest/ref 和操作 Receipt。需要原始录制时由显式 Recording/Media owner 管理 retention、consent 和访问权限。

### 7.4 XR 连续输入路径

```text
WebXR pose/controller
  → DataChannel or bounded WebSocket
  → XR semantic adapter
  → XRInputEnvelope / TeleopSignal Port
  → Controller-role Card or TeleoperationService
```

`XRInputEnvelope` 的候选字段至少包括：

- 限定的 browser auth session ref、`WebRtcPeerRef/PeerEpoch`、`XrInputStreamRef/StreamEpoch` 与 sequence；
- `PrincipalRef` 或不可伪造的 session-to-principal binding；
- source monotonic timestamp、gateway received time、deadline/freshness；
- browser reference space、目标 `FrameRef`、transform/calibration revision；
- pose/controller schema version、unit、uncertainty/quality；
- control-mode/teleop-session reference。

浏览器坐标不能直接冒充机器人坐标；Gateway 只执行已版本化的坐标与 Schema 转换，Teleoperation owner 或 Controller-role Card 决定输入如何参与控制。

### 7.5 物理遥操作路径

```text
TeleopSignal
  → Controller-role Card / TeleoperationService
  → CommandEndpoint
  → AuthorityDecision
  → LeaseGrant + FencingToken
  → local Safety / deadman / mode gate
  → Driver EnforcementPoint
  → device-native safety gate
  → EffectReceipt
```

DataChannel ACK、WebSocket send success、Gateway accepted 或 `RTCPeerConnection.connected` 只能证明各自阶段，不能产生 `Succeeded` 物理 Receipt。

## 8. WebRTC 会话与 signaling 边界

WebRTC 没有完整的标准应用 signaling，因此 ParaEGOX 需要 Gateway-owned session protocol。首版建议：

1. 通过 authenticated HTTPS 创建短期 browser/signaling session，返回允许的 media/data tracks、codec/profile、expiry 和 signaling endpoint；不建立跨所有领域复用的泛 `ExternalSessionId`。
2. 由 WebRealtimeGateway 直接拥有 SDP offer/answer、trickle ICE、PeerConnection 和终止；避免 ConsoleGateway → XR server → camera endpoint 的多级 SDP proxy。
3. 同源 reverse proxy 可以做字节路由和 TLS ingress，但不是 session、Authority 或 media truth owner。
4. 每次 reconnect/PeerConnection replacement 产生新的 peer/session epoch，旧 channel 和旧 input sequence 立即被 fence。
5. 每个 session 的 allowed tracks、source refs、data schemas、rate/byte budget、viewer/controller role 和 expiry 都必须显式。
6. STUN 只参与地址发现/连通性；公网 profile 必须提供正式 TURN 中继、短期凭证、容量、地域、失败与成本观测。
7. Transport fallback 必须是显式 route/profile transition；同一 active input binding 不得同时由 WebSocket 和 DataChannel 双投。

协议 profile 建议：

| 场景 | 首选 |
| --- | --- |
| Console query、日志、部署与普通 operation | HTTPS + SSE/WebSocket |
| 单机器人双向遥操作 | authenticated custom JSEP + WebRTC media/data |
| camera/encoder 向中心媒体服务单向 ingest | WHIP adapter |
| 中心媒体服务向 viewer 播放 | 可替换 WHEP adapter；保留 JSEP fallback，不能写入 Kernel contract |
| 少量 viewer 的 robot-edge 直连 | WebRealtimeGateway 直接终止 WebRTC |
| 多 viewer、共享转码或录制 | 有证据后引入 MediaService/SFU，不由 Runtime 自动承担 |

## 9. WebXR 语义边界

WebXR 前端 SDK/应用拥有：

- 浏览器 immersive session 与 user activation；
- reference space、head/controller pose、buttons/axes、render loop；
- 本地 UI、提示、permission 请求和可选 haptics；
- 将浏览器事件编码为版本化外部 payload。

XR semantic adapter 拥有：

- 外部 session 与 Principal 的绑定；
- payload size/rate/schema/sequence/deadline admission；
- browser time 到可解释时间字段的转换与 uncertainty；
- browser reference space 到 ParaEGOX `FrameRef`/CalibrationRef 的显式映射；
- continuous signal 与 discrete operation 的分流；
- malformed、late、replayed、wrong-session input 的拒绝事实。

XR adapter 不拥有：

- 控制模式仲裁；
- 机器人资源 lease；
- SafetyDecision；
- Driver/Actuator；
- WebXR 客户端设备身份到 Node/Card/Instance 的等价映射。

WebXR 静态资源属于 `apps/console` 或独立前端 Artifact。离线机器人 profile 可以由 ConsoleGateway/静态文件服务托管该 Artifact，但不能把整份 HTML/JS 嵌入 Card 实现类并让其拥有 Runtime 生命周期。

## 10. DataChannel 与 WebSocket 的客观选择

| 数据 | 推荐通道 | 理由 |
| --- | --- | --- |
| signaling、登录、session create/revoke | HTTPS | 容易鉴权、代理、限流、审计和重试控制 |
| Inspection delta、log tail | SSE 或 WebSocket | 支持 cursor/resync；不需要 WebRTC |
| 高频 pose、head/controller analog signal | DataChannel 候选 | 可用 unordered/partial reliability，避免旧样本阻塞新样本；必须先基准 |
| arm/disarm、mode switch、任务提交 | 可靠 Operation API | 需要明确 Authority、deadline、幂等与 terminal Receipt |
| haptic/teleop feedback | DataChannel 或 typed reliable channel | 依据 freshness 与丢失容忍度声明，不与物理 success 混淆 |
| 音视频 | WebRTC media track | 由 congestion/media pipeline 管理 |

DataChannel label 不能直接映射为 Zenoh key expression。Gateway 必须显式映射到已经允许的 Schema/Port/Operation，并执行 bounded ingress、principal/session、freshness 和 capability 检查。

## 11. 身份、权限与会话

需要分开以下身份和短期会话，不能压成一个泛 `SessionId`：

| 身份 | owner | 生命周期 |
| --- | --- | --- |
| 外部用户/设备身份 | OIDC/mTLS/WebAuthn 等外部 identity adapter | 由外部身份系统决定 |
| `PrincipalRef` | ParaEGOX Identity/Authority boundary | 稳定内部主体引用 |
| browser auth session ref | Console/Web Gateway | 短期 browser/API 登录会话 |
| `WebRtcPeerRef/PeerEpoch`（候选） | Media Gateway role | 每次 PeerConnection replacement 变化 |
| `XrInputStreamRef/StreamEpoch`（候选） | XR Input Gateway role | 每次 XR input stream replacement 变化 |
| `TeleoperationSessionRef`（未来候选） | Teleoperation owner | 与控制权、mode、lease 关联；不由 Transport 创建 |

规则：

- 登录 Console、TLS 建连、WebRTC DTLS 成功都不自动产生机器人控制权限。
- Gateway 将外部身份映射到 `PrincipalRef`，真正内部访问使用 audience/scope/expiry/subject-incarnation 受限的 `CapabilityGrant`。
- browser/peer/XR stream session 不能冒充 Node、Card、CardInstance、DeckRun、FabricSession、AgentSession 或 DeviceSession。
- session token 不放 query string；CORS、CSRF、origin、cookie/token policy 和 rate limit 必须按 endpoint 类型显式配置。
- TURN 使用短期、session-bound credential；浏览器不能获得内部 Zenoh endpoint 或永久 Fabric secret。
- media/view/control/recording 权限分开授予；能看视频不等于能控制，能控制不等于能录制。

## 12. RuntimeHost、Deployment、CardDefinition、Card 与 Deck

### 12.1 RuntimeHost 与尚未冻结的 managed Gateway envelope

当前 ParaEGOX RuntimeOwnershipTree 已定义 CardInstance/ServiceInstance，但尚未定义 Gateway 如何成为受管理 workload；Deck Link 也只连接 Card Port，因此“RuntimeHost 是否拥有 Gateway lifecycle/recovery”和“Gateway 直接参与 planned PortBinding”目前都还不是完整合同。本研究不为填空而把 Gateway 伪装成 CardDefinition、Card、CoreService，也不直接发明公共 `GatewayInstance`。

建议在 W0 比较并冻结：

- 复用 Runtime 中性的 managed-instance execution descriptor，由上层保留 Gateway 语义；
- 或使用现有 ServiceSpec/ServiceInstance 作为长期运行外壳，但不能让它吞并 Gateway 协议语义；
- externally service-managed Gateway 由 systemd/container/external workload manager 持有生命周期，ParaEGOX 只通过窄 `ExternalWorkloadAdapter` 请求和观察；
- internal Card Port 到 Gateway endpoint 使用 Deployment-owned exposure/binding、窄 ServiceContract 还是限定 Gateway endpoint contract。

合同冻结后，ParaEGOX-managed Gateway 的 RuntimeHost 才负责：

- managed Gateway workload 的 ExecutionDomain、预算、启动、排空、取消、liveness/recovery 和 Inspection；
- signaling loop、XR ingress、codec worker 的实际 PID/TID/loop/capacity/epoch 对账；
- bounded queue、retained bytes、FD/socket/SHM 和 child process cleanup；
- revision transition 时 prepare/activate/drain/retire/rollback。

RuntimeHost 不拥有 SDP、ICE candidate、codec policy、XR frame、browser token、media session truth 或 teleoperation policy。Gateway 不得私自创建 daemon thread/private event loop；native codec 无法可靠取消时进入 ProcessDomain 或明确 external workload。

Rust RuntimeHost 也不把 async task cancellation 当作 native codec、socket、GPU 或外部进程已经停止的证明。Python/C++ Gateway worker 的 versioned ProcessDomain protocol 必须携带 generation、credits、deadline/cancel、terminal status 与 process census；dispatch 后无终态时进入 `Uncertain → query/reconcile`，不能透明重放 peer/control effect。

### 12.2 DeploymentController

目标规则是：一旦 managed Gateway/exposure contract 冻结，长期存在的 ConsoleGateway/WebRealtimeGateway workload、endpoint policy、placement、Artifact digest、resource budget、allowed codec/profile 和静态内部连接属于 Deployment desired state。以下变化通常产生新 DeploymentRevision：

- Gateway Artifact/config/endpoint/placement 变化；
- 长期 listening endpoint、trust policy 或 allowed feature set 变化；
- internal endpoint/binding、execution budget、LivenessSpec、FailureContainmentSpec 或 RecoveryPolicy 变化。

每个用户登录、PeerConnection、XR immersive session、ICE restart 或 viewer join/leave 都是 Gateway 内的 ephemeral state，**不产生 DeploymentRevision**。Gateway 只把 observed session facts投影给 Inspection。

### 12.3 CardDefinition、Card 与 Deck

- Gateway/Driver/CoreService 不因为“是一个组件”就必须拥有 CardDefinition。
- 可复用 encoder、format converter、XR pose mapper 只有在具有独立配置、复用、实例隔离、ExecutionRequirements 和观测边界时才成为 CardDefinition。
- Deck 通过 Card In/Out、Service/Permission/Feature requirement 表达应用媒体/teleop 意图，不选择 WebRTC、TURN 或 Gateway placement；DeploymentProfile/Exposure policy 决定是否部署 Web Gateway 组合。
- Card Port 与非 Card Gateway endpoint 如何形成 authoritative planned connection 尚待 W0/ADR；在此之前不能声称 Deck Link 已可直接连接 Gateway，也不能为了复用 Link 强制为 Gateway 定义 CardDefinition。
- PeerConnection、browser tab、XRSession 不是 CardDefinition、Card、CardInstance 或 DeckRun。
- 单应用的 teleop controller 可以是 Controller-role Card；只有跨 Deck 共享控制权、session/policy 状态时才研究 TeleoperationService。

## 13. 执行隔离、有界性与“Lane”关系

本研究不恢复公共 Lane。必须表达的是：

| 工作 | 执行要求 |
| --- | --- |
| HTTPS signaling/auth/session | bounded async I/O；不得执行 codec 或物理领域逻辑 |
| Console SSE/WS fan-out | background/interactive budget；慢消费者隔离、cursor/resync |
| WebRTC packet/peer callbacks | fixed-cost admission/handoff；不得阻塞或完整执行业务逻辑 |
| 视频编码/转码 | CPU/GPU/native 风险，通常 ProcessDomain/external worker/accelerator |
| XR high-rate ingress | 独立 bounded latest-value/age budget；不能被日志或编码拖住 |
| discrete teleop operation | control budget、deadline、Authority、Receipt；不能被 media backlog 反压 |

CardDefinition `ExecutionRequirements`、Link `DeliveryProfile`、目标 Node facts 和 Deployment policy 编译为 `DeploymentPlan.execution`。Runtime 内部可以使用 ready-queue group，但它不是公共 Lane，不拥有自己的线程、第二份 payload backlog 或独立生命周期。

媒体和 XR 的 ingress、Mailbox、inflight、codec buffers、PeerConnection send queue、retained media payload/SHM、browser fan-out 全部需要 items/bytes/age/credits 预算。扩大视频队列不是降级策略；对实时媒体和 pose，过期数据通常应丢弃并报告，而不是排队等待。

## 14. 断线、重连与安全收敛

| 故障 | 必须行为 |
| --- | --- |
| ConsoleGateway 崩溃 | 本地 Runtime/控制/Safety 与已接受 ControlRequest 继续；Web 客户端无法发起新请求并显示 unavailable/stale，TUI/CLI typed clients 不受 Web BFF 故障直接影响 |
| OpsService 崩溃 | 无法接受新 ControlRequest，在途协调进入 unavailable/恢复查询；DeploymentController reconcile、RuntimeHost、node-local Inspection 与 Safety 继续，恢复后不透明重放 |
| federated Inspection role 崩溃 | 聚合视图 unavailable/stale；source facts 与 node-local query 继续，恢复后按 source revision/epoch/cursor resync |
| WebRealtimeGateway 崩溃 | media/XR session 失效；旧 peer epoch 被 fence；本地 deadman/lease 按策略收敛 |
| WebRTC media 断流 | media track unavailable；是否暂停 teleop 由显式 TeleoperationPolicy/ODD 决定 |
| DataChannel/WS 输入断流 | 不依赖 disconnect callback；本地 freshness/deadman/lease expiry 触发 safe behavior |
| TURN 不可用 | session 显式失败或降级为被批准的 profile；不静默暴露内部 endpoint，不自动切换双 active transport |
| Fabric 分区 | 不接受新的未授权远端控制；本地 Safety/最低自治继续或 fail-closed |
| peer reconnect/ICE restart | 新 peer/session epoch；旧 input、old channel 和 delayed callback 拒绝 |
| Gateway overload | 先按 profile 丢弃过期媒体/pose、拒绝新 session；不让 control/Safety 路径被后台 fan-out 挤压 |
| shutdown/revision rollout | 停止新 session → revoke/expire control → drain bounded media → close peer → cleanup worker/FD/SHM；不得遗留 daemon task |

Gateway 可以在 disconnect callback 中额外发送 neutral/stop 请求，但它只是补充动作，不能作为安全证明。真实收敛依赖目标 Node 的单调时钟、lease/deadman、fencing、Safety 和下游 safe-output gate。

## 15. Inspection、Evidence、日志与隐私

WebRealtimeGateway 至少投影：

- 限定的 browser auth session、WebRtcPeerRef/PeerEpoch、XrInputStreamRef/StreamEpoch、principal ref、role、created/expires 和 state；
- signaling/ICE/DTLS/peer state、selected candidate/relay class；
- codec/profile、track state、bitrate、RTT、jitter、packet loss、NACK/PLI/keyframe；
- DataChannel state、ingress rate/bytes、late/replay/schema/auth rejection；
- codec queue、drop、frame age、CPU/GPU/native worker health；
- session revoke、grant/lease refs 和断线收敛状态。

ConsoleGateway 至少投影 cache source revision、observed_at、staleness、watch lag、connected clients、slow-consumer drop/resync 和 operation correlation。

InspectionService 另行拥有 projection revision/cursor/cache/freshness；OpsService 另行拥有 ControlRequest journal/progress/OpsReceipt。三者即使 development profile 共进程，也不能共享真值 owner、权限或用 Gateway cache 恢复 operation。

必须分层：

- Inspection/Metric/Trace/Log 用于观测和诊断，可采样。
- AuthorityDecision、Operation Receipt、lease/fence、session grant/revoke 和录制 consent 属于权威 Evidence 候选。
- 原始音视频默认不是 Evidence；录制是独立 product/security decision。
- 浏览器媒体、XR pose 和用户身份可能包含敏感信息，retention、export、redaction、access 与 deletion owner 必须显式。

## 16. 建议包结构

以下目录表达目标 owner，不要求现在创建空包：

```text
apps/
└── console/                     # Web Console 与 WebXR client Artifact

src/paraegox/
├── gateways/
│   └── web/
│       ├── console/             # HTTP/SSE/WS → InspectionClient/OpsClient
│       └── realtime/
│           ├── sessions/        # Gateway-local browser/peer/XR scoped state
│           ├── signaling/       # authenticated JSEP；WHIP/WHEP adapters
│           ├── webrtc/          # ICE/DTLS/SRTP/media/data termination
│           └── xr_adapter/      # WebXR payload → typed XR/teleop contract
└── services/
    ├── inspection/              # node-local/federated read-only projection
    ├── ops/                     # ControlRequest journal/coordination only
    ├── media/                   # 条件式：多 viewer/transcode/record/SFU 后再建
    └── teleoperation/           # 条件式：跨 Deck 控制权/session policy 后再建
```

`WebSessionCoordinator` 首版只是 WebRealtimeGateway 内部协作组件，不自动成为 CoreService，也不拥有一个跨 browser/peer/XR/teleop 领域复用的泛 Session 类型。只有至少三个真实消费者需要共享同一类 session state，且其一致性、分代和恢复语义已明确时，才研究独立 Session Service。

## 17. 分期实施与验证

本工作流不阻塞 Kernel P0–P3，但必须建立在已有 owner/contract 上：

| 切片 | 前置 | 结果 | 首次验证 |
| --- | --- | --- | --- |
| W0 contracts/threat model | P0 边界、Identity/Grant/Receipt seam | managed Gateway/exposure seam、限定 browser/peer/XR session、公开 Console protocol、禁止依赖 | import/schema/threat review；尚不启动 server |
| W1 local read-only Console | P6a node-local Inspection | snapshot + SSE/WS watch；无写操作 | stale/unknown、慢消费者、restart/resync |
| W2 distributed Console/OPS | P5 + P6b | federated Inspection + OpsService ControlRequest | partition、expected revision、same-id/different-digest、Uncertain reconcile、single terminal OpsReceipt |
| M1 view-only WebRTC | P4 + 高带宽 payload ownership | simulated/recorded camera → browser media | ICE/TURN、loss/jitter、codec worker crash、bounded frame age |
| X1 WebXR view-only | P1 physical frame/time/calibration contract + M1 | headset pose/input 只进入 Inspection/simulation | secure context、frame/calibration mismatch、late/replay rejection |
| X2 simulated teleoperation | P3 + P6a + X1 | XR input → full simulated Command/Lease/Safety/Receipt | disconnect、reconnect fencing、deadman、mode handoff、video-loss policy |
| M2 production media edge | P5/P6b + target benchmark | robot-edge/central placement、TURN、optional WHIP/WHEP | multi-network、credential expiry、cost/capacity、rollback |
| X3 real teleoperation | H1 per-device gate | 受限 ODD、最小资源/速度的真实控制 | HIL、独立 E-Stop/deadman、activation expiry/revoke、人工接管 |

Python SDK、Gateway/worker 与 browser 辅助命令使用 `uv`；Rust native mechanism 使用 Cargo。WebRTC/codec/browser 测试应进入可选 dependency group，不能阻塞 Kernel 最小环境；发布 gate 还需覆盖 Rust↔Python/C++ 的 canonical Schema、IPC、buffer ownership、error/terminal-state 与 cleanup conformance。

### 17.1 最小验证矩阵

1. ConsoleGateway 在没有 RuntimeHost 私有 import、Zenoh Session 和数据库内部句柄时仍可用公开 fixture 测试。
2. 浏览器 client 不能通过修改 URL、DataChannel label 或 payload target 访问未声明 Port/Operation。
3. Peer reconnect 后旧 epoch 的 pose/command 无法进入当前 teleop session。
4. 高频媒体、日志和 XR 洪峰下，discrete control 的 admission/start latency 与 queue age 仍满足声明；所有 buffers 有界。
5. codec/native worker wedge 不阻塞 signaling、session revoke 和 local deadman。
6. TURN-only、direct ICE、packet loss、reorder、bandwidth drop、peer restart 和 Gateway kill 都有可解释事实。
7. DataChannel send/ACK 不产生物理 `Succeeded`；只有 effect owner 的 Receipt 可以。
8. 关闭/revision rollout 后无残留 PeerConnection、Task、Thread、Process、FD、socket、SHM 或 retained media payload。
9. XR timestamp/frame/calibration/sequence/uncertainty 缺失、过期或错代时 fail-fast。
10. 断开 browser、Gateway、Fabric 或 OpsService 任一层，本地 safety/deadman 均按 profile 收敛；OpsService 断开不停止 DeploymentController reconcile。
11. Rust RuntimeHost + Python/C++ codec/Gateway worker crash、cancel、deadline 和 version mismatch 不产生 orphan、双 restart owner、双 Zenoh route或伪 terminal success。
12. safe Rust、Tokio 或低平均延迟不被当作 WebRTC/XR 硬实时或功能安全证据；目标平台 worst-case benchmark 与本地 safety island gate 仍独立通过。

## 18. 已确定、待 ADR 与仍开放的边界

### 18.1 已确定的架构方向

- 不恢复宽泛 `runtime/io`。
- Console 是客户端，ConsoleGateway 是窄 InspectionProtocol/OpsProtocol 外部边界；InspectionService 与 OpsService 是独立 CoreService，且都不是每 Node 必备的 Console Bridge。
- WebXR 不是 transport；WebRTC 是外部实时协议，不是内部 Fabric backend。
- Zenoh 保持唯一内部 production Fabric。
- 首个 production Fabric 使用 Zenoh 原生 Rust API；Gateway 语言保持可选，且不能因此取得 raw Fabric owner。
- 浏览器、DataChannel、WebRTC session 不直接获得 Node/Card/Instance identity 或物理 Capability。
- Driver/Camera Card 不拥有公网 WebRTC server；RuntimeHost 不理解协议语义。
- 遥操作断线安全依赖本地 lease/deadman/fencing/Safety，不依赖 best-effort stop packet。
- 每个 active XR/control input 只有一种 active external route；禁止 WebSocket/DataChannel 双投和隐式 fallback。

### 18.2 Proposed ADR 候选

进入 W1/M1/X2 代码前，建议分别评审：

1. Web Gateway managed-workload、external exposure/typed endpoint、ConsoleGateway、OpsService、RuntimeHost 和 Zenoh Fabric 的所有权与禁止依赖；Inspection/Ops 边界已由 ADR-0003 提议冻结。
2. browser auth session、WebRTC peer、XR input stream、未来 teleop session 的限定 identity/epoch、Principal 映射、grant/revoke 与 reconnect fencing。
3. WebRTC signaling/ICE/TURN、media/data profile 与 single-active external route。
4. XR input 的时间、坐标、校准、freshness、teleop deadman 和 physical control boundary。

不需要把具体 Web 框架、aiortc/SFU 产品、codec 列表或首版 TURN 部署写入长期 ADR。

### 18.3 开放问题

- ConsoleGateway 与 WebRealtimeGateway 的首个 production profile 是否同 Node；逻辑 owner 不合并。
- managed Gateway 是复用 Runtime 中性 instance envelope、使用 ServiceInstance 外壳，还是需要新的限定 contract；不因语义角色直接新增万能 `GatewayInstance`。
- Card Port 与 Gateway external exposure/internal endpoint 的唯一 desired/observed owner、连接和 revision 模型。
- custom JSEP、WHIP/WHEP 的目标产品组合；WHEP 未标准化前必须可替换。
- 何时由 robot-edge 直接 WebRTC，何时引入中心 MediaService/SFU。
- DataChannel 与 WebSocket 对目标 XR 设备的 latency、loss、background behavior 和功耗基准。
- camera/encoder/Gateway 间 `MediaSample`/`EncodedVideoSample` 的 BlobRef/BufferRef、SHM lifetime、zero-copy 与 codec negotiation。
- TURN region、credential issuer、quota、relay cost、日志隐私和高可用 owner。
- OIDC/mTLS/WebAuthn 的产品选择与 headless/device enrollment 流程。
- 浏览器 reference space 到机器人 FrameGraph 的校准 UX、revision 和失效流程。
- 视频或 haptic feedback 过期时是否撤销 teleop lease，以及具体 ODD 门槛。
- media recording/retention/export/redaction/consent 的 service owner。

这些开放问题不阻塞 W0 合同研究，也不构成把协议重新塞入 Runtime 的理由。

## 19. 研究结论

推荐按方案 D 修订 ParaEGOX 架构与计划：

```text
external browser protocols
        ↓ terminate + authenticate + translate
ConsoleGateway / WebRealtimeGateway
        ↓ stable ParaEGOX contracts
OpsService / InspectionService / typed Port / Operation
        ↓
Zenoh-native Fabric + Authority/Lease/Safety owners
```

后续实现首先交付只读、可故障注入的最小纵向切片，不建立总 `runtime/io`、全能 Console Bridge、浏览器直连 Zenoh、Driver 自托管 WebRTC 或每个 peer 一次 DeploymentRevision。Rust-first mechanisms 只固定 Runtime/Fabric 参考底座，Gateway 和 codec workload 继续按 polyglot fault boundary 部署；只有多消费者和共享状态的真实证据出现后，才提升 MediaService、TeleoperationService 或 Session Service。
