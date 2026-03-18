//! Reference reading and resolution.
//! Parity: libgit2 src/libgit2/refs.c, src/libgit2/refdb_fs.c

use std::fs;
use std::path::Path;

use crate::{MuonGitError, OID};

/// Read a reference value (raw string) from the git directory.
///
/// Checks loose refs first (`git_dir/{name}`), then falls back to packed-refs.
/// The returned string may be a symbolic ref (starting with "ref: ") or a hex OID.
pub fn read_reference(git_dir: &Path, name: &str) -> Result<String, MuonGitError> {
    // Try loose ref first
    let loose_path = git_dir.join(name);
    if loose_path.is_file() {
        let content = fs::read_to_string(&loose_path)?;
        return Ok(content.trim().to_string());
    }

    // Try packed-refs
    if let Some(value) = lookup_packed_ref(git_dir, name)? {
        return Ok(value);
    }

    Err(MuonGitError::NotFound(format!("reference not found: {}", name)))
}

/// Look up a reference in the packed-refs file.
///
/// The packed-refs format is one ref per line: `{hex_oid} {refname}`
/// Lines starting with '#' are comments. Lines starting with '^' are peeled OIDs (ignored here).
fn lookup_packed_ref(git_dir: &Path, name: &str) -> Result<Option<String>, MuonGitError> {
    let packed_path = git_dir.join("packed-refs");
    if !packed_path.is_file() {
        return Ok(None);
    }

    let content = fs::read_to_string(&packed_path)?;
    for line in content.lines() {
        // Skip comments and peel lines
        if line.starts_with('#') || line.starts_with('^') {
            continue;
        }

        // Format: "{oid} {refname}"
        if let Some((oid_hex, refname)) = line.split_once(' ') {
            if refname == name {
                return Ok(Some(oid_hex.to_string()));
            }
        }
    }

    Ok(None)
}

/// Resolve a reference to a final OID, following symbolic refs.
///
/// Symbolic refs (values starting with "ref: ") are followed until a hex OID is found.
/// A maximum depth of 10 is enforced to prevent infinite loops.
pub fn resolve_reference(git_dir: &Path, name: &str) -> Result<OID, MuonGitError> {
    let mut current_name = name.to_string();
    let max_depth = 10;

    for _ in 0..max_depth {
        let value = read_reference(git_dir, &current_name)?;

        if let Some(target) = value.strip_prefix("ref: ") {
            current_name = target.trim().to_string();
            continue;
        }

        // Should be a hex OID
        let oid = OID::from_hex(value.trim())?;
        return Ok(oid);
    }

    Err(MuonGitError::Invalid(format!(
        "symbolic reference chain too deep for: {}",
        name
    )))
}

/// List all references (both loose and packed).
///
/// Returns a vector of `(refname, value)` pairs where value is a hex OID string
/// or a symbolic ref string.
pub fn list_references(git_dir: &Path) -> Result<Vec<(String, String)>, MuonGitError> {
    let mut refs = std::collections::HashMap::new();

    // Collect packed refs first (loose refs override them)
    let packed_path = git_dir.join("packed-refs");
    if packed_path.is_file() {
        let content = fs::read_to_string(&packed_path)?;
        for line in content.lines() {
            if line.starts_with('#') || line.starts_with('^') || line.is_empty() {
                continue;
            }
            if let Some((oid_hex, refname)) = line.split_once(' ') {
                refs.insert(refname.to_string(), oid_hex.to_string());
            }
        }
    }

    // Walk loose refs directory tree
    let refs_dir = git_dir.join("refs");
    if refs_dir.is_dir() {
        collect_loose_refs(&refs_dir, "refs", &mut refs)?;
    }

    let mut result: Vec<(String, String)> = refs.into_iter().collect();
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

/// Recursively collect loose refs from the refs directory.
fn collect_loose_refs(
    dir: &Path,
    prefix: &str,
    refs: &mut std::collections::HashMap<String, String>,
) -> Result<(), MuonGitError> {
    let entries = fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let full_ref = format!("{}/{}", prefix, name_str);

        if file_type.is_dir() {
            collect_loose_refs(&entry.path(), &full_ref, refs)?;
        } else if file_type.is_file() {
            let content = fs::read_to_string(entry.path())?;
            refs.insert(full_ref, content.trim().to_string());
        }
    }
    Ok(())
}

