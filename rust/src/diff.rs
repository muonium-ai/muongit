//! Tree-to-tree and index-to-workdir diff
//! Parity: libgit2 src/libgit2/diff.c

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::error::MuonGitError;
use crate::index::{read_index, IndexEntry};
use crate::oid::OID;
use crate::tree::{file_mode, TreeEntry};

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

/// Compute the diff between the index (staging area) and the working directory.
/// Returns deltas for modified, deleted, and new (untracked) files.
pub fn diff_index_to_workdir(git_dir: &Path, workdir: &Path) -> Result<Vec<DiffDelta>, MuonGitError> {
    let index = read_index(git_dir)?;
    let mut deltas = Vec::new();

    let indexed_paths: BTreeSet<&str> = index.entries.iter().map(|e| e.path.as_str()).collect();

    // Check each index entry against the working directory
    for entry in &index.entries {
        let file_path = workdir.join(&entry.path);
        if !file_path.exists() {
            deltas.push(DiffDelta {
                status: DiffStatus::Deleted,
                old_entry: Some(index_entry_to_tree_entry(entry)),
                new_entry: None,
                path: entry.path.clone(),
            });
        } else {
            let metadata = fs::metadata(&file_path)?;
            let file_size = metadata.len() as u32;

            // Quick size check, then content hash
            let modified = if file_size != entry.file_size {
                true
            } else {
                let content = fs::read(&file_path)?;
                let oid = OID::hash_object(crate::ObjectType::Blob, &content);
                oid != entry.oid
            };

            if modified {
                let content = fs::read(&file_path)?;
                let workdir_oid = OID::hash_object(crate::ObjectType::Blob, &content);
                let workdir_mode = if is_executable(&file_path) {
                    file_mode::BLOB_EXE
                } else {
                    file_mode::BLOB
                };
                deltas.push(DiffDelta {
                    status: DiffStatus::Modified,
                    old_entry: Some(index_entry_to_tree_entry(entry)),
                    new_entry: Some(TreeEntry {
                        mode: workdir_mode,
                        name: entry.path.clone(),
                        oid: workdir_oid,
                    }),
                    path: entry.path.clone(),
                });
            }
        }
    }

    // Find new (untracked) files
    let mut new_files = Vec::new();
    collect_workdir_files(workdir, workdir, git_dir, &indexed_paths, &mut new_files)?;
    new_files.sort();

    for rel_path in new_files {
        let file_path = workdir.join(&rel_path);
        let content = fs::read(&file_path)?;
        let oid = OID::hash_object(crate::ObjectType::Blob, &content);
        let mode = if is_executable(&file_path) {
            file_mode::BLOB_EXE
        } else {
            file_mode::BLOB
        };
        deltas.push(DiffDelta {
            status: DiffStatus::Added,
            old_entry: None,
            new_entry: Some(TreeEntry {
                mode,
                name: rel_path.clone(),
                oid,
            }),
            path: rel_path,
        });
    }

    Ok(deltas)
}

/// Convert an IndexEntry to a TreeEntry for diff results.
fn index_entry_to_tree_entry(entry: &IndexEntry) -> TreeEntry {
    TreeEntry {
        mode: entry.mode,
        name: entry.path.clone(),
        oid: entry.oid.clone(),
    }
}

/// Check if a file is executable.
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

