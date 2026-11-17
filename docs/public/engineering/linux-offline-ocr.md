# Offline Linux OCR

The Linux x64 OCR component provides Tesseract, its ELF dependency closure, English and Simplified Chinese language data, and the TSV configuration. It is embedded in the native service component. The Rust OCR runtime invokes the bundled native tool and models.

## Execution and integrity

The phone and desktop use the same Rust PNG-to-regions pipeline. Rust owns image bounds, screen signatures, TSV parsing and confidence filtering. The native helper selects the included ELF loader, Tesseract libraries and language data for that process only. No global library-path change is required.

The component manifest records the factory recipe, file hashes and permissions. The closure includes `tessdata/configs/tsv`. The Rust helper selects TSV output with `tessedit_create_tsv=1` and checks its header.

The [native OCR fixture](../../../crates/phone-runner/fixtures/ocr-probe.png) renders `7319`. The [source asset inventory](../../../config/source-assets.json) records its font and provenance. The separate [provider fixture](../../../crates/assistant/fixtures/vision-probe.png) exercises model image input.

## Maintainer build

The OCR factory uses a pinned Ubuntu base image, CA package and [archive snapshot](https://snapshot.ubuntu.com/). The collector records package versions and native file hashes, and includes each package's copyright file.

```sh
cargo run --locked -p ai-phone-release-tools -- linux-ocr build \
  /factory/ocr-context /factory/ocr
```

Run on a Linux x64 factory host. The Cargo-built tool collects and seals the closure from the pinned Ubuntu image. The Windows backend embeds it with the Rust OCR helper.

## Availability

Current platform support, installer availability and distribution requirements are maintained in [release status](../release-status.md).

## 中文

Linux x64 OCR 组件内置 Tesseract、ELF 动态依赖、英文与简体中文字库和 TSV 配置。Cargo 构建的工具从固定 Ubuntu 快照收集依赖与许可，再生成离线清单。Windows 安装包随附这个组件。

封装清单包含 TSV 配置文件；Rust 通过 `tessedit_create_tsv=1` 直接选择输出格式，不依赖该文件启用 TSV。适配器拒绝无效 TSV 头，不把错误格式当成空识别结果。当前宿主范围、完整安装包供应和分发要求统一见[发行状态](../release-status.md)。
