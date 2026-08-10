# Local Operator CLI/Ops Program

> 状态：Active
> Program ID：`local-operator-cli-ops`
> 授权日期：2026-08-10
> 最近重排：2026-08-10；先完成本地可见闭环，再进入远端 artifact 路线，OpsService 最后准入
> 授权来源：当前工作区用户明确要求优先完成 CLI、部署查看、Inspection/Ops 路线与可验证 TUI，并冻结新的 Remote Agent 扩张
> 当前 committed anchor：`main` 仍为 `4334a59af1656429f0401c0b780134c8871148e9`；当前窄 immutable code ref 为 `build/mac-source-snapshot-20260810-r356-local-cli-ops-test-stack-fmt`（`2b3d055f194c03cf28dd5f91960c05d5013125a4`）。它沿 r353 functional candidate、r354 Ubuntu rustfmt 与 r355/r356 test-only stack fix 递进，精确包含 M0/M1/M2a/I0、治理、CI 与正式文档路径，并排除 7 个未完成 Runtime S1 路径
> 当前最近动作：r356 已在固定 host-key 的 Ubuntu exact-ref worktree 通过 locked format/metadata、workspace all-targets check、Clippy `-D warnings`、workspace all-targets test compile、完整 governance 与 workspace doctest；`paraegox-local` 174/174 项在 non-root、默认线程栈下通过。相同 r356 binary 还以 non-root 真实进程执行了 `tests/system/test_m2_local_lifecycle_cli.py` 的 owner-contention 与完整 lifecycle 两个场景，均通过。M2a 因而已有 exact-ref Ubuntu candidate evidence；I0 的 sudo/ownership system matrix 与 macOS artifact workflow 仍未执行，因为当前 Ubuntu consumer 没有 non-root passwordless-sudo 身份，不能用 root 或跳过 fixture 冒充通过
> 当前 M3 动作：M3a snapshot 的 public grammar 与 JSON v1 合同已获授权；这里只冻结合同，不声称 CLI 已实现或通过平台验证。连续 watch 拆为 M3b，仍未登记 public grammar
> 当前 D0 动作：已将快速可见的 compiled-in deterministic deployment 拆为 D0a，并在本 Program 冻结 exact CLI/JSON 合同；它不依赖 external Artifact，不触发 ADR-0004 A0。external-artifact 路线拆为 A1/D0b，只有 [ADR-0011](../adr/ADR-0011-local-immutable-artifact-materialization-and-deployment-selection.md) 被用户显式接受后才可实现；其当前 `Proposed` 状态不是实现授权

## 一句话结果

ParaEGOX 当前先交付一条普通开发者能直接验证的本地路线：初始化私有工作区、检查配置、以 compiled-in `deterministic-echo-v1` 经真实 DeploymentController/Runtime 部署到 `ActiveReady`，再查看 Inspection、Evidence/日志与附着 TUI。只有 ADR-0011 显式 Accepted 后，才把 external Artifact build/inspect/materialize、external-artifact deploy、replace/restart 和 rollback 接入完整 golden path；之后再进入 Node enrollment、只传输不激活的 `push` 与远端部署，最后评审 OpsService。Remote Agent 新能力在整个 Program 中继续冻结。

## 为什么重排

现有代码已经积累 Kernel、Runtime、Deployment、Node、Fabric、Model、Agent、Inspection 和本地组合机制，但交付顺序长期由底层 tranche 推动。用户最先需要的是“拿到东西后能初始化、能部署、能看见、出错能解释、失败能回退”，而不是继续扩张 remote contract、session、connector、proxy 或通用编排抽象。

现有 TUI 入口只在 `chat` 启动链中做一次严格的 Inspection `Latest` 读取，再显示启动状态。这个 one-shot 切片的目的，是先证明 owner 边界、IPC、失败关闭和 UI 启动顺序，不是假装已经有持续运维控制台。当前路线以此为已有证据，先用不需要 Artifact/Installation 新 owner 的 D0a 补齐可重复的本地部署可见基线，再让 Inspection、Evidence/日志和 TUI 汇入；external Artifact 安装语义留给显式决策后的 D0b。

## 权威与所有权边界

