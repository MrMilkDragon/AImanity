fn main() {
    use std::{env, fs, path::PathBuf};
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
