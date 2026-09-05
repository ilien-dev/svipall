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

//! HTTP/3 wire protocol and QPACK implementation.
//!
//! This module provides a high level API for sending and receiving HTTP/3
//! requests and responses on top of the QUIC transport protocol.
//!
//! ## Connection setup
//!
//! HTTP/3 connections require a QUIC transport-layer connection, see
//! [Connection setup] for a full description of the setup process.
//!
//! To use HTTP/3, the QUIC connection must be configured with a suitable
//! Application Layer Protocol Negotiation (ALPN) Protocol ID:
//!
//! ```
//! let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION)?;
//! config.set_application_protos(quiche::h3::APPLICATION_PROTOCOL)?;
//! # Ok::<(), quiche::Error>(())
//! ```
//!
//! The QUIC handshake is driven by [sending] and [receiving] QUIC packets.
//!
//! Once the handshake has completed, the first step in establishing an HTTP/3
//! connection is creating its configuration object:
//!
//! ```
//! let h3_config = quiche::h3::Config::new()?;
//! # Ok::<(), quiche::h3::Error>(())
//! ```
//!
//! HTTP/3 client and server connections are both created using the
//! [`with_transport()`] function, the role is inferred from the type of QUIC
//! connection:
//!
//! ```no_run
//! # let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();
//! # let scid = quiche::ConnectionId::from_ref(&[0xba; 16]);
//! # let peer = "127.0.0.1:1234".parse().unwrap();
//! # let local = "127.0.0.1:4321".parse().unwrap();
//! # let mut conn = quiche::accept(&scid, None, local, peer, &mut config).unwrap();
//! # let h3_config = quiche::h3::Config::new()?;
//! let h3_conn = quiche::h3::Connection::with_transport(&mut conn, &h3_config)?;
//! # Ok::<(), quiche::h3::Error>(())
//! ```
//!
//! ## Sending a request
//!
//! An HTTP/3 client can send a request by using the connection's
//! [`send_request()`] method to queue request headers; [sending] QUIC packets
//! causes the requests to get sent to the peer:
//!
//! ```no_run
//! # let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();
//! # let scid = quiche::ConnectionId::from_ref(&[0xba; 16]);
//! # let peer = "127.0.0.1:1234".parse().unwrap();
//! # let local = "127.0.0.1:4321".parse().unwrap();
//! # let mut conn = quiche::connect(None, &scid, local, peer, &mut config).unwrap();
//! # let h3_config = quiche::h3::Config::new()?;
//! # let mut h3_conn = quiche::h3::Connection::with_transport(&mut conn, &h3_config)?;
//! let req = vec![
//!     quiche::h3::Header::new(b":method", b"GET"),
//!     quiche::h3::Header::new(b":scheme", b"https"),
//!     quiche::h3::Header::new(b":authority", b"quic.tech"),
//!     quiche::h3::Header::new(b":path", b"/"),
//!     quiche::h3::Header::new(b"user-agent", b"quiche"),
//! ];
//!
//! h3_conn.send_request(&mut conn, &req, true)?;
//! # Ok::<(), quiche::h3::Error>(())
//! ```
//!
//! An HTTP/3 client can send a request with additional body data by using
//! the connection's [`send_body()`] method:
//!
//! ```no_run
//! # let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();
//! # let scid = quiche::ConnectionId::from_ref(&[0xba; 16]);
//! # let peer = "127.0.0.1:1234".parse().unwrap();
//! # let local = "127.0.0.1:4321".parse().unwrap();
//! # let mut conn = quiche::connect(None, &scid, local, peer, &mut config).unwrap();
//! # let h3_config = quiche::h3::Config::new()?;
//! # let mut h3_conn = quiche::h3::Connection::with_transport(&mut conn, &h3_config)?;
//! let req = vec![
//!     quiche::h3::Header::new(b":method", b"GET"),
//!     quiche::h3::Header::new(b":scheme", b"https"),
//!     quiche::h3::Header::new(b":authority", b"quic.tech"),
//!     quiche::h3::Header::new(b":path", b"/"),
//!     quiche::h3::Header::new(b"user-agent", b"quiche"),
//! ];
//!
//! let stream_id = h3_conn.send_request(&mut conn, &req, false)?;
//! h3_conn.send_body(&mut conn, stream_id, b"Hello World!", true)?;
//! # Ok::<(), quiche::h3::Error>(())
//! ```
//!
//! ## Handling requests and responses
//!
//! After [receiving] QUIC packets, HTTP/3 data is processed using the
//! connection's [`poll()`] method. On success, this returns an [`Event`] object
//! and an ID corresponding to the stream where the `Event` originated.
//!
//! An HTTP/3 server uses [`poll()`] to read requests and responds to them using
//! [`send_response()`] and [`send_body()`]:
//!
//! ```no_run
//! use quiche::h3::NameValue;
//!
//! # let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();
//! # let scid = quiche::ConnectionId::from_ref(&[0xba; 16]);
//! # let peer = "127.0.0.1:1234".parse().unwrap();
//! # let local = "127.0.0.1:1234".parse().unwrap();
//! # let mut conn = quiche::accept(&scid, None, local, peer, &mut config).unwrap();
//! # let h3_config = quiche::h3::Config::new()?;
//! # let mut h3_conn = quiche::h3::Connection::with_transport(&mut conn, &h3_config)?;
//! loop {
//!     match h3_conn.poll(&mut conn) {
//!         Ok((stream_id, quiche::h3::Event::Headers{list, more_frames})) => {
//!             let mut headers = list.into_iter();
//!
//!             // Look for the request's method.
//!             let method = headers.find(|h| h.name() == b":method").unwrap();
//!
//!             // Look for the request's path.
//!             let path = headers.find(|h| h.name() == b":path").unwrap();
//!
//!             if method.value() == b"GET" && path.value() == b"/" {
//!                 let resp = vec![
//!                     quiche::h3::Header::new(b":status", 200.to_string().as_bytes()),
//!                     quiche::h3::Header::new(b"server", b"quiche"),
//!                 ];
//!
//!                 h3_conn.send_response(&mut conn, stream_id, &resp, false)?;
//!                 h3_conn.send_body(&mut conn, stream_id, b"Hello World!", true)?;
//!             }
//!         },
//!
//!         Ok((stream_id, quiche::h3::Event::Data)) => {
//!             // Request body data, handle it.
//!             # return Ok(());
//!         },
//!
//!         Ok((stream_id, quiche::h3::Event::Finished)) => {
//!             // Peer terminated stream, handle it.
//!         },
//!
//!         Ok((stream_id, quiche::h3::Event::Reset(err))) => {
//!             // Peer reset the stream, handle it.
//!         },
//!
//!         Ok((_flow_id, quiche::h3::Event::PriorityUpdate)) => (),
//!
//!         Ok((goaway_id, quiche::h3::Event::GoAway)) => {
//!              // Peer signalled it is going away, handle it.
//!         },
//!
//!         Err(quiche::h3::Error::Done) => {
//!             // Done reading.
//!             break;
//!         },
//!
//!         Err(e) => {
//!             // An error occurred, handle it.
//!             break;
//!         },
//!     }
//! }
//! # Ok::<(), quiche::h3::Error>(())
//! ```
//!
//! An HTTP/3 client uses [`poll()`] to read responses:
//!
//! ```no_run
//! use quiche::h3::NameValue;
//!
//! # let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();
//! # let scid = quiche::ConnectionId::from_ref(&[0xba; 16]);
//! # let peer = "127.0.0.1:1234".parse().unwrap();
//! # let local = "127.0.0.1:1234".parse().unwrap();
//! # let mut conn = quiche::connect(None, &scid, local, peer, &mut config).unwrap();
//! # let h3_config = quiche::h3::Config::new()?;
//! # let mut h3_conn = quiche::h3::Connection::with_transport(&mut conn, &h3_config)?;
//! loop {
//!     match h3_conn.poll(&mut conn) {
//!         Ok((stream_id, quiche::h3::Event::Headers{list, more_frames})) => {
//!             let status = list.iter().find(|h| h.name() == b":status").unwrap();
//!             println!("Received {} response on stream {}",
//!                      std::str::from_utf8(status.value()).unwrap(),
//!                      stream_id);
//!         },
//!
//!         Ok((stream_id, quiche::h3::Event::Data)) => {
//!             let mut body = vec![0; 4096];
//!
//!             // Consume all body data received on the stream.
//!             while let Ok(read) =
//!                 h3_conn.recv_body(&mut conn, stream_id, &mut body)
//!             {
//!                 println!("Received {} bytes of payload on stream {}",
//!                          read, stream_id);
//!             }
//!         },
//!
//!         Ok((stream_id, quiche::h3::Event::Finished)) => {
//!             // Peer terminated stream, handle it.
//!         },
//!
//!         Ok((stream_id, quiche::h3::Event::Reset(err))) => {
//!             // Peer reset the stream, handle it.
//!         },
//!
//!         Ok((_prioritized_element_id, quiche::h3::Event::PriorityUpdate)) => (),
//!
//!         Ok((goaway_id, quiche::h3::Event::GoAway)) => {
//!              // Peer signalled it is going away, handle it.
//!         },
//!
//!         Err(quiche::h3::Error::Done) => {
//!             // Done reading.
//!             break;
//!         },
//!
//!         Err(e) => {
//!             // An error occurred, handle it.
//!             break;
//!         },
//!     }
//! }
//! # Ok::<(), quiche::h3::Error>(())
//! ```
//!
//! ## Detecting end of request or response
//!
//! A single HTTP/3 request or response may consist of several HEADERS and DATA
//! frames; it is finished when the QUIC stream is closed. Calling [`poll()`]
//! repeatedly will generate an [`Event`] for each of these. The application may
//! use these event to do additional HTTP semantic validation.
//!
//! ## HTTP/3 protocol errors
//!
//! Quiche is responsible for managing the HTTP/3 connection, ensuring it is in
//! a correct state and validating all messages received by a peer. This mainly
//! takes place in the [`poll()`] method. If an HTTP/3 error occurs, quiche will
//! close the connection and send an appropriate CONNECTION_CLOSE frame to the
//! peer. An [`Error`] is returned to the application so that it can perform any
//! required tidy up such as closing sockets.
//!
//! [`application_proto()`]: ../struct.Connection.html#method.application_proto
//! [`stream_finished()`]: ../struct.Connection.html#method.stream_finished
//! [Connection setup]: ../index.html#connection-setup
//! [sending]: ../index.html#generating-outgoing-packets
//! [receiving]: ../index.html#handling-incoming-packets
//! [`with_transport()`]: struct.Connection.html#method.with_transport
//! [`poll()`]: struct.Connection.html#method.poll
//! [`Event`]: enum.Event.html
//! [`Error`]: enum.Error.html
//! [`send_request()`]: struct.Connection.html#method.send_response
//! [`send_response()`]: struct.Connection.html#method.send_response
//! [`send_body()`]: struct.Connection.html#method.send_body

use std::collections::HashSet;
use std::collections::VecDeque;

#[cfg(feature = "sfv")]
use std::convert::TryFrom;
use std::fmt;
use std::fmt::Write;

#[cfg(feature = "qlog")]
use qlog::events::h3::H3FrameCreated;
#[cfg(feature = "qlog")]
use qlog::events::h3::H3FrameParsed;
#[cfg(feature = "qlog")]
use qlog::events::h3::H3Owner;
#[cfg(feature = "qlog")]
use qlog::events::h3::H3PriorityTargetStreamType;
#[cfg(feature = "qlog")]
use qlog::events::h3::H3StreamType;
#[cfg(feature = "qlog")]
use qlog::events::h3::H3StreamTypeSet;
#[cfg(feature = "qlog")]
use qlog::events::h3::Http3EventType;
#[cfg(feature = "qlog")]
use qlog::events::h3::Http3Frame;
#[cfg(feature = "qlog")]
use qlog::events::EventData;
#[cfg(feature = "qlog")]
use qlog::events::EventImportance;
#[cfg(feature = "qlog")]
use qlog::events::EventType;

use crate::range_buf::BufFactory;
use crate::BufSplit;

/// List of ALPN tokens of supported HTTP/3 versions.
///
/// This can be passed directly to the [`Config::set_application_protos()`]
/// method when implementing HTTP/3 applications.
///
/// [`Config::set_application_protos()`]:
/// ../struct.Config.html#method.set_application_protos
pub const APPLICATION_PROTOCOL: &[&[u8]] = &[b"h3"];

// The offset used when converting HTTP/3 urgency to quiche urgency.
const PRIORITY_URGENCY_OFFSET: u8 = 124;

// Parameter values as specified in [Extensible Priorities].
//
// [Extensible Priorities]: https://www.rfc-editor.org/rfc/rfc9218.html#section-4.
const PRIORITY_URGENCY_LOWER_BOUND: u8 = 0;
const PRIORITY_URGENCY_UPPER_BOUND: u8 = 7;
const PRIORITY_URGENCY_DEFAULT: u8 = 3;
const PRIORITY_INCREMENTAL_DEFAULT: bool = false;

#[cfg(feature = "qlog")]
const QLOG_FRAME_CREATED: EventType = EventType::Http3EventType(Http3EventType::FrameCreated);
#[cfg(feature = "qlog")]
const QLOG_FRAME_PARSED: EventType = EventType::Http3EventType(Http3EventType::FrameParsed);
#[cfg(feature = "qlog")]
const QLOG_STREAM_TYPE_SET: EventType = EventType::Http3EventType(Http3EventType::StreamTypeSet);

