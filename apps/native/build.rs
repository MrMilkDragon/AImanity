fn main() {
    use std::{env, fs, path::PathBuf};
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../skills/builtin");
    println!("cargo::rerun-if-changed={}", root.display());
    let core = include_str!("../../skills/core.txt");
    let mut entries = Vec::new();
    for entry in fs::read_dir(root).expect("read built-in Skills") {
        let entry = entry.expect("read Skill directory");
        if !entry.file_type().unwrap().is_dir() {
            continue;
        }
        let id = entry.file_name().into_string().unwrap();
        let body = fs::read_to_string(entry.path().join("SKILL.md")).expect("read SKILL.md");
        let description = body
            .lines()
            .skip(1)
            .take_while(|s| *s != "---")
            .find_map(|s| s.strip_prefix("description: "))
            .expect("SKILL.md description");
        let required = core.lines().any(|name| name == id);
        entries.push(format!(r#"Skill {{ id: {id:?}, description: {description:?}, core: {required}, body: include_str!(concat!(env!("CARGO_MANIFEST_DIR"),"/../../skills/builtin/{id}/SKILL.md")) }}"#));
    }
    entries.sort_unstable();
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("skills.rs"),
        format!("&[{}]", entries.join(",")),
    )
    .expect("embed built-in Skills");
    println!("cargo::rerun-if-env-changed=AIMANITY_BUNDLE_INTEGRITY");
    let inventory = match env::var_os("AIMANITY_BUNDLE_INTEGRITY") {
        Some(path) => {
            let path = PathBuf::from(path);
            println!("cargo::rerun-if-changed={}", path.display());
            fs::read(path).expect("read the package resource inventory")
        }
        None => b"{}".to_vec(),
    };
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("integrity.json"),
        inventory,
    )
    .expect("embed the package resource inventory");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let plist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Info.plist");
        println!("cargo::rerun-if-changed={}", plist.display());
        println!(
            "cargo::rustc-link-arg-bin=aimanity-native=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            plist.display()
        );
    }
}
