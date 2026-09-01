//! Boim provider (fork-only — see `docs/MAINTAINING_FORK.md`).
//!
//! Boim Desktop is an in-house agent built on `OpenAI` Codex, so it writes the
//! IDENTICAL rollout JSONL format as Codex — `runtime/sessions/YYYY/MM/DD/
//! rollout-<ts>-<uuid>.jsonl` — tagging `session_meta` with
//! `originator: "boim_desktop"`. Like [`super::ditcodeagent`], this module
//! therefore reuses the Codex rollout parser and metadata extractors, validates
//! paths against the Boim root, and re-tags the provider on the result.
//!
//! Boim ships in two flavors that differ ONLY by root directory:
//!
//! | flavor | id         | root                                |
//! |--------|------------|-------------------------------------|
//! | prod   | `boim`     | `$BOIM_HOME` else `~/.boim`         |
//! | dev    | `boim-dev` | `$BOIM_DEV_HOME` (no default)       |
//!
//! They must stay separate providers rather than one merged store: both roots
//! carry the same `originator` and the same `cwd` values (a developer runs the
//! dev build against the same `Documents/Boim/<project>` workspaces), so
//! merging them would collapse two distinct session sets into one project.
//! Only the root tells them apart.
//!
//! The dev flavor has no default path on purpose. Its root lives inside a
//! developer's own checkout, which no teammate shares, so it stays invisible
//! unless `BOIM_DEV_HOME` is set.
//!
//! This module holds the implementation for both; [`super::boim_dev`] is the
//! thin public surface for the dev flavor.

use super::codex;
use super::ProviderInfo;
use crate::models::{ClaudeMessage, ClaudeProject, ClaudeSession};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// One Boim installation flavor. Everything below is parameterized by this so
/// prod and dev cannot drift apart.
pub(super) struct Flavor {
    /// Provider id as the frontend knows it.
    pub(super) provider: &'static str,
    /// Project path scheme, including `://`.
    pub(super) scheme: &'static str,
    /// Human-readable name.
    pub(super) display_name: &'static str,
    /// Environment variable that overrides (dev: supplies) the root.
    pub(super) home_env: &'static str,
    /// Directory under the user's home used when `home_env` is unset.
    /// `None` means the flavor exists only when `home_env` is set.
    pub(super) default_home_subdir: Option<&'static str>,
}

pub(super) const PROD: Flavor = Flavor {
    provider: "boim",
    scheme: "boim://",
    display_name: "Boim",
    home_env: "BOIM_HOME",
    default_home_subdir: Some(".boim"),
};

pub(super) const DEV: Flavor = Flavor {
    provider: "boim-dev",
    scheme: "boim-dev://",
    display_name: "Boim Dev",
    home_env: "BOIM_DEV_HOME",
    default_home_subdir: None,
};

/// Boim home for `flavor`: `$<home_env>` when set and present, else the default
/// subdirectory of the user's home when the flavor has one. `None` unless the
/// directory exists.
pub(super) fn home_dir(flavor: &Flavor) -> Option<PathBuf> {
    if let Ok(home) = std::env::var(flavor.home_env) {
        let home = home.trim();
        if !home.is_empty() {
            let p = PathBuf::from(home);
            return if p.exists() { Some(p) } else { None };
        }
    }
    let subdir = flavor.default_home_subdir?;
    let p = crate::utils::home_dir()?.join(subdir);
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

/// Detect a Boim installation of this flavor.
pub(super) fn detect_flavor(flavor: &Flavor) -> Option<ProviderInfo> {
    let base = home_dir(flavor)?;
    Some(ProviderInfo {
        id: flavor.provider.to_string(),
        display_name: flavor.display_name.to_string(),
        is_available: !session_dirs(flavor).is_empty(),
        base_path: base.to_string_lossy().to_string(),
    })
}

/// Base path (the Boim home) for this flavor, for the file watcher.
pub(super) fn get_base_path_for(flavor: &Flavor) -> Option<String> {
    home_dir(flavor).map(|p| p.to_string_lossy().to_string())
}

/// The existing session roots under `runtime/`. `archived_sessions` is not
/// written by the current CLI but is accepted so archived history is picked up
/// if it ever appears, mirroring the Codex layout.
fn session_dirs(flavor: &Flavor) -> Vec<PathBuf> {
    let Some(base) = home_dir(flavor) else {
        return Vec::new();
    };
    let runtime = base.join("runtime");
    [runtime.join("sessions"), runtime.join("archived_sessions")]
        .into_iter()
        .filter(|p| p.is_dir())
        .collect()
}

/// All `rollout-*.jsonl` files under this flavor's session roots.
fn rollout_files(flavor: &Flavor) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in session_dirs(flavor) {
        for entry in WalkDir::new(dir)
            .min_depth(1)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .filter(|e| codex::is_discoverable_rollout(e.path()))
        {
            files.push(entry.path().to_path_buf());
        }
    }
    files
}

