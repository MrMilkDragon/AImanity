use ai_phone_api::wire;
pub fn command(fix: &wire::LocationFix) -> Option<Vec<String>> {
    ai_phone_host_runtime::android::location::shell(fix).map(|script| vec!["shell".into(), script])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn location_legacy_marker_can_be_disabled_after_partial_cleanup() {
        use std::{fs, os::unix::fs::PermissionsExt, process::Command};
        let folder =
            std::env::temp_dir().join(format!("aiphone-location-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&folder).unwrap();
        let marker = folder.join("before");
        fs::write(&marker, "true\n").unwrap();
        for name in ["cmd", "appops"] {
            let script = folder.join(name);
            fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
            fs::set_permissions(script, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let body = command(&wire::LocationFix::default()).unwrap()[1].replace(
            "/data/local/tmp/ai-phone-location-before",
            marker.to_str().unwrap(),
        );
        let result = Command::new("/bin/sh")
            .arg("-c")
            .arg(body)
            .env(
                "PATH",
                format!("{}:{}", folder.display(), std::env::var("PATH").unwrap()),
            )
            .output()
            .unwrap();
        let retained = marker.exists();
        fs::remove_dir_all(folder).unwrap();
        assert!(
            result.status.success(),
            "optional legacy metadata must not interrupt cleanup"
        );
        assert!(!retained, "no stale ownership marker after disabling");
    }

    #[test]
    fn location_recovers_partial_provider_setup_and_guest_reboot_and_restores_original_settings() {
        use std::{fs, os::unix::fs::PermissionsExt, process::Command};
        let folder =
            std::env::temp_dir().join(format!("aiphone-gps-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&folder).unwrap();
        for name in ["cmd", "appops"] {
            let path = folder.join(name);
            fs::write(
                &path,
                r#"#!/bin/sh
set -eu
cd "$LOCATION_FIXTURE"
case "${0##*/}:$1:$2" in
  appops:get:*) printf 'Uid mode: MOCK_LOCATION: '; cat appop; printf '\n' ;;
  appops:*) for value do :; done; printf '%s' "$value" > appop ;;
  cmd:location:is-location-enabled) cat enabled; printf '\n' ;;
  cmd:location:set-location-enabled) printf '%s' "$3" > enabled ;;
  *)
    file="provider-$4"
    case "$3" in
      add-test-provider)
        if [ "$4" = network ] && [ -f fail-network ]; then rm fail-network; exit 42; fi
        : > "$file" ;;
      remove-test-provider) rm -f "$file" ;;
      *)
        test -f "$file" || exit 43
        if [ "$3" = set-test-provider-location ]; then printf '%s' "$6" > fix; fi ;;
    esac ;;
esac
"#,
            )
            .unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(folder.join("enabled"), "false").unwrap();
        fs::write(folder.join("appop"), "ignore").unwrap();
        fs::write(folder.join("boot"), "first").unwrap();
        fs::write(folder.join("fail-network"), "").unwrap();
        let fix = wire::LocationFix {
            enabled: true,
            point: Some(wire::GeoPoint {
                latitude: 31.,
                longitude: 121.,
                accuracy_m: 5.,
            }),
            ..Default::default()
        };
        let run = |fix: &wire::LocationFix| {
            let body = command(fix).unwrap()[1]
                .replace(
                    "/data/local/tmp/ai-phone-location-before",
                    folder.join("before").to_str().unwrap(),
                )
                .replace(
                    "/proc/sys/kernel/random/boot_id",
                    folder.join("boot").to_str().unwrap(),
                );
            Command::new("/bin/sh")
                .args(["-c", &body])
                .env("LOCATION_FIXTURE", &folder)
                .env(
                    "PATH",
                    format!("{}:{}", folder.display(), std::env::var("PATH").unwrap()),
                )
                .output()
                .unwrap()
                .status
                .success()
        };
        assert!(!run(&fix));
        assert!(!folder.join("before.ready").exists());
        assert!(run(&fix));
        assert_eq!(fs::read_to_string(folder.join("fix")).unwrap(), "31,121");
        for name in ["provider-gps", "provider-network"] {
            fs::remove_file(folder.join(name)).unwrap();
        }
        fs::write(folder.join("boot"), "second").unwrap();
        assert!(run(&fix));
        assert!(run(&wire::LocationFix::default()));
        assert_eq!(
            fs::read_to_string(folder.join("enabled")).unwrap().trim(),
            "false"
        );
        assert_eq!(fs::read_to_string(folder.join("appop")).unwrap(), "ignore");
        assert!(!folder.join("before").exists());
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn rejects_nonfinite_and_out_of_bounds_fixes() {
        let mut fix = wire::LocationFix {
            enabled: true,
            point: Some(wire::GeoPoint {
                latitude: 31.2,
                longitude: 121.5,
                accuracy_m: 5.,
            }),
            ..Default::default()
        };
        assert!(command(&fix).unwrap()[1].contains("31.2,121.5"));
        fix.point.as_mut().unwrap().longitude = 181.;
        assert!(command(&fix).is_none());
        fix.point.as_mut().unwrap().longitude = f64::NAN;
        assert!(command(&fix).is_none());
        fix.enabled = false;
        assert!(command(&fix).unwrap()[1].contains("remove-test-provider"));
    }
}
