//! Local exits svipall starts itself, and keeps alive.
//!
//! An `ssh -D` tunnel or a Tor daemon was always usable as a proxy URL, but it was the operator's
//! to start and nobody noticed when it died: the pool kept choosing it until every fetch through it
//! had failed. A `[[tunnels]]` entry hands the process to svipall. It is started with the server,
//! probed with a SOCKS5 greeting (which reaches no site, so it costs no standing anywhere and says
//! nothing about this machine to anyone), restarted with a backoff when it exits or stops
//! answering, and marked down and up in `exits` as it goes, so the ladder passes a dead one over.
//!
//! One supervisor per process, started by whichever long-lived server comes up first. The CLI
//! never starts one: a tunnel that lived for one fetch would cost its whole start-up on every call.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};
use svipall_core::config::Tunnel;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::Notify;

/// How long a started tunnel may take to answer before it counts as failed. `ssh` negotiating a
/// key exchange and Tor building its first circuit both fit comfortably.
const START_GRACE: Duration = Duration::from_secs(45);
/// Probe period while every tunnel answers. A loopback handshake costs next to nothing, and a
/// dead one found in between by the ladder wakes the supervisor at once anyway.
const IDLE_TICK: Duration = Duration::from_secs(10);
/// Probe period while any tunnel is starting or down.
const BUSY_TICK: Duration = Duration::from_secs(2);
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

struct State {
    tunnel: Tunnel,
    child: Option<Child>,
    /// When the current child was started, for the start-up grace.
    started: Option<Instant>,
    up: bool,
    /// Answering on its port without being ours: started by hand before svipall was.
    adopted: bool,
    /// Processes started so far; every one after the first is a restart.
    launches: u32,
    last_error: Option<String>,
    backoff: Duration,
    next_start: Instant,
}

static STATE: LazyLock<Mutex<HashMap<String, State>>> = LazyLock::new(Default::default);
static WAKE: LazyLock<Notify> = LazyLock::new(Notify::new);
static STARTED: OnceLock<()> = OnceLock::new();

/// Start supervising `tunnels`. Only the first call in a process does anything, so both servers
/// can call it without starting every tunnel twice.
pub fn start(tunnels: &[Tunnel]) {
    if tunnels.is_empty() || STARTED.set(()).is_err() {
        return;
    }
    {
        let mut state = STATE.lock().unwrap();
        for t in tunnels {
            if !t.country.trim().is_empty() {
                svipall_core::store::set_proxy_region(&t.exit(), &t.country);
            }
            state.insert(
                t.name.clone(),
                State {
                    tunnel: t.clone(),
                    child: None,
                    started: None,
                    up: false,
                    adopted: false,
                    launches: 0,
                    last_error: None,
                    backoff: Duration::from_secs(1),
                    next_start: Instant::now(),
                },
            );
        }
    }
    tokio::spawn(async {
        loop {
            let busy = tick().await;
            let wait = if busy { BUSY_TICK } else { IDLE_TICK };
            tokio::select! {
                _ = WAKE.notified() => {}
                _ = tokio::time::sleep(wait) => {}
            }
        }
    });
}

/// Look again now: the ladder found an exit it could not reach.
pub fn wake() {
    WAKE.notify_one();
}

/// Is `proxy` one of the supervised tunnels?
pub fn is_tunnel(proxy: &str) -> bool {
    STATE
        .lock()
        .unwrap()
        .values()
        .any(|s| s.tunnel.exit() == proxy)
}

/// Stop every tunnel this process started. Adopted ones are left alone: they were not ours.
pub async fn stop_all() {
    let children: Vec<Child> = STATE
        .lock()
        .unwrap()
        .values_mut()
        .filter_map(|s| s.child.take())
        .collect();
    for mut c in children {
        let _ = c.kill().await;
    }
}

/// Each tunnel's state, for `web_status`.
pub fn status() -> Value {
    let state = STATE.lock().unwrap();
    let mut rows: Vec<Value> = state
        .values()
        .map(|s| {
            json!({
                "name": s.tunnel.name,
                "exit": s.tunnel.exit(),
                "up": s.up,
                "pid": s.child.as_ref().and_then(|c| c.id()),
                "adopted": s.adopted,
                "restarts": s.launches.saturating_sub(1),
                "last_error": s.last_error,
            })
        })
        .collect();
    rows.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    json!(rows)
}

