//! Checkout - materialize index entries into the working directory
//! Parity: libgit2 src/libgit2/checkout.c

use std::fs;
use std::path::Path;

use crate::blob::read_blob;
use crate::error::MuonGitError;
use crate::index::{read_index, IndexEntry};

/// Options for checkout behavior.
#[derive(Debug, Clone, Default)]
pub struct CheckoutOptions {
    /// If true, overwrite existing files in the workdir.
    pub force: bool,
}

/// Result of a checkout operation.
#[derive(Debug, Clone, Default)]
pub struct CheckoutResult {
    /// Files written to the workdir.
    pub updated: Vec<String>,
    /// Files skipped because they already exist (when force is false).
    pub conflicts: Vec<String>,
}

/// Checkout the index to the working directory.
///
/// Reads the index, then for each entry reads the blob from the ODB
/// and writes it to the workdir at the entry's path.
pub fn checkout_index(
    git_dir: &Path,
    workdir: &Path,
    opts: &CheckoutOptions,
) -> Result<CheckoutResult, MuonGitError> {
    let index = read_index(git_dir)?;
    let mut result = CheckoutResult::default();

    for entry in &index.entries {
        checkout_entry(git_dir, workdir, entry, opts, &mut result)?;
    }

    Ok(result)
}

/// Checkout a single index entry to the working directory.
fn checkout_entry(
    git_dir: &Path,
    workdir: &Path,
    entry: &IndexEntry,
    opts: &CheckoutOptions,
    result: &mut CheckoutResult,
) -> Result<(), MuonGitError> {
    let target_path = workdir.join(&entry.path);

    // Check for existing file when not forcing
    if !opts.force && target_path.exists() {
        result.conflicts.push(entry.path.clone());
        return Ok(());
    }

    // Create parent directories
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Read blob content
    let blob = read_blob(git_dir, &entry.oid)?;

    // Write file
    fs::write(&target_path, &blob.data)?;

    // Set file permissions based on mode
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = entry.mode & 0o777;
        // Git stores executable as 0o100755, regular as 0o100644
        // Extract just the permission bits
        let perms = if mode & 0o111 != 0 { 0o755 } else { 0o644 };
        fs::set_permissions(&target_path, fs::Permissions::from_mode(perms))?;
    }

    result.updated.push(entry.path.clone());
    Ok(())
}

