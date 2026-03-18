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

/// Parse a tree object from its raw binary data
pub fn parse_tree(oid: OID, data: &[u8]) -> Result<Tree, MuonGitError> {
    let mut entries = Vec::new();
    let mut i = 0;

    while i < data.len() {
        // Parse mode (octal digits until space)
        let mode_start = i;
        while i < data.len() && data[i] != b' ' {
            i += 1;
        }
        if i >= data.len() {
            return Err(MuonGitError::InvalidObject(
                "tree entry: missing space after mode".into(),
            ));
        }
        let mode_str = std::str::from_utf8(&data[mode_start..i])
            .map_err(|_| MuonGitError::InvalidObject("tree entry: invalid mode".into()))?;
        let mode = u32::from_str_radix(mode_str, 8)
            .map_err(|_| MuonGitError::InvalidObject(format!("tree entry: invalid mode '{}'", mode_str)))?;
        i += 1; // skip space

        // Parse name (until null byte)
        let name_start = i;
        while i < data.len() && data[i] != 0 {
            i += 1;
        }
        if i >= data.len() {
            return Err(MuonGitError::InvalidObject(
                "tree entry: missing null after name".into(),
            ));
        }
        let name = std::str::from_utf8(&data[name_start..i])
            .map_err(|_| MuonGitError::InvalidObject("tree entry: invalid name".into()))?
            .to_string();
        i += 1; // skip null

        // Read 20-byte raw OID
        if i + 20 > data.len() {
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

/// Serialize tree entries to raw binary data (without the object header).
/// Entries are sorted by name with tree-sorting rules.
pub fn serialize_tree(entries: &[TreeEntry]) -> Vec<u8> {
    let mut sorted: Vec<&TreeEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| {
        let a_key = if a.is_tree() {
            format!("{}/", a.name)
        } else {
            a.name.clone()
        };
        let b_key = if b.is_tree() {
            format!("{}/", b.name)
        } else {
            b.name.clone()
        };
        a_key.cmp(&b_key)
    });

    let mut buf = Vec::new();
    for entry in sorted {
        // Mode as octal string
        let mode_str = format!("{:o}", entry.mode);
        buf.extend_from_slice(mode_str.as_bytes());
        buf.push(b' ');
        // Name
        buf.extend_from_slice(entry.name.as_bytes());
        buf.push(0);
        // Raw 20-byte OID
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
