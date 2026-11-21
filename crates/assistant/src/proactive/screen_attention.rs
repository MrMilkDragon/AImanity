pub(crate) const KEY: &str = "screen:change";
const COOLDOWN_MS: u64 = 60_000;

#[derive(serde::Deserialize)]
pub(crate) struct Sample(u32, u32, String, String, #[serde(default)] String);

impl Sample {
    fn same_view(&self, other: &Self) -> bool {
        self.3 == other.3
            && (self.4.is_empty()
                || other.4.is_empty()
                || self.4.len() == other.4.len()
                    && self
                        .4
                        .bytes()
                        .zip(other.4.bytes())
                        .filter(|(a, b)| a != b)
                        .take(7)
                        .count()
                        < 7)
    }
}

#[derive(Default)]
pub(super) struct WindowWatch {
    settled: Option<Sample>,
    candidate: Option<Sample>,
    next_wake_at: u64,
}

impl WindowWatch {
    pub(super) fn observe(&mut self, sample: Option<Sample>, now: u64) -> Option<String> {
        let Some(sample) = sample else {
            self.settled = None;
            self.candidate = None;
            return None;
        };
        let same_window = self
            .settled
            .as_ref()
            .is_some_and(|prior| (prior.0, prior.1, &prior.2) == (sample.0, sample.1, &sample.2));
        if !same_window {
            self.settled = Some(sample);
            self.candidate = None;
            return None;
        }
        if let Some(prior) = &self.settled
            && prior.same_view(&sample)
        {
            if prior.4.is_empty() && !sample.4.is_empty() {
                self.settled = Some(sample);
            }
            self.candidate = None;
            return None;
        }
        if !self
            .candidate
            .as_ref()
            .is_some_and(|prior| prior.same_view(&sample))
        {
            self.candidate = Some(sample);
            return None;
        }
        if now < self.next_wake_at {
            return None;
        }
        self.candidate = None;
        let application = sample.2.clone();
        self.settled = Some(sample);
        self.next_wake_at = now.saturating_add(COOLDOWN_MS);
        Some(application)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(window: u32, pid: u32, title: &str) -> Option<Sample> {
        Some(Sample(
            window,
            pid,
            "editor".into(),
            title.into(),
            String::new(),
        ))
    }

    #[test]
    fn resumes_content_detection_after_a_title_only_baseline() {
        let frame = |pixels: &str| {
            Some(Sample(
                10,
                2,
                "editor".into(),
                "title".into(),
                pixels.into(),
            ))
        };
        let mut watch = WindowWatch::default();
        assert_eq!(watch.observe(frame(""), 0), None);
        assert_eq!(watch.observe(frame("aaaaaaaa"), 1000), None);
        assert_eq!(watch.observe(frame("bbbbbbbb"), 2000), None);
        assert_eq!(
            watch.observe(frame("bbbbbbbb"), 3000).as_deref(),
            Some("editor")
        );
    }

    #[test]
    fn wakes_on_stable_window_content_and_ignores_caret_and_transient_frames() {
        let sample = |pixels: &str| {
            Some(Sample(
                10,
                2,
                "window".into(),
                "title".into(),
                pixels.into(),
            ))
        };
        let mut watch = WindowWatch::default();
        for (time, pixels) in [
            (0, "aaaaaaaaaaaaaaaaaaaaaaaa"),
            (1, "aabaaaaaaaaaaaaaaaaaaaaa"),
            (2, "aabbbbaaaaaaaaaaaaaaaaaa"),
            (3, "cccccccccccccccccccccccc"),
            (4, "aaaaaaaaaaaaaaaaaaaaaaaa"),
            (5, "bbbbbbbbbbbbbaaaaaaaaaaa"),
        ] {
            assert_eq!(watch.observe(sample(pixels), time), None);
        }
        assert_eq!(
            watch
                .observe(sample("bbbbbbbbbbbbbaaaaaabaaaa"), 6)
                .as_deref(),
            Some("window")
        );
        assert_eq!(
            watch.observe(sample("bbbbbbbbbbbbbaaaaaaaaaaa"), 70_000),
            None
        );
    }

    #[test]
    fn observes_a_settled_title_change_once_without_reacting_to_focus_or_flicker() {
        let mut watch = WindowWatch::default();
        assert_eq!(watch.observe(sample(1, 7, "one"), 0), None);
        assert_eq!(watch.observe(sample(1, 7, "flash"), 3000), None);
        assert_eq!(watch.observe(sample(1, 7, "one"), 6000), None);
        assert_eq!(watch.observe(sample(2, 7, "two"), 9000), None);
        assert_eq!(watch.observe(sample(2, 7, "two"), 12000), None);
        assert_eq!(watch.observe(sample(2, 7, "changed"), 15000), None);
        assert_eq!(
            watch.observe(sample(2, 7, "changed"), 18000).as_deref(),
            Some("editor")
        );
        assert_eq!(watch.observe(sample(2, 7, "changed"), 21000), None);
        assert_eq!(watch.observe(sample(2, 7, "more"), 24000), None);
        assert_eq!(watch.observe(sample(2, 7, "more"), 27000), None);
        assert_eq!(watch.observe(None, 30000), None);
        assert_eq!(watch.observe(sample(2, 7, "after reconnect"), 33000), None);
        assert_eq!(watch.observe(sample(2, 7, "more"), 36000), None);
        assert_eq!(watch.observe(sample(2, 7, "more"), 39000), None);
        assert_eq!(
            watch.observe(sample(2, 7, "more"), 78000).as_deref(),
            Some("editor")
        );
        assert_eq!(watch.observe(sample(2, 7, "more"), 81000), None);
        assert_eq!(watch.observe(sample(2, 9, "reused window ID"), 84000), None);
        assert_eq!(watch.observe(None, 87000), None);
        assert_eq!(watch.observe(sample(2, 9, "new baseline"), 90000), None);
        assert_eq!(watch.observe(sample(2, 9, "new title"), 93000), None);
        assert_eq!(watch.observe(sample(2, 9, "new title"), 96000), None);
        assert_eq!(watch.observe(sample(2, 9, "new baseline"), 99000), None);
        assert_eq!(watch.observe(sample(2, 9, "new baseline"), 138000), None);
        assert_eq!(watch.observe(sample(2, 9, "new title"), 141000), None);
        assert_eq!(
            watch.observe(sample(2, 9, "new title"), 144000).as_deref(),
            Some("editor")
        );
    }
}
