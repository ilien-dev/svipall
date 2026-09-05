# Package manager manifests

Every channel here installs **the same release artefacts** the GitHub release publishes. Nothing in
this directory compiles anything, and nothing rewrites a binary: a package manager that ships a
different build from the tarball is a second product to support.

```bash
scripts/render-packaging.sh 1.0.0            # fetches the release's sha256sums.txt
scripts/render-packaging.sh 1.0.0 sums.txt   # or uses one you already have
```

That fills `packaging/templates/` into `packaging/dist/` and fails loudly on any placeholder it
could not resolve. The release workflow runs it and attaches the result to the release, so the
manifests for a version always exist next to the artefacts they describe.

## One-time setup, per channel

None of this can be automated from inside this repository, because each one lives somewhere else.

| Channel | Repository to create | Then, on every release |
|---|---|---|
| **Homebrew** | `ilien-dev/homebrew-svipall`, with a `Formula/` directory | Copy `dist/homebrew/svipall.rb` into `Formula/` and push. `brew install ilien-dev/svipall/svipall` |
| **Scoop** | `ilien-dev/scoop-svipall`, with a `bucket/` directory | Copy `dist/scoop/svipall.json` into `bucket/` and push. The manifest carries `checkver`/`autoupdate`, so [excavator](https://github.com/ScoopInstaller/Excavator) can do later bumps |
| **winget** | none — a PR to `microsoft/winget-pkgs` | `wingetcreate submit dist/winget/` , or open the PR by hand under `manifests/i/ilien-dev/svipall/<version>/`. Microsoft's validation runs an install on a clean VM |
| **AUR** | `svipall-bin`, on `aur.archlinux.org` | Copy `dist/aur/PKGBUILD`, run `makepkg --printsrcinfo > .SRCINFO`, push both |
| **.deb / .rpm** | none — attached to the release | Built by the release workflow with `cargo deb --no-build` / `cargo generate-rpm`, from the `dist` binaries |
| **npm** | none — `npm publish` from `packaging/npm/` | Bump its `version` to match the release first. It downloads the archive at install time and verifies it |

Automating the pushes needs a token with write access to a repository that is not this one. That is
a decision with a blast radius, so it is deliberately not wired up here: the workflow renders the
manifests, and a person moves them.

## Keeping the versions in step

`packaging/npm/package.json` carries its own `version`, and it is the URL the postinstall builds —
an npm package a version behind downloads an archive that does not exist. The release workflow's
`version` job already refuses a tag that disagrees with the crate or with the plugin manifest;
extend it to this file when npm is actually published.

## What none of these do

- **They do not install a browser.** The browser tiers want one, the http tier does not, and
  `svipall browser install` is ~190 MB that nobody should spend on somebody's behalf. Homebrew and
  Scoop say so in their caveats; `svipall doctor` says it on every machine.
- **They are not signed by Apple or Microsoft.** There is no Apple Developer ID and no Authenticode
  certificate for this project. macOS builds are signed ad-hoc, which stops the "damaged" dialog
  but not the quarantine flag on a browser download; the release carries a GitHub build
  attestation, verifiable with `gh attestation verify`. Homebrew, Scoop, winget and the install
  scripts all check the published sha256 instead.
