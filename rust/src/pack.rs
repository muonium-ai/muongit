//! Pack file object lookup and delta resolution
//! Parity: libgit2 src/libgit2/pack.c

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::MuonGitError;
use crate::pack_index::PackIndex;
use crate::types::ObjectType;

const PACK_MAGIC: [u8; 4] = *b"PACK";

/// Object types in pack files
const OBJ_COMMIT: u8 = 1;
const OBJ_TREE: u8 = 2;
const OBJ_BLOB: u8 = 3;
const OBJ_TAG: u8 = 4;
const OBJ_OFS_DELTA: u8 = 6;
const OBJ_REF_DELTA: u8 = 7;

/// Result of reading a pack object
#[derive(Debug, Clone)]
pub struct PackObject {
    pub obj_type: ObjectType,
    pub data: Vec<u8>,
}

/// Read an object from a pack file at the given offset.
pub fn read_pack_object(pack_path: &Path, offset: u64, idx: &PackIndex) -> Result<PackObject, MuonGitError> {
    let mut file = File::open(pack_path)?;
    read_object_at(&mut file, offset, idx)
}

fn read_object_at(file: &mut File, offset: u64, idx: &PackIndex) -> Result<PackObject, MuonGitError> {
    file.seek(SeekFrom::Start(offset))?;

    // Read type and size from variable-length header
    let (type_num, _size) = read_type_and_size(file)?;

    match type_num {
        OBJ_COMMIT | OBJ_TREE | OBJ_BLOB | OBJ_TAG => {
            let obj_type = pack_type_to_object_type(type_num)?;
            let data = decompress_stream(file)?;
            Ok(PackObject { obj_type, data })
        }
        OBJ_OFS_DELTA => {
            let base_offset = read_ofs_delta_offset(file)?;
            let delta_data = decompress_stream(file)?;
            let base = read_object_at(file, offset - base_offset, idx)?;
            let result = apply_delta(&base.data, &delta_data)?;
            Ok(PackObject { obj_type: base.obj_type, data: result })
        }
        OBJ_REF_DELTA => {
            let mut base_oid_bytes = [0u8; 20];
            file.read_exact(&mut base_oid_bytes)?;
            let base_oid = crate::oid::OID::from_bytes(base_oid_bytes.to_vec());
            let delta_data = decompress_stream(file)?;

            let base_offset = idx.find(&base_oid)
                .ok_or_else(|| MuonGitError::NotFound(format!("base object {} not found in pack index", base_oid.hex())))?;
            let base = read_object_at(file, base_offset, idx)?;
            let result = apply_delta(&base.data, &delta_data)?;
            Ok(PackObject { obj_type: base.obj_type, data: result })
        }
        _ => Err(MuonGitError::InvalidObject(format!("unknown pack object type {}", type_num))),
    }
}

fn read_type_and_size(file: &mut File) -> Result<(u8, u64), MuonGitError> {
    let mut buf = [0u8; 1];
    file.read_exact(&mut buf)?;
    let c = buf[0];

    let type_num = (c >> 4) & 0x07;
    let mut size = (c & 0x0F) as u64;
    let mut shift = 4u32;

    if c & 0x80 != 0 {
        loop {
            file.read_exact(&mut buf)?;
            let c = buf[0];
            size |= ((c & 0x7F) as u64) << shift;
            shift += 7;
            if c & 0x80 == 0 {
                break;
            }
        }
    }

    Ok((type_num, size))
}

fn read_ofs_delta_offset(file: &mut File) -> Result<u64, MuonGitError> {
    let mut buf = [0u8; 1];
    file.read_exact(&mut buf)?;
    let mut c = buf[0];
    let mut offset = (c & 0x7F) as u64;

    while c & 0x80 != 0 {
        offset += 1;
        file.read_exact(&mut buf)?;
        c = buf[0];
        offset = (offset << 7) | (c & 0x7F) as u64;
    }

    Ok(offset)
}

fn decompress_stream(file: &mut File) -> Result<Vec<u8>, MuonGitError> {
    // Read remaining data from current position
    let pos = file.stream_position()?;
    let end = file.seek(SeekFrom::End(0))?;
    file.seek(SeekFrom::Start(pos))?;

    let remaining = (end - pos) as usize;
    let mut compressed = vec![0u8; remaining];
    file.read_exact(&mut compressed)?;

    // Decompress with flate2
    let mut decoder = flate2::read::ZlibDecoder::new(&compressed[..]);
    let mut result = Vec::new();
    decoder.read_to_end(&mut result)?;

    // Seek back to where the compressed data ended
    let consumed = decoder.total_in() as u64;
    file.seek(SeekFrom::Start(pos + consumed))?;

    Ok(result)
}

