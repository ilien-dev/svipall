//! Chrome's HTTP/3 SETTINGS frame, measured rather than assumed.
//!
//! `docs/http3.md` has carried a row reading *"HTTP/3 SETTINGS — not compared against Chrome's.
//! Its contents and order are a fingerprint exactly as HTTP/2's are"* since the QUIC engine was
//! built. The reason it stayed open is that a SETTINGS frame travels on a control stream at 1-RTT,
//! so unlike the ClientHello it cannot be read out of a datagram nobody answered: something has to
//! complete a handshake with Chrome first.
//!
//! So this runs a QUIC server. A certificate for the name `quic.test` is generated in this process
//! and deleted when the run ends (`quiche::selfsigned`), Chrome is told to resolve that name to a
//! socket this process owns and to force QUIC to it, and once the handshake is up the peer's
//! control stream carries the frame. `peer_settings_raw` hands it back in the order it arrived,
//! which is the half of the fingerprint a decoded struct would throw away.
//!
//! Nothing here asserts. It prints what Chrome sent, the way `fingerprint` prints what an endpoint
//! reported; the assertion built on top of it is
//! `crates/svipall-quic/tests/settings.rs`, which is offline and runs in `qc`.

use anyhow::{anyhow, Context, Result};
use std::net::{SocketAddr, UdpSocket};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// The largest datagram we will read, matching the engine's own ceiling.
const MAX_DATAGRAM: usize = 1500;

/// How long Chrome gets to start, resolve, handshake and open its control stream. A local
/// handshake is milliseconds; this is the budget for the browser starting up cold.
const DEADLINE: Duration = Duration::from_secs(30);

/// A server that answers exactly one connection, for exactly as long as it takes to hear a
/// SETTINGS frame. Nothing about it is a production QUIC server and it must never become one.
///
/// The certificate is made in this process and deleted when `ca` drops, so nothing about this
/// measurement leaves a key on disk or in the repository.
fn server_config(ca: &quiche::selfsigned::SelfSigned) -> Result<quiche::Config> {
    let mut c = quiche::Config::new(quiche::PROTOCOL_VERSION)?;
    c.load_cert_chain_from_pem_file(
        ca.cert_pem
            .to_str()
            .ok_or_else(|| anyhow!("the certificate path is not UTF-8"))?,
    )?;
    c.load_priv_key_from_pem_file(
        ca.key_pem
            .to_str()
            .ok_or_else(|| anyhow!("the key path is not UTF-8"))?,
    )?;
    c.set_application_protos(&[b"h3"])?;
    c.set_max_idle_timeout(20_000);
    c.set_max_recv_udp_payload_size(MAX_DATAGRAM);
    c.set_max_send_udp_payload_size(MAX_DATAGRAM);
    c.set_initial_max_data(10_000_000);
    c.set_initial_max_stream_data_bidi_local(1_000_000);
    c.set_initial_max_stream_data_bidi_remote(1_000_000);
    c.set_initial_max_stream_data_uni(1_000_000);
    c.set_initial_max_streams_bidi(100);
    c.set_initial_max_streams_uni(100);
    Ok(c)
}

