//! Moving a logged-in profile between machines.
//!
//! `web_login` is a person sitting down and passing a challenge by hand. That is the most expensive
//! thing this tool ever asks for, and until now the result lived in one directory on one machine:
//! set up a second machine, do it again. Worse, a profile that cannot be moved cannot be backed up,
//! so a disk failure costs every login.
//!
//! A profile is cookies and site storage — it *is* the session. So the archive is encrypted, with a
//! password the operator supplies, and there is no way to write one without a password. An
//! unencrypted export would be a file that logs whoever holds it into everything, sitting in a
//! downloads folder.
//!
//! What is not copied: caches, code caches, GPU shader caches and crash reports. They are most of
//! the bytes, none of the value, and a cache from another machine is a cache full of paths that do
//! not exist here.

use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Directories inside a profile that are pure cost to carry.
///
/// Matched on the path relative to the profile root, so a `Cache` directory nested under
/// `Default/` is caught as readily as one at the top.
const SKIP: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "GrShaderCache",
    "ShaderCache",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "Crashpad",
    "component_crx_cache",
    "extensions_crx_cache",
    "Service Worker",
    "blob_storage",
];

/// Should this path go into the archive?
///
/// Pure, so the rule can be checked without a profile on disk.
pub fn worth_keeping(relative: &Path) -> bool {
    !relative.components().any(|c| {
        let name = c.as_os_str().to_string_lossy();
        SKIP.iter().any(|s| name.eq_ignore_ascii_case(s))
    })
}

/// Write `profile` to `archive`, encrypted with `password`.
///
/// Returns how many files went in and how many bytes the archive is.
pub fn export(profile: &Path, archive: &Path, password: &str) -> Result<(usize, u64)> {
    if password.trim().is_empty() {
        anyhow::bail!(
            "a profile export needs a password: the archive is the session, and an unencrypted \
             one is a file that logs whoever holds it into everything"
        );
    }
    if !profile.is_dir() {
        anyhow::bail!("no profile at {}", profile.display());
    }
    if let Some(parent) = archive.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let file =
        std::fs::File::create(archive).with_context(|| format!("create {}", archive.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .with_aes_encryption(zip::AesMode::Aes256, password);

    let mut count = 0usize;
    let mut buf = Vec::new();
    for entry in walk(profile)? {
        let Ok(relative) = entry.strip_prefix(profile) else {
            continue;
        };
        if !worth_keeping(relative) {
            continue;
        }
        // Forward slashes: an archive written on Windows has to open on the other two.
        let name = relative.to_string_lossy().replace('\\', "/");
        zip.start_file(name, options)?;
        buf.clear();
        std::fs::File::open(&entry)?.read_to_end(&mut buf)?;
        zip.write_all(&buf)?;
        count += 1;
    }
    zip.finish()?;
    let bytes = std::fs::metadata(archive).map(|m| m.len()).unwrap_or(0);
    Ok((count, bytes))
}

/// Unpack an archive into `profile`.
pub fn import(archive: &Path, profile: &Path, password: &str) -> Result<usize> {
    let file =
        std::fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file)?;
    std::fs::create_dir_all(profile)?;
    let mut count = 0usize;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index_decrypt(i, password.as_bytes())
            .map_err(|e| anyhow::anyhow!("cannot read the archive — wrong password? ({e})"))?;
        // `enclosed_name` refuses `..` and absolute paths. An archive is untrusted input even when
        // the operator made it: this is the difference between unpacking a profile and writing
        // wherever the archive says.
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        let target = profile.join(&relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&target)?;
        std::io::copy(&mut entry, &mut out)?;
        count += 1;
    }
    Ok(count)
}

/// Every file under `root`, depth first.
fn walk(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                // Pruned here as well as when writing, so a cache directory is never even walked.
                if path.strip_prefix(root).map(worth_keeping).unwrap_or(true) {
                    stack.push(path);
                }
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("svipall-profile-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn sample_profile() -> PathBuf {
        let dir = scratch("src");
        std::fs::create_dir_all(dir.join("Default")).expect("dirs");
        std::fs::create_dir_all(dir.join("Default/Cache/js")).expect("dirs");
        std::fs::write(dir.join("Default/Cookies"), b"session=abc").expect("write");
        std::fs::write(dir.join("Local State"), b"{}").expect("write");
        std::fs::write(dir.join("Default/Cache/js/blob"), vec![0u8; 4096]).expect("write");
        dir
    }

    #[test]
    fn a_profile_survives_a_round_trip() {
        // The point of the whole file: the login a person did by hand, on another machine.
        let src = sample_profile();
        let out = scratch("archive").join("profile.zip");
        let (files, bytes) = export(&src, &out, "correct horse").expect("exported");
        assert!(files >= 2, "{files} files");
        assert!(bytes > 0);

        let dst = scratch("dst");
        let restored = import(&out, &dst, "correct horse").expect("imported");
        assert_eq!(restored, files);
        assert_eq!(
            std::fs::read(dst.join("Default/Cookies")).expect("cookies"),
            b"session=abc"
        );
    }

    #[test]
    fn the_wrong_password_does_not_produce_a_profile() {
        let src = sample_profile();
        let out = scratch("archive2").join("profile.zip");
        export(&src, &out, "right").expect("exported");
        let dst = scratch("dst2");
        assert!(import(&out, &dst, "wrong").is_err());
    }

    #[test]
    fn there_is_no_way_to_write_one_without_a_password() {
        // An unencrypted export is a file that logs whoever holds it into everything, sitting in
        // a downloads folder.
        let src = sample_profile();
        let out = scratch("archive3").join("profile.zip");
        let err = export(&src, &out, "  ").expect_err("refused");
        assert!(err.to_string().contains("password"), "{err}");
    }

    #[test]
    fn caches_are_left_behind() {
        // Most of the bytes, none of the value, and a cache from another machine is full of paths
        // that do not exist on this one.
        assert!(!worth_keeping(Path::new("Default/Cache/js/blob")));
        assert!(!worth_keeping(Path::new("Default/Code Cache/x")));
        assert!(!worth_keeping(Path::new("GPUCache/data_0")));
        assert!(worth_keeping(Path::new("Default/Cookies")));
        assert!(worth_keeping(Path::new("Local State")));
        // A file that merely has "cache" in its name is not a cache directory.
        assert!(worth_keeping(Path::new("Default/CacheableThing.db")));
    }

    #[test]
    fn a_cache_is_not_carried_even_when_it_is_the_biggest_thing_there() {
        let src = sample_profile();
        let out = scratch("archive4").join("profile.zip");
        let (files, _) = export(&src, &out, "pw").expect("exported");
        let dst = scratch("dst4");
        import(&out, &dst, "pw").expect("imported");
        assert!(!dst.join("Default/Cache/js/blob").exists());
        assert_eq!(files, 2, "only the two files worth carrying");
    }

    #[test]
    fn a_profile_that_is_not_there_is_an_error_rather_than_an_empty_archive() {
        // An empty archive that imports cleanly is the worst outcome: it looks like it worked.
        let out = scratch("archive5").join("profile.zip");
        assert!(export(Path::new("no/such/profile"), &out, "pw").is_err());
    }
}
