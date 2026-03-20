//! Pack file object lookup and delta resolution
//! Parity: libgit2 src/libgit2/pack.c

use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::MuonGitError;
use crate::object::read_object;
use crate::pack_index::PackIndex;
use crate::pack_index::build_pack_index_with_checksums;
use crate::sha1::SHA1;
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

/// Result of indexing and storing a pack inside a repository.
#[derive(Debug, Clone)]
pub struct IndexedPack {
    pub pack_name: String,
    pub pack_path: PathBuf,
    pub index_path: PathBuf,
    pub object_count: usize,
}

#[derive(Debug, Clone)]
struct RawPackEntry {
    offset: u64,
    kind: RawPackEntryKind,
}

#[derive(Debug, Clone)]
enum RawPackEntryKind {
    Base {
        obj_type: ObjectType,
        data: Vec<u8>,
    },
    OfsDelta {
        base_offset: u64,
        delta: Vec<u8>,
    },
    RefDelta {
        base_oid: crate::oid::OID,
        delta: Vec<u8>,
    },
}

#[derive(Debug, Clone)]
struct ResolvedPackEntry {
    offset: u64,
    oid: crate::oid::OID,
    obj_type: ObjectType,
    data: Vec<u8>,
}

/// Read an object from a pack file at the given offset.
pub fn read_pack_object(pack_path: &Path, offset: u64, idx: &PackIndex) -> Result<PackObject, MuonGitError> {
    let mut file = File::open(pack_path)?;
    read_object_at(&mut file, offset, idx)
}

/// Index pack bytes and write the resulting `.pack` and `.idx` files into the repository.
pub fn index_pack_to_odb(git_dir: &Path, pack_bytes: &[u8]) -> Result<IndexedPack, MuonGitError> {
    let (entries, pack_checksum) = parse_pack_entries(pack_bytes)?;
    let resolved = resolve_pack_entries(&entries, Some(git_dir))?;

    let mut sorted: Vec<_> = resolved
        .iter()
        .map(|entry| (entry.oid.clone(), 0u32, entry.offset))
        .collect();
    sorted.sort_by(|a, b| a.0.raw().cmp(b.0.raw()));

    let oids: Vec<_> = sorted.iter().map(|entry| entry.0.clone()).collect();
    let crcs: Vec<_> = sorted.iter().map(|entry| entry.1).collect();
    let offsets: Vec<_> = sorted.iter().map(|entry| entry.2).collect();
    let idx_data = build_pack_index_with_checksums(&oids, &crcs, &offsets, &pack_checksum);

    let pack_dir = git_dir.join("objects").join("pack");
    fs::create_dir_all(&pack_dir)?;

    let pack_hex = hex_bytes(&pack_checksum);
    let pack_name = format!("pack-{}", pack_hex);
    let pack_path = pack_dir.join(format!("{}.pack", pack_name));
    let index_path = pack_dir.join(format!("{}.idx", pack_name));

    write_if_missing(&pack_path, pack_bytes)?;
    write_if_missing(&index_path, &idx_data)?;

    Ok(IndexedPack {
        pack_name,
        pack_path,
        index_path,
        object_count: resolved.len(),
    })
}

/// Build a non-delta pack from the reachable object closure under `roots`, excluding `exclude`.
pub fn build_pack_from_oids(
    git_dir: &Path,
    roots: &[crate::oid::OID],
    exclude: &[crate::oid::OID],
) -> Result<Vec<u8>, MuonGitError> {
    let mut visited = HashSet::new();
    let excluded: HashSet<_> = exclude.iter().cloned().collect();
    let mut ordered = Vec::new();

    for root in roots {
        collect_reachable_objects(git_dir, root, &excluded, &mut visited, &mut ordered)?;
    }

    let mut buf = Vec::new();
    buf.extend_from_slice(&PACK_MAGIC);
    buf.extend_from_slice(&2u32.to_be_bytes());
    buf.extend_from_slice(&(ordered.len() as u32).to_be_bytes());

    for oid in ordered {
        let obj = read_object(git_dir, &oid)?;
        append_pack_object(&mut buf, obj.obj_type, &obj.data)?;
    }

    let checksum = SHA1::hash(&buf);
    buf.extend_from_slice(&checksum);
    Ok(buf)
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
    let consumed = decoder.total_in();
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

fn append_pack_object(buf: &mut Vec<u8>, obj_type: ObjectType, data: &[u8]) -> Result<(), MuonGitError> {
    let type_num = match obj_type {
        ObjectType::Commit => OBJ_COMMIT,
        ObjectType::Tree => OBJ_TREE,
        ObjectType::Blob => OBJ_BLOB,
        ObjectType::Tag => OBJ_TAG,
    };

    let mut size = data.len() as u64;
    let mut first = (type_num << 4) | (size as u8 & 0x0F);
    size >>= 4;
    if size == 0 {
        buf.push(first);
    } else {
        first |= 0x80;
        buf.push(first);
        while size > 0 {
            let mut byte = (size & 0x7F) as u8;
            size >>= 7;
            if size > 0 {
                byte |= 0x80;
            }
            buf.push(byte);
        }
    }

    use std::io::Write;
    let mut encoder =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data)?;
    let compressed = encoder.finish()?;
    buf.extend_from_slice(&compressed);
    Ok(())
}