/// Scan Boim projects for this flavor (rollouts grouped by `cwd`).
pub(super) fn scan_projects_for(flavor: &Flavor) -> Vec<ClaudeProject> {
    struct Agg {
        session_count: usize,
        message_count: usize,
        last_modified: String,
    }
    let mut by_cwd: HashMap<String, Agg> = HashMap::new();

    for path in rollout_files(flavor) {
        if let Ok(info) = codex::extract_project_scan_info(&path) {
            let cwd = info.cwd.clone().unwrap_or_else(|| "unknown".to_string());
            let e = by_cwd.entry(cwd).or_insert_with(|| Agg {
                session_count: 0,
                message_count: 0,
                last_modified: String::new(),
            });
            e.session_count += 1;
            e.message_count += info.message_count;
            if info.last_modified > e.last_modified {
                e.last_modified = info.last_modified;
            }
        }
    }

    let mut projects: Vec<ClaudeProject> = by_cwd
        .into_iter()
        .map(|(cwd, agg)| {
            let name = Path::new(&cwd)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| cwd.clone());
            ClaudeProject {
                name,
                path: format!("{}{cwd}", flavor.scheme),
                actual_path: cwd,
                session_count: agg.session_count,
                message_count: agg.message_count,
                last_modified: agg.last_modified,
                git_info: None,
                provider: Some(flavor.provider.to_string()),
                storage_type: None,
                custom_directory_label: None,
            }
        })
        .collect();

    projects.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    projects
}

/// Load the sessions for one Boim project of this flavor (filtered by `cwd`).
pub(super) fn load_sessions_for(flavor: &Flavor, project_path: &str) -> Vec<ClaudeSession> {
    let target_cwd = project_path
        .strip_prefix(flavor.scheme)
        .unwrap_or(project_path);
    let mut sessions = Vec::new();

    for path in rollout_files(flavor) {
        if let Ok(info) = codex::extract_session_info(&path) {
            if info.cwd.as_deref().unwrap_or("unknown") != target_cwd {
                continue;
            }
            sessions.push(ClaudeSession {
                session_id: info.file_path.clone(),
                actual_session_id: info.session_id,
                file_path: info.file_path,
                project_name: Path::new(target_cwd)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                message_count: info.message_count,
                first_message_time: info.first_message_time,
                last_message_time: info.last_message_time,
                last_modified: info.last_modified,
                has_tool_use: info.has_tool_use,
                has_errors: false,
                summary: info.summary,
                is_renamed: false,
                provider: Some(flavor.provider.to_string()),
                storage_type: None,
                entrypoint: None,
            });
        }
    }

    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    sessions
}

/// Load all messages from one Boim rollout file of this flavor.
pub(super) fn load_messages_for(
    flavor: &Flavor,
    session_path: &str,
) -> Result<Vec<ClaudeMessage>, String> {
    let path = Path::new(session_path);
    if !path.exists() {
        return Err(format!("Session file not found: {session_path}"));
    }
    let canonical = validate_under_base(flavor, path)?;
    let mut messages = codex::parse_rollout_file(&canonical)?;
    // The Codex parser tags messages "codex"; re-tag for this provider.
    for msg in &mut messages {
        msg.provider = Some(flavor.provider.to_string());
    }
    Ok(messages)
}

/// Search across all Boim rollouts of this flavor.
pub(super) fn search_for(flavor: &Flavor, query: &str, limit: usize) -> Vec<ClaudeMessage> {
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    for path in rollout_files(flavor) {
        let Ok(messages) = load_messages_for(flavor, &path.to_string_lossy()) else {
            continue;
        };
        for msg in messages {
            if results.len() >= limit {
                return results;
            }
            if let Some(content) = &msg.content {
                if crate::utils::search_json_value_case_insensitive(content, &query_lower) {
                    results.push(msg);
                }
            }
        }
    }
    results
}

