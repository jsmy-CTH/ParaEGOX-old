# DeveloperLocal Chat、Node schema v1/v2/v3 与 Deployment v1/v2 启动/退出

> 状态：当前工作树运行手册；本地文档，不进入 Git
> 适用范围：macOS/Linux DeveloperLocal，非生产

## 公开入口与配置权威

DeveloperLocal 当前有三个公开动作，但它们使用彼此独立的 strict config 和 owner chain：

```text
paraegox chat --config <absolute-paraegox.toml>
paraegox node --config <absolute-node.toml>
paraegox deployment --config <absolute-paraegox-deployment.toml>
```

`chat` 只表达“启动对话”。provider、model、本机 Fabric listener 和 durable state root 都由同一份
versioned TOML 配置决定；不存在 `chat deepseek-v1`、`chat openai-v1` 或 provider 专属启动命令，
也不能再用 `--model`、`--state-root`、`--fabric-listen` 覆盖配置。DeepSeek 只是当前用于真实模型验证的
可替换配置，不是 CLI 概念。

`node` 只表达“启动 Node 宿主基座”。Node schema v1 保留 G1 feature-only 路径；schema v2 增量启动
G2 host-side Runtime-control listener、Node-control ingress 与 observation bridge；schema v3 在同一控制
路径上额外固定 deterministic managed-Agent provider selection，并发布 PXEA v2。它不读取 Chat 配置，
不启动 Textual 或 Agent chat。Chat 与 Node 配置不得复用同一个 state root。

`deployment` 只表达“启动 Controller-side Deployment owner graph”。schema v1 消费 schema-v2 Node 的
PXEA v1；schema v2 消费 schema-v3 Node 的 PXEA v2，并在 base managed-ready 后依次完成 Fabric、Agent
stack 与 descriptor 三段 PXAG/PXAH。两种 artifact 都必须由独立 whole-file SHA-256 pin 固定，并配合
单独 provision 的 Controller/Authority signing seeds 与 Runtime/Node mTLS client credentials。它不读取
Chat 配置、不启动 Textual，也不构成 remote Agent conversation。Deployment、Node 与 Chat 的 owner
state 必须使用互不重叠的根目录。

Chat 配置保留一份无凭据示例：[paraegox.example.toml](../../configs/paraegox.example.toml)。示例选择
DeepSeek，并把 Secret 写成引用：

```toml
secret_ref = "env:DEEPSEEK_API_KEY"
```

它不是 API key 本身。不要把真实 key 写入 TOML、命令行、仓库、日志或 state root。

当前配置 Schema v1 的完整顶层字段为 `schema_version`、`state_root`、`fabric_listen` 和 `[model]`。
未知字段、未知 schema version、未知 provider、provider 不允许的字段或错误的 SecretRef 都会 fail
closed。配置文件必须是不超过 64 KiB 的普通文件，并通过绝对、lexically canonical 的路径传入。

当前编译内配置 profile 为：

| `model.provider` | `model.model` | `model.secret_ref` |
| --- | --- | --- |
| `deterministic-echo-v1` | 必须省略 | 必须省略 |
| `openai-responses-v1` | 必填 | 必须为 `env:OPENAI_API_KEY` |
| `deepseek-chat-completions-v1` | 必填；当前只接受 `deepseek-v4-flash` 或 `deepseek-v4-pro` | 必须为 `env:DEEPSEEK_API_KEY` |

这是一组精确、静态、无 fallback 的 DeveloperLocal profile，不是动态 plugin、自动模型路由或
provider discovery。

## Node schema v1/v2/v3 配置、credential 与启动

T1 的无凭据模板是
[paraegox-node.agent-bootstrap.example.toml](../../configs/paraegox-node.agent-bootstrap.example.toml)；
旧 schema-v2 模板仍是 [paraegox-node.example.toml](../../configs/paraegox-node.example.toml)。其中所有
16-byte hex 都是非秘密 reference，两个 32-byte Ed25519 值是 public verification key；配置不能表达
Controller/Authority private seed、Runtime response seed、PXNB/PXOB token、certificate 内容或 provider
Secret。两个模板都用 `[restricted_runtime_apply]` 选择 Runtime-control listener 的冻结 PXRP/PXCB pins，
并用 `[node_control]` 选择独立 Node-control listener 的 pins。schema-v3 模板还严格要求：

```toml
[managed_agent_bootstrap]
provider = "deterministic-echo-v1"
```

当前不存在第二个 provider 值。`route_config_carrier_digest` 与 provider ref/config digest 都由配置和固定
profile 派生，不是 TOML 输入；手工加入会因未知字段而被拒绝。若保留 schema v2，完整删除
`[managed_agent_bootstrap]`；若保留 G1，另把 `schema_version` 改为 `1` 并完整删除 `[node_control]`。
两处 `192.0.2.10` 都是文档地址，不能直接作为普通主机的 listener 使用。

先以最终运行 `paraegox` 的同一个非 root UID/GID 准备路径。下面是开发证书示例；`NODE_IP` 必须替换为
本机真实拥有的非 loopback IPv4，`NODE_CONFIG`、`NODE_STATE` 也必须保持绝对、lexically canonical：

