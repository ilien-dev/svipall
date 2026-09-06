# Patches

Every deviation from upstream, why it exists, and the test that would catch its loss. Anchored to
named functions, not line numbers.

## Worker identity scoped to its browser (2026-09-05)

`BrowserConfig::worker_init_script` is carried through `HandlerConfig` to `TargetConfig`.
Worker attachment reads that value before the legacy process-wide fallback. An empty script
explicitly selects native workers. This keeps different persistent profile seeds consistent with
their own document, and allows a native-hardware pool after an emulated pool in the same process.
`svipall-mcp/tests/local_sessions.rs` probes document/worker agreement and native WebGL accessors.

---

## 1. The isolated world and its script URL are generated, not constants

**Where** `src/world.rs` (new), consumed by `handler/frame.rs` (`on_frame_execution_context_created`,
`ensure_isolated_world`) and `handler/target.rs` (`on_response`).

**Was** two hard-coded strings:

```
__chromiumoxide_utility_world__
____chromiumoxide_utility_world___evaluation_script__
```

**Why** This is the reason the fork exists. Injected residue — property, context and script names
that no real browser session produces — is one of the detection categories that still works in 2026,
and it is checked by enumeration. A constant that spells out the automation library is the easiest
possible tell: the world name is visible to anything else attached over the protocol, and the
`sourceURL` surfaces in stack traces taken from code that ran in that world.

**Now** both are derived from a seed the embedder supplies once, through `world::seed(u64)`. svipall
passes `IdentityProfile::noise_seed`, the same seed that drives canvas and audio noise, so one
identity really does govern everything. Only the first call takes effect: changing the name
mid-session would silently create a second isolated world and leak the first one's execution
contexts.

**Tests** `world::tests` — five of them. The load-bearing one asserts that no generated name
contains `chromium`, `oxide`, `utility`, `world`, or the name of any other automation framework, for
a spread of seeds. Another asserts the world name and the script URL do not share a prefix, so
leaking one does not give up the other.

---

## 2. `Performance.enable` and `Log.enable` are no longer sent

**Where** `handler/target.rs`, `page_init_commands`.

**Why** Nothing in this crate consumes `Performance.metrics` or `Log.entryAdded`, and svipall never
did either. They were two commands sent on every page for no consumer. Fewer commands is less
protocol chatter and fewer behaviours that differ with a debugger attached.

`Target.setAutoAttach` stays: out-of-process iframes need it, and so does
`Runtime.runIfWaitingForDebugger`.

**Test** covered indirectly by the full suite continuing to pass; nothing read these events, so
their removal has no observable behaviour to assert.

---

## 3. Deliberately **not** patched: `Runtime.enable`

The original plan was to remove it. Measurement killed that.

Between roughly 2022 and 2025, a page could detect any CDP client in four lines: plant a getter on a
thrown `Error`'s `stack`, pass the error to `console.debug`, and see whether the getter fired. With
the `Runtime` domain enabled the browser serialised the argument for the debugging client, and the
serialisation read `stack`. It was the highest-precision automation signal available.

Chrome changed that serialisation path during 2025. Verified here, against this browser, with the
domain explicitly enabled (`enable_runtime()` returning `Ok`): the getter does not fire. The client
still sends `Runtime.enable`; it is simply no longer observable from the page.

Removing it would have meant rebuilding execution-context discovery on top of
`Page.createIsolatedWorld` return values, because `Runtime.executionContextCreated` is the only way
the client learns a context exists. That is a rewrite of `FrameManager` with a real chance of
breaking `evaluate`, the behaviour layer and the captcha solver — to close a vector that is not
open. So it stays, and `svipall-bench fingerprint` carries a permanent check that fails if a future
Chrome reopens the path.

---

## 4. Runtime and features trimmed

**Where** `Cargo.toml`, and `listeners.rs` tests.

Upstream defaults to `async-std` and offers a `fetcher` feature backed by another crate. svipall uses
neither: it runs on tokio and provisions its own browser. Both are gone, which removed `async-std`
and `async-global-executor` from the dependency graph entirely. Upstream's own tests in
`listeners.rs` were written with `#[async_std::test]` and are ported to `#[tokio::test]` — they are
kept because they exercise code this repository now owns.

The `cfg_if!` arms for the removed runtime are still in `async_process.rs`, `browser.rs`, `conn.rs`
and `utils.rs`. They compile away, and rewriting ten nested conditionals for cosmetics would risk a
faithful copy for no behavioural gain. This is why `scripts/qc.*` runs `clippy -D warnings` with
`--exclude svipall-cdp` and reports on this crate separately: the patches are covered by their own
tests, not by lints over nine and a half thousand lines of someone else's code.

