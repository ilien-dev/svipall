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

use std::fmt::Display;

use std::ops::Index;
use std::ops::IndexMut;
use std::ops::RangeInclusive;

use std::time::Instant;

use crate::Error;
use crate::Result;
use crate::DEFAULT_INITIAL_CONGESTION_WINDOW_PACKETS;

use crate::crypto;
use crate::rand;
use crate::ranges;
use crate::recovery;
use crate::stream;

const FORM_BIT: u8 = 0x80;
const FIXED_BIT: u8 = 0x40;
const KEY_PHASE_BIT: u8 = 0x04;

const TYPE_MASK: u8 = 0x30;
const PKT_NUM_MASK: u8 = 0x03;

pub const MAX_CID_LEN: u8 = 20;

pub const MAX_PKT_NUM_LEN: usize = 4;

const SAMPLE_LEN: usize = 16;

// Set the min skip skip interval to 2x the default number of initial packet
// count.
const MIN_SKIP_COUNTER_VALUE: u64 = DEFAULT_INITIAL_CONGESTION_WINDOW_PACKETS as u64 * 2;

const RETRY_AEAD_ALG: crypto::Algorithm = crypto::Algorithm::AES128_GCM;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Epoch {
    Initial = 0,
    Handshake = 1,
    Application = 2,
}

static EPOCHS: [Epoch; 3] = [Epoch::Initial, Epoch::Handshake, Epoch::Application];

impl Epoch {
    /// Returns an ordered slice containing the `Epoch`s that fit in the
    /// provided `range`.
    pub fn epochs(range: RangeInclusive<Epoch>) -> &'static [Epoch] {
        &EPOCHS[*range.start() as usize..=*range.end() as usize]
    }

    pub const fn count() -> usize {
        3
    }
}

impl Display for Epoch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", usize::from(*self))
    }
}

impl From<Epoch> for usize {
    fn from(e: Epoch) -> Self {
        e as usize
    }
}

impl<T> Index<Epoch> for [T]
where
    T: Sized,
{
    type Output = T;

    fn index(&self, index: Epoch) -> &Self::Output {
        self.index(usize::from(index))
    }
}

impl<T> IndexMut<Epoch> for [T]
where
    T: Sized,
{
    fn index_mut(&mut self, index: Epoch) -> &mut Self::Output {
        self.index_mut(usize::from(index))
    }
}

/// QUIC packet type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    /// Initial packet.
    Initial,

    /// Retry packet.
    Retry,

    /// Handshake packet.
    Handshake,

    /// 0-RTT packet.
    ZeroRTT,

    /// Version negotiation packet.
    VersionNegotiation,

    /// 1-RTT short header packet.
    Short,
}

impl Type {
    pub(crate) fn from_epoch(e: Epoch) -> Type {
        match e {
            Epoch::Initial => Type::Initial,

            Epoch::Handshake => Type::Handshake,

            Epoch::Application => Type::Short,
        }
    }

    pub(crate) fn to_epoch(self) -> Result<Epoch> {
        match self {
            Type::Initial => Ok(Epoch::Initial),

            Type::ZeroRTT => Ok(Epoch::Application),

            Type::Handshake => Ok(Epoch::Handshake),

            Type::Short => Ok(Epoch::Application),

            _ => Err(Error::InvalidPacket),
        }
    }

    #[cfg(feature = "qlog")]
    pub(crate) fn to_qlog(self) -> qlog::events::quic::PacketType {
        match self {
            Type::Initial => qlog::events::quic::PacketType::Initial,

            Type::Retry => qlog::events::quic::PacketType::Retry,

            Type::Handshake => qlog::events::quic::PacketType::Handshake,

            Type::ZeroRTT => qlog::events::quic::PacketType::ZeroRtt,

            Type::VersionNegotiation => qlog::events::quic::PacketType::VersionNegotiation,

            Type::Short => qlog::events::quic::PacketType::OneRtt,
        }
    }
}

/// A QUIC connection ID.
pub struct ConnectionId<'a>(ConnectionIdInner<'a>);

enum ConnectionIdInner<'a> {
    Vec(Vec<u8>),
    Ref(&'a [u8]),
}

impl<'a> ConnectionId<'a> {
    /// Creates a new connection ID from the given vector.
    #[inline]
    pub const fn from_vec(cid: Vec<u8>) -> Self {
        Self(ConnectionIdInner::Vec(cid))
    }

