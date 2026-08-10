# Local Operator CLI/Ops Program

> 状态：Active
> Program ID：`local-operator-cli-ops`
> 授权日期：2026-08-10
> 最近重排：2026-08-11；在保留 M4a/M4b 拆分、ADR-0003 Proposed 与 OpsService 最后准入的前提下，根据已 Accepted 的 ADR-0011 同步 Artifact F0：A1→D0b 只作为同一 admission candidate 的内部实现顺序，二者必须在同一 exact ref 由真实 Controller/Runtime/TUI consumer 共同过门，A1 不独立登记、合并或发布
> 授权来源：当前工作区用户明确要求优先完成 CLI、部署查看、Inspection/Ops 路线与可验证 TUI，并冻结新的 Remote Agent 扩张
> 当前 committed anchor：`main` 仍为 `4334a59af1656429f0401c0b780134c8871148e9`；当前 D0a exact-ref 验证锚点为 `build/mac-source-snapshot-20260810-r363-d0a-compiled-local-deploy`（`20ef3f281501e3399d83c7e42e0150f208f4e8cd`）。它在 r356 M0/M1/M2a/I0 基线上递进包含 D0a 合同修正、compiled-in local deploy 实现、focused/system evidence、治理登记与 CI 接线，不包含 external Artifact、replace/restart、rollback 或 Remote Agent 能力扩张
> 当前 D0a 验证锚点证据：r363 已在固定 host-key 的 Ubuntu exact-ref worktree 通过 `cargo fmt --all --check`、locked metadata、workspace all-targets check、Clippy `-D warnings`、workspace all-targets `test --no-run`、完整 governance 与 workspace doctest；`paraegox-local` 在 non-root、默认线程栈下原样短 `TMPDIR` 重跑 182/182 通过，同一 exact-ref 真实 D0a binary 的 `tests/system/test_d0a_compiled_local_deploy_cli.py` 3/3 函数通过。首次 182 项运行使用过长 `TMPDIR`，其中 3 项只因 Unix-domain socket `sun_path` 超长失败；改用短 `TMPDIR` 后对原样代码和测试全量重跑即 182/182，因此这 3 项是验证环境路径限制，不是产品失败。GitHub macOS run `31382789910` 也已在同一 commit 成功完成原生 CLI 编译、public CLI/light init/deploy smoke、relocated bundle、Textual child + Rust Agent IPC smoke、checksum/archive/upload；commit-addressed artifact `9060699090` 于 `2026-08-17T11:23:46Z` 过期。该 Mac run 不替代 Ubuntu D0a ActiveReady system evidence，`init` 的 non-root/passwordless-sudo ownership matrix仍未闭合
> 当前 M3 动作：M3a snapshot 的 public grammar 与 JSON v1 合同已获授权，候选实现与 evidence 正在中央 immutable CI 收口；在该 gate 完成前仍不标 `Validated` 或 `Completed`。连续 watch 保持 M3b 独立后续，仍未登记 public grammar
> 当前 M5 动作：M5 已拆为 M5a attach TUI 与 M5b logs integration；M5a 的 exact grammar、只附着已 Running generation、双 locator、ADR-0009 Python direct typed-client边界、实现、真实 consumer、治理登记与 focused/Linux/macOS evidence 已形成 `Implemented candidate` 并接入 exact CI，但同一 immutable exact ref 的完整门禁与下列未覆盖矩阵尚未收口，因此不是 `Validated` 或 `Completed`。M5b 继续等待 M5a + M4b
> 当前 M4 动作：原 M4 已拆为 M4a/M4b。M4a current-Running verified PXMT Receipt one-shot snapshot 已在 immutable exact ref `474cf2272f45938f12531d23ac559b93884b5ba9` 通过 Ubuntu完整Rust/governance/pytest及4/4真实exact-binary M4a场景，状态升级为 `Validated`，但不是 `Completed`。M4b 仍为 Planned/Blocked，只有它才拥有durable PXEV、失败后可查与bounded structured logs的原M4完成条件；M4a不完成M4，不解锁M5b
> 当前 D0 动作：D0a 的 exact CLI/JSON、compiled-in deterministic deployment 与真实 binary system evidence 已在 r363 达到 exact-ref `Validated`；它不依赖 external Artifact，不触发 ADR-0004 A0。ADR-0011 及其 authorization receipt 已 Accepted，本 Program 现冻结 A1/D0b Artifact F0 合同；二者仍未实现、未登记或发布，且A1没有独立public/admission状态，R0/D1继续后置到joint D0b gate之后
> 当前 Artifact F0 动作：首个且唯一的 profile 是 `developer-local-echo-prefix-v1`；候选内部先建立 bounded non-executable model-data 的 reproducible build、zero-mutation inspect 和 immutable materialization，再由 D0b 在独立 fresh lifecycle/deployment state 真实消费。本次只是六条 intended-public grammar 与 owner contract 的冻结，不是代码、`governance.toml` 预登记、独立 A1 admission 或 capability 完成声明

## 一句话结果

ParaEGOX 当前先交付一条普通开发者能直接验证的本地路线：初始化私有工作区、检查配置、以 compiled-in `deterministic-echo-v1` 经真实 DeploymentController/Runtime 部署到 `ActiveReady`，读取 Inspection，并用独立 TUI 附着该已运行 generation。M4a 先只读取该 generation 的 verified PXMT Receipt；只有 M4b 完成 durable PXEV、失败后可查与 bounded structured logs 后，M5b 才把这些诊断数据汇入同一 TUI。ADR-0011 已 Accepted，因此下一条路线是在同一 Artifact F0 candidate 内先实现 `developer-local-echo-prefix-v1` 的 build/inspect/materialize 机制证据，再让 D0b 在 fresh workspace 经 DeploymentController/Runtime/TUI 真实消费 external bytes；只有整条链在同一 exact ref 通过 admission/governance/merge gate 后才成为用户可用 surface。R0 replace/restart 与 D1 rollback 仍等待该 D0b gate。之后再进入 Node enrollment、只传输不激活的 `push` 与远端部署，最后评审 OpsService。Remote Agent 新能力在整个 Program 中继续冻结。

## 为什么重排

现有代码已经积累 Kernel、Runtime、Deployment、Node、Fabric、Model、Agent、Inspection 和本地组合机制，但交付顺序长期由底层 tranche 推动。用户最先需要的是“拿到东西后能初始化、能部署、能看见、出错能解释、失败能回退”，而不是继续扩张 remote contract、session、connector、proxy 或通用编排抽象。

现有 TUI 入口只在 `chat` 启动链中做一次严格的 Inspection `Latest` 读取，再显示启动状态；Textual child 的退出也会让该 `chat` composition 进入 joined shutdown。这个 one-shot 切片证明了 owner 边界、IPC、失败关闭和 UI 启动顺序，但不能冒充“附着一个已经由 `up`/`deploy` 持有的实例”。当前路线已用不需要 Artifact/Installation 新 owner 的 D0a 建立可重复的本地部署可见基线，因此先用 M5a 把 TUI 变成真正的非 owning attach client；M4a 只增加当前 Running generation 的 Receipt 可见性，它不代替 M4b 的 Evidence/日志生产者与失败后查询。只有 M4b 完成后 M5b 才汇入这些诊断数据。external Artifact 的 owner 决策现已生效，但 ArtifactStore materialization、Deployment desired selection、Runtime live generation与TUI/Agent external-prefix observation必须在一个 admission candidate 内按 A1-internal→D0b 顺序共同证明；不得独立合并 A1，也不得用文档或 compiled-in D0a 代替真实 consumer。

## 权威与所有权边界

- 本 Program 是当前交付优先级的权威，但只在 Accepted ADR 与 [`governance.toml`](../../governance.toml) 已登记边界内生效。
- [ADR-0005](../adr/ADR-0005-typed-domain-graphs-and-runtime-assembly-boundary.md) 已 **Accepted**：保留 Deck、ServiceDependency 与 activation constraint 各自的 typed graph 及必要纯算法，不建设通用 Graph Engine、Graph Store、持久 Graph Schema 或中央 workflow runtime。本 Program 不能重新打开该决定。
- [ADR-0009](../adr/ADR-0009-agent-conversation-and-client-boundary.md) 已 **Accepted**：本地 Python Textual 直接消费版本化 `AgentConversationClient` 与独立 Inspection typed client，typed client 自己负责 authenticated/no-retry owner IPC；TUI 不持 raw Zenoh Session，Rust local parent也不得演变成 replacement `ConsoleBridge` 或 domain-traffic proxy。本 Program 只能为 attach 增加 generation-bound bootstrap pin/handoff，不能静默 supersede 这条依赖方向。
- [ADR-0003](../adr/ADR-0003-ops-service-operation-boundary.md) 继续保持 **Proposed**。本 Program 不接受它，也不授权提前实现完整 OpsService、federated Inspection、ConsoleGateway 或 Web Console；OpsService 只能位于本路线最后的 O0 准入门。
- [ADR-0004](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md) 的 A0 是条件式 gate，只在出现以下真实 fixture 时触发：多个独立 DeckLock 需要统一 release/update/uninstall；installation-owned mutable state 需要跨 run/升级/重部署存续；或同一 release 需要多次隔离安装/多 Artifact 需要共同稳定安装 owner。在触发前不预建 Application、Installation、active pointer、uninstall 或 GC；触发后也不能把它们塞进 DeploymentController、Deck、CLI 或 `paraegox-local` 私有目录。
- [ADR-0011](../adr/ADR-0011-local-immutable-artifact-materialization-and-deployment-selection.md) 已 **Accepted**，唯一生效证据是 [`local-artifact-baseline-v1.authorization-receipt`](local-artifact-baseline-v1.authorization-receipt)。本 Program 冻结的决策三元组为 ADR SHA-256 `fc4309f87f1770409a941d4d4ecd0694c302820ebfeeca599b9326c7bca27779`、ADR index SHA-256 `93ebf6b87a1f369916538b4bc56bf343a8e22fc3b20bcbe7a59e91fae5c0fc0d` 与 receipt SHA-256 `e491156e32cbd6d61ca84a3aa1d328950f29248d348d66eccd10729c4ccbaa0e`。它授权本 Program 准入单个 bounded non-executable model-data Artifact 的 A1/D0b 合同，但 ADR/receipt 本身不是实现、治理登记或 capability 完成证据。
- 当前本地公共入口仍由 `DeveloperLocal composition root` 拥有。M1 离线 CLI 与 I0 本地初始化不创建新 owner，不取得 Secret、网络、服务生命周期或 domain durable-state mutation 权限；M2a 只在同一 composition root 内增加窄 lifecycle seam。
- M3a 仍位于同一 composition root：Inspection owner 继续独占 projection，lifecycle owner 只提供 config-commitment-bound 的当前 Running generation rendezvous，CLI 只做一次只读 PXIB/PXIQ/PXIP v2 `Latest`；三者都不取得彼此的 state 或 action authority。
- M4a 不创建 Evidence 或日志 owner。RuntimeHost 继续是 PXMT 事实、签名与 terminal outcome 的唯一 owner；DeveloperLocal composition 内的 owner-private Receipt adapter 只在现有真实激活输出上独立验证并一次性供应同一 canonical PXMT bytes，不签名、改写、持久化或升级 outcome。lifecycle 只定位 exact config/current Running generation 的 pinned adapter bootstrap，不返回 Receipt 内容、不代理 typed query；CLI typed client 只做一次读取并重新验证 canonical bytes、digest、request/target/store/key correlation 与 Runtime Ed25519 签名。
- M5a 仍不创建新的 lifecycle、Session、Inspection 或 durable TUI state owner。lifecycle supervisor 只原子返回同一 config/current Running generation 的 conversation PXAB 与 Inspection PXIB verified pins；Rust local parent 只做 lifecycle query、token-free child handoff 与 child supervision，不读取 bootstrap/token，也不代理 domain traffic。按 Accepted [ADR-0009](../adr/ADR-0009-agent-conversation-and-client-boundary.md)，Python Textual child 继续直接消费版本化 `AgentConversationClient` 与独立 `DeveloperLocalInspectionClientV2`；raw token 不进入 Textual App/widget字段、child argv/env、日志或持久文件，Python 也不创建或持有 raw Zenoh Session。typed client对自己拥有的 mutable token buffers在错误与 close路径 best-effort 清零，但合同不虚构 CPython 对运算中 immutable `bytes` 临时副本的强制内存擦除。AgentService 继续独占 Session/Turn/cancel mutation，Inspection 继续独占 snapshot/freshness，TUI detach 不触发 `down` 或任何 owner shutdown。
- `init` 只生成开发者本地配置工作区；D0a 只确保一个 compiled-in、无 external bytes、无 installation-owned state 的 deterministic fixture 经现有 Controller/Runtime 到达 `ActiveReady`。两者都不是安装器、Installation owner 或 active-pointer owner，也都不触发 A0。
- Deployment desired state、Runtime apply、Node facts、Inspection projection、Evidence 和领域副作用继续由各自真实 owner 持有。Artifact F0 中，build/inspect contract owner 独占 canonical manifest/payload strict decode 与可重现 object ref，`inspect` 零持久变化；ArtifactStore 是 exact pair以及唯一snapshot内materialization records/owner Receipt的唯一写者；DeploymentController 独占 desired object ref、DeploymentRevision、deployment operation 与 Deployment Receipt；RuntimeHost 只经 read-only access port 重开并复验 Slice 绑定的 pair，仍是 live generation/PXMT 唯一 owner。lifecycle 只为 fresh D0b 启动唯一 supervisor，不得用 D0a `run_up` 或旧 desired state 冒充 external deployment。CLI/TUI 只能调用 bounded seam，不能成为第二写者或总 Receipt 签发者。若真实 fixture 触发 A0，相关 mutation 在最小后继 ADR Accepted 前必须以 `A0_APPLICATION_ADMISSION_REQUIRED` 失败关闭。
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
- I0 不是 Installation owner，也不产生 Release、Installation、Deployment 或 Ops Receipt；因此它不触发 A0。D0a 同样不产生稳定安装身份或 active pointer；只有出现 ADR-0004 的真实触发 fixture 时才进入 A0。

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

### D0a — compiled-in local deploy

首个且仅有的 D0a exact public grammar 冻结为：

```text
paraegox deploy --local --config <absolute-paraegox.toml> --json
```

参数顺序固定，不接受默认/相对配置路径、artifact/release/installation/path/digest、target、provider/model/Secret、replace/restart/rollback、retry 或额外参数。它仅复用现有严格 DeveloperLocal chat schema v1，且只接受配置已选择的 `deterministic-echo-v1`；provisioned 或任何其他 profile 在 deployment/lifecycle/domain mutation 前以 exit 2 失败关闭。

D0a 是“确保 compiled-in deterministic profile 已经运行并返回一次可验证的 deployment 结果”，不是 Artifact 安装。实现内部必须复用唯一 M2a `run_up()` 与同一 hidden supervisor；首次起动继续由现有 composition 执行真实 Authority/Runtime/Node owner 链、`RunningStack::activate()`、真实 DeploymentController commit/apply 与 Runtime authenticated apply，并且只有 Fabric/Model/Agent Runtime terminal 均为 `ActiveReady` 才可形成成功结果。不得启动第二个 Controller、重新 apply 已终结的同一请求，或让并发 follower 创建新 revision/apply。

supervisor 只能在上述真实 owner 链完成且对 receipt/revision/digest 做严格关联后，缓存一份窄的 verified deployment projection，并经现有 lifecycle UDS 提供 owner-private、只读、config/generation-bound `DeployQuery`。每个 query 必须携带并严格匹配该请求从 `run_up()` 得到的 expected generation；mismatch、非当前 Running generation 或 down race 都以 exit 1 失败且不 retry，不能把旧请求的 mutation 归给新 generation。该 action 不是公共 Ops API，lifecycle 不成为 DeploymentController proxy 或第二份 desired-state 权威；CLI 只读取这份投影。D0a 不创建 external Artifact bytes/store/manifest、ReleaseId、产品/Application Installation identity、record、owner、active/current pointer、uninstall/GC，也不授权 replace、restart、rollback、持续 reconcile 或当前健康检查。现有 RuntimeHost/DeveloperFixture-local legacy `installation_id` 保持原有内部 build/store identity 语义，不得公开、删除或重解释成产品安装身份。

D0a JSON v1 的 top-level 字段严格且仅有：

```text
schema_version
command
mode
ok
profile
changed
generation
deployment_revision
controller_snapshot_sequence
runtime_apply_request_digest
runtime_terminal_receipt_digest
terminal_outcome
current_health_checked
diagnostics
```

- `schema_version` 固定为 JSON number `1`；`command = "deploy"`；`mode = "local"`；`ok` 与 `current_health_checked` 是 JSON boolean，`changed` 是 JSON boolean 或 null。`current_health_checked` 始终为 `false`：成功只是 owner Receipt 关联的 point-in-time terminal outcome，不是当前 Inspection health。
- 成功时 `ok = true`、`profile = "deterministic-echo-v1"`、`terminal_outcome = "active_ready"`、`diagnostics = []`，其他结果字段均为非 null；`generation` 是 16-byte lifecycle identity 的精确 32 字符 lower-case hex JSON string且无 `0x` 前缀，`deployment_revision` 与 `controller_snapshot_sequence` 是无前导零的 canonical 十进制 JSON string，绝不编码为 JSON number。两个 digest 必须是精确 64 字符 lower-case hex JSON string。`runtime_apply_request_digest` 精确投影现有 PXMT `model_agent_request_digest`，不能命名或解释为另一个 DeploymentController operation/commit Receipt。
- `changed` 的唯一公式是 `run_up.changed && !model_agent_replayed`，并且 DeployQuery 已匹配同一个 expected generation、projection 为完整可验证的 `ActiveReady`。`fabric_replayed` 仍可作为内部关联校验但不改变此 public boolean。已运行实例、并发 follower、相同请求或 model/agent durable replay 均为 `changed = false`，不得借共享 generation、旧 query 或历史 Receipt 声称是本请求的新变化。
- 错误时 `ok = false`，`profile`、`generation`、`deployment_revision`、`controller_snapshot_sequence`、`runtime_apply_request_digest`、`runtime_terminal_receipt_digest` 与 `terminal_outcome` 均为 JSON null，且 `diagnostics` 恰有一项；该项仅含稳定 `code` 与 public-safe `message`。grammar/config/unsupported-profile 等在 `run_up` 前失败、或已有 Running generation 且本请求可证明没有接受 mutation 时，`changed = false`；一旦本请求已接受新的 lifecycle generation，或 owner/query/I/O failure 使零变化无法被证明，`changed = null`，不得用 `false` 掩盖可能已经运行的实例，也不得用 `true` 猜测尚未取回的 deployment projection。不得输出 config/state/bootstrap/socket 路径、uid/gid、PID/PGID、capability/token、Secret/SecretRef、credential/seed/key、endpoint/route、raw receipt 或未审计底层错误。
- 对该 `--json` grammar，stdout 在可写时严格为一个 compact JSON object 加一个 LF，stderr 为空。成功为 exit 0；已识别 `deploy` 的 grammar、absolute/config safety/config-authority 或 unsupported profile 错误为 exit 2；lifecycle/owner/activation/evidence/query/I/O 与输出失败为 exit 1。stdout failure 仍失败关闭，不得从进程、文件、transport ACK 或日志合成成功。

### Artifact F0 — A1 build/inspect/materialize 与 D0b fresh deploy

Artifact F0 首批且仅有以下六条 intended-public exact grammar；命令、选项与参数顺序逐 token 固定。它们当前只冻结 compatibility candidate，并未登记、合并或发布；前四条即使在候选内部可执行，也必须保持 implementation-internal，直到后两条的真实 Controller/Runtime/TUI consumer 在同一 exact ref 完成 admission gate：

```text
paraegox artifact build --profile developer-local-echo-prefix-v1 --source <ABS> --output <ABS> --json
paraegox artifact inspect --manifest <ABS> --payload <ABS> --json
paraegox artifact materialize --config <ABS> --manifest <ABS> --payload <ABS> --operation-id <32hex> --json
paraegox artifact materialization query --config <ABS> --operation-id <32hex> --json
paraegox deploy --local --config <ABS> --artifact-object-ref <REF> --materialization-receipt-ref <REF> --operation-id <32hex> --json
paraegox deployment operation query --config <ABS> --operation-id <32hex> --json
```

- `<ABS>` 必须是 lexical-canonical absolute path；六条命令均不接受默认路径、相对路径、选项换序、重复选项、额外参数、隐藏 fallback 或透明 retry。每个 `<32hex>` 是非零 16-byte operation identity 的精确 32 字符 lower-case hex，无 `0x` 前缀。
- D0a 的 `deploy --local --config <ABS> --json` 仍是原样五 token grammar。dispatcher 先匹配 exact D0a 五 token，再匹配 exact D0b 十一 token；不得用可选 artifact flags 把两条合同合并，也不得改变 D0a JSON/行为。若一个 malformed `deploy` argv 含有 exact ASCII option token `--artifact-object-ref`、`--materialization-receipt-ref` 或 `--operation-id` 中任意一个，它只归入 D0b `PXLC-DEPLOY-EXTERNAL-GRAMMAR`；其余 malformed `deploy` 仍归入原 D0a `PXLC-DEPLOY-GRAMMAR`。因此既有 D0a 缺项、换序、重复、`--retry` 或其他额外 option 的诊断与 bytes 不变，只有此前不存在且本来就非法的三个 reserved option 被划入 D0b。`artifact {build|inspect|materialize|materialization query}` namespace 必须在既有 offline/lifecycle dispatch 前识别；exact ASCII `deployment operation` 前缀必须在既有 `deployment --config` process grammar 前识别，其他 `deployment` argv 不得被新 query parser 吞并。
- `artifact build` 与 `artifact inspect` 完全离线且零 domain mutation；`inspect` 还必须零 filesystem mutation。`artifact materialize`/`artifact materialization query` 只进入 ArtifactStore seam；D0b deploy/query 只进入下文 owner seam。没有一条命令创建 Installation、active/current pointer、Graph operation 或通用 Ops operation。

首个 profile 的 immutable execution contract 固定为：

- `profile = "developer-local-echo-prefix-v1"`，`runtime_kind = "managed_model_data_v1"`。payload 长度严格为 1..64 bytes，必须是 UTF-8 且每个 byte 都是 printable ASCII `0x20..0x7e`；因此 CR、LF、NUL 与其他 control 全部非法，最后一个 byte 还必须精确为 ASCII space `0x20`。
- canonical manifest 绑定 payload length/digest、上述 profile/runtime kind、`adapter_abi = "bounded-text-model-data-v1"`、`target_profile = "developer-local-managed-model-v1"` 与 `entrypoint = "literal-prefix-v1"`。四个 execution 值都由 profile 固定，不是 CLI/config/manifest 可自由 override 的输入或可选 default；build 必须逐字写入 canonical manifest，inspect/ArtifactStore/Controller/Runtime 必须逐字匹配，且实现批的跨语言 golden 必须冻结其 exact bytes。`entrypoint` 不新增 public JSON top-level 字段。不兼容变化只能创建显式 successor profile/contract，不能扩宽 v1 decoder。
- RuntimeHost 必须从 Slice 绑定的 terminal ArtifactStore object 通过 read-only access port exact reopen canonical manifest 与 payload，再重算并复验 pair、profile/runtime kind、length、ABI、target 与 entrypoint；只有全部通过后，仓库内编译 adapter 才可接受 0..16384-byte 的既有 prompt并产生精确 `payload || prompt` bytes。这份 external payload 必须真实改变 Agent/TUI 可见结果，不能回退到 D0a compiled-in fixture。
- payload 不是 executable、dynamic library、ProcessDomain program、script 或 eval 输入，不能创建 subprocess、读取路径/环境、访问网络或取得其他 ambient OS authority；本合同不作 sandbox/containment 声明，也不构成 Installation/A0。出现 ADR-0004 的真实触发 fixture时才在任何 Artifact/deployment/runtime mutation 前返回 `A0_APPLICATION_ADMISSION_REQUIRED`。

canonical manifest 是 big-endian、fixed-size、精确 206-byte 的 `PXAM` v1；header 精确 80 bytes，布局严格为：

```text
0..4      magic = PXAM
4..6      version = u16_be(1)
6..8      header_len = u16_be(80)
8..12     frame_len = u32_be(206)
12..14    profile_len = u16_be(30)
14..16    runtime_kind_len = u16_be(21)
16..18    adapter_abi_len = u16_be(26)
18..20    target_profile_len = u16_be(32)
20..22    entrypoint_len = u16_be(17)
22..24    reserved = u16_be(0)
24..32    payload_len = u64_be(1..64)
32..64    payload_digest[32]
64..68    max_prompt_bytes = u32_be(16384)
68..72    max_output_bytes = u32_be(32768)
72..76    effect_flags = u32_be(0)
76..80    reserved = u32_be(0)
80..110   ASCII "developer-local-echo-prefix-v1"
110..131  ASCII "managed_model_data_v1"
131..157  ASCII "bounded-text-model-data-v1"
157..189  ASCII "developer-local-managed-model-v1"
189..206  ASCII "literal-prefix-v1"
```

payload 与 manifest digest domain 固定为：

```text
payload_digest  = SHA-256("paraegox.artifact.payload.sha256.v1" || exact_payload_bytes)
manifest_digest = SHA-256("paraegox.artifact.manifest.sha256.v1" || exact_206_byte_PXAM)
```

binary `ArtifactObjectRefV1` 是 big-endian、精确 72-byte 的 `PXAK` v1：

```text
0..4    magic = PXAK
4..6    version = u16_be(1)
6..8    frame_len = u16_be(72)
8..40   payload_digest[32]
40..72  manifest_digest[32]
```

所有 decoder 必须拒绝错误 magic/version/header/frame/string length、reserved、literal、limit、effect flags、digest、trailing 或 oversize，strict decode 后 canonical re-encode 必须逐 byte 等于输入。`max_prompt_bytes` 与 `max_output_bytes` 是固定 contract limits而非自由 metadata；本 profile 的最大输出实际为 `64 + 16384 = 16448` bytes，不得截断、补零或利用 32768 上限扩宽 payload/prompt。

`artifact build --output <ABS>` 的 `<ABS>` 是 non-root、lexical-canonical absolute artifact directory；整条既有父链必须无 symlink并由当前 uid/gid安全控制。首次 build 原子创建 mode-0700目录，成功后严格只有 `manifest.pxam`（精确206 bytes、mode0600、regular、single-link）与逐byte等于accepted source的 `payload.bin`（1..64 bytes、mode0600、regular、single-link），不得设置executable bit、跟随/发布symlink/hardlink或覆盖既有对象。新pair必须经同父目录temp、fsync、identity recheck与原子no-overwrite发布后才返回 `changed = true`；同source、同exact bytes重跑必须返回 `changed = false` 且两个既有文件的inode/bytes不变。目录/文件/额外entry、owner/mode/link/content任一冲突都fail-closed且不覆盖；错误退出时，只有安全清理并证明零最终mutation才可 `changed = false`，留下任何可观察对象或durability不确定都必须为null。build不读写state root、ArtifactStore、Controller或Runtime。`artifact inspect` 只接受显式manifest/payload路径；无论成功或失败都不得创建、写入、rename、删除、chmod/chown任何对象，也不创建domain state。

两类 public reference 的 canonical text wire 固定为：

```text
sha256:<64-lowerhex-payload-digest>:<64-lowerhex-manifest-digest>
pxamr1:<64-lowerhex-store-instance>:<canonical-decimal-sequence>:<32-lowerhex-operation-id>:<64-lowerhex-receipt-digest>
```

- `ArtifactObjectRefV1` 的两个 digest 不可拆分、换序、截断或降格为 payload-only identity；相同 payload 加不同 manifest 是不同 object。`<canonical-decimal-sequence>` 是大于零、无前导零且不编码成 JSON number 的 owner sequence。
- materialization Receipt ref 的 store instance、operation id、sequence 与 receipt digest 必须逐项关联 ArtifactStore terminal record；CLI、Controller、Runtime 或文件存在都不能另签、改写或从 ref 文本推导 terminal outcome。

ArtifactStore v1 的 root、初始化与 capacity profile 同样固定：

- 唯一稳定 layout 是 `<state_root>/artifact-store-v1/{artifact.lock,artifact.snapshot,objects/}`；object directory 精确为 `objects/o-<64-lowerhex-payload-digest>-<64-lowerhex-manifest-digest>/`，其中稳定对象只有 `manifest.pxam` 与 `payload.bin`。初始化只使用 fixed sibling `<state_root>/.artifact-store-v1.initializing/`，snapshot transaction 只使用 owner root 内的 `.artifact.snapshot.next`，pair transaction 只使用目标 object directory 内的 `.manifest.pxam.next` 与 `.payload.bin.next`；不得生成随机、递增、按 operation 命名或可由扫描发现的替代路径。
- local layout 只有在本 invocation 的完整 config、输入 pair、A0、compatibility 与所有 path preflight 已通过后，才可创建或验证 `<state_root>`；它必须是当前 uid/gid 拥有、mode 0700、全链 no-symlink 的 real directory。这个动作只是 non-authoritative scaffolding，不生成 ArtifactStore identity、counter、operation 或 Receipt。materialize 的最长 derived suffix 固定为 178 bytes；Linux `PATH_MAX = 4096` 下 `<state_root>` 的 UTF-8 byte length 必须 `<= 3917`，否则在任何 filesystem effect 前返回既有 `PXLC-STATE-ROOT-TOO-LONG`。query 无论成功、NotFound或失败都不得创建 state root、final/staging store root、lock、snapshot next、object directory或pair temp。
- Accepted 初始化不创建 empty snapshot。首个 authoritative `artifact.snapshot` 必须已经包含首个 exact PXAQ/PXAA、一次生成并永久保留的random nonzero `artifact_store_instance`、nonzero immutable config commitment、`snapshot_sequence = 1`、operation high-water/count均为1、object high-water/count均为0。staging 内的0600 lock、0700 `objects/`、snapshot与各层directory必须逐层 fsync、no-follow exact reopen并复核 identity；随后只以 `RENAME_NOREPLACE` 将整个 `.artifact-store-v1.initializing/` 原子发布成 `artifact-store-v1/`，fsync `<state_root>`，再 exact reopen final root/snapshot。这一次 directory publication 才是首个 ArtifactStore authoritative/operation mutation。
- final 不存在时，只有 identity、layout、snapshot checksum/bytes 与 exact PXAQ 全部可证明的 staging，才可由同一 exact materialize request 在 staging exclusive lock 下完成上述 publication；read-only query遇到这种可归因于所查operation的same-request staging时返回下文精确`UNCERTAIN`行且不完成publication。different/cross-request、unknown/partial bytes、identity不明，或 final 与 staging 同时存在，全部精确分类为`OWNER`并失败关闭，不得猜 store instance、operation、counter或归属。final root 已存在但 snapshot 缺失、损坏或不可 strict reopen 同样是`OWNER`，绝不是 zero/virgin store，不能重新初始化或补 empty snapshot。
- stable final root 的 entry set 精确为 `artifact.lock`、`artifact.snapshot`、`objects`；transaction期间只可额外出现 fixed `.artifact.snapshot.next`。final/staging owner root、`objects/`与每个object directory都必须是当前uid/gid拥有、mode0700、no-symlink的real directory；lock、snapshot、next、manifest、payload与pair temp都必须是当前uid/gid拥有、mode0600、regular、single-link文件，每次访问以no-follow descriptor复核device/inode/owner/mode/nlink/length。`objects/` 只允许 snapshot 索引的最多64个canonical object directory，加上最多一个与最后一条object-publication-uncertain PXAW-U精确归因的incomplete/quarantine directory及上述fixed pair temp；该最后U可以尚无PXAX，也可以已有matching PXAX-U，后者仍保留directory/bytes/block。enumeration唯一语义是拒绝extra/unknown entry，绝不从目录反推 object table、operation、counter或recovery结论。`artifact.snapshot` 是唯一 owner record、store identity、config commitment与两个 high-water 的事实来源；不存在独立 operation、Receipt、journal或counter file。PXAQ/PXAA/PXMU/PXAV/PXAW/PXAX只是snapshot内的canonical records。
- 最多64个 terminal objects、最多1024个materialization operations；8 MiB（8388608 bytes）是checked defense ceiling而不是v1 public workload可自然耗尽的配额，精确容量公式见下文。任一count上限、budget `> 8388608` 或checked arithmetic overflow都返回稳定 pre-effect owner error，不接受新 admission、不覆盖旧 object/operation，也不把容量失败改写为 `uncertain`。v1 没有 GC、delete、retain、evict、后台清理、目录扫描恢复或online migration；已 terminal但未被Deployment引用的完整 object仍计入上限，CLI/Runtime不得为释放容量而删除或覆盖。

