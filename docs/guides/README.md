# Guides

本目录提供面向任务的开发和使用指南。每篇 Guide 应帮助读者完成一个具体目标，而不是重复系统架构。

未来可能包括：

- 创建和运行一个 Core Service。
- 定义 ServiceSpec 和依赖绑定。
- 定义带 Artifact export/entrypoint 引用的 `CardDefinition`，声明 In/Out，并在 Deck 中创建 Card。
- 创建一个 Driver-backed Sensor Card 或 Processor Card。
- 用 CardDefinition Port 组成 Deck Link，并理解 `DeploymentPlan.bindings` 到 live PortBinding 的安装链。
- 使用仿真环境验证 Command 与 Receipt 闭环。
- 开发 OPS 命令或 TUI 视图。

Guide 中的命令、配置和输出必须由当前仓库实际验证。尚未实现的目标只能留在 Architecture 或 Plans 中，不得写成可执行教程。

稳定字段和完整协议应链接 [`reference/`](../reference/README.md)，故障恢复步骤应进入 [`runbooks/`](../runbooks/README.md)。
