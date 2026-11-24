use super::{ReleaseStatus, UpdateState, set, store_release_status};
use gpui::{App, PromptButton, PromptLevel};
use jcode_desktop_updater::paired::{self, Change, Preview, Progress};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

static REQUEST: AtomicBool = AtomicBool::new(false);

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
            let detail = format!("{}\n\n{}\n\nJcode builds and installs this pair together. Desktop restarts only after active turns finish.{disk}",
                describe("CLI", &preview.cli), describe("Desktop", &preview.desktop));
            let answer = window.update(cx, |_, window, cx| {
                let install = [PromptButton::cancel("Not now"), PromptButton::new("Build and Update")];
                let ok = [PromptButton::cancel("OK")];
                window.prompt(PromptLevel::Info, "Jcode update available", Some(&detail), if enough_space { &install[..] } else { &ok[..] }, cx)
            })?;
            if !enough_space || !matches!(answer.await, Ok(1)) {
                return Ok(false);
            }
            set(UpdateState::ApplyingPair);
            let _ = cx.update(|cx| cx.refresh_windows());
            let preview = Arc::new(preview);
            let confirmed = preview.clone();
            cx.background_executor().spawn(async move { paired::submit(&confirmed) }).await?;
            loop {
                let submitted = preview.clone();
                let progress = cx.background_executor().spawn(async move { paired::progress(&submitted) }).await?;
                match progress {
                    Progress::Completed => return Ok(true),
                    Progress::Failed => anyhow::bail!("Rollout failed or rolled back. See jcode rollout --status for details."),
                    Progress::InProgress => cx.background_executor().timer(Duration::from_secs(2)).await,
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
