# svipall-core

Core primitives for svipall — fast Rust port of webone server.py.

- `classify.rs` — wall detection (cloudflare, vendor, hold, login, etc.)
- `throttle.rs` — per-domain throttling and cooldowns
- `profiles.rs` — per-domain browser profiles with LRU eviction
- `ladder.rs` — tier escalation logic
- `extraction.rs` — html->text/markdown + BM25 filter
- `domain.rs` — domain extraction
