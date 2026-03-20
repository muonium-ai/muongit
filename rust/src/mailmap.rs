//! Mailmap — email/name mapping for canonical author identities
//! Parity: libgit2 src/libgit2/mailmap.c

use std::fs;
use std::path::Path;

use crate::error::MuonGitError;
use crate::types::Signature;

/// A single mailmap entry
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailmapEntry {
    pub real_name: Option<String>,
    pub real_email: Option<String>,
    pub replace_name: Option<String>,
    pub replace_email: String,
}

/// A mailmap holding name/email mappings
#[derive(Debug, Clone)]
pub struct Mailmap {
    entries: Vec<MailmapEntry>,
}

impl Mailmap {
    /// Create an empty mailmap
    pub fn new() -> Self {
        Mailmap {
            entries: Vec::new(),
        }
    }

    /// Load mailmap from a file
    pub fn load(path: &Path) -> Result<Self, MuonGitError> {
        let mut mm = Mailmap::new();
        if path.exists() {
            let content = fs::read_to_string(path)?;
            mm.parse(&content);
        }
        Ok(mm)
    }

    /// Load mailmap for a repository
    ///
    /// Reads .mailmap from the workdir root.
    pub fn load_for_repo(workdir: &Path) -> Result<Self, MuonGitError> {
        let mailmap_path = workdir.join(".mailmap");
        Self::load(&mailmap_path)
    }

    /// Parse mailmap content
    ///
    /// Supports four forms:
    /// - `<real@email> <old@email>`
    /// - `Real Name <real@email> <old@email>`
    /// - `Real Name <real@email> Old Name <old@email>`
    /// - `<real@email> Old Name <old@email>`
    pub fn parse(&mut self, content: &str) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(entry) = parse_mailmap_line(line) {
                self.entries.push(entry);
            }
        }
        // Sort by (replace_email, replace_name) for binary search
        self.entries.sort_by(|a, b| {
            let email_cmp = a
                .replace_email
                .to_lowercase()
                .cmp(&b.replace_email.to_lowercase());
            if email_cmp != std::cmp::Ordering::Equal {
                return email_cmp;
            }
            let a_name = a.replace_name.as_deref().unwrap_or("");
            let b_name = b.replace_name.as_deref().unwrap_or("");
            a_name.to_lowercase().cmp(&b_name.to_lowercase())
        });
    }

    /// Add a mapping entry
    pub fn add_entry(&mut self, entry: MailmapEntry) {
        self.entries.push(entry);
    }

    /// Resolve a name/email pair to canonical values
    pub fn resolve(&self, name: &str, email: &str) -> (String, String) {
        let email_lower = email.to_lowercase();

        // First try exact match with both name and email
        for entry in &self.entries {
            if entry.replace_email.to_lowercase() == email_lower {
                if let Some(ref rn) = entry.replace_name {
                    if rn.to_lowercase() == name.to_lowercase() {
                        let resolved_name = entry.real_name.as_deref().unwrap_or(name);
                        let resolved_email = entry.real_email.as_deref().unwrap_or(email);
                        return (resolved_name.to_string(), resolved_email.to_string());
                    }
                }
            }
        }

        // Then try email-only match (entries with no replace_name)
        for entry in &self.entries {
            if entry.replace_email.to_lowercase() == email_lower && entry.replace_name.is_none() {
                let resolved_name = entry.real_name.as_deref().unwrap_or(name);
                let resolved_email = entry.real_email.as_deref().unwrap_or(email);
                return (resolved_name.to_string(), resolved_email.to_string());
            }
        }

        (name.to_string(), email.to_string())
    }

    /// Resolve a signature to canonical values
    pub fn resolve_signature(&self, sig: &Signature) -> Signature {
        let (name, email) = self.resolve(&sig.name, &sig.email);
        Signature {
            name,
            email,
            time: sig.time,
            offset: sig.offset,
        }
    }

    /// Number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the mailmap is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Parse a single mailmap line
