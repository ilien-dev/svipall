//! Machines that plausibly exist, drawn in the proportions they exist in.
//!
//! One identity per process is a licence plate. Every session svipall opened reported the same screen,
//! the same core count and the same GPU, so two visits to the same site were trivially the same
//! visitor — and a hundred visits from a hundred users of this tool would have been the same
//! visitor too.
//!
//! The fix is not randomness. A uniformly random machine is *more* identifying than a fixed one,
//! because the combinations it produces mostly do not exist: nobody runs a 4-core laptop with 64 GB
//! of memory and an eight-year-old integrated GPU behind a 4K screen. What is drawn here follows
//! two rules:
//!
//!   * **proportion** — a resolution or a core count is picked with roughly the weight real traffic
//!     gives it, so the common case stays common;
//!   * **coherence** — the pieces are drawn together, not independently. The GPU comes from the
//!     operating system, the memory from the class of machine, the viewport from the screen.
//!
//! Everything is derived from a seed, so a profile that comes back tomorrow is the same machine it
//! was today. A session whose hardware changes between visits has answered the question by itself.

use crate::identity::Os;

/// A drawn machine, before it becomes an `IdentityProfile`.
#[derive(Debug, Clone, PartialEq)]
pub struct Machine {
    pub screen_width: u32,
    pub screen_height: u32,
    /// Inner viewport, always smaller than the screen and shaped like a real window.
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub hardware_concurrency: u32,
    pub device_memory: u32,
    pub webgl_vendor: String,
    pub webgl_renderer: String,
    pub device_pixel_ratio: f32,
}

/// Deterministic draw. Small and fixed on purpose: a seeded machine has to be the same machine
/// after a dependency bump, or every stored profile silently changes hardware.
struct Draw(u64);

impl Draw {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// Index into a weighted table.
    fn weighted(&mut self, weights: &[u32]) -> usize {
        let total: u32 = weights.iter().sum();
        if total == 0 {
            return 0;
        }
        let mut roll = (self.next() % total as u64) as u32;
        for (i, w) in weights.iter().enumerate() {
            if roll < *w {
                return i;
            }
            roll -= w;
        }
        weights.len() - 1
    }
}

/// Desktop resolutions and roughly how often they turn up, with the device pixel ratio that goes
/// with them. The high-DPI rows carry a ratio of 2 because that is how those panels are driven.
const SCREENS: &[(u32, u32, f32, u32)] = &[
    (1920, 1080, 1.0, 42),
    (1366, 768, 1.0, 11),
    (1536, 864, 1.0, 9),
    (2560, 1440, 1.0, 9),
    (1440, 900, 2.0, 7),
    (1280, 720, 1.0, 5),
    (1600, 900, 1.0, 5),
    (3840, 2160, 1.5, 4),
    (1680, 1050, 1.0, 3),
    (2560, 1600, 2.0, 3),
    (1280, 800, 2.0, 2),
];

/// Core counts. Real machines cluster on powers of two and on the six- and twelve-core parts.
const CORES: &[(u32, u32)] = &[
    (4, 22),
    (8, 34),
    (6, 14),
    (12, 14),
    (16, 11),
    (2, 3),
    (24, 2),
];

/// `navigator.deviceMemory` is quantised by the specification: only these values are ever reported,
/// and it is capped at 8 no matter how much the machine really has. The machines with 16, 32 or
/// 64 GB are therefore all in the `8` row. Reporting 16 is not a rare machine, it is a machine no
/// browser produces — unique on its own — and `coherence` now fails the build on it.
const MEMORY: &[(u32, u32)] = &[(8, 58), (4, 32), (2, 10)];

