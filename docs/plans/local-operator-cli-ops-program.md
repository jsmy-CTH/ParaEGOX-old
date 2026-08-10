# Local Operator CLI/Ops Program

> 状态：Active
> Program ID：`local-operator-cli-ops`
> 授权日期：2026-08-10
> 最近重排：2026-08-10；先完成本地可见闭环，再进入远端 artifact 路线，OpsService 最后准入
> 授权来源：当前工作区用户明确要求优先完成 CLI、部署查看、Inspection/Ops 路线与可验证 TUI，并冻结新的 Remote Agent 扩张
> 当前 committed anchor：`4334a59af1656429f0401c0b780134c8871148e9`；工作区中的后续 r352 与其他候选改动必须原样保留，但未提交、未验证的内容不构成完成证据
> 当前最近动作：M2a 三条生命周期 grammar、lifecycle JSON v1、single-owner headless 机制与 Ubuntu system-harness 候选已形成；2026-08-10 已在独立 r352 overlay 上通过 macOS focused check/clippy/lifecycle tests，并真实验证并发 `up` 同 generation、一项 `changed = true`、一项 `changed = false` 与 joined `down`。这不是 immutable candidate 或 Ubuntu 完成证据；candidate commit 与 exact-ref Ubuntu Rust/完整治理/真实进程通过证据仍未形成。I0 `init` 已获实现授权，但代码或测试存在本身不构成完成证据

## 一句话结果

ParaEGOX 当前优先交付一条普通开发者可以初始化私有本地工作区、检查配置、构建并检查 immutable artifact、本地部署、启动、查看状态与证据、附着 TUI、替换、重启和回滚的 golden path；该本地闭环完成后，才进入显式 Node enrollment、只传输不激活的 `push` 与远端部署，最后才评审 OpsService 准入。Remote Agent 新能力在整个 Program 中继续冻结。

## 为什么重排

现有代码已经积累 Kernel、Runtime、Deployment、Node、Fabric、Model、Agent、Inspection 和本地组合机制，但交付顺序长期由底层 tranche 推动。用户最先需要的是“拿到东西后能初始化、能部署、能看见、出错能解释、失败能回退”，而不是继续扩张 remote contract、session、connector、proxy 或通用编排抽象。

现有 TUI 入口只在 `chat` 启动链中做一次严格的 Inspection `Latest` 读取，再显示启动状态。这个 one-shot 切片的目的，是先证明 owner 边界、IPC、失败关闭和 UI 启动顺序，不是假装已经有持续运维控制台。当前路线以此为已有证据，但先补齐可重复的本地安装/部署闭环，再让 Inspection、Evidence/日志和 TUI 作为并行可见性分支汇入。

## 权威与所有权边界

- 本 Program 是当前交付优先级的权威，但只在 Accepted ADR 与 [`governance.toml`](../../governance.toml) 已登记边界内生效。
- [ADR-0005](../adr/ADR-0005-typed-domain-graphs-and-runtime-assembly-boundary.md) 已 **Accepted**：保留 Deck、ServiceDependency 与 activation constraint 各自的 typed graph 及必要纯算法，不建设通用 Graph Engine、Graph Store、持久 Graph Schema 或中央 workflow runtime。本 Program 不能重新打开该决定。
- [ADR-0003](../adr/ADR-0003-ops-service-operation-boundary.md) 继续保持 **Proposed**。本 Program 不接受它，也不授权提前实现完整 OpsService、federated Inspection、ConsoleGateway 或 Web Console；OpsService 只能位于本路线最后的 O0 准入门。
- [ADR-0004](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md) 的 A0 gate 约束本路线：稳定 Release/Installation identity、active pointer、升级/卸载或 installation-owned state 不能被塞进 DeploymentController、Deck、CLI 或 `paraegox-local` 私有目录。
- 当前本地公共入口仍由 `DeveloperLocal composition root` 拥有。M1 离线 CLI 与 I0 本地初始化不创建新 owner，不取得 Secret、网络、服务生命周期或 domain durable-state mutation 权限；M2a 只在同一 composition root 内增加窄 lifecycle seam。
- `init` 只生成开发者本地配置工作区，不是安装器、Installation owner、Deployment owner 或 state owner；它不触发 A0。`deploy --local` 对稳定 artifact、installation record、active release 与回滚的需求会触发 A0，且必须先完成最小决策。
- Deployment desired state、Runtime apply、Node facts、Artifact/Release、Installation、Inspection projection、Evidence 和领域副作用继续由各自真实 owner 持有。CLI/TUI 只能调用公开 bounded seam，不能成为第二写者。
- “Ops”在本 Program 中先表示用户可操作、可诊断的产品路线，不等于 ADR-0003 所描述的持久化 OpsService 已经实现或被接受。

