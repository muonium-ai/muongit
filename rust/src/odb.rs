//! Loose object read/write for the git object database.
//! Parity: libgit2 src/libgit2/odb_loose.c

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::{MuonGitError, OID, ObjectType};
use crate::sha1::SHA1;

/// Parse a type string into an ObjectType.
fn parse_object_type(s: &str) -> Result<ObjectType, MuonGitError> {
    match s {
        "commit" => Ok(ObjectType::Commit),
        "tree" => Ok(ObjectType::Tree),
        "blob" => Ok(ObjectType::Blob),
        "tag" => Ok(ObjectType::Tag),
        _ => Err(MuonGitError::InvalidObject(format!("unknown object type: {}", s))),
    }
}

/// Return the type name string for an ObjectType.
fn type_name(obj_type: ObjectType) -> &'static str {
    match obj_type {
        ObjectType::Commit => "commit",
        ObjectType::Tree => "tree",
        ObjectType::Blob => "blob",
        ObjectType::Tag => "tag",
    }
}

/// Compute the loose object path: objects/{first 2 hex chars}/{remaining 38 hex chars}
fn loose_object_path(git_dir: &Path, oid: &OID) -> std::path::PathBuf {
    let hex = oid.hex();
    git_dir.join("objects").join(&hex[..2]).join(&hex[2..])
}

/// Read a loose object from the git object database.
///
/// The loose object is stored at `objects/{xx}/{rest}` inside the git directory,
/// zlib-compressed. The decompressed format is: `"{type} {size}\0{content}"`.
pub fn read_loose_object(git_dir: &Path, oid: &OID) -> Result<(ObjectType, Vec<u8>), MuonGitError> {
    let path = loose_object_path(git_dir, oid);
    let compressed = fs::read(&path).map_err(|_| {
        MuonGitError::NotFound(format!("loose object not found: {}", oid.hex()))
    })?;

    // Decompress
    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed).map_err(|e| {
        MuonGitError::InvalidObject(format!("failed to decompress object {}: {}", oid.hex(), e))
    })?;

    // Parse header: "{type} {size}\0{content}"
    let null_pos = decompressed.iter().position(|&b| b == 0).ok_or_else(|| {
        MuonGitError::InvalidObject(format!("no null byte in object header: {}", oid.hex()))
    })?;

    let header = std::str::from_utf8(&decompressed[..null_pos]).map_err(|_| {
        MuonGitError::InvalidObject(format!("invalid UTF-8 in object header: {}", oid.hex()))
    })?;

    let space_pos = header.find(' ').ok_or_else(|| {
        MuonGitError::InvalidObject(format!("no space in object header: {}", oid.hex()))
    })?;

    let obj_type_str = &header[..space_pos];
    let size_str = &header[space_pos + 1..];

    let obj_type = parse_object_type(obj_type_str)?;
    let size: usize = size_str.parse().map_err(|_| {
        MuonGitError::InvalidObject(format!("invalid size in object header: {}", oid.hex()))
    })?;

    let content = decompressed[null_pos + 1..].to_vec();
    if content.len() != size {
        return Err(MuonGitError::InvalidObject(format!(
            "object size mismatch: header says {} but got {}",
            size,
            content.len()
        )));
    }

    Ok((obj_type, content))
}

/// Write a loose object to the git object database.
///
/// Computes the SHA-1 of `"{type} {size}\0{data}"`, compresses the full
/// content with zlib, and writes it to `objects/{xx}/{rest}`.
pub fn write_loose_object(git_dir: &Path, obj_type: ObjectType, data: &[u8]) -> Result<OID, MuonGitError> {
    // Build the full object content: header + data
    let header = format!("{} {}\0", type_name(obj_type), data.len());
    let mut full_content = Vec::with_capacity(header.len() + data.len());
    full_content.extend_from_slice(header.as_bytes());
    full_content.extend_from_slice(data);

    // Compute SHA-1
    let digest = SHA1::hash(&full_content);
    let oid = OID::from_bytes(digest.to_vec());

    let path = loose_object_path(git_dir, &oid);

    // Don't overwrite if it already exists (objects are immutable)
    if path.exists() {
        return Ok(oid);
    }

    // Ensure the fan-out directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Compress with zlib
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&full_content).map_err(|e| {
        MuonGitError::Io(e)
    })?;
    let compressed = encoder.finish().map_err(|e| {
        MuonGitError::Io(e)
    })?;

    // Write atomically: write to temp file then rename
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, &compressed)?;
    fs::rename(&tmp_path, &path)?;

    Ok(oid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Repository;

    #[test]
    fn test_write_and_read_blob() {
        let tmp = std::env::temp_dir().join("muongit_test_odb_blob");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        let git_dir = repo.git_dir();

        let data = b"hello world\n";
        let oid = write_loose_object(git_dir, ObjectType::Blob, data).unwrap();

        // Verify the OID matches what hash_object produces
        let expected_oid = OID::hash_object(ObjectType::Blob, data);
        assert_eq!(oid.hex(), expected_oid.hex());

        // Read it back
        let (obj_type, content) = read_loose_object(git_dir, &oid).unwrap();
        assert_eq!(obj_type, ObjectType::Blob);
        assert_eq!(content, data);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_write_and_read_commit() {
        let tmp = std::env::temp_dir().join("muongit_test_odb_commit");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        let git_dir = repo.git_dir();

        let commit_data = b"tree 4b825dc642cb6eb9a060e54bf899d69f82b3e3b0\nauthor Test <test@test.com> 1000000000 +0000\ncommitter Test <test@test.com> 1000000000 +0000\n\nInitial commit\n";
        let oid = write_loose_object(git_dir, ObjectType::Commit, commit_data).unwrap();

        let (obj_type, content) = read_loose_object(git_dir, &oid).unwrap();
        assert_eq!(obj_type, ObjectType::Commit);
        assert_eq!(content, commit_data);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_write_idempotent() {
        let tmp = std::env::temp_dir().join("muongit_test_odb_idempotent");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        let git_dir = repo.git_dir();

        let data = b"same content";
        let oid1 = write_loose_object(git_dir, ObjectType::Blob, data).unwrap();
        let oid2 = write_loose_object(git_dir, ObjectType::Blob, data).unwrap();
        assert_eq!(oid1.hex(), oid2.hex());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_read_nonexistent() {
        let tmp = std::env::temp_dir().join("muongit_test_odb_noexist");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = Repository::init(tmp.to_string_lossy().into_owned(), false).unwrap();
        let git_dir = repo.git_dir();

        let fake_oid = OID::from_hex("0000000000000000000000000000000000000000").unwrap();
        let result = read_loose_object(git_dir, &fake_oid);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_loose_object_path() {
        let git_dir = Path::new("/repo/.git");
        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let path = super::loose_object_path(git_dir, &oid);
        assert_eq!(
            path.to_string_lossy(),
            "/repo/.git/objects/aa/f4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        );
    }
}
