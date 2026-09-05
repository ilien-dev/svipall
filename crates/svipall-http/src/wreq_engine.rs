//! The engine that actually looks like a browser.
//!
//! `wreq` drives BoringSSL with Chrome's cipher list, extension order, GREASE values and HTTP/2
//! SETTINGS. The emulation profile also decides the platform, so it is selected from the same
//! `IdentityProfile` everything else reads — otherwise the default profile advertises macOS, which
//! on a Windows host contradicts every other signal the browser tiers emit.

use crate::{FetcherConfig, HttpFetcher, HttpRequest, HttpResponse};
use svipall_core::identity::Engine;
use svipall_core::{IdentityProfile, Os};
use wreq_util::{Emulation, Platform, Profile};

pub struct WreqFetcher {
    client: wreq::Client,
    identity: IdentityProfile,
}

/// The emulation profile for an identity: Chrome or Firefox, at or below the advertised major.
fn profile_of(id: &IdentityProfile) -> Profile {
    match id.engine {
        Engine::Chrome => profile_for(id.chrome_major),
        Engine::Firefox => firefox_profile_for(id.chrome_major),
    }
}

/// Closest Firefox emulation profile at or below the advertised Gecko major. `wreq-util` 0.2
/// ships Firefox 109 through 149; a request above that lands on the newest it has.
fn firefox_profile_for(major: u16) -> Profile {
    match major {
        0..=116 => Profile::Firefox109,
        117..=127 => Profile::Firefox117,
        128..=132 => Profile::Firefox128,
        133..=134 => Profile::Firefox133,
        135 => Profile::Firefox135,
        136..=138 => Profile::Firefox136,
        139..=141 => Profile::Firefox139,
        142 => Profile::Firefox142,
        143 => Profile::Firefox143,
        144 => Profile::Firefox144,
        145 => Profile::Firefox145,
        146 => Profile::Firefox146,
        147 => Profile::Firefox147,
        148 => Profile::Firefox148,
        _ => Profile::Firefox149,
    }
}

/// Closest Chrome emulation profile at or below the advertised major.
///
/// `wreq-util` ships specific versions, not a continuum, so a request for 145 has to land on the
/// newest profile that does not overstate what the handshake will look like.
fn profile_for(major: u16) -> Profile {
    match major {
        0..=100 => Profile::Chrome100,
        101..=103 => Profile::Chrome101,
        104 => Profile::Chrome104,
        105 => Profile::Chrome105,
        106 => Profile::Chrome106,
        107 => Profile::Chrome107,
        108 => Profile::Chrome108,
        109..=113 => Profile::Chrome109,
        114..=115 => Profile::Chrome114,
        116 => Profile::Chrome116,
        117 => Profile::Chrome117,
        118 => Profile::Chrome118,
        119 => Profile::Chrome119,
        120..=122 => Profile::Chrome120,
        123 => Profile::Chrome123,
        124..=125 => Profile::Chrome124,
        126 => Profile::Chrome126,
        127 => Profile::Chrome127,
        128 => Profile::Chrome128,
        129 => Profile::Chrome129,
        130 => Profile::Chrome130,
        131 => Profile::Chrome131,
        132 => Profile::Chrome132,
        133 => Profile::Chrome133,
        134 => Profile::Chrome134,
        135 => Profile::Chrome135,
        136 => Profile::Chrome136,
        137 => Profile::Chrome137,
        138 => Profile::Chrome138,
        139 => Profile::Chrome139,
        140 => Profile::Chrome140,
        141 => Profile::Chrome141,
        142 => Profile::Chrome142,
        143 => Profile::Chrome143,
        144 => Profile::Chrome144,
        145 => Profile::Chrome145,
        146 => Profile::Chrome146,
        147 => Profile::Chrome147,
        148 => Profile::Chrome148,
        _ => Profile::Chrome149,
    }
}

fn platform_for(os: Os) -> Platform {
    match os {
        Os::Windows => Platform::Windows,
        Os::MacOs => Platform::MacOS,
        Os::Linux => Platform::Linux,
    }
}

impl WreqFetcher {
    pub fn new(cfg: FetcherConfig) -> anyhow::Result<Self> {
        let emulation = Emulation::builder()
            .profile(profile_of(&cfg.identity))
            .platform(platform_for(cfg.identity.os))
            .build();
        let mut b = wreq::Client::builder()
            .emulation(emulation)
            .cookie_store(true)
            // wreq follows no redirect unless told to, and reqwest follows ten — so the two
            // engines disagreed about what a fetch even is, and the default build was the one
            // that got it wrong. Any URL that redirects (http to https, a bare host to `www`, a
            // trailing slash, a login gate) came back as the 3xx stub, and a classifier looking
            // at "Found. Redirecting to /i/flow/login" has no way to tell that from a page.
            // Ten is what a browser allows and what the other engine already used.
            .redirect(wreq::redirect::Policy::limited(10))
            .timeout(cfg.timeout);
        if let Some(p) = &cfg.proxy {
            b = b.proxy(wreq::Proxy::all(p)?);
        }
        Ok(Self {
            client: b.build()?,
            identity: cfg.identity,
        })
    }
}

