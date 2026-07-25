//! Funding surfaces, in one place.
//!
//! linear-tui is MIT and stays that way: nothing in this module gates a
//! feature, checks a licence, or touches the network. It only holds the links
//! that the CLI (`linear-tui sponsor`, `linear-tui --version`), the TUI help
//! overlay, and the README all render, so they can't drift apart.

/// GitHub Sponsors profile for the maintainer — the default way to fund the
/// project. Mirrored in `.github/FUNDING.yml`, which drives the repo's Sponsor
/// button; change both together.
pub const SPONSOR_URL: &str = "https://github.com/sponsors/Euraba";

/// Where companies write about paid support, priority fixes, or contract work.
pub const COMMERCIAL_CONTACT: &str = "rares@trydio.com";

/// Long-form tiers and what each one actually buys.
pub const SPONSORS_DOC_URL: &str =
    "https://github.com/Euraba/linear-tui/blob/main/docs/SPONSORS.md";

/// One line, short enough for the TUI help overlay and the `--version` footer.
pub fn one_liner() -> String {
    format!("Free and MIT. Support it: {SPONSOR_URL}")
}

/// The full `linear-tui sponsor` output.
pub fn blurb() -> String {
    format!(
        "linear-tui is free software (MIT), built and maintained in the open.\n\
         There is no paid tier and no licence check — if it's useful to you or\n\
         your team, sponsoring is what keeps it maintained.\n\
         \n\
         Sponsor:     {SPONSOR_URL}\n\
         Tiers:       {SPONSORS_DOC_URL}\n\
         Commercial:  {COMMERCIAL_CONTACT}\n\
         \n\
         Commercial enquiries welcome: paid support, priority bug fixes,\n\
         private packaging, and contract feature work."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blurb_carries_every_funding_link() {
        let b = blurb();
        for link in [SPONSOR_URL, SPONSORS_DOC_URL, COMMERCIAL_CONTACT] {
            assert!(b.contains(link), "blurb is missing {link}");
        }
    }

    #[test]
    fn one_liner_is_a_single_short_line() {
        let l = one_liner();
        assert!(!l.contains('\n'), "one_liner must stay on one line");
        // The TUI help overlay is drawn in a 60%-wide centred box; keep this
        // comfortably inside a narrow (80-column) terminal.
        let width = l.chars().count();
        assert!(width <= 64, "one_liner too wide ({width} cols): {l}");
        assert!(l.contains(SPONSOR_URL));
    }
}
