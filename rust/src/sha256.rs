//! Pure Rust SHA-256 implementation
//! Parity: libgit2 EXPERIMENTAL_SHA256 uses SHA-256 for object IDs

pub struct SHA256 {
    h: [u32; 8],
    buffer: Vec<u8>,
    total_length: u64,
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
    0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
    0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl SHA256 {
    pub fn new() -> Self {
        Self {
            h: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            buffer: Vec::new(),
            total_length: 0,
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
        self.total_length += data.len() as u64;

        while self.buffer.len() >= 64 {
            let block: [u8; 64] = self.buffer[..64].try_into().unwrap();
            self.process_block(&block);
            self.buffer.drain(..64);
        }
    }

    pub fn finalize(mut self) -> [u8; 32] {
        let mut padded = std::mem::take(&mut self.buffer);
        padded.push(0x80);

        while padded.len() % 64 != 56 {
            padded.push(0x00);
        }

        let bit_length = self.total_length * 8;
        padded.extend_from_slice(&bit_length.to_be_bytes());

        for chunk in padded.chunks_exact(64) {
            let block: [u8; 64] = chunk.try_into().unwrap();
            self.process_block(&block);
        }

        let mut digest = [0u8; 32];
        for (i, &h) in self.h.iter().enumerate() {
            digest[i * 4..i * 4 + 4].copy_from_slice(&h.to_be_bytes());
        }
        digest
    }

    fn process_block(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];

        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }

        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.h;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.h[0] = self.h[0].wrapping_add(a);
        self.h[1] = self.h[1].wrapping_add(b);
        self.h[2] = self.h[2].wrapping_add(c);
        self.h[3] = self.h[3].wrapping_add(d);
        self.h[4] = self.h[4].wrapping_add(e);
        self.h[5] = self.h[5].wrapping_add(f);
        self.h[6] = self.h[6].wrapping_add(g);
        self.h[7] = self.h[7].wrapping_add(h);
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
        let type_name = match obj_type {
            crate::ObjectType::Commit => "commit",
            crate::ObjectType::Tree => "tree",
            crate::ObjectType::Blob => "blob",
            crate::ObjectType::Tag => "tag",
        };

        let header = format!("{} {}\0", type_name, data.len());
        let mut sha = SHA256::new();
        sha.update(header.as_bytes());
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