/// A specialized [`Result`] type for quiche HTTP/3 operations.
///
/// This type is used throughout quiche's HTTP/3 public API for any operation
/// that can produce an error.
///
/// [`Result`]: https://doc.rust-lang.org/std/result/enum.Result.html
pub type Result<T> = std::result::Result<T, Error>;

/// An HTTP/3 error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// There is no error or no work to do
    Done,

    /// The provided buffer is too short.
    BufferTooShort,

    /// Internal error in the HTTP/3 stack.
    InternalError,

    /// Endpoint detected that the peer is exhibiting behavior that causes.
    /// excessive load.
    ExcessiveLoad,

    /// Stream ID or Push ID greater that current maximum was
    /// used incorrectly, such as exceeding a limit, reducing a limit,
    /// or being reused.
    IdError,

    /// The endpoint detected that its peer created a stream that it will not
    /// accept.
    StreamCreationError,

    /// A required critical stream was closed.
    ClosedCriticalStream,

    /// No SETTINGS frame at beginning of control stream.
    MissingSettings,

    /// A frame was received which is not permitted in the current state.
    FrameUnexpected,

    /// Frame violated layout or size rules.
    FrameError,

    /// QPACK Header block decompression failure.
    QpackDecompressionFailed,

    /// Error originated from the transport layer.
    TransportError(crate::Error),

    /// The underlying QUIC stream (or connection) doesn't have enough capacity
    /// for the operation to complete. The application should retry later on.
    StreamBlocked,

    /// Error in the payload of a SETTINGS frame.
    SettingsError,

    /// Server rejected request.
    RequestRejected,

    /// Request or its response cancelled.
    RequestCancelled,

    /// Client's request stream terminated without containing a full-formed
    /// request.
    RequestIncomplete,

    /// An HTTP message was malformed and cannot be processed.
    MessageError,

    /// The TCP connection established in response to a CONNECT request was
    /// reset or abnormally closed.
    ConnectError,

    /// The requested operation cannot be served over HTTP/3. Peer should retry
    /// over HTTP/1.1.
    VersionFallback,
}

/// HTTP/3 error codes sent on the wire.
///
/// As defined in [RFC9114](https://www.rfc-editor.org/rfc/rfc9114.html#http-error-codes).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WireErrorCode {
    /// No error. This is used when the connection or stream needs to be closed,
    /// but there is no error to signal.
    NoError = 0x100,
    /// Peer violated protocol requirements in a way that does not match a more
    /// specific error code or endpoint declines to use the more specific
    /// error code.
    GeneralProtocolError = 0x101,
    /// An internal error has occurred in the HTTP stack.
    InternalError = 0x102,
    /// The endpoint detected that its peer created a stream that it will not
    /// accept.
    StreamCreationError = 0x103,
    /// A stream required by the HTTP/3 connection was closed or reset.
    ClosedCriticalStream = 0x104,
    /// A frame was received that was not permitted in the current state or on
    /// the current stream.
    FrameUnexpected = 0x105,
    /// A frame that fails to satisfy layout requirements or with an invalid
    /// size was received.
    FrameError = 0x106,
    /// The endpoint detected that its peer is exhibiting a behavior that might
    /// be generating excessive load.
    ExcessiveLoad = 0x107,
    /// A stream ID or push ID was used incorrectly, such as exceeding a limit,
    /// reducing a limit, or being reused.
    IdError = 0x108,
    /// An endpoint detected an error in the payload of a SETTINGS frame.
    SettingsError = 0x109,
    /// No SETTINGS frame was received at the beginning of the control stream.
    MissingSettings = 0x10a,
    /// A server rejected a request without performing any application
    /// processing.
    RequestRejected = 0x10b,
    /// The request or its response (including pushed response) is cancelled.
    RequestCancelled = 0x10c,
    /// The client's stream terminated without containing a fully formed
    /// request.
    RequestIncomplete = 0x10d,
    /// An HTTP message was malformed and cannot be processed.
    MessageError = 0x10e,
    /// The TCP connection established in response to a CONNECT request was
    /// reset or abnormally closed.
    ConnectError = 0x10f,
    /// The requested operation cannot be served over HTTP/3. The peer should
    /// retry over HTTP/1.1.
    VersionFallback = 0x110,
}

impl Error {
    fn to_wire(self) -> u64 {
        match self {
            Error::Done => WireErrorCode::NoError as u64,
            Error::InternalError => WireErrorCode::InternalError as u64,
            Error::StreamCreationError => WireErrorCode::StreamCreationError as u64,
            Error::ClosedCriticalStream => WireErrorCode::ClosedCriticalStream as u64,
            Error::FrameUnexpected => WireErrorCode::FrameUnexpected as u64,
            Error::FrameError => WireErrorCode::FrameError as u64,
            Error::ExcessiveLoad => WireErrorCode::ExcessiveLoad as u64,
            Error::IdError => WireErrorCode::IdError as u64,
            Error::MissingSettings => WireErrorCode::MissingSettings as u64,
            Error::QpackDecompressionFailed => 0x200,
            Error::BufferTooShort => 0x999,
            Error::TransportError { .. } | Error::StreamBlocked => 0xFF,
            Error::SettingsError => WireErrorCode::SettingsError as u64,
            Error::RequestRejected => WireErrorCode::RequestRejected as u64,
            Error::RequestCancelled => WireErrorCode::RequestCancelled as u64,
            Error::RequestIncomplete => WireErrorCode::RequestIncomplete as u64,
            Error::MessageError => WireErrorCode::MessageError as u64,
            Error::ConnectError => WireErrorCode::ConnectError as u64,
            Error::VersionFallback => WireErrorCode::VersionFallback as u64,
        }
    }

    #[cfg(feature = "ffi")]
    fn to_c(self) -> libc::ssize_t {
        match self {
            Error::Done => -1,
            Error::BufferTooShort => -2,
            Error::InternalError => -3,
            Error::ExcessiveLoad => -4,
            Error::IdError => -5,
            Error::StreamCreationError => -6,
            Error::ClosedCriticalStream => -7,
            Error::MissingSettings => -8,
            Error::FrameUnexpected => -9,
            Error::FrameError => -10,
            Error::QpackDecompressionFailed => -11,
            // -12 was previously used for TransportError, skip it
            Error::StreamBlocked => -13,
            Error::SettingsError => -14,
            Error::RequestRejected => -15,
            Error::RequestCancelled => -16,
            Error::RequestIncomplete => -17,
            Error::MessageError => -18,
            Error::ConnectError => -19,
            Error::VersionFallback => -20,

            Error::TransportError(quic_error) => quic_error.to_c() - 1000,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl From<super::Error> for Error {
    fn from(err: super::Error) -> Self {
        match err {
            super::Error::Done => Error::Done,

            _ => Error::TransportError(err),
        }
    }
}

impl From<octets::BufferTooShortError> for Error {
    fn from(_err: octets::BufferTooShortError) -> Self {
        Error::BufferTooShort
    }
}

/// An HTTP/3 configuration.
pub struct Config {
    max_field_section_size: Option<u64>,
    qpack_max_table_capacity: Option<u64>,
    qpack_blocked_streams: Option<u64>,
    connect_protocol_enabled: Option<u64>,
    /// additional settings are settings that are not part of the H3
    /// settings explicitly handled above
    additional_settings: Option<Vec<(u64, u64)>>,
}

impl Config {
    /// Creates a new configuration object with default settings.
    pub const fn new() -> Result<Config> {
        Ok(Config {
            max_field_section_size: None,
            qpack_max_table_capacity: None,
            qpack_blocked_streams: None,
            connect_protocol_enabled: None,
            additional_settings: None,
        })
    }

    /// The settings Chrome sends, in the values Chrome sends them.
    ///
    /// Measured, not guessed: `bench h3-ref` runs a QUIC server on loopback that the Chrome
    /// svipall provisions completes a handshake with, and reads its control stream. Four runs of
    /// Chrome for Testing 152.0.7977.75 produced the same four settings, in this order, with one
    /// fresh GREASE appended. `crates/svipall-quic/tests/settings.rs` asserts it offline.
    ///
    /// `h3_datagram` is not here because it is not a choice this object makes: it is `Some(1)` iff
    /// the transport enabled datagrams, and `Config::chrome` does. Saying it twice is how the two
    /// come to disagree.
    pub const fn chrome() -> Result<Config> {
        Ok(Config {
            qpack_max_table_capacity: Some(65_536),
            max_field_section_size: Some(262_144),
            qpack_blocked_streams: Some(100),
            // Chrome sends no ENABLE_CONNECT_PROTOCOL over a plain fetch, and an extra setting is
            // as visible as a missing one.
            connect_protocol_enabled: None,
            additional_settings: None,
        })
    }

    /// Sets the `SETTINGS_MAX_FIELD_SECTION_SIZE` setting.
    ///
    /// By default no limit is enforced. When a request whose headers exceed
    /// the limit set by the application is received, the call to the [`poll()`]
    /// method will return the [`Error::ExcessiveLoad`] error, and the
    /// connection will be closed.
    ///
    /// [`poll()`]: struct.Connection.html#method.poll
    /// [`Error::ExcessiveLoad`]: enum.Error.html#variant.ExcessiveLoad
    pub fn set_max_field_section_size(&mut self, v: u64) {
        self.max_field_section_size = Some(v);
    }

    /// Sets the `SETTINGS_QPACK_MAX_TABLE_CAPACITY` setting.
    ///
    /// The default value is `0`.
    pub fn set_qpack_max_table_capacity(&mut self, v: u64) {
        self.qpack_max_table_capacity = Some(v);
    }

    /// Sets the `SETTINGS_QPACK_BLOCKED_STREAMS` setting.
    ///
    /// The default value is `0`.
    pub fn set_qpack_blocked_streams(&mut self, v: u64) {
        self.qpack_blocked_streams = Some(v);
    }

    /// Sets or omits the `SETTINGS_ENABLE_CONNECT_PROTOCOL` setting.
    ///
    /// The default value is `false`.
    pub fn enable_extended_connect(&mut self, enabled: bool) {
        if enabled {
            self.connect_protocol_enabled = Some(1);
        } else {
            self.connect_protocol_enabled = None;
        }
    }

    /// Sets additional HTTP/3 settings.
    ///
    /// The default value is no additional settings.
    /// The `additional_settings` parameter must not the following
    /// settings as they are already handled by this library:
    ///
    /// - SETTINGS_QPACK_MAX_TABLE_CAPACITY
    /// - SETTINGS_MAX_FIELD_SECTION_SIZE
    /// - SETTINGS_QPACK_BLOCKED_STREAMS
    /// - SETTINGS_ENABLE_CONNECT_PROTOCOL
    /// - SETTINGS_H3_DATAGRAM
    ///
    /// If such a setting is present in the `additional_settings`,
    /// the method will return the [`Error::SettingsError`] error.
    ///
    /// If a setting identifier is present twice in `additional_settings`,
    /// the method will return the [`Error::SettingsError`] error.
    ///
    /// [`Error::SettingsError`]: enum.Error.html#variant.SettingsError
    pub fn set_additional_settings(&mut self, additional_settings: Vec<(u64, u64)>) -> Result<()> {
        let explicit_quiche_settings = HashSet::from([
            frame::SETTINGS_QPACK_MAX_TABLE_CAPACITY,
            frame::SETTINGS_MAX_FIELD_SECTION_SIZE,
            frame::SETTINGS_QPACK_BLOCKED_STREAMS,
            frame::SETTINGS_ENABLE_CONNECT_PROTOCOL,
            frame::SETTINGS_H3_DATAGRAM,
            frame::SETTINGS_H3_DATAGRAM_00,
        ]);

        let dedup_settings: HashSet<u64> =
            additional_settings.iter().map(|(key, _)| *key).collect();

        if dedup_settings.len() != additional_settings.len()
            || !explicit_quiche_settings.is_disjoint(&dedup_settings)
        {
            return Err(Error::SettingsError);
        }
        self.additional_settings = Some(additional_settings);
        Ok(())
    }
}

/// A trait for types with associated string name and value.
pub trait NameValue {
    /// Returns the object's name.
    fn name(&self) -> &[u8];

    /// Returns the object's value.
    fn value(&self) -> &[u8];
}

impl<N, V> NameValue for (N, V)
where
    N: AsRef<[u8]>,
    V: AsRef<[u8]>,
{
    fn name(&self) -> &[u8] {
        self.0.as_ref()
    }

    fn value(&self) -> &[u8] {
        self.1.as_ref()
    }
}

/// An owned name-value pair representing a raw HTTP header.
#[derive(Clone, PartialEq, Eq)]
pub struct Header(Vec<u8>, Vec<u8>);

fn try_print_as_readable(hdr: &[u8], f: &mut fmt::Formatter) -> fmt::Result {
    match std::str::from_utf8(hdr) {
        Ok(s) => f.write_str(&s.escape_default().to_string()),
        Err(_) => write!(f, "{hdr:?}"),
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char('"')?;
        try_print_as_readable(&self.0, f)?;
        f.write_str(": ")?;
        try_print_as_readable(&self.1, f)?;
        f.write_char('"')
    }
}

impl Header {
    /// Creates a new header.
    ///
    /// Both `name` and `value` will be cloned.
    pub fn new(name: &[u8], value: &[u8]) -> Self {
        Self(name.to_vec(), value.to_vec())
    }
}

impl NameValue for Header {
    fn name(&self) -> &[u8] {
        &self.0
    }

    fn value(&self) -> &[u8] {
        &self.1
    }
}

/// A non-owned name-value pair representing a raw HTTP header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderRef<'a>(&'a [u8], &'a [u8]);

impl<'a> HeaderRef<'a> {
    /// Creates a new header.
    pub const fn new(name: &'a [u8], value: &'a [u8]) -> Self {
        Self(name, value)
    }
}

impl NameValue for HeaderRef<'_> {
    fn name(&self) -> &[u8] {
        self.0
    }

    fn value(&self) -> &[u8] {
        self.1
    }
}

/// An HTTP/3 connection event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Request/response headers were received.
    Headers {
        /// The list of received header fields. The application should validate
        /// pseudo-headers and headers.
        list: Vec<Header>,

        /// Whether more frames will follow the headers on the stream.
        more_frames: bool,
    },

    /// Data was received.
    ///
    /// This indicates that the application can use the [`recv_body()`] method
    /// to retrieve the data from the stream.
    ///
    /// Note that [`recv_body()`] will need to be called repeatedly until the
    /// [`Done`] value is returned, as the event will not be re-armed until all
    /// buffered data is read.
    ///
    /// [`recv_body()`]: struct.Connection.html#method.recv_body
    /// [`Done`]: enum.Error.html#variant.Done
    Data,

    /// Stream was closed,
    Finished,

    /// Stream was reset.
    ///
    /// The associated data represents the error code sent by the peer.
    Reset(u64),

    /// PRIORITY_UPDATE was received.
    ///
    /// This indicates that the application can use the
    /// [`take_last_priority_update()`] method to take the last received
    /// PRIORITY_UPDATE for a specified stream.
    ///
    /// This event is triggered once per stream until the last PRIORITY_UPDATE
    /// is taken. It is recommended that applications defer taking the
    /// PRIORITY_UPDATE until after [`poll()`] returns [`Done`].
    ///
    /// [`take_last_priority_update()`]: struct.Connection.html#method.take_last_priority_update
    /// [`poll()`]: struct.Connection.html#method.poll
    /// [`Done`]: enum.Error.html#variant.Done
    PriorityUpdate,

    /// GOAWAY was received.
    GoAway,
}

/// Extensible Priorities parameters.
///
/// The `TryFrom` trait supports constructing this object from the serialized
/// Structured Fields Dictionary field value. I.e, use `TryFrom` to parse the
/// value of a Priority header field or a PRIORITY_UPDATE frame. Using this
/// trait requires the `sfv` feature to be enabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Priority {
    urgency: u8,
    incremental: bool,
}

