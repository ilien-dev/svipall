#[derive(Debug, Clone)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub device_scale_factor: Option<f64>,
    pub emulating_mobile: bool,
    pub is_landscape: bool,
    pub has_touch: bool,
    /// The display the window claims to sit on, and where on it the window sits.
    ///
    /// Upstream sent neither, so `screen.width`/`screen.height` stayed at the 800x600 headless
    /// default while the launch flags sized the window past it — a window wider than the display
    /// holding it, which no machine produces — and `window.screenX` reported wherever the process
    /// actually parked the window, including far off-screen. Both are page-readable, and both are
    /// fields the protocol already carries. See PATCHES.md §7.
    pub screen: Option<(u32, u32)>,
    pub position: Option<(u32, u32)>,
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport {
            width: 800,
            height: 600,
            device_scale_factor: None,
            emulating_mobile: false,
            is_landscape: false,
            has_touch: false,
            screen: None,
            position: None,
        }
    }
}