ArtifactStore materialization 的 canonical owner bytes 同样属于 F0 v1，不得由语言对象、JSON、文件名或 serde 默认值代替。所有整数为 big-endian，所有 frame 都必须 exact EOF、strict decode 后 canonical re-encode 逐 byte 相等；16-byte operation id、32-byte commitment/digest/store instance 均不得全零。`operation_sequence` 是 ArtifactStore-global admission high-water：首份 PXAA 精确为 1，每个新 operation 的 durable PXAA 精确加一，绝不复用、倒退、跳号或按完成顺序重排；same operation/same PXAQ replay 必须沿用原 sequence。PXAA bytes 与 operation high-water/count 位于同一 snapshot publication；首份由initial root atomic publication承载，之后由exact snapshot successor承载，不存在第二个 counter file/writer。`u64::MAX`、checked addition、1024-operation或8-MiB preflight失败必须发生在新 PXAA 前；若 publication 后无法证明 old 或 new完整 commit，只能按 exact operation/PXAQ 与下文canonical snapshot-next规则重开，能恢复 canonical PXAA才用其 sequence并终结原 operation为 U，否则 ArtifactStore owner整体失败关闭、`changed = null`，且绝不猜测/复用候选 sequence。

`PXAQ` v1 是 fixed 176-byte canonical materialization request：

```text
0..4      magic = PXAQ
4..6      version = u16_be(1)
6         action = M
7         reserved = 0
8..10     header_len = u16_be(176)
10..12    reserved = 0
12..16    frame_len = u32_be(176)
16..32    operation_id[16]
32..64    config_commitment[32]
64..136   exact PXAK v1[72]
136..140  contract_flags = u32_be(0)
140..144  reserved = 0
144..176  request_digest[32]
```

`request_digest = SHA-256("paraegox.artifact.materialization-request.sha256.v1" || frame[0..144])`。manifest/payload path 不进入 request identity；producer 必须先通过同一 fd 的 strict pair 验证，再从 exact bytes 生成 PXAK/PXAQ，因此 path replacement 不能改变已 admitted request，也不能让 path 成为 object identity。

`PXAA` v1 是 fixed 208-byte durable admission：

```text
0..4      magic = PXAA
4..6      version = u16_be(1)
6         action = M
7         state = A
8..10     header_len = u16_be(208)
10..12    reserved = 0
12..16    frame_len = u32_be(208)
16..48    artifact_store_instance[32]
48..56    operation_sequence = u64_be(nonzero)
56..72    operation_id[16]
72..104   PXAQ request_digest[32]
104..176  exact PXAK v1[72]
176..208  admission_digest[32]
```

`admission_digest = SHA-256("paraegox.artifact.materialization-admission.sha256.v1" || frame[0..176])`。capacity、A0、config、pair 与 compatibility preflight 必须先完成；包含 exact PXAQ/PXAA 与 operation high-water/count 的durable snapshot commit是首次materialization owner mutation，首个operation由上述initial root atomic publication承载，后续operation由snapshot successor承载。相同 operation/request 只能读取或推进这一 admission；不同 request/object/config commitment 必须在另一个 owner effect 前 conflict。

`PXMU` v1 是 fixed 240-byte durable materializing record；它表示 owner 已在同一 operation 的唯一 mutation lease 下进入 pair publication，不是 transport ACK 或内存 flag：

```text
0..4      magic = PXMU
4..6      version = u16_be(1)
6         action = M
7         state = P
8..10     header_len = u16_be(240)
10..12    reserved = 0
12..16    frame_len = u32_be(240)
16..48    artifact_store_instance[32]
48..56    operation_sequence = u64_be(nonzero)
56..72    operation_id[16]
72..104   PXAQ request_digest[32]
104..136  PXAA admission_digest[32]
136..208  exact PXAK v1[72]
208..240  materializing_digest[32]
```

`materializing_digest = SHA-256("paraegox.artifact.materializing.sha256.v1" || frame[0..208])`。同一 admission 最多发布一份 exact PXMU。restart 必须先按下文规则收口 snapshot-next，再以durable PXMU锁存recovery-start facts并恢复同一operation，不得先改写为U或生成新id/sequence：只从canonical PXAQ/PXAA中的exact PXAK与owner-derived fixed object location执行一次bounded no-follow reopen，不扫描目录、不接受path fallback、不重放unknown temp write、不覆盖/删除任何既有对象。若recovery-start snapshot已含exact PXAV且canonical final pair的owner/mode/nlink/identity、bytes、object high-water与directory durability全部可证明，则必须复用该PXAV并以原operation/operation sequence终结为PXAW-E与PXAX-E；若snapshot尚无PXAV，但canonical final pair可经exact reopen、file refsync与object-directory fsync重新证明，则必须在一个successor内按原object high-water规则加入exact PXAV，再终结为PXAW-M与PXAX-M。两种情况不得互换M/E，也不得根据creator operation id推断，因为PXAV不携带该字段。若只见partial pair、对象/identity不一致、durability或归属任何一项不明，只能以保留全部verified facts的PXAW-U/PXAX终结原operation、归因quarantine并阻断新object publication。recovery不能把PXAA/PXMU、文件存在或清理成功单独解释成completed object。

`PXAV` v1 是 fixed 192-byte immutable object terminal；它只证明 ArtifactStore 已对完整 pair 做 final no-overwrite publish、directory sync 与 exact reopen，不代表 Deployment selection：

```text
0..4      magic = PXAV
4..6      version = u16_be(1)
6         action = M
7         state = R
8..10     header_len = u16_be(192)
10..12    reserved = 0
12..16    frame_len = u32_be(192)
16..48    artifact_store_instance[32]
48..56    object_sequence = u64_be(nonzero)
56..128   exact PXAK v1[72]
128..136  payload_len = u64_be(1..64)
136..140  manifest_len = u32_be(206)
140..160  reserved = 0
160..192  object_terminal_digest[32]
```

`object_terminal_digest = SHA-256("paraegox.artifact.object-terminal.sha256.v1" || frame[0..160])`。`object_sequence` 是独立的 ArtifactStore-global terminal-object high-water：首个新 unique object精确为1，之后每个新 durable PXAV精确加一；相同 PXAK 只能 strict复验并复用原 PXAV/sequence，不得因新 operation或replay新造 object、跳号、复用 sequence或发布等价但 byte-different terminal。PXAV 与 object high-water successor必须在同一 durable owner commit一起fsync；64-object/8-MiB/`u64::MAX` 在 pair publication前 checked preflight。若 final pair/PXAV/high-water 的 identity或durability无法证明，原 operation只能进入PXAW-U，ArtifactStore在 exact bounded recovery完成前拒绝新object publication，绝不扫描目录、覆盖或猜测下一个sequence。

`PXAW` v1 是 fixed 304-byte materialization operation terminal，state byte 严格为 `M`（本 operation 完成新物化）、`E`（exact object 已存在且完成关联）、`F`（owner-confirmed failed）或 `U`（effect/耐久性无法证明）：

```text
0..4      magic = PXAW
4..6      version = u16_be(1)
6         action = M
7         state = M | E | F | U
8..10     header_len = u16_be(304)
10..12    reserved = 0
12..16    frame_len = u32_be(304)
16..48    artifact_store_instance[32]
48..56    operation_sequence = u64_be(nonzero)
56..72    operation_id[16]
72..104   PXAQ request_digest[32]
104..136  PXAA admission_digest[32]
136..168  PXMU materializing_digest[32] or all-zero
168..200  PXAV object_terminal_digest[32] or all-zero
200..272  exact PXAK v1[72]
272..304  operation_terminal_digest[32]
```

`operation_terminal_digest = SHA-256("paraegox.artifact.materialization-terminal.sha256.v1" || frame[0..272])`。`M|E` 要求 PXMU 与可 exact reopen 的 PXAV digest 都 nonzero；`M` 只能表示本 operation 的正常执行或 bounded recovery 从“没有PXAV”推进到新durable PXAV，`E` 只能表示该 operation 在正常执行或 bounded recovery开始时已经看到并strict复验既有exact PXAV。PXAV不携带creator operation id，M/E只由本 operation 开始或恢复开始时PXAV是否已durable存在决定，不得事后猜测。`F` 的PXAV digest必须全零，并且owner必须证明没有任何pair temp/final、incomplete directory或quarantine/remnant存在；它可以保留已durable PXMU，但此时还必须证明PXMU后发生了零pair effect。一旦partial/remnant已经存在或可能存在、identity/attribution/durability不明，只能终结`U`；`U`必须分别保留已验证PXMU/PXAV digest（若存在）或全零（若未知），不能猜测。`F|U`都不删除admission/progress或已验证object evidence；只有最后一条object-publication-uncertain U可以按snapshot规则持有quarantine，且其matching PXAX-U不清flag/bytes/block。

`PXAX` v1 是 fixed 240-byte ArtifactStore owner Receipt；只有 durable PXAW 后才能签发，state 必须逐 byte 等于 PXAW：

```text
0..4      magic = PXAX
4..6      version = u16_be(1)
6         action = M
7         state = M | E | F | U
8..10     header_len = u16_be(240)
10..12    reserved = 0
12..16    frame_len = u32_be(240)
16..48    artifact_store_instance[32]
48..56    operation_sequence = u64_be(nonzero)
56..72    operation_id[16]
72..104   PXAQ request_digest[32]
104..136  PXAW operation_terminal_digest[32]
136..208  exact PXAK v1[72]
208..240  receipt_digest[32]
```

`receipt_digest = SHA-256("paraegox.artifact.materialization-receipt.sha256.v1" || frame[0..208])`。`pxamr1` text ref 的四个变量必须分别来自该 PXAX 的 store instance、ArtifactStore-global operation sequence、operation id 与 receipt digest；消费 ref 的 typed read 对 `M|E` 必须取得并逐层复验 exact `PXAX → PXAW → PXMU → PXAV → PXAA → PXAQ → PXAM/payload`。`F|U` 始终必须取得 `PXAX → PXAW → PXAA → PXAQ`；PXMU只在PXAW的`materializing_digest` nonzero时存在且必须取得，PXAV与PXAM/payload只在PXAW的`object_terminal_digest` nonzero时存在且必须取得。对应digest为all-zero时该frame/对象必须absent，不能伪造一个“全零frame”满足链。只有完整`M|E`链可进入D0b；ref文本、文件存在或`F|U` Receipt本身都不是materialized证明。PXAX 是本地 store-identity 与 durable-owner correlation Receipt，不声明跨主机签名或 remote trust。

#### ArtifactStore v1 snapshot authority

`artifact.snapshot` 的 outer frame 固定为 canonical `PXAZ` v1，header精确192 bytes；`PXAZ`、下述`PXAY`与`PXOP` magic在本仓现有codec中未占用并由本合同保留。所有整数均big-endian：

```text
0..4      magic = PXAZ
4..6      version = u16_be(1)
6..8      header_len = u16_be(192)
8..16     frame_len = u64_be(192 + body_len)
16..18    body_version = u16_be(1)
18..20    owner_kind = u16_be(1)
20..22    checksum_alg = u16_be(1)  # SHA-256
22..24    checksum_version = u16_be(1)
24..28    state_flags = u32_be
28..32    reserved = 0
32..64    artifact_store_instance[32]
64..96    immutable config_commitment[32]
96..104   snapshot_sequence = u64_be(>= 1)
104..112  operation_high_water = u64_be
112..120  object_high_water = u64_be
120..124  operation_count = u32_be
124..128  object_count = u32_be
128..136  body_len = u64_be
136..144  accounted_rest_bytes = u64_be
144..152  quarantine_bytes = u64_be
152..160  reserved = 0
160..192  checksum[32]
```

`state_flags`只允许bit0 `OBJECT_PUBLICATION_BLOCKED`，其他bit必须为0。bit0为0严格要求`quarantine_bytes = 0`；bit0为1允许`quarantine_bytes = 0..=540`，但snapshot最后一条operation必须是可由其exact PXAQ/PXAA、optional PXMU/PXAV与canonical object directory逐项归因的object-publication-uncertain `PXAW-U`，filesystem中还必须实际存在至少一个由该U canonical归因的entry（empty child directory或zero-byte temp允许bytes为0），且即使已有exact `PXAX-U`也不得清除此flag。nonzero `quarantine_bytes`精确等于该最后U所归因、且尚未由indexed PXAV object accounting覆盖的regular temp/final manifest与payload byte length之和：unindexed partial final可以计入；已有PXAV的stable final pair已由`sum(indexed_object(...))`计入，不得再计，只计该directory中的额外temp/remnant；每个regular-file byte必须恰计一次。directory、zero-length lock、snapshot与filesystem block allocation都不计。bit0为0但bytes nonzero、bit0为1但无实际attributed entry或无上述最后U、归因到非最后operation、indexed pair double-count、漏计/重复计、`quarantine_bytes > 540`，以及unknown/无法归因/identity不明的temp或partial都拒绝整份snapshot并令owner失败关闭，不能靠设置flag吸收。checksum精确为`SHA256("paraegox.artifact.store-snapshot.sha256.v1" || u64_be(160) || header[0..160] || u64_be(body_len) || body)`；decoder先做bounded length/read与exact EOF，再按offset、reserved、flag、count、body和cross-frame规则strict decode、重算checksum，并要求canonical re-encode逐byte等于输入，不能接受short/trailing、unknown version/algorithm或checksum后字段。

PXAZ body固定为`PXAY` v1；其64-byte body header与section顺序精确为：

```text
0..4      magic = PXAY
4..6      version = u16_be(1)
6..8      header_len = u16_be(64)
8..16     body_len = u64_be(64 + object_table_len + operation_table_len)
16..20    object_count = u32_be
20..24    operation_count = u32_be
24..32    object_table_len = u64_be
32..40    operation_table_len = u64_be
40..64    reserved = 0
64..      object_count exact PXAV v1 records, then operation_count PXOP v1 entries
```

PXAY与PXAZ的body length/count必须逐项相等，`object_table_len = object_count * 192`，`operation_table_len`精确等于后续PXOP `entry_len`之和；不得交换table、插入padding/index/footer或保留第二份journal。每个operation table entry是header精确32 bytes的`PXOP` v1：

```text
0..4      magic = PXOP
4..6      version = u16_be(1)
6..8      header_len = u16_be(32)
8..12     entry_len = u32_be
12..16    presence_flags = u32_be
16..32    operation_id[16]
32..      exact PXAQ176 || exact PXAA208 || [PXMU240] || [PXAW304] || [PXAX240]
```

`presence_flags`的bit0/bit1/bit2分别且只表示PXMU/PXAW/PXAX presence；合法flag与`entry_len`组合只有`0/416`、`1/656`、`2/720`、`3/960`、`6/960`、`7/1200`。flags 4、5、unknown bit、length/presence不一致、PXAX无PXAW、嵌套frame换序或entry trailing都拒绝；header operation id必须逐byte等于PXAQ/PXAA及所有present nested frame的operation id。

PXAY的canonical state invariants全部是read与successor publication gate：PXAV按table order的`object_sequence`必须精确为`1..=object_count`，PXOP按table order的nested PXAA `operation_sequence`必须精确为`1..=operation_count`；两个count分别等于对应high-water，PXAV的PXAK ref唯一、operation id唯一。snapshot/PXAQ的config commitment全部逐byte相等；store instance、operation id/sequence、PXAK、request/admission/materializing/object-terminal/operation-terminal/receipt digest在每条可达链上strict关联。`M|E`以及带nonzero object digest的`U`必须恰好命中一个PXAV；最多一条operation没有PXAX且只能是最后一条，最多一个PXAV未被terminal PXAX引用且必须由最后一条incomplete operation的exact PXAK归属。每个durable successor的`snapshot_sequence`必须精确等于前一份加1，禁止复用、跳号、倒退或重排历史table。

首次snapshot的nonzero config commitment在所有successor中必须逐byteimmutable。每次materialize与query都先把本次strict current config commitment和snapshot比较，再允许读取/推进operation；每份PXAQ也必须等于snapshot。D0b PXDQ的config commitment还必须同时等于本次current config、snapshot以及其Receipt链引用的PXAQ。current-vs-snapshot/root mismatch固定映射`PXLC-LIFECYCLE-CONFIGURATION`；PXDQ-vs-PXAQ/Receipt chain mismatch固定映射`PXLC-DEPLOY-MATERIALIZATION-RECEIPT`，不得改写为NotFound、owner I/O或generic artifact failure。

除initial directory publication外，每次snapshot mutation都只执行fixed-next protocol：取得exclusive `artifact.lock`后strict reopen final root/snapshot；以`O_EXCL | O_NOFOLLOW | O_CLOEXEC`和mode0600创建唯一`.artifact.snapshot.next`；写完后fsync、close、no-follow exact reopen并复验全部bytes；再复核final snapshot、root与lock descriptor identity及原sequence未变；只允许一份exact `N + 1`且符合上述permitted successor的canonical next原子replace `artifact.snapshot`，fsync owner root，最后exact reopen final snapshot。old/new任何一项无法证明都不得猜测成功。有效next只能由same operation/same exact PXAQ的materialize完成rename；query绝不rename或清理，在active snapshot已经含该request的exact terminal时可只返回active，active尚无terminal时精确分类`UNCERTAIN`。partial/unknown、cross-request/cross-operation或identity不明的next一律分类`OWNER`并fail-closed，不扫描目录或猜测counter/recovery；same invocation只可清理它自己可逐byte、inode与phase证明的exact temp，restart遇到unknown temp永不删除。

publication successor只有五种：admission successor原子追加exact PXAQ/PXAA并同步增加operation count/high-water；PXMU successor把最后一条exact admitted PXOP从presence flags 0推进到1，只追加其唯一exact PXMU，snapshot sequence精确`N + 1`而operation/object count与两个high-water全部不变；object successor原子追加exact PXAV并同步增加object count/high-water；PXAW successor只把该operation推进到terminal record；PXAX successor再单独加入owner Receipt。因此672-byte admitted只能先durable发布PXMU successor成为912-byte materializing，且该commit必须发生在object-directory mkdir或任何pair effect前，不能以pair或内存flag跳过。创建新object directory前必须先strict pin final root与其`objects/` descriptor及identity；只在该pinned `objects/`下descriptor-relative、no-follow、no-overwrite mkdir canonical child，随即fsync新child与pinned parent，close/exact reopen parent并从同一reopened parent exact reopen child，逐项复核parent/child device/inode/owner/mode/nlink与entry set。已存在child也只能经这一pinned parent strict reopen，不得从absolute path或重新解析的root继续。随后在pinned child内以fixed `.manifest.pxam.next`/`.payload.bin.next`做0600 `O_EXCL | O_NOFOLLOW | O_CLOEXEC` write/fsync/close/exact reopen，再identity-checked no-overwrite发布成`manifest.pxam`/`payload.bin`；两个final均到位后必须fsync child，并经同一个仍pinned且identity未变的`objects/` parent再次reopen child与exact pair，只有该reopen全链成功才允许PXAV successor。不得由mkdir、文件存在、单文件publish或未从pinned parent重开的pair提前增加PXAV/high-water。

`artifact.lock`固定为owner root中的zero-length、mode0600、当前uid/gid、regular、single-link、no-follow `O_CLOEXEC`文件。materialize/query都必须先canonical strict-open并验证lock metadata/identity/layout，再在本invocation任何owner effect前只尝试一次锁：mutation取exclusive，query取shared，不等待、重试或另建lock；query因而只可使用初始化时已存在的lock。strict-valid owner上的`WouldBlock`/`EWOULDBLOCK`统一映射`PXLC-ARTIFACT-UNCERTAIN`；其他lock syscall failure映射`PXLC-ARTIFACT-IO`；lock metadata/identity/layout不合法映射`PXLC-ARTIFACT-OWNER`，三者不得互换。lock abstraction不暴露raw fd，所有handle/clone的`Drop`必须显式unlock而不是只依赖close。任何fork/spawn/exec、supervisor或workload创建前，parent必须显式unlock并drop全部clone；child即使意外继承，也必须在进入任何业务路径前显式unlock并close，不能把`CLOEXEC`当作fork safety证明。

restart mutation在exclusive lock内必须先收口canonical snapshot-next，再从settled snapshot的PXMU锁存recovery-start state。snapshot已有exact PXAV且pair全链可证明时终结E；snapshot无PXAV但exact final pair可经上述same-pinned-parent reopen、refsync/child-dirsync重新证明时，先在同一object successor追加PXAV/high-water再终结M；partial pair、identity/attribution/durability不明时只能U，能精确归因最后operation时按上述规则记录quarantine/block，否则整个owner strict失败。virgin（final与staging都absent）的query固定NotFound/`changed = false`，valid store中operation absent同样NotFound。valid、attributable same-request staging，或canonical same-operation `N + 1` next且active尚无terminal，精确映射`PXLC-ARTIFACT-UNCERTAIN`；partial/unknown/cross-request staging或next、final+staging、任何extra entry以及root/lock/snapshot的owner/mode/link/identity/layout/codec/checksum strict failure，精确映射`PXLC-ARTIFACT-OWNER`。所有query路径的`changed = false`；`OWNER`除已strict parse并回显的input operation id外，`state`、object ref与Receipt ref全部null。query不得补PXAX、fsync、cleanup、recovery或任何successor；PXAW已存在而PXAX缺失时按下文四行返回相应terminal且Receipt为null，只有materialize可推进PXAX。v1不做online migration、version fallback或content sniffing；未来格式只能由offline successor root迁移。

capacity accounting只接受以下checked regular-file公式。PXAY body最大值是`64 + 64 * 192 + 1024 * 1200 = 1241152` bytes，PXAZ snapshot最大值是`192 + 1241152 = 1241344` bytes。canonical snapshot frame-length vectors依次为：initial admitted `672`、materializing `912`、加入PXAV后的object-terminal `1104`、加入PXAW但尚无Receipt的materialized-terminal `1408`、加入PXAX后的materialized-receipt `1648`，以及复用同一object的第二个E operation receipt `2848` bytes；两份U/block frame仍分别是`1216`与`1456`。这些只是frame bytes，不能冒充header中的logical accounting。每次decode与successor都必须重算并要求header的`accounted_rest_bytes = current_snapshot_frame_bytes + sum(indexed_object(manifest_len 206 + payload_len 1..64)) + attributed_quarantine_regular_file_bytes`；本golden的payload精确20 bytes，因此object-terminal/materialized-terminal/materialized-receipt/second-E的`accounted_rest_bytes`分别是`1330`、`1634`、`1874`、`3074`，而admitted/materializing与zero-quarantine且尚无PXAV的两份U/block仍分别等于其`672`、`912`、`1216`、`1456` frame length。`1259164 = 1241344 + 64 * (206 + 64) + 540`只是这些逻辑regular-file components逐项取cap所得的componentwise conservative accounting upper bound，不声明该组合可由一份canonical snapshot或合法preflight history同时达到；它不包含directory、zero-length `artifact.lock`、inode metadata、filesystem block/slack或`du`物理占用。transaction calculator按`accounted_before + candidate_snapshot + new_pair_bytes_not_already_counted + known_owner_temp_bytes_not_already_counted`逐项checked；fixed pair publication使后两项的conservative combined maximum为270，因此`2500778 = 1259164 + 1241344 + 270`只是componentwise conservative transaction upper bound，同样不包含directory、zero-length lock、inode metadata或filesystem block allocation，不能声称是physical peak或constructible state。计算结果`<= 8388608`接受，`> 8388608`或任一overflow拒绝；64/1024 count caps严格支配8-MiB defense ceiling，所以public v1不能通过合法history自然耗尽它。8-MiB边界只用pure/injected checked calculator测试，禁止用padding、超额quarantine、伪造history或放宽count caps制造“合法耗尽”。

materialization hardcoded shared goldens 全部位于 `tests/fixtures/wire/`，文件名与内容边界冻结为：`artifact_f0_pxam_v1.hex`（206 bytes）、`artifact_f0_pxak_v1.hex`（72 bytes）、`artifact_f0_pxaq_v1.hex`（176 bytes）、`artifact_f0_pxaa_v1.hex`（208 bytes）、`artifact_f0_pxmu_v1.hex`（240 bytes）、`artifact_f0_pxav_v1.hex`（192 bytes）、`artifact_f0_pxaw_materialized_v1.hex`（304 bytes）、`artifact_f0_pxaw_already_materialized_v1.hex`（304 bytes）、`artifact_f0_pxaw_failed_v1.hex`（304 bytes）、`artifact_f0_pxaw_uncertain_v1.hex`（304 bytes）与对应四份 `artifact_f0_pxax_*_v1.hex`（各240 bytes）。snapshot fixtures固定为`artifact_f0_pxaz_admitted_v1.hex`（672 bytes）、`artifact_f0_pxaz_materializing_v1.hex`（912 bytes）、`artifact_f0_pxaz_object_terminal_v1.hex`（1104 bytes）、`artifact_f0_pxaz_materialized_terminal_v1.hex`（1408 bytes）、`artifact_f0_pxaz_materialized_receipt_v1.hex`（1648 bytes）、`artifact_f0_pxaz_already_materialized_receipt_v1.hex`（2848 bytes）、`artifact_f0_pxaz_uncertain_blocked_v1.hex`（1216 bytes：object_count0、PXMU present、PXAW-U、PXAX absent、flags3、bit0=1、quarantine=0）与`artifact_f0_pxaz_uncertain_receipt_blocked_v1.hex`（1456 bytes：同一U加入matching PXAX-U、flags7，bit0/quarantine归属不变）。两份U/block fixture的filesystem harness都必须实际建立canonical attributed empty child或zero-byte temp；snapshot bytes本身不能伪造entry存在。

四份PXAW-only query expected JSON固定为`artifact_f0_query_pxaw_only_materialized_v1.json`、`artifact_f0_query_pxaw_only_already_materialized_v1.json`、`artifact_f0_query_pxaw_only_failed_v1.json`与`artifact_f0_query_pxaw_only_uncertain_v1.json`；每份都是按上述top-level key order的一个compact object加单一LF，复用对应hardcoded PXAW/PXAQ的exact operation id与O，`changed=false`、`materialization_receipt_ref=null`，并逐项冻结四行各自ok/state/diagnostic/exit语义。现有1408-byte M snapshot直接作为第一份query输入，不新造等价M snapshot。

`artifact_f0_pxop_presence_v1.txt`逐行冻结上述六个flags/length组合；`artifact_f0_store_capacity_v1.txt`只hardcode pure/injected checked calculator的component input/result、cap与overflow，不伪造canonical snapshot或history：包括全部公式、snapshot-frame `1241344` accept/`1241345` reject、`objects-64`（64 accept/65 reject）、`operations-1024`（1024 accept/1025 reject）、`quarantine-540`（540 accept/541 reject）、indexed object与额外temp各计一次的component row，以及`8388608` accept/`8388609`与arithmetic overflow reject；这些isolated rows不得被组合后宣称为constructible maximum。`blocked-zero-byte`与`last-u-attribution`语义由上述两份U/block snapshot及filesystem harness冻结：bit0=1、quarantine=0时必须存在canonical attributed empty child或zero-byte temp，无entry拒绝；PXAW-U前与matching PXAX-U后保留相同flag/bytes/block，non-last/mismatch拒绝；另有negative证明PXAV final pair只在indexed sum出现一次、额外temp才进quarantine，把stable pair再计入quarantine必须拒绝。`artifact_f0_text_refs_v1.txt`冻结exact object/ref行，所有text fixture只有单一末尾LF。PXAA/PXMU/PXAW/PXAX happy vector的ArtifactStore-global operation sequence固定为1，PXAV object sequence固定为1；bounded-recovery golden必须分别hardcode“recovery-start snapshot已有PXAV→E”与“recovery-start snapshot无PXAV、由本次恢复新发布→M”，并拒绝交换后的terminal bytes；独立successor vector必须hardcode sequence 2、same-operation/same-object replay仍为1以及`u64::MAX` preflight reject。所有frame/snapshot/JSON golden必须由独立Rust与Python decoder消费且expected bytes hardcoded，不能在测试中调用production encoder生成期望值；negative matrix逐项覆盖flags、table/sequence ordering、config、checksum/domain、short/trailing、max+1、cross-frame digest/store/id/ref、temp归因、restart M/E、fork/lock继承与cross-request next。

D0b 也不复用未锚定的既有 Deployment DTO 或 serde layout。它只复用已经 canonical 的 PXAK、PXAX、Runtime authenticated apply/Envelope 与 PXMT，并使用下文明确冻结的 artifact-bound PlanContent v2/PXTE11/PXAR12 successor；下列 external-deployment owner frames 是完整新增锚点。`execution_profile_commitment` 精确为 `SHA-256("paraegox.artifact.execution-profile.sha256.v1" || u16_be(30) || "developer-local-echo-prefix-v1" || u16_be(21) || "managed_model_data_v1" || u16_be(26) || "bounded-text-model-data-v1" || u16_be(32) || "developer-local-managed-model-v1" || u16_be(17) || "literal-prefix-v1" || u32_be(16384) || u32_be(32768) || u32_be(0))`；这些长度/limit/flag 是 bytes，不是自由字符串拼接。

`PXDQ` v1 是 fixed 288-byte D0b canonical request：

```text
0..4      magic = PXDQ
4..6      version = u16_be(1)
6         action = D
7         reserved = 0
8..10     header_len = u16_be(288)
10..12    reserved = 0
12..16    frame_len = u32_be(288)
16..32    deployment_operation_id[16]
32..64    config_commitment[32]
64..136   exact PXAK v1[72]
136..168  materialization_store_instance[32]
168..176  materialization_operation_sequence = u64_be(nonzero)
176..192  materialization_operation_id[16]
192..224  PXAX receipt_digest[32]
224..256  execution_profile_commitment[32]
256..288  deployment_request_digest[32]
```

`deployment_request_digest = SHA-256("paraegox.deployment.external-request.sha256.v1" || frame[0..256])`。`deployment_operation_id`是DeploymentController typed namespace中的D0b operation identity，必须由PXDQ/PXDK以及该operation全部PXDM/PXDO逐byte保留。四个 materialization locator 字段必须 canonical 重建用户给出的 `pxamr1` ref，并经 typed ArtifactStore read 验证一份 state `M|E` 的 exact PXAX；其中`materialization_operation_id`仍只是ArtifactStore typed namespace中的既有operation identity，不能被重新解释为deployment或Runtime operation。两类typed id都不互相派生、替代或建立alias；raw 16 bytes偶然相同不新增public reject，也不能绕过各自frame/digest/ref correlation。`F|U` Receipt 永不准入 D0b。request 不包含 mutable path、workspace A state 或 compiled-in fallback selector。

