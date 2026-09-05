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

use std::cmp;

use std::sync::Arc;

use std::collections::hash_map;
use std::collections::HashMap;
use std::collections::HashSet;

use intrusive_collections::intrusive_adapter;
use intrusive_collections::KeyAdapter;
use intrusive_collections::RBTree;
use intrusive_collections::RBTreeAtomicLink;

use smallvec::SmallVec;

use crate::range_buf::DefaultBufFactory;
use crate::BufFactory;
use crate::Error;
use crate::Result;

const DEFAULT_URGENCY: u8 = 127;

// The default size of the receiver stream flow control window.
const DEFAULT_STREAM_WINDOW: u64 = 32 * 1024;

/// The maximum size of the receiver stream flow control window.
pub const MAX_STREAM_WINDOW: u64 = 16 * 1024 * 1024;

/// A simple no-op hasher for Stream IDs.
///
/// The QUIC protocol and quiche library guarantees stream ID uniqueness, so
/// we can save effort by avoiding using a more complicated algorithm.
#[derive(Default)]
pub struct StreamIdHasher {
    id: u64,
}

/// Return value type of `RecvBuf::reset()`
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct RecvBufResetReturn {
    /// Returns the difference between the previous max_data offset
    /// received and the final size reported by the reset
    pub max_data_delta: u64,

    /// The amount of flow control credit that should be returned to the
    /// connection level flow control.
    pub consumed_flowcontrol: u64,
}

impl RecvBufResetReturn {
    pub fn zero() -> Self {
        Self {
            max_data_delta: 0,
            consumed_flowcontrol: 0,
        }
    }
}

/// Action to perform when reading from a stream's receive buffer.
pub enum RecvAction<'a> {
    /// Emit data by copying it into the provided buffer.
    Emit { out: &'a mut [u8] },
    /// Discard up to the specified number of bytes without copying.
    Discard { len: usize },
}

impl std::hash::Hasher for StreamIdHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.id
    }

    #[inline]
    fn write_u64(&mut self, id: u64) {
        self.id = id;
    }

    #[inline]
    fn write(&mut self, _: &[u8]) {
        // We need a default write() for the trait but stream IDs will always
        // be a u64 so we just delegate to write_u64.
        unimplemented!()
    }
}

type BuildStreamIdHasher = std::hash::BuildHasherDefault<StreamIdHasher>;

pub type StreamIdHashMap<V> = HashMap<u64, V, BuildStreamIdHasher>;
pub type StreamIdHashSet = HashSet<u64, BuildStreamIdHasher>;

/// Keeps track of QUIC streams and enforces stream limits.
#[derive(Default)]
pub struct StreamMap<F: BufFactory = DefaultBufFactory> {
    /// Map of streams indexed by stream ID.
    streams: StreamIdHashMap<Stream<F>>,

    /// Set of streams that were completed and garbage collected.
    ///
    /// Instead of keeping the full stream state forever, we collect completed
    /// streams to save memory, but we still need to keep track of previously
    /// created streams, to prevent peers from re-creating them.
    collected: StreamIdHashSet,

    /// Peer's maximum bidirectional stream count limit.
    peer_max_streams_bidi: u64,

    /// Peer's maximum unidirectional stream count limit.
    peer_max_streams_uni: u64,

    /// The total number of bidirectional streams opened by the peer.
    peer_opened_streams_bidi: u64,

    /// The total number of unidirectional streams opened by the peer.
    peer_opened_streams_uni: u64,

    /// Local maximum bidirectional stream count limit.
    local_max_streams_bidi: u64,
    local_max_streams_bidi_next: u64,

    /// Local maximum unidirectional stream count limit.
    local_max_streams_uni: u64,
    local_max_streams_uni_next: u64,

    /// The total number of bidirectional streams opened by the local endpoint.
    local_opened_streams_bidi: u64,

    /// The total number of unidirectional streams opened by the local endpoint.
    local_opened_streams_uni: u64,