- 本 Program 是当前交付优先级的权威，但只在 Accepted ADR 与 [`governance.toml`](../../governance.toml) 已登记边界内生效。
- [ADR-0005](../adr/ADR-0005-typed-domain-graphs-and-runtime-assembly-boundary.md) 已 **Accepted**：保留 Deck、ServiceDependency 与 activation constraint 各自的 typed graph 及必要纯算法，不建设通用 Graph Engine、Graph Store、持久 Graph Schema 或中央 workflow runtime。本 Program 不能重新打开该决定。
- [ADR-0003](../adr/ADR-0003-ops-service-operation-boundary.md) 继续保持 **Proposed**。本 Program 不接受它，也不授权提前实现完整 OpsService、federated Inspection、ConsoleGateway 或 Web Console；OpsService 只能位于本路线最后的 O0 准入门。
- [ADR-0004](../adr/ADR-0004-deck-workload-and-application-admission-boundary.md) 的 A0 是条件式 gate，只在出现以下真实 fixture 时触发：多个独立 DeckLock 需要统一 release/update/uninstall；installation-owned mutable state 需要跨 run/升级/重部署存续；或同一 release 需要多次隔离安装/多 Artifact 需要共同稳定安装 owner。在触发前不预建 Application、Installation、active pointer、uninstall 或 GC；触发后也不能把它们塞进 DeploymentController、Deck、CLI 或 `paraegox-local` 私有目录。
- [ADR-0011](../adr/ADR-0011-local-immutable-artifact-materialization-and-deployment-selection.md) 当前是 **Proposed**：它提议单 external Artifact 的 immutable materialization 与 Deployment selection 边界，不创建 Installation active pointer。它可以作为 A1/D0b 的候选决策，但在用户显式接受前不授权实现、治理登记或 capability 声明。
- 当前本地公共入口仍由 `DeveloperLocal composition root` 拥有。M1 离线 CLI 与 I0 本地初始化不创建新 owner，不取得 Secret、网络、服务生命周期或 domain durable-state mutation 权限；M2a 只在同一 composition root 内增加窄 lifecycle seam。
- M3a 仍位于同一 composition root：Inspection owner 继续独占 projection，lifecycle owner 只提供 config-commitment-bound 的当前 Running generation rendezvous，CLI 只做一次只读 PXIB/PXIQ/PXIP v2 `Latest`；三者都不取得彼此的 state 或 action authority。
- `init` 只生成开发者本地配置工作区；D0a 只确保一个 compiled-in、无 external bytes、无 installation-owned state 的 deterministic fixture 经现有 Controller/Runtime 到达 `ActiveReady`。两者都不是安装器、Installation owner 或 active-pointer owner，也都不触发 A0。
- Deployment desired state、Runtime apply、Node facts、Inspection projection、Evidence 和领域副作用继续由各自真实 owner 持有。未来 external Artifact bytes/materialization 只能在 ADR-0011 Accepted 后由其准入的 owner 持有；若真实 fixture 触发 A0，还必须先准入对应的最小 Application/Installation 或更窄 owner。CLI/TUI 只能调用 bounded seam，不能成为第二写者。
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

除上述 M1、I0、D0a、M3a snapshot 与下文 M2a grammar 外，external Artifact build/inspect/materialize、D0b、replace/restart、rollback、Inspection watch、Evidence/logs、TUI、push 和 remote deploy 的 exact command name、参数顺序与 JSON schema 尚未冻结。Program 可以先冻结合同；尚未实现的 public API/CLI 不得预登记到 `governance.toml`，必须在实现、真实 consumer 与 system test 同一批次内同步登记 producer、consumer、owner、权限、失败语义与兼容规则。

## Step DAG 与当前进度图

```text
M0 文档/治理 ─> M1 离线 CLI ─┬─> I0 本地 init ───────────┐
                         └─> M2a headless 生命周期 ─┬─> D0a compiled-in local deploy ─┐
                                                    └─> M3a snapshot ─> M3b watch ─┤
D0a + M3a ─> M4 Evidence/日志；M3b + M4 ─> M5 attach TUI
ADR-0011 Proposed ──用户显式 Accepted──> A1 external Artifact ─> D0b external-artifact deploy ─> R0 replace/restart ─> D1 rollback
ADR-0004 真实触发 fixture ─> A0 条件 gate ─> 最小 owner ADR Accepted 或稳定拒绝（当前 D0a 不触发）
D1 + M5 ─> G0 本地 golden path ─> N0 Node enroll/transfer ─> N1 push-only ─> N2 remote deploy ─> O0 OpsService 最后准入
```

