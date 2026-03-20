//! Commit graph walking for log-style traversal.
//! Parity target: libgit2 `git_revwalk`

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::commit::Commit;
use crate::error::MuonGitError;
use crate::refs::resolve_reference;
use crate::revparse::{read_commit, revparse, RevSpec};
use crate::OID;

pub const SORT_NONE: u32 = 0;
pub const SORT_TOPOLOGICAL: u32 = 1 << 0;
pub const SORT_TIME: u32 = 1 << 1;
pub const SORT_REVERSE: u32 = 1 << 2;

/// A reusable revision walker.
#[derive(Debug, Clone)]
pub struct Revwalk {
    git_dir: PathBuf,
    roots: Vec<OID>,
    hidden: Vec<OID>,
    sort_mode: u32,
    first_parent_only: bool,
    prepared: Option<Vec<OID>>,
    cursor: usize,
}

impl Revwalk {
    pub fn new(git_dir: &Path) -> Self {
        Self {
            git_dir: git_dir.to_path_buf(),
            roots: Vec::new(),
            hidden: Vec::new(),
            sort_mode: SORT_NONE,
            first_parent_only: false,
            prepared: None,
            cursor: 0,
        }
    }

    pub fn reset(&mut self) {
        self.roots.clear();
        self.hidden.clear();
        self.first_parent_only = false;
        self.invalidate();
    }

    pub fn sorting(&mut self, sort_mode: u32) {
        self.sort_mode = sort_mode;
        self.invalidate();
    }

    pub fn simplify_first_parent(&mut self) {
        self.first_parent_only = true;
        self.invalidate();
    }

    pub fn push(&mut self, oid: OID) {
        self.roots.push(oid);
        self.invalidate();
    }

    pub fn push_head(&mut self) -> Result<(), MuonGitError> {
        self.push(resolve_reference(&self.git_dir, "HEAD")?);
        Ok(())
    }

    pub fn push_ref(&mut self, refname: &str) -> Result<(), MuonGitError> {
        self.push(resolve_reference(&self.git_dir, refname)?);
        Ok(())
    }

    pub fn hide(&mut self, oid: OID) {
        self.hidden.push(oid);
        self.invalidate();
    }

    pub fn hide_head(&mut self) -> Result<(), MuonGitError> {
        self.hide(resolve_reference(&self.git_dir, "HEAD")?);
        Ok(())
    }

    pub fn hide_ref(&mut self, refname: &str) -> Result<(), MuonGitError> {
        self.hide(resolve_reference(&self.git_dir, refname)?);
        Ok(())
    }

    pub fn push_revspec(&mut self, revspec: &RevSpec) -> Result<(), MuonGitError> {
        if !revspec.is_range {
            let oid = revspec.to.clone().ok_or_else(|| {
                MuonGitError::InvalidSpec("revspec is missing a target commit".into())
            })?;
            self.push(oid);
            return Ok(());
        }

        let from = revspec.from.clone().ok_or_else(|| {
            MuonGitError::InvalidSpec("range is missing a left-hand side".into())
        })?;
        let to = revspec.to.clone().ok_or_else(|| {
            MuonGitError::InvalidSpec("range is missing a right-hand side".into())
        })?;

        if revspec.uses_merge_base {
            self.push(from.clone());
            self.push(to.clone());
            for base in merge_bases(&self.git_dir, &from, &to)? {
                self.hide(base);
            }
        } else {
            self.push(to);
            self.hide(from);
        }

        Ok(())
    }