    /// Queue of stream IDs corresponding to streams that have buffered data
    /// ready to be sent to the peer. This also implies that the stream has
    /// enough flow control credits to send at least some of that data.
    flushable: RBTree<StreamFlushablePriorityAdapter>,

    /// Set of stream IDs corresponding to streams that have outstanding data
    /// to read. This is used to generate a `StreamIter` of streams without
    /// having to iterate over the full list of streams.
    pub readable: RBTree<StreamReadablePriorityAdapter>,

    /// Set of stream IDs corresponding to streams that have enough flow control
    /// capacity to be written to, and is not finished. This is used to generate
    /// a `StreamIter` of streams without having to iterate over the full list
    /// of streams.
    pub writable: RBTree<StreamWritablePriorityAdapter>,

    /// Set of stream IDs corresponding to streams that are almost out of flow
    /// control credit and need to send MAX_STREAM_DATA. This is used to
    /// generate a `StreamIter` of streams without having to iterate over the
    /// full list of streams.
    almost_full: StreamIdHashSet,

    /// Set of stream IDs corresponding to streams that are blocked. The value
    /// of the map elements represents the offset of the stream at which the
    /// blocking occurred.
    blocked: StreamIdHashMap<u64>,

    /// Set of stream IDs corresponding to streams that are reset. The value
    /// of the map elements is a tuple of the error code and final size values
    /// to include in the RESET_STREAM frame.
    reset: StreamIdHashMap<(u64, u64)>,

    /// Set of stream IDs corresponding to streams that are shutdown on the
    /// receive side, and need to send a STOP_SENDING frame. The value of the
    /// map elements is the error code to include in the STOP_SENDING frame.
    stopped: StreamIdHashMap<u64>,

    /// The maximum size of a stream window.
    max_stream_window: u64,
}

impl<F: BufFactory> StreamMap<F> {
    pub fn new(max_streams_bidi: u64, max_streams_uni: u64, max_stream_window: u64) -> Self {
        StreamMap {
            local_max_streams_bidi: max_streams_bidi,
            local_max_streams_bidi_next: max_streams_bidi,

            local_max_streams_uni: max_streams_uni,
            local_max_streams_uni_next: max_streams_uni,

            max_stream_window,

            ..StreamMap::default()
        }
    }

    /// Returns the stream with the given ID if it exists.
    pub fn get(&self, id: u64) -> Option<&Stream<F>> {
        self.streams.get(&id)
    }

    /// Returns the mutable stream with the given ID if it exists.
    pub fn get_mut(&mut self, id: u64) -> Option<&mut Stream<F>> {
        self.streams.get_mut(&id)
    }