```sh
PARAEGOX_BIN=/absolute/path/to/paraegox
NODE_CONFIG=/var/tmp/paraegox-node.toml
NODE_STATE=/var/tmp/paraegox-node
RUNTIME_IP=192.0.2.10
NODE_IP=192.0.2.10
RUNTIME_CN=paraegox-principal-05050505050505050505050505050505
NODE_CN=paraegox-principal-19191919191919191919191919191919

umask 077
install -d -m 0700 "$NODE_STATE"
install -d -m 0700 "$NODE_STATE/credentials"
cp configs/paraegox-node.example.toml "$NODE_CONFIG"
chmod 0600 "$NODE_CONFIG"
```

编辑 schema-v2/v3 `NODE_CONFIG`，同步完成四件事：

1. 把 `state_root` 和六条 credential path 改到 `NODE_STATE/credentials`；六条 path 必须互不相同；
2. 把 Runtime locator 改成 `tls/<RUNTIME_IP>:<unused-port>`，Node locator 改成
   `tls/<NODE_IP>:<another-unused-port>`；两条 route 也必须保持各自配置的精确值；
3. 保持 Runtime listener certificate 的 CN 与 `runtime_principal` 对应，Node listener certificate 的
   CN 与 `node_certificate_principal` 对应；示例分别对应 `RUNTIME_CN` 与 `NODE_CN`；
4. 只有外部 enrollment owner 可以成组替换 Controller/Authority verification key、credential/trust ref
   与 principal pin；不要为了本机启动自行生成或写入 Controller/Authority private seed。

若使用 schema v1，只保留第一个 locator 和 Runtime 的三条 credential path；Node listener 的 CA、
certificate、key 以及整个 `[node_control]` 表都必须省略，不能保留半套 schema-v2/v3 输入。schema v2
不得保留 `[managed_agent_bootstrap]`，schema v3 则不得省略它或改变唯一 provider 值。

开发环境可用下面的 OpenSSL 命令产生短期 CA 和 listener credential。CA private key 只属于外部
enrollment owner，不能放入 `NODE_STATE`、配置或仓库；示例把它留在单独的 owner-private 临时目录，使用
完毕后应转移到正式 enrollment storage 或安全销毁：

```sh
CA_WORK="$(mktemp -d /var/tmp/paraegox-node-ca.XXXXXX)"
chmod 0700 "$CA_WORK"
openssl genpkey -algorithm ED25519 -out "$CA_WORK/runtime-ca.key"
openssl req -x509 -new -key "$CA_WORK/runtime-ca.key" \
  -out "$NODE_STATE/credentials/root-ca.pem" -days 7 \
  -subj "/CN=paraegox-development-runtime-root" \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign"
openssl genpkey -algorithm ED25519 \
  -out "$NODE_STATE/credentials/runtime-key.pem"
openssl req -new -key "$NODE_STATE/credentials/runtime-key.pem" \
  -out "$CA_WORK/runtime.csr" -subj "/CN=$RUNTIME_CN"
printf 'subjectAltName=IP:%s\nextendedKeyUsage=serverAuth\nbasicConstraints=critical,CA:FALSE\n' \
  "$RUNTIME_IP" > "$CA_WORK/runtime.ext"
openssl x509 -req -in "$CA_WORK/runtime.csr" \
  -CA "$NODE_STATE/credentials/root-ca.pem" -CAkey "$CA_WORK/runtime-ca.key" \
  -CAcreateserial -out "$NODE_STATE/credentials/runtime.pem" -days 7 \
  -extfile "$CA_WORK/runtime.ext"
openssl genpkey -algorithm ED25519 -out "$CA_WORK/node-ca.key"
openssl req -x509 -new -key "$CA_WORK/node-ca.key" \
  -out "$NODE_STATE/credentials/node-root-ca.pem" -days 7 \
  -subj "/CN=paraegox-development-node-root" \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign"
openssl genpkey -algorithm ED25519 \
  -out "$NODE_STATE/credentials/node-key.pem"
openssl req -new -key "$NODE_STATE/credentials/node-key.pem" \
  -out "$CA_WORK/node.csr" -subj "/CN=$NODE_CN"
printf 'subjectAltName=IP:%s\nextendedKeyUsage=serverAuth\nbasicConstraints=critical,CA:FALSE\n' \
  "$NODE_IP" > "$CA_WORK/node.ext"
openssl x509 -req -in "$CA_WORK/node.csr" \
  -CA "$NODE_STATE/credentials/node-root-ca.pem" -CAkey "$CA_WORK/node-ca.key" \
  -CAcreateserial -out "$NODE_STATE/credentials/node.pem" -days 7 \
  -extfile "$CA_WORK/node.ext"
chmod 0644 "$NODE_STATE/credentials/root-ca.pem" \
  "$NODE_STATE/credentials/runtime.pem" \
  "$NODE_STATE/credentials/node-root-ca.pem" \
  "$NODE_STATE/credentials/node.pem"
chmod 0600 "$NODE_STATE/credentials/runtime-key.pem" \
  "$NODE_STATE/credentials/node-key.pem"
```

这些命令只生成 Ubuntu/Node 进程消费的两套 listener credential。公开 Deployment connector 所需的两套
client certificate/private key 仍属于 Controller enrollment owner，不由 Node config 或这组 listener
命令生成；它们必须按 PXEA 中的 Controller principal、credential/trust pins 与两条 listener 分别
provision。只有同时具备 PXEA 独立 pin 和正确 client credentials，Controller 才能尝试 application
exchange；Node host ready 本身仍不证明该连接发生过。

