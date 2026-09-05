use chromiumoxide_cdp::cdp::browser_protocol::emulation::{
    ScreenOrientation, ScreenOrientationType, SetDeviceMetricsOverrideParams,
    SetTouchEmulationEnabledParams,
};
use chromiumoxide_types::Method;

use crate::cmd::CommandChain;
use crate::handler::viewport::Viewport;
use std::time::Duration;

#[derive(Debug)]
pub struct EmulationManager {
    pub emulating_mobile: bool,
    pub has_touch: bool,
    pub needs_reload: bool,
    pub request_timeout: Duration,
}

impl EmulationManager {
    pub fn new(request_timeout: Duration) -> Self {
        Self {
            emulating_mobile: false,
            has_touch: false,
            needs_reload: false,
            request_timeout,
        }
    }

    pub fn init_commands(&mut self, viewport: &Viewport) -> CommandChain {
        let orientation = if viewport.is_landscape {
            ScreenOrientation::new(ScreenOrientationType::LandscapePrimary, 90)
        } else {
            ScreenOrientation::new(ScreenOrientationType::PortraitPrimary, 0)
        };

        let mut set_device = SetDeviceMetricsOverrideParams::builder()
            .mobile(viewport.emulating_mobile)
            .width(viewport.width)
            .height(viewport.height)
            .device_scale_factor(viewport.device_scale_factor.unwrap_or(1.))
            .screen_orientation(orientation);
        // The display the window claims to sit on, and where on it. Both are page-readable
        // (`screen.width`, `window.screenX`) and both were left to whatever headless or the window
        // manager happened to report. See PATCHES.md §7.
        if let Some((w, h)) = viewport.screen {
            set_device = set_device.screen_width(w).screen_height(h);
        }
        if let Some((x, y)) = viewport.position {
            set_device = set_device.position_x(x).position_y(y);
        }
        let set_device = set_device.build().unwrap();

        // Upstream hard-coded `true`, which put `ontouchstart` on `window` and a non-zero
        // `maxTouchPoints` on a desktop identity that declares no touch screen — a contradiction a
        // page reads in one line. The viewport already carries the answer. See PATCHES.md §5.
        let set_touch = SetTouchEmulationEnabledParams::new(viewport.has_touch);

        let chain = CommandChain::new(
            vec![
                (
                    set_device.identifier(),
                    serde_json::to_value(set_device).unwrap(),
                ),
                (
                    set_touch.identifier(),
                    serde_json::to_value(set_touch).unwrap(),
                ),
            ],
            self.request_timeout,
        );

        self.needs_reload = self.emulating_mobile != viewport.emulating_mobile
            || self.has_touch != viewport.has_touch;
        chain
    }
}