/// Return true when `session_path` is a Boim rollout of this flavor.
pub(super) fn is_session_path_for(flavor: &Flavor, session_path: &str) -> bool {
    validate_under_base(flavor, Path::new(session_path)).is_ok()
}

/// Confine `session_path` to this flavor's session roots and confirm it is a
/// rollout file. Canonicalizes both sides.
///
/// This is what keeps the two flavors — and Codex itself — apart: the rollouts
/// are byte-identical, so the root is the only discriminator.
fn validate_under_base(flavor: &Flavor, path: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Failed to resolve session path: {e}"))?;
    if !codex::is_rollout_jsonl(&canonical) {
        return Err(format!(
            "Not a {} rollout file: {}",
            flavor.display_name,
            path.display()
        ));
    }
    let allowed: Vec<PathBuf> = session_dirs(flavor)
        .into_iter()
        .filter_map(|d| d.canonicalize().ok())
        .collect();
    if allowed.is_empty() {
        return Err(format!(
            "{} session directories not found",
            flavor.display_name
        ));
    }
    if allowed.iter().any(|d| canonical.starts_with(d)) {
        Ok(canonical)
    } else {
        Err(format!(
            "Session path is outside {} session directories: {}",
            flavor.display_name,
            path.display()
        ))
    }
}

// ============================================================================
// Public surface — prod flavor. The dev flavor lives in `super::boim_dev`.
// ============================================================================

/// Detect a Boim (prod) installation.
pub fn detect() -> Option<ProviderInfo> {
    detect_flavor(&PROD)
}

/// Base path (the Boim home), for the file watcher.
pub fn get_base_path() -> Option<String> {
    get_base_path_for(&PROD)
}

/// Scan Boim projects (rollouts grouped by `cwd`).
pub fn scan_projects() -> Result<Vec<ClaudeProject>, String> {
    Ok(scan_projects_for(&PROD))
}

/// Load the sessions for one Boim project (filtered by `cwd`).
pub fn load_sessions(
    project_path: &str,
    _exclude_sidechain: bool,
) -> Result<Vec<ClaudeSession>, String> {
    Ok(load_sessions_for(&PROD, project_path))
}

/// Load all messages from one Boim rollout file.
pub fn load_messages(session_path: &str) -> Result<Vec<ClaudeMessage>, String> {
    load_messages_for(&PROD, session_path)
}

/// Search across all Boim rollouts.
pub fn search(query: &str, limit: usize) -> Result<Vec<ClaudeMessage>, String> {
    Ok(search_for(&PROD, query, limit))
}

