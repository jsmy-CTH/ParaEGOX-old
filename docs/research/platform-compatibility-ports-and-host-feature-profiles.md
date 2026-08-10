# 平台兼容 Port、host-platform support evidence 与 OS Backend 边界

> 状态：Research Complete，结论为 `proceed`（进入后续 admission，未授权实现）
> 日期：2026-08-02
> 深度：Standard
> 评审策略：多路只读仓库审阅 + 主线综合
> 范围：Runtime、RuntimeHost、DeploymentTenureAuthority、Controller/Runtime journal、local IPC、进程 containment、资源观测与 OS service identity
> 实现状态：尚未实现统一平台层；S7-E 已按 PC-G 将新增 OS 机制保持为 owner-private seam，并由提交 `1ed704c`、Ubuntu CI `30748840399` 证明 exact Linux ext4 reference vertical。macOS APFS 在缺少 FD-anchored extended-ACL 与 crash-durability 证明时 fail closed，Windows 仍 unsupported；S7-E 证据只满足请求 PCA 的技术前置，不代表 PCA 已准入或 macOS/Windows production support
> 相关术语：[Capability、Service Contract 与 Feature Support 边界](../concepts/capability-service-feature-boundaries.md)
> 当前实施计划：[Kernel Foundation](../plans/kernel-foundation.md)

## 一句话结论

ParaEGOX 应统一平台能力的**语义、要求、结果和证明等级**，并由 Linux、macOS、Windows backend 分别实现；Authority、Runtime、RuntimeHost 等 owner 继续拥有自己的状态机、journal、授权、retry 和生命周期。缺少所需能力时必须 fail closed，不能把较弱实现静默伪装成等价支持。本文只准入后续 admission 候选，不授权自动进入 PC0–PC3 实现。

## 1. 研究问题与成功标准

本研究回答：

1. 当前分散在 Runtime、RuntimeHost 和 Deployment 中的 OS 机制是否应形成统一兼容层。
2. 哪些语义可以跨平台统一，哪些必须保留 Linux、macOS、Windows 的真实差异。
3. 如何减少安全路径、进程树、local IPC、锁和 durable publish 的重复实现，又不形成新的全局 owner。
4. 统一层应何时进入 P2e 之后的开发计划，怎样避免再次拖慢首个可运行纵向闭环。
5. 什么证据出现后，才允许抽取新的共享 crate 或公共 contract。

成功必须同时满足：

- Card、Deck、Deployment 和 Runtime 上层状态机不依赖 UID、PID、Unix socket、`/proc`、SID 或 Job Object 等平台细节。
- 同一个上层结果在不同 backend 中具有明确、可比较的证明等级；`true` 或“成功返回”不能掩盖证明强度差异。
- 平台层不拥有 journal payload、DeploymentWriterEpoch、Runtime generation、restart budget、授权 allowlist、rollout 或第二份配置真相。
- Installer/Planner/Runtime 只接受满足 exact requirement 的 host-platform support evidence（工作称谓）；`degraded/unknown/unsupported` 不触发隐式 fallback。
- 每个受支持 backend 都通过同一语义 conformance suite 和自己的真实系统 Harness。
- 新的共享包必须满足治理规则已有的“至少两个独立生产消费者”；本文另加“至少两个真实 OS backend”作为保守研究门槛，二者共同证明稳定交集后才进入 package admission。

## 2. 范围、假设与非目标

### 2.1 范围

- Runtime ProcessDomain 的 spawn、IPC pipe、TERM/KILL、reap、process-tree cleanup 与资源 census。
- RuntimeHost 外部 service-manager/watchdog 对 host child/process group 的所有权。
- DeploymentTenureAuthority 的安全路径、文件锁、Unix socket、peer credential、service account 与 key handle。
- S7-E 及其后续 Controller/Runtime owner-specific store 和 authenticated local control channel。
- Linux、macOS 与 Windows 的能力匹配、CI 和 rollout 入口。

### 2.2 假设

- Linux/POSIX 继续是首个 reference profile；本文不改变已接受的 S7/P2e 交付依赖。
- macOS 当前是开发与部分 POSIX 验证环境，不等于完整 production profile。
- Windows backend 尚未实现；Named Pipe、SID/token、Job Object、Windows service 与对应 durable filesystem 机制目前只是 PC2 待一手资料和实机验证的候选，不是本文已证明的等价实现。
- `FeatureReport` 表示目标实际支持事实，不表示 Principal 获得 `CapabilityGrant`。

