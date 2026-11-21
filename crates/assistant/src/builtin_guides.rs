pub(crate) fn for_application(application: &str) -> Option<&'static str> {
    Some(match application {
        "com.tencent.mm" => "wechat",
        "info.plateaukao.einkbro" => "browser-android",
        "com.android.documentsui" => "files-android",
        "com.xingin.xhs" => "xiaohongshu",
        "linux:libreoffice-writer" => "writer",
        "linux:libreoffice-calc" => "calc",
        "linux:libreoffice-impress" => "impress",
        "linux:libreoffice-draw" => "draw",
        _ => return None,
    })
}

#[cfg(test)]
pub(crate) fn test_files(root: &std::path::Path) {
    fn copy(source: &std::path::Path, target: &std::path::Path) {
        std::fs::create_dir_all(target).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = target.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/builtin");
    copy(&source, &root.join("skills/builtin"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guides_follow_observed_application_ids() {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/builtin");
        for app in [
            "com.xingin.xhs",
            "com.tencent.mm",
            "com.android.documentsui",
            "info.plateaukao.einkbro",
            "linux:libreoffice-calc",
            "linux:libreoffice-draw",
            "linux:libreoffice-impress",
            "linux:libreoffice-writer",
        ] {
            assert!(crate::application_identity::valid(app));
            assert!(
                source
                    .join(for_application(app).unwrap())
                    .join("SKILL.md")
                    .is_file()
            );
        }
        assert_eq!(for_application("com.xingin.xhs.fake"), None);
        assert_eq!(for_application("a private file — LibreOffice Writer"), None);
    }
}