`PXDK` v1 是 fixed 240-byte fresh-only DeploymentController admission：

```text
0..4      magic = PXDK
4..6      version = u16_be(1)
6         action = D
7         state = A
8..10     header_len = u16_be(240)
10..12    reserved = 0
12..16    frame_len = u32_be(240)
16..48    controller_store_instance[32]
48..56    admission_sequence = u64_be(nonzero)
56..72    deployment_operation_id[16]
72..104   PXDQ deployment_request_digest[32]
104..176  exact PXAK v1[72]
176..208  previous_desired_head_digest[32] = all-zero
208..240  admission_digest[32]
```

`admission_digest = SHA-256("paraegox.deployment.external-admission.sha256.v1" || frame[0..208])`。`admission_sequence` 是 Controller-global deployment-admission high-water：首份 PXDK精确为1，每个后继新operation精确加一，绝不复用、倒退、跳号或按terminal顺序重排；same id/same PXDQ replay沿用原sequence。PXDK与high-water successor必须在同一durable Controller commit一起fsync；`u64::MAX`/checked addition与owner capacity在PXDK前失败关闭。若publication后无法证明old/new完整commit，只能按exact id/PXDQ bounded重开；恢复canonical PXDK后用同sequence终结原operation为U，否则Controller owner整体失败关闭、`changed = null`，不得猜测下一个sequence。F0 只接受 all-zero previous head；任何 lifecycle generation、Deployment operation 或 desired head 已存在时都必须在 PXDK 前返回 replace-required。PXDK durable publication 是 D0b 的首次 lifecycle/deployment mutation，且只能在唯一 supervisor 的 owner lock 下发生。因此 F0 workspace 最多 admission 一个 D0b operation；同 id/same PXDQ 只 replay/推进它，任何第二 id 即使第一项 failed也不建立 queue或第二 PXDK，只能等待 R0 的显式 replacement/recovery 合同。

`PXDM` v1 是 fixed 496-byte append-only deployment operation record；state byte严格为 `C`（committed）、`P`（applying）、`R`（active_ready）、`F`（failed）、`U`（uncertain）或 `S`（superseded），outcome byte严格为 nonterminal `N` 或对应 terminal `R|F|U|S`：

```text
0..4      magic = PXDM
4..6      version = u16_be(1)
6         state = C | P | R | F | U | S
7         outcome = N | R | F | U | S
8..10     header_len = u16_be(496)
10..12    reserved = 0
12..16    frame_len = u32_be(496)
16..48    controller_store_instance[32]
48..56    record_sequence = u64_be(nonzero)
56..72    deployment_operation_id[16]
72..104   PXDQ deployment_request_digest[32]
104..136  PXDK admission_digest[32]
136..208  exact PXAK v1[72]
208..240  PXAX receipt_digest[32]
240..248  deployment_revision = u64_be or zero
248..256  controller_snapshot_sequence = u64_be or zero
256..288  desired_head_digest[32] or all-zero
288..320  runtime_apply_request_digest[32] or all-zero
320..352  runtime_terminal_receipt_digest[32] or all-zero
352..368  lifecycle_generation[16] or all-zero
368..400  execution_profile_commitment[32]
400..432  previous_record_digest[32] or all-zero
432..464  superseding_desired_head_digest[32] or all-zero
464..496  operation_record_digest[32]
```

`operation_record_digest = SHA-256("paraegox.deployment.external-operation-record.sha256.v1" || frame[0..464])`。`record_sequence` 是该 operation 内从 1 开始、每个 successor 精确加一的局部序列，不是 Controller 全局 journal offset。首条 `C` 使用 all-zero previous record，要求 revision、snapshot、desired head nonzero，Runtime/generation/superseding字段全零，outcome `N`；`P` 要求前述 committed 字段与 runtime apply digest、generation nonzero，superseding 全零，outcome `N`；`R` 要求所有 committed/applying 字段与 Runtime terminal digest nonzero、superseding全零，outcome `R`。PXDQ/PXDK以及全部PXDM/PXDO的16-byte operation字段始终是同一个deployment operation id；这些frame不复制或冒充PXAR内的Runtime ApplyOperationId。PXDM/PXDO只通过`runtime_apply_request_digest`关联Runtime owner：该字段一旦nonzero就必须逐byte等于durable exact PXAR12的Envelope request digest以及关联PXMT的request digest；`runtime_terminal_receipt_digest`一旦nonzero就必须等于该exact PXMT terminal Receipt digest。`F|U` 必须逐项保留前一 verified record 的所有 nonzero 字段，只允许把本次新获得且验证相关的后续字段从 zero推进到 nonzero，不得清空或改写；outcome分别为`F|U`。若 failure/uncertainty 发生在 PXDK 后、首条 `C` 前，第一份 PXDM 可以是 `F|U`，previous record与所有尚未产生的 progress 字段全零；这不是“未 admission”。`S` 要求 superseding desired head nonzero且不同于本 operation desired head，保留全部既有字段，outcome `S`。首份 PXDM 的 previous record全零，之后必须精确引用同 operation 的前一 canonical PXDM；任何 operation 都从自己的 sequence 1开始且不能串接另一 operation 的 digest。`F|U|S` 不再接受 successor；`R` 在 F0 中同样终结，只有后续 R0 以新 operation/new desired 获得显式 replacement authority 后，才可为旧 operation追加恰一份 `S` successor并签发新的 PXDO-S；旧 PXDO-R 继续是历史 point-in-time Receipt，latest query返回 S，不能改写旧 bytes。

`PXDO` v1 是 fixed 432-byte terminal DeploymentController Receipt，只允许 `R|F|U|S`，并从 terminal PXDM逐字段复制 verified facts：

```text
0..4      magic = PXDO
4..6      version = u16_be(1)
6         action = D
7         outcome = R | F | U | S
8..10     header_len = u16_be(432)
10..12    reserved = 0
12..16    frame_len = u32_be(432)
16..48    controller_store_instance[32]
48..56    receipt_sequence = u64_be(nonzero)
56..72    deployment_operation_id[16]
72..104   PXDQ deployment_request_digest[32]
104..136  terminal PXDM operation_record_digest[32]
136..208  exact PXAK v1[72]
208..240  PXAX receipt_digest[32]
240..248  deployment_revision = u64_be or zero
248..256  controller_snapshot_sequence = u64_be or zero
256..288  desired_head_digest[32] or all-zero
288..320  runtime_apply_request_digest[32] or all-zero
320..352  runtime_terminal_receipt_digest[32] or all-zero
352..368  lifecycle_generation[16] or all-zero
368..400  superseding_desired_head_digest[32] or all-zero
400..432  deployment_receipt_digest[32]
```

`deployment_receipt_digest = SHA-256("paraegox.deployment.external-receipt.sha256.v1" || frame[0..400])`。`receipt_sequence` 是 Controller owner 在 terminal PXDO durable publication时分配的大于零、全 store 单调递增且不复用的 Receipt 序列；它不是 PXDM 的 operation-local record sequence。PXDO 的 optional-zero字段必须与 terminal PXDM逐 byte一致，`R` 要求 revision/snapshot/desired/runtime request/runtime terminal/generation全 nonzero且 superseding全零；`F|U|S` 遵循 PXDM verified-prefix preservation。canonical text `DeploymentReceiptRefV1` 固定为 `pxdor1:<64-lowerhex-controller-store-instance>:<canonical-decimal-receipt-sequence>:<32-lowerhex-operation-id>:<64-lowerhex-deployment-receipt-digest>`；它必须 canonical reparse/re-encode并经 Controller typed query取得 exact PXDQ/PXDK/PXDM/PXDO，不能从 JSON、desired 文件或 Runtime PXMT 自行合成。

D0b hardcoded shared goldens 同样位于 `tests/fixtures/wire/`，冻结为 `artifact_f0_pxdq_v1.hex`（288 bytes）、`artifact_f0_pxdk_v1.hex`（240 bytes）、`artifact_f0_pxdm_committed_v1.hex`、`artifact_f0_pxdm_applying_v1.hex`、`artifact_f0_pxdm_active_ready_v1.hex`、`artifact_f0_pxdm_failed_v1.hex`、`artifact_f0_pxdm_uncertain_v1.hex`、`artifact_f0_pxdm_superseded_v1.hex`（各496 bytes）、对应四个 terminal outcome 的 `artifact_f0_pxdo_{active_ready,failed,uncertain,superseded}_v1.hex`（各432 bytes）与加入 `artifact_f0_text_refs_v1.txt` 的 exact `pxdor1` 行。PXDK happy vector的Controller-global admission sequence固定为1；owner-private pure high-water successor vector（不构成F0第二个public admission）必须hardcode 2、same id replay仍为1与`u64::MAX` preflight reject。实现批还必须 hardcode `execution_profile_commitment` 与四个 D0b digest domain；golden、state transition与 frozen JSON expected value不能由同一 production encoder/mapper共同生成。

#### Artifact-bound Plan/Slice successor

PXDQ/PXDK/PXDM/PXDO 只拥有 external deployment operation，不能单独成为 Runtime desired state。D0b 因而固定增加一个 owner-private、只服务本 profile 的 PlanContent/PXTE/PXAR successor；它是现有 DeploymentPlanner/Controller fixed-profile PlanContent 的真实后继，不是普通 DTO，也不扩写或重新解释 D0a 的 PXTE v8、PXAR v9、PXMT v1、PXMJ v1、PXMA v1。仓库当前的 Remote successor 已占用 PXTE v10/PXAR v11，所以 Artifact F0 必须使用 collision-free 的 PXTE v11/PXAR v12。deployment、materialization与Runtime apply identity是三个typed owner namespace：各自只由本owner的request/admission/state/Receipt链持久和关联，不定义为相等或互相派生；跨namespace raw bytes偶合不赋予alias语义，也不单独构成public reject。

`ArtifactExecutionBindingV1` 是 owner-private exact 192-byte value，逐 byte 等于 `PXDQ[64..256]`，布局固定为：

```text
0..72    exact PXAK v1
72..104  materialization_store_instance[32]
104..112 materialization_operation_sequence = u64_be(nonzero)
112..128 materialization_operation_id[16]
128..160 exact PXAX receipt_digest[32]
160..192 execution_profile_commitment[32]
```

每个 identity/digest 都必须 nonzero，PXAK 必须 strict decode/re-encode；binding 自身与 PXDQ/PXAX/PXAK 逐项关联，不能从 JSON/ref 文本或路径拼出。它不是独立 public A1 surface。其 digest 精确使用 Runtime contract 的 length-framed `Digest32Builder`：

```text
Digest32Builder("paraegox.runtime.artifact-execution-binding.sha256.v1")
  .field_bytes(exact_192_byte_binding)
```

Artifact-bound `PlanContent` 沿用 exact 32-byte magic `ParaEGOX\0deployment-plan-content`，但 strict version 为 2、shape 为 3；generic PlanContent v1 decoder与bytes保持不变并与v2双向cross-reject。v2 位于既有 `managed_model_agent_stack_producer.rs` owner 内，不扩写 generic `planner.rs`，布局为：

```text
0..32    magic = "ParaEGOX\0deployment-plan-content"
32..34   version = u16_be(2)
34       shape = 3  # artifact-bound-managed-model-agent-stack
35       reserved = 0
36..40   frame_len = u32_be(exact total length)
40..56   RuntimeHostId[16]
56..248  exact ArtifactExecutionBindingV1[192]
248..252 PXTE v11 length = u32_be(nonzero, <= 2506)
252..EOF exact PXTE v11
```

`frame_len` 必须等于 `252 + PXTE11 length` 且最大 2758 bytes；target 与 PXTE11/PXMM target必须相同，PlanContent binding 与 PXTE11 内 binding 必须逐 byte相等，所有 reserved、length、EOF 与 canonical re-encode 都 strict。`PlanContentDigestV2` 精确为：

```text
Digest32Builder("paraegox.deployment.plan-content.sha256.v2")
  .field_bytes(exact_PlanContent_v2_frame)
```

Artifact-bound SourcePlanDigest 的 domain 固定为 `paraegox.deployment.artifact-bound-managed-model-agent-stack-desired.sha256.v1`。它按下列顺序使用一个 `Digest32Builder`，digest 使用 `field_digest`、identity/frame 使用 `field_bytes`、revision 使用 `field_u64`，不允许省略、换序或改成 raw string concatenation：下文独立且immutable的artifact-external cutover marker digest；target、source scope、source plan ref；successor source revision；exact active managed-Fabric predecessor target-slice digest；PXDQ deployment request digest；PXDK admission digest；PlanContentDigestV2；exact PXTE v11 bytes。PlanContentDigestV2 还必须由 PXMJ v2 单独持久保留；PXDM/PXDO 的 `desired_head_digest` 精确为 `PXAR v12.target_slice_digest()`，不能改成 PlanContentDigest、PXDQ/PXDK digest 或 object ref digest。

PXTE v11 是 big-endian exact wrapper；它保留 exact PXMM v1 与 exact PXTE v8 desired structural base，但两者都不是 active CAS predecessor。布局与上限固定为：

```text
0..4       magic = PXTE
4..6       version = u16_be(11)
6..276     exact PXMM v1[270]
276..308   artifact-bound compatibility digest[32]
308..310   artifact profile version = u16_be(1)
310..312   binding version = u16_be(1)
312..316   binding length = u32_be(192)
316..320   embedded PXTE v8 length = u32_be(nonzero, <= 1994)
320..512   exact ArtifactExecutionBindingV1[192]
512..EOF   exact PXTE v8
```

canonical frame length 由 exact EOF 决定，最大 `512 + MAX_MANAGED_MODEL_AGENT_STACK_TARGET_EXECUTION_BYTES = 2506` bytes。内嵌 PXTE v8 只允许 `FabricModelAndAgent`，Artifact F0 没有 empty/deactivate variant；outer PXMM 与 inner PXTE v8 projection 必须相同，inner Model adapter 必须等于下述 fixed mapping，PlanContent/PXTE 两份 binding 必须相同。PXTE v11 的 `ExpectedActive` 仍严格指向当次在 lower owner 中 exact reopen 的 active managed-Fabric/PXTE-v5 target slice，而不是 inner PXTE v8 或任何 PXMJ/PXMA file。execution digest 精确为：

```text
Digest32Builder("paraegox.runtime.target-execution.sha256.v11")
  .field_bytes(exact_PXTE_v11)
```

artifact-bound compatibility digest 使用 domain `paraegox.runtime.compiled-artifact-bound-managed-model-agent-stack-compatibility.sha256.v1`。它按固定顺序 length-frame：exact PXMM-v1 compatibility digest；ASCII `PXTE`、u16_be(11)、u32_be(2506)；ASCII `PXAR`、u16_be(12)、u32_be(6630)；u16_be artifact profile version 1、u16_be binding version 1、u32_be binding width 192；exact adapter id、u32_be adapter version、exact capability id；execution profile commitment；三个 exact domain strings `paraegox.runtime.target-execution.sha256.v11`、`paraegox.runtime.target-plan-assignments.sha256.v12`、`paraegox.runtime.artifact-execution-binding.sha256.v1`；ASCII `PXMT`、u16_be(1)；exact 10-byte PXTA-zero。digest字段使用 `field_digest`，上述整数分别使用 `field_u16` 或 `field_bytes(u32_be)`，其余使用 `field_bytes`；不得复用 PXMM-v1 compatibility digest 作为完整 successor digest。

PXAR v12 复用现有 canonical Runtime apply Envelope v2、Controller签名 transcript、RuntimeSliceCommitment 与 exact 10-byte PXTA-zero，outer header 固定为：

```text
0..4    magic = PXAR
4..6    version = u16_be(12)
6..10   envelope-v2 length = u32_be(<= 4096)
10..14  bindings length = u32_be(10)
14..18  PXTE v11 length = u32_be(nonzero, <= 2506)
18..    exact envelope-v2 || exact PXTA-zero || exact PXTE v11
```

PXAR v12 的componentwise pre-length arithmetic cap是6630 bytes；它是bounded read/allocation gate，不是可构造canonical signed frame的最大正例，因为Envelope自身canonical上限会让该组合不可达。cap内bytes仍必须通过每段length、exact EOF、canonical re-encode、Envelope control commitment/target/provenance、PXTA与PXTE互相一致，strict nested decode可以继续拒绝cap内noncanonical bytes。首次构造PXAR12前，Controller只在PXMJ2-A已经durable、lower predecessor已strict确认且尚无durable PXMJ2-C/PXAR12时执行一次bounded 64-byte CSPRNG draw，固定切分为独立的Runtime `ApplyOperationId[16]`、`TemporalConstraintId[16]`与authentication nonce[32]。三者都必须nonzero，且保留现有`FreshManagedModelAgentStackApplyV1::try_new`约束`ApplyOperationId != TemporalConstraintId`；不得新增Runtime id必须不同于deployment/materialization id的raw-byte reject。fresh triple只由成功的PXMJ2-C publication取得durable identity；若C从未durable，重开仍处于无Runtime identity的A prefix并可重新抽取candidate。C一旦durable，exact PXAR12就是该triple、签名与Runtime request identity的唯一事实源，之后P/R/F/U、transport、retry、query、PXMJ/PXMA reopen都必须复用同一exact bytes，不得重新抽取、重签或从deployment/materialization id派生。assignment domain 固定为 `paraegox.runtime.target-plan-assignments.sha256.v12`，算法仍且只为：

```text
Digest32Builder("paraegox.runtime.target-plan-assignments.sha256.v12")
  .field_digest(exact_PXTA_assignment_digest)
  .field_digest(PXTE_v11_execution_digest)
```

PXMT 保持 version 1、wire layout、signing transcript、digest domain、terminal enum 与2048-byte上限不变；只增加对 PXAR v12 的严格构造/相关校验入口。PXMT 的 operation id必须逐byte等于exact PXAR12 Envelope中的Runtime ApplyOperationId，而不是PXDQ中的deployment operation id或PXAQ中的materialization operation id；其request digest、assignment digest 与 target-slice digest必须来自同一exact PXAR12/PXTE11，所以 v9 request、v12 request或其 terminal bytes绝不能互换通过。既有 PXMT/D0a 构造与 bytes逐 byte不变。

##### PXMJ v2 Controller state

Controller owner-private durable state 使用 canonical `PXMJ` v2。它的 header 精确为 192 bytes，所有整数big-endian：

```text
0..4      magic = PXMJ
4..6      version = u16_be(2)
6..8      header_len = u16_be(192)
8..12     frame_len = u32_be(192 + body_len + 32)
12        phase = ASCII A | C | P | R | F | U | S
13..16    reserved = 0
16..24    controller_snapshot_sequence = u64_be(nonzero)
24..56    controller_store_instance[32]
56..64    admission_high_water = u64_be(nonzero)
64..72    receipt_high_water = u64_be
72..80    deployment_revision = u64_be or zero before C
80..112   lower_predecessor_target_slice_digest[32] or all-zero before C
112..144  PlanContentDigestV2[32] or all-zero before C
144..148  PXDQ length = u32_be(288)
148..152  PXDK length = u32_be(240)
152..156  PlanContent v2 length = u32_be(0 or <= 2758)
156..160  PXTE v11 length = u32_be(0 or <= 2506)
160..164  PXAR v12 length = u32_be(0 or <= 6630)
164..168  PXMT v1 length = u32_be(0 or <= 2048)
168..170  PXDM count = u16_be(0..=4)
170..172  PXDO count = u16_be(0..=2)
172..176  body_len = u32_be
176..192  reserved = 0
```

body 顺序严格为 `exact PXDQ || exact PXDK || exact PlanContent-v2 || exact PXTE11 || exact PXAR12 || exact PXMT1 || PXDMs-in-record-sequence || PXDOs-in-receipt-sequence`；零长度段完全不占bytes，所有header length之和必须精确等于body_len，之后恰有32-byte checksum并exact EOF。PXDQ/PXDK始终存在且逐项相关，并与每份PXDM/PXDO保留同一deployment operation id；PlanContent/PXTE/PXAR三段只能全部 absent 或全部 present，present时PlanContent内PXTE、独立PXTE和PXAR内PXTE必须逐 byte相同。PXMT若存在必须strict关联exact PXAR12，包括PXMT operation id等于PXAR Runtime ApplyOperationId；PXDM/PXDO不存该Runtime id，只把其nonzero runtime apply digest关联到同一PXAR/PXMT。PXDM必须从record sequence 1连续递增、每项previous digest精确指向前项；PXDO必须从Receipt sequence 1连续递增并精确引用对应terminal PXDM，不允许洞、换序或跨operation bytes。

trailing checksum 精确为：

```text
Digest32Builder(
  "paraegox.deployment.artifact-bound-managed-model-agent-stack-state.sha256.v2"
).field_bytes(exact_header_0_through_192 || exact_body).finish()
```

componentwise decoder/allocation upper bound精确为 `192 + 288 + 240 + 2758 + 2506 + 6630 + 2048 + 4*496 + 2*432 + 32 = 17542` bytes；它不声称存在17542-byte canonical frame，cap内noncanonical nested PXAR12仍由strict decode拒绝。超出任一分段或该总allocation上限都在decode/admission时失败关闭。PXMJ2 header 的 `controller_snapshot_sequence` 从phase A的1开始，每个durable successor精确加一且不跳号；它是owner snapshot序列，不等于PXDM/PXDO内的committed snapshot pin。phase C header精确为2，PXDM-C的`controller_snapshot_sequence`也精确为2；从此以后每一份PXDM successor与由它签发的每一份PXDO都必须保留首次C值2，即使当前PXMJ2 header已推进到P=3、R=4或后续F/U/S sequence。pre-C F/U没有committed snapshot，其PXDM/PXDO该字段必须为0。`admission_high_water` 等于latest PXDK admission sequence，Artifact F0恒为1；`receipt_high_water` 为0或latest PXDO Receipt sequence，F0首个terminal为1。mutable PXMJ2 frame checksum只证明该snapshot，绝不能用作cutover marker digest。immutable marker digest在phase A admission commit时精确派生为：

```text
Digest32Builder(
  "paraegox.deployment.artifact-external-cutover-marker.sha256.v1"
)
  .field_bytes(controller_store_instance[32])
  .field_u64(admission_high_water)
  .field_digest(PXDQ_deployment_request_digest)
  .field_digest(PXDK_admission_digest)
  .finish()
```

该四元组在每个successor都由固定header/PXDQ/PXDK strict重算，必须作为上述Artifact-bound SourcePlanDigest的首个digest字段，且PXAR12 provenance必须携带由它计算出的exact SourcePlanDigest；它不随phase、snapshot sequence、revision、PXDM/PXDO或state checksum变化，不需要第二marker字段/file，也不会与PlanContent/PXMJ2 checksum成环。

phase、presence与crash-prefix状态表固定为：

- `A`：header snapshot sequence 1、admission high-water 1、receipt high-water/revision/predecessor/PlanContentDigest全零；PlanContent/PXTE/PXAR/PXMT absent，PXDM/PXDO count均0，因此没有可伪造的PXDM snapshot字段。这是唯一首次commit；它的durable存在建立cutover marker，marker digest使用上述独立稳定公式而非frame checksum。
- `C`：header snapshot sequence 2；PlanContent/PXTE/PXAR全部present，revision精确1、predecessor与PlanContentDigest nonzero；PXAR包含本次fresh Runtime id/temporal id/auth nonce且从本commit起immutable。PXMT absent，PXDM恰为`[C]`，其runtime apply digest仍按committed shape为zero，controller snapshot字段精确为2，PXDO空。`P` header sequence 3，复用同一exact PXAR并保留相同三段与header facts，PXDM恰为`[C,P]`且两项snapshot字段都为2；PXDM-P的runtime apply digest精确关联该PXAR，PXMT/PXDO仍空。
- `R`：header snapshot sequence 4；保留完整 committed/applying prefix，PXMT present且关联PXAR12；PXDM恰为`[C,P,R]`且三项snapshot字段都为2，PXDO恰为`[R]`且其snapshot字段也为2，receipt high-water为1。Artifact F0的happy terminal到此为止。
- `F|U`：phase必须等于最后一份PXDM state，并有恰一份同outcome PXDO。若从A直接在C前终结，则terminal header sequence为2，PlanContent/PXTE/PXAR可全部absent且revision/predecessor/PlanContentDigest保持零，PXDM只含`[F]`或`[U]`，PXDM/PXDO的controller snapshot字段都为0；若在C/P后终结，header按当前durable prefix精确加一，必须保留全部已durable三段/header/PXDM prefix并只追加对应terminal，所有PXDM/PXDO的committed snapshot字段仍为2。PXMT只能在已经取得并strict验证相关PXMT时present；不能为填段而合成。`F|U`不再接受successor。
- `S` 只为后续R0读取 `R→S` 历史兼容：从R successor时header snapshot sequence为5，PXDM最多`[C,P,R,S]`、PXDO最多`[R,S]`，它们的committed snapshot字段全部仍为2，receipt high-water为2并保留原PXMT。Artifact F0不能产生S、第二operation、replacement或新Receipt；decoder可strict read，mutator必须拒绝。

任一非A phase必须逐 byte等于最后PXDM state；C/P无PXDO，R/F/U/S的latest PXDO必须等于phase。reopen必须独立重算当前header successor sequence和committed C pin：任何post-C PXDM/PXDO不是2、任何pre-C F/U不是0、R golden不是header 4/records 2，或presence/phase/high-water/chain不一致、mixed operation、old/new frame publication不明、A marker无法重建，都使整个Controller owner失败关闭。PXMJ2 frame的durable存在本身就是workspace B top-level cutover marker；不得再创建parallel marker、counter或desired file。

PXMJ v1 继续只允许PXTE8/PXAR9/PXMT1且保持现有79-byte-header codec与2-MiB bound；v2不reinterpret它。F0之前没有hardcoded PXMJ1 golden，因此不能声称“既有golden不变”：本批首次从当前v1 encoder冻结predecessor fixture，并要求之后v1 encoder逐 byte等于该fixture。v1/v2不得cross-open、自动迁移、fallback或dual-write。

##### PXMA v2 Runtime state

Runtime owner-private snapshot 使用 `PXMA` v2，保留v1 exact 208-byte outer header、4-MiB总上限、每类最多256项的replay/terminal count与顶层payload section顺序，只改变明确列出的version/domain/nested request与terminal archive。header offsets固定为：

```text
0..4      magic = PXMA
4..6      version = u16_be(2)
6..8      header_len = u16_be(208)
8..12     frame_len = u32_be(exact total)
12..20    snapshot_sequence = u64_be(nonzero)
20..52    runtime_store_instance[32]
52..84    owner_target_fingerprint[32]
84..116   transition_projection_digest[32]
116..124  fabric_generation_high_water = u64_be
124..132  model_generation_high_water = u64_be
132..140  agent_generation_high_water = u64_be
140       durable phase = u8
141..143  physical_binding_census = u16_be
143       census_complete = bool 0|1
144       fabric_ready = bool 0|1
145       model_ready = bool 0|1
146       agent_ready = bool 0|1
147       fabric_to_agent_dependency_ready = bool 0|1
148       model_to_agent_dependency_ready = bool 0|1
149..168  reserved = 0
168..172  payload_len = u32_be
172..176  reserved = 0
176..208  checksum[32]
```

payload 顶层顺序保持 `runtime_host_epoch || optional writer_fence || optional revision_high_water || optional active || optional pending || tenure_nonces || request_nonces || temporal_lineages || terminals || optional quarantine_reason`；presence tag、list count、generation/channel/replay字段与v1的canonical编码顺序不变。durable phase numeric values仍严格为 `1 ExactZero`、`2 ModelStartIntent`、`3 AgentStartIntent`、`4 ActiveReady`、`5 AgentRetireIntent`、`6 ModelRetireIntent`、`7 FabricStopIntent`、`8 RecoveryIntent`、`9 Uncertain`、`10 Quarantined`，unknown值拒绝。checksum保留v1 length-framed raw-SHA算法但使用新domain：

```text
SHA-256(
  "paraegox.runtime.managed-model-agent-stack-snapshot.sha256.v2" ||
  u64_be(176) || exact_header[0..176] ||
  u64_be(payload_len) || exact_payload
)
```

active/pending嵌套request严格改为exact PXAR12，pre-length bound为0..6630且nonempty presence必须strict decode/re-encode；PXMA v1仍只允许PXAR9并保留6118-byte pre-length bound。6119只在synthetic bounded-length/predecoder seam中证明“v1 cap+1 reject、v2 cap gate accept”，不构造或接受为canonical signed-frame positive；6630同样只是v2 arithmetic cap，随后strict nested decode仍可拒绝noncanonical bytes。真实positive只使用可构造canonical PXAR9/PXAR12 fixture，且任何v12不能进入v1、任何v9不能进入v2。v2 terminal record不再只存一个untyped request digest，canonical record精确为：

```text
source_scope[16]
operation_id[16]
request_digest[32]
request_len u32_be(nonzero, <= 6630)
exact PXAR12
receipt_len u32_be(nonzero, <= 2048)
exact PXMT1
```

decode/restart必须证明stored request_digest等于PXAR12 envelope request digest且等于PXMT request digest，terminal的operation id逐byte等于PXAR12 Runtime ApplyOperationId且等于PXMT operation id，并逐项复验source scope、assignment digest、target-slice/desired-head、response channel、Runtime store/key与signature；它绝不等同或映射为deployment/materialization operation id，只比较digest也不能准入。按independent field caps计算的v2 terminal `8750` bytes与整份snapshot `2303090` bytes都只是componentwise checked arithmetic/preallocation ceilings，不是可构造canonical positive大小；cap内nested request仍必须strict decode，noncanonical bytes照常拒绝。该allocation ceiling仍小于保留的4194304-byte outer bound；count、checked addition或outer bound任一失败都拒绝整份snapshot。

PXMA v2 的optional pending body保留v1中kind所在的exact one-byte位置，但该byte是version-specific contract，v2严格定义为：

```text
1 = ActivateArtifactV12
2 = RetireCurrentArtifactV12
3 = RecoverActiveV12
```

- tag 1只允许active absent，pending必须携带本次exact PXAR12 activation request；它不接受PXAR9、empty或另一个operation。
- tag 2只允许durable phase 5/6/7，要求active present；pending中的exact PXAR12必须逐 byte等于`active.request`，response channel及Fabric/Model/Agent三个generation必须分别等于active值。它只授权依序清理这份retained request已启动的current generations，不产生第二signed request、新desired、deactivate request或PXTE `EmptyDeactivate`，也不取得replacement authority。phase 5/6/7的readiness、dependency-ready与physical-census shape继续逐项使用现有v1 invariant，不能因tag改名而放宽。
- tag 3只允许retained recovery：active present时，pending的exact PXAR12、response channel和三个generation必须逐项等于active。active absent只在durable phase 8允许，且terminal archive中必须恰有一条source scope与operation id同时等于pending的完整ActiveReady record；pending exact request与response channel必须从该record的exact PXAR12取得并逐byte相等，Fabric/Model/Agent三个generation、readiness与physical-census facts必须从其exact PXMT1取得并逐项相等，同时通过上文全部request digest、signature、Runtime store/key、assignment、target-slice/desired-head与channel关联。匹配record为zero或multiple、terminal非ActiveReady、任一field漂移都拒绝整份snapshot；不得从caller、digest-only terminal、PXAR9、新request或v1 active-absent permissive behavior补值。

