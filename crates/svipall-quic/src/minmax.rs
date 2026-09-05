// Copyright (C) 2020, Cloudflare, Inc.
// Copyright (C) 2017, Google, Inc.
//
// Use of this source code is governed by the following BSD-style license:
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are
// met:
//
//    * Redistributions of source code must retain the above copyright
// notice, this list of conditions and the following disclaimer.
//    * Redistributions in binary form must reproduce the above
// copyright notice, this list of conditions and the following disclaimer
// in the documentation and/or other materials provided with the
// distribution.
//
//    * Neither the name of Google Inc. nor the names of its
// contributors may be used to endorse or promote products derived from
// this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
// A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
// OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
// LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
// DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
// THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

// lib/minmax.c: windowed min/max tracker
//
// Kathleen Nichols' algorithm for tracking the minimum (or maximum)
// value of a data stream over some fixed time interval.  (E.g.,
// the minimum RTT over the past five minutes.) It uses constant
// space and constant time per update yet almost always delivers
// the same minimum as an implementation that has to keep all the
// data in the window.
//
// The algorithm keeps track of the best, 2nd best & 3rd best min
// values, maintaining an invariant that the measurement time of
// the n'th best >= n-1'th best. It also makes sure that the three
// values are widely separated in the time window since that bounds
// the worse case error when that data is monotonically increasing
// over the window.
//
// Upon getting a new min, we can forget everything earlier because
// it has no value - the new min is <= everything else in the window
// by definition and it's the most recent. So we restart fresh on
// every new min and overwrites 2nd & 3rd choices. The same property
// holds for 2nd & 3rd best.

use std::ops::Deref;

use std::time::Duration;
use std::time::Instant;

#[derive(Copy, Clone)]
struct MinmaxSample<T> {
    time: Instant,
    value: T,
}

pub struct Minmax<T> {
    estimate: [MinmaxSample<T>; 3],
}

impl<T> Deref for Minmax<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.estimate[0].value
    }
}

impl<T: PartialOrd + Copy> Minmax<T> {
    pub fn new(val: T) -> Self {
        Minmax {
            estimate: [MinmaxSample {
                time: Instant::now(),
                value: val,
            }; 3],
        }
    }

    /// Resets the estimates to the given value.
    pub fn reset(&mut self, time: Instant, meas: T) -> T {
        let val = MinmaxSample { time, value: meas };

        for i in self.estimate.iter_mut() {
            *i = val;
        }

        self.estimate[0].value
    }

    /// Updates the min estimate based on the given measurement, and returns it.
    pub fn running_min(&mut self, win: Duration, time: Instant, meas: T) -> T {
        let val = MinmaxSample { time, value: meas };

        let delta_time = time.duration_since(self.estimate[2].time);

        // Reset if there's nothing in the window or a new min value is found.
        if val.value <= self.estimate[0].value || delta_time > win {
            return self.reset(time, meas);
        }

        if val.value <= self.estimate[1].value {
            self.estimate[2] = val;
            self.estimate[1] = val;
        } else if val.value <= self.estimate[2].value {
            self.estimate[2] = val;
        }

        self.subwin_update(win, time, meas)
    }

    /// Updates the max estimate based on the given measurement, and returns it.
    #[cfg(test)]
    pub fn running_max(&mut self, win: Duration, time: Instant, meas: T) -> T {
        let val = MinmaxSample { time, value: meas };

        let delta_time = time.duration_since(self.estimate[2].time);

        // Reset if there's nothing in the window or a new max value is found.
        if val.value >= self.estimate[0].value || delta_time > win {
            return self.reset(time, meas);
        }

        if val.value >= self.estimate[1].value {
            self.estimate[2] = val;
            self.estimate[1] = val;
        } else if val.value >= self.estimate[2].value {
            self.estimate[2] = val
        }

        self.subwin_update(win, time, meas)
    }

    /// As time advances, update the 1st, 2nd and 3rd estimates.
    fn subwin_update(&mut self, win: Duration, time: Instant, meas: T) -> T {
        let val = MinmaxSample { time, value: meas };

        let delta_time = time.duration_since(self.estimate[0].time);

        if delta_time > win {
            // Passed entire window without a new val so make 2nd estimate the
            // new val & 3rd estimate the new 2nd choice. we may have to iterate
            // this since our 2nd estimate may also be outside the window (we
            // checked on entry that the third estimate was in the window).
            self.estimate[0] = self.estimate[1];
            self.estimate[1] = self.estimate[2];
            self.estimate[2] = val;

            if time.duration_since(self.estimate[0].time) > win {
                self.estimate[0] = self.estimate[1];
                self.estimate[1] = self.estimate[2];
                self.estimate[2] = val;
            }
        } else if self.estimate[1].time == self.estimate[0].time && delta_time > win.div_f32(4.0) {
            // We've passed a quarter of the window without a new val so take a
            // 2nd estimate from the 2nd quarter of the window.
            self.estimate[2] = val;
            self.estimate[1] = val;
        } else if self.estimate[2].time == self.estimate[1].time && delta_time > win.div_f32(2.0) {
            // We've passed half the window without finding a new val so take a
            // 3rd estimate from the last half of the window.
            self.estimate[2] = val;
        }

        self.estimate[0].value
    }
}
