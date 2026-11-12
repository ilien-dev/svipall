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
///
/// `cooldown` is the hold this outcome really put on the domain, in seconds, or `None` when it put
/// none: the note never claims a cooldown that does not exist, or one of another length.
pub fn wall_next_step(
    kind: &WallKind,
    domain: &str,
    tier: &str,
    solver: bool,
    dashboard: &str,
    cooldown: Option<u64>,
) -> String {
    let mut s = wall_advice(kind, tier, solver, dashboard);
    if let Some(secs) = cooldown.filter(|s| *s > 0) {
        s.push_str(&format!(
            " The domain is on a {} cooldown for every session on this machine; \
             web_status(clear_cooldown=\"{domain}\") retries sooner.",
            duration(secs)
        ));
    }
    s
}

/// `solve_and_continue` ran and the wall is still there. The general wall advice offers
/// solve_and_continue whenever a solver exists, which here is the call that just failed: measured
/// live on two managed challenges, the note sent the caller straight back into the same check.
pub fn solve_did_not_clear(kind: &WallKind, domain: &str, solver: bool, dashboard: &str) -> String {
    // The challenge did clear. What is left is the site saying no, and that is all there is to
    // say: neither another call nor a person at the dashboard has anything to answer.
    if *kind == WallKind::Origin {
        return wall_next_step(kind, domain, "warm", false, dashboard, None);
    }
    let mut s = format!(
        "the challenge did not clear. {}",
        wall_next_step(kind, domain, "warm", false, dashboard, None)
    );
    s.push_str(" Calling solve_and_continue again meets the same check");
    if solver {
        s.push_str(&format!("; a person can answer it at {dashboard}"));
    }
    s.push('.');
    s
}