tag 0、4..255、phase/presence不匹配、tag2/3 request/channel/generation任一漂移全部拒绝整份snapshot。PXMA v1的one-byte tags继续保持`1 ActivateStack`、`2 DeactivateStack`、`3 RecoverActive`及其PXAR9/EmptyDeactivate语义，不能用v2名称重新解释；v1/v2 decoder按outer version双向cross-reject。缺少retained request/correlation时v2只能进入Uncertain/Quarantined，不能发明deactivate contract或回退D0a。

PXMA v1继续只允许PXAR9/PXMT1并保留原terminal codec；F0之前同样没有hardcoded PXMA1 golden，本批首次冻结当前v1 encoder的predecessor fixture。PXMA1/PXMA2双向cross-reject且restart不能降级、重新封装或仅凭request digest把v9 terminal当成v12。

本 profile 与现有 `ManagedModelAdapterBindingV1` 的映射固定为：

```text
adapter_id      = ASCII "px-art-prefix-v1"  # exact 16 bytes
adapter_version = u32_be(1)
capability_id   = ASCII "px-bounded-text1" # existing exact 16 bytes
```

Runtime 只有在该 binding、execution profile commitment、PXAM literals/limits与完整 ArtifactExecutionBinding 都匹配时才可调用 artifact backend。D0a 的 `px-fixture-echo1` binding 不接受 Artifact bytes，也不能成为 fallback。现有 `RuntimeModelBackendResolverV1` 只增加一个 default-fail artifact方法，使旧实现者与原 `resolve(plan)` D0a路径无需修改：resolver-owned read-only port按 Slice binding返回 exact PXAX/PXAW/PXMU/PXAV/PXAA/PXAQ/PXAM/payload read bundle；Runtime 自己逐层strict reverify，并在任何 generation/workload effect前把 payload复制到generation-owned内存；artifact resolver只接受上述fixed adapter并产生 `payload || prompt` backend。任何missing/tamper/race/unsupported都直接失败，不调用旧resolve、不切fixture、不扫描store。

#### Controller top-level desired authority 与 cutover

`ArtifactExternalDeploymentControllerStoreV1` 位于既有 `managed_model_agent_stack_apply.rs`，持久编码PXMJ v2；它独占PXDQ/PXDK、PlanContent v2/PlanContentDigestV2、PXAR12、DeploymentRevision、PXDM chain、PXDO以及Controller-global admission/Receipt两个high-water，是workspace B唯一external top-level desired authority。既有 `ManagedFabricSuccessorStoreV1` 只保留下层active Fabric predecessor authority，不再是external top-level desired head，不能签发D0b operation/Receipt。

唯一supervisor必须同时持有两者的owner lock并遵守固定effect顺序：包含exact PXDQ/PXDK与admission high-water 1的canonical PXMJ2-A frame在同一首次durable commit发布；该frame的存在是workspace B第一次lifecycle/deployment mutation与唯一cutover marker，marker digest按上述immutable四元组公式派生而非使用mutable frame checksum；之后才可初始化lower Authority/Runtime并建立managed-Fabric predecessor；exact active Fabric predecessor确认后，Controller按上述一次64-byte CSPRNG draw构造fresh Runtime ApplyOperationId/TemporalConstraintId/auth nonce与signed PXAR12，并在一次top-level PXMJ2-C durable successor内原子写入PlanContent v2、PlanContentDigestV2、PXTE11、successor revision、该exact PXAR12与PXDM-C；transport前先以同一PXAR的request digest durable PXMJ2-P/PXDM-P；只在strict验证同一Runtime operation的相关PXMT1后提交PXMJ2-R、PXDM-R与PXDO-R/Receipt high-water。任一步失败或不确定只按PXDQ/PXDK/PXDM已验证prefix终结F/U，不回滚/重写已durable facts，也不调用D0a。

PXMJ v2 marker一旦durable存在，同一workspace中所有D0a或旧managed-stack入口都只能返回replace-required；lower store不能在supervisor重启、top-level failure或output loss后独立重开成为第二desired writer。PXMJ2 reopen必须在同一lock scope strict reopen lower predecessor与全部top-level bytes；缺失、版本混合、head冲突或无法证明old/new commit都失败关闭，不从lower store反推top-level success。这样PXMJ2是唯一desired head，PXMA2仍只是Runtime live/recovery owner，不形成Controller双写。

新增 successor hardcoded shared goldens 固定为 `artifact_f0_binding_v1.hex`（192 bytes）、`artifact_f0_plan_content_v2.hex`、`artifact_f0_pxte_v11.hex`、`artifact_f0_runtime_slice_v11.hex`（exact PXTA-zero || PXTE11）、`artifact_f0_pxar_v12.hex`、`artifact_f0_pxmt_artifact_v1.hex`，以及本批首次建立的四份primary state fixture `artifact_f0_pxmj_v1.hex`、`artifact_f0_pxmj_v2.hex`、`artifact_f0_pxma_v1.hex`、`artifact_f0_pxma_v2.hex`；每份`.hex`都只能是单行lower-case hex加一个LF。所有wire/state golden的测试常量只能来自单一shared semantic ledger `tests/fixtures/wire/artifact_f0_semantic_ledger_v1.json`，其他fixture不得复制一份可独立漂移的常量表。ledger的top-level key顺序精确为`format,payload_utf8,artifact_store,deployment,runtime_apply,authority,signing,predecessor,agent_plan,model_plan,runtime_terminal,controller_shapes,runtime_state,text`；内容必须是下列单行canonical compact JSON加一个LF，不能pretty-print、重排key、转义ASCII、追加空白或第二个换行。`artifact_store.snapshot_frame_bytes`只表示PXAZ frame length；`artifact_store.accounted_logical_bytes`逐项映射该snapshot header的`accounted_rest_bytes`，二者不得混同：

```json
{"format":"paraegox-artifact-f0-semantic-ledger-v1","payload_utf8":"artifact-f0-prefix: ","artifact_store":{"store_instance_hex":"a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0","config_commitment_hex":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","primary_operation_id_hex":"a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2","second_operation_id_hex":"a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3","primary_operation_sequence":1,"second_operation_sequence":2,"object_sequence":1,"failed_materializing_present":false,"uncertain_materializing_present":true,"uncertain_object_terminal_present":false,"snapshot_sequences":{"admitted":1,"materializing":2,"object_terminal":3,"materialized_terminal":4,"materialized_receipt":5,"already_materialized_receipt":9,"uncertain_blocked":3,"uncertain_receipt_blocked":4},"snapshot_frame_bytes":{"admitted":672,"materializing":912,"object_terminal":1104,"materialized_terminal":1408,"materialized_receipt":1648,"already_materialized_receipt":2848,"uncertain_blocked":1216,"uncertain_receipt_blocked":1456},"accounted_logical_bytes":{"admitted":672,"materializing":912,"object_terminal":1330,"materialized_terminal":1634,"materialized_receipt":1874,"already_materialized_receipt":3074,"uncertain_blocked":1216,"uncertain_receipt_blocked":1456}},"deployment":{"controller_store_instance_hex":"d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0","operation_id_hex":"d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1","lifecycle_generation_hex":"d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2","superseding_desired_head_digest_hex":"d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3","admission_sequence":1,"deployment_revision":1,"receipt_sequence":1,"controller_committed_snapshot_sequence":2},"runtime_apply":{"artifact_operation_id_hex":"d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4","artifact_temporal_constraint_id_hex":"d5d5d5d5d5d5d5d5d5d5d5d5d5d5d5d5","artifact_authentication_nonce_hex":"d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6","legacy_operation_id_hex":"6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d","legacy_temporal_constraint_id_hex":"6e6e6e6e6e6e6e6e6e6e6e6e6e6e6e6e","legacy_authentication_nonce_hex":"6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f","clock_domain_hex":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","clock_generation":3,"original_budget_nanos":6000083,"remaining_budget_nanos":6000083,"runtime_store_instance_hex":"4444444444444444444444444444444444444444444444444444444444444444"},"authority":{"source_scope_hex":"01010101010101010101010101010101","source_plan_ref_hex":"02020202020202020202020202020202","writer_hex":"09090909090909090909090909090909","writer_principal_hex":"09090909090909090909090909090909","writer_epoch":1,"tenure_authority_hex":"07070707070707070707070707070707","tenure_key_ref_hex":"08080808080808080808080808080808","tenure_algorithm":1,"tenure_algorithm_version":1,"tenure_supersedes_through_epoch":0,"tenure_nonce_utf8":"test-only-tenure-nonce","request_principal_hex":"09090909090909090909090909090909","request_key_ref_hex":"0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c","request_algorithm":1,"request_algorithm_version":1},"signing":{"test_only":true,"tenure_seed_hex":"1111111111111111111111111111111111111111111111111111111111111111","tenure_public_key_hex":"d04ab232742bb4ab3a1368bd4615e4e6d0224ab71a016baf8520a332c9778737","controller_seed_hex":"2222222222222222222222222222222222222222222222222222222222222222","controller_public_key_hex":"a09aa5f47a6759802ff955f8dc2d2a14a5c99d23be97f864127ff9383455a4f0","runtime_seed_hex":"7777777777777777777777777777777777777777777777777777777777777777","runtime_public_key_hex":"c853ad0f0cd2b619aea92ceec4fd56a24d6499d584ce79257e45cfd8139b60a7"},"predecessor":{"target_hex":"05050505050505050505050505050505","manifest_digest_hex":"fad22cd7f146653019a6b9570d06c222a34689d5b669481cdb7b314ec05edf53","build_instance_id_hex":"1111111111111111111111111111111111111111111111111111111111111111","build_descriptor_digest_hex":"29e532abc1ac2f6ea13b45ce7029020e2863e1d302c5cdab0dab0e272652a2c1","runtime_artifact_sha256_hex":"2222222222222222222222222222222222222222222222222222222222222222","active_target_slice_digest_hex":"2044e64adb9e0744f0334b8dc0ee816b1d2f857f655a7a576c9b61cf1efe51da","legacy_cutover_marker_digest_hex":"c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0","legacy_source_revision":3,"legacy_successor_revision":4,"fabric_service_id_hex":"51515151515151515151515151515151","fabric_prepare_budget_nanos":1000000000,"fabric_start_budget_nanos":2000000000,"fabric_readiness_budget_nanos":3000000000,"fabric_drain_budget_nanos":4000000000,"fabric_stop_budget_nanos":5000000000,"fabric_listen_endpoint":"tcp/127.0.0.1:7447"},"agent_plan":{"service_id_hex":"65656565656565656565656565656565","prepare_budget_nanos":1000000,"start_budget_nanos":2000000,"readiness_budget_nanos":3000000,"drain_budget_nanos":4000000,"stop_budget_nanos":5000000,"max_sessions":16,"max_turns_per_session":64,"max_requests_per_session":64,"max_event_batch":64,"submit_binding_id_hex":"61616161616161616161616161616161","control_binding_id_hex":"62626262626262626262626262626262","submit_key_expression":"paraegox/agent/v1/submit","control_key_expression":"paraegox/agent/v1/control","max_items":8,"max_bytes":262144,"max_frame_bytes":65536,"max_response_body_bytes":65536,"handler_timeout_nanos":5000000000,"provider_profile":2,"provider_ref_hex":"63636363636363636363636363636363","provider_config_digest_hex":"6464646464646464646464646464646464646464646464646464646464646464","provider_secret_present":false},"model_plan":{"service_id_hex":"89898989898989898989898989898989","prepare_budget_nanos":23,"start_budget_nanos":29,"readiness_budget_nanos":31,"drain_budget_nanos":37,"stop_budget_nanos":41,"max_in_flight":8,"artifact_adapter_id":"px-art-prefix-v1","legacy_adapter_id":"px-fixture-echo1","adapter_version":1,"capability_id":"px-bounded-text1"},"runtime_terminal":{"runtime_peer_hex":"71717171717171717171717171717171","local_endpoint_identity_digest_hex":"7272727272727272727272727272727272727272727272727272727272727272","peer_credentials_digest_hex":"7373737373737373737373737373737373737373737373737373737373737373","response_key_ref_hex":"76767676767676767676767676767676","response_algorithm":1,"response_algorithm_version":1,"fabric_generation":7,"model_generation":8,"agent_generation":9,"outcome":"active_ready","lifecycle_effect":"may_have_started","head":"committed_incoming","physical_binding_census":2,"census_complete":true,"fabric_ready":true,"model_ready":true,"agent_ready":true,"fabric_to_agent_dependency_ready":true,"model_to_agent_dependency_ready":true,"exact_zero":false,"quarantined":false,"resource_census_digest_hex":"7474747474747474747474747474747474747474747474747474747474747474","raw_outcome_digest_hex":"7575757575757575757575757575757575757575757575757575757575757575","completion_runtime_host_epoch":9,"completion_snapshot_sequence":11,"selection_clock_generation":3,"selection_observed_at_nanos":13},"controller_shapes":{"pxmj1_phase":"receipt_durable","pxmj1_archived_active_present":false,"pxmj2_phase":"R","pxmj2_snapshot_sequence":4,"pxdm_record_sequences":[1,2,3],"pxdo_receipt_sequence":1,"failed_prefix":"pre_commit","uncertain_prefix":"pre_commit","superseded_prefix":"post_ready","superseded_record_sequence":4,"superseded_receipt_sequence":2},"runtime_state":{"owner_target_fingerprint_hex":"5555555555555555555555555555555555555555555555555555555555555555","transition_projection_digest_hex":"6666666666666666666666666666666666666666666666666666666666666666","snapshot_sequence":12,"runtime_host_epoch":9,"fabric_generation_high_water":7,"model_generation_high_water":8,"agent_generation_high_water":9,"legacy_revision_high_water":4,"artifact_revision_high_water":1,"pending_admitted_clock_generation":3,"pending_admitted_at_nanos":100,"pending_deadline_nanos":6000183,"include_writer_fence":true,"include_revision_high_water":true,"include_one_tenure_replay":true,"include_one_request_replay":true,"include_one_temporal_replay":true,"primary_shape":"active_ready_with_matching_terminal","pending_activate_shape":"phase2_tag1_active_absent_terminal_absent_generations_7_8_absent","pending_retire_shape":"phase5_tag2_active_present_matching_terminal_generations_7_8_9","pending_recover_active_shape":"phase8_tag3_active_present_matching_terminal_generations_7_8_9","pending_recover_archived_shape":"phase8_tag3_active_absent_unique_matching_active_ready_terminal_generations_7_8_9"},"text":{"pxop_presence_lines":["flags=0,length=416","flags=1,length=656","flags=2,length=720","flags=3,length=960","flags=6,length=960","flags=7,length=1200"],"capacity_lines":["stable-components=1241344+64*(206+64)+540,result=1259164","transaction-components=1259164+1241344+270,result=2500778","snapshot-frame=1241344,result=accept","snapshot-frame=1241345,result=reject","objects=64,result=accept","objects=65,result=reject","operations=1024,result=accept","operations=1025,result=reject","quarantine=540,result=accept","quarantine=541,result=reject","indexed-object-bytes=270,extra-temp-bytes=270,count=once-each,result=accept","defense-ceiling=8388608,result=accept","defense-ceiling=8388609,result=reject","checked-add=u64-max-plus-one,result=reject"],"reference_labels":["artifact_object_ref","materialization_receipt_ref","deployment_receipt_ref"],"successor_digest_labels":["binding","compatibility","execution","assignment","cutover_marker","plan_content","source_plan","target_slice"]}}
```

ledger中未直接列出的digest、checksum、replay、channel与terminal-result bytes全部严格按本Program已冻结domain/codec从上述语义常量派生，expected bytes由独立实现生成后hardcode，不能调用production encoder充当oracle。三份签名分别使用`tenure_seed_hex`、`controller_seed_hex`、`runtime_seed_hex`，不得跨owner复用；PXDQ固定绑定primary PXAX-M；F/U fixture固定为pre-C首record，S fixture固定为C/P/R/S chain；v1 fixture使用legacy fresh triple与`px-fixture-echo1`，v2 fixture使用artifact fresh triple与`px-art-prefix-v1`。production绝不读取ledger或test seed：它继续在首次PXAR12 candidate前执行上述one-shot 64-byte CSPRNG并固定切分，不能从deployment operation id或fixture bytes派生。

当前golden的materialization、deployment、Runtime与temporal typed constants故意互不相同，但这种fixture取值不新增跨owner raw-inequality规则。PXMJ2 fixture固定为phase R、header snapshot sequence 4、admission/Receipt high-water均1、revision 1、exact PlanContent/PXTE11/PXAR12/PXMT和PXDM `[C,P,R]`、PXDO `[R]`，其中每份PXDM/PXDO的committed controller snapshot字段都精确为2；PXMA2 fixture固定为ActiveReady，active与terminal archive内是同一exact PXAR12，terminal同时保留关联PXMT1，且两者operation id都等于PXAR Runtime ApplyOperationId。两份v1 fixture由当前predecessor encoder首次冻结，之后必须证明v1 encoder逐 byte等于它们，而不是声称仓库此前已有hardcoded PXMJ1/PXMA1 golden。

PXMA2 pending kind由四份hardcoded companion fixture逐 byte冻结：`artifact_f0_pxma_v2_pending_activate.hex` 使用tag1且active absent；`artifact_f0_pxma_v2_pending_retire_current.hex` 使用phase5/tag2且active present、request/channel/三generation逐项相同；`artifact_f0_pxma_v2_pending_recover_active.hex` 使用tag3并逐项重用retained active facts；`artifact_f0_pxma_v2_pending_recover_archived.hex` 使用phase8/tag3、active absent，并由唯一matching完整ActiveReady terminal PXAR12/PXMT1逐项恢复上述全部facts。negative matrix必须对每份fixture分别mutation tag、phase、active presence、request byte、response channel与每个generation；archived fixture还要分别证明zero/multiple/non-ActiveReady terminal以及readiness/census/store/key/signature/assignment/desired任一漂移都拒绝。tag2不接受新signed request/PXTE EmptyDeactivate，tag3不接受digest-only或caller-supplied recovery，v1 tag2仍只按原PXAR9 DeactivateStack语义解码。

`artifact_f0_successor_digests_v1.txt` 以固定顺序hardcode binding、compatibility、execution、assignment、cutover-marker、PlanContent、SourcePlan与target-slice digest及单一LF。expected bytes/digests必须由独立decoder消费，不能调用production encoder生成。真实hardcoded positive只使用现有可构造canonical PXAR9 fixture与本批可构造canonical PXAR12 fixture，并证明双向cross-version reject。synthetic bounded-length/predecoder seam另冻结6118为v1 arithmetic cap、6119为v1 cap+1 reject但v12 cap gate accept、6630为v12 arithmetic cap、6631为v12 cap+1 reject；6118/6119/6630都不得靠padding、扩字段或伪造signature构造成所谓“最大canonical positive”，cap内noncanonical bytes必须继续被full strict decoder拒绝。v12不能进入v1 state/terminal，v9不能进入v2 state/terminal，四份state fixture必须双向magic/version/checksum/canonical cross-reject。

successor最小negative matrix必须逐项hardcode，不能用一个fuzz/round-trip case概括：

- PXTE v8/v10/v11 与 PXAR v9/v11/v12 全部错误组合cross-reject；每个新frame逐项拒绝magic/version/length/reserved/trailing/oversize与noncanonical re-encode；
- binding每个字段的single-bit mutation、全零identity/digest、sequence 0、PXAK pair swap、PXAX `F|U`、PXDQ/PXAX locator drift及execution-profile mismatch全部pre-effect拒绝；
- inner PXTE8 empty、outer/inner projection drift、adapter id/version/capability drift、PlanContent/PXTE binding drift与wrong ExpectedActive全部拒绝；binding任一bit必须同时改变binding、PlanContent、SourcePlan、execution/assignment与target-slice关联digest；
- Runtime read chain missing/tampered/wrong Receipt、inode/pair replacement及reopen→effect race全部在Model/Agent effect前失败，且零fixture fallback；
- PXMT-v12 correlation不能使用v9 request、assignment或slice通过；真实canonical PXAR9/PXAR12 positives、6118/6119/6630/6631 synthetic predecoder边界、PXMA2 exact-PXAR12 terminal archive与四份PXMJ/PXMA v1/v2 restart fixture必须证明v9/v12不能重新封装或跨版通过，且synthetic length accept绝不冒充signed-frame positive。operation-correlation negatives分别交换deployment/materialization/Runtime/temporal typed role、漂移PXAR Runtime id或PXMT/PXMA terminal id、漂移PXDM/PXDO runtime request digest，并证明C durable后重开绝不重新抽取fresh triple；它们验证role/correlation而不是仅以跨namespace raw equality拒绝。PXMJ2 mutation/reopen negatives逐项交换header 1/2/3/4与PXDM/PXDO committed pin 0/2，证明A无record、pre-C terminal只能0、post-C全链只能2且R header只能4。PXMA2 restart精确恢复v12 ActiveReady/pending/terminal，版本混合或缺证据绝不回退D0a；PXMJ2 marker存在时lower predecessor与D0a入口不能独立重开desired authority；
- architecture/dependency guard证明没有Graph、Application/Installation、Process/reference worker、第二ArtifactStore writer、第二Controller desired head、dynamic adapter、retry、fallback或payload/path ingress到Controller。

`artifact build` 与 `artifact inspect` 的 JSON v1 top-level key 顺序严格且仅有：

```text
schema_version
command
ok
changed
profile
artifact_object_ref
payload_length
runtime_kind
adapter_abi
target_profile
diagnostics
```

`artifact materialize` 与 `artifact materialization query` 的 JSON v1 top-level key 顺序严格且仅有：

```text
schema_version
command
ok
changed
operation_id
state
artifact_object_ref
materialization_receipt_ref
diagnostics
```

其 `state` enum 严格为 `admitted|materializing|materialized|already_materialized|failed|uncertain`。D0b `deploy` 与 `deployment operation query` 的 JSON v1 top-level key 顺序严格且仅有：

```text
schema_version
command
mode
ok
changed
operation_id
state
profile
artifact_object_ref
materialization_receipt_ref
generation
deployment_revision
controller_snapshot_sequence
deployment_receipt_ref
runtime_apply_request_digest
runtime_terminal_receipt_digest
terminal_outcome
current_health_checked
diagnostics
```

其 `state` enum 严格为 `admitted|committed|applying|active_ready|failed|uncertain|superseded`。

JSON scalar 类型不由值猜测：`schema_version` 与 `payload_length` 是 JSON number；`ok`、非 null `changed`、`current_health_checked` 是 JSON boolean；`diagnostics` 是 array；其余 non-null 字段全部是 JSON string。下表中的 `PF` 精确表示 profile string `"developer-local-echo-prefix-v1"`，`O` 表示 canonical object ref，`MR` 表示 canonical `pxamr1`，`DR` 表示 canonical `pxdor1`，`V` 表示必须从最后一份 owner-verified canonical record 原样保留的值；`—` 精确表示 JSON null，不允许省略 key、空字符串、零字符串、空 object/array 或错误类型代替。

build/inspect 的逐 outcome envelope 固定为：

| command / outcome | `ok` | `changed` | `profile` | `artifact_object_ref` | `payload_length` | `runtime_kind` / `adapter_abi` / `target_profile` | `diagnostics` | exit |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| build / first published pair | true | true | `developer-local-echo-prefix-v1` | O | 1..64 number | 三个固定字符串 | `[]` | 0 |
| build / exact byte-identical replay | true | false | 同上 | O | 1..64 number | 三个固定字符串 | `[]` | 0 |
| inspect / valid exact pair | true | false | 同上 | O | 1..64 number | 三个固定字符串 | `[]` | 0 |
| build 或 inspect / applicable grammar、platform、path、profile、compatibility 或 A0 pre-effect reject | false | false | — | — | — | 三字段均 — | 恰一项对应 exit-2 stable diagnostic | 2 |
| build 或 inspect / execution identity reject | false | false | — | — | — | 三字段均 — | `PXLC-EXECUTION-IDENTITY` 恰一项 | 1 |
| build / owner effect 或 durability 无法证明 | false | — | — | — | — | 三字段均 — | `PXLC-ARTIFACT-UNCERTAIN` 恰一项 | 1 |
| build 或 inspect / I/O failure 且已证明零 mutation | false | false | — | — | — | 三字段均 — | `PXLC-ARTIFACT-IO` 恰一项 | 1 |

`inspect` 永远不能产生 `changed = true|null`；build 只有完整 pair final publish、directory sync与exact reopen全部归因于本 invocation 时为 true。stdout delivery failure 没有一份可声称“已交付”的 JSON envelope；实现仍必须尝试 `PXLC-ARTIFACT-JSON-OUTPUT`、exit 1，且不能因此改变或回滚已 durable 的 owner事实。

materialization 的逐 state envelope 固定为：

| command / owner outcome | `ok` | `changed` | `operation_id` | `state` | `artifact_object_ref` | `materialization_receipt_ref` | `diagnostics` | exit |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| materialize / `admitted` | true | true 或 replay 时 false | exact input 32hex | `admitted` | O | — | `[]` | 0 |
| materialize / `materializing` | true | true 或 replay/readback时 false | exact input 32hex | `materializing` | O | — | `[]` | 0 |
| materialize / `materialized` | true | true 或 terminal replay时 false | exact input 32hex | `materialized` | O | MR（PXAX state M） | `[]` | 0 |
| materialize / `already_materialized` | true | 新 operation 完成关联时 true、terminal replay时 false | exact input 32hex | `already_materialized` | O | MR（PXAX state E） | `[]` | 0 |
| materialize / `failed` | false | 已证明本调用推进时 true、纯 replay时 false | exact input 32hex | `failed` | O | MR（PXAX state F） | `PXLC-ARTIFACT-MATERIALIZATION-FAILED` 恰一项 | 1 |
| materialize / `uncertain` | false | 无法证明 effect 时 —；已证明 terminal commit归因时 true；纯 replay时 false | exact input 32hex | `uncertain` | O | MR（PXAX state U） | `PXLC-ARTIFACT-UNCERTAIN` 恰一项 | 1 |
| materialization query / PXAW-M present、PXAX absent | true | false | exact input 32hex | `materialized` | O | — | `[]` | 0 |
| materialization query / PXAW-E present、PXAX absent | true | false | exact input 32hex | `already_materialized` | O | — | `[]` | 0 |
| materialization query / PXAW-F present、PXAX absent | false | false | exact input 32hex | `failed` | O | — | `PXLC-ARTIFACT-MATERIALIZATION-FAILED` 恰一项 | 1 |
| materialization query / PXAW-U present、PXAX absent | false | false | exact input 32hex | `uncertain` | O | — | `PXLC-ARTIFACT-UNCERTAIN` 恰一项 | 1 |
| materialization query / `admitted`、`materializing` 或 PXAX-present terminal | 与同 state 上述值相同 | false | exact input 32hex | 对应 exact state | 按同 state | 按同 state | 按同 state | 按同 state |
| materialization query / NotFound | false | false | exact input 32hex | — | — | — | `PXLC-ARTIFACT-NOT-FOUND` 恰一项 | 1 |
| materialization query / attributable same-request staging，或canonical same-operation N+1且active尚无terminal | false | false | exact input 32hex | `uncertain` | O | — | `PXLC-ARTIFACT-UNCERTAIN` 恰一项 | 1 |
| 任一 materialization 命令 / strict-valid lock `WouldBlock` 或 `EWOULDBLOCK` | false | false | exact input 32hex | `uncertain` | — | — | `PXLC-ARTIFACT-UNCERTAIN` 恰一项 | 1 |
| 任一 materialization 命令 / strict-valid owner上的其他lock syscall failure | false | false | exact input 32hex | — | — | — | `PXLC-ARTIFACT-IO` 恰一项 | 1 |
| 任一 materialization 命令 / partial、unknown或cross-request staging/next，final+staging，extra entry，或root/lock/snapshot strict failure | false | false | exact input 32hex | — | — | — | `PXLC-ARTIFACT-OWNER` 恰一项 | 1 |
| 任一 materialization 命令 / 其逐命令 total order 所列的 exit-2 pre-effect reject | false | false | 仅当 32hex 已 strict parse 时保留，否则 — | — | — | — | 恰一项对应 exit-2 stable diagnostic | 2 |
| 任一 materialization 命令 / execution identity reject | false | false | 仅当 32hex 已 strict parse 时保留，否则 — | — | — | — | `PXLC-EXECUTION-IDENTITY` 恰一项 | 1 |
| materialize / 本invocation已推进owner effect、但之后无法读取足够record分类 | false | — | exact input 32hex | `uncertain` | 已由 PXAQ 验证时 O，否则 — | 已验证 PXAX 时 MR，否则 — | `PXLC-ARTIFACT-UNCERTAIN` 恰一项 | 1 |

`admitted` 精确对应只有 PXAA、尚无 PXMU/PXAW；`materializing` 精确对应 PXMU 已 durable、尚无 PXAW。MR 对所有四种 PXAX terminal state 都必须保留；PXAW-only四行即使已terminal也固定MR null且绝不由query补签PXAX。只有 state `M|E`且PXAX已存在的完整链可供 D0b admission，不能因PXAW-only成功行或`failed|uncertain`的Receipt非null而升级为materialized。

mutating materialize只有在PXAX successor durable publication与final snapshot exact reopen均成功后才可形成terminal output；PXAW-only terminal只允许query按上表只读返回。materialize、materialization query与terminal replay的每一条return path——success、progress、terminal error、pre-effect error、lock `WouldBlock`、owner/IO failure与NotFound——都必须先把已验证public projection复制成不持有fd/guard/borrow的owned value，再显式unlock并drop lock、root、objects、child、snapshot、pair及其全部clone/handle，之后才允许JSON serialize、stdout write与flush。serializer/writer不得捕获任何owner handle；blocked stdout期间其他合法query/mutation必须能独立进入lock acquisition。serialize/write/flush失败只尝试既有JSON-output failure/exit路径，不重新取得ArtifactStore lock、不重读/回滚snapshot、不删除temp、不改变`changed`归因，也不能因未交付输出改写已durable事实。

D0b 的逐 state envelope 固定为：