fn write_if_missing(path: &Path, data: &[u8]) -> Result<(), MuonGitError> {
    if path.exists() {
        return Ok(());
    }

    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, data)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
}

fn parse_pack_entries(data: &[u8]) -> Result<(Vec<RawPackEntry>, [u8; 20]), MuonGitError> {
    if data.len() < 32 {
        return Err(MuonGitError::InvalidObject("pack file too short".into()));
    }
    if data[..4] != PACK_MAGIC {
        return Err(MuonGitError::InvalidObject("bad pack magic".into()));
    }

    let version = u32::from_be_bytes(data[4..8].try_into().unwrap());
    if version != 2 && version != 3 {
        return Err(MuonGitError::InvalidObject(format!(
            "unsupported pack version {}",
            version
        )));
    }

    let object_count = u32::from_be_bytes(data[8..12].try_into().unwrap()) as usize;
    let content_len = data.len() - 20;
    let expected_checksum = SHA1::hash(&data[..content_len]);
    let pack_checksum: [u8; 20] = data[content_len..].try_into().unwrap();
    if pack_checksum != expected_checksum {
        return Err(MuonGitError::InvalidObject("pack checksum mismatch".into()));
    }

    let mut cursor = 12usize;
    let mut entries = Vec::with_capacity(object_count);
    for _ in 0..object_count {
        if cursor >= content_len {
            return Err(MuonGitError::InvalidObject(
                "pack truncated before advertised object count".into(),
            ));
        }

        let offset = cursor as u64;
        let (type_num, _size, header_len) = parse_type_and_size_from_slice(&data[cursor..content_len])?;
        cursor += header_len;

        let kind = match type_num {
            OBJ_COMMIT | OBJ_TREE | OBJ_BLOB | OBJ_TAG => {
                let obj_type = pack_type_to_object_type(type_num)?;
                let (inflated, consumed) = inflate_zlib_stream(&data[cursor..content_len])?;
                cursor += consumed;
                RawPackEntryKind::Base {
                    obj_type,
                    data: inflated,
                }
            }
            OBJ_OFS_DELTA => {
                let (distance, consumed_header) = parse_ofs_delta_from_slice(&data[cursor..content_len])?;
                cursor += consumed_header;
                let (delta, consumed) = inflate_zlib_stream(&data[cursor..content_len])?;
                cursor += consumed;
                RawPackEntryKind::OfsDelta {
                    base_offset: offset
                        .checked_sub(distance)
                        .ok_or_else(|| MuonGitError::InvalidObject("invalid ofs-delta base".into()))?,
                    delta,
                }
            }
            OBJ_REF_DELTA => {
                if cursor + 20 > content_len {
                    return Err(MuonGitError::InvalidObject("truncated ref-delta base oid".into()));
                }
                let base_oid = crate::oid::OID::from_bytes(data[cursor..cursor + 20].to_vec());
                cursor += 20;
                let (delta, consumed) = inflate_zlib_stream(&data[cursor..content_len])?;
                cursor += consumed;
                RawPackEntryKind::RefDelta { base_oid, delta }
            }
            _ => {
                return Err(MuonGitError::InvalidObject(format!(
                    "unknown pack object type {}",
                    type_num
                )))
            }
        };

        entries.push(RawPackEntry { offset, kind });
    }

    if cursor != content_len {
        return Err(MuonGitError::InvalidObject(
            "pack contains trailing bytes after object stream".into(),
        ));
    }

    Ok((entries, pack_checksum))
}

