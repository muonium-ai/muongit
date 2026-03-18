//! SHA-1 implementation using the `sha1` crate for hardware-accelerated hashing.
//! Parity: libgit2 uses SHA-1 for object IDs (see src/util/hash/sha1/)

use sha1_crate::{Sha1, Digest};

pub struct SHA1 {
    hasher: Sha1,
}

impl SHA1 {
    pub fn new() -> Self {
        Self { hasher: Sha1::new() }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }

    pub fn finalize(self) -> [u8; 20] {
        self.hasher.finalize().into()
    }

    /// Hash data in one call
    pub fn hash(data: &[u8]) -> [u8; 20] {
        let mut sha = SHA1::new();
        sha.update(data);
        sha.finalize()
    }
}

impl Default for SHA1 {
    fn default() -> Self {
        Self::new()
    }
}

/// Build git object header ("type size\0") into a stack buffer, returns valid slice length
pub fn build_object_header(obj_type: crate::ObjectType, data_len: usize, buf: &mut [u8; 32]) -> usize {
    let type_bytes: &[u8] = match obj_type {
        crate::ObjectType::Commit => b"commit ",
        crate::ObjectType::Tree => b"tree ",
        crate::ObjectType::Blob => b"blob ",
        crate::ObjectType::Tag => b"tag ",
    };
    let mut pos = type_bytes.len();
    buf[..pos].copy_from_slice(type_bytes);
    // Write decimal size
    if data_len == 0 {
        buf[pos] = b'0';
        pos += 1;
    } else {
        let start = pos;
        let mut v = data_len;
        while v > 0 {
            buf[pos] = b'0' + (v % 10) as u8;
            v /= 10;
            pos += 1;
        }
        buf[start..pos].reverse();
    }
    buf[pos] = 0; // null terminator
    pos += 1;
    pos
}

impl crate::OID {
    /// Create an OID by hashing data with SHA-1 (git object style)
    pub fn hash_object(obj_type: crate::ObjectType, data: &[u8]) -> Self {
        let mut header_buf = [0u8; 32];
        let header_len = build_object_header(obj_type, data.len(), &mut header_buf);
        let mut sha = SHA1::new();
        sha.update(&header_buf[..header_len]);
        sha.update(data);
        Self::from_bytes(sha.finalize().to_vec())
    }

    /// SHA-1 digest length in bytes
    pub const SHA1_LENGTH: usize = 20;

    /// SHA-1 hex string length
    pub const SHA1_HEX_LENGTH: usize = 40;

    /// Whether this OID is all zeros
    pub fn is_zero(&self) -> bool {
        self.raw().iter().all(|&b| b == 0)
    }

    /// Zero OID
    pub fn zero() -> Self {
        Self::from_bytes(vec![0u8; Self::SHA1_LENGTH])
    }

    /// Create OID from raw bytes (direct, no hex round-trip)
    pub fn from_bytes(raw: Vec<u8>) -> Self {
        Self::new(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha1_empty() {
        let digest = SHA1::hash(b"");
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(hex, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn test_sha1_hello() {
        let digest = SHA1::hash(b"hello");
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(hex, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
    }

    #[test]
    fn test_sha1_git_blob() {
        // git hash-object equivalent: "hello\n" as blob
        let data = b"hello\n";
        let oid = crate::OID::hash_object(crate::ObjectType::Blob, data);
        assert_eq!(oid.hex(), "ce013625030ba8dba906f756967f9e9ca394464a");
    }

    #[test]
    fn test_oid_zero() {
        let z = crate::OID::zero();
        assert!(z.is_zero());
        assert_eq!(z.hex(), "0000000000000000000000000000000000000000");
    }
}
