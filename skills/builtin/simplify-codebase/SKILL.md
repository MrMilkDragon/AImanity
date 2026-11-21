---
name: simplify-codebase
description: 精简代码、冗余、死代码、服务边界和生命周期。Codebase simplification, dependency and lifecycle audit.
---

# 删除之前确认所有者

顺着一个真实入口追踪到存储或最终副作用，画出调用者、状态持有者和取消路径。查动态注册、协议字段、安装脚本和旧数据读取。

重复实现只保留真正拥有该决策的一处。删除无人调用的分支或重复转换时记录旧调用者如何继续工作。将历史迁移和生成文件与业务代码分开审视。

用同一正常操作核对改前改后输出、数据库写入和耗时。对尚未确认的删除候选写下文件、调用路径与待核证据。

配套资料按需用 `skill_read` 的 `resource` 参数读取：

- [investigation](references/investigation.md)
- [boundaries-and-lifecycle](references/boundaries-and-lifecycle.md)