/// Write (create or update) a direct reference pointing to an OID.
/// Creates intermediate directories as needed.
pub fn write_reference(git_dir: &Path, name: &str, oid: &OID) -> Result<(), MuonGitError> {
    let ref_path = git_dir.join(name);
    if let Some(parent) = ref_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&ref_path, format!("{}\n", oid.hex()))?;
    Ok(())
}

/// Write (create or update) a symbolic reference.
pub fn write_symbolic_reference(git_dir: &Path, name: &str, target: &str) -> Result<(), MuonGitError> {
    let ref_path = git_dir.join(name);
    if let Some(parent) = ref_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&ref_path, format!("ref: {}\n", target))?;
    Ok(())
}

/// Delete a loose reference file. Returns true if it existed and was deleted.
pub fn delete_reference(git_dir: &Path, name: &str) -> Result<bool, MuonGitError> {
    let ref_path = git_dir.join(name);
    if ref_path.exists() {
        fs::remove_file(&ref_path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Update a reference only if its current value matches `old_oid` (compare-and-swap).
/// Pass `OID::zero()` for `old_oid` to require that the ref does not yet exist (create-only).
pub fn update_reference(git_dir: &Path, name: &str, new_oid: &OID, old_oid: &OID) -> Result<(), MuonGitError> {
    let ref_path = git_dir.join(name);

    if old_oid.is_zero() {
        if ref_path.exists() {
            return Err(MuonGitError::Conflict(format!(
                "reference '{}' already exists", name
            )));
        }
    } else {
        let current = read_reference(git_dir, name)?;
        if current != old_oid.hex() {
            return Err(MuonGitError::Conflict(format!(
                "reference '{}' expected {}, got {}", name, old_oid.hex(), current
            )));
        }
    }

    write_reference(git_dir, name, new_oid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Repository;

    #[test]
    fn test_read_head() {
        let tmp = std::env::temp_dir().join("muongit_test_refs_head");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        let git_dir = repo.git_dir();

        let head = read_reference(git_dir, "HEAD").unwrap();
        assert_eq!(head, "ref: refs/heads/main");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_head_unborn() {
        let tmp = std::env::temp_dir().join("muongit_test_refs_unborn");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        let git_dir = repo.git_dir();

        // HEAD -> refs/heads/main, but main doesn't exist yet
        let result = resolve_reference(git_dir, "HEAD");
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_head_with_commit() {
        let tmp = std::env::temp_dir().join("muongit_test_refs_resolve");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        let git_dir = repo.git_dir();

        // Write a fake commit OID to refs/heads/main
        let fake_oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let main_ref_path = git_dir.join("refs").join("heads").join("main");
        std::fs::write(&main_ref_path, format!("{}\n", fake_oid)).unwrap();

        let oid = resolve_reference(git_dir, "HEAD").unwrap();
        assert_eq!(oid.hex(), fake_oid);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_packed_refs() {
        let tmp = std::env::temp_dir().join("muongit_test_refs_packed");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        let git_dir = repo.git_dir();

        // Write a packed-refs file
        let packed_content = "# pack-refs with: peeled fully-peeled sorted\n\
            aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d refs/heads/feature\n\
            bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d refs/tags/v1.0\n";
        std::fs::write(git_dir.join("packed-refs"), packed_content).unwrap();

        // Read from packed-refs
        let value = read_reference(git_dir, "refs/heads/feature").unwrap();
        assert_eq!(value, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");

        let value = read_reference(git_dir, "refs/tags/v1.0").unwrap();
        assert_eq!(value, "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");

        // Non-existent ref
        let result = read_reference(git_dir, "refs/heads/nonexistent");
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_list_references() {
        let tmp = std::env::temp_dir().join("muongit_test_refs_list");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        let git_dir = repo.git_dir();

        // Write a loose ref
        let main_ref_path = git_dir.join("refs").join("heads").join("main");
        std::fs::write(&main_ref_path, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d\n").unwrap();

        // Write packed-refs with another ref
        let packed_content = "# pack-refs\n\
            bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d refs/tags/v1.0\n";
        std::fs::write(git_dir.join("packed-refs"), packed_content).unwrap();

        let refs_list = list_references(git_dir).unwrap();

        // Should have at least refs/heads/main and refs/tags/v1.0
        let ref_names: Vec<&str> = refs_list.iter().map(|(n, _)| n.as_str()).collect();
        assert!(ref_names.contains(&"refs/heads/main"));
        assert!(ref_names.contains(&"refs/tags/v1.0"));

        // Verify values
        for (name, value) in &refs_list {
            if name == "refs/heads/main" {
                assert_eq!(value, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
            }
            if name == "refs/tags/v1.0" {
                assert_eq!(value, "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
            }
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_loose_overrides_packed() {
        let tmp = std::env::temp_dir().join("muongit_test_refs_override");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        let git_dir = repo.git_dir();

        // Write packed-refs
        let packed_content = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d refs/heads/main\n";
        std::fs::write(git_dir.join("packed-refs"), packed_content).unwrap();

        // Write loose ref with different OID
        let main_ref_path = git_dir.join("refs").join("heads").join("main");
        std::fs::write(&main_ref_path, "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d\n").unwrap();

        // Loose should win
        let value = read_reference(git_dir, "refs/heads/main").unwrap();
        assert_eq!(value, "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");

        // list_references should also return the loose value
        let refs_list = list_references(git_dir).unwrap();
        for (name, value) in &refs_list {
            if name == "refs/heads/main" {
                assert_eq!(value, "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
            }
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_write_and_read_reference() {
        let tmp = std::env::temp_dir().join("muongit_test_ref_write");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        write_reference(repo.git_dir(), "refs/heads/feature", &oid).unwrap();

        let value = read_reference(repo.git_dir(), "refs/heads/feature").unwrap();
        assert_eq!(value, oid.hex());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_write_symbolic_reference() {
        let tmp = std::env::temp_dir().join("muongit_test_ref_sym");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        write_symbolic_reference(repo.git_dir(), "refs/heads/alias", "refs/heads/main").unwrap();

        let value = read_reference(repo.git_dir(), "refs/heads/alias").unwrap();
        assert_eq!(value, "ref: refs/heads/main");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_delete_reference() {
        let tmp = std::env::temp_dir().join("muongit_test_ref_delete");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        write_reference(repo.git_dir(), "refs/heads/feature", &oid).unwrap();

        assert!(delete_reference(repo.git_dir(), "refs/heads/feature").unwrap());
        assert!(read_reference(repo.git_dir(), "refs/heads/feature").is_err());
        assert!(!delete_reference(repo.git_dir(), "refs/heads/nonexistent").unwrap());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_update_reference_success() {
        let tmp = std::env::temp_dir().join("muongit_test_ref_update");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        let oid1 = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let oid2 = OID::from_hex("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();

        // Create with zero old
        update_reference(repo.git_dir(), "refs/heads/feature", &oid1, &OID::zero()).unwrap();
        assert_eq!(read_reference(repo.git_dir(), "refs/heads/feature").unwrap(), oid1.hex());

        // Update with matching old
        update_reference(repo.git_dir(), "refs/heads/feature", &oid2, &oid1).unwrap();
        assert_eq!(read_reference(repo.git_dir(), "refs/heads/feature").unwrap(), oid2.hex());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_update_reference_conflict() {
        let tmp = std::env::temp_dir().join("muongit_test_ref_cas");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        let oid1 = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let oid2 = OID::from_hex("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let oid_wrong = OID::from_hex("ccf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();

        write_reference(repo.git_dir(), "refs/heads/feature", &oid1).unwrap();

        // Wrong old value should fail
        assert!(update_reference(repo.git_dir(), "refs/heads/feature", &oid2, &oid_wrong).is_err());

        // Create-only should fail if exists
        assert!(update_reference(repo.git_dir(), "refs/heads/feature", &oid2, &OID::zero()).is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
