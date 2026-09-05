// Copyright (C) 2023, Cloudflare, Inc.
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

use std::cmp;

use std::collections::BTreeMap;
use std::collections::VecDeque;

use std::time::Duration;
use std::time::Instant;

use crate::stream::RecvAction;
use crate::stream::RecvBufResetReturn;
use crate::Error;
use crate::Result;

use crate::flowcontrol;

use crate::range_buf::RangeBuf;

use super::DEFAULT_STREAM_WINDOW;

/// Receive-side stream buffer.
///
/// Stream data received by the peer is buffered in a list of data chunks
/// ordered by offset in ascending order. Contiguous data can then be read
/// into a slice.
#[derive(Debug, Default)]
pub struct RecvBuf {
    /// Chunks of data received from the peer that have not yet been read by
    /// the application, ordered by offset.
    data: BTreeMap<u64, RangeBuf>,

    /// The lowest data offset that has yet to be read by the application.
    off: u64,

    /// The total length of data received on this stream.
    len: u64,

    /// Receiver flow controller.
    flow_control: flowcontrol::FlowControl,

    /// The final stream offset received from the peer, if any.
    fin_off: Option<u64>,

    /// The error code received via RESET_STREAM.
    error: Option<u64>,

    /// Whether incoming data is validated but not buffered.
    drain: bool,
}

impl RecvBuf {
    /// Creates a new receive buffer.
    pub fn new(max_data: u64, max_window: u64) -> RecvBuf {
        RecvBuf {
            flow_control: flowcontrol::FlowControl::new(
                max_data,
                cmp::min(max_data, DEFAULT_STREAM_WINDOW),
                max_window,
            ),
            ..RecvBuf::default()
        }
    }

    /// Inserts the given chunk of data in the buffer.
    ///
    /// This also takes care of enforcing stream flow control limits, as well
    /// as handling incoming data that overlaps data that is already in the
    /// buffer.
    pub fn write(&mut self, buf: RangeBuf) -> Result<()> {
        if buf.max_off() > self.max_data() {
            return Err(Error::FlowControl);
        }

        if let Some(fin_off) = self.fin_off {
            // Stream's size is known, forbid data beyond that point.
            if buf.max_off() > fin_off {
                return Err(Error::FinalSize);
            }

            // Stream's size is already known, forbid changing it.
            if buf.fin() && fin_off != buf.max_off() {
                return Err(Error::FinalSize);
            }
        }

        // Stream's known size is lower than data already received.
        if buf.fin() && buf.max_off() < self.len {
            return Err(Error::FinalSize);
        }

        // We already saved the final offset, so there's nothing else we
        // need to keep from the RangeBuf if it's empty.
        if self.fin_off.is_some() && buf.is_empty() {
            return Ok(());
        }

        if buf.fin() {
            self.fin_off = Some(buf.max_off());
        }

        // No need to store empty buffer that doesn't carry the fin flag.
        if !buf.fin() && buf.is_empty() {
            return Ok(());
        }

        // Check if data is fully duplicate, that is the buffer's max offset is
        // lower or equal to the offset already stored in the recv buffer.
        if self.off >= buf.max_off() {
            // An exception is applied to empty range buffers, because an empty
            // buffer's max offset matches the max offset of the recv buffer.
            //
            // By this point all spurious empty buffers should have already been
            // discarded, so allowing empty buffers here should be safe.
            if !buf.is_empty() {
                return Ok(());
            }
        }

        let mut tmp_bufs = VecDeque::with_capacity(2);
        tmp_bufs.push_back(buf);

        'tmp: while let Some(mut buf) = tmp_bufs.pop_front() {
            // Discard incoming data below current stream offset. Bytes up to
            // `self.off` have already been received so we should not buffer
            // them again. This is also important to make sure `ready()` doesn't
            // get stuck when a buffer with lower offset than the stream's is
            // buffered.
            if self.off_front() > buf.off() {
                buf = buf.split_off((self.off_front() - buf.off()) as usize);
            }

            // Handle overlapping data. If the incoming data's starting offset
            // is above the previous maximum received offset, there is clearly
            // no overlap so this logic can be skipped. However do still try to
            // merge an empty final buffer (i.e. an empty buffer with the fin
            // flag set, which is the only kind of empty buffer that should
            // reach this point).
            if buf.off() < self.max_off() || buf.is_empty() {
                for (_, b) in self.data.range(buf.off()..) {
                    let off = buf.off();

                    // We are past the current buffer.
                    if b.off() > buf.max_off() {
                        break;
                    }

                    // New buffer is fully contained in existing buffer.
                    if off >= b.off() && buf.max_off() <= b.max_off() {
                        continue 'tmp;
                    }

                    // New buffer's start overlaps existing buffer.
                    if off >= b.off() && off < b.max_off() {
                        buf = buf.split_off((b.max_off() - off) as usize);
                    }

                    // New buffer's end overlaps existing buffer.
                    if off < b.off() && buf.max_off() > b.off() {
                        tmp_bufs.push_back(buf.split_off((b.off() - off) as usize));
                    }
                }
            }

            self.len = cmp::max(self.len, buf.max_off());

            if !self.drain {
                self.data.insert(buf.max_off(), buf);
            } else {
                // we are not storing any data, off == len
                self.off = self.len;
            }
        }

