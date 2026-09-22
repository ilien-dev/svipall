# Privacy and safety

Lifted out of the README so that file stays readable. Everything here is the same text, with its links repointed.

- **Hidden-text sanitization.** Extraction removes selected hidden elements, inline hiding styles
  and zero-width characters but does not resolve the full CSS cascade or detect all hidden content.
  Visible malicious instructions can remain, so this is not a complete prompt-injection defense and
  returned page content has to be treated as untrusted data.
- **Credentials can be referenced without placing values in a tool call.** `{"do":"type","ref":"e4","text":"${SHOP_PASSWORD}"}` is
  substituted from `~/.svipall/secrets.env` on the way to the browser, and `web_status` lists
  names, never values. Returned page text, screenshots or API responses can still expose data the site
  displays, because secret substitution is not output redaction.
- **Origin policy, checked before the request:** `allow_origins`, `block_origins` (blocking wins)
  and `block_ads` (cached lists, silent when offline). `refuse_private_addresses` stops an agent
  following a link to `169.254.169.254`. It is **off by default** on purpose, because fetching
  `http://localhost` is an ordinary thing to ask a local-first tool to do, so turn it on for an
  installation where an agent chooses its own URLs.
- **robots.txt** is reported by default and can be made binding with `robots=obey`.
- **No Svipall telemetry or periodic update polling.** The explicit `svipall update --check` and
  `svipall update --install` commands contact GitHub release metadata only when you run them.
  Browsing can contact page resources, redirects, challenge endpoints and other origins a page uses, and optional
  DNS-over-HTTPS contacts the configured resolver. Results go to your connected client, and what
  happens to them there depends on that client. Startup can download Chrome for Testing when no browser
  is installed and browser tiers are enabled; explicit `browser install` / `browser_setup` also
  contacts the release metadata and download servers. Set `browser_auto_install=false` to turn off
  automatic provisioning. Svipall fetches and caches the configured blocklists
  (StevenBlack/hosts and EasyPrivacy by default) when `block_ads=true`. A launched browser may also make its own
  requests, and Svipall is not a network firewall. See [native exposure](#automatic-routing-privacy-and-practical-limits).
- **No breaking of access controls.** Svipall evades bot detection on public pages and does not crack
  passwords, bypass paywalls or forge authentication. A login wall is passed by *you*, once, in a
  visible window, and the cookies are kept.

---
