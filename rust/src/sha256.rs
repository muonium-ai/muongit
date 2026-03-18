//! SHA-256 implementation using the `sha2` crate for hardware-accelerated hashing.
//! Parity: libgit2 EXPERIMENTAL_SHA256 uses SHA-256 for object IDs

use sha2_crate::{Sha256 as Sha256Impl, Digest};

pub struct SHA256 {
    hasher: Sha256Impl,
}

impl SHA256 {
    pub fn new() -> Self {
        Self { hasher: Sha256Impl::new() }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }

    pub fn finalize(self) -> [u8; 32] {
        self.hasher.finalize().into()
    }

    /// Hash data in one call
    pub fn hash(data: &[u8]) -> [u8; 32] {
        let mut sha = SHA256::new();
        sha.update(data);
        sha.finalize()
    }
}

impl Default for SHA256 {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::OID {
    /// SHA-256 digest length in bytes
    pub const SHA256_LENGTH: usize = 32;

    /// SHA-256 hex string length
    pub const SHA256_HEX_LENGTH: usize = 64;

    /// Create an OID by hashing data with SHA-256 (git object style, experimental)
    pub fn hash_object_sha256(obj_type: crate::ObjectType, data: &[u8]) -> Self {
        let mut header_buf = [0u8; 32];
        let header_len = crate::sha1::build_object_header(obj_type, data.len(), &mut header_buf);
        let mut sha = SHA256::new();
        sha.update(&header_buf[..header_len]);
        sha.update(data);
        Self::from_bytes(sha.finalize().to_vec())
    }

    /// Zero OID for SHA-256
    pub fn zero_sha256() -> Self {
        Self::from_bytes(vec![0u8; Self::SHA256_LENGTH])
    }
}

/// Hash algorithm selection (matching libgit2 EXPERIMENTAL_SHA256)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgorithm {
    SHA1,
    SHA256,
}

impl HashAlgorithm {
    /// Digest length in bytes
    pub fn digest_length(&self) -> usize {
        match self {
            HashAlgorithm::SHA1 => 20,
            HashAlgorithm::SHA256 => 32,
        }
    }

    /// Hex string length
    pub fn hex_length(&self) -> usize {
        self.digest_length() * 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_empty() {
        let digest = SHA256::hash(b"");
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(hex, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn test_sha256_hello() {
        let digest = SHA256::hash(b"hello");
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(hex, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    }

    #[test]
    fn test_sha256_git_blob() {
        let data = b"hello\n";
        let oid = crate::OID::hash_object_sha256(crate::ObjectType::Blob, data);
        assert_eq!(oid.hex().len(), 64);
        assert!(!oid.is_zero());
    }

    #[test]
    fn test_sha256_longer() {
        let digest = SHA256::hash(b"The quick brown fox jumps over the lazy dog");
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(hex, "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592");
    }

    #[test]
    fn test_hash_algorithm() {
        assert_eq!(HashAlgorithm::SHA1.digest_length(), 20);
        assert_eq!(HashAlgorithm::SHA256.digest_length(), 32);
        assert_eq!(HashAlgorithm::SHA1.hex_length(), 40);
        assert_eq!(HashAlgorithm::SHA256.hex_length(), 64);
    }

    #[test]
    fn test_zero_sha256() {
        let z = crate::OID::zero_sha256();
        assert!(z.is_zero());
        assert_eq!(z.hex().len(), 64);
    }
}
