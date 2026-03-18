//! Tree object read/write

use crate::error::MuonGitError;
use crate::oid::OID;

/// File mode constants for tree entries
pub mod file_mode {
    pub const TREE: u32 = 0o040000;
    pub const BLOB: u32 = 0o100644;
    pub const BLOB_EXE: u32 = 0o100755;
    pub const LINK: u32 = 0o120000;
    pub const GITLINK: u32 = 0o160000;
}

/// A single entry in a tree object
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub mode: u32,
    pub name: String,
    pub oid: OID,
}

impl TreeEntry {
    pub fn is_tree(&self) -> bool {
        self.mode == file_mode::TREE
    }

    pub fn is_blob(&self) -> bool {
        self.mode == file_mode::BLOB || self.mode == file_mode::BLOB_EXE
    }
}

/// A parsed git tree object
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    pub oid: OID,
    pub entries: Vec<TreeEntry>,
}

/// Fast inline octal parse from bytes (no string allocation)
#[inline]
fn parse_octal(bytes: &[u8]) -> u32 {
    let mut val: u32 = 0;
    for &b in bytes {
        val = val.wrapping_mul(8).wrapping_add((b - b'0') as u32);
    }
    val
}

/// Parse a tree object from its raw binary data
pub fn parse_tree(oid: OID, data: &[u8]) -> Result<Tree, MuonGitError> {
    let mut entries = Vec::new();
    let len = data.len();
    let mut i = 0;

    while i < len {
        // Parse mode (octal digits until space)
        let mode_start = i;
        while i < len && data[i] != b' ' {
            i += 1;
        }
        if i >= len {
            return Err(MuonGitError::InvalidObject(
                "tree entry: missing space after mode".into(),
            ));
        }
        let mode = parse_octal(&data[mode_start..i]);
        i += 1; // skip space

        // Parse name (until null byte)
        let name_start = i;
        while i < len && data[i] != 0 {
            i += 1;
        }
        if i >= len {
            return Err(MuonGitError::InvalidObject(
                "tree entry: missing null after name".into(),
            ));
        }
        let name = std::str::from_utf8(&data[name_start..i])
            .map_err(|_| MuonGitError::InvalidObject("tree entry: invalid name".into()))?
            .to_string();
        i += 1; // skip null

        // Read 20-byte raw OID
        if i + 20 > len {
            return Err(MuonGitError::InvalidObject(
                "tree entry: truncated OID".into(),
            ));
        }
        let oid_bytes = data[i..i + 20].to_vec();
        i += 20;

        entries.push(TreeEntry {
            mode,
            name,
            oid: OID::from_bytes(oid_bytes),
        });
    }

    Ok(Tree { oid, entries })
}

/// Return the octal mode string bytes for common git modes (no allocation).
#[inline]
fn mode_bytes(mode: u32) -> &'static [u8] {
    match mode {
        0o100644 => b"100644",
        0o040000 => b"40000",
        0o100755 => b"100755",
        0o120000 => b"120000",
        0o160000 => b"160000",
        _ => b"",
    }
}

