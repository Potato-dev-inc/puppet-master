//! Unpacking a downloaded archive safely: `.zip`, `.tar.gz`, or a bare program (`Raw`).
//!
//! The archive is data from the internet, so nothing in it decides where a file goes:
//! * a path that is absolute, has a drive letter, a `..` part or a `:` in a part is refused (the whole unpack fails);
//! * a symbolic link or a hard link is refused (a zip entry marked as a link, a tar entry of type 1 or 2);
//! * a file is created new (never over another), so a duplicate name cannot write through anything;
//! * limits: [`MAX_EXTRACT_BYTES`] in total, [`MAX_ENTRIES`] entries, a zip's checksum is verified per file;
//! * nothing is made executable here; the caller marks the one program it will run.
//!
//! Only the standard library, `flate2` (already in the build) and our own small readers: no new dependency.
//! The caller has already checked the archive's SHA-256 against the registry before it calls [`extract`].

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use flate2::read::{DeflateDecoder, GzDecoder};

use super::registry::ArchiveKind;

pub const MAX_EXTRACT_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 50_000;

/// The relative path an entry name means, or an error when it could leave the folder. `Ok(None)`: the entry is the
/// folder itself (`.`, `./`) and is skipped.
pub fn safe_entry_path(name: &str) -> Result<Option<PathBuf>, String> {
    let flat = name.replace('\\', "/");
    if flat.contains('\0') {
        return Err(format!("an entry name has a NUL byte ({name:?})"));
    }
    if flat.starts_with('/') {
        return Err(format!("an entry has an absolute path ({name:?})"));
    }
    let mut out = PathBuf::new();
    for part in flat.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(format!("an entry climbs out of the folder ({name:?})")),
            p if p.contains(':') => return Err(format!("an entry has a drive letter or stream name ({name:?})")),
            p => out.push(p),
        }
    }
    Ok((out.components().count() > 0).then_some(out))
}

struct Budget {
    bytes: u64,
    entries: usize,
}

impl Budget {
    fn entry(&mut self) -> Result<(), String> {
        self.entries += 1;
        if self.entries > MAX_ENTRIES {
            return Err(format!("it has more than {MAX_ENTRIES} entries"));
        }
        Ok(())
    }
    fn room(&self) -> u64 {
        MAX_EXTRACT_BYTES.saturating_sub(self.bytes)
    }
}

fn target(dest: &Path, rel: &Path) -> Result<PathBuf, String> {
    let path = dest.join(rel);
    if !path.starts_with(dest) {
        return Err(format!("an entry would be written outside the folder ({})", rel.display()));
    }
    Ok(path)
}

fn make_dir(dest: &Path, rel: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target(dest, rel)?).map_err(|e| format!("cannot make a folder: {e}"))
}

/// Copies at most `size` bytes from `from` to a new file at `dest/rel`; returns what was written.
fn write_file(dest: &Path, rel: &Path, from: &mut dyn Read, size: u64, budget: &mut Budget) -> Result<u64, String> {
    if size > budget.room() {
        return Err(format!("it holds more than {} MiB once unpacked", MAX_EXTRACT_BYTES / 1024 / 1024));
    }
    let path = target(dest, rel)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot make a folder: {e}"))?;
    }
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| format!("cannot create {}: {e}", rel.display()))?;
    let written = std::io::copy(&mut from.take(size), &mut file).map_err(|e| format!("cannot write {}: {e}", rel.display()))?;
    file.flush().map_err(|e| e.to_string())?;
    budget.bytes += written;
    Ok(written)
}

/// Unpacks `archive` into the (new, empty) folder `dest`. `raw_name` is the program's relative path for `Raw`.
/// Returns the number of files written.
pub fn extract(kind: ArchiveKind, archive: &Path, dest: &Path, raw_name: &str) -> Result<usize, String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("cannot make {}: {e}", dest.display()))?;
    match kind {
        ArchiveKind::Zip => extract_zip(archive, dest),
        ArchiveKind::TarGz => {
            let file = File::open(archive).map_err(|e| e.to_string())?;
            extract_tar(GzDecoder::new(file), dest)
        }
        ArchiveKind::TarBz2 => Err("a .tar.bz2 archive is not supported yet".into()),
        ArchiveKind::Raw => {
            let rel = safe_entry_path(raw_name)?.ok_or("the program has no name")?;
            let mut file = File::open(archive).map_err(|e| e.to_string())?;
            let size = file.metadata().map_err(|e| e.to_string())?.len();
            write_file(dest, &rel, &mut file, size, &mut Budget { bytes: 0, entries: 0 })?;
            Ok(1)
        }
    }
}

