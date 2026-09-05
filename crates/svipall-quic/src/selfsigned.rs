//! A throwaway certificate, made in this process, for a server that lives for one measurement.
//!
//! **This module is svipall's, not upstream quiche's.** See `PATCHES.md` entry 11.
//!
//! The QUIC handshake reference — `bench h3-ref`, and the offline test in `tests/settings.rs` —
//! needs a server a real Chrome will talk to, and a server needs a certificate. The obvious answer
//! is to commit one, which is what rustls, hyper and quiche itself all do. This does not, for one
//! reason that is about operating a repository rather than about cryptography: **a committed
//! `key.pem` is a private key in a public repository.** It is inert, it authenticates a name that
//! resolves nowhere, and it will still be flagged by every secret scanner that looks at the tree,
//! blocked by push protection on some accounts, and re-triaged by every person who ever greps for
//! one. A key that is generated, used and deleted inside a single process is not a finding.
//!
//! The other reason it is better is that it removes a maintenance edge nobody would remember: a
//! committed certificate expires, and the failure lands years later on somebody who has no idea
//! why a QUIC test suddenly stopped handshaking.
//!
//! There is no new dependency. BoringSSL is already linked into this binary — it is the whole
//! reason this crate exists in the vendored form it does — and generating a P-256 self-signed
//! certificate is a dozen of its calls.

// This module is ours, not upstream quiche's, so it is linted like the rest of the workspace
// rather than under the crate-wide `allow` that exists for vendored code. Same call, and same
// reasoning, as the patches in `svipall-cdp`.
#![warn(clippy::all)]

use btls_sys as ffi;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;

/// A certificate and key on disk, deleted when this value is dropped.
///
/// quiche loads both by path (`load_cert_chain_from_pem_file`), so they have to exist as files for
/// as long as it takes to build a `Config`. They are written under a directory of their own so the
/// cleanup is a single `remove_dir_all` and cannot take anything else with it.
pub struct SelfSigned {
    dir: PathBuf,
    /// The PEM certificate.
    pub cert_pem: PathBuf,
    /// The PEM private key. Ephemeral: this is the only place it has ever existed.
    pub key_pem: PathBuf,
    /// SHA-256 over the DER `SubjectPublicKeyInfo` — what Chrome's
    /// `--ignore-certificate-errors-spki-list` takes, base64-encoded.
    pub spki_sha256: [u8; 32],
}

impl SelfSigned {
    /// The pin in the form Chrome's flag wants.
    pub fn spki_pin(&self) -> String {
        base64_pin(&self.spki_sha256)
    }
}

