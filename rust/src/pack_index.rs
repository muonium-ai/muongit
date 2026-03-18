//! Pack index (.idx) file parsing
//! Parity: libgit2 src/libgit2/pack.c (index parsing)

use std::fs;
use std::path::Path;

use crate::error::MuonGitError;
use crate::oid::OID;

const IDX_MAGIC: [u8; 4] = [0xFF, 0x74, 0x4F, 0x63]; // "\377tOc"
const IDX_VERSION: u32 = 2;
const FANOUT_COUNT: usize = 256;

/// A parsed pack index file
#[derive(Debug, Clone)]
pub struct PackIndex {
    /// Total number of objects
    pub count: u32,
    /// Fanout table (256 entries)
    pub fanout: [u32; FANOUT_COUNT],
    /// Sorted OIDs
    pub oids: Vec<OID>,
    /// CRC32 checksums for each object
    pub crcs: Vec<u32>,
    /// Pack file offsets for each object
    pub offsets: Vec<u64>,
}

impl PackIndex {
    /// Look up an OID in the index. Returns the pack file offset if found.
    pub fn find(&self, oid: &OID) -> Option<u64> {
        let raw = oid.raw();
        if raw.is_empty() {
            return None;
        }
        let first_byte = raw[0] as usize;

        let start = if first_byte == 0 { 0 } else { self.fanout[first_byte - 1] as usize };
        let end = self.fanout[first_byte] as usize;

        // Binary search within the range
        let slice = &self.oids[start..end];
        match slice.binary_search_by(|entry| entry.raw().cmp(raw)) {
            Ok(pos) => Some(self.offsets[start + pos]),
            Err(_) => None,
        }
    }

    /// Check if the index contains a given OID.
    pub fn contains(&self, oid: &OID) -> bool {
        self.find(oid).is_some()
    }
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// Parse a pack index file from disk.
pub fn read_pack_index(path: &Path) -> Result<PackIndex, MuonGitError> {
    let data = fs::read(path)?;
    parse_pack_index(&data)
}

/// Parse pack index bytes.
pub fn parse_pack_index(data: &[u8]) -> Result<PackIndex, MuonGitError> {
    // Minimum size: 8 (header) + 1024 (fanout) + 40 (checksums) = 1072
    if data.len() < 1072 {
        return Err(MuonGitError::InvalidObject("pack index too short".into()));
    }

    // Validate magic and version
    if data[0..4] != IDX_MAGIC {
        return Err(MuonGitError::InvalidObject("bad pack index magic".into()));
    }
    let version = read_u32(data, 4);
    if version != IDX_VERSION {
        return Err(MuonGitError::InvalidObject(format!("unsupported pack index version {}", version)));
    }

    // Read fanout table
    let mut fanout = [0u32; FANOUT_COUNT];
    for (i, entry) in fanout.iter_mut().enumerate() {
        *entry = read_u32(data, 8 + i * 4);
    }
    let count = fanout[255];

    // Validate sizes
    let oid_table_start = 8 + FANOUT_COUNT * 4; // 1032
    let crc_table_start = oid_table_start + count as usize * 20;
    let offset_table_start = crc_table_start + count as usize * 4;
    let min_size = offset_table_start + count as usize * 4 + 40; // +40 for two checksums
    if data.len() < min_size {
        return Err(MuonGitError::InvalidObject("pack index truncated".into()));
    }

    // Read OIDs
    let mut oids = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let start = oid_table_start + i * 20;
        oids.push(OID::from_bytes(data[start..start + 20].to_vec()));
    }

    // Read CRC32s
    let mut crcs = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        crcs.push(read_u32(data, crc_table_start + i * 4));
    }

    // Read offsets
    let large_offset_start = offset_table_start + count as usize * 4;
    let mut offsets = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let raw_offset = read_u32(data, offset_table_start + i * 4);
        if raw_offset & 0x80000000 != 0 {
            // Large offset — index into the 8-byte offset table
            let large_idx = (raw_offset & 0x7FFFFFFF) as usize;
            let lo = large_offset_start + large_idx * 8;
            if lo + 8 > data.len() {
                return Err(MuonGitError::InvalidObject("pack index large offset out of bounds".into()));
            }
            let hi = u64::from_be_bytes(data[lo..lo + 8].try_into().unwrap());
            offsets.push(hi);
        } else {
            offsets.push(raw_offset as u64);
        }
    }

    Ok(PackIndex { count, fanout, oids, crcs, offsets })
}

