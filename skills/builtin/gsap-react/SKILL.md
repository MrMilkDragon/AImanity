---
name: gsap-react
description: 为 React 用户项目管理 GSAP 动画与挂载生命周期。GSAP React useGSAP, scope and cleanup.
---

# 绑定 React 生命周期

将动画作用域绑定组件根 ref，在 useGSAP 里建立动画并由 context 在卸载时回收。事件延迟创建的动画也纳入相同 context。

区分属性更新与重新挂载。快速改变 props、反复开关和开发环境重复挂载后，画面应只有一份动画与监听。

配套资料按需用 `skill_read` 的 `resource` 参数读取：

- [upstream](references/upstream.md)