    /// Returns the mutable stream with the given ID if it exists, or creates
    /// a new one otherwise.
    ///
    /// The `local` parameter indicates whether the stream's creation was
    /// requested by the local application rather than the peer, and is
    /// used to validate the requested stream ID, and to select the initial
    /// flow control values from the local and remote transport parameters
    /// (also passed as arguments).
    ///
    /// This also takes care of enforcing both local and the peer's stream
    /// count limits. If one of these limits is violated, the `StreamLimit`
    /// error is returned.
    pub(crate) fn get_or_create(
        &mut self,
        id: u64,
        local_params: &crate::TransportParams,
        peer_params: &crate::TransportParams,
        local: bool,
        is_server: bool,
    ) -> Result<&mut Stream<F>> {
        let (stream, is_new_and_writable) = match self.streams.entry(id) {
            hash_map::Entry::Vacant(v) => {
                // Stream has already been closed and garbage collected.
                if self.collected.contains(&id) {
                    return Err(Error::Done);
                }

                if local != is_local(id, is_server) {
                    return Err(Error::InvalidStreamState(id));
                }

                let (max_rx_data, max_tx_data) = match (local, is_bidi(id)) {
                    // Locally-initiated bidirectional stream.
                    (true, true) => (
                        local_params.initial_max_stream_data_bidi_local,
                        peer_params.initial_max_stream_data_bidi_remote,
                    ),

                    // Locally-initiated unidirectional stream.
                    (true, false) => (0, peer_params.initial_max_stream_data_uni),

                    // Remotely-initiated bidirectional stream.
                    (false, true) => (
                        local_params.initial_max_stream_data_bidi_remote,
                        peer_params.initial_max_stream_data_bidi_local,
                    ),

                    // Remotely-initiated unidirectional stream.
                    (false, false) => (local_params.initial_max_stream_data_uni, 0),
                };

                // The two least significant bits from a stream id identify the
                // type of stream. Truncate those bits to get the sequence for
                // that stream type.
                let stream_sequence = id >> 2;

                // Enforce stream count limits.
                match (is_local(id, is_server), is_bidi(id)) {
                    (true, true) => {
                        let n = cmp::max(self.local_opened_streams_bidi, stream_sequence + 1);

                        if n > self.peer_max_streams_bidi {
                            return Err(Error::StreamLimit);
                        }

                        self.local_opened_streams_bidi = n;
                    }

                    (true, false) => {
                        let n = cmp::max(self.local_opened_streams_uni, stream_sequence + 1);

                        if n > self.peer_max_streams_uni {
                            return Err(Error::StreamLimit);
                        }

                        self.local_opened_streams_uni = n;
                    }

                    (false, true) => {
                        let n = cmp::max(self.peer_opened_streams_bidi, stream_sequence + 1);

                        if n > self.local_max_streams_bidi {
                            return Err(Error::StreamLimit);
                        }

                        self.peer_opened_streams_bidi = n;
                    }

                    (false, false) => {
                        let n = cmp::max(self.peer_opened_streams_uni, stream_sequence + 1);

                        if n > self.local_max_streams_uni {
                            return Err(Error::StreamLimit);
                        }

                        self.peer_opened_streams_uni = n;
                    }
                };

                let s = Stream::new(
                    id,
                    max_rx_data,
                    max_tx_data,
                    is_bidi(id),
                    local,
                    self.max_stream_window,
                );

                let is_writable = s.is_writable();

                (v.insert(s), is_writable)
            }

            hash_map::Entry::Occupied(v) => (v.into_mut(), false),
        };

        // Newly created stream might already be writable due to initial flow
        // control limits.
        if is_new_and_writable {
            self.writable.insert(Arc::clone(&stream.priority_key));
        }

        Ok(stream)
    }

    /// Adds the stream ID to the readable streams set.
    ///
    /// If the stream was already in the list, this does nothing.
    pub fn insert_readable(&mut self, priority_key: &Arc<StreamPriorityKey>) {
        if !priority_key.readable.is_linked() {
            self.readable.insert(Arc::clone(priority_key));
        }
    }

    /// Removes the stream ID from the readable streams set.
    pub fn remove_readable(&mut self, priority_key: &Arc<StreamPriorityKey>) {
        if !priority_key.readable.is_linked() {
            return;
        }

        let mut c = {
            let ptr = Arc::as_ptr(priority_key);
            unsafe { self.readable.cursor_mut_from_ptr(ptr) }
        };

        c.remove();
    }

    /// Adds the stream ID to the writable streams set.
    ///
    /// This should also be called anytime a new stream is created, in addition
    /// to when an existing stream becomes writable.
    ///
    /// If the stream was already in the list, this does nothing.
    pub fn insert_writable(&mut self, priority_key: &Arc<StreamPriorityKey>) {
        if !priority_key.writable.is_linked() {
            self.writable.insert(Arc::clone(priority_key));
        }
    }

    /// Removes the stream ID from the writable streams set.
    ///
    /// This should also be called anytime an existing stream stops being
    /// writable.
    pub fn remove_writable(&mut self, priority_key: &Arc<StreamPriorityKey>) {
        if !priority_key.writable.is_linked() {
            return;
        }

        let mut c = {
            let ptr = Arc::as_ptr(priority_key);
            unsafe { self.writable.cursor_mut_from_ptr(ptr) }
        };

        c.remove();
    }

