//! Four walls a browser nobody is driving gets through, and svipall did not.
//!
//! Each was measured on a live site first and is rebuilt here as the smallest page that fails the
//! same way, so the fix is checked against the mechanism and not against somebody's hostname:
//!
//! * an interstitial that verifies the visitor by itself and **watches its own answer field**.
//!   Asking the page for that field, from the page's own world, is what it treats as a script;
//! * an interstitial whose widget stops and waits for **a tick in its box**, inside a frame from
//!   another origin and behind a closed shadow root;
//! * a proof-of-work page whose widget does the sum itself once its box is ticked;
//! * the same proof of work on a page where no widget ever ran, so the sum is done here.
//!
//! The site is a server of its own rather than `support::Site`: it has to issue a challenge,
//! remember it, check what comes back and only then serve the page.

mod support;

use base64::Engine;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use svipall::server::SvipallServer;
use svipall::tools::SolveAndContinueParams;
use svipall_core::pow::{hex, Hash, Signed};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct Req {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl Req {
    fn cookie(&self, name: &str) -> bool {
        self.headers
            .get("cookie")
            .is_some_and(|c| c.split(';').any(|p| p.trim() == format!("{name}=1")))
    }
}

struct Resp {
    status: u16,
    content_type: &'static str,
    body: String,
    headers: Vec<(String, String)>,
}

impl Resp {
    fn html(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "text/html; charset=utf-8",
            body,
            headers: Vec::new(),
        }
    }
    fn json(body: Value) -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            body: body.to_string(),
            headers: Vec::new(),
        }
    }
    fn with(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// What the site saw, for the assertions a returned page cannot make.
#[derive(Default)]
struct Seen {
    /// Times the watched interstitial caught a look at its answer field.
    tripped: AtomicUsize,
    /// Proofs the server checked and accepted, by the route they arrived on.
    accepted: Mutex<Vec<&'static str>>,
    /// Proofs it refused, with why.
    refused: Mutex<Vec<String>>,
}

struct Wall {
    port: u16,
    seen: Arc<Seen>,
}

const SECRET: &str = "fixture-secret";

/// The page behind every wall: a title, and enough prose to be a page rather than a stub.
fn manual() -> Resp {
    Resp::html(
        200,
        format!(
            "<!doctype html><html><head><title>The manual</title></head><body><h1>The manual</h1>{}</body></html>",
            "<p>Turn the mode dial to the green square, half-press the shutter button to focus, \
             and press it fully to take the picture. The card slot is behind the door on the \
             grip side of the body.</p>"
                .repeat(3)
        ),
    )
}

fn sign(what: &str) -> String {
    hex(&Hash::Sha256.digest(&[SECRET.as_bytes(), what.as_bytes()]))
}

/// A digest-form challenge whose answer is `secret`.
fn digest_challenge(secret: u64) -> Value {
    let salt = "0f3a9c5e7b1d?expires=1794466351&";
    let challenge = hex(&Hash::Sha256.digest(&[salt.as_bytes(), secret.to_string().as_bytes()]));
    json!({"algorithm": "SHA-256", "challenge": challenge, "maxnumber": 5000, "salt": salt,
           "signature": sign(&challenge)})
}

/// A derived-form challenge whose answer is `secret`, in the shape one search engine serves.
fn derived_challenge(secret: u64) -> Value {
    let mut parameters = json!({
        "algorithm": "PBKDF2/SHA-256", "cost": 200, "keyLength": 32, "keyPrefix": "00",
        "nonce": "45a27178a346b6b15426944219577c42", "salt": "c7d0fd8cf85dc0b86d357306aaff3b0b",
        "expiresAt": 1794466351u64,
    });
    let key = Signed::parse(&json!({"parameters": parameters, "signature": ""}).to_string())
        .expect("a challenge")
        .key_for(secret)
        .expect("derived");
    parameters["keyPrefix"] = json!(hex(&key[..16]));
    let signature = sign(&parameters.to_string());
    json!({"parameters": parameters, "signature": signature})
}

/// The server's side of the proof: its own signature on what it issued, and the counter.
fn check(payload_b64: &str) -> Result<(), String> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(payload_b64.trim())
        .map_err(|e| format!("not base64: {e}"))?;
    let p: Value = serde_json::from_slice(&raw).map_err(|e| format!("not json: {e}"))?;
    if let Some(challenge) = p.get("challenge").filter(|c| c.is_object()) {
        if challenge["signature"] != json!(sign(&challenge["parameters"].to_string())) {
            return Err("the parameters are not the ones that were signed".into());
        }
        let c = Signed::parse(&challenge.to_string()).ok_or("not a challenge")?;
        let n = p["solution"]["counter"].as_u64().ok_or("no counter")?;
        if !c.accepts(n) {
            return Err(format!("counter {n} does not derive the key"));
        }
        let key = hex(&c.key_for(n).expect("derived"));
        if p["solution"]["derivedKey"] != json!(key) {
            return Err("the derived key is not the counter's".into());
        }
        return Ok(());
    }
    let challenge = p["challenge"].as_str().ok_or("no challenge")?;
    if p["signature"] != json!(sign(challenge)) {
        return Err("the challenge is not one that was signed".into());
    }
    let c = Signed::parse(&p.to_string()).ok_or("not a challenge")?;
    let n = p["number"].as_u64().ok_or("no number")?;
    // The payload carries no bound, so the server applies the one it issued.
    if n > 5000 || !c.accepts(n) {
        return Err(format!("number {n} does not hash to the challenge"));
    }
    Ok(())
}

