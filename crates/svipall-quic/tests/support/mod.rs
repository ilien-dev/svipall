//! Reading our own first flight, the same way Chrome's was read.
//!
//! A QUIC Initial packet is encrypted with keys derived from a salt published in RFC 9001 and the
//! connection id that travels in the clear in its own header, so anything holding the datagram can
//! read it. That is what makes these tests offline: the client's first flight is taken straight out
//! of `send()` and decoded here, with no socket, no server and no packet capture — and the
//! reference in `docs/http3.md` was produced by pointing exactly this decoder at a real Chrome.

use btls_sys as ffi;
use std::collections::BTreeMap;

/// RFC 9001 section 5.2, fixed for QUIC v1.
const INITIAL_SALT: [u8; 20] = [
    0x38, 0x76, 0x2c, 0xf7, 0xf5, 0x59, 0x34, 0xb3, 0x4d, 0x17, 0x9a, 0xe6, 0xa4, 0xc8, 0x0c, 0xad,
    0xcc, 0xbb, 0x7f, 0x0a,
];

/// What a ClientHello said, in the order it said it.
pub struct ClientHello {
    pub session_id_len: usize,
    pub ciphers: Vec<u16>,
    /// `(type, length)` per extension, in wire order.
    pub extensions: Vec<(u16, usize)>,
    pub transport_params: Vec<(u64, Vec<u8>)>,
}

impl ClientHello {
    pub fn has(&self, ext: u16) -> bool {
        self.extensions.iter().any(|(t, _)| *t == ext)
    }
    pub fn order(&self) -> Vec<u16> {
        self.extensions.iter().map(|(t, _)| *t).collect()
    }
}

/// Open a client connection to a name that will never answer, and read the ClientHello it wrote.
pub fn first_flight(config: &mut quiche::Config) -> ClientHello {
    let mut scid = [0u8; 16];
    fill(&mut scid);
    let scid = quiche::ConnectionId::from_ref(&scid);
    let mut conn = quiche::connect(
        Some("quic.test"),
        &scid,
        "127.0.0.1:0".parse().unwrap(),
        "127.0.0.1:443".parse().unwrap(),
        config,
    )
    .expect("a client connection");

    let mut out = [0u8; 1500];
    let mut crypto: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
    while let Ok((written, _)) = conn.send(&mut out) {
        let mut at = 0usize;
        while at < written {
            match read_initial(&out[at..written], &mut crypto) {
                Some(end) => at += end,
                None => break,
            }
        }
    }
    parse(&assemble(&crypto))
}

/// Any bytes will do: a connection id only has to be unpredictable, and none of this is real.
fn fill(buf: &mut [u8]) {
    let mut x = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
        | 1;
    for b in buf.iter_mut() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = x as u8;
    }
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let mut len = 0u32;
    unsafe {
        ffi::HMAC(
            ffi::EVP_sha256(),
            key.as_ptr() as *const _,
            key.len(),
            data.as_ptr(),
            data.len(),
            out.as_mut_ptr(),
            &mut len,
        );
    }
    out
}

fn hkdf_expand(prk: &[u8], info: &[u8], n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev: Vec<u8> = Vec::new();
    let mut counter = 1u8;
    while out.len() < n {
        let mut input = prev.clone();
        input.extend_from_slice(info);
        input.push(counter);
        prev = hmac_sha256(prk, &input).to_vec();
        out.extend_from_slice(&prev);
        counter += 1;
    }
    out.truncate(n);
    out
}

/// TLS 1.3 HKDF-Expand-Label, which QUIC borrows wholesale.
fn expand_label(secret: &[u8], label: &str, n: usize) -> Vec<u8> {
    let full = format!("tls13 {label}");
    let mut info = Vec::new();
    info.extend_from_slice(&(n as u16).to_be_bytes());
    info.push(full.len() as u8);
    info.extend_from_slice(full.as_bytes());
    info.push(0);
    hkdf_expand(secret, &info, n)
}

fn aes_ecb_block(key: &[u8], block: &[u8]) -> [u8; 16] {
    let mut k = std::mem::MaybeUninit::<ffi::AES_KEY>::zeroed();
    let mut out = [0u8; 16];
    unsafe {
        ffi::AES_set_encrypt_key(key.as_ptr(), 128, k.as_mut_ptr());
        ffi::AES_encrypt(block.as_ptr(), out.as_mut_ptr(), k.as_ptr());
    }
    out
}

fn aes_gcm_open(key: &[u8], nonce: &[u8], aad: &[u8], ct: &[u8]) -> Option<Vec<u8>> {
    let mut out = vec![0u8; ct.len()];
    let mut out_len = 0usize;
    let ok = unsafe {
        let mut ctx = std::mem::MaybeUninit::<ffi::EVP_AEAD_CTX>::zeroed();
        if ffi::EVP_AEAD_CTX_init(
            ctx.as_mut_ptr(),
            ffi::EVP_aead_aes_128_gcm(),
            key.as_ptr(),
            key.len(),
            16,
            std::ptr::null_mut(),
        ) != 1
        {
            return None;
        }
        let r = ffi::EVP_AEAD_CTX_open(
            ctx.as_ptr(),
            out.as_mut_ptr(),
            &mut out_len,
            out.len(),
            nonce.as_ptr(),
            nonce.len(),
            ct.as_ptr(),
            ct.len(),
            aad.as_ptr(),
            aad.len(),
        );
        ffi::EVP_AEAD_CTX_cleanup(ctx.as_mut_ptr());
        r
    };
    (ok == 1).then(|| {
        out.truncate(out_len);
        out
    })
}

