---
name: postgres-patterns
description: 数据库、PostgreSQL、SQL 查询、索引、锁和事务性能。Postgres query plans, indexes and pagination.
---

# 从查询计划定位开销

选择一条慢请求，记录行数、过滤条件、排序与耗时。在可写副本上对真实参数读取 EXPLAIN (ANALYZE, BUFFERS)，将估计行数与实际行数逐节点比较。

围绕 WHERE 与 ORDER BY 设计索引；复合索引先考虑等值列，再考虑范围与排序列。分页需要稳定的次序与唯一的末级键。

用相同数据与参数再次测量。读写成本一起看，核对批量操作数、事务隔离与并发下的最终状态。

配套资料按需用 `skill_read` 的 `resource` 参数读取：

- [upstream](references/upstream.md)