/// The field a form carried, whether it came as `multipart/form-data` or urlencoded.
fn field(req: &Req, name: &str) -> Option<String> {
    let body = String::from_utf8_lossy(&req.body);
    if let Some(at) = body.find(&format!("name=\"{name}\"")) {
        let rest = &body[at..];
        let start = rest.find("\r\n\r\n")? + 4;
        let end = rest[start..].find("\r\n")? + start;
        return Some(rest[start..end].to_string());
    }
    url::form_urlencoded::parse(body.as_bytes())
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

const WATCHED: &str = r#"<!doctype html><html><head><title>Just a moment...</title></head><body>
<h1>Performing security verification</h1>
<p>This website uses a security service to protect against malicious bots.</p>
<form><input type="hidden" name="cf-turnstile-response" id="answer" value=""></form>
<script>(() => {
  // What the live page does, reduced to the part that mattered: the lookups a document offers
  // are wrapped, so a question about the answer field is seen by the page being asked.
  let tripped = false;
  for (const proto of [Document.prototype, Element.prototype]) {
    for (const fn of ['querySelector', 'querySelectorAll']) {
      const real = proto[fn];
      proto[fn] = function (sel) {
        if (String(sel).includes('turnstile-response')) tripped = true;
        return real.call(this, sel);
      };
    }
  }
  setTimeout(() => {
    if (tripped) { fetch('/watched/tripped', { method: 'POST' }).finally(() => location.reload()); return; }
    document.cookie = 'watched=1; path=/';
    location.reload();
  }, 2500);
})();</script></body></html>"#;

/// The host page keeps its widget where the live one does: a frame from another origin, inside a
/// shadow root the document cannot open.
const TICK: &str = r#"<!doctype html><html><head><title>Just a moment...</title></head><body>
<h1>Performing security verification</h1>
<p>This website uses a security service to protect against malicious bots.</p>
<div id="host" style="margin:40px 0 0 60px"></div>
<script>(() => {
  const root = document.getElementById('host').attachShadow({ mode: 'closed' });
  const frame = document.createElement('iframe');
  frame.src = 'http://localhost:PORT/challenges.cloudflare.com/turnstile/frame';
  frame.width = 300; frame.height = 65; frame.style.border = '0';
  root.appendChild(frame);
  addEventListener('message', (e) => {
    if (e.origin !== 'http://localhost:PORT' || e.data !== 'ticked') return;
    document.cookie = 'ticked=1; path=/';
    location.reload();
  });
})();</script></body></html>"#;

/// The box is 24 px square, 16 px in and centred: where the live frame draws its own. Only a
/// click the browser itself delivered counts.
const TICK_FRAME: &str = r#"<!doctype html><html><body style="margin:0;font:14px sans-serif">
<input type="checkbox" id="box" style="position:absolute;left:16px;top:20px;width:24px;height:24px;margin:0">
<span style="position:absolute;left:52px;top:23px">Verify you are human</span>
<script>document.getElementById('box').addEventListener('click', (e) => {
  if (e.isTrusted) parent.postMessage('ticked', '*');
});</script></body></html>"#;

/// A proof-of-work page whose widget works: tick the box and it fetches, counts, fills its field
/// and says so, and the page's own script posts the form and reloads.
const POW: &str = r#"<!doctype html><html><head><title>Captcha</title></head><body>
<h1>Verification required</h1><p>Please complete the challenge to continue.</p>
<form id="pow-form" method="post" action="/pow/verify">
<altcha-widget id="altcha-widget" challenge="/pow/challenge" name="altcha"></altcha-widget>
<div id="note">Waiting for verification.</div></form>
<script>
customElements.define('altcha-widget', class extends HTMLElement {
  connectedCallback() {
    this.innerHTML = '<div class="altcha" data-state="unverified"><input type="checkbox" id="pow-box" required>'
      + '<label for="pow-box">I\'m not a robot</label><input type="hidden" name="altcha" value=""></div>';
    this.querySelector('#pow-box').addEventListener('click', async (e) => {
      if (!e.isTrusted) return;
      this.firstChild.dataset.state = 'verifying';
      const ch = await (await fetch(this.getAttribute('challenge'))).json();
      const enc = new TextEncoder();
      let n = 0;
      for (; n <= ch.maxnumber; n++) {
        const d = new Uint8Array(await crypto.subtle.digest('SHA-256', enc.encode(ch.salt + n)));
        if ([...d].map((b) => b.toString(16).padStart(2, '0')).join('') === ch.challenge) break;
      }
      this.querySelector('input[name=altcha]').value = btoa(JSON.stringify({
        algorithm: ch.algorithm, challenge: ch.challenge, number: n, salt: ch.salt, signature: ch.signature }));
      this.firstChild.dataset.state = 'verified';
      this.dispatchEvent(new CustomEvent('verified'));
    });
  }
});
document.getElementById('altcha-widget').addEventListener('verified', () => {
  const form = document.getElementById('pow-form');
  const xhr = new XMLHttpRequest();
  xhr.open('POST', form.action, true);
  xhr.setRequestHeader('X-Requested-With', 'XMLHttpRequest');
  xhr.onload = () => { if (xhr.status === 200 && JSON.parse(xhr.responseText).verified) location.reload(); };
  xhr.send(new FormData(form));
});
</script></body></html>"#;

/// The same wall where the widget's script never arrived: the element is there, nothing runs.
const POW_BARE: &str = r#"<!doctype html><html><head><title>Captcha</title></head><body>
<h1>Verification required</h1><p>Please complete the challenge to continue.</p>
<form method="post" action="/bare/verify">
<altcha-widget challengeurl="/bare/challenge" name="altcha"></altcha-widget>
</form></body></html>"#;

/// An interstitial that verifies the visitor by itself and then reloads into whatever the site
/// answers next. Built like the live one, with its options object in the markup, a timed reload
/// and a line for the reader without scripts, and worded as the live one is today.
const CLEARING: &str = r#"<!doctype html><html><head><title>Just a moment...</title>
<meta name="robots" content="noindex,nofollow"><meta http-equiv="refresh" content="360"></head><body>
<h1>127.0.0.1</h1><h2>Performing security verification</h2>
<p>This website uses a security service to protect against malicious bots. This page is
displayed while the website verifies you are not a bot.</p>
<noscript><span id="challenge-error-text">Enable JavaScript and cookies to continue</span></noscript>
<script>window._cf_chl_opt = {cRay: 'fixture'};
setTimeout(() => { document.cookie = 'COOKIE=1; path=/'; location.reload(); }, AFTER);</script>
</body></html>"#;

fn clearing(cookie: &str, after_ms: u32) -> Resp {
    Resp::html(
        403,
        CLEARING
            .replace("COOKIE", cookie)
            .replace("AFTER", &after_ms.to_string()),
    )
}

/// A page that really is this short.
const TINY: &str = "<!doctype html><html><head><title>Tiny</title></head><body><h1>Tiny</h1>\
                    <p>You got past the challenge.</p></body></html>";

/// What a web server says when it will not serve a client, with nothing else on the page.
const STUB: &str = "<html><head><title>403 Forbidden</title></head><body><center>\
                    <h1>403 Forbidden</h1></center><hr><center>nginx</center></body></html>";

/// A page that prints what the browser says it is, in the document and in a worker.
const WHO: &str = r#"<!doctype html><html><head><title>Who is asking</title></head><body>
<h1>Who is asking</h1>
<p>This page writes down the version the browser reading it claims to be, where a script reads
it and where a worker does, so that the two can be compared with the browser that was started.</p>
<p id="document">document: pending</p><p id="hints">hints: pending</p><p id="worker">worker: pending</p>
<script>
const major = (ua) => (/Chrome\/(\d+)/.exec(ua) || [0, '?'])[1];
document.getElementById('document').textContent = 'document says major ' + major(navigator.userAgent);
const brands = navigator.userAgentData ? navigator.userAgentData.brands : [];
const chromium = brands.find((b) => b.brand === 'Chromium');
document.getElementById('hints').textContent = 'hints say major ' + (chromium ? chromium.version : '?');
const w = new Worker(URL.createObjectURL(new Blob(['postMessage(navigator.userAgent)'])));
w.onmessage = (e) => { document.getElementById('worker').textContent = 'worker says major ' + major(e.data); };
</script></body></html>"#;

impl Wall {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let seen = Arc::new(Seen::default());
        let state = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    continue;
                };
                let seen = state.clone();
                tokio::spawn(async move {
                    let Some(req) = read(&mut sock).await else {
                        return;
                    };
                    if req.path.starts_with("/hang") {
                        tokio::time::sleep(Duration::from_secs(8)).await;
                    }
                    let r = route(&req, port, &seen);
                    let mut head = format!(
                        "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n",
                        r.status,
                        r.content_type,
                        r.body.len()
                    );
                    for (k, v) in &r.headers {
                        head.push_str(&format!("{k}: {v}\r\n"));
                    }
                    head.push_str("\r\n");
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(r.body.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Self { port, seen }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

async fn read(sock: &mut tokio::net::TcpStream) -> Option<Req> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let (method, target) = (first.next()?.to_string(), first.next()?.to_string());
    // Asked as a proxy, the request line carries the whole address. The site is the same one
    // whatever name it was reached by, and a name that is not loopback is what lets a fetch
    // climb the ladder the way it does on the open web.
    let path = match target.strip_prefix("http://") {
        Some(rest) => rest
            .find('/')
            .map_or("/".to_string(), |i| rest[i..].to_string()),
        None => target,
    };
    let headers: HashMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let want = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    while buf.len() < head_end + want {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Some(Req {
        method,
        path,
        headers,
        body: buf[head_end..].to_vec(),
    })
}

fn route(req: &Req, port: u16, seen: &Seen) -> Resp {
    let path = req.path.split('?').next().unwrap_or("");
    let verdict = |seen: &Seen, route: &'static str, payload: Option<String>| -> bool {
        match payload
            .ok_or("no payload".to_string())
            .and_then(|p| check(&p))
        {
            Ok(()) => {
                seen.accepted.lock().unwrap().push(route);
                true
            }
            Err(why) => {
                seen.refused.lock().unwrap().push(format!("{route}: {why}"));
                false
            }
        }
    };
    match (req.method.as_str(), path) {
        ("GET", "/who") => Resp::html(200, WHO.into()),
        ("GET", "/") | ("GET", "/plain") => manual(),
        ("GET", "/tiny") if req.cookie("tiny") => Resp::html(200, TINY.into()),
        ("GET", "/tiny") => clearing("tiny", 2500),
        ("GET", "/slow") if req.cookie("slow") => manual(),
        ("GET", "/slow") => clearing("slow", 5000),
        ("GET", "/refused") if req.cookie("refused") => Resp::html(403, STUB.into()),
        ("GET", "/refused") => clearing("refused", 2500),
        ("GET", "/watched") if req.cookie("watched") => manual(),
        ("GET", "/watched") => Resp::html(403, WATCHED.into()),
        ("POST", "/watched/tripped") => {
            seen.tripped.fetch_add(1, Ordering::SeqCst);
            Resp::json(json!({}))
        }
        ("GET", "/tick") if req.cookie("ticked") => manual(),
        ("GET", "/tick") => Resp::html(403, TICK.replace("PORT", &port.to_string())),
        // The same wall in front of a page that takes eight seconds to finish loading: all of
        // its text is there at once, and one picture is not.
        ("GET", "/tick-heavy") if req.cookie("heavy") => {
            let mut page = manual();
            page.body = page
                .body
                .replace("</body>", "<img src=\"/hang\" alt=\"\"></body>");
            page
        }
        ("GET", "/tick-heavy") => Resp::html(
            403,
            TICK.replace("PORT", &port.to_string())
                .replace("ticked=1", "heavy=1"),
        ),
        ("GET", "/hang") => Resp::html(200, String::new()),
        // And in front of a page that shows a widget of its own, beside a form nobody is
        // filling in: the wall is gone, and a widget is still on the page asking.
        ("GET", "/tick-widget") if req.cookie("widget") => {
            let mut page = manual();
            page.body = page.body.replace(
                "</body>",
                &format!(
                    "<form><iframe src=\"http://localhost:{port}/challenges.cloudflare.com/\
                     turnstile/frame\" width=\"300\" height=\"65\"></iframe></form></body>"
                ),
            );
            page
        }
        ("GET", "/tick-widget") => Resp::html(
            403,
            TICK.replace("PORT", &port.to_string())
                .replace("ticked=1", "widget=1"),
        ),
        ("GET", "/challenges.cloudflare.com/turnstile/frame") => Resp::html(200, TICK_FRAME.into()),
        ("GET", "/pow") if req.cookie("pow") => manual(),
        ("GET", "/pow") => Resp::html(403, POW.into()),
        ("GET", "/pow/challenge") => Resp::json(digest_challenge(1234)),
        ("POST", "/pow/verify") => {
            // The page's own script posts it; a form submitted any other way is not this route.
            let ok = req.headers.contains_key("x-requested-with")
                && verdict(seen, "the page's own script", field(req, "altcha"));
            let r = Resp::json(json!({"ok": ok, "verified": ok}));
            if ok {
                r.with("Set-Cookie", "pow=1; Path=/")
            } else {
                r
            }
        }
        ("GET", "/bare") if req.cookie("bare") => manual(),
        ("GET", "/bare") => Resp::html(403, POW_BARE.into()),
        ("GET", "/bare/challenge") => Resp::json(derived_challenge(41)),
        ("POST", "/bare/verify") => {
            if verdict(seen, "the form", field(req, "altcha")) {
                let mut r = Resp::html(303, String::new());
                r = r
                    .with("Location", "/bare")
                    .with("Set-Cookie", "bare=1; Path=/");
                r
            } else {
                Resp::html(
                    403,
                    "<title>Captcha</title><h1>Verification failed</h1>".into(),
                )
            }
        }
        _ => Resp::html(404, "<title>Not found</title>".into()),
    }
}

async fn solve(site: &Wall, path: &str) -> (Value, Duration) {
    // A wall that does not clear is this test failing, not a reason to open a window on the
    // desk of whoever is running it and wait three minutes for them.
    std::env::set_var("SVIPALL_HUMAN_ASSIST", "0");
    let s = SvipallServer::new(None, svipall_core::Config::default(), None);
    let started = Instant::now();
    let out = s
        .solve_and_continue_json(SolveAndContinueParams {
            url: site.url(path),
            profile: None,
            extraction: None,
            timeout_s: Some(45),
            max_tokens: Some(400),
        })
        .await
        .expect("the tool answers");
    eprintln!("{path}: {:.1}s {out}", started.elapsed().as_secs_f32());
    (out, started.elapsed())
}

fn assert_the_manual(out: &Value) {
    assert_eq!(out["title"], "The manual", "{out}");
    assert_eq!(out["still_blocked"], false, "{out}");
    assert!(
        out["content"].as_str().unwrap_or("").contains("mode dial"),
        "{out}"
    );
}

#[tokio::test]
#[ignore = "needs a real browser"]
async fn an_interstitial_that_watches_its_answer_field_is_waited_out_unseen() {
    support::isolate();
    let site = Wall::start().await;
    let (out, took) = solve(&site, "/watched").await;
    assert_eq!(
        site.seen.tripped.load(Ordering::SeqCst),
        0,
        "the page saw its answer field being asked for: {out}"
    );
    assert_the_manual(&out);
    // The page lets a visitor through after two and a half seconds. Sitting out the whole
    // timeout on a wall that has already gone is a minute and a half of nothing, per call.
    assert!(took < Duration::from_secs(25), "took {took:?}");
}

#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_widget_waiting_on_its_box_gets_the_box_ticked() {
    support::isolate();
    let site = Wall::start().await;
    let (out, _) = solve(&site, "/tick").await;
    assert_the_manual(&out);
}

#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_proof_of_work_widget_is_left_to_do_its_own_sum() {
    support::isolate();
    let site = Wall::start().await;
    let (out, _) = solve(&site, "/pow").await;
    let refused = site.seen.refused.lock().unwrap().clone();
    assert!(
        refused.is_empty(),
        "the server refused a proof: {refused:?}"
    );
    assert_eq!(
        *site.seen.accepted.lock().unwrap(),
        vec!["the page's own script"],
        "{out}"
    );
    assert_the_manual(&out);
}

#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_proof_of_work_with_no_widget_running_is_computed_here() {
    support::isolate();
    let site = Wall::start().await;
    let (out, _) = solve(&site, "/bare").await;
    let refused = site.seen.refused.lock().unwrap().clone();
    assert!(
        refused.is_empty(),
        "the server refused a proof: {refused:?}"
    );
    assert_eq!(
        *site.seen.accepted.lock().unwrap(),
        vec!["the form"],
        "{out}"
    );
    assert_the_manual(&out);
}