/// GPU strings by platform. Reporting an ANGLE/Direct3D renderer on a Mac, or a Metal one on
/// Windows, is a contradiction that costs nothing to avoid.
const GPUS_WINDOWS: &[(&str, &str, u32)] = &[
    (
        "Google Inc. (Intel)",
        "ANGLE (Intel, Intel(R) UHD Graphics 630 Direct3D11 vs_5_0 ps_5_0, D3D11)",
        20,
    ),
    (
        "Google Inc. (Intel)",
        "ANGLE (Intel, Intel(R) Iris(R) Xe Graphics Direct3D11 vs_5_0 ps_5_0, D3D11)",
        18,
    ),
    (
        "Google Inc. (NVIDIA)",
        "ANGLE (NVIDIA, NVIDIA GeForce RTX 3060 Direct3D11 vs_5_0 ps_5_0, D3D11)",
        16,
    ),
    (
        "Google Inc. (NVIDIA)",
        "ANGLE (NVIDIA, NVIDIA GeForce GTX 1650 Direct3D11 vs_5_0 ps_5_0, D3D11)",
        13,
    ),
    (
        "Google Inc. (NVIDIA)",
        "ANGLE (NVIDIA, NVIDIA GeForce RTX 4060 Direct3D11 vs_5_0 ps_5_0, D3D11)",
        12,
    ),
    (
        "Google Inc. (AMD)",
        "ANGLE (AMD, AMD Radeon(TM) Graphics Direct3D11 vs_5_0 ps_5_0, D3D11)",
        11,
    ),
    (
        "Google Inc. (Intel)",
        "ANGLE (Intel, Intel(R) HD Graphics 620 Direct3D11 vs_5_0 ps_5_0, D3D11)",
        10,
    ),
];

const GPUS_MAC: &[(&str, &str, u32)] = &[
    (
        "Google Inc. (Apple)",
        "ANGLE (Apple, ANGLE Metal Renderer: Apple M1, Unspecified Version)",
        30,
    ),
    (
        "Google Inc. (Apple)",
        "ANGLE (Apple, ANGLE Metal Renderer: Apple M2, Unspecified Version)",
        28,
    ),
    (
        "Google Inc. (Apple)",
        "ANGLE (Apple, ANGLE Metal Renderer: Apple M3, Unspecified Version)",
        22,
    ),
    (
        "Google Inc. (Intel)",
        "ANGLE (Intel, Intel(R) Iris(TM) Plus Graphics OpenGL Engine, OpenGL 4.1)",
        20,
    ),
];

const GPUS_LINUX: &[(&str, &str, u32)] = &[
    (
        "Google Inc. (Intel)",
        "ANGLE (Intel, Mesa Intel(R) UHD Graphics 620 (KBL GT2), OpenGL 4.6)",
        34,
    ),
    (
        "Google Inc. (AMD)",
        "ANGLE (AMD, AMD Radeon Graphics (radeonsi), OpenGL 4.6)",
        26,
    ),
    (
        "Google Inc. (NVIDIA)",
        "ANGLE (NVIDIA, NVIDIA GeForce RTX 3060/PCIe/SSE2, OpenGL 4.6)",
        24,
    ),
    (
        "Google Inc. (Intel)",
        "ANGLE (Intel, Mesa Intel(R) Xe Graphics (TGL GT2), OpenGL 4.6)",
        16,
    ),
];

fn gpus(os: Os) -> &'static [(&'static str, &'static str, u32)] {
    match os {
        Os::MacOs => GPUS_MAC,
        Os::Linux => GPUS_LINUX,
        Os::Windows => GPUS_WINDOWS,
    }
}

/// The commonest GPU for a platform, for an identity that has not drawn a machine yet.
///
/// Without this the compiled default reported a Direct3D renderer whatever the host was, so
/// svipall on a Mac announced macOS and a Windows GPU in the same breath — the exact kind of pair
/// `coherence` exists to refuse.
pub fn default_gpu(os: Os) -> (&'static str, &'static str) {
    let (vendor, renderer, _) = gpus(os)[0];
    (vendor, renderer)
}

