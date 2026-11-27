//! Jcode Desktop's presentation-independent models, free of GPUI.
//!
//! `jcode-desktop-ui` re-exports these modules at its crate root, so UI code
//! keeps addressing them as `crate::diff_model`, `crate::learning`, and so on.
//! Test a change here with `justrust test -p jcode-desktop-model`.
//!
//! The files sit in directories by subject (diffs, search, prompts, text) while
//! the modules stay flat, so paths such as `jcode_desktop_model::diff_model`
//! do not change when a file moves.

#[path = "prompt/command_workflows.rs"]
pub mod command_workflows;
#[path = "search/context_files.rs"]
pub mod context_files;
#[path = "diff/patch.rs"]
pub mod diff;
#[path = "diff/model.rs"]
pub mod diff_model;
pub mod friendly_error;
#[path = "search/fs_probe.rs"]
pub mod fs_probe;
#[path = "search/global.rs"]
pub mod global_search;
pub mod learning;
pub mod machine_state;
#[path = "text/markdown_file.rs"]
pub mod markdown_file;
pub mod pdf_render;
pub mod platform_dirs;
#[path = "search/probe_cache.rs"]
pub mod probe_cache;
#[path = "prompt/completion.rs"]
pub mod prompt_completion;
#[path = "text/buffer.rs"]
pub mod text_buffer;
#[path = "text/field.rs"]
pub mod text_field;
pub mod todoist;
pub mod window_placement;

#[cfg(test)]
#[path = "search/global_tests.rs"]
mod global_search_tests;
#[path = "prompt/cache.rs"]
pub mod prompt_cache;
