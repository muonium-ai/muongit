/// Git object types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ObjectType {
    Commit = 1,
    Tree = 2,
    Blob = 3,
    Tag = 4,
}

/// Git signature (author/committer)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub name: String,
    pub email: String,
    pub time: i64,
    pub offset: i32,
}

impl Signature {
    pub fn new(name: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            email: email.into(),
            time: 0,
            offset: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_type() {
        assert_eq!(ObjectType::Commit as i32, 1);
        assert_eq!(ObjectType::Tree as i32, 2);
        assert_eq!(ObjectType::Blob as i32, 3);
        assert_eq!(ObjectType::Tag as i32, 4);
    }

    #[test]
    fn test_signature() {
        let sig = Signature::new("Test User", "test@example.com");
        assert_eq!(sig.name, "Test User");
        assert_eq!(sig.email, "test@example.com");
    }
}
