//! Fetch, push, and clone operations
//! Parity: libgit2 src/libgit2/fetch.c, src/libgit2/push.c, src/libgit2/clone.c

use std::path::Path;

use crate::checkout::{reset, ResetMode};
use crate::error::MuonGitError;
use crate::merge_base::merge_base;
use crate::oid::OID;
use crate::pack::{build_pack_from_oids, index_pack_to_odb, IndexedPack};
use crate::refs;
use crate::remote::{add_remote, get_remote, parse_refspec};
use crate::remote_transport::{
    advertise_receive_pack, advertise_upload_pack, receive_pack, upload_pack, TransportOptions,
};
use crate::repository::Repository;
use crate::transport::{pkt_line_decode, PktLine, RemoteRef, ServerCapabilities};

// --- Fetch ---

/// Result of computing fetch wants: which OIDs we need from the remote.
#[derive(Debug, Clone)]
pub struct FetchNegotiation {
    /// OIDs we need to fetch (remote has, we don't).
    pub wants: Vec<OID>,
    /// OIDs we already have (common ancestors for negotiation).
    pub haves: Vec<OID>,
    /// Remote refs that matched the fetch refspecs.
    pub matched_refs: Vec<MatchedRef>,
}

/// A remote ref matched against a fetch refspec.
#[derive(Debug, Clone)]
pub struct MatchedRef {
    /// Remote ref name (e.g. "refs/heads/main").
    pub remote_name: String,
    /// Local ref name after refspec mapping (e.g. "refs/remotes/origin/main").
    pub local_name: String,
    /// OID of the remote ref.
    pub oid: OID,
}

/// Match a ref name against a refspec pattern (supports trailing glob).
/// E.g., "refs/heads/main" matches "refs/heads/*" and returns "main".
fn refspec_match<'a>(name: &'a str, pattern: &str) -> Option<&'a str> {
    if let Some(prefix) = pattern.strip_suffix('*') {
        name.strip_prefix(prefix)
    } else if name == pattern {
        Some("")
    } else {
        None
    }
}

/// Apply a refspec to map a remote ref name to a local ref name.
/// Returns the local name if the ref matches the refspec.
pub fn apply_refspec(remote_name: &str, refspec: &str) -> Option<String> {
    let (_force, src, dst) = parse_refspec(refspec)?;
    let matched = refspec_match(remote_name, src)?;

    if let Some(dst_prefix) = dst.strip_suffix('*') {
        Some(format!("{}{}", dst_prefix, matched))
    } else {
        Some(dst.to_string())
    }
}

/// Compute which objects we need to fetch from the remote.
/// `remote_refs` are the refs advertised by the remote.
/// `refspecs` are the fetch refspecs (e.g. "+refs/heads/*:refs/remotes/origin/*").
/// `git_dir` is used to check which OIDs we already have locally.
pub fn compute_fetch_wants(
    remote_refs: &[RemoteRef],
    refspecs: &[String],
    git_dir: &Path,
) -> Result<FetchNegotiation, MuonGitError> {
    let mut wants = Vec::new();
    let mut matched_refs = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for rref in remote_refs {
        for refspec in refspecs {
            if let Some(local_name) = apply_refspec(&rref.name, refspec) {
                matched_refs.push(MatchedRef {
                    remote_name: rref.name.clone(),
                    local_name: local_name.clone(),
                    oid: rref.oid.clone(),
                });

                // Check if we already have this OID locally
                let already_have = refs::resolve_reference(git_dir, &local_name)
                    .map(|local_oid| local_oid == rref.oid)
                    .unwrap_or(false);

                if !already_have && seen.insert(rref.oid.clone()) {
                    wants.push(rref.oid.clone());
                }
            }
        }
    }

    // Collect local refs as haves for negotiation
    let haves = collect_local_refs(git_dir);

    Ok(FetchNegotiation {
        wants,
        haves,
        matched_refs,
    })
}

/// Collect all local ref OIDs for negotiation (haves).
fn collect_local_refs(git_dir: &Path) -> Vec<OID> {
    let mut oids = Vec::new();

    // Walk refs/heads and refs/remotes
    for dir in &["refs/heads", "refs/remotes"] {
        let ref_dir = git_dir.join(dir);
        if ref_dir.is_dir() {
            collect_refs_recursive(&ref_dir, &mut oids);
        }
    }

    oids
}