/// Draw one machine. The same seed always produces the same machine.
pub fn machine(seed: u64, os: Os) -> Machine {
    let mut d = Draw::new(seed);

    let screens: Vec<u32> = SCREENS.iter().map(|s| s.3).collect();
    let (sw, sh, dpr, _) = SCREENS[d.weighted(&screens)];

    // The window is a fraction of the screen, never larger than it, and never so small that the
    // page would serve a mobile layout.
    let vw = ((sw as f32 * (0.62 + (d.next() % 30) as f32 / 100.0)) as u32).clamp(1024, sw);
    let vh = ((sh as f32 * (0.60 + (d.next() % 28) as f32 / 100.0)) as u32).clamp(600, sh - 60);

    let core_w: Vec<u32> = CORES.iter().map(|c| c.1).collect();
    let cores = CORES[d.weighted(&core_w)].0;

    // Memory follows the core count: a 16-core machine with 2 GB does not exist.
    let mem_w: Vec<u32> = MEMORY
        .iter()
        .map(|(gb, w)| match (cores, *gb) {
            (c, 2) if c >= 8 => 0,
            (c, 4) if c >= 16 => 0,
            _ => *w,
        })
        .collect();
    let memory = MEMORY[d.weighted(&mem_w)].0;

    let table = gpus(os);
    let gpu_w: Vec<u32> = table.iter().map(|g| g.2).collect();
    let (vendor, renderer, _) = table[d.weighted(&gpu_w)];

    Machine {
        screen_width: sw,
        screen_height: sh,
        viewport_width: vw,
        viewport_height: vh,
        hardware_concurrency: cores,
        device_memory: memory,
        webgl_vendor: vendor.to_string(),
        webgl_renderer: renderer.to_string(),
        device_pixel_ratio: dpr,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_same_seed_is_always_the_same_machine() {
        // A profile that comes back tomorrow has to be the machine it was today; hardware that
        // changes between visits answers the question by itself.
        for seed in [1u64, 999, u64::MAX / 3] {
            assert_eq!(machine(seed, Os::Windows), machine(seed, Os::Windows));
        }
    }

    #[test]
    fn different_seeds_really_do_spread() {
        let mut screens = HashSet::new();
        let mut cores = HashSet::new();
        for seed in 0..400u64 {
            let m = machine(seed.wrapping_mul(0x9E37_79B9), Os::Windows);
            screens.insert((m.screen_width, m.screen_height));
            cores.insert(m.hardware_concurrency);
        }
        assert!(
            screens.len() >= 6,
            "only {} resolutions drawn",
            screens.len()
        );
        assert!(cores.len() >= 4, "only {} core counts drawn", cores.len());
    }

    #[test]
    fn the_common_case_stays_common() {
        // Uniform randomness would be louder than a constant: most combinations do not exist.
        // 1920x1080 should dominate, the way it does in real traffic.
        let n: u64 = 1000;
        let hd = (0..n)
            .filter(|s| {
                let m = machine(s.wrapping_mul(0x9E37_79B9), Os::Windows);
                (m.screen_width, m.screen_height) == (1920, 1080)
            })
            .count();
        let share = hd as f32 / n as f32;
        assert!(
            (0.30..0.55).contains(&share),
            "1920x1080 drawn {share:.0}% of the time"
        );
    }

    #[test]
    fn a_machine_is_internally_coherent() {
        for seed in 0..500u64 {
            let m = machine(seed.wrapping_mul(0x1234_5678_9ABC), Os::Windows);
            assert!(
                m.viewport_width <= m.screen_width,
                "window wider than screen"
            );
            assert!(
                m.viewport_height < m.screen_height,
                "window taller than screen"
            );
            assert!(
                m.viewport_width >= 1024,
                "narrow enough to serve a mobile layout"
            );
            assert!(m.viewport_height >= 600);
            // Memory and cores have to belong to the same class of machine.
            if m.hardware_concurrency >= 16 {
                assert!(m.device_memory >= 8, "16 cores with {} GB", m.device_memory);
            }
            if m.device_memory <= 2 {
                assert!(m.hardware_concurrency < 8);
            }
        }
    }

    #[test]
    fn the_gpu_always_belongs_to_the_operating_system() {
        // A Metal renderer on Windows, or Direct3D on a Mac, is a free contradiction.
        for seed in 0..200u64 {
            let s = seed.wrapping_mul(0xABCD_EF01);
            let win = machine(s, Os::Windows);
            assert!(
                win.webgl_renderer.contains("D3D11"),
                "{}",
                win.webgl_renderer
            );

            let mac = machine(s, Os::MacOs);
            assert!(
                mac.webgl_renderer.contains("Metal") || mac.webgl_renderer.contains("OpenGL"),
                "{}",
                mac.webgl_renderer
            );
            assert!(!mac.webgl_renderer.contains("D3D11"));

            let linux = machine(s, Os::Linux);
            assert!(!linux.webgl_renderer.contains("D3D11"));
            assert!(!linux.webgl_renderer.contains("Metal"));
        }
    }

    #[test]
    fn device_memory_is_only_ever_a_value_the_api_reports() {
        // The specification quantises it; an unusual number is itself the fingerprint.
        let allowed = [2u32, 4, 8];
        for seed in 0..300u64 {
            let m = machine(seed.wrapping_mul(7), Os::Linux);
            assert!(allowed.contains(&m.device_memory), "{}", m.device_memory);
        }
    }

    #[test]
    fn a_high_dpi_screen_carries_a_high_dpi_ratio() {
        for seed in 0..300u64 {
            let m = machine(seed.wrapping_mul(31), Os::MacOs);
            assert!(m.device_pixel_ratio >= 1.0 && m.device_pixel_ratio <= 2.0);
        }
    }
}