| command / owner outcome | `ok` | `changed` | `operation_id` / `state` / `profile` | object / MR | `generation` | revision / snapshot | DR | Runtime request / terminal digest | `terminal_outcome` | diagnostics / exit |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| deploy / `admitted` | true | true 或 replay时 false | exact id / `admitted` / PF | O / MR | — | — / — | — | — / — | — | `[]` / 0 |
| deploy / `committed` | true | true 或 replay/readback时 false | exact id / `committed` / PF | O / MR | — | nonzero decimal / nonzero decimal | — | — / — | — | `[]` / 0 |
| deploy / `applying` | true | true 或 replay/readback时 false | exact id / `applying` / PF | O / MR | 32hex | nonzero decimal / nonzero decimal | — | 64hex / — | — | `[]` / 0 |
| deploy / `active_ready` | true | true 或 terminal replay时 false | exact id / `active_ready` / PF | O / MR | 32hex | nonzero decimal / nonzero decimal | DR | 64hex / 64hex | `active_ready` | `[]` / 0 |
| deploy / `failed` | false | owner推进归因时 true、纯 replay时 false | exact id / `failed` / PF | O / MR | V 或 — | V 或 — / V 或 — | DR | V 或 — / V 或 — | `failed` | `PXLC-DEPLOY-FAILED` / 1 |
| deploy / `uncertain` | false | effect无法证明时 —；terminal commit归因时 true；纯 replay时 false | exact id / `uncertain` / PF | O / MR | V 或 — | V 或 — / V 或 — | DR | V 或 — / V 或 — | `uncertain` | `PXLC-DEPLOY-UNCERTAIN` / 1 |
| deploy / `superseded` | false | owner推进归因时 true、纯 replay时 false | exact id / `superseded` / PF | O / MR | V 或 — | V / V | DR | V 或 — / V 或 — | `superseded` | `PXLC-DEPLOY-SUPERSEDED` / 1 |
| deployment operation query / 任一已存在 state | 与同 state 上述值相同 | false | 按同 state | 按同 state | 按同 state | 按同 state | 按同 state | 按同 state | 按同 state | 按同 state |
| deployment operation query / NotFound | false | false | exact input id / — / — | — / — | — | — / — | — | — / — | — | `PXLC-DEPLOY-NOT-FOUND` / 1 |
| 任一 D0b 命令 / 其逐命令 total order 所列的 exit-2 pre-effect reject | false | false | 仅当 id 已 strict parse时保留 / — / — | — / — | — | — / — | — | — / — | — | 恰一项对应 exit-2 stable diagnostic / 2 |
| 任一 D0b 命令 / execution identity reject | false | false | 仅当 id 已 strict parse时保留 / — / — | — / — | — | — / — | — | — / — | — | `PXLC-EXECUTION-IDENTITY` / 1 |
| admission 可能发生但 owner record不足以分类 | false | — | exact input id / `uncertain` / PF（若 PXDQ 已验证） | 验证到哪一项就原样保留，否则 — | V 或 — | V 或 — / V 或 — | 已验证 PXDO 时 DR，否则 — | V 或 — / V 或 — | `uncertain` | `PXLC-DEPLOY-UNCERTAIN` / 1 |

D0b 两条命令的 `mode` 始终是 string `"local"`、`current_health_checked` 始终是 boolean `false`。terminal DR 对 `R|F|U|S` 都非 null；失败状态的 `V` 只允许来自 terminal PXDM/PXDO 的 nonzero verified prefix，不能从文件存在、进程状态或更后阶段的猜测补值。`superseded` 在 Artifact F0 fresh-only 路线不能由本批主动制造；它仅为后续 R0 的显式 successor 保留 strict read兼容性，F0 遇到任何既有 desired仍必须 pre-effect replace-required。

Artifact F0 自有 diagnostic code、exact public-safe message 与 exit taxonomy 固定如下；除“严格 config decoder code”外不得透传底层 errno、path、owner identity、frame bytes 或自由文本：

| code | exact `message` | exit |
| --- | --- | --- |
| `PXLC-ARTIFACT-GRAMMAR` | `artifact command grammar is invalid` | 2 |
| `PXLC-ARTIFACT-PATH` | `artifact path is invalid or unsafe` | 2 |
| `PXLC-ARTIFACT-PROFILE` | `artifact profile is unsupported` | 2 |
| `PXLC-ARTIFACT-COMPATIBILITY` | `artifact bytes are invalid or incompatible` | 2 |
| `PXLC-ARTIFACT-CONFLICT` | `artifact operation conflicts with its durable request` | 2 |
| `PXLC-ARTIFACT-CAPACITY` | `artifact store capacity is exhausted` | 2 |
| `A0_APPLICATION_ADMISSION_REQUIRED` | `application admission is required before this operation` | 2 |
| `PXLC-ARTIFACT-NOT-FOUND` | `artifact operation was not found` | 1 |
| `PXLC-ARTIFACT-MATERIALIZATION-FAILED` | `artifact materialization failed` | 1 |
| `PXLC-ARTIFACT-UNCERTAIN` | `artifact operation outcome is uncertain` | 1 |
| `PXLC-ARTIFACT-OWNER` | `artifact store owner state failed strict validation` | 1 |
| `PXLC-ARTIFACT-IO` | `artifact operation could not complete` | 1 |
| `PXLC-ARTIFACT-JSON-OUTPUT` | `artifact JSON output could not be written` | 1 |
| `PXLC-DEPLOY-EXTERNAL-GRAMMAR` | `external deploy requires the exact artifact and operation arguments` | 2 |
| `PXLC-DEPLOYMENT-QUERY-GRAMMAR` | `deployment operation query requires the exact config and operation arguments` | 2 |
| `PXLC-ARG-NON-UTF8` | `arguments must be valid UTF-8` | 2 |
| `PXLC-PLATFORM-UNSUPPORTED` | `DeveloperLocal modes require the Unix DeveloperLocal platform` | 2 |
| `PXLC-CONFIG-PATH-INVALID` | `config path must name an absolute lexically canonical regular file` | 2 |
| `PXLC-CONFIG-FILE-READ` | `config file could not be read` | 2 |
| `PXLC-CONFIG-FILE-TOO-LARGE` | `config file exceeds the 64 KiB limit` | 2 |
| `PXLC-CONFIG-DOCUMENT-INVALID` | `config file is not a strict ParaEGOX TOML document` | 2 |
| `PXLC-CONFIG-SCHEMA-UNSUPPORTED` | `config schema_version is not supported` | 2 |
| `PXLC-STATE-ROOT-TOO-LONG` | `state root exceeds the bounded DeveloperLocal limit` | 2 |
| `PXLC-STATE-ROOT-INVALID` | `state root must be a non-root, lexically canonical absolute Unix path` | 2 |
| `PXLC-FABRIC-LISTEN-INVALID` | `Fabric listen must be canonical tcp/127.0.0.1:PORT with PORT in 1..=65535` | 2 |
| `PXLC-MODEL-MISSING` | `the selected provisioned chat profile requires model in config` | 2 |
| `PXLC-CONFIG-PROVIDER-INVALID` | `config model fields do not match the selected provider` | 2 |
| `PXLC-MODEL-INVALID` | `the model identifier is invalid for the selected provisioned chat profile` | 2 |
| `PXLC-CONFIG-PROVIDER-UNKNOWN` | `config selects an unsupported model provider` | 2 |
| `PXLC-LIFECYCLE-CONFIGURATION` | `managed-local lifecycle configuration authority changed` | 2 |
| `PXLC-EXECUTION-IDENTITY` | `DeveloperLocal commands require a non-root user and group` | 1 |
| `PXLC-DEPLOY-ARTIFACT` | `deployment artifact reference is invalid or incompatible` | 2 |
| `PXLC-DEPLOY-MATERIALIZATION-RECEIPT` | `materialization receipt is invalid or not ready` | 2 |
| `PXLC-DEPLOY-CONFLICT` | `deployment operation conflicts with its durable request` | 2 |
| `PXLC-DEPLOY-REPLACE-REQUIRED` | `existing deployment state requires explicit replacement` | 2 |
| `PXLC-DEPLOY-CAPACITY` | `deployment owner capacity is exhausted` | 2 |
| `PXLC-DEPLOY-NOT-FOUND` | `deployment operation was not found` | 1 |
| `PXLC-DEPLOY-FAILED` | `deployment operation failed` | 1 |
| `PXLC-DEPLOY-UNCERTAIN` | `deployment operation outcome is uncertain` | 1 |
| `PXLC-DEPLOY-SUPERSEDED` | `deployment operation was superseded` | 1 |
| `PXLC-DEPLOY-OWNER` | `deployment owner could not complete the operation` | 1 |
| `PXLC-DEPLOY-JSON-OUTPUT` | `deployment JSON output could not be written` | 1 |

每个 failure envelope 的 `diagnostics` 恰一项且该 object 的 key 顺序严格为 `code,message`；所有 success/non-error progress 精确为 `[]`。同一 invocation 同时可观察多个 failure时，以下六行是逐 code、从左到右的完整 total order，不允许按实现层级、最后错误或自由文本改序：

| exact command | diagnostic total order（左侧优先） |
| --- | --- |
| `artifact build` | `PXLC-ARTIFACT-GRAMMAR` → `PXLC-PLATFORM-UNSUPPORTED` → `PXLC-EXECUTION-IDENTITY` → `PXLC-ARTIFACT-PATH` → `PXLC-ARTIFACT-PROFILE` → `PXLC-ARTIFACT-COMPATIBILITY` → `A0_APPLICATION_ADMISSION_REQUIRED` → `PXLC-ARTIFACT-UNCERTAIN` → `PXLC-ARTIFACT-IO` → `PXLC-ARTIFACT-JSON-OUTPUT` |
| `artifact inspect` | `PXLC-ARTIFACT-GRAMMAR` → `PXLC-PLATFORM-UNSUPPORTED` → `PXLC-EXECUTION-IDENTITY` → `PXLC-ARTIFACT-PATH` → `PXLC-ARTIFACT-COMPATIBILITY` → `PXLC-ARTIFACT-IO` → `PXLC-ARTIFACT-JSON-OUTPUT` |
| `artifact materialize` | `PXLC-ARTIFACT-GRAMMAR` → `PXLC-ARG-NON-UTF8` → `PXLC-PLATFORM-UNSUPPORTED` → `PXLC-EXECUTION-IDENTITY` → `PXLC-CONFIG-PATH-INVALID` → `PXLC-CONFIG-FILE-READ` → `PXLC-CONFIG-FILE-TOO-LARGE` → `PXLC-CONFIG-DOCUMENT-INVALID` → `PXLC-CONFIG-SCHEMA-UNSUPPORTED` → `PXLC-STATE-ROOT-TOO-LONG` → `PXLC-STATE-ROOT-INVALID` → `PXLC-FABRIC-LISTEN-INVALID` → `PXLC-MODEL-MISSING` → `PXLC-CONFIG-PROVIDER-INVALID` → `PXLC-MODEL-INVALID` → `PXLC-CONFIG-PROVIDER-UNKNOWN` → `PXLC-LIFECYCLE-CONFIGURATION` → `PXLC-ARTIFACT-PATH` → `PXLC-ARTIFACT-PROFILE` → `PXLC-ARTIFACT-COMPATIBILITY` → `A0_APPLICATION_ADMISSION_REQUIRED` → `PXLC-ARTIFACT-CONFLICT` → `PXLC-ARTIFACT-CAPACITY` → `PXLC-ARTIFACT-MATERIALIZATION-FAILED` → `PXLC-ARTIFACT-UNCERTAIN` → `PXLC-ARTIFACT-OWNER` → `PXLC-ARTIFACT-IO` → `PXLC-ARTIFACT-JSON-OUTPUT` |
| `artifact materialization query` | `PXLC-ARTIFACT-GRAMMAR` → `PXLC-ARG-NON-UTF8` → `PXLC-PLATFORM-UNSUPPORTED` → `PXLC-EXECUTION-IDENTITY` → `PXLC-CONFIG-PATH-INVALID` → `PXLC-CONFIG-FILE-READ` → `PXLC-CONFIG-FILE-TOO-LARGE` → `PXLC-CONFIG-DOCUMENT-INVALID` → `PXLC-CONFIG-SCHEMA-UNSUPPORTED` → `PXLC-STATE-ROOT-TOO-LONG` → `PXLC-STATE-ROOT-INVALID` → `PXLC-FABRIC-LISTEN-INVALID` → `PXLC-MODEL-MISSING` → `PXLC-CONFIG-PROVIDER-INVALID` → `PXLC-MODEL-INVALID` → `PXLC-CONFIG-PROVIDER-UNKNOWN` → `PXLC-LIFECYCLE-CONFIGURATION` → `PXLC-ARTIFACT-NOT-FOUND` → `PXLC-ARTIFACT-MATERIALIZATION-FAILED` → `PXLC-ARTIFACT-UNCERTAIN` → `PXLC-ARTIFACT-OWNER` → `PXLC-ARTIFACT-IO` → `PXLC-ARTIFACT-JSON-OUTPUT` |
| D0b `deploy` | `PXLC-DEPLOY-EXTERNAL-GRAMMAR` → `PXLC-ARG-NON-UTF8` → `PXLC-PLATFORM-UNSUPPORTED` → `PXLC-EXECUTION-IDENTITY` → `PXLC-CONFIG-PATH-INVALID` → `PXLC-CONFIG-FILE-READ` → `PXLC-CONFIG-FILE-TOO-LARGE` → `PXLC-CONFIG-DOCUMENT-INVALID` → `PXLC-CONFIG-SCHEMA-UNSUPPORTED` → `PXLC-STATE-ROOT-TOO-LONG` → `PXLC-STATE-ROOT-INVALID` → `PXLC-FABRIC-LISTEN-INVALID` → `PXLC-MODEL-MISSING` → `PXLC-CONFIG-PROVIDER-INVALID` → `PXLC-MODEL-INVALID` → `PXLC-CONFIG-PROVIDER-UNKNOWN` → `PXLC-LIFECYCLE-CONFIGURATION` → `PXLC-DEPLOY-ARTIFACT` → `PXLC-DEPLOY-MATERIALIZATION-RECEIPT` → `A0_APPLICATION_ADMISSION_REQUIRED` → `PXLC-DEPLOY-CONFLICT` → `PXLC-DEPLOY-REPLACE-REQUIRED` → `PXLC-DEPLOY-CAPACITY` → `PXLC-DEPLOY-FAILED` → `PXLC-DEPLOY-UNCERTAIN` → `PXLC-DEPLOY-SUPERSEDED` → `PXLC-DEPLOY-OWNER` → `PXLC-DEPLOY-JSON-OUTPUT` |
| `deployment operation query` | `PXLC-DEPLOYMENT-QUERY-GRAMMAR` → `PXLC-ARG-NON-UTF8` → `PXLC-PLATFORM-UNSUPPORTED` → `PXLC-EXECUTION-IDENTITY` → `PXLC-CONFIG-PATH-INVALID` → `PXLC-CONFIG-FILE-READ` → `PXLC-CONFIG-FILE-TOO-LARGE` → `PXLC-CONFIG-DOCUMENT-INVALID` → `PXLC-CONFIG-SCHEMA-UNSUPPORTED` → `PXLC-STATE-ROOT-TOO-LONG` → `PXLC-STATE-ROOT-INVALID` → `PXLC-FABRIC-LISTEN-INVALID` → `PXLC-MODEL-MISSING` → `PXLC-CONFIG-PROVIDER-INVALID` → `PXLC-MODEL-INVALID` → `PXLC-CONFIG-PROVIDER-UNKNOWN` → `PXLC-LIFECYCLE-CONFIGURATION` → `PXLC-DEPLOY-NOT-FOUND` → `PXLC-DEPLOY-FAILED` → `PXLC-DEPLOY-UNCERTAIN` → `PXLC-DEPLOY-SUPERSEDED` → `PXLC-DEPLOY-OWNER` → `PXLC-DEPLOY-JSON-OUTPUT` |

该 total order 也冻结 parser 的 code collapse：任何缺少、额外、换序、重复或未知 option，以及固定 ASCII command/option/profile token不匹配，都只产生对应命令行首项 grammar code，不再泄漏通用 `MODE-*`、`OPTION-*` 或 `CONFIG-PATH-MISSING` code。build/inspect 的 non-UTF-8 `<ABS>` value 精确归为 `PXLC-ARTIFACT-PATH`，其他 non-UTF-8 value归为其 grammar；其余四条命令在 exact token shape 已成立后遇到任一 non-UTF-8 value，精确归为 `PXLC-ARG-NON-UTF8`。因此表中没有未排序的 generic parser分支。

上述顺序同时冻结ArtifactStore config-chain分类：materialize/query在strict读取PXAZ authority后发现本次current config与其immutable config不等，必须在任何operation/pair读取或snapshot推进前命中表内`PXLC-LIFECYCLE-CONFIGURATION`；D0b deploy先证明current、PXAZ与PXDQ current authority一致，再逐项关联PXDQ与Receipt链PXAQ，后者不等只能在表内`PXLC-DEPLOY-MATERIALIZATION-RECEIPT`失败。测试必须分别提供current-vs-root与PXDQ-vs-receipt mutation，证明前者不会落到artifact owner/NotFound、后者不会落到lifecycle configuration或deploy artifact，并证明两者并存时左侧的lifecycle configuration稳定优先。

两条 query 在完成 grammar/identity/config-authority validation后只读既有 owner record：明确不执行 A0、profile/ref compatibility、capacity、conflict、replace或任何其他 mutation preflight，也不创建 recovery operation。一个 exact readback 的 terminal `failed|uncertain|superseded` 只能压过它右侧后来发生的 owner transport/I/O/JSON-output failure；它绝不能压过本次 query 左侧的 grammar、platform、identity、config或lifecycle-authority validation，也不能把 malformed request变成历史 terminal响应。mutating command同样先完成自己左侧全部 request/admission validation，再允许已有 exact terminal压过其右侧 delivery failure。

共同 JSON/exit 语义固定为：

- `schema_version` 是 JSON number `1`；`command` 分别为 `"artifact.build"`、`"artifact.inspect"`、`"artifact.materialize"`、`"artifact.materialization.query"`、`"deploy"`、`"deployment.operation.query"`，D0b 的 `mode = "local"`。build/inspect 成功逐字公开上述 profile、runtime kind、adapter ABI 与 target profile；`payload_length` 是 1..64 的 JSON number，`entrypoint` 只在 canonical manifest 内。
- inspect 和两条 query 的 `changed` 始终为 `false`。mutating command 的 `changed = true` 只表示本次调用可证明新建或推进了其 owner state，byte-identical build/replay/already-terminal 为 `false`；一旦已经可能接受 mutation 而 exact effect 无法证明，必须为 JSON null。`changed`、文件存在、exit 0 与 transport ACK 都不升级 owner state。
- 同一 operation id + 同一 canonical request 只查询、返回或推进同一 durable operation，也可重放该 operation 自己已提交的 exact desired；同一 id + 不同 request/ref 必须在 owner effect 前 conflict。任何不同 operation id，或该 D0b operation 首次 admission 前已有不属于它的 Deployment desired head（包括 D0a），都必须在 Controller/Runtime/lifecycle effect 前返回 `PXLC-DEPLOY-REPLACE-REQUIRED`；R0 才拥有 replacement。
- mutating command 和 query 对 `admitted|materializing|materialized|already_materialized|committed|applying|active_ready` 的有效 owner-correlated响应使用 `ok = true`、exit 0；`failed|uncertain|superseded` 使用 `ok = false`、exit 1 并保留已验证的 operation/ref/Receipt/terminal 字段，不得将其清空后伪装成“未发生”。query 找不到 operation、owner/receipt/correlation/timeout/I/O/output failure同样 exit 1；只有已识别 namespace 的 grammar、path/config/ref/profile/compatibility 或 pre-effect conflict 为 exit 2。stdout 可写时严格为一个 compact JSON object 加一个 LF，stderr 为空；不能完整交付 JSON 时仍 exit 1。
- 输入或查询失败且没有 durable operation identity 时，nullable state/ref/Receipt/result 字段按上述逐命令矩阵为 null；strict parse成功的 query `operation_id` 只是回显 request identity，不声称 durable admission。query 只读对应owner的canonical snapshot，绝不因 NotFound、output-loss recovery或 readback创建/推进 operation。成功 `active_ready` 只是关联 Controller committed revision 与 Runtime PXMT terminal 的 point-in-time deployment outcome，不是 Inspection health。
- D0b 的 `generation`、`operation_id` 与 digest 分别严格为 32/32/64 lower-case hex string；revision/sequence 使用无前导零 canonical decimal string而非 JSON number。所有非 null object/Receipt ref 都必须 canonical reparse/re-encode且和请求、operation、PlanContent/Slice、Controller revision、Runtime apply/PXMT terminal 逐项关联；CLI 不签发总 Receipt。

Owner 与 fresh-only seam 冻结为：

- build/inspect contract producer 是 canonical manifest/payload 与 strict decoder 的唯一权威；build 可重复产生 byte-identical pair，inspect 必须严格 reparse/re-encode、重算 digest/compatibility 且零 mutation。ArtifactStore 是 exact manifest/payload pair与唯一PXAZ/PXAY snapshot中materialization admission/progress/terminal/Receipt的唯一写者；snapshot以外没有journal/counter/Receipt record，它也不选择desired object、不创建Runtime generation，没有GC/active pointer。
- DeploymentController 只消费 strict manifest projection、完整 object ref 与关联 materialization Receipt，独占 desired ref、artifact-bound PlanContent v2、PXTE11/PXAR12 target Slice、DeploymentRevision、deployment operation 与 Deployment Receipt；完整 pair、profile/ABI/target/entrypoint commitment 必须经 exact ArtifactExecutionBinding 同时进入 PlanContentDigestV2、SourcePlanDigest 与 Slice digest。它不读取 payload、不扫描 ArtifactStore、不接受 mutable path。
- RuntimeHost 只能经 Runtime-owned read-only Artifact access port exact reopen terminal pair，在任何 Model/Agent 或其他 workload effect 前复验 canonical bytes、pair、Slice、profile/ABI/target/entrypoint、operation/revision/fence；它独占 live generation 与 PXMT terminal Receipt，不得发布 pair、写 ArtifactStore 或签发第二份 materialization/deployment Receipt。
- D0b 只允许独立 fresh workspace：该 workspace 可先完成 A1 materialization，但不得已有 lifecycle generation、Deployment desired head 或 deployment operation；D0b 必须是该 workspace 的第一次 lifecycle/deployment mutation，直接启动唯一 supervisor 并走 external-artifact Controller/Runtime path，不能先调用 D0a `run_up()`、不能复用 workspace A 的 D0a state，也不能以 compiled-in fallback完成。workspace A 继续保留 D0a/M3/M5；workspace B 才执行 materialize→D0b。replace/restart 只由后续 R0 授权。
- CLI/TUI、lifecycle、ArtifactStore、Controller 与 Runtime 都不得成为另一 owner 的第二 writer；任何 failure、timeout、output loss 或 missing evidence 必须保留 `Failed`/`Uncertain` 并用原 operation id query，不透明生成新 id或 retry。A1/D0b 当前均只有 `Contract frozen`：未实现、未验证、未登记或发布。Artifact crate/store与前四条 CLI 在内部实现序列中也不得独立取得 governance/public authority、进入 main branch 或声称可用；只有六条命令、真实 Artifact producer、D0b Controller/Runtime/TUI consumer、两个 system harness 与 `governance.toml` 在同一 immutable candidate ref 出现并共同过 admission/review/merge gate后，整条 Artifact F0 surface才能发布。
- profile literals/limits、PXAM/PXAK/PXAQ/PXAA/PXMU/PXAV/PXAW/PXAX、PXAZ/PXAY/PXOP与PXDQ/PXDK/PXDM/PXDO bytes、全部digest domains、两个text refs、root/temp/build filenames、六条grammar、JSON key order/type/null/preserve matrix、diagnostic/exit taxonomy、operation replay/conflict、snapshot/lock/recovery与owner/fresh seam共同构成Artifact F0 v1兼容单元；decoder不做version negotiation、content sniffing、unknown-field容忍或旧格式fallback。任何不兼容变化必须先回到Program并使用显式successor version/profile/command/schema、offline successor root与新goldens，不得静默改写v1。

### M3a — 本地 Inspection snapshot

首个且仅有的 M3a exact public grammar 冻结为：

```text
paraegox inspection snapshot --config <absolute-paraegox.toml> --json
```

参数顺序固定，不接受默认配置、相对路径、`--bootstrap`、socket/token/state-root、cursor、retry、watch 或额外参数。它复用现有严格 DeveloperLocal chat schema v1 与 config commitment，只读取当前 Running generation 的 owner-private Inspection projection；不解析 Secret value，不启动、停止、恢复或重启 owner，不创建 lifecycle/domain state，也不写 desired state。

M3a JSON v1 的 top-level 字段严格且仅有：

```text
schema_version
command
ok
changed
snapshot
diagnostics
```

- `schema_version` 固定为 JSON number `1`，`command` 固定为 `"inspection.snapshot"`，`changed` 始终为 `false`。成功为 `ok = true`、`diagnostics = []`；`snapshot` 恰含 `snapshot_version`、`projection_id`、`observation_clock_ref`、`projection_revision`、`projected_at_nanos`、`overall`、`projection_digest`、`sources` 和 `node`，其中 `snapshot_version = 2`。除这两个 schema/version 字段外，PXIS v2 的全部 `u64` revision、epoch、sequence 与 nanoseconds 都编码为无前导零的 canonical 十进制 JSON string，避免常见自动化在 `2^53 - 1` 以上丢失身份或 cursor 精度。
- `sources` 是固定 Authority、DeploymentController、RuntimeHost、FabricService、AgentService 顺序的五项数组。每项严格含 `owner`、`freshness`、`subject_ref`、`coordinate`、`observed_at_nanos`、`valid_until_nanos`、`liveness`、`readiness`、`health`、`feature_support`、`reason`、`owner_fact_digest`；`coordinate` 只能为 null，或严格的 `{kind: authority_tenure, tenure_epoch, fact_sequence}`、`{kind: deployment_revision, revision, fact_sequence}`、`{kind: runtime_host_epoch, runtime_host_epoch, snapshot_sequence}`、`{kind: fabric_service_generation, service_generation, observation_sequence}`、`{kind: agent_service_generation, service_generation, observation_sequence}` 之一，不能压成一个可互换 revision。
- `node` 严格含 `freshness`、`node_ref`、`node_incarnation_ref`、`registration_epoch`、`status_sequence`、`observed_at_nanos`、`valid_until_nanos`、`liveness`、`readiness`、`health`、`feature_support`、`reason`、`node_status_digest`。16-byte identity/ref 与 32-byte digest 分别编码为 32/64 字符 lower-case hex；所有 enum 严格采用现有 PXIS v2 variant 的 lower-case snake-case 名称；全部 PXIS `u64` 使用上述 canonical 十进制 string，缺失 option 字段保留为 JSON null，不接受 wall-clock 替换 owner-local nanoseconds。
- `Fresh`/`Stale`/`Partitioned`/`Missing`、各维度 `Unknown` 与 conservative `overall` 都是 Inspection owner 已投影的事实。CLI 不按进程存在、连接成功或本机 wall clock 重算 freshness，不把 stale/unknown 变成命令失败或健康；完整且严格解码的 stale/unknown snapshot 仍为 exit 0。
- 错误时 `snapshot = null` 且 `diagnostics` 恰有一项，只有稳定 `code` 与 public-safe `message`。已识别 inspection namespace 的 grammar、absolute/config 安全或 config-authority drift 为 exit 2；locator/bootstrap/socket/peer/token、PXIQ/PXIP/PXIS correlation/protocol、NotFound、timeout/I/O 与输出失败为 exit 1；成功仅为 exit 0。stdout 可写时严格为一个 compact JSON object 加一个 LF，stderr 为空。
- 输出不得包含 config/state/bootstrap/socket 路径、uid/gid、PID/PGID、capability/token、Secret/SecretRef、credential、seed、private/signing key、endpoint/route 或未审计底层错误。M3a 不声明当前健康、Deployment 收敛、Agent 可对话、Evidence、history、stream/watch、retry/reconnect、OpsService、federation、Remote Agent 或生产支持。

### M4a — 当前 Running generation 的 Receipt snapshot

首个且仅有的 M4a exact public grammar 冻结为：

```text
paraegox receipt snapshot --config <absolute-paraegox.toml> --json
```

参数顺序固定，不接受默认/相对配置、`--bootstrap`、socket/token/state-root/store/evidence path、generation、cursor/list/since/follow/watch、retry/reconnect、raw/export、provider/model/Secret 或额外参数。它复用现有严格 DeveloperLocal chat schema v1 与 config commitment，只读取 lifecycle 已分类为 `Running` 且 `owner_readiness_observed = true` 的当前 generation；never-started、starting/stopping/stopped、failed/unknown 或 config drift 都失败关闭。它不解析 Secret value，不隐式调用 `up`、`deploy`、`down`、restart、orphan recovery 或 reconcile；已 Running 的 provisioned profile 在移除启动时 Secret 环境后仍必须可读。

CLI 只执行一次同配置 lifecycle `Status`，得到 expected generation 后只再执行一次 Receipt locator 和一次 typed `Latest`，三段都不 retry、reconnect 或 fallback。Receipt locator 复用现有 exact 54-byte `PXLO` v1 request shape：`0..4 = PXLO`、`4 = version 1`、`5 = action R`、`6..38 = config commitment`、`38..54 = expected generation`，之后必须 exact EOF。lifecycle 仅在记录仍为同一 config/current Running/ready generation、未 stopping，且 Receipt adapter bootstrap 已在 Ready event 前完成安全 pin 时返回 `PXRL` v1；否则不返回旧 generation 或 cached success。

`PXRL` v1 是 lifecycle 到 CLI 的 owner-locator，不是 Receipt 响应；所有整数为 big-endian，header 固定 160 bytes，path 为 1..4096 bytes，frame 最大 4256 bytes：

```text
0..4      magic = PXRL
4..6      u16 version = 1
6         action = R
7         outcome = R
8..10     u16 header_len = 160
10..12    reserved = 0
12..16    u32 frame_len
16..20    u32 bootstrap_path_len
20..24    u32 bootstrap_content_len
24..40    lifecycle generation[16]
40..72    config commitment[32]
72..104   SHA-256(bootstrap content)
104..112  u64 bootstrap device
112..120  u64 bootstrap inode
120..128  reserved = 0
128..160  SHA-256 locator frame digest
160..     bootstrap path bytes
```

locator digest 严格为 `SHA-256("paraegox.local.receipt-locator-response.v1" || frame[0..128] || path)`。decoder 必须拒绝错误 magic/version/action/outcome/header/frame/path/content length、reserved、zero generation/config/content/frame digest、UTF-8/NUL/relative/非 lexical-canonical path、expected generation/config mismatch、trailing/oversize 和非 canonical re-encode。locator 不携带 bootstrap bytes、capability token、PXMT bytes 或 public JSON 字段；lifecycle 不打开 Receipt endpoint、不验证 PXMT、不返回 Receipt projection。

locator 所 pin 的 owner-private `PXRB` v1 bootstrap 也使用 big-endian canonical frame，header 固定 320 bytes，socket path 为 1..512 bytes，因此 `bootstrap_content_len` 必须为 321..832：

```text
0..4      magic = PXRB
4..6      u16 version = 1
6..8      u16 header_len = 320
8..12     u32 frame_len
12..16    u32 socket_path_len
16..32    lifecycle generation[16]
32..64    config commitment[32]
64..96    generation token[32]
96..100   u32 server uid
100..104  u32 server gid
104..112  u64 operation timeout nanos = 5000000000
112..128  request-id seed[16]
128..144  RuntimeHost target[16]
144..176  Runtime store instance[32]
176..192  Runtime response key ref[16]
192..224  Runtime response Ed25519 public key[32]
224..256  expected PXMT request digest[32]
256..288  expected PXMT Receipt digest[32]
288..320  SHA-256 bootstrap digest
320..     socket path bytes
```

bootstrap digest 严格为 `SHA-256("paraegox.local.receipt-bootstrap.v1" || frame[0..288] || socket_path)`。decoder 必须拒绝 zero identity/token/seed/key/digest、root/uid-gid mismatch、错误 fixed timeout、长度/path/canonical re-encode，且 PXRB 中的 generation/config/request/receipt 必须分别与 PXRL 和本次取回的 PXMT 精确一致。

