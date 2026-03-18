//! Blob object read/write

use std::path::Path;

use crate::error::MuonGitError;
use crate::oid::OID;
use crate::types::ObjectType;

/// A parsed git blob object
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob {
    pub oid: OID,
    pub data: Vec<u8>,
    pub size: usize,
}

/// Read a blob from the object database
pub fn read_blob(git_dir: &Path, oid: &OID) -> Result<Blob, MuonGitError> {
    let (obj_type, data) = crate::odb::read_loose_object(git_dir, oid)?;
    if obj_type != ObjectType::Blob {
        return Err(MuonGitError::InvalidObject(format!(
            "expected blob, got {:?}",
            obj_type
        )));
    }
    let size = data.len();
    Ok(Blob {
        oid: oid.clone(),
        data,
        size,
    })
}

/// Write data as a blob to the object database, returns the OID
pub fn write_blob(git_dir: &Path, data: &[u8]) -> Result<OID, MuonGitError> {
    crate::odb::write_loose_object(git_dir, ObjectType::Blob, data)
}

/// Write a file's contents as a blob to the object database
pub fn write_blob_from_file(git_dir: &Path, file_path: &Path) -> Result<OID, MuonGitError> {
    let data = std::fs::read(file_path)?;
    write_blob(git_dir, &data)
}

/// Compute the blob OID for data without writing to the ODB (hash-object --stdin)
pub fn hash_blob(data: &[u8]) -> OID {
    OID::hash_object(ObjectType::Blob, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_blob() {
        let oid = hash_blob(b"hello\n");
        assert_eq!(oid.hex(), "ce013625030ba8dba906f756967f9e9ca394464a");
    }

    #[test]
    fn test_hash_blob_empty() {
        let oid = hash_blob(b"");
        assert_eq!(oid.hex(), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    }

    #[test]
    fn test_write_and_read_blob() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_blob_rw");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let content = b"blob content\n";
        let oid = write_blob(repo.git_dir(), content).unwrap();
        let blob = read_blob(repo.git_dir(), &oid).unwrap();

        assert_eq!(blob.data, content);
        assert_eq!(blob.size, content.len());
        assert_eq!(blob.oid, oid);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_write_blob_from_file() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_blob_file");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let file_path = tmp.join("test.txt");
        std::fs::write(&file_path, b"file content\n").unwrap();

        let oid = write_blob_from_file(repo.git_dir(), &file_path).unwrap();
        let expected = hash_blob(b"file content\n");
        assert_eq!(oid, expected);

        let blob = read_blob(repo.git_dir(), &oid).unwrap();
        assert_eq!(blob.data, b"file content\n");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_read_non_blob_type_errors() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_blob_type_err");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = crate::repository::Repository::init(tmp.to_str().unwrap(), false).unwrap();

        // Write a commit object
        let data = b"tree 0000000000000000000000000000000000000000\nauthor T <t@t> 0 +0000\ncommitter T <t@t> 0 +0000\n\nm\n";
        let oid = crate::odb::write_loose_object(repo.git_dir(), ObjectType::Commit, data).unwrap();

        // Try reading it as a blob
        let result = read_blob(repo.git_dir(), &oid);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
