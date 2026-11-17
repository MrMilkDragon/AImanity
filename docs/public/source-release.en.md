# Source builds and contribution

[简体中文](source-release.zh-CN.md) · [Repository](https://github.com/MrMilkDragon/AImanity)

The source contains the Rust workbench, Rust execution services, Android bridges, Linux desktop recipe, protobuf contracts, built-in Skills, installation/backup/recovery tools, tests and CI workflows. The phone and computer share the selected Agent. PostgreSQL persists task checkpoints; Redis provides caching.

Original code uses [Apache-2.0](../../LICENSE). Dependencies, assets and guest applications retain their own terms; see [third-party notices](../../THIRD_PARTY_NOTICES.md). [Download the macOS or Windows client](release-status.md).

## Prerequisites and startup

Use Rust 1.96.0, Android 35 native build tools and Xcode Command Line Tools on Apple Silicon. The [macOS factory](engineering/macos-full-runtime.md) prepares the offline device resources.

```sh
./ai-phone doctor
./ai-phone install --resources /path/to/host
```

The host defaults to direct networking. Import a subscription in the workbench proxy when needed. Save model profiles and select the active profile in Configuration → Agent.

See [setup](getting-started.en.md), [runtime components](runtime-bundles.md), [architecture](architecture.en.md), [backup and recovery](backup-restore.en.md) and [release status](release-status.md).

## Source and user data

Source export uses an allowlist with privacy and licence checks. [Contribution guide](../../CONTRIBUTING.md)

```sh
cargo run --quiet --locked -p ai-phone-release-tools -- source check
cargo run --quiet --locked -p ai-phone-release-tools -- source export /path/to/new-review-directory --git
```

Export requires a new destination and produces a source manifest and local review repository without a remote. Review the exported diff against the public checkout before committing. Ongoing contributions preserve public history. Do not update a running user's state with a direct Git checkout; use versioned artifacts, backup, migration and recovery.

## Automated checks

Source CI checks protobuf, Rust, native workbench, adapters and archive/installation contracts. Prebuilt component verification stages and executes actual packaged files. Disposable PostgreSQL fixtures exercise migrations, role restrictions, backup/restore and worker recovery; restart tests run sequentially. CI does not use real devices, account credentials or paid model calls.

Native CI builds macOS ARM64 and Windows x64 desktop clients, inspects package contents and verifies virtual-phone control applications on matching workers. See [runtime components](engineering/index.md), [release status](release-status.md) and [compatibility](compatibility.md).
