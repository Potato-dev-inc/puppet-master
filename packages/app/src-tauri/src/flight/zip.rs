//! A small ZIP writer (deflate, no ZIP64, UTF-8 names) so the diagnostic file needs no new crate:
//! `flate2` (MIT OR Apache-2.0) does the compression and `crc32fast` (MIT OR Apache-2.0) the checksums,
//! and both were already in the build through `ureq`.
//!
//! Format: PKWARE APPNOTE 6.3.x. Local header + data per file, then the central directory and the
//! end-of-central-directory record.

use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::io::Write;

struct Entry {
    name: String,
    crc: u32,
    compressed: u32,
    size: u32,
    offset: u32,
}

pub struct ZipWriter<W: Write> {
    out: W,
    written: u64,
    entries: Vec<Entry>,
    dos_time: u16,
    dos_date: u16,
}

fn dos_datetime(unix_secs: u64) -> (u16, u16) {
    let days = (unix_secs / 86_400) as i64;
    let secs = unix_secs % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    let year = (year.clamp(1980, 2107) - 1980) as u16;
    let date = (year << 9) | ((month as u16) << 5) | day as u16;
    let time = (((secs / 3600) as u16) << 11) | ((((secs % 3600) / 60) as u16) << 5) | ((secs % 60) / 2) as u16;
    (time, date)
}

impl<W: Write> ZipWriter<W> {
    pub fn new(out: W) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let (dos_time, dos_date) = dos_datetime(now);
        Self { out, written: 0, entries: Vec::new(), dos_time, dos_date }
    }

    fn put(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.out.write_all(bytes)?;
        self.written += bytes.len() as u64;
        Ok(())
    }

    /// Add one file. `name` uses `/` separators and no leading slash.
    pub fn add(&mut self, name: &str, data: &[u8]) -> std::io::Result<()> {
        if data.len() > u32::MAX as usize / 2 || self.written > u32::MAX as u64 / 2 {
            return Err(std::io::Error::other("diagnostic file too large for a plain zip"));
        }
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data)?;
        let compressed = encoder.finish()?;
        let crc = crc32fast::hash(data);
        let offset = self.written as u32;
        let mut header = Vec::with_capacity(30 + name.len());
        header.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
        header.extend_from_slice(&20_u16.to_le_bytes()); // version needed
        header.extend_from_slice(&0x0800_u16.to_le_bytes()); // flags: names are UTF-8
        header.extend_from_slice(&8_u16.to_le_bytes()); // deflate
        header.extend_from_slice(&self.dos_time.to_le_bytes());
        header.extend_from_slice(&self.dos_date.to_le_bytes());
        header.extend_from_slice(&crc.to_le_bytes());
        header.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        header.extend_from_slice(&(data.len() as u32).to_le_bytes());
        header.extend_from_slice(&(name.len() as u16).to_le_bytes());
        header.extend_from_slice(&0_u16.to_le_bytes()); // extra length
        header.extend_from_slice(name.as_bytes());
        self.put(&header)?;
        self.put(&compressed)?;
        self.entries.push(Entry {
            name: name.to_string(),
            crc,
            compressed: compressed.len() as u32,
            size: data.len() as u32,
            offset,
        });
        Ok(())
    }

    /// Write the directory and hand back the output.
    pub fn finish(mut self) -> std::io::Result<W> {
        let dir_start = self.written as u32;
        let entries = std::mem::take(&mut self.entries);
        for e in &entries {
            let mut rec = Vec::with_capacity(46 + e.name.len());
            rec.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
            rec.extend_from_slice(&20_u16.to_le_bytes()); // version made by
            rec.extend_from_slice(&20_u16.to_le_bytes()); // version needed
            rec.extend_from_slice(&0x0800_u16.to_le_bytes());
            rec.extend_from_slice(&8_u16.to_le_bytes());
            rec.extend_from_slice(&self.dos_time.to_le_bytes());
            rec.extend_from_slice(&self.dos_date.to_le_bytes());
            rec.extend_from_slice(&e.crc.to_le_bytes());
            rec.extend_from_slice(&e.compressed.to_le_bytes());
            rec.extend_from_slice(&e.size.to_le_bytes());
            rec.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            rec.extend_from_slice(&[0; 2 + 2 + 2 + 2]); // extra, comment, disk, internal attrs
            rec.extend_from_slice(&0_u32.to_le_bytes()); // external attrs
            rec.extend_from_slice(&e.offset.to_le_bytes());
            rec.extend_from_slice(e.name.as_bytes());
            self.put(&rec)?;
        }
        let dir_len = self.written as u32 - dir_start;
        let mut end = Vec::with_capacity(22);
        end.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
        end.extend_from_slice(&[0; 4]); // disk numbers
        end.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        end.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        end.extend_from_slice(&dir_len.to_le_bytes());
        end.extend_from_slice(&dir_start.to_le_bytes());
        end.extend_from_slice(&0_u16.to_le_bytes()); // comment length
        self.put(&end)?;
        self.out.flush()?;
        Ok(self.out)
    }
}

/// Read a zip made by [`ZipWriter`] back: `(name, bytes)` per file. Used by tests to prove the
/// archive is well formed (checksums included); not a general reader.
#[cfg(test)]
pub fn read_back(zip: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    use flate2::read::DeflateDecoder;
    use std::io::Read;
    let u16_at = |at: usize| u16::from_le_bytes([zip[at], zip[at + 1]]) as usize;
    let u32_at = |at: usize| u32::from_le_bytes([zip[at], zip[at + 1], zip[at + 2], zip[at + 3]]);
    let eocd = zip.len().checked_sub(22).ok_or("too short")?;
    if u32_at(eocd) != 0x0605_4b50 {
        return Err("no end-of-central-directory record".into());
    }
    let count = u16_at(eocd + 10);
    let mut at = u32_at(eocd + 16) as usize;
    let mut out = Vec::new();
    for _ in 0..count {
        if u32_at(at) != 0x0201_4b50 {
            return Err("bad central directory entry".into());
        }
        let crc = u32_at(at + 16);
        let compressed = u32_at(at + 20) as usize;
        let size = u32_at(at + 24) as usize;
        let name_len = u16_at(at + 28);
        let extra = u16_at(at + 30) + u16_at(at + 32);
        let local = u32_at(at + 42) as usize;
        let name = String::from_utf8(zip[at + 46..at + 46 + name_len].to_vec()).map_err(|e| e.to_string())?;
        if u32_at(local) != 0x0403_4b50 {
            return Err("bad local header".into());
        }
        let data_at = local + 30 + u16_at(local + 26) + u16_at(local + 28);
        let mut data = Vec::with_capacity(size);
        DeflateDecoder::new(&zip[data_at..data_at + compressed]).read_to_end(&mut data).map_err(|e| e.to_string())?;
        if data.len() != size || crc32fast::hash(&data) != crc {
            return Err(format!("{name}: size or checksum mismatch"));
        }
        out.push((name, data));
        at += 46 + name_len + extra;
    }
    Ok(out)
}