#[async_trait::async_trait]
impl HttpFetcher for WreqFetcher {
    fn engine(&self) -> &'static str {
        "wreq"
    }

    fn identity(&self) -> &IdentityProfile {
        &self.identity
    }

    async fn send(&self, req: HttpRequest) -> anyhow::Result<HttpResponse> {
        let method = wreq::Method::from_bytes(req.method.to_uppercase().as_bytes())?;
        let mut r = self.client.request(method, &req.url);
        // The emulation already installs Chrome's header set in Chrome's order. Only headers the
        // caller actually specified are applied on top, so an API call can still set its own
        // content-type or authorization without disturbing the navigation fingerprint.
        for (k, v) in &req.headers {
            r = r.header(k.as_str(), v.as_str());
        }
        if let Some(body) = req.body {
            r = r.body(body);
        }
        let resp = r.send().await?;
        let status = resp.status().as_u16();
        let final_url = resp.uri().to_string();
        let http_version = match resp.version() {
            wreq::Version::HTTP_2 => "HTTP/2.0",
            wreq::Version::HTTP_3 => "HTTP/3.0",
            wreq::Version::HTTP_10 => "HTTP/1.0",
            _ => "HTTP/1.1",
        };
        let headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
            .collect();
        let content_type = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.to_ascii_lowercase())
            .unwrap_or_default();
        Ok(HttpResponse {
            status,
            final_url,
            headers,
            content_type,
            body: resp.bytes().await?.to_vec(),
            http_version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_never_overstates_the_advertised_major() {
        // A profile newer than what we claim would produce a handshake that contradicts the UA.
        assert!(matches!(profile_for(149), Profile::Chrome149));
        assert!(matches!(profile_for(147), Profile::Chrome147));
        assert!(matches!(profile_for(145), Profile::Chrome145));
        assert!(matches!(profile_for(131), Profile::Chrome131));
        assert!(matches!(profile_for(90), Profile::Chrome100));
    }

    /// Every major the identity layer may advertise must land on a profile that claims no newer
    /// Chrome than the User-Agent does — a handshake ahead of the UA is the cross-check vendors
    /// run — and the ceiling itself must have a profile of its own.
    #[test]
    fn every_major_the_identity_can_advertise_has_a_profile_that_does_not_overstate_it() {
        use svipall_core::MAX_EMULATED_CHROME;
        for major in 100..=MAX_EMULATED_CHROME {
            let name = format!("{:?}", profile_for(major));
            let claimed: u16 = name.trim_start_matches("Chrome").parse().unwrap();
            assert!(claimed <= major, "profile {name} overstates major {major}");
        }
        assert!(matches!(
            profile_for(MAX_EMULATED_CHROME),
            Profile::Chrome149
        ));
    }

    #[test]
    fn platform_follows_the_identity() {
        assert_eq!(platform_for(Os::Windows), Platform::Windows);
        assert_eq!(platform_for(Os::MacOs), Platform::MacOS);
        assert_eq!(platform_for(Os::Linux), Platform::Linux);
    }

    #[test]
    fn a_firefox_identity_selects_a_firefox_profile_that_does_not_overstate_it() {
        use svipall_core::{IdentityProfile, MAX_EMULATED_FIREFOX};
        let id = IdentityProfile::firefox(MAX_EMULATED_FIREFOX, Os::Windows);
        let name = format!("{:?}", profile_of(&id));
        assert!(
            name.starts_with("Firefox"),
            "a Firefox identity picks a Firefox profile: {name}"
        );
        // Never a Chrome profile for a Firefox identity, and never a claimed Firefox above the UA.
        for major in [117, 128, 135, 143, 149] {
            let name = format!("{:?}", firefox_profile_for(major));
            let claimed: u16 = name.trim_start_matches("Firefox").parse().unwrap();
            assert!(claimed <= major, "profile {name} overstates major {major}");
        }
        // A Chrome identity still picks a Chrome profile.
        let chrome = IdentityProfile::for_major(149, Os::Linux);
        assert!(format!("{:?}", profile_of(&chrome)).starts_with("Chrome"));
    }
}