依赖说明：I0 可在 M0/M1 收口后与 M2a exact-ref 收口并行，D0a 只等待 I0 + M2a，不等待 A0、ADR-0011 或 A1。M3a 合同可以先冻结，但实现与完成证据必须等待 M2a 的 immutable exact-ref gate；M3b 再消费已验证的 M3a。M4 同时等待真实 D0a point-in-time Receipt projection 与 M3a read-only projection，M5 同时等待 M3b 与 M4。A1 与 D0b 必须等待 ADR-0011 由用户显式 Accepted，D0b 再消费 D0a 已验证的本地路径与 A1；R0/D1 不得越过 D0b。A0 不是正常链的预置层，只在 ADR-0004 的真实 fixture 出现时截断相关副作用。只有 D1 与 M5 都完成，G0 完整本地 golden path 才成立。N0–N2 不得越过 G0，O0 永远最后。

| Step | 当前状态 | 依赖 | 用户可见结果 | 完成证据 |
| --- | --- | --- | --- | --- |
| M0 文档与治理权威 | Validated candidate（r356 Ubuntu complete-governance PASS） | 无 | 正式 docs 可版本化，只有 `docs/workbench` 保持本地；本 Program 成为当前路线 | candidate commit 追踪全部正式 docs；治理检查拒绝未追踪正式 docs 和已追踪 workbench；文档链接/状态检查通过 |
| M1 离线 CLI 首切片 | Active（r356 Ubuntu Rust/unit PASS；artifact/process smoke 待验） | M0 | 用户能查询版本、严格检查配置、离线诊断静态前置条件 | exact grammar 正负测试、稳定 JSON/exit code、Secret/network/state 零副作用测试；对应 public API 登记 |
| M2a 本地 headless 生命周期 | Validated candidate（r356 Ubuntu Rust、174/174 local 与 2/2 real-process 场景 PASS） | M1 | 用一份现有严格 chat 配置执行 `up/status/down`，三条操作共享一个 headless whole-local composition owner | exact grammar/JSON 正负测试；pre-state Secret 失败零副作用；并发同 generation 且 `changed` 与请求相关；private lock/record/same-user UDS、signal/joined shutdown、owned-socket 清理与 terminal record；同一 exact Ubuntu ref 的完整证据 |
| I0 本地 init | Active（r356 Ubuntu Rust/unit PASS；sudo ownership matrix 与 macOS artifact 待验） | M1；可与 M2a 收口并行 | 一条离线命令生成 private deterministic-echo config workspace，不创建 domain state | exact grammar/JSON 正负测试；byte-identical 幂等 `changed = false`；文件/权限/symlink/内容冲突不覆盖；Secret/owner/network/state 零副作用；public API 登记与 exact-ref 平台证据 |
| A0 条件式 Application/Installation gate | Not triggered（当前 D0a） | ADR-0004 真实触发 fixture | 只在 multi-Deck 统一发布/更新/卸载、installation-owned mutable state、多隔离安装或多 Artifact 稳定 owner 出现时准入最小 owner | 与触发条件对应的真实 fixture、owner/consumer/failure evidence，以及最小后继 ADR Accepted 或稳定拒绝；不预建 Application/Installation |
| ADR-0011 external-Artifact 边界 | Proposed（未授权实现） | 用户显式决策 | 决定单 external Artifact 的 immutable materialization、Deployment selection 与 Runtime reopen 边界 | 用户显式 Accepted 与有效决策记录；文档/代码存在不得替代决策 |
| A1 external Artifact build/inspect/materialize | Planned/Blocked | ADR-0011 用户显式 Accepted；若触发 A0 还需其最小 ADR Accepted | 用户能构建、只读 inspect 并由真实 owner 物化单个 immutable Artifact，不创建 Installation active pointer | reproducible bytes/digest、strict canonical manifest、tamper/unsupported-target pre-effect fail-closed、crash/idempotency/Receipt 与 zero-install inspect 证据 |
| D0a compiled-in local deploy | Authorized（contract only；未实现、未验证） | I0 + M2a | 用 exact CLI 把唯一 deterministic fixture 经现有 Controller/Runtime 到达 point-in-time `ActiveReady`；重复/并发 follower 不新增 revision/apply | exact grammar/strict JSON、真实 owner/receipt correlation、same-request/replay/concurrency、unsupported profile pre-effect reject、down race/supervisor crash/output failure/tamper/no-leak 的 focused/system/exact-ref 证据 |
| D0b external-artifact local deploy | Planned/Blocked（未登记 public grammar） | D0a + A1 + ADR-0011 用户显式 Accepted | 本地消费 exact immutable Artifact owner Receipt，由 Controller 选择 desired revision、Runtime 复验并到达 Ready，无 Installation active pointer | materialized/committed/activated/ready 不混同；crash/partial failure、同 operation 幂等、Uncertain query/reconcile、owner Receipt 关联与失败不破坏旧实例 |
| R0 replace/restart | Planned/Blocked | D0b | 显式替换新 Artifact；或在同 Artifact/state 上安全停止后重启，不透明自动 retry | forward DeploymentRevision、generation fencing、joined stop、SIGKILL/owner loss、stale generation、same-state restart、partial failure 与 no-double-active 证据 |
| D1 rollback | Planned/Blocked | D0b + R0 | 选择已知 `ArtifactObjectRefV1` 为新的前向 DeploymentRevision，并重新验证 Ready | target identity/compatibility、历史 Artifact 完整对象复验、rollback partial failure、Ready/Uncertain 与 Receipt correlation；不切 active pointer |
| M3a 本地 Inspection snapshot | Authorized（contract only；未实现、未验证） | M2a | 用户通过一条 exact read-only CLI 读取带 typed source/revision/freshness 的 PXIS v2 snapshot | exact grammar/JSON/exit/channel；安全 locator；一次 Latest、无 retry/mutation；fresh→stale 与 no-leak 的 focused/system/exact-ref 证据 |
| M3b 本地 Inspection watch | Planned/Deferred（未登记 public grammar） | M3a | 在另行授权后持续观察 projection 变化 | projection-aware cursor、NotModified/gap/reset、断连、backpressure、bounded reconnect 与 restart 证据；不得写 desired state |
| M4 Evidence 与日志诊断 | Planned | D0a + M3a | 用户从失败定位到 owner、Receipt/Evidence 和有界日志，而不是只看“启动失败” | Secret-free 输出、bounded retention/query、owner-issued Receipt correlation、缺证据时 Unknown/Uncertain |
| M5 可附着 TUI | Planned | M3b + M4 | TUI attach 已运行实例并展示状态、原因与证据；detach 后服务继续 | attach/detach、断连/恢复、慢消费者、终端恢复、无隐藏写操作的人工与自动化证据 |
| G0 本地 golden path | Derived gate，未满足 | D1 + M5 | 新用户可复现 init 到 rollback 的完整本地路线 | immutable exact-ref Mac/Linux guide run、claim-to-evidence、已知限制；不是独立实现 Step |
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

