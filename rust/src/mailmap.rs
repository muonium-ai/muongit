//! Git mailmap: email address mapping for signatures
//! Parity: libgit2 src/libgit2/mailmap.c

use std::fs;
use std::path::Path;

use crate::error::MuonGitError;

/// A single mailmap entry mapping old name/email to real name/email.
#[derive(Debug, Clone)]
struct MailmapEntry {
    real_name: Option<String>,
    real_email: Option<String>,
    replace_name: Option<String>,
    replace_email: String,
}

/// A mailmap that resolves author identities.
#[derive(Debug, Clone, Default)]
pub struct Mailmap {
    entries: Vec<MailmapEntry>,
}

impl Mailmap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a mapping entry.
    pub fn add_entry(
        &mut self,
        real_name: Option<&str>,
        real_email: Option<&str>,
        replace_name: Option<&str>,
        replace_email: &str,
    ) {
        self.entries.push(MailmapEntry {
            real_name: real_name.map(|s| s.to_string()),
            real_email: real_email.map(|s| s.to_string()),
            replace_name: replace_name.map(|s| s.to_string()),
            replace_email: replace_email.to_lowercase(),
        });
    }

    /// Parse mailmap from a buffer.
    pub fn from_buffer(buf: &str) -> Result<Self, MuonGitError> {
        let mut mm = Mailmap::new();
        for line in buf.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            mm.parse_line(line)?;
        }
        Ok(mm)
    }

    /// Load mailmap from a repository (.mailmap file and mailmap.file/mailmap.blob config).
    pub fn from_repository(git_dir: &Path) -> Result<Self, MuonGitError> {
        // Try .mailmap in worktree (parent of .git dir)
        let worktree = git_dir.parent().unwrap_or(git_dir);
        let mailmap_path = worktree.join(".mailmap");
        if mailmap_path.exists() {
            let content = fs::read_to_string(&mailmap_path)
                .map_err(|e| MuonGitError::NotFound(format!("cannot read .mailmap: {}", e)))?;
            return Self::from_buffer(&content);
        }
        Ok(Mailmap::new())
    }

    /// Resolve a name/email pair through the mailmap.
    pub fn resolve(&self, name: &str, email: &str) -> (String, String) {
        let email_lower = email.to_lowercase();
        let mut resolved_name = name.to_string();
        let mut resolved_email = email.to_string();

        for entry in &self.entries {
            // Match by email (required)
            if entry.replace_email != email_lower {
                continue;
            }
            // Match by name (optional)
            if let Some(ref rn) = entry.replace_name {
                if !rn.eq_ignore_ascii_case(name) {
                    continue;
                }
            }
            if let Some(ref real_name) = entry.real_name {
                resolved_name = real_name.clone();
            }
            if let Some(ref real_email) = entry.real_email {
                resolved_email = real_email.clone();
            }
            break;
        }

        (resolved_name, resolved_email)
    }

    /// Resolve a signature's name and email through the mailmap.
    pub fn resolve_signature(
        &self,
        name: &str,
        email: &str,
    ) -> (String, String) {
        self.resolve(name, email)
    }

    /// Parse a single mailmap line.
    fn parse_line(&mut self, line: &str) -> Result<(), MuonGitError> {
        // Formats:
        // Proper Name <proper@email> Commit Name <commit@email>
        // Proper Name <proper@email> <commit@email>
        // <proper@email> <commit@email>
        // Proper Name <commit@email>

        let mut rest = line;
        let (name1, email1, after1) = parse_name_email(rest)?;
        rest = after1.trim();

        if rest.is_empty() {
            // "Proper Name <commit@email>" or "<proper@email>"
            if let Some(ref e) = email1 {
                self.add_entry(name1.as_deref(), None, None, e);
            }
        } else {
            let (name2, email2, _) = parse_name_email(rest)?;
            if let Some(ref commit_email) = email2 {
                self.add_entry(
                    name1.as_deref(),
                    email1.as_deref(),
                    name2.as_deref(),
                    commit_email,
                );
            } else if let Some(ref e) = email1 {
                self.add_entry(name1.as_deref(), None, None, e);
            }
        }
        Ok(())
    }
}

/// Parse a "Name <email>" pair from the start of a string.
/// Returns (name, email, remaining).
fn parse_name_email(s: &str) -> Result<(Option<String>, Option<String>, &str), MuonGitError> {
    let s = s.trim();
    if let Some(lt) = s.find('<') {
        let name = s[..lt].trim();
        let name = if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        };
        let after_lt = &s[lt + 1..];
        if let Some(gt) = after_lt.find('>') {
            let email = after_lt[..gt].trim().to_string();
            let rest = &after_lt[gt + 1..];
            Ok((name, Some(email), rest))
        } else {
            Err(MuonGitError::InvalidObject(
                "unterminated email in mailmap".into(),
            ))
        }
    } else {
        // Just a name, no email bracket
        Ok((Some(s.to_string()), None, ""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_email_mapping() {
        let mm = Mailmap::from_buffer("Proper Name <proper@email> <commit@email>\n").unwrap();
        let (name, email) = mm.resolve("Old Name", "commit@email");
        assert_eq!(name, "Proper Name");
        assert_eq!(email, "proper@email");
    }

    #[test]
    fn test_name_only_mapping() {
        let mm = Mailmap::from_buffer("Proper Name <commit@email>\n").unwrap();
        let (name, email) = mm.resolve("Wrong Name", "commit@email");
        assert_eq!(name, "Proper Name");
        assert_eq!(email, "commit@email"); // email unchanged
    }

    #[test]
    fn test_full_mapping() {
        let buf = "Real Name <real@email> Commit Name <commit@email>\n";
        let mm = Mailmap::from_buffer(buf).unwrap();

        // Only matches when both name and email match
        let (name, email) = mm.resolve("Commit Name", "commit@email");
        assert_eq!(name, "Real Name");
        assert_eq!(email, "real@email");

        // Doesn't match different name
        let (name, email) = mm.resolve("Other Name", "commit@email");
        assert_eq!(name, "Other Name");
        assert_eq!(email, "commit@email");
    }

    #[test]
    fn test_email_only_mapping() {
        let mm = Mailmap::from_buffer("<proper@email> <commit@email>\n").unwrap();
        let (name, email) = mm.resolve("Any Name", "commit@email");
        assert_eq!(name, "Any Name"); // name unchanged
        assert_eq!(email, "proper@email");
    }

    #[test]
    fn test_case_insensitive_email() {
        let mm = Mailmap::from_buffer("Proper <proper@email> <COMMIT@EMAIL>\n").unwrap();
        let (name, email) = mm.resolve("Old", "commit@email");
        assert_eq!(name, "Proper");
        assert_eq!(email, "proper@email");
    }

    #[test]
    fn test_empty_mailmap() {
        let mm = Mailmap::from_buffer("").unwrap();
        let (name, email) = mm.resolve("Name", "email@test.com");
        assert_eq!(name, "Name");
        assert_eq!(email, "email@test.com");
    }

    #[test]
    fn test_comments_and_blanks() {
        let buf = "# Comment\n\nProper <proper@email> <old@email>\n";
        let mm = Mailmap::from_buffer(buf).unwrap();
        let (_, email) = mm.resolve("Name", "old@email");
        assert_eq!(email, "proper@email");
    }

    #[test]
    fn test_no_match() {
        let mm = Mailmap::from_buffer("Proper <proper@email> <old@email>\n").unwrap();
        let (name, email) = mm.resolve("Name", "other@email");
        assert_eq!(name, "Name");
        assert_eq!(email, "other@email");
    }
}
