---
name: backend-patterns
description: 后端接口、事务、服务边界、异步任务及批量访问。Backend API design, transactions and batching.
---

# 后端调用与持久化

从一个真实请求画出数据库和外部服务往返，标记每次读写的 owner。多个同类查询按键批量取回，并维持调用方要求的顺序。

在一笔事务中提交必须同时生效的数据，完成提交后才发布结果。把慢网络调用移出持锁区。用相同输入比较首次响应、整轮耗时和错误语义。

在 Rust 服务复用已有类型、连接池和取消通道；用户工程采用其他栈时沿用其约定。需要示例时读取配套的上游后端指南。

配套资料按需用 `skill_read` 的 `resource` 参数读取：

- [upstream](references/upstream.md)
