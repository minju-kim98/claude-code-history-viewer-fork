//! Boim Dev provider (fork-only — see `docs/MAINTAINING_FORK.md`).
//!
//! The development build of Boim Desktop. It writes the same Codex-format
//! rollouts as [`super::boim`] and differs ONLY by root: `$BOIM_DEV_HOME`,
//! with no default, because that root lives inside a developer's own checkout
//! (`<repo>/.boim-dev`) which no teammate shares. Unset the variable and this
//! provider simply does not appear.
//!
//! It is a separate provider rather than a merged store because the dev build
//! runs against the same workspaces as prod: same `originator`, same `cwd`, so
//! merging would collapse two distinct session sets into one project.
//!
//! Everything here delegates to `boim`'s flavor-parameterized implementation.

use super::boim::{self, DEV};
use super::ProviderInfo;
use crate::models::{ClaudeMessage, ClaudeProject, ClaudeSession};

/// Detect a Boim Dev installation (present only when `BOIM_DEV_HOME` is set).
pub fn detect() -> Option<ProviderInfo> {
    boim::detect_flavor(&DEV)
}

/// Base path (the Boim Dev home), for the file watcher.
pub fn get_base_path() -> Option<String> {
    boim::get_base_path_for(&DEV)
}

/// Scan Boim Dev projects (rollouts grouped by `cwd`).
pub fn scan_projects() -> Result<Vec<ClaudeProject>, String> {
    Ok(boim::scan_projects_for(&DEV))
}

/// Load the sessions for one Boim Dev project (filtered by `cwd`).
pub fn load_sessions(
    project_path: &str,
    _exclude_sidechain: bool,
) -> Result<Vec<ClaudeSession>, String> {
    Ok(boim::load_sessions_for(&DEV, project_path))
}

/// Load all messages from one Boim Dev rollout file.
pub fn load_messages(session_path: &str) -> Result<Vec<ClaudeMessage>, String> {
    boim::load_messages_for(&DEV, session_path)
}

/// Search across all Boim Dev rollouts.
pub fn search(query: &str, limit: usize) -> Result<Vec<ClaudeMessage>, String> {
    Ok(boim::search_for(&DEV, query, limit))
}

/// Return true when `session_path` is a Boim Dev rollout.
pub fn is_session_path(session_path: &str) -> bool {
    boim::is_session_path_for(&DEV, session_path)
}
