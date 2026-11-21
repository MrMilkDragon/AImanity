---
name: game-architect
description: 用户游戏项目的状态、输入、系统边界与存档。Game software architecture and simulations.
---

# 组织游戏代码

把输入事件、规则状态和渲染快照分开。给一个玩法画出输入到反馈的最短路径，确认时钟、随机数和状态所有者。

存档记录可重建状态需要的数据。用重放的同组输入比较状态与可见结果，先实现竖切片再拆分系统。
