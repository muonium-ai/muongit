//! Git grafts — commit parent overrides
//! Parity: libgit2 src/libgit2/grafts.c

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::error::MuonGitError;
use crate::oid::OID;

/// A graft entry: a commit with overridden parents
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Graft {
    pub oid: OID,
    pub parents: Vec<OID>,
}

/// A collection of grafts loaded from .git/info/grafts or .git/shallow
#[derive(Debug, Clone, Default)]
pub struct Grafts {
    entries: HashMap<String, Graft>,
}

impl Grafts {
    /// Create an empty grafts set
    pub fn new() -> Self {
        Self::default()
    }

    /// Load grafts from a file (typically .git/info/grafts)
    pub fn load(path: &Path) -> Result<Self, MuonGitError> {
        let mut grafts = Grafts::new();
        if path.exists() {
            let content = fs::read_to_string(path)?;
            grafts.parse(&content)?;
        }
        Ok(grafts)
    }

    /// Load grafts for a repository (checks info/grafts)
    pub fn load_for_repo(git_dir: &Path) -> Result<Self, MuonGitError> {
        let grafts_path = git_dir.join("info").join("grafts");
        Self::load(&grafts_path)
    }

    /// Load shallow entries (commits with truncated parents)
    pub fn load_shallow(git_dir: &Path) -> Result<Self, MuonGitError> {
        let shallow_path = git_dir.join("shallow");
        if !shallow_path.exists() {
            return Ok(Grafts::new());
        }
        let content = fs::read_to_string(&shallow_path)?;
        let mut grafts = Grafts::new();
        // Shallow format: one OID per line, meaning "this commit has no parents"
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let oid = OID::from_hex(line)?;
            grafts.add(Graft {
                oid,
                parents: vec![],
            });
        }
        Ok(grafts)
    }

    /// Parse grafts from content string
    ///
    /// Format: each line is `COMMIT_OID [PARENT_OID ...]`
    pub fn parse(&mut self, content: &str) -> Result<(), MuonGitError> {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            let oid = OID::from_hex(parts[0])?;
            let parents: Result<Vec<OID>, _> = parts[1..].iter().map(|h| OID::from_hex(h)).collect();
            let parents = parents?;

            self.add(Graft { oid, parents });
        }
        Ok(())
    }

    /// Add a graft entry
    pub fn add(&mut self, graft: Graft) {
        self.entries.insert(graft.oid.hex(), graft);
    }

    /// Remove a graft entry
    pub fn remove(&mut self, oid: &OID) -> bool {
        self.entries.remove(&oid.hex()).is_some()
    }

    /// Look up a graft for a commit
    pub fn get(&self, oid: &OID) -> Option<&Graft> {
        self.entries.get(&oid.hex())
    }

    /// Check if a commit has a graft
    pub fn contains(&self, oid: &OID) -> bool {
        self.entries.contains_key(&oid.hex())
    }

    /// Get parents for a commit, returning grafted parents if available
    pub fn get_parents(&self, oid: &OID) -> Option<&[OID]> {
        self.entries.get(&oid.hex()).map(|g| g.parents.as_slice())
    }

    /// Number of graft entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the grafts set is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// List all grafted commit OIDs
    pub fn oids(&self) -> Vec<OID> {
        self.entries.values().map(|g| g.oid.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_repo(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp")
            .join(name);
        if base.exists() {
            fs::remove_dir_all(&base).unwrap();
        }
        let git_dir = base.join(".git");
        fs::create_dir_all(git_dir.join("info")).unwrap();
        (base, git_dir)
    }

    #[test]
    fn test_grafts_parse() {
        let mut grafts = Grafts::new();
        let content = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb cccccccccccccccccccccccccccccccccccccccc\ndddddddddddddddddddddddddddddddddddddddd\n";
        grafts.parse(content).unwrap();

        assert_eq!(grafts.len(), 2);

        let oid_a = OID::from_hex("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let graft = grafts.get(&oid_a).unwrap();
        assert_eq!(graft.parents.len(), 2);

        let oid_d = OID::from_hex("dddddddddddddddddddddddddddddddddddddddd").unwrap();
        let graft = grafts.get(&oid_d).unwrap();
        assert_eq!(graft.parents.len(), 0);
    }

    #[test]
    fn test_grafts_load_file() {
        let (_base, git_dir) = setup_repo("grafts_load");
        let grafts_path = git_dir.join("info").join("grafts");
        fs::write(
            &grafts_path,
            "# comment\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n",
        )
        .unwrap();

        let grafts = Grafts::load_for_repo(&git_dir).unwrap();
        assert_eq!(grafts.len(), 1);
        let oid = OID::from_hex("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        assert!(grafts.contains(&oid));
    }

    #[test]
    fn test_grafts_shallow() {
        let (_base, git_dir) = setup_repo("grafts_shallow");
        let shallow_path = git_dir.join("shallow");
        fs::write(
            &shallow_path,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n",
        )
        .unwrap();

        let grafts = Grafts::load_shallow(&git_dir).unwrap();
        assert_eq!(grafts.len(), 2);
        let oid = OID::from_hex("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let graft = grafts.get(&oid).unwrap();
        assert!(graft.parents.is_empty());
    }

    #[test]
    fn test_grafts_add_remove() {
        let mut grafts = Grafts::new();
        let oid = OID::from_hex("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let parent = OID::from_hex("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();

        grafts.add(Graft {
            oid: oid.clone(),
            parents: vec![parent.clone()],
        });
        assert!(grafts.contains(&oid));
        assert_eq!(grafts.get_parents(&oid).unwrap().len(), 1);

        assert!(grafts.remove(&oid));
        assert!(!grafts.contains(&oid));
    }

    #[test]
    fn test_grafts_empty_file() {
        let (_base, git_dir) = setup_repo("grafts_empty");
        let grafts = Grafts::load_for_repo(&git_dir).unwrap();
        assert!(grafts.is_empty());
    }
}