## 明确冻结与非目标

从本 Program 生效到 Program 复盘并获得用户再次明确解冻之间：

- 不新增 Remote Agent contract、runtime state、session、connector、proxy、public CLI、TUI 或 Web 功能。
- N0–N2 的 Node enrollment、artifact transfer 与 remote deploy 只属于 operator 部署路线，不构成 Remote Agent 解冻，也不得借用 Agent session、connector 或 proxy 充当传输/控制平面。
- 只允许为保持现有候选代码可编译、修复安全问题或修复已证明回归而做最小 Remote 变更；不得借此扩张能力声明。
- 不建设通用 workflow/saga/graph engine、Graph Service、Graph Store、Web Console、federated control plane 或生产 HA。
- 不通过 shell/SSH、直接写 store、私有 Runtime API、隐藏 fallback、透明 retry 或 workspace rsync 快速拼出 operator 命令。
- 不把进程存在、transport ACK、文件已复制、CLI exit 0 或日志文本推导为 Installed、Activated、Ready 或 RolledBack。

Remote Agent 解冻需要独立于 N0–N2：本地与远端 deployment golden path 有 exact-ref 可复核证据、Program 风险已复盘，并由用户明确修改本 Program 或建立后继 Program。

## 已冻结的首批 public grammar

### M1 — 离线检查

```text
paraegox version --json
paraegox config check <chat|node|deployment> --config <absolute TOML> --json
paraegox doctor <chat|node|deployment> --config <absolute TOML> --offline --json
```

共同约束：

- `version --json` 不加载配置。
- `config check` 与 `doctor --offline` 必须复用对应 `chat`、`node`、`deployment` 的严格配置 decoder，不能维护第二套宽松 schema。
- 三条命令只输出一个 bounded JSON object；非法 grammar、相对路径、错误 schema、未知字段或不安全文件必须稳定非零退出。
- 不解析 Secret value，不启动任何 owner/service，不打开网络，不创建或修改 state root。
- `doctor --offline` 只检查本机静态前置条件；它不声称 endpoint 可达、服务健康、Deployment 已收敛或 Agent 可对话。
- 首次实现与验证入口是 [`main.rs`](../../crates/paraegox-local/src/main.rs)、[`config.rs`](../../crates/paraegox-local/src/config.rs) 与 [`error.rs`](../../crates/paraegox-local/src/error.rs)。

### I0 — 本地初始化

首个且仅有的 I0 exact public grammar 冻结为：

```text
paraegox init --directory <absolute-directory> --json
```

I0 合同固定为：

- `<absolute-directory>` 必须是非根目录的 lexical-canonical absolute path，本身就是 private developer-local config workspace；命令不接受相对路径、默认目录、profile、provider、model、Secret、state-root 或网络参数覆盖。新建目录固定 mode 0700；既有目录只有在非 symlink、归当前 uid/gid、mode 0700 时才可接受，且不要求目录为空。
- 工作区内唯一生成的配置文件是 `paraegox.toml`，内容是现有严格 chat schema v1：`state_root = "<directory>/state"`、`fabric_listen = "tcp/127.0.0.1:7447"`、`[model] provider = "deterministic-echo-v1"`；不写 `model` 或 `secret_ref`。
- `state` 只作为配置中的未来 domain state path；I0 不创建该目录，不初始化 identity、owner、lifecycle record、socket、Deployment/Runtime/Node state 或任何其他 domain state。
- I0 完全离线，不读取 Secret，不启动 owner/service/child process，不打开 network socket，也不执行 artifact 安装、激活、迁移或权限授予。
- 新建 workspace 和 mode-0600 配置必须 private、原子发布且失败关闭。目标内容与固定模板 byte-identical 时成功返回 `changed = false`；任何文件、hardlink、类型、所有者、权限、symlink、发布临时对象或内容冲突都不得覆盖、合并、删除或“修复”既有数据。
- I0 不是 Installation owner，也不产生 Release、Installation、Deployment 或 Ops Receipt；因此它不触发 A0。首次 `deploy --local` 的稳定安装需求才触发 A0。

