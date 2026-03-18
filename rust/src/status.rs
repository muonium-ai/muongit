//! Working directory status
//! Parity: libgit2 src/libgit2/status.c

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::index::{read_index, IndexEntry};
use crate::oid::OID;

/// Status of a file in the working directory
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// File is in the index but not in the working directory
    Deleted,
    /// File is in the working directory but not in the index
    New,
    /// File content or size has changed compared to the index
    Modified,
}

/// A single status entry
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub path: String,
    pub status: FileStatus,
}

/// Compute the working directory status by comparing the index against the workdir.
/// Returns a list of files that are new, modified, or deleted.
pub fn workdir_status(git_dir: &Path, workdir: &Path) -> Result<Vec<StatusEntry>, crate::error::MuonGitError> {
    let index = read_index(git_dir)?;
    let mut entries = Vec::new();

    // Track which paths are in the index
    let indexed_paths: BTreeSet<&str> = index.entries.iter().map(|e| e.path.as_str()).collect();

    // Check each index entry against the working directory
    for entry in &index.entries {
        let file_path = workdir.join(&entry.path);
        if !file_path.exists() {
            entries.push(StatusEntry {
                path: entry.path.clone(),
                status: FileStatus::Deleted,
            });
        } else if is_modified(&file_path, entry)? {
            entries.push(StatusEntry {
                path: entry.path.clone(),
                status: FileStatus::Modified,
            });
        }
    }

    // Find new (untracked) files in the working directory
    let mut new_files = Vec::new();
    collect_files(workdir, workdir, git_dir, &indexed_paths, &mut new_files)?;
    new_files.sort();
    for path in new_files {
        entries.push(StatusEntry {
            path,
            status: FileStatus::New,
        });
    }

    Ok(entries)
}

/// Check if a working directory file has been modified compared to its index entry.
/// Uses file size as a quick check. For a full implementation, you'd also hash the content.
fn is_modified(file_path: &Path, entry: &IndexEntry) -> Result<bool, crate::error::MuonGitError> {
    let metadata = fs::metadata(file_path)?;
    let file_size = metadata.len() as u32;

    // Quick check: if file size differs, it's definitely modified
    if file_size != entry.file_size {
        return Ok(true);
    }

    // Content hash check for same-size files
    let content = fs::read(file_path)?;
    let oid = OID::hash_object(crate::ObjectType::Blob, &content);
    Ok(oid != entry.oid)
}

/// Recursively collect untracked files in the working directory.
fn collect_files(
    dir: &Path,
    workdir: &Path,
    git_dir: &Path,
    indexed: &BTreeSet<&str>,
    result: &mut Vec<String>,
) -> Result<(), crate::error::MuonGitError> {
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
            collect_files(&path, workdir, git_dir, indexed, result)?;
        } else {
            let relative = path.strip_prefix(workdir)
                .map_err(|_| crate::error::MuonGitError::InvalidObject("path prefix error".into()))?;
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
    fn test_clean_workdir() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_status_clean");
        let _ = fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        // Create a file and matching index entry
        let content = b"hello\n";
        let file_path = repo.workdir().unwrap().join("hello.txt");
        fs::write(&file_path, content).unwrap();

        let oid = OID::hash_object(crate::ObjectType::Blob, content);
        let mut index = Index::new();
        index.add(make_index_entry("hello.txt", &oid, content.len() as u32));
        write_index(repo.git_dir(), &index).unwrap();

        let status = workdir_status(repo.git_dir(), repo.workdir().unwrap()).unwrap();
        assert!(status.is_empty());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_modified_file() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_status_modified");
        let _ = fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let content = b"hello\n";
        let file_path = repo.workdir().unwrap().join("hello.txt");
        fs::write(&file_path, content).unwrap();

        let oid = OID::hash_object(crate::ObjectType::Blob, content);
        let mut index = Index::new();
        index.add(make_index_entry("hello.txt", &oid, content.len() as u32));
        write_index(repo.git_dir(), &index).unwrap();

        // Modify the file
        fs::write(&file_path, b"changed\n").unwrap();

        let status = workdir_status(repo.git_dir(), repo.workdir().unwrap()).unwrap();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].path, "hello.txt");
        assert_eq!(status[0].status, FileStatus::Modified);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_deleted_file() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_status_deleted");
        let _ = fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let content = b"hello\n";
        let oid = OID::hash_object(crate::ObjectType::Blob, content);
        let mut index = Index::new();
        index.add(make_index_entry("hello.txt", &oid, content.len() as u32));
        write_index(repo.git_dir(), &index).unwrap();

        // Don't create the file — it's "deleted"
        let status = workdir_status(repo.git_dir(), repo.workdir().unwrap()).unwrap();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].path, "hello.txt");
        assert_eq!(status[0].status, FileStatus::Deleted);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_new_file() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_status_new");
        let _ = fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        // Empty index
        let index = Index::new();
        write_index(repo.git_dir(), &index).unwrap();

        // Create a file not in the index
        let file_path = repo.workdir().unwrap().join("new.txt");
        fs::write(&file_path, b"new\n").unwrap();

        let status = workdir_status(repo.git_dir(), repo.workdir().unwrap()).unwrap();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].path, "new.txt");
        assert_eq!(status[0].status, FileStatus::New);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_mixed_status() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_status_mixed");
        let _ = fs::remove_dir_all(&tmp);
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

        // a.txt: unchanged
        fs::write(repo.workdir().unwrap().join("a.txt"), content_a).unwrap();
        // b.txt: modified
        fs::write(repo.workdir().unwrap().join("b.txt"), b"modified\n").unwrap();
        // c.txt: deleted (not created)
        // d.txt: new (untracked)
        fs::write(repo.workdir().unwrap().join("d.txt"), b"new\n").unwrap();

        let status = workdir_status(repo.git_dir(), repo.workdir().unwrap()).unwrap();

        let modified: Vec<_> = status.iter().filter(|s| s.status == FileStatus::Modified).collect();
        let deleted: Vec<_> = status.iter().filter(|s| s.status == FileStatus::Deleted).collect();
        let new: Vec<_> = status.iter().filter(|s| s.status == FileStatus::New).collect();

        assert_eq!(modified.len(), 1);
        assert_eq!(modified[0].path, "b.txt");
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].path, "c.txt");
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].path, "d.txt");

        let _ = fs::remove_dir_all(&tmp);
    }
}
