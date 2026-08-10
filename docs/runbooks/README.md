# Runbooks

本目录保存面向操作者的诊断、恢复和回滚手册。Runbook 只能描述已经存在并验证过的运行入口。

## 每份 Runbook 至少包含

1. 适用症状和不适用情况。
2. 所需权限、设备和环境。
3. 首先收集的 Inspection、Receipt、Trace 和日志证据。
4. 从低风险到高风险的操作步骤。
5. 每一步的预期结果和停止条件。
6. 回滚或恢复方式。
7. 最终成功证据，以及仍需人工处理的情况。

涉及停止服务、终止进程、释放 Resource Lease、重启机器人或物理执行器的操作，必须明确授权边界和不可逆风险。

在 OPS 与 Inspection Protocol 尚未实现前，本目录不编写依赖这些未实现能力的故障命令；
已经由当前工作树验证的启动、退出和恢复入口可以形成明确标注范围的 Runbook。

当前已有运行入口：

- [DeveloperLocal 启动与退出](developer-local.md) — 一条命令本地启动、正常退出、恢复与常见拒绝；明确标注 deterministic provider 和非生产边界。
