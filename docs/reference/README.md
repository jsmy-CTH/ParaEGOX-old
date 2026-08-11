# Reference

本目录保存与版本对应、可以作为实现事实查询的参考资料。

未来内容包括：

- 公共 Rust crate/API、Python SDK/API，以及二者共同消费的语言中立 wire contract。
- CardDefinition、PortSpec/SchemaRef、DeliveryProfile、DeckSpec、DeckLock、ServiceSpec、DeckTopology、DeploymentPlan.bindings/execution、CardProfile、DeploymentProfile 和 ZenohTopologyProfile Schema。
- Signal、Event、Command、Query 与 Receipt 协议。
- 生命周期、Readiness、Fault 和错误码。
- 配置项、环境变量和命令行接口。
- InspectionProtocol、OpsProtocol、ControlRequest/OpsReceipt 与 InspectionClient/OpsClient API。
- ProcessDomain worker 的 handshake、generation、credit、deadline/cancellation、terminal result 和版本兼容规则。
- Rust toolchain/target triple/libc/CPU feature 与 Python runtime/ABI、Zenoh、ROS2、GPU/device 的已验证支持矩阵。
- CardDefinition、In/Out、Link、PortBinding、Card、CardInstance、DeckRun 等术语及其代码映射。

Reference 必须来自真实实现或生成产物，并标明适用版本。目标架构和未实现字段不得写入 Reference；相关内容应留在 [`architecture/`](../architecture/README.md)、[`adr/`](../adr/README.md) 或 [`plans/`](../plans/README.md)。

根据 [ADR-0006](../adr/ADR-0006-rust-first-core-and-polyglot-workloads.md)，跨进程和跨语言合同的权威是唯一 Schema、canonical encoding、digest、版本和 unknown-field 规则。Rust struct/enum layout、trait object、Tokio handle、Python object、任意 pickle、语言默认 serializer 或生成 binding 都不是协议真相。Reference 必须把 language-neutral contract 与具体 Rust/Python binding 分开，并链接 byte-level golden vectors 和双向兼容测试；没有实现与证据时不得虚构稳定 Rust ABI、Python SDK 或 worker protocol。

当前没有正式 Application/Installation Schema；在多 Deck、稳定安装 identity 或应用私有持久状态触发 Proposed ADR 并产生真实实现前，Reference 不得虚构 ApplicationSpec/Lock/Instance/Controller 或 application_id。
