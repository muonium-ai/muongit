//! Git worktree support — multiple working trees for a single repository.
//! Parity: libgit2 src/libgit2/worktree.c

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::MuonGitError;
use crate::refs::{resolve_reference, write_reference};

/// A linked worktree entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// Name of the worktree (basename under .git/worktrees/).
    pub name: String,
    /// Filesystem path to the worktree working directory.
    pub path: PathBuf,
    /// Path to the worktree's gitdir inside the parent's .git/worktrees/<name>/.
    pub gitdir_path: PathBuf,
    /// Whether this worktree is locked.
    pub locked: bool,
}

/// Options for creating a new worktree.
#[derive(Debug, Clone, Default)]
pub struct WorktreeAddOptions {
    /// Lock the newly created worktree immediately.
    pub lock: bool,
    /// Branch reference (e.g. "refs/heads/feature"). If None, creates a new
    /// branch named after the worktree pointing at HEAD.
    pub reference: Option<String>,
}

/// Options controlling worktree prune behavior.
#[derive(Debug, Clone, Default)]
pub struct WorktreePruneOptions {
    /// Prune even if the worktree is valid (on-disk data exists).
    pub valid: bool,
    /// Prune even if the worktree is locked.
    pub locked: bool,
    /// Also remove the working tree directory.
    pub working_tree: bool,
}

