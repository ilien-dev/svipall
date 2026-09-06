# Package manager manifests

Every channel here installs **the same release artefacts** the GitHub release publishes. Nothing in
this directory compiles anything, and nothing rewrites a binary: a package manager that ships a
different build from the tarball is a second product to support.

```bash
scripts/render-packaging.sh 1.0.0-rc            # fetches the release's sha256sums.txt
scripts/render-packaging.sh 1.0.0-rc sums.txt   # or uses one you already have
```

```powershell
pwsh scripts/render-packaging.ps1 -Version 1.0.0-rc
```

Either one fills `packaging/templates/` into `packaging/dist/` and fails loudly on any placeholder
it could not resolve. They produce byte-identical output, LF and no BOM. The release workflow runs
the bash one and attaches the result to the release, so the manifests for a version always exist
next to the artefacts they describe.

## What each channel costs to publish

The distinction that matters is not technical, it is whether somebody else has to say yes.

### Nothing but this repository and its releases

| Channel | Platforms | Where it lives |
|---|---|---|
| `install.sh` / `install.ps1` | macOS, Linux, Windows | This repo, plus the release assets |
| **Homebrew tap** | macOS and Linux | [`ilien-dev/homebrew-svipall`](https://github.com/ilien-dev/homebrew-svipall) |
| **Scoop bucket** | Windows | [`ilien-dev/scoop-svipall`](https://github.com/ilien-dev/scoop-svipall) |
| `.deb` / `.rpm` | Debian/Ubuntu, Fedora/RHEL | Attached to each release; `dpkg -i` / `rpm -i` |
| Container image | Anywhere with Docker | `ghcr.io`, pushed with the workflow's own `GITHUB_TOKEN` |

A tap and a bucket are **repositories, not submissions**. `brew install ilien-dev/svipall/svipall`
and `scoop bucket add svipall …` work the moment the file is in the repo; nobody reviews either.
What does need review is `homebrew-core` or Scoop's own `main` bucket, and neither is necessary.

### Published, one account

| Channel | What it needs |
|---|---|
| npm | An npmjs.com account. `npm publish` from `packaging/npm/`, and `npx --yes svipall-mcp` is then the cheapest MCP configuration there is: nothing installed first |

**On an account whose second factor is a passkey, `--otp` does not apply** — that flag takes a TOTP
code, and the CLI cannot run a WebAuthn ceremony. npm falls back to a browser flow and prints a URL
to approve; the publish blocks until you do. A granular access token with **Bypass 2FA** is the
other route, and the one CI would need.

Better than either, once a package exists: **trusted publishing**. Configure it in the package's
settings on npmjs.com against this repository and `release.yml`, give the job `id-token: write`, and
releases publish over OIDC with no stored secret at all — npm generates provenance attestations by
itself. It cannot be used for a package's *first* publish, because there is no package to configure
it on yet.

### Somebody else has to say yes

| Channel | What it needs |
|---|---|
| winget | A pull request to `microsoft/winget-pkgs`, reviewed by Microsoft, with validation that installs on a clean VM |
| AUR | A separate account on `aur.archlinux.org` with an SSH key. No human review for a `-bin` package, but it is another account, and Arch only |

Both templates are kept here in case somebody wants to submit them. Neither is advertised in the
README, and neither is on the critical path.

## Publishing a release into the tap and the bucket

Once a release has published:

```bash
scripts/render-packaging.sh <version>
cp packaging/dist/homebrew/svipall.rb   ../homebrew-svipall/Formula/
cp packaging/dist/scoop/svipall.json    ../scoop-svipall/bucket/
```

then commit and push each. Automating the push needs a token with write access to a repository that
is not this one, which is a decision with a blast radius, so it is deliberately not wired up: the
workflow renders the manifests and a person moves them.

## The container image is private until you say otherwise

A package that GitHub Actions creates in `ghcr.io` starts **private**, so the first `docker pull`
by anybody else fails with an authentication error that looks like a broken release. Make it public
once, in the package's own settings on GitHub, after the first successful `image` job. There is
nothing to create beforehand: the package comes into existence when the workflow pushes it.

## Keeping the versions in step

`packaging/npm/package.json` carries its own `version`, and it is the URL the postinstall builds:
an npm package a version behind downloads an archive that does not exist. The release workflow's
`version` job refuses a tag that disagrees with the crate, the plugin manifest or this file.

## What none of these do

- **They do not install a browser.** The browser tiers want one, the http tier does not, and
  `svipall browser install` is ~190 MB that nobody should spend on somebody's behalf. The tap and
  the bucket say so in their notes; `svipall doctor` says it on every machine.
- **They are not signed by Apple or Microsoft.** There is no Apple Developer ID and no Authenticode
  certificate for this project. macOS builds are signed ad-hoc, which stops the "damaged" dialog
  but not the quarantine flag on a browser download; the release carries a GitHub build
  attestation, verifiable with `gh attestation verify`. Every channel here checks the published
  sha256 instead.
