//! The single answer to "who are we pretending to be".
//!
//! There used to be four: a `UA` constant in the MCP server, the header block in the http tier, the
//! stealth init script in the browser pool, and whatever Chromium binary happened to launch. They
//! disagreed, and measurements showed it — a UA claiming Chrome 152 over a rustls handshake, and a
//! stealth script hard-coding `Win32` regardless of the host OS.
//!
//! Everything that states an identity now reads it from here.

use serde::Serialize;

/// Highest Chrome major any compiled-in TLS engine can emulate.
///
/// This is the ceiling for what we may *claim*, because TLS is the one layer that cannot lie: the
/// handshake is what it is. Verified against `wreq-util` 0.2, whose newest Chrome profile is 149.
pub const MAX_EMULATED_CHROME: u16 = 149;

/// Highest Firefox major the TLS engine can emulate, the same ceiling for the same reason.
/// `wreq-util` 0.2 carries Firefox profiles through 151.
pub const MAX_EMULATED_FIREFOX: u16 = 151;

/// Which browser this identity is. It decides the whole shape: Chrome sends client hints and a
/// `priority` header, Firefox sends neither and orders its headers differently; the two engines
/// also disagree on which JavaScript surfaces even exist. An identity that mixes them is the
/// contradiction anti-bot vendors look for, so the engine is chosen once and everything downstream
/// reads it from here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    #[default]
    Chrome,
    Firefox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Windows,
    MacOs,
    Linux,
}

impl Os {
    pub fn host() -> Self {
        #[cfg(target_os = "windows")]
        {
            Os::Windows
        }
        #[cfg(target_os = "macos")]
        {
            Os::MacOs
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            Os::Linux
        }
    }