owner-private Receipt adapter 的版本化 bootstrap 必须精确绑定 lifecycle generation、config commitment、socket path/identity、32-byte generation token、server uid/gid、固定 5-second operation timeout、request-id seed、RuntimeHost target、Runtime store instance、Runtime response key ref/public key、expected PXMT request digest 与 expected Receipt digest；私有 token/seed 使用 zeroizing 存储且 Debug 脱敏。CLI 对 bootstrap 的整条现存 path chain 执行 no-symlink/owner/private-mode 检查，以 `O_NOFOLLOW|O_CLOEXEC` 在同一 fd 上交叉验证 regular、uid/gid、mode 0600、link-count 1、device/inode/content length/SHA-256 与 canonical bootstrap digest。

本 one-shot client 只生成 sequence `1`，其 16-byte request id 精确为 `first16(SHA-256("paraegox.local.receipt-request-id.v1" || request-id seed || u64_be(1)))`；结果全零即失败，不能改用随机值、wall clock、PID 或第二个 sequence。CLI 只向 mode-0600 的 same-uid/gid owner socket 发一次 token-authenticated `Latest`：request transport 恰为 208 bytes，即 bootstrap 的 raw generation token 32 bytes，随后是不带 outer length prefix 的 fixed 176-byte `PXRQ` v1。`PXRQ` 所有整数使用 big-endian，布局严格为：

```text
0..4      magic = PXRQ
4..6      u16 version = 1
6         action = L
7         reserved = 0
8..10     u16 header_len = 176
10..12    reserved = 0
12..16    u32 frame_len = 176
16..32    request_id[16]
32..48    lifecycle generation[16]
48..80    config commitment[32]
80..112   expected PXMT request digest[32]
112..144  expected PXMT Receipt digest[32]
144..176  SHA-256 request frame digest
```

request frame digest 严格为 `SHA-256("paraegox.local.receipt-latest-request.v1" || frame[0..144])`。server 必须验证 exact 208-byte transport 与 EOF、same uid/gid peer、constant-time token、全部 nonzero correlation、reserved/header/frame/action/version、digest、canonical re-encode，以及 request id、generation、config、两个 expected PXMT digest 与本 PXRB/immutable slot 全部一致；token 不进入 PXRQ frame/digest或任何响应。wrong peer、token、frame、request id、generation、config 或 digest correlation 一律直接关闭且不发送任何 response bytes，不能返回可区分的认证 oracle。

只有上述认证与 correlation 全部成功后，server 才返回一个 outer `u32_be frame_len`，随后是一份 `PXRO` v1 frame 并 write-half close；客户端必须继续读取 exact EOF。`PXRO` header 固定 224 bytes，payload 为 0..2048 bytes，frame 为 224..2272 bytes，含 4-byte outer length 的 response transport 为 228..2276 bytes。布局严格为：

```text
0..4      magic = PXRO
4..6      u16 version = 1
6         action = L
7         outcome = R or N
8..10     u16 header_len = 224
10..12    reserved = 0
12..16    u32 frame_len
16..20    u32 payload_len
20..24    reserved = 0
24..40    request_id[16]
40..56    lifecycle generation[16]
56..88    config commitment[32]
88..120   expected PXMT request digest[32]
120..152  expected PXMT Receipt digest[32]
152..184  SHA-256 payload digest or all-zero
184..192  reserved = 0
192..224  SHA-256 response frame digest
224..     canonical PXMT payload
```

response frame digest 严格为 `SHA-256("paraegox.local.receipt-latest-response.v1" || frame[0..192] || payload)`。ASCII outcome `R` 要求 payload 1..2048 bytes、payload digest 精确为 `SHA-256(payload)`、outer length/`frame_len = 224 + payload_len`，且 payload 是与两个 expected digest、Runtime target/store/key 和 Ed25519 transcript 相符的 exact canonical PXMT v1。ASCII outcome `N` 只允许 header-only：outer length 与 `frame_len = 224`、`payload_len = 0`、payload digest 全零，其余 correlation 字段仍逐 byte 回显请求；它仅表示一个已通过 peer/token/canonical/correlation 验证的请求在竞态中遇到已进入 retiring、不可再供应 bytes 的同 generation immutable slot，client 映射为 `PXLC-RECEIPT-NOT-FOUND`。never-started、旧 generation、错误 expected digest、认证失败或任意 malformed input 都不得得到 `N`，也不得借 `N` 返回 cached Receipt。

PXRB 中的 5-second timeout 是一次不可续期的绝对 operation deadline：client 从 connect 前开始，覆盖 peer 验证、完整 208-byte write、write-half close、outer length、PXRO frame 与 EOF；server 对每个 accepted exchange 从读取 request 到 response write-half close也只有同一 5-second deadline，partial progress 不能重置。endpoint 同时最多处理 8 个 exchange，不建立无界等待队列；超出容量的连接直接关闭且不响应。每次 public command 只允许一次 connect/request/response，任何 partial/trailing/oversize、length-prefix mismatch、timeout、EOF、`N` 或 endpoint replacement 都失败，自动 retry/reconnect/fallback budget 精确为 0。

PXRB v1 精确绑定 PXRQ/PXRO v1；PXRL、PXRB、PXRQ、PXRO 的 magic/version/offset/length/reserved、四个 frame digest domain 与一个 request-id domain、raw token placement、request fixed-length 与 response outer-length framing、`R`/`N` 语义共同构成一个兼容单元。decoder 必须 strict decode 后 canonical re-encode 并逐 byte 相等，不做 version negotiation、content sniffing、旧格式 fallback 或宽松保留字段；任何不兼容变化必须使用显式 successor version 与新 golden，不得静默改写 v1。

adapter 在发布 Ready 之前必须从现有真实 activation outcome 中取得 exact canonical PXMT，独立 strict decode/re-encode，核对 request/receipt digest、Runtime target/store/key，并用该 Runtime response public key 严格验证 Ed25519 signing transcript。CLI typed client 取回同一 bytes 后必须重做这些验证，再投影 public-safe allowlist；server 或 client 都不能用 D0a 摘要、Inspection 投影、进程存在、endpoint 可达或日志文本代替 PXMT 签名/关联。adapter 只保有该 RunningStack 生命期内的不可变 bytes，down/join 时与 owner graph 一起关闭并安全清理 endpoint/bootstrap；它不是 durable Receipt/Evidence store 或第二 writer。

M4a JSON v1 的 top-level 字段严格且仅有，并按下列顺序序列化：

```text
schema_version
command
ok
changed
generation
snapshot
diagnostics
```

- `schema_version` 固定为 JSON number `1`，`command = "receipt.snapshot"`，`changed` 始终为 `false`。成功时 `ok = true`、`generation` 是当前 16-byte lifecycle identity 的精确 32 字符 lower-case hex、`diagnostics = []`；`ok` 只表示本次 locator/exchange/PXMT 验证成功，不是当前 health、Deployment 收敛或一个可泛化的终态成功推断。
- 成功的 `snapshot` 字段严格且仅有，并按下列顺序序列化：

```text
snapshot_version
query_scope
source_owner
record_kind
receipt_version
request_digest
receipt_digest
request_mode
terminal_outcome
lifecycle_effect
desired_head
desired_head_digest
fabric_generation
model_generation
agent_generation
physical_binding_census
census_complete
fabric_ready
model_ready
agent_ready
fabric_to_agent_dependency_ready
model_to_agent_dependency_ready
exact_zero
quarantined
resource_census_digest
raw_outcome_digest
completion_runtime_host_epoch
completion_snapshot_sequence
selection_clock_generation
selection_observed_at_nanos
current_health_checked
```

- `snapshot_version` 与 `receipt_version` 都是 JSON number `1`；`query_scope = "current_running_generation"`、`source_owner = "runtime_host"`、`record_kind = "managed_model_agent_stack_terminal_receipt"`。`request_digest`、`receipt_digest`、`desired_head_digest`、`resource_census_digest` 与 `raw_outcome_digest` 都是非 null、精确 64 字符 lower-case hex；不输出 raw receipt、signature、signing transcript、Runtime key/channel 或未在此 allowlist 中的 PXMT 内部 identity。
- M4a v1 只接受当前真实 Running/ready producer 的 `request_mode = "fabric_model_and_agent"`、`terminal_outcome = "active_ready"`、`lifecycle_effect = "may_have_started"`、`desired_head = "committed_incoming"`；三个 generation 全部非 null、`physical_binding_census = 2`、census/readiness/dependency 全为 true、`exact_zero = false`、`quarantined = false`。即使其他 PXMT v1 terminal 可以被 contract decoder 识别，adapter/client 也必须以 `PXLC-RECEIPT-PROTOCOL` 拒绝，不能将其序列化成 M4a 成功或冒充 failed/stopped outcome 查询。
- `physical_binding_census` 是 JSON number，布尔字段都是 JSON boolean。三个 service generation 与 `completion_runtime_host_epoch`、`completion_snapshot_sequence`、`selection_clock_generation`、`selection_observed_at_nanos` 都是非 null、无前导零的 canonical 十进制 JSON string，不能使用 JSON number。`current_health_checked` 始终为 `false`：PXMT 是 point-in-time Runtime terminal，不替代 M3a Inspection。
- 错误时 `ok = false`、`generation = null`、`snapshot = null`、`changed = false`，`diagnostics` 恰有一项且仅含稳定 `code` 与 public-safe `message`。稳定 taxonomy 逐项固定为 `PXLC-RECEIPT-GRAMMAR`、严格 chat config decoder 已有的 ConfigError code、`PXLC-LIFECYCLE-CONFIGURATION`、`PXLC-EXECUTION-IDENTITY`、`PXLC-RECEIPT-NOT-RUNNING`、`PXLC-RECEIPT-LOCATOR`、`PXLC-RECEIPT-BOOTSTRAP`、`PXLC-RECEIPT-PEER`、`PXLC-RECEIPT-PROTOCOL`、`PXLC-RECEIPT-NOT-FOUND`、`PXLC-RECEIPT-IO`、`PXLC-RECEIPT-JSON-OUTPUT`。已识别 receipt namespace 的 grammar、absolute/config 安全或 config-authority drift 为 exit 2；execution identity、not-running、locator/bootstrap/peer/protocol、NotFound、I/O 为 exit 1，timeout 精确归入 `PXLC-RECEIPT-IO`，完整 JSON 无法交付精确归入 `PXLC-RECEIPT-JSON-OUTPUT`；成功仅为 exit 0。
- stdout 可写时严格为一个 compact JSON object 加一个 LF，stderr 为空；无法完整交付 JSON 的 stdout failure 仍以 exit 1 失败关闭。任何 envelope 都不得包含 config/state/bootstrap/socket/store path、uid/gid、PID/PGID、capability/token、Secret/SecretRef、credential/seed/private key、endpoint/route、raw PXMT/signature 或未审计底层错误。

### M5a — 附着已运行实例的 TUI

首个且仅有的 M5a exact public grammar 冻结为：

```text
paraegox tui --config <absolute-paraegox.toml>
```

- 参数顺序固定，不接受默认/相对配置、`--json`、bootstrap/socket/token/state-root、generation、cursor、watch/retry/reconnect、provider/model/Secret、`--start`、`--stop`、`--restart` 或额外参数。它复用严格 DeveloperLocal chat schema v1 与 config commitment，不读取 Secret value；只附着由 M2a lifecycle 已经分类为 `Running` 且 `owner_readiness_observed = true` 的同配置 generation。never-started、starting/stopping/stopped、failed/unknown 或 config drift 均在 Textual child 启动前失败，绝不隐式调用 `up`、`deploy`、`down`、restart、orphan recovery 或 reconcile。
- 客户端先做一次同配置只读 lifecycle Status 取得 expected generation，再且只再做一次 same-uid/gid private `TuiAttachQuery`。该 query 复用 exact 54-byte `PXLO` v1 request shape，以 action byte `T` 携带 config commitment 与 expected generation；不并行或顺序调用两个独立 locator，不 retry。supervisor 只在同一个 Ready event 前已经同步捕获 conversation PXAB 和 Inspection PXIB 的 verified pins、当前记录仍是同一 Running/ready generation 且未 stopping 时，才返回一个 bounded canonical response。
- lifecycle response `PXTL` 与 child handoff `PXTH` 使用不同 magic/digest domain但共享下列唯一 canonical wire layout；所有整数均为 big-endian，header固定288 bytes，两条 bootstrap-file path分别为1..4096 bytes，frame最大8480 bytes：

```text
0..4      magic = PXTL（lifecycle response）或 PXTH（child handoff）
4..6      u16 version = 1
6         action = T
7         outcome = R
8..10     u16 header_len = 288
10..12    u16 locator_count = 2
12..16    u32 frame_len
16..32    lifecycle generation[16]
32..64    config commitment[32]
64..160   conversation pin record（kind C）
160..256  Inspection pin record（kind I）
256..288  SHA-256 frame digest
288..     conversation path bytes，随后 Inspection path bytes
```

- 每个96-byte pin record相对其 base 的布局严格为：`+0 kind`（`C`/`I`）、`+1 reserved = 0`、`+2..4 u16 record_len = 96`、`+4..8 u32 path_len`、`+8..12 u32 content_len`、`+12..16 u32 uid`、`+16..20 u32 gid`、`+20..24 u32 mode`、`+24..32 u64 nlink`、`+32..40 u64 dev`、`+40..48 u64 ino`、`+48..80 SHA-256(content)`、`+80..96 reserved = 0`。conversation PXAB content length只能为144..656，Inspection PXIB只能为128..640；两者 uid/gid须为当前 non-root euid/egid、mode须为`0o600`、nlink须为1、content digest不得全零。generation/config不得全零；两条 path必须distinct、UTF-8、NUL-free、lexical-canonical absolute，并按 conversation后Inspection精确拼接。
- `PXTL` digest严格为 `SHA-256("paraegox.local.tui-attach-locator-response.v1" || frame[0..256] || both_path_payload)`；`PXTH` digest严格为 `SHA-256("paraegox.local.tui-attach-handoff.v1" || frame[0..256] || both_path_payload)`。decoder拒绝错误 magic/kind/order/count/version/action/outcome/header/record/frame/path/content length、reserved、trailing、oversize、uid/gid/mode/nlink、all-zero generation/config/content/frame digest与非 canonical path；strict decode后的 canonical re-encode必须逐 byte等于输入。PXTL/PXTH都不包含 PXAB/PXIB bytes或 raw token，也不进入公共输出或持久记录。
- Rust local parent 不读取 bootstrap、不构造 Agent/Inspection client、不持有 generation token，也不代理 `Open`/`Submit`/`Cancel`/`Latest`/`Watch`。lifecycle UDS只返回一个 raw PXTL frame到 EOF；parent用本请求的 expected generation/config严格解码 PXTL，再以同一字段 canonical重编码为 PXTH，Python只接受 PXTH。parent 创建一对 owner-private `UnixStream`，把 child endpoint精确 dup到 fd 3，并只以 hidden internal grammar `paraegox-console --tui-attach-fd 3` 启动同级 child；该 mode不接受两个既有 path options或其他参数。parent endpoint只写一个 PXTH frame后 half-close，child 必须校验 same-uid/gid peer、一个 frame、一个 reader与 exact EOF。handoff 不进入普通 argv/env value、日志或持久文件。attach child使用 `env_clear`；只从 parent复制 `PATH`、`TERM`、`COLORTERM`、`LANG`、`LC_ALL`、`NO_COLOR` 六个名字，其中 `PATH` 与 `TERM` 必须存在且非空，其他四个可缺失或为空。值必须是无 ASCII control/newline的 UTF-8，`PATH` 上限4096 bytes，其余每项上限128 bytes；不满足即在 child start前失败。再由既有 bundle script固定设置其自身 `PYTHONPATH`/`PYTHONNOUSERSITE`。不得复制其他环境变量，尤其是 API key、Secret、credential、proxy、preload或 canary。
- Python child 严格解码 handoff，先确认两个 locator 共享同一 nonzero expected generation/config commitment，再把两组 pins交给 ADR-0009 已准入的直接 typed clients。每个 client 都必须验证 lexical-canonical absolute path、整条现存 path chain无 symlink、parent uid/gid与 mode 0700/02750，并用 `O_NOFOLLOW|O_CLOEXEC` 打开对应 bootstrap；在同一个 descriptor 上验证 regular、uid/gid、mode 0600、link-count 1、dev/ino/content length/SHA-256 与 bootstrap canonical digest，从而保持“lifecycle generation/config → Ready-captured exact pin → 本次 exact bytes → canonical bootstrap/token/socket”的相关链，而不是猜测 PXAB/PXIB本身编码了 lifecycle generation/config。Agent endpoint继续做 private socket identity；Inspection endpoint还必须匹配当前 owner 的唯一 canonical `.pxi-<32-lowerhex>-socket.pin` hardlink，原 socket与 pin均为预期 uid/gid、mode 0600、link-count 2且 exact dev/ino一致。随后每次 IPC 都验证 same uid/gid peer、socket identity前后不变、token/request/correlation/digest/trailing/timeout；任一不匹配都整体失败且不 retry。Textual App只持 opaque typed-client interface与 typed results，App/widget字段不接收 path、token、Secret/SecretRef、owner socket或 raw Zenoh Session；typed client对其 mutable token buffers做 best-effort清零，但 CPython transient immutable copies不在强擦除声明内。这里的“Textual 不持 raw token”精确表示 presentation fields不持 raw bytes，而同一 Python process内的 ADR-0009 typed client仍负责认证 IPC。
- App 启动对 exact PXAB conversation scope 至多执行一次 typed `Open`；它可能由 AgentService 创建或重开该既定 Session，但不会创建 lifecycle/Runtime/Deployment owner。之后只允许现有 bounded one-pending-request conversation `Submit` 与用户显式 `/cancel`；Request/Turn identity、journal、terminal、cancel intent、replay/conflict与容量语义继续由 AgentService 独占。detach 不自动 cancel、seal、delete 或 replay 已接受请求；它关闭两个 typed clients后退出，Session、Runtime 与整个 Running generation 继续由 supervisor 持有。M5a 不新增 history/pending-request discovery：重新附着只做 `Open(EXISTING)` 并得到空的本地 transcript；先前已接受但尚未终结的 request继续由 AgentService处理，新 client既不猜测其 identity也不重放，新的显式 submit只能接受 owner返回的 capacity/conflict/terminal。
- Inspection 在 UI 可见前由 `DeveloperLocalInspectionClientV2` 严格执行一次 `Latest`。之后可以在 Python client coordinator 内按当前 `(PXIS/protocol version, projection_id, projection_revision)` 发 one-shot `Watch`：任意时刻至多一个 in-flight；相邻 Watch 开始时间固定至少间隔 1 秒；前一 response 已验证并交付/丢弃前不取下一项；`NotModified` 不合成新 revision，也不建立 queue。完整的新 snapshot 直接替换当前显示，即使跳过中间 revision也只表示“当前 cache”，不表示 history。现有 producer 只证明 r1 到 freshness stale r2；这个内部受限 loop 不冻结 M3b public watch grammar，不声称持续 refresh、心跳或当前健康。
- Agent 或 Inspection 在 UI 启动后的 EOF、timeout、retired generation、peer/protocol/correlation 或 lifecycle down 显示对应 public-safe `unavailable`，不能映射成 source `Partitioned`、成功、健康或 stopped。两个 typed protocol本来就是每个 operation 一个 bounded connection/exchange；前一成功或 `NotModified` 后按节流计划发下一次 Watch不算 reconnect，但任一 connect/exchange失败后的自动 retry/reconnect budget 固定为 0。未失败的另一 channel 可以继续显示/工作。重新附着只能由用户退出并重新执行 exact `paraegox tui`，重新经历 Status、atomic dual locator、pins、handoff 与初始 Latest；旧 projection clock/revision 不跨进程比较。
- global `paraegox --help` 成功并包含 exact `paraegox tui --config <absolute-paraegox.toml>` 行；为保持现有单一 global-help grammar，`paraegox tui --help` 不是第二条 help surface，而是 recognized TUI grammar error，返回 exit 2 与 `PXLC-TUI-GRAMMAR`。UI 启动前，recognized grammar/config/config-authority failure 为 exit 2；execution identity、not-running、terminal、locator/handoff/bootstrap/peer/protocol/I/O/child-start failure 为 exit 1，并且 stderr 恰有一条 `paraegox: code=<stable-code> message=<public-safe-message>`，stdout 不输出 bootstrap/path/token。稳定 taxonomy 使用 `PXLC-TUI-GRAMMAR`、既有 ConfigError/PXLC-LIFECYCLE-CONFIGURATION、`PXLC-EXECUTION-IDENTITY`，以及 `PXLC-TUI-NOT-RUNNING|TERMINAL|LOCATOR|HANDOFF|BOOTSTRAP|PEER|PROTOCOL|IO|CHILD`。
- 为同时满足一条 public diagnostic与 ADR-0009 direct child，hidden fd mode在 Textual接管 terminal前不得写 stdout/stderr或 traceback；Rust parent把该 mode的 child stderr直接重定向到 null而不是可能阻塞的未drain pipe，只按 private exit status映射一次 public错误：20=`HANDOFF`，21=`BOOTSTRAP`，22=`PEER`，23=`PROTOCOL`（包含 initial Inspection NotFound/correlation/frame拒绝），24=`IO`（包含 timeout）；任何其他 nonzero、signal、parent wait/join failure或无法分类的 child failure=`CHILD`。这些20..24只是 parent/child private ABI，不是public CLI exit code；parent统一返回public exit 1与恰一条对应 `PXLC-TUI-*` stderr。既有 `chat` path mode的输出/exit保持不变。UI 启动后的 typed unavailable在TUI内显示；用户 `/quit`、Escape或Ctrl-C完成terminal restore、child join与两个typed client close后exit 0。child crash、terminal restore或join失败均按`CHILD`返回exit 1，但仍不得调用lifecycle `down`。
- Rust parent必须在spawn前保存当前terminal state，并以supervision latch处理POSIX `SIGINT`/`SIGTERM`，不能按default handler先于child退出。Textual raw-mode内的Ctrl-C按键仍由App clean exit并最终返回0；外部送达parent的首个SIGINT/SIGTERM只向child转发至多一次、关闭handoff writer并进入wait/reap，不调用`down`。parent在所有child结果后恢复原terminal；child若5秒内未退出则只对该presentation child发送一次SIGKILL并reap。外部signal、forced kill、terminal restore或wait/reap失败都以唯一`PXLC-TUI-CHILD`、public exit1结束；任何路径都不得遗留child或停止Session/Runtime。
- M5a 只提供已运行实例的 Agent conversation 与 Inspection presentation；它没有 Evidence/log view、M3b public stream、deployment/lifecycle action、current-health inference、remote attach、OpsService、Web Console 或 production support。M4a Receipt snapshot 也不解锁该 view；Evidence/logs 只在 M4b 完成后由 M5b 通过另一个 bounded typed read adapter加入，不改变本 grammar。

上述 Artifact F0 已冻结 A1 与 D0b 的六条 intended-public exact grammar、JSON v1、reference、profile 和 owner seam；它们仍未实现、未验证、未登记或发布。A1只是同一candidate内先行的internal implementation slice，不是可独立merge的里程碑；Artifact crate/store、前四CLI、D0b真实consumer、六条system/contract surface与governance rows必须同一exact ref共同过门。除已冻结的 M1、I0、D0a、Artifact F0 candidate、M3a snapshot、M4a Receipt snapshot、M5a TUI 与下文 M2a grammar 外，R0 replace/restart、D1 rollback、M3b public Inspection watch、M4b Evidence/logs、M5b integration、push 和 remote deploy 的 exact command name、参数顺序与 JSON schema 尚未冻结。

M4a 的实现、真实consumer、system harness、public row与immutable exact-ref门禁已把它升级为`Validated`。

M5a仍只到`Implemented candidate`。治理登记本身仍不代表immutable exact-ref验证或里程碑完成。其余尚未实现的 public API/CLI 不得预登记到 `governance.toml`，必须在实现、真实 consumer 与 system test 同一批次内同步登记 producer、consumer、owner、权限、失败语义与兼容规则。

## Step DAG 与当前进度图

```text
M0 文档/治理 ─> M1 离线 CLI ─┬─> I0 本地 init ──────────┐
                         └─> M2a headless 生命周期 ─┬─> D0a compiled-in local deploy
                                                    └─> M3a snapshot ─┬─> M5a attach TUI
                                                                      └─> M3b public watch（独立后续）
D0a + M3a ─> M4a current-Running PXMT snapshot ─> M4b durable PXEV/failure query/logs
M5a + M4b ─> M5b TUI logs integration
ADR-0011 Accepted + authorization receipt（decision complete）
  └─> Artifact F0 single admission candidate（六条合同冻结，未发布）
        └─> A1 internal mechanism/evidence ─> D0b fresh real consumer
              └─> same exact-ref admission + governance + review + merge gate ─> R0 replace/restart ─> D1 rollback
ADR-0004 真实触发 fixture ─> A0 条件 gate ─> 最小 owner ADR Accepted 或稳定拒绝（当前 D0a 不触发）
D1 + M5b ─> G0 本地 golden path ─> N0 Node enroll/transfer ─> N1 push-only ─> N2 remote deploy ─> O0 OpsService 最后准入
```

依赖说明：I0 可在 M0/M1 收口后与 M2a exact-ref 收口并行，D0a 只等待 I0 + M2a，不等待 A0、ADR-0011 或 Artifact F0。M3a 的 immutable exact-ref gate 是 M5a 唯一新增前置；M5a 不等待 M3b、M4a/M4b、ADR-0003 或 OpsService。M3b 是消费 M3a 的独立后续 public watch 路线，既不阻塞 M5a，也不被 M5a 内部受限的一次一请求 Watch loop 冒充完成。M4a 同时等待真实 D0a point-in-time PXMT 输出与 M3a 已验证的 read-only owner-locator 路型，但只增加 current-Running Receipt 可见性。M4b 再等待 M4a 与对 D0a/PXAR9 PXEV producer、唯一 writer、失败生命期查询和 structured-log redaction/retention 的显式决策；M5b 只等待 M5a + M4b，不被 M4a 解锁，也不依赖 M3b。ADR-0011 Accepted + receipt 决策 gate与六条A1/D0b contract freeze已完成，D0a历史路径前置也由r363满足；执行上仍先形成A1内部机制证据再接D0b，但A1没有独立admission/merge状态，D0b也不是“等待一个已发布A1”。二者必须在同一immutable ref由真实Controller/Runtime/TUI消费、system evidence、governance与review共同收口，之后R0/D1才可前进。A0 不是正常链的预置层，只在 ADR-0004 的真实 fixture 出现时截断相关副作用。只有 D1 与 M5b 都完成，G0 完整本地 golden path 才成立。N0–N2 不得越过 G0，O0 永远最后。

| Step | 当前状态 | 依赖 | 用户可见结果 | 完成证据 |
| --- | --- | --- | --- | --- |
| M0 文档与治理权威 | Validated candidate（r356 Ubuntu complete-governance PASS） | 无 | 正式 docs 可版本化，只有 `docs/workbench` 保持本地；本 Program 成为当前路线 | candidate commit 追踪全部正式 docs；治理检查拒绝未追踪正式 docs 和已追踪 workbench；文档链接/状态检查通过 |
| M1 离线 CLI 首切片 | Active（r356 Ubuntu Rust/unit PASS；artifact/process smoke 待验） | M0 | 用户能查询版本、严格检查配置、离线诊断静态前置条件 | exact grammar 正负测试、稳定 JSON/exit code、Secret/network/state 零副作用测试；对应 public API 登记 |
| M2a 本地 headless 生命周期 | Validated candidate（r356 Ubuntu Rust、174/174 local 与 2/2 real-process 场景 PASS） | M1 | 用一份现有严格 chat 配置执行 `up/status/down`，三条操作共享一个 headless whole-local composition owner | exact grammar/JSON 正负测试；pre-state Secret 失败零副作用；并发同 generation 且 `changed` 与请求相关；private lock/record/same-user UDS、signal/joined shutdown、owned-socket 清理与 terminal record；同一 exact Ubuntu ref 的完整证据 |
| I0 本地 init | Active（r363 Ubuntu Rust/unit + macOS artifact light smoke PASS；sudo ownership matrix 待验） | M1；可与 M2a 收口并行 | 一条离线命令生成 private deterministic-echo config workspace，不创建 domain state | exact grammar/JSON 正负测试；byte-identical 幂等 `changed = false`；文件/权限/symlink/内容冲突不覆盖；Secret/owner/network/state 零副作用；public API 登记与 exact-ref 平台证据 |
| A0 条件式 Application/Installation gate | Not triggered（当前 D0a） | ADR-0004 真实触发 fixture | 只在 multi-Deck 统一发布/更新/卸载、installation-owned mutable state、多隔离安装或多 Artifact 稳定 owner 出现时准入最小 owner | 与触发条件对应的真实 fixture、owner/consumer/failure evidence，以及最小后继 ADR Accepted 或稳定拒绝；不预建 Application/Installation |
| ADR-0011 external-Artifact 边界 | Accepted / decision complete（receipt 三元组已冻结） | 已满足 | 决定单 external Artifact 的 immutable materialization、Deployment selection 与 Runtime reopen 边界 | Accepted ADR、ADR index 与 authorization receipt 的三个冻结 SHA；不把决策记录当成实现证据 |
| A1 external Artifact build/inspect/materialize | Contract frozen / Internal slice（未实现；不可独立登记/合并/发布） | ADR-0011 Accepted + receipt；必须与D0b同一candidate；若触发A0还需其最小ADR Accepted | 无独立用户可用结果；只在候选内部构建、inspect并物化`developer-local-echo-prefix-v1`供D0b真实消费 | 六条合同中的前四条 grammar/JSON/reference/profile golden；reproducible pair、strict inspect、tamper/compatibility、crash/durability/idempotency/Receipt 与 noexec/no-install/no-Graph 证据；这些证据单独不完成admission |
| D0a compiled-in local deploy | Validated（r363 Ubuntu exact-ref gates、182/182 non-root local、3/3 真实 binary system functions + macOS artifact light smoke PASS） | I0 + M2a | 用 exact CLI 把唯一 deterministic fixture 经现有 Controller/Runtime 到达 point-in-time `ActiveReady`；重复/并发 follower 不新增 revision/apply | exact grammar/strict JSON、真实 owner/receipt correlation、same-request/replay/concurrency、unsupported profile pre-effect reject、down race/supervisor crash/output failure/tamper/no-leak 的 focused/system/exact-ref 证据 |
| D0b external-artifact local deploy | Contract frozen / Joint candidate（未实现、未登记、未发布） | 同candidate内A1 internal evidence；D0a r363与ADR-0011 gate已满足 | joint gate后才允许用户在独立fresh workspace消费exact pair/owner Receipt，由Controller/Runtime到达Ready | exact grammar/JSON/fresh-only；pair 进入 Plan/Slice；Controller/Runtime/TUI真实消费与Receipt correlation；replay/conflict/output-loss/Uncertain/tamper-race/no-second-writer/no-double-active；与A1同一exact-ref/governance/review/merge gate |
| R0 replace/restart | Planned/Blocked | D0b | 显式替换新 Artifact；或在同 Artifact/state 上安全停止后重启，不透明自动 retry | forward DeploymentRevision、generation fencing、joined stop、SIGKILL/owner loss、stale generation、same-state restart、partial failure 与 no-double-active 证据 |
| D1 rollback | Planned/Blocked | D0b + R0 | 选择已知 `ArtifactObjectRefV1` 为新的前向 DeploymentRevision，并重新验证 Ready | target identity/compatibility、历史 Artifact 完整对象复验、rollback partial failure、Ready/Uncertain 与 Receipt correlation；不切 active pointer |
| M3a 本地 Inspection snapshot | Implemented candidate（中央 immutable exact-ref CI pending；不是 Validated/Completed） | M2a | 用户通过一条 exact read-only CLI 读取带 typed source/revision/freshness 的 PXIS v2 snapshot | exact grammar/JSON/exit/channel；安全 locator；一次 Latest、无 retry/mutation；fresh→stale 与 no-leak 的 focused/system/exact-ref 证据 |
| M3b 本地 Inspection watch | Planned/Deferred（未登记 public grammar） | M3a | 在另行授权后持续观察 projection 变化 | projection-aware cursor、NotModified/gap/reset、断连、backpressure、bounded reconnect 与 restart 证据；不得写 desired state |
| M4a current-Running PXMT Receipt snapshot | Validated（r410 immutable exact-ref `474cf227…`；不是 Completed） | D0a + M3a | 用 exact read-only CLI 读取当前 Running generation 的一份 verified Runtime PXMT public-safe snapshot | Ubuntu run `31423939905` 完整门禁与M4a 4/4 exact-binary场景；exact grammar/JSON/exit/taxonomy、Status→PXRL→typed Latest、canonical/digest/correlation/Ed25519双重验证、Secret-free/no-mutation/down-no-cache/ABA证据；Mac仅light artifact边界 |
| M4b durable Evidence 与 structured logs | Planned/Blocked（需 producer/writer/failure-lifecycle 决策，未冻结 public grammar） | M4a + 显式 M4b 决策 | 用户在 activation 失败或 owner 退出后仍能查到 owner-issued PXEV、Receipt ref 与 bounded structured logs | D0a/PXAR9 真实 producer→唯一 durable writer→typed reader；重启/失败后查询、storage-full/retention/cursor/truncation/redaction、Unknown/Uncertain 与无 raw-store 证据 |
| M5a 已运行实例 attach TUI | Implemented candidate（已实现、登记并接入 exact CI；不是 Validated/Completed） | M3a | 用户用 exact CLI 附着同一 Running generation，由 Python Textual 直接 typed clients查看 Agent conversation 与 Inspection；detach 后 Session/Runtime 继续 | exact grammar、atomic dual locator/pins、token-free child handoff、pinned direct clients、一次 Latest + 单 Watch/≥1s、no reconnect、TTY/slow-consumer、detach/no-owner-stop 的 focused/Linux system 证据；Mac bounded artifact light smoke |
| M5b TUI logs integration | Planned/Blocked（未登记 surface） | M5a + M4b | 把 M4b 已准入的 bounded Evidence/log projection 加入既有 attach TUI | M4b owner Receipt/cursor/retention 与 typed client/TUI backpressure、redaction、unavailable/no-health-inference 的组合证据；不发明第二套日志 owner/API |
| G0 本地 golden path | Derived gate，未满足 | D1 + M5b | 新用户可复现 init 到 rollback 的完整本地路线 | immutable exact-ref Mac/Linux guide run、claim-to-evidence、已知限制；不是独立实现 Step |
| N0 Node enroll/transfer contract | Planned | G0 | 显式登记目标、信任、传输身份和暂存边界；不依赖 Remote Agent | target identity/host-key/trust pin、Secret-safe credential reference、transport interruption/resume/cleanup 与 staging integrity 证据 |
| N1 push-only | Planned | N0 | 只把一个 exact artifact 传到 target staging，不安装、不激活、不重启 | source/target digest 相等；重复传输幂等；中断无半成品；证明零 install/activate/restart/domain mutation |
| N2 remote deploy | Planned | N1；复用已验证 D0b/D1 | 远端复用 external Artifact materialize、Controller commit、Runtime activate/Ready 与 owner Receipt 语义，不引入 Installation active pointer | transport 与 operation identity 分离；断连/timeout 为 Uncertain；远端 rollback、两平台/两主机 exact-ref smoke 与无 SSH hidden fallback |
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