/// List names of linked worktrees for a repository.
pub fn worktree_list(git_dir: &Path) -> Result<Vec<String>, MuonGitError> {
    let worktrees_dir = git_dir.join("worktrees");
    if !worktrees_dir.is_dir() {
        return Ok(vec![]);
    }

    let mut names = Vec::new();
    for entry in fs::read_dir(&worktrees_dir).map_err(|e| {
        MuonGitError::NotFound(format!("cannot read worktrees dir: {}", e))
    })? {
        let entry = entry.map_err(|e| {
            MuonGitError::NotFound(format!("cannot read dir entry: {}", e))
        })?;
        let path = entry.path();
        if path.is_dir() && is_worktree_dir(&path) {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Look up a linked worktree by name.
pub fn worktree_lookup(git_dir: &Path, name: &str) -> Result<Worktree, MuonGitError> {
    let wt_dir = git_dir.join("worktrees").join(name);
    if !wt_dir.is_dir() {
        return Err(MuonGitError::NotFound(format!(
            "worktree '{}' not found",
            name
        )));
    }
    if !is_worktree_dir(&wt_dir) {
        return Err(MuonGitError::Invalid(format!(
            "worktree '{}' has invalid structure",
            name
        )));
    }
    open_worktree(git_dir, name)
}

/// Validate that a worktree's on-disk structure is intact.
pub fn worktree_validate(worktree: &Worktree) -> Result<(), MuonGitError> {
    if !worktree.gitdir_path.is_dir() {
        return Err(MuonGitError::NotFound(format!(
            "worktree gitdir missing: {}",
            worktree.gitdir_path.display()
        )));
    }
    if !is_worktree_dir(&worktree.gitdir_path) {
        return Err(MuonGitError::Invalid(format!(
            "worktree '{}' has invalid gitdir structure",
            worktree.name
        )));
    }
    if !worktree.path.is_dir() {
        return Err(MuonGitError::NotFound(format!(
            "worktree working directory missing: {}",
            worktree.path.display()
        )));
    }
    Ok(())
}

/// Add a new linked worktree.
pub fn worktree_add(
    git_dir: &Path,
    name: &str,
    worktree_path: &Path,
    options: Option<&WorktreeAddOptions>,
) -> Result<Worktree, MuonGitError> {
    let opts = options.cloned().unwrap_or_default();
    let wt_meta = git_dir.join("worktrees").join(name);
    if wt_meta.exists() {
        return Err(MuonGitError::Conflict(format!(
            "worktree '{}' already exists",
            name
        )));
    }

    // Determine the branch ref
    let branch_ref = if let Some(ref r) = opts.reference {
        r.clone()
    } else {
        let head_oid = resolve_reference(git_dir, "HEAD")?;
        let new_branch = format!("refs/heads/{}", name);
        write_reference(git_dir, &new_branch, &head_oid)?;
        new_branch
    };

    // Create metadata dir and worktree dir
    fs::create_dir_all(&wt_meta).map_err(|e| {
        MuonGitError::NotFound(format!("cannot create worktree metadata: {}", e))
    })?;
    fs::create_dir_all(worktree_path).map_err(|e| {
        MuonGitError::NotFound(format!("cannot create worktree dir: {}", e))
    })?;

    let abs_worktree = resolve_real_path(worktree_path)?;

    // Write gitdir file (points to worktree's .git file)
    let gitfile_in_wt = abs_worktree.join(".git");
    fs::write(
        wt_meta.join("gitdir"),
        format!("{}\n", gitfile_in_wt.display()),
    )
    .map_err(|e| MuonGitError::NotFound(format!("cannot write gitdir: {}", e)))?;

    // Write commondir file
    fs::write(wt_meta.join("commondir"), "../..\n")
        .map_err(|e| MuonGitError::NotFound(format!("cannot write commondir: {}", e)))?;

    // Write HEAD as symbolic ref
    fs::write(wt_meta.join("HEAD"), format!("ref: {}\n", branch_ref))
        .map_err(|e| MuonGitError::NotFound(format!("cannot write HEAD: {}", e)))?;

    // Create .git file in worktree (gitlink pointing back to metadata)
    let abs_wt_meta = resolve_real_path(&wt_meta)?;
    fs::write(&gitfile_in_wt, format!("gitdir: {}\n", abs_wt_meta.display()))
        .map_err(|e| MuonGitError::NotFound(format!("cannot write gitlink: {}", e)))?;

    // Lock if requested
    if opts.lock {
        fs::write(wt_meta.join("locked"), "")
            .map_err(|e| MuonGitError::NotFound(format!("cannot write lock: {}", e)))?;
    }

    Ok(Worktree {
        name: name.to_string(),
        path: abs_worktree,
        gitdir_path: abs_wt_meta,
        locked: opts.lock,
    })
}

/// Lock a worktree with an optional reason.
pub fn worktree_lock(
    git_dir: &Path,
    name: &str,
    reason: Option<&str>,
) -> Result<(), MuonGitError> {
    let wt_meta = git_dir.join("worktrees").join(name);
    if !wt_meta.exists() {
        return Err(MuonGitError::NotFound(format!(
            "worktree '{}' not found",
            name
        )));
    }
    let lock_path = wt_meta.join("locked");
    if lock_path.exists() {
        return Err(MuonGitError::Locked(format!(
            "worktree '{}' is already locked",
            name
        )));
    }
    fs::write(&lock_path, reason.unwrap_or(""))
        .map_err(|e| MuonGitError::NotFound(format!("cannot write lock: {}", e)))?;
    Ok(())
}

/// Unlock a worktree. Returns true if was locked, false if was not.
pub fn worktree_unlock(git_dir: &Path, name: &str) -> Result<bool, MuonGitError> {
    let lock_path = git_dir.join("worktrees").join(name).join("locked");
    if lock_path.exists() {
        fs::remove_file(&lock_path)
            .map_err(|e| MuonGitError::NotFound(format!("cannot remove lock: {}", e)))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Check whether a worktree is locked. Returns the lock reason if locked, None otherwise.
pub fn worktree_is_locked(git_dir: &Path, name: &str) -> Result<Option<String>, MuonGitError> {
    let lock_path = git_dir.join("worktrees").join(name).join("locked");
    if !lock_path.exists() {
        return Ok(None);
    }
    let reason = fs::read_to_string(&lock_path)
        .map_err(|e| MuonGitError::NotFound(format!("cannot read lock: {}", e)))?
        .trim()
        .to_string();
    Ok(Some(reason))
}

/// Check if a worktree can be pruned with the given options.
pub fn worktree_is_prunable(
    git_dir: &Path,
    name: &str,
    options: Option<&WorktreePruneOptions>,
) -> Result<bool, MuonGitError> {
    let opts = options.cloned().unwrap_or_default();
    let wt = worktree_lookup(git_dir, name)?;
    if wt.locked && !opts.locked {
        return Ok(false);
    }
    if wt.path.is_dir() && !opts.valid {
        return Ok(false);
    }
    Ok(true)
}

/// Prune (remove) a worktree's metadata. Optionally removes the working directory.
pub fn worktree_prune(
    git_dir: &Path,
    name: &str,
    options: Option<&WorktreePruneOptions>,
) -> Result<(), MuonGitError> {
    let opts = options.cloned().unwrap_or_default();
    let wt = worktree_lookup(git_dir, name)?;

    if wt.locked && !opts.locked {
        return Err(MuonGitError::Locked(format!(
            "worktree '{}' is locked",
            name
        )));
    }
    if wt.path.is_dir() && !opts.valid {
        return Err(MuonGitError::Conflict(format!(
            "worktree '{}' is still valid; use valid flag to override",
            name
        )));
    }

    // Remove working tree directory if requested
    if opts.working_tree && wt.path.exists() {
        fs::remove_dir_all(&wt.path).map_err(|e| {
            MuonGitError::NotFound(format!("cannot remove worktree dir: {}", e))
        })?;
    }

    // Remove metadata directory
    let wt_meta = git_dir.join("worktrees").join(name);
    if wt_meta.exists() {
        fs::remove_dir_all(&wt_meta).map_err(|e| {
            MuonGitError::NotFound(format!("cannot remove worktree metadata: {}", e))
        })?;
    }

    // Clean up worktrees dir if empty
    let worktrees_dir = git_dir.join("worktrees");
    if let Ok(mut entries) = fs::read_dir(&worktrees_dir) {
        if entries.next().is_none() {
            let _ = fs::remove_dir(&worktrees_dir);
        }
    }

    Ok(())
}

// --- Internal helpers ---

fn is_worktree_dir(path: &Path) -> bool {
    path.join("gitdir").exists()
        && path.join("commondir").exists()
        && path.join("HEAD").exists()
}

fn open_worktree(git_dir: &Path, name: &str) -> Result<Worktree, MuonGitError> {
    let wt_dir = git_dir.join("worktrees").join(name);
    let gitdir_content = fs::read_to_string(wt_dir.join("gitdir"))
        .map_err(|e| MuonGitError::NotFound(format!("cannot read gitdir: {}", e)))?
        .trim()
        .to_string();

    // The worktree path is the parent of the .git file referenced in gitdir
    let worktree_path = PathBuf::from(&gitdir_content)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();

    let lock_path = wt_dir.join("locked");
    let locked = lock_path.exists();

    Ok(Worktree {
        name: name.to_string(),
        path: worktree_path,
        gitdir_path: wt_dir,
        locked,
    })
}

fn resolve_real_path(path: &Path) -> Result<PathBuf, MuonGitError> {
    fs::canonicalize(path)
        .map_err(|e| MuonGitError::NotFound(format!("cannot resolve path: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::serialize_commit;
    use crate::odb::write_loose_object;
    use crate::oid::OID;
    use crate::refs::write_symbolic_reference;
    use crate::repository::Repository;
    use crate::tree::serialize_tree;
    use crate::types::Signature;

    fn test_dir(name: &str) -> PathBuf {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp");
        fs::create_dir_all(&base).unwrap();
        let p = base.join(format!("test_worktree_{}", name));
        if p.exists() {
            fs::remove_dir_all(&p).unwrap();
        }
        p
    }

    fn setup_repo(name: &str) -> (PathBuf, PathBuf, OID) {
        let tmp = test_dir(name);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir().to_path_buf();

        // Create an initial commit
        let sig = Signature {
            name: "Test".to_string(),
            email: "test@test.com".to_string(),
            time: 1700000000,
            offset: 0,
        };
        let tree_data = serialize_tree(&[]);
        let tree_oid =
            write_loose_object(&git_dir, crate::types::ObjectType::Tree, &tree_data).unwrap();
        let commit_data = serialize_commit(&tree_oid, &[], &sig, &sig, "initial", None);
        let commit_oid =
            write_loose_object(&git_dir, crate::types::ObjectType::Commit, &commit_data).unwrap();

        crate::refs::write_reference(&git_dir, "refs/heads/main", &commit_oid).unwrap();
        write_symbolic_reference(&git_dir, "HEAD", "refs/heads/main").unwrap();

        (tmp, git_dir, commit_oid)
    }

    #[test]
    fn test_worktree_list_empty() {
        let (_tmp, git_dir, _) = setup_repo("list_empty");
        let names = worktree_list(&git_dir).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn test_worktree_add_and_list() {
        let (tmp, git_dir, _) = setup_repo("add_list");
        let wt_path = tmp.join("wt-feature");

        let wt = worktree_add(&git_dir, "feature", &wt_path, None).unwrap();
        assert_eq!(wt.name, "feature");
        assert!(!wt.locked);

        let names = worktree_list(&git_dir).unwrap();
        assert_eq!(names, vec!["feature"]);
    }

    #[test]
    fn test_worktree_lookup() {
        let (tmp, git_dir, _) = setup_repo("lookup");
        let wt_path = tmp.join("wt-lookup");

        worktree_add(&git_dir, "mylookup", &wt_path, None).unwrap();

        let wt = worktree_lookup(&git_dir, "mylookup").unwrap();
        assert_eq!(wt.name, "mylookup");
        assert!(!wt.locked);
    }

    #[test]
    fn test_worktree_lookup_not_found() {
        let (_tmp, git_dir, _) = setup_repo("lookup_nf");
        let result = worktree_lookup(&git_dir, "nope");
        assert!(result.is_err());
    }

    #[test]
    fn test_worktree_validate() {
        let (tmp, git_dir, _) = setup_repo("validate");
        let wt_path = tmp.join("wt-validate");

        let wt = worktree_add(&git_dir, "val", &wt_path, None).unwrap();
        assert!(worktree_validate(&wt).is_ok());
    }

    #[test]
    fn test_worktree_add_duplicate() {
        let (tmp, git_dir, _) = setup_repo("dup");
        let wt_path = tmp.join("wt-dup");

        worktree_add(&git_dir, "dup", &wt_path, None).unwrap();
        let result = worktree_add(&git_dir, "dup", &wt_path, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_worktree_add_with_lock() {
        let (tmp, git_dir, _) = setup_repo("add_lock");
        let wt_path = tmp.join("wt-locked");

        let opts = WorktreeAddOptions {
            lock: true,
            ..Default::default()
        };
        let wt = worktree_add(&git_dir, "locked", &wt_path, Some(&opts)).unwrap();
        assert!(wt.locked);
    }

    #[test]
    fn test_worktree_add_with_reference() {
        let (tmp, git_dir, head_oid) = setup_repo("add_ref");
        let wt_path = tmp.join("wt-ref");

        // Create a branch first
        write_reference(&git_dir, "refs/heads/mybranch", &head_oid).unwrap();

        let opts = WorktreeAddOptions {
            reference: Some("refs/heads/mybranch".to_string()),
            ..Default::default()
        };
        let wt = worktree_add(&git_dir, "myref", &wt_path, Some(&opts)).unwrap();
        assert_eq!(wt.name, "myref");

        // Check HEAD in worktree metadata points to the branch
        let head = fs::read_to_string(git_dir.join("worktrees/myref/HEAD")).unwrap();
        assert_eq!(head.trim(), "ref: refs/heads/mybranch");
    }

    #[test]
    fn test_worktree_lock_unlock() {
        let (tmp, git_dir, _) = setup_repo("lock_unlock");
        let wt_path = tmp.join("wt-lockunlock");

        worktree_add(&git_dir, "lu", &wt_path, None).unwrap();

        // Not locked initially
        assert_eq!(worktree_is_locked(&git_dir, "lu").unwrap(), None);

        // Lock it
        worktree_lock(&git_dir, "lu", Some("maintenance")).unwrap();
        assert_eq!(
            worktree_is_locked(&git_dir, "lu").unwrap(),
            Some("maintenance".to_string())
        );

        // Can't double-lock
        assert!(worktree_lock(&git_dir, "lu", None).is_err());

        // Unlock
        let was_locked = worktree_unlock(&git_dir, "lu").unwrap();
        assert!(was_locked);

        // Unlock again returns false
        let was_locked2 = worktree_unlock(&git_dir, "lu").unwrap();
        assert!(!was_locked2);
    }

    #[test]
    fn test_worktree_is_prunable() {
        let (tmp, git_dir, _) = setup_repo("prunable");
        let wt_path = tmp.join("wt-prunable");

        worktree_add(&git_dir, "pr", &wt_path, None).unwrap();

        // Not prunable by default (working dir exists)
        assert!(!worktree_is_prunable(&git_dir, "pr", None).unwrap());

        // Prunable with valid flag
        let opts = WorktreePruneOptions {
            valid: true,
            ..Default::default()
        };
        assert!(worktree_is_prunable(&git_dir, "pr", Some(&opts)).unwrap());

        // Lock it — not prunable even with valid
        worktree_lock(&git_dir, "pr", None).unwrap();
        assert!(!worktree_is_prunable(&git_dir, "pr", Some(&opts)).unwrap());

        // Prunable with both flags
        let opts2 = WorktreePruneOptions {
            valid: true,
            locked: true,
            ..Default::default()
        };
        assert!(worktree_is_prunable(&git_dir, "pr", Some(&opts2)).unwrap());
    }

    #[test]
    fn test_worktree_prune() {
        let (tmp, git_dir, _) = setup_repo("prune");
        let wt_path = tmp.join("wt-prune");

        worktree_add(&git_dir, "tobepruned", &wt_path, None).unwrap();

        // Can't prune without valid flag
        assert!(worktree_prune(&git_dir, "tobepruned", None).is_err());

        // Prune with working tree removal
        let opts = WorktreePruneOptions {
            valid: true,
            working_tree: true,
            ..Default::default()
        };
        worktree_prune(&git_dir, "tobepruned", Some(&opts)).unwrap();

        // Should be gone
        assert!(worktree_list(&git_dir).unwrap().is_empty());
        assert!(!wt_path.exists());
    }

    #[test]
    fn test_worktree_prune_locked_fails() {
        let (tmp, git_dir, _) = setup_repo("prune_locked");
        let wt_path = tmp.join("wt-prune-locked");

        worktree_add(&git_dir, "plk", &wt_path, None).unwrap();
        worktree_lock(&git_dir, "plk", Some("keep")).unwrap();

        let opts = WorktreePruneOptions {
            valid: true,
            ..Default::default()
        };
        assert!(worktree_prune(&git_dir, "plk", Some(&opts)).is_err());
    }

    #[test]
    fn test_worktree_gitlink_structure() {
        let (tmp, git_dir, _) = setup_repo("gitlink");
        let wt_path = tmp.join("wt-gitlink");

        let wt = worktree_add(&git_dir, "gl", &wt_path, None).unwrap();

        // Check .git file in worktree is a gitlink
        let gitlink = fs::read_to_string(wt.path.join(".git")).unwrap();
        assert!(gitlink.starts_with("gitdir: "));

        // Check metadata has commondir
        let commondir =
            fs::read_to_string(git_dir.join("worktrees/gl/commondir")).unwrap();
        assert_eq!(commondir.trim(), "../..");

        // Check metadata has HEAD as symbolic ref
        let head = fs::read_to_string(git_dir.join("worktrees/gl/HEAD")).unwrap();
        assert!(head.starts_with("ref: refs/heads/"));
    }
}