/// Chrome, pointed at our socket and told to speak QUIC to it.
///
/// Three flags carry the whole trick. `--host-resolver-rules` sends the name here without touching
/// DNS, `--origin-to-force-quic-on` stops Chrome from trying TCP first (it has no `Alt-Svc` for a
/// name it has never seen, and its own rule is never to open a first connection over QUIC), and the
/// SPKI pin accepts this one certificate rather than turning certificate checking off.
fn launch_chrome(port: u16, ca: &quiche::selfsigned::SelfSigned) -> Result<Child> {
    let exe = svipall_mcp::browser::managed_browser().ok_or_else(|| {
        anyhow!("no managed Chrome for Testing under ~/.svipall/browser/cft; run a browser-tier fetch once to provision one")
    })?;
    // The pin is computed from the certificate we just made, so the two can never drift — which
    // is the failure a committed pin beside a committed certificate invites.
    let pin = ca.spki_pin();
    let profile = std::env::temp_dir().join(format!("svipall-h3ref-{port}"));
    let _ = std::fs::remove_dir_all(&profile);

    let child = Command::new(&exe)
        .arg("--headless=new")
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-background-networking")
        .arg("--enable-quic")
        .arg(format!("--origin-to-force-quic-on=quic.test:{port}"))
        .arg(format!(
            "--host-resolver-rules=MAP quic.test 127.0.0.1:{port}"
        ))
        .arg(format!("--ignore-certificate-errors-spki-list={pin}"))
        .arg(format!("https://quic.test:{port}/"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("launching {}", exe.display()))?;
    Ok(child)
}

/// What one measurement saw.
pub struct Reference {
    /// `(id, value)` in the order the frame carried them.
    pub settings: Vec<(u64, u64)>,
    /// The ALPN that was negotiated, so a run that quietly fell back is visible.
    pub alpn: String,
}

/// Drive one connection until the peer's SETTINGS arrive, or the deadline passes.
fn capture(
    socket: &UdpSocket,
    local: SocketAddr,
    ca: &quiche::selfsigned::SelfSigned,
) -> Result<Reference> {
    socket.set_read_timeout(Some(Duration::from_millis(100)))?;
    let mut config = server_config(ca)?;
    let h3_config = quiche::h3::Config::new()?;

    let mut buf = [0u8; 65535];
    let mut out = [0u8; MAX_DATAGRAM];
    let mut conn: Option<quiche::Connection> = None;
    let mut h3: Option<quiche::h3::Connection> = None;
    let started = Instant::now();

    while started.elapsed() < DEADLINE {
        // Read whatever arrived, then always give the connection a chance to write: a handshake
        // stalls forever if the only thing that ever pumps `send` is an inbound datagram.
        match socket.recv_from(&mut buf) {
            Ok((len, from)) => {
                let hdr = match quiche::Header::from_slice(&mut buf[..len], quiche::MAX_CONN_ID_LEN)
                {
                    Ok(h) => h,
                    // A datagram we cannot parse is not ours to explain; a browser sends a few.
                    Err(_) => continue,
                };
                if conn.is_none() {
                    if hdr.ty != quiche::Type::Initial {
                        continue;
                    }
                    let mut scid = [0u8; quiche::MAX_CONN_ID_LEN];
                    getrandom(&mut scid);
                    let scid = quiche::ConnectionId::from_ref(&scid);
                    conn = Some(quiche::accept(&scid, None, local, from, &mut config)?);
                }
                let c = conn.as_mut().expect("a connection was just made");
                let info = quiche::RecvInfo { from, to: local };
                // A datagram the connection rejects is a fact about that datagram, not the run.
                let _ = c.recv(&mut buf[..len], info);
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(e.into()),
        }

        let Some(c) = conn.as_mut() else { continue };

        if c.is_established() && h3.is_none() {
            h3 = Some(quiche::h3::Connection::with_transport(c, &h3_config)?);
        }
        if let Some(h) = h3.as_mut() {
            // Poll until it has nothing left to say. The control stream is read here, and
            // `peer_settings_raw` is populated as a side effect of reading it.
            loop {
                match h.poll(c) {
                    Ok(_) => continue,
                    Err(quiche::h3::Error::Done) => break,
                    // A transport error ends the run; anything else is this connection being a
                    // browser (a stream reset, a request we will not answer) and is not fatal.
                    Err(quiche::h3::Error::TransportError(e)) => return Err(e.into()),
                    Err(_) => break,
                }
            }
            if let Some(raw) = h.peer_settings_raw() {
                let settings = raw.to_vec();
                let alpn = String::from_utf8_lossy(c.application_proto()).into_owned();
                return Ok(Reference { settings, alpn });
            }
        }

        loop {
            match c.send(&mut out) {
                Ok((written, info)) => {
                    socket.send_to(&out[..written], info.to)?;
                }
                Err(quiche::Error::Done) => break,
                Err(e) => return Err(e.into()),
            }
        }
        if c.is_closed() {
            return Err(anyhow!(
                "the connection closed before a SETTINGS frame arrived"
            ));
        }
    }
    Err(anyhow!(
        "no SETTINGS frame in {}s; Chrome may have declined the certificate or the port",
        DEADLINE.as_secs()
    ))
}

/// Fill a buffer with randomness, without adding a dependency for sixteen bytes.
fn getrandom(buf: &mut [u8]) {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut at = 0;
    while at < buf.len() {
        let mut h = RandomState::new().build_hasher();
        h.write_usize(at);
        for b in h.finish().to_le_bytes() {
            if at < buf.len() {
                buf[at] = b;
                at += 1;
            }
        }
    }
}

/// The names the HTTP/3 registry gives the identifiers Chrome is known to send, so the printed
/// table reads as something rather than as a column of hex.
fn name_of(id: u64) -> &'static str {
    match id {
        0x01 => "QPACK_MAX_TABLE_CAPACITY",
        0x06 => "MAX_FIELD_SECTION_SIZE",
        0x07 => "QPACK_BLOCKED_STREAMS",
        0x08 => "ENABLE_CONNECT_PROTOCOL",
        0x0c => "ENABLE_WEBTRANSPORT (draft)",
        0x33 => "H3_DATAGRAM",
        0x276 => "H3_DATAGRAM (draft 00)",
        0x2b60 => "ENABLE_WEBTRANSPORT (Google)",
        _ if is_grease(id) => "GREASE (reserved)",
        _ => "unregistered",
    }
}

/// RFC 9114 section 7.2.4.1: reserved identifiers are `0x1f * N + 0x21`.
pub fn is_grease(id: u64) -> bool {
    id >= 0x21 && (id - 0x21).is_multiple_of(0x1f)
}

pub fn run() -> Result<()> {
    let socket = UdpSocket::bind("127.0.0.1:0").context("binding a loopback UDP socket")?;
    let local = socket.local_addr()?;
    let port = local.port();
    println!("serving QUIC on {local}, pointing Chrome at https://quic.test:{port}/\n");

    let ca = quiche::selfsigned::generate("quic.test")?;
    let mut child = launch_chrome(port, &ca)?;
    let captured = capture(&socket, local, &ca);
    let _ = child.kill();
    let _ = child.wait();
    let r = captured?;

    println!("ALPN negotiated: {}\n", r.alpn);
    println!("HTTP/3 SETTINGS, in the order Chrome sent them:\n");
    println!("id        name                          value");
    for (id, value) in &r.settings {
        println!("0x{:<6x}  {:<28}  {}", id, name_of(*id), value);
    }
    println!("\n{} settings", r.settings.len());

    println!(
        "\n{}",
        serde_json::to_string(&serde_json::json!({
            "alpn": r.alpn,
            "settings": r.settings.iter().map(|(i, v)| serde_json::json!([i, v])).collect::<Vec<_>>(),
        }))?
    );
    Ok(())
}
