//! The stale-while-revalidate ticket cache (memory + optional disk), plus the
//! Settings-overlay cache-mode toggle.

use crate::cache;
use crate::domain::{Issue, IssueDetail};
use crate::settings::Settings;

use super::App;

impl App {
    /// Look up a cached issue detail (memory first, then disk for
    /// [`CacheMode::Disk`](crate::settings::CacheMode::Disk)), populating the
    /// memory layer from disk on a hit.
    pub(super) fn cached_detail(&mut self, id: &str) -> Option<IssueDetail> {
        if !self.cache_mode.uses_memory() {
            return None;
        }
        if let Some(d) = self.detail_cache.get(id) {
            return Some(d.clone());
        }
        if self.cache_mode.uses_disk() {
            if let Some(d) = cache::read_detail(id) {
                self.detail_cache.insert(id.to_string(), d.clone());
                return Some(d);
            }
        }
        None
    }

    /// Store a freshly-fetched detail into the cache (memory, plus disk for
    /// [`CacheMode::Disk`](crate::settings::CacheMode::Disk)).
    pub(super) fn store_detail(&mut self, detail: &IssueDetail) {
        if !self.cache_mode.uses_memory() {
            return;
        }
        self.detail_cache.insert(detail.id.clone(), detail.clone());
        if self.cache_mode.uses_disk() {
            cache::write_detail(detail);
        }
    }

    /// Look up a cached issue list (memory first, then disk).
    pub(super) fn cached_list(&mut self, key: &str) -> Option<Vec<Issue>> {
        if !self.cache_mode.uses_memory() {
            return None;
        }
        if let Some(v) = self.issue_list_cache.get(key) {
            return Some(v.clone());
        }
        if self.cache_mode.uses_disk() {
            if let Some(v) = cache::read_list(key) {
                self.issue_list_cache.insert(key.to_string(), v.clone());
                return Some(v);
            }
        }
        None
    }

    /// Store a freshly-fetched issue list into the cache.
    pub(super) fn store_list(&mut self, key: &str, issues: &[Issue]) {
        if !self.cache_mode.uses_memory() {
            return;
        }
        self.issue_list_cache
            .insert(key.to_string(), issues.to_vec());
        if self.cache_mode.uses_disk() {
            cache::write_list(key, issues);
        }
    }

    /// Cycle the cache mode (Off → Memory → Disk → Off), persist it, and drop
    /// the in-memory caches when caching is turned off.
    pub(super) fn cycle_cache_mode(&mut self) {
        self.cache_mode = self.cache_mode.cycle();
        Settings {
            cache_mode: self.cache_mode,
        }
        .save();
        if !self.cache_mode.uses_memory() {
            self.detail_cache.clear();
            self.issue_list_cache.clear();
        }
        self.status = format!("Cache mode: {}", self.cache_mode.label());
    }
}