    /// Creates a new connection ID from the given slice.
    #[inline]
    pub const fn from_ref(cid: &'a [u8]) -> Self {
        Self(ConnectionIdInner::Ref(cid))
    }

    /// Returns a new owning connection ID from the given existing one.
    #[inline]
    pub fn into_owned(self) -> ConnectionId<'static> {
        ConnectionId::from_vec(self.into())
    }
}

impl Default for ConnectionId<'_> {
    #[inline]
    fn default() -> Self {
        Self::from_vec(Vec::new())
    }
}

impl From<Vec<u8>> for ConnectionId<'_> {
    #[inline]
    fn from(v: Vec<u8>) -> Self {
        Self::from_vec(v)
    }
}

impl From<ConnectionId<'_>> for Vec<u8> {
    #[inline]
    fn from(id: ConnectionId<'_>) -> Self {
        match id.0 {
            ConnectionIdInner::Vec(cid) => cid,
            ConnectionIdInner::Ref(cid) => cid.to_vec(),
        }
    }
}

impl PartialEq for ConnectionId<'_> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl Eq for ConnectionId<'_> {}

impl AsRef<[u8]> for ConnectionId<'_> {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        match &self.0 {
            ConnectionIdInner::Vec(v) => v.as_ref(),
            ConnectionIdInner::Ref(v) => v,
        }
    }
}

impl std::hash::Hash for ConnectionId<'_> {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_ref().hash(state);
    }
}

impl std::ops::Deref for ConnectionId<'_> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        match &self.0 {
            ConnectionIdInner::Vec(v) => v.as_ref(),
            ConnectionIdInner::Ref(v) => v,
        }
    }
}

impl Clone for ConnectionId<'_> {
    #[inline]
    fn clone(&self) -> Self {
        Self::from_vec(self.as_ref().to_vec())
    }
}

impl std::fmt::Debug for ConnectionId<'_> {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for c in self.as_ref() {
            write!(f, "{c:02x}")?;
        }

        Ok(())
    }
}

/// A QUIC packet's header.
#[derive(Clone, PartialEq, Eq)]
pub struct Header<'a> {
    /// The type of the packet.
    pub ty: Type,

    /// The version of the packet.
    pub version: u32,

    /// The destination connection ID of the packet.
    pub dcid: ConnectionId<'a>,

    /// The source connection ID of the packet.
    pub scid: ConnectionId<'a>,

    /// The packet number. It's only meaningful after the header protection is
    /// removed.
    pub(crate) pkt_num: u64,

    /// The length of the packet number. It's only meaningful after the header
    /// protection is removed.
    pub(crate) pkt_num_len: usize,

    /// The address verification token of the packet. Only present in `Initial`
    /// and `Retry` packets.
    pub token: Option<Vec<u8>>,

    /// The list of versions in the packet. Only present in
    /// `VersionNegotiation` packets.
    pub versions: Option<Vec<u32>>,

    /// The key phase bit of the packet. It's only meaningful after the header
    /// protection is removed.
    pub(crate) key_phase: bool,
}