### 2.3 非目标

- 不在当前 S7-F 前置创建 `paraegox-platform`、万能 `Platform` trait、动态 backend registry 或插件系统；S7-E 已完成也不自动授权这些 surface。
- 不用最低公分母削弱现有 Linux/POSIX 证明；S7-E 的 Ubuntu CI 只证明 exact Linux ext4 reference vertical，不能外推为 macOS/Windows 等价支持或 platform workstream admission。
- 不把三个 owner-specific journal 抽成 generic storage service、shared transaction 或第二 writer。
- 不在本文冻结公共 Rust trait、YAML、CLI、wire field 或最终 package 名称。
- 不因一个 backend 能编译，就宣称它具备 production containment、资源 enforcement 或服务账号隔离。

## 3. 当前仓库证据

| 证据 | 类型 | 强度 | 对结论的影响 |
| --- | --- | --- | --- |
| Runtime contract 的 `ProcessResourceLimits` 描述资源要求而不暴露 `/proc` 或 Job Object | local | 高 | 上层合同已经具备平台中立基础 |
| [`process_platform.rs`](../../crates/paraegox-runtime/src/process_platform.rs) 集中 POSIX spawn/process-group/pipe/TERM/KILL/reap，并仅在 Linux 实现 `/proc` census | local | 高 | Runtime 已有隐式 backend seam，但进程与资源观测仍混在一个实现中 |
| [`process_domain.rs`](../../crates/paraegox-runtime/src/process_domain.rs) 的状态机基本不感知 OS，只在 Linux live resource enforcement 处分支 | local | 高 | 可以抽取窄 port，不需要改写 Domain 状态机 |
| [`service_manager.rs`](../../crates/paraegox-runtime-host/src/service_manager.rs) 另有一套 POSIX process-group、pipe、TERM/KILL、存在性和 reap 逻辑 | local | 高 | 已出现第二条 owner-local reference implementation path 和重复机制，但它不计入 PC3 的“第二个独立生产消费者”门 |
| [`tenure_authority_process.rs`](../../crates/paraegox-deployment/src/tenure_authority_process.rs) 已有 Unix/unsupported 平台分派，并直接验证 Unix peer UID/GID | local | 高 | authenticated local channel 可以统一语义，身份表示不能统一为模糊整数 |
| [`tenure_authority/store.rs`](../../crates/paraegox-deployment/src/tenure_authority/store.rs)、[`controller_store.rs`](../../crates/paraegox-deployment/src/controller_store.rs) 与 [`runtime_store.rs`](../../crates/paraegox-runtime/src/runtime_store.rs) 各自拥有安全路径、锁、atomic replace、file/directory sync；`ProductionReference` 精确接受 Linux ext4，macOS APFS 当前 fail closed | committed + Ubuntu CI system evidence | 高 | owner-private seam 已按 PC-G 生效；APFS mode bits 不能证明没有 inherited extended ACL，不能把 POSIX 可编译或 fixture 通过外推为 production support；durable primitive 可共享要求，但 ACL 与 filesystem crash 语义必须逐 backend 证明 |
| [`process_workspace.rs`](../../crates/paraegox-runtime/src/process_workspace.rs) 验证 root/workspace 类型与 device/inode identity并请求`0700`创建；Authority store/key/socket另有完整no-follow、ancestor、link-count、owner/group/mode验证 | local | 高 | 安全对象身份存在可复用交集，但验证强度和owner policy不能被一个模糊“secure path”抹平 |
| [ADR-0007](../adr/ADR-0007-p2e-reference-journal-and-crash-recovery.md) 要求三个 owner 拥有三个独立 journal，并要求目标 OS 单独证明锁/fork/exec/crash 语义 | accepted decision | 高 | 禁止“共享平台层”演变为共享状态 owner 或伪等价证明 |
| Feature 支持已与授权、ServiceContract 分开 | local draft terminology | 中高 | 平台支持应进入限定 Feature profile/report，不使用 capability 词根 |

当前实现事实是“多个 owner 各自拥有一部分平台 adapter”，不是“完全没有平台层”。S7-E 已让三个 store、installer、Authority/Runtime authenticated local channel 与 process/service-manager seam 保持 owner-private，并由 exact Linux ext4 CI vertical 提供真实 consumer evidence；这是 PC-G 的预期结果，不是最终跨平台架构已经完成。主要缺口仍是统一的要求词汇、证明等级、backend conformance 和跨 owner 的窄机制交集。两路只读审阅覆盖真实代码 seam/重复点，以及最低公分母、巨型 trait、owner adapter 三类方案的所有权和失败反例；它们不构成外部平台 API 或 Windows filesystem/service 语义的一手证明。