启动前再次确认：state root 与精确的 `credentials` child 都是当前 euid/egid 的 mode `0700` 真实目录；
schema v2/v3 的六个文件（schema v1 为三个）是互不相同、非 symlink、link count 1 的普通文件；listener
key 精确为当前 euid/egid mode `0600`；CA/certificate 不允许 group/other 写。然后以非 root 运行：

```sh
test "$(id -u)" -ne 0
test "$(id -g)" -ne 0
"$PARAEGOX_BIN" node --config "$NODE_CONFIG"
```

只有输出 `paraegox: node ready` 才表示所选 schema 的本地 owner 已启动，且 parent 已完成 authenticated
PXNQ/PXNS Latest equality check。schema v1 此时只有 fixed-rejection Runtime listener 与 feature-only
NodeDaemon；schema v2/v3 此时还包括 PXCC/PXDR Runtime-control listener、observation-capable NodeDaemon
和 authenticated PXNR/PXNE/PXNS/PXNA Node-control listener。schema v3 还在 marker 前原子发布 PXEA v2，
但它本身不启动 Fabric 或 Agent。三种情况下都不能由 marker 推导 Controller 已连接、PXFB cutover 已
完成、Runtime observation 已发布、PXAR 已提交、Agent chat 可用或 distributed readiness。

## Deployment schema v1/v2、PXEA handoff 与启动

T1 的无凭据模板是
[paraegox-deployment.agent-bootstrap.example.toml](../../configs/paraegox-deployment.agent-bootstrap.example.toml)；
旧 schema-v1 模板仍是
[paraegox-deployment.example.toml](../../configs/paraegox-deployment.example.toml)。公开 grammar 只有：

```text
paraegox deployment --config <absolute-paraegox-deployment.toml>
```

不存在 `deployment controller`、endpoint/route override flag 或交互式 first-use trust。两个 schema 的
共同顶层字段是：

- `schema_version`、`state_root`；
- `enrollment_artifact_file` 与独立获得的小写 64-hex `enrollment_artifact_sha256`；
- `controller_signing_seed_file`、`authority_signing_seed_file`；
- `authority_state_directory`、`authority_socket_path`；
- `[runtime_connector]` 与 `[node_connector]`，每个表都恰含
  `root_ca_certificate_file`、`client_certificate_file`、`client_private_key_file`。

schema v1 必须到此结束。schema v2 还必须恰有：

```toml
[managed_agent_bootstrap]
fabric_service_id = "<nonzero-16-byte-hex>"
agent_service_id = "<different-nonzero-16-byte-hex>"
fabric_listen = "tcp/127.0.0.1:<1..65535>"
limits_profile = "developer-agent-bootstrap-v1"
```

配置不接受 endpoint、route、target、principal、manifest、trust ref、credential ref 或 public key
override；这些跨主机语义只来自 PXEA。schema v2 的 desired service IDs/listen/limits 只来自 Deployment
配置，provider selection 只来自 PXEA v2，两者不得互相覆盖。未知字段、重复字段、schema/字段组合不是
精确 v1 或 v2、相对/非 canonical path、路径重叠或 alias 都会 fail closed。TOML 只保存 path、public
SHA-256 pin 与 schema-v2 desired，不保存 seed、certificate/key bytes、bearer token、challenge 或
Secret value。

### 1. 从匹配的 Node schema 取得并独立 pin PXEA

`paraegox node` 在 Runtime/Node durable bootstrap 已获证明后，按 schema 原子发布并严格 reopen：

```text
schema v2 → <NODE_STATE>/node/enrollment-v1.pxea
schema v3 → <NODE_STATE>/node/enrollment-v2.pxea
```

PXEA v1/v2 都是 canonical、public-safe、Runtime-attested handoff。v2 保留 v1 的 immutable Runtime
manifest、Runtime response public key、tenure verification key、Runtime/Node endpoint/route/transport/
identity refs、Node incarnation/registration 与 observation endpoint，并额外提交 managed-Agent provider
profile/ref/config digest；两者都不携带 PXNB/PXOB token、signing seed、private-key bytes/path 或 provider
Secret。Deployment schema v1 只接受 PXEA v1，schema v2 只接受 PXEA v2。先在 Node owner 侧计算匹配
完整文件的 SHA-256，例如 T1：

```sh
NODE_PXEA="$NODE_STATE/node/enrollment-v2.pxea"
test -f "$NODE_PXEA"
sha256sum "$NODE_PXEA"
```

把 PXEA 文件经认证传输通道复制到 Controller 侧，把其 64 位小写 SHA-256 经独立可信通道交给
Deployment 配置 owner；不能从同一份未认证 payload 自报 digest。macOS 可用 `shasum -a 256` 复核收到的
完整文件。Deployment 在解析任何 attacker-controlled length、signature 或 semantic field 之前先比较这份
whole-file pin；随后验证 Runtime signature，并把本地 Controller/Authority seeds 导出的 public keys 与
artifact pins 交叉核对。PXEA signature 只证明 Runtime identity continuity，不替代首次 pin。

### 2. Provision Controller authority 与两套 connector identity