---

## 5. `Emulation.setTouchEmulationEnabled` follows the viewport

**Where** `handler/emulation.rs`, `init_commands`.

Upstream hard-codes `true`, so every target got touch emulation regardless of what the `Viewport`
declared. On a desktop identity that means `navigator.maxTouchPoints > 0` and `ontouchstart` on
`window` — a machine with a touch screen it does not claim to have, readable in one line and
contradicting nothing less than the form factor. `Viewport` already carries `has_touch`; it is now
used. A mobile identity still gets touch, because it declares it.

Caught by `svipall-bench tells`, which asserts `maxTouchPoints` against the declared identity at
every browser tier.

---

## 6. `--hide-scrollbars` is no longer passed with `--headless`

**Where** `browser.rs`, `BrowserConfig::launch`.

Upstream adds `--hide-scrollbars` to both headless modes. It is a screenshot convenience with a
side effect: on a page long enough to scroll, `window.innerWidth` equals
`document.documentElement.clientWidth`, where a real desktop Chrome differs by the width of the
scrollbar. Screenshots are taken through `Page.captureScreenshot`, which does not need it.

---

## 7. `Viewport` carries the screen and the window position

**Where** `handler/viewport.rs`, `handler/emulation.rs`.

`Emulation.setDeviceMetricsOverride` accepts `screenWidth`, `screenHeight`, `positionX` and
`positionY`; upstream sends none of them. Two consequences, both page-readable:

* Headless reports a fixed 800x600 display while the launch flags size the window past it, so
  `window.outerWidth > screen.width` — a window wider than the display holding it.
* A window parked off-screen so it stays out of the operator's way reports its real coordinates, so
  `window.screenX` is `-32000`. Nobody browses there.

`Viewport` gains `screen` and `position`, both `Option`, both `None` by default — an embedder that
sets neither gets upstream's behaviour exactly. svipall fills them from the identity's screen.

---

## 8. Workers are given the identity before they run

**Where** `handler/target.rs` (`TargetAttachedToTarget`), new module `worker.rs`.

`Page.addScriptToEvaluateOnNewDocument` covers documents. A `Worker` is a separate target with its
own realm and its own `WorkerNavigator`, and none of the document's overrides reach it — so a
document declaring eight cores beside a worker reporting the host's real thirty-two is one identity
contradicting itself, which a page establishes in a single `postMessage`.

Workers already attach paused, because `Target.setAutoAttach` is sent with
`waitForDebuggerOnStart` (see §2). That leaves a window between the attach and
`Runtime.runIfWaitingForDebugger` in which the realm exists and none of the worker's own code has
run: the script goes in there, ahead of the resume that was already queued.

The script is supplied by the embedder through `worker::set_init_script`, the same arrangement
`world` uses for the isolated world name — this crate does not know what identity is being worn.
Unset, nothing is injected and the behaviour is upstream's. `service_worker` targets are excluded
because §2's handler detaches from them immediately.

---

## 9. An event this protocol definition does not know is not an error

**Where** `src/conn.rs`, the `Err` arm of the `Message<T>` deserialization in `poll_next`, plus the
new `is_unknown_event` beside it.

**Was** every payload that failed to deserialize was logged at `error!` and returned as an `Err`,
which the handler then logged again as *"WS Connection error"*. A single `bench tells` run emitted
**55** of those, and the committed evasion baselines had to be scrubbed of about a thousand more
before they were readable.

**Why it is not an error** the protocol definitions compiled into this crate are pinned, and Chrome
is not: a current Chrome emits events from domains newer than the definitions here. The client is
*designed* to ignore those — nothing is waiting on an event — and an `error!` for a message the
client is built to skip is a log that trains its reader to skip the level that is supposed to mean
something. The baseline README had already noticed and said so; this is that line acted on.

**Now** the discriminator is `id`, and it is the right one:

- a message **with** `id` is a response to a call we made, something is waiting on it, and a
  deserialization failure there is a real fault. Unchanged: still `error!`, still an `Err`.
- a message with **no** `id` and a `method` is an event. Logged at `debug!` and skipped the way
  `Ping`/`Pong` already are — `wake_by_ref` and `Poll::Pending`, so the stream simply continues.
- anything else — not an object, or an object with neither field — is left exactly as it was.
  Not recognising a shape is not a reason to swallow it.

**Caught by** `conn::ignorable_tests`, three tests over the classifier: an unknown event is
ignored, a response with an unparseable `result` and one with an `error` are both **not**, and a
payload that is not a CDP message at all is not either.

**Verified end to end** `bench tells --assert` went from 55 of those lines to **0**, with
160/160 probes still clean.