## 4. 三种方案

### 4.1 统一最低公分母

提供一套所有平台都能返回成功的 `spawn/kill/write/peer_id` API。

优点是调用简单、`cfg` 较少；缺点是最危险：POSIX process group、Linux cgroup、Windows Job Object 的 containment 范围不同，Unix UID/GID 与 Windows SID/token 不同，`rename + fsync` 与 `ReplaceFile + FlushFileBuffers` 的 crash 语义也不同。最低公分母会把“清空受控进程树”退化成“尝试终止某个 PID”。

**结论：拒绝。**

### 4.2 单一巨大 `Platform` trait

把进程、文件、IPC、身份、随机数、资源和 service manager 全部放进一个 trait，再通过运行时 backend 选择。

它比散落 `cfg` 更整齐，但会形成新的万能 owner、mock-only consumer、动态选择和隐式 fallback 风险；任一修改都会让所有 owner 和 backend 重新编译/联测。

**结论：拒绝。**

### 4.3 窄 owner port + exact feature profile + OS backend

上层 owner 只依赖若干窄语义 port；backend 静态选择并报告精确支持与证明等级。平台层执行机制，但不决定授权、desired state、retry 或 terminal outcome。

**结论：采用。** 先保持 owner-private，满足抽取门后再决定是否形成共享 crate。

## 5. 推荐结构

```text
DeploymentTenureAuthority   Runtime/ProcessDomain   RuntimeHost supervisor
          │                         │                         │
          └──────────── owner-specific narrow ports ─────────┘
                                    │
                    exact host-platform support evidence
                                    │
                 ┌──────────────────┼──────────────────┐
                 ▼                  ▼                  ▼
           Linux backend      macOS backend      Windows backend
```

下列名称只是职责组，不是已准入 API 名称：

| 职责组 | 统一要求 | backend 必须保留的差异 |
| --- | --- | --- |
| Owned process tree | spawn、graceful stop、forced stop、wait/reap、exact-zero/uncertain evidence | process group、cgroup/pidfd、Job Object 的 containment 范围 |
| Resource observation | bounded census、指标单位、freshness、overflow/work limit、unsupported | `/proc` 可见性、Job accounting、macOS 可提供指标范围 |
| Authenticated local channel | bounded frame、peer identity evidence、ACL、deadline、closed/uncertain | Unix socket credential 与 Named Pipe token/SID |
| Secure filesystem object | no-follow、exact object identity、owner/ACL、non-inherited handle | inode/device 与 Windows file identity/security descriptor |
| Crash-consistent publish | exclusive crash-released lock、same-directory publish、data/metadata durability、recovery verdict | filesystem/OS 的 flush、replace、directory durability 保证 |
| Service supervision | exact child/service identity、start/stop/reap/restart ownership evidence | systemd/launchd/Windows SCM 生命周期 |
| System entropy | nonzero CSPRNG bytes、failure is fatal | OS RNG API 与 early-boot availability |

平台实现不得直接返回含义不足的 `bool`。结果至少区分：

- `direct_child_exited`；
- `owned_group_absent`；
- `contained_tree_empty`；
- `unsupported`；
- `uncertain`。

具体类型名和 wire 编码等待真实 consumer 与 ADR，不由本文冻结。

## 6. 平台支持证据与授权边界

`host-platform support evidence` 只是本文的工作称谓，不是新的 `HostFeatureReport` Schema，也不能取代未来由正确 owner 产生的 `NodeFeatureReport`。build、install 与 live 三种 lifecycle 的值不得默认复用 epoch、cache 或 source authority。最终公共名称、producer 与 canonical Schema 等待 owner 裁决和 ADR。

平台支持事实只能使用限定 Feature profile/report 语义，不能叫 `CapabilityLayer` 或 `CapabilityProfile`：

- Feature 回答“目标实际能证明什么”。
- Requirement 回答“本次部署必须具备什么”。
- `CapabilityGrant` 回答“某个 Principal 被允许请求什么”。

平台支持候选维度包括 local peer identity、service-principal isolation、crash-released exclusive lock、durable atomic publish、process-tree containment、aggregate resource accounting 与 external supervision。每项必须有 backend/version、source epoch、support state 与 evidence profile；`unknown/degraded` 不能自动满足 exact requirement。