/// Checkout specific paths from the index to the working directory.
pub fn checkout_paths(
    git_dir: &Path,
    workdir: &Path,
    paths: &[&str],
    opts: &CheckoutOptions,
) -> Result<CheckoutResult, MuonGitError> {
    let index = read_index(git_dir)?;
    let mut result = CheckoutResult::default();

    for path in paths {
        if let Some(entry) = index.find(path) {
            let entry = entry.clone();
            checkout_entry(git_dir, workdir, &entry, opts, &mut result)?;
        } else {
            return Err(MuonGitError::NotFound(format!(
                "path '{}' not in index",
                path
            )));
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{write_index, Index, IndexEntry};
    use crate::odb::write_loose_object;
    use crate::repository::Repository;
    use crate::types::ObjectType;

    fn setup_repo(name: &str) -> (std::path::PathBuf, Repository) {
        let tmp =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../tmp/{}", name));
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        (tmp, repo)
    }

    fn add_blob_to_index(
        git_dir: &Path,
        index: &mut Index,
        path: &str,
        content: &[u8],
        executable: bool,
    ) {
        let oid = write_loose_object(git_dir, ObjectType::Blob, content).unwrap();
        let mode = if executable { 0o100755 } else { 0o100644 };
        index.add(IndexEntry {
            ctime_secs: 0,
            ctime_nanos: 0,
            mtime_secs: 0,
            mtime_nanos: 0,
            dev: 0,
            ino: 0,
            mode,
            uid: 0,
            gid: 0,
            file_size: content.len() as u32,
            oid,
            flags: path.len().min(0xFFF) as u16,
            path: path.to_string(),
        });
    }

    #[test]
    fn test_checkout_basic() {
        let (tmp, repo) = setup_repo("test_checkout_basic");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let mut index = Index::new();
        add_blob_to_index(git_dir, &mut index, "hello.txt", b"Hello, world!\n", false);
        add_blob_to_index(git_dir, &mut index, "src/main.rs", b"fn main() {}\n", false);
        write_index(git_dir, &index).unwrap();

        let opts = CheckoutOptions { force: true };
        let result = checkout_index(git_dir, workdir, &opts).unwrap();

        assert_eq!(result.updated.len(), 2);
        assert!(result.conflicts.is_empty());
        assert_eq!(
            fs::read_to_string(workdir.join("hello.txt")).unwrap(),
            "Hello, world!\n"
        );
        assert_eq!(
            fs::read_to_string(workdir.join("src/main.rs")).unwrap(),
            "fn main() {}\n"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_checkout_creates_directories() {
        let (tmp, repo) = setup_repo("test_checkout_dirs");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let mut index = Index::new();
        add_blob_to_index(
            git_dir,
            &mut index,
            "a/b/c/deep.txt",
            b"deep content",
            false,
        );
        write_index(git_dir, &index).unwrap();

        let opts = CheckoutOptions { force: true };
        let result = checkout_index(git_dir, workdir, &opts).unwrap();

        assert_eq!(result.updated.len(), 1);
        assert!(workdir.join("a/b/c/deep.txt").exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_checkout_conflict_detection() {
        let (tmp, repo) = setup_repo("test_checkout_conflict");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        // Create existing file
        fs::write(workdir.join("existing.txt"), "local changes").unwrap();

        let mut index = Index::new();
        add_blob_to_index(git_dir, &mut index, "existing.txt", b"index content", false);
        write_index(git_dir, &index).unwrap();

        // Without force: should detect conflict
        let opts = CheckoutOptions { force: false };
        let result = checkout_index(git_dir, workdir, &opts).unwrap();

        assert!(result.updated.is_empty());
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(result.conflicts[0], "existing.txt");
        // Original content preserved
        assert_eq!(
            fs::read_to_string(workdir.join("existing.txt")).unwrap(),
            "local changes"
        );

        // With force: should overwrite
        let opts = CheckoutOptions { force: true };
        let result = checkout_index(git_dir, workdir, &opts).unwrap();

        assert_eq!(result.updated.len(), 1);
        assert_eq!(
            fs::read_to_string(workdir.join("existing.txt")).unwrap(),
            "index content"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_checkout_executable_mode() {
        let (tmp, repo) = setup_repo("test_checkout_exec");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let mut index = Index::new();
        add_blob_to_index(git_dir, &mut index, "script.sh", b"#!/bin/sh\necho hi\n", true);
        write_index(git_dir, &index).unwrap();

        let opts = CheckoutOptions { force: true };
        checkout_index(git_dir, workdir, &opts).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::metadata(workdir.join("script.sh")).unwrap().permissions();
            assert!(perms.mode() & 0o111 != 0, "file should be executable");
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_checkout_paths() {
        let (tmp, repo) = setup_repo("test_checkout_paths");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let mut index = Index::new();
        add_blob_to_index(git_dir, &mut index, "a.txt", b"aaa", false);
        add_blob_to_index(git_dir, &mut index, "b.txt", b"bbb", false);
        add_blob_to_index(git_dir, &mut index, "c.txt", b"ccc", false);
        write_index(git_dir, &index).unwrap();

        let opts = CheckoutOptions { force: true };
        let result = checkout_paths(git_dir, workdir, &["a.txt", "c.txt"], &opts).unwrap();

        assert_eq!(result.updated.len(), 2);
        assert!(workdir.join("a.txt").exists());
        assert!(!workdir.join("b.txt").exists());
        assert!(workdir.join("c.txt").exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_checkout_path_not_in_index() {
        let (tmp, repo) = setup_repo("test_checkout_notfound");
        let git_dir = repo.git_dir();
        let workdir = repo.workdir().unwrap();

        let index = Index::new();
        write_index(git_dir, &index).unwrap();

        let opts = CheckoutOptions { force: true };
        let result = checkout_paths(git_dir, workdir, &["nonexistent.txt"], &opts);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