        Ok(())
    }

    /// Reads contiguous data from the receive buffer.
    ///
    /// Data is written into the given `out` buffer, up to the length of `out`.
    ///
    /// Only contiguous data is removed, starting from offset 0. The offset is
    /// incremented as data is taken out of the receive buffer. If there is no
    /// data at the expected read offset, the `Done` error is returned.
    ///
    /// On success the amount of data read and a flag indicating
    /// if there is no more data in the buffer, are returned as a tuple.
    pub fn emit(&mut self, out: &mut [u8]) -> Result<(usize, bool)> {
        self.emit_or_discard(RecvAction::Emit { out })
    }

    /// Reads or discards contiguous data from the receive buffer.
    ///
    /// Passing an `action` of `StreamRecvAction::Emit` results in data being
    /// written into the provided buffer, up to its length.
    ///
    /// Passing an `action` of `StreamRecvAction::Discard` results in up to
    /// the indicated number of bytes being discarded without copying.
    ///
    /// Only contiguous data is removed, starting from offset 0. The offset is
    /// incremented as data is taken out of the receive buffer. If there is no
    /// data at the expected read offset, the `Done` error is returned.
    ///
    /// On success the amount of data read or discarded, and a flag indicating
    /// if there is no more data in the buffer, are returned as a tuple.
    pub fn emit_or_discard(&mut self, mut action: RecvAction) -> Result<(usize, bool)> {
        let mut len = 0;
        let mut cap = match &action {
            RecvAction::Emit { out } => out.len(),
            RecvAction::Discard { len } => *len,
        };

        if !self.ready() {
            return Err(Error::Done);
        }

        // The stream was reset, so clear its data and return the error code
        // instead.
        if let Some(e) = self.error {
            self.data.clear();
            return Err(Error::StreamReset(e));
        }

        while cap > 0 && self.ready() {
            let mut entry = match self.data.first_entry() {
                Some(entry) => entry,
                None => break,
            };

            let buf = entry.get_mut();

            let buf_len = cmp::min(buf.len(), cap);

            // Only copy data if we're emitting, not discarding.
            if let RecvAction::Emit { ref mut out } = action {
                out[len..len + buf_len].copy_from_slice(&buf[..buf_len]);
            }

            self.off += buf_len as u64;

            len += buf_len;
            cap -= buf_len;

            if buf_len < buf.len() {
                buf.consume(buf_len);

                // We reached the maximum capacity, so end here.
                break;
            }

            entry.remove();
        }

        // Update consumed bytes for flow control.
        self.flow_control.add_consumed(len as u64);

        Ok((len, self.is_fin()))
    }

    /// Resets the stream at the given offset.
    pub fn reset(&mut self, error_code: u64, final_size: u64) -> Result<RecvBufResetReturn> {
        // Stream's size is already known, forbid changing it.
        if let Some(fin_off) = self.fin_off {
            if fin_off != final_size {
                return Err(Error::FinalSize);
            }
        }

        // Stream's known size is lower than data already received.
        if final_size < self.len {
            return Err(Error::FinalSize);
        }

        if self.error.is_some() {
            // We already verified that the final size matches
            return Ok(RecvBufResetReturn::zero());
        }

        // Calculate how many bytes need to be removed from the connection flow
        // control.
        let result = RecvBufResetReturn {
            max_data_delta: final_size - self.len,
            consumed_flowcontrol: final_size - self.off,
        };

        self.error = Some(error_code);

        // Clear all data already buffered.
        self.off = final_size;

        self.data.clear();

        // In order to ensure the application is notified when the stream is
        // reset, enqueue a zero-length buffer at the final size offset.
        let buf = RangeBuf::from(b"", final_size, true);
        self.write(buf)?;

        Ok(result)
    }

    /// Commits the new max_data limit.
    pub fn update_max_data(&mut self, now: Instant) {
        self.flow_control.update_max_data(now);
    }

    /// Return the new max_data limit.
    pub fn max_data_next(&mut self) -> u64 {
        self.flow_control.max_data_next()
    }

    /// Return the current flow control limit.
    pub fn max_data(&self) -> u64 {
        self.flow_control.max_data()
    }

    /// Return the current window.
    pub fn window(&self) -> u64 {
        self.flow_control.window()
    }

    /// Autotune the window size.
    pub fn autotune_window(&mut self, now: Instant, rtt: Duration) {
        self.flow_control.autotune_window(now, rtt);
    }

    /// Shuts down receiving data and returns the number of bytes
    /// that should be returned to the connection level flow
    /// control
    pub fn shutdown(&mut self) -> Result<u64> {
        if self.drain {
            return Err(Error::Done);
        }

        self.drain = true;

        self.data.clear();

        let consumed = self.max_off() - self.off;
        self.off = self.max_off();

        Ok(consumed)
    }

    /// Returns the lowest offset of data buffered.
    pub fn off_front(&self) -> u64 {
        self.off
    }

    /// Returns true if we need to update the local flow control limit.
    pub fn almost_full(&self) -> bool {
        self.fin_off.is_none() && self.flow_control.should_update_max_data()
    }

    /// Returns the largest offset ever received.
    pub fn max_off(&self) -> u64 {
        self.len
    }

    /// Returns true if the receive-side of the stream is complete.
    ///
    /// This happens when the stream's receive final size is known, and the
    /// application has read all data from the stream.
    pub fn is_fin(&self) -> bool {
        if self.fin_off == Some(self.off) {
            return true;
        }

        false
    }

    /// Returns true if the stream is not storing incoming data.
    pub fn is_draining(&self) -> bool {
        self.drain
    }

    /// Returns true if the stream has data to be read.
    pub fn ready(&self) -> bool {
        let (_, buf) = match self.data.first_key_value() {
            Some(v) => v,
            None => return false,
        };

        buf.off() == self.off
    }
}