I0 JSON v1 的 top-level 字段严格且仅有：

```text
schema_version
command
ok
changed
profile
config_relative_path
state_relative_path
diagnostics
```

- `schema_version` 固定为 JSON number `1`；`command` 固定为 `"init"`；`ok` 与 `changed` 是 JSON boolean。
- 成功时 `profile = "deterministic-echo-v1"`、`config_relative_path = "paraegox.toml"`、`state_relative_path = "state"`、`diagnostics = []`。响应只返回稳定相对路径，不回显绝对 workspace 或 state path。
- 错误时三个字符串字段均为 JSON null，且 `diagnostics` 恰有一项；该项只有稳定 `code` 与 public-safe `message`，不得泄露 workspace 绝对路径、Secret 或底层未审计错误文本。`changed` 必须如实表示返回时已知可观察或不确定的持久变化：grammar/path/既有对象拒绝为 `false`；留下新 workspace、已链接最终配置，或清理/耐久性不确定时为 `true`；既有 workspace 中只创建过临时文件且已完成身份核验、删除与目录 fsync 时仍为 `false`。
- 对该 `--json` grammar，stdout 在可写时严格为一个 compact JSON object 加一个 LF，stderr 为空。成功为 exit 0；已识别 `init` 的 grammar/path/既有对象或发布冲突为 exit 2；I/O 或发布结果不确定为 exit 1。所有错误均为 `ok = false`，stdout failure 仍失败关闭。

除上述 M1、I0 与下文 M2a grammar 外，artifact、deploy、replace/restart、rollback、Inspection、Evidence/logs、TUI、push 和 remote deploy 的 exact command name、参数顺序与 JSON schema 尚未冻结。每个 Step 开始前必须登记 producer、consumer、owner、权限、失败语义、兼容规则和测试入口。

## Step DAG 与当前进度图

```text
                         ┌──────────────> I0 本地 init ───────────────────────┐
M0 文档/治理 ─> M1 离线 CLI ─> M2a headless 生命周期 ─> A0 最小决策 ─> A1 artifact ─> D0 local deploy ─> R0 replace/restart ─> D1 rollback ─┐
                                      └──────────────> M3 Inspection ───────────────┐                                                │
                                                               D0 ──────────────────┴─> M4 Evidence/日志 ─> M5 attach TUI ─────────┤
                                                                                                                                      ▼
G0 本地 golden path ─> N0 Node enroll/transfer ─> N1 push-only ─> N2 remote deploy ─> O0 OpsService 最后准入
```

依赖说明：I0 可在 M0/M1 收口后与 M2a exact-ref 收口并行，但 D0 必须同时等待 I0、M2a 与 A1；A0 是由 D0 的稳定安装需求触发，不是由 I0 触发。M3 可在 M2a 后与 A0–D1 分支并行；M4 同时等待真实 D0 Receipt surface 与 M3 read-only projection，M5 再消费二者。只有 D1 与 M5 都完成，G0 本地 golden path 才成立。N0–N2 不得越过 G0，O0 永远最后。

