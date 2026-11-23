use ai_phone_api::wire::RegisteredDevice;

pub(super) fn visible(device: &RegisteredDevice) -> bool {
    device.runtime_state == "managed" || (device.paired && device.connected)
}

pub(super) fn slots(
    devices: &[RegisteredDevice],
    current: &[Option<RegisteredDevice>; 2],
    pick: Option<(usize, &str)>,
) -> [Option<RegisteredDevice>; 2] {
    let lookup = |id: &str| devices.iter().find(|d| d.id == id && visible(d));
    let mut slots = current
        .each_ref()
        .map(|old| old.as_ref().and_then(|d| lookup(&d.id)));
    if let Some((index, id)) = pick
        && let Some(device) = lookup(id)
        && !slots.iter().flatten().any(|d| d.id == id)
    {
        slots[index] = Some(device);
    }
    for index in 0..2 {
        if slots[index].is_none() {
            let companion = slots[1 - index];
            slots[index] = devices
                .iter()
                .filter(|d| visible(d) && companion.is_none_or(|c| c.id != d.id))
                .min_by_key(|d| companion.map_or(d.kind != "phone", |c| c.kind == d.kind));
        }
    }
    slots.map(|d| d.cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn any_two_visible_devices_can_share_the_stage() {
        let device = |id: &str, kind: &str| RegisteredDevice {
            id: id.into(),
            kind: kind.into(),
            paired: true,
            connected: true,
            ..Default::default()
        };
        let mut devices = vec![
            device("a", "phone"),
            device("b", "phone"),
            device("c", "desktop"),
            device("d", "desktop"),
        ];
        let start = slots(&devices, &[None, None], None);
        let ids = |s: &[Option<RegisteredDevice>; 2]| {
            s.each_ref().map(|d| d.as_ref().map(|d| d.id.clone()))
        };
        assert_eq!(ids(&start), [Some("a".into()), Some("c".into())]);
        let phones = slots(&devices, &start, Some((1, "b")));
        assert_eq!(ids(&phones), [Some("a".into()), Some("b".into())]);
        let desktops = slots(&devices, &start, Some((0, "d")));
        assert_eq!(ids(&desktops), [Some("d".into()), Some("c".into())]);
        devices[3].connected = false;
        assert_eq!(ids(&slots(&devices, &desktops, None)), ids(&start));
    }
}
