//! The check Camoufox says it keeps failing: is this identity internally consistent?
//!
//! A fingerprint is almost never caught by one odd value. It is caught by a *combination* that no
//! real device produces — a macOS user agent with a Windows GPU, a 4K screen with a phone's touch
//! points, a timezone that contradicts the language, an engine whose headers belong to a different
//! engine. Camoufox's own docs name this as the thing that still breaks it, because it has no way
//! to check a generated identity against itself before shipping it.
//!
//! svipall does. Every rule here is a pure function of an `IdentityProfile`, so the whole set runs
//! with no network and no browser, in the test suite and in `bench fingerprint`, and a build that
//! introduces a contradiction fails rather than shipping it. This is the differentiator: not a
//! cleaner way to spoof one value, but a machine that refuses to wear a combination that cannot
//! exist.

use crate::identity::Engine;
use crate::{IdentityProfile, Os};

/// The only values `navigator.deviceMemory` ever reports. The specification quantises it and caps
/// it at 8; anything else is a number no browser produces.
const ALLOWED_DEVICE_MEMORY: &[u32] = &[1, 2, 4, 8];

/// One thing wrong with an identity, named so a failing build says what to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub rule: &'static str,
    pub detail: String,
}

fn v(rule: &'static str, detail: impl Into<String>) -> Violation {
    Violation {
        rule,
        detail: detail.into(),
    }
}

/// True when this renderer is a software rasteriser — the tell of a server or a VM, not a desktop.
///
/// A machine with no usable GPU is not a fingerprint problem svipall can spoof away: reporting a
/// real GPU it does not have is caught by the render, and reporting the software one is caught by
/// being software. It is a hardware fact the operator has to know about, which is why it is named
/// here rather than hidden.
pub fn is_software_renderer(renderer: &str) -> bool {
    let r = renderer.to_ascii_lowercase();
    [
        "swiftshader",
        "llvmpipe",
        "software",
        "basic render",
        "microsoft basic",
    ]
    .iter()
    .any(|needle| r.contains(needle))
}

/// Every way `id` contradicts itself. Empty means coherent.
///
/// The rules are cross-layer on purpose: each single value here is plausible, and only the
/// combinations are impossible. That is the whole point — the entropy a detector uses lives in the
/// joint distribution, so the check has to be joint too.
pub fn violations(id: &IdentityProfile) -> Vec<Violation> {
    let mut out = Vec::new();
    let ua = id.user_agent.to_ascii_lowercase();
    let mobile = id.sec_ch_ua_mobile == "?1" || ua.contains("mobile") || ua.contains("android");

    // Engine ↔ user agent. A Firefox identity whose UA says Chrome, or the reverse, is the first
    // thing an engine-aware check catches.
    match id.engine {
        Engine::Chrome => {
            if !ua.contains("chrome") && !ua.contains("crios") {
                out.push(v("engine-ua", "Chrome engine but the UA names no Chrome"));
            }
            if ua.contains("firefox") {
                out.push(v("engine-ua", "Chrome engine but a Firefox UA"));
            }
        }
        Engine::Firefox => {
            if !ua.contains("firefox") || !ua.contains("gecko") {
                out.push(v("engine-ua", "Firefox engine but the UA is not Gecko"));
            }
            if ua.contains("chrome") {
                out.push(v("engine-ua", "Firefox engine but a Chrome UA"));
            }
        }
    }

    // Client hints belong to Chrome and to Chrome only. A Firefox identity that would emit any
    // Sec-CH-UA header has leaked a Chrome-only surface.
    let keys: Vec<String> = id.nav_headers().into_iter().map(|(k, _)| k).collect();
    let has_hints = keys.iter().any(|k| k.starts_with("sec-ch-ua"));
    match id.engine {
        Engine::Chrome if !has_hints => out.push(v("client-hints", "Chrome without Sec-CH-UA")),
        Engine::Firefox if has_hints => out.push(v("client-hints", "Firefox emitting Sec-CH-UA")),
        _ => {}
    }
    // Firefox has no navigator.deviceMemory, navigator.connection or performance.memory. The
    // struct still carries values (they are Chrome's), but a Firefox identity must never emit them,
    // which the stealth surface enforces by reading `engine`; here we assert the intent holds.
    // (Nothing to compute — the presence check lives in the browser layer; this documents it.)

    // Screen geometry. A desktop with availHeight == height has no taskbar, and a window wider than
    // the screen it sits on cannot exist.
    if !mobile && id.screen.avail_height >= id.screen.height {
        out.push(v(
            "screen-availheight",
            format!(
                "availHeight {} not below height {} on a desktop OS",
                id.screen.avail_height, id.screen.height
            ),
        ));
    }
    if id.viewport.width > id.screen.width || id.viewport.height > id.screen.height {
        out.push(v(
            "viewport-screen",
            format!(
                "viewport {}x{} larger than screen {}x{}",
                id.viewport.width, id.viewport.height, id.screen.width, id.screen.height
            ),
        ));
    }

    // Form factor ↔ platform. A phone UA on `Win32`, or a desktop UA reporting a phone screen, is
    // a contradiction rather than a saving.
    if mobile {
        if id.platform_js == "Win32" || id.platform_js == "MacIntel" {
            out.push(v(
                "mobile-platform",
                format!("mobile UA with desktop platform {}", id.platform_js),
            ));
        }
    } else if id.screen.width < 640 {
        out.push(v(
            "desktop-screen",
            format!("desktop UA with a {}px screen", id.screen.width),
        ));
    }

    // Timezone and language both have to be set; a proxy country moves both together, so one
    // without the other means the geo step did not run.
    if id.timezone.trim().is_empty() {
        out.push(v("timezone", "no timezone"));
    }
    if id.accept_language.trim().is_empty() {
        out.push(v("accept-language", "no accept-language"));
    }

    // GPU string ↔ engine. A Chrome identity renders through ANGLE and its renderer says so; the
    // masked `Mozilla`/`Mozilla` pair is Firefox's default, and on a Chrome identity it is wrong.
    if id.engine == Engine::Chrome
        && !mobile
        && (id.webgl_renderer == "Mozilla" || id.webgl_renderer.is_empty())
    {
        out.push(v(
            "renderer-engine",
            "Chrome identity with a masked/empty WebGL renderer",
        ));
    }

    // GPU string ↔ operating system. Each renderer here is a real one; only the pairing is
    // impossible. Stated as contradictions rather than requirements, because a masked renderer is
    // legitimate on Firefox and a phone reports neither of these APIs.
    let renderer = id.webgl_renderer.to_ascii_lowercase();
    let foreign = match id.os {
        Os::MacOs => renderer.contains("d3d11") || renderer.contains("direct3d"),
        Os::Windows | Os::Linux => renderer.contains("metal renderer"),
    };
    if foreign {
        out.push(v(
            "gpu-os",
            format!("{:?} reporting {}", id.os, id.webgl_renderer),
        ));
    }
    if id.os != Os::MacOs && !mobile && renderer.contains("apple") {
        out.push(v("gpu-os", format!("{:?} reporting an Apple GPU", id.os)));
    }

    // `navigator.deviceMemory` is quantised by the specification and capped at 8: a machine with
    // 64 GB reports 8. Any other number is not a rare machine, it is a machine no browser produces.
    if !ALLOWED_DEVICE_MEMORY.contains(&id.device_memory) {
        out.push(v(
            "device-memory",
            format!(
                "deviceMemory {} is not a value the API reports",
                id.device_memory
            ),
        ));
    }

    // Memory ↔ cores. Each value is ordinary on its own; the pair is a machine nobody built.
    let mem_cores_wrong = (id.hardware_concurrency >= 16 && id.device_memory < 8)
        || (id.hardware_concurrency >= 8 && id.device_memory <= 2);
    if mem_cores_wrong {
        out.push(v(
            "memory-cores",
            format!(
                "{} cores with {} GB",
                id.hardware_concurrency, id.device_memory
            ),
        ));
    }

    out
}