impl<'a> Header<'a> {
    /// Parses a QUIC packet header from the given buffer.
    ///
    /// The `dcid_len` parameter is the length of the destination connection ID,
    /// required to parse short header packets.
    ///
    /// ## Examples:
    ///
    /// ```no_run
    /// # const LOCAL_CONN_ID_LEN: usize = 16;
    /// # let mut buf = [0; 512];
    /// # let mut out = [0; 512];
    /// # let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    /// let (len, src) = socket.recv_from(&mut buf).unwrap();
    ///
    /// let hdr = quiche::Header::from_slice(&mut buf[..len], LOCAL_CONN_ID_LEN)?;
    /// # Ok::<(), quiche::Error>(())
    /// ```
    #[inline]
    pub fn from_slice<'b>(buf: &'b mut [u8], dcid_len: usize) -> Result<Header<'a>> {
        let mut b = octets::OctetsMut::with_slice(buf);
        Header::from_bytes(&mut b, dcid_len)
    }

    pub(crate) fn from_bytes<'b>(
        b: &'b mut octets::OctetsMut,
        dcid_len: usize,
    ) -> Result<Header<'a>> {
        let first = b.get_u8()?;

        if !Header::is_long(first) {
            // Decode short header.
            let dcid = b.get_bytes(dcid_len)?;

            return Ok(Header {
                ty: Type::Short,
                version: 0,
                dcid: dcid.to_vec().into(),
                scid: ConnectionId::default(),
                pkt_num: 0,
                pkt_num_len: 0,
                token: None,
                versions: None,
                key_phase: false,
            });
        }

        // Decode long header.
        let version = b.get_u32()?;

        let ty = if version == 0 {
            Type::VersionNegotiation
        } else {
            match (first & TYPE_MASK) >> 4 {
                0x00 => Type::Initial,
                0x01 => Type::ZeroRTT,
                0x02 => Type::Handshake,
                0x03 => Type::Retry,
                _ => return Err(Error::InvalidPacket),
            }
        };

        let dcid_len = b.get_u8()?;
        if crate::version_is_supported(version) && dcid_len > MAX_CID_LEN {
            return Err(Error::InvalidPacket);
        }
        let dcid = b.get_bytes(dcid_len as usize)?.to_vec();

        let scid_len = b.get_u8()?;
        if crate::version_is_supported(version) && scid_len > MAX_CID_LEN {
            return Err(Error::InvalidPacket);
        }
        let scid = b.get_bytes(scid_len as usize)?.to_vec();

        // End of invariants.

        let mut token: Option<Vec<u8>> = None;
        let mut versions: Option<Vec<u32>> = None;

        match ty {
            Type::Initial => {
                token = Some(b.get_bytes_with_varint_length()?.to_vec());
            }

            Type::Retry => {
                const TAG_LEN: usize = RETRY_AEAD_ALG.tag_len();

                // Exclude the integrity tag from the token.
                if b.cap() < TAG_LEN {
                    return Err(Error::InvalidPacket);
                }

                let token_len = b.cap() - TAG_LEN;
                token = Some(b.get_bytes(token_len)?.to_vec());
            }

            Type::VersionNegotiation => {
                let mut list: Vec<u32> = Vec::new();

                while b.cap() > 0 {
                    let version = b.get_u32()?;
                    list.push(version);
                }

                versions = Some(list);
            }

            _ => (),
        };

        Ok(Header {
            ty,
            version,
            dcid: dcid.into(),
            scid: scid.into(),
            pkt_num: 0,
            pkt_num_len: 0,
            token,
            versions,
            key_phase: false,
        })
    }

    pub(crate) fn to_bytes(&self, out: &mut octets::OctetsMut) -> Result<()> {
        let mut first = 0;

        // Encode pkt num length.
        first |= self.pkt_num_len.saturating_sub(1) as u8;

        // Encode short header.
        if self.ty == Type::Short {
            // Unset form bit for short header.
            first &= !FORM_BIT;

            // Set fixed bit.
            first |= FIXED_BIT;

            // Set key phase bit.
            if self.key_phase {
                first |= KEY_PHASE_BIT;
            } else {
                first &= !KEY_PHASE_BIT;
            }

            out.put_u8(first)?;
            out.put_bytes(&self.dcid)?;

            return Ok(());
        }

        // Encode long header.
        let ty: u8 = match self.ty {
            Type::Initial => 0x00,
            Type::ZeroRTT => 0x01,
            Type::Handshake => 0x02,
            Type::Retry => 0x03,
            _ => return Err(Error::InvalidPacket),
        };

        first |= FORM_BIT | FIXED_BIT | (ty << 4);

        out.put_u8(first)?;

        out.put_u32(self.version)?;

        out.put_u8(self.dcid.len() as u8)?;
        out.put_bytes(&self.dcid)?;

        out.put_u8(self.scid.len() as u8)?;
        out.put_bytes(&self.scid)?;

        // Only Initial and Retry packets have a token.
        match self.ty {
            Type::Initial => {
                match self.token {
                    Some(ref v) => {
                        out.put_varint(v.len() as u64)?;
                        out.put_bytes(v)?;
                    }

                    // No token, so length = 0.
                    None => {
                        out.put_varint(0)?;
                    }
                }
            }

            Type::Retry => {
                // Retry packets don't have a token length.
                out.put_bytes(self.token.as_ref().unwrap())?;
            }

            _ => (),
        }

        Ok(())
    }

    /// Returns true if the packet has a long header.
    ///
    /// The `b` parameter represents the first byte of the QUIC header.
    fn is_long(b: u8) -> bool {
        b & FORM_BIT != 0
    }
}