| Step | 当前状态 | 依赖 | 用户可见结果 | 完成证据 |
| --- | --- | --- | --- | --- |
| M0 文档与治理权威 | Active | 无 | 正式 docs 可版本化，只有 `docs/workbench` 保持本地；本 Program 成为当前路线 | candidate commit 追踪全部正式 docs；治理检查拒绝未追踪正式 docs 和已追踪 workbench；文档链接/状态检查通过 |
| M1 离线 CLI 首切片 | Active（候选实现已出现，尚未形成验收证据） | M0 | 用户能查询版本、严格检查配置、离线诊断静态前置条件 | exact grammar 正负测试、稳定 JSON/exit code、Secret/network/state 零副作用测试；对应 public API 登记 |
| M2a 本地 headless 生命周期 | Active（源码/测试候选与 macOS focused evidence 已形成，尚无 exact-ref Ubuntu 验收证据） | M1 | 用一份现有严格 chat 配置执行 `up/status/down`，三条操作共享一个 headless whole-local composition owner | exact grammar/JSON 正负测试；pre-state Secret 失败零副作用；并发同 generation 且 `changed` 与请求相关；private lock/record/same-user UDS、signal/joined shutdown、owned-socket 清理与 terminal record；同一 exact Ubuntu ref 的完整证据 |
| I0 本地 init | Authorized（实现切片已开始；尚无完成证据） | M1；可与 M2a 收口并行 | 一条离线命令生成 private deterministic-echo config workspace，不创建 domain state | exact grammar/JSON 正负测试；byte-identical 幂等 `changed = false`；文件/权限/symlink/内容冲突不覆盖；Secret/owner/network/state 零副作用；public API 登记与 exact-ref 平台证据 |
| A0 最小 Release/Installation 决策 | Planned | M2a；由 D0 需求触发 | 明确 artifact、release、installation、active pointer、DeploymentRevision 与 Receipt 分别由谁拥有 | 按 ADR-0004 形成并接受最小后继决策，或以稳定 diagnostic 拒绝 D0；不得用 plan 或代码存在代替 Accepted decision |
| A1 artifact build/inspect | Planned | A0 Accepted | 用户能构建 immutable artifact，并在无安装副作用时读取版本、digest、manifest 与目标兼容性 | reproducible bytes/digest、tamper/unsupported-target fail-closed、inspect 零安装副作用、producer/consumer 与兼容证据 |
| D0 local deploy | Planned | I0 + M2a + A1 | 本地执行 verify → install → activate → wait Ready → owner Receipt，而不是只复制文件或启动进程 | crash/partial failure、同请求幂等、old/new active 唯一性、Ready/Uncertain、失败不破坏旧实例、owner-issued Receipt |
| R0 replace/restart | Planned | D0 | 显式替换新 artifact；或在同 artifact/state 上安全停止后重启，不透明自动 retry | generation fencing、joined stop、SIGKILL/owner loss、stale generation、same-state restart、replace partial failure 与 no-double-active 证据 |
| D1 rollback | Planned | R0 | 指定已知 release/digest 回滚为新的前向 revision，并重新验证 Ready | target identity/compatibility、旧 artifact/state、rollback partial failure、Ready/Uncertain 与 Receipt correlation |
| M3 本地 Inspection | Planned | M2a | 用户读取带 source/revision/freshness 的 snapshot，并在授权后持续 watch | 只读协议与 owner 边界；断连、stale/unknown、backpressure、重连和 cursor 证据；不得写 desired state |
| M4 Evidence 与日志诊断 | Planned | D0 + M3 | 用户从失败定位到 owner、Receipt/Evidence 和有界日志，而不是只看“启动失败” | Secret-free 输出、bounded retention/query、owner-issued Receipt correlation、缺证据时 Unknown/Uncertain |
| M5 可附着 TUI | Planned | M3 + M4 | TUI attach 已运行实例并展示状态、原因与证据；detach 后服务继续 | attach/detach、断连/恢复、慢消费者、终端恢复、无隐藏写操作的人工与自动化证据 |
| G0 本地 golden path | Derived gate，未满足 | D1 + M5 | 新用户可复现 init 到 rollback 的完整本地路线 | immutable exact-ref Mac/Linux guide run、claim-to-evidence、已知限制；不是独立实现 Step |
| N0 Node enroll/transfer contract | Planned | G0 | 显式登记目标、信任、传输身份和暂存边界；不依赖 Remote Agent | target identity/host-key/trust pin、Secret-safe credential reference、transport interruption/resume/cleanup 与 staging integrity 证据 |
| N1 push-only | Planned | N0 | 只把一个 exact artifact 传到 target staging，不安装、不激活、不重启 | source/target digest 相等；重复传输幂等；中断无半成品；证明零 install/activate/restart/domain mutation |
| N2 remote deploy | Planned | N1 | 远端复用与 D0 等价的 verify/install/activate/Ready/Receipt 语义 | transport 与 operation identity 分离；断连/timeout 为 Uncertain；远端 rollback、两平台/两主机 exact-ref smoke 与无 SSH hidden fallback |
| O0 OpsService 最后准入 | Planned/Blocked | N2 + ADR-0003 Accepted 或后继决策 | 多客户端共享 durable ControlRequest/Receipt；真实 owner 继续执行副作用 | 至少两个独立真实客户端/operation consumer、crash-consistent journal、idempotency、Uncertain query/reconcile、owner Receipt；若准入条件不足则保持 deferred |

状态只表示已授权范围与当前验收证据，不使用百分比掩盖依赖或平台缺口。

## 各 Step 的最小交付合同