fn parse_mailmap_line(line: &str) -> Option<MailmapEntry> {
    // Extract all <email> tokens and text between them
    let mut emails = Vec::new();
    let mut names = Vec::new();
    let mut current_name = String::new();
    let mut in_email = false;
    let mut current_email = String::new();

    for ch in line.chars() {
        match ch {
            '<' => {
                in_email = true;
                current_email.clear();
                let name = current_name.trim().to_string();
                if !name.is_empty() {
                    names.push(name);
                }
                current_name.clear();
            }
            '>' => {
                in_email = false;
                emails.push(current_email.trim().to_string());
            }
            _ => {
                if in_email {
                    current_email.push(ch);
                } else {
                    current_name.push(ch);
                }
            }
        }
    }

    match emails.len() {
        1 => {
            // Only one email — not a valid mailmap line
            None
        }
        2 => {
            // Two emails: first is real, second is replace
            let real_email = &emails[0];
            let replace_email = &emails[1];

            let real_name = if !names.is_empty() && !names[0].is_empty() {
                Some(names[0].clone())
            } else {
                None
            };
            let replace_name = if names.len() > 1 && !names[1].is_empty() {
                Some(names[1].clone())
            } else {
                None
            };

            Some(MailmapEntry {
                real_name,
                real_email: if real_email.is_empty() {
                    None
                } else {
                    Some(real_email.clone())
                },
                replace_name,
                replace_email: replace_email.clone(),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_email_only() {
        let mut mm = Mailmap::new();
        mm.parse("<real@example.com> <old@example.com>\n");
        assert_eq!(mm.len(), 1);

        let (name, email) = mm.resolve("Someone", "old@example.com");
        assert_eq!(email, "real@example.com");
        assert_eq!(name, "Someone"); // name not changed
    }

    #[test]
    fn test_parse_name_and_email() {
        let mut mm = Mailmap::new();
        mm.parse("Real Name <real@example.com> <old@example.com>\n");

        let (name, email) = mm.resolve("Old Name", "old@example.com");
        assert_eq!(name, "Real Name");
        assert_eq!(email, "real@example.com");
    }

    #[test]
    fn test_parse_full_mapping() {
        let mut mm = Mailmap::new();
        mm.parse("Real Name <real@example.com> Old Name <old@example.com>\n");

        // With matching old name
        let (name, email) = mm.resolve("Old Name", "old@example.com");
        assert_eq!(name, "Real Name");
        assert_eq!(email, "real@example.com");

        // With non-matching old name — email-only fallback not available
        let (name, email) = mm.resolve("Other Name", "old@example.com");
        assert_eq!(name, "Other Name");
        assert_eq!(email, "old@example.com");
    }

    #[test]
    fn test_resolve_signature() {
        let mut mm = Mailmap::new();
        mm.parse("Proper Name <proper@example.com> <typo@example.com>\n");

        let sig = Signature {
            name: "Wrong Name".to_string(),
            email: "typo@example.com".to_string(),
            time: 1000000000,
            offset: 0,
        };

        let resolved = mm.resolve_signature(&sig);
        assert_eq!(resolved.name, "Proper Name");
        assert_eq!(resolved.email, "proper@example.com");
        assert_eq!(resolved.time, sig.time);
    }

    #[test]
    fn test_no_match() {
        let mm = Mailmap::new();
        let (name, email) = mm.resolve("Name", "email@example.com");
        assert_eq!(name, "Name");
        assert_eq!(email, "email@example.com");
    }

    #[test]
    fn test_comments_and_empty_lines() {
        let mut mm = Mailmap::new();
        mm.parse("# comment\n\n<real@example.com> <old@example.com>\n");
        assert_eq!(mm.len(), 1);
    }

    #[test]
    fn test_load_missing_file() {
        let mm = Mailmap::load(Path::new("/nonexistent/.mailmap")).unwrap();
        assert!(mm.is_empty());
    }
}