#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_browser_tier_says_it_is_the_browser_it_is() {
    // The interstitial lets a browser through when what it claims and what it is agree. The
    // browser tiers used to claim the newest version the http tier can forge a handshake for,
    // whatever was installed: five majors off on this machine, and the wall never cleared.
    support::isolate();
    let site = Wall::start().await;
    let cfg = svipall_core::Config::default();
    let installed = svipall::browser::BrowserPool::new(cfg.clone())
        .browser_major()
        .expect("a browser whose version can be read");
    let s = SvipallServer::new(None, cfg, None);
    let out = s
        .fetch_json(svipall::tools::WebFetchParams {
            url: site.url("/who"),
            mode: Some("stealth".into()),
            cache: Some("bypass".into()),
            ..Default::default()
        })
        .await
        .value;
    let content = out["content"].as_str().unwrap_or("").to_string();
    eprintln!("installed {installed}: {content}");
    for realm in ["document says", "hints say", "worker says"] {
        assert!(
            content.contains(&format!("{realm} major {installed}")),
            "{realm} something other than Chrome {installed}: {content}"
        );
    }
}

/// One fetch. With a tier named, of the loopback address. Without one, of a name that is not
/// loopback, reached through the fixture as its proxy: a loopback fetch never leaves the http
/// tier, and these tests are about what the tiers above it do.
async fn fetch(site: &Wall, path: &str, mode: Option<&str>) -> (Value, Duration) {
    let s = SvipallServer::new(
        None,
        svipall_core::Config {
            request_min_interval_ms: 1,
            http_engine: "reqwest".into(),
            block_ads: false,
            ..Default::default()
        },
        None,
    );
    let (url, proxy) = match mode {
        Some(_) => (site.url(path), None),
        None => (
            format!("http://behind{}.test{path}", path.replace('/', "-")),
            Some(site.url("")),
        ),
    };
    let started = Instant::now();
    let out = s
        .fetch_json(svipall::tools::WebFetchParams {
            url,
            proxy,
            mode: mode.map(str::to_string),
            cache: Some("bypass".into()),
            robots: Some("ignore".into()),
            ..Default::default()
        })
        .await
        .value;
    s.shutdown_configuration().await;
    eprintln!("{path}: {:.1}s {out}", started.elapsed().as_secs_f32());
    (out, started.elapsed())
}

