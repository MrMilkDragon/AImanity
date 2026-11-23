# Source builds and contribution

[简体中文](source-release.zh-CN.md) · [Repository](https://github.com/MrMilkDragon/AImanity)

The Rust client, model adapter and embedded workspace live in `apps/native` and `crates/model`. Model profiles, messages, queued work and knowledge use the workspace database.

Original code uses [Apache-2.0](../../LICENSE). Dependencies, assets and guest applications retain their own terms; see [third-party notices](../../THIRD_PARTY_NOTICES.md). [Download the macOS or Windows client](release-status.md).

## Prerequisites and startup

Run the Rust client from the repository. Device runtimes are optional when developing chat or storage.

```sh
cargo run --locked --manifest-path apps/native/Cargo.toml --bin aimanity-native
```

In Agent settings, save your model endpoint and select the active Agent.

See [setup](getting-started.en.md), [runtime components](runtime-bundles.md), [architecture](architecture.en.md), [backup and recovery](backup-restore.en.md) and [release status](release-status.md).

## Source and user data

Source export uses an allowlist with privacy and licence checks. [Contribution guide](../../CONTRIBUTING.md)

```sh
cargo run --quiet --locked -p ai-phone-release-tools -- source check
cargo run --quiet --locked -p ai-phone-release-tools -- source export /path/to/new-review-directory --git
```

Source export creates a review repository in a new directory.

## Automated checks

Source CI builds the protobuf and Rust targets. The native workspace test checks the message and attachment after a restart, then quits in the middle of a model request and checks the queued job on reopen.

Native CI builds macOS ARM64 and Windows x64 desktop clients, inspects package contents and verifies virtual-phone control applications on matching workers. See [runtime components](engineering/index.md), [release status](release-status.md) and [compatibility](compatibility.md).