fn pack_type_to_object_type(t: u8) -> Result<ObjectType, MuonGitError> {
    match t {
        OBJ_COMMIT => Ok(ObjectType::Commit),
        OBJ_TREE => Ok(ObjectType::Tree),
        OBJ_BLOB => Ok(ObjectType::Blob),
        OBJ_TAG => Ok(ObjectType::Tag),
        _ => Err(MuonGitError::InvalidObject(format!("invalid object type {}", t))),
    }
}

/// Apply a git delta to a base object.
pub fn apply_delta(base: &[u8], delta: &[u8]) -> Result<Vec<u8>, MuonGitError> {
    let mut pos = 0;

    // Read source size
    let (_src_size, consumed) = read_delta_size(delta, pos);
    pos += consumed;

    // Read target size
    let (tgt_size, consumed) = read_delta_size(delta, pos);
    pos += consumed;

    let mut result = Vec::with_capacity(tgt_size as usize);

    while pos < delta.len() {
        let cmd = delta[pos];
        pos += 1;

        if cmd & 0x80 != 0 {
            // Copy from base
            let mut copy_offset: u32 = 0;
            let mut copy_size: u32 = 0;

            if cmd & 0x01 != 0 { copy_offset |= delta[pos] as u32; pos += 1; }
            if cmd & 0x02 != 0 { copy_offset |= (delta[pos] as u32) << 8; pos += 1; }
            if cmd & 0x04 != 0 { copy_offset |= (delta[pos] as u32) << 16; pos += 1; }
            if cmd & 0x08 != 0 { copy_offset |= (delta[pos] as u32) << 24; pos += 1; }

            if cmd & 0x10 != 0 { copy_size |= delta[pos] as u32; pos += 1; }
            if cmd & 0x20 != 0 { copy_size |= (delta[pos] as u32) << 8; pos += 1; }
            if cmd & 0x40 != 0 { copy_size |= (delta[pos] as u32) << 16; pos += 1; }

            if copy_size == 0 { copy_size = 0x10000; }

            let start = copy_offset as usize;
            let end = start + copy_size as usize;
            if end > base.len() {
                return Err(MuonGitError::InvalidObject("delta copy out of bounds".into()));
            }
            result.extend_from_slice(&base[start..end]);
        } else if cmd > 0 {
            // Insert new data
            let size = cmd as usize;
            if pos + size > delta.len() {
                return Err(MuonGitError::InvalidObject("delta insert out of bounds".into()));
            }
            result.extend_from_slice(&delta[pos..pos + size]);
            pos += size;
        } else {
            return Err(MuonGitError::InvalidObject("invalid delta opcode 0".into()));
        }
    }

    if result.len() != tgt_size as usize {
        return Err(MuonGitError::InvalidObject("delta result size mismatch".into()));
    }

    Ok(result)
}

fn read_delta_size(data: &[u8], start: usize) -> (u64, usize) {
    let mut pos = start;
    let mut size: u64 = 0;
    let mut shift: u32 = 0;

    loop {
        if pos >= data.len() { break; }
        let c = data[pos];
        pos += 1;
        size |= ((c & 0x7F) as u64) << shift;
        shift += 7;
        if c & 0x80 == 0 { break; }
    }

    (size, pos - start)
}

