use super::{ReleaseStatus, UpdateState, current, paired_dialog, set, store_release_status};
use gpui::{App, PromptButton, PromptLevel};
use jcode_desktop_updater::paired::{self, BlockedAction, Change, Install, Preview, Progress};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

static REQUEST: AtomicBool = AtomicBool::new(false);
static CANCEL_RETRY: AtomicBool = AtomicBool::new(false);
const POLL_INTERVAL: Duration = Duration::from_secs(2);

pub(super) fn cancel_retry() {
    CANCEL_RETRY.store(true, Ordering::Release);
}

struct Request;
impl Drop for Request {
    fn drop(&mut self) {
        REQUEST.store(false, Ordering::Release);
    }
}

pub(super) fn release_status(preview: Option<&Preview>) -> ReleaseStatus {
    match preview {
        Some(preview) => ReleaseStatus::Newer {
            version: format!(
                "{} ({})",
                preview.desktop.target_version,
                preview
                    .desktop
                    .target_sha
                    .chars()
                    .take(9)
                    .collect::<String>()
            ),
        },
        None => ReleaseStatus::Current {
            latest: crate::build_info::VERSION.into(),
        },
    }
}

fn describe(name: &str, change: &Change) -> String {
    let mut text = format!(
        "{name}\nInstalled: {} [{}]\nTarget source: v{} [{}] ({} commits)",
        change.installed_version,
        change.installed_sha,
        change.target_version,
        change.target_sha,
        change.commits
    );
    for subject in &change.subjects {
        text.push_str("\n• ");
        text.push_str(subject);
    }
    if change.commits > change.subjects.len() {
        text.push_str(&format!(
            "\n… and {} more",
            change.commits - change.subjects.len()
        ));
    }
    text
}

pub(super) fn request(cx: &mut App) {
    let Some(window) = cx.active_window() else {
        return;
    };
    if REQUEST.swap(true, Ordering::AcqRel) {
        return;
    }
    let request = Request;
    set(UpdateState::Checking);
    cx.refresh_windows();
    cx.spawn(async move |cx| {
        let _request = request;
        let result = async {
            let preview = cx.background_executor().spawn(async { paired::check() }).await?;
            store_release_status(release_status(preview.as_ref()));
            let Some(preview) = preview else {
                let _ = window.update(cx, |_, window, cx| {
                    let _response = window.prompt(PromptLevel::Info, "Jcode is up to date", Some("The installed CLI and Desktop match the fork heads."), &["OK"], cx);
                });
                return Ok(false);
            };
            let enough_space = preview.free_gib >= preview.minimum_gib;
            let disk = if enough_space { String::new() } else {
                format!("\n\nOnly {} GiB free. Rollout needs at least {} GiB, and a release build may need more.", preview.free_gib, preview.minimum_gib)
            };
            let detail = format!("{}\n\n{}\n\nJcode builds and installs this pair together. Active work can finish first, and unanswered questions will still be available after the update.{disk}",
                describe("CLI", &preview.cli), describe("Desktop", &preview.desktop));
            let answer = window.update(cx, |_, window, cx| {
                let install = [PromptButton::cancel("Not now"), PromptButton::new("Build and Update")];
                let ok = [PromptButton::cancel("OK")];
                window.prompt(PromptLevel::Info, "Jcode update available", Some(&detail), if enough_space { &install[..] } else { &ok[..] }, cx)
            })?;
            if !enough_space || !matches!(answer.await, Ok(1)) {
                return Ok(false);
            }
            let preview = Arc::new(preview);
            let mut install = Install::WhenIdle;
            loop {
                show_state(cx, UpdateState::ApplyingPair);
                let confirmed = preview.clone();
                let receipt = Arc::new(cx.background_executor().spawn(async move { paired::submit(&confirmed, install) }).await?);
                let poll = || {
                    let (preview, receipt) = (preview.clone(), receipt.clone());
                    cx.background_executor().spawn(async move { paired::progress(&preview, &receipt) })
                };
                let (sessions, mut retry_blockers) = loop {
                    match poll().await? {
                        Progress::Completed => return Ok(true),
                        Progress::Failed => anyhow::bail!("The update did not complete. Check for updates again to retry."),
                        Progress::Blocked { sessions, retry_blockers } => break (sessions, retry_blockers),
                        Progress::InProgress => show_state(cx, UpdateState::ApplyingPair),
                        Progress::Draining(blockers) => show_state(cx, UpdateState::PairDrain { blockers, retry: false }),
                    }
                    cx.background_executor().timer(POLL_INTERVAL).await;
                };
                show_state(cx, UpdateState::ApplyingPair);
                let (answer, _closing) = cx.update(|app| {
                    let dialog_window = app.windows().into_iter()
                        .filter_map(|w| w.downcast::<crate::workspace::Workspace>())
                        .min_by_key(|w| w.window_id() != window.window_id());
                    dialog_window.map(|handle| handle.update(app, |_, window, cx| {
                        paired_dialog::show(sessions.clone(), window, cx)
                    })).transpose()
                })?.ok_or_else(|| anyhow::anyhow!("The update was blocked:\n{}", sessions.join("\n")))?;
                match answer.recv().await.unwrap_or(BlockedAction::Cancel) {
                    BlockedAction::Cancel => return Ok(false),
                    BlockedAction::InstallNow => install = Install::Now,
                    BlockedAction::RetryWhenIdle => {
                        CANCEL_RETRY.store(false, Ordering::Release);
                        while let Some(blockers) = retry_blockers.filter(|blockers| !blockers.is_empty()) {
                            show_state(cx, UpdateState::PairDrain { blockers, retry: true });
                            cx.background_executor().timer(POLL_INTERVAL).await;
                            if CANCEL_RETRY.load(Ordering::Acquire) {
                                return Ok(false);
                            }
                            let Progress::Blocked { retry_blockers: latest, .. } = poll().await? else {
                                anyhow::bail!("The blocked update changed. Check for updates again.");
                            };
                            retry_blockers = latest;
                        }
                        if CANCEL_RETRY.load(Ordering::Acquire) {
                            return Ok(false);
                        }
                        install = Install::WhenIdle;
                    }
                }
            }
        }.await;
        match result {
            Ok(true) => {
                store_release_status(release_status(None));
                set(UpdateState::Finished { message: "The CLI and Desktop update completed.".into() });
            }
            Ok(false) => set(UpdateState::Idle),
            Err(error) => {
                let message = format!("{error:#}");
                set(UpdateState::Failed { message: message.clone() });
                let _ = window.update(cx, |_, window, cx| {
                    let _response = window.prompt(PromptLevel::Warning, "Couldn't update Jcode", Some(&message), &["OK"], cx);
                });
            }
        }
        let _ = cx.update(|cx| cx.refresh_windows());
    }).detach();
}

fn show_state(cx: &gpui::AsyncApp, state: UpdateState) {
    if current() != state {
        set(state);
        let _ = cx.update(|cx| cx.refresh_windows());
    }
}
