// Copyright (C) 2018-2019, Cloudflare, Inc.
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

use std::str::FromStr;
use std::time::Duration;
use std::time::Instant;

use crate::frame;
use crate::packet;
use crate::ranges::RangeSet;
pub(crate) use crate::recovery::bandwidth::Bandwidth;
use crate::Config;
use crate::Result;

#[cfg(feature = "qlog")]
use qlog::events::EventData;

use smallvec::SmallVec;

use self::congestion::recovery::LegacyRecovery;
use self::gcongestion::GRecovery;
pub use gcongestion::BbrBwLoReductionStrategy;
pub use gcongestion::BbrParams;

// Loss Recovery
const INITIAL_PACKET_THRESHOLD: u64 = 3;

const MAX_PACKET_THRESHOLD: u64 = 20;

// Time threshold used to calculate the loss time.
//
// https://www.rfc-editor.org/rfc/rfc9002.html#section-6.1.2
const INITIAL_TIME_THRESHOLD: f64 = 9.0 / 8.0;

// Reduce the sensitivity to packet reordering after the first reordering event.
//
// Packet reorder is not a real loss event so quickly reduce the sensitivity to
// avoid penializing subsequent packet reordering.
//
// https://www.rfc-editor.org/rfc/rfc9002.html#section-6.1.2
//
// Implementations MAY experiment with absolute thresholds, thresholds from
// previous connections, adaptive thresholds, or the including of RTT variation.
// Smaller thresholds reduce reordering resilience and increase spurious
// retransmissions, and larger thresholds increase loss detection delay.
const PACKET_REORDER_TIME_THRESHOLD: f64 = 5.0 / 4.0;

// # Experiment: enable_relaxed_loss_threshold
//
// Time threshold overhead used to calculate the loss time.
//
// The actual threshold is calcualted as 1 + INITIAL_TIME_THRESHOLD_OVERHEAD and
// equivalent to INITIAL_TIME_THRESHOLD.
const INITIAL_TIME_THRESHOLD_OVERHEAD: f64 = 1.0 / 8.0;
// # Experiment: enable_relaxed_loss_threshold
//
// The factor by which to increase the time threshold on spurious loss.
const TIME_THRESHOLD_OVERHEAD_MULTIPLIER: f64 = 2.0;

const GRANULARITY: Duration = Duration::from_millis(1);

const MAX_PTO_PROBES_COUNT: usize = 2;

const MINIMUM_WINDOW_PACKETS: usize = 2;

const LOSS_REDUCTION_FACTOR: f64 = 0.5;

// How many non ACK eliciting packets we send before including a PING to solicit
// an ACK.
pub(super) const MAX_OUTSTANDING_NON_ACK_ELICITING: usize = 24;

#[derive(Default)]
struct LossDetectionTimer {
    time: Option<Instant>,
}

impl LossDetectionTimer {
    fn update(&mut self, timeout: Instant) {
        self.time = Some(timeout);
    }

    fn clear(&mut self) {
        self.time = None;
    }
}

impl std::fmt::Debug for LossDetectionTimer {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self.time {
            Some(v) => {
                let now = Instant::now();
                if v > now {
                    let d = v.duration_since(now);
                    write!(f, "{d:?}")
                } else {
                    write!(f, "exp")
                }
            }
            None => write!(f, "none"),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct RecoveryConfig {
    pub initial_rtt: Duration,
    pub max_send_udp_payload_size: usize,
    pub max_ack_delay: Duration,
    pub cc_algorithm: CongestionControlAlgorithm,
    pub custom_bbr_params: Option<BbrParams>,
    pub hystart: bool,
    pub pacing: bool,
    pub max_pacing_rate: Option<u64>,
    pub initial_congestion_window_packets: usize,
    pub enable_relaxed_loss_threshold: bool,
}

impl RecoveryConfig {
    pub fn from_config(config: &Config) -> Self {
        Self {
            initial_rtt: config.initial_rtt,
            max_send_udp_payload_size: config.max_send_udp_payload_size,
            max_ack_delay: Duration::ZERO,
            cc_algorithm: config.cc_algorithm,
            custom_bbr_params: config.custom_bbr_params,
            hystart: config.hystart,
            pacing: config.pacing,
            max_pacing_rate: config.max_pacing_rate,
            initial_congestion_window_packets: config.initial_congestion_window_packets,
            enable_relaxed_loss_threshold: config.enable_relaxed_loss_threshold,
        }
    }
}

#[enum_dispatch::enum_dispatch(RecoveryOps)]
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub(crate) enum Recovery {
    Legacy(LegacyRecovery),
    GCongestion(GRecovery),
}

#[derive(Debug, Default, PartialEq)]
pub struct OnAckReceivedOutcome {
    pub lost_packets: usize,
    pub lost_bytes: usize,
    pub acked_bytes: usize,
    pub spurious_losses: usize,
}

#[derive(Debug, Default)]
pub struct OnLossDetectionTimeoutOutcome {
    pub lost_packets: usize,
    pub lost_bytes: usize,
}

#[enum_dispatch::enum_dispatch]
/// Api for the Recovery implementation
pub trait RecoveryOps {
    fn lost_count(&self) -> usize;
    fn bytes_lost(&self) -> u64;