此处冻结 contract 并授权 M2a 候选实现；milestone 是否完成仍由可复现 evidence 决定。r356 immutable exact ref 已在 Ubuntu 绑定 locked Rust format/metadata/workspace check/Clippy/test compile、完整治理、workspace doctest、non-root 默认栈下的 `paraegox-local` 174/174，以及 `tests/system/test_m2_local_lifecycle_cli.py` 中 owner-contention 和完整 lifecycle 两个真实进程场景。后两项覆盖 pre-state Secret 失败零 lifecycle/domain state 副作用、并发 same-config 单 generation 与请求相关 `changed`、只读 status、joined down/terminal record、config drift 和 no-leak；由于 consumer 未安装 pytest，测试函数由 Python 3.11 直接加载执行，而不是经 pytest runner。macOS artifact workflow 仍只验证 help 可见性，不验证 `up/status/down` envelope、owner 或进程语义。

### I0 — 本地 init

生产者是 `paraegox-local` 的窄 filesystem adapter 与既有严格 chat decoder，消费者是新用户、开发自动化和 D0a guide。I0 只拥有“按固定模板原子创建 private config workspace”的一次性动作，不拥有其配置指向的 state，也不持有常驻 lifecycle。恢复只允许重新运行 exact 请求：byte-identical 完成态返回 `changed = false`；任何 uncertain 临时状态必须在发布前可安全清理或在下次请求中 fail-closed，不得覆盖既有对象。实现与测试必须同时冻结上述 grammar、JSON、mode/symlink/owner 检查和零副作用边界。

### A0 — 条件式 Application/Installation gate

