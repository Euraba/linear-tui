# Sponsoring linear-tui

linear-tui is MIT-licensed and will stay that way. There is no paid tier, no
licence key, and no feature behind a paywall — the binary you install is the
whole thing, and it never phones home.

What sponsorship buys is **maintenance**: keeping up with Linear's API,
answering issues, shipping the `.deb`/AUR packages, and the unglamorous work
that keeps a terminal client working on everyone's machine.

**[Sponsor on GitHub →](https://github.com/sponsors/Euraba)**

## Tiers

These are commitments made in good faith by one maintainer, not an enterprise
support contract. Response times are targets, not guarantees — see
[Honest limits](#honest-limits).

| Tier | Amount | What you get |
| ---- | ------ | ------------ |
| **Supporter** | $5 / month | Name in `SPONSORS.md`, if you want it. Mostly: you keep the lights on. |
| **Power user** | $25 / month | The above, plus your issues get looked at first. |
| **Team** | $100 / month | The above, plus your team's logo in the README, and bug reports triaged within ~2 business days. |
| **Company** | $500 / month | The above, plus a private channel (email or shared Slack Connect), input on the roadmap, and priority on bugs that block your team. |

One-off contributions are equally welcome — GitHub Sponsors supports them, and
there's no expectation of a recurring commitment.

## Commercial work

Separate from sponsorship, the following is available as paid contract work:

- **Feature development** — something your team needs that isn't on the
  roadmap. Built in the open under MIT unless you need otherwise.
- **Private packaging and deployment** — internal distribution, pinned builds,
  air-gapped installs, custom config defaults across a fleet.
- **Integration work** — wiring the `serve` JSON-RPC backend or the one-shot
  CLI into your own tooling, editor plugins, or agent workflows.
- **Support retainer** — a fixed monthly block of hours for a team that depends
  on linear-tui in its daily workflow.

Enquiries: **rares@trydio.com**. Scope and rate are agreed up front; there are
no per-seat fees and nothing you install changes.

## Why not just sell it?

Because it wouldn't work. linear-tui is MIT, and the entire source is public —
a paywalled fork is a `git revert` away for anyone who wants one. Gating
features would cost the project its users without producing meaningful revenue.
Sponsorship and contract work are the honest models for a tool like this, so
those are the ones on offer.

## Honest limits

- This is maintained alongside other work. Response targets above are what the
  maintainer aims for, and they'll be communicated if they slip.
- Sponsorship does not buy an SLA, an indemnity, or a guarantee that a specific
  feature ships. If you need any of those, the contract work above is the right
  route, with terms written down.
- Sponsoring never changes the licence or the binary. Every sponsor and every
  non-sponsor runs exactly the same MIT-licensed build.

## For maintainers of this repo

The funding links live in three places and must be changed together:

- `.github/FUNDING.yml` — the repo's Sponsor button
- `src/sponsor.rs` — `linear-tui sponsor`, `linear-tui --version`, the `?` help overlay
- this file, and the README's Sponsor section
