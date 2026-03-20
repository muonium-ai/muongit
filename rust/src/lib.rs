//! MuonGit - Native Rust port of libgit2
//! API parity target: libgit2 v1.9.0

pub mod oid;
pub mod types;
pub mod error;
pub mod sha1;
pub mod sha256;
pub mod repository;
pub mod odb;
pub mod object;
pub mod patch;
pub mod refs;
pub mod refdb;
pub mod branch;
pub mod revparse;
pub mod revwalk;
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
pub mod checkout;
pub mod merge;
pub mod merge_base;
pub mod remote;
pub mod pack_index;
pub mod pack;
pub mod transport;
pub mod remote_transport;
pub mod fetch;
pub mod attributes;
pub mod filter;
pub mod submodule;
pub mod cherrypick;
pub mod revert;
pub mod rebase;
pub mod stash;
pub mod blame;
pub mod worktree;

pub use oid::OID;
pub use types::{ObjectType, Signature};
pub use error::MuonGitError;
pub use object::GitObject;
pub use patch::{
    Patch, PatchApplyResult, PatchFile, PatchFileApplyResult, PatchFileStatus, PatchHunk,
    PatchLine, PatchLineKind, PatchReject,
};
pub use branch::{Branch, BranchType, BranchUpstream};
pub use refdb::{RefDb, Reference};
pub use revparse::{resolve_revision, revparse, revparse_single, RevSpec};
pub use revwalk::{Revwalk, SORT_NONE, SORT_REVERSE, SORT_TIME, SORT_TOPOLOGICAL};
pub use repository::Repository;

/// Library version information
pub mod version {
    /// Version string read from the root VERSION file at build time.
    pub const STRING: &str = env!("MUONGIT_VERSION");
    pub const LIBGIT2_PARITY: &str = "1.9.0";
}