S7-E 的 descriptor/installer/manifest 只能在已有 canonical owner 内承载最终获准字段。本文不授权添加 side-file、环境变量或第二份 platform configuration authority。

## 7. Owner 边界

| Owner | 继续独占 | 可消费的平台机制 |
| --- | --- | --- |
| DeploymentTenureAuthority | epoch、proof signing、allowlist、request authorization、Authority journal | secure key/path、authenticated local peer、exclusive lock、durable publish |
| DeploymentController | plan/revision/allocation、request signing、rollout/query/reconcile、Controller journal | secure store primitives、authenticated local client、service identity |
| RuntimeHost/Runtime | apply/recovery/assembly、generation、resource/action/terminal journal | secure store primitives、process containment、resource observation、local endpoint |
| OS service manager adapter | exact host child/process-group、restart window、backoff/quarantine | backend process/service primitive；不取得 Runtime journal 或 recovery authority |

平台层永远不拥有：

- Deployment/Runtime desired state；
- writer tenure、revision 或 generation；
- retry/reconcile/restart budget；
- authorization policy、Secret 或 key rotation；
- owner Receipt、terminal selection 或第二 journal。

## 8. 后续候选工作池与依赖

### PC-G — S7-E 已执行、后续持续有效的治理 guardrail（不是新阶段）

- S7-E 没有因平台抽取改变交付 DAG；当前 S7-F 也不增加共享平台重构前置。
- S7-E 的 Controller/Runtime store、Authority client、Runtime endpoint 与 installer 已将 OS syscall 限制在 owner-private adapter/module；S7-F 新增 OS 机制继续遵守同一约束。
- 不创建顶层 platform crate、公共 trait 或运行时 backend registry。
- 对所有 unsupported path 保持结构化 fail-closed，不以测试 skip 冒充支持。

### PCA — platform-workstream admission decision

依赖：S7-E 最小 executable vertical 已完成。

当前状态：S7-E executable vertical 已由 `1ed704c` / CI `30748840399` 完成，技术前置已满足；PCA 尚未由用户明确接受或 Proposed ADR/topic admission 准入。

- 由用户明确接受后续交付范围，或通过 Proposed ADR/topic admission 冻结 owner、目标 OS、完成证据、预算和与 P3 的优先级。
- 重新核对 PC0–PC3 是否仍是最小路线；未通过 PCA 时不得自动进入任何 backend 或共享抽取实现。

### PC0 — Linux reference semantics 与 conformance（候选）

依赖：PCA 已明确准入，且 S7-E 最小 executable vertical 与真实 owner consumer 已完成。

- 从实际重复中冻结最小内部 feature vocabulary、requirement 与 evidence level。
- 分离 process lifecycle/containment、resource observation、authenticated local channel 和 durable filesystem primitive 的 conformance suite。
- 以当前 Linux/POSIX backend 证明现有保证没有因抽象而削弱。
- 明确 cgroup v2/pidfd 是否作为 production Linux profile，process group 仅保留 reference/trusted profile。

### PC1 — macOS backend 与 CI（候选）

依赖：PC0。

- 增加 macOS compile/lint 和可运行的 POSIX conformance CI。
- 为 secure filesystem object 提供不经 path reopen、绑定已验证 file descriptor/object identity 的 extended-ACL 读取与拒绝证据；在此之前 APFS `ProductionReference` 保持 unsupported，不以 `0700/0600` mode bits 代替 ACL 证明。
- 对 peer identity、APFS file/directory durable publish 与 crash recovery、launchd/service identity、process-tree cleanup 分别给出真实证据。
- 缺少 Linux `/proc` 等价资源 enforcement 时显式报告 unsupported/degraded，不外推 Linux 结论。

### PC2 — Windows backend（候选）

依赖：PC0；可与 PC1 的研究并行，production 准入独立。

- 研究并实现 Named Pipe + token/SID、Security Descriptor、Job Object、Windows service、file lock/replace/flush 与 CSPRNG adapter。
- 建立 Windows-specific crash、service identity、process-tree 和 ACL Harness。
- 在等价证据出现前，Windows 只可作为 unsupported target，不以 compile-only 宣称支持。

### PC3 — 共享抽取评估与 public admission gate（候选）

依赖：治理要求的至少两个独立生产消费者、本文建议的附加门槛“至少两个真实 OS backend”，以及二者之间已经证明的稳定交集。

