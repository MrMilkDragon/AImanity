# Third-party sources and licence status

Original AImanity code is licensed under **Apache-2.0**. Third-party software, data and assets retain their own licences, notices and source requirements.

## Distribution scope

The public **source export** contains project source, lockfiles, attribution records and selected upstream licence texts. It excludes SDK archives, guest images, prebuilt runtime components and personal data.

A **complete client bundle** includes the native Rust workbench, device tools, speech model and guest system images. See [runtime bundles](docs/public/runtime-bundles.md) and [release status](docs/public/release-status.md).

See [distribution requirements](docs/public/distribution-requirements.md) for the installer inventory and [offline Android runtime](docs/public/engineering/android-runtime.md) for the packaged sources.

## Components and attribution

| Component | Source / licence evidence | Distribution scope and obligations |
| --- | --- | --- |
| redb 5.0.0 | [Upstream](https://github.com/cberner/redb), [MIT licence](docs/public/licenses/native/redb-LICENSE-MIT.txt) | Embedded database in the Fluorine client. |
| Rhai 1.27.0 | [Upstream](https://github.com/rhaiscript/rhai), [MIT licence](docs/public/licenses/native/rhai-LICENSE-MIT.txt) | Embedded interpreter for application MCP tools. |
| Iced 0.14.0 | [Upstream](https://github.com/iced-rs/iced/tree/0.14.0), [MIT licence](docs/public/licenses/native-ui/iced-LICENSE) | Native workbench UI; locked dependencies in apps/native/Cargo.lock |
| iced_winit 0.14.1 | [Upstream revision](https://github.com/iced-rs/iced/tree/38237dd294da3256c2ac4d3f8288fc73956c91e0), [bundled MIT licence](apps/native/vendor/iced_winit/LICENSE) | Patched window integration for the native client. |
| Cryoglyph 0.1.0 | [Upstream](https://github.com/iced-rs/cryoglyph), [licences](apps/native/vendor/cryoglyph) | Native GPU text renderer. The bundled source uses lru 0.18 for its glyph cache. |
| Enigo 0.6.1 | [Upstream](https://github.com/enigo-rs/enigo), [MIT licence](docs/public/licenses/native-ui/enigo-LICENSE) | User-authorized keyboard and pointer input on local and paired desktops |
| XCap 0.9.10 | [Upstream](https://github.com/nashaofu/xcap), [Apache-2.0 licence](docs/public/licenses/native-ui/xcap-LICENSE) | Screen capture on local and paired desktops |
| rfd 0.15.4 | [Upstream](https://github.com/PolyMeilex/rfd), [MIT licence](docs/public/licenses/native-ui/rfd-LICENSE) | Native attachment open/save dialogs |
| Rodio 0.21.1 | [Upstream](https://github.com/RustAudio/rodio/tree/v0.21.1), [MIT licence](docs/public/licenses/native-ui/rodio-LICENSE.MIT) | Native device audio mixing and resampling |
| CPAL 0.16.0 | [Upstream](https://github.com/RustAudio/cpal/tree/0.16.0), [Apache-2.0 licence](docs/public/licenses/native-ui/cpal-LICENSE.APACHE) | System speaker and microphone access |
| arboard 3.7.0 | [Upstream](https://github.com/1Password/arboard), [MIT licence](docs/public/licenses/native-ui/arboard-LICENSE.MIT) | Native clipboard support |
| Lucide / Feather icons | [Upstream](https://github.com/lucide-icons/lucide), [ISC / MIT licences](docs/public/licenses/native-ui/lucide-LICENSE) | Native SVG controls adapted from the existing workbench icon family |
| pulldown-cmark 0.12.2 | [Upstream](https://github.com/pulldown-cmark/pulldown-cmark), [MIT licence](docs/public/licenses/native-ui/pulldown-cmark-LICENSE) | Shared conversation Markdown through Iced |
| open 5.4.5 | [Upstream](https://github.com/Byron/open-rs), [MIT licence](docs/public/licenses/native-ui/open-LICENSE.md) | User-activated conversation links |
| wgpu 27.0.1 | [Upstream](https://github.com/gfx-rs/wgpu/tree/v27.0.1), [MIT](docs/public/licenses/native-ui/wgpu-LICENSE.MIT) / [Apache-2.0](docs/public/licenses/native-ui/wgpu-LICENSE.APACHE) | GPU device textures and glass rendering |
| Static Linux ARM C runtime | [musl 1.2.5 copyright](docs/public/licenses/musl/COPYRIGHT), [Rust 1.96.0 build recipe and patches](https://github.com/rust-lang/rust/blob/1.96.0/src/ci/docker/scripts/musl.sh) | The Mac factory's Linux ARM desktop bridge uses Rust's musl target. The complete upstream copyright and terms are included in the native client and service notices |
| Runtime dependencies | Exact versions in Cargo.lock | [Inventory](docs/public/dependency-licenses.json): locked server and native-client build graphs, licence metadata and notice-file hashes. Redistribution/source-offer review remains open |
| EinkBro 16.7.1 Android browser | [Pinned release/source](config/phone-browser.json), [upstream notice](docs/public/licenses/guest-apps/einkbro-LICENSE.md), [GPL-3.0 text](docs/public/licenses/guest-apps/GPL-3.0.txt) | ARM64/x86_64 APKs are selected per guest and use the Android system WebView. Application data is excluded; embedded libraries retain their own notices and source requirements |
| rust-apk 1.0.0 and RustCrypto RSA 0.10.0 | [rust-apk source and licence](https://crates.io/crates/rust-apk/1.0.0), [RustCrypto RSA source and licence](https://crates.io/crates/rsa/0.10.0) | Apache-2.0 or MIT. Maintainer tools for aligning, signing and verifying Android APKs. |
| Native app icon | Original AImanity SVG, Apache-2.0; hashes in config/source-assets.json | Original project implementation; source asset identities are recorded in the asset policy. |
| Linux speech and fonts | Debian eSpeak NG, voice data and shared libraries; Noto Sans CJK Regular and Bold. | Package versions and licenses under `host/tools/licenses` and `/usr/share/doc/aimanity-workspace/noto-cjk.copyright`. [Debian eSpeak NG sources](https://sources.debian.org/src/espeak-ng/) · [Noto CJK](https://github.com/notofonts/noto-cjk) |
| Native OCR test glyphs | DejaVu Sans; Bitstream Vera grant and public-domain DejaVu changes | Generated diagnostic PNG only, no font binary. [Font notice](docs/public/licenses/fixtures/dejavu.txt); font/image hashes in config/source-assets.json |
| Linux QEMU/SSH host | Pinned Ubuntu factory packages, QEMU 8.2.2, OpenSSH, BIOS/UEFI and ELF dependencies | [Host component policy](docs/public/container-host-runtime.md). Distribution copyright files, package versions and hashes retained locally; no binaries in source exports. Complete binary attribution and corresponding-source review remain open |
| Container OCR | [Tesseract](https://github.com/tesseract-ocr/tesseract), Apache-2.0 · [Language data](config/ocr-models.json) · [Data licence](docs/public/licenses/tesseract-models/LICENSE) | The macOS workspace uses the bundled OCR container. Its distribution copyright files cover Tesseract and the native dependencies. |
| Offline speech recognition model | OpenAI Whisper Base, Q5_1 conversion from [ggerganov/whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp/tree/5359861c739e955e79d9a303bcbc70fb988958b1); [MIT licence](docs/public/licenses/whisper/LICENSE), [download pin](config/speech-model.json) | Both clients include the multilingual model at `host/speech/ggml-base-q5_1.bin` and retain the upstream copyright and permission notice |
| Whisper Rust bindings | whisper-rs 0.16.0 and whisper-rs-sys 0.15.0, [source revision](https://codeberg.org/tazz4843/whisper-rs/src/commit/7558e1b72f54f2f22a53589afb77e65681834c36), [Unlicense](docs/public/licenses/native/whisper-rs-LICENSE) | Native clients statically link the Rust bindings and retain their shared public-domain dedication |
| whisper.cpp and ggml | Vendored in the [whisper-rs-sys 0.15.0 crate](https://crates.io/crates/whisper-rs-sys/0.15.0), [MIT licence](docs/public/licenses/native/whisper.cpp-LICENSE), copyright 2023–2024 The ggml authors | Native clients statically link this inference code and include its original copyright and permission notice |
| Windows offline OCR | [Debian Tesseract source](https://tracker.debian.org/pkg/tesseract) · [English and Simplified Chinese language data source](https://tracker.debian.org/pkg/tesseract-lang) | The Windows Linux service bundles the native ELF dependencies, language data and package copyright files. `runtime/services/ocr/source-records.json` lists copied files and the exact factory package versions. [Factory](docs/public/engineering/linux-offline-ocr.md) |
| Offline Android | [AOSP, Avo, QEMU and ADB sources](docs/public/engineering/android-runtime.md) | The Android capsule includes original notices, source and build manifests, and kernel configuration. |
| Paired Android device selection | [Genymobile scrcpy device selection](https://github.com/Genymobile/scrcpy/blob/master/doc/connection.md), [AOSP ADB services](https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/docs/dev/services.md) | AImanity’s Rust gateway selects an authorized ADB transport by serial and transport ID. The pairing path uses the phone’s existing ADB shell and screenshot service. |
| Remote sharing transport | [OpenSSH portable](https://github.com/openssh/openssh-portable), [ssh manual](https://man.openbsd.org/ssh#W) | The native client uses the installed SSH program for authenticated stdio forwarding to the workspace loopback service. |
| Android playback on Windows | [scrcpy server 3.3.4](https://github.com/Genymobile/scrcpy/tree/v3.3.4), [Apache-2.0](https://github.com/Genymobile/scrcpy/blob/v3.3.4/LICENSE) | The Android capsule includes Genymobile’s unmodified server and its licence. |
| mise 2026.10.22 | [Official source / MIT](https://github.com/jdx/mise/tree/v2026.10.22) | Managed-host development downloads the pinned executable separately; not bundled in the client. Retain upstream notices in the installed tool distribution |
| Lima 2.2.1 and Alpine 3.24.1 | [Lima source and licence](https://github.com/lima-vm/lima/tree/27bc4c4b101e27994e01e36566a0f2091bbca507) · [Alpine-Lima v0.2.50 image build](https://raw.githubusercontent.com/lima-vm/alpine-lima/v0.2.50/Makefile) · [Alpine package build recipes](https://github.com/alpinelinux/aports) | The macOS bundle includes Lima’s Apache-2.0 licence, the ARM64 ISO and signed Alpine packages listed in the [package policy](config/container-host-runtime.json). The ISO boots Linux 6.18.38-0-virt. |
| Go 1.26.9 factory compiler | [Go source / BSD-3-Clause and notices](https://go.googlesource.com/go/+/refs/tags/go1.26.9/LICENSE) | Maintainer-only private compiler; never a global user prerequisite. Preserve Go/runtime/module notices in the resulting Lima distribution |
| Extra Android apps | Separately installed APKs | No APK redistribution in source bundles; pin source, version, signature and licence before automating downloads |
| Debian guest packages | [Debian source archive](https://sources.debian.org/) | The shared root filesystem and Windows offline tools include distribution copyright files and package inventory. |
| Linux guest application appearance | [Ubuntu Yaru](https://github.com/ubuntu/yaru), [GNOME Files](https://gitlab.gnome.org/GNOME/nautilus), [GNOME Text Editor](https://gitlab.gnome.org/GNOME/gnome-text-editor), [LibreOffice Breeze icons](https://github.com/LibreOffice/core/tree/master/icon-themes/breeze) | The guest includes Debian's Yaru Blue GTK 3 assets with their package copyright in `/usr/share/doc/yaru-theme-gtk/`. Files, Text Editor and Breeze icons retain their Debian package notices. |
| Dunst and libnotify | [Dunst / BSD-3-Clause](https://github.com/dunst-project/dunst), [libnotify / LGPL-2.1-or-later](https://gitlab.gnome.org/GNOME/libnotify) | Included in the Linux desktop image for notifications. Retain distribution copyright files and applicable source/redistribution obligations |
| Redis 8 | [Redis licences](https://redis.io/legal/licenses/) | macOS installs the bundled Alpine package. The Windows guest includes Redis from Debian, with the package copyright and version in its offline inventory. |
| Mihomo v1.19.33 | [Corresponding source and GPLv3 licence](https://github.com/MetaCubeX/mihomo/tree/v1.19.33), [platform pins and licence](config/mihomo-release.json) | Fluorine packages the upstream executable for Apple Silicon, Windows x64, Linux x64 and each Android APK ABI. The workspace starts it on demand. |
| GeoIP routing data | [MetaCubeX/meta-rules-dat](https://github.com/MetaCubeX/meta-rules-dat), immutable commit and hash in config/mihomo-release.json | Downloaded separately, excluded from source archives; retain upstream licence and data-source obligations in binary distribution |

Binary releases include the component notices and applicable corresponding-source references.


The native client bundles licence texts generated from the locked Rust dependencies for each platform.

The native-client notice supplements record source URLs, exact revisions or archive checksums, and licence text hashes in `config/native-notice-supplements.json`. The objc2 grant includes the original Apple SDK provision, MIT text and attribution.

## Source availability and Linux system libraries

The Alpine ISO lists package names, licences and aports commits in `apks/aarch64/APKINDEX.tar.gz`. Each separate APK records its `origin` and `commit` in `.PKGINFO`. Use those fields to open the corresponding `APKBUILD` and its patches in aports. [BusyBox 1.37.0-r31 build recipe](https://github.com/alpinelinux/aports/blob/c3ef5d10e6ef6528852c51f0564963e2f8c1be19/main/busybox/APKBUILD)


Complete installers must include these attribution texts together with the notices and source access required by their runtime images and server dependencies. Signing and installation acceptance are reported separately in release status. Component terms and copyright notices apply independently of Apache-2.0.

## Rust standard-library runtime

The clients are built with Rust 1.96.0. Its standard-library copyright collection and 13 accompanying licence/attribution files are pinned in config/rust-runtime-notices.json, verified against the installed official component manifest and supplied in the native notice collection. The full compiler is not bundled. Changing the toolchain requires a runtime-notice review.

### Location interface

OpenStreetMap map data is attributed in the map and governed by [ODbL](https://www.openstreetmap.org/copyright); hosted tiles follow the provider's [tile usage policy](https://operations.osmfoundation.org/policies/tiles/). Tiles and the optional network-location lookup are contacted only after simulated location is enabled. No FakeLocation source or application binary is bundled.

## Local WhatsApp connection

WhatsApp uses the MIT-licensed `whatsapp-rust` 0.7.0 linked-device implementation. Its transitive dependencies and required notices must be included in the generated binary inventories.

The linked-device adapter is unofficial. The platform's terms apply to pairing and delivery. Session databases, linked-device state and QR payloads remain private user data. Chromium in the Linux desktop retains its own notices and source obligations.


## Installer runtime

Windows and macOS clients use the native Rust workbench. See [runtime bundles](docs/public/runtime-bundles.md).


Windows installers are produced by unmodified Inno Setup 7.1.1. Its original installer copyright and notices are preserved; the [full upstream grant](docs/public/licenses/windows-installer/LICENSE) is included in the source. Compiler and InnoUnp 2.71.3 source/download identities are pinned in config/windows-installer.json. The build enables the pinned x86 unpacker’s PE large-address flag after archive verification. The compiler and unpacker are maintainer tools, excluded from the installed application runtime.

## Media and channel runtime component


## Native host and guest services

The Linux phone bridge includes a verbatim AOSP `emulator_controller.proto` schema from `platform/prebuilts/android-emulator` commit `bbe98768a47ce9166f768e791768ae5f066c04df`. Its original 2018 Android Open Source Project copyright and Apache-2.0 header are preserved. [Exact provenance](packages/protocol/vendor/android-emulator/provenance.json) records the upstream URL, Git blob identity, byte count and SHA-256 digest. The Linux phone bridge uses the independent Android tools release, with its corresponding source information and notice.

The [native service licence collection](docs/public/native-services-licenses.txt) and [inventory](docs/public/native-services-license-inventory.json) identify the locked Rust service dependencies, full terms, copyright notices and source references. The Linux guest image retains these materials under `/usr/share/doc/ai-phone/`.

Image processing enables PNG in image 0.25.10. The x11rb protocol bindings retain the separate xcb-proto copyright and permission notice. The host archive decoder links liblzma through xz2 and lzma-sys; the collection preserves the vendored XZ terms separately from the Rust wrapper licences. XZ command-line programs and shell utilities are not part of this static library build. This software includes code from [XZ Utils](https://tukaani.org/xz/).

## Development Skill references

The offline Skill library includes selected MIT-licensed Markdown references from the following projects. Each corresponding Skill directory includes its upstream copyright notice and license.

- GSAP guides from [GreenSock](https://github.com/greensock/gsap-skills) for animation.
- Backend and PostgreSQL guides from [Everything Claude Code](https://github.com/affaan-m/ECC).
- Interface critique and typography references from [Hallmark](https://github.com/Nutlope/hallmark) and [UI UX Pro Max](https://github.com/nextlevelbuilder/ui-ux-pro-max-skill).
- Interaction and spring references from UI Interaction Kit by ChangerXu.
- Code review references from [simplify-codebase](https://github.com/tt-a1i/simplify-codebase).
