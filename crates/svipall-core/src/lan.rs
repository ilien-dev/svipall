//! The address a phone can reach the dashboard on.
//!
//! Half the challenges a person is asked to finish are easier on a phone than on the machine
//! running the crawl: tapping four tiles with a thumb beats dragging a mouse across a laptop. The
//! panel already works there — every coordinate it sends is a fraction of the picture rather than a
//! pixel — but nobody can open it, because the only address ever printed is `localhost`, which on a
//! phone means the phone.
//!
//! So: find this machine's address on the local network, and print it beside the loopback one.
//!
//! Only when the server is actually reachable there. Binding to `127.0.0.1` and advertising a LAN
//! address prints a link that cannot work, which is worse than printing nothing: it is a minute of
//! somebody's time and a wrong conclusion about the network.

use std::net::{IpAddr, UdpSocket};

/// This machine's address on the local network.
///
/// No packet is sent and no name is looked up: connecting a UDP socket only fixes which interface
/// the kernel would use, and the address it picks is the answer. A machine with no route out has
/// none, which is a real answer too.
pub fn local_ipv4() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // Documentation-range address: routable enough to pick an interface, and nothing is ever sent
    // to it.
    socket.connect("192.0.2.1:9").ok()?;
    let addr = socket.local_addr().ok()?.ip();
    (!addr.is_loopback() && !addr.is_unspecified()).then_some(addr)
}

/// Is this bind address reachable from another machine?
pub fn reachable_off_box(bind: &str) -> bool {
    match bind.trim().parse::<IpAddr>() {
        Ok(ip) => !ip.is_loopback(),
        // A name rather than an address: assume the operator meant it.
        Err(_) => !bind.eq_ignore_ascii_case("localhost") && !bind.trim().is_empty(),
    }
}

/// The URL to hand to a phone, if there is one worth handing over.
///
/// `None` when the server is bound to loopback — see the module note. It is not an oversight that
/// this is quiet: the operator chose that bind, and the fix is a config change, not a warning on
/// every start.
pub fn dashboard_url(bind: &str, ip: Option<IpAddr>, port: u16, token: &str) -> Option<String> {
    if !reachable_off_box(bind) {
        return None;
    }
    let ip = ip?;
    Some(format!("http://{ip}:{port}/human?t={token}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Option<IpAddr> {
        s.parse().ok()
    }

    #[test]
    fn a_server_on_loopback_advertises_no_lan_address_at_all() {
        // Printing one would be a link that cannot work, which costs somebody a minute and teaches
        // them something false about their network.
        assert_eq!(
            dashboard_url("127.0.0.1", ip("192.168.1.40"), 8787, "tok"),
            None
        );
        assert_eq!(
            dashboard_url("localhost", ip("192.168.1.40"), 8787, "tok"),
            None
        );
    }

    #[test]
    fn a_server_listening_everywhere_advertises_the_address_a_phone_can_reach() {
        assert_eq!(
            dashboard_url("0.0.0.0", ip("192.168.1.40"), 8787, "tok").as_deref(),
            Some("http://192.168.1.40:8787/human?t=tok")
        );
    }

    #[test]
    fn the_token_travels_with_it_because_the_page_is_useless_without_one() {
        let url = dashboard_url("0.0.0.0", ip("10.0.0.2"), 9000, "abc123").expect("advertised");
        assert!(url.ends_with("?t=abc123"), "{url}");
    }

    #[test]
    fn a_machine_with_no_address_on_the_network_advertises_nothing() {
        assert_eq!(dashboard_url("0.0.0.0", None, 8787, "tok"), None);
    }

    #[test]
    fn a_bind_that_is_not_loopback_counts_as_reachable() {
        assert!(reachable_off_box("0.0.0.0"));
        assert!(reachable_off_box("192.168.1.40"));
        assert!(!reachable_off_box("127.0.0.1"));
        assert!(!reachable_off_box("::1"));
        assert!(!reachable_off_box(""));
    }

    #[test]
    fn asking_this_machine_for_its_address_never_panics_and_never_returns_loopback() {
        // Runs on a build machine with no network as readily as on a laptop; either answer is fine
        // as long as it is not a lie.
        if let Some(addr) = local_ipv4() {
            assert!(!addr.is_loopback(), "{addr}");
        }
    }
}