    /// Adds the stream ID to the flushable streams set.
    ///
    /// If the stream was already in the list, this does nothing.
    pub fn insert_flushable(&mut self, priority_key: &Arc<StreamPriorityKey>) {
        if !priority_key.flushable.is_linked() {
            self.flushable.insert(Arc::clone(priority_key));
        }
    }

    /// Removes the stream ID from the flushable streams set.
    pub fn remove_flushable(&mut self, priority_key: &Arc<StreamPriorityKey>) {
        if !priority_key.flushable.is_linked() {
            return;
        }

        let mut c = {
            let ptr = Arc::as_ptr(priority_key);
            unsafe { self.flushable.cursor_mut_from_ptr(ptr) }
        };

        c.remove();
    }

    pub fn peek_flushable(&self) -> Option<Arc<StreamPriorityKey>> {
        self.flushable.front().clone_pointer()
    }

    /// Updates the priorities of a stream.
    pub fn update_priority(&mut self, old: &Arc<StreamPriorityKey>, new: &Arc<StreamPriorityKey>) {
        if old.readable.is_linked() {
            self.remove_readable(old);
            self.readable.insert(Arc::clone(new));
        }

        if old.writable.is_linked() {
            self.remove_writable(old);
            self.writable.insert(Arc::clone(new));
        }

        if old.flushable.is_linked() {
            self.remove_flushable(old);
            self.flushable.insert(Arc::clone(new));
        }
    }

    /// Adds the stream ID to the almost full streams set.
    ///
    /// If the stream was already in the list, this does nothing.
    pub fn insert_almost_full(&mut self, stream_id: u64) {
        self.almost_full.insert(stream_id);
    }

    /// Removes the stream ID from the almost full streams set.
    pub fn remove_almost_full(&mut self, stream_id: u64) {
        self.almost_full.remove(&stream_id);
    }

    /// Adds the stream ID to the blocked streams set with the
    /// given offset value.
    ///
    /// If the stream was already in the list, this does nothing.
    pub fn insert_blocked(&mut self, stream_id: u64, off: u64) {
        self.blocked.insert(stream_id, off);
    }

    /// Removes the stream ID from the blocked streams set.
    pub fn remove_blocked(&mut self, stream_id: u64) {
        self.blocked.remove(&stream_id);
    }

    /// Adds the stream ID to the reset streams set with the
    /// given error code and final size values.
    ///
    /// If the stream was already in the list, this does nothing.
    pub fn insert_reset(&mut self, stream_id: u64, error_code: u64, final_size: u64) {
        self.reset.insert(stream_id, (error_code, final_size));
    }

    /// Removes the stream ID from the reset streams set.
    pub fn remove_reset(&mut self, stream_id: u64) {
        self.reset.remove(&stream_id);
    }

    /// Adds the stream ID to the stopped streams set with the
    /// given error code.
    ///
    /// If the stream was already in the list, this does nothing.
    pub fn insert_stopped(&mut self, stream_id: u64, error_code: u64) {
        self.stopped.insert(stream_id, error_code);
    }

    /// Removes the stream ID from the stopped streams set.
    pub fn remove_stopped(&mut self, stream_id: u64) {
        self.stopped.remove(&stream_id);
    }

    /// Updates the peer's maximum bidirectional stream count limit.
    pub fn update_peer_max_streams_bidi(&mut self, v: u64) {
        self.peer_max_streams_bidi = cmp::max(self.peer_max_streams_bidi, v);
    }

    /// Updates the peer's maximum unidirectional stream count limit.
    pub fn update_peer_max_streams_uni(&mut self, v: u64) {
        self.peer_max_streams_uni = cmp::max(self.peer_max_streams_uni, v);
    }

    /// Commits the new max_streams_bidi limit.
    pub fn update_max_streams_bidi(&mut self) {
        self.local_max_streams_bidi = self.local_max_streams_bidi_next;
    }

