# The benchmarks, and the rules they are read under

`svipall-bench` is a workspace member at `bench/`, not under `crates/`. It has eight modes; two of
them belong in the gate because they touch no network, and the rest are run by hand — because they
reach the network, or because they start a browser.

```
cargo run -p svipall-bench --release -- micro --assert          # CPU budgets + structural counts
cargo run -p svipall-bench --release -- tells --assert          # automation tells, loopback page
cargo run -p svipall-bench --release -- fingerprint             # what public detectors see
cargo run -p svipall-bench --release -- fingerprint --engine chrome   # identity coherence, offline
cargo run -p svipall-bench --release -- evasion --set S --runs 3      # walls, by target set
cargo run -p svipall-bench --release -- extract --corpus DIR          # extraction quality
cargo run -p svipall-bench --release -- cache                         # cold vs warm fetch
cargo run -p svipall-bench --release -- h3 --set S                    # who offers h3, and does it work
cargo run -p svipall-bench --release --features http3 -- h3-ref       # Chrome's own HTTP/3 SETTINGS
```

## What each one is for

| Mode | Asserts? | What it protects |
|---|---|---|
| `micro` | yes, in `qc` | CPU budgets with 25 % headroom, plus the structural invariants: exactly one DOM parse, zero disk reads on the hot path |
| `tells` | yes, in `qc` | That nothing of ours is readable from a page — no named global, no shadowed host object, no realm disagreeing with another (worker **and** same-origin iframe), no accessor that stringifies to its own source in any realm, no window at an impossible coordinate. Thirty-two probes at all four browser tiers against a document served on loopback, and the probe list is frozen, so one lost to a syntax error fails the build instead of shrinking the total |
| `fingerprint --engine chrome` | yes, in `qc` | Every identity svipall can wear, checked against itself offline. A Chrome UA on a Firefox engine, a desktop with no taskbar, a renderer that contradicts the engine — all fail the build |
| `fingerprint` (no flag) | no | The same, plus eight network checks against `tls.peet.ws`. Needs the network. The sixteen it used to run inside a live browser are `tells` probes now: they needed a document, not a network, and there they ran at one tier and asserted nothing |
| `evasion` | no | Success rate against sites with known walls, per target set. `--http3` primes `Alt-Svc` and speaks QUIC where a site advertised it |
| `h3` | no | How many targets advertise HTTP/3 at all — the ceiling on anything the transport can do — and whether the QUIC engine gets the same page. `--h3-first` reverses the request order, because these vendors score an address and whichever request goes second is asking a server that has already seen us |
| `h3-ref` | no | What a real Chrome sends as HTTP/3 SETTINGS, read off a QUIC server this process runs on loopback. No network, but it starts a browser, so it is not in the gate; the assertion built on it is `crates/svipall-quic/tests/settings.rs`, which is offline and does run in the gate|
| `extract` | in `qc` when the corpora are on disk | Main-content extraction against published gold standards |

## The rule every evasion number is read under

Any figure in `bench/baseline/` is the **median of three runs with its range**, targets in a fresh
random order each run, cooldowns cleared before each run, from **one residential address with no
proxy**. The reputation spend is **not** cleared: a cooldown is the site's word about the
previous run and clearing it is what makes three runs comparable, while the spend is what this
address has done to that host and is the thing these numbers are meant to respect. `bench evasion`
refuses to start a set whose address has already spent past the soft line, says how long the wait
is, and records `spent_before` on every cell so a published number carries the state it was taken
in. `--ignore-budget` measures anyway and marks the run `forced_over_budget`.

A change counts as an improvement **only when the median moves outside the previous range**.
Anything less is the noise band of a twelve- or thirty-one-target list run from one address, and
reading it as signal is how a benchmark starts lying.

Runs that improved nothing are published too. `bench/baseline/README.md` keeps the history,
including the rounds of work that moved no number, and the two measurement bugs that were found
and fixed rather than quietly corrected.

## Three lists, and why they are never one number

* **`hard12`** — svipall's own list: twelve sites chosen *because* they have walls, scored by
  whether the expected text came back with no wall reported.
* **`public31`** — the list an independent benchmark published in May 2026 (seven stealth tools,
  31 targets, 651 verdicts), scored with **that benchmark's own four-way rule**
  (`ok | gated | blocked | error`) ported verbatim into `bench/src/targets.rs`, so a cell here
  means what a cell there means. Twenty-five of its 31 pass for every tool measured there,
  including unpatched automation; the signal lives in six cells.
* **`vendors8`** — two targets each behind the proof-of-work vendor, the edge vendor, the
  fingerprinting vendor and the managed challenge.

A 9/12 and a 28/31 are not the same kind of number. Quoting one against the other — in either
direction — is reading noise as signal, so all three are published, each beside its list.

**The membership of `hard12` and `public31` is frozen.** A new target goes in a new list; a test in
`targets.rs` enforces the count. Moving a target is how a benchmark becomes a press release.

### The ported rule is not always right, and that is recorded rather than chased

`public31`'s body rule counts `cdn-cgi/challenge-platform` as a gate. Every Cloudflare customer
page carries that script, challenge or not, so the rule marks fully delivered pages as `gated` —
measured directly on `medium.com` and `canadianinsider.com`, both answering `200` with their real
titles and 45–50 KB of their own content. svipall's classifier is right and the ported rule is
over-broad. Escalating those cells to win them back would mean opening a browser on a page already
in hand, so it is not done: the disagreement is published instead.

## Regenerating the baselines

```
cargo run -p svipall-bench --release -- fingerprint                    > baseline/fingerprint.json 2> baseline/fingerprint.txt
cargo run -p svipall-bench --release -- tells --assert                 > baseline/tells.json       2> baseline/tells.txt
cargo run -p svipall-bench --release -- evasion --set hard12   --runs 3 > baseline/evasion.json    2> baseline/evasion.txt
cargo run -p svipall-bench --release -- evasion --set public31 --runs 3 > baseline/public31.json   2> baseline/public31.txt
cargo run -p svipall-bench --release -- evasion --set vendors8 --runs 3 > baseline/vendors8.json   2> baseline/vendors8.txt
```

stdout is the machine-readable copy, stderr the human one. Both are committed.

`--exit URL` runs any set through an exit you supply, which is the only way to separate "svipall
cannot do this" from "this address cannot". The un-proxied column stays the headline, because it is
the one anybody can reproduce without buying anything.

## A note for Windows

The modes that open a browser and the ones that toggle a feature flag both write
`target/release/svipall-bench.exe`. Windows keeps the image of a process that has just exited
locked for a moment, so `qc` gives the `--features onnx` step its own `CARGO_TARGET_DIR` rather
than relinking the same path three times in a row.
