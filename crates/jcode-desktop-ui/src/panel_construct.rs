//! Restoring what a reload keeps of a panel, and the account and unfinished-work
//! panels.
use super::*;

impl Panel {
    pub fn restore_snapshot(&mut self, snapshot: PanelSnapshot, cx: &mut Context<Self>) {
        if self.restore_side_document_snapshot(&snapshot, cx) {
            return;
        }
        if let Some(transcript) = snapshot.transcript {
            self.restore_transcript(transcript);
        } else {
            self.turn_outcome = snapshot.turn_outcome;
        }
        self.finished_unseen = snapshot.finished_unseen;
        self.clean_chat_override = snapshot.clean_chat_override;
        self.image_pane_open = snapshot.image_pane_open;
        self.image_pane_selected = None;
        self.prompt_queue = snapshot.prompt_queue;
        self.prompt_queue.replay_pending_on_connect = true;
        self.pending_ask_draft = snapshot.ask_draft;
        if let (Some(draft), Some(card)) = (&self.pending_ask_draft, &self.ask_card)
            && draft.matches_questions(&card.questions)
        {
            let draft = self.pending_ask_draft.take().expect("checked above");
            self.ask_card
                .as_mut()
                .expect("checked above")
                .restore_draft(draft, cx);
        }
        self.title = snapshot.title.into();
        self.working_dir = snapshot.working_dir;
        if self.is_pending_session() {
            self.draft_config = snapshot.draft_config;
            if let Some(model) = self.draft_config.model.clone() {
                self.pick_draft_model(&model, cx);
            }
            self.reasoning_effort.clone_from(&self.draft_config.effort);
            self.pinned_modes.clone_from(&self.draft_config.modes);
            self.sync_effort_menu(cx);
            self.sync_mode_menu(cx);
        }
        self.stick_to_bottom = snapshot.stick_to_bottom;
        self.startup_layout = snapshot.startup_layout;
        self.pending_history_scroll =
            (!snapshot.stick_to_bottom).then_some((snapshot.scroll_x, snapshot.scroll_y));
        self.transcript_list
            .set_offset_from_scrollbar(point(px(snapshot.scroll_x), px(snapshot.scroll_y)));
        self.input.update(cx, |input, cx| {
            input.restore(snapshot.draft, cx);
            input.set_working_dir(self.working_dir.as_deref());
        });
        self.schedule_ask_recovery(cx);
    }

    pub(crate) fn observe_draft_images(input: &Entity<PromptInput>, cx: &mut Context<Self>) {
        let owner = cx.weak_entity();
        gpui::App::observe(cx, input, move |input, cx| {
            let has_images = input.read(cx).has_draft_images();
            if owner
                .read_with(cx, |panel, _| panel.draft_has_images != has_images)
                .unwrap_or(false)
            {
                let _ = owner.update(cx, |panel, cx| {
                    panel.draft_has_images = has_images;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(crate) fn new_accounts(
        source_session: &str,
        preview: Option<PreviewState>,
        bridge: Bridge,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::new(
            format!("accounts://{source_session}"),
            Some("accounts".into()),
            None,
            bridge,
            cx,
        );
        panel.items.clear();
        panel.preview_state = preview;
        panel.open_login_picker(cx);
        panel
    }

    pub(crate) fn choose_account_model(&mut self, cx: &mut Context<Self>) {
        if self.is_side_document() {
            return;
        }
        self.open_recovery_models(cx);
    }

    pub fn new_unfinished_work(
        sessions: Vec<crate::harness::UnfinishedSession>,
        session_opener: SessionOpener,
        bridge: Bridge,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::new(
            "unfinished-work".into(),
            Some("unfinished work".into()),
            None,
            bridge,
            cx,
        );
        panel.unfinished_work = Some(sessions);
        panel.unfinished_session_opener = Some(session_opener);
        panel
    }

    pub fn set_unfinished_work(
        &mut self,
        sessions: Vec<crate::harness::UnfinishedSession>,
        cx: &mut Context<Self>,
    ) {
        if self.unfinished_work.as_ref() != Some(&sessions) {
            self.unfinished_work = Some(sessions);
            cx.notify();
        }
    }
}