/// Return true when `session_path` is a Boim rollout.
pub fn is_session_path(session_path: &str) -> bool {
    is_session_path_for(&PROD, session_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use serial_test::serial;
    use std::fs;
    use tempfile::TempDir;

    /// Sets both flavor env vars so neither can fall through to a real home.
    struct HomeGuard {
        vars: Vec<(&'static str, Option<String>)>,
    }
    impl HomeGuard {
        fn set(prod: Option<&Path>, dev: Option<&Path>) -> Self {
            let mut vars = Vec::new();
            for (key, value) in [("BOIM_HOME", prod), ("BOIM_DEV_HOME", dev)] {
                vars.push((key, std::env::var(key).ok()));
                match value {
                    Some(path) => std::env::set_var(key, path),
                    None => std::env::set_var(key, ""),
                }
            }
            Self { vars }
        }
    }
    impl Drop for HomeGuard {
        fn drop(&mut self) {
            for (key, original) in &self.vars {
                match original {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    /// A rollout shaped like the real Boim Desktop writes: Codex `session_meta`
    /// carrying the Boim originator, then a user turn.
    fn write_rollout(sessions_dir: &Path, filename: &str, id: &str, cwd: &str, prompt: &str) {
        let path = sessions_dir.join(filename);
        let lines = [
            json!({
                "type": "session_meta",
                "payload": {
                    "id": id, "session_id": id, "cwd": cwd,
                    "originator": "boim_desktop", "model_provider": "openai-api",
                    "source": "vscode"
                }
            }),
            json!({
                "timestamp": "2026-07-13T04:21:51Z",
                "type": "response_item",
                "payload": {
                    "type": "message", "role": "user", "created_at": "2026-07-13T04:21:51Z",
                    "content": [{ "type": "input_text", "text": prompt }]
                }
            }),
        ];
        let body = lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, format!("{body}\n")).unwrap();
    }

    /// Create `<root>/runtime/sessions` and return it.
    fn make_root(root: &Path) -> PathBuf {
        let sessions = root.join("runtime").join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        sessions
    }

    #[test]
    #[serial]
    fn scan_load_and_retag_via_boim_home() {
        let tmp = TempDir::new().unwrap();
        let sessions = make_root(tmp.path());
        write_rollout(
            &sessions,
            "rollout-2026-07-13T13-21-04-uuid1.jsonl",
            "uuid1",
            "/Users/jack/proj",
            "안녕",
        );
        let _guard = HomeGuard::set(Some(tmp.path()), None);

        let info = detect().unwrap();
        assert_eq!(info.id, "boim");
        assert!(info.is_available);

        let projects = scan_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].provider.as_deref(), Some("boim"));
        assert_eq!(projects[0].path, "boim:///Users/jack/proj");
        assert_eq!(projects[0].actual_path, "/Users/jack/proj");

        let sess = load_sessions("boim:///Users/jack/proj", false).unwrap();
        assert_eq!(sess.len(), 1);
        assert_eq!(sess[0].provider.as_deref(), Some("boim"));
        let file_path = sess[0].file_path.clone();

        // The Codex parser does the work; messages must come back re-tagged.
        let msgs = load_messages(&file_path).unwrap();
        assert!(!msgs.is_empty());
        assert!(msgs.iter().all(|m| m.provider.as_deref() == Some("boim")));

        assert!(is_session_path(&file_path));
        assert_eq!(search("안녕", 10).unwrap().len(), 1);
    }

    /// The whole reason the flavors are separate providers: identical rollouts
    /// under two roots must not bleed into each other.
    #[test]
    #[serial]
    fn flavors_do_not_see_each_others_sessions() {
        let prod = TempDir::new().unwrap();
        let dev = TempDir::new().unwrap();
        let prod_sessions = make_root(prod.path());
        let dev_sessions = make_root(dev.path());
        // Same cwd on both sides — only the root differs.
        write_rollout(
            &prod_sessions,
            "rollout-2026-07-13T13-21-04-prod1.jsonl",
            "prod1",
            "/w/shared",
            "prod turn",
        );
        write_rollout(
            &dev_sessions,
            "rollout-2026-07-13T13-21-05-dev1.jsonl",
            "dev1",
            "/w/shared",
            "dev turn",
        );
        let _guard = HomeGuard::set(Some(prod.path()), Some(dev.path()));

        let prod_sessions_found = load_sessions("boim:///w/shared", false).unwrap();
        let dev_sessions_found =
            super::super::boim_dev::load_sessions("boim-dev:///w/shared", false).unwrap();
        assert_eq!(prod_sessions_found.len(), 1);
        assert_eq!(dev_sessions_found.len(), 1);
        assert_ne!(
            prod_sessions_found[0].file_path,
            dev_sessions_found[0].file_path
        );

        // Each flavor rejects the other's file outright.
        assert!(!is_session_path(&dev_sessions_found[0].file_path));
        assert!(!super::super::boim_dev::is_session_path(
            &prod_sessions_found[0].file_path
        ));

        let dev_projects = super::super::boim_dev::scan_projects().unwrap();
        assert_eq!(dev_projects.len(), 1);
        assert_eq!(dev_projects[0].provider.as_deref(), Some("boim-dev"));
        assert_eq!(dev_projects[0].path, "boim-dev:///w/shared");
    }

    #[test]
    #[serial]
    fn dev_flavor_is_absent_without_its_env_var() {
        let prod = TempDir::new().unwrap();
        make_root(prod.path());
        let _guard = HomeGuard::set(Some(prod.path()), None);

        assert!(detect().is_some());
        assert!(super::super::boim_dev::detect().is_none());
        assert!(super::super::boim_dev::scan_projects().unwrap().is_empty());
    }

    #[test]
    #[serial]
    fn load_messages_rejects_path_outside_base() {
        let tmp = TempDir::new().unwrap();
        make_root(tmp.path());
        let _guard = HomeGuard::set(Some(tmp.path()), None);

        // A developer's own Codex rollout must never resolve as Boim.
        let outside = TempDir::new().unwrap();
        write_rollout(outside.path(), "rollout-x.jsonl", "x", "/c", "hi");
        let stray = outside.path().join("rollout-x.jsonl");
        assert!(load_messages(&stray.to_string_lossy()).is_err());
        assert!(!is_session_path(&stray.to_string_lossy()));
    }
}