fn collect_refs_recursive(dir: &Path, oids: &mut Vec<OID>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_refs_recursive(&path, oids);
            } else if let Ok(content) = std::fs::read_to_string(&path) {
                let hex = content.trim();
                if hex.len() == 40 {
                    if let Ok(oid) = OID::from_hex(hex) {
                        oids.push(oid);
                    }
                }
            }
        }
    }
}

/// Update local refs after a successful fetch.
/// Writes the fetched OIDs to the mapped local ref names.
pub fn update_refs_from_fetch(
    git_dir: &Path,
    matched_refs: &[MatchedRef],
) -> Result<usize, MuonGitError> {
    let mut updated = 0;
    for mref in matched_refs {
        refs::write_reference(git_dir, &mref.local_name, &mref.oid)?;
        updated += 1;
    }
    Ok(updated)
}

// --- Push ---

/// A ref update for push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushUpdate {
    /// Local ref name (source).
    pub src_ref: String,
    /// Remote ref name (destination).
    pub dst_ref: String,
    /// OID of the local ref (what we're pushing).
    pub src_oid: OID,
    /// OID of the remote ref (for fast-forward check); zero OID if creating.
    pub dst_oid: OID,
    /// Whether this is a force push.
    pub force: bool,
}

/// Compute push updates: which refs to push to the remote.
/// `push_refspecs` are like "refs/heads/main:refs/heads/main" or "+refs/heads/*:refs/heads/*".
pub fn compute_push_updates(
    push_refspecs: &[&str],
    git_dir: &Path,
    remote_refs: &[RemoteRef],
) -> Result<Vec<PushUpdate>, MuonGitError> {
    let mut updates = Vec::new();

    for refspec in push_refspecs {
        let (force, src, dst) = parse_refspec(refspec)
            .ok_or_else(|| MuonGitError::Invalid(format!("invalid push refspec: {}", refspec)))?;

        // Resolve the local ref
        let src_oid = refs::resolve_reference(git_dir, src)?;

        // Find the remote ref for the destination
        let dst_oid = remote_refs
            .iter()
            .find(|r| r.name == dst)
            .map(|r| r.oid.clone())
            .unwrap_or_else(OID::zero);

        updates.push(PushUpdate {
            src_ref: src.to_string(),
            dst_ref: dst.to_string(),
            src_oid,
            dst_oid,
            force,
        });
    }

    Ok(updates)
}

/// Build a push report line for each update.
/// Format: "<old-oid> <new-oid> <refname>\n"
pub fn build_push_report(updates: &[PushUpdate]) -> String {
    let mut report = String::new();
    for u in updates {
        report.push_str(&format!(
            "{} {} {}\n",
            u.dst_oid.hex(),
            u.src_oid.hex(),
            u.dst_ref,
        ));
    }
    report
}

// --- Clone ---

/// Options for clone.
#[derive(Debug, Clone)]
pub struct CloneOptions {
    /// Remote name (default: "origin").
    pub remote_name: String,
    /// Branch to checkout after clone (default: remote HEAD).
    pub branch: Option<String>,
    /// Whether to create a bare clone.
    pub bare: bool,
    /// Transport and credential options for the remote session.
    pub transport: TransportOptions,
}

