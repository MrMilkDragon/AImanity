use crate::wire::DesktopActionRequest;

pub struct Views<T>(Vec<(String, Option<String>, T)>);
impl<T> Default for Views<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}
impl<T> Views<T> {
    pub fn remember(&mut self, request: &DesktopActionRequest, id: String, value: T) {
        // Manual previews can advance one frame while a click is in flight.
        if request.epoch == 2 {
            if self
                .0
                .iter()
                .filter(|(_, owner, _)| owner.is_none())
                .count()
                == 2
                && let Some(oldest) = self.0.iter().position(|(_, owner, _)| owner.is_none())
            {
                self.0.remove(oldest);
            }
        } else {
            self.0.retain(|(_, owner, _)| owner.is_none());
        }
        let owner = (request.epoch != 2).then(|| request.control_session.clone());
        self.0.push((id, owner, value));
    }
    pub fn take(&mut self, request: &DesktopActionRequest) -> Option<T> {
        let owner = (request.epoch != 2).then_some(request.control_session.as_str());
        let i = self.0.iter().position(|(id, source, _)| {
            *id == request.observation_id && source.as_deref() == owner
        })?;
        let (_, _, value) = self.0.swap_remove(i);
        self.0.clear();
        Some(value)
    }
}
