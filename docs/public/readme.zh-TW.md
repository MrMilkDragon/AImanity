# AImanity 0.0.6 Fluorine

[English](../../README.md) · [全部語言](../../README.md) · Apache-2.0

## 下載與現況

[下載 0.0.6 Fluorine](https://github.com/MrMilkDragon/AImanity/releases/tag/v0.0.6) · [安裝驗收與實機覆蓋](release-status.md)

0.0.6 採全新安裝。工作區第一次開啟時沒有裝置。可按需建立 Android 或 Linux 客體，配對遠端裝置，或授權當前任務使用本機桌面。

## 功能與情境

Agent 可依任務選擇已授權的裝置。多台裝置共用一段對話、待辦和工作檔案。分開的任務可交給子 Agent 平行處理。

Copilot 接收語音修正，Liberty 自由探索，Task & Goal 持續執行指定目標。

## 架構與資料

原生介面使用 Rust、Iced 和 wgpu，協定使用 protobuf。裝置、聊天與任務由內嵌 redb 儲存在本機。

## 後續方向、責任與授權

接下來將擴充語音服務、應用程式專用技能，以及收到通知後啟動的辦公流程。

介面和初次配置支援十種語言。Skill 左側語言入口即時切換介面，阿拉伯語採 RTL；不自動改變客體系統語言、記憶或翻譯工具原文。

使用者須管理權限、帳號、外部操作及第三方條款。不得用於詐騙、欺瞞或未經同意的行為。原創程式碼採 Apache-2.0，第三方元件遵守各自授權。

[發行狀態](release-status.md) · [技術架構](architecture.en.md) · [相容範圍](compatibility.md) · [第三方聲明](../../THIRD_PARTY_NOTICES.md) · [免責說明](../../DISCLAIMER.md)
