//! Apply the source release's desktop helper without rebuilding the guest or touching its home.
use crate::Result;
use std::{io::ErrorKind, path::Path};

const INSTALL: &str = r#"
target=/usr/local/bin/desktop-bridge
temporary=$(mktemp /usr/local/bin/.desktop-bridge.XXXXXXXX)
trap 'rm -f "$temporary"' EXIT
cat > "$temporary"
if cmp -s "$temporary" "$target"; then exit 0; fi
chmod 0755 "$temporary"
"$temporary" --version >/dev/null
mv -f "$temporary" "$target"
"#;

pub async fn staged(root: &Path) -> Result<Option<Vec<u8>>> {
    match tokio::fs::read(root.join("runtime/services/bin/desktop-bridge")).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn install_args(container: &str) -> [&str; 9] {
    [
        "exec", "-i", "-u", "0", container, "/bin/sh", "-eu", "-c", INSTALL,
    ]
}
