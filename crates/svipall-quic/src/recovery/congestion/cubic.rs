// Copyright (C) 2019, Cloudflare, Inc.
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

//! CUBIC Congestion Control
//!
//! This implementation is based on the following draft:
//! <https://tools.ietf.org/html/draft-ietf-tcpm-rfc8312bis-02>
//!
//! Note that Slow Start can use HyStart++ when enabled.

use std::cmp;

use std::time::Duration;
use std::time::Instant;

use super::rtt::RttStats;
use super::Acked;
use super::Sent;

use super::reno;
use super::Congestion;
use super::CongestionControlOps;
use crate::recovery::MINIMUM_WINDOW_PACKETS;

pub(crate) static CUBIC: CongestionControlOps = CongestionControlOps {
    on_init,
    on_packet_sent,
    on_packets_acked,
    congestion_event,
    checkpoint,
    rollback,
    #[cfg(feature = "qlog")]
    state_str,
    debug_fmt,
};

/// CUBIC Constants.
///
/// These are recommended value in RFC8312.
const BETA_CUBIC: f64 = 0.7;

const C: f64 = 0.4;

/// Threshold for rolling back state, as percentage of lost packets relative to
/// cwnd.
const ROLLBACK_THRESHOLD_PERCENT: usize = 20;

/// Minimum threshold for rolling back state, as number of packets.
const MIN_ROLLBACK_THRESHOLD: usize = 2;

/// Default value of alpha_aimd in the beginning of congestion avoidance.
const ALPHA_AIMD: f64 = 3.0 * (1.0 - BETA_CUBIC) / (1.0 + BETA_CUBIC);

/// CUBIC State Variables.
///
/// We need to keep those variables across the connection.
/// k, w_max, w_est are described in the RFC.
#[derive(Debug, Default)]
pub struct State {
    k: f64,

    w_max: f64,

    w_est: f64,

    alpha_aimd: f64,

    // Used in CUBIC fix (see on_packet_sent())
    last_sent_time: Option<Instant>,

    // Store cwnd increment during congestion avoidance.
    cwnd_inc: usize,

    // CUBIC state checkpoint preceding the last congestion event.
    prior: PriorState,
}

/// Stores the CUBIC state from before the last congestion event.
///
/// <https://tools.ietf.org/id/draft-ietf-tcpm-rfc8312bis-00.html#section-4.9>
#[derive(Debug, Default)]
struct PriorState {
    congestion_window: usize,

    ssthresh: usize,

    w_max: f64,

    k: f64,

    epoch_start: Option<Instant>,

    lost_count: usize,
}

/// CUBIC Functions.
///
/// Note that these calculations are based on a count of cwnd as bytes,
/// not packets.
/// Unit of t (duration) and RTT are based on seconds (f64).
impl State {
    // K = cubic_root ((w_max - cwnd) / C) (Eq. 2)
    fn cubic_k(&self, cwnd: usize, max_datagram_size: usize) -> f64 {
        let w_max = self.w_max / max_datagram_size as f64;
        let cwnd = cwnd as f64 / max_datagram_size as f64;

        libm::cbrt((w_max - cwnd) / C)
    }

    // W_cubic(t) = C * (t - K)^3 + w_max (Eq. 1)
    fn w_cubic(&self, t: Duration, max_datagram_size: usize) -> f64 {
        let w_max = self.w_max / max_datagram_size as f64;

        (C * (t.as_secs_f64() - self.k).powi(3) + w_max) * max_datagram_size as f64
    }

    // W_est = W_est + alpha_aimd * (segments_acked / cwnd)  (Eq. 4)
    fn w_est_inc(&self, acked: usize, cwnd: usize, max_datagram_size: usize) -> f64 {
        self.alpha_aimd * (acked as f64 / cwnd as f64) * max_datagram_size as f64
    }
}

fn on_init(_r: &mut Congestion) {}

fn on_packet_sent(r: &mut Congestion, sent_bytes: usize, bytes_in_flight: usize, now: Instant) {
    // See https://github.com/torvalds/linux/commit/30927520dbae297182990bb21d08762bcc35ce1d
    // First transmit when no packets in flight
    let cubic = &mut r.cubic_state;

    if let Some(last_sent_time) = cubic.last_sent_time {
        if bytes_in_flight == 0 {
            let delta = now - last_sent_time;

            // We were application limited (idle) for a while.
            // Shift epoch start to keep cwnd growth to cubic curve.
            if let Some(recovery_start_time) = r.congestion_recovery_start_time {
                if delta.as_nanos() > 0 {
                    r.congestion_recovery_start_time = Some(recovery_start_time + delta);
                }
            }
        }
    }

    cubic.last_sent_time = Some(now);

    reno::on_packet_sent(r, sent_bytes, bytes_in_flight, now);
}