impl Default for Priority {
    fn default() -> Self {
        Priority {
            urgency: PRIORITY_URGENCY_DEFAULT,
            incremental: PRIORITY_INCREMENTAL_DEFAULT,
        }
    }
}

impl Priority {
    /// Creates a new Priority.
    pub const fn new(urgency: u8, incremental: bool) -> Self {
        Priority {
            urgency,
            incremental,
        }
    }
}

#[cfg(feature = "sfv")]
#[cfg_attr(docsrs, doc(cfg(feature = "sfv")))]
impl TryFrom<&[u8]> for Priority {
    type Error = Error;

    /// Try to parse an Extensible Priority field value.
    ///
    /// The field value is expected to be a Structured Fields Dictionary; see
    /// [Extensible Priorities].
    ///
    /// If the `u` or `i` fields are contained with correct types, a constructed
    /// Priority object is returned. Note that urgency values outside of valid
    /// range (0 through 7) are clamped to 7.
    ///
    /// If the `u` or `i` fields are contained with the wrong types,
    /// Error::Done is returned.
    ///
    /// Omitted parameters will yield default values.
    ///
    /// [Extensible Priorities]: https://www.rfc-editor.org/rfc/rfc9218.html#section-4.
    fn try_from(value: &[u8]) -> std::result::Result<Self, Self::Error> {
        let dict = match sfv::Parser::parse_dictionary(value) {
            Ok(v) => v,

            Err(_) => return Err(Error::Done),
        };

        let urgency = match dict.get("u") {
            // If there is a u parameter, try to read it as an Item of type
            // Integer. If the value out of the spec's allowed range
            // (0 through 7), that's an error so set it to the upper
            // bound (lowest priority) to avoid interference with
            // other streams.
            Some(sfv::ListEntry::Item(item)) => match item.bare_item.as_int() {
                Some(v) => {
                    if !(PRIORITY_URGENCY_LOWER_BOUND as i64..=PRIORITY_URGENCY_UPPER_BOUND as i64)
                        .contains(&v)
                    {
                        PRIORITY_URGENCY_UPPER_BOUND
                    } else {
                        v as u8
                    }
                }

                None => return Err(Error::Done),
            },

            Some(sfv::ListEntry::InnerList(_)) => return Err(Error::Done),

            // Omitted so use default value.
            None => PRIORITY_URGENCY_DEFAULT,
        };

        let incremental = match dict.get("i") {
            Some(sfv::ListEntry::Item(item)) => item.bare_item.as_bool().ok_or(Error::Done)?,

            // Omitted so use default value.
            _ => false,
        };

        Ok(Priority::new(urgency, incremental))
    }
}

struct ConnectionSettings {
    pub max_field_section_size: Option<u64>,
    pub qpack_max_table_capacity: Option<u64>,
    pub qpack_blocked_streams: Option<u64>,
    pub connect_protocol_enabled: Option<u64>,
    pub h3_datagram: Option<u64>,
    pub additional_settings: Option<Vec<(u64, u64)>>,
    pub raw: Option<Vec<(u64, u64)>>,
}

#[derive(Default)]
struct QpackStreams {
    pub encoder_stream_id: Option<u64>,
    pub encoder_stream_bytes: u64,
    pub decoder_stream_id: Option<u64>,
    pub decoder_stream_bytes: u64,
}

/// Statistics about the connection.
///
/// A connection's statistics can be collected using the [`stats()`] method.
///
/// [`stats()`]: struct.Connection.html#method.stats
#[derive(Clone, Default)]
pub struct Stats {
    /// The number of bytes received on the QPACK encoder stream.
    pub qpack_encoder_stream_recv_bytes: u64,
    /// The number of bytes received on the QPACK decoder stream.
    pub qpack_decoder_stream_recv_bytes: u64,
}

fn close_conn_critical_stream<F: BufFactory>(conn: &mut super::Connection<F>) -> Result<()> {
    conn.close(
        true,
        Error::ClosedCriticalStream.to_wire(),
        b"Critical stream closed.",
    )?;

    Err(Error::ClosedCriticalStream)
}

fn close_conn_if_critical_stream_finished<F: BufFactory>(
    conn: &mut super::Connection<F>,
    stream_id: u64,
) -> Result<()> {
    if conn.stream_finished(stream_id) {
        close_conn_critical_stream(conn)?;
    }

    Ok(())
}

/// An HTTP/3 connection.
pub struct Connection {
    is_server: bool,

    next_request_stream_id: u64,
    next_uni_stream_id: u64,

    streams: crate::stream::StreamIdHashMap<stream::Stream>,

    local_settings: ConnectionSettings,
    peer_settings: ConnectionSettings,

    control_stream_id: Option<u64>,
    peer_control_stream_id: Option<u64>,

    qpack_encoder: qpack::Encoder,
    qpack_decoder: qpack::Decoder,

    local_qpack_streams: QpackStreams,
    peer_qpack_streams: QpackStreams,

    max_push_id: u64,

    finished_streams: VecDeque<u64>,

    frames_greased: bool,

    local_goaway_id: Option<u64>,
    peer_goaway_id: Option<u64>,
}

impl Connection {
    fn new(config: &Config, is_server: bool, enable_dgram: bool) -> Result<Connection> {
        let initial_uni_stream_id = if is_server { 0x3 } else { 0x2 };
        let h3_datagram = if enable_dgram { Some(1) } else { None };

        Ok(Connection {
            is_server,

            next_request_stream_id: 0,

            next_uni_stream_id: initial_uni_stream_id,

            streams: Default::default(),

            local_settings: ConnectionSettings {
                max_field_section_size: config.max_field_section_size,
                qpack_max_table_capacity: config.qpack_max_table_capacity,
                qpack_blocked_streams: config.qpack_blocked_streams,
                connect_protocol_enabled: config.connect_protocol_enabled,
                h3_datagram,
                additional_settings: config.additional_settings.clone(),
                raw: Default::default(),
            },

            peer_settings: ConnectionSettings {
                max_field_section_size: None,
                qpack_max_table_capacity: None,
                qpack_blocked_streams: None,
                h3_datagram: None,
                connect_protocol_enabled: None,
                additional_settings: Default::default(),
                raw: Default::default(),
            },

            control_stream_id: None,
            peer_control_stream_id: None,

            qpack_encoder: qpack::Encoder::new(),
            qpack_decoder: qpack::Decoder::new(),

            local_qpack_streams: Default::default(),
            peer_qpack_streams: Default::default(),

            max_push_id: 0,

            finished_streams: VecDeque::new(),

            frames_greased: false,

            local_goaway_id: None,
            peer_goaway_id: None,
        })
    }

    /// Creates a new HTTP/3 connection using the provided QUIC connection.
    ///
    /// This will also initiate the HTTP/3 handshake with the peer by opening
    /// all control streams (including QPACK) and sending the local settings.
    ///
    /// On success the new connection is returned.
    ///
    /// The [`StreamLimit`] error is returned when the HTTP/3 control stream
    /// cannot be created due to stream limits.
    ///
    /// The [`InternalError`] error is returned when either the underlying QUIC
    /// connection is not in a suitable state, or the HTTP/3 control stream
    /// cannot be created due to flow control limits.
    ///
    /// [`StreamLimit`]: ../enum.Error.html#variant.StreamLimit
    /// [`InternalError`]: ../enum.Error.html#variant.InternalError
    pub fn with_transport<F: BufFactory>(
        conn: &mut super::Connection<F>,
        config: &Config,
    ) -> Result<Connection> {
        let is_client = !conn.is_server;
        if is_client && !(conn.is_established() || conn.is_in_early_data()) {
            trace!("{} QUIC connection must be established or in early data before creating an HTTP/3 connection", conn.trace_id());
            return Err(Error::InternalError);
        }

        let mut http3_conn = Connection::new(config, conn.is_server, conn.dgram_enabled())?;

        match http3_conn.send_settings(conn) {
            Ok(_) => (),

            Err(e) => {
                conn.close(true, e.to_wire(), b"Error opening control stream")?;
                return Err(e);
            }
        };

        // Try opening QPACK streams, but ignore errors if it fails since we
        // don't need them right now.
        http3_conn.open_qpack_encoder_stream(conn).ok();
        http3_conn.open_qpack_decoder_stream(conn).ok();

        if conn.grease {
            // Try opening a GREASE stream, but ignore errors since it's not
            // critical.
            http3_conn.open_grease_stream(conn).ok();
        }

        Ok(http3_conn)
    }