M2a 不包含 `restart`、SIGKILL/owner 丢失后的 orphan recovery 或强制清理，也不包含健康/Inspection、Receipt/Evidence/日志、attach/TUI 或任何 Remote Agent 扩张；它们分别属于 R0、M3、M4a/M4b、M5a/M5b 或冻结范围。

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

此处冻结 contract 并授权 M2a 候选实现；milestone 是否完成仍由可复现 evidence 决定。r356 immutable exact ref 已在 Ubuntu 绑定 locked Rust format/metadata/workspace check/Clippy/test compile、完整治理、workspace doctest、non-root 默认栈下的 `paraegox-local` 174/174，以及 `tests/system/test_m2_local_lifecycle_cli.py` 中 owner-contention 和完整 lifecycle 两个真实进程场景。后两项覆盖 pre-state Secret 失败零 lifecycle/domain state 副作用、并发 same-config 单 generation 与请求相关 `changed`、只读 status、joined down/terminal record、config drift 和 no-leak；由于 consumer 未安装 pytest，测试函数由 Python 3.11 直接加载执行，而不是经 pytest runner。r375 macOS artifact workflow 还运行真实 deterministic `init→up→status→down→status` light smoke，核对 canonical JSON envelope、稳定 generation/readiness 与 joined socket/process cleanup；它是 single-runner 平台轻量证据，不替代 Linux concurrency、config-authority、owner-loss、sudo peer、`/proc` 或完整 validation。

### I0 — 本地 init

生产者是 `paraegox-local` 的窄 filesystem adapter 与既有严格 chat decoder，消费者是新用户、开发自动化和 D0a guide。I0 只拥有“按固定模板原子创建 private config workspace”的一次性动作，不拥有其配置指向的 state，也不持有常驻 lifecycle。恢复只允许重新运行 exact 请求：byte-identical 完成态返回 `changed = false`；任何 uncertain 临时状态必须在发布前可安全清理或在下次请求中 fail-closed，不得覆盖既有对象。实现与测试必须同时冻结上述 grammar、JSON、mode/symlink/owner 检查和零副作用边界。

### A0 — 条件式 Application/Installation gate

A0 不是 D0a 的固定前置层，也不是“创建完整 Application 平台”。它只在 ADR-0004 的三类真实 fixture 出现时触发：多个独立 DeckLock 需要统一 release/update/uninstall；installation-owned mutable state 需要跨 DeckRun、升级或重部署存续；或同一 release 需要多次隔离安装/多 Artifact 需要共同稳定安装 owner。当前单个 compiled-in deterministic fixture 无 external bytes、无 installation-owned state、无多隔离安装，因此 I0 和 D0a 都不触发 A0。

一旦触发，必须在相关 build/materialization/deployment/data mutation 前提交与真实条件对应的 fixture、identity producer、独立 consumer、唯一 writer、crash/partial failure 与 retain/delete/GC 边界。合法出口只有接受一份定义实际最小 owner 的后继 ADR，或以稳定 diagnostic 拒绝该能力。不得用 CLI 命令名、目录、自由字符串 scope、plan 或预建 Application/Installation/active pointer 冒充 owner 证据。

### A1 — external Artifact build/inspect/materialize

A1 的 Accepted decision gate 与 Program contract 已完成，但 delivery状态只能是 `Contract frozen / Internal slice`：它没有实现、真实外部 consumer、governance row 或 exact-ref evidence，也不能独立进入main/merge/admission。候选分支可以先实现和验证 Artifact crate/store与前四CLI，为同一candidate内的D0b接线提供机制；在D0b真实Controller/Runtime/TUI consumer、两份system harness、六条surface与governance rows齐备前，这些入口保持implementation-internal，不得打包、发布或声称用户可用。实现必须严格消费上文 profile/reference/grammar/JSON：build 从 exact source 产生 byte-identical payload/canonical manifest，inspect 严格只读且零 mutation，ArtifactStore 以 durable admission→pair publication→exact reopen→terminal record/Receipt 的唯一顺序物化完整 ref。任何 tamper、truncate、oversize、unknown version/field、pair swap、ABI/target/entrypoint mismatch 必须在 store mutation 前失败关闭。

A1 internal claim-to-evidence 必须在最终包含D0b的同一 immutable candidate ref完整覆盖；任何较早内部commit的结果只能是开发证据，不能单独升级状态：

- 单一canonical semantic ledger是全部wire/state golden的唯一测试常量源；hardcoded Rust/Python goldens逐byte覆盖206-byte PXAM、72-byte PXAK、PXAQ/PXAA/PXMU/PXAV/PXAW/PXAX、672/912/1104/1216/1408/1456/1648/2848-byte PXAZ frame及其672/912/1330/1216/1634/1456/1874/3074-byte logical accounting、PXAY/PXOP、四份PXAW-only query JSON、capacity vectors、text object/Receipt refs、全部SHA-256 domain、pair swap与全部frozen JSON key order；两种语言独立strict decode同一hardcoded expected bytes，不调用production encoder生成期望值；同输入reproducible build byte/digest一致，payload/string/limit/reserved/flag/order/checksum/short/trailing与1241344/1241345-byte边界各有正反例；
- build 不执行 payload；inspect 前后 workspace/store/lifecycle/domain digest 不变；tamper、truncate、oversize、unknown、payload/manifest pair swap、相同 payload/不同 manifest 与 unsupported ABI/target/entrypoint 都在 pre-effect拒绝；
- 首次materialize在全部config/pair/A0/path preflight后才建立non-authoritative state-root scaffolding，并以含首个PXAQ/PXAA的snapshot-sequence-1 staging directory做唯一atomic `RENAME_NOREPLACE` authority publication；valid attributable same-request staging/canonical same-operation next在active无terminal时精确UNCERTAIN，different/partial/unknown、final+staging、extra entry与root/lock/snapshot strict failure精确OWNER且owner fields为null，final-without-valid-snapshot绝不按zero store处理。same operation + same request只replay/advance同一snapshot record，same operation + different request/ref pre-effect conflict；current/PXAZ/PXAQ config与D0b PXDQ/Receipt-chain config的两类错误按冻结total order稳定区分；
- admission、flags0→1 PXMU、pinned-objects parent下no-overwrite child mkdir、child/parent fsync与exact reopen、两份pair temp write/sync、各final no-overwrite publish、final child fsync、same-pinned-parent pair reopen、PXAV、PXAW、PXAX每个独立snapshot successor与Receipt output逐点crash。fault points至少位于mkdir后/parent fsync前、parent fsync后/child reopen前、每份temp与final、child fsync前后、same-parent reopen前后及PXAV前后；每份next精确N+1并在replace前后复核root/lock/final。restart先收口next再锁存PXMU recovery-start：snapshot已有PXAV必须复用并E，无PXAV但pair可沿同一pinned parent refsync/dirsync证明则同一successor加PXAV/HW并M，partial/identity/durability不明才U；F必须证明零pair effect/remnant，U在PXAX前后都可保留exact quarantine/block。golden与fault harness拒绝交换M/E，证明零目录推理、零新operation、零overwrite/delete/path fallback；
- private absolute root与0700目录、固定exact-three stable entries、0600 regular single-link lock/pair、owner/mode、178-byte suffix/3917-byte state-root边界、path traversal、symlink/hardlink、nlink/inode replacement、short write与no-overwrite；strict-valid lock的WouldBlock/EWOULDBLOCK对materialize/query都输出exact UNCERTAIN，其他lock syscall为IO，metadata/identity/layout invalid为OWNER，且try-lock先于本invocation任何effect。shared/exclusive lock无retry，Drop explicit unlock，fork/spawn/exec前drop全部clone且child防御性unlock/close。query对virgin/operation-absent分别NotFound且零创建，四种PXAW-only状态逐行返回exact JSON，绝不fsync/cleanup/recover/补PXAX；
- materialize/query/replay的success、error、NotFound与output-loss全部在JSON serialize/write/flush前显式unlock/drop每一个handle与clone；blocked-stdout harness必须证明第一调用阻塞write时第二个合法query与mutation仍可进入lock acquisition，failing-stdout harness证明不relock、不rollback/cleanup、不改变durable bytes或`changed`归因，随后原operation query取得同一terminal/Receipt；
- PXAA operation sequence与PXAV object sequence各从1逐次+1并和各自snapshot successor的high-water/count同commit，replay沿用，overflow/ambiguous publication失败关闭。精确验证64 objects、1024 operations、last-U在PXAX前后quarantine归因、zero-byte attributed entry、540/541-byte边界、1241152-byte body与1241344-byte snapshot；1259164只声明componentwise conservative logical regular-file accounting upper bound，不声明64个indexed objects、最大snapshot与540-byte unindexed quarantine可由同一canonical history同时达到，2500778只声明componentwise conservative transaction upper，两者都排除directory/zero lock/filesystem blocks。capacity text fixture只喂pure/injected checked calculator的isolated component caps与overflow，不制造canonical state；calculator证明8388608接受、8388609或overflow拒绝，同时证明count caps令public v1不能自然耗尽8-MiB defense ceiling，不用padding、假history或超额quarantine造边界；
- architecture negatives 证明没有 Application/Installation/InstallationId/active pointer/uninstall/GC、没有 RuntimeHost `install-v1` alias、没有 Process/reference-worker执行，也没有 Graph/workflow owner；A0 三类触发 fixture在首个 mutation 前精确拒绝。

Artifact F0 A1-internal→D0b 实现序列的 exact write-set union 冻结为以下路径；候选内可按owner拆commit并先完成A1 mechanism evidence，但不能越出union、不能让D0b在缺少这些机制时伪造consumer，也不能把任何A1-only commit独立merge/admit/release：

- 本 Program、workspace `Cargo.toml` 与 `Cargo.lock`；新建 `crates/paraegox-artifact/Cargo.toml`、`crates/paraegox-artifact/src/{lib.rs,contract.rs,store.rs}`；
- `crates/paraegox-local/Cargo.toml` 与 `crates/paraegox-local/src/{artifact.rs,config.rs,error.rs,main.rs,layout.rs,lifecycle.rs,composition.rs}`；
- `crates/paraegox-runtime-contracts/Cargo.toml` 与 `crates/paraegox-runtime-contracts/src/{lib.rs,managed_model_agent_stack_plan.rs}`；
- `crates/paraegox-deployment/Cargo.toml` 与 `crates/paraegox-deployment/src/{lib.rs,developer_fixture_agent_stack.rs,managed_model_agent_stack_producer.rs,managed_model_agent_stack_apply.rs,runtime_control_client.rs}`；
- `crates/paraegox-runtime/Cargo.toml` 与 `crates/paraegox-runtime/src/{lib.rs,managed_model_agent_stack_runtime.rs,managed_model_agent_stack_state.rs,managed_model_runtime.rs,managed_service_assembly.rs,runtime_control_endpoint.rs,admission.rs}`；
- 仅与本合同相关的 `tests/fixtures/**/*artifact*`、`tests/system/test_a1_local_artifact_cli.py`、`tests/system/test_d0b_external_artifact_deploy_cli.py`、必要的 `tests/governance/test_artifact_f0_boundary.py`、`.github/workflows/{ci.yml,macos-cli-artifact.yml}`；只有A1 producer、D0b Controller/Runtime/TUI consumer、六条命令与两个system harness在同一candidate存在时才于该candidate更新`governance.toml`，任何A1-only阶段不预登记且不可merge。

明确不在 write-set：`crates/paraegox-process/**`、reference worker、RuntimeHost install-v1 文件、ADR-0003/0004/0005/0011、其他 Program/guide，以及 Ops/Remote Agent/Graph/push/remote/R0/D1 surface。若真实实现必须越界，停止该批并回到 Program/ADR 决策，不以“接线”名义扩张。

### D0a — compiled-in local deploy

D0a 按上文 exact grammar 确保唯一 compiled-in `deterministic-echo-v1` 走过现有 DeveloperLocal composition、唯一 M2a supervisor、真实 DeploymentController 与 Runtime terminal `ActiveReady`。它不安装 bytes，不引入产品/Application Installation或Artifact owner，不依赖 ADR-0011，不触发 A0；既有 legacy `installation_id` 继续只是内部RuntimeHost/DeveloperFixture身份且不进入public JSON。成功输出只摘要 verified owner references/digests 与 point-in-time outcome，不是新的“总 Receipt”或当前健康声明。

实现批次必须同时包含 exact CLI/serializer、expected-generation-bound owner-private `DeployQuery`、真实 consumer、`tests/system/test_d0a_compiled_local_deploy_cli.py`、相关 CI 与该已实现 public surface 的 `governance.toml` 登记。r363 已将这些 surface 收在同一 immutable exact ref，其 focused/system 证据覆盖 init→deploy、重复 deploy、up→deploy、并发 follower、generation mismatch、config drift、provisioned pre-effect reject、query failure/down race/supervisor crash/output failure/tamper、`changed = run_up.changed && !model_agent_replayed` 与 no-leak。

