//! MuonGit - Native Rust port of libgit2
//! API parity target: libgit2 v1.9.0

pub mod oid;
pub mod types;
pub mod error;
pub mod sha1;
pub mod sha256;
pub mod repository;
pub mod odb;
pub mod refs;
pub mod commit;
pub mod tree;
pub mod blob;
pub mod tag;
pub mod config;
pub mod reflog;
pub mod index;
pub mod diff;
pub mod status;
pub mod ignore;
pub mod merge_base;
pub mod pack_index;
pub mod pack;

pub use oid::OID;
pub use types::{ObjectType, Signature};
pub use error::MuonGitError;
pub use repository::Repository;

/// Library version information
pub mod version {
    /// Version string read from the root VERSION file at build time.
    pub const STRING: &str = env!("MUONGIT_VERSION");
    pub const LIBGIT2_PARITY: &str = "1.9.0";
}
