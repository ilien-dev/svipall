# Limits, stated on purpose

Lifted out of the README so that file stays readable. Everything here is the same text, with its links repointed.

Most of these are permanent and deliberate; one is a build you have to ask for, and it says so.

- **No bundled proxies or IP rotation service.** You bring your own exit; Svipall configures declared
  timezone, locale and languages and applies supported DNS/WebRTC controls. It does not guarantee
  that all browser traffic uses the exit, and does not detect the proxy's
  country (that would require a geolocation service), so you declare it.
- **No paid or remote captcha solving.** Solving quality is bounded by the models and your hands,
  with no paid-solver quota. Site restrictions and local attempt/time budgets still apply.
  Unresolved challenges can be parked for human assistance and answer replay on the live page.
- **HTTP/3 is off by default, and that is a build choice, not a limit.** It works: a vendored quiche
  on the same BoringSSL the http tier already links, emitting Chrome's QUIC ClientHello — ALPS 17613,
  ECH GREASE, `compress_certificate`, `trust_anchors`, extension permutation, a GREASE transport
  parameter — and Chrome's HTTP/3 SETTINGS frame, both asserted offline against a capture of a real
  Chrome that `bench h3-ref` takes from a loopback QUIC server a real browser handshakes with. It is
  off because a QUIC stack is 37,000 vendored lines to carry for a transport most sites still do not
  offer, and because it can only ever be a *second* visit: `Alt-Svc` is how a site says it speaks h3,
  so the first fetch of any domain is TCP exactly as before. Build with `--features http3` and set
  `http3 = true`.
  **Measured:** four of twelve `hard12` targets advertise h3 at all, and the evasion median does not
  move — 8/12 either way, against a noise floor of 123–369 s per run. What *does* move is cost: on a
  site that offers h3 and walls the cheap tier over TCP, a page arrives in **950 ms at the http tier
  instead of 2,967 ms with a browser**, five runs each; the worst case for a site that advertises h3
  and does not deliver is one extra 568 ms, once per domain per six hours. Still not Chrome: the
  `trust_anchors` payload is empty where Chrome sends a list, and one extension Chrome sends
  (`0x12e0`) is not in this BoringSSL at all — so an h3 engine carries a Chrome version ceiling of its
  own, set by the age of the linked library. The whole record, including **the two reasons this
  project previously gave for not doing HTTP/3 and why both were wrong**, is in
  [`docs/http3.md`](http3.md).
- **Browser-specific fingerprint defenses can conflict with emulation.** Brave is a recorded case:
  with it selected, a public detector saw `navigator.brave` and randomised plugin names next to a
  User-Agent claiming Chrome. Brave, Vivaldi and Opera are therefore sorted last among detected
  browsers. A build **two or more majors** behind the stable channel is flagged for the opposite
  reason — it differs substantially from the reference stable channel. This is a diagnostic
  heuristic, not proof of detection. `web_status` and applicable blocked-result notes can report
  these conditions, and `browser_setup` installs or updates a
  dedicated Chrome for Testing.
- **Software rendering can affect fingerprint consistency.** A browser may report `SwiftShader`
  or `llvmpipe` without hardware acceleration; this does not uniquely identify a VM. Changing a
  renderer string does not reproduce the claimed hardware's output. `web_status` reports detected
  GPU limitations. Supplied model paths support CPU execution; speed depends on the machine.
- **Injected page content can affect detection.** In a recorded run, a local security product
  injected resources into pages. Svipall can report recognized injection evidence in a blocked
  result, but cannot reliably identify every injecting product or remove it.

---
