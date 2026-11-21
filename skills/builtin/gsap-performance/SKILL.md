---
name: gsap-performance
description: 网页动效性能、隐藏暂停和资源清理。GSAP frame performance, layout cost and cleanup.
---

# 找到真正占用帧的工作

在重复同一次开合或拖拽时采样主线程，区分脚本、布局、绘制和合成。将多次布局读取集中到写入前。

高频输入用预建的更新器写最新值，避免每次 pointermove 新建时间线。隐藏时暂停持续动画，退出时释放计时器、监听和层。

配套资料按需用 `skill_read` 的 `resource` 参数读取：

- [upstream](references/upstream.md)