impl std::fmt::Debug for Header<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.ty)?;

        if self.ty != Type::Short {
            write!(f, " version={:x}", self.version)?;
        }

        write!(f, " dcid={:?}", self.dcid)?;

        if self.ty != Type::Short {
            write!(f, " scid={:?}", self.scid)?;
        }

        if let Some(ref token) = self.token {
            write!(f, " token=")?;
            for b in token {
                write!(f, "{b:02x}")?;
            }
        }

        if let Some(ref versions) = self.versions {
            write!(f, " versions={versions:x?}")?;
        }

        if self.ty == Type::Short {
            write!(f, " key_phase={}", self.key_phase)?;
        }

        Ok(())
    }
}

pub fn pkt_num_len(pn: u64, largest_acked: u64) -> usize {
    let num_unacked: u64 = pn.saturating_sub(largest_acked) + 1;
    // computes ceil of num_unacked.log2() + 1
    let min_bits = u64::BITS - num_unacked.leading_zeros() + 1;
    // get the num len in bytes
    min_bits.div_ceil(8) as usize
}

pub fn decrypt_hdr(b: &mut octets::OctetsMut, hdr: &mut Header, aead: &crypto::Open) -> Result<()> {
    let mut first = {
        let (first_buf, _) = b.split_at(1)?;
        first_buf.as_ref()[0]
    };

    let mut pn_and_sample = b.peek_bytes_mut(MAX_PKT_NUM_LEN + SAMPLE_LEN)?;

    let (mut ciphertext, sample) = pn_and_sample.split_at(MAX_PKT_NUM_LEN)?;

    let ciphertext = ciphertext.as_mut();

    let mask = aead.new_mask(sample.as_ref())?;

    if Header::is_long(first) {
        first ^= mask[0] & 0x0f;
    } else {
        first ^= mask[0] & 0x1f;
    }

    let pn_len = usize::from((first & PKT_NUM_MASK) + 1);

    let ciphertext = &mut ciphertext[..pn_len];

    for i in 0..pn_len {
        ciphertext[i] ^= mask[i + 1];
    }

    // Extract packet number corresponding to the decoded length.
    let pn = match pn_len {
        1 => u64::from(b.get_u8()?),

        2 => u64::from(b.get_u16()?),

        3 => u64::from(b.get_u24()?),

        4 => u64::from(b.get_u32()?),

        _ => return Err(Error::InvalidPacket),
    };

    // Write decrypted first byte back into the input buffer.
    let (mut first_buf, _) = b.split_at(1)?;
    first_buf.as_mut()[0] = first;

    hdr.pkt_num = pn;
    hdr.pkt_num_len = pn_len;

    if hdr.ty == Type::Short {
        hdr.key_phase = (first & KEY_PHASE_BIT) != 0;
    }

    Ok(())
}

pub fn decode_pkt_num(largest_pn: u64, truncated_pn: u64, pn_len: usize) -> u64 {
    let pn_nbits = pn_len * 8;
    let expected_pn = largest_pn + 1;
    let pn_win = 1 << pn_nbits;
    let pn_hwin = pn_win / 2;
    let pn_mask = pn_win - 1;
    let candidate_pn = (expected_pn & !pn_mask) | truncated_pn;

    if candidate_pn + pn_hwin <= expected_pn && candidate_pn < (1 << 62) - pn_win {
        return candidate_pn + pn_win;
    }

    if candidate_pn > expected_pn + pn_hwin && candidate_pn >= pn_win {
        return candidate_pn - pn_win;
    }

    candidate_pn
}

pub fn decrypt_pkt<'a>(
    b: &'a mut octets::OctetsMut,
    pn: u64,
    pn_len: usize,
    payload_len: usize,
    aead: &crypto::Open,
) -> Result<octets::Octets<'a>> {
    let payload_offset = b.off();

    let (header, mut payload) = b.split_at(payload_offset)?;

    let payload_len = payload_len
        .checked_sub(pn_len)
        .ok_or(Error::InvalidPacket)?;

    let mut ciphertext = payload.peek_bytes_mut(payload_len)?;

    let payload_len = aead.open_with_u64_counter(pn, header.as_ref(), ciphertext.as_mut())?;

    Ok(b.get_bytes(payload_len)?)
}

