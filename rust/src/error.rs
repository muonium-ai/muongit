/// Errors from MuonGit operations
#[derive(Debug)]
pub enum MuonGitError {
    NotFound(String),
    InvalidObject(String),
    Ambiguous(String),
    BufferTooShort,
    BareRepo,
    UnbornBranch,
    Unmerged,
    NotFastForward,
    InvalidSpec(String),
    Conflict(String),
    Locked(String),
    Auth(String),
    Certificate(String),
    Invalid(String),
    Io(std::io::Error),
}

impl std::fmt::Display for MuonGitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(msg) => write!(f, "not found: {}", msg),
            Self::InvalidObject(msg) => write!(f, "invalid object: {}", msg),
            Self::Ambiguous(msg) => write!(f, "ambiguous: {}", msg),
            Self::BufferTooShort => write!(f, "buffer too short"),
            Self::BareRepo => write!(f, "operation not allowed on bare repo"),
            Self::UnbornBranch => write!(f, "unborn branch"),
            Self::Unmerged => write!(f, "unmerged entries exist"),
            Self::NotFastForward => write!(f, "not fast-forward"),
            Self::InvalidSpec(msg) => write!(f, "invalid spec: {}", msg),
            Self::Conflict(msg) => write!(f, "conflict: {}", msg),
            Self::Locked(msg) => write!(f, "locked: {}", msg),
            Self::Auth(msg) => write!(f, "auth error: {}", msg),
            Self::Certificate(msg) => write!(f, "certificate error: {}", msg),
            Self::Invalid(msg) => write!(f, "invalid: {}", msg),
            Self::Io(err) => write!(f, "io error: {}", err),
        }
    }
}

impl std::error::Error for MuonGitError {}

impl From<std::io::Error> for MuonGitError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}