/// Recursively collect untracked files in the working directory.
fn collect_workdir_files(
    dir: &Path,
    workdir: &Path,
    git_dir: &Path,
    indexed: &BTreeSet<&str>,
    result: &mut Vec<String>,
) -> Result<(), MuonGitError> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        // Skip .git directory
        if path == git_dir || path.file_name().map(|n| n == ".git").unwrap_or(false) {
            continue;
        }

        if path.is_dir() {
            collect_workdir_files(&path, workdir, git_dir, indexed, result)?;
        } else {
            let relative = path.strip_prefix(workdir)
                .map_err(|_| MuonGitError::InvalidObject("path prefix error".into()))?;
            let rel_str = relative.to_string_lossy().to_string();
            if !indexed.contains(rel_str.as_str()) {
                result.push(rel_str);
            }
        }
    }

    Ok(())
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

    // --- Index-to-workdir diff tests ---

    use crate::index::{Index, IndexEntry, write_index};

    fn make_index_entry(path: &str, oid: &OID, file_size: u32) -> IndexEntry {
        IndexEntry {
            ctime_secs: 0, ctime_nanos: 0,
            mtime_secs: 0, mtime_nanos: 0,
            dev: 0, ino: 0,
            mode: 0o100644, uid: 0, gid: 0,
            file_size,
            oid: oid.clone(),
            flags: 0,
            path: path.to_string(),
        }
    }

    #[test]
    fn test_diff_workdir_clean() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_diff_workdir_clean");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let content = b"hello\n";
        let file_path = repo.workdir().unwrap().join("hello.txt");
        std::fs::write(&file_path, content).unwrap();

        let oid = OID::hash_object(crate::ObjectType::Blob, content);
        let mut index = Index::new();
        index.add(make_index_entry("hello.txt", &oid, content.len() as u32));
        write_index(repo.git_dir(), &index).unwrap();

        let deltas = diff_index_to_workdir(repo.git_dir(), repo.workdir().unwrap()).unwrap();
        assert!(deltas.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_diff_workdir_modified() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_diff_workdir_mod");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let content = b"hello\n";
        let file_path = repo.workdir().unwrap().join("hello.txt");
        std::fs::write(&file_path, content).unwrap();

        let oid = OID::hash_object(crate::ObjectType::Blob, content);
        let mut index = Index::new();
        index.add(make_index_entry("hello.txt", &oid, content.len() as u32));
        write_index(repo.git_dir(), &index).unwrap();

        // Modify the file
        std::fs::write(&file_path, b"changed\n").unwrap();

        let deltas = diff_index_to_workdir(repo.git_dir(), repo.workdir().unwrap()).unwrap();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].status, DiffStatus::Modified);
        assert_eq!(deltas[0].path, "hello.txt");
        assert!(deltas[0].old_entry.is_some());
        assert!(deltas[0].new_entry.is_some());
        // New entry OID should differ from old
        assert_ne!(deltas[0].old_entry.as_ref().unwrap().oid, deltas[0].new_entry.as_ref().unwrap().oid);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_diff_workdir_deleted() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_diff_workdir_del");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let content = b"hello\n";
        let oid = OID::hash_object(crate::ObjectType::Blob, content);
        let mut index = Index::new();
        index.add(make_index_entry("hello.txt", &oid, content.len() as u32));
        write_index(repo.git_dir(), &index).unwrap();

        // Don't create the file — it's deleted
        let deltas = diff_index_to_workdir(repo.git_dir(), repo.workdir().unwrap()).unwrap();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].status, DiffStatus::Deleted);
        assert_eq!(deltas[0].path, "hello.txt");
        assert!(deltas[0].old_entry.is_some());
        assert!(deltas[0].new_entry.is_none());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_diff_workdir_new_file() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_diff_workdir_new");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        // Empty index
        let index = Index::new();
        write_index(repo.git_dir(), &index).unwrap();

        // Create a file not in the index
        std::fs::write(repo.workdir().unwrap().join("new.txt"), b"new\n").unwrap();

        let deltas = diff_index_to_workdir(repo.git_dir(), repo.workdir().unwrap()).unwrap();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].status, DiffStatus::Added);
        assert_eq!(deltas[0].path, "new.txt");
        assert!(deltas[0].old_entry.is_none());
        assert!(deltas[0].new_entry.is_some());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_diff_workdir_mixed() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_diff_workdir_mixed");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let content_a = b"aaa\n";
        let content_b = b"bbb\n";
        let oid_a = OID::hash_object(crate::ObjectType::Blob, content_a);
        let oid_b = OID::hash_object(crate::ObjectType::Blob, content_b);

        let mut index = Index::new();
        index.add(make_index_entry("a.txt", &oid_a, content_a.len() as u32));
        index.add(make_index_entry("b.txt", &oid_b, content_b.len() as u32));
        index.add(make_index_entry("c.txt", &oid_a, content_a.len() as u32));
        write_index(repo.git_dir(), &index).unwrap();

        let wd = repo.workdir().unwrap();
        // a.txt: unchanged
        std::fs::write(wd.join("a.txt"), content_a).unwrap();
        // b.txt: modified
        std::fs::write(wd.join("b.txt"), b"modified\n").unwrap();
        // c.txt: deleted (not created)
        // d.txt: new
        std::fs::write(wd.join("d.txt"), b"new\n").unwrap();

        let deltas = diff_index_to_workdir(repo.git_dir(), wd).unwrap();

        let modified: Vec<_> = deltas.iter().filter(|d| d.status == DiffStatus::Modified).collect();
        let deleted: Vec<_> = deltas.iter().filter(|d| d.status == DiffStatus::Deleted).collect();
        let added: Vec<_> = deltas.iter().filter(|d| d.status == DiffStatus::Added).collect();

        assert_eq!(modified.len(), 1);
        assert_eq!(modified[0].path, "b.txt");
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].path, "c.txt");
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].path, "d.txt");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