### M0 — 文档与治理权威

生产者是 maintainers 与治理检查器，消费者是所有后续实现和评审。正式文档必须进入 Git；local workbench 必须保持未追踪。回退时可以撤销 Program 的 Active 状态，但不能恢复为整个 `docs/` 被忽略，因为那会再次让架构和进度脱离版本历史。

### M1 — 离线 CLI

生产者是 `paraegox-local` 的公共命令分派与既有 config decoder，消费者是开发者、CI 和后续安装脚本。上线方式是 additive grammar；旧 `chat/node/deployment` 行为不变。若 JSON 或 grammar 不兼容，必须建立显式 successor，不能静默改变字段意义。

### M2a — 本地 headless 生命周期

生产者仍是 `paraegox-local` 的 DeveloperLocal composition root 与它拥有的窄本地 lifecycle seam，消费者是不理解内部 crate 的本地 operator 和自动化。M2a 只冻结以下三条 additive exact public grammar；参数顺序固定，不接受额外参数：

```text
paraegox up --config <absolute-chat.toml> --json
paraegox status --config <absolute-chat.toml> --json
paraegox down --config <absolute-chat.toml> --json
```

三条命令只接受同一份现有严格 DeveloperLocal chat schema v1；不建立第二套 lifecycle config，也不接受 Node/Deployment 配置、state-root、PID、provider/model 或 Secret 的命令行覆盖。`up` 复用现有 chat composition 的 Authority、Deployment、Runtime-owned Fabric/Model/Agent、NodeDaemon、owner-private Inspection 与 Agent IPC owner graph，但以 headless 形态运行，不启动 Textual child。对 provisioned profile，`up` 必须先沿用现有 chat seam 解析配置选择的 SecretRef，把 Secret value 单次移入既有 zeroizing owner 边界；Secret 缺失或非法必须发生在该请求创建或修改 lifecycle state root、owner lock、generation record、control socket 或任何领域 state/owner 之前。增加 operator lifecycle state 不放宽原有 pre-state Secret 保证。`status` 与 `down` 不解析 Secret value、不打开领域网络、不读写 Deployment/Runtime/Node 私有 store，也不另建 OpsService、RuntimeHost service manager、通用 daemon 或自动 restart owner。

三条操作共享一个 composition owner：生命周期全程持有的 private owner lock 串行化 owner 权限，config-bound CSPRNG generation 的 durable record 负责状态与关联，mode-0600 且校验同 uid/gid peer 的 private UDS 只传 bounded control request；PID/PGID 不写入记录，也不是控制或恢复权限。`up` 只有在当前 generation 已跨过现有 composition 的 bounded owner-readiness 边界且 lifecycle owner 仍存活时才成功返回 `running`；socket 存在或单独的 stdout marker 同样不是成功依据。并发 same-config `up` 只能由一个请求接受一个 generation，且每个响应的 `changed` 必须与该请求是否赢得该 generation 相关；跟随 winner 的请求不能借共享状态声称 `changed = true`。同配置已处于 `running` 时是成功且 `changed = false`；同 root/config authority 漂移必须在新 mutation 前失败关闭。`status` 只读并且 `changed` 永远为 `false`，它不执行 Inspection，也不把 lifecycle 存活分类升级为健康或收敛。`down` 通过同一个 lifecycle owner 触发现有反向 joined shutdown；只有所有已启动 owner 已 join、精确 owned socket 经 identity recheck 后 unlink 并 fsync 父目录、随后 terminal record durable publish 后才可返回 `stopped`。从未启动或已经安全停止时是成功且 `changed = false`。

M2a 不包含 `restart`、SIGKILL/owner 丢失后的 orphan recovery 或强制清理，也不包含健康/Inspection、日志/Evidence、attach/TUI 或任何 Remote Agent 扩张；它们分别属于 R0、M3、M4、M5 或冻结范围。

M2a lifecycle JSON v1 的 top-level 字段严格且仅有：

```text
schema_version
command
ok
state
generation
changed
owner_readiness_observed
inspection_checked
diagnostics
```