#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_tier_looking_at_an_interstitial_gives_it_the_time_it_needs() {
    // The headful tier read the page a second after it loaded and reported the interstitial it
    // was still showing. Every such site cost one failed attempt before the tier that waits.
    support::isolate();
    let site = Wall::start().await;
    let (out, _) = fetch(&site, "/slow", Some("real")).await;
    assert_eq!(out["title"], "The manual", "{out}");
    assert_eq!(out["tier_used"], "real", "{out}");
    assert!(out["blocked_reason"].is_null(), "{out}");
    assert_eq!(out["challenge_passed"], true, "{out}");
}

#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_page_that_is_no_challenge_is_not_waited_on() {
    support::isolate();
    let site = Wall::start().await;
    let (out, took) = fetch(&site, "/plain", Some("real")).await;
    assert_eq!(out["title"], "The manual", "{out}");
    assert!(out["warm"].is_null(), "a wait was spent on a page: {out}");
    assert!(out["challenge_passed"].is_null(), "{out}");
    // A browser start, a load and the settle after it. The wait a challenge gets is twelve more.
    assert!(took < Duration::from_secs(12), "took {took:?}");
}

#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_page_with_a_widget_of_its_own_is_not_taken_for_the_wall_it_replaced() {
    // Measured on a demo page: its interstitial was ticked and gave way to a page that shows
    // widgets of its own, and the tick went on waiting twenty-five seconds for "the widget" to
    // stop asking. The one it ticked was gone with the document it was on.
    support::isolate();
    let site = Wall::start().await;
    let (out, _) = fetch(&site, "/tick-widget", Some("real")).await;
    assert_eq!(out["title"], "The manual", "{out}");
    let secs = out["warm"]["secs"].as_f64().unwrap_or(f64::MAX);
    assert!(secs < 12.0, "the wait took {secs}s: {out}");
}