/// Build a pack index from components (for testing).
pub fn build_pack_index(oids: &[OID], crcs: &[u32], offsets: &[u64]) -> Vec<u8> {
    let _count = oids.len();
    let mut buf = Vec::new();

    // Header
    buf.extend_from_slice(&IDX_MAGIC);
    buf.extend_from_slice(&IDX_VERSION.to_be_bytes());

    // Build fanout table
    let mut fanout = [0u32; FANOUT_COUNT];
    for oid in oids {
        let first = oid.raw()[0] as usize;
        for entry in fanout.iter_mut().skip(first) {
            *entry += 1;
        }
    }
    for f in &fanout {
        buf.extend_from_slice(&f.to_be_bytes());
    }

    // OID table (must be sorted — caller responsibility)
    for oid in oids {
        buf.extend_from_slice(oid.raw());
    }

    // CRC32 table
    for crc in crcs {
        buf.extend_from_slice(&crc.to_be_bytes());
    }

    // Offset table
    for offset in offsets {
        if *offset > 0x7FFFFFFF {
            // Would need large offset table — not used in basic tests
            buf.extend_from_slice(&(*offset as u32).to_be_bytes());
        } else {
            buf.extend_from_slice(&(*offset as u32).to_be_bytes());
        }
    }

    // Pack checksum (dummy)
    buf.extend_from_slice(&[0u8; 20]);

    // Index checksum (dummy — we don't validate it during parse)
    buf.extend_from_slice(&[0u8; 20]);

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted_oids() -> (Vec<OID>, Vec<u32>, Vec<u64>) {
        let mut oids = vec![
            OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap(),
            OID::from_hex("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap(),
            OID::from_hex("ccf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap(),
        ];
        oids.sort_by(|a, b| a.raw().cmp(b.raw()));
        let crcs = vec![0x12345678, 0x23456789, 0x3456789A];
        let offsets = vec![12u64, 256, 1024];
        (oids, crcs, offsets)
    }

    #[test]
    fn test_parse_pack_index() {
        let (oids, crcs, offsets) = sorted_oids();
        let data = build_pack_index(&oids, &crcs, &offsets);
        let idx = parse_pack_index(&data).unwrap();

        assert_eq!(idx.count, 3);
        assert_eq!(idx.oids.len(), 3);
        assert_eq!(idx.crcs.len(), 3);
        assert_eq!(idx.offsets.len(), 3);
    }

    #[test]
    fn test_pack_index_find() {
        let (oids, crcs, offsets) = sorted_oids();
        let data = build_pack_index(&oids, &crcs, &offsets);
        let idx = parse_pack_index(&data).unwrap();

        assert_eq!(idx.find(&oids[0]), Some(offsets[0]));
        assert_eq!(idx.find(&oids[1]), Some(offsets[1]));
        assert_eq!(idx.find(&oids[2]), Some(offsets[2]));

        let missing = OID::from_hex("ddf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        assert!(idx.find(&missing).is_none());
    }

    #[test]
    fn test_pack_index_contains() {
        let (oids, crcs, offsets) = sorted_oids();
        let data = build_pack_index(&oids, &crcs, &offsets);
        let idx = parse_pack_index(&data).unwrap();

        assert!(idx.contains(&oids[0]));
        assert!(idx.contains(&oids[1]));

        let missing = OID::from_hex("0000000000000000000000000000000000000001").unwrap();
        assert!(!idx.contains(&missing));
    }

    #[test]
    fn test_pack_index_fanout() {
        let (oids, crcs, offsets) = sorted_oids();
        let data = build_pack_index(&oids, &crcs, &offsets);
        let idx = parse_pack_index(&data).unwrap();

        // All three OIDs start with 0xaa, 0xbb, 0xcc
        assert_eq!(idx.fanout[0xa9], 0); // nothing before 0xaa
        assert_eq!(idx.fanout[0xaa], 1); // one object with first byte <= 0xaa
        assert_eq!(idx.fanout[0xbb], 2);
        assert_eq!(idx.fanout[0xcc], 3);
        assert_eq!(idx.fanout[255], 3); // total
    }

    #[test]
    fn test_pack_index_empty() {
        let data = build_pack_index(&[], &[], &[]);
        let idx = parse_pack_index(&data).unwrap();
        assert_eq!(idx.count, 0);
        assert!(idx.oids.is_empty());
    }

    #[test]
    fn test_pack_index_bad_magic() {
        let mut data = build_pack_index(&[], &[], &[]);
        data[0] = 0x00;
        assert!(parse_pack_index(&data).is_err());
    }
}