A0 不是 D0a 的固定前置层，也不是“创建完整 Application 平台”。它只在 ADR-0004 的三类真实 fixture 出现时触发：多个独立 DeckLock 需要统一 release/update/uninstall；installation-owned mutable state 需要跨 DeckRun、升级或重部署存续；或同一 release 需要多次隔离安装/多 Artifact 需要共同稳定安装 owner。当前单个 compiled-in deterministic fixture 无 external bytes、无 installation-owned state、无多隔离安装，因此 I0 和 D0a 都不触发 A0。

一旦触发，必须在相关 build/materialization/deployment/data mutation 前提交与真实条件对应的 fixture、identity producer、独立 consumer、唯一 writer、crash/partial failure 与 retain/delete/GC 边界。合法出口只有接受一份定义实际最小 owner 的后继 ADR，或以稳定 diagnostic 拒绝该能力。不得用 CLI 命令名、目录、自由字符串 scope、plan 或预建 Application/Installation/active pointer 冒充 owner 证据。

### A1 — external Artifact build/inspect/materialize

A1 只能在 ADR-0011 被用户显式 Accepted 后开始；`Proposed` 文本不授权代码、public contract、package/store 或 `governance.toml` 登记。获准后，build 只从 exact source/build inputs 产生 immutable、可重复验证的 Artifact 与 canonical manifest；inspect 只读 bytes/metadata 且零物化/部署副作用；materialize 由决策准入的 owner 以 immutable object 和 owner Receipt 发布，不创建 Installation、active/current pointer、Deployment desired state 或 Runtime generation。篡改、未知字段、unsupported target、digest/pair mismatch 必须在任何 store/deployment/runtime 副作用前失败关闭。

### D0a — compiled-in local deploy

D0a 按上文 exact grammar 确保唯一 compiled-in `deterministic-echo-v1` 走过现有 DeveloperLocal composition、唯一 M2a supervisor、真实 DeploymentController 与 Runtime terminal `ActiveReady`。它不安装 bytes，不引入产品/Application Installation或Artifact owner，不依赖 ADR-0011，不触发 A0；既有 legacy `installation_id` 继续只是内部RuntimeHost/DeveloperFixture身份且不进入public JSON。成功输出只摘要 verified owner references/digests 与 point-in-time outcome，不是新的“总 Receipt”或当前健康声明。

实现批次必须同时包含 exact CLI/serializer、expected-generation-bound owner-private `DeployQuery`、真实 consumer、`tests/system/test_d0a_compiled_local_deploy_cli.py`、相关 CI 与该已实现 public surface 的 `governance.toml` 登记；不在此 contract-only 批次预登记尚不存在的 API。focused/system 证据至少覆盖 init→deploy、重复 deploy、up→deploy、并发 follower、down→up generation mismatch、config drift、provisioned pre-effect reject、query failure/down race/supervisor crash/output failure/tamper、`changed = run_up.changed && !model_agent_replayed` 与 no-leak，并且绑定同一 immutable exact Ubuntu ref 的完整门禁。

### D0b — external-artifact local deploy

D0b 必须等待 ADR-0011 用户显式 Accepted、A1 与 D0a；当前不冻结 public grammar/JSON，不授权实现。它未来只能消费严格验证的 immutable Artifact projection 与 materialization Receipt reference，由 DeploymentController 独占 desired Artifact 选择/DeploymentRevision，Runtime 经只读 port 重开并复验 exact object 后拥有 live generation。Materialized、Committed、Activated 与 Ready 不得互相推导；CLI 只聚合 owner references，不写 store、不创建 active pointer、不合成第二份 desired state 或 Receipt。若具体 fixture 出现 ADR-0004 触发条件，D0b 在副作用前还必须通过 A0。

### R0 — replace/restart

R0 吸收原 M2b 的 restart/异常恢复，并在 D0b 后增加真实 external Artifact replace；它不从 D0a 的 compiled-in fixture 抢跑。same-state restart 只有在前一 generation 已证明 joined/retired terminal 后才产生新 generation；replace 必须明确 old/new Artifact commitment、forward DeploymentRevision、state compatibility 与 no-double-active，不创建 Installation active pointer。owner 丢失、SIGKILL、陈旧 generation、超时或证据缺失不能以 PID 消失合成 stopped，也不能自动透明 replay；应返回 Failed/Unknown/Uncertain 并提供 bounded recover/reconcile 路径。

### D1 — rollback

