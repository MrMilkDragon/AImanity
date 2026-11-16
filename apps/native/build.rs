fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let plist =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../client/src-tauri/Info.plist");
        println!("cargo::rerun-if-changed={}", plist.display());
        println!(
            "cargo::rustc-link-arg-bin=aimanity-native=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            plist.display()
        );
    }
}
