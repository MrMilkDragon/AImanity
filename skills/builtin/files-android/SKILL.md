---
name: files-android
description: 操作 Android DocumentsUI，查找下载文件、识别文件类型筛选范围并核对手机与电脑传输结果。
metadata:
  version: "0.2.0"
---

# Android Files / 手机文件

Open the actual DocumentsUI screen through phone-use. It may restore a previous directory rather than Downloads. Check the current title, breadcrumb and visible file list before selecting a file. Names, modified times and sizes help distinguish duplicate names; do not infer which file is newest from list order alone.

顶部文档/图片/音频/视频芯片可以筛选文件。优先本次原生目标，再用OCR/截图。芯片选中后，其文字可能从UI树消失；取消时根据当前截图识别同一芯片，不沿用旧截图坐标。筛选标题与面包屑可能不一致，不能默认筛选范围就是当前子目录，也不能直接声称全局搜索。

要在电脑编辑时，从手机文件列表取原件的实际路径，按 device-transfer 指引交给 Linux，再用 computer-use 打开。工作文件和 Android 下载目录是两个位置，各用文件回执里的路径。

电脑回传成功后，如果原先打开的下载列表还只显示旧文件，重新进入「文件」再查看同一目录，按回执里的名称选择结果。若任务还要回复消息，在来源会话中核对发出的附件。
