# Configuration

Lifted out of the README so that file stays readable. Everything here is the same text, with its links repointed.

Use `svipall config show`, `svipall config set key=value`, or `svipall config preset local`.
The default identity policy is `auto`: learn useful emulated routes first, with at most one native
browser attempt as a last resort. Existing explicit `emulated` or `native` settings are preserved;
use `svipall config preset auto` to migrate an existing installation. Connected MCP
clients can save browser policy through `web_status` with a `configure` object; running MCP/REST
servers apply it on the next request. See [local configuration and sessions](local-configuration.md)
for identity modes, bounded waits, browser provisioning and which settings apply live.

### Automatic routing, privacy and practical limits

No per-site setup is required. Automatic fetches learn locally by domain, route family, exit and
browser environment. Successful delivery with full content quality and observed latency can
promote an emulated route after two supporting observations. Repeated failures demote it; evidence
expires after 24 hours. Routes that repeatedly fail are skipped for 30 minutes; the strongest
allowed emulated probe remains available, and a repeatedly failing native fallback is also paused.
The current implementation additionally remembers classified fingerprint/hold walls for 30 minutes
to avoid weaker probes when a headful route is permitted. Generic errors and native-only walls
do not supply that evidence, and a later delivery clears the marker on its route.
Within a fetch, it also uses the existing managed-challenge discriminator to skip
weaker routes when headful emulation is allowed. Ordinary interstitials retain cheaper exploration;
this per-call decision does not create fingerprint-wall memory or change the caller's deadline.
This is a heuristic: it cannot prove that the requested information is
complete or guarantee the best route or a successful fetch. Short pages are returned with quality
labels and do not, by themselves, trigger a native attempt.

Privacy takes priority over delivery scores: even a successful native route stays last. Automatic
native fallback is excluded for named profiles, isolated visits, mobile requests, forced tiers and
non-GET requests. Native and emulated automatic profiles use separate directories and cookie jars.
Detected login walls, subscriptions and missing pages stop escalation. HTTP 429/503 triggers
backoff. Quotas expressed only in page text can escape classification, as the current audit shows;
respect an observed restriction even when the tool labels the response as delivered.

**Native mode exposes real browser/device characteristics**, potentially including graphics,
hardware capabilities, screen, language and timezone. Sites can correlate these across visits and
cookie profiles. Emulation reduces some exposure but guarantees neither anonymity nor IP hiding.
The browser uses tool-managed profiles, and its sandbox remains enabled. Launch flags no longer
request disabling site isolation, client phishing detection or IPC flooding protection. Browser
updates and host configuration still matter. Results report `identity_used`, `native_fallback`
and a `privacy_notice` whenever native fallback was attempted, even if it failed. To prevent all automatic native
fallback, run `svipall config set auto_native_fallback=false`; `browser_identity=emulated` also
keeps browser requests emulated. Explicit `browser_identity=native` is a separate manual override.

Defaults permit **12 top-level transport attempts per 60 seconds per domain and exit**, a minimum
**1 second between scheduled attempts**, and **6 attempts per automatic fetch**, within its total
timeout. Exceeding the visit window starts a **15-minute cooldown**. HTTP 429/503 stops escalation
and persists a cooldown of at least 15 minutes, or the full `Retry-After` when longer. Rejected calls
do not prolong that cooldown. The existing decaying address budget can stop work sooner.

The visit ledger is transactional and persists across restarts. Changing identity or forcing a
tier does not reset it. Successful cache hits do not consume visits. Returned pages are preserved
when further attempts are refused, with `stopped_reason` and `cooldown_seconds_left`; callers should
wait instead of repeatedly retrying. The legacy `clear_cooldown` action does not erase this ledger.
These limits reduce traffic and exposure; they cannot promise that a site will not block an IP.

The accounting unit is a fetch attempt or supported browser-tool navigation, **not every network
request**: resource loads, redirects, origin warmup, challenge exchanges, scripts and interactive
actions can generate additional traffic. Local development hosts are exempt. This is not a browser
firewall or a universal request ceiling. Adjust limits through `svipall config set` or
`web_status(configure={...})`; `svipall status` reports the effective limits. Learning and admission
need no third-party solver, service, API key or downloaded learning model.

`~/.svipall/config.toml` (or `$SVIPALL_HOME/config.toml`). Every field has a default, so a missing or
partial file is fine. The default state directory contains:

| | |
|---|---|
| `config.toml` | The settings below |
| `settings.toml` | Validated settings saved by CLI/MCP, overriding `config.toml` |
| `secrets.env` | Credentials can be referenced by name in supported action calls; this does not redact returned content |
| `domain_tiers.json` | Legacy starting-tier memory for explicit emulated/native identity policies |
| `automatic_routes.json` | Local route evidence under hashed context keys, expiring after 24 hours |
| `traffic.sqlite3` | Transactional visit windows and cooldowns, shared across modes and processes |
| `pools.json`, `exit_health.json` | Exits per domain, and what each one has done on each |
| `reputation.json` | What each address has spent with each host, decaying with a half-life |
| `svipall.db` | Page cache, crawl frontiers, notes, watches, quality histograms and the request log |
| `jobs.db` | Challenges seen, how they were answered, and the corpus |
| `profiles/`, `auto_profiles/`, `sessions/` | Named profiles, the per-domain ones the ladder makes, and the one-fetch isolated ones |
| `models/` | Models you installed, which win over the embedded ones |
| `browser/` | Chrome for Testing, provisioned automatically when absent or installed explicitly |
| `in/`, `out/` | Where `file://` reads from and a relative `out_file` lands |
| `screenshots/` | What `web_screenshot` wrote |

