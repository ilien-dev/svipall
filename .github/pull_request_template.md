<!--
Title: a Conventional Commit of at most 72 characters, e.g. "feat: export the cover image".
Branch: the same type + / + 1 to 5 lowercase words joined by hyphens, e.g. feat/export-cover-image.
One PR = one change. Read CONTRIBUTING.md before opening it.
Comments like this one are removed before the limits are counted.
-->

## What
<!-- At most 280 characters. The one change this PR makes, in one or two sentences. -->

## Why
<!-- At most 280 characters. The problem it solves for the people who use Svipall. Link the issue: "Closes #12". -->

## How
<!-- At most 500 characters. The approach, and the main files it touches. -->

## Risk
<!-- At most 200 characters. What could break, and how to undo it. "None: <reason>" when nothing can. -->

## Measurement
<!--
At most 500 characters. The numbers on main and on this branch, before -> after: they must
show the change adds value, or at least that nothing gets worse. How to measure here: CONTRIBUTING.md.
Required for `feat`, `fix` or `perf` changes to what users get. Otherwise "Not applicable:" and the reason.
-->

## Evidence
<!--
At most 300 characters of text; images, GIFs and links do not count.
How you checked it, and how a reviewer can check it in a minute (a command, a page, a step).
Screenshots are optional and always welcome; for a visual change, before and after.
Capture only that window or page, never the whole screen. Drag the files into this box on GitHub.
-->

## Checklist
- [ ] This PR does one thing, and nothing else is mixed in
- [ ] I searched open PRs and issues and nobody is already doing this
- [ ] `cargo test --workspace` passes
- [ ] `scripts/qc.sh` passes
- [ ] I did not edit any version number or CHANGELOG.md

## Contributor Licence Agreement

I have read CLA.md and I agree to it for this and my future contributions to Svipall.

<!-- Keep the line above unchanged: it is how CLA.md is accepted. You keep the copyright in your
     work, and the maintainer may also license it under other terms, including proprietary ones.
     Below it, add "Signed-off-by: Your Name <you@example.com>" to certify the Developer
     Certificate of Origin. Without both, the pull request cannot be merged. -->
