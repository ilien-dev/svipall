//! What an error says when a call was wrong, so the next call is right.
//!
//! A model that reads `task not found` retries the same id; one that reads where ids come from
//! makes the right call instead. Every message here names the tool or the parameter that fixes
//! the situation, and nothing else. Kept together so the wording is tested once and the tools
//! only quote it.

/// The tiers a caller may name, in ladder order.
pub const TIERS: &[&str] = &["http", "browser", "stealth", "real", "warm"];

/// `mode` or `max_tier` named something that is not a tier.
pub fn not_a_tier(field: &str, value: &str) -> String {
    format!(
        "{field} {value:?} is not a tier. Omit {field} so auto picks one, or use {}.",
        TIERS.join(", ")
    )
}

/// A URL that none of the fetchers can take.
pub fn not_a_url(url: &str) -> String {
    format!("{url:?} is not a URL. Give http(s)://…, file:///… under ~/.svipall/in, or raw:<html>.")
}

/// A tool that opens a page in a browser was given something that is not an http(s) address.
pub fn needs_http_url(tool: &str, url: &str) -> String {
    format!("{tool} opens the page in a browser, so it needs an http(s) URL, not {url:?}.")
}

/// A `taskId` nobody handed out, or one from before the server restarted.
pub fn unknown_task(task_id: &str) -> String {
    format!(
        "no task {task_id:?}. A taskId comes from solve_turnstile, solve_recaptcha_v2, \
         solve_hcaptcha or solve_image_captcha in this server's lifetime; to read a blocked page \
         use solve_and_continue instead."
    )
}

/// A `session_id` that is not open.
pub fn unknown_session(session_id: &str) -> String {
    format!(
        "no open session {session_id:?}. browser_open returns one, and it ends with browser_close \
         or when the server restarts; web_status lists the sessions open now."
    )
}

/// A widget solve with nothing to solve.
pub fn widget_request_error(sitekey: &str, page_url: &str) -> Option<String> {
    let sitekey = sitekey.trim();
    if sitekey.is_empty() {
        return Some(
            "sitekey is empty. It is the widget's data-sitekey attribute on the page; with only \
             the page URL, use solve_and_continue, which reads it there."
                .into(),
        );
    }
    if sitekey.contains(char::is_whitespace) {
        return Some(format!(
            "sitekey {sitekey:?} contains whitespace; a site key is one token from data-sitekey."
        ));
    }
    if !(page_url.starts_with("http://") || page_url.starts_with("https://")) {
        return Some(needs_http_url("a widget solve", page_url));
    }
    None
}

/// A proxy that no tier could use.
pub fn not_a_proxy(proxy: &str) -> Option<String> {
    let ok = ["http://", "https://", "socks5://", "socks5h://"]
        .iter()
        .any(|s| proxy.starts_with(s))
        && url::Url::parse(proxy).is_ok_and(|u| u.host_str().is_some());
    (!ok).then(|| {
        format!(
            "{proxy:?} is not a proxy URL. Use socks5h://user:pass@host:port (socks5h resolves \
             DNS at the exit), socks5://, http:// or https://."
        )
    })
}

/// A `cursor` that no earlier result produced.
pub const CURSOR_IGNORED: &str =
    "cursor was not one from an earlier result; starting from the top of the page";

/// `web_search` with nothing to search for.
pub const EMPTY_QUERY: &str = "query is empty; nothing was asked of any engine";

/// `web_watch remove` on a page nobody was watching.
pub const NO_SUCH_WATCH: &str = "no watch on that url; web_watch list shows the ones there are";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_names_the_way_out() {
        assert!(not_a_tier("mode", "bogus").contains("Omit mode"));
        assert!(not_a_tier("max_tier", "x").contains("warm"));
        assert!(not_a_url("x").contains("raw:<html>"));
        assert!(unknown_task("t").contains("solve_and_continue"));
        assert!(unknown_session("s").contains("browser_open"));
        assert!(unknown_session("s").contains("web_status"));
        assert!(NO_SUCH_WATCH.contains("web_watch list"));
    }

    #[test]
    fn a_widget_solve_is_refused_before_it_is_queued() {
        assert!(widget_request_error("", "https://x.example")
            .is_some_and(|m| m.contains("solve_and_continue")));
        assert!(widget_request_error("a b", "https://x.example").is_some());
        assert!(widget_request_error("0x4AAA", "x.example").is_some_and(|m| m.contains("http")));
        assert!(widget_request_error("0x4AAA", "https://x.example").is_none());
    }

    #[test]
    fn a_proxy_is_a_url_with_a_scheme_a_tier_can_use() {
        assert!(not_a_proxy("not a url").is_some_and(|m| m.contains("socks5h")));
        assert!(not_a_proxy("ftp://h:1").is_some());
        assert!(not_a_proxy("socks5h://u:p@10.0.0.5:1080").is_none());
        assert!(not_a_proxy("http://proxy.example:3128").is_none());
    }
}