    pub fn push_range(&mut self, spec: &str) -> Result<(), MuonGitError> {
        let revspec = revparse(&self.git_dir, spec)?;
        if !revspec.is_range {
            return Err(MuonGitError::InvalidSpec(format!(
                "'{}' is not a revision range",
                spec
            )));
        }
        self.push_revspec(&revspec)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<OID>, MuonGitError> {
        self.prepare()?;
        let Some(prepared) = self.prepared.as_ref() else {
            return Ok(None);
        };
        if self.cursor >= prepared.len() {
            return Ok(None);
        }
        let oid = prepared[self.cursor].clone();
        self.cursor += 1;
        Ok(Some(oid))
    }

    pub fn collect_all(&mut self) -> Result<Vec<OID>, MuonGitError> {
        self.prepare()?;
        Ok(self.prepared.clone().unwrap_or_default())
    }

    fn invalidate(&mut self) {
        self.prepared = None;
        self.cursor = 0;
    }

    fn prepare(&mut self) -> Result<(), MuonGitError> {
        if self.prepared.is_some() {
            return Ok(());
        }

        let hidden = collect_ancestors(&self.git_dir, &self.hidden, self.first_parent_only)?;
        let commits = collect_visible_commits(
            &self.git_dir,
            &self.roots,
            &hidden,
            self.first_parent_only,
        )?;

        let mut ordered = if self.sort_mode & SORT_TOPOLOGICAL != 0 {
            topo_sort(&commits, self.sort_mode, self.first_parent_only)
        } else {
            let mut ids: Vec<OID> = commits.keys().cloned().collect();
            ids.sort_by(|lhs, rhs| compare_commits(lhs, rhs, &commits, self.sort_mode));
            ids
        };

        if self.sort_mode & SORT_REVERSE != 0 {
            ordered.reverse();
        }

        self.prepared = Some(ordered);
        self.cursor = 0;
        Ok(())
    }
}

fn collect_ancestors(
    git_dir: &Path,
    starts: &[OID],
    first_parent_only: bool,
) -> Result<HashSet<OID>, MuonGitError> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    for oid in starts {
        if visited.insert(oid.clone()) {
            queue.push_back(oid.clone());
        }
    }

    while let Some(oid) = queue.pop_front() {
        let commit = read_commit(git_dir, &oid)?;
        for parent in selected_parents(&commit, first_parent_only) {
            if visited.insert(parent.clone()) {
                queue.push_back(parent.clone());
            }
        }
    }

    Ok(visited)
}

fn collect_visible_commits(
    git_dir: &Path,
    roots: &[OID],
    hidden: &HashSet<OID>,
    first_parent_only: bool,
) -> Result<HashMap<OID, Commit>, MuonGitError> {
    let mut commits = HashMap::new();
    let mut queue = VecDeque::new();
    let mut seen = HashSet::new();

    for oid in roots {
        if !hidden.contains(oid) && seen.insert(oid.clone()) {
            queue.push_back(oid.clone());
        }
    }

    while let Some(oid) = queue.pop_front() {
        if hidden.contains(&oid) {
            continue;
        }

        let commit = read_commit(git_dir, &oid)?;
        for parent in selected_parents(&commit, first_parent_only) {
            if !hidden.contains(parent) && seen.insert(parent.clone()) {
                queue.push_back(parent.clone());
            }
        }
        commits.insert(oid, commit);
    }

    Ok(commits)
}

fn topo_sort(
    commits: &HashMap<OID, Commit>,
    sort_mode: u32,
    first_parent_only: bool,
) -> Vec<OID> {
    let mut child_counts: HashMap<OID, usize> =
        commits.keys().cloned().map(|oid| (oid, 0usize)).collect();

    for commit in commits.values() {
        for parent in selected_parents(commit, first_parent_only) {
            if let Some(child_count) = child_counts.get_mut(parent) {
                *child_count += 1;
            }
        }
    }

    let mut ready: Vec<OID> = child_counts
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(oid, _)| oid.clone())
        .collect();
    ready.sort_by(|lhs, rhs| compare_commits(lhs, rhs, commits, sort_mode));

    let mut ordered = Vec::with_capacity(commits.len());
    while !ready.is_empty() {
        let oid = ready.remove(0);
        ordered.push(oid.clone());

        let commit = &commits[&oid];
        for parent in selected_parents(commit, first_parent_only) {
            let Some(child_count) = child_counts.get_mut(parent) else {
                continue;
            };
            *child_count -= 1;
            if *child_count == 0 {
                ready.push(parent.clone());
            }
        }

        ready.sort_by(|lhs, rhs| compare_commits(lhs, rhs, commits, sort_mode));
    }

    ordered
}