// ---- tar ---------------------------------------------------------------------------------------------------

fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn octal(bytes: &[u8]) -> Result<u64, String> {
    if bytes.first().is_some_and(|b| b & 0x80 != 0) {
        return Err("a tar entry uses a size format that is not supported".into());
    }
    let text = cstr(bytes);
    let text = text.trim();
    if text.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(text, 8).map_err(|_| "a tar header is damaged".to_string())
}

/// The `path` of a pax extended header (`<len> path=<value>\n` records).
fn pax_path(data: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(data);
    text.split('\n').find_map(|record| record.split_once(' ').and_then(|(_, kv)| kv.strip_prefix("path=")).map(str::to_string))
}

fn read_block(from: &mut dyn Read, buf: &mut [u8; 512]) -> Result<bool, String> {
    let mut filled = 0;
    while filled < 512 {
        let n = from.read(&mut buf[filled..]).map_err(|e| format!("cannot read the archive: {e}"))?;
        if n == 0 {
            return if filled == 0 { Ok(false) } else { Err("the archive ends in the middle of an entry".into()) };
        }
        filled += n;
    }
    Ok(true)
}

fn skip(from: &mut dyn Read, bytes: u64) -> Result<(), String> {
    let copied = std::io::copy(&mut from.take(bytes), &mut std::io::sink()).map_err(|e| format!("cannot read the archive: {e}"))?;
    if copied < bytes {
        return Err("the archive ends in the middle of an entry".into());
    }
    Ok(())
}

fn read_small(from: &mut dyn Read, size: u64) -> Result<Vec<u8>, String> {
    if size > 64 * 1024 {
        return Err("a tar header record is too large".into());
    }
    let mut data = vec![0u8; size as usize];
    from.read_exact(&mut data).map_err(|_| "the archive ends in the middle of an entry".to_string())?;
    Ok(data)
}

pub fn extract_tar<R: Read>(mut from: R, dest: &Path) -> Result<usize, String> {
    let mut budget = Budget { bytes: 0, entries: 0 };
    let (mut files, mut next_name): (usize, Option<String>) = (0, None);
    let mut header = [0u8; 512];
    while read_block(&mut from, &mut header)? {
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let expected = octal(&header[148..156])?;
        let sum: u64 = header.iter().enumerate().map(|(i, b)| if (148..156).contains(&i) { 32 } else { *b as u64 }).sum();
        if sum != expected {
            return Err("a tar header is damaged (checksum)".into());
        }
        let mut name = cstr(&header[0..100]);
        if &header[257..262] == b"ustar" {
            let prefix = cstr(&header[345..500]);
            if !prefix.is_empty() {
                name = format!("{prefix}/{name}");
            }
        }
        let size = octal(&header[124..136])?;
        let padding = (512 - size % 512) % 512;
        match header[156] {
            b'x' => {
                next_name = pax_path(&read_small(&mut from, size)?).or(next_name);
                skip(&mut from, padding)?;
            }
            b'g' => skip(&mut from, size + padding)?,
            b'L' => {
                next_name = Some(cstr(&read_small(&mut from, size)?));
                skip(&mut from, padding)?;
            }
            b'1' | b'2' => return Err("it contains a symbolic or hard link, which is refused".into()),
            kind @ (b'0' | 0 | b'7' | b'5') => {
                budget.entry()?;
                let name = next_name.take().unwrap_or(name);
                let rel = safe_entry_path(&name)?;
                match (kind, rel) {
                    (b'5', Some(rel)) => {
                        make_dir(dest, &rel)?;
                        skip(&mut from, size + padding)?;
                    }
                    (b'5', None) => skip(&mut from, size + padding)?,
                    (_, Some(rel)) => {
                        let written = write_file(dest, &rel, &mut from, size, &mut budget)?;
                        if written < size {
                            return Err("the archive ends in the middle of a file".into());
                        }
                        skip(&mut from, padding)?;
                        files += 1;
                    }
                    (_, None) => return Err("a file entry has no name".into()),
                }
            }
            other => return Err(format!("it contains an entry of an unsupported type ({})", other as char)),
        }
    }
    Ok(files)
}

// ---- zip ---------------------------------------------------------------------------------------------------

