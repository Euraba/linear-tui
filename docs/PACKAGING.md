# Packaging & releases

`linear-tui` ships via GitHub Releases, the AUR (Arch), and a Debian/Ubuntu
`.deb`. CI (`.github/workflows/ci.yml`) gates every push on
`fmt + clippy -D warnings + test + release build`; the release pipeline
(`.github/workflows/release.yml`) runs on a `v*` tag.

## Installing (users)

**Arch / `yay`**
```bash
yay -S linear-tui        # builds from source
# or, once published, the prebuilt variant:
yay -S linear-tui-bin
```

**Debian / Ubuntu (`.deb`)**
```bash
# grab the .deb from the latest release, then:
sudo apt install ./linear-tui_<version>_amd64.deb
```

**Prebuilt binary (any glibc Linux)**
```bash
curl -fsSL https://github.com/Euraba/linear-tui/releases/latest/download/linear-tui-<version>-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo install -m755 linear-tui-*/linear-tui /usr/local/bin/linear-tui
```

**From source / crates-style**
```bash
cargo install --git https://github.com/Euraba/linear-tui --locked
```

## Cutting a release (maintainer)

1. Bump `version` in `Cargo.toml` (and `pkgver` in `packaging/aur/PKGBUILD*`),
   commit, and make sure `main` is green.
2. Tag and push:
   ```bash
   git tag v0.1.1 && git push origin v0.1.1
   ```
3. The **Release** workflow then:
   - builds the release binary,
   - produces `linear-tui-<ver>-x86_64-unknown-linux-gnu.tar.gz`, a
     `linear-tui_<ver>_amd64.deb`, and `SHA256SUMS`,
   - creates the GitHub Release with those assets and auto-generated notes,
   - publishes the AUR `linear-tui` package **if** the `AUR_SSH_PRIVATE_KEY`
     secret is set (see below); otherwise that job no-ops.

> Releases are built from `main`. Merge any outstanding feature/fix branches
> (e.g. the API-key hardening) before tagging so the published binary includes
> them.

## One-time AUR setup

The auto-publish job needs an AUR account and an SSH deploy key:

1. Create an account at <https://aur.archlinux.org> and add your **public** SSH
   key under *My Account*.
2. Add the matching **private** key as a repo secret named
   `AUR_SSH_PRIVATE_KEY` (Settings → Secrets and variables → Actions).
3. First submission of the `linear-tui` package must exist on the AUR. Either
   let the deploy action create it on the next tagged release, or seed it
   manually once:
   ```bash
   git clone ssh://aur@aur.archlinux.org/linear-tui.git
   cp packaging/aur/PKGBUILD linear-tui/
   cd linear-tui && updpkgsums && makepkg --printsrcinfo > .SRCINFO
   git add PKGBUILD .SRCINFO && git commit -m "Initial import" && git push
   ```
4. To also offer `linear-tui-bin`, repeat with `packaging/aur/PKGBUILD-bin` in a
   separate AUR repo named `linear-tui-bin`.

Validate a PKGBUILD locally with `namcap PKGBUILD` and a real build with
`makepkg -si` in a clean checkout.

## A real APT repository (optional)

The steps above give a downloadable `.deb`. For `apt install linear-tui` from a
hosted repo you need to serve a signed APT repository. Easiest paths:

- **Hosted (no infra):** push the `.deb` to a free service like Cloudsmith or
  packagecloud.io and follow their "add this repo" snippet.
- **Self-hosted on GitHub Pages:** generate the repo with `aptly` or
  `apt-ftparchive`, sign `Release` with a GPG key (stored as an Actions secret),
  and publish to the `gh-pages` branch. Users then add the key + a
  `deb [signed-by=...] https://<you>.github.io/linear-tui ./` source line.

This isn't wired up yet — say the word and it can be added as a follow-up
(it needs a dedicated GPG signing key).
