use crate::{Error, Result};

const ALUMINUM: (&str, u64) = (
    "348baa7207524397b74de603f08357b6d3b73947514eb9f662c6eb4e52a365a5",
    1_101_660_160,
);

pub(crate) fn select<'a>(
    bundle: (&'a str, u64),
    installed_size: u64,
    overlay_identity: Option<&[u8]>,
    release_layer: bool,
) -> Result<&'a str> {
    [Some(bundle), release_layer.then_some(ALUMINUM)]
        .into_iter()
        .flatten()
        .find(|(identity, size)| {
            *size == installed_size && overlay_identity.is_none_or(|old| old == identity.as_bytes())
        })
        .map(|(identity, _)| identity)
        .ok_or(Error::Invalid("vm_disk_base_changed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_layers_preserve_the_backing_disk_selected_by_the_overlay() {
        let calcium = ("calcium", 214_106_112);
        assert_eq!(
            select(calcium, calcium.1, Some(calcium.0.as_bytes()), true).unwrap(),
            calcium.0
        );
        assert_eq!(
            select(calcium, ALUMINUM.1, Some(ALUMINUM.0.as_bytes()), true).unwrap(),
            ALUMINUM.0
        );
        assert!(select(calcium, ALUMINUM.1, Some(calcium.0.as_bytes()), true).is_err());
        assert!(select(calcium, calcium.1, Some(ALUMINUM.0.as_bytes()), true).is_err());
        assert!(select(calcium, ALUMINUM.1, Some(ALUMINUM.0.as_bytes()), false).is_err());
    }
}
