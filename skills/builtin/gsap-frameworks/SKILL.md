---
name: gsap-frameworks
description: 为 Vue、Svelte 等用户项目集成动画。GSAP framework lifecycle and scoped teardown.
---

# 让动画跟随组件所有者

等框架提交目标节点后启动补间，用组件作用域查找元素。在卸载钩子还原属性并清除事件监听。

将响应式更新映射到当前动画的目标值；对于列表项移动使用稳定业务键。服务端渲染仅输出可访问的起始内容。

配套资料按需用 `skill_read` 的 `resource` 参数读取：

- [upstream](references/upstream.md)
