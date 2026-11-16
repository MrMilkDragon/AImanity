use crate::{Error, Result, filesystem};
use std::{fs, path::Path};

pub(crate) fn symbolic(target: &Path, link: &Path) -> Result<()> {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link)?;
    #[cfg(windows)]
    {
        let resolved = if target.is_absolute() {
            target.to_owned()
        } else {
            link.parent()
                .ok_or(Error::Invalid("source_resource_link"))?
                .join(target)
        };
        if resolved.is_dir() {
            std::os::windows::fs::symlink_dir(target, link)?;
        } else {
            std::os::windows::fs::symlink_file(target, link)?;
        }
    }
    Ok(())
}

fn ensure(link: &Path, target: &Path, original: &Path, create: bool) -> Result<()> {
    match fs::symlink_metadata(link) {
        Ok(metadata) => verify_existing_link(link, target, &metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
            create_link(link, original)
        }
        Err(error) => Err(error.into()),
    }
}

fn verify_existing_link(link: &Path, target: &Path, metadata: &fs::Metadata) -> Result<()> {
    if !metadata.file_type().is_symlink() {
        return Err(Error::Invalid("source_resource_link_replaced"));
    }
    if filesystem::normalized_target(link, 0)? != target {
        return Err(Error::Invalid("source_resource_link_changed"));
    }
    Ok(())
}

fn create_link(link: &Path, original: &Path) -> Result<()> {
    filesystem::directory(
        link.parent()
            .ok_or(Error::Invalid("source_resource_link"))?,
        true,
    )?;
    symbolic(original, link)
}

pub(crate) fn persistent(root: &Path, home: &Path, create: bool) -> Result<()> {
    for (name, target) in [
        ("data", home.join("user/data")),
        (".env", home.join("user/.env")),
    ] {
        let expected = filesystem::normalized_target(&target, 0)?;
        ensure(&root.join(name), &expected, &target, create)?;
    }
    Ok(())
}