pub fn encrypt_hdr(
    b: &mut octets::OctetsMut,
    pn_len: usize,
    payload: &[u8],
    aead: &crypto::Seal,
) -> Result<()> {
    let sample = &payload[MAX_PKT_NUM_LEN - pn_len..SAMPLE_LEN + (MAX_PKT_NUM_LEN - pn_len)];

    let mask = aead.new_mask(sample)?;

    let (mut first, mut rest) = b.split_at(1)?;

    let first = first.as_mut();

    if Header::is_long(first[0]) {
        first[0] ^= mask[0] & 0x0f;
    } else {
        first[0] ^= mask[0] & 0x1f;
    }

    let pn_buf = rest.slice_last(pn_len)?;
    for i in 0..pn_len {
        pn_buf[i] ^= mask[i + 1];
    }

    Ok(())
}

pub fn encrypt_pkt(
    b: &mut octets::OctetsMut,
    pn: u64,
    pn_len: usize,
    payload_len: usize,
    payload_offset: usize,
    extra_in: Option<&[u8]>,
    aead: &crypto::Seal,
) -> Result<usize> {
    let (mut header, mut payload) = b.split_at(payload_offset)?;

    let ciphertext_len =
        aead.seal_with_u64_counter(pn, header.as_ref(), payload.as_mut(), payload_len, extra_in)?;

    encrypt_hdr(&mut header, pn_len, payload.as_ref(), aead)?;

    Ok(payload_offset + ciphertext_len)
}

pub fn encode_pkt_num(pn: u64, pn_len: usize, b: &mut octets::OctetsMut) -> Result<()> {
    match pn_len {
        1 => b.put_u8(pn as u8)?,

        2 => b.put_u16(pn as u16)?,

        3 => b.put_u24(pn as u32)?,

        4 => b.put_u32(pn as u32)?,

        _ => return Err(Error::InvalidPacket),
    };

    Ok(())
}

pub fn negotiate_version(scid: &[u8], dcid: &[u8], out: &mut [u8]) -> Result<usize> {
    let mut b = octets::OctetsMut::with_slice(out);

    let first = rand::rand_u8() | FORM_BIT;

    b.put_u8(first)?;
    b.put_u32(0)?;

    b.put_u8(scid.len() as u8)?;
    b.put_bytes(scid)?;
    b.put_u8(dcid.len() as u8)?;
    b.put_bytes(dcid)?;
    b.put_u32(crate::PROTOCOL_VERSION_V1)?;

    Ok(b.off())
}

pub fn retry(
    scid: &[u8],
    dcid: &[u8],
    new_scid: &[u8],
    token: &[u8],
    version: u32,
    out: &mut [u8],
) -> Result<usize> {
    let mut b = octets::OctetsMut::with_slice(out);

    if !crate::version_is_supported(version) {
        return Err(Error::UnknownVersion);
    }

    let hdr = Header {
        ty: Type::Retry,
        version,
        dcid: ConnectionId::from_ref(scid),
        scid: ConnectionId::from_ref(new_scid),
        pkt_num: 0,
        pkt_num_len: 0,
        token: Some(token.to_vec()),
        versions: None,
        key_phase: false,
    };

    hdr.to_bytes(&mut b)?;

    let tag = compute_retry_integrity_tag(&b, dcid, version)?;

    b.put_bytes(tag.as_ref())?;

    Ok(b.off())
}

pub fn verify_retry_integrity(b: &octets::OctetsMut, odcid: &[u8], version: u32) -> Result<()> {
    const TAG_LEN: usize = RETRY_AEAD_ALG.tag_len();

    let tag = compute_retry_integrity_tag(b, odcid, version)?;

    crypto::verify_slices_are_equal(&b.as_ref()[..TAG_LEN], tag.as_ref())
}

