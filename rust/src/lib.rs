//! MuonGit - Native Rust port of libgit2
//! API parity target: libgit2 v1.9.0

pub mod oid;
pub mod types;
pub mod error;
pub mod sha1;
pub mod repository;
pub mod odb;
pub mod refs;
pub mod commit;
pub mod tree;
pub mod blob;
pub mod tag;

pub use oid::OID;
pub use types::{ObjectType, Signature};
pub use error::MuonGitError;
pub use repository::Repository;

/// Library version information
pub mod version {
    pub const MAJOR: u32 = 0;
    pub const MINOR: u32 = 1;
    pub const PATCH: u32 = 0;
    pub const LIBGIT2_PARITY: &str = "1.9.0";

    pub fn string() -> String {
        format!("{}.{}.{}", MAJOR, MINOR, PATCH)
    }
}