    /// `navigator.platform`.
    pub fn platform_js(self) -> &'static str {
        match self {
            Os::Windows => "Win32",
            Os::MacOs => "MacIntel",
            Os::Linux => "Linux x86_64",
        }
    }

    /// The `Sec-CH-UA-Platform` token, quotes included.
    pub fn sec_ch_ua_platform(self) -> &'static str {
        match self {
            Os::Windows => "\"Windows\"",
            Os::MacOs => "\"macOS\"",
            Os::Linux => "\"Linux\"",
        }
    }

    fn ua_platform(self) -> &'static str {
        match self {
            Os::Windows => "Windows NT 10.0; Win64; x64",
            Os::MacOs => "Macintosh; Intel Mac OS X 10_15_7",
            Os::Linux => "X11; Linux x86_64",
        }
    }

    /// Firefox's platform token, which differs from Chrome's: macOS uses `10.15` with a dot, not
    /// Chrome's frozen `10_15_7`, and Firefox on Windows/Linux drops the `Win64; x64` / adds no
    /// arch. Getting this wrong is the first thing an engine-aware check catches.
    fn ua_platform_firefox(self) -> &'static str {
        match self {
            Os::Windows => "Windows NT 10.0; Win64; x64; rv:VER",
            Os::MacOs => "Macintosh; Intel Mac OS X 10.15; rv:VER",
            Os::Linux => "X11; Linux x86_64; rv:VER",
        }
    }

    /// Height taken by the OS shell (taskbar, dock, menu bar). `screen.availHeight` equal to
    /// `screen.height` on a desktop OS is a contradiction fingerprinters look for.
    fn chrome_height(self) -> u32 {
        match self {
            Os::Windows => 48,
            Os::MacOs => 25,
            Os::Linux => 27,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Screen {
    pub width: u32,
    pub height: u32,
    pub avail_width: u32,
    pub avail_height: u32,
    pub color_depth: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    /// Browser UI above the viewport: tab strip, address bar, bookmarks. Never a pixel or two.
    pub outer_extra_height: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityProfile {
    /// The engine whose network and JS shape this identity wears.
    #[serde(default)]
    pub engine: Engine,
    pub chrome_major: u16,
    pub full_version: String,
    pub os: Os,
    pub user_agent: String,
    pub sec_ch_ua: String,
    pub sec_ch_ua_platform: &'static str,
    pub sec_ch_ua_mobile: &'static str,
    pub accept_language: String,
    pub timezone: String,
    pub platform_js: &'static str,
    pub screen: Screen,
    pub viewport: Viewport,
    pub hardware_concurrency: u32,
    pub device_memory: u32,
    pub webgl_vendor: String,
    pub webgl_renderer: String,
    /// Seed for canvas and audio noise. Stable per identity, so a site that fingerprints the same
    /// profile twice sees the same value — noise that changes every load is as telling as none.
    pub noise_seed: u64,

    // Surfaces below this line were reporting whatever the real Chromium happened to say, which
    // may not agree with the machine this identity claims to be. A detector does not need any one
    // of them to be wrong; it needs two of them to disagree.
    /// `navigator.deviceMemory` is already above, but `navigator.storage.estimate()` reports a
    /// quota derived from the real disk. Bytes, rounded the way Chrome rounds it.
    pub storage_quota: u64,
    /// `performance.memory.jsHeapSizeLimit`. Chrome reports a fixed ceiling per platform, not the
    /// machine's RAM, so a value derived from the host is itself the tell.
    pub js_heap_limit: u64,
    /// `navigator.connection`: effective type, downlink in Mbit/s and round-trip in ms. A desktop
    /// on a fixed line is `4g` with a high downlink — the API reports the class, not the medium.
    pub connection: Connection,
    /// How many audio and video devices `navigator.mediaDevices.enumerateDevices()` admits to.
    /// Zero devices on a desktop is not a normal machine.
    pub media_devices: MediaDevices,
    /// `devicePixelRatio`. Coupled to the screen, so it lives with it rather than being assumed 1.
    pub device_pixel_ratio: f32,
}

/// What `navigator.connection` reports.
#[derive(Debug, Clone, Serialize)]
pub struct Connection {
    pub effective_type: &'static str,
    pub downlink: f32,
    pub rtt: u32,
}

/// What `navigator.mediaDevices.enumerateDevices()` admits to.
///
/// The counts matter more than the labels: without permission Chrome returns entries with empty
/// labels, so a page that finds *labelled* devices has found a patch.
#[derive(Debug, Clone, Serialize)]
pub struct MediaDevices {
    pub audio_inputs: u8,
    pub audio_outputs: u8,
    pub video_inputs: u8,
}

impl IdentityProfile {
    /// The rule: never claim a Chrome newer than the TLS layer can actually produce.
    ///
    /// Two mismatches are possible when the installed browser is newer than the emulation ceiling.
    /// Claiming the browser's version leaves a handshake that disagrees with the User-Agent, and
    /// that cross-check is exactly what anti-bot vendors run. Claiming the emulated version instead
    /// is only visible to a site that probes for JS APIs newer than the announced major, which is
    /// rare and mostly harmless. So the advertised version is the lower of the two, and the browser
    /// tiers override their UA down to match.
    pub fn resolve(browser_major: Option<u16>, cfg: &crate::Config) -> Self {
        let major = browser_major
            .unwrap_or(MAX_EMULATED_CHROME)
            .min(MAX_EMULATED_CHROME);
        Self::for_major(major, Os::host()).with_config(cfg)
    }

    /// A Firefox identity for the http tier, honouring the configured locale and timezone. Used
    /// when `http_firefox` is set; the browser tiers keep the Chrome identity `resolve` builds.
    pub fn resolve_firefox(cfg: &crate::Config) -> Self {
        Self::firefox(MAX_EMULATED_FIREFOX, Os::host()).with_config(cfg)
    }

    fn with_config(mut self, cfg: &crate::Config) -> Self {
        if !cfg.locale.trim().is_empty() {
            self.accept_language = cfg.locale.clone();
        }
        if !cfg.timezone.trim().is_empty() {
            self.timezone = cfg.timezone.clone();
        }
        self
    }

    pub fn for_major(chrome_major: u16, os: Os) -> Self {
        let full_version = format!("{}.0.0.0", chrome_major);
        let user_agent = format!(
            "Mozilla/5.0 ({}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{} Safari/537.36",
            os.ua_platform(),
            full_version
        );
        // Brand list shape as Chromium emits it, and as verified coming out of wreq's emulation.
        let sec_ch_ua = format!(
            "\"Google Chrome\";v=\"{m}\", \"Not.A/Brand\";v=\"8\", \"Chromium\";v=\"{m}\"",
            m = chrome_major
        );
        let (width, height) = (1920, 1080);
        Self {
            engine: Engine::Chrome,
            chrome_major,
            full_version,
            os,
            user_agent,
            sec_ch_ua,
            sec_ch_ua_platform: os.sec_ch_ua_platform(),
            sec_ch_ua_mobile: "?0",
            accept_language: "en-US,en;q=0.9".into(),
            timezone: "America/New_York".into(),
            platform_js: os.platform_js(),
            screen: Screen {
                width,
                height,
                avail_width: width,
                avail_height: height - os.chrome_height(),
                color_depth: 24,
            },
            viewport: Viewport {
                width: 1366,
                height: 768,
                outer_extra_height: 106,
            },
            hardware_concurrency: 8,
            device_memory: 8,
            webgl_vendor: crate::fleet::default_gpu(os).0.into(),
            webgl_renderer: crate::fleet::default_gpu(os).1.into(),
            noise_seed: crate::domain::stable_hash(&format!("{}-{:?}", chrome_major, os)),
            // 10 GiB. Chrome reports a quota derived from free disk space and rounds it hard; a
            // precise, unusual number is more identifying than a plausible round one.
            storage_quota: 10 * 1024 * 1024 * 1024,
            // Chrome's desktop ceiling, and it does not vary with installed RAM.
            js_heap_limit: 4 * 1024 * 1024 * 1024,
            connection: Connection {
                effective_type: "4g",
                downlink: 10.0,
                rtt: 50,
            },
            // A laptop: microphone, speakers, webcam.
            media_devices: MediaDevices {
                audio_inputs: 1,
                audio_outputs: 1,
                video_inputs: 1,
            },
            device_pixel_ratio: 1.0,
        }
    }

    /// A Firefox identity: Gecko engine, no client hints, Firefox's header order and `accept`.
    ///
    /// Built on the Chrome identity's machine and screen so the fleet and geo logic apply
    /// unchanged, then the engine-specific surfaces are switched over. The three fields Firefox
    /// does not expose to the page at all — `navigator.deviceMemory`, `navigator.connection`,
    /// `performance.memory` — keep their values in the struct but are never emitted for a Firefox
    /// identity (the stealth surface reads `engine` and omits them); reporting them *is* the tell.
    pub fn firefox(gecko_major: u16, os: Os) -> Self {
        let ver = gecko_major.min(MAX_EMULATED_FIREFOX);
        let mut id = Self::for_major(ver, os);
        id.engine = Engine::Firefox;
        id.full_version = format!("{ver}.0");
        id.user_agent = format!(
            "Mozilla/5.0 ({}) Gecko/20100101 Firefox/{ver}.0",
            os.ua_platform_firefox().replace("VER", &format!("{ver}.0"))
        );
        // Firefox sends no Sec-CH-UA of any kind. Blanking these keeps a Firefox identity from
        // ever emitting a Chrome-only header by accident.
        id.sec_ch_ua = String::new();
        id.sec_ch_ua_platform = "";
        id.sec_ch_ua_mobile = "";
        // Firefox's real GPU strings are not ANGLE; on the browser tier a patched build reports the
        // host's true renderer (see docs/firefox.md). The http tier has no WebGL, so the default is
        // only a placeholder there.
        id.webgl_vendor = "Mozilla".into();
        id.webgl_renderer = "Mozilla".into();
        id.noise_seed = crate::domain::stable_hash(&format!("firefox-{ver}-{os:?}"));
        id
    }

    /// Wear a machine drawn from the fleet, instead of the one hard-coded value everyone gets.
    ///
    /// The seed decides which machine, so a domain that keeps its seed keeps its hardware. That
    /// matters more than the variety does: a profile whose screen and GPU change between visits
    /// has identified itself without anyone having to fingerprint it.
    ///
    /// The noise seed moves with the machine, so canvas, audio and text geometry belong to the
    /// same device as the hardware they are meant to be produced by.
    pub fn as_machine(mut self, seed: u64) -> Self {
        let m = crate::fleet::machine(seed, self.os);
        self.screen = Screen {
            width: m.screen_width,
            height: m.screen_height,
            avail_width: m.screen_width,
            avail_height: m.screen_height.saturating_sub(self.os.chrome_height()),
            color_depth: self.screen.color_depth,
        };
        self.viewport = Viewport {
            width: m.viewport_width,
            height: m.viewport_height,
            outer_extra_height: self.viewport.outer_extra_height,
        };
        self.hardware_concurrency = m.hardware_concurrency;
        self.device_memory = m.device_memory;
        self.webgl_vendor = m.webgl_vendor;
        self.webgl_renderer = m.webgl_renderer;
        self.device_pixel_ratio = m.device_pixel_ratio;
        self.noise_seed = seed;
        self
    }

    /// Become a phone.
    ///
    /// Worth having for a reason that has nothing to do with stealth: mobile layouts carry less
    /// chrome and fewer widgets, so the same article costs materially fewer tokens. Everything has
    /// to move together — a desktop user agent with a phone viewport, or a phone that reports eight
    /// cores and a 1920px screen, is a contradiction rather than a saving.
    pub fn as_phone(mut self) -> Self {
        self.user_agent = format!(
            "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/{} Mobile Safari/537.36",
            self.full_version
        );
        self.sec_ch_ua_mobile = "?1";
        self.sec_ch_ua_platform = "\"Android\"";
        self.platform_js = "Linux armv81";
        self.screen = Screen {
            width: 412,
            height: 915,
            avail_width: 412,
            avail_height: 915,
            color_depth: 24,
        };
        self.viewport = Viewport {
            width: 412,
            height: 823,
            outer_extra_height: 0,
        };
        self.device_pixel_ratio = 2.625;
        self.hardware_concurrency = 8;
        self.device_memory = 8;
        self.webgl_vendor = "Google Inc. (Qualcomm)".into();
        self.webgl_renderer = "ANGLE (Qualcomm, Adreno (TM) 740, OpenGL ES 3.2)".into();
        self
    }

    /// Move this identity to a country, so nothing contradicts the address the traffic leaves from.
    ///
    /// The check a site runs is one line: read `Intl.DateTimeFormat().resolvedOptions().timeZone`
    /// and compare it with where the request came from. Routing a domain through Germany while the
    /// browser insists it is in New York fails that immediately, and it was failing for every
    /// proxied domain svipall had.
    ///
    /// An unknown country leaves the identity alone. Wearing the wrong country is a contradiction;
    /// wearing none only looks unproxied.
    pub fn in_country(mut self, code: &str) -> Self {
        if let Some(region) = crate::geo::for_country(code) {
            self.timezone = region.timezone.to_string();
            self.accept_language = region.accept_language.to_string();
        }
        self
    }

    /// Just the language tag (`en-US`), for CDP's locale override, which rejects a full
    /// `Accept-Language` list.
    pub fn locale_tag(&self) -> String {
        self.accept_language
            .split([',', ';'])
            .next()
            .unwrap_or("en-US")
            .trim()
            .to_string()
    }

    /// `accept_language` is a header: a list of tags with quality values. `navigator.languages` and
    /// Chromium's `--lang` switch take the tags alone, so the quality values are dropped here
    /// rather than at each call site — passing the header where the list belongs put `en;q=0.9`
    /// into a list that may only ever hold language tags.
    pub fn language_tags(&self) -> Vec<String> {
        self.accept_language
            .split(',')
            .map(|t| t.split(';').next().unwrap_or(t).trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()
    }

    /// The headers the browser sends on a top-level navigation, in the order it sends them. Order
    /// is part of the fingerprint, which is why this is a `Vec` and not a map, and it is the whole
    /// reason the engine matters here: Firefox leads with `User-Agent`, sends no `Sec-CH-UA*` and
    /// no `priority`, and orders `accept` / `accept-language` / `accept-encoding` its own way. A
    /// Chrome header block over a Firefox TLS handshake is a contradiction in the first packet.
    pub fn nav_headers(&self) -> Vec<(String, String)> {
        let headers: Vec<(&str, &str)> = match self.engine {
            Engine::Chrome => vec![
                ("sec-ch-ua", self.sec_ch_ua.as_str()),
                ("sec-ch-ua-mobile", self.sec_ch_ua_mobile),
                ("sec-ch-ua-platform", self.sec_ch_ua_platform),
                ("upgrade-insecure-requests", "1"),
                ("user-agent", self.user_agent.as_str()),
                ("accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7"),
                ("sec-fetch-site", "none"),
                ("sec-fetch-mode", "navigate"),
                ("sec-fetch-user", "?1"),
                ("sec-fetch-dest", "document"),
                ("accept-encoding", "gzip, deflate, br, zstd"),
                ("accept-language", self.accept_language.as_str()),
                ("priority", "u=0, i"),
            ],
            // Firefox's real navigation header set and order (Firefox 128+): User-Agent first, its
            // own `accept`, no client hints, no `priority`, and `Sec-Fetch-*` after the accepts.
            Engine::Firefox => vec![
                ("user-agent", self.user_agent.as_str()),
                ("accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/png,image/svg+xml,*/*;q=0.8"),
                ("accept-language", self.accept_language.as_str()),
                ("accept-encoding", "gzip, deflate, br, zstd"),
                ("upgrade-insecure-requests", "1"),
                ("sec-fetch-dest", "document"),
                ("sec-fetch-mode", "navigate"),
                ("sec-fetch-site", "none"),
                ("sec-fetch-user", "?1"),
                ("priority", "u=0, i"),
            ],
        };
        headers
            .into_iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// `userAgentMetadata` for CDP `Emulation.setUserAgentOverride`, which is what actually
    /// populates `navigator.userAgentData` — a surface the old init script never touched.
    pub fn ua_metadata(&self) -> serde_json::Value {
        let m = self.chrome_major.to_string();
        serde_json::json!({
            "brands": [
                {"brand": "Google Chrome", "version": m},
                {"brand": "Not.A/Brand", "version": "8"},
                {"brand": "Chromium", "version": m},
            ],
            "fullVersionList": [
                {"brand": "Google Chrome", "version": self.full_version},
                {"brand": "Not.A/Brand", "version": "8.0.0.0"},
                {"brand": "Chromium", "version": self.full_version},
            ],
            "fullVersion": self.full_version,
            "platform": match self.os { Os::Windows => "Windows", Os::MacOs => "macOS", Os::Linux => "Linux" },
            "platformVersion": match self.os { Os::Windows => "15.0.0", Os::MacOs => "14.6.1", Os::Linux => "6.8.0" },
            "architecture": "x86",
            "model": "",
            "mobile": false,
            "bitness": "64",
            "wow64": false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The newest major the TLS layer can emulate; every test below dresses as it.
    const LATEST: u16 = MAX_EMULATED_CHROME;

    /// `navigator.languages` and `--lang` take tags. A quality value reaching either of them is a
    /// header where a list belongs, and it is visible to any page that reads the array.
    #[test]
    fn language_tags_carry_no_quality_values() {
        let mut id = IdentityProfile::for_major(LATEST, Os::Windows);
        id.accept_language = "en-US,en;q=0.9".into();
        assert_eq!(id.language_tags(), vec!["en-US", "en"]);
        id.accept_language = "de-DE,de;q=0.9,en-US;q=0.8,en;q=0.7".into();
        assert_eq!(id.language_tags(), vec!["de-DE", "de", "en-US", "en"]);
        assert!(id.language_tags().iter().all(|t| !t.contains(';')));
    }

    #[test]
    fn a_firefox_identity_is_gecko_all_the_way_down() {
        let id = IdentityProfile::firefox(MAX_EMULATED_FIREFOX, Os::Windows);
        assert_eq!(id.engine, Engine::Firefox);
        assert!(
            id.user_agent.contains("Gecko/20100101 Firefox/"),
            "{}",
            id.user_agent
        );
        assert!(
            !id.user_agent.contains("Chrome"),
            "no Chrome token: {}",
            id.user_agent
        );
        assert!(
            id.user_agent.contains("rv:"),
            "Gecko UA carries rv: {}",
            id.user_agent
        );
        let headers = id.nav_headers();
        let keys: Vec<&str> = headers.iter().map(|(k, _)| k.as_str()).collect();
        // User-Agent leads, and no client hints appear at all.
        assert_eq!(
            keys.first(),
            Some(&"user-agent"),
            "Firefox leads with UA: {keys:?}"
        );
        assert!(
            !keys.iter().any(|k| k.starts_with("sec-ch-ua")),
            "no client hints: {keys:?}"
        );
        // The accept header is Firefox's, not Chrome's signed-exchange one.
        let accept = &headers.iter().find(|(k, _)| k == "accept").unwrap().1;
        assert!(
            !accept.contains("signed-exchange"),
            "Firefox accept: {accept}"
        );
        assert!(accept.contains("image/svg+xml"), "Firefox accept: {accept}");
    }

    #[test]
    fn chrome_still_sends_client_hints_and_leads_with_them() {
        // The Firefox branch must not have changed the Chrome one.
        let id = IdentityProfile::for_major(LATEST, Os::Windows);
        assert_eq!(id.engine, Engine::Chrome);
        let keys: Vec<String> = id.nav_headers().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys.first().map(String::as_str), Some("sec-ch-ua"));
        assert!(keys.iter().any(|k| k == "priority"));
        assert!(id.user_agent.contains("Chrome/"));
    }

    #[test]
    fn the_firefox_ceiling_is_the_one_the_tls_engine_can_emulate() {
        // Asking for a Firefox newer than the profiles carried is clamped, exactly as Chrome is.
        let id = IdentityProfile::firefox(999, Os::Linux);
        assert!(id.chrome_major <= MAX_EMULATED_FIREFOX);
        assert!(id
            .user_agent
            .contains(&format!("Firefox/{MAX_EMULATED_FIREFOX}.0")));
    }

    #[test]
    fn a_seeded_machine_is_coherent_with_the_identity_that_wears_it() {
        let id = IdentityProfile::for_major(LATEST, Os::Windows).as_machine(0xC0FFEE);
        assert!(id.screen.avail_height < id.screen.height, "no taskbar");
        assert!(
            id.viewport.width <= id.screen.width,
            "window wider than screen"
        );
        assert!(id.webgl_renderer.contains("D3D11"), "{}", id.webgl_renderer);
        assert_eq!(id.noise_seed, 0xC0FFEE, "noise must belong to this machine");
    }

    #[test]
    fn the_same_seed_gives_the_same_machine_twice() {
        // Hardware that changes between two visits from one profile is self-identifying.
        let a = IdentityProfile::for_major(LATEST, Os::Windows).as_machine(42);
        let b = IdentityProfile::for_major(LATEST, Os::Windows).as_machine(42);
        assert_eq!(a.screen.width, b.screen.width);
        assert_eq!(a.webgl_renderer, b.webgl_renderer);
        assert_eq!(a.hardware_concurrency, b.hardware_concurrency);
    }

    #[test]
    fn two_seeds_really_are_two_machines() {
        let a = IdentityProfile::for_major(LATEST, Os::Windows).as_machine(1);
        let b = IdentityProfile::for_major(LATEST, Os::Windows).as_machine(9_999_999);
        assert!(
            a.screen.width != b.screen.width
                || a.hardware_concurrency != b.hardware_concurrency
                || a.webgl_renderer != b.webgl_renderer,
            "the fleet produced one machine twice"
        );
    }

    #[test]
    fn a_machine_and_a_country_compose_without_fighting() {
        // Hardware and locale are independent axes; applying both must not undo either.
        let id = IdentityProfile::for_major(LATEST, Os::Windows)
            .as_machine(7)
            .in_country("JP");
        assert_eq!(id.timezone, "Asia/Tokyo");
        assert!(id.accept_language.starts_with("ja-JP"));
        assert!(id.webgl_renderer.contains("D3D11"));
    }

    #[test]
    fn a_phone_is_a_phone_all_the_way_through() {
        // A desktop user agent with a phone viewport, or a phone claiming a 1920px screen, is a
        // contradiction rather than a saving.
        let id = IdentityProfile::for_major(LATEST, Os::Windows).as_phone();
        assert!(id.user_agent.contains("Mobile"), "{}", id.user_agent);
        assert!(id.user_agent.contains("Android"));
        assert_eq!(id.sec_ch_ua_mobile, "?1");
        assert!(id.screen.width < 500, "desktop screen on a phone");
        assert!(id.viewport.width <= id.screen.width);
        assert!(id.device_pixel_ratio > 2.0, "phones are high-DPI");
        assert!(!id.webgl_renderer.contains("D3D11"), "Direct3D on Android");
    }

    /// The cross-check this exists to survive: timezone against exit address.
    #[test]
    fn moving_to_a_country_moves_the_timezone_and_the_languages_together() {
        let id = IdentityProfile::for_major(LATEST, Os::Windows).in_country("DE");
        assert_eq!(id.timezone, "Europe/Berlin");
        assert!(
            id.accept_language.starts_with("de-DE"),
            "{}",
            id.accept_language
        );
        assert_eq!(id.locale_tag(), "de-DE");
    }

    #[test]
    fn an_unknown_country_leaves_the_identity_untouched() {
        let before = IdentityProfile::for_major(LATEST, Os::Windows);
        let after = before.clone().in_country("ZZ");
        assert_eq!(before.timezone, after.timezone);
        assert_eq!(before.accept_language, after.accept_language);
    }

    #[test]
    fn the_language_list_the_browser_reports_follows_the_country() {
        // `navigator.languages` is built from accept_language, so moving country has to move both
        // or the two disagree with each other.
        let id = IdentityProfile::for_major(LATEST, Os::Windows).in_country("JP");
        let langs: Vec<&str> = id
            .accept_language
            .split(',')
            .map(|t| t.split(';').next().unwrap_or(t).trim())
            .collect();
        assert_eq!(langs.first(), Some(&"ja-JP"));
        assert!(langs.contains(&"en"), "{langs:?}");
    }

    #[test]
    fn advertised_major_is_clamped_to_what_tls_can_emulate() {
        let id = IdentityProfile::resolve(Some(152), &crate::Config::default());
        assert_eq!(id.chrome_major, MAX_EMULATED_CHROME);
        assert!(id
            .user_agent
            .contains(&format!("Chrome/{}.0.0.0", MAX_EMULATED_CHROME)));
        assert!(id
            .sec_ch_ua
            .contains(&format!("\"Google Chrome\";v=\"{}\"", MAX_EMULATED_CHROME)));
    }

    #[test]
    fn an_older_browser_wins_over_the_cap() {
        let id = IdentityProfile::resolve(Some(140), &crate::Config::default());
        assert_eq!(id.chrome_major, 140);
        assert!(id.user_agent.contains("Chrome/140.0.0.0"));
    }

    #[test]
    fn ua_sec_ch_ua_and_metadata_all_state_the_same_version() {
        let id = IdentityProfile::for_major(LATEST, Os::Windows);
        let meta = id.ua_metadata();
        assert!(id.user_agent.contains(&format!("Chrome/{LATEST}")));
        assert!(id.sec_ch_ua.contains(&format!("v=\"{LATEST}\"")));
        assert_eq!(meta["brands"][0]["version"], LATEST.to_string());
        assert_eq!(meta["fullVersion"], format!("{LATEST}.0.0.0"));
    }

    /// `availHeight == height` on a desktop OS says there is no taskbar, dock or menu bar.
    #[test]
    fn available_screen_is_smaller_than_the_screen() {
        for os in [Os::Windows, Os::MacOs, Os::Linux] {
            let id = IdentityProfile::for_major(LATEST, os);
            assert!(
                id.screen.avail_height < id.screen.height,
                "{os:?} reports no OS chrome at all"
            );
            assert!(id.screen.avail_width <= id.screen.width);
        }
    }

    /// Measured on the old stealth tier: outerHeight - innerHeight was 1px. Real Chrome is ~85-120.
    #[test]
    fn window_chrome_is_a_realistic_height() {
        let id = IdentityProfile::for_major(LATEST, Os::Windows);
        assert!((85..=140).contains(&id.viewport.outer_extra_height));
    }

    #[test]
    fn ua_platform_matches_the_declared_os() {
        assert!(IdentityProfile::for_major(LATEST, Os::Windows)
            .user_agent
            .contains("Windows NT"));
        assert!(IdentityProfile::for_major(LATEST, Os::MacOs)
            .user_agent
            .contains("Macintosh"));
        assert!(IdentityProfile::for_major(LATEST, Os::Linux)
            .user_agent
            .contains("X11; Linux"));
        assert_eq!(
            IdentityProfile::for_major(LATEST, Os::MacOs).platform_js,
            "MacIntel"
        );
    }

    #[test]
    fn nav_headers_are_in_chrome_order_and_unique() {
        let id = IdentityProfile::for_major(LATEST, Os::Windows);
        let h = id.nav_headers();
        let names: Vec<&str> = h.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names.first(), Some(&"sec-ch-ua"));
        assert!(
            names.iter().position(|n| *n == "user-agent")
                < names.iter().position(|n| *n == "accept"),
            "user-agent precedes accept in Chrome"
        );
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "duplicate header name");
    }

    /// Noise that changes per load is as identifying as no noise at all.
    #[test]
    fn noise_seed_is_stable_for_the_same_identity() {
        let a = IdentityProfile::for_major(LATEST, Os::Windows);
        let b = IdentityProfile::for_major(LATEST, Os::Windows);
        let c = IdentityProfile::for_major(LATEST, Os::Linux);
        assert_eq!(a.noise_seed, b.noise_seed);
        assert_ne!(a.noise_seed, c.noise_seed);
    }
}
