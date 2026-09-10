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
| npm | An npmjs.com account. `npm publish` from `packaging/npm/`, and `npx --yes --package=svipall svipall-mcp` is then the cheapest MCP configuration there is: nothing installed first |

**On an account whose second factor is a passkey, `--otp` does not apply** — that flag takes a TOTP
code, and the CLI cannot run a WebAuthn ceremony. npm falls back to a browser flow and prints a URL
to approve; the publish blocks until you do. A granular access token with **Bypass 2FA** is the
other route, and the one CI would need.

The first publish was manual for a reason that cannot be worked around: **trusted publishing is
configured in a package's settings, and there is no package until something has been published**.

From the second release on it is the route, and `release.yml` already carries the `npm` job for it:
`id-token: write`, no stored secret, and npm generates a provenance attestation by itself. What has
to match exactly, because npm validates none of it when you save the form and only fails at publish
time:

| Field on npmjs.com | Value |
|---|---|
| Provider | GitHub Actions |
| Organization or user | `ilien-dev` |
| Repository | `svipall` |
| Workflow filename | `release.yml` (the filename, not a path, and with the extension) |
| Environment name | leave empty |

`package.json`'s `repository.url` must also match the repository, which is another thing npm checks
only at publish time. It does.

Once a release has published through it, delete any granular access token still on the account:
nothing needs one any more.

crates.io works the same way, per crate, and `scripts/crates-trusted-publishing.sh` configures all
of them in one run: it asks for a token with the **`trusted-publishing`** scope, creates only the
configurations that are missing, and is safe to run again when a crate joins the workspace. Delete
the token afterwards. The `crates` job asks for a token only when a crate is actually missing from
the registry, so a re-run after a partial release does not depend on it.

## The MCP Registry

`server.json` in the repository root is the submission, and the `mcp-registry` job in `release.yml`
sends it. The registry stores **metadata only**: it does not host a byte of this project. What it
does is check, for every package `server.json` names, that the artefact on that package's own
registry carries the server's name — which is how it knows the submission is ours and not somebody
claiming our name.

| Package | Where the name has to be | Written in |
|---|---|---|
| npm | `mcpName` | `packaging/npm/package.json` |
| oci | `LABEL io.modelcontextprotocol.server.name` | `Dockerfile` |
| cargo | a **visible** `mcp-name:` line | `crates/svipall/README.md` |

`registry_manifest.rs` asserts all three against `server.json` offline, so a rename fails `qc`
rather than a release. The cargo one is the trap it exists for: crates.io strips HTML comments when
it renders a README, so the `<!-- mcp-name: … -->` form the registry's own documentation shows for
PyPI and NuGet leaves the validator nothing to find.

**The name is `dev.ilien.svipall/mcp`, and it is permanent.** The registry has no rename and no
unpublish; a different name is a second server, forever. It also decides the authentication: only
DNS authentication grants a *subdomain* of the domain it verifies, so `.well-known` HTTP auth — which
grants the bare domain alone — cannot publish this name.

### The one secret, and the record it answers to

Unlike every other channel here, this one needs a stored secret. Generate the key once:

```bash
openssl genpkey -algorithm Ed25519 -out key.pem
openssl pkey -in key.pem -pubout -outform DER | tail -c 32 | base64   # the TXT record's p=
openssl pkey -in key.pem -noout -text | grep -A3 "priv:" | tail -n +2 | tr -d ' :\n'   # MCP_PRIVATE_KEY
```

Then, once each:

| Where | What |
|---|---|
| DNS for `ilien.dev` | a TXT record on the apex: `v=MCPv1; k=ed25519; p=<public key>` |
| Repository secrets | `MCP_PRIVATE_KEY` = the hex private key |

Keep `key.pem` off this machine's repositories and out of the release. Rotating it is a new TXT
record and a new secret; it does not touch anything already published.

### What the job refuses to do

It runs last, after `npm`, `crates` and `image-manifest`, and it checks that `svipall@<version>` is
on npm and `svipall <version>` is on crates.io before it authenticates. A submission naming a
version a registry cannot serve is a permanent record of a package nobody can install, and the
registry's own error for it — "Registry validation failed for package" — does not say which one.

Re-running a release is safe: the job asks the registry what it already holds and does nothing when
that is this version.

### Somebody else has to say yes

| Channel | What it needs |
|---|---|
| winget | A pull request to `microsoft/winget-pkgs`, reviewed by Microsoft, with validation that installs on a clean VM |
| AUR | A separate account on `aur.archlinux.org` with an SSH key. No human review for a `-bin` package, but it is another account, and Arch only |

Both templates are kept here in case somebody wants to submit them. Neither is advertised in the
README, and neither is on the critical path.

## Publishing a release into the tap and the bucket

The `tap-bucket` job in `release.yml` does it, right after the release publishes: it commits the
rendered `Formula/svipall.rb` and `bucket/svipall.json` and pushes. Its credential is one deploy
key per repository, so the worst a leaked key can do is write to that one repository. Once each:

```bash
ssh-keygen -t ed25519 -N "" -C release -f tap && ssh-keygen -t ed25519 -N "" -C release -f bucket
gh repo deploy-key add tap.pub    -R ilien-dev/homebrew-svipall --allow-write -t svipall-release
gh repo deploy-key add bucket.pub -R ilien-dev/scoop-svipall    --allow-write -t svipall-release
gh secret set HOMEBREW_TAP_DEPLOY_KEY  -R ilien-dev/svipall < tap
gh secret set SCOOP_BUCKET_DEPLOY_KEY  -R ilien-dev/svipall < bucket
rm tap tap.pub bucket bucket.pub
```

Rotating one is the same three lines for that repository, after deleting its old deploy key. By
hand, for a release the job missed: `scripts/render-packaging.sh <version>`, then copy both files
from `packaging/dist/` into the two repositories and push.

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