fn compare_commits(
    lhs: &OID,
    rhs: &OID,
    commits: &HashMap<OID, Commit>,
    sort_mode: u32,
) -> Ordering {
    let use_time = sort_mode == SORT_NONE || sort_mode & SORT_TIME != 0;
    if use_time {
        let lhs_time = commits
            .get(lhs)
            .map(|commit| commit.committer.time)
            .unwrap_or_default();
        let rhs_time = commits
            .get(rhs)
            .map(|commit| commit.committer.time)
            .unwrap_or_default();
        rhs_time
            .cmp(&lhs_time)
            .then_with(|| lhs.hex().cmp(&rhs.hex()))
    } else {
        lhs.hex().cmp(&rhs.hex())
    }
}

fn selected_parents(commit: &Commit, first_parent_only: bool) -> &[OID] {
    if first_parent_only && !commit.parent_ids.is_empty() {
        &commit.parent_ids[..1]
    } else {
        &commit.parent_ids
    }
}

fn merge_bases(git_dir: &Path, left: &OID, right: &OID) -> Result<Vec<OID>, MuonGitError> {
    if left == right {
        return Ok(vec![left.clone()]);
    }

    let left_ancestors = collect_ancestors(git_dir, std::slice::from_ref(left), false)?;

    let mut common = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(right.clone());
    visited.insert(right.clone());

    while let Some(oid) = queue.pop_front() {
        if left_ancestors.contains(&oid) {
            common.push(oid.clone());
            continue;
        }

        let commit = read_commit(git_dir, &oid)?;
        for parent in &commit.parent_ids {
            if visited.insert(parent.clone()) {
                queue.push_back(parent.clone());
            }
        }
    }

    let mut best = common.clone();
    for candidate in &common {
        let candidate_ancestors =
            collect_ancestors(git_dir, std::slice::from_ref(candidate), false)?;
        best.retain(|oid| oid == candidate || !candidate_ancestors.contains(oid));
    }

    best.sort_by_key(|oid| oid.hex());
    best.dedup();
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::serialize_commit;
    use crate::odb::write_loose_object;
    use crate::refs::write_reference;
    use crate::repository::Repository;
    use crate::types::{ObjectType, Signature};

    struct Fixture {
        a: OID,
        b: OID,
        c: OID,
        d: OID,
        e: OID,
    }

    fn test_dir(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp").join(name)
    }

    fn make_commit(
        git_dir: &Path,
        tree_oid: &OID,
        parents: &[&OID],
        time: i64,
        message: &str,
    ) -> OID {
        let sig = Signature {
            name: "Muon Test".into(),
            email: "test@muon.ai".into(),
            time,
            offset: 0,
        };
        let parent_ids: Vec<OID> = parents.iter().map(|oid| (*oid).clone()).collect();
        let data = serialize_commit(tree_oid, &parent_ids, &sig, &sig, message, None);
        write_loose_object(git_dir, ObjectType::Commit, &data).unwrap()
    }

    fn setup_fixture(name: &str) -> (Repository, Fixture) {
        let tmp = test_dir(name);
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = Repository::init(tmp.to_str().unwrap(), false).unwrap();
        let git_dir = repo.git_dir();
        let tree = write_loose_object(git_dir, ObjectType::Tree, &[]).unwrap();

        let a = make_commit(git_dir, &tree, &[], 1, "A\n");
        let b = make_commit(git_dir, &tree, &[&a], 2, "B\n");
        let c = make_commit(git_dir, &tree, &[&b], 3, "C\n");
        let d = make_commit(git_dir, &tree, &[&b], 4, "D\n");
        let e = make_commit(git_dir, &tree, &[&c, &d], 5, "E\n");

        write_reference(git_dir, "refs/heads/main", &e).unwrap();
        write_reference(git_dir, "refs/heads/mainline", &c).unwrap();
        write_reference(git_dir, "refs/heads/feature", &d).unwrap();

        (repo, Fixture { a, b, c, d, e })
    }

    #[test]
    fn test_resolve_revision_expressions() {
        let (repo, fixture) = setup_fixture("test_revwalk_resolve_revision");

        assert_eq!(
            crate::revparse::resolve_revision(repo.git_dir(), "HEAD").unwrap(),
            fixture.e
        );
        assert_eq!(
            crate::revparse::resolve_revision(repo.git_dir(), "mainline").unwrap(),
            fixture.c
        );
        assert_eq!(
            crate::revparse::resolve_revision(repo.git_dir(), &fixture.d.hex()).unwrap(),
            fixture.d
        );
        assert_eq!(
            crate::revparse::resolve_revision(repo.git_dir(), "HEAD~1").unwrap(),
            fixture.c
        );
        assert_eq!(
            crate::revparse::resolve_revision(repo.git_dir(), "HEAD^2").unwrap(),
            fixture.d
        );

        let _ = std::fs::remove_dir_all(repo.workdir().unwrap());
    }

    #[test]
    fn test_revparse_ranges() {
        let (repo, fixture) = setup_fixture("test_revwalk_revparse_ranges");

        let two_dot = crate::revparse::revparse(repo.git_dir(), "mainline..feature").unwrap();
        assert!(two_dot.is_range);
        assert!(!two_dot.uses_merge_base);
        assert_eq!(two_dot.from, Some(fixture.c.clone()));
        assert_eq!(two_dot.to, Some(fixture.d.clone()));

        let three_dot =
            crate::revparse::revparse(repo.git_dir(), "mainline...feature").unwrap();
        assert!(three_dot.is_range);
        assert!(three_dot.uses_merge_base);
        assert_eq!(three_dot.from, Some(fixture.c));
        assert_eq!(three_dot.to, Some(fixture.d));

        let _ = std::fs::remove_dir_all(repo.workdir().unwrap());
    }

    #[test]
    fn test_revwalk_default_order_and_first_parent() {
        let (repo, fixture) = setup_fixture("test_revwalk_default_order");

        let mut walker = Revwalk::new(repo.git_dir());
        walker.push_head().unwrap();
        assert_eq!(
            walker.collect_all().unwrap(),
            vec![
                fixture.e.clone(),
                fixture.d.clone(),
                fixture.c.clone(),
                fixture.b.clone(),
                fixture.a.clone(),
            ]
        );

        let mut first_parent = Revwalk::new(repo.git_dir());
        first_parent.push_head().unwrap();
        first_parent.simplify_first_parent();
        assert_eq!(
            first_parent.collect_all().unwrap(),
            vec![fixture.e, fixture.c, fixture.b, fixture.a]
        );

        let _ = std::fs::remove_dir_all(repo.workdir().unwrap());
    }

    #[test]
    fn test_revwalk_range_semantics() {
        let (repo, fixture) = setup_fixture("test_revwalk_range_semantics");

        let mut two_dot = Revwalk::new(repo.git_dir());
        two_dot.push_range("mainline..feature").unwrap();
        assert_eq!(two_dot.collect_all().unwrap(), vec![fixture.d.clone()]);

        let mut three_dot = Revwalk::new(repo.git_dir());
        three_dot.push_range("mainline...feature").unwrap();
        assert_eq!(
            three_dot.collect_all().unwrap(),
            vec![fixture.d, fixture.c]
        );

        let _ = std::fs::remove_dir_all(repo.workdir().unwrap());
    }

    #[test]
    fn test_revwalk_topological_time_order() {
        let (repo, fixture) = setup_fixture("test_revwalk_topological_order");

        let mut walker = Revwalk::new(repo.git_dir());
        walker.push_head().unwrap();
        walker.sorting(SORT_TOPOLOGICAL | SORT_TIME);

        assert_eq!(
            walker.collect_all().unwrap(),
            vec![fixture.e, fixture.d, fixture.c, fixture.b, fixture.a]
        );

        let _ = std::fs::remove_dir_all(repo.workdir().unwrap());
    }
}