/// A SHA-256 digest in the base64 Chrome's `--ignore-certificate-errors-spki-list` reads.
///
/// Base64 without a dependency: the alphabet is one line and pulling a crate into a vendored TLS
/// stack to encode thirty-two bytes is the wrong trade. Public so it can be checked against a
/// known vector rather than only through a digest nothing else knows.
pub fn base64_pin(digest: &[u8; 32]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(44);
    for chunk in digest.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

impl Drop for SelfSigned {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Anything that went wrong on the way to a certificate. There is nothing a caller can do about
/// any of them differently, so they are one type with a sentence each.
#[derive(Debug)]
pub struct Error(pub &'static str);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "generating a self-signed certificate: {}", self.0)
    }
}

impl std::error::Error for Error {}

/// Generate a P-256 self-signed certificate for `common_name`, valid for a day.
///
/// A day rather than a decade on purpose: nothing that uses this outlives a single command, and a
/// short life is the difference between a test artifact and something that could be mistaken for a
/// credential if it ever escaped the temporary directory.
pub fn generate(common_name: &str) -> Result<SelfSigned, Error> {
    let dir = std::env::temp_dir().join(format!(
        "svipall-selfsigned-{}-{}",
        std::process::id(),
        nonce()
    ));
    std::fs::create_dir_all(&dir).map_err(|_| Error("could not make a temporary directory"))?;
    let cert_pem = dir.join("cert.pem");
    let key_pem = dir.join("key.pem");

    // SAFETY: every pointer below is either freshly returned by BoringSSL and checked against
    // null, or a borrow of one that outlives the call. Each owning handle is freed exactly once,
    // on every path, by the guards declared beside it.
    let spki = unsafe {
        let ec = ffi::EC_KEY_new_by_curve_name(ffi::NID_X9_62_prime256v1);
        if ec.is_null() {
            return Err(Error("no P-256 group"));
        }
        let _ec = Guard(ec, |p| ffi::EC_KEY_free(p));
        if ffi::EC_KEY_generate_key(ec) != 1 {
            return Err(Error("key generation failed"));
        }

        let pkey = ffi::EVP_PKEY_new();
        if pkey.is_null() {
            return Err(Error("no key container"));
        }
        let _pkey = Guard(pkey, |p| ffi::EVP_PKEY_free(p));
        // `set1` rather than `assign`: it takes a reference instead of ownership, so the guard
        // above stays correct and there is no branch where the EC key is freed twice or not at all.
        if ffi::EVP_PKEY_set1_EC_KEY(pkey, ec) != 1 {
            return Err(Error("could not attach the key"));
        }

        let x = ffi::X509_new();
        if x.is_null() {
            return Err(Error("no certificate"));
        }
        let _x = Guard(x, |p| ffi::X509_free(p));
        // Version 3, encoded as 2. An X.509 v1 certificate cannot carry a subjectAltName, and
        // Chrome has required one since 58 — a common name alone is rejected outright.
        if ffi::X509_set_version(x, 2) != 1 {
            return Err(Error("could not set the version"));
        }
        ffi::ASN1_INTEGER_set(ffi::X509_get_serialNumber(x), 1);
        ffi::X509_gmtime_adj(ffi::X509_getm_notBefore(x), -3600);
        ffi::X509_gmtime_adj(ffi::X509_getm_notAfter(x), 86_400);
        if ffi::X509_set_pubkey(x, pkey) != 1 {
            return Err(Error("could not set the public key"));
        }

        let name = ffi::X509_get_subject_name(x);
        let cn = CString::new(common_name).map_err(|_| Error("the name has a NUL in it"))?;
        let field = CString::new("CN").expect("a literal without NUL");
        if ffi::X509_NAME_add_entry_by_txt(
            name,
            field.as_ptr(),
            ffi::MBSTRING_ASC,
            cn.as_ptr() as *const u8,
            -1,
            -1,
            0,
        ) != 1
        {
            return Err(Error("could not set the common name"));
        }
        // Self-signed: the issuer is the subject.
        if ffi::X509_set_issuer_name(x, name) != 1 {
            return Err(Error("could not set the issuer"));
        }

        let mut ctx: ffi::X509V3_CTX = std::mem::zeroed();
        ffi::X509V3_set_ctx(&mut ctx, x, x, ptr::null(), ptr::null(), 0);
        let san = CString::new(format!("DNS:{common_name}"))
            .map_err(|_| Error("the name has a NUL in it"))?;
        let ext =
            ffi::X509V3_EXT_nconf_nid(ptr::null(), &ctx, ffi::NID_subject_alt_name, san.as_ptr());
        if ext.is_null() {
            return Err(Error("could not build the subjectAltName"));
        }
        let added = ffi::X509_add_ext(x, ext, -1);
        ffi::X509_EXTENSION_free(ext);
        if added != 1 {
            return Err(Error("could not add the subjectAltName"));
        }

        if ffi::X509_sign(x, pkey, ffi::EVP_sha256()) == 0 {
            return Err(Error("signing failed"));
        }

        write_pem(&cert_pem, |bio| ffi::PEM_write_bio_X509(bio, x))?;
        write_pem(&key_pem, |bio| {
            ffi::PEM_write_bio_PrivateKey(
                bio,
                pkey,
                ptr::null(),
                ptr::null(),
                0,
                None,
                ptr::null_mut(),
            )
        })?;

        // The pin is over the whole DER SubjectPublicKeyInfo, not over the public key bits:
        // `X509_pubkey_digest` hashes the latter and would produce a value Chrome never matches.
        let mut der: *mut u8 = ptr::null_mut();
        let len = ffi::i2d_X509_PUBKEY(ffi::X509_get_X509_PUBKEY(x), &mut der);
        if len <= 0 || der.is_null() {
            return Err(Error("could not encode the public key"));
        }
        let mut out = [0u8; 32];
        ffi::SHA256(der, len as usize, out.as_mut_ptr());
        ffi::OPENSSL_free(der as *mut std::ffi::c_void);
        out
    };

    Ok(SelfSigned {
        dir,
        cert_pem,
        key_pem,
        spki_sha256: spki,
    })
}

/// Frees one BoringSSL handle exactly once, however the function it lives in returns.
struct Guard<T>(*mut T, unsafe fn(*mut T));

impl<T> Drop for Guard<T> {
    fn drop(&mut self) {
        // SAFETY: the pointer was checked non-null before this guard was made, and nothing else
        // frees it.
        unsafe { (self.1)(self.0) }
    }
}

/// Run one `PEM_write_bio_*` into a file, and make sure the file is closed before we return.
///
/// SAFETY: `write` receives a live `BIO` that this function owns and frees.
unsafe fn write_pem(
    path: &Path,
    write: impl FnOnce(*mut ffi::BIO) -> std::os::raw::c_int,
) -> Result<(), Error> {
    let p = CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| Error("the path has a NUL in it"))?;
    let mode = CString::new("w").expect("a literal without NUL");
    let bio = ffi::BIO_new_file(p.as_ptr(), mode.as_ptr());
    if bio.is_null() {
        return Err(Error("could not open the file to write"));
    }
    let ok = write(bio);
    ffi::BIO_free(bio);
    if ok != 1 {
        return Err(Error("could not write the PEM"));
    }
    Ok(())
}

/// Enough randomness to keep two concurrent runs out of each other's directory. Not a secret.
fn nonce() -> u64 {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut h = RandomState::new().build_hasher();
    h.write_u64(
        std::time::UNIX_EPOCH
            .elapsed()
            .map_or(0, |d| d.as_nanos() as u64),
    );
    h.finish()
}