- `schema_version` 固定为 JSON number `1`；`command` 只能是 `up`、`status` 或 `down`；`ok`、`changed`、`owner_readiness_observed` 与 `inspection_checked` 都是 JSON boolean。
- `state` 只能是 `never_started | starting | running | stopping | stopped | failed | unknown`。`running` 只表示当前 lifecycle owner 存活且本 generation 曾跨过 bounded owner-readiness；它不是持续健康、Deployment 收敛、Agent 可对话或 Inspection freshness 声明。
- `generation` 在 mutation 被 owner 接受后是 bounded lower-case hex JSON string，在尚无 accepted generation 时为 JSON null；不得用 PID/PGID 充当 generation。`owner_readiness_observed` 对一个 generation 单调地记录是否曾跨过上述 readiness，因此在 `stopping`、`stopped`、`failed` 或 `unknown` 时也不得被解释成当前健康。
- `inspection_checked` 固定为 `false`。M2a `status` 不读取 Inspection，不输出 source、revision/epoch、observed time、freshness、stale、partial、reconcile-required 或领域 health；这些属于 M3。
- `diagnostics` 始终是 bounded JSON array；每项严格包含稳定 `code` 与 public-safe `message`，成功时为空。任何 envelope 都不得包含绝对/相对路径、state root、PID/PGID、Secret、SecretRef、credential、seed、private/signing key 或其内容。
- 对这三条 `--json` grammar，stdout 在可写时严格为一个 compact JSON object 加一个 LF，stderr 为空。Exit 0 与 `ok = true` 表示请求成功或 `status` 成功分类到非 `failed`/`unknown` 状态；exit 1 与 `ok = false` 表示 lifecycle/start/owner/join/output failure，或 `failed`/`unknown`；exit 2 与 `ok = false` 表示已识别 lifecycle 命令的 grammar、绝对配置路径、严格 chat 配置或 config-authority validation failure。不能交付 JSON 的 stdout failure 仍以 exit 1 失败关闭。

此处只冻结 contract 并授权 M2a 候选实现，不构成 milestone 完成或平台验证证据。验收必须把同一个 immutable exact Ubuntu ref 绑定到 locked Rust format/metadata/check/clippy/test、完整治理检查和 `tests/system/test_m2_local_lifecycle_cli.py` 的真实进程结果，并至少证明 pre-state Secret 失败零 lifecycle/domain state 副作用、并发 same-config 只有一个 generation 且 `changed` 正确关联、只读 status、joined down 后 owned-socket unlink/fsync 与 durable terminal record，以及 config drift/no-leak 边界。当前尚无这组 exact-ref Ubuntu 通过证据。2026-08-10 的本机 focused run 已在不合入未完成 Runtime S1 候选的独立 r352 overlay 上通过 `paraegox-local` all-targets check、Clippy `-D warnings`、19 项 lifecycle focused tests，并以真实进程证明并发 `up` 与 `down`；它没有 immutable candidate ref，也没有运行 Linux-only system harness，因此只作为开发诊断证据。macOS artifact workflow 对 lifecycle 仍只验证 help 可见性，不验证 `up/status/down` envelope、owner 或进程语义。

### I0 — 本地 init

生产者是 `paraegox-local` 的窄 filesystem adapter 与既有严格 chat decoder，消费者是新用户、开发自动化和 D0 guide。I0 只拥有“按固定模板原子创建 private config workspace”的一次性动作，不拥有其配置指向的 state，也不持有常驻 lifecycle。恢复只允许重新运行 exact 请求：byte-identical 完成态返回 `changed = false`；任何 uncertain 临时状态必须在发布前可安全清理或在下次请求中 fail-closed，不得覆盖既有对象。实现与测试必须同时冻结上述 grammar、JSON、mode/symlink/owner 检查和零副作用边界。

### A0 — 最小 Release/Installation 决策

A0 不是“创建完整 Application 平台”，而是按 ADR-0004 对 D0 的真实触发条件做最小准入。决策至少要分清 immutable Artifact bytes/digest、ProductRelease 或更窄 release identity、Installation identity/record/active pointer、DeploymentRevision、Runtime apply、owner Receipt 与 EvidenceRef；明确谁是唯一 writer、崩溃点、重复请求、partial install、兼容检查、卸载/retain 和 rollback authority。合法出口只有接受一份最小后继 ADR，或以稳定 diagnostic 拒绝 D0。I0、目录名或 CLI 命令名不能被当成 owner 证据。

### A1 — artifact build/inspect

build 只从 exact source/build inputs 产生 immutable、可重复验证的 artifact 与 manifest；inspect 只读 bytes 和 metadata，不创建 Installation、Deployment desired state 或 Runtime state。目标平台、build identity、digest、兼容范围与 canonical manifest 必须可机器读取。篡改、未知字段、unsupported target 或 digest mismatch 在任何安装副作用前失败关闭。