fn wall_advice(kind: &WallKind, tier: &str, solver: bool, dashboard: &str) -> String {
    // The domain used to be spelled out here. It is the host of the `url` in the same response,
    // so it said nothing the reader did not have, and on a `raw:` page it rendered as "the
    // profile" with a hole in the middle of it.
    let login_hint = "web_login(url) opens a visible window: pass the check once by hand and the \
                      profile keeps the cookies for later real/warm fetches.";
    match kind {
        // A page nobody recognised is not known to be a check a person can pass, so it gets no
        // login advice; a widget on it is named by the captcha step appended to this one.
        WallKind::Generic => format!(
            "Blocked at tier {tier} by a page svipall does not recognise; what it says is in \
             `content`. Read it, or route the domain through another exit with web_route."
        ),
        WallKind::Hold | WallKind::Cloudflare => {
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
        WallKind::Status => format!("Hard HTTP block at tier {tier}. Use web_route with a proxy."),
        WallKind::Origin => format!(
            "The challenge was passed at tier {tier}, and the site's own server then refused: \
             it will not serve this client or this address. That is not a challenge to solve, \
             so no higher tier and no solver changes it. Route the domain through another exit \
             with web_route, or sign in once with web_login(url) if the site serves members only."
        ),
        WallKind::None => String::new(),
    }
}

/// `90s`, `15 min`, `2 h`.
pub fn duration(secs: u64) -> String {
    if secs < 120 {
        format!("{secs}s")
    } else if secs < 7200 {
        format!("{} min", secs.div_ceil(60))
    } else {
        format!("{} h", secs.div_ceil(3600))
    }
}

/// Said wherever the limiter speaks: a caller that does not know the budget is shared reads a
/// refusal caused by another session as its own fault.
pub const SHARED: &str = "shared by every session on this machine";

/// The longest a caller may ask to wait, in seconds.
pub const MAX_WAIT_SECS: u64 = 900;

/// Nothing was requested, because the limiter or a hold said no for `secs` more seconds.
pub fn limiter_refusal(kind: &str, secs: u64, domain: &str, limit: u32, window: u64) -> String {
    let wait = wait_hint(secs);
    match kind {
        "visit_limit" => format!(
            "Nothing was requested: the visit budget for this site ({limit} visits per {window}s, \
             per site and exit) is {SHARED}, and it is used up. The next slot frees in {secs}s; \
             {wait}."
        ),
        _ => format!(
            "Nothing was requested: {domain} is on hold for {} more, {SHARED}. To wait, {wait}; \
             web_status(clear_cooldown=\"{domain}\") drops the hold.",
            duration(secs),
        ),
    }
}

fn wait_hint(secs: u64) -> String {
    if secs <= MAX_WAIT_SECS {
        format!("call again with wait={secs}")
    } else {
        format!(
            "call again with wait={MAX_WAIT_SECS}, which covers {} of it",
            duration(MAX_WAIT_SECS)
        )
    }
}

/// The fetch waited for the limiter or a hold before it went out.
pub fn limiter_delay(kind: &str, secs: u64) -> String {
    match kind {
        "visit_limit" => {
            format!("waited {secs}s for a free slot: the visit budget for this site is {SHARED}")
        }
        _ => format!("waited {secs}s for a hold on this site to end; holds are {SHARED}"),
    }
}

/// A 429 or a 503 answered, and the site is now held for `secs`.
pub fn server_backoff(status: u16, secs: u64, named: bool, domain: &str) -> String {
    let why = match (status, named) {
        (_, true) => format!("The site answered {status} and asked for {secs}s"),
        (503, false) => {
            format!("The site answered 503, a server error rather than a ban, so it rests {secs}s")
        }
        _ => format!(
            "The site answered {status} (too many requests), so it rests {}; a repeat rests longer",
            duration(secs)
        ),
    };
    format!(
        "{why}. No request goes to it until then from any session on this machine. To wait it \
         out, {}; web_status(clear_cooldown=\"{domain}\") drops the hold.",
        wait_hint(secs)
    )
}

/// The answer came from the memory of a walk up the whole ladder that failed moments ago.
pub fn remembered_failure(ago: u64, expires_in: u64) -> String {
    format!(
        "Not requested: {ago}s ago every tier up to the top was refused by this site, and that is \
         remembered for {} more. web_fetch(url, fresh=true) walks the ladder anyway and, if \
         it gets through, forgets it.",
        duration(expires_in)
    )
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
    fn a_wall_note_states_only_what_is_true_of_that_outcome() {
        let d = "http://d";
        let none = wall_next_step(&WallKind::Status, "x.test", "http", false, d, None);
        assert!(!none.contains("cooldown"), "{none}");
        let set = wall_next_step(&WallKind::Status, "x.test", "warm", false, d, Some(900));
        assert!(set.contains("15 min cooldown"), "{set}");
        assert!(set.contains("clear_cooldown=\"x.test\""), "{set}");
        // An unrecognised page is not known to be a check a person can pass.
        let generic = wall_next_step(&WallKind::Generic, "x.test", "warm", true, d, None);
        assert!(!generic.contains("web_login"), "{generic}");
        assert!(
            wall_next_step(&WallKind::Cloudflare, "x", "warm", false, d, None)
                .contains("web_login")
        );
        // A refusal behind a passed challenge leaves nothing to solve, and the note says so
        // instead of sending the caller back into a check that is no longer there.
        let origin = solve_did_not_clear(&WallKind::Origin, "x.test", true, d);
        assert!(
            origin.contains("challenge was passed") && origin.contains("web_route"),
            "{origin}"
        );
        assert!(
            !origin.contains("did not clear") && !origin.contains(d),
            "{origin}"
        );
        assert_eq!(duration(40), "40s");
        assert_eq!(duration(900), "15 min");
        let refusal = limiter_refusal("visit_limit", 37, "x.test", 12, 60);
        assert!(
            refusal.contains(SHARED) && refusal.contains("wait=37"),
            "{refusal}"
        );
        let long = limiter_refusal("cooldown", 3600, "x.test", 12, 60);
        assert!(
            long.contains("wait=900") && long.contains("clear_cooldown"),
            "{long}"
        );
        assert!(server_backoff(503, 30, false, "x.test").contains("server error"));
    }

    #[test]
    fn a_solve_that_did_not_clear_does_not_send_the_caller_back_to_it() {
        let note = solve_did_not_clear(&WallKind::Cloudflare, "x.test", true, "http://d/human");
        assert!(!note.contains("solve_and_continue(url) answers"), "{note}");
        assert!(note.contains("again meets the same check"), "{note}");
        assert!(note.contains("http://d/human"), "{note}");
        assert!(note.contains("web_login"), "{note}");
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
