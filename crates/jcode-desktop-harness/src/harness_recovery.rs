//! Clients observe recovery. Only the daemon admits continuation turns.
#[derive(Default)]
pub(super) struct Recovery {
    consumed: bool,
}

impl Recovery {
    pub(super) fn supersede(&mut self) {
        self.consumed = true;
    }

    pub(super) fn notice(&mut self, message: &str) -> bool {
        if self.consumed || message.trim().is_empty() {
            return false;
        }
        self.consumed = true;
        true
    }
}
