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

//! Proportional Rate Reduction
//!
//! This implementation is based on the following RFC:
//!
//! <https://datatracker.ietf.org/doc/html/rfc6937>

use std::cmp;

#[derive(Default, Debug)]
pub struct PRR {
    // Total bytes delivered during recovery.
    prr_delivered: usize,

    // FlightSize at the start of recovery.
    recoverfs: usize,

    // Total bytes sent during recovery.
    prr_out: usize,

    // Total additional bytes can be sent for retransmit during recovery.
    pub snd_cnt: usize,
}

impl PRR {
    pub fn on_packet_sent(&mut self, sent_bytes: usize) {
        self.prr_out += sent_bytes;

        self.snd_cnt = self.snd_cnt.saturating_sub(sent_bytes);
    }

    pub fn congestion_event(&mut self, bytes_in_flight: usize) {
        self.prr_delivered = 0;

        self.recoverfs = bytes_in_flight;

        self.prr_out = 0;

        self.snd_cnt = 0;
    }

    pub fn on_packet_acked(
        &mut self,
        delivered_data: usize,
        pipe: usize,
        ssthresh: usize,
        max_datagram_size: usize,
    ) {
        self.prr_delivered += delivered_data;

        self.snd_cnt = if pipe > ssthresh {
            // Proportional Rate Reduction.
            if self.recoverfs > 0 {
                (self.prr_delivered * ssthresh)
                    .div_ceil(self.recoverfs)
                    .saturating_sub(self.prr_out)
            } else {
                0
            }
        } else {
            // PRR-SSRB.
            let limit = cmp::max(
                self.prr_delivered.saturating_sub(self.prr_out),
                delivered_data,
            ) + max_datagram_size;

            // Attempt to catch up, as permitted by limit
            cmp::min(ssthresh - pipe, limit)
        };

        // snd_cnt should be a positive number.
        self.snd_cnt = cmp::max(self.snd_cnt, 0);
    }
}
