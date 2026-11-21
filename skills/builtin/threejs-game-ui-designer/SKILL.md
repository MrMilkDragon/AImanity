---
name: threejs-game-ui-designer
description: 用户 Three.js 应用的三维场景 UI、标注与交互。Three.js scene overlays and picking.
---

# 把三维目标映射到界面

确认控件应固定于屏幕还是锚定世界坐标。世界标注在相机更新后投影到当前视口，处理相机背后的点和视口裁剪。

指针位置根据真实画布边界与缩放换算。优先使用一个命中所有者；叠层不应挡住无关的场景操作。