    /// Sends an HTTP/3 request.
    ///
    /// The request is encoded from the provided list of headers without a
    /// body, and sent on a newly allocated stream. To include a body,
    /// set `fin` as `false` and subsequently call [`send_body()`] with the
    /// same `conn` and the `stream_id` returned from this method.
    ///
    /// On success the newly allocated stream ID is returned.
    ///
    /// The [`StreamBlocked`] error is returned when the underlying QUIC stream
    /// doesn't have enough capacity for the operation to complete. When this
    /// happens the application should retry the operation once the stream is
    /// reported as writable again.
    ///
    /// [`send_body()`]: struct.Connection.html#method.send_body
    /// [`StreamBlocked`]: enum.Error.html#variant.StreamBlocked
    pub fn send_request<T: NameValue, F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
        headers: &[T],
        fin: bool,
    ) -> Result<u64> {
        // If we received a GOAWAY from the peer, MUST NOT initiate new
        // requests.
        if self.peer_goaway_id.is_some() {
            return Err(Error::FrameUnexpected);
        }

        let stream_id = self.next_request_stream_id;

        self.streams
            .insert(stream_id, <stream::Stream>::new(stream_id, true));

        // The underlying QUIC stream does not exist yet, so calls to e.g.
        // stream_capacity() will fail. By writing a 0-length buffer, we force
        // the creation of the QUIC stream state, without actually writing
        // anything.
        if let Err(e) = conn.stream_send(stream_id, b"", false) {
            self.streams.remove(&stream_id);

            if e == super::Error::Done {
                return Err(Error::StreamBlocked);
            }

            return Err(e.into());
        };

        self.send_headers(conn, stream_id, headers, fin)?;

        // To avoid skipping stream IDs, we only calculate the next available
        // stream ID when a request has been successfully buffered.
        self.next_request_stream_id = self
            .next_request_stream_id
            .checked_add(4)
            .ok_or(Error::IdError)?;

        Ok(stream_id)
    }

    /// Sends an HTTP/3 response on the specified stream with default priority.
    ///
    /// This method sends the provided `headers` as a single initial response
    /// without a body.
    ///
    /// To send a non-final 1xx, then a final 200+ without body:
    ///   * send_response() with `fin` set to `false`.
    ///   * [`send_additional_headers()`] with fin set to `true` using the same
    ///     `stream_id` value.
    ///
    /// To send a non-final 1xx, then a final 200+ with body:
    ///   * send_response() with `fin` set to `false`.
    ///   * [`send_additional_headers()`] with fin set to `false` and same
    ///     `stream_id` value.
    ///   * [`send_body()`] with same `stream_id`.
    ///
    /// To send a final 200+ with body:
    ///   * send_response() with `fin` set to `false`.
    ///   * [`send_body()`] with same `stream_id`.
    ///
    /// Additional headers can only be sent during certain phases of an HTTP/3
    /// message exchange, see [Section 4.1 of RFC 9114]. The [`FrameUnexpected`]
    /// error is returned if this method, or [`send_response_with_priority()`],
    /// are called multiple times with the same `stream_id` value.
    ///
    /// The [`StreamBlocked`] error is returned when the underlying QUIC stream
    /// doesn't have enough capacity for the operation to complete. When this
    /// happens the application should retry the operation once the stream is
    /// reported as writable again.
    ///
    /// [`send_body()`]: struct.Connection.html#method.send_body
    /// [`send_additional_headers()`]:
    ///     struct.Connection.html#method.send_additional_headers
    /// [`send_response_with_priority()`]:
    ///     struct.Connection.html#method.send_response_with_priority
    /// [`FrameUnexpected`]: enum.Error.html#variant.FrameUnexpected
    /// [`StreamBlocked`]: enum.Error.html#variant.StreamBlocked
    pub fn send_response<T: NameValue, F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
        stream_id: u64,
        headers: &[T],
        fin: bool,
    ) -> Result<()> {
        let priority = Default::default();

        self.send_response_with_priority(conn, stream_id, headers, &priority, fin)?;

        Ok(())
    }

    /// Sends an HTTP/3 response on the specified stream with specified
    /// priority.
    ///
    /// This method sends the provided `headers` as a single initial response
    /// without a body.
    ///
    /// To send a non-final 1xx, then a final 200+ without body:
    ///   * send_response_with_priority() with `fin` set to `false`.
    ///   * [`send_additional_headers()`] with fin set to `true` using the same
    ///     `stream_id` value.
    ///
    /// To send a non-final 1xx, then a final 200+ with body:
    ///   * send_response_with_priority() with `fin` set to `false`.
    ///   * [`send_additional_headers()`] with fin set to `false` and same
    ///     `stream_id` value.
    ///   * [`send_body()`] with same `stream_id`.
    ///
    /// To send a final 200+ with body:
    ///   * send_response_with_priority() with `fin` set to `false`.
    ///   * [`send_body()`] with same `stream_id`.
    ///
    /// The `priority` parameter represents [Extensible Priority]
    /// parameters. If the urgency is outside the range 0-7, it will be clamped
    /// to 7.
    ///
    /// Additional headers can only be sent during certain phases of an HTTP/3
    /// message exchange, see [Section 4.1 of RFC 9114]. The [`FrameUnexpected`]
    /// error is returned if this method, or [`send_response()`],
    /// are called multiple times with the same `stream_id` value.
    ///
    /// The [`StreamBlocked`] error is returned when the underlying QUIC stream
    /// doesn't have enough capacity for the operation to complete. When this
    /// happens the application should retry the operation once the stream is
    /// reported as writable again.
    ///
    /// [`send_body()`]: struct.Connection.html#method.send_body
    /// [`send_additional_headers()`]:
    ///     struct.Connection.html#method.send_additional_headers
    /// [`send_response()`]:
    ///     struct.Connection.html#method.send_response
    /// [`FrameUnexpected`]: enum.Error.html#variant.FrameUnexpected
    /// [`StreamBlocked`]: enum.Error.html#variant.StreamBlocked
    /// [Extensible Priority]: https://www.rfc-editor.org/rfc/rfc9218.html#section-4.
    pub fn send_response_with_priority<T: NameValue, F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
        stream_id: u64,
        headers: &[T],
        priority: &Priority,
        fin: bool,
    ) -> Result<()> {
        match self.streams.get(&stream_id) {
            Some(s) => {
                // Only one initial HEADERS allowed.
                if s.local_initialized() {
                    return Err(Error::FrameUnexpected);
                }

                s
            }

            None => return Err(Error::FrameUnexpected),
        };

        self.send_headers(conn, stream_id, headers, fin)?;

        // Clamp and shift urgency into quiche-priority space
        let urgency = priority
            .urgency
            .clamp(PRIORITY_URGENCY_LOWER_BOUND, PRIORITY_URGENCY_UPPER_BOUND)
            + PRIORITY_URGENCY_OFFSET;

        conn.stream_priority(stream_id, urgency, priority.incremental)?;

        Ok(())
    }

    /// Sends additional HTTP/3 headers.
    ///
    /// After the initial request or response headers have been sent, using
    /// [`send_request()`] or [`send_response()`] respectively, this method can
    /// be used send an additional HEADERS frame. For example, to send a single
    /// instance of trailers after a request with a body, or to issue another
    /// non-final 1xx after a preceding 1xx, or to issue a final response after
    /// a preceding 1xx.
    ///
    /// Additional headers can only be sent during certain phases of an HTTP/3
    /// message exchange, see [Section 4.1 of RFC 9114]. The [`FrameUnexpected`]
    /// error is returned when this method is called during the wrong phase,
    /// such as before initial headers have been sent, or if trailers have
    /// already been sent.
    ///
    /// The [`StreamBlocked`] error is returned when the underlying QUIC stream
    /// doesn't have enough capacity for the operation to complete. When this
    /// happens the application should retry the operation once the stream is
    /// reported as writable again.
    ///
    /// [`send_request()`]: struct.Connection.html#method.send_request
    /// [`send_response()`]: struct.Connection.html#method.send_response
    /// [`StreamBlocked`]: enum.Error.html#variant.StreamBlocked
    /// [`FrameUnexpected`]: enum.Error.html#variant.FrameUnexpected
    /// [Section 4.1 of RFC 9114]:
    ///     https://www.rfc-editor.org/rfc/rfc9114.html#section-4.1.
    pub fn send_additional_headers<T: NameValue, F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
        stream_id: u64,
        headers: &[T],
        is_trailer_section: bool,
        fin: bool,
    ) -> Result<()> {
        // Clients can only send trailer headers.
        if !self.is_server && !is_trailer_section {
            return Err(Error::FrameUnexpected);
        }

        match self.streams.get(&stream_id) {
            Some(s) => {
                // Initial HEADERS must have been sent.
                if !s.local_initialized() {
                    return Err(Error::FrameUnexpected);
                }

                // Only one trailing HEADERS allowed.
                if s.trailers_sent() {
                    return Err(Error::FrameUnexpected);
                }

                s
            }

            None => return Err(Error::FrameUnexpected),
        };

        self.send_headers(conn, stream_id, headers, fin)?;

        if is_trailer_section {
            // send_headers() might have tidied the stream away, so we need to
            // check again.
            if let Some(s) = self.streams.get_mut(&stream_id) {
                s.mark_trailers_sent();
            }
        }

        Ok(())
    }

    /// Sends additional HTTP/3 headers with specified priority.
    ///
    /// After the initial request or response headers have been sent, using
    /// [`send_request()`] or [`send_response()`] respectively, this method can
    /// be used send an additional HEADERS frame. For example, to send a single
    /// instance of trailers after a request with a body, or to issue another
    /// non-final 1xx after a preceding 1xx, or to issue a final response after
    /// a preceding 1xx.
    ///
    /// The `priority` parameter represents [Extensible Priority]
    /// parameters. If the urgency is outside the range 0-7, it will be clamped
    /// to 7.
    ///
    /// Additional headers can only be sent during certain phases of an HTTP/3
    /// message exchange, see [Section 4.1 of RFC 9114]. The [`FrameUnexpected`]
    /// error is returned when this method is called during the wrong phase,
    /// such as before initial headers have been sent, or if trailers have
    /// already been sent.
    ///
    /// The [`StreamBlocked`] error is returned when the underlying QUIC stream
    /// doesn't have enough capacity for the operation to complete. When this
    /// happens the application should retry the operation once the stream is
    /// reported as writable again.
    ///
    /// [`send_request()`]: struct.Connection.html#method.send_request
    /// [`send_response()`]: struct.Connection.html#method.send_response
    /// [`StreamBlocked`]: enum.Error.html#variant.StreamBlocked
    /// [`FrameUnexpected`]: enum.Error.html#variant.FrameUnexpected
    /// [Section 4.1 of RFC 9114]:
    ///     https://www.rfc-editor.org/rfc/rfc9114.html#section-4.1.
    /// [Extensible Priority]: https://www.rfc-editor.org/rfc/rfc9218.html#section-4.
    pub fn send_additional_headers_with_priority<T: NameValue, F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
        stream_id: u64,
        headers: &[T],
        priority: &Priority,
        is_trailer_section: bool,
        fin: bool,
    ) -> Result<()> {
        self.send_additional_headers(conn, stream_id, headers, is_trailer_section, fin)?;

        // Clamp and shift urgency into quiche-priority space
        let urgency = priority
            .urgency
            .clamp(PRIORITY_URGENCY_LOWER_BOUND, PRIORITY_URGENCY_UPPER_BOUND)
            + PRIORITY_URGENCY_OFFSET;

        conn.stream_priority(stream_id, urgency, priority.incremental)?;

        Ok(())
    }

    fn encode_header_block<T: NameValue>(&mut self, headers: &[T]) -> Result<Vec<u8>> {
        let headers_len = headers
            .iter()
            .fold(0, |acc, h| acc + h.value().len() + h.name().len() + 32);

        let mut header_block = vec![0; headers_len];
        let len = self
            .qpack_encoder
            .encode(headers, &mut header_block)
            .map_err(|_| Error::InternalError)?;

        header_block.truncate(len);

        Ok(header_block)
    }

    fn send_headers<T: NameValue, F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
        stream_id: u64,
        headers: &[T],
        fin: bool,
    ) -> Result<()> {
        let mut d = [42; 10];
        let mut b = octets::OctetsMut::with_slice(&mut d);

        if !self.frames_greased && conn.grease {
            self.send_grease_frames(conn, stream_id)?;
            self.frames_greased = true;
        }

        let header_block = self.encode_header_block(headers)?;

        let overhead = octets::varint_len(frame::HEADERS_FRAME_TYPE_ID)
            + octets::varint_len(header_block.len() as u64);

        // Headers need to be sent atomically, so make sure the stream has
        // enough capacity.
        match conn.stream_writable(stream_id, overhead + header_block.len()) {
            Ok(true) => (),

            Ok(false) => return Err(Error::StreamBlocked),

            Err(e) => {
                if conn.stream_finished(stream_id) {
                    self.streams.remove(&stream_id);
                }

                return Err(e.into());
            }
        };

        b.put_varint(frame::HEADERS_FRAME_TYPE_ID)?;
        b.put_varint(header_block.len() as u64)?;
        let off = b.off();
        conn.stream_send(stream_id, &d[..off], false)?;

        // Sending header block separately avoids unnecessary copy.
        conn.stream_send(stream_id, &header_block, fin)?;

        trace!(
            "{} tx frm HEADERS stream={} len={} fin={}",
            conn.trace_id(),
            stream_id,
            header_block.len(),
            fin
        );

        qlog_with_type!(QLOG_FRAME_CREATED, conn.qlog, q, {
            let qlog_headers = headers
                .iter()
                .map(|h| qlog::events::h3::HttpHeader {
                    name: String::from_utf8_lossy(h.name()).into_owned(),
                    value: String::from_utf8_lossy(h.value()).into_owned(),
                })
                .collect();

            let frame = Http3Frame::Headers {
                headers: qlog_headers,
            };
            let ev_data = EventData::H3FrameCreated(H3FrameCreated {
                stream_id,
                length: Some(header_block.len() as u64),
                frame,
                ..Default::default()
            });

            q.add_event_data_now(ev_data).ok();
        });

        if let Some(s) = self.streams.get_mut(&stream_id) {
            s.initialize_local();
        }

        if fin && conn.stream_finished(stream_id) {
            self.streams.remove(&stream_id);
        }

        Ok(())
    }

    /// Sends an HTTP/3 body chunk on the given stream.
    ///
    /// On success the number of bytes written is returned, or [`Done`] if no
    /// bytes could be written (e.g. because the stream is blocked).
    ///
    /// Note that the number of written bytes returned can be lower than the
    /// length of the input buffer when the underlying QUIC stream doesn't have
    /// enough capacity for the operation to complete.
    ///
    /// When a partial write happens (including when [`Done`] is returned) the
    /// application should retry the operation once the stream is reported as
    /// writable again.
    ///
    /// [`Done`]: enum.Error.html#variant.Done
    pub fn send_body<F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
        stream_id: u64,
        body: &[u8],
        fin: bool,
    ) -> Result<usize> {
        self.do_send_body(
            conn,
            stream_id,
            body,
            fin,
            |conn: &mut super::Connection<F>,
             header: &[u8],
             stream_id: u64,
             body: &[u8],
             body_len: usize,
             fin: bool| {
                conn.stream_send(stream_id, header, false)?;
                Ok(conn
                    .stream_send(stream_id, &body[..body_len], fin)
                    .map(|v| (v, v))?)
            },
        )
    }

    /// Sends an HTTP/3 body chunk provided as a raw buffer on the given stream.
    ///
    /// If the capacity allows it the buffer will be appended to the stream's
    /// send queue with zero copying.
    ///
    /// On success the number of bytes written is returned, or [`Done`] if no
    /// bytes could be written (e.g. because the stream is blocked).
    ///
    /// Note that the number of written bytes returned can be lower than the
    /// length of the input buffer when the underlying QUIC stream doesn't have
    /// enough capacity for the operation to complete.
    ///
    /// When a partial write happens (including when [`Done`] is returned) the
    /// remaining (unwrittent) buffer will also be returned. The application
    /// should retry the operation once the stream is reported as writable
    /// again.
    ///
    /// [`Done`]: enum.Error.html#variant.Done
    pub fn send_body_zc<F>(
        &mut self,
        conn: &mut super::Connection<F>,
        stream_id: u64,
        body: &mut F::Buf,
        fin: bool,
    ) -> Result<usize>
    where
        F: BufFactory,
        F::Buf: BufSplit,
    {
        self.do_send_body(
            conn,
            stream_id,
            body,
            fin,
            |conn: &mut super::Connection<F>,
             header: &[u8],
             stream_id: u64,
             body: &mut F::Buf,
             mut body_len: usize,
             fin: bool| {
                let with_prefix = body.try_add_prefix(header);
                if !with_prefix {
                    conn.stream_send(stream_id, header, false)?;
                } else {
                    body_len += header.len();
                }

                let (mut n, rem) =
                    conn.stream_send_zc(stream_id, body.clone(), Some(body_len), fin)?;

                if with_prefix {
                    n -= header.len();
                }

                if let Some(rem) = rem {
                    let _ = std::mem::replace(body, rem);
                }

                Ok((n, n))
            },
        )
    }

    fn do_send_body<F, B, R, SND>(
        &mut self,
        conn: &mut super::Connection<F>,
        stream_id: u64,
        body: B,
        fin: bool,
        write_fn: SND,
    ) -> Result<R>
    where
        F: BufFactory,
        B: AsRef<[u8]>,
        SND: FnOnce(&mut super::Connection<F>, &[u8], u64, B, usize, bool) -> Result<(usize, R)>,
    {
        let mut d = [42; 10];
        let mut b = octets::OctetsMut::with_slice(&mut d);

        let len = body.as_ref().len();

        // Validate that it is sane to send data on the stream.
        if stream_id % 4 != 0 {
            return Err(Error::FrameUnexpected);
        }

        match self.streams.get_mut(&stream_id) {
            Some(s) => {
                if !s.local_initialized() {
                    return Err(Error::FrameUnexpected);
                }

                if s.trailers_sent() {
                    return Err(Error::FrameUnexpected);
                }
            }

            None => {
                return Err(Error::FrameUnexpected);
            }
        };

        // Avoid sending 0-length DATA frames when the fin flag is false.
        if len == 0 && !fin {
            return Err(Error::Done);
        }

        let overhead =
            octets::varint_len(frame::DATA_FRAME_TYPE_ID) + octets::varint_len(len as u64);

        let stream_cap = match conn.stream_capacity(stream_id) {
            Ok(v) => v,

            Err(e) => {
                if conn.stream_finished(stream_id) {
                    self.streams.remove(&stream_id);
                }

                return Err(e.into());
            }
        };

        // Make sure there is enough capacity to send the DATA frame header.
        if stream_cap < overhead {
            let _ = conn.stream_writable(stream_id, overhead + 1);
            return Err(Error::Done);
        }

        // Cap the frame payload length to the stream's capacity.
        let body_len = std::cmp::min(len, stream_cap - overhead);

        // If we can't send the entire body, set the fin flag to false so the
        // application can try again later.
        let fin = if body_len != len { false } else { fin };

        // Again, avoid sending 0-length DATA frames when the fin flag is false.
        if body_len == 0 && !fin {
            let _ = conn.stream_writable(stream_id, overhead + 1);
            return Err(Error::Done);
        }

        b.put_varint(frame::DATA_FRAME_TYPE_ID)?;
        b.put_varint(body_len as u64)?;
        let off = b.off();

        // Return how many bytes were written, excluding the frame header.
        // Sending body separately avoids unnecessary copy.
        let (written, ret) = write_fn(conn, &d[..off], stream_id, body, body_len, fin)?;

        trace!(
            "{} tx frm DATA stream={} len={} fin={}",
            conn.trace_id(),
            stream_id,
            written,
            fin
        );

        qlog_with_type!(QLOG_FRAME_CREATED, conn.qlog, q, {
            let frame = Http3Frame::Data { raw: None };
            let ev_data = EventData::H3FrameCreated(H3FrameCreated {
                stream_id,
                length: Some(written as u64),
                frame,
                ..Default::default()
            });

            q.add_event_data_now(ev_data).ok();
        });

        if written < len {
            // Ensure the peer is notified that the connection or stream is
            // blocked when the stream's capacity is limited by flow control.
            //
            // We only need enough capacity to send a few bytes, to make sure
            // the stream doesn't hang due to congestion window not growing
            // enough.
            let _ = conn.stream_writable(stream_id, overhead + 1);
        }

        if fin && written == len && conn.stream_finished(stream_id) {
            self.streams.remove(&stream_id);
        }

        Ok(ret)
    }

    /// Returns whether the peer enabled HTTP/3 DATAGRAM frame support.
    ///
    /// Support is signalled by the peer's SETTINGS, so this method always
    /// returns false until they have been processed using the [`poll()`]
    /// method.
    ///
    /// [`poll()`]: struct.Connection.html#method.poll
    pub fn dgram_enabled_by_peer<F: BufFactory>(&self, conn: &super::Connection<F>) -> bool {
        self.peer_settings.h3_datagram == Some(1) && conn.dgram_max_writable_len().is_some()
    }

    /// Returns whether the peer enabled extended CONNECT support.
    ///
    /// Support is signalled by the peer's SETTINGS, so this method always
    /// returns false until they have been processed using the [`poll()`]
    /// method.
    ///
    /// [`poll()`]: struct.Connection.html#method.poll
    pub fn extended_connect_enabled_by_peer(&self) -> bool {
        self.peer_settings.connect_protocol_enabled == Some(1)
    }

    /// Reads request or response body data into the provided buffer.
    ///
    /// Applications should call this method whenever the [`poll()`] method
    /// returns a [`Data`] event.
    ///
    /// On success the amount of bytes read is returned, or [`Done`] if there
    /// is no data to read.
    ///
    /// [`poll()`]: struct.Connection.html#method.poll
    /// [`Data`]: enum.Event.html#variant.Data
    /// [`Done`]: enum.Error.html#variant.Done
    pub fn recv_body<F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
        stream_id: u64,
        out: &mut [u8],
    ) -> Result<usize> {
        let mut total = 0;

        // Try to consume all buffered data for the stream, even across multiple
        // DATA frames.
        while total < out.len() {
            let stream = self.streams.get_mut(&stream_id).ok_or(Error::Done)?;

            if stream.state() != stream::State::Data {
                break;
            }

            let (read, fin) = match stream.try_consume_data(conn, &mut out[total..]) {
                Ok(v) => v,

                Err(Error::Done) => break,

                Err(e) => return Err(e),
            };

            total += read;

            // No more data to read, we are done.
            if read == 0 || fin {
                break;
            }

            // Process incoming data from the stream. For example, if a whole
            // DATA frame was consumed, and another one is queued behind it,
            // this will ensure the additional data will also be returned to
            // the application.
            match self.process_readable_stream(conn, stream_id, false) {
                Ok(_) => unreachable!(),

                Err(Error::Done) => (),

                Err(e) => return Err(e),
            };

            if conn.stream_finished(stream_id) {
                break;
            }
        }

        // While body is being received, the stream is marked as finished only
        // when all data is read by the application.
        if conn.stream_finished(stream_id) {
            self.process_finished_stream(stream_id);
        }

        if total == 0 {
            return Err(Error::Done);
        }

        Ok(total)
    }

    /// Sends a PRIORITY_UPDATE frame on the control stream with specified
    /// request stream ID and priority.
    ///
    /// The `priority` parameter represents [Extensible Priority]
    /// parameters. If the urgency is outside the range 0-7, it will be clamped
    /// to 7.
    ///
    /// The [`StreamBlocked`] error is returned when the underlying QUIC stream
    /// doesn't have enough capacity for the operation to complete. When this
    /// happens the application should retry the operation once the stream is
    /// reported as writable again.
    ///
    /// [`StreamBlocked`]: enum.Error.html#variant.StreamBlocked
    /// [Extensible Priority]: https://www.rfc-editor.org/rfc/rfc9218.html#section-4.
    pub fn send_priority_update_for_request<F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
        stream_id: u64,
        priority: &Priority,
    ) -> Result<()> {
        let mut d = [42; 20];
        let mut b = octets::OctetsMut::with_slice(&mut d);

        // Validate that it is sane to send PRIORITY_UPDATE.
        if self.is_server {
            return Err(Error::FrameUnexpected);
        }

        if stream_id % 4 != 0 {
            return Err(Error::FrameUnexpected);
        }

        let control_stream_id = self.control_stream_id.ok_or(Error::FrameUnexpected)?;

        let urgency = priority
            .urgency
            .clamp(PRIORITY_URGENCY_LOWER_BOUND, PRIORITY_URGENCY_UPPER_BOUND);

        let mut field_value = format!("u={urgency}");

        if priority.incremental {
            field_value.push_str(",i");
        }

        let priority_field_value = field_value.as_bytes();
        let frame_payload_len = octets::varint_len(stream_id) + priority_field_value.len();

        let overhead = octets::varint_len(frame::PRIORITY_UPDATE_FRAME_REQUEST_TYPE_ID)
            + octets::varint_len(stream_id)
            + octets::varint_len(frame_payload_len as u64);

        // Make sure the control stream has enough capacity.
        match conn.stream_writable(control_stream_id, overhead + priority_field_value.len()) {
            Ok(true) => (),

            Ok(false) => return Err(Error::StreamBlocked),

            Err(e) => {
                return Err(e.into());
            }
        }

        b.put_varint(frame::PRIORITY_UPDATE_FRAME_REQUEST_TYPE_ID)?;
        b.put_varint(frame_payload_len as u64)?;
        b.put_varint(stream_id)?;
        let off = b.off();
        conn.stream_send(control_stream_id, &d[..off], false)?;

        // Sending field value separately avoids unnecessary copy.
        conn.stream_send(control_stream_id, priority_field_value, false)?;

        trace!(
            "{} tx frm PRIORITY_UPDATE request_stream={} priority_field_value={}",
            conn.trace_id(),
            stream_id,
            field_value,
        );

        qlog_with_type!(QLOG_FRAME_CREATED, conn.qlog, q, {
            let frame = Http3Frame::PriorityUpdate {
                target_stream_type: H3PriorityTargetStreamType::Request,
                prioritized_element_id: stream_id,
                priority_field_value: field_value.clone(),
            };

            let ev_data = EventData::H3FrameCreated(H3FrameCreated {
                stream_id,
                length: Some(priority_field_value.len() as u64),
                frame,
                ..Default::default()
            });

            q.add_event_data_now(ev_data).ok();
        });

        Ok(())
    }

    /// Take the last PRIORITY_UPDATE for a prioritized element ID.
    ///
    /// When the [`poll()`] method returns a [`PriorityUpdate`] event for a
    /// prioritized element, the event has triggered and will not rearm until
    /// applications call this method. It is recommended that applications defer
    /// taking the PRIORITY_UPDATE until after [`poll()`] returns [`Done`].
    ///
    /// On success the Priority Field Value is returned, or [`Done`] if there is
    /// no PRIORITY_UPDATE to read (either because there is no value to take, or
    /// because the prioritized element does not exist).
    ///
    /// [`poll()`]: struct.Connection.html#method.poll
    /// [`PriorityUpdate`]: enum.Event.html#variant.PriorityUpdate
    /// [`Done`]: enum.Error.html#variant.Done
    pub fn take_last_priority_update(&mut self, prioritized_element_id: u64) -> Result<Vec<u8>> {
        if let Some(stream) = self.streams.get_mut(&prioritized_element_id) {
            return stream.take_last_priority_update().ok_or(Error::Done);
        }

        Err(Error::Done)
    }

    /// Processes HTTP/3 data received from the peer.
    ///
    /// On success it returns an [`Event`] and an ID, or [`Done`] when there are
    /// no events to report.
    ///
    /// Note that all events are edge-triggered, meaning that once reported they
    /// will not be reported again by calling this method again, until the event
    /// is re-armed.
    ///
    /// The events [`Headers`], [`Data`] and [`Finished`] return a stream ID,
    /// which is used in methods [`recv_body()`], [`send_response()`] or
    /// [`send_body()`].
    ///
    /// The event [`GoAway`] returns an ID that depends on the connection role.
    /// A client receives the largest processed stream ID. A server receives the
    /// the largest permitted push ID.
    ///
    /// The event [`PriorityUpdate`] only occurs at servers. It returns a
    /// prioritized element ID that is used in the method
    /// [`take_last_priority_update()`], which rearms the event for that ID.
    ///
    /// If an error occurs while processing data, the connection is closed with
    /// the appropriate error code, using the transport's [`close()`] method.
    ///
    /// [`Event`]: enum.Event.html
    /// [`Done`]: enum.Error.html#variant.Done
    /// [`Headers`]: enum.Event.html#variant.Headers
    /// [`Data`]: enum.Event.html#variant.Data
    /// [`Finished`]: enum.Event.html#variant.Finished
    /// [`GoAway`]: enum.Event.html#variant.GoAWay
    /// [`PriorityUpdate`]: enum.Event.html#variant.PriorityUpdate
    /// [`recv_body()`]: struct.Connection.html#method.recv_body
    /// [`send_response()`]: struct.Connection.html#method.send_response
    /// [`send_body()`]: struct.Connection.html#method.send_body
    /// [`recv_dgram()`]: struct.Connection.html#method.recv_dgram
    /// [`take_last_priority_update()`]: struct.Connection.html#method.take_last_priority_update
    /// [`close()`]: ../struct.Connection.html#method.close
    pub fn poll<F: BufFactory>(&mut self, conn: &mut super::Connection<F>) -> Result<(u64, Event)> {
        // When connection close is initiated by the local application (e.g. due
        // to a protocol error), the connection itself might be in a broken
        // state, so return early.
        if conn.local_error.is_some() {
            return Err(Error::Done);
        }

        // Process control streams first.
        if let Some(stream_id) = self.peer_control_stream_id {
            match self.process_control_stream(conn, stream_id) {
                Ok(ev) => return Ok(ev),

                Err(Error::Done) => (),

                Err(e) => return Err(e),
            };
        }

        if let Some(stream_id) = self.peer_qpack_streams.encoder_stream_id {
            match self.process_control_stream(conn, stream_id) {
                Ok(ev) => return Ok(ev),

                Err(Error::Done) => (),

                Err(e) => return Err(e),
            };
        }

        if let Some(stream_id) = self.peer_qpack_streams.decoder_stream_id {
            match self.process_control_stream(conn, stream_id) {
                Ok(ev) => return Ok(ev),

                Err(Error::Done) => (),

                Err(e) => return Err(e),
            };
        }

        // Process finished streams list.
        if let Some(finished) = self.finished_streams.pop_front() {
            return Ok((finished, Event::Finished));
        }

        // Process HTTP/3 data from readable streams.
        for s in conn.readable() {
            trace!("{} stream id {} is readable", conn.trace_id(), s);

            let ev = match self.process_readable_stream(conn, s, true) {
                Ok(v) => Some(v),

                Err(Error::Done) => None,

                // Return early if the stream was reset, to avoid returning
                // a Finished event later as well.
                Err(Error::TransportError(crate::Error::StreamReset(e))) => {
                    return Ok((s, Event::Reset(e)))
                }

                Err(e) => return Err(e),
            };

            if conn.stream_finished(s) {
                self.process_finished_stream(s);
            }

            // TODO: check if stream is completed so it can be freed
            if let Some(ev) = ev {
                return Ok(ev);
            }
        }

        // Process finished streams list once again, to make sure `Finished`
        // events are returned when receiving empty stream frames with the fin
        // flag set.
        if let Some(finished) = self.finished_streams.pop_front() {
            if conn.stream_readable(finished) {
                // The stream is finished, but is still readable, it may
                // indicate that there is a pending error, such as reset.
                if let Err(crate::Error::StreamReset(e)) = conn.stream_recv(finished, &mut []) {
                    return Ok((finished, Event::Reset(e)));
                }
            }
            return Ok((finished, Event::Finished));
        }

        Err(Error::Done)
    }

    /// Sends a GOAWAY frame to initiate graceful connection closure.
    ///
    /// When quiche is used in the server role, the `id` parameter is the stream
    /// ID of the highest processed request. This can be any valid ID between 0
    /// and 2^62-4. However, the ID cannot be increased. Failure to satisfy
    /// these conditions will return an error.
    ///
    /// This method does not close the QUIC connection. Applications are
    /// required to call [`close()`] themselves.
    ///
    /// [`close()`]: ../struct.Connection.html#method.close
    pub fn send_goaway<F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
        id: u64,
    ) -> Result<()> {
        let mut id = id;

        // TODO: server push
        //
        // In the meantime always send 0 from client.
        if !self.is_server {
            id = 0;
        }

        if self.is_server && id % 4 != 0 {
            return Err(Error::IdError);
        }

        if let Some(sent_id) = self.local_goaway_id {
            if id > sent_id {
                return Err(Error::IdError);
            }
        }

        if let Some(stream_id) = self.control_stream_id {
            let mut d = [42; 10];
            let mut b = octets::OctetsMut::with_slice(&mut d);

            let frame = frame::Frame::GoAway { id };

            let wire_len = frame.to_bytes(&mut b)?;
            let stream_cap = conn.stream_capacity(stream_id)?;

            if stream_cap < wire_len {
                return Err(Error::StreamBlocked);
            }

            trace!("{} tx frm {:?}", conn.trace_id(), frame);

            qlog_with_type!(QLOG_FRAME_CREATED, conn.qlog, q, {
                let ev_data = EventData::H3FrameCreated(H3FrameCreated {
                    stream_id,
                    length: Some(octets::varint_len(id) as u64),
                    frame: frame.to_qlog(),
                    ..Default::default()
                });

                q.add_event_data_now(ev_data).ok();
            });

            let off = b.off();
            conn.stream_send(stream_id, &d[..off], false)?;

            self.local_goaway_id = Some(id);
        }

        Ok(())
    }

    /// Gets the raw settings from peer including unknown and reserved types.
    ///
    /// The order of settings is the same as received in the SETTINGS frame.
    pub fn peer_settings_raw(&self) -> Option<&[(u64, u64)]> {
        self.peer_settings.raw.as_deref()
    }

    fn open_uni_stream<F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
        ty: u64,
    ) -> Result<u64> {
        let stream_id = self.next_uni_stream_id;

        let mut d = [0; 8];
        let mut b = octets::OctetsMut::with_slice(&mut d);

        match ty {
            // Control and QPACK streams are the most important to schedule.
            stream::HTTP3_CONTROL_STREAM_TYPE_ID
            | stream::QPACK_ENCODER_STREAM_TYPE_ID
            | stream::QPACK_DECODER_STREAM_TYPE_ID => {
                conn.stream_priority(stream_id, 0, false)?;
            }

            // TODO: Server push
            stream::HTTP3_PUSH_STREAM_TYPE_ID => (),

            // Anything else is a GREASE stream, so make it the least important.
            _ => {
                conn.stream_priority(stream_id, 255, false)?;
            }
        }

        conn.stream_send(stream_id, b.put_varint(ty)?, false)?;

        // To avoid skipping stream IDs, we only calculate the next available
        // stream ID when data has been successfully buffered.
        self.next_uni_stream_id = self
            .next_uni_stream_id
            .checked_add(4)
            .ok_or(Error::IdError)?;

        Ok(stream_id)
    }

    fn open_qpack_encoder_stream<F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
    ) -> Result<()> {
        let stream_id = self.open_uni_stream(conn, stream::QPACK_ENCODER_STREAM_TYPE_ID)?;

        self.local_qpack_streams.encoder_stream_id = Some(stream_id);

        qlog_with_type!(QLOG_STREAM_TYPE_SET, conn.qlog, q, {
            let ev_data = EventData::H3StreamTypeSet(H3StreamTypeSet {
                stream_id,
                owner: Some(H3Owner::Local),
                stream_type: H3StreamType::QpackEncode,
                ..Default::default()
            });

            q.add_event_data_now(ev_data).ok();
        });

        Ok(())
    }

    fn open_qpack_decoder_stream<F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
    ) -> Result<()> {
        let stream_id = self.open_uni_stream(conn, stream::QPACK_DECODER_STREAM_TYPE_ID)?;

        self.local_qpack_streams.decoder_stream_id = Some(stream_id);

        qlog_with_type!(QLOG_STREAM_TYPE_SET, conn.qlog, q, {
            let ev_data = EventData::H3StreamTypeSet(H3StreamTypeSet {
                stream_id,
                owner: Some(H3Owner::Local),
                stream_type: H3StreamType::QpackDecode,
                ..Default::default()
            });

            q.add_event_data_now(ev_data).ok();
        });

        Ok(())
    }

    /// Send GREASE frames on the provided stream ID.
    fn send_grease_frames<F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
        stream_id: u64,
    ) -> Result<()> {
        let mut d = [0; 8];

        let stream_cap = match conn.stream_capacity(stream_id) {
            Ok(v) => v,

            Err(e) => {
                if conn.stream_finished(stream_id) {
                    self.streams.remove(&stream_id);
                }

                return Err(e.into());
            }
        };

        let grease_frame1 = grease_value();
        let grease_frame2 = grease_value();
        let grease_payload = b"GREASE is the word";

        let overhead = octets::varint_len(grease_frame1) + // frame type
            1 + // payload len
            octets::varint_len(grease_frame2) + // frame type
            1 + // payload len
            grease_payload.len(); // payload

        // Don't send GREASE if there is not enough capacity for it. Greasing
        // will _not_ be attempted again later on.
        if stream_cap < overhead {
            return Ok(());
        }

        // Empty GREASE frame.
        let mut b = octets::OctetsMut::with_slice(&mut d);
        conn.stream_send(stream_id, b.put_varint(grease_frame1)?, false)?;

        let mut b = octets::OctetsMut::with_slice(&mut d);
        conn.stream_send(stream_id, b.put_varint(0)?, false)?;

        trace!(
            "{} tx frm GREASE stream={} len=0",
            conn.trace_id(),
            stream_id
        );

        qlog_with_type!(QLOG_FRAME_CREATED, conn.qlog, q, {
            let frame = Http3Frame::Reserved { length: Some(0) };
            let ev_data = EventData::H3FrameCreated(H3FrameCreated {
                stream_id,
                length: Some(0),
                frame,
                ..Default::default()
            });

            q.add_event_data_now(ev_data).ok();
        });

        // GREASE frame with payload.
        let mut b = octets::OctetsMut::with_slice(&mut d);
        conn.stream_send(stream_id, b.put_varint(grease_frame2)?, false)?;

        let mut b = octets::OctetsMut::with_slice(&mut d);
        conn.stream_send(stream_id, b.put_varint(18)?, false)?;

        conn.stream_send(stream_id, grease_payload, false)?;

        trace!(
            "{} tx frm GREASE stream={} len={}",
            conn.trace_id(),
            stream_id,
            grease_payload.len()
        );

        qlog_with_type!(QLOG_FRAME_CREATED, conn.qlog, q, {
            let frame = Http3Frame::Reserved {
                length: Some(grease_payload.len() as u64),
            };
            let ev_data = EventData::H3FrameCreated(H3FrameCreated {
                stream_id,
                length: Some(grease_payload.len() as u64),
                frame,
                ..Default::default()
            });

            q.add_event_data_now(ev_data).ok();
        });

        Ok(())
    }

    /// Opens a new unidirectional stream with a GREASE type and sends some
    /// unframed payload.
    fn open_grease_stream<F: BufFactory>(&mut self, conn: &mut super::Connection<F>) -> Result<()> {
        let ty = grease_value();
        match self.open_uni_stream(conn, ty) {
            Ok(stream_id) => {
                conn.stream_send(stream_id, b"GREASE is the word", true)?;

                trace!("{} open GREASE stream {}", conn.trace_id(), stream_id);

                qlog_with_type!(QLOG_STREAM_TYPE_SET, conn.qlog, q, {
                    let ev_data = EventData::H3StreamTypeSet(H3StreamTypeSet {
                        stream_id,
                        owner: Some(H3Owner::Local),
                        stream_type: H3StreamType::Unknown,
                        stream_type_value: Some(ty),
                        ..Default::default()
                    });

                    q.add_event_data_now(ev_data).ok();
                });
            }

            Err(Error::IdError) => {
                trace!("{} GREASE stream blocked", conn.trace_id(),);

                return Ok(());
            }

            Err(e) => return Err(e),
        };

        Ok(())
    }

    /// Sends SETTINGS frame based on HTTP/3 configuration.
    fn send_settings<F: BufFactory>(&mut self, conn: &mut super::Connection<F>) -> Result<()> {
        let stream_id = match self.open_uni_stream(conn, stream::HTTP3_CONTROL_STREAM_TYPE_ID) {
            Ok(v) => v,

            Err(e) => {
                trace!("{} Control stream blocked", conn.trace_id(),);

                if e == Error::Done {
                    return Err(Error::InternalError);
                }

                return Err(e);
            }
        };

        self.control_stream_id = Some(stream_id);

        qlog_with_type!(QLOG_STREAM_TYPE_SET, conn.qlog, q, {
            let ev_data = EventData::H3StreamTypeSet(H3StreamTypeSet {
                stream_id,
                owner: Some(H3Owner::Local),
                stream_type: H3StreamType::Control,
                ..Default::default()
            });

            q.add_event_data_now(ev_data).ok();
        });

        let grease = if conn.grease {
            Some((grease_value(), grease_value()))
        } else {
            None
        };

        let frame = frame::Frame::Settings {
            max_field_section_size: self.local_settings.max_field_section_size,
            qpack_max_table_capacity: self.local_settings.qpack_max_table_capacity,
            qpack_blocked_streams: self.local_settings.qpack_blocked_streams,
            connect_protocol_enabled: self.local_settings.connect_protocol_enabled,
            h3_datagram: self.local_settings.h3_datagram,
            grease,
            additional_settings: self.local_settings.additional_settings.clone(),
            raw: Default::default(),
        };

        let mut d = [42; 128];
        let mut b = octets::OctetsMut::with_slice(&mut d);

        frame.to_bytes(&mut b)?;

        let off = b.off();

        if let Some(id) = self.control_stream_id {
            conn.stream_send(id, &d[..off], false)?;

            trace!(
                "{} tx frm SETTINGS stream={} len={}",
                conn.trace_id(),
                id,
                off
            );

            qlog_with_type!(QLOG_FRAME_CREATED, conn.qlog, q, {
                let frame = frame.to_qlog();
                let ev_data = EventData::H3FrameCreated(H3FrameCreated {
                    stream_id: id,
                    length: Some(off as u64),
                    frame,
                    ..Default::default()
                });

                q.add_event_data_now(ev_data).ok();
            });
        }

        Ok(())
    }

    fn process_control_stream<F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
        stream_id: u64,
    ) -> Result<(u64, Event)> {
        close_conn_if_critical_stream_finished(conn, stream_id)?;

        if !conn.stream_readable(stream_id) {
            return Err(Error::Done);
        }

        match self.process_readable_stream(conn, stream_id, true) {
            Ok(ev) => return Ok(ev),

            Err(Error::Done) => (),

            Err(e) => return Err(e),
        };

        close_conn_if_critical_stream_finished(conn, stream_id)?;

        Err(Error::Done)
    }

    fn process_readable_stream<F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
        stream_id: u64,
        polling: bool,
    ) -> Result<(u64, Event)> {
        self.streams
            .entry(stream_id)
            .or_insert_with(|| <stream::Stream>::new(stream_id, false));

        // We need to get a fresh reference to the stream for each
        // iteration, to avoid borrowing `self` for the entire duration
        // of the loop, because we'll need to borrow it again in the
        // `State::FramePayload` case below.
        while let Some(stream) = self.streams.get_mut(&stream_id) {
            match stream.state() {
                stream::State::StreamType => {
                    stream.try_fill_buffer(conn)?;

                    let varint = match stream.try_consume_varint() {
                        Ok(v) => v,

                        Err(_) => continue,
                    };

                    let ty = stream::Type::deserialize(varint)?;

                    if let Err(e) = stream.set_ty(ty) {
                        conn.close(true, e.to_wire(), b"")?;
                        return Err(e);
                    }

                    qlog_with_type!(QLOG_STREAM_TYPE_SET, conn.qlog, q, {
                        let ty_val = if matches!(ty, stream::Type::Unknown) {
                            Some(varint)
                        } else {
                            None
                        };

                        let ev_data = EventData::H3StreamTypeSet(H3StreamTypeSet {
                            stream_id,
                            owner: Some(H3Owner::Remote),
                            stream_type: ty.to_qlog(),
                            stream_type_value: ty_val,
                            ..Default::default()
                        });

                        q.add_event_data_now(ev_data).ok();
                    });

                    match &ty {
                        stream::Type::Control => {
                            // Only one control stream allowed.
                            if self.peer_control_stream_id.is_some() {
                                conn.close(
                                    true,
                                    Error::StreamCreationError.to_wire(),
                                    b"Received multiple control streams",
                                )?;

                                return Err(Error::StreamCreationError);
                            }

                            trace!(
                                "{} open peer's control stream {}",
                                conn.trace_id(),
                                stream_id
                            );

                            close_conn_if_critical_stream_finished(conn, stream_id)?;

                            self.peer_control_stream_id = Some(stream_id);
                        }

                        stream::Type::Push => {
                            // Only clients can receive push stream.
                            if self.is_server {
                                conn.close(
                                    true,
                                    Error::StreamCreationError.to_wire(),
                                    b"Server received push stream.",
                                )?;

                                return Err(Error::StreamCreationError);
                            }
                        }

                        stream::Type::QpackEncoder => {
                            // Only one qpack encoder stream allowed.
                            if self.peer_qpack_streams.encoder_stream_id.is_some() {
                                conn.close(
                                    true,
                                    Error::StreamCreationError.to_wire(),
                                    b"Received multiple QPACK encoder streams",
                                )?;

                                return Err(Error::StreamCreationError);
                            }

                            close_conn_if_critical_stream_finished(conn, stream_id)?;

                            self.peer_qpack_streams.encoder_stream_id = Some(stream_id);
                        }

                        stream::Type::QpackDecoder => {
                            // Only one qpack decoder allowed.
                            if self.peer_qpack_streams.decoder_stream_id.is_some() {
                                conn.close(
                                    true,
                                    Error::StreamCreationError.to_wire(),
                                    b"Received multiple QPACK decoder streams",
                                )?;

                                return Err(Error::StreamCreationError);
                            }

                            close_conn_if_critical_stream_finished(conn, stream_id)?;

                            self.peer_qpack_streams.decoder_stream_id = Some(stream_id);
                        }

                        stream::Type::Unknown => {
                            // Unknown stream types are ignored.
                            // TODO: we MAY send STOP_SENDING
                        }

                        stream::Type::Request => unreachable!(),
                    }
                }

                stream::State::PushId => {
                    stream.try_fill_buffer(conn)?;

                    let varint = match stream.try_consume_varint() {
                        Ok(v) => v,

                        Err(_) => continue,
                    };

                    if let Err(e) = stream.set_push_id(varint) {
                        conn.close(true, e.to_wire(), b"")?;
                        return Err(e);
                    }
                }

                stream::State::FrameType => {
                    stream.try_fill_buffer(conn)?;

                    let varint = match stream.try_consume_varint() {
                        Ok(v) => v,

                        Err(_) => continue,
                    };

                    match stream.set_frame_type(varint) {
                        Err(Error::FrameUnexpected) => {
                            let msg = format!("Unexpected frame type {varint}");

                            conn.close(true, Error::FrameUnexpected.to_wire(), msg.as_bytes())?;

                            return Err(Error::FrameUnexpected);
                        }

                        Err(e) => {
                            conn.close(true, e.to_wire(), b"Error handling frame.")?;

                            return Err(e);
                        }

                        _ => (),
                    }
                }

                stream::State::FramePayloadLen => {
                    stream.try_fill_buffer(conn)?;

                    let payload_len = match stream.try_consume_varint() {
                        Ok(v) => v,

                        Err(_) => continue,
                    };

                    // DATA frames are handled uniquely. After this point we lose
                    // visibility of DATA framing, so just log here.
                    if Some(frame::DATA_FRAME_TYPE_ID) == stream.frame_type() {
                        trace!(
                            "{} rx frm DATA stream={} wire_payload_len={}",
                            conn.trace_id(),
                            stream_id,
                            payload_len
                        );

                        qlog_with_type!(QLOG_FRAME_PARSED, conn.qlog, q, {
                            let frame = Http3Frame::Data { raw: None };

                            let ev_data = EventData::H3FrameParsed(H3FrameParsed {
                                stream_id,
                                length: Some(payload_len),
                                frame,
                                ..Default::default()
                            });

                            q.add_event_data_now(ev_data).ok();
                        });
                    }

                    if let Err(e) = stream.set_frame_payload_len(payload_len) {
                        conn.close(true, e.to_wire(), b"")?;
                        return Err(e);
                    }
                }

                stream::State::FramePayload => {
                    // Do not emit events when not polling.
                    if !polling {
                        break;
                    }

                    stream.try_fill_buffer(conn)?;

                    let (frame, payload_len) = match stream.try_consume_frame() {
                        Ok(frame) => frame,

                        Err(Error::Done) => return Err(Error::Done),

                        Err(e) => {
                            conn.close(true, e.to_wire(), b"Error handling frame.")?;

                            return Err(e);
                        }
                    };

                    match self.process_frame(conn, stream_id, frame, payload_len) {
                        Ok(ev) => return Ok(ev),

                        Err(Error::Done) => {
                            // This might be a frame that is processed internally
                            // without needing to bubble up to the user as an
                            // event. Check whether the frame has FIN'd by QUIC
                            // to prevent trying to read again on a closed stream.
                            if conn.stream_finished(stream_id) {
                                break;
                            }
                        }

                        Err(e) => return Err(e),
                    };
                }

                stream::State::Data => {
                    // Do not emit events when not polling.
                    if !polling {
                        break;
                    }

                    if !stream.try_trigger_data_event() {
                        break;
                    }

                    return Ok((stream_id, Event::Data));
                }

                stream::State::QpackInstruction => {
                    let mut d = [0; 4096];

                    // Read data from the stream and discard immediately.
                    loop {
                        let (recv, fin) = conn.stream_recv(stream_id, &mut d)?;

                        match stream.ty() {
                            Some(stream::Type::QpackEncoder) => {
                                self.peer_qpack_streams.encoder_stream_bytes += recv as u64
                            }
                            Some(stream::Type::QpackDecoder) => {
                                self.peer_qpack_streams.decoder_stream_bytes += recv as u64
                            }
                            _ => unreachable!(),
                        };

                        if fin {
                            close_conn_critical_stream(conn)?;
                        }
                    }
                }

                stream::State::Drain => {
                    // Discard incoming data on the stream.
                    conn.stream_shutdown(stream_id, crate::Shutdown::Read, 0x100)?;

                    break;
                }

                stream::State::Finished => break,
            }
        }

        Err(Error::Done)
    }

    fn process_finished_stream(&mut self, stream_id: u64) {
        let stream = match self.streams.get_mut(&stream_id) {
            Some(v) => v,

            None => return,
        };

        if stream.state() == stream::State::Finished {
            return;
        }

        match stream.ty() {
            Some(stream::Type::Request) | Some(stream::Type::Push) => {
                stream.finished();

                self.finished_streams.push_back(stream_id);
            }

            _ => (),
        };
    }

    fn process_frame<F: BufFactory>(
        &mut self,
        conn: &mut super::Connection<F>,
        stream_id: u64,
        frame: frame::Frame,
        payload_len: u64,
    ) -> Result<(u64, Event)> {
        trace!(
            "{} rx frm {:?} stream={} payload_len={}",
            conn.trace_id(),
            frame,
            stream_id,
            payload_len
        );

        qlog_with_type!(QLOG_FRAME_PARSED, conn.qlog, q, {
            // HEADERS frames are special case and will be logged below.
            if !matches!(frame, frame::Frame::Headers { .. }) {
                let frame = frame.to_qlog();
                let ev_data = EventData::H3FrameParsed(H3FrameParsed {
                    stream_id,
                    length: Some(payload_len),
                    frame,
                    ..Default::default()
                });

                q.add_event_data_now(ev_data).ok();
            }
        });

        match frame {
            frame::Frame::Settings {
                max_field_section_size,
                qpack_max_table_capacity,
                qpack_blocked_streams,
                connect_protocol_enabled,
                h3_datagram,
                additional_settings,
                raw,
                ..
            } => {
                self.peer_settings = ConnectionSettings {
                    max_field_section_size,
                    qpack_max_table_capacity,
                    qpack_blocked_streams,
                    connect_protocol_enabled,
                    h3_datagram,
                    additional_settings,
                    raw,
                };

                if let Some(1) = h3_datagram {
                    // The peer MUST have also enabled DATAGRAM with a TP
                    if conn.dgram_max_writable_len().is_none() {
                        conn.close(
                            true,
                            Error::SettingsError.to_wire(),
                            b"H3_DATAGRAM sent with value 1 but max_datagram_frame_size TP not set.",
                        )?;

                        return Err(Error::SettingsError);
                    }
                }
            }

            frame::Frame::Headers { header_block } => {
                // Servers reject too many HEADERS frames.
                if let Some(s) = self.streams.get_mut(&stream_id) {
                    if self.is_server && s.headers_received_count() == 2 {
                        conn.close(
                            true,
                            Error::FrameUnexpected.to_wire(),
                            b"Too many HEADERS frames",
                        )?;
                        return Err(Error::FrameUnexpected);
                    }

                    s.increment_headers_received();
                }

                // Use "infinite" as default value for max_field_section_size if
                // it is not configured by the application.
                let max_size = self
                    .local_settings
                    .max_field_section_size
                    .unwrap_or(u64::MAX);

                let headers = match self.qpack_decoder.decode(&header_block[..], max_size) {
                    Ok(v) => v,

                    Err(e) => {
                        let e = match e {
                            qpack::Error::HeaderListTooLarge => Error::ExcessiveLoad,

                            _ => Error::QpackDecompressionFailed,
                        };

                        conn.close(true, e.to_wire(), b"Error parsing headers.")?;

                        return Err(e);
                    }
                };

                qlog_with_type!(QLOG_FRAME_PARSED, conn.qlog, q, {
                    let qlog_headers = headers
                        .iter()
                        .map(|h| qlog::events::h3::HttpHeader {
                            name: String::from_utf8_lossy(h.name()).into_owned(),
                            value: String::from_utf8_lossy(h.value()).into_owned(),
                        })
                        .collect();

                    let frame = Http3Frame::Headers {
                        headers: qlog_headers,
                    };

                    let ev_data = EventData::H3FrameParsed(H3FrameParsed {
                        stream_id,
                        length: Some(payload_len),
                        frame,
                        ..Default::default()
                    });

                    q.add_event_data_now(ev_data).ok();
                });

                let more_frames = !conn.stream_finished(stream_id);

                return Ok((
                    stream_id,
                    Event::Headers {
                        list: headers,
                        more_frames,
                    },
                ));
            }

            frame::Frame::Data { .. } => {
                // Do nothing. The Data event is returned separately.
            }

            frame::Frame::GoAway { id } => {
                if !self.is_server && id % 4 != 0 {
                    conn.close(
                        true,
                        Error::FrameUnexpected.to_wire(),
                        b"GOAWAY received with ID of non-request stream",
                    )?;

                    return Err(Error::IdError);
                }

                if let Some(received_id) = self.peer_goaway_id {
                    if id > received_id {
                        conn.close(
                            true,
                            Error::IdError.to_wire(),
                            b"GOAWAY received with ID larger than previously received",
                        )?;

                        return Err(Error::IdError);
                    }
                }

                self.peer_goaway_id = Some(id);

                return Ok((id, Event::GoAway));
            }

            frame::Frame::MaxPushId { push_id } => {
                if !self.is_server {
                    conn.close(
                        true,
                        Error::FrameUnexpected.to_wire(),
                        b"MAX_PUSH_ID received by client",
                    )?;

                    return Err(Error::FrameUnexpected);
                }

                if push_id < self.max_push_id {
                    conn.close(true, Error::IdError.to_wire(), b"MAX_PUSH_ID reduced limit")?;

                    return Err(Error::IdError);
                }

                self.max_push_id = push_id;
            }

            frame::Frame::PushPromise { .. } => {
                if self.is_server {
                    conn.close(
                        true,
                        Error::FrameUnexpected.to_wire(),
                        b"PUSH_PROMISE received by server",
                    )?;

                    return Err(Error::FrameUnexpected);
                }

                if stream_id % 4 != 0 {
                    conn.close(
                        true,
                        Error::FrameUnexpected.to_wire(),
                        b"PUSH_PROMISE received on non-request stream",
                    )?;

                    return Err(Error::FrameUnexpected);
                }

                // TODO: implement more checks and PUSH_PROMISE event
            }

            frame::Frame::CancelPush { .. } => {
                // TODO: implement CANCEL_PUSH frame
            }

            frame::Frame::PriorityUpdateRequest {
                prioritized_element_id,
                priority_field_value,
            } => {
                if !self.is_server {
                    conn.close(
                        true,
                        Error::FrameUnexpected.to_wire(),
                        b"PRIORITY_UPDATE received by client",
                    )?;

                    return Err(Error::FrameUnexpected);
                }

                if prioritized_element_id % 4 != 0 {
                    conn.close(
                        true,
                        Error::FrameUnexpected.to_wire(),
                        b"PRIORITY_UPDATE for request stream type with wrong ID",
                    )?;

                    return Err(Error::FrameUnexpected);
                }

                if prioritized_element_id > conn.streams.max_streams_bidi() * 4 {
                    conn.close(
                        true,
                        Error::IdError.to_wire(),
                        b"PRIORITY_UPDATE for request stream beyond max streams limit",
                    )?;

                    return Err(Error::IdError);
                }

                // If the PRIORITY_UPDATE is valid, consider storing the latest
                // contents. Due to reordering, it is possible that we might
                // receive frames that reference streams that have not yet to
                // been opened and that's OK because it's within our concurrency
                // limit. However, we discard PRIORITY_UPDATE that refers to
                // streams that we know have been collected.
                if conn.streams.is_collected(prioritized_element_id) {
                    return Err(Error::Done);
                }

                // If the stream did not yet exist, create it and store.
                let stream = self
                    .streams
                    .entry(prioritized_element_id)
                    .or_insert_with(|| <stream::Stream>::new(prioritized_element_id, false));

                let had_priority_update = stream.has_last_priority_update();
                stream.set_last_priority_update(Some(priority_field_value));

                // Only trigger the event when there wasn't already a stored
                // PRIORITY_UPDATE.
                if !had_priority_update {
                    return Ok((prioritized_element_id, Event::PriorityUpdate));
                } else {
                    return Err(Error::Done);
                }
            }

            frame::Frame::PriorityUpdatePush {
                prioritized_element_id,
                ..
            } => {
                if !self.is_server {
                    conn.close(
                        true,
                        Error::FrameUnexpected.to_wire(),
                        b"PRIORITY_UPDATE received by client",
                    )?;

                    return Err(Error::FrameUnexpected);
                }

                if prioritized_element_id % 3 != 0 {
                    conn.close(
                        true,
                        Error::FrameUnexpected.to_wire(),
                        b"PRIORITY_UPDATE for push stream type with wrong ID",
                    )?;

                    return Err(Error::FrameUnexpected);
                }

                // TODO: we only implement this if we implement server push
            }

            frame::Frame::Unknown { .. } => (),
        }

        Err(Error::Done)
    }

    /// Collects and returns statistics about the connection.
    #[inline]
    pub fn stats(&self) -> Stats {
        Stats {
            qpack_encoder_stream_recv_bytes: self.peer_qpack_streams.encoder_stream_bytes,
            qpack_decoder_stream_recv_bytes: self.peer_qpack_streams.decoder_stream_bytes,
        }
    }
}