由同一个 enrollment owner 准备以下输入，不能为“先跑起来”而现场生成互不相干的新 key：

- 精确 32-byte raw Controller Ed25519 signing seed；其 public key 必须匹配 PXEA 的 Controller request
  authority；
- 另一份精确 32-byte raw tenure-Authority Ed25519 signing seed；两把 key 必须不同，且 Authority public
  key 必须匹配 PXEA tenure pin；
- Runtime connector 的 CA、Controller client certificate 和 client private key；
- Node connector 的另一组 CA、Controller client certificate 和 client private key。

两组 client certificate 必须分别满足 PXEA 固定的 listener trust、credential ref 与 Controller
principal；不能把 Node listener key、CA private key 或服务端 certificate 搬到 Controller 配置中代替
client identity。

以最终运行 `paraegox deployment` 的同一个非 root UID/GID 预建目录。下面以 macOS canonical
`/private/tmp` 为例；Linux 可以换成同样绝对、canonical 且不重叠的私有路径：

```sh
DEPLOY_ROOT=/private/tmp/paraegox-deployment-controller
AUTHORITY_ROOT=/private/tmp/paraegox-deployment-authority-state
AUTHORITY_SOCKET_ROOT=/private/tmp/paraegox-deployment-authority-socket
DEPLOY_INPUT=/private/tmp/paraegox-deployment-input
DEPLOY_SECRETS=/private/tmp/paraegox-deployment-secrets
DEPLOY_CREDENTIALS=/private/tmp/paraegox-deployment-credentials

umask 077
install -d -m 0700 "$DEPLOY_ROOT" "$AUTHORITY_ROOT" "$DEPLOY_INPUT" \
  "$DEPLOY_SECRETS" "$DEPLOY_CREDENTIALS"
install -d -m 02750 "$AUTHORITY_SOCKET_ROOT"
```

`state_root`、`authority_state_directory` 与 Authority socket parent 必须是三个不同、互不包含的真实
directory；前两者及九个 input file 的精确 parent 为当前 euid/egid、mode `0700`，socket parent 精确为
当前 euid/egid、mode `02750`。PXEA、两份 CA 与两份 client certificate 必须是 owner-readable、非
executable 且 group/other 不可写的普通单-link 文件；两份 raw seed 和两份 client private key 必须是
精确 mode `0600`。九个 file path 及 inode 必须互不相同，整条 path chain 不得含 symlink。

把模板复制到绝对普通文件路径，替换所有 placeholder path 和 PXEA SHA-256；不要修改 PXEA 内容或手工
编辑 Controller journal：

```sh
DEPLOY_CONFIG=/private/tmp/paraegox-deployment.toml
cp configs/paraegox-deployment.agent-bootstrap.example.toml "$DEPLOY_CONFIG"
chmod 0600 "$DEPLOY_CONFIG"
```

上例选择 T1 schema v2；若运行 predecessor schema v1，改用
`configs/paraegox-deployment.example.toml`，并配套 PXEA v1。

### 3. 启动、Ready 与非 Ready

先保持与 Deployment schema 匹配的 Node process 正常运行（v1→Node v2，v2→Node v3）并让两条 TLS
listener 对 Controller 可达，再以配置文件与 owner files 的同一非 root 用户启动：

```sh
PARAEGOX_BIN=/absolute/path/to/native/paraegox
test "$(id -u)" -ne 0
test "$(id -g)" -ne 0
"$PARAEGOX_BIN" deployment --config "$DEPLOY_CONFIG"
```

schema v1 只有 stdout 出现精确并已 flush 的 `paraegox: deployment ready`，才表示 predecessor facade
已返回 `Ready`：remote connector/cutover state 已持久化，精确 PXFR managed-serving terminal 已 durable
`ResponseDurable`，且 fresh post-PXFR PXDR Describe 已完成 transport/correlation/signature/succession
校验、报告 `ManagedReady` 并持久提交。

schema v2 只输出精确并已 flush 的 `paraegox: deployment agent bootstrap ready`。除上述 base
managed-ready 之外，它还要求 PXFJ v7 的三个有序 PXAG/PXAH slot 都是 exact `ReceiptDurable`：

1. Fabric apply：PXAG 携带逐字不变 PXAR v6，PXAH 携带已验证 PXFT ActiveReady；
2. Agent-stack apply：PXAG 携带逐字不变 PXAR v7，PXAH 携带已验证 PXST ActiveReady；
3. descriptor Describe：PXAH 携带以该精确 PXST digest 和当前 Fabric/Agent generation 为根的 bounded
   opaque PXAP descriptor。

Node 的 `paraegox: node ready`、TLS handshake、单独观察到 ManagedReady、旧 Deployment marker 或 unit
test 通过都不能替代 schema-v2 marker。RequestDurable restart 只使用已提交 request；terminal same-state
Resume 只重验 exact historical request/receipt，不生成替代请求、receipt 或 descriptor。

PXFB delivery uncertainty 不会重放或合成 PXFR；publish uncertainty、需要明确 reconciliation 的持久
状态、非法 response、owner 提前退出或缺少 durable PXFR 都不会输出 Ready。公开 process 会 joined 关闭
它已启动的 Controller/connector/Authority owners，然后以稳定 `PXLC-DEPLOYMENT-*` 错误非零退出。当前
public facade 每次启动只执行一次有界、无重试的 remote attempt；它不是 daemon 或 continuous
reconciler。检测到已有 Controller/successor state 时只走 strict Resume，绝不会隐式清空后 Fresh。