#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_ticked_wall_that_is_gone_is_not_waited_on_for_a_form_it_never_had() {
    // Measured on a crates index: the box was ticked, the wall was gone four seconds later, and
    // the fetch then spent twelve more waiting for the page behind it to finish loading, in
    // case a form had been sent. Nothing had: a wall that goes away has no form.
    support::isolate();
    let site = Wall::start().await;
    let (out, _) = fetch(&site, "/tick-heavy", Some("real")).await;
    assert_eq!(out["title"], "The manual", "{out}");
    let secs = out["warm"]["secs"].as_f64().unwrap_or(f64::MAX);
    // Six seconds for the widget to have had its chance, the tick, and the look after it.
    assert!(secs < 10.0, "the wait took {secs}s: {out}");
}

#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_short_page_behind_a_passed_challenge_is_the_page() {
    // Measured on a public test page: the challenge cleared, the page behind it was 78
    // characters, and because the http tier had been refused first it was reported as "the
    // refusal again, not the page".
    support::isolate();
    let site = Wall::start().await;
    let (out, _) = fetch(&site, "/tiny", None).await;
    assert!(out["blocked_reason"].is_null(), "{out}");
    assert_eq!(out["title"], "Tiny", "{out}");
    assert_eq!(out["status"], 200, "{out}");
    assert!(
        out["content"]
            .as_str()
            .unwrap_or("")
            .contains("You got past the challenge"),
        "{out}"
    );
    assert_eq!(out["challenge_passed"], true, "{out}");
}