/// QUIC's variable-length integer: the top two bits say how many bytes it occupies.
pub fn varint(b: &[u8], i: &mut usize) -> u64 {
    let first = b[*i];
    let n = 1usize << (first >> 6);
    let mut v = (first & 0x3f) as u64;
    for k in 1..n {
        v = (v << 8) | b[*i + k] as u64;
    }
    *i += n;
    v
}

/// Decrypt one client Initial and add its CRYPTO frames to `crypto`; return where it ended, so
/// coalesced packets in one datagram can be walked.
fn read_initial(pkt: &[u8], crypto: &mut BTreeMap<u64, Vec<u8>>) -> Option<usize> {
    if pkt.len() < 32 || pkt[0] & 0x80 == 0 || pkt[0] & 0x30 != 0x00 {
        return None;
    }
    let mut i = 5; // first byte and version
    let dcid_len = pkt[i] as usize;
    i += 1;
    let dcid = pkt[i..i + dcid_len].to_vec();
    i += dcid_len;
    i += 1 + pkt[i] as usize; // source connection id
    let token_len = varint(pkt, &mut i) as usize;
    i += token_len;
    let length = varint(pkt, &mut i) as usize;
    let pn_offset = i;
    let packet_end = pn_offset + length;
    if packet_end > pkt.len() || pn_offset + 20 > pkt.len() {
        return None;
    }

    let initial = hmac_sha256(&INITIAL_SALT, &dcid);
    let client = expand_label(&initial, "client in", 32);
    let key = expand_label(&client, "quic key", 16);
    let iv = expand_label(&client, "quic iv", 12);
    let hp = expand_label(&client, "quic hp", 16);

    // Header protection: a mask taken from the ciphertext unmasks the packet number.
    let mask = aes_ecb_block(&hp, &pkt[pn_offset + 4..pn_offset + 20]);
    let first = pkt[0] ^ (mask[0] & 0x0f);
    let pn_len = ((first & 0x03) + 1) as usize;
    let pn_bytes: Vec<u8> = (0..pn_len)
        .map(|k| pkt[pn_offset + k] ^ mask[1 + k])
        .collect();
    let pn = pn_bytes.iter().fold(0u64, |a, b| (a << 8) | *b as u64);

    let mut header = pkt[..pn_offset + pn_len].to_vec();
    header[0] = first;
    header[pn_offset..].copy_from_slice(&pn_bytes);

    let mut nonce = iv.clone();
    for (k, b) in pn.to_be_bytes().iter().enumerate() {
        nonce[4 + k] ^= b;
    }

    let plain = aes_gcm_open(&key, &nonce, &header, &pkt[pn_offset + pn_len..packet_end])?;

    // Only PADDING, PING and CRYPTO appear in a client Initial.
    let mut j = 0usize;
    while j < plain.len() {
        match plain[j] {
            0x00 | 0x01 => j += 1,
            0x06 => {
                j += 1;
                let off = varint(&plain, &mut j);
                let len = varint(&plain, &mut j) as usize;
                crypto.insert(off, plain[j..j + len].to_vec());
                j += len;
            }
            _ => break,
        }
    }
    Some(packet_end)
}

/// Splice the CRYPTO stream together from offset zero, stopping at the first hole.
fn assemble(crypto: &BTreeMap<u64, Vec<u8>>) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for (off, data) in crypto {
        let off = *off as usize;
        if off > out.len() {
            break;
        }
        if off + data.len() > out.len() {
            out.extend_from_slice(&data[out.len() - off..]);
        }
    }
    out
}

fn be16(b: &[u8], i: usize) -> u16 {
    u16::from_be_bytes([b[i], b[i + 1]])
}

fn parse(ch: &[u8]) -> ClientHello {
    assert_eq!(
        ch[0], 0x01,
        "the CRYPTO stream does not start with a ClientHello"
    );
    let declared = ((ch[1] as usize) << 16) | ((ch[2] as usize) << 8) | ch[3] as usize;
    assert_eq!(
        declared,
        ch.len() - 4,
        "the ClientHello was not fully reassembled"
    );
    let body = &ch[4..];

    let mut i = 2 + 32;
    let session_id_len = body[i] as usize;
    i += 1 + session_id_len;
    let cs_len = be16(body, i) as usize;
    i += 2;
    let ciphers = (0..cs_len / 2).map(|k| be16(body, i + k * 2)).collect();
    i += cs_len;
    i += 1 + body[i] as usize; // legacy_compression_methods
    let end = i + 2 + be16(body, i) as usize;
    i += 2;

    let mut extensions = Vec::new();
    let mut transport_params = Vec::new();
    while i + 4 <= end {
        let t = be16(body, i);
        let len = be16(body, i + 2) as usize;
        if t == 57 {
            let tp = &body[i + 4..i + 4 + len];
            let mut j = 0usize;
            while j < tp.len() {
                let id = varint(tp, &mut j);
                let n = varint(tp, &mut j) as usize;
                transport_params.push((id, tp[j..j + n].to_vec()));
                j += n;
            }
        }
        extensions.push((t, len));
        i += 4 + len;
    }

    ClientHello {
        session_id_len,
        ciphers,
        extensions,
        transport_params,
    }
}
