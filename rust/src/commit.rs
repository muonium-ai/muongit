//! Commit object read/write

use crate::error::MuonGitError;
use crate::oid::OID;
use crate::types::{ObjectType, Signature};

/// A parsed git commit object
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub oid: OID,
    pub tree_id: OID,
    pub parent_ids: Vec<OID>,
    pub author: Signature,
    pub committer: Signature,
    pub message: String,
    pub message_encoding: Option<String>,
}

/// Parse a commit object from its raw data content
pub fn parse_commit(oid: OID, data: &[u8]) -> Result<Commit, MuonGitError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| MuonGitError::InvalidObject("commit is not valid UTF-8".into()))?;

    let mut tree_id: Option<OID> = None;
    let mut parent_ids: Vec<OID> = Vec::new();
    let mut author: Option<Signature> = None;
    let mut committer: Option<Signature> = None;
    let mut message_encoding: Option<String> = None;

    // Split at first blank line
    let (header_section, message) = match text.find("\n\n") {
        Some(idx) => (&text[..idx], &text[idx + 2..]),
        None => (text, ""),
    };

    for line in header_section.split('\n') {
        if let Some(hex) = line.strip_prefix("tree ") {
            tree_id = Some(OID::from_hex(hex)?);
        } else if let Some(hex) = line.strip_prefix("parent ") {
            parent_ids.push(OID::from_hex(hex)?);
        } else if let Some(sig_str) = line.strip_prefix("author ") {
            author = Some(parse_signature(sig_str));
        } else if let Some(sig_str) = line.strip_prefix("committer ") {
            committer = Some(parse_signature(sig_str));
        } else if let Some(enc) = line.strip_prefix("encoding ") {
            message_encoding = Some(enc.to_string());
        }
    }

    Ok(Commit {
        oid,
        tree_id: tree_id
            .ok_or_else(|| MuonGitError::InvalidObject("commit missing tree".into()))?,
        parent_ids,
        author: author
            .ok_or_else(|| MuonGitError::InvalidObject("commit missing author".into()))?,
        committer: committer
            .ok_or_else(|| MuonGitError::InvalidObject("commit missing committer".into()))?,
        message: message.to_string(),
        message_encoding,
    })
}

/// Serialize a commit to its raw data representation (without the object header)
pub fn serialize_commit(
    tree_id: &OID,
    parent_ids: &[OID],
    author: &Signature,
    committer: &Signature,
    message: &str,
    message_encoding: Option<&str>,
) -> Vec<u8> {
    let mut buf = String::new();
    buf.push_str(&format!("tree {}\n", tree_id.hex()));
    for pid in parent_ids {
        buf.push_str(&format!("parent {}\n", pid.hex()));
    }
    buf.push_str(&format!("author {}\n", format_signature(author)));
    buf.push_str(&format!("committer {}\n", format_signature(committer)));
    if let Some(enc) = message_encoding {
        buf.push_str(&format!("encoding {}\n", enc));
    }
    buf.push('\n');
    buf.push_str(message);
    buf.into_bytes()
}

/// Parse "Name <email> timestamp offset" into a Signature
fn parse_signature(s: &str) -> Signature {
    let email_start = match s.find('<') {
        Some(i) => i,
        None => return Signature { name: s.to_string(), email: String::new(), time: 0, offset: 0 },
    };
    let email_end = match s.find('>') {
        Some(i) => i,
        None => return Signature { name: s.to_string(), email: String::new(), time: 0, offset: 0 },
    };

    let name = s[..email_start].trim().to_string();
    let email = s[email_start + 1..email_end].to_string();
    let remainder = s[email_end + 1..].trim();
    let parts: Vec<&str> = remainder.split_whitespace().collect();

    let time = parts.first().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    let offset = parts.get(1).map(|s| parse_timezone_offset(s)).unwrap_or(0);

    Signature { name, email, time, offset }
}

/// Format a Signature into "Name <email> timestamp offset"
pub fn format_signature(sig: &Signature) -> String {
    let sign = if sig.offset >= 0 { "+" } else { "-" };
    let abs = sig.offset.unsigned_abs();
    let hours = abs / 60;
    let minutes = abs % 60;
    format!("{} <{}> {} {}{:02}{:02}", sig.name, sig.email, sig.time, sign, hours, minutes)
}