<details>
<summary><b>Common <code>config.toml</code> settings</b></summary>

```toml
# Browser and tiers
browser_path = ""            # wins over everything when set and the file exists. Order after that:
                             # SVIPALL_BROWSER / CHROME_PATH / CHROME_BIN /
                             # PUPPETEER_EXECUTABLE_PATH, then the one `browser install` put in
                             # ~/.svipall/browser, then auto-detection
max_tier = "warm"            # cap for mode=auto
browser_auto_install = true  # provision a managed browser on startup if none is installed
browser_identity = "auto"    # emulated routes first; native only as a last resort
auto_native_fallback = true # false prohibits automatic real-device exposure
auto_max_attempts = 6       # per automatic fetch, including native; valid range 1..6
request_limit = 12          # top-level attempts per domain and exit in the window
request_window_seconds = 60
request_cooldown_seconds = 900
request_min_interval_ms = 1000
browser_timeout_ms = 45000
warm_wait_ms = 20000         # how long `warm` waits for a challenge to clear
warm_adaptive = true         # allow recognized proof-of-work to reach one renewal
warm_max_wait_ms = 55000     # hard warm-stage budget; the request timeout still applies
browser_idle_secs = 180
warm_keep_max = 2            # cleared pages held open between fetches; 0 disables holding entirely
warm_keep_secs = 120         # how long a held page may go unused. Above the proof-of-work token
                             # lifetime and below browser_idle_secs, and a test asserts both
http_engine = "auto"         # the emulating engine when built with `impersonate`, else reqwest
http_firefox = false         # present Gecko coherently on the http tier: TLS, headers, UA, no Sec-CH-UA
http3 = false                # speak HTTP/3 to sites that advertised it. Needs `--features http3`;
                             # a first visit is TCP either way, because Alt-Svc is what turns it on

# Identity and exits
locale = ""                  # empty = follow the exit's declared country
timezone = ""
exit_strategy = "sticky"     # or round_robin, for domains with a pool of exits
reputation_budget = 250      # what one address may have outstanding with one host; 0 = off
reputation_half_life_hours = 6   # how long until half of what was spent stops counting
dns_over_https = ""          # e.g. https://dns.example/dns-query; empty = off; not a network-wide DNS policy

# Crawling and output
parallelism = 4              # web_fetch_many / web_crawl; tightened further by machine load
max_tokens_per_fetch = 25000
max_tokens_total = 60000     # cap across a whole crawl
overlap_blocks = 1           # blocks of the previous page a `cursor` continuation repeats; 0 = none

# Policy
allow_origins = []
block_origins = []           # blocking wins over allowing
refuse_private_addresses = false  # OFF by default, and not because the risk is small: fetching
                             # http://localhost is an ordinary thing for an operator to ask for.
                             # Turn it on for an installation where an agent picks its own URLs.
local_roots = []             # directories file:// may read; empty = ~/.svipall/in only
block_ads = false            # a real trade: pages whose third parties all fail load differently
blocklist_sources = [        # only fetched when block_ads = true
  "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts",
  "https://easylist.to/easylist/easyprivacy.txt",
]

# Solver and dashboard
corpus_keep_days = 30        # how long solved captchas keep their images for export-corpus; 0 = none
solver_workers = 4
dashboard_port = 8787
dashboard_bind = "127.0.0.1" # set to a LAN address to answer challenges from a phone
log_level = "info"

# REST API
rest_port = 0                # 0 = off. `svipall serve` starts it anyway; this is what makes
                              # svipall-mcp mount it too. It exposes the 19 routes listed above.
rest_bind = "127.0.0.1"
api_key = ""                 # empty = ~/.svipall/api_key, generated on first use and printed once
max_jobs = 2                 # long jobs at once — not `parallelism`, which bounds one job's fetches
```
</details>

| Env var | Default | Effect |
|---|---|---|
| `SVIPALL_HOME` | `~/.svipall` | Config, cache, profiles, models, blocklists |
| `SVIPALL_BROWSER` | — | Path to a browser binary. Also honoured: `CHROME_PATH`, `CHROME_BIN`, `PUPPETEER_EXECUTABLE_PATH` |
| `SVIPALL_HTTP_ENGINE` | `http_engine` | Which http engine runs; beats the config file |
| `SVIPALL_HUMAN_ASSIST` | on | Open a visible window when a token captcha cannot be auto-solved |
| `SVIPALL_HUMAN_WAIT_SECS` | 180 | How long that window waits |
| `SVIPALL_DASHBOARD_PORT` | `dashboard_port` | Port the human dashboard listens on |
| `SVIPALL_REST_PORT` | `rest_port` | Port the REST API listens on inside `svipall-mcp`. The Docker knob |
| `SVIPALL_API_KEY` | — | Pin the bearer key, for a container whose home is not writable |

---