    /// Sets the max_streams_bidi limit to the given value.
    pub fn set_max_streams_bidi(&mut self, max: u64) {
        self.local_max_streams_bidi = max;
        self.local_max_streams_bidi_next = max;
    }

    /// Returns the current max_streams_bidi limit.
    pub fn max_streams_bidi(&self) -> u64 {
        self.local_max_streams_bidi
    }

    /// Returns the new max_streams_bidi limit.
    pub fn max_streams_bidi_next(&mut self) -> u64 {
        self.local_max_streams_bidi_next
    }

    /// Commits the new max_streams_uni limit.
    pub fn update_max_streams_uni(&mut self) {
        self.local_max_streams_uni = self.local_max_streams_uni_next;
    }

    /// Returns the new max_streams_uni limit.
    pub fn max_streams_uni_next(&mut self) -> u64 {
        self.local_max_streams_uni_next
    }

    /// Returns the number of bidirectional streams that can be created
    /// before the peer's stream count limit is reached.
    pub fn peer_streams_left_bidi(&self) -> u64 {
        self.peer_max_streams_bidi - self.local_opened_streams_bidi
    }

    /// Returns the number of unidirectional streams that can be created
    /// before the peer's stream count limit is reached.
    pub fn peer_streams_left_uni(&self) -> u64 {
        self.peer_max_streams_uni - self.local_opened_streams_uni
    }

    /// Drops completed stream.
    ///
    /// This should only be called when Stream::is_complete() returns true for
    /// the given stream.
    pub fn collect(&mut self, stream_id: u64, local: bool) {
        if !local {
            // If the stream was created by the peer, give back a max streams
            // credit.
            if is_bidi(stream_id) {
                self.local_max_streams_bidi_next =
                    self.local_max_streams_bidi_next.saturating_add(1);
            } else {
                self.local_max_streams_uni_next = self.local_max_streams_uni_next.saturating_add(1);
            }
        }

        let s = self.streams.remove(&stream_id).unwrap();

        self.remove_readable(&s.priority_key);

        self.remove_writable(&s.priority_key);

        self.remove_flushable(&s.priority_key);

        self.collected.insert(stream_id);
    }

    /// Creates an iterator over streams that have outstanding data to read.
    pub fn readable(&self) -> StreamIter {
        StreamIter {
            streams: self.readable.iter().map(|s| s.id).collect(),
            index: 0,
        }
    }

    /// Creates an iterator over streams that can be written to.
    pub fn writable(&self) -> StreamIter {
        StreamIter {
            streams: self.writable.iter().map(|s| s.id).collect(),
            index: 0,
        }
    }

    /// Creates an iterator over streams that need to send MAX_STREAM_DATA.
    pub fn almost_full(&self) -> StreamIter {
        StreamIter::from(&self.almost_full)
    }

    /// Creates an iterator over streams that need to send STREAM_DATA_BLOCKED.
    pub fn blocked(&self) -> hash_map::Iter<'_, u64, u64> {
        self.blocked.iter()
    }

    /// Creates an iterator over streams that need to send RESET_STREAM.
    pub fn reset(&self) -> hash_map::Iter<'_, u64, (u64, u64)> {
        self.reset.iter()
    }

    /// Creates an iterator over streams that need to send STOP_SENDING.
    pub fn stopped(&self) -> hash_map::Iter<'_, u64, u64> {
        self.stopped.iter()
    }

    /// Returns true if the stream has been collected.
    pub fn is_collected(&self, stream_id: u64) -> bool {
        self.collected.contains(&stream_id)
    }

    /// Returns true if there are any streams that have data to write.
    pub fn has_flushable(&self) -> bool {
        !self.flushable.is_empty()
    }

    /// Returns true if there are any streams that have data to read.
    pub fn has_readable(&self) -> bool {
        !self.readable.is_empty()
    }

    /// Returns true if there are any streams that need to update the local
    /// flow control limit.
    pub fn has_almost_full(&self) -> bool {
        !self.almost_full.is_empty()
    }

    /// Returns true if there are any streams that are blocked.
    pub fn has_blocked(&self) -> bool {
        !self.blocked.is_empty()
    }

    /// Returns true if there are any streams that are reset.
    pub fn has_reset(&self) -> bool {
        !self.reset.is_empty()
    }

    /// Returns true if there are any streams that need to send STOP_SENDING.
    pub fn has_stopped(&self) -> bool {
        !self.stopped.is_empty()
    }

    /// Returns true if the max bidirectional streams count needs to be updated
    /// by sending a MAX_STREAMS frame to the peer.
    pub fn should_update_max_streams_bidi(&self) -> bool {
        self.local_max_streams_bidi_next != self.local_max_streams_bidi
            && self.local_max_streams_bidi_next / 2
                > self.local_max_streams_bidi - self.peer_opened_streams_bidi
    }

    /// Returns true if the max unidirectional streams count needs to be updated
    /// by sending a MAX_STREAMS frame to the peer.
    pub fn should_update_max_streams_uni(&self) -> bool {
        self.local_max_streams_uni_next != self.local_max_streams_uni
            && self.local_max_streams_uni_next / 2
                > self.local_max_streams_uni - self.peer_opened_streams_uni
    }

    /// Returns the number of active streams in the map.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.streams.len()
    }
}

