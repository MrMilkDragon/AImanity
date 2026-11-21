---
name: game-developer
description: 为用户实现实时交互、输入、更新循环和渲染性能。Game implementation and frame budgets.
---

# 实现实时交互

从引擎真实主循环接入输入，让模拟按合适的步长前进，渲染读取最近可展示状态。事件和连续输入分别处理。

用一段可重放输入采样输入排队、模拟、渲染和呈现。将耗时最高的一段与帧预算比较，在相同场景复测修改。