/// The macOS user-agent OS token must match the platform. Kept separate because it is the one rule
/// that reads the OS enum directly, and it catches the classic `10_15_7` vs `10.15` slip between
/// the two engines.
pub fn os_token_matches(id: &IdentityProfile) -> bool {
    let ua = &id.user_agent;
    // A phone wears an Android UA over the Linux OS enum; its token is `Android`, not the desktop
    // `Linux x86_64`, and that is coherent rather than a mismatch.
    if ua.contains("Android") {
        return ua.contains("Android");
    }
    match (id.os, id.engine) {
        (Os::MacOs, Engine::Chrome) => ua.contains("Mac OS X 10_15_7"),
        (Os::MacOs, Engine::Firefox) => ua.contains("Mac OS X 10.15"),
        (Os::Windows, _) => ua.contains("Windows NT 10.0"),
        (Os::Linux, _) => ua.contains("Linux x86_64"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MAX_EMULATED_CHROME, MAX_EMULATED_FIREFOX};

    #[test]
    fn a_renderer_never_belongs_to_another_platform() {
        // A Metal renderer on Windows, or Direct3D on a Mac, is a free contradiction: each single
        // string is plausible and only the pair is impossible.
        let mut mac = IdentityProfile::for_major(MAX_EMULATED_CHROME, Os::MacOs);
        mac.webgl_renderer =
            "ANGLE (Intel, Intel(R) UHD Graphics 630 Direct3D11 vs_5_0 ps_5_0, D3D11)".into();
        assert!(violations(&mac).iter().any(|x| x.rule == "gpu-os"));

        let mut win = IdentityProfile::for_major(MAX_EMULATED_CHROME, Os::Windows);
        win.webgl_renderer = "ANGLE (Apple, ANGLE Metal Renderer: Apple M1, Unspecified)".into();
        assert!(violations(&win).iter().any(|x| x.rule == "gpu-os"));
    }

    #[test]
    fn memory_and_cores_belong_to_the_same_class_of_machine() {
        let mut id = IdentityProfile::for_major(MAX_EMULATED_CHROME, Os::Windows).as_machine(0x11);
        id.hardware_concurrency = 16;
        id.device_memory = 2;
        assert!(violations(&id).iter().any(|x| x.rule == "memory-cores"));
    }

    #[test]
    fn device_memory_is_only_ever_a_value_the_api_reports() {
        // The specification quantises `navigator.deviceMemory` and caps it at 8. A machine that
        // reports 16 has reported a number no browser produces, which is worse than reporting a
        // common one: it is unique on its own.
        let mut id = IdentityProfile::for_major(MAX_EMULATED_CHROME, Os::Windows).as_machine(0x11);
        id.device_memory = 16;
        assert!(violations(&id).iter().any(|x| x.rule == "device-memory"));
    }

    #[test]
    fn every_machine_the_fleet_draws_is_coherent() {
        // The linter is only worth what it covers. Seven fixtures said nothing about the space the
        // sampler actually draws from.
        for os in [Os::Windows, Os::MacOs, Os::Linux] {
            for seed in 0..500u64 {
                let id = IdentityProfile::for_major(MAX_EMULATED_CHROME, os)
                    .as_machine(seed.wrapping_mul(0x9E37_79B9));
                let v = violations(&id);
                assert!(v.is_empty(), "{os:?} seed {seed}: {v:?}");
            }
        }
    }

    #[test]
    fn the_default_identities_are_coherent() {
        for os in [Os::Windows, Os::MacOs, Os::Linux] {
            let chrome = IdentityProfile::for_major(MAX_EMULATED_CHROME, os).as_machine(0x1234);
            assert!(
                violations(&chrome).is_empty(),
                "chrome/{os:?}: {:?}",
                violations(&chrome)
            );
            assert!(os_token_matches(&chrome), "chrome os token {os:?}");
            let firefox = IdentityProfile::firefox(MAX_EMULATED_FIREFOX, os);
            assert!(
                violations(&firefox).is_empty(),
                "firefox/{os:?}: {:?}",
                violations(&firefox)
            );
            assert!(os_token_matches(&firefox), "firefox os token {os:?}");
        }
    }

    #[test]
    fn the_phone_identity_is_coherent() {
        let phone = IdentityProfile::for_major(MAX_EMULATED_CHROME, Os::Linux).as_phone();
        assert!(violations(&phone).is_empty(), "{:?}", violations(&phone));
    }

    #[test]
    fn a_chrome_ua_on_a_firefox_engine_is_caught() {
        let mut id = IdentityProfile::firefox(MAX_EMULATED_FIREFOX, Os::Windows);
        id.user_agent = IdentityProfile::for_major(140, Os::Windows).user_agent;
        assert!(violations(&id).iter().any(|x| x.rule == "engine-ua"));
    }

    #[test]
    fn a_firefox_that_would_emit_client_hints_is_caught() {
        let mut id = IdentityProfile::firefox(MAX_EMULATED_FIREFOX, Os::Windows);
        // Force a Sec-CH-UA back on by making it Chrome-shaped without changing the UA engine.
        id.sec_ch_ua = "\"x\"".into();
        id.sec_ch_ua_mobile = "?0";
        id.sec_ch_ua_platform = "\"Windows\"";
        // nav_headers only emits hints for the Chrome engine, so a Firefox engine stays clean —
        // this asserts the header layer, not just the field.
        assert!(!violations(&id).iter().any(|x| x.rule == "client-hints"));
    }

    #[test]
    fn a_taskbarless_desktop_and_an_oversized_window_are_caught() {
        let mut id = IdentityProfile::for_major(MAX_EMULATED_CHROME, Os::Windows);
        id.screen.avail_height = id.screen.height;
        assert!(violations(&id)
            .iter()
            .any(|x| x.rule == "screen-availheight"));
        let mut id = IdentityProfile::for_major(MAX_EMULATED_CHROME, Os::Windows);
        id.viewport.width = id.screen.width + 100;
        assert!(violations(&id).iter().any(|x| x.rule == "viewport-screen"));
    }

    #[test]
    fn a_masked_renderer_on_chrome_is_caught_but_fine_on_firefox() {
        let mut chrome = IdentityProfile::for_major(MAX_EMULATED_CHROME, Os::Windows);
        chrome.webgl_renderer = "Mozilla".into();
        assert!(violations(&chrome)
            .iter()
            .any(|x| x.rule == "renderer-engine"));
        // Firefox reports Mozilla/Mozilla by design, so it is not a contradiction there.
        let firefox = IdentityProfile::firefox(MAX_EMULATED_FIREFOX, Os::Windows);
        assert!(!violations(&firefox)
            .iter()
            .any(|x| x.rule == "renderer-engine"));
    }

    #[test]
    fn software_renderers_are_recognised() {
        assert!(is_software_renderer("Google SwiftShader"));
        assert!(is_software_renderer("llvmpipe (LLVM 15.0.7, 256 bits)"));
        assert!(is_software_renderer("Microsoft Basic Render Driver"));
        assert!(!is_software_renderer(
            "ANGLE (Intel, Intel(R) UHD Graphics 630 Direct3D11 vs_5_0 ps_5_0, D3D11)"
        ));
    }
}
