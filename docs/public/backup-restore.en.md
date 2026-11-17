# Private backup and recovery

[简体中文](backup-restore.zh-CN.md)

Run this command from the installation to save PostgreSQL, the Linux writable layer, and private device and workspace data. The backup pauses the installation's services and resumes them after capture. Keep the complete `.backup` directory in private encrypted storage.

```sh
./ai-phone backup --output data/backups/my-backup.backup
```

Restore to an empty directory and a dedicated PostgreSQL 17 database. Prepare a private configuration directory containing the target administrator's `.env` and its local PostgreSQL socket. Supply the matching Android SDK path.

```sh
assistant --state-backup restore --root /private/restore-config \
  --pg-socket /private/restore-config/socket --database ai_phone_recovered \
  --sdk-root /private/android-sdk --archive /private/my-backup.backup \
  --destination /private/AImanity-recovered
```

The restore verifies database rows and files, relocates Android disk paths, and pauses tasks and notification triggers. The report and quarantine marker are under `data/`. The Linux home is restored with the private files.

To restore the Linux writable layer, prepare a separate native guest with an empty desktop layer, the original shared filesystem, and the restored `data` mounted as its installation data. While the desktop is stopped, run:

```sh
assistant --state-backup recover-desktop --root /private/native-guest-release \
  --destination /private/AImanity-recovered
```

The writable layer retains ownership, permissions, deletion markers and extended attributes. The restored installation stays quarantined. Stop the original instance before verifying and activating the recovered copy.
