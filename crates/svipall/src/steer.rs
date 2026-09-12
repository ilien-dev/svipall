//! What svipall says back when a call was wrong or a page did not arrive, so the next call is right.
//!
//! A model that reads `task not found` retries the same id; one that reads where ids come from
//! makes the right call instead. Every message here names the tool or the parameter that fixes
//! the situation, and nothing else. Kept together so the wording is tested once and the tools
//! only quote it.

use svipall_core::WallKind;

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
///
/// A bare path is called out first because it is the mistake the markdown itself invites: links to
/// the page's own site are written the way the page wrote them, `/wiki/Web_crawler`, and joining
/// that to the response's `url` is the whole fix.
pub fn not_a_url(url: &str) -> String {
    if url.starts_with('/') {
        return format!(
            "{url:?} is a path, not a URL. Links to a page's own site come back as the page wrote \
             them; join it to that response's `url` — https://host{url} — and fetch that."
        );
    }
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

/// The next step when a page came back from behind a captcha.
///
/// Written here rather than at the call site because it is the most-read sentence svipall
/// produces, and because it arrives with the arguments already filled in, at the exact moment of
/// the decision — which beats any general instruction the model was given earlier in the session.
/// It used to name `solve_turnstile` and nothing else, whose answer is a bare token bound to the
/// session and address that produced it: the wrong branch whenever the goal is the content, which
/// is nearly every time anybody fetches a page.
///
/// The page's own address is deliberately not repeated here: it is already the `url` of the same
/// response, and a `raw:` fetch would otherwise print the whole document inside the advice about
/// it — twice. Only the sitekey, which the caller does not otherwise have, is spelled out.
pub fn captcha_next_step(widget: &str, token_tool: &str, sitekey: &str) -> String {
    format!(
        "The page exposes a {widget} widget: solve_and_continue(url) answers it in place and \
         returns the page behind it. {token_tool}(sitekey={sitekey:?}, pageUrl=url) returns a \
         bare token instead, and is right only for a form you post yourself."
    )
}

/// The same step when this installation has a solver but no widget could be read off the page.
pub fn captcha_solver_available(dashboard: &str) -> String {
    format!(
        "solve_and_continue(url) answers a challenge in place and returns the page behind it; a \
         person can also answer one at {dashboard}."
    )
}

/// Human-readable next step for a wall that survived the ladder.
///
/// Here rather than in `server.rs` for the reason this module exists: it is a message the model
/// reads and acts on, so it is worth testing as prose, and the tools and parameters it names have
/// to be real ones. Kept next to the captcha wording it composes with.
pub fn wall_next_step(
    kind: &WallKind,
    domain: &str,
    tier: &str,
    solver: bool,
    dashboard: &str,
) -> String {
    // The domain used to be spelled out here. It is the host of the `url` in the same response,
    // so it said nothing the reader did not have, and on a `raw:` page it rendered as "the
    // profile" with a hole in the middle of it.
    let login_hint = "web_login(url) opens a visible window: pass the check once by hand and the \
                      profile keeps the cookies for later real/warm fetches.";
    match kind {
        WallKind::Hold | WallKind::Generic | WallKind::Cloudflare => {
            let mut s = format!("Challenge still present at tier {}. {} Or route the domain through a proxy with web_route.", tier, login_hint);
            if solver {
                s.push(' ');
                s.push_str(&captcha_solver_available(dashboard));
            }
            s
        }
        WallKind::Vendor => format!("Browser-fingerprinting wall (DataDome / PerimeterX / Incapsula) at tier {}. {} A residential proxy via web_route usually helps too.", tier, login_hint),
        WallKind::Login => "Login wall. Use web_login(url, profile=NAME) to sign in once, then pass profile=NAME to web_fetch / web_act.".to_string(),
        WallKind::Gate => "Geo or consent gate instead of the page. Use web_act to dismiss it (click the accept/continue button) or web_route to change the exit country.".to_string(),
        WallKind::Empty => format!("Page did not render text at tier {}. Try web_act with a wait action, or a css_selector for the region you need.", tier),
        WallKind::NotFound => "The URL does not exist (404/410). Check the address; escalating tiers cannot help.".to_string(),
        WallKind::SoftNotFound => "The page answered 200 but says it does not exist. Check the address; escalating tiers cannot help, and the stub is not the page you asked for.".to_string(),
        WallKind::Paywall => format!("The article exists and is being withheld behind a subscription. {login_hint} Only a profile that is signed in changes the answer; a proxy does not."),
        WallKind::Status => format!("Hard HTTP block at tier {}; domain is on a 15 min cooldown. Use web_route with a proxy, or web_status(clear_cooldown=\"{}\") to retry sooner.", tier, domain),
        WallKind::None => String::new(),
    }
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
        // The markdown hands the model paths now, so the message that fires when it passes one
        // back has to name the join rather than repeat the list of accepted schemes.
        let path = not_a_url("/wiki/Web_crawler");
        assert!(path.contains("join"), "{path}");
        assert!(path.contains("`url`"), "{path}");
        assert!(unknown_task("t").contains("solve_and_continue"));
        // The note on a blocked page offers the tool that ends the problem before the one that
        // returns a token, and never the token tool without the condition that makes it right.
        let note = captcha_next_step("Turnstile", "solve_turnstile", "0x4AAA");
        assert!(
            !note.contains("x.test"),
            "the note repeats the page it is about: {note}"
        );
        let (ends, token) = (
            note.find("solve_and_continue")
                .expect("names the page tool"),
            note.find("solve_turnstile").expect("names the token tool"),
        );
        assert!(ends < token, "{note}");
        assert!(note.contains("post yourself"), "{note}");
        assert!(captcha_solver_available("http://d").contains("solve_and_continue"));
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