impl Default for CloneOptions {
    fn default() -> Self {
        Self {
            remote_name: "origin".to_string(),
            branch: None,
            bare: false,
            transport: TransportOptions::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FetchOptions {
    pub refspecs: Option<Vec<String>>,
    pub transport: TransportOptions,
}

#[derive(Debug, Clone)]
pub struct FetchResult {
    pub advertised_refs: Vec<RemoteRef>,
    pub capabilities: ServerCapabilities,
    pub matched_refs: Vec<MatchedRef>,
    pub updated_refs: usize,
    pub indexed_pack: Option<IndexedPack>,
}

#[derive(Debug, Clone, Default)]
pub struct PushOptions {
    pub refspecs: Option<Vec<String>>,
    pub transport: TransportOptions,
}

#[derive(Debug, Clone)]
pub struct PushResult {
    pub advertised_refs: Vec<RemoteRef>,
    pub updated_tracking_refs: usize,
    pub report: String,
}

/// Set up a new repository for clone: init repo, add remote, configure HEAD.
/// Returns the initialized repository. The caller is responsible for
/// fetching objects and checking out the working tree.
pub fn clone_setup(
    path: &str,
    url: &str,
    opts: &CloneOptions,
) -> Result<Repository, MuonGitError> {
    let repo = Repository::init(path, opts.bare)?;

    // Add the remote
    add_remote(repo.git_dir(), &opts.remote_name, url)?;

    // If a specific branch is requested, set up HEAD to track it
    if let Some(branch) = &opts.branch {
        let target = format!("refs/heads/{}", branch);
        refs::write_symbolic_reference(repo.git_dir(), "HEAD", &target)?;
    }

    Ok(repo)
}

/// After fetching, set up HEAD and the default branch for a clone.
/// `default_branch` should be resolved from the remote (e.g. from symref capability).
pub fn clone_finish(
    git_dir: &Path,
    remote_name: &str,
    default_branch: &str,
    head_oid: &OID,
) -> Result<(), MuonGitError> {
    let local_branch = format!("refs/heads/{}", default_branch);
    let remote_ref = format!("refs/remotes/{}/{}", remote_name, default_branch);

    // Create the local branch pointing to the fetched commit
    refs::write_reference(git_dir, &local_branch, head_oid)?;

    // Update the remote tracking ref
    refs::write_reference(git_dir, &remote_ref, head_oid)?;

    // Point HEAD at the local branch
    refs::write_symbolic_reference(git_dir, "HEAD", &local_branch)?;

    Ok(())
}

/// Extract the default branch from server capabilities (symref=HEAD:refs/heads/main).
pub fn default_branch_from_caps(caps: &crate::transport::ServerCapabilities) -> Option<String> {
    let symref = caps.get("symref")?;
    // symref is like "HEAD:refs/heads/main"
    let (head_part, target) = symref.split_once(':')?;
    if head_part == "HEAD" {
        target.strip_prefix("refs/heads/").map(|s| s.to_string())
    } else {
        None
    }
}

pub fn clone_repository(
    url: &str,
    path: &str,
    opts: &CloneOptions,
) -> Result<Repository, MuonGitError> {
    let repo = clone_setup(path, url, opts)?;
    let fetch_opts = FetchOptions {
        refspecs: None,
        transport: opts.transport.clone(),
    };
    let fetch = fetch_remote(&repo, &opts.remote_name, &fetch_opts)?;
    let (branch, head_oid) = resolve_clone_head(&fetch, opts.branch.as_deref())?;
    clone_finish(repo.git_dir(), &opts.remote_name, &branch, &head_oid)?;
    if !opts.bare {
        let workdir = repo.workdir().ok_or(MuonGitError::BareRepo)?;
        reset(repo.git_dir(), Some(workdir), "HEAD", ResetMode::Hard)?;
    }
    Ok(repo)
}

pub fn fetch_remote(
    repo: &Repository,
    remote_name: &str,
    opts: &FetchOptions,
) -> Result<FetchResult, MuonGitError> {
    let remote = get_remote(repo.git_dir(), remote_name)?;
    let advertised = advertise_upload_pack(&remote.url, &opts.transport)?;
    let refspecs = opts
        .refspecs
        .clone()
        .unwrap_or_else(|| remote.fetch_refspecs.clone());
    let negotiation = compute_fetch_wants(&advertised.refs, &refspecs, repo.git_dir())?;

    if negotiation.wants.is_empty() {
        let updated_refs = update_refs_from_fetch(repo.git_dir(), &negotiation.matched_refs)?;
        return Ok(FetchResult {
            advertised_refs: advertised.refs,
            capabilities: advertised.capabilities,
            matched_refs: negotiation.matched_refs,
            updated_refs,
            indexed_pack: None,
        });
    }

    let requested_caps = fetch_capabilities(&advertised.capabilities);
    let caps: Vec<_> = requested_caps.iter().map(String::as_str).collect();
    let request = crate::transport::build_want_have(&negotiation.wants, &negotiation.haves, &caps);
    let response = upload_pack(&remote.url, &request, &opts.transport)?;
    let pack = extract_pack_from_fetch_response(&response)?;
    let indexed_pack = match pack {
        Some(pack_bytes) => Some(index_pack_to_odb(repo.git_dir(), &pack_bytes)?),
        None => None,
    };
    let updated_refs = update_refs_from_fetch(repo.git_dir(), &negotiation.matched_refs)?;

    Ok(FetchResult {
        advertised_refs: advertised.refs,
        capabilities: advertised.capabilities,
        matched_refs: negotiation.matched_refs,
        updated_refs,
        indexed_pack,
    })
}

pub fn push_remote(
    repo: &Repository,
    remote_name: &str,
    opts: &PushOptions,
) -> Result<PushResult, MuonGitError> {
    let remote = get_remote(repo.git_dir(), remote_name)?;
    let advertised = advertise_receive_pack(&remote.url, &opts.transport)?;
    let refspecs = if let Some(refspecs) = &opts.refspecs {
        refspecs.clone()
    } else {
        default_push_refspecs(repo.git_dir())?
    };
    let push_refspecs: Vec<_> = refspecs.iter().map(String::as_str).collect();
    let updates = compute_push_updates(&push_refspecs, repo.git_dir(), &advertised.refs)?;

    for update in &updates {
        if !update.force && !is_fast_forward(repo.git_dir(), &update.dst_oid, &update.src_oid)? {
            return Err(MuonGitError::NotFastForward);
        }
    }

    let exclude: Vec<_> = advertised.refs.iter().map(|r| r.oid.clone()).collect();
    let roots: Vec<_> = updates.iter().map(|u| u.src_oid.clone()).collect();
    let pack = build_pack_from_oids(repo.git_dir(), &roots, &exclude)?;
    let request = build_push_request(&updates, &pack, &advertised.capabilities);
    let response = receive_pack(&remote.url, &request, &opts.transport)?;
    let report = parse_push_response(&response)?;
    let updated_tracking_refs = update_tracking_refs_after_push(repo.git_dir(), remote_name, &updates)?;

    Ok(PushResult {
        advertised_refs: advertised.refs,
        updated_tracking_refs,
        report,
    })
}

impl Repository {
    pub fn fetch(
        &self,
        remote_name: &str,
        opts: &FetchOptions,
    ) -> Result<FetchResult, MuonGitError> {
        fetch_remote(self, remote_name, opts)
    }

    pub fn push(
        &self,
        remote_name: &str,
        opts: &PushOptions,
    ) -> Result<PushResult, MuonGitError> {
        push_remote(self, remote_name, opts)
    }
}

fn fetch_capabilities(caps: &ServerCapabilities) -> Vec<String> {
    let mut requested = Vec::new();
    if caps.has("side-band-64k") {
        requested.push("side-band-64k".to_string());
    } else if caps.has("side-band") {
        requested.push("side-band".to_string());
    }
    if caps.has("ofs-delta") {
        requested.push("ofs-delta".to_string());
    }
    if caps.has("include-tag") {
        requested.push("include-tag".to_string());
    }
    requested
}

fn extract_pack_from_fetch_response(response: &[u8]) -> Result<Option<Vec<u8>>, MuonGitError> {
    let (lines, consumed) = pkt_line_decode(response)?;
    let mut pack = Vec::new();

    for line in lines {
        if let PktLine::Data(data) = line {
            if data.starts_with(b"ACK ") || data == b"NAK\n" {
                continue;
            }
            match data.first().copied() {
                Some(1) => pack.extend_from_slice(&data[1..]),
                Some(2) => {}
                Some(3) => {
                    return Err(MuonGitError::Invalid(
                        String::from_utf8_lossy(&data[1..]).trim().to_string(),
                    ))
                }
                _ => {}
            }
        }
    }

    if !pack.is_empty() {
        return Ok(Some(pack));
    }

    if consumed < response.len() && response[consumed..].starts_with(b"PACK") {
        return Ok(Some(response[consumed..].to_vec()));
    }

    Ok(None)
}

fn build_push_request(
    updates: &[PushUpdate],
    pack: &[u8],
    caps: &ServerCapabilities,
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut requested_caps = vec!["report-status".to_string()];
    if caps.has("ofs-delta") {
        requested_caps.push("ofs-delta".to_string());
    }

    for (idx, update) in updates.iter().enumerate() {
        let line = if idx == 0 {
            format!(
                "{} {} {}\0{}\n",
                update.dst_oid.hex(),
                update.src_oid.hex(),
                update.dst_ref,
                requested_caps.join(" ")
            )
        } else {
            format!(
                "{} {} {}\n",
                update.dst_oid.hex(),
                update.src_oid.hex(),
                update.dst_ref
            )
        };
        out.extend_from_slice(&crate::transport::pkt_line_encode(line.as_bytes()));
    }
    out.extend_from_slice(&crate::transport::pkt_line_flush());
    out.extend_from_slice(pack);
    out
}

fn parse_push_response(response: &[u8]) -> Result<String, MuonGitError> {
    let (lines, consumed) = pkt_line_decode(response)?;
    let mut text = String::new();

    for line in lines {
        if let PktLine::Data(data) = line {
            let payload = match data.first().copied() {
                Some(1) | Some(2) => &data[1..],
                Some(3) => {
                    return Err(MuonGitError::Invalid(
                        String::from_utf8_lossy(&data[1..]).trim().to_string(),
                    ))
                }
                _ => &data[..],
            };
            text.push_str(&String::from_utf8_lossy(payload));
        }
    }

    if consumed < response.len() {
        text.push_str(&String::from_utf8_lossy(&response[consumed..]));
    }

    for line in text.lines() {
        if line.starts_with("unpack ") && line != "unpack ok" {
            return Err(MuonGitError::Invalid(line.to_string()));
        }
        if let Some(rest) = line.strip_prefix("ng ") {
            return Err(MuonGitError::Invalid(rest.to_string()));
        }
    }

    Ok(text)
}

fn resolve_clone_head(
    fetch: &FetchResult,
    branch: Option<&str>,
) -> Result<(String, OID), MuonGitError> {
    if let Some(branch) = branch {
        let ref_name = format!("refs/heads/{}", branch);
        let head_oid = fetch
            .advertised_refs
            .iter()
            .find(|r| r.name == ref_name)
            .map(|r| r.oid.clone())
            .ok_or_else(|| MuonGitError::NotFound(format!("remote branch '{}' not found", branch)))?;
        return Ok((branch.to_string(), head_oid));
    }

    if let Some(default_branch) = default_branch_from_caps(&fetch.capabilities) {
        let ref_name = format!("refs/heads/{}", default_branch);
        if let Some(head_oid) = fetch
            .advertised_refs
            .iter()
            .find(|r| r.name == ref_name)
            .map(|r| r.oid.clone())
        {
            return Ok((default_branch, head_oid));
        }
    }

    let head_oid = fetch
        .advertised_refs
        .iter()
        .find(|r| r.name == "HEAD")
        .map(|r| r.oid.clone());
    if let Some(head_oid) = head_oid {
        if let Some(branch_ref) = fetch
            .advertised_refs
            .iter()
            .find(|r| r.name.starts_with("refs/heads/") && r.oid == head_oid)
        {
            return Ok((
                branch_ref
                    .name
                    .trim_start_matches("refs/heads/")
                    .to_string(),
                head_oid,
            ));
        }
    }

    for candidate in ["main", "master"] {
        let ref_name = format!("refs/heads/{}", candidate);
        if let Some(head_oid) = fetch
            .advertised_refs
            .iter()
            .find(|r| r.name == ref_name)
            .map(|r| r.oid.clone())
        {
            return Ok((candidate.to_string(), head_oid));
        }
    }

    Err(MuonGitError::NotFound(
        "could not determine remote default branch".into(),
    ))
}

fn default_push_refspecs(git_dir: &Path) -> Result<Vec<String>, MuonGitError> {
    let head = refs::read_reference(git_dir, "HEAD")?;
    let target = head
        .strip_prefix("ref: ")
        .map(str::trim)
        .ok_or_else(|| MuonGitError::InvalidSpec("HEAD is detached; provide push refspecs".into()))?;
    Ok(vec![format!("{}:{}", target, target)])
}

fn is_fast_forward(git_dir: &Path, old_oid: &OID, new_oid: &OID) -> Result<bool, MuonGitError> {
    if old_oid.is_zero() {
        return Ok(true);
    }
    Ok(merge_base(git_dir, old_oid, new_oid)? == Some(old_oid.clone()))
}

fn update_tracking_refs_after_push(
    git_dir: &Path,
    remote_name: &str,
    updates: &[PushUpdate],
) -> Result<usize, MuonGitError> {
    let mut updated = 0;
    for update in updates {
        if let Some(branch) = update.dst_ref.strip_prefix("refs/heads/") {
            let tracking_ref = format!("refs/remotes/{}/{}", remote_name, branch);
            refs::write_reference(git_dir, &tracking_ref, &update.src_oid)?;
            updated += 1;
        }
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::get_remote;

    #[test]
    fn test_refspec_match_glob() {
        assert_eq!(refspec_match("refs/heads/main", "refs/heads/*"), Some("main"));
        assert_eq!(refspec_match("refs/heads/feature/x", "refs/heads/*"), Some("feature/x"));
        assert_eq!(refspec_match("refs/tags/v1", "refs/heads/*"), None);
    }

    #[test]
    fn test_refspec_match_exact() {
        assert_eq!(refspec_match("refs/heads/main", "refs/heads/main"), Some(""));
        assert_eq!(refspec_match("refs/heads/dev", "refs/heads/main"), None);
    }

    #[test]
    fn test_apply_refspec_glob() {
        let result = apply_refspec("refs/heads/main", "+refs/heads/*:refs/remotes/origin/*");
        assert_eq!(result, Some("refs/remotes/origin/main".to_string()));

        let result = apply_refspec("refs/heads/feature/x", "+refs/heads/*:refs/remotes/origin/*");
        assert_eq!(result, Some("refs/remotes/origin/feature/x".to_string()));
    }

    #[test]
    fn test_apply_refspec_exact() {
        let result = apply_refspec("refs/heads/main", "refs/heads/main:refs/heads/main");
        assert_eq!(result, Some("refs/heads/main".to_string()));
    }

    #[test]
    fn test_apply_refspec_no_match() {
        let result = apply_refspec("refs/tags/v1", "+refs/heads/*:refs/remotes/origin/*");
        assert_eq!(result, None);
    }

    #[test]
    fn test_compute_fetch_wants() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_fetch_wants");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let oid1 = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let oid2 = OID::from_hex("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();

        let remote_refs = vec![
            RemoteRef { oid: oid1.clone(), name: "refs/heads/main".to_string() },
            RemoteRef { oid: oid2.clone(), name: "refs/heads/dev".to_string() },
            RemoteRef { oid: oid1.clone(), name: "refs/tags/v1".to_string() },
        ];
        let refspecs = vec!["+refs/heads/*:refs/remotes/origin/*".to_string()];

        let neg = compute_fetch_wants(&remote_refs, &refspecs, repo.git_dir()).unwrap();

        // Should want both branch OIDs (not the tag, since refspec doesn't match)
        assert_eq!(neg.wants.len(), 2);
        assert!(neg.wants.contains(&oid1));
        assert!(neg.wants.contains(&oid2));
        assert_eq!(neg.matched_refs.len(), 2);
        assert_eq!(neg.matched_refs[0].local_name, "refs/remotes/origin/main");
        assert_eq!(neg.matched_refs[1].local_name, "refs/remotes/origin/dev");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_compute_fetch_wants_skips_existing() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_fetch_wants_skip");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();

        // Write the ref locally so it's already up to date
        refs::write_reference(repo.git_dir(), "refs/remotes/origin/main", &oid).unwrap();

        let remote_refs = vec![
            RemoteRef { oid: oid.clone(), name: "refs/heads/main".to_string() },
        ];
        let refspecs = vec!["+refs/heads/*:refs/remotes/origin/*".to_string()];

        let neg = compute_fetch_wants(&remote_refs, &refspecs, repo.git_dir()).unwrap();

        // Already have it, so no wants
        assert_eq!(neg.wants.len(), 0);
        assert_eq!(neg.matched_refs.len(), 1);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_update_refs_from_fetch() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_fetch_update_refs");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let matched = vec![
            MatchedRef {
                remote_name: "refs/heads/main".to_string(),
                local_name: "refs/remotes/origin/main".to_string(),
                oid: oid.clone(),
            },
        ];

        let count = update_refs_from_fetch(repo.git_dir(), &matched).unwrap();
        assert_eq!(count, 1);

        let resolved = refs::resolve_reference(repo.git_dir(), "refs/remotes/origin/main").unwrap();
        assert_eq!(resolved, oid);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_compute_push_updates() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_push_updates");
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();

        let local_oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let remote_oid = OID::from_hex("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();

        // Create a local ref
        refs::write_reference(repo.git_dir(), "refs/heads/main", &local_oid).unwrap();

        let remote_refs = vec![
            RemoteRef { oid: remote_oid.clone(), name: "refs/heads/main".to_string() },
        ];

        let updates = compute_push_updates(
            &["refs/heads/main:refs/heads/main"],
            repo.git_dir(),
            &remote_refs,
        ).unwrap();

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].src_oid, local_oid);
        assert_eq!(updates[0].dst_oid, remote_oid);
        assert_eq!(updates[0].dst_ref, "refs/heads/main");
        assert!(!updates[0].force);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_build_push_report() {
        let oid1 = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let oid2 = OID::from_hex("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();

        let updates = vec![PushUpdate {
            src_ref: "refs/heads/main".to_string(),
            dst_ref: "refs/heads/main".to_string(),
            src_oid: oid1.clone(),
            dst_oid: oid2.clone(),
            force: false,
        }];

        let report = build_push_report(&updates);
        assert!(report.contains(&oid2.hex()));
        assert!(report.contains(&oid1.hex()));
        assert!(report.contains("refs/heads/main"));
    }

    #[test]
    fn test_clone_setup() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_clone_setup");
        let _ = std::fs::remove_dir_all(&tmp);

        let repo = clone_setup(
            tmp.to_str().unwrap(),
            "https://example.com/repo.git",
            &CloneOptions::default(),
        ).unwrap();

        // Verify remote was created
        let remote = get_remote(repo.git_dir(), "origin").unwrap();
        assert_eq!(remote.url, "https://example.com/repo.git");
        assert_eq!(remote.fetch_refspecs[0], "+refs/heads/*:refs/remotes/origin/*");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_clone_finish() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_clone_finish");
        let _ = std::fs::remove_dir_all(&tmp);
        let _repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = tmp.join(".git");

        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        clone_finish(&git_dir, "origin", "main", &oid).unwrap();

        // Local branch should exist
        let resolved = refs::resolve_reference(&git_dir, "refs/heads/main").unwrap();
        assert_eq!(resolved, oid);

        // Remote tracking ref should exist
        let remote_resolved = refs::resolve_reference(&git_dir, "refs/remotes/origin/main").unwrap();
        assert_eq!(remote_resolved, oid);

        // HEAD should point to refs/heads/main
        let head = std::fs::read_to_string(git_dir.join("HEAD")).unwrap();
        assert!(head.contains("refs/heads/main"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_default_branch_from_caps() {
        use crate::transport::ServerCapabilities;

        let caps = ServerCapabilities {
            capabilities: vec![
                "multi_ack".into(),
                "symref=HEAD:refs/heads/main".into(),
            ],
        };
        assert_eq!(default_branch_from_caps(&caps), Some("main".to_string()));

        let caps2 = ServerCapabilities {
            capabilities: vec!["multi_ack".into()],
        };
        assert_eq!(default_branch_from_caps(&caps2), None);
    }

    #[test]
    fn test_clone_setup_with_branch() {
        let tmp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/test_clone_branch");
        let _ = std::fs::remove_dir_all(&tmp);

        let opts = CloneOptions {
            branch: Some("develop".to_string()),
            ..CloneOptions::default()
        };
        let repo = clone_setup(tmp.to_str().unwrap(), "https://example.com/repo.git", &opts).unwrap();

        let head = std::fs::read_to_string(repo.git_dir().join("HEAD")).unwrap();
        assert!(head.contains("refs/heads/develop"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