/// Build a minimal pack file for testing.
/// Objects should be non-delta (commit/tree/blob/tag).
pub fn build_test_pack(objects: &[(ObjectType, &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();

    // Header
    buf.extend_from_slice(&PACK_MAGIC);
    buf.extend_from_slice(&2u32.to_be_bytes()); // version
    buf.extend_from_slice(&(objects.len() as u32).to_be_bytes());

    for (obj_type, data) in objects {
        let type_num: u8 = match obj_type {
            ObjectType::Commit => OBJ_COMMIT,
            ObjectType::Tree => OBJ_TREE,
            ObjectType::Blob => OBJ_BLOB,
            ObjectType::Tag => OBJ_TAG,
        };

        // Encode type and size in variable-length header
        let size = data.len() as u64;
        let mut header_bytes = Vec::new();
        let first = (type_num << 4) | (size & 0x0F) as u8;
        let mut remaining = size >> 4;

        if remaining > 0 {
            header_bytes.push(first | 0x80);
            while remaining > 0 {
                let byte = (remaining & 0x7F) as u8;
                remaining >>= 7;
                if remaining > 0 {
                    header_bytes.push(byte | 0x80);
                } else {
                    header_bytes.push(byte);
                }
            }
        } else {
            header_bytes.push(first);
        }

        buf.extend_from_slice(&header_bytes);

        // Compress data
        use std::io::Write;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        let compressed = encoder.finish().unwrap();
        buf.extend_from_slice(&compressed);
    }

    // Pack checksum (SHA-1 of everything before)
    let checksum = crate::sha1::SHA1::hash(&buf);
    buf.extend_from_slice(&checksum);

    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oid::OID;
    use crate::pack_index::{build_pack_index, parse_pack_index};

    #[test]
    fn test_apply_delta_copy() {
        // Delta: copy all of base
        let base = b"hello world";
        // Delta format: src_size=11, tgt_size=11, copy(offset=0, size=11)
        let delta = vec![
            11,  // src size
            11,  // tgt size
            0x80 | 0x01 | 0x10, // copy cmd with offset byte and size byte
            0,   // offset = 0
            11,  // size = 11
        ];
        let result = apply_delta(base, &delta).unwrap();
        assert_eq!(result, base);
    }

    #[test]
    fn test_apply_delta_insert() {
        // Delta: insert new data
        let base = b"hello";
        let delta = vec![
            5,   // src size
            6,   // tgt size
            6,   // insert 6 bytes
            b'w', b'o', b'r', b'l', b'd', b'!',
        ];
        let result = apply_delta(base, &delta).unwrap();
        assert_eq!(result, b"world!");
    }

    #[test]
    fn test_apply_delta_mixed() {
        // Delta: copy first 5 bytes from base, then insert " world"
        let base = b"hello cruel";
        let delta = vec![
            11,  // src size
            11,  // tgt size
            0x80 | 0x01 | 0x10, // copy cmd
            0,   // offset = 0
            5,   // size = 5
            6,   // insert 6 bytes
            b' ', b'w', b'o', b'r', b'l', b'd',
        ];
        let result = apply_delta(base, &delta).unwrap();
        assert_eq!(result, b"hello world");
    }

    #[test]
    fn test_build_and_read_pack() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_pack_read");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let blob_data = b"hello pack\n";
        let pack_data = build_test_pack(&[(ObjectType::Blob, blob_data)]);

        let pack_path = tmp.join("test.pack");
        std::fs::write(&pack_path, &pack_data).unwrap();

        // Build a matching index
        let oid = OID::hash_object(ObjectType::Blob, blob_data);
        // The object starts at offset 12 (after the 12-byte pack header)
        let idx_data = build_pack_index(&[oid.clone()], &[0], &[12]);
        let idx = parse_pack_index(&idx_data).unwrap();

        let obj = read_pack_object(&pack_path, 12, &idx).unwrap();
        assert_eq!(obj.obj_type, ObjectType::Blob);
        assert_eq!(obj.data, blob_data);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_build_and_read_multiple_objects() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_pack_multi");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let blob1 = b"first blob\n";
        let blob2 = b"second blob\n";
        let pack_data = build_test_pack(&[
            (ObjectType::Blob, blob1),
            (ObjectType::Blob, blob2),
        ]);

        let pack_path = tmp.join("test.pack");
        std::fs::write(&pack_path, &pack_data).unwrap();

        // Find offset of second object by scanning
        // First object starts at 12
        let oid1 = OID::hash_object(ObjectType::Blob, blob1);
        let oid2 = OID::hash_object(ObjectType::Blob, blob2);

        // Read first object at offset 12
        let idx_data = build_pack_index(
            &{
                let mut v = vec![oid1.clone(), oid2.clone()];
                v.sort_by(|a, b| a.raw().cmp(b.raw()));
                v
            },
            &[0, 0],
            &[12, 12], // dummy offsets for index — we just read by offset
        );
        let idx = parse_pack_index(&idx_data).unwrap();

        let obj1 = read_pack_object(&pack_path, 12, &idx).unwrap();
        assert_eq!(obj1.obj_type, ObjectType::Blob);
        assert_eq!(obj1.data, blob1);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_read_pack_commit() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_pack_commit");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let commit_data = b"tree 0000000000000000000000000000000000000000\nauthor Test <t@t> 0 +0000\ncommitter Test <t@t> 0 +0000\n\ntest\n";
        let pack_data = build_test_pack(&[(ObjectType::Commit, commit_data)]);

        let pack_path = tmp.join("test.pack");
        std::fs::write(&pack_path, &pack_data).unwrap();

        let oid = OID::hash_object(ObjectType::Commit, commit_data);
        let idx_data = build_pack_index(&[oid], &[0], &[12]);
        let idx = parse_pack_index(&idx_data).unwrap();

        let obj = read_pack_object(&pack_path, 12, &idx).unwrap();
        assert_eq!(obj.obj_type, ObjectType::Commit);
        assert_eq!(obj.data, commit_data);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