Ubuntu 已验证精确 r130 commit `92923eef016e6ce060c32113ad3cf5e59ee8520c`：workspace format、Local
all-target check 与 warnings-denied Clippy、完整 non-root Local 138/138（49.57 秒）、workspace all-target
check 与 warnings-denied Clippy 均通过；所有 workspace all-target test executable 还在 `--no-run` 下完成
编译和链接。`--no-run` 不是完整 workspace tests 已执行的证据。完整 Deployment suite 也以 `nobody`、
link count 1 binary、16 MiB stack、单 test thread 运行并以 393/393（172.97 秒）通过。

r130 精确预编译 public binary 的 T1 process harness 在**同一 Ubuntu 主机**、同一个 `nobody` UID/GID
与真实 non-loopback mTLS 下以 1/1 通过，覆盖：

- fresh schema-v3 Node Ready 并发布 PXEA v2；
- 独立 fresh Deployment state 的错误 whole-file SHA-256 在任何成功路径前 fail closed；
- schema-v2 Fresh Deployment 完成 Fabric→Agent→descriptor 并输出新 marker，SIGTERM joined exit 0；
- Node 与 Deployment 分别用原 state/config Resume 到同一 Ready，SIGTERM joined exit 0；
- Node 停止后，独立 fresh correct-config Deployment 稳定非零、无任一 Deployment Ready marker，且
  Authority socket 清理；
- fixture logs 不含 seed 或 TLS private-key bytes。

pytest parent shell/main stack 是默认 8192 KiB；harness 仍对全部子进程设置既有的 16 MiB
`RUST_MIN_STACK`，r130 production 自身在具名、有界 16 MiB Deployment executor thread 上承载 root
future。不要把该结果写成“未设置 `RUST_MIN_STACK`”。

descriptor 只是 bootstrap evidence，不是可使用的 Agent capability。该 smoke 没有验证 descriptor
access/authorization、Agent session、Agent data plane、Echo/conversation、reconnect、remote TUI、
distributed/two-host、provider mismatch 或 Authority-owner failure。下一阶段 T2 是 asymmetric remote
Agent data plane + Echo；reconnect/TUI 继续后置。

## Textual 子进程前提

当前展示层是 Python Textual。公开入口仍只有 `paraegox chat --config ...`；Rust parent 在后端私有
Runtime/Inspection IPC ready 后启动内部 `paraegox-console`，不会让操作者直接调用它，也没有另一套
frontend 或 transport fallback。开发工作树启动前先安装并激活锁定的 Python 环境，确保 child
executable 可以从 `PATH` 解析：

```zsh
uv sync --locked
source .venv/bin/activate
command -v paraegox-console
```

该内部 child 只通过 Python typed `AgentConversationClient` 消费 Runtime Agent bootstrap；它不打开 raw
Zenoh、不读取模型配置或 API key，也不取得 AgentSession journal、retry/reconcile 权威。这不是把旧的
全能 ConsoleBridge 换名复活。另一个 strict typed Inspection client 会验证 owner-private PXIB v2
bootstrap，并在 Textual App 创建前执行一次无重试 PXIQ Latest，严格关联 PXIP 并解码完整
PXIS v2 snapshot。读取失败时 UI 不启动；成功时只显示三行只读 startup status。这不是
watch、retry、cache、background refresh、持续监控、Ops 或 federated Inspection。

在 macOS CI bundle 中，`paraegox`、同级 `paraegox-console` 和 `python/` vendored packages 必须保持
在同一目录结构，并且宿主 `PATH` 必须提供外部 Python 3.11 或更高版本的 `python3`。
该 bundle 不内置 Python runtime。

## 使用 DeepSeek 启动

先复制或直接编辑仓库中的唯一示例，至少确认 `state_root` 和 `fabric_listen` 没有与旧进程冲突。
macOS 必须使用 canonical `/private/tmp/...`，不能使用指向它的 `/tmp/...` symlink；Linux 使用满足
同样绝对、canonical、非根目录约束的私有路径。首次启动和以后每次重启必须保持相同的非 root
有效 UID/GID。

在终端中交互式读入 Secret，再把配置文件的绝对路径交给唯一公开入口：

```zsh
read -s 'DEEPSEEK_API_KEY?DeepSeek API key: '; echo
export DEEPSEEK_API_KEY
CONFIG_PATH="$(pwd)/configs/paraegox.example.toml"
PARAEGOX_BIN=/absolute/path/to/native/paraegox
test -x "$PARAEGOX_BIN"
"$PARAEGOX_BIN" chat --config "$CONFIG_PATH"
unset DEEPSEEK_API_KEY
```

`PARAEGOX_BIN` 必须指向指定服务器或 GitHub CI 生成并下载到 Mac 的原生构建 artifact。当前工作流禁止
在 Mac 运行 Cargo；Mac 是源码 authority，但 Rust 编译与验证只在服务器或 CI 执行。