/// Serialize tree entries to raw binary data (without the object header).
/// Entries are sorted by name with tree-sorting rules.
pub fn serialize_tree(entries: &[TreeEntry]) -> Vec<u8> {
    let mut sorted: Vec<&TreeEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| {
        // Compare without allocation: append '/' byte for trees during comparison
        let a_name = a.name.as_bytes();
        let b_name = b.name.as_bytes();
        let a_trail: &[u8] = if a.is_tree() { b"/" } else { b"" };
        let b_trail: &[u8] = if b.is_tree() { b"/" } else { b"" };
        a_name.iter().chain(a_trail).cmp(b_name.iter().chain(b_trail))
    });

    // Pre-allocate: each entry is ~28 bytes (6 mode + 1 space + ~12 name + 1 null + 20 oid)
    let mut buf = Vec::with_capacity(entries.len() * 40);
    for entry in sorted {
        let mb = mode_bytes(entry.mode);
        if !mb.is_empty() {
            buf.extend_from_slice(mb);
        } else {
            // Fallback for uncommon modes
            let mode_str = format!("{:o}", entry.mode);
            buf.extend_from_slice(mode_str.as_bytes());
        }
        buf.push(b' ');
        buf.extend_from_slice(entry.name.as_bytes());
        buf.push(0);
        buf.extend_from_slice(entry.oid.raw());
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ObjectType;

    #[test]
    fn test_serialize_and_parse_tree() {
        let blob_oid = OID::from_hex("ce013625030ba8dba906f756967f9e9ca394464a").unwrap();
        let entries = vec![TreeEntry {
            mode: file_mode::BLOB,
            name: "hello.txt".into(),
            oid: blob_oid.clone(),
        }];

        let data = serialize_tree(&entries);
        let tree_oid = OID::hash_object(ObjectType::Tree, &data);
        let tree = parse_tree(tree_oid, &data).unwrap();

        assert_eq!(tree.entries.len(), 1);
        assert_eq!(tree.entries[0].name, "hello.txt");
        assert_eq!(tree.entries[0].mode, file_mode::BLOB);
        assert_eq!(tree.entries[0].oid, blob_oid);
    }

    #[test]
    fn test_tree_multiple_entries_sorted() {
        let oid1 = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let oid2 = OID::from_hex("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let oid3 = OID::from_hex("ccf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();

        // Insert out of order
        let entries = vec![
            TreeEntry { mode: file_mode::BLOB, name: "z.txt".into(), oid: oid1.clone() },
            TreeEntry { mode: file_mode::BLOB, name: "a.txt".into(), oid: oid2.clone() },
            TreeEntry { mode: file_mode::TREE, name: "lib".into(), oid: oid3.clone() },
        ];

        let data = serialize_tree(&entries);
        let tree_oid = OID::hash_object(ObjectType::Tree, &data);
        let tree = parse_tree(tree_oid, &data).unwrap();

        assert_eq!(tree.entries.len(), 3);
        assert_eq!(tree.entries[0].name, "a.txt");
        assert_eq!(tree.entries[1].name, "lib");
        assert!(tree.entries[1].is_tree());
        assert_eq!(tree.entries[2].name, "z.txt");
    }

    #[test]
    fn test_tree_entry_types() {
        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();

        let blob = TreeEntry { mode: file_mode::BLOB, name: "f".into(), oid: oid.clone() };
        assert!(blob.is_blob());
        assert!(!blob.is_tree());

        let exe = TreeEntry { mode: file_mode::BLOB_EXE, name: "f".into(), oid: oid.clone() };
        assert!(exe.is_blob());

        let tree = TreeEntry { mode: file_mode::TREE, name: "d".into(), oid: oid.clone() };
        assert!(tree.is_tree());
        assert!(!tree.is_blob());
    }

    #[test]
    fn test_parse_empty_tree() {
        let oid = OID::hash_object(ObjectType::Tree, &[]);
        let tree = parse_tree(oid, &[]).unwrap();
        assert!(tree.entries.is_empty());
    }

    #[test]
    fn test_tree_roundtrip_through_odb() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_tree_odb");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let blob_oid = OID::from_hex("ce013625030ba8dba906f756967f9e9ca394464a").unwrap();
        let entries = vec![TreeEntry {
            mode: file_mode::BLOB,
            name: "file.txt".into(),
            oid: blob_oid,
        }];

        let tree_data = serialize_tree(&entries);
        let oid = crate::odb::write_loose_object(repo.git_dir(), ObjectType::Tree, &tree_data).unwrap();

        let (read_type, read_data) = crate::odb::read_loose_object(repo.git_dir(), &oid).unwrap();
        assert_eq!(read_type, ObjectType::Tree);

        let tree = parse_tree(oid, &read_data).unwrap();
        assert_eq!(tree.entries.len(), 1);
        assert_eq!(tree.entries[0].name, "file.txt");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