- 评估共享代码是否值得形成内部 leaf crate；若涉及新 package/public contract/persistent field，先提交 Proposed ADR 与 governance admission。
- 共享层只抽取无状态机制与纯 evidence types，不抽取 owner state machine 或 generic journal。
- 若交集不足，保留各 owner adapter，不为目录对称强行抽取。

PC0–PC3 只是 P2e 之后的候选工作池；通过 PCA 后可与 P3 的 Linux reference 开发并行。只有某个目标 profile 明确要求相应 backend 时，它才成为该目标 milestone 的硬依赖，且不得因为本文存在而自动抢占 P3。

## 9. 验证策略

每个 backend 的证据分三层：

1. **语义 conformance**：同一 requirement、result/evidence、unsupported/uncertain precedence。
2. **真实 OS component/system Harness**：真实账号/token、socket/pipe、lock、crash、restart、process tree 与 filesystem。
3. **owner vertical**：Authority、Controller、RuntimeHost 通过真实 backend 完成一次各自的安全/恢复链，而不是只用 fake port。

必测反例：

- backend 声称能力但只能得到较弱 evidence；
- peer identity 缺失、变化或与 ACL principal 不一致；
- child 逃离 reference process group；
- lock handle 被 child 继承；
- rename/replace 后 durability 不确定；
- resource census 不完整、权限失败或超出 work bound；
- service wrapper 与真实 child 身份不一致；
- feature report 过期、target/backend/version 不匹配；
- unsupported backend 被环境变量或 fallback 强制启用。

## 10. Rollout、降级与回滚

- 首次收拢只在 internal seam 后替换实现，不改变 canonical wire、journal payload 或 public CLI。
- 每个 backend 由编译/安装 profile 静态选择；不得运行时探测失败后切换较弱 backend。
- rollout 先以 shadow conformance 比较旧实现和新 adapter 的结果/evidence，再切换唯一调用路径。
- 发现语义差异时回滚 adapter wiring，保留 owner journal 和 canonical contract；不 dual-write、不同时运行两套 lifecycle owner。
- 移除旧 OS 路径前搜索所有 `cfg`、syscall 和 system test consumer，并保留至少一个真实 owner vertical。

## 11. 风险、反例与开放问题

主要风险：

- trait 为了方便不断扩张，最终成为新的 Kernel/Platform god object；
- feature report 被错误当作 authorization、readiness 或永久机器标签；
- fake backend 通过单元测试，却没有真实 service account/process/filesystem 证据；
- 共享 durable primitive 暗中取得 payload、migration 或 retry 权威；
- macOS/Windows 被迫模拟 Linux `/proc`/signal 术语，导致假等价；
- 平台重构抢占当前 S7-F 或后续 P3，再次降低用户可见开发速度。

开放问题：

- 最终平台支持 evidence 由 installer、NodeDaemon 还是 OS service manager 产生和更新；不同 lifecycle 是否需要 build/install/live 三类值。
- Linux production containment 采用 cgroup v2、pidfd 或二者组合的精确 profile。
- macOS 能否为目标 ProcessDomain 提供足够的 aggregate resource accounting 与 escaped-descendant containment。
- Windows durable directory/replace 证明与 Unix journal protocol 是否共享上层 transaction contract，还是需要 backend-specific successor。
- 何时出现足以准入共享 crate 的第二个 backend 和第二个独立 owner consumer。

这些问题不阻塞 PC-G；在 PC0/PC1/PC2 进入实现前分别研究和裁决。

## 12. 决策影响与完成证据

当前结论不需要修改 Accepted ADR，也不授权公共接口。出现以下任一变化时需要 Proposed ADR：

- 新增顶层共享 platform package；
- 新增 public feature/requirement/evidence Schema；
- 修改 descriptor/manifest 的 canonical persistent bytes；
- 让 platform layer 取得 service lifecycle、journal 或 retry owner；
- 把新的 OS backend 宣称为 production support profile。

整个平台兼容工作流只有在以下证据齐备时才能标记完成：

- 每个声明支持的 OS 都有 pinned CI runner/toolchain 与真实 backend system Harness；
- exact host-platform support requirement（工作称谓）在 install/placement/startup 路径 fail closed；
- common conformance suite 与 owner vertical 全绿；
- 不存在静默 fallback、第二配置真相或 owner state 下沉；
- 至少一次跨 backend 的相同上层 workload/Authority/Runtime lifecycle 产生语义一致、证明等级如实不同的结果；
- 文档明确列出仍 unsupported 的 containment、resource、service 与 filesystem profile。