启动器按配置选择 DeepSeek adapter，并只在 Secret resolver 边界读取 `DEEPSEEK_API_KEY`；TOML
保存的是固定引用而非 Secret value。Textual child 环境会移除 `OPENAI_API_KEY` 和
`DEEPSEEK_API_KEY`。当前 DeepSeek adapter 固定官方 HTTPS Chat Completions endpoint、关闭 thinking、
不流式、无 redirect/retry/proxy fallback，并对 request/response/output/timeout 做有界校验。

历史源码快照 r22（`ff2d8109`）已取得以下精确证据：

- Ubuntu 上 workspace format、locked metadata 和 locked all-targets check 通过；
- Inspection tests 39/39 通过；
- DeveloperLocal tests 在非 root 身份下 89/89 通过；
- Deployment tests 在非 root 身份下 364/364 通过；
- 原生 Intel macOS 运行已到达 typed Inspection markers、Runtime ready 和真实 Textual→Runtime
  Echo terminal。

r22 workspace Clippy 因约 30 个历史结构 lint 未通过；修正已进入后续源码快照，但 r29 macOS
artifact workflow 没有运行 workspace Clippy，因此这里不把它冒充为 fresh Clippy pass。

原生 Intel macOS r29 commit `944ce332`、run `31238285076` 已通过 locked Textual tests 和完整
governance checker，构建并验证公开 native CLI，组装可搬移 bundle，并真实走通 PTY 下的 typed
Inspection markers、Runtime ready、Textual→Runtime Echo、priority Ctrl-C、terminal restoration 和
父进程 joined shutdown；bundle checksum、executable mode、archive 和 artifact upload 也已通过。
这完成了 Textual replacement gate，旧 Rust reference frontend 已在本退役批删除。真实 credentialed
DeepSeek smoke 仍尚未运行，因此不能把 DeepSeek 路径描述为外部验证通过或 production ready。

Ubuntu r33 commit `7618f6a51c5eb5731874d2cdf3231603e3a824f7` 是 G1 Node 当前精确证据：workspace
format、locked metadata、workspace all-target check 与 warnings-denied workspace all-target Clippy
通过；9 个 focused Local filter 各运行 1 test 并通过，3 个非 root Runtime split-trust/provisioning filter
各运行 1 test 并通过，完整 `paraegox-local` unit binary 以非 root 身份 98/98 通过。

真实 smoke 的 state root 为 `/var/tmp/paraegox-r33-node-smoke`。ready 时 parent PID 18602 只有一个 child
PID 18765；listener 为 `172.17.0.2:17448`、`/tmp/pxl-.../r.sock` 和
`/tmp/pxl-.../node/n.sock`。state root 只出现 `credentials`、`developer-node-identity-v1`/PXNI、
`rt/runtime.lock` + snapshot、`node/bootstrap`/PXNB 和 `node/store`/PXND，没有 Authority、Controller、
Fabric、Model、Agent、Inspection 或 Textual owner state。SIGTERM 返回 0；同配置 restart 再次 ready 且
PXNI/PXNB SHA-256 不变；SIGINT 返回 0。强制终止唯一 child 后 parent 返回 1 并报告
`PXLC-NODE-CHILD`；root 启动返回 1 并报告 `PXLC-EXECUTION-IDENTITY`。两次拒绝后 PXNI/PXNB hash
仍不变且 TCP port 可重新 bind。

host-side G2 的 validation ref 为 r51 `b1d1206d2187b85d335ae352c226274d8e9d5827`。Ubuntu 已实际通过 workspace
format、公开 help focused test 1/1、完整 governance checker、workspace all-target check、warnings-denied
workspace all-target Clippy，以及完整 non-root `paraegox-local` suite 111/111。所有 workspace all-target
test executable 也已通过 `--no-run` 完成编译和链接；workspace doc tests 通过，其中 Fabric 2 个、Kernel
1 个、runtime-contracts 1 个 compile-fail doctest，其余 crate 为 0。完整 workspace test suite 没有在
这组结果中执行，不能由 `--no-run`、check 或 Clippy 反推。

PXQR authentication nonce 与精确 Node observation challenge 绑定修复后，对应 Deployment source/binary
lineage 的完整 non-root Deployment suite 374/374 也通过。此后直到 r51 没有修改 Deployment 源码，但交接
没有保留该 invocation 的精确 immutable ref；因此它不是“r51 实跑 Deployment 374/374”的证据。

r48 真实 non-root schema-v2 process smoke 已验证：fresh start 到达 Ready，进程树严格为 parent + 1 个
hidden Node child；TLS listener 为 `172.17.0.2:28448`/`:28449`，UDS 为 Runtime `r.sock`、management
`n.sock` 与 observation `o.sock`，state 只包含 credentials、PXNI、PXNB、PXND 与 Runtime owner state，
stderr 为空。携带已配置 Controller client certificate 时两条 TLS handshake 均成功，不带 client
certificate 时两端均拒绝。SIGTERM exit 0 并清理；同 state restart 再次 Ready/exit 0，PXNI/PXNB/PXND
digest 稳定。强杀 Node child 后 parent exit 1 + `PXLC-NODE-CHILD`，listener、PXOB 与进程全部清理；同
state 随后再次 Ready，SIGINT exit 0 并清理。root negative 为 exit 1 +
`PXLC-EXECUTION-IDENTITY`，state hash 不变。

