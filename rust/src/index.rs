//! Git index (staging area) read/write
//! Parity: libgit2 src/libgit2/index.c

use std::fs;
use std::path::Path;

use crate::error::MuonGitError;
use crate::oid::OID;
use crate::sha1::SHA1;

const INDEX_SIGNATURE: &[u8] = b"DIRC";
const INDEX_VERSION: u32 = 2;
const ENTRY_FIXED_SIZE: usize = 62; // 10*4 + 20 + 2

/// A single entry in the git index
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub ctime_secs: u32,
    pub ctime_nanos: u32,
    pub mtime_secs: u32,
    pub mtime_nanos: u32,
    pub dev: u32,
    pub ino: u32,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub file_size: u32,
    pub oid: OID,
    pub flags: u16,
    pub path: String,
}

/// The parsed git index
#[derive(Debug, Clone)]
pub struct Index {
    pub version: u32,
    pub entries: Vec<IndexEntry>,
}

impl Index {
    pub fn new() -> Self {
        Index {
            version: INDEX_VERSION,
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, entry: IndexEntry) {
        if let Some(pos) = self.entries.iter().position(|e| e.path == entry.path) {
            self.entries[pos] = entry;
        } else {
            self.entries.push(entry);
            self.entries.sort_by(|a, b| a.path.cmp(&b.path));
        }
    }

    pub fn remove(&mut self, path: &str) -> bool {
        if let Some(pos) = self.entries.iter().position(|e| e.path == path) {
            self.entries.remove(pos);
            true
        } else {
            false
        }
    }

    pub fn find(&self, path: &str) -> Option<&IndexEntry> {
        self.entries.iter().find(|e| e.path == path)
    }
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(data[offset..offset + 2].try_into().unwrap())
}

/// Read and parse the git index file.
pub fn read_index(git_dir: &Path) -> Result<Index, MuonGitError> {
    let index_path = git_dir.join("index");
    if !index_path.exists() {
        return Ok(Index::new());
    }

    let data = fs::read(&index_path)?;
    parse_index(&data)
}

/// Parse raw index file bytes.
fn parse_index(data: &[u8]) -> Result<Index, MuonGitError> {
    if data.len() < 12 {
        return Err(MuonGitError::InvalidObject("index too short".into()));
    }

    // Validate signature
    if &data[0..4] != INDEX_SIGNATURE {
        return Err(MuonGitError::InvalidObject("bad index signature".into()));
    }

    let version = read_u32(data, 4);
    if version != 2 {
        return Err(MuonGitError::InvalidObject(format!("unsupported index version {}", version)));
    }

    let entry_count = read_u32(data, 8) as usize;

    // Validate checksum
    if data.len() < 20 {
        return Err(MuonGitError::InvalidObject("index too short for checksum".into()));
    }
    let content = &data[..data.len() - 20];
    let stored_checksum = &data[data.len() - 20..];
    let computed = SHA1::hash(content);
    if computed != stored_checksum {
        return Err(MuonGitError::InvalidObject("index checksum mismatch".into()));
    }

    let mut entries = Vec::with_capacity(entry_count);
    let mut offset = 12;

    for _ in 0..entry_count {
        if offset + ENTRY_FIXED_SIZE > content.len() {
            return Err(MuonGitError::InvalidObject("index truncated".into()));
        }

        let ctime_secs = read_u32(data, offset);
        let ctime_nanos = read_u32(data, offset + 4);
        let mtime_secs = read_u32(data, offset + 8);
        let mtime_nanos = read_u32(data, offset + 12);
        let dev = read_u32(data, offset + 16);
        let ino = read_u32(data, offset + 20);
        let mode = read_u32(data, offset + 24);
        let uid = read_u32(data, offset + 28);
        let gid = read_u32(data, offset + 32);
        let file_size = read_u32(data, offset + 36);

        let oid = OID::from_bytes(data[offset + 40..offset + 60].to_vec());
        let flags = read_u16(data, offset + 60);

        // Read null-terminated path
        let path_start = offset + ENTRY_FIXED_SIZE;
        let path_end = data[path_start..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| path_start + p)
            .ok_or_else(|| MuonGitError::InvalidObject("unterminated path in index".into()))?;

        let path = String::from_utf8(data[path_start..path_end].to_vec())
            .map_err(|_| MuonGitError::InvalidObject("invalid UTF-8 path in index".into()))?;

        // Compute padding to 8-byte alignment
        let entry_len = ENTRY_FIXED_SIZE + path.len() + 1; // +1 for null terminator
        let padded_len = (entry_len + 7) & !7;
        offset += padded_len;

        entries.push(IndexEntry {
            ctime_secs,
            ctime_nanos,
            mtime_secs,
            mtime_nanos,
            dev,
            ino,
            mode,
            uid,
            gid,
            file_size,
            oid,
            flags,
            path,
        });
    }

    Ok(Index { version, entries })
}

/// Write the index to the git directory.
pub fn write_index(git_dir: &Path, index: &Index) -> Result<(), MuonGitError> {
    let data = serialize_index(index);
    let index_path = git_dir.join("index");
    fs::write(&index_path, &data)?;
    Ok(())
}

/// Serialize an index to bytes.
fn serialize_index(index: &Index) -> Vec<u8> {
    let mut buf = Vec::new();

    // Header
    buf.extend_from_slice(INDEX_SIGNATURE);
    buf.extend_from_slice(&index.version.to_be_bytes());

    // Sort entries by path
    let mut sorted = index.entries.clone();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));

    buf.extend_from_slice(&(sorted.len() as u32).to_be_bytes());

    for entry in &sorted {
        buf.extend_from_slice(&entry.ctime_secs.to_be_bytes());
        buf.extend_from_slice(&entry.ctime_nanos.to_be_bytes());
        buf.extend_from_slice(&entry.mtime_secs.to_be_bytes());
        buf.extend_from_slice(&entry.mtime_nanos.to_be_bytes());
        buf.extend_from_slice(&entry.dev.to_be_bytes());
        buf.extend_from_slice(&entry.ino.to_be_bytes());
        buf.extend_from_slice(&entry.mode.to_be_bytes());
        buf.extend_from_slice(&entry.uid.to_be_bytes());
        buf.extend_from_slice(&entry.gid.to_be_bytes());
        buf.extend_from_slice(&entry.file_size.to_be_bytes());
        buf.extend_from_slice(entry.oid.raw());

        // Flags: lower 12 bits = min(path_len, 0xFFF), upper bits from entry
        let name_len = std::cmp::min(entry.path.len(), 0xFFF) as u16;
        let flags = (entry.flags & 0xF000) | name_len;
        buf.extend_from_slice(&flags.to_be_bytes());

        // Path + null padding to 8-byte alignment
        buf.extend_from_slice(entry.path.as_bytes());
        let entry_len = ENTRY_FIXED_SIZE + entry.path.len() + 1;
        let padded_len = (entry_len + 7) & !7;
        let pad_count = padded_len - ENTRY_FIXED_SIZE - entry.path.len();
        buf.extend(std::iter::repeat(0u8).take(pad_count));
    }

    // Checksum
    let checksum = SHA1::hash(&buf);
    buf.extend_from_slice(&checksum);

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(path: &str, mode: u32, oid: &OID, file_size: u32) -> IndexEntry {
        IndexEntry {
            ctime_secs: 0,
            ctime_nanos: 0,
            mtime_secs: 0,
            mtime_nanos: 0,
            dev: 0,
            ino: 0,
            mode,
            uid: 0,
            gid: 0,
            file_size,
            oid: oid.clone(),
            flags: 0,
            path: path.to_string(),
        }
    }

    #[test]
    fn test_read_write_empty_index() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_index_empty");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let index = Index::new();
        write_index(repo.git_dir(), &index).unwrap();

        let loaded = read_index(repo.git_dir()).unwrap();
        assert_eq!(loaded.version, 2);
        assert!(loaded.entries.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_read_write_single_entry() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_index_single");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let oid = OID::from_hex("ce013625030ba8dba906f756967f9e9ca394464a").unwrap();
        let entry = make_entry("hello.txt", 0o100644, &oid, 6);

        let mut index = Index::new();
        index.add(entry);
        write_index(repo.git_dir(), &index).unwrap();

        let loaded = read_index(repo.git_dir()).unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].path, "hello.txt");
        assert_eq!(loaded.entries[0].mode, 0o100644);
        assert_eq!(loaded.entries[0].oid, oid);
        assert_eq!(loaded.entries[0].file_size, 6);
        assert_eq!(loaded.entries[0].flags & 0xFFF, 9); // "hello.txt".len()

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_read_write_multiple_entries_sorted() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_index_multi");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let mut index = Index::new();
        index.add(make_entry("z.txt", 0o100644, &oid, 0));
        index.add(make_entry("a.txt", 0o100644, &oid, 0));
        index.add(make_entry("lib/main.c", 0o100644, &oid, 0));

        write_index(repo.git_dir(), &index).unwrap();

        let loaded = read_index(repo.git_dir()).unwrap();
        assert_eq!(loaded.entries.len(), 3);
        assert_eq!(loaded.entries[0].path, "a.txt");
        assert_eq!(loaded.entries[1].path, "lib/main.c");
        assert_eq!(loaded.entries[2].path, "z.txt");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_add_remove_find() {
        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let mut index = Index::new();
        index.add(make_entry("foo.txt", 0o100644, &oid, 0));
        index.add(make_entry("bar.txt", 0o100644, &oid, 0));

        assert!(index.find("foo.txt").is_some());
        assert!(index.find("nonexistent").is_none());

        assert!(index.remove("foo.txt"));
        assert!(!index.remove("foo.txt"));
        assert!(index.find("foo.txt").is_none());
        assert_eq!(index.entries.len(), 1);
    }

    #[test]
    fn test_checksum_validation() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_index_checksum");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let mut index = Index::new();
        index.add(make_entry("test.txt", 0o100644, &oid, 10));
        write_index(repo.git_dir(), &index).unwrap();

        // Corrupt the data
        let index_path = repo.git_dir().join("index");
        let mut data = std::fs::read(&index_path).unwrap();
        data[20] ^= 0xFF; // flip a byte in the entry data
        std::fs::write(&index_path, &data).unwrap();

        assert!(read_index(repo.git_dir()).is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
