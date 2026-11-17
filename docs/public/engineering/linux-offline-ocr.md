# Offline Linux OCR

The Linux x64 OCR component provides Tesseract, its ELF dependency closure, English and Simplified Chinese language data, and the TSV configuration. It is embedded in the native service component. The Rust OCR runtime invokes the bundled native tool and models.

## Execution and integrity

The phone and desktop use the same Rust PNG-to-regions pipeline. Rust owns image bounds, screen signatures, TSV parsing and confidence filtering. The native helper selects the included ELF loader, Tesseract libraries and language data for that process only. No global library-path change is required.

The component manifest records the factory recipe, file hashes and permissions. The closure includes `tessdata/configs/tsv`. The Rust helper selects TSV output with `tessedit_create_tsv=1` and checks its header.

The [native OCR fixture](../../../crates/phone-runner/fixtures/ocr-probe.png) renders `7319`. The [source asset inventory](../../../config/source-assets.json) records its font and provenance. The separate [provider fixture](../../../crates/assistant/fixtures/vision-probe.png) exercises model image input.

## Maintainer build

The Windows guest factory installs Tesseract and its language data from the signed Debian Trixie archive. The collector records package versions, native file hashes and package copyright files.

```sh
cargo build --locked -p ai-phone-release-tools --bin release-tools
sudo target/debug/release-tools windows-guest-assets /factory/windows-guest-assets
target/debug/release-tools linux-ocr verify /factory/windows-guest-assets/ocr
```

Run this on the Linux x64 factory host with the Debian Trixie archive keyring. The Windows backend embeds the sealed OCR files alongside the Rust OCR helper.

## Availability

Current platform support, installer availability and distribution requirements are maintained in [release status](../release-status.md).

## 中文

Linux x64 OCR 组件内置 Tesseract、ELF 动态依赖、英文与简体中文字库和 TSV 配置。Cargo 构建的工具从 Debian Trixie 签名仓库收集依赖与许可，再生成离线清单。Windows 安装包随附这个组件。

封装清单包含 TSV 配置文件；Rust 通过 `tessedit_create_tsv=1` 直接选择输出格式，不依赖该文件启用 TSV。适配器拒绝无效 TSV 头，不把错误格式当成空识别结果。当前宿主范围、完整安装包供应和分发要求统一见[发行状态](../release-status.md)。
