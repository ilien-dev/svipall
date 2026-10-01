//! A supervised tunnel is started, found answering, restarted when it dies, and marked down and
//! up in the exit ledger as it goes.
//!
//! The tunnel is this test binary itself, run as `socks_fixture`: a SOCKS5 greeting responder
//! that lives a few seconds and exits. No `sh`, no `ssh`, so it runs the same on every platform.
mod support;
use std::time::{Duration, Instant};
use svipall_core::config::Tunnel;

const PORT_VAR: &str = "SVIPALL_SOCKS_FIXTURE_PORT";
const LIFE_VAR: &str = "SVIPALL_SOCKS_FIXTURE_SECS";

/// Not a test: the child process the supervisor starts. Does nothing unless the variables say so.
#[tokio::test]
#[ignore = "the supervised child in `a_tunnel_is_started_and_restarted_when_it_dies`"]
async fn socks_fixture() {
    let Some(port) = std::env::var(PORT_VAR)
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
    else {
        return;
    };
    let life: u64 = std::env::var(LIFE_VAR)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap();
    let serve = async {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                continue;
            };
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut hello = [0u8; 3];
                if s.read_exact(&mut hello).await.is_ok() && hello[0] == 5 {
                    let _ = s.write_all(&[5, 0]).await;
                }
            });
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(life), serve).await;
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn row() -> serde_json::Value {
    svipall::tunnels::status()[0].clone()
}

#[tokio::test]
async fn a_tunnel_is_started_and_restarted_when_it_dies() {
    support::isolate();
    let port = free_port();
    std::env::set_var(PORT_VAR, port.to_string());
    std::env::set_var(LIFE_VAR, "4");
    let exe = std::env::current_exe().unwrap();
    let t = Tunnel {
        name: "fixture".into(),
        command: vec![
            exe.display().to_string(),
            "socks_fixture".into(),
            "--exact".into(),
            "--ignored".into(),
            "--quiet".into(),
        ],
        socks_port: port,
        country: String::new(),
    };
    let exit = t.exit();
    assert!(
        svipall::tunnels::probe(port).await.is_err(),
        "nothing answers before the supervisor starts it"
    );
    svipall::tunnels::start(std::slice::from_ref(&t));
    assert!(svipall::tunnels::is_tunnel(&exit));

    let deadline = Instant::now() + Duration::from_secs(40);
    let (mut first_up, mut seen_down_after_up) = (false, false);
    loop {
        let r = row();
        let up = r["up"].as_bool().unwrap_or(false);
        if up && !first_up {
            first_up = true;
            assert!(
                !svipall_core::exits::is_down(&exit),
                "an answering tunnel is not passed over: {r}"
            );
            assert!(r["pid"].is_u64(), "it is ours: {r}");
        }
        if first_up && !up {
            seen_down_after_up = true;
            assert!(
                svipall_core::exits::is_down(&exit),
                "a dead tunnel is passed over: {r}"
            );
        }
        if seen_down_after_up && up && r["restarts"].as_u64() >= Some(1) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "not restarted in time (up once: {first_up}, down after: {seen_down_after_up}): {r}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    svipall::tunnels::stop_all().await;
}