/// A QUIC stream.
pub struct Stream<F: BufFactory = DefaultBufFactory> {
    /// Receive-side stream buffer.
    pub recv: recv_buf::RecvBuf,

    /// Send-side stream buffer.
    pub send: send_buf::SendBuf<F>,

    pub send_lowat: usize,

    /// Whether the stream is bidirectional.
    pub bidi: bool,

    /// Whether the stream was created by the local endpoint.
    pub local: bool,

    /// The stream's urgency (lower is better). Default is `DEFAULT_URGENCY`.
    pub urgency: u8,

    /// Whether the stream can be flushed incrementally. Default is `true`.
    pub incremental: bool,

    pub priority_key: Arc<StreamPriorityKey>,
}

impl<F: BufFactory> Stream<F> {
    /// Creates a new stream with the given flow control limits.
    pub fn new(
        id: u64,
        max_rx_data: u64,
        max_tx_data: u64,
        bidi: bool,
        local: bool,
        max_window: u64,
    ) -> Self {
        let priority_key = Arc::new(StreamPriorityKey {
            id,
            ..Default::default()
        });

        Stream {
            recv: recv_buf::RecvBuf::new(max_rx_data, max_window),
            send: send_buf::SendBuf::new(max_tx_data),
            send_lowat: 1,
            bidi,
            local,
            urgency: priority_key.urgency,
            incremental: priority_key.incremental,
            priority_key,
        }
    }

    /// Returns true if the stream has data to read.
    pub fn is_readable(&self) -> bool {
        self.recv.ready()
    }

    /// Returns true if the stream has enough flow control capacity to be
    /// written to, and is not finished.
    pub fn is_writable(&self) -> bool {
        !self.send.is_shutdown()
            && !self.send.is_fin()
            && (self.send.off_back() + self.send_lowat as u64) < self.send.max_off()
    }

    /// Returns true if the stream has data to send and is allowed to send at
    /// least some of it.
    pub fn is_flushable(&self) -> bool {
        let off_front = self.send.off_front();

        !self.send.is_empty() && off_front < self.send.off_back() && off_front < self.send.max_off()
    }

    /// Returns true if the stream is complete.
    ///
    /// For bidirectional streams this happens when both the receive and send
    /// sides are complete. That is when all incoming data has been read by the
    /// application, and when all outgoing data has been acked by the peer.
    ///
    /// For unidirectional streams this happens when either the receive or send
    /// side is complete, depending on whether the stream was created locally
    /// or not.
    pub fn is_complete(&self) -> bool {
        match (self.bidi, self.local) {
            // For bidirectional streams we need to check both receive and send
            // sides for completion.
            (true, _) => self.recv.is_fin() && self.send.is_complete(),

            // For unidirectional streams generated locally, we only need to
            // check the send side for completion.
            (false, true) => self.send.is_complete(),

            // For unidirectional streams generated by the peer, we only need
            // to check the receive side for completion.
            (false, false) => self.recv.is_fin(),
        }
    }
}