/// Generates an HTTP/3 GREASE variable length integer.
pub fn grease_value() -> u64 {
    let n = super::rand::rand_u64_uniform(148_764_065_110_560_899);
    31 * n + 33
}

#[doc(hidden)]
pub mod testing {
    use super::*;

    use crate::test_utils;

    /// Session is an HTTP/3 test helper structure. It holds a client, server
    /// and pipe that allows them to communicate.
    ///
    /// `default()` creates a session with some sensible default
    /// configuration. `with_configs()` allows for providing a specific
    /// configuration.
    ///
    /// `handshake()` performs all the steps needed to establish an HTTP/3
    /// connection.
    ///
    /// Some utility functions are provided that make it less verbose to send
    /// request, responses and individual headers. The full quiche API remains
    /// available for any test that need to do unconventional things (such as
    /// bad behaviour that triggers errors).
    pub struct Session {
        pub pipe: test_utils::Pipe,
        pub client: Connection,
        pub server: Connection,
    }

    impl Session {
        pub fn new() -> Result<Session> {
            fn path_relative_to_manifest_dir(path: &str) -> String {
                std::fs::canonicalize(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            }

            let mut config = crate::Config::new(crate::PROTOCOL_VERSION)?;
            config.load_cert_chain_from_pem_file(&path_relative_to_manifest_dir(
                "examples/cert.crt",
            ))?;
            config
                .load_priv_key_from_pem_file(&path_relative_to_manifest_dir("examples/cert.key"))?;
            config.set_application_protos(&[b"h3"])?;
            config.set_initial_max_data(1500);
            config.set_initial_max_stream_data_bidi_local(150);
            config.set_initial_max_stream_data_bidi_remote(150);
            config.set_initial_max_stream_data_uni(150);
            config.set_initial_max_streams_bidi(5);
            config.set_initial_max_streams_uni(5);
            config.verify_peer(false);
            config.enable_dgram(true, 3, 3);
            config.set_ack_delay_exponent(8);

            let h3_config = Config::new()?;
            Session::with_configs(&mut config, &h3_config)
        }

        pub fn with_configs(config: &mut crate::Config, h3_config: &Config) -> Result<Session> {
            let pipe = test_utils::Pipe::with_config(config)?;
            let client_dgram = pipe.client.dgram_enabled();
            let server_dgram = pipe.server.dgram_enabled();
            Ok(Session {
                pipe,
                client: Connection::new(h3_config, false, client_dgram)?,
                server: Connection::new(h3_config, true, server_dgram)?,
            })
        }

        /// Do the HTTP/3 handshake so both ends are in sane initial state.
        pub fn handshake(&mut self) -> Result<()> {
            self.pipe.handshake()?;

            // Client streams.
            self.client.send_settings(&mut self.pipe.client)?;
            self.pipe.advance().ok();

            self.client
                .open_qpack_encoder_stream(&mut self.pipe.client)?;
            self.pipe.advance().ok();

            self.client
                .open_qpack_decoder_stream(&mut self.pipe.client)?;
            self.pipe.advance().ok();

            if self.pipe.client.grease {
                self.client.open_grease_stream(&mut self.pipe.client)?;
            }

            self.pipe.advance().ok();

            // Server streams.
            self.server.send_settings(&mut self.pipe.server)?;
            self.pipe.advance().ok();

            self.server
                .open_qpack_encoder_stream(&mut self.pipe.server)?;
            self.pipe.advance().ok();

            self.server
                .open_qpack_decoder_stream(&mut self.pipe.server)?;
            self.pipe.advance().ok();

            if self.pipe.server.grease {
                self.server.open_grease_stream(&mut self.pipe.server)?;
            }

            self.advance().ok();

            while self.client.poll(&mut self.pipe.client).is_ok() {
                // Do nothing.
            }

            while self.server.poll(&mut self.pipe.server).is_ok() {
                // Do nothing.
            }

            Ok(())
        }

        /// Advances the session pipe over the buffer.
        pub fn advance(&mut self) -> crate::Result<()> {
            self.pipe.advance()
        }

        /// Polls the client for events.
        pub fn poll_client(&mut self) -> Result<(u64, Event)> {
            self.client.poll(&mut self.pipe.client)
        }

        /// Polls the server for events.
        pub fn poll_server(&mut self) -> Result<(u64, Event)> {
            self.server.poll(&mut self.pipe.server)
        }

        /// Sends a request from client with default headers.
        ///
        /// On success it returns the newly allocated stream and the headers.
        pub fn send_request(&mut self, fin: bool) -> Result<(u64, Vec<Header>)> {
            let req = vec![
                Header::new(b":method", b"GET"),
                Header::new(b":scheme", b"https"),
                Header::new(b":authority", b"quic.tech"),
                Header::new(b":path", b"/test"),
                Header::new(b"user-agent", b"quiche-test"),
            ];

            let stream = self.client.send_request(&mut self.pipe.client, &req, fin)?;

            self.advance().ok();

            Ok((stream, req))
        }

        /// Sends a response from server with default headers.
        ///
        /// On success it returns the headers.
        pub fn send_response(&mut self, stream: u64, fin: bool) -> Result<Vec<Header>> {
            let resp = vec![
                Header::new(b":status", b"200"),
                Header::new(b"server", b"quiche-test"),
            ];

            self.server
                .send_response(&mut self.pipe.server, stream, &resp, fin)?;

            self.advance().ok();

            Ok(resp)
        }

        /// Sends some default payload from client.
        ///
        /// On success it returns the payload.
        pub fn send_body_client(&mut self, stream: u64, fin: bool) -> Result<Vec<u8>> {
            let bytes = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

            self.client
                .send_body(&mut self.pipe.client, stream, &bytes, fin)?;

            self.advance().ok();

            Ok(bytes)
        }

        /// Fetches DATA payload from the server.
        ///
        /// On success it returns the number of bytes received.
        pub fn recv_body_client(&mut self, stream: u64, buf: &mut [u8]) -> Result<usize> {
            self.client.recv_body(&mut self.pipe.client, stream, buf)
        }

        /// Sends some default payload from server.
        ///
        /// On success it returns the payload.
        pub fn send_body_server(&mut self, stream: u64, fin: bool) -> Result<Vec<u8>> {
            let bytes = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

            self.server
                .send_body(&mut self.pipe.server, stream, &bytes, fin)?;

            self.advance().ok();

            Ok(bytes)
        }

        /// Fetches DATA payload from the client.
        ///
        /// On success it returns the number of bytes received.
        pub fn recv_body_server(&mut self, stream: u64, buf: &mut [u8]) -> Result<usize> {
            self.server.recv_body(&mut self.pipe.server, stream, buf)
        }

        /// Sends a single HTTP/3 frame from the client.
        pub fn send_frame_client(
            &mut self,
            frame: frame::Frame,
            stream_id: u64,
            fin: bool,
        ) -> Result<()> {
            let mut d = [42; 65535];

            let mut b = octets::OctetsMut::with_slice(&mut d);

            frame.to_bytes(&mut b)?;

            let off = b.off();
            self.pipe.client.stream_send(stream_id, &d[..off], fin)?;

            self.advance().ok();

            Ok(())
        }

        /// Send an HTTP/3 DATAGRAM with default data from the client.
        ///
        /// On success it returns the data.
        pub fn send_dgram_client(&mut self, flow_id: u64) -> Result<Vec<u8>> {
            let bytes = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
            let len = octets::varint_len(flow_id) + bytes.len();
            let mut d = vec![0; len];
            let mut b = octets::OctetsMut::with_slice(&mut d);

            b.put_varint(flow_id)?;
            b.put_bytes(&bytes)?;

            self.pipe.client.dgram_send(&d)?;

            self.advance().ok();

            Ok(bytes)
        }

        /// Receives an HTTP/3 DATAGRAM from the server.
        ///
        /// On success it returns the DATAGRAM length, flow ID and flow ID
        /// length.
        pub fn recv_dgram_client(&mut self, buf: &mut [u8]) -> Result<(usize, u64, usize)> {
            let len = self.pipe.client.dgram_recv(buf)?;
            let mut b = octets::Octets::with_slice(buf);
            let flow_id = b.get_varint()?;

            Ok((len, flow_id, b.off()))
        }

        /// Send an HTTP/3 DATAGRAM with default data from the server
        ///
        /// On success it returns the data.
        pub fn send_dgram_server(&mut self, flow_id: u64) -> Result<Vec<u8>> {
            let bytes = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
            let len = octets::varint_len(flow_id) + bytes.len();
            let mut d = vec![0; len];
            let mut b = octets::OctetsMut::with_slice(&mut d);

            b.put_varint(flow_id)?;
            b.put_bytes(&bytes)?;

            self.pipe.server.dgram_send(&d)?;

            self.advance().ok();

            Ok(bytes)
        }

        /// Receives an HTTP/3 DATAGRAM from the client.
        ///
        /// On success it returns the DATAGRAM length, flow ID and flow ID
        /// length.
        pub fn recv_dgram_server(&mut self, buf: &mut [u8]) -> Result<(usize, u64, usize)> {
            let len = self.pipe.server.dgram_recv(buf)?;
            let mut b = octets::Octets::with_slice(buf);
            let flow_id = b.get_varint()?;

            Ok((len, flow_id, b.off()))
        }

        /// Sends a single HTTP/3 frame from the server.
        pub fn send_frame_server(
            &mut self,
            frame: frame::Frame,
            stream_id: u64,
            fin: bool,
        ) -> Result<()> {
            let mut d = [42; 65535];

            let mut b = octets::OctetsMut::with_slice(&mut d);

            frame.to_bytes(&mut b)?;

            let off = b.off();
            self.pipe.server.stream_send(stream_id, &d[..off], fin)?;

            self.advance().ok();

            Ok(())
        }

        /// Sends an arbitrary buffer of HTTP/3 stream data from the client.
        pub fn send_arbitrary_stream_data_client(
            &mut self,
            data: &[u8],
            stream_id: u64,
            fin: bool,
        ) -> Result<()> {
            self.pipe.client.stream_send(stream_id, data, fin)?;

            self.advance().ok();

            Ok(())
        }

        /// Sends an arbitrary buffer of HTTP/3 stream data from the server.
        pub fn send_arbitrary_stream_data_server(
            &mut self,
            data: &[u8],
            stream_id: u64,
            fin: bool,
        ) -> Result<()> {
            self.pipe.server.stream_send(stream_id, data, fin)?;

            self.advance().ok();

            Ok(())
        }
    }
}

#[cfg(feature = "ffi")]
mod ffi;
#[cfg(feature = "internal")]
#[doc(hidden)]
pub mod frame;
#[cfg(not(feature = "internal"))]
mod frame;
#[doc(hidden)]
pub mod qpack;
mod stream;
