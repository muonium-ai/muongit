/// Object identifier (SHA-1 / SHA-256)
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct OID {
    raw: Vec<u8>,
}

impl OID {
    pub fn from_hex(hex: &str) -> Result<Self, crate::MuonGitError> {
        let raw: Result<Vec<u8>, _> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
            .collect();
        match raw {
            Ok(bytes) => Ok(Self { raw: bytes }),
            Err(_) => Err(crate::MuonGitError::Invalid("invalid hex in OID".into())),
        }
    }

    pub fn hex(&self) -> String {
        self.raw.iter().map(|b| format!("{:02x}", b)).collect()
    }

    pub fn raw(&self) -> &[u8] {
        &self.raw
    }

    /// Create OID directly from raw bytes (no hex conversion)
    pub fn new(raw: Vec<u8>) -> Self {
        Self { raw }
    }
}

impl std::fmt::Debug for OID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OID({})", self.hex())
    }
}

impl std::fmt::Display for OID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oid_from_hex() {
        let hex = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let oid = OID::from_hex(hex).unwrap();
        assert_eq!(oid.hex(), hex);
    }

    #[test]
    fn test_oid_equality() {
        let a = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let b = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        assert_eq!(a, b);
    }
}
