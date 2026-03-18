//! Pure Rust SHA-1 implementation
//! Parity: libgit2 uses SHA-1 for object IDs (see src/util/hash/sha1/)

pub struct SHA1 {
    h: [u32; 5],
    buffer: Vec<u8>,
    total_length: u64,
}

impl SHA1 {
    pub fn new() -> Self {
        Self {
            h: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
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

    pub fn finalize(mut self) -> [u8; 20] {
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

        let mut digest = [0u8; 20];
        for (i, &h) in self.h.iter().enumerate() {
            digest[i * 4..i * 4 + 4].copy_from_slice(&h.to_be_bytes());
        }
        digest
    }

    fn process_block(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 80];

        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }

        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let [mut a, mut b, mut c, mut d, mut e] = self.h;

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        self.h[0] = self.h[0].wrapping_add(a);
        self.h[1] = self.h[1].wrapping_add(b);
        self.h[2] = self.h[2].wrapping_add(c);
        self.h[3] = self.h[3].wrapping_add(d);
        self.h[4] = self.h[4].wrapping_add(e);
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

impl crate::OID {
    /// Create an OID by hashing data with SHA-1 (git object style)
    pub fn hash_object(obj_type: crate::ObjectType, data: &[u8]) -> Self {
        let type_name = match obj_type {
            crate::ObjectType::Commit => "commit",
            crate::ObjectType::Tree => "tree",
            crate::ObjectType::Blob => "blob",
            crate::ObjectType::Tag => "tag",
        };

        let header = format!("{} {}\0", type_name, data.len());
        let mut sha = SHA1::new();
        sha.update(header.as_bytes());
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

    /// Create OID from raw bytes
    pub fn from_bytes(raw: Vec<u8>) -> Self {
        let hex = raw.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        // Use from_hex which we know works
        Self::from_hex(&hex).unwrap()
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
