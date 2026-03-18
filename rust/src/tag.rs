//! Tag object read/write

use crate::error::MuonGitError;
use crate::oid::OID;
use crate::types::{ObjectType, Signature};

/// A parsed git annotated tag object
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub oid: OID,
    pub target_id: OID,
    pub target_type: ObjectType,
    pub tag_name: String,
    pub tagger: Option<Signature>,
    pub message: String,
}

/// Parse a tag object from its raw data content
pub fn parse_tag(oid: OID, data: &[u8]) -> Result<Tag, MuonGitError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| MuonGitError::InvalidObject("tag is not valid UTF-8".into()))?;

    let mut target_id: Option<OID> = None;
    let mut target_type: Option<ObjectType> = None;
    let mut tag_name: Option<String> = None;
    let mut tagger: Option<Signature> = None;

    let (header_section, message) = match text.find("\n\n") {
        Some(idx) => (&text[..idx], &text[idx + 2..]),
        None => (text, ""),
    };

    for line in header_section.split('\n') {
        if let Some(hex) = line.strip_prefix("object ") {
            target_id = Some(OID::from_hex(hex)?);
        } else if let Some(type_name) = line.strip_prefix("type ") {
            target_type = Some(parse_object_type_name(type_name)?);
        } else if let Some(name) = line.strip_prefix("tag ") {
            tag_name = Some(name.to_string());
        } else if let Some(sig_str) = line.strip_prefix("tagger ") {
            tagger = Some(parse_signature_line(sig_str));
        }
    }

    Ok(Tag {
        oid,
        target_id: target_id
            .ok_or_else(|| MuonGitError::InvalidObject("tag missing object".into()))?,
        target_type: target_type
            .ok_or_else(|| MuonGitError::InvalidObject("tag missing type".into()))?,
        tag_name: tag_name
            .ok_or_else(|| MuonGitError::InvalidObject("tag missing tag name".into()))?,
        tagger,
        message: message.to_string(),
    })
}

/// Serialize a tag to its raw data representation (without the object header)
pub fn serialize_tag(
    target_id: &OID,
    target_type: ObjectType,
    tag_name: &str,
    tagger: Option<&Signature>,
    message: &str,
) -> Vec<u8> {
    let mut buf = String::new();
    buf.push_str(&format!("object {}\n", target_id.hex()));
    buf.push_str(&format!("type {}\n", object_type_name(target_type)));
    buf.push_str(&format!("tag {}\n", tag_name));
    if let Some(tagger) = tagger {
        buf.push_str(&format!("tagger {}\n", format_signature_line(tagger)));
    }
    buf.push('\n');
    buf.push_str(message);
    buf.into_bytes()
}

fn object_type_name(t: ObjectType) -> &'static str {
    match t {
        ObjectType::Commit => "commit",
        ObjectType::Tree => "tree",
        ObjectType::Blob => "blob",
        ObjectType::Tag => "tag",
    }
}

fn parse_object_type_name(name: &str) -> Result<ObjectType, MuonGitError> {
    match name {
        "commit" => Ok(ObjectType::Commit),
        "tree" => Ok(ObjectType::Tree),
        "blob" => Ok(ObjectType::Blob),
        "tag" => Ok(ObjectType::Tag),
        _ => Err(MuonGitError::InvalidObject(format!(
            "unknown object type '{}'",
            name
        ))),
    }
}

/// Parse "Name <email> timestamp offset" into a Signature
fn parse_signature_line(s: &str) -> Signature {
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
    let offset = parts.get(1).map(|s| parse_tz_offset(s)).unwrap_or(0);

    Signature { name, email, time, offset }
}

fn format_signature_line(sig: &Signature) -> String {
    let sign = if sig.offset >= 0 { "+" } else { "-" };
    let abs = sig.offset.unsigned_abs();
    let hours = abs / 60;
    let minutes = abs % 60;
    format!("{} <{}> {} {}{:02}{:02}", sig.name, sig.email, sig.time, sign, hours, minutes)
}

fn parse_tz_offset(s: &str) -> i32 {
    if s.len() < 5 { return 0; }
    let sign: i32 = if s.starts_with('-') { -1 } else { 1 };
    let digits = &s[1..];
    if digits.len() != 4 { return 0; }
    let hours: i32 = digits[..2].parse().unwrap_or(0);
    let minutes: i32 = digits[2..].parse().unwrap_or(0);
    sign * (hours * 60 + minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_serialize_tag() {
        let target = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let tagger = Signature {
            name: "Tagger".into(),
            email: "tagger@example.com".into(),
            time: 1234567890,
            offset: 0,
        };

        let data = serialize_tag(&target, ObjectType::Commit, "v1.0", Some(&tagger), "Release v1.0\n");
        let oid = OID::hash_object(ObjectType::Tag, &data);
        let tag = parse_tag(oid, &data).unwrap();

        assert_eq!(tag.target_id, target);
        assert_eq!(tag.target_type, ObjectType::Commit);
        assert_eq!(tag.tag_name, "v1.0");
        assert_eq!(tag.tagger.as_ref().unwrap().name, "Tagger");
        assert_eq!(tag.message, "Release v1.0\n");
    }

    #[test]
    fn test_tag_without_tagger() {
        let target = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let data = serialize_tag(&target, ObjectType::Commit, "v0.1", None, "lightweight\n");
        let oid = OID::hash_object(ObjectType::Tag, &data);
        let tag = parse_tag(oid, &data).unwrap();

        assert!(tag.tagger.is_none());
        assert_eq!(tag.tag_name, "v0.1");
    }

    #[test]
    fn test_tag_missing_object() {
        let data = b"type commit\ntag v1\n\nmsg\n";
        let oid = OID::zero();
        assert!(parse_tag(oid, data).is_err());
    }

    #[test]
    fn test_tag_targeting_tree() {
        let target = OID::from_hex("4b825dc642cb6eb9a060e54bf899d69f7cb46237").unwrap();
        let data = serialize_tag(&target, ObjectType::Tree, "tree-tag", None, "tag a tree\n");
        let oid = OID::hash_object(ObjectType::Tag, &data);
        let tag = parse_tag(oid, &data).unwrap();

        assert_eq!(tag.target_type, ObjectType::Tree);
    }

    #[test]
    fn test_tag_odb_roundtrip() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_tag_odb");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let target = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let tagger = Signature { name: "T".into(), email: "t@t.com".into(), time: 100, offset: 0 };
        let tag_data = serialize_tag(&target, ObjectType::Commit, "v1.0", Some(&tagger), "msg\n");
        let oid = crate::odb::write_loose_object(repo.git_dir(), ObjectType::Tag, &tag_data).unwrap();

        let (read_type, read_data) = crate::odb::read_loose_object(repo.git_dir(), &oid).unwrap();
        assert_eq!(read_type, ObjectType::Tag);

        let tag = parse_tag(oid, &read_data).unwrap();
        assert_eq!(tag.tag_name, "v1.0");
        assert_eq!(tag.target_id, target);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
