//! Generic object lookup and peeling.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::blob::Blob;
use crate::commit::{parse_commit, Commit};
use crate::error::MuonGitError;
use crate::oid::OID;
use crate::pack::read_pack_object;
use crate::pack_index::read_pack_index;
use crate::repository::Repository;
use crate::tag::{parse_tag, Tag};
use crate::tree::{parse_tree, Tree};
use crate::types::ObjectType;

/// A generic git object loaded from the object database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitObject {
    pub oid: OID,
    pub obj_type: ObjectType,
    pub data: Vec<u8>,
    pub size: usize,
}

impl GitObject {
    fn new(oid: OID, obj_type: ObjectType, data: Vec<u8>) -> Self {
        let size = data.len();
        Self {
            oid,
            obj_type,
            data,
            size,
        }
    }

    pub fn as_blob(&self) -> Result<Blob, MuonGitError> {
        if self.obj_type != ObjectType::Blob {
            return Err(MuonGitError::InvalidObject(format!(
                "expected blob, got {:?}",
                self.obj_type
            )));
        }
        Ok(Blob {
            oid: self.oid.clone(),
            data: self.data.clone(),
            size: self.size,
        })
    }

    pub fn as_commit(&self) -> Result<Commit, MuonGitError> {
        if self.obj_type != ObjectType::Commit {
            return Err(MuonGitError::InvalidObject(format!(
                "expected commit, got {:?}",
                self.obj_type
            )));
        }
        parse_commit(self.oid.clone(), &self.data)
    }

    pub fn as_tree(&self) -> Result<Tree, MuonGitError> {
        if self.obj_type != ObjectType::Tree {
            return Err(MuonGitError::InvalidObject(format!(
                "expected tree, got {:?}",
                self.obj_type
            )));
        }
        parse_tree(self.oid.clone(), &self.data)
    }

    pub fn as_tag(&self) -> Result<Tag, MuonGitError> {
        if self.obj_type != ObjectType::Tag {
            return Err(MuonGitError::InvalidObject(format!(
                "expected tag, got {:?}",
                self.obj_type
            )));
        }
        parse_tag(self.oid.clone(), &self.data)
    }

    pub fn peel(&self, git_dir: &Path) -> Result<GitObject, MuonGitError> {
        let mut current = self.clone();
        let mut seen = HashSet::from([current.oid.clone()]);

        while current.obj_type == ObjectType::Tag {
            let tag = current.as_tag()?;
            if !seen.insert(tag.target_id.clone()) {
                return Err(MuonGitError::InvalidObject(
                    "tag peel cycle detected".into(),
                ));
            }
            current = read_object(git_dir, &tag.target_id)?;
        }

        Ok(current)
    }
}

/// Read a generic object by OID from loose or packed storage.
pub fn read_object(git_dir: &Path, oid: &OID) -> Result<GitObject, MuonGitError> {
    match crate::odb::read_loose_object(git_dir, oid) {
        Ok((obj_type, data)) => Ok(GitObject::new(oid.clone(), obj_type, data)),
        Err(MuonGitError::NotFound(_)) => read_packed_object(git_dir, oid),
        Err(err) => Err(err),
    }
}

impl Repository {
    /// Read a generic object by OID from this repository.
    pub fn read_object(&self, oid: &OID) -> Result<GitObject, MuonGitError> {
        read_object(self.git_dir(), oid)
    }
}

fn read_packed_object(git_dir: &Path, oid: &OID) -> Result<GitObject, MuonGitError> {
    let pack_dir = git_dir.join("objects").join("pack");
    let entries = match fs::read_dir(&pack_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(MuonGitError::NotFound(format!(
                "object not found: {}",
                oid.hex()
            )));
        }
        Err(err) => return Err(MuonGitError::Io(err)),
    };

    let mut index_paths = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("idx") {
            index_paths.push(path);
        }
    }
    index_paths.sort();

    for idx_path in index_paths {
        let idx = read_pack_index(&idx_path)?;
        let Some(offset) = idx.find(oid) else {
            continue;
        };

        let pack_path = idx_path.with_extension("pack");
        let obj = read_pack_object(&pack_path, offset, &idx)?;
        return Ok(GitObject::new(oid.clone(), obj.obj_type, obj.data));
    }

    Err(MuonGitError::NotFound(format!(
        "object not found: {}",
        oid.hex()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp").join(name)
    }

    #[test]
    fn test_read_loose_object_and_convert_to_blob() {
        let tmp = test_dir("test_object_loose_lookup");
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let blob_data = b"object api loose blob\n";
        let blob_oid = crate::blob::write_blob(repo.git_dir(), blob_data).unwrap();

        let obj = repo.read_object(&blob_oid).unwrap();
        assert_eq!(obj.oid, blob_oid);
        assert_eq!(obj.obj_type, ObjectType::Blob);
        assert_eq!(obj.size, blob_data.len());

        let blob = obj.as_blob().unwrap();
        assert_eq!(blob.data, blob_data);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_read_packed_object_by_oid() {
        let tmp = test_dir("test_object_pack_lookup");
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let pack_dir = repo.git_dir().join("objects").join("pack");
        fs::create_dir_all(&pack_dir).unwrap();

        let blob_data = b"packed object payload\n";
        let blob_oid = OID::hash_object(ObjectType::Blob, blob_data);
        let pack_data = crate::pack::build_test_pack(&[(ObjectType::Blob, blob_data)]);
        let idx_data =
            crate::pack_index::build_pack_index(&[blob_oid.clone()], &[0], &[12]);

        fs::write(pack_dir.join("test.pack"), pack_data).unwrap();
        fs::write(pack_dir.join("test.idx"), idx_data).unwrap();

        let obj = read_object(repo.git_dir(), &blob_oid).unwrap();
        assert_eq!(obj.obj_type, ObjectType::Blob);
        assert_eq!(obj.size, blob_data.len());
        assert_eq!(obj.data, blob_data);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_peel_tag_to_target_object() {
        let tmp = test_dir("test_object_peel_tag");
        let _ = fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let blob_data = b"peeled blob\n";
        let blob_oid = crate::blob::write_blob(repo.git_dir(), blob_data).unwrap();
        let tag_data = crate::tag::serialize_tag(
            &blob_oid,
            ObjectType::Blob,
            "v1.0",
            None,
            "annotated blob tag\n",
        );
        let tag_oid =
            crate::odb::write_loose_object(repo.git_dir(), ObjectType::Tag, &tag_data).unwrap();

        let tag_object = read_object(repo.git_dir(), &tag_oid).unwrap();
        let tag = tag_object.as_tag().unwrap();
        assert_eq!(tag.target_id, blob_oid);
        assert_eq!(tag.target_type, ObjectType::Blob);

        let peeled = tag_object.peel(repo.git_dir()).unwrap();
        assert_eq!(peeled.oid, blob_oid);
        assert_eq!(peeled.obj_type, ObjectType::Blob);
        assert_eq!(peeled.data, blob_data);

        let _ = fs::remove_dir_all(&tmp);
    }
}
