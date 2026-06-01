//! Image support: pulling image URLs out of an issue's markdown, the on-disk
//! cache location, and the per-URL render state the UI holds.
//!
//! Linear writes images embedded in issue descriptions and comments as markdown
//! `![alt](url)`. We extract those URLs, download+cache the bytes under
//! `~/.cache/linear-tui/images/`, decode them, and render them with the
//! terminal's graphics protocol (kitty/sixel/iterm2, or a halfblocks fallback)
//! via [`ratatui_image`].

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use ratatui_image::protocol::StatefulProtocol;
use sha2::{Digest, Sha256};

use crate::models::IssueDetail;

/// The render state for one image URL, kept in the UI's session-long cache
/// (keyed by URL). The decoded protocol is reused across viewer opens.
pub enum ImageState {
    /// Bytes are being fetched/decoded by the worker.
    Loading,
    /// Decoded and ready; the viewer renders it.
    Ready(StatefulProtocol),
    /// Download or decode failed; the message is shown in the viewer.
    Failed(String),
}

/// Image URLs referenced by an issue's description and comment bodies, in the
/// order they first appear, de-duplicated. Recognises markdown image syntax
/// `![alt](url)` (an optional `"title"` after the url is dropped).
pub fn detail_image_urls(detail: &IssueDetail) -> Vec<String> {
    let mut urls = Vec::new();
    if let Some(desc) = &detail.description {
        collect_image_urls(desc, &mut urls);
    }
    for c in &detail.comments {
        collect_image_urls(&c.body, &mut urls);
    }
    // Keep first occurrence only, preserving order.
    let mut seen = HashSet::new();
    urls.retain(|u| seen.insert(u.clone()));
    urls
}

/// Append every `![alt](url)` image URL found in `md` to `out`.
fn collect_image_urls(md: &str, out: &mut Vec<String>) {
    let bytes = md.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        // An image starts with `![`; the link target is between `](` and `)`.
        if bytes[i] == b'!' && bytes[i + 1] == b'[' {
            if let Some(rel) = md[i + 2..].find("](") {
                let open = i + 2 + rel + 2; // index just past "]("
                if let Some(close) = md[open..].find(')') {
                    let inner = &md[open..open + close];
                    // `url "optional title"` — keep only the url.
                    let url = inner.split_whitespace().next().unwrap_or("").trim();
                    if is_http_url(url) {
                        out.push(url.to_string());
                    }
                    i = open + close + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
}

fn is_http_url(s: &str) -> bool {
    s.starts_with("https://") || s.starts_with("http://")
}

/// Whether `url`'s host is a Linear host. We only attach the personal API key
/// (sent verbatim in `Authorization`) when fetching from Linear, so the secret
/// never leaks to third-party image hosts referenced in an issue.
pub fn is_linear_host(url: &str) -> bool {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    // Strip optional `user@` and `:port`.
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    host.eq_ignore_ascii_case("linear.app") || {
        let lower = host.to_ascii_lowercase();
        lower.ends_with(".linear.app")
    }
}

/// `~/.cache/linear-tui/images/` (honours `$XDG_CACHE_HOME`), if a cache dir
/// can be determined.
pub fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("linear-tui").join("images"))
}

/// On-disk cache path for an image URL: `<cache_dir>/<sha256(url)>[.<ext>]`.
/// The sha256 keeps filenames safe/stable; the extension (when the URL has a
/// recognisable one) is cosmetic — decoding sniffs the actual format.
pub fn cache_path(url: &str) -> Option<PathBuf> {
    let dir = cache_dir()?;
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let digest = hasher.finalize();
    let mut name = String::with_capacity(digest.len() * 2 + 5);
    for b in digest {
        let _ = write!(name, "{b:02x}");
    }
    if let Some(ext) = url_extension(url) {
        name.push('.');
        name.push_str(&ext);
    }
    Some(dir.join(name))
}

/// A recognised raster image extension from the URL path, lower-cased.
fn url_extension(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last = path.rsplit('/').next().unwrap_or(path);
    let ext = last.rsplit_once('.')?.1.to_ascii_lowercase();
    const KNOWN: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];
    KNOWN.contains(&ext.as_str()).then_some(ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_markdown_images_in_order_deduped() {
        let md = "intro ![a](https://x/1.png) mid ![b](http://y/2.jpg \"t\") \
                  again ![c](https://x/1.png) and [a link](https://x/page) too";
        let mut out = Vec::new();
        collect_image_urls(md, &mut out);
        assert_eq!(out, vec!["https://x/1.png", "http://y/2.jpg", "https://x/1.png"]);
    }

    #[test]
    fn ignores_non_http_and_plain_links() {
        let mut out = Vec::new();
        collect_image_urls("![rel](/local/a.png) ![ok](https://z/b.png)", &mut out);
        assert_eq!(out, vec!["https://z/b.png"]);
    }

    #[test]
    fn linear_host_gate_is_strict() {
        assert!(is_linear_host("https://uploads.linear.app/abc/def.png"));
        assert!(is_linear_host("https://linear.app/x.png"));
        assert!(is_linear_host("https://UPLOADS.LINEAR.APP/x.png?sig=1"));
        // Must not be fooled into leaking the key to look-alikes.
        assert!(!is_linear_host("https://linear.app.evil.com/x.png"));
        assert!(!is_linear_host("https://evil.com/linear.app/x.png"));
        assert!(!is_linear_host("https://i.imgur.com/x.png"));
    }

    #[test]
    fn cache_path_uses_sha256_and_keeps_known_ext() {
        let p = cache_path("https://uploads.linear.app/a/b/photo.PNG?x=1").unwrap();
        let name = p.file_name().unwrap().to_string_lossy();
        // 64 hex chars + ".png"
        assert!(name.ends_with(".png"), "got {name}");
        assert_eq!(name.len(), 64 + 4);
        assert!(name[..64].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn cache_path_omits_unknown_ext() {
        let p = cache_path("https://uploads.linear.app/a/b/file").unwrap();
        let name = p.file_name().unwrap().to_string_lossy();
        assert_eq!(name.len(), 64);
    }
}
