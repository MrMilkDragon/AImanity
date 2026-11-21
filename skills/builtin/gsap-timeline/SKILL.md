---
name: gsap-timeline
description: 多段 UI 动画编排、反向恢复与重放。GSAP timelines, labels and reversal.
---

# 编排可中断过渡

用标签表示具体状态，例如面板已展开或控件已可输入。把相关动作放到同一 timeline，以相对于标签的位置控制交错。

反向时从当前进度返回。选择同一终点后重复点击应保持稳定；交互可用性由实际状态管理，避免等待无关的收尾动效。

配套资料按需用 `skill_read` 的 `resource` 参数读取：

- [upstream](references/upstream.md)