r363（`20ef3f281501e3399d83c7e42e0150f208f4e8cd`）的 Ubuntu exact-ref 结果是：format、locked metadata、workspace all-targets check、Clippy `-D warnings`、workspace all-targets `test --no-run`、完整 governance 与 workspace doctest 全部 PASS；non-root 默认线程栈的 `paraegox-local` 全量测试在短 `TMPDIR` 下 182/182 PASS；同一真实 D0a binary 直接执行三个 system function 为 3/3 PASS。第一次全量运行的 3 个失败都是过长 `TMPDIR` 触发 Unix-domain socket `sun_path` 上限；不改代码和测试，只换短 `TMPDIR` 原样重跑即 182/182，所以记为验证环境限制而非产品失败。GitHub macOS workflow run [`31382789910`](https://github.com/jsmy-CTH/ParaEGOX/actions/runs/31382789910) 在同一 commit 上全部 PASS，生成 artifact `paraegox-macos-x86_64-20ef3f281501e3399d83c7e42e0150f208f4e8cd`（id `9060699090`，过期时间 `2026-08-17T11:23:46Z`）；其证据边界是 native build、public CLI/light init/deploy、relocated bundle 与 Textual/Agent IPC smoke，不是 D0a ActiveReady system test。`init` 的 sudo/ownership matrix仍未闭合；因此本记录只把 D0a 升为 exact-ref `Validated`，不把整个 Program、external Artifact、replace/restart、rollback 或远程路线升级为完成。

### D0b — external-artifact local deploy

D0b 的 intended-public contract 与 owner seam 已冻结，状态为 `Contract frozen / Joint candidate`，不是等待一个已独立admitted A1的blocked step。r363 D0a与ADR-0011 Accepted/receipt gate已满足；下一执行动作是在同一candidate先完成A1 internal mechanism/evidence后立即接入本真实consumer，最终统一做exact-ref admission/governance/review/merge。任一中间点都不能把external path或前四CLI标成实现可用。D0b 只能在 workspace B 的 fresh lifecycle/deployment state中消费已 terminal materialized 的完整 ref 与 owner Receipt，且 D0b 必须是第一次 lifecycle mutation；不得先调用 D0a `run_up()`、读取 workspace A、保留旧 desired、用 compiled-in fallback或让 ArtifactStore选择 active object。

Controller 必须把完整 ref、materialization Receipt commitment、profile/ABI/target/entrypoint写入 exact ArtifactExecutionBinding、PlanContent v2 与 PXTE11/PXAR12 Slice，再由 PXMJ2 唯一top-level desired authority独占 commit/revision/deployment operation/Receipt。Runtime 对 authenticated apply 与 exact Slice 做 read-only pre-effect reopen/reverify，只有 exact external payload 驱动的 `prefix || prompt` 已由 Agent/TUI 外部观察、Runtime PXMT terminal 为 `ActiveReady` 且 Controller关联当前未 supersede revision时，CLI 才可返回 `state = "active_ready"`。Materialized、Committed、Applying、Runtime terminal与Ready互不推导；Inspection仍独占当前 health。

D0b claim-to-evidence 必须在同一 immutable candidate ref 完整覆盖：

- 六条中后两条 exact grammar/JSON、D0a 五 token不变、help与strict dispatch；malformed deploy的三个D0b reserved option精确归external grammar而其余旧D0a negatives逐 byte不变；workspace B fresh initial成功，workspace A 既有 D0a desired精确返回 `PXLC-DEPLOY-REPLACE-REQUIRED`，fresh workspace不得先出现D0a lifecycle mutation；
- complete object ref/materialization Receipt/profile/ABI/target/entrypoint逐项进入 exact 192-byte binding、PlanContent v2、PXTE11/PXAR12与target Slice digest；D0b admission只接受M/E的exact PXAX→PXAW→PXMU→PXAV→PXAA→PXAQ→PXAM/payload链，并证明current/PXAZ/PXAQ/PXDQ四份config commitment相等。current-vs-root mutation稳定命中`PXLC-LIFECYCLE-CONFIGURATION`，PXDQ-vs-receipt mutation稳定命中`PXLC-DEPLOY-MATERIALIZATION-RECEIPT`；hardcoded successor/predecessorgolden和cross-version negatives同时通过。DeploymentController 独占revision/deployment operation/Receipt；PXDQ/PXDK/PXDM/PXDO deployment id全链不变，materialization id只由ArtifactStore Receipt链解释；source/dependency guard证明Controller不读payload/store目录；
- Runtime只经 read-only port pre-effect exact reopen；missing、wrong pair/digest/target/entrypoint、materialized-only、inode/pair replacement与 reopen→effect tamper race全部稳定拒绝且不改变 active head；Runtime不写 ArtifactStore、不签第二份 materialization/deployment Receipt；
- same operation + same canonical request只 replay/推进同一 operation及其exact desired，同 id + different request pre-effect conflict；different id或首次admission前任何非本operation既有desired一律replace-required；PXDK Controller-global admission sequence从1逐次+1、same replay沿用、PXDK/high-water同一durable commit、overflow/preflight与ambiguous publication fail-closed；terminal output loss以原id query，timeout/owner-loss/correlation缺失保留 `uncertain` 且零透明retry；
- 真实 TUI/Agent response逐 byte证明 external prefix改变结果；owner Receipt refs、Controller revision、Runtime apply request digest与PXMT terminal digest全链相关；一次64-byte CSPRNG draw产生Runtime ApplyOperationId/TemporalConstraintId/auth nonce并在C首次durable，PXMT/PXMA2 terminal id等于PXAR Runtime id，PXDM/PXDO只以exact request digest桥接且不把deployment/materialization id当Runtime id。single semantic ledger、fixed injected constants与role-swap/correlation-drift negatives证明typed separation及C后不重抽，跨namespace raw coincidence本身不触发额外reject；CLI/file/process/ACK/log不能合成Ready，`current_health_checked = false`；
- fault/race覆盖 admitted→committed→applying→active_ready、failed/uncertain/superseded、Controller commit后Runtime失败、supervisor crash、down/output fault、stale generation与并发 follower；PXMJ2 marker/cutover、lower predecessor重开、PXMA2 restart与v1/v2cross-reject证明任何时刻无第二writer/desired head、无active pointer、无两个live generation/double-active，也不破坏workspace A的D0a/M3/M5基线。

Ubuntu non-root ext4必须对同一最终immutable ref执行全部Rust/focused、跨语言golden、完整governance、deterministic crash harness与A1/D0b两个真实binary system files；只有这一joint result才能同时admit/merge/register/publish Artifact F0，绝不先升级A1。Mac只允许candidate内offline build/inspect、grammar/JSON/golden与bundled light smoke；不运行本机Cargo，也不以Mac light证据替代Ubuntu owner、ext4 durability、fault、ActiveReady或joint admission矩阵。

### R0 — replace/restart

R0 吸收原 M2b 的 restart/异常恢复，并在 D0b 后增加真实 external Artifact replace；它不从 D0a 的 compiled-in fixture 抢跑。same-state restart 只有在前一 generation 已证明 joined/retired terminal 后才产生新 generation；replace 必须明确 old/new Artifact commitment、forward DeploymentRevision、state compatibility 与 no-double-active，不创建 Installation active pointer。owner 丢失、SIGKILL、陈旧 generation、超时或证据缺失不能以 PID 消失合成 stopped，也不能自动透明 replay；应返回 Failed/Unknown/Uncertain 并提供 bounded recover/reconcile 路径。

### D1 — rollback

rollback 依赖 D0b/R0，选择一个已知、仍物化且重新验证兼容的历史 `ArtifactObjectRefV1`，作为新的 forward DeploymentRevision 执行完整 commit/apply/Ready 流程，不倒退 revision/generation high-water，不产生 Installation revision，也不以目录、Git、backup rename 或 symlink/active pointer 切换冒充成功。历史 payload/manifest/state 不兼容、partial activation 或失联必须保留可查询 Receipt 并报告 Failed/Uncertain；只有新 forward revision 取得 exact `ActiveReady` 才能摘要为 RolledBack。

### M3a — Inspection snapshot

生产者是现有真实 owner facts 经 `paraegox-local` 当前 RunningStack 交给现有 Inspection projection/PXIB/PXIQ/PXIP v2 owner 的 immutable cache，消费者是公共 CLI/operator automation。lifecycle control 只增加一个 config-commitment-bound、same-uid/gid、transient locator action：composition 必须等 lifecycle control 与 Inspection endpoint 都 Ready 后，才把 owner-private PXIB path 交给该 Running generation；locator 只在 exact config/current Running generation 返回它，不持久化、不进入公共 JSON，也不返回 token。CLI 严格加载 PXIB 后直连 Inspection 并只发一次 `Latest`；lifecycle 不是 Inspection proxy，Inspection 仍是 projection owner，CLI 不是 cache、freshness、health 或 mutation owner。命令不得隐式调用 `up`、orphan recovery、restart 或 retry。

M3a 候选批的写集限于本 Program，以及 `crates/paraegox-local/src/{config.rs,main.rs,error.rs,lifecycle.rs,composition.rs,inspection_client.rs}`、`tests/system/test_m3_local_inspection_cli.py`、`.github/workflows/ci.yml`、仅用于 help/light-envelope smoke 的 `.github/workflows/macos-cli-artifact.yml`，以及与实现和真实 consumer 同批新增的 `governance.toml` public API 登记；不修改 `crates/paraegox-inspection/**`、ADR-0003、Python console/TUI、Ops/Remote Agent/Graph。focused tests 必须覆盖 exact grammar/serializer、typed coordinate 与 null/hex/enum、`2^53 - 1`、`2^53`、`u64::MAX` 的 canonical decimal string、Running/config/uid-gid locator、unsafe PXIB/file/socket/peer/token/correlation/trailing/timeout、一次 Latest/无 retry，以及 stale/unknown 不升级。exact-binary system evidence 必须覆盖真实 headless `up` 后 r1、真实 freshness 边界后的 stale r2、重复 r2 revision 不增长、provisioned Secret env 移除后读取、`down` 后无 cached success、path/config/root/extra-arg、Secret/path/token canary、stdout 一个 JSON+LF/stderr 空，并证明 lifecycle/domain 文件未被 snapshot 修改。Mac 不以 Rust/system smoke 替代 Ubuntu exact-ref gate。

上述实现、真实 consumer、focused/system evidence 与治理登记已经形成候选，但中央 immutable exact-ref CI 尚未完整收口。因此本 Program 只把 M3a 标为 `Implemented candidate`，明确不是 `Validated` 或 `Completed`；只有同一 Ubuntu exact ref 的完整门禁与 evidence 可复现后才能升级。ADR-0003 继续 Proposed，M3a 不需要也不构成其 acceptance。

### M3b — Inspection watch

连续 watch 后置且尚无 public grammar/JSONL schema。后续准入必须把 cursor 定义为至少 `(snapshot/protocol version, projection_id, projection_revision)`，不能只用 revision；同 projection 的 NotModified、gap/cursor-ahead 必须显式 resync，新 projection_id 表示 restart reset。一个外层 coordinator 可以循环既有 one-shot Watch，但每次仍只有一个 in-flight exchange，stdout flush 前不得取下一项，不得建无界 queue；poll floor、backpressure、bounded reconnect budget、断连与 down/up 后的 projection reset 都必须有证据。endpoint 断连不等于 source `Partitioned`，restart 不得比较旧 clock/revision，也不得自动调用 `up`、restart 或 orphan recovery。现有 producer 目前只足以证明 initial snapshot 与一次 freshness 变 stale；在真实 refresh/recovery producer 与上述测试存在前，不冻结 watch command，也不声称持续运维能力。

### M4a — current-Running verified PXMT Receipt

M4a 的真实 producer 是现有 D0a/M2a DeveloperLocal activation 路径中由 Runtime 签出的 PXMT v1；fixture 与 provisioned profile 都已经产生同一类 canonical Receipt。新增 owner-private adapter 只消费这份真实输出，使其在同一 RunningStack 生命期内可被一次性查询；它不从 D0a 摘要重建 Receipt，不让 lifecycle 返回 domain payload，不让 Inspection 容纳 Receipt history，也不让 CLI 打开任何 owner store。这一命名使用 `receipt` 而非 `evidence`，因为当前 D0a/PXAR9 没有 PXEV producer，且本切片不消费 `EvidenceRecordV1`、`EvidenceRefV1` 或 `LocalEvidenceStore`。

M4a validated implementation 的 exact write-set 冻结为：

- 本 Program；
- `crates/paraegox-local/src/{config.rs,error.rs,main.rs,layout.rs,composition.rs,lifecycle.rs,receipt_snapshot.rs}`；新的一个 `receipt_snapshot.rs` 内聚 owner-private bootstrap/endpoint、bounded typed client、PXMT 验证与 public-safe projection，不为 server/client 再拆一调用 wrapper；
- shared wire goldens `tests/fixtures/wire/{m4a_receipt_locator_v1.hex,m4a_receipt_bootstrap_v1.hex,m4a_receipt_latest_request_transport_v1.hex,m4a_receipt_latest_ready_response_transport_v1.hex,m4a_receipt_latest_not_found_response_transport_v1.hex}`；request transport golden 必须包含 raw token + PXRQ，两个 response transport golden 必须包含 outer u32 length + PXRO frame/payload；
- `tests/system/test_m4a_local_receipt_snapshot_cli.py`；
- `.github/workflows/{ci.yml,macos-cli-artifact.yml}`；
- 只在 exact CLI、真实 Runtime Receipt producer、可执行 consumer 与 system evidence 已在同一候选批出现时才同步更新的 `governance.toml`。

合同冻结阶段不预登记 `governance.toml`；当前候选只因为 exact CLI、真实 Runtime Receipt producer、可执行 consumer 与 system evidence 已同批出现，才按上面的 write-set 同步新增对应治理行，登记不能替代 immutable exact-ref 验证。候选实现不修改 `crates/paraegox-evidence/**`、`crates/paraegox-runtime/**`、`crates/paraegox-runtime-contracts/**`、`crates/paraegox-deployment/**`、`crates/paraegox-inspection/**`、`crates/paraegox-agent-service/**`、`src/paraegox_sdk/**`、Cargo dependency/feature、ADR、其他 Program/guide、M3b/M4b/M5b/Ops/Remote Agent/Graph 或 persistent format。如果实现证明必须越过这个写集，必须停止并回到 Program/M4b 决策，不得以“接线”名义修改 Runtime、Inspection 或 Evidence owner。

M4a 的 claim-to-evidence 验收矩阵固定为：

| 合同 claim | focused / consumer evidence | Linux exact-binary system evidence | Mac artifact 边界 |
| --- | --- | --- | --- |
| exact grammar、JSON、exit 与 taxonomy | parser exact positive/negative；global help 只增加唯一 grammar；成功/每个稳定错误 envelope 的 exact key set/type/order、decimal/hex/enum/null 与 stdout failure | 真实 binary 验证 help、relative/default/extra/unknown/config/root/not-running，stdout 一 JSON+LF、stderr 空，exit 0/1/2 精确映射 | 验证 help/grammar/not-running light envelope；不冒充 Linux owner/fault 矩阵 |
| 一次 Status 与一次 PXRL owner locator | exact 54-byte PXLO action `R`、expected generation/config、same-peer、single exchange/exact EOF/no retry；PXRL 布局、digest domain、strict canonical re-encode | marker 证明恰一次 Status、一次 locator、一次 typed Latest；starting/stopping/down、config drift、old/new generation 和 locator replacement 都不返回 cached success | 只运行 never-started/not-running pre-effect envelope；不进入 Running locator，也不声称 raw Status/PXRL 计数、owner、ABA 或 fault 覆盖 |
| 完整 typed Latest ABI 与兼容性 | hardcoded shared goldens逐 byte 覆盖 PXRL、PXRB、raw token + 176-byte PXRQ、outer u32 + PXRO `R`/`N`；request-id、四个 frame digest domain、header/offset/reserved/length/EOF、strict re-encode 与 incompatible-v1 reject | marker 捕获 exact 208-byte request、228..2276-byte response、一次 sequence 1；wrong peer/token/correlation 静默关闭，只有 retiring correlated slot 返回 `N`；5s single deadline、concurrency 8 与 zero retry/fallback | 只证明包含该实现的 artifact 可编译与 light CLI envelope；不执行 PXRB/PXRQ/PXRO `R|N`、golden、silent-close、deadline 或 capacity 语义 |
| 真实 Runtime PXMT 与双重验证 | adapter 与 typed client 各自对 canonical decode/re-encode、request/receipt digest、target/store/key correlation、Ed25519 signing transcript 建立独立正/反/tamper 证据；D0a digest 与 exact bytes 一致 | 真实 deterministic Runtime PXMT 通过；替换 receipt、request digest、public key、signature、target/store 或 canonical bytes 全部 fail-closed，无 fallback | 不启动 M4a Running owner、不读取 PXMT，也不声称 Receipt 或 tamper/peer 证据 |
| bootstrap、peer、bounded protocol 与 TOCTOU fencing | no-symlink/private-parent/regular/0600/nlink-1/device/inode/length/SHA pin；same-uid/gid socket/token/request/correlation；partial/trailing/oversize/NotFound/5s timeout/8-in-flight 上限与 zero retry | locator 后执行 `down` 再 new `up`、替换同名 bootstrap/socket、wrong peer、token/correlation 篡改、endpoint 提前退出；旧请求失败而新请求可独立成功 | 不创建 Receipt bootstrap/socket，不声称 pin、peer、replacement、down/up race 或 bounded protocol 证据 |
| 只读、无 Secret、无第二 owner | 命令不包含 mutation/raw-store API；public projection allowlist、Debug 脱敏、token/seed zeroizing；fixture/provisioned 序列化一致 | 查询前后 lifecycle generation/record、Controller revision/snapshot、Runtime/Deployment/Agent/Inspection/Evidence 文件 digest 不变；provisioned 在 Secret env 移除后成功；不存在或不可读 Evidence state 也不影响 Receipt 查询 | 只验证 rejected/not-running envelope 不泄漏配置、state 与注入 canary，且 relocated bundle 输出一致；不声称 Running no-mutation或 Secret-removal 证据 |
| point-in-time 而非健康/失败历史 | 固定 ActiveReady 投影、`current_health_checked=false` 与其他 valid PXMT terminal 一律按 protocol 拒绝的 contract 测试；M4a server 只准入 current Running `ActiveReady` 行 | 真实 Running 输出 `active_ready`；`down` 后、never-started 或 activation failure 都返回 snapshot null/稳定错误，不从 D0a 摘要或日志恢复 | 只验证 never-started/not-running 时 snapshot null；不验证 point-in-time success、down 后缓存或 failure-surviving query |
| bounded 且脱敏 | PXMT 最大 2048 bytes、locator/bootstrap/request/response/in-flight/time 上限；全字段 canary、输出故障与未审计 low-level error 抑制 | Secret/SecretRef/config/state/bootstrap/socket/store path、token/key/signature/raw PXMT 与 error canary 不出现在 stdout/stderr；输出失败为 exit 1 且不改变 owner state | 只验证 light envelope 的有界单行输出与配置/state/Secret canary；不替代 Linux frame、timeout、oversize或输出故障证据 |
| exact-ref 状态诚实 | 实现、真实 consumer、system harness 与 governance row 同批；Program 只在当前 evidence 上升级 | Ubuntu 对同一 immutable exact ref 执行必需 Rust、完整 governance 与 system gates | macOS workflow 是 artifact/light 边界，不替代 Ubuntu exact-ref 验收 |

上述实现、真实consumer、focused/system harness、治理与workflow接线已在immutable exact ref `474cf2272f45938f12531d23ac559b93884b5ba9` 完整收口，因此本Program把M4a升级为`Validated`，但明确不是`Completed`。Ubuntu run `31423939905` 对该exact ref的format、locked metadata、workspace all-target check、Clippy `-D warnings`、doctest与完整governance全部成功；pytest为578 passed/2 skipped并包含M4a 4/4真实exact-binary system场景，Runtime为768 passed/2 ignored，独立Rust ProcessDomain为1 passed。该4/4矩阵覆盖真实Running read-only success、occupied-port activation failure后无cached success、Starting/Stopping只返回not-running，以及bootstrap已加载后的down/new-up同名socket ABA：旧请求失败、新generation独立成功。

macOS run `31423938755` 在同一exact ref成功完成console 63项、lifecycle Rust 3项及全部bundle/TUI/checksum/upload步骤；artifact id为`9076680242`，name为`paraegox-macos-x86_64-474cf2272f45938f12531d23ac559b93884b5ba9`，size为26907776 bytes，digest为`sha256:a50e684c12643b0fc5173e6d5a8a9f41885a65cb171f95ae9b17f3f54cc9e72b`，到期时间为`2026-08-17T19:32:58Z`。该Mac结果仍只证明native artifact、relocated bundle、help/grammar/not-running envelope与既有TUI light smoke，不执行M4a Running owner、PXMT read、peer/tamper/ABA或Linux no-mutation矩阵，也不替代Ubuntu validation。M4b仍为Planned/Blocked；M4a没有durable failure evidence/log owner，仍不完成原M4或解锁M5b。

M4a 的明确 nonclaims 是：不生产、append、query 或返回 PXEV/`EvidenceRecordV1`/`EvidenceRefV1`；不修改或 raw-open `paraegox-evidence`/`LocalEvidenceStore`；不提供 activation failure、stopped、failed、owner crash 或重启后查询；不提供 history/list/cursor/watch/follow、retention/expiry/truncation、log/trace/metric/exporter、structured startup diagnostic 或 incident timeline；不返回 raw Receipt/signature/key/channel/path/token；不替代 Inspection/current health、D0a deploy outcome、M3b、OpsService、Remote Agent 或 production support；不完成原 M4，不解锁 M5b，不满足 `governance.toml` 的 P6a producer→store→reader checkpoint。在首次公开发布前可整批撤回 grammar/adapter，因为它不引入 persistent format；一旦公开发布，grammar、JSON、PXRL/PXRB/PXRQ/PXRO、token/length framing 或语义不兼容变化必须走显式 successor/deprecation。

### M4b — durable PXEV、failure-surviving query 与 structured logs

M4b 才是原 M4 Evidence/日志诊断的完成步骤。当前真实代码中，只有 distributed PXAR8 Runtime 将 PXTP transport facts 包成 PXEV，并向 `LocalEvidenceStore` append/fsync/readback；D0a/PXAR9 未产生 PXEV，store 没有运行中 read service/IPC，failed activation 会拆除 RunningStack/endpoint，而当前 lifecycle locator 只对 Running generation 生效。此外 supervisor 将 stdout/stderr 重定向到 null，Agent journal 含有会话/request payload 且不是 public-safe 日志。这些是 blocker，不能用 M4a、raw store 或文本 grep 绕过。

M4b 实现前必须以被接受或当前用户显式授权的窄决策冻结：D0a/PXAR9 Runtime owner 如何产生 owner-issued PXEV/Receipt ref；Runtime 与 EvidenceService/store 之间的唯一 writer 与 storage-full/uncertain-commit 语义；activation failure 或 lifecycle owner 退出后哪个独立生命期仍可查询；typed filter/cursor/page、retention/expiry/truncation/gap/reset；structured diagnostic producer、redaction 与 bounded backpressure；Inspection 只投影 Evidence availability/ref 而不容纳 payload/history；M5b Python 经独立 direct typed client 消费，不经 Rust parent proxy。composition/lifecycle 绝不得伪造 `owner_ref = Runtime` 的 PXEV，CLI/TUI 绝不 raw-open store。ADR-0003 仍保持 Proposed；若 M4b 只需 node-local Evidence read owner，不得借此提前实现 OpsService。

M4b 当前不冻结 public command/JSON 或 write-set，不预登记 governance。它的最小完成证据是同一 immutable exact ref 上的真实 D0a/PXAR9 producer→唯一 durable commit owner→失败/重启后 typed reader，再加上 bounded structured logs、retention/cursor/truncation、redaction、storage-full/uncertain 和 no-health-inference 证据。只有 M4b 达到这些条件时，原 M4 才可标记完成并解锁 M5b。日志不替代 Receipt；probe、stdout、进程退出码、文件复制或 transport ACK 不能在证据不足时推导副作用成功，数据不可用、过期、缺失或被截断必须显式保持 Unknown/Uncertain。

### M5a — 已运行实例 attach TUI

生产者仍是 lifecycle、AgentService 与 Inspection 三个既有 owner：lifecycle 只提供同一 config/current Running generation 的 atomic dual locator；AgentService 独占 Session/Request/Turn/cancel journal 与 terminal；Inspection 独占 PXIS projection、freshness 与 one-shot `Latest`/`Watch`。消费者是 Accepted ADR-0009 已准入的 Python `AgentConversationClient`、独立 `DeveloperLocalInspectionClientV2` 与 Textual App。Rust `paraegox` parent 只拥有本次前台 attach 的 lifecycle query、token-free child bootstrap handoff、child/terminal supervision，不建立 presentation proxy、不读取 owner bootstrap/token、不代理 domain traffic，也不成为 lifecycle、Session、Inspection、Deployment、Evidence 或日志 owner。

M5a 是 additive public grammar。新 `paraegox tui` 只附着已经 Running/ready 的 generation，并为这个新入口增加 private pinned-handoff child mode。既有 `paraegox chat` 继续保留“由当前前台 composition 启动、向 Python direct typed clients交付现有两个 private bootstrap path、并在 child 退出后 joined shutdown”的 public 与内部边界；M5a 不要求它改 handoff、不改变其 grammar/exit/lifecycle，也不以新 Program 静默 supersede ADR-0009。两个入口最终都仍是 Python direct typed clients；M5a 只是让 attach 路径在 locator→child-open race中携带完整 pins，而不是引入 Rust `ConsoleBridge` 的后继。

Agent `Open` 可能由 AgentService 创建或重开 exact conversation scope 的 Session，这一 mutation 必须仍由 AgentService journal/terminal 证明；除用户提交文本或显式 `/cancel` 外，不允许其他 domain mutation。关闭 Python typed clients 不等于关闭 Session，detach 或 child crash 也绝不触发 lifecycle `down`、Runtime stop 或 Deployment reconciliation。

M5a 候选实现批的冻结 exact write-set 是：

- `crates/paraegox-local/src/{config.rs,error.rs,main.rs,lifecycle.rs,composition.rs}`；其中 handoff 只作为这些既有 local modules内的 pure bounded codec/FD transfer，不形成新 component或 domain proxy；
- `src/paraegox_sdk/{console_client.py,console_tui.py}`；
- `tests/console/{test_console_client.py,test_console_tui.py}`、共享跨语言 golden `tests/fixtures/wire/m5a_tui_attach_handoff_v1.hex` 与 `tests/system/test_m5a_local_tui_attach.py`；
- `.github/workflows/{ci.yml,macos-cli-artifact.yml}`，以及只在实现、真实 consumer 与 system evidence 同批出现时才更新的 `governance.toml`。

该实现批不修改 `crates/paraegox-runtime/**`、`crates/paraegox-inspection/**`、`scripts/paraegox-console`、Cargo dependency/feature、ADR、其他 Program/guide、既有 `chat` child grammar、M3b/M4a/M4b/Ops/Remote Agent/Graph 或 persistent domain format，也不新增 `tui_bridge.rs`、`console_bridge.py` 或任何 Rust presentation/domain proxy。若实现证明必须越过这个列表，必须先回到 Program 重新评审 write-set，不能以“接线”名义静默扩张。`paraegox tui`、private locator/handoff、真实 consumer、system evidence 与 `governance.toml` public API row 已在同一候选批登记；登记只冻结候选兼容边界，不替代 immutable exact-ref 验证或里程碑完成判定。

后继实现的 claim-to-evidence 验收矩阵固定为：

| 合同 claim | focused / consumer evidence | Linux exact-binary system evidence | Mac artifact 边界 |
| --- | --- | --- | --- |
| exact grammar 与 pre-effect failure | parser exact positive/negative；global `--help`列出grammar而 `tui --help`为exit2 grammar error；absolute/config safety、root/uid-gid、config-authority 与 never-started/stopping 分类；child 未启动 | 对真实 binary 验证global help、`tui --help`、extra/default/relative/path/config/root、never-started/stopping；失败前无 lifecycle/domain/terminal mutation，stderr 仅一条 public-safe diagnostic | 验证global help/grammar/not-running light envelope；另以 bundled artifact 执行 bounded `init→up→tui→down` happy path，不声称 stopping/root/Linux fault matrix |
| hidden child diagnostic channel | fd mode pre-UI stdout/stderr/traceback均为空；private 20/21/22/23/24精确映射HANDOFF/BOOTSTRAP/PEER/PROTOCOL/IO，其他nonzero/signal/wait/join failure映射CHILD；legacy chat mode不变 | 对每类真实bootstrap/peer/protocol/NotFound/timeout fault验证public exit1、parent stderr恰一行、无child第二行或raw error；signal/crash/join只报CHILD | pure Python/private-exit mapping与static workflow检查；bundled artifact只跑成功 child，不声称20..24、signal或真实owner fault |
| 一次 atomic dual locator | exact 54-byte PXLO v1 `T` request、expected generation/config commitment、same-peer、单 exchange/no retry；PXTL v1 header288/max8480、C/I 96-byte records、独立digest domain与canonical re-encode | marker 证明一次 Status 后恰一次 raw PXTL-to-EOF；任一 locator 缺失/替换/不安全均整体失败，不能降级为单 channel attach | shared frame golden与真实 Running happy-path attach会消费双 locator，但不计数raw Status/PXTL exchange，也不替代Linux same-peer/fault evidence |
| generation 与 bootstrap TOCTOU fencing | Ready 前同步捕获两份 regular/0600/nlink-1 pin；strict no-follow reopen 校验 owner、parent/socket identity、dev/ino/len/SHA、lifecycle generation/config→pin与bootstrap token/correlation链 | 在 locator 后暂停，执行 `down` 再 new `up` 并替换同名 PXAB/PXIB；旧 attach 必须 exit 1、零 reconnect，new generation 的独立新 attach 可成功 | bundled artifact验证attach前后同一Running generation；不执行down/new-up ABA、替换或race fault |
| ADR-0009 direct-client/handoff boundary | PXTL strict decode→同字段PXTH canonical re-encode；Rust/Python独立PXTH golden vector与两个不同digest domain；hidden grammar恰为 `--tui-attach-fd 3`且与旧path mode互斥；UnixStream same-peer、single-frame exact EOF、partial/unknown/trailing拒绝；fd3含path/generation/config pins但token-free | canary 证明child argv/env与public output/UI/transcript不含config/state/bootstrap/socket path、generation/raw token、Secret/SecretRef/credential/key；public parent argv允许且只含用户显式config path；marker证明Rust parent不打开owner bootstrap/socket、不代理Agent/Inspection traffic | shared hardcoded PXTH fixture、bundled Python/Ruff与真实fd3 child happy path；输出canary有界，但不替代Linux locator/owner或fault evidence |
| Python pinned bootstrap与认证 | direct clients验证handoff generation/config一致、canonical no-symlink chain、parent 0700/02750，并以 `O_NOFOLLOW` + `O_CLOEXEC` 单 fd校验uid/gid/mode/nlink/dev/ino/len/SHA与bootstrap digest；Agent socket普通identity；Inspection唯一 `.pxi-<32-lowerhex>-socket.pin` 与 original必须 mode0600/nlink2/same inode；peer/token/correlation完整 | locator/pin、tamper/replace/symlink/hardlink/permission/owner/peer/token/correlation/partial-read矩阵整体失败、无 retry；Textual只持opaque client interface，App/widget字段不取得pin/path/token/raw socket bytes | Python fake-UDS/getpeereid focused机制加真实happy-path pins；不声称wrong-peer、tamper矩阵或完整 lifecycle correlation |
| Agent conversation owner语义 | Python `AgentConversationClient` exact `Open` 至多一次；one-pending `Submit`、typed terminal、显式 `/cancel`、conflict/replay/capacity；重附着 `Open(EXISTING)`但无history/pending discovery，Textual只消费 typed result | 真实 deterministic profile 完成 Open→submit→terminal；pending 时 detach 不自动 cancel/replay，新 attach不猜request id且接受owner capacity/conflict；只有这些显式 Agent 操作可改变 Agent journal | bundled artifact真实完成一次 deterministic Echo 与clean detach；不覆盖pending detach/re-attach、capacity/conflict或history恢复 |
| Inspection 初始值与节流 | UI 可见前一次 `Latest`；之后任意时刻至多一个 one-shot `Watch`，start-to-start 固定 `>= 1s`，交付/丢弃前不取下一项；NotModified 不造 revision/queue | 真实 Running 首屏显示 canonical r1；producer 到期只观察 stale r2，重复 r2 不增长；slow child marker证明无并发 Watch、无 unbounded queue | bundled artifact只要求真实r1首屏；r2/NotModified/timer由focused fixture覆盖，不声称real slow-Watch或producer freshness |
| 断连与显式重新附着 | Agent/Inspection channel 分别进入 public-safe `unavailable`；不把 EOF/timeout映射成 `Partitioned`、健康、成功或 stopped；自动 retry/reconnect budget 恰为 0 | 分别切断 owner channel与执行 `down`，marker证明无 reconnect/up/restart/reconcile；另一 channel 可继续，只有用户退出并再次执行 exact CLI 才建立新 attach | 只验证happy-path UI状态与显式Escape；不切断任一real channel，也不声称process fencing或zero-reconnect fault evidence |
| detach、异常退出与 terminal restore | `/quit`、Escape、raw-mode Ctrl-C、child crash、handoff EOF；parent保存/恢复termios，SIGINT/SIGTERM latch不default-exit、forward-once、5s后presentation-child-only kill/reap；不发送owner stop | PTY驱动clean exit0与crash；分别signal parent并验证唯一CHILD exit1、terminal恢复、child归零；前后status同一Running generation且Session/Runtime继续；最终显式`down`仍clean join | bundled artifact以PTY执行Escape clean detach、termios/alternate-screen restore、前后same-generation status与最终joined down；不替代Linux crash/signal/kill/reap evidence |
| bounded、redacted、无第二 owner | handoff/UI/transcript有固定 frame、text、queue/history upper bound；slow/oversize/invalid input fail-closed；无 secret/path/token/raw low-level error | 长输入、慢消费者、输出失败与 Secret/path/token canary；除 AgentService 接受的 Open/Submit/cancel 外，lifecycle/Deployment/Inspection state与 generation不变 | Ruff/Python/static bundle、2MiB bounded capture与Secret/path/environment canary；不覆盖real长输入、slow consumer或输出失败 |

当前 `tests/system/test_m5a_local_tui_attach.py` 已实现的 Linux exact-binary 场景覆盖 grammar/never-started、真实 Running r1→stale r2、一次 Echo、clean detach、SIGINT/SIGTERM 与五秒 child kill/reap、private 20/21/23/24/other 映射、bootstrap generation race、wrong-peer/root execution及最终 cleanup。它还没有覆盖 stopping pre-effect、一次 Status/一次 raw PXTL-to-EOF 的精确计数、pending request detach/re-attach 后的 capacity/conflict、Agent 与 Inspection channel 分别断连且另一 channel继续、real slow-Watch/backpressure、长输入/slow consumer或输出失败。focused Rust/Python机制测试与 shared PXTH golden不能冒充这些缺失的 system 场景；在同一 immutable exact-ref完整门禁与这些高价值矩阵收口前，M5a 只保持 `Implemented candidate`，不能标 `Validated` 或 `Completed`。

M5a 的明确 nonclaims 是：不提供 Agent transcript/history或detach后pending-request discovery，不提供 M3b public watch/JSONL/Inspection history/gap recovery，不提供 M4a/M4b/M5b Receipt/Evidence/logs，不声称 r2 之后持续刷新、心跳、当前健康或 Deployment 收敛，不提供自动 reconnect/retry/recovery，不提供 lifecycle/deploy/down/restart/rollback action，不提供 remote attach、OpsService、Web Console、生产 HA、新的 durable TUI owner、Rust presentation proxy或 replacement `ConsoleBridge`。当前 producer 只足以验证 r1 到 freshness stale r2；UI 显示 `unavailable` 只是 presentation-side连接状态，不是 Inspection source `Partitioned`。M5a 验证失败时可以在发布前整批撤回新 grammar/handoff，因为它不引入 persistent format或迁移；一旦公开发布，grammar 或语义变化必须走显式兼容 successor/deprecation，不能把 `chat` 或 `tui` 静默换义。

### M5b — TUI logs integration

M5b 只在 M5a 与 M4b 都完成后开始，M4a 的 current-Running PXMT snapshot 单独不能解锁它。生产者是 M4b 准入的 owner-issued Receipt/Evidence 与 bounded structured-log projection；消费者是 M4b 另行准入的 direct typed client 与既有 Python Textual presentation。M5b 不新增 public CLI grammar，不借 Rust local parent代理 log/domain traffic，不让 Python读取 raw log/store/Receipt path或 token，不让 TUI成为 retention、cursor、Evidence 或日志 owner，也不从日志文本、缺失记录、进程/transport状态推导成功或当前健康。

M5b 的字段、cursor、retention、truncation、Unavailable/Unknown/Uncertain 与 owner correlation 必须由 M4b 先冻结并验证；在此之前本 Program 不猜测 log schema、Evidence API 或 governance row，也不将 M4a JSON 直接塞进 TUI。M3b 是独立的 public Inspection watch 后续，不是 M5b 前置，也不能用 M5a 内部 Watch loop或 M5b logs view冒充完成。

### N0 — Node enroll/transfer contract

N0 只建立 operator 部署 target 的显式身份、信任、credential reference、staging root 与 bounded transfer contract。它不等于 Remote Agent enrollment，不创建 AgentSession，不复用 Agent transport，也不授权任意 shell。目标、host key/trust pin、artifact identity 与 transfer identity 必须显式；凭证值不进入配置、命令行、日志或 Receipt。

### N1 — push-only

`push` 的产品语义固定为 transfer-and-stage only：它可以验证 source digest、传输、原子发布 target staging artifact 并返回 TransferReceipt；绝不安装、切 active pointer、提交 DeploymentRevision、启动/停止/restart 任何 owner。远端已经存在同 digest 时可幂等 `changed = false`；断连、timeout 或 checksum 不完整不能报告成功。

### N2 — remote deploy

remote deploy 只消费 N1 已验证 staging Artifact，并复用已验证 D0b/D1 的 Artifact/DeploymentController/Runtime owner、状态和 Receipt 语义，不创建 Installation active pointer。传输成功与部署成功是两次独立 operation；SSH 可作为明确受限的 bootstrap/rescue 研究对象，不能成为 hidden production fallback 或任意 executor。控制端断连不证明远端未发生副作用，必须进入 Uncertain 并向真实 owner query/reconcile。

### O0 — OpsService 最后准入

O0 只有在 N2 后、ADR-0003 被 Accepted 或由后继决策替代、且至少两个独立真实客户端/operation consumer 证明 durable ControlRequest、幂等、watch/cancel 与 Uncertain reconcile 的共同需求时，才能实现最小 OpsService。它只拥有 operation record/journal，真实副作用仍由 typed owner 执行。准入条件不足时，O0 的正确结果是保持 deferred，并继续使用窄 owner CLI；不得创建占位 OpsService、第二写者或通用 workflow engine 来“完成计划”。

## Graph 决定

本 Program 直接执行 Accepted ADR-0005，不再把“是否复制 EAGOS Graph Engine”列为开放问题：

1. DeckTopology/DataLink、ServiceDependency 与 activation constraint 保持不可互换的 typed graph，各自由领域 owner 定义 validator、错误、生命周期与证据。
2. 只有两个独立真实生产消费者已经证明相同 pure algorithm 需求后，才允许按实际交集抽取 internal Graph Foundation；它不得包含 I/O、持久状态、async scheduler、retry、approval、Receipt、compensation 或 rollback。
3. D0a、D0b、R0/D1 与 N1–N2 使用 owner-specific、Receipt-backed 状态机。实现可以使用局部拓扑排序或确定性步骤表，但不能把部署正确性、恢复或权限委托给通用 graph executor。
4. O0 也不得隐藏 workflow/saga engine。未来若至少两个独立、持久、跨 owner workflow 证明共享语义，必须以新的证据和后继 ADR 重新准入；“步骤很多”或“UI 想画图”都不构成反例。

## Golden path 验收场景

### G0 本地闭环

1. `paraegox init --directory <absolute-directory> --json` 生成 private `paraegox.toml`，但不创建 `state`；再次运行 byte-identical 请求返回 `changed = false`，冲突不覆盖。
2. 对生成配置执行 M1 `config check` 与 `doctor --offline`，全程无 Secret、网络和 state mutation。
3. 执行 `paraegox deploy --local --config <absolute-paraegox.toml> --json`：唯一 compiled-in deterministic fixture 通过真实 Controller/Runtime 返回 point-in-time `active_ready`；相同请求、重复执行与并发 follower 不创建新 revision/apply，且 `changed = false`。这一 D0a 基线不等待 Artifact/Installation 路线。
4. 读取 M2a lifecycle status，再由 M3a Inspection snapshot 解释 freshness、stale、unknown、partial 或 reconcile-required，不能把 lifecycle、历史 deployment outcome 与当前健康混成一份状态。M3b public watch 是另行授权和验收的独立后续，不阻塞下一步。
5. 执行 `paraegox tui --config <absolute-paraegox.toml>`，一次性附着同一 Running generation 的 Agent conversation 与 Inspection：首屏只声明 initial r1，现有 producer 后续只证明 freshness stale r2；断连只显示 `unavailable`且不自动重连。退出 TUI 后 lifecycle status 仍是同一 Running generation，Session/Runtime 继续由原 owner持有。至此形成不等待 M3b/M4a/M4b 的快速可见本地 attach 基线。
6. M4a 通过 `paraegox receipt snapshot --config <absolute-paraegox.toml> --json` 读取同一 current Running generation 的 verified PXMT public-safe projection；`current_health_checked = false`，退出 owner 后不保留 cached success，这一步不完成 M4 也不解锁 TUI logs。
7. M4b 建立真实 D0a/PXAR9 PXEV producer、唯一 durable commit/query owner、failure-surviving typed read 与 bounded structured logs 后，M5b 再把这些投影接入同一 TUI；缺证据、截断或不可用保持 Unknown/Uncertain，不从日志合成当前健康。这个集成不改变 `paraegox tui` grammar。
8. 保留上述 workspace A 的 D0a/M3/M5 running state不变；以 A1 exact build/inspect 产生并零 mutation复验 `developer-local-echo-prefix-v1`，再为独立 workspace B 执行 `init` 与 ArtifactStore materialize/query。workspace B 到此只有配置和已物化 pair，不得有 lifecycle generation或Deployment desired；任何 tamper/unsupported 在 store/deployment/runtime副作用前拒绝。
9. 在 workspace B 以 D0b exact deploy作为第一次 lifecycle/deployment mutation，消费同一完整 object ref/materialization Receipt；DeploymentController提交Plan/Slice/revision，Runtime pre-effect reopen复验并由真实TUI/Agent观察 `prefix || prompt` 后到达 `ActiveReady`。Materialized/Committed/Applying/Ready各自由owner evidence证明；output loss以原operation query，任何缺证据保持Failed/Uncertain。workspace A的D0a/M3/M5 generation、revision和owner state必须保持不变。
10. 完成 same-state restart 与 new-Artifact replace，证明 joined stop、generation fencing、forward revision 与 no-double-active；再选择已知历史 `ArtifactObjectRefV1` 作为新 forward revision rollback 并重新验证 Ready，不使用 Installation active pointer。

### 远端闭环

1. 显式 enroll 一个部署 target，固定信任与 staging boundary，不创建 Remote Agent session。
2. push exact Artifact，只获得 TransferReceipt；服务状态、DeploymentController desired Artifact selection 与 DeploymentRevision 均不改变。
3. remote deploy 消费 staging Artifact 并获得独立 Artifact materialization、Deployment 与 Runtime owner Receipt references；它不创建 Installation active pointer。控制端失联时报告 Uncertain，再 query/reconcile。
4. 在真实两主机 exact-ref 场景执行远端 status、Evidence 与 rollback；不能用单机 mock、`--no-run`、一次 marker 或截图代替。

## 决策与变更规则

- 本 Program 可重排 Draft `kernel-foundation.md` 中的候选顺序，但不能覆盖 Accepted ADR。
- I0 与 D0a 都不触发 A0。A0 只由 ADR-0004 的真实 fixture 触发：多 DeckLock 统一发布/更新/卸载、installation-owned mutable state、或多隔离安装/多 Artifact 共同稳定 owner。一旦触发，相关副作用必须在最小后继 ADR Accepted 前以稳定 diagnostic 失败关闭。
- ADR-0011 的 Accepted 状态与有效 authorization receipt 已完成决策 gate；本 Program 进一步冻结A1/D0b intended-public contract。实现内部按A1 mechanism/evidence→fresh D0b real consumer排序，但Artifact crate/store与前四CLI不得独立登记、merge或发布；A1 producer、D0b Controller/Runtime/TUI consumer、六条命令、两个system harness与governance rows必须在同一immutable candidate ref共同通过admission/review/merge。决策记录、合同文本或A1-only evidence都不构成capability evidence；R0/D1继续依赖joint D0b gate。
- 如 M3a、M3b、M4a、M4b、M5a、M5b 或 N0–N2 需要完整 OpsService 语义，不能把 O0 前移；必须继续使用窄 typed owner seam，或先单独评审并接受 ADR-0003 后再修改本 Program。
- 新增公共命令、JSON 字段、persistent format、state mutation、Secret access 或网络行为必须先在本 Program 冻结对应 Step。尚未实现的 public surface 不得预登记；实现、真实 consumer、system test 与 `governance.toml` 登记必须在同一批次内提交。
- milestone 只有在列出的 evidence 可由评审者对 immutable exact ref 复现后才能标 Completed；代码存在、工作树存在、文档存在或某个下游模块引用它都不够。
- Mac 是唯一 writable source authority；本机不得运行 Cargo/rustc/rustfmt。Rust、完整治理与真实 system evidence 必须在 admitted Ubuntu/CI host 对同一 exact ref 执行，且不能把 Mac focused evidence升级为平台验收。
- Program 结束后，把稳定命令写入 guides/reference，把故障恢复写入 runbooks，把 claim-to-evidence 写入 testing；本文件保留交付历史、O0 准入结论与 Remote Agent 冻结/解冻结论。