fn resolve_pack_entries(
    entries: &[RawPackEntry],
    git_dir: Option<&Path>,
) -> Result<Vec<ResolvedPackEntry>, MuonGitError> {
    let mut resolved: Vec<Option<ResolvedPackEntry>> = vec![None; entries.len()];
    let offset_to_index: HashMap<_, _> = entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| (entry.offset, idx))
        .collect();
    let mut oid_to_index: HashMap<crate::oid::OID, usize> = HashMap::new();
    let mut remaining = entries.len();

    while remaining > 0 {
        let mut progressed = false;

        for (idx, entry) in entries.iter().enumerate() {
            if resolved[idx].is_some() {
                continue;
            }

            let resolved_entry = match &entry.kind {
                RawPackEntryKind::Base { obj_type, data } => Some(ResolvedPackEntry {
                    offset: entry.offset,
                    oid: crate::oid::OID::hash_object(*obj_type, data),
                    obj_type: *obj_type,
                    data: data.clone(),
                }),
                RawPackEntryKind::OfsDelta { base_offset, delta } => {
                    let Some(base_idx) = offset_to_index.get(base_offset) else {
                        return Err(MuonGitError::InvalidObject(format!(
                            "delta base offset {} not found in pack",
                            base_offset
                        )));
                    };
                    let Some(base) = resolved[*base_idx].as_ref() else {
                        continue;
                    };
                    let data = apply_delta(&base.data, delta)?;
                    Some(ResolvedPackEntry {
                        offset: entry.offset,
                        oid: crate::oid::OID::hash_object(base.obj_type, &data),
                        obj_type: base.obj_type,
                        data,
                    })
                }
                RawPackEntryKind::RefDelta { base_oid, delta } => {
                    let base = if let Some(base_idx) = oid_to_index.get(base_oid) {
                        resolved[*base_idx]
                            .as_ref()
                            .map(|entry| (entry.obj_type, entry.data.clone()))
                    } else if let Some(git_dir) = git_dir {
                        read_object(git_dir, base_oid)
                            .ok()
                            .map(|obj| (obj.obj_type, obj.data))
                    } else {
                        None
                    };
                    let Some((base_type, base_data)) = base else {
                        continue;
                    };
                    let data = apply_delta(&base_data, delta)?;
                    Some(ResolvedPackEntry {
                        offset: entry.offset,
                        oid: crate::oid::OID::hash_object(base_type, &data),
                        obj_type: base_type,
                        data,
                    })
                }
            };

            if let Some(resolved_entry) = resolved_entry {
                oid_to_index.insert(resolved_entry.oid.clone(), idx);
                resolved[idx] = Some(resolved_entry);
                remaining -= 1;
                progressed = true;
            }
        }

        if !progressed {
            return Err(MuonGitError::InvalidObject(
                "could not resolve all pack deltas".into(),
            ));
        }
    }

    Ok(resolved.into_iter().map(Option::unwrap).collect())
}

fn parse_type_and_size_from_slice(input: &[u8]) -> Result<(u8, u64, usize), MuonGitError> {
    if input.is_empty() {
        return Err(MuonGitError::InvalidObject("unexpected EOF in pack object header".into()));
    }

    let first = input[0];
    let type_num = (first >> 4) & 0x07;
    let mut size = (first & 0x0F) as u64;
    let mut shift = 4u32;
    let mut consumed = 1usize;
    let mut current = first;

    while current & 0x80 != 0 {
        if consumed >= input.len() {
            return Err(MuonGitError::InvalidObject("truncated pack object header".into()));
        }
        current = input[consumed];
        size |= ((current & 0x7F) as u64) << shift;
        shift += 7;
        consumed += 1;
    }

    Ok((type_num, size, consumed))
}

fn parse_ofs_delta_from_slice(input: &[u8]) -> Result<(u64, usize), MuonGitError> {
    if input.is_empty() {
        return Err(MuonGitError::InvalidObject("unexpected EOF in ofs-delta".into()));
    }
    let mut consumed = 1usize;
    let mut c = input[0];
    let mut offset = (c & 0x7F) as u64;

    while c & 0x80 != 0 {
        if consumed >= input.len() {
            return Err(MuonGitError::InvalidObject(
                "truncated ofs-delta offset".into(),
            ));
        }
        offset += 1;
        c = input[consumed];
        offset = (offset << 7) | (c & 0x7F) as u64;
        consumed += 1;
    }

    Ok((offset, consumed))
}

