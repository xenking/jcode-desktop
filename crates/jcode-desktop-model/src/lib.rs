//! Jcode Desktop's presentation-independent models, free of GPUI.
//!
//! `jcode-desktop-ui` re-exports these modules at its crate root, so UI code
//! keeps addressing them as `crate::diff_model`, `crate::learning`, and so on.
//! Test a change here with `justrust test -p jcode-desktop-model`.

pub mod command_workflows;
pub mod context_files;
pub mod diff;
pub mod diff_model;
pub mod friendly_error;
pub mod global_search;
pub mod learning;
pub mod markdown_file;
pub mod pdf_render;
pub mod platform_dirs;
pub mod prompt_completion;
pub mod text_buffer;
pub mod todoist;
pub mod window_placement;

#[cfg(test)]
#[path = "global_search_tests.rs"]
mod global_search_tests;
pub mod prompt_cache;