fn assert_the_origin_refused(out: &Value) {
    assert_eq!(out["wall_kind"], "origin", "{out}");
    assert_eq!(out["challenge_passed"], true, "{out}");
    let reason = out["blocked_reason"].as_str().unwrap_or("");
    assert!(reason.starts_with("origin refusal"), "{out}");
    assert!(reason.contains("403"), "{out}");
    let note = out["note"].as_str().unwrap_or("");
    assert!(
        note.contains("web_route") && note.contains("web_login"),
        "{out}"
    );
    assert!(
        !note.contains("solve_and_continue(url) answers"),
        "there is no challenge left to answer: {out}"
    );
    // Nothing is withheld: what the server said is still handed over.
    assert!(
        out["content"]
            .as_str()
            .unwrap_or("")
            .contains("403 Forbidden"),
        "{out}"
    );
}

#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_refusal_behind_a_passed_challenge_ends_the_solve_and_says_so() {
    // Measured on a manuals site: the interstitial cleared, the site's own server answered a
    // bare 403, and the solver waited out its whole timeout on a page with nothing to solve.
    support::isolate();
    let site = Wall::start().await;
    let (out, took) = solve(&site, "/refused").await;
    assert_eq!(out["solved"], false, "{out}");
    assert_eq!(out["still_blocked"], true, "{out}");
    assert_the_origin_refused(&out);
    assert!(took < Duration::from_secs(25), "took {took:?}");
}

