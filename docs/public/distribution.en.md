# Clients, runtimes, user data and updates

[简体中文](distribution.zh-CN.md)

The installer supplies the devices and services. Configure resources and models in the client. See [downloads and host requirements](release-status.md).

## Implemented storage boundaries

Native setup uses its application data directory, with an owned `host` subdirectory:

```text
host/
  deployment.json             setup checkpoint
  tools/                      owned development bootstrap tools
  releases/<payload-sha256>/  verified versioned code and runtime component
    data -> host/user/data
    .env -> host/user/.env
  user/
    .env                      private database environment
    data/                     guests, account/model/channel state, files
```

`crates/host-runtime/src/source_cli.rs` stages the versioned [runtime components](runtime-bundles.md) and validates the links to persistent user data.

The contributor staging command uses the same native, content-addressed source stager.

PostgreSQL uses private storage in the Linux guest. Backups capture the database, Android state, Linux changes and home directory, and private files. Restore writes to an empty target and leaves it quarantined for validation. User Skills, learned experience, memory, knowledge, attachments and disabled-Skill preferences persist across updates.

## Native client updates

macOS and Windows download their installer through the workbench and verify its Ed25519 manifest and digest. macOS replaces the app bundle. Windows runs the Inno installer. Both reopen the updated client. Linux users install the newer DEB or RPM with the same package manager. Linux keeps devices and conversations under `$XDG_DATA_HOME/dev.aimanity.native/host`, which defaults to `~/.local/share/dev.aimanity.native/host`. [Linux installation](release-status.md#linux)

After verifying both installers, sign the update manifest on the release maintainer's machine:

```sh
cargo run --quiet --locked -p ai-phone-release-tools -- sign-update /path/to/offline-key \
  AImanity-0.0.5-macos-arm64.dmg AImanity-0.0.5-windows-x64-setup.exe native-stable.json
```

For sequential builds, keep both installer receipts and one installer locally. Pipe the other installer into `sign-update --stdin macos` or `sign-update --stdin windows` before the four usual arguments. The streamed installer filename locates its adjacent receipt. Publish the shared update feed after both downloads are live.

Attach the DMG, EXE, DEB and RPM to the GitHub release. Publish the macOS and Windows `native-stable.json` on the `update-feed` branch.

Full packages must contain clean Android/Linux baselines, emulator, prebuilt services and matched runtime components, with provenance, notices and applicable corresponding source. See [distribution requirements](distribution-requirements.md), [release status](release-status.md), and [backup/recovery](backup-restore.en.md).