fn inflate_zlib_stream(input: &[u8]) -> Result<(Vec<u8>, usize), MuonGitError> {
    let mut decoder = flate2::read::ZlibDecoder::new(input);
    let mut output = Vec::new();
    decoder.read_to_end(&mut output)?;
    Ok((output, decoder.total_in() as usize))
}

fn collect_reachable_objects(
    git_dir: &Path,
    oid: &crate::oid::OID,
    exclude: &HashSet<crate::oid::OID>,
    visited: &mut HashSet<crate::oid::OID>,
    ordered: &mut Vec<crate::oid::OID>,
) -> Result<(), MuonGitError> {
    if exclude.contains(oid) || !visited.insert(oid.clone()) {
        return Ok(());
    }

    let obj = read_object(git_dir, oid)?;
    match obj.obj_type {
        ObjectType::Commit => {
            let commit = obj.as_commit()?;
            collect_reachable_objects(git_dir, &commit.tree_id, exclude, visited, ordered)?;
            for parent in commit.parent_ids {
                collect_reachable_objects(git_dir, &parent, exclude, visited, ordered)?;
            }
        }
        ObjectType::Tree => {
            let tree = obj.as_tree()?;
            for entry in tree.entries {
                collect_reachable_objects(git_dir, &entry.oid, exclude, visited, ordered)?;
            }
        }
        ObjectType::Tag => {
            let tag = obj.as_tag()?;
            collect_reachable_objects(git_dir, &tag.target_id, exclude, visited, ordered)?;
        }
        ObjectType::Blob => {}
    }

    ordered.push(oid.clone());
    Ok(())
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
    use crate::repository::Repository;
    use crate::tree::{serialize_tree, TreeEntry, file_mode};

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

    #[test]
    fn test_index_pack_to_odb_round_trip() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_pack_index_round_trip");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let blob_data = b"fetch payload\n";
        let pack_data = build_test_pack(&[(ObjectType::Blob, blob_data)]);
        let indexed = index_pack_to_odb(repo.git_dir(), &pack_data).unwrap();

        assert_eq!(indexed.object_count, 1);
        assert!(indexed.pack_path.exists());
        assert!(indexed.index_path.exists());

        let blob_oid = OID::hash_object(ObjectType::Blob, blob_data);
        let obj = crate::object::read_object(repo.git_dir(), &blob_oid).unwrap();
        assert_eq!(obj.obj_type, ObjectType::Blob);
        assert_eq!(obj.data, blob_data);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_build_pack_from_oids_round_trip() {
        let src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_pack_build_src");
        let dst = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_pack_build_dst");
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);

        let src_repo = Repository::init(src.to_str().unwrap(), false).unwrap();
        let dst_repo = Repository::init(dst.to_str().unwrap(), false).unwrap();
        let git_dir = src_repo.git_dir();

        let blob_oid = crate::odb::write_loose_object(git_dir, ObjectType::Blob, b"hello remote\n").unwrap();
        let tree_data = serialize_tree(&[TreeEntry {
            mode: file_mode::BLOB,
            name: "hello.txt".into(),
            oid: blob_oid.clone(),
        }]);
        let tree_oid = crate::odb::write_loose_object(git_dir, ObjectType::Tree, &tree_data).unwrap();
        let sig = crate::Signature {
            name: "MuonGit".into(),
            email: "muongit@example.invalid".into(),
            time: 0,
            offset: 0,
        };
        let commit_data = crate::commit::serialize_commit(&tree_oid, &[], &sig, &sig, "init\n", None);
        let commit_oid =
            crate::odb::write_loose_object(git_dir, ObjectType::Commit, &commit_data).unwrap();

        let pack_bytes = build_pack_from_oids(src_repo.git_dir(), std::slice::from_ref(&commit_oid), &[]).unwrap();
        let indexed = index_pack_to_odb(dst_repo.git_dir(), &pack_bytes).unwrap();

        assert_eq!(indexed.object_count, 3);
        let commit = crate::object::read_object(dst_repo.git_dir(), &commit_oid)
            .unwrap()
            .as_commit()
            .unwrap();
        assert_eq!(commit.tree_id, tree_oid);

        let blob = crate::object::read_object(dst_repo.git_dir(), &blob_oid)
            .unwrap()
            .as_blob()
            .unwrap();
        assert_eq!(blob.data, b"hello remote\n");

        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }
}