### D0 — local deploy

D0 是第一个真正触发 A0 的产品动作。其 owner-specific 状态机至少分离 `Verified → Installed → Activated → Ready`，失败时显式为 `Failed` 或 `Uncertain`；Transferred、Installed、Activated 与 Ready 永不互相推导。Artifact/Release owner 验证 bytes，Installation owner 管 installation record/active pointer，DeploymentController 管 committed desired revision，RuntimeHost 管本地副作用，各自签发自己的 Receipt。CLI 只汇总引用，不写这些 store。实现不得把这些步骤降低成通用 graph node 或让 graph scheduler成为恢复权威。

### R0 — replace/restart

R0 吸收原 M2b 的 restart/异常恢复，并在 D0 后增加真实 release replace。same-state restart 只有在前一 generation 已证明 joined stop 后才产生新 generation；replace 必须明确 old/new artifact、active pointer、DeploymentRevision、state compatibility 与 no-double-active。owner 丢失、SIGKILL、陈旧 generation、超时或证据缺失不能以 PID 消失合成 stopped，也不能自动透明 replay；应返回 Failed/Unknown/Uncertain 并提供 bounded recover/reconcile 路径。

### D1 — rollback

rollback 选择一个已知 release/digest，作为新的前向 Installation/Deployment revision 执行完整 verify、compatibility、activate 和 Ready 检查，不倒退 revision counter，也不以目录/Git 指针切换冒充成功。旧 artifact 或旧 state 不兼容、partial activation 或失联必须保留可查询 Receipt 并报告 Failed/Uncertain；只有新 revision Ready 才报告 RolledBack。

### M3 — Inspection

生产者是现有真实 owner 的 observed facts 与 Inspection projection，消费者是 CLI/TUI。snapshot/watch 只读，必须携带 source、revision/epoch、observed time、freshness 和 stale/unknown；网络或 producer 失效不能合成健康，也不能成为 desired-state 写者。

### M4 — Evidence 与日志

生产者是领域 owner 的 Receipt/Evidence 与 bounded diagnostic adapters，消费者是 CLI/TUI/runbook。日志不替代 Receipt；probe、stdout、进程退出码、文件复制或 transport ACK 不能在证据不足时推导副作用成功。恢复路径要说明数据不可用、过期或被截断，而不是无限重试。

### M5 — 可附着 TUI

生产者是 Inspection/Evidence 客户端与现有 Textual console，消费者是本地 operator。第一版以查看和解释为主；任何写操作必须另行冻结权限与 owner API。TUI 崩溃或 detach 不得停止底层服务，也不得改变 Deployment reconciliation。

### N0 — Node enroll/transfer contract

N0 只建立 operator 部署 target 的显式身份、信任、credential reference、staging root 与 bounded transfer contract。它不等于 Remote Agent enrollment，不创建 AgentSession，不复用 Agent transport，也不授权任意 shell。目标、host key/trust pin、artifact identity 与 transfer identity 必须显式；凭证值不进入配置、命令行、日志或 Receipt。

### N1 — push-only

`push` 的产品语义固定为 transfer-and-stage only：它可以验证 source digest、传输、原子发布 target staging artifact 并返回 TransferReceipt；绝不安装、切 active pointer、提交 DeploymentRevision、启动/停止/restart 任何 owner。远端已经存在同 digest 时可幂等 `changed = false`；断连、timeout 或 checksum 不完整不能报告成功。

### N2 — remote deploy

remote deploy 只消费 N1 已验证 staging artifact，并复用 D0/D1 的 owner、状态和 Receipt 语义。传输成功与部署成功是两次独立 operation；SSH 可作为明确受限的 bootstrap/rescue 研究对象，不能成为 hidden production fallback 或任意 executor。控制端断连不证明远端未发生副作用，必须进入 Uncertain 并向真实 owner query/reconcile。

### O0 — OpsService 最后准入

O0 只有在 N2 后、ADR-0003 被 Accepted 或由后继决策替代、且至少两个独立真实客户端/operation consumer 证明 durable ControlRequest、幂等、watch/cancel 与 Uncertain reconcile 的共同需求时，才能实现最小 OpsService。它只拥有 operation record/journal，真实副作用仍由 typed owner 执行。准入条件不足时，O0 的正确结果是保持 deferred，并继续使用窄 owner CLI；不得创建占位 OpsService、第二写者或通用 workflow engine 来“完成计划”。

