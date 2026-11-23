# AImanity 0.0.5 Europium

[English](../../README.md) · [言語一覧](../../README.md) · Apache-2.0

## ダウンロードと現状

[macOS 0.0.5 Europium (DMG)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-macos-arm64.dmg) · [Windows 0.0.5 Europium (EXE)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-windows-x64-setup.exe) · [Linux (DEB)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-linux-amd64.deb) · [Linux (RPM)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-linux-x86_64.rpm)

Apple Silicon 搭載の macOS 15.0 以降、または Windows 11 x64 に対応します。各インストーラーに Android、Linux、エミュレーター、ローカルサービスが含まれます。

Linux x86_64 では KVM と glibc 2.36 以降が必要です。Debian／Ubuntu は DEB、Fedora／Enterprise Linux 10 は RPM を選んでください。更新時は同じ形式の新しいパッケージをインストールすると、端末と会話を引き継げます。 [インストールコマンド](release-status.md#linux)

## 機能と利用例

AImanity は Android と Linux を一つの会話と一つの Agent で操作します。モデル設定は複数保存でき、設定画面で使用するものを選べます。

- WeChatなどの実画面と通知を確認し、利用者の指示に沿って操作・返信。アカウントやアプリ側の制限は引き続き有効です。
- 電話で受信したファイルをLinuxへ送り、表を処理・確認して返却。機器間転送と連絡先への送付は別々に検証します。
- 指示を追加して生成を中断しても、既存のメッセージと操作履歴は保持。通知からAgentを起動でき、Liberty は両方の機器を横断して動作します。
- Skill、関連付けた知識文書、作業ファイルは共有。モデル/API、記憶、外部接続はAgentに属します。デバイスは標準で直接接続します。プロキシはワークスペースで設定できます。

副作用のある操作には観察、機器、タスク、制御epoch、一意の操作IDを結び付け、結果不明時は再実行前に観察します。

## 技術とデータ

`apps/native`はRust（Iced/wgpu）、`crates`はRust/Axum/Tokio、`packages/protocol`はprotobufです。データは SQLx 経由で PostgreSQL に保存します。ビルドには Cargo を使用します。

ソースと利用者データは分離します。機器ディスク、アカウント、会話、添付、鍵は公開しません。更新は版管理された配布物、バックアップ、移行、ロールバックを使い、利用者データ上での直接の`git pull`を更新方式にしません。

## 今後・責任・ライセンス

今後は音声サービスの選択肢、アプリ向けのスキル、通知をきっかけに始まる事務作業の手順を拡充します。

画面と初期設定は10言語に対応。Skill左の言語ボタンで切り替えます。ゲストOS言語やツール原文は自動翻訳しません。アラビア語は右から左へ表示します。

権限、アカウント、外部操作、各アプリの規約は利用者が管理してください。詐欺、なりすまし、同意のない操作に使用しないでください。独自コードはApache-2.0、第三者部品は各ライセンスに従います。

[公開状況](release-status.md) · [構成](architecture.en.md) · [互換性](compatibility.md) · [第三者声明](../../THIRD_PARTY_NOTICES.md) · [免責事項](../../DISCLAIMER.md)