#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_refusal_behind_a_passed_challenge_ends_the_fetch_and_says_so() {
    support::isolate();
    let site = Wall::start().await;
    let (out, _) = fetch(&site, "/refused", None).await;
    assert_the_origin_refused(&out);
    assert_eq!(out["status"], 403, "{out}");
    let attempts = out["attempts"].as_array().cloned().unwrap_or_default();
    // The tier that passed the challenge is the last one asked: no tier above it would be
    // answered differently by the same server.
    assert!(
        !attempts
            .iter()
            .any(|a| a.as_str().unwrap_or("").starts_with("warm")
                || a.as_str().unwrap_or("").starts_with("native")),
        "{out}"
    );
}

/// The fixture server is the judge in the tests above, so its own judgement is checked without a
/// browser: a right answer is taken, and a wrong or unsigned one is not.
#[test]
fn the_fixture_server_checks_the_proof_it_is_handed() {
    let b64 = |v: Value| base64::engine::general_purpose::STANDARD.encode(v.to_string());

    let d = digest_challenge(1234);
    let payload = |n: u64| {
        b64(
            json!({"algorithm": d["algorithm"], "challenge": d["challenge"], "number": n,
                   "salt": d["salt"], "signature": d["signature"]}),
        )
    };
    assert_eq!(check(&payload(1234)), Ok(()));
    assert!(check(&payload(1233)).is_err());

    let c = derived_challenge(41);
    let key = |n: u64| hex(&Signed::parse(&c.to_string()).unwrap().key_for(n).unwrap());
    let payload = |n: u64, c: &Value| {
        b64(json!({"challenge": c, "solution": {"counter": n, "derivedKey": key(n), "time": 3}}))
    };
    assert_eq!(check(&payload(41, &c)), Ok(()));
    assert!(check(&payload(40, &c)).is_err());
    // An easier challenge than the one issued, signed by nobody.
    let mut forged = c.clone();
    forged["parameters"]["cost"] = json!(1);
    assert!(check(&payload(41, &forged)).is_err());
}
