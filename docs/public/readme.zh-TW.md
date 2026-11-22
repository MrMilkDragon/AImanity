# AImanity

[English](../../README.md) · [全部語言](../../README.md) · Apache-2.0

## 下載與現況

[macOS 0.0.5 Europium (DMG)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-macos-arm64.dmg) · [Windows 0.0.5 Europium (EXE)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-windows-x64-setup.exe) · [Linux (DEB)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-linux-amd64.deb) · [Linux (RPM)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-linux-x86_64.rpm)

支援 Apple Silicon macOS 15.0 以上與 Windows 11 x64。安裝包內建 Android、Linux、模擬器及本機服務。

Linux x86_64 需啟用 KVM 並使用 glibc 2.36 以上。Debian／Ubuntu 請選 DEB，Fedora／Enterprise Linux 10 請選 RPM。更新時安裝同格式的新版套件，即可保留裝置與聊天。 [安裝指令](release-status.md#linux)

## 功能與情境

AImanity 將 Android 手機與 Linux 桌面放在同一個工作台，兩台裝置共用一條對話和目前選用的 Agent。可在設定中儲存並切換模型設定。

- 操作微信等應用：觀察真實畫面、檢視通知，依使用者指示回覆；帳號及應用限制仍然有效。
- 跨裝置工作：手機收件，傳到 Linux 處理、核對試算表，再交回手機。裝置間傳輸成功與交付聯絡人是不同結果，需分別驗證。
- 持續任務：可持續補充指示和中斷生成，先前訊息與操作紀錄保持；通知可喚醒 Agent，Liberty 可跨裝置運作。
- Skill、關聯知識文件、工作檔案全域共享；模型/API、記憶、外部連接屬於 Agent。裝置預設直接連線，可在工作台設定網路代理。

有副作用的動作綁定觀察、裝置、任務、控制 epoch 及唯一 action ID；結果未知時先觀察再決定是否重試。

## 架構與資料

`apps/native`：Rust（Iced/wgpu）；`crates`：Rust/Axum/Tokio；`packages/protocol`：protobuf。資料透過 SQLx 存入 PostgreSQL，專案使用 Cargo 建置。

程式碼與使用者狀態分離。裝置磁碟、帳號、聊天、附件、金鑰及私有證據不進入公開儲存庫。更新應使用版本化產物、備份、遷移與回復，不直接在使用者資料上執行 `git pull`。

## 後續方向、責任與授權

接下來將擴充語音服務、應用程式專用技能，以及收到通知後啟動的辦公流程。

介面和初次配置支援十種語言。Skill 左側語言入口即時切換介面，阿拉伯語採 RTL；不自動改變客體系統語言、記憶或翻譯工具原文。

使用者須管理權限、帳號、外部操作及第三方條款。不得用於詐騙、欺瞞或未經同意的行為。原創程式碼採 Apache-2.0，第三方元件遵守各自授權。

[發行狀態](release-status.md) · [技術架構](architecture.en.md) · [相容範圍](compatibility.md) · [第三方聲明](../../THIRD_PARTY_NOTICES.md) · [免責說明](../../DISCLAIMER.md)
