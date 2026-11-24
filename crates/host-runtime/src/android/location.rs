//! Mock providers are owned by this feature and removed when it is disabled.
use ai_phone_api::wire;
pub async fn apply(client: &ai_phone_adb::Client, fix: &wire::LocationFix) -> Result<(), String> {
    let script = shell(fix).ok_or("定位坐标无效")?;
    let result = client
        .shell(
            &script,
            ai_phone_adb::Limits {
                stdout: 256,
                stderr: 1024,
            },
            std::time::Duration::from_secs(8),
        )
        .await
        .map_err(|e| e.to_string())?;
    if result.code != 0 {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_owned());
    }
    let expected: &[u8] = if fix.enabled {
        b"location-applied"
    } else {
        b"location-disabled"
    };
    if !result.stdout.trim_ascii_end().ends_with(expected) {
        return Err("安卓未确认定位".into());
    }
    Ok(())
}
pub fn shell(fix: &wire::LocationFix) -> Option<String> {
    let body = if fix.enabled {
        let p = fix.point.as_ref()?;
        if !p.latitude.is_finite()
            || !(-90.0..=90.0).contains(&p.latitude)
            || !p.longitude.is_finite()
            || !(-180.0..=180.0).contains(&p.longitude)
            || !p.accuracy_m.is_finite()
            || !(0.0..=100_000.0).contains(&p.accuracy_m)
        {
            return None;
        }
        format!(
            r#"set -e
marker=/data/local/tmp/ai-phone-location-before
if [ ! -f "$marker" ]; then
  umask 077
  cmd location is-location-enabled > "$marker.new"
  appops get 0 android:mock_location | sed -n 's/.*MOCK_LOCATION: \([a-z]*\).*/\1/p' | head -1 > "$marker.appop"
  mv "$marker.new" "$marker"
fi
boot="$(cat /proc/sys/kernel/random/boot_id)"
if [ ! -f "$marker.ready" ] || [ "$(cat "$marker.boot" 2>/dev/null || true)" != "$boot" ]; then
  appops set 0 android:mock_location allow
  cmd location providers add-test-provider gps --requiresSatellite --supportsAltitude --supportsSpeed --supportsBearing
  cmd location providers add-test-provider network --requiresNetwork
  printf '%s' "$boot" > "$marker.boot"
  touch "$marker.ready"
fi
cmd location set-location-enabled true
for provider in gps network; do
  cmd location providers set-test-provider-enabled "$provider" true
  cmd location providers set-test-provider-location "$provider" --location {},{} --accuracy {}
done
printf 'location-applied'
"#,
            p.latitude,
            p.longitude,
            p.accuracy_m.max(1.0)
        )
    } else {
        r#"set -e
marker=/data/local/tmp/ai-phone-location-before
if [ -f "$marker" ]; then
  rm -f "$marker.ready"
  cmd location providers remove-test-provider gps
  cmd location providers remove-test-provider network
  cmd location set-location-enabled "$(cat "$marker")"
  previous="$(cat "$marker.appop" 2>/dev/null || true)"
  case "$previous" in allow|ignore|deny|foreground) ;; *) previous=default ;; esac
  appops set 0 android:mock_location "$previous"
  rm -f "$marker.appop" "$marker.boot"
  rm "$marker"
fi
printf 'location-disabled'
"#
        .to_string()
    };
    Some(body)
}