fn compute_retry_integrity_tag(
    b: &octets::OctetsMut,
    odcid: &[u8],
    version: u32,
) -> Result<Vec<u8>> {
    const KEY_LEN: usize = RETRY_AEAD_ALG.key_len();
    const TAG_LEN: usize = RETRY_AEAD_ALG.tag_len();

    const RETRY_INTEGRITY_KEY_V1: [u8; KEY_LEN] = [
        0xbe, 0x0c, 0x69, 0x0b, 0x9f, 0x66, 0x57, 0x5a, 0x1d, 0x76, 0x6b, 0x54, 0xe3, 0x68, 0xc8,
        0x4e,
    ];

    const RETRY_INTEGRITY_NONCE_V1: [u8; crypto::MAX_NONCE_LEN] = [
        0x46, 0x15, 0x99, 0xd3, 0x5d, 0x63, 0x2b, 0xf2, 0x23, 0x98, 0x25, 0xbb,
    ];

    let (key, nonce) = match version {
        crate::PROTOCOL_VERSION_V1 => (&RETRY_INTEGRITY_KEY_V1, RETRY_INTEGRITY_NONCE_V1),

        _ => (&RETRY_INTEGRITY_KEY_V1, RETRY_INTEGRITY_NONCE_V1),
    };

    let hdr_len = b.off();

    let mut pseudo = vec![0; 1 + odcid.len() + hdr_len];

    let mut pb = octets::OctetsMut::with_slice(&mut pseudo);

    pb.put_u8(odcid.len() as u8)?;
    pb.put_bytes(odcid)?;
    pb.put_bytes(&b.buf()[..hdr_len])?;

    let key = crypto::PacketKey::new(
        RETRY_AEAD_ALG,
        key.to_vec(),
        nonce.to_vec(),
        crypto::Seal::ENCRYPT,
    )?;

    let mut out_tag = vec![0_u8; TAG_LEN];

    let out_len = key.seal_with_u64_counter(0, &pseudo, &mut out_tag, 0, None)?;

    // Ensure that the output only contains the AEAD tag.
    if out_len != out_tag.len() {
        return Err(Error::CryptoFail);
    }

    Ok(out_tag)
}

pub struct KeyUpdate {
    /// 1-RTT key used prior to a key update.
    pub crypto_open: crypto::Open,

    /// The packet number triggered the latest key-update.
    ///
    /// Incoming packets with lower pn should use this (prev) crypto key.
    pub pn_on_update: u64,

    /// Whether ACK frame for key-update has been sent.
    pub update_acked: bool,

    /// When the old key should be discarded.
    pub timer: Instant,
}

pub struct PktNumSpace {
    /// The largest packet number received.
    pub largest_rx_pkt_num: u64,

    /// Time the largest packet number received.
    pub largest_rx_pkt_time: Instant,

    /// The largest non-probing packet number.
    pub largest_rx_non_probing_pkt_num: u64,

    /// The largest packet number send in the packet number space so far.
    pub largest_tx_pkt_num: Option<u64>,

    /// Range of packet numbers that we need to send an ACK for.
    pub recv_pkt_need_ack: ranges::RangeSet,

    /// Tracks received packet numbers.
    pub recv_pkt_num: PktNumWindow,

    /// Track if a received packet is ack eliciting.
    pub ack_elicited: bool,
}

impl PktNumSpace {
    pub fn new() -> PktNumSpace {
        PktNumSpace {
            largest_rx_pkt_num: 0,
            largest_rx_pkt_time: Instant::now(),
            largest_rx_non_probing_pkt_num: 0,
            largest_tx_pkt_num: None,
            recv_pkt_need_ack: ranges::RangeSet::new(crate::MAX_ACK_RANGES),
            recv_pkt_num: PktNumWindow::default(),
            ack_elicited: false,
        }
    }

    pub fn clear(&mut self) {
        self.ack_elicited = false;
    }

    pub fn ready(&self) -> bool {
        self.ack_elicited
    }

    pub fn on_packet_sent(&mut self, sent_pkt: &recovery::Sent) {
        // Track the largest packet number sent
        self.largest_tx_pkt_num = self.largest_tx_pkt_num.max(Some(sent_pkt.pkt_num));
    }
}

pub struct CryptoContext {
    pub key_update: Option<KeyUpdate>,
    pub crypto_open: Option<crypto::Open>,
    pub crypto_seal: Option<crypto::Seal>,
    pub crypto_0rtt_open: Option<crypto::Open>,
    pub crypto_stream: stream::Stream,
}

