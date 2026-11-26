//! The first prompt of a fork is sent by the harness over the child's own
//! connection, so this panel never sees a local submit. Paint it here exactly
//! as `send_prompt` would, without sending it a second time: the question and
//! the working indicator must be visible the moment the fork opens, even
//! though the runtime already has the prompt.
use super::*;

impl Panel {
    /// Echo a prompt that another component already delivered to this session.
    pub(crate) fn echo_external_prompt(&mut self, content: String, cx: &mut Context<Self>) {
        let content = content.trim();
        if content.is_empty() {
            return;
        }
        // A reload can replay the same update; the echo must stay single.
        if self
            .items
            .iter()
            .any(|item| matches!(item, Item::User(text) if text == content))
        {
            self.set_working_now(cx);
            return;
        }
        let index = self.items.len();
        self.items.push(Item::User(content.to_string()));
        self.stamp_user(index);
        if let Some(layout) = &mut self.startup_layout {
            layout.committed = true;
        }
        self.pending_users.push_back(index);
        self.stick_to_bottom = true;
        self.transcript_list.scroll_to_end();
        self.set_working_now(cx);
    }

    /// The turn is already running in the runtime; say so before the first
    /// `MessageAccepted` arrives instead of showing an idle panel.
    fn set_working_now(&mut self, cx: &mut Context<Self>) {
        if !self.activity_active() {
            self.status = "running".into();
        }
        self.transcript_measurements.invalidate();
        cx.notify();
    }
}
