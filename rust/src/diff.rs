//! Tree-to-tree diff
//! Parity: libgit2 src/libgit2/diff.c

use crate::tree::TreeEntry;

/// The kind of change for a diff entry
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffStatus {
    Added,
    Deleted,
    Modified,
}

/// A single diff delta between two trees
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffDelta {
    pub status: DiffStatus,
    pub old_entry: Option<TreeEntry>,
    pub new_entry: Option<TreeEntry>,
    pub path: String,
}

/// Compute the diff between two trees.
/// Both entry lists should be sorted by name (as git trees are).
pub fn diff_trees(old_entries: &[TreeEntry], new_entries: &[TreeEntry]) -> Vec<DiffDelta> {
    let mut deltas = Vec::new();
    let mut oi = 0;
    let mut ni = 0;

    while oi < old_entries.len() && ni < new_entries.len() {
        let old = &old_entries[oi];
        let new = &new_entries[ni];

        match old.name.cmp(&new.name) {
            std::cmp::Ordering::Less => {
                // Entry only in old tree — deleted
                deltas.push(DiffDelta {
                    status: DiffStatus::Deleted,
                    old_entry: Some(old.clone()),
                    new_entry: None,
                    path: old.name.clone(),
                });
                oi += 1;
            }
            std::cmp::Ordering::Greater => {
                // Entry only in new tree — added
                deltas.push(DiffDelta {
                    status: DiffStatus::Added,
                    old_entry: None,
                    new_entry: Some(new.clone()),
                    path: new.name.clone(),
                });
                ni += 1;
            }
            std::cmp::Ordering::Equal => {
                // Same name — check if modified
                if old.oid != new.oid || old.mode != new.mode {
                    deltas.push(DiffDelta {
                        status: DiffStatus::Modified,
                        old_entry: Some(old.clone()),
                        new_entry: Some(new.clone()),
                        path: old.name.clone(),
                    });
                }
                oi += 1;
                ni += 1;
            }
        }
    }

    // Remaining old entries are deletions
    while oi < old_entries.len() {
        let old = &old_entries[oi];
        deltas.push(DiffDelta {
            status: DiffStatus::Deleted,
            old_entry: Some(old.clone()),
            new_entry: None,
            path: old.name.clone(),
        });
        oi += 1;
    }

    // Remaining new entries are additions
    while ni < new_entries.len() {
        let new = &new_entries[ni];
        deltas.push(DiffDelta {
            status: DiffStatus::Added,
            old_entry: None,
            new_entry: Some(new.clone()),
            path: new.name.clone(),
        });
        ni += 1;
    }

    deltas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oid::OID;
    use crate::tree::file_mode;

    fn entry(name: &str, oid_hex: &str, mode: u32) -> TreeEntry {
        TreeEntry {
            mode,
            name: name.to_string(),
            oid: OID::from_hex(oid_hex).unwrap(),
        }
    }

    #[test]
    fn test_diff_identical_trees() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let entries = vec![
            entry("a.txt", oid, file_mode::BLOB),
            entry("b.txt", oid, file_mode::BLOB),
        ];
        let deltas = diff_trees(&entries, &entries);
        assert!(deltas.is_empty());
    }

    #[test]
    fn test_diff_added_file() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let old = vec![entry("a.txt", oid, file_mode::BLOB)];
        let new = vec![
            entry("a.txt", oid, file_mode::BLOB),
            entry("b.txt", oid, file_mode::BLOB),
        ];
        let deltas = diff_trees(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].status, DiffStatus::Added);
        assert_eq!(deltas[0].path, "b.txt");
        assert!(deltas[0].old_entry.is_none());
        assert!(deltas[0].new_entry.is_some());
    }

    #[test]
    fn test_diff_deleted_file() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let old = vec![
            entry("a.txt", oid, file_mode::BLOB),
            entry("b.txt", oid, file_mode::BLOB),
        ];
        let new = vec![entry("a.txt", oid, file_mode::BLOB)];
        let deltas = diff_trees(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].status, DiffStatus::Deleted);
        assert_eq!(deltas[0].path, "b.txt");
        assert!(deltas[0].old_entry.is_some());
        assert!(deltas[0].new_entry.is_none());
    }

    #[test]
    fn test_diff_modified_file() {
        let oid1 = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let oid2 = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let old = vec![entry("a.txt", oid1, file_mode::BLOB)];
        let new = vec![entry("a.txt", oid2, file_mode::BLOB)];
        let deltas = diff_trees(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].status, DiffStatus::Modified);
        assert_eq!(deltas[0].path, "a.txt");
        assert!(deltas[0].old_entry.is_some());
        assert!(deltas[0].new_entry.is_some());
    }

    #[test]
    fn test_diff_mode_change() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let old = vec![entry("script.sh", oid, file_mode::BLOB)];
        let new = vec![entry("script.sh", oid, file_mode::BLOB_EXE)];
        let deltas = diff_trees(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].status, DiffStatus::Modified);
    }

    #[test]
    fn test_diff_empty_to_full() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let new = vec![
            entry("a.txt", oid, file_mode::BLOB),
            entry("b.txt", oid, file_mode::BLOB),
        ];
        let deltas = diff_trees(&[], &new);
        assert_eq!(deltas.len(), 2);
        assert!(deltas.iter().all(|d| d.status == DiffStatus::Added));
    }

    #[test]
    fn test_diff_full_to_empty() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let old = vec![
            entry("a.txt", oid, file_mode::BLOB),
            entry("b.txt", oid, file_mode::BLOB),
        ];
        let deltas = diff_trees(&old, &[]);
        assert_eq!(deltas.len(), 2);
        assert!(deltas.iter().all(|d| d.status == DiffStatus::Deleted));
    }

    #[test]
    fn test_diff_mixed_changes() {
        let oid1 = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let oid2 = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let old = vec![
            entry("a.txt", oid1, file_mode::BLOB),
            entry("b.txt", oid1, file_mode::BLOB),
            entry("c.txt", oid1, file_mode::BLOB),
        ];
        let new = vec![
            entry("a.txt", oid1, file_mode::BLOB), // unchanged
            entry("b.txt", oid2, file_mode::BLOB), // modified
            entry("d.txt", oid1, file_mode::BLOB), // added
        ];
        let deltas = diff_trees(&old, &new);
        assert_eq!(deltas.len(), 3);
        assert_eq!(deltas[0].status, DiffStatus::Modified);
        assert_eq!(deltas[0].path, "b.txt");
        assert_eq!(deltas[1].status, DiffStatus::Deleted);
        assert_eq!(deltas[1].path, "c.txt");
        assert_eq!(deltas[2].status, DiffStatus::Added);
        assert_eq!(deltas[2].path, "d.txt");
    }
}