这组 smoke 仍没有发送或验证完整 PXCC/PXNR application exchange，不是 PXFB cutover、双机 Controller
sequence、remote Agent chat 或 reconnect 证据。上面的证书命令与 Ready marker 也不得被解释成这些能力。

## 离线 Echo 配置

需要先验证本机 owner 链而不访问外部模型时，把同一份配置中的 `[model]` 改为：

```toml
[model]
provider = "deterministic-echo-v1"
```

并使用独立 `state_root`。Echo 返回 `echo: <input>`，只证明 Authority→Deployment→Runtime→
Runtime-managed Zenoh Fabric→ModelService→AgentService→内部 Textual child 的 DeveloperLocal 链路；
它不证明外部模型可达，也不代表真实 Agent 推理。

同一个 state root 只能属于一个精确 provider/profile/config/adapter binding。更换 provider、model
或 adapter binding 会 fail closed；请使用新的 state root，不要手工修改 identity manifest 或 journal。

## 正常退出和再次启动

按 `Esc` 或 `Ctrl-C` 退出。启动器会按 Textual child/Agent IPC → Runtime → NodeDaemon → Authority
的 owner 顺序关闭并等待；Runtime 内部按 Agent→Model→Fabric 停止服务。正常退出保留 durable Active
desired state；使用同一配置和 state root 再次启动会走真实恢复，不会重新伪造状态。

正常退出不提交 `EmptyDeactivate`。该动作是不可逆的永久退役控制操作，不是 TUI 的关闭键。

Node 可用 `Ctrl-C` 或向 parent 发送 SIGTERM 退出。schema v1 的 parent 先 joined NodeDaemon child，再
joined Runtime；schema v2/v3 按 Node-control endpoint/worker → NodeDaemon → Runtime 顺序 joined。正常退出
返回 0，释放 TLS/UDS listener，并保留 PXNI、PXNB、Runtime snapshot 与 PXND 供同配置严格 restart；
schema v2/v3 的临时 PXOB 由 child lifecycle owner 清理。child 非预期退出不是成功关机：parent 必须以
`PXLC-NODE-CHILD` 非零退出并仍执行 cleanup；schema-v2/v3 Node-control worker 提前退出同样使整条 Node
host composition fail closed。

Deployment 可用 `Ctrl-C` 或 SIGTERM 退出；它先停止并 joined Node/Runtime connector 与所持 durable store
owner，最后 joined Authority。正常退出保留 Controller/successor/Authority state，下一次同配置只走 strict
Resume。owner 在 signal 前退出、joined shutdown 失败或输出 Ready 失败都不是成功退出，且不会补写 Ready。

## 常见拒绝

- `PXLC-CONFIG-PATH-*`：必须传入一个绝对、lexically canonical 的普通 TOML 文件路径。
- `PXLC-CONFIG-DOCUMENT-INVALID`：TOML 无法 strict decode，包含未知/重复字段或不是 UTF-8。
- `PXLC-CONFIG-SCHEMA-UNSUPPORTED`：Chat version 不是 `1`、Deployment version 不在 `1..=2`，或 Node
  version 不在 `1..=3`；已知 version 与可选表不匹配则分别进入对应的 Deployment/Node config invalid。
- `PXLC-CONFIG-PROVIDER-*`：provider 未知，或 model/SecretRef 与所选 provider 不匹配。
- `PXLC-PROVIDER-SECRET`：配置引用的环境 Secret 缺失、为空、过长或包含非法字符；失败发生在任何
  state/owner 创建之前。
- `state root ... invalid`：使用绝对、canonical、非 `/` 路径；macOS 不要写 `/tmp`。
- `Fabric ... invalid`：只接受 `tcp/127.0.0.1:<1..65535>`，端口不能有前导零。
- 端口已占用：修改配置使用另一个本机端口，或停止占用该端口的旧进程。
- identity manifest 拒绝：state root 已绑定另一 profile/model/config/adapter；使用新的 state root。
- state 目录、父目录、配置文件或 identity manifest 是 symlink：使用新的真实路径；不要手工修 journal。
- `PXLC-NODE-CONFIG-INVALID`：Node ref/public key 不合法或别名、restricted transport pin 不一致、TLS
  path 不是绝对 canonical 普通文件路径，或配置混入 private seed/未知字段。
- `PXLC-NODE-CREDENTIAL-FILES`：state root/`credentials` owner 或 mode 不符，所选 schema 的三条或
  六条 TLS path 不在同一精确目录、互相重复、symlink/hardlink/可被非 owner 替换，private key 不是
  精确 `0600`，或 certificate/CA 可被 group/other 写。
- `PXLC-EXECUTION-IDENTITY`：Node 禁止 effective uid 0 或 gid 0；切换到预先拥有 state/credential
  的专用非 root 账号，不要用 root 生成 PXNI 后再降权。
- `PXLC-NODE-CHILD`：唯一 NodeDaemon child 非预期退出；parent 已把它视为全链失败并执行 joined
  cleanup，检查 child 原因后使用同一未漂移 config 重启。
- `PXLC-DEPLOYMENT-CONFIG-INVALID` / `PXLC-DEPLOYMENT-PREPARATION`：Deployment path、权限、inode、
  whole-file PXEA pin、artifact signature/cross-pin、seed 或 connector credential 输入不满足 strict gate；
  不要删除 journal 或改 artifact 来绕过。
