//! On-disk cache for fetched tickets (issue lists + issue details), used when
//! the cache mode is `Disk`. Each entry is a small JSON file under the OS cache
//! dir, named by a sha256 of its cache key.
//!
//! Caching is stale-while-revalidate, so these files are only ever a starting
//! point for instant display — every view kicks off a background refresh that
//! overwrites them, bounding staleness.

use std::fmt::Write as _;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::models::{Issue, IssueDetail, View};

/// `~/.cache/linear-tui/tickets/` (honours `$XDG_CACHE_HOME`).
fn tickets_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("linear-tui").join("tickets"))
}

fn hash_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Stable cache key for an issue list: team + view + optional project filter.
pub fn list_key(team_id: &str, view: View, project_id: Option<&str>) -> String {
    format!("{team_id}|{}|{}", view.label(), project_id.unwrap_or("-"))
}

fn detail_path(issue_id: &str) -> Option<PathBuf> {
    Some(
        tickets_dir()?
            .join("detail")
            .join(format!("{}.json", hash_hex(issue_id))),
    )
}

fn list_path(key: &str) -> Option<PathBuf> {
    Some(
        tickets_dir()?
            .join("lists")
            .join(format!("{}.json", hash_hex(key))),
    )
}

pub fn read_detail(issue_id: &str) -> Option<IssueDetail> {
    let data = std::fs::read_to_string(detail_path(issue_id)?).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn write_detail(detail: &IssueDetail) {
    write_json(detail_path(&detail.id), detail);
}

pub fn read_list(key: &str) -> Option<Vec<Issue>> {
    let data = std::fs::read_to_string(list_path(key)?).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn write_list(key: &str, issues: &[Issue]) {
    write_json(list_path(key), &issues);
}

/// Best-effort JSON write (create parent dir, serialize, write). Any failure is
/// silently ignored — the cache is an optimisation, never a source of truth.
fn write_json<T: serde::Serialize>(path: Option<PathBuf>, value: &T) {
    let Some(path) = path else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_vec(value) {
        let _ = std::fs::write(path, json);
    }
}