    /// Returns whether or not we should elicit an ACK even if we wouldn't
    /// otherwise have constructed an ACK eliciting packet.
    fn should_elicit_ack(&self, epoch: packet::Epoch) -> bool;

    fn next_acked_frame(&mut self, epoch: packet::Epoch) -> Option<frame::Frame>;

    fn next_lost_frame(&mut self, epoch: packet::Epoch) -> Option<frame::Frame>;

    fn get_largest_acked_on_epoch(&self, epoch: packet::Epoch) -> Option<u64>;
    fn has_lost_frames(&self, epoch: packet::Epoch) -> bool;
    fn loss_probes(&self, epoch: packet::Epoch) -> usize;
    #[cfg(test)]
    fn inc_loss_probes(&mut self, epoch: packet::Epoch);

    fn ping_sent(&mut self, epoch: packet::Epoch);

    fn on_packet_sent(
        &mut self,
        pkt: Sent,
        epoch: packet::Epoch,
        handshake_status: HandshakeStatus,
        now: Instant,
        trace_id: &str,
    );
    fn get_packet_send_time(&self, now: Instant) -> Instant;

    #[allow(clippy::too_many_arguments)]
    fn on_ack_received(
        &mut self,
        ranges: &RangeSet,
        ack_delay: u64,
        epoch: packet::Epoch,
        handshake_status: HandshakeStatus,
        now: Instant,
        skip_pn: Option<u64>,
        trace_id: &str,
    ) -> Result<OnAckReceivedOutcome>;

    fn on_loss_detection_timeout(
        &mut self,
        handshake_status: HandshakeStatus,
        now: Instant,
        trace_id: &str,
    ) -> OnLossDetectionTimeoutOutcome;
    fn on_pkt_num_space_discarded(
        &mut self,
        epoch: packet::Epoch,
        handshake_status: HandshakeStatus,
        now: Instant,
    );
    fn on_path_change(
        &mut self,
        epoch: packet::Epoch,
        now: Instant,
        _trace_id: &str,
    ) -> (usize, usize);
    fn loss_detection_timer(&self) -> Option<Instant>;
    fn cwnd(&self) -> usize;
    fn cwnd_available(&self) -> usize;
    fn rtt(&self) -> Duration;

    fn min_rtt(&self) -> Option<Duration>;

    fn max_rtt(&self) -> Option<Duration>;

    fn rttvar(&self) -> Duration;

    fn pto(&self) -> Duration;

    /// The most recent data delivery rate estimate.
    fn delivery_rate(&self) -> Bandwidth;

    /// Maximum bandwidth estimate, if one is available.
    fn max_bandwidth(&self) -> Option<Bandwidth>;

    /// Statistics from when a CCA first exited the startup phase.
    fn startup_exit(&self) -> Option<StartupExit>;

    fn max_datagram_size(&self) -> usize;

    fn pmtud_update_max_datagram_size(&mut self, new_max_datagram_size: usize);

    fn update_max_datagram_size(&mut self, new_max_datagram_size: usize);

    fn on_app_limited(&mut self);

    // Since a recovery module is path specific, this tracks the largest packet
    // sent per path.
    #[cfg(test)]
    fn largest_sent_pkt_num_on_path(&self, epoch: packet::Epoch) -> Option<u64>;

    #[cfg(test)]
    fn app_limited(&self) -> bool;