/// Parse "+0530" or "-0800" into minutes offset
fn parse_timezone_offset(s: &str) -> i32 {
    if s.len() < 5 {
        return 0;
    }
    let sign: i32 = if s.starts_with('-') { -1 } else { 1 };
    let digits = &s[1..];
    if digits.len() != 4 {
        return 0;
    }
    let hours: i32 = digits[..2].parse().unwrap_or(0);
    let minutes: i32 = digits[2..].parse().unwrap_or(0);
    sign * (hours * 60 + minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_signature() {
        let sig = parse_signature("Test User <test@example.com> 1234567890 +0530");
        assert_eq!(sig.name, "Test User");
        assert_eq!(sig.email, "test@example.com");
        assert_eq!(sig.time, 1234567890);
        assert_eq!(sig.offset, 330); // 5*60+30
    }

    #[test]
    fn test_format_signature() {
        let sig = Signature {
            name: "Test".into(),
            email: "test@test.com".into(),
            time: 1000,
            offset: -480,
        };
        assert_eq!(format_signature(&sig), "Test <test@test.com> 1000 -0800");
    }

    #[test]
    fn test_parse_and_serialize_commit() {
        let tree = OID::from_hex("4b825dc642cb6eb9a060e54bf899d69f7cb46237").unwrap();
        let author = Signature {
            name: "Author".into(),
            email: "author@example.com".into(),
            time: 1234567890,
            offset: 0,
        };
        let committer = Signature {
            name: "Committer".into(),
            email: "committer@example.com".into(),
            time: 1234567890,
            offset: 0,
        };

        let data = serialize_commit(&tree, &[], &author, &committer, "Initial commit\n", None);
        let oid = OID::hash_object(ObjectType::Commit, &data);
        let commit = parse_commit(oid.clone(), &data).unwrap();

        assert_eq!(commit.tree_id, tree);
        assert!(commit.parent_ids.is_empty());
        assert_eq!(commit.author.name, "Author");
        assert_eq!(commit.committer.email, "committer@example.com");
        assert_eq!(commit.message, "Initial commit\n");
        assert!(commit.message_encoding.is_none());
    }

    #[test]
    fn test_parse_commit_with_parents() {
        let tree = OID::from_hex("4b825dc642cb6eb9a060e54bf899d69f7cb46237").unwrap();
        let parent1 = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let parent2 = OID::from_hex("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let sig = Signature {
            name: "Test".into(),
            email: "test@test.com".into(),
            time: 0,
            offset: 0,
        };

        let data = serialize_commit(&tree, &[parent1.clone(), parent2.clone()], &sig, &sig, "merge\n", None);
        let oid = OID::hash_object(ObjectType::Commit, &data);
        let commit = parse_commit(oid, &data).unwrap();

        assert_eq!(commit.parent_ids.len(), 2);
        assert_eq!(commit.parent_ids[0], parent1);
        assert_eq!(commit.parent_ids[1], parent2);
    }

    #[test]
    fn test_parse_commit_missing_tree() {
        let data = b"author Test <t@t.com> 0 +0000\ncommitter Test <t@t.com> 0 +0000\n\nmsg\n";
        let oid = OID::zero();
        assert!(parse_commit(oid, data).is_err());
    }

    #[test]
    fn test_commit_roundtrip_with_encoding() {
        let tree = OID::from_hex("4b825dc642cb6eb9a060e54bf899d69f7cb46237").unwrap();
        let sig = Signature {
            name: "Test".into(),
            email: "test@test.com".into(),
            time: 100,
            offset: 0,
        };
        let data = serialize_commit(&tree, &[], &sig, &sig, "msg\n", Some("UTF-8"));
        let oid = OID::hash_object(ObjectType::Commit, &data);
        let commit = parse_commit(oid, &data).unwrap();
        assert_eq!(commit.message_encoding.as_deref(), Some("UTF-8"));
    }
}