rollback 依赖 D0b/R0，选择一个已知、仍物化且重新验证兼容的历史 `ArtifactObjectRefV1`，作为新的 forward DeploymentRevision 执行完整 commit/apply/Ready 流程，不倒退 revision/generation high-water，不产生 Installation revision，也不以目录、Git、backup rename 或 symlink/active pointer 切换冒充成功。历史 payload/manifest/state 不兼容、partial activation 或失联必须保留可查询 Receipt 并报告 Failed/Uncertain；只有新 forward revision 取得 exact `ActiveReady` 才能摘要为 RolledBack。

### M3a — Inspection snapshot

生产者是现有真实 owner facts 经 `paraegox-local` 当前 RunningStack 交给现有 Inspection projection/PXIB/PXIQ/PXIP v2 owner 的 immutable cache，消费者是公共 CLI/operator automation。lifecycle control 只增加一个 config-commitment-bound、same-uid/gid、transient locator action：composition 必须等 lifecycle control 与 Inspection endpoint 都 Ready 后，才把 owner-private PXIB path 交给该 Running generation；locator 只在 exact config/current Running generation 返回它，不持久化、不进入公共 JSON，也不返回 token。CLI 严格加载 PXIB 后直连 Inspection 并只发一次 `Latest`；lifecycle 不是 Inspection proxy，Inspection 仍是 projection owner，CLI 不是 cache、freshness、health 或 mutation owner。命令不得隐式调用 `up`、orphan recovery、restart 或 retry。

M3a 的后续实现写集限于本 Program，以及 `crates/paraegox-local/src/{config.rs,main.rs,error.rs,lifecycle.rs,composition.rs}`、一个新的 owner-coherent `inspection_client.rs`、`tests/system/test_m3_local_inspection_cli.py`、`.github/workflows/ci.yml`、仅用于 help/light-envelope smoke 的 `.github/workflows/macos-cli-artifact.yml`，以及与实现和真实 consumer 同批新增的 `governance.toml` public API 登记；不修改 `crates/paraegox-inspection/**`、ADR-0003、Python console/TUI、Ops/Remote Agent/Graph。focused tests 必须覆盖 exact grammar/serializer、typed coordinate 与 null/hex/enum、`2^53 - 1`、`2^53`、`u64::MAX` 的 canonical decimal string、Running/config/uid-gid locator、unsafe PXIB/file/socket/peer/token/correlation/trailing/timeout、一次 Latest/无 retry，以及 stale/unknown 不升级。exact-binary system evidence 必须覆盖真实 headless `up` 后 r1、真实 freshness 边界后的 stale r2、重复 r2 revision 不增长、provisioned Secret env 移除后读取、`down` 后无 cached success、path/config/root/extra-arg、Secret/path/token canary、stdout 一个 JSON+LF/stderr 空，并证明 lifecycle/domain 文件未被 snapshot 修改。Mac 不以 Rust/system smoke 替代 Ubuntu exact-ref gate。

本 Program 合同只把 M3a 置为 Authorized/contract；在 Rust 实现、真实 consumer、focused/system 结果与同一 immutable exact Ubuntu ref 的完整门禁出现前，不把尚不存在的 surface 预登记为 `registry.public_apis`，也不得标 Implemented、Validated 或 Completed。ADR-0003 继续 Proposed，M3a 不需要也不构成其 acceptance。

### M3b — Inspection watch

连续 watch 后置且尚无 public grammar/JSONL schema。后续准入必须把 cursor 定义为至少 `(snapshot/protocol version, projection_id, projection_revision)`，不能只用 revision；同 projection 的 NotModified、gap/cursor-ahead 必须显式 resync，新 projection_id 表示 restart reset。一个外层 coordinator 可以循环既有 one-shot Watch，但每次仍只有一个 in-flight exchange，stdout flush 前不得取下一项，不得建无界 queue；poll floor、backpressure、bounded reconnect budget、断连与 down/up 后的 projection reset 都必须有证据。endpoint 断连不等于 source `Partitioned`，restart 不得比较旧 clock/revision，也不得自动调用 `up`、restart 或 orphan recovery。现有 producer 目前只足以证明 initial snapshot 与一次 freshness 变 stale；在真实 refresh/recovery producer 与上述测试存在前，不冻结 watch command，也不声称持续运维能力。

### M4 — Evidence 与日志

生产者是领域 owner 的 Receipt/Evidence 与 bounded diagnostic adapters，消费者是 CLI/TUI/runbook。日志不替代 Receipt；probe、stdout、进程退出码、文件复制或 transport ACK 不能在证据不足时推导副作用成功。恢复路径要说明数据不可用、过期或被截断，而不是无限重试。

