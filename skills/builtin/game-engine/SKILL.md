---
name: game-engine
description: 用户游戏引擎开发、帧循环、坐标与渲染。Game engines, render loops and collision spaces.
---

# 让各系统共享空间与时间

定义世界、相机、界面与显示像素的坐标转换。从同一帧快照产生绘制与命中区域，窗口缩放时同步更新。

将输入采集、模拟步进、可见集和提交绘制拆开测时。用稳定输入和同一相机对照更新效率。