fn u16_at(b: &[u8], at: usize) -> usize {
    u16::from_le_bytes([b[at], b[at + 1]]) as usize
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<usize, String> {
    let mut file = File::open(archive).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    // The end-of-central-directory record is in the last 64 KiB + 22 bytes.
    let tail_len = len.min(65_557);
    file.seek(SeekFrom::Start(len - tail_len)).map_err(|e| e.to_string())?;
    let mut tail = vec![0u8; tail_len as usize];
    file.read_exact(&mut tail).map_err(|e| e.to_string())?;
    let eocd = (0..tail.len().saturating_sub(21)).rev().find(|&i| tail[i..i + 4] == [0x50, 0x4b, 0x05, 0x06]).ok_or("it is not a zip file")?;
    let record = &tail[eocd..];
    let (entries, cd_size, cd_offset) = (u16_at(record, 10), u32_at(record, 12), u32_at(record, 16));
    if entries == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF {
        return Err("a zip64 archive is not supported".into());
    }
    if cd_size > 64 * 1024 * 1024 || cd_offset as u64 + cd_size as u64 > len {
        return Err("the zip directory is damaged".into());
    }
    file.seek(SeekFrom::Start(cd_offset as u64)).map_err(|e| e.to_string())?;
    let mut directory = vec![0u8; cd_size as usize];
    file.read_exact(&mut directory).map_err(|_| "the zip directory is damaged".to_string())?;

    let mut budget = Budget { bytes: 0, entries: 0 };
    let (mut at, mut files) = (0usize, 0usize);
    for _ in 0..entries {
        if at + 46 > directory.len() || directory[at..at + 4] != [0x50, 0x4b, 0x01, 0x02] {
            return Err("the zip directory is damaged".into());
        }
        let d = &directory[at..];
        let (made_by, flags, method) = (u16_at(d, 4), u16_at(d, 8), u16_at(d, 10));
        let (crc, comp, size) = (u32_at(d, 16), u32_at(d, 20) as u64, u32_at(d, 24) as u64);
        let (n, m, k) = (u16_at(d, 28), u16_at(d, 30), u16_at(d, 32));
        let (attrs, local) = (u32_at(d, 38), u32_at(d, 42) as u64);
        if at + 46 + n + m + k > directory.len() {
            return Err("the zip directory is damaged".into());
        }
        let name = String::from_utf8_lossy(&directory[at + 46..at + 46 + n]).into_owned();
        at += 46 + n + m + k;
        budget.entry()?;
        if comp == 0xFFFF_FFFF || size == 0xFFFF_FFFF {
            return Err("a zip64 entry is not supported".into());
        }
        if flags & 1 != 0 {
            return Err("an encrypted zip is not supported".into());
        }
        let unix_mode = (attrs >> 16) & 0o170000;
        if (made_by >> 8 == 3 && unix_mode == 0o120000) || attrs & 0x400 != 0 {
            return Err("it contains a symbolic link, which is refused".into());
        }
        let rel = safe_entry_path(&name)?;
        if name.ends_with('/') || name.ends_with('\\') {
            if let Some(rel) = rel {
                make_dir(dest, &rel)?;
            }
            continue;
        }
        let rel = rel.ok_or("a file entry has no name")?;
        if size > budget.room() {
            return Err(format!("it holds more than {} MiB once unpacked", MAX_EXTRACT_BYTES / 1024 / 1024));
        }
        file.seek(SeekFrom::Start(local)).map_err(|e| e.to_string())?;
        let mut head = [0u8; 30];
        file.read_exact(&mut head).map_err(|_| "a zip entry is damaged".to_string())?;
        if head[0..4] != [0x50, 0x4b, 0x03, 0x04] {
            return Err("a zip entry is damaged".into());
        }
        file.seek(SeekFrom::Start(local + 30 + u16_at(&head, 26) as u64 + u16_at(&head, 28) as u64)).map_err(|e| e.to_string())?;
        let packed = (&mut file).take(comp);
        let mut crc_reader = CrcReader { inner: Box::new(match method {
            0 => Box::new(packed) as Box<dyn Read>,
            8 => Box::new(DeflateDecoder::new(packed)),
            other => return Err(format!("a zip entry uses compression method {other}, which is not supported")),
        }), crc: flate2::Crc::new() };
        let written = write_file(dest, &rel, &mut crc_reader, size, &mut budget)?;
        if written != size {
            return Err(format!("{name} is shorter than the zip says"));
        }
        // The entry must end exactly at its declared size.
        let mut extra = [0u8; 1];
        if crc_reader.read(&mut extra).map_err(|e| e.to_string())? != 0 {
            return Err(format!("{name} is longer than the zip says"));
        }
        if crc_reader.crc.sum() != crc {
            return Err(format!("{name} is damaged (checksum)"));
        }
        files += 1;
    }
    Ok(files)
}

struct CrcReader<'a> {
    inner: Box<dyn Read + 'a>,
    crc: flate2::Crc,
}