impl CryptoContext {
    pub fn new() -> CryptoContext {
        let crypto_stream = stream::Stream::new(
            0, // dummy
            u64::MAX,
            u64::MAX,
            true,
            true,
            stream::MAX_STREAM_WINDOW,
        );
        CryptoContext {
            key_update: None,
            crypto_open: None,
            crypto_seal: None,
            crypto_0rtt_open: None,
            crypto_stream,
        }
    }

    pub fn clear(&mut self) {
        self.crypto_open = None;
        self.crypto_seal = None;
        self.crypto_stream = <stream::Stream>::new(
            0, // dummy
            u64::MAX,
            u64::MAX,
            true,
            true,
            stream::MAX_STREAM_WINDOW,
        );
    }

    pub fn data_available(&self) -> bool {
        self.crypto_stream.is_flushable()
    }

    pub fn crypto_overhead(&self) -> Option<usize> {
        Some(self.crypto_seal.as_ref()?.alg().tag_len())
    }

    pub fn has_keys(&self) -> bool {
        self.crypto_open.is_some() && self.crypto_seal.is_some()
    }
}

/// QUIC recommends skipping packet numbers to elicit a [faster ACK] or to
/// mitigate an [optimistic ACK attack] (OACK attack). quiche currently skips
/// packets only for the purposes of optimistic attack mitigation.
///
/// ## What is an Optimistic ACK attack
/// A typical endpoint is responsible for making concurrent progress on multiple
/// connections and needs to fairly allocate resources across those connection.
/// In order to ensure fairness, an endpoint relies on "recovery signals" to
/// determine the optimal sending rate per connection. For example, when a new
/// flow joins a shared network, it might induce packet loss for other flows and
/// cause those flows to yield bandwidth on the network. ACKs are the primary
/// source of recovery signals for a QUIC connection.
///
/// The goal of an OACK attack is to mount a DDoS attack by exploiting recovery
/// signals and causing a server to expand its sending rate. A server with an
/// inflated sending rate would then be able to flood a shared network and
/// cripple all other flows. The fundamental reason that makes OACK
/// attach possible is the use of unvalidated ACK data, which is then used to
/// modify internal state. Therefore at a high level, a mitigation should
/// validate the incoming ACK data before use.
///
/// ## Optimistic ACK attack mitigation
/// quiche follows the RFC's recommendation for mitigating an [optimistic ACK
/// attack] by skipping packets and validating that the peer does NOT send ACKs
/// for those skipped packets. If an ACK for a skipped packet is received, the
/// connection is closed with a [PROTOCOL_VIOLATION] error.
///
/// A robust mitigation should skip packets randomly to ensure that an attacker
/// can't predict which packet number was skipped. Skip/validation should happen
/// "periodically" over the lifetime of the connection. Since skipping packets
/// also elicits a faster ACK, we need to balance the skip frequency to
/// sufficiently validate the peer without impacting other aspects of recovery.
///
/// A naive approach could be to skip a random packet number in the range
/// 200-500 (pick some static range). While this might work, its not apparent if
/// a static range is effective for all networks with varying bandwidths/RTTs.
///
/// Since an attacker can potentially influence the sending rate once per
/// "round", it would be ideal to validate the peer once per round. Therefore,
/// an ideal range seems to be one that dynamically adjusts based on packets
/// sent per round, ie. adjust skip range based on the current CWND.
///
/// [faster ACK]: https://www.rfc-editor.org/rfc/rfc9002.html#section-6.2.4
/// [optimistic ACK attack]: https://www.rfc-editor.org/rfc/rfc9000.html#section-21.4
/// [PROTOCOL_VIOLATION]: https://www.rfc-editor.org/rfc/rfc9000#section-13.1
pub struct PktNumManager {
    // TODO:
    // Defer including next_pkt_num in order to reduce the size of this patch
    // /// Next packet number.
    // next_pkt_num: u64,
    /// Track if we have skipped a packet number.
    skip_pn: Option<u64>,

    /// Track when to skip the next packet number
    ///
    /// None indicates the counter is not armed while Some(0) indicates that the
    /// counter has expired.
    pub skip_pn_counter: Option<u64>,
}

impl PktNumManager {
    pub fn new() -> Self {
        PktNumManager {
            skip_pn: None,
            skip_pn_counter: None,
        }
    }

