// Copyright (C) 2021, Cloudflare, Inc.
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are
// met:
//
//     * Redistributions of source code must retain the above copyright notice,
//       this list of conditions and the following disclaimer.
//
//     * Redistributions in binary form must reproduce the above copyright
//       notice, this list of conditions and the following disclaimer in the
//       documentation and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS
// IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO,
// THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
// PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
// CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
// EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
// PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
// PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
// LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
// NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
// SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::time::Duration;
use std::time::Instant;

// When autotuning the receiver window, decide how much
// we increase the window.
const WINDOW_INCREASE_FACTOR: u64 = 2;

// When autotuning the receiver window, check if the last
// update is within RTT * this constant.
const WINDOW_TRIGGER_FACTOR: u32 = 2;

#[derive(Default, Debug)]
pub struct FlowControl {
    /// Total consumed bytes by the receiver.
    consumed: u64,

    /// Flow control limit.
    max_data: u64,

    /// The receive window. This value is used for updating
    /// flow control limit.
    window: u64,

    /// The maximum receive window.
    max_window: u64,

    /// Last update time of max_data for autotuning the window.
    last_update: Option<Instant>,
}

impl FlowControl {
    pub fn new(max_data: u64, window: u64, max_window: u64) -> Self {
        Self {
            max_data,

            window,

            max_window,

            ..Default::default()
        }
    }

    /// Returns the current window size.
    pub fn window(&self) -> u64 {
        self.window
    }

    /// Returns the current flow limit.
    pub fn max_data(&self) -> u64 {
        self.max_data
    }

    /// Returns the consumed bytes by the receiver.
    #[cfg(test)]
    pub fn consumed(&self) -> u64 {
        self.consumed
    }

    /// Update consumed bytes.
    pub fn add_consumed(&mut self, consumed: u64) {
        self.consumed += consumed;
    }

    /// Returns true if the flow control needs to update max_data.
    ///
    /// This happens when the available window is smaller than the half
    /// of the current window.
    pub fn should_update_max_data(&self) -> bool {
        let available_window = self.max_data - self.consumed;

        available_window < (self.window / 2)
    }

    /// Returns the new max_data limit.
    pub fn max_data_next(&self) -> u64 {
        self.consumed + self.window
    }

    /// Commits the new max_data limit.
    pub fn update_max_data(&mut self, now: Instant) {
        self.max_data = self.max_data_next();
        self.last_update = Some(now);
    }

    /// Autotune the window size. When there is an another update
    /// within RTT x 2, bump the window x 2, capped by
    /// max_window.
    pub fn autotune_window(&mut self, now: Instant, rtt: Duration) {
        if let Some(last_update) = self.last_update {
            if now - last_update < rtt * WINDOW_TRIGGER_FACTOR {
                self.window = std::cmp::min(self.window * WINDOW_INCREASE_FACTOR, self.max_window);
            }
        }
    }

    /// Make sure the lower bound of the window is same to
    /// the current window.
    pub fn ensure_window_lower_bound(&mut self, min_window: u64) {
        if min_window > self.window {
            self.window = min_window;
        }
    }
}