    #[cfg(test)]
    fn sent_packets_len(&self, epoch: packet::Epoch) -> usize;

    fn bytes_in_flight(&self) -> usize;

    fn bytes_in_flight_duration(&self) -> Duration;

    #[cfg(test)]
    fn in_flight_count(&self, epoch: packet::Epoch) -> usize;

    #[cfg(test)]
    fn pacing_rate(&self) -> u64;

    #[cfg(test)]
    fn pto_count(&self) -> u32;

    // This value might be `None` when experiment `enable_relaxed_loss_threshold`
    // is enabled for gcongestion
    #[cfg(test)]
    fn pkt_thresh(&self) -> Option<u64>;

    #[cfg(test)]
    fn time_thresh(&self) -> f64;

    #[cfg(test)]
    fn lost_spurious_count(&self) -> usize;

    #[cfg(test)]
    fn detect_lost_packets_for_test(
        &mut self,
        epoch: packet::Epoch,
        now: Instant,
    ) -> (usize, usize);

    fn update_app_limited(&mut self, v: bool);

    fn delivery_rate_update_app_limited(&mut self, v: bool);

    fn update_max_ack_delay(&mut self, max_ack_delay: Duration);

    #[cfg(feature = "qlog")]
    fn state_str(&self, now: Instant) -> &'static str;

    #[cfg(feature = "qlog")]
    fn get_updated_qlog_event_data(&mut self) -> Option<EventData>;

    #[cfg(feature = "qlog")]
    fn get_updated_qlog_cc_state(&mut self, now: Instant) -> Option<&'static str>;

    fn send_quantum(&self) -> usize;

    fn get_next_release_time(&self) -> ReleaseDecision;

    fn gcongestion_enabled(&self) -> bool;
}

impl Recovery {
    pub fn new_with_config(recovery_config: &RecoveryConfig) -> Self {
        let grecovery = GRecovery::new(recovery_config);
        if let Some(grecovery) = grecovery {
            Recovery::from(grecovery)
        } else {
            Recovery::from(LegacyRecovery::new_with_config(recovery_config))
        }
    }

    #[cfg(feature = "qlog")]
    pub fn maybe_qlog(&mut self, qlog: &mut qlog::streamer::QlogStreamer, now: Instant) {
        if let Some(ev_data) = self.get_updated_qlog_event_data() {
            qlog.add_event_data_with_instant(ev_data, now).ok();
        }

        if let Some(cc_state) = self.get_updated_qlog_cc_state(now) {
            let ev_data =
                EventData::CongestionStateUpdated(qlog::events::quic::CongestionStateUpdated {
                    old: None,
                    new: cc_state.to_string(),
                    trigger: None,
                });

            qlog.add_event_data_with_instant(ev_data, now).ok();
        }
    }

    #[cfg(test)]
    pub fn new(config: &Config) -> Self {
        Self::new_with_config(&RecoveryConfig::from_config(config))
    }
}

/// Available congestion control algorithms.
///
/// This enum provides currently available list of congestion control
/// algorithms.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub enum CongestionControlAlgorithm {
    /// Reno congestion control algorithm. `reno` in a string form.
    Reno = 0,
    /// CUBIC congestion control algorithm (default). `cubic` in a string form.
    CUBIC = 1,
    /// BBRv2 congestion control algorithm implementation from gcongestion
    /// branch. `bbr2_gcongestion` in a string form.
    Bbr2Gcongestion = 4,
}

impl FromStr for CongestionControlAlgorithm {
    type Err = crate::Error;

    /// Converts a string to `CongestionControlAlgorithm`.
    ///
    /// If `name` is not valid, `Error::CongestionControl` is returned.
    fn from_str(name: &str) -> std::result::Result<Self, Self::Err> {
        match name {
            "reno" => Ok(CongestionControlAlgorithm::Reno),
            "cubic" => Ok(CongestionControlAlgorithm::CUBIC),
            "bbr" => Ok(CongestionControlAlgorithm::Bbr2Gcongestion),
            "bbr2" => Ok(CongestionControlAlgorithm::Bbr2Gcongestion),
            "bbr2_gcongestion" => Ok(CongestionControlAlgorithm::Bbr2Gcongestion),
            _ => Err(crate::Error::CongestionControl),
        }
    }
}

#[derive(Clone)]
pub struct Sent {
    pub pkt_num: u64,