fn on_packets_acked(
    r: &mut Congestion,
    bytes_in_flight: usize,
    packets: &mut Vec<Acked>,
    now: Instant,
    rtt_stats: &RttStats,
) {
    for pkt in packets.drain(..) {
        on_packet_acked(r, bytes_in_flight, &pkt, now, rtt_stats);
    }
}

fn on_packet_acked(
    r: &mut Congestion,
    bytes_in_flight: usize,
    packet: &Acked,
    now: Instant,
    rtt_stats: &RttStats,
) {
    let in_congestion_recovery = r.in_congestion_recovery(packet.time_sent);

    if in_congestion_recovery {
        r.prr.on_packet_acked(
            packet.size,
            bytes_in_flight,
            r.ssthresh.get(),
            r.max_datagram_size,
        );

        return;
    }

    if r.app_limited {
        return;
    }

    // Detecting spurious congestion events.
    // <https://tools.ietf.org/id/draft-ietf-tcpm-rfc8312bis-00.html#section-4.9>
    //
    // When the recovery episode ends with recovering
    // a few packets (less than cwnd / mss * ROLLBACK_THRESHOLD_PERCENT(%)), it's
    // considered as spurious and restore to the previous state.
    if r.congestion_recovery_start_time.is_some() {
        let new_lost = r.lost_count - r.cubic_state.prior.lost_count;

        let rollback_threshold =
            (r.congestion_window / r.max_datagram_size) * ROLLBACK_THRESHOLD_PERCENT / 100;

        let rollback_threshold = rollback_threshold.max(MIN_ROLLBACK_THRESHOLD);

        if new_lost < rollback_threshold {
            let did_rollback = rollback(r);
            if did_rollback {
                return;
            }
        }
    }

    if r.congestion_window < r.ssthresh.get() {
        // In Slow start, bytes_acked_sl is used for counting
        // acknowledged bytes.
        r.bytes_acked_sl += packet.size;

        if r.bytes_acked_sl >= r.max_datagram_size {
            if r.hystart.in_css() {
                r.congestion_window += r.hystart.css_cwnd_inc(r.max_datagram_size);
            } else {
                r.congestion_window += r.max_datagram_size;
            }

            r.bytes_acked_sl -= r.max_datagram_size;
        }

        if r.hystart.on_packet_acked(packet, rtt_stats.latest_rtt, now) {
            // Exit to congestion avoidance if CSS ends.
            r.ssthresh.update(r.congestion_window, true);
        }
    } else {
        // Congestion avoidance.
        let ca_start_time;

        // In CSS, use css_start_time instead of congestion_recovery_start_time.
        if r.hystart.in_css() {
            ca_start_time = r.hystart.css_start_time().unwrap();

            // Reset w_max and k when CSS started.
            if r.cubic_state.w_max == 0.0 {
                r.cubic_state.w_max = r.congestion_window as f64;
                r.cubic_state.k = 0.0;

                r.cubic_state.w_est = r.congestion_window as f64;
                r.cubic_state.alpha_aimd = ALPHA_AIMD;
            }
        } else {
            match r.congestion_recovery_start_time {
                Some(t) => ca_start_time = t,
                None => {
                    // When we come here without congestion_event() triggered,
                    // initialize congestion_recovery_start_time, w_max and k.
                    ca_start_time = now;
                    r.congestion_recovery_start_time = Some(now);

                    r.cubic_state.w_max = r.congestion_window as f64;
                    r.cubic_state.k = 0.0;

                    r.cubic_state.w_est = r.congestion_window as f64;
                    r.cubic_state.alpha_aimd = ALPHA_AIMD;
                }
            }
        }

        let t = now.saturating_duration_since(ca_start_time);

        // target = w_cubic(t + rtt)
        let target = r
            .cubic_state
            .w_cubic(t + *rtt_stats.min_rtt, r.max_datagram_size);

        // Clipping target to [cwnd, 1.5 x cwnd]
        let target = f64::max(target, r.congestion_window as f64);
        let target = f64::min(target, r.congestion_window as f64 * 1.5);

        // Update w_est.
        let w_est_inc =
            r.cubic_state
                .w_est_inc(packet.size, r.congestion_window, r.max_datagram_size);
        r.cubic_state.w_est += w_est_inc;

        if r.cubic_state.w_est >= r.cubic_state.w_max {
            r.cubic_state.alpha_aimd = 1.0;
        }

        let mut cubic_cwnd = r.congestion_window;

        if r.cubic_state.w_cubic(t, r.max_datagram_size) < r.cubic_state.w_est {
            // AIMD friendly region (W_cubic(t) < W_est)
            cubic_cwnd = cmp::max(cubic_cwnd, r.cubic_state.w_est as usize);
        } else {
            // Concave region or convex region use same increment.
            let cubic_inc = r.max_datagram_size * (target as usize - cubic_cwnd) / cubic_cwnd;

            cubic_cwnd += cubic_inc;
        }

        // Update the increment and increase cwnd by MSS.
        r.cubic_state.cwnd_inc += cubic_cwnd - r.congestion_window;

        if r.cubic_state.cwnd_inc >= r.max_datagram_size {
            r.congestion_window += r.max_datagram_size;
            r.cubic_state.cwnd_inc -= r.max_datagram_size;
        }
    }
}