    pub fn on_packet_sent(
        &mut self,
        cwnd: usize,
        max_datagram_size: usize,
        handshake_completed: bool,
    ) {
        // Decrement skip_pn_counter for each packet sent
        if let Some(counter) = &mut self.skip_pn_counter {
            *counter = counter.saturating_sub(1);
        } else if self.should_arm_skip_counter(handshake_completed) {
            self.arm_skip_counter(cwnd, max_datagram_size);
        }
    }

    fn should_arm_skip_counter(&self, handshake_completed: bool) -> bool {
        // Arm if the counter is not set
        let counter_not_set = self.skip_pn_counter.is_none();
        // Don't arm until we have verified the current skip_pn. Rearming the
        // counter could result in overwriting the skip_pn before
        // validating the current skip_pn.
        let no_current_skip_packet = self.skip_pn.is_none();

        // Skip pn only after the handshake has completed
        counter_not_set && no_current_skip_packet && handshake_completed
    }

    pub fn should_skip_pn(&self, handshake_completed: bool) -> bool {
        // Only skip a new packet once we have validated  the peer hasn't sent the
        // current skip packet.  For the purposes of OACK, a skip_pn is
        // considered validated once we receive an ACK for pn > skip_pn.
        let no_current_skip_packet = self.skip_pn.is_none();
        let counter_expired = match self.skip_pn_counter {
            // Skip if counter has expired
            Some(counter) => counter == 0,
            // Don't skip if the counter has not been set
            None => false,
        };

        // Skip pn only after the handshake has completed
        counter_expired && no_current_skip_packet && handshake_completed
    }

    pub fn skip_pn(&self) -> Option<u64> {
        self.skip_pn
    }

    pub fn set_skip_pn(&mut self, skip_pn: Option<u64>) {
        if skip_pn.is_some() {
            // Never overwrite skip_pn until the previous one has been verified
            debug_assert!(self.skip_pn.is_none());
            // The skip_pn_counter should be expired
            debug_assert_eq!(self.skip_pn_counter.unwrap(), 0);
        }

        self.skip_pn = skip_pn;
        // unset the counter
        self.skip_pn_counter = None;
    }

    // Dynamically vary the skip counter based on the CWND.
    fn arm_skip_counter(&mut self, cwnd: usize, max_datagram_size: usize) {
        let packets_per_cwnd = (cwnd / max_datagram_size) as u64;
        let lower = packets_per_cwnd / 2;
        let upper = packets_per_cwnd * 2;
        // rand_u64_uniform requires a non-zero value so add 1
        let skip_range = upper - lower + 1;
        let rand_skip_value = rand::rand_u64_uniform(skip_range);

        // Skip calculation:
        // skip_counter = min_skip
        //                + lower
        //                + rand(skip_range.lower, skip_range.upper)
        //
        //```
        // c: the current packet number
        // s: range of random packet number to skip from
        //
        // curr_pn
        //  |
        //  v                 |--- (upper - lower) ---|
        // [c x x x x x x x x s s s s s s s s s s s s s x x]
        //    |--min_skip---| |------skip_range-------|
        //
        //```
        let skip_pn_counter = MIN_SKIP_COUNTER_VALUE + lower + rand_skip_value;

        self.skip_pn_counter = Some(skip_pn_counter);
    }
}

#[derive(Clone, Copy, Default)]
pub struct PktNumWindow {
    lower: u64,
    window: u128,
}

impl PktNumWindow {
    pub fn insert(&mut self, seq: u64) {
        // Packet is on the left end of the window.
        if seq < self.lower {
            return;
        }

        // Packet is on the right end of the window.
        if seq > self.upper() {
            let diff = seq - self.upper();
            self.lower += diff;

            self.window = self.window.checked_shl(diff as u32).unwrap_or(0);
        }

        let mask = 1_u128 << (self.upper() - seq);
        self.window |= mask;
    }

    pub fn contains(&mut self, seq: u64) -> bool {
        // Packet is on the right end of the window.
        if seq > self.upper() {
            return false;
        }

        // Packet is on the left end of the window.
        if seq < self.lower {
            return true;
        }

        let mask = 1_u128 << (self.upper() - seq);
        self.window & mask != 0
    }

    fn upper(&self) -> u64 {
        self.lower.saturating_add(size_of::<u128>() as u64 * 8) - 1
    }
}
