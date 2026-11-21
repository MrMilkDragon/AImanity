//! Transfer the retired Windows 0.0.4 input identity through Android Package Manager.
use super::{Device, Error, Result, VERSION, package_version};

pub(super) const IDENTITY: &str = r#"set -eu
apk=$(pm path --user 0 dev.aiphone.input)
sha256sum "${apk#package:}" | cut -d' ' -f1"#;
const OLD_APK: &str = "3d40af210b0cce17e651a6d5da52a8b0fc5a945c0644673cbe9340c03146a37d";

pub(super) async fn matches(adb: &mut impl Device) -> Result<bool> {
    Ok(adb.execute(&["shell", "sh", "-c", IDENTITY], 10).await? == OLD_APK)
}

const BACKUP: &str = r#"set -eu
umask 077
save=/data/misc/aimanity-input-upgrade
pkg=dev.aiphone.input
data=/data/user/0/$pkg
apk=$(pm path --user 0 "$pkg")
apk=${apk#package:}
[ "$(sha256sum "$apk" | cut -d' ' -f1)" = "$1" ]
if [ ! -f "$save/complete" ]; then
  mkdir -p "$save"
  pm disable-user --user 0 "$pkg" >/dev/null
  cp "$apk" "$save/previous.apk"
  cmp "$apk" "$save/previous.apk"
  tar --exclude=./cache --exclude=./code_cache --exclude=./lib -C "$data" -cf "$save/user.tar" .
  tar -tf "$save/user.tar" >/dev/null
  touch "$save/complete"
  sync
fi
pm uninstall --user 0 "$pkg""#;

const RESTORE: &str = r#"set -eu
save=/data/misc/aimanity-input-upgrade
data=/data/user/0/dev.aiphone.input
test -f "$save/complete"
if [ ! -f "$save/restored" ]; then
  uid=$(stat -c %u "$data")
  cat "$save/user.tar" | su "$uid,$uid" tar -C "$data" -xf -
  restorecon -RF "$data"
  touch "$save/restored"
  sync
fi"#;

pub(super) async fn transfer(
    adb: &mut impl Device,
    installed: Option<u64>,
    apk: &str,
) -> Result<()> {
    if installed == Some(0) {
        let result = adb
            .execute(
                &["shell", "su", "0", "sh", "-c", BACKUP, "upgrade", OLD_APK],
                30,
            )
            .await?;
        if result != "Success" {
            return Err(Error::Invalid("phone_input_backup"));
        }
    }
    adb.execute(
        &[
            "shell",
            "su",
            "0",
            "test",
            "-f",
            "/data/misc/aimanity-input-upgrade/complete",
        ],
        5,
    )
    .await?;
    adb.execute(
        &["install", "--no-incremental", "-r", "--user", "0", apk],
        60,
    )
    .await?;
    if package_version(adb).await?.is_none_or(|v| v < VERSION) {
        return Err(Error::Invalid("phone_input_install_unknown"));
    }
    adb.execute(&["shell", "su", "0", "sh", "-c", RESTORE], 30)
        .await?;
    Ok(())
}

pub(super) async fn finish(adb: &mut impl Device) -> Result<()> {
    let remove = "set -eu; save=/data/misc/aimanity-input-upgrade; [ -e \"$save\" ] || exit 0; cd \"$save\"; rm -f previous.apk user.tar complete restored; rmdir \"$save\"";
    adb.execute(&["shell", "su", "0", "sh", "-c", remove], 5)
        .await?;
    Ok(())
}