- `PXLC-DEPLOYMENT-RECONCILE-REQUIRED`：durable state 明确需要 operator reconciliation；没有输出
  Deployment Ready，也不会 blind retry/replay uncertain publish。
- `PXLC-DEPLOYMENT-OWNER-EXIT` / `PXLC-DEPLOYMENT-JOINED-SHUTDOWN`：owner 提前退出或未完成 joined
  cleanup；均为非零，不能按 Ready 处理。

## 当前能力边界

现有 DeveloperLocal 路径的目标是一个非生产、单机、固定 profile 的系统基座：真实 Authority、
DeploymentController、Runtime-owned Fabric/Model/Agent、独立 NodeDaemon reference child、一次性
Inspection owner/IPC 和内部 Textual child。Textual 通过两个分离的 typed client 消费 Agent conversation 和
一份 PXIS v2 startup snapshot；Inspection 只是 App 前的单次 Latest 读取和三行只读展示，不是持续
监控、Ops 或 federation。配置入口不新增第二 desired-state owner，Textual 也不直接持有 Fabric、
provider 或 Secret。旧 Rust reference frontend 已在 r29 replacement gate 全绿后的本退役批删除；
内部 Textual child 是唯一当前展示路径，没有第二 frontend 或 fallback。

当前每次模型调用不流式、同时只允许一个 invocation、每轮只发送当前输入。会话历史回灌、Memory、
Tools、规划、多 Agent 编排、双 Node、持续 Node observation/reconciliation、federated Inspection、
OpsService、Web Console 和 production readiness 仍不能由这个入口宣称。内部双目标 fixture scaffolding
也不是公开命令或可运行分布式系统证据。

Node 是另一条不包含 Chat 的宿主基座。schema v1 仅有 split-trust Runtime fixed-rejection listener、
feature-only NodeDaemon、稳定 owner-private state 和 joined lifecycle。schema v2 增加真实 Runtime-control
listener、带观测 capability 的 NodeDaemon、Controller-authenticated Node-control ingress，以及将 PXQR
结果提交给 Node 唯一 durable owner 的 bridge；invalid raw request 在 Controller Ed25519 认证或 canonical
decode 失败时直接丢弃，不重试。restricted control transport 会使用 Fabric-owned Zenoh session，但它
不是 managed Fabric CoreService；本进程仍没有 Controller/Authority、managed Fabric、Agent/Model、
Inspection、Textual 或 TUI。schema v3 只增加 deterministic provider selection pin 与 PXEA v2 publication，
不把这些服务塞进 Node owner。

独立 public Controller composition 现已在单 Ubuntu 主机、同一 `nobody` UID/GID 和真实 non-loopback
mTLS 下完成 base managed-ready，以及三段 Fabric PXAG/PXAH → Agent PXAG/PXAH → descriptor PXAG/PXAH，
并覆盖 Fresh/Resume、Node clean restart、错误 PXEA SHA-256 与 Node-down negative。T1 Ready 只证明
bootstrap descriptor 已按 exact durable journal 取得；它不是 descriptor access/authorization、Agent
session/data plane、Echo/conversation、remote TUI、断网重连、provider mismatch、Authority failure、双主机
或 distributed Agent OS 证据。T2-A 已定义 remote-Agent contract 与状态图；T2-B 只增加下述 Fabric
传输基座，Echo、reconnect 与 TUI 继续后置。

### T2-B Fabric 证据边界

精确 ref `0bcc49d03d15cca720021868a08d37912f62128a` 增加两种非对称 mTLS profile。单个
`FabricService` 仍只拥有一个私有 Zenoh 1.9 `Session`：Ubuntu 侧在原有 loopback listener
之外增加一个 non-loopback TLS listener，且没有 connector；名义 Mac 侧只有一个 TLS
connector，没有 listener。两侧均采用默认拒绝 ACL，以证书精确 CN 为 subject，只允许
remote-Agent submit 与 control 两条精确 query route。实现见
[`service.rs`](../../crates/paraegox-fabric/src/service.rs)。

Ubuntu 验证中，完整 Fabric suite 为 50/50 全绿，format 与 Clippy 均通过；聚焦的
[真实网络测试](../../crates/paraegox-fabric/tests/remote_agent_mtls.rs) 用时 7.05 秒。正确 CN 的两条
精确 route 成功，sentinel/parent/child route 与同 CA 错误 CN 均被 ACL 拒绝；同一 Session
中的 T1 本地两条 route 仍成功；独立 plaintext loopback peer 被拒绝；shutdown 后两个端口
均能重新绑定。`**` 只由严格 constructor/static test 在 Session 打开前拒绝；put/subscriber
也只有“策略中无 allow rule”的静态证据，未进行真实网络实跑。

同一 Session 的 T1 本地调用依赖精确 Zenoh 1.9 local-face 边界，任何 Zenoh 升级都必须重跑
上述矩阵。错误 CN 可在 ACL 拒绝前完成握手并占用唯一 session，故 `max_sessions = 1` 有可用性
风险。该证据来自单 Ubuntu 主机，不是双机 Mac process 证明；当前也没有 Runtime PXTE9
state owner、真实 Mac connector composition、APFS outbox、Controller Describe source、公开
CLI/marker、Echo、reconnect 或 TUI，因此本 runbook 不提供这些尚不存在的启动步骤。
