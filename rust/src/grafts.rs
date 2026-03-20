//! Git grafts: commit parent overrides
//! Parity: libgit2 src/libgit2/grafts.c

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::error::MuonGitError;
use crate::oid::OID;

/// A graft entry: a commit with overridden parents.
#[derive(Debug, Clone)]
pub struct Graft {
    pub oid: OID,
    pub parents: Vec<OID>,
}

/// A collection of grafts loaded from .git/info/grafts or .git/shallow.
#[derive(Debug, Clone, Default)]
pub struct Grafts {
    entries: HashMap<String, Graft>,
}

impl Grafts {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load grafts from a file.
    pub fn load(path: &Path) -> Result<Self, MuonGitError> {
        let mut grafts = Grafts::new();
        if path.exists() {
            let content = fs::read_to_string(path)
                .map_err(|e| MuonGitError::NotFound(format!("cannot read grafts: {}", e)))?;
            grafts.parse(&content)?;
        }
        Ok(grafts)
    }

    /// Load grafts for a repository.
    pub fn load_for_repo(git_dir: &Path) -> Result<Self, MuonGitError> {
        Self::load(&git_dir.join("info/grafts"))
    }

    /// Load shallow entries for a repository.
    pub fn load_shallow(git_dir: &Path) -> Result<Self, MuonGitError> {
        let path = git_dir.join("shallow");
        if !path.exists() {
            return Ok(Grafts::new());
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| MuonGitError::NotFound(format!("cannot read shallow: {}", e)))?;

        let mut grafts = Grafts::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let oid = OID::from_hex(line)
                .map_err(|_| MuonGitError::InvalidObject("invalid OID in shallow".into()))?;
            grafts.add(Graft {
                oid,
                parents: vec![],
            });
        }
        Ok(grafts)
    }

    /// Parse grafts from a content string.
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
            let oid = OID::from_hex(parts[0])
                .map_err(|_| MuonGitError::InvalidObject("invalid graft OID".into()))?;
            let mut parents = Vec::new();
            for part in &parts[1..] {
                let parent = OID::from_hex(part)
                    .map_err(|_| MuonGitError::InvalidObject("invalid parent OID".into()))?;
                parents.push(parent);
            }
            self.add(Graft { oid, parents });
        }
        Ok(())
    }

    /// Add a graft entry.
    pub fn add(&mut self, graft: Graft) {
        self.entries.insert(graft.oid.hex.clone(), graft);
    }

    /// Remove a graft entry. Returns true if it existed.
    pub fn remove(&mut self, oid: &OID) -> bool {
        self.entries.remove(&oid.hex).is_some()
    }

    /// Look up a graft for a commit.
    pub fn get(&self, oid: &OID) -> Option<&Graft> {
        self.entries.get(&oid.hex)
    }

    /// Check if a commit has a graft.
    pub fn contains(&self, oid: &OID) -> bool {
        self.entries.contains_key(&oid.hex)
    }

    /// Get parents for a commit, returning grafted parents if available.
    pub fn get_parents(&self, oid: &OID) -> Option<&[OID]> {
        self.entries.get(&oid.hex).map(|g| g.parents.as_slice())
    }

    /// Number of graft entries.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Whether the grafts set is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// List all grafted commit OIDs.
    pub fn oids(&self) -> Vec<OID> {
        self.entries.values().map(|g| g.oid.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp");
        fs::create_dir_all(&base).unwrap();
        let p = base.join(format!("test_grafts_{}", name));
        if p.exists() {
            fs::remove_dir_all(&p).unwrap();
        }
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn test_grafts_parse() {
        let mut grafts = Grafts::new();
        let content = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d\n";
        grafts.parse(content).unwrap();
        assert_eq!(grafts.count(), 1);

        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        assert!(grafts.contains(&oid));

        let parents = grafts.get_parents(&oid).unwrap();
        assert_eq!(parents.len(), 1);
    }

    #[test]
    fn test_grafts_no_parents() {
        let mut grafts = Grafts::new();
        let content = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d\n";
        grafts.parse(content).unwrap();

        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let parents = grafts.get_parents(&oid).unwrap();
        assert!(parents.is_empty());
    }

    #[test]
    fn test_grafts_comments_and_blanks() {
        let mut grafts = Grafts::new();
        let content = "# comment\n\naaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d\n";
        grafts.parse(content).unwrap();
        assert_eq!(grafts.count(), 1);
    }

    #[test]
    fn test_grafts_add_remove() {
        let mut grafts = Grafts::new();
        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        grafts.add(Graft {
            oid: oid.clone(),
            parents: vec![],
        });
        assert!(grafts.contains(&oid));
        assert!(grafts.remove(&oid));
        assert!(!grafts.contains(&oid));
        assert!(!grafts.remove(&oid));
    }

    #[test]
    fn test_grafts_load_file() {
        let tmp = test_dir("load_file");
        let grafts_path = tmp.join("grafts");
        fs::write(
            &grafts_path,
            "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d\n",
        )
        .unwrap();

        let grafts = Grafts::load(&grafts_path).unwrap();
        assert_eq!(grafts.count(), 1);
    }

    #[test]
    fn test_grafts_load_missing_file() {
        let tmp = test_dir("load_missing");
        let grafts = Grafts::load(&tmp.join("nonexistent")).unwrap();
        assert!(grafts.is_empty());
    }

    #[test]
    fn test_grafts_empty() {
        let grafts = Grafts::new();
        assert!(grafts.is_empty());
        assert_eq!(grafts.count(), 0);
        assert!(grafts.oids().is_empty());
    }
}