    pub frames: SmallVec<[frame::Frame; 1]>,

    pub time_sent: Instant,

    pub time_acked: Option<Instant>,

    pub time_lost: Option<Instant>,

    pub size: usize,

    pub ack_eliciting: bool,

    pub in_flight: bool,

    pub delivered: usize,

    pub delivered_time: Instant,

    pub first_sent_time: Instant,

    pub is_app_limited: bool,

    pub tx_in_flight: usize,

    pub lost: u64,

    pub has_data: bool,

    pub is_pmtud_probe: bool,
}

impl std::fmt::Debug for Sent {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "pkt_num={:?} ", self.pkt_num)?;
        write!(f, "pkt_sent_time={:?} ", self.time_sent)?;
        write!(f, "pkt_size={:?} ", self.size)?;
        write!(f, "delivered={:?} ", self.delivered)?;
        write!(f, "delivered_time={:?} ", self.delivered_time)?;
        write!(f, "first_sent_time={:?} ", self.first_sent_time)?;
        write!(f, "is_app_limited={} ", self.is_app_limited)?;
        write!(f, "tx_in_flight={} ", self.tx_in_flight)?;
        write!(f, "lost={} ", self.lost)?;
        write!(f, "has_data={} ", self.has_data)?;
        write!(f, "is_pmtud_probe={}", self.is_pmtud_probe)?;

        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct HandshakeStatus {
    pub has_handshake_keys: bool,

    pub peer_verified_address: bool,

    pub completed: bool,
}

#[cfg(test)]
impl Default for HandshakeStatus {
    fn default() -> HandshakeStatus {
        HandshakeStatus {
            has_handshake_keys: true,

            peer_verified_address: true,

            completed: true,
        }
    }
}

// We don't need to log all qlog metrics every time there is a recovery event.
// Instead, we can log only the MetricsUpdated event data fields that we care
// about, only when they change. To support this, the QLogMetrics structure
// keeps a running picture of the fields.
#[derive(Default)]
#[cfg(feature = "qlog")]
struct QlogMetrics {
    min_rtt: Duration,
    smoothed_rtt: Duration,
    latest_rtt: Duration,
    rttvar: Duration,
    cwnd: u64,
    bytes_in_flight: u64,
    ssthresh: Option<u64>,
    pacing_rate: u64,
}

#[cfg(feature = "qlog")]
impl QlogMetrics {
    // Make a qlog event if the latest instance of QlogMetrics is different.
    //
    // This function diffs each of the fields. A qlog MetricsUpdated event is
    // only generated if at least one field is different. Where fields are
    // different, the qlog event contains the latest value.
    fn maybe_update(&mut self, latest: Self) -> Option<EventData> {
        let mut emit_event = false;

        let new_min_rtt = if self.min_rtt != latest.min_rtt {
            self.min_rtt = latest.min_rtt;
            emit_event = true;
            Some(latest.min_rtt.as_secs_f32() * 1000.0)
        } else {
            None
        };

        let new_smoothed_rtt = if self.smoothed_rtt != latest.smoothed_rtt {
            self.smoothed_rtt = latest.smoothed_rtt;
            emit_event = true;
            Some(latest.smoothed_rtt.as_secs_f32() * 1000.0)
        } else {
            None
        };

        let new_latest_rtt = if self.latest_rtt != latest.latest_rtt {
            self.latest_rtt = latest.latest_rtt;
            emit_event = true;
            Some(latest.latest_rtt.as_secs_f32() * 1000.0)
        } else {
            None
        };

        let new_rttvar = if self.rttvar != latest.rttvar {
            self.rttvar = latest.rttvar;
            emit_event = true;
            Some(latest.rttvar.as_secs_f32() * 1000.0)
        } else {
            None
        };

        let new_cwnd = if self.cwnd != latest.cwnd {
            self.cwnd = latest.cwnd;
            emit_event = true;
            Some(latest.cwnd)
        } else {
            None
        };

        let new_bytes_in_flight = if self.bytes_in_flight != latest.bytes_in_flight {
            self.bytes_in_flight = latest.bytes_in_flight;
            emit_event = true;
            Some(latest.bytes_in_flight)
        } else {
            None
        };

        let new_ssthresh = if self.ssthresh != latest.ssthresh {
            self.ssthresh = latest.ssthresh;
            emit_event = true;
            latest.ssthresh
        } else {
            None
        };

        let new_pacing_rate = if self.pacing_rate != latest.pacing_rate {
            self.pacing_rate = latest.pacing_rate;
            emit_event = true;
            Some(latest.pacing_rate)
        } else {
            None
        };

        if emit_event {
            // QVis can't use all these fields and they can be large.
            return Some(EventData::MetricsUpdated(
                qlog::events::quic::MetricsUpdated {
                    min_rtt: new_min_rtt,
                    smoothed_rtt: new_smoothed_rtt,
                    latest_rtt: new_latest_rtt,
                    rtt_variance: new_rttvar,
                    congestion_window: new_cwnd,
                    bytes_in_flight: new_bytes_in_flight,
                    ssthresh: new_ssthresh,
                    pacing_rate: new_pacing_rate,
                    ..Default::default()
                },
            ));
        }

        None
    }
}

/// When the pacer thinks is a good time to release the next packet
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseTime {
    Immediate,
    At(Instant),
}

/// When the next packet should be release and if it can be part of a burst
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseDecision {
    time: ReleaseTime,
    allow_burst: bool,
}

impl ReleaseTime {
    /// Add the specific delay to the current time
    fn inc(&mut self, delay: Duration) {
        match self {
            ReleaseTime::Immediate => {}
            ReleaseTime::At(time) => *time += delay,
        }
    }