/// One pass over every tunnel. True while any of them is not up, so the next pass comes soon.
async fn tick() -> bool {
    let names: Vec<String> = STATE.lock().unwrap().keys().cloned().collect();
    let mut busy = false;
    for name in names {
        busy |= !check(&name).await;
    }
    busy
}

/// Bring one tunnel to the state it should be in. True when it answers.
async fn check(name: &str) -> bool {
    let Some((port, exit)) = STATE
        .lock()
        .unwrap()
        .get(name)
        .map(|s| (s.tunnel.socks_port, s.tunnel.exit()))
    else {
        return true;
    };
    let answers = probe(port).await;

    let mut state = STATE.lock().unwrap();
    let Some(s) = state.get_mut(name) else {
        return true;
    };
    // Did our process die?
    if let Some(child) = s.child.as_mut() {
        if let Ok(Some(code)) = child.try_wait() {
            s.child = None;
            s.started = None;
            s.last_error = Some(format!("exited ({code})"));
            s.next_start = Instant::now() + s.backoff;
            s.backoff = (s.backoff * 2).min(MAX_BACKOFF);
        }
    }
    match answers {
        Ok(()) => {
            s.adopted = s.child.is_none();
            s.backoff = Duration::from_secs(1);
            s.started = None;
            if !s.up {
                s.up = true;
                s.last_error = None;
                svipall_core::exits::mark_up(&exit);
                tracing::info!(tunnel = name, "tunnel is up");
            }
            return true;
        }
        Err(e) => {
            if s.up {
                tracing::warn!(tunnel = name, "tunnel stopped answering: {e}");
            }
            s.up = false;
            s.adopted = false;
            if s.last_error.is_none() || s.child.is_some() {
                s.last_error = Some(e.clone());
            }
            svipall_core::exits::mark_down(&exit, &format!("tunnel {name}: {e}"));
            // It answered once and has gone quiet with its process still running: give it the
            // same grace as a fresh start, then replace it.
            if s.child.is_some() && s.started.is_none() {
                s.started = Some(Instant::now());
            }
        }
    }
    // Started, alive, and still silent past its grace: it is hung, so it goes.
    if let (Some(child), Some(at)) = (s.child.as_mut(), s.started) {
        if at.elapsed() > START_GRACE {
            let _ = child.start_kill();
            s.child = None;
            s.started = None;
            s.last_error = Some(format!("did not answer within {}s", START_GRACE.as_secs()));
            s.next_start = Instant::now() + s.backoff;
            s.backoff = (s.backoff * 2).min(MAX_BACKOFF);
        } else {
            return false;
        }
    }
    if s.child.is_none() && Instant::now() >= s.next_start {
        match spawn(&s.tunnel) {
            Ok(child) => {
                s.launches += 1;
                s.child = Some(child);
                s.started = Some(Instant::now());
            }
            Err(e) => {
                s.last_error = Some(format!("cannot start {}: {e}", s.tunnel.command[0]));
                s.next_start = Instant::now() + s.backoff;
                s.backoff = (s.backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
    false
}

fn spawn(t: &Tunnel) -> std::io::Result<Child> {
    let secrets = crate::secrets::load();
    let argv: Vec<String> = t
        .command
        .iter()
        .map(|a| crate::secrets::expand(a, &secrets))
        .collect();
    Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
}

/// A SOCKS5 greeting offering no authentication, and the method the server picks. Any SOCKS5
/// server answers it without being asked to connect anywhere.
pub async fn probe(port: u16) -> Result<(), String> {
    let go = async {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .map_err(|e| format!("nothing listening on port {port} ({e})"))?;
        s.write_all(&[5, 1, 0]).await.map_err(|e| e.to_string())?;
        let mut reply = [0u8; 2];
        s.read_exact(&mut reply)
            .await
            .map_err(|e| format!("no SOCKS5 answer on port {port} ({e})"))?;
        match reply {
            [5, 0] => Ok(()),
            [5, 0xff] => Err(format!(
                "port {port} is a SOCKS5 server that wants credentials"
            )),
            other => Err(format!("port {port} did not answer as SOCKS5 ({other:?})")),
        }
    };
    tokio::time::timeout(PROBE_TIMEOUT, go)
        .await
        .unwrap_or_else(|_| Err(format!("port {port} did not answer in time")))
}