fn congestion_event(
    r: &mut Congestion,
    bytes_in_flight: usize,
    _lost_bytes: usize,
    largest_lost_pkt: &Sent,
    now: Instant,
) {
    let time_sent = largest_lost_pkt.time_sent;
    let in_congestion_recovery = r.in_congestion_recovery(time_sent);

    // Start a new congestion event if packet was sent after the
    // start of the previous congestion recovery period.
    if !in_congestion_recovery {
        r.congestion_recovery_start_time = Some(now);

        // Fast convergence
        if (r.congestion_window as f64) < r.cubic_state.w_max {
            r.cubic_state.w_max = r.congestion_window as f64 * (1.0 + BETA_CUBIC) / 2.0;
        } else {
            r.cubic_state.w_max = r.congestion_window as f64;
        }

        let ssthresh = (r.congestion_window as f64 * BETA_CUBIC) as usize;
        let ssthresh = cmp::max(ssthresh, r.max_datagram_size * MINIMUM_WINDOW_PACKETS);
        r.ssthresh.update(ssthresh, r.hystart.in_css());
        r.congestion_window = ssthresh;

        r.cubic_state.k = if r.cubic_state.w_max < r.congestion_window as f64 {
            0.0
        } else {
            r.cubic_state
                .cubic_k(r.congestion_window, r.max_datagram_size)
        };

        r.cubic_state.cwnd_inc = (r.cubic_state.cwnd_inc as f64 * BETA_CUBIC) as usize;

        r.cubic_state.w_est = r.congestion_window as f64;
        r.cubic_state.alpha_aimd = ALPHA_AIMD;

        if r.hystart.in_css() {
            r.hystart.congestion_event();
        }

        r.prr.congestion_event(bytes_in_flight);
    }
}

fn checkpoint(r: &mut Congestion) {
    r.cubic_state.prior.congestion_window = r.congestion_window;
    r.cubic_state.prior.ssthresh = r.ssthresh.get();
    r.cubic_state.prior.w_max = r.cubic_state.w_max;
    r.cubic_state.prior.k = r.cubic_state.k;
    r.cubic_state.prior.epoch_start = r.congestion_recovery_start_time;
    r.cubic_state.prior.lost_count = r.lost_count;
}

fn rollback(r: &mut Congestion) -> bool {
    // Don't go back to slow start.
    if r.cubic_state.prior.congestion_window < r.cubic_state.prior.ssthresh {
        return false;
    }

    if r.congestion_window >= r.cubic_state.prior.congestion_window {
        return false;
    }

    r.congestion_window = r.cubic_state.prior.congestion_window;
    r.ssthresh.update(r.cubic_state.prior.ssthresh, false);
    r.cubic_state.w_max = r.cubic_state.prior.w_max;
    r.cubic_state.k = r.cubic_state.prior.k;
    r.congestion_recovery_start_time = r.cubic_state.prior.epoch_start;

    true
}

#[cfg(feature = "qlog")]
fn state_str(r: &Congestion, now: Instant) -> &'static str {
    reno::state_str(r, now)
}

fn debug_fmt(r: &Congestion, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    write!(
        f,
        "cubic={{ k={} w_max={} }} ",
        r.cubic_state.k, r.cubic_state.w_max
    )
}