/// Returns true if the stream was created locally.
pub fn is_local(stream_id: u64, is_server: bool) -> bool {
    (stream_id & 0x1) == (is_server as u64)
}

/// Returns true if the stream is bidirectional.
pub fn is_bidi(stream_id: u64) -> bool {
    (stream_id & 0x2) == 0
}

#[derive(Clone, Debug)]
pub struct StreamPriorityKey {
    pub urgency: u8,
    pub incremental: bool,
    pub id: u64,

    pub readable: RBTreeAtomicLink,
    pub writable: RBTreeAtomicLink,
    pub flushable: RBTreeAtomicLink,
}

impl Default for StreamPriorityKey {
    fn default() -> Self {
        Self {
            urgency: DEFAULT_URGENCY,
            incremental: true,
            id: Default::default(),
            readable: Default::default(),
            writable: Default::default(),
            flushable: Default::default(),
        }
    }
}

impl PartialEq for StreamPriorityKey {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for StreamPriorityKey {}

impl PartialOrd for StreamPriorityKey {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for StreamPriorityKey {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        // Ignore priority if ID matches.
        if self.id == other.id {
            return cmp::Ordering::Equal;
        }

        // First, order by urgency...
        if self.urgency != other.urgency {
            return self.urgency.cmp(&other.urgency);
        }

        // ...when the urgency is the same, and both are not incremental, order
        // by stream ID...
        if !self.incremental && !other.incremental {
            return self.id.cmp(&other.id);
        }

        // ...non-incremental takes priority over incremental...
        if self.incremental && !other.incremental {
            return cmp::Ordering::Greater;
        }
        if !self.incremental && other.incremental {
            return cmp::Ordering::Less;
        }

        // ...finally, when both are incremental, `other` takes precedence (so
        // `self` is always sorted after other same-urgency incremental
        // entries).
        cmp::Ordering::Greater
    }
}

intrusive_adapter!(pub StreamWritablePriorityAdapter = Arc<StreamPriorityKey>: StreamPriorityKey { writable: RBTreeAtomicLink });

impl KeyAdapter<'_> for StreamWritablePriorityAdapter {
    type Key = StreamPriorityKey;

    fn get_key(&self, s: &StreamPriorityKey) -> Self::Key {
        s.clone()
    }
}

intrusive_adapter!(pub StreamReadablePriorityAdapter = Arc<StreamPriorityKey>: StreamPriorityKey { readable: RBTreeAtomicLink });

impl KeyAdapter<'_> for StreamReadablePriorityAdapter {
    type Key = StreamPriorityKey;

    fn get_key(&self, s: &StreamPriorityKey) -> Self::Key {
        s.clone()
    }
}

intrusive_adapter!(pub StreamFlushablePriorityAdapter = Arc<StreamPriorityKey>: StreamPriorityKey { flushable: RBTreeAtomicLink });

impl KeyAdapter<'_> for StreamFlushablePriorityAdapter {
    type Key = StreamPriorityKey;

    fn get_key(&self, s: &StreamPriorityKey) -> Self::Key {
        s.clone()
    }
}

/// An iterator over QUIC streams.
#[derive(Default)]
pub struct StreamIter {
    streams: SmallVec<[u64; 8]>,
    index: usize,
}

impl StreamIter {
    #[inline]
    fn from(streams: &StreamIdHashSet) -> Self {
        StreamIter {
            streams: streams.iter().copied().collect(),
            index: 0,
        }
    }
}

impl Iterator for StreamIter {
    type Item = u64;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let v = self.streams.get(self.index)?;
        self.index += 1;
        Some(*v)
    }
}

impl ExactSizeIterator for StreamIter {
    #[inline]
    fn len(&self) -> usize {
        self.streams.len() - self.index
    }
}

mod recv_buf;
mod send_buf;