## Graph 决定

本 Program 直接执行 Accepted ADR-0005，不再把“是否复制 EAGOS Graph Engine”列为开放问题：

1. DeckTopology/DataLink、ServiceDependency 与 activation constraint 保持不可互换的 typed graph，各自由领域 owner 定义 validator、错误、生命周期与证据。
2. 只有两个独立真实生产消费者已经证明相同 pure algorithm 需求后，才允许按实际交集抽取 internal Graph Foundation；它不得包含 I/O、持久状态、async scheduler、retry、approval、Receipt、compensation 或 rollback。
3. D0–D1 与 N1–N2 使用 owner-specific、Receipt-backed 状态机。实现可以使用局部拓扑排序或确定性步骤表，但不能把部署正确性、恢复或权限委托给通用 graph executor。
4. O0 也不得隐藏 workflow/saga engine。未来若至少两个独立、持久、跨 owner workflow 证明共享语义，必须以新的证据和后继 ADR 重新准入；“步骤很多”或“UI 想画图”都不构成反例。

## Golden path 验收场景

### G0 本地闭环

1. `paraegox init --directory <absolute-directory> --json` 生成 private `paraegox.toml`，但不创建 `state`；再次运行 byte-identical 请求返回 `changed = false`，冲突不覆盖。
2. 对生成配置执行 M1 `config check` 与 `doctor --offline`，全程无 Secret、网络和 state mutation。
3. 构建一个 immutable ParaEGOX artifact，并用只读 inspect 获得 version、digest、manifest 与 target compatibility。
4. local deploy 明确完成 verify、install、activate、Ready 与 owner Receipt；任何缺失证据只能是 Failed/Uncertain。
5. 读取 M2a lifecycle status，再由 M3 Inspection snapshot/watch 解释 freshness、stale、unknown、partial 或 reconcile-required，不能把两者混成一份“健康”状态。
6. 查询 bounded、无 Secret 的日志/Evidence，并 attach TUI 查看 owner、状态、原因与 EvidenceRef；detach 后服务继续运行。
7. 完成 same-state restart 与 new-artifact replace，证明 joined stop、generation fencing 与 no-double-active。
8. 指定已知旧 release/digest rollback 为新 revision，并重新验证 Ready。

### 远端闭环

1. 显式 enroll 一个部署 target，固定信任与 staging boundary，不创建 Remote Agent session。
2. push exact artifact，只获得 TransferReceipt；服务状态、active release 与 DeploymentRevision 均不改变。
3. remote deploy 消费 staging artifact并获得独立 Deployment/Installation/Runtime Receipts；控制端失联时报告 Uncertain，再 query/reconcile。
4. 在真实两主机 exact-ref 场景执行远端 status、Evidence 与 rollback；不能用单机 mock、`--no-run`、一次 marker 或截图代替。

## 决策与变更规则

- 本 Program 可重排 Draft `kernel-foundation.md` 中的候选顺序，但不能覆盖 Accepted ADR。
- I0 不触发 A0；任何 local/remote deploy、stable Installation identity、active pointer、升级、卸载或 installation-owned state 都必须先通过 A0。
- 如 M3–M5 或 N0–N2 需要完整 OpsService 语义，不能把 O0 前移；必须继续使用窄 typed owner seam，或先单独评审并接受 ADR-0003 后再修改本 Program。
- 新增公共命令、JSON 字段、persistent format、state mutation、Secret access 或网络行为必须先更新 `governance.toml` 与本 Program 的对应 Step。
- milestone 只有在列出的 evidence 可由评审者对 immutable exact ref 复现后才能标 Completed；代码存在、工作树存在、文档存在或某个下游模块引用它都不够。
- Mac 是唯一 writable source authority；本机不得运行 Cargo/rustc/rustfmt。Rust、完整治理与真实 system evidence 必须在 admitted Ubuntu/CI host 对同一 exact ref 执行，且不能把 Mac focused evidence升级为平台验收。
- Program 结束后，把稳定命令写入 guides/reference，把故障恢复写入 runbooks，把 claim-to-evidence 写入 testing；本文件保留交付历史、O0 准入结论与 Remote Agent 冻结/解冻结论。
