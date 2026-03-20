//! First-class reference database API.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::MuonGitError;
use crate::oid::OID;
use crate::repository::Repository;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub name: String,
    pub value: String,
    pub symbolic_target: Option<String>,
    pub target: Option<OID>,
}

impl Reference {
    fn from_raw(name: String, value: String) -> Result<Self, MuonGitError> {
        let trimmed = value.trim().to_string();
        if let Some(target) = trimmed.strip_prefix("ref: ") {
            Ok(Self {
                name,
                value: trimmed.clone(),
                symbolic_target: Some(target.trim().to_string()),
                target: None,
            })
        } else {
            let oid = OID::from_hex(&trimmed)?;
            Ok(Self {
                name,
                value: trimmed,
                symbolic_target: None,
                target: Some(oid),
            })
        }
    }

    pub fn is_symbolic(&self) -> bool {
        self.symbolic_target.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct RefDb {
    git_dir: PathBuf,
}

impl RefDb {
    pub fn open(git_dir: &Path) -> Self {
        Self {
            git_dir: git_dir.to_path_buf(),
        }
    }

    pub fn read(&self, name: &str) -> Result<Reference, MuonGitError> {
        let value = crate::refs::read_reference(&self.git_dir, name)?;
        Reference::from_raw(name.to_string(), value)
    }

    pub fn resolve(&self, name: &str) -> Result<OID, MuonGitError> {
        crate::refs::resolve_reference(&self.git_dir, name)
    }

    pub fn list(&self) -> Result<Vec<Reference>, MuonGitError> {
        let refs = crate::refs::list_references(&self.git_dir)?;
        refs.into_iter()
            .map(|(name, value)| Reference::from_raw(name, value))
            .collect()
    }

    pub fn write(&self, name: &str, oid: &OID) -> Result<(), MuonGitError> {
        crate::refs::write_reference(&self.git_dir, name, oid)
    }

    pub fn write_symbolic(&self, name: &str, target: &str) -> Result<(), MuonGitError> {
        crate::refs::write_symbolic_reference(&self.git_dir, name, target)
    }

    pub fn delete(&self, name: &str) -> Result<bool, MuonGitError> {
        let loose_deleted = crate::refs::delete_reference(&self.git_dir, name)?;
        let packed_deleted = delete_packed_reference(&self.git_dir, name)?;
        Ok(loose_deleted || packed_deleted)
    }
}

impl Repository {
    pub fn refdb(&self) -> RefDb {
        RefDb::open(self.git_dir())
    }
}

pub(crate) fn packed_references(git_dir: &Path) -> Result<BTreeMap<String, String>, MuonGitError> {
    let packed_path = git_dir.join("packed-refs");
    if !packed_path.exists() {
        return Ok(BTreeMap::new());
    }

    let mut refs = BTreeMap::new();
    let content = fs::read_to_string(&packed_path)?;
    for line in content.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        if let Some((oid_hex, refname)) = line.split_once(' ') {
            refs.insert(refname.to_string(), oid_hex.to_string());
        }
    }
    Ok(refs)
}

pub(crate) fn delete_packed_reference(git_dir: &Path, name: &str) -> Result<bool, MuonGitError> {
    let mut refs = packed_references(git_dir)?;
    let deleted = refs.remove(name).is_some();
    if deleted {
        write_packed_references(git_dir, &refs)?;
    }
    Ok(deleted)
}

fn write_packed_references(
    git_dir: &Path,
    refs: &BTreeMap<String, String>,
) -> Result<(), MuonGitError> {
    let packed_path = git_dir.join("packed-refs");
    if refs.is_empty() {
        if packed_path.exists() {
            fs::remove_file(&packed_path)?;
        }
        return Ok(());
    }

    let mut content = String::from("# pack-refs with: sorted\n");
    for (name, oid_hex) in refs {
        content.push_str(&format!("{} {}\n", oid_hex, name));
    }
    fs::write(packed_path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp").join(name)
    }

    #[test]
    fn test_refdb_reads_loose_packed_and_symbolic_refs() {
        let tmp = test_dir("test_refdb_reads_loose_packed_and_symbolic_refs");
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir();

        let main_oid = OID::from_hex("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let packed_oid = OID::from_hex("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        crate::refs::write_reference(git_dir, "refs/heads/main", &main_oid).unwrap();
        fs::write(
            git_dir.join("packed-refs"),
            format!("{} refs/heads/release\n", packed_oid.hex()),
        )
        .unwrap();

        let refdb = repo.refdb();
        let head = refdb.read("HEAD").unwrap();
        assert!(head.is_symbolic());
        assert_eq!(head.symbolic_target.as_deref(), Some("refs/heads/main"));

        let packed = refdb.read("refs/heads/release").unwrap();
        assert_eq!(packed.target.as_ref(), Some(&packed_oid));

        let refs = refdb.list().unwrap();
        assert!(refs.iter().any(|entry| entry.name == "refs/heads/main"));
        assert!(refs.iter().any(|entry| entry.name == "refs/heads/release"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_refdb_delete_removes_packed_ref() {
        let tmp = test_dir("test_refdb_delete_removes_packed_ref");
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir();

        let packed_oid = OID::from_hex("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        fs::write(
            git_dir.join("packed-refs"),
            format!("{} refs/heads/release\n", packed_oid.hex()),
        )
        .unwrap();

        let refdb = repo.refdb();
        assert!(refdb.delete("refs/heads/release").unwrap());
        assert!(refdb.read("refs/heads/release").is_err());

        let _ = fs::remove_dir_all(&tmp);
    }
}