    /// Set the time to the later of two times
    fn set_max(&mut self, other: Instant) {
        match self {
            ReleaseTime::Immediate => *self = ReleaseTime::At(other),
            ReleaseTime::At(time) => *self = ReleaseTime::At(other.max(*time)),
        }
    }
}

impl ReleaseDecision {
    pub(crate) const EQUAL_THRESHOLD: Duration = Duration::from_micros(50);

    /// Get the [`Instant`] the next packet should be released. It will never be
    /// in the past.
    #[inline]
    pub fn time(&self, now: Instant) -> Option<Instant> {
        match self.time {
            ReleaseTime::Immediate => None,
            ReleaseTime::At(other) => other.gt(&now).then_some(other),
        }
    }

    /// Can this packet be appended to a previous burst
    #[inline]
    pub fn can_burst(&self) -> bool {
        self.allow_burst
    }

    /// Check if the two packets can be released at the same time
    #[inline]
    pub fn time_eq(&self, other: &Self, now: Instant) -> bool {
        let delta = match (self.time(now), other.time(now)) {
            (None, None) => Duration::ZERO,
            (Some(t), None) | (None, Some(t)) => t.duration_since(now),
            (Some(t1), Some(t2)) if t1 < t2 => t2.duration_since(t1),
            (Some(t1), Some(t2)) => t1.duration_since(t2),
        };

        delta <= Self::EQUAL_THRESHOLD
    }
}

/// Recovery statistics
#[derive(Default, Debug)]
pub struct RecoveryStats {
    startup_exit: Option<StartupExit>,
}

impl RecoveryStats {
    // Record statistics when a CCA first exits startup.
    pub fn set_startup_exit(&mut self, startup_exit: StartupExit) {
        if self.startup_exit.is_none() {
            self.startup_exit = Some(startup_exit);
        }
    }
}

/// Statistics from when a CCA first exited the startup phase.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StartupExit {
    /// The congestion_window recorded at Startup exit.
    pub cwnd: usize,

    /// The bandwidth estimate recorded at Startup exit.
    pub bandwidth: Option<u64>,

    /// The reason a CCA exited the startup phase.
    pub reason: StartupExitReason,
}

impl StartupExit {
    fn new(cwnd: usize, bandwidth: Option<Bandwidth>, reason: StartupExitReason) -> Self {
        let bandwidth = bandwidth.map(Bandwidth::to_bytes_per_second);
        Self {
            cwnd,
            bandwidth,
            reason,
        }
    }
}

/// The reason a CCA exited the startup phase.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StartupExitReason {
    /// Exit startup due to excessive loss
    Loss,

    /// Exit startup due to bandwidth plateau.
    BandwidthPlateau,

    /// Exit startup due to persistent queue.
    PersistentQueue,
}

mod bandwidth;
mod bytes_in_flight;
mod congestion;
mod gcongestion;
mod rtt;