impl Read for CrcReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.crc.update(&buf[..n]);
        Ok(n)
    }
}

#[cfg(test)]
pub(crate) mod test_archives {
    //! Small archive writers for tests (stored zips, tar.gz), able to make the hostile shapes too.
    use std::io::Write;

    pub struct ZipEntry<'a> {
        pub name: &'a str,
        pub data: &'a [u8],
        /// Unix mode in the high 16 bits of the external attributes (0 = none).
        pub mode: u32,
        pub deflate: bool,
    }

    pub fn zip(entries: &[ZipEntry]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut directory: Vec<u8> = Vec::new();
        for e in entries {
            let mut crc = flate2::Crc::new();
            crc.update(e.data);
            let packed: Vec<u8> = if e.deflate {
                let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                enc.write_all(e.data).unwrap();
                enc.finish().unwrap()
            } else {
                e.data.to_vec()
            };
            let method: u16 = if e.deflate { 8 } else { 0 };
            let offset = out.len() as u32;
            out.extend_from_slice(&[0x50, 0x4b, 3, 4]);
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&crc.sum().to_le_bytes());
            out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            out.extend_from_slice(&(e.data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(e.name.as_bytes());
            out.extend_from_slice(&packed);
            directory.extend_from_slice(&[0x50, 0x4b, 1, 2]);
            directory.extend_from_slice(&((3u16 << 8) | 20).to_le_bytes());
            directory.extend_from_slice(&20u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&method.to_le_bytes());
            directory.extend_from_slice(&[0; 4]);
            directory.extend_from_slice(&crc.sum().to_le_bytes());
            directory.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            directory.extend_from_slice(&(e.data.len() as u32).to_le_bytes());
            directory.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            directory.extend_from_slice(&[0; 6]);
            directory.extend_from_slice(&[0; 2]);
            directory.extend_from_slice(&(e.mode << 16).to_le_bytes());
            directory.extend_from_slice(&offset.to_le_bytes());
            directory.extend_from_slice(e.name.as_bytes());
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&directory);
        out.extend_from_slice(&[0x50, 0x4b, 5, 6, 0, 0, 0, 0]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    /// One tar entry: `kind` is the type flag (`b'0'` file, `b'5'` folder, `b'2'` symlink).
    pub fn tar_entry(name: &str, kind: u8, data: &[u8]) -> Vec<u8> {
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..107].copy_from_slice(b"0000644");
        h[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
        h[156] = kind;
        h[257..263].copy_from_slice(b"ustar\0");
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|b| *b as u32).sum();
        h[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        let mut out = h.to_vec();
        out.extend_from_slice(data);
        out.resize(out.len() + (512 - data.len() % 512) % 512, 0);
        out
    }

    pub fn tar_gz(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut raw: Vec<u8> = entries.concat();
        raw.extend_from_slice(&[0u8; 1024]);
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&raw).unwrap();
        enc.finish().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_archives::*;
    use super::*;

    fn temp() -> PathBuf {
        std::env::temp_dir().join(format!("pm-acp-archive-{}", uuid::Uuid::new_v4()))
    }

    fn run(kind: ArchiveKind, bytes: &[u8]) -> (Result<usize, String>, PathBuf) {
        let dir = temp();
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("a.bin");
        std::fs::write(&archive, bytes).unwrap();
        let out = dir.join("out");
        (extract(kind, &archive, &out, "tool.exe"), dir)
    }

    #[test]
    fn entry_paths_that_could_escape_are_refused() {
        for bad in ["../x", "a/../../x", "/etc/x", "\\x", "C:\\x", "C:/x", "a/b:stream", "a\\..\\x", "x\0y"] {
            assert!(safe_entry_path(bad).is_err(), "{bad:?}");
        }
        assert_eq!(safe_entry_path("./bin/tool.exe").unwrap(), Some(PathBuf::from("bin").join("tool.exe")));
        assert_eq!(safe_entry_path("a\\b").unwrap(), Some(PathBuf::from("a").join("b")));
        assert_eq!(safe_entry_path("./").unwrap(), None);
    }

    #[test]
    fn a_zip_unpacks_stored_and_deflated_files() {
        let bytes = zip(&[
            ZipEntry { name: "bin/", data: b"", mode: 0, deflate: false },
            ZipEntry { name: "bin/tool.exe", data: b"MZ stored", mode: 0o100755, deflate: false },
            ZipEntry { name: "docs/readme.txt", data: &vec![b'a'; 5000], mode: 0, deflate: true },
        ]);
        let (result, dir) = run(ArchiveKind::Zip, &bytes);
        assert_eq!(result.unwrap(), 2);
        assert_eq!(std::fs::read(dir.join("out/bin/tool.exe")).unwrap(), b"MZ stored");
        assert_eq!(std::fs::read(dir.join("out/docs/readme.txt")).unwrap().len(), 5000);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_zip_with_a_traversal_name_a_link_or_a_bad_checksum_is_refused() {
        for (name, mode) in [("../evil.txt", 0), ("a/../../evil.txt", 0), ("/abs/evil.txt", 0), ("C:\\evil.txt", 0), ("link", 0o120777)] {
            let bytes = zip(&[ZipEntry { name, data: b"x", mode, deflate: false }]);
            let (result, dir) = run(ArchiveKind::Zip, &bytes);
            assert!(result.is_err(), "{name}");
            assert!(!dir.join("evil.txt").exists() && !dir.join("out").join("evil.txt").exists());
            let _ = std::fs::remove_dir_all(dir);
        }
        // a flipped byte in the data: the CRC catches it
        let mut bytes = zip(&[ZipEntry { name: "a.txt", data: b"hello world", mode: 0, deflate: false }]);
        let at = bytes.windows(5).position(|w| w == b"hello").unwrap();
        bytes[at] = b'j';
        let (result, dir) = run(ArchiveKind::Zip, &bytes);
        assert!(result.unwrap_err().contains("checksum"));
        let _ = std::fs::remove_dir_all(dir);
        // not a zip at all
        assert!(run(ArchiveKind::Zip, b"just text, not an archive at all").0.is_err());
    }

    #[test]
    fn a_tar_gz_unpacks_and_refuses_traversal_and_links() {
        let good = tar_gz(&[tar_entry("pkg/", b'5', b""), tar_entry("pkg/tool", b'0', b"#!/bin/sh\n"), tar_entry("./top.txt", b'0', &vec![b'z'; 700])]);
        let (result, dir) = run(ArchiveKind::TarGz, &good);
        assert_eq!(result.unwrap(), 2);
        assert_eq!(std::fs::read(dir.join("out/pkg/tool")).unwrap(), b"#!/bin/sh\n");
        assert_eq!(std::fs::read(dir.join("out/top.txt")).unwrap().len(), 700);
        let _ = std::fs::remove_dir_all(dir);

        for entry in [tar_entry("../evil", b'0', b"x"), tar_entry("/abs/evil", b'0', b"x"), tar_entry("a/../../evil", b'0', b"x"), tar_entry("link", b'2', b""), tar_entry("hard", b'1', b""), tar_entry("dev", b'3', b"")] {
            let (result, dir) = run(ArchiveKind::TarGz, &tar_gz(&[tar_entry("ok.txt", b'0', b"fine"), entry]));
            assert!(result.is_err());
            assert!(!dir.join("evil").exists());
            let _ = std::fs::remove_dir_all(dir);
        }
        // a header that was changed after the checksum was made
        let mut raw = tar_gz(&[tar_entry("a.txt", b'0', b"abc")]);
        assert!(run(ArchiveKind::TarGz, &raw.split_off(5)).0.is_err());
    }

    #[test]
    fn a_raw_download_becomes_the_named_program_and_bz2_is_refused() {
        let (result, dir) = run(ArchiveKind::Raw, b"binary bytes");
        assert_eq!(result.unwrap(), 1);
        assert_eq!(std::fs::read(dir.join("out/tool.exe")).unwrap(), b"binary bytes");
        let _ = std::fs::remove_dir_all(dir);
        assert!(run(ArchiveKind::TarBz2, b"x").0.unwrap_err().contains("not supported"));
        let dir = temp();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a"), b"x").unwrap();
        assert!(extract(ArchiveKind::Raw, &dir.join("a"), &dir.join("out"), "../x").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