### M5 — 可附着 TUI

生产者是 Inspection/Evidence 客户端与现有 Textual console，消费者是本地 operator。第一版以查看和解释为主；任何写操作必须另行冻结权限与 owner API。TUI 崩溃或 detach 不得停止底层服务，也不得改变 Deployment reconciliation。

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
4. 读取 M2a lifecycle status，再由 M3a Inspection snapshot 与另行验收后的 M3b watch 解释 freshness、stale、unknown、partial 或 reconcile-required，不能把 lifecycle、历史 deployment outcome 与当前健康混成一份状态。
5. 查询 bounded、无 Secret 的日志/Evidence，并 attach TUI 查看 owner、状态、原因与 EvidenceRef；detach 后服务继续运行。至此形成快速可见本地基线，但不声称 external Artifact、replace 或 rollback。
6. ADR-0011 经用户显式 Accepted 后，构建一个 immutable ParaEGOX Artifact，以只读 inspect 获取 version/digest/manifest/target compatibility，再由 Artifact owner 物化；任何 tamper/unsupported 在 store/deployment/runtime 副作用前拒绝。
7. D0b 消费 exact immutable Artifact reference，由 DeploymentController 提交 forward revision、Runtime 复验并到达 `ActiveReady`；Materialized/Committed/Activated/Ready 各自只由 owner evidence 证明，任何缺失证据只能是 Failed/Uncertain。
8. 完成 same-state restart 与 new-Artifact replace，证明 joined stop、generation fencing、forward revision 与 no-double-active；再选择已知历史 `ArtifactObjectRefV1` 作为新 forward revision rollback 并重新验证 Ready，不使用 Installation active pointer。

### 远端闭环

1. 显式 enroll 一个部署 target，固定信任与 staging boundary，不创建 Remote Agent session。
2. push exact Artifact，只获得 TransferReceipt；服务状态、DeploymentController desired Artifact selection 与 DeploymentRevision 均不改变。
3. remote deploy 消费 staging Artifact 并获得独立 Artifact materialization、Deployment 与 Runtime owner Receipt references；它不创建 Installation active pointer。控制端失联时报告 Uncertain，再 query/reconcile。
4. 在真实两主机 exact-ref 场景执行远端 status、Evidence 与 rollback；不能用单机 mock、`--no-run`、一次 marker 或截图代替。

## 决策与变更规则

- 本 Program 可重排 Draft `kernel-foundation.md` 中的候选顺序，但不能覆盖 Accepted ADR。
- I0 与 D0a 都不触发 A0。A0 只由 ADR-0004 的真实 fixture 触发：多 DeckLock 统一发布/更新/卸载、installation-owned mutable state、或多隔离安装/多 Artifact 共同稳定 owner。一旦触发，相关副作用必须在最小后继 ADR Accepted 前以稳定 diagnostic 失败关闭。
- ADR-0011 的 `Proposed` 状态不授权 A1/D0b。只有用户显式 Accepted 并留下有效决策记录后，才能实现 external Artifact build/inspect/materialize 与 external-artifact deploy；R0/D1 继续依赖 D0b。
- 如 M3a–M5 或 N0–N2 需要完整 OpsService 语义，不能把 O0 前移；必须继续使用窄 typed owner seam，或先单独评审并接受 ADR-0003 后再修改本 Program。
- 新增公共命令、JSON 字段、persistent format、state mutation、Secret access 或网络行为必须先在本 Program 冻结对应 Step。尚未实现的 public surface 不得预登记；实现、真实 consumer、system test 与 `governance.toml` 登记必须在同一批次内提交。
- milestone 只有在列出的 evidence 可由评审者对 immutable exact ref 复现后才能标 Completed；代码存在、工作树存在、文档存在或某个下游模块引用它都不够。
- Mac 是唯一 writable source authority；本机不得运行 Cargo/rustc/rustfmt。Rust、完整治理与真实 system evidence 必须在 admitted Ubuntu/CI host 对同一 exact ref 执行，且不能把 Mac focused evidence升级为平台验收。
- Program 结束后，把稳定命令写入 guides/reference，把故障恢复写入 runbooks，把 claim-to-evidence 写入 testing；本文件保留交付历史、O0 准入结论与 Remote Agent 冻结/解冻结论。
