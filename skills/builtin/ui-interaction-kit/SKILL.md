---
name: ui-interaction-kit
description: 滑块回弹、弹簧、拖拽吸附、展开收起、即时反馈。Spring motion, drag, overlays and gestures.
---

# 让反馈跟手

拖动开始时保存抓取点与控件的相对位置。移动期间位置直接跟手，释放后才向最近的合法目标回弹。值、滑块中心和命中区使用同一坐标系。

同一弹簧保存位置和速度，目标改变时从当前状态继续。切换浮窗时保持锚点；短暂离开按钮进入面板的路径应持续可点击。

用快速往返拖动、中途释放和键盘单步调整核对数值。背景反馈在失焦和静止时停止请求帧。

配套资料按需用 `skill_read` 的 `resource` 参数读取：

- [12-instant-feedback](references/12-instant-feedback.md)
- [13-spring-physics](references/13-spring-physics.md)
- [06-overlays](references/06-overlays.md)
