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

The client downloads the stable release for its platform and verifies its Ed25519 manifest and installer digest. macOS replaces the application bundle and reopens it. Windows runs the existing Inno installer and relaunches the installed client. The persistent host directory stays outside the application bundle and installer payload. Windows updates reuse the pinned QEMU base and existing writable disk.

After verifying both installers, sign the update manifest on the release maintainer's machine:

```sh
cargo run --quiet --locked -p ai-phone-release-tools -- sign-update /path/to/offline-key \
  AImanity-0.0.4-macos-arm64.dmg AImanity-0.0.4-windows-x64-setup.exe native-stable.json
```

Attach the DMG and EXE to the GitHub release. Publish the signed `native-stable.json` on the `update-feed` branch. To move from Calcium to Dysprosium, first uninstall 0.0.3, then download and install 0.0.4 from Releases. The updater handles later releases from the Dysprosium installation.

Full packages must contain clean Android/Linux baselines, emulator, prebuilt services and matched runtime components, with provenance, notices and applicable corresponding source. See [distribution requirements](distribution-requirements.md), [release status](release-status.md), and [backup/recovery](backup-restore.en.md).
