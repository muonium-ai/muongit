use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use muongit::branch::BranchType;
use muongit::checkout::{ResetMode, RestoreOptions, SwitchOptions};
use muongit::diff;
use muongit::fetch::{CloneOptions, FetchOptions, PushOptions};
use muongit::index::read_index;
use muongit::object::read_object;
use muongit::patch::Patch;
use muongit::porcelain::{AddOptions, CommitOptions};
use muongit::refs;
use muongit::remote::{get_remote, list_remotes};
use muongit::remote_transport::{RemoteAuth, TransportOptions};
use muongit::repository::Repository;
use muongit::revparse::resolve_revision;
use muongit::revwalk::{Revwalk, SORT_TIME, SORT_TOPOLOGICAL};
use muongit::status::{self, FileStatus};
use muongit::{Signature, OID};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return Err("usage: muongit-conformance <write-scenario|snapshot> ...".into());
    }

    match args[0].as_str() {
        "write-scenario" => {
            if args.len() != 3 {
                return Err(
                    "usage: muongit-conformance write-scenario <root> <fixture-script>".into(),
                );
            }
            let checkpoints =
                write_scenario(Path::new(&args[1]), Path::new(&args[2])).map_err(format_error)?;
            print_checkpoints_json(&checkpoints);
            Ok(())
        }
        "snapshot" => {
            if args.len() != 2 {
                return Err("usage: muongit-conformance snapshot <repo>".into());
            }
            let snapshot = snapshot_repository(Path::new(&args[1])).map_err(format_error)?;
            print_snapshot_json(&snapshot);
            Ok(())
        }
        other => Err(format!("unknown command: {other}")),
    }
}

fn format_error(error: muongit::MuonGitError) -> String {
    format!("{error:?}")
}

#[derive(Clone)]
struct Checkpoint {
    name: &'static str,
    repo: PathBuf,
}

struct Snapshot {
    repo_kind: String,
    head: String,
    head_oid: String,
    refs: Vec<String>,
    local_branches: Vec<String>,
    remote_branches: Vec<String>,
    remotes: Vec<String>,
    revisions: Vec<String>,
    walks: Vec<String>,
    head_commit: String,
    tree_entries: Vec<String>,
    worktree_files: Vec<String>,
    index_entries: Vec<String>,
    status: Vec<String>,
    hello_patch: String,
}

fn write_scenario(
    root: &Path,
    fixture_script: &Path,
) -> Result<Vec<Checkpoint>, muongit::MuonGitError> {
    let _ = fs::remove_dir_all(root);
    fs::create_dir_all(root)?;
    let checkpoints_root = root.join("checkpoints");
    fs::create_dir_all(&checkpoints_root)?;

    let base_repo_path = root.join("workspace");
    let repo = Repository::init(base_repo_path.to_string_lossy().to_string(), false)?;
    let workdir = repo
        .workdir()
        .ok_or(muongit::MuonGitError::BareRepo)?
        .to_path_buf();

    write_text(workdir.join("hello.txt"), "hello base\n")?;
    write_text(workdir.join("docs/guide.txt"), "guide v1\n")?;
    write_text(workdir.join("remove-me.txt"), "remove me\n")?;
    repo.add(
        &["hello.txt", "docs/guide.txt", "remove-me.txt"],
        &AddOptions::default(),
    )?;
    repo.commit("initial", &commit_options(1))?;

    repo.create_branch("feature", None, false)?;
    repo.switch_branch("feature", &SwitchOptions::default())?;

    write_text(workdir.join("hello.txt"), "hello feature\n")?;
    write_text(workdir.join("notes/ideas.txt"), "ideas v1\n")?;
    repo.remove(&["remove-me.txt"])?;
    repo.add(&["hello.txt", "notes/ideas.txt"], &AddOptions::default())?;
    repo.commit("feature-work", &commit_options(2))?;

    let old_hello = fs::read_to_string(workdir.join("hello.txt"))?;
    let patch = Patch::from_text(
        Some("hello.txt"),
        Some("hello.txt"),
        &old_hello,
        "hello patched\nfeature line\n",
        3,
    );
    repo.apply_patch(&patch)?;
    repo.add(&["hello.txt"], &AddOptions::default())?;
    repo.commit("patch-apply", &commit_options(3))?;

    let feature_clean = checkpoints_root.join("feature-clean");
    copy_dir(&base_repo_path, &feature_clean)?;

    let detached_checkout = checkpoints_root.join("detached-checkout");
    copy_dir(&feature_clean, &detached_checkout)?;
    let detached_repo = Repository::open(detached_checkout.to_string_lossy().to_string())?;
    detached_repo.checkout_revision("HEAD~1", &SwitchOptions::default())?;
    detached_repo.create_branch("detached-copy", None, false)?;

    let restore_dirty = checkpoints_root.join("restore-dirty");
    copy_dir(&feature_clean, &restore_dirty)?;
    let restore_repo = Repository::open(restore_dirty.to_string_lossy().to_string())?;
    let restore_workdir = restore_repo
        .workdir()
        .ok_or(muongit::MuonGitError::BareRepo)?
        .to_path_buf();
    write_text(restore_workdir.join("hello.txt"), "hello dirty\n")?;
    write_text(restore_workdir.join("staged-only.txt"), "staged only\n")?;
    restore_repo.add(&["hello.txt", "staged-only.txt"], &AddOptions::default())?;
    restore_repo.restore(
        &["hello.txt"],
        &RestoreOptions {
            source: None,
            staged: true,
            worktree: true,
        },
    )?;
    write_text(restore_workdir.join("scratch.txt"), "scratch\n")?;

    let reset_hard = checkpoints_root.join("reset-hard");
    copy_dir(&feature_clean, &reset_hard)?;
    let reset_repo = Repository::open(reset_hard.to_string_lossy().to_string())?;
    reset_repo.reset("HEAD~1", ResetMode::Hard)?;

    let remote_root = checkpoints_root.join("remote-scenario");
    fs::create_dir_all(&remote_root)?;
    let fixture = GitFixture::new(&remote_root)?;
    let process = FixtureProcess::http(fixture_script, &fixture.remote_git_dir, "alice", "s3cret")?;

    let remote_clone = checkpoints_root.join("remote-clone");
    let remote_repo = Repository::clone_with_options(
        &process.url,
        remote_clone.to_string_lossy().to_string(),
        &CloneOptions {
            transport: basic_auth(),
            ..CloneOptions::default()
        },
    )?;
    fixture.commit_and_push("hello.txt", "hello remote\n", "remote update")?;
    remote_repo.fetch(
        "origin",
        &FetchOptions {
            transport: basic_auth(),
            ..FetchOptions::default()
        },
    )?;
    remote_repo.reset("refs/remotes/origin/main", ResetMode::Hard)?;
    let remote_workdir = remote_repo
        .workdir()
        .ok_or(muongit::MuonGitError::BareRepo)?
        .to_path_buf();
    write_text(remote_workdir.join("local.txt"), "local push\n")?;
    remote_repo.add(&["local.txt"], &AddOptions::default())?;
    remote_repo.commit("local push", &commit_options(4))?;
    remote_repo.push(
        "origin",
        &PushOptions {
            transport: basic_auth(),
            ..PushOptions::default()
        },
    )?;
    process.stop();

    Ok(vec![
        Checkpoint {
            name: "feature-clean",
            repo: feature_clean,
        },
        Checkpoint {
            name: "detached-checkout",
            repo: detached_checkout,
        },
        Checkpoint {
            name: "restore-dirty",
            repo: restore_dirty,
        },
        Checkpoint {
            name: "reset-hard",
            repo: reset_hard,
        },
        Checkpoint {
            name: "remote-clone",
            repo: remote_clone,
        },
        Checkpoint {
            name: "remote-bare",
            repo: fixture.remote_git_dir,
        },
    ])
}

fn snapshot_repository(path: &Path) -> Result<Snapshot, muongit::MuonGitError> {
    let repo = Repository::open(path.to_string_lossy().to_string())?;
    let git_dir = repo.git_dir();
    let workdir = repo.workdir().map(Path::to_path_buf);
    let refdb = repo.refdb();
    let head_raw = repo.head().unwrap_or_default();
    let head_oid = refs::resolve_reference(git_dir, "HEAD")
        .map(|oid| oid.hex())
        .unwrap_or_default();

    let refs = snapshot_refs(&refdb)?;
    let local_branches = snapshot_branches(&repo, Some(BranchType::Local))?;
    let remote_branches = snapshot_branches(&repo, Some(BranchType::Remote))?;
    let remotes = snapshot_remotes(git_dir);
    let revisions = snapshot_revisions(git_dir);
    let walks = snapshot_walks(git_dir);
    let head_commit = snapshot_head_commit(git_dir, &head_oid)?;
    let tree_entries = snapshot_tree_entries(git_dir, &head_oid)?;
    let worktree_files = snapshot_workdir_files(workdir.as_deref())?;
    let index_entries = snapshot_index_entries(git_dir)?;
    let status = snapshot_status(git_dir, workdir.as_deref())?;
    let hello_patch = snapshot_hello_patch(git_dir)?;

    Ok(Snapshot {
        repo_kind: if repo.is_bare() { "bare" } else { "worktree" }.to_string(),
        head: head_raw,
        head_oid,
        refs,
        local_branches,
        remote_branches,
        remotes,
        revisions,
        walks,
        head_commit,
        tree_entries,
        worktree_files,
        index_entries,
        status,
        hello_patch,
    })
}

fn snapshot_refs(refdb: &muongit::refdb::RefDb) -> Result<Vec<String>, muongit::MuonGitError> {
    let mut refs = Vec::new();
    for reference in refdb.list()? {
        refs.push(format!("{}|{}", reference.name, reference.value));
    }
    refs.sort();
    Ok(refs)
}

fn snapshot_branches(
    repo: &Repository,
    kind: Option<BranchType>,
) -> Result<Vec<String>, muongit::MuonGitError> {
    let mut lines = Vec::new();
    for branch in repo.list_branches(kind)? {
        let target = branch.target.map(|oid| oid.hex()).unwrap_or_default();
        let upstream = branch
            .upstream
            .map(|value| format!("{}/{}", value.remote_name, value.merge_ref))
            .unwrap_or_default();
        lines.push(format!(
            "{}|{}|{}|{}",
            branch.name,
            target,
            if branch.is_head { "head" } else { "" },
            upstream
        ));
    }
    lines.sort();
    Ok(lines)
}

fn snapshot_remotes(git_dir: &Path) -> Vec<String> {
    let mut remotes = Vec::new();
    if let Ok(names) = list_remotes(git_dir) {
        for name in names {
            if let Ok(remote) = get_remote(git_dir, &name) {
                remotes.push(format!("{}|{}", remote.name, remote.url));
            }
        }
    }
    remotes.sort();
    remotes
}

fn snapshot_revisions(git_dir: &Path) -> Vec<String> {
    let specs = [
        "HEAD",
        "HEAD~1",
        "main",
        "feature",
        "detached-copy",
        "refs/remotes/origin/main",
    ];
    let mut values = Vec::new();
    for spec in specs {
        let value = resolve_revision(git_dir, spec)
            .map(|oid| oid.hex())
            .unwrap_or_else(|_| "!".to_string());
        values.push(format!("{spec}|{value}"));
    }
    values
}

fn snapshot_walks(git_dir: &Path) -> Vec<String> {
    let mut lines = Vec::new();

    let mut head = Revwalk::new(git_dir);
    if head.push_head().is_ok() {
        lines.push(format!(
            "HEAD|{}",
            join_oids(head.collect_all().unwrap_or_default())
        ));
    } else {
        lines.push("HEAD|!".into());
    }

    let mut first_parent = Revwalk::new(git_dir);
    if first_parent.push_head().is_ok() {
        first_parent.simplify_first_parent();
        lines.push(format!(
            "HEAD:first-parent|{}",
            join_oids(first_parent.collect_all().unwrap_or_default())
        ));
    } else {
        lines.push("HEAD:first-parent|!".into());
    }

    let mut topo = Revwalk::new(git_dir);
    if topo.push_head().is_ok() {
        topo.sorting(SORT_TOPOLOGICAL | SORT_TIME);
        lines.push(format!(
            "HEAD:topo-time|{}",
            join_oids(topo.collect_all().unwrap_or_default())
        ));
    } else {
        lines.push("HEAD:topo-time|!".into());
    }

    for range in ["main..feature", "main...feature"] {
        let mut walk = Revwalk::new(git_dir);
        if walk.push_range(range).is_ok() {
            lines.push(format!(
                "{range}|{}",
                join_oids(walk.collect_all().unwrap_or_default())
            ));
        } else {
            lines.push(format!("{range}|!"));
        }
    }

    lines
}

fn snapshot_head_commit(git_dir: &Path, head_oid: &str) -> Result<String, muongit::MuonGitError> {
    if head_oid.is_empty() {
        return Ok(String::new());
    }
    let oid = OID::from_hex(head_oid)?;
    let commit = read_object(git_dir, &oid)?.as_commit()?;
    let parents = commit
        .parent_ids
        .iter()
        .map(OID::hex)
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "{}|{}|{}|{}",
        commit.oid.hex(),
        commit.tree_id.hex(),
        parents,
        hex(commit.message.as_bytes())
    ))
}

fn snapshot_tree_entries(
    git_dir: &Path,
    head_oid: &str,
) -> Result<Vec<String>, muongit::MuonGitError> {
    if head_oid.is_empty() {
        return Ok(Vec::new());
    }
    let oid = OID::from_hex(head_oid)?;
    let commit = read_object(git_dir, &oid)?.as_commit()?;
    let mut entries = Vec::new();
    collect_tree_entries(git_dir, &commit.tree_id, "", &mut entries)?;
    entries.sort();
    Ok(entries)
}

fn collect_tree_entries(
    git_dir: &Path,
    tree_oid: &OID,
    prefix: &str,
    out: &mut Vec<String>,
) -> Result<(), muongit::MuonGitError> {
    let tree = read_object(git_dir, tree_oid)?.as_tree()?;
    for entry in tree.entries {
        let path = if prefix.is_empty() {
            entry.name.clone()
        } else {
            format!("{prefix}/{}", entry.name)
        };
        if entry.is_tree() {
            collect_tree_entries(git_dir, &entry.oid, &path, out)?;
        } else {
            let blob = read_object(git_dir, &entry.oid)?.as_blob()?;
            out.push(format!(
                "{:o}|{}|{}|{}",
                entry.mode,
                path,
                entry.oid.hex(),
                hex(&blob.data)
            ));
        }
    }
    Ok(())
}

fn snapshot_workdir_files(workdir: Option<&Path>) -> Result<Vec<String>, muongit::MuonGitError> {
    let Some(workdir) = workdir else {
        return Ok(Vec::new());
    };
    let mut files = Vec::new();
    collect_workdir_files(workdir, workdir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_workdir_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<String>,
) -> Result<(), muongit::MuonGitError> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.file_name().map(|name| name == ".git").unwrap_or(false) {
            continue;
        }
        if path.is_dir() {
            collect_workdir_files(root, &path, out)?;
        } else {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| muongit::MuonGitError::InvalidObject("path prefix".into()))?;
            out.push(format!(
                "{}|{}",
                relative.to_string_lossy(),
                hex(&fs::read(&path)?)
            ));
        }
    }
    Ok(())
}

fn snapshot_index_entries(git_dir: &Path) -> Result<Vec<String>, muongit::MuonGitError> {
    let mut entries = read_index(git_dir)?
        .entries
        .into_iter()
        .map(|entry| format!("{:o}|{}|{}", entry.mode, entry.path, entry.oid.hex()))
        .collect::<Vec<_>>();
    entries.sort();
    Ok(entries)
}

fn snapshot_status(
    git_dir: &Path,
    workdir: Option<&Path>,
) -> Result<Vec<String>, muongit::MuonGitError> {
    let Some(workdir) = workdir else {
        return Ok(Vec::new());
    };

    let head_entries = head_index_entries(git_dir)?;
    let index_entries = read_index(git_dir)?;
    let mut staged = BTreeMap::new();
    let mut paths = BTreeSet::new();

    for (path, head_entry) in &head_entries {
        paths.insert(path.clone());
        match index_entries.find(path) {
            None => {
                staged.insert(path.clone(), 'D');
            }
            Some(index_entry)
                if index_entry.oid != head_entry.oid || index_entry.mode != head_entry.mode =>
            {
                staged.insert(path.clone(), 'M');
            }
            _ => {}
        }
    }
    for entry in &index_entries.entries {
        paths.insert(entry.path.clone());
        if !head_entries.contains_key(&entry.path) {
            staged.insert(entry.path.clone(), 'A');
        }
    }

    let mut unstaged = BTreeMap::new();
    for entry in status::workdir_status(git_dir, workdir)? {
        let code = match entry.status {
            FileStatus::Deleted => 'D',
            FileStatus::New => '?',
            FileStatus::Modified => 'M',
        };
        paths.insert(entry.path.clone());
        unstaged.insert(entry.path, code);
    }

    let mut lines = Vec::new();
    for path in paths {
        let staged_code = staged.get(&path).copied().unwrap_or(' ');
        let worktree_code = unstaged.get(&path).copied().unwrap_or(' ');
        let both = if staged_code == ' ' && worktree_code == '?' {
            "??".to_string()
        } else {
            format!("{staged_code}{worktree_code}")
        };
        if both.trim().is_empty() {
            continue;
        }
        lines.push(format!("{both}|{path}"));
    }
    Ok(lines)
}

fn snapshot_hello_patch(git_dir: &Path) -> Result<String, muongit::MuonGitError> {
    let head = match resolve_revision(git_dir, "HEAD") {
        Ok(oid) => oid,
        Err(_) => return Ok(String::new()),
    };
    let previous = match resolve_revision(git_dir, "HEAD~1") {
        Ok(oid) => oid,
        Err(_) => return Ok(String::new()),
    };
    let old_text = tree_blob_text(git_dir, &previous, "hello.txt")?;
    let new_text = tree_blob_text(git_dir, &head, "hello.txt")?;
    if old_text == new_text {
        return Ok(String::new());
    }
    let patch_text = diff::format_patch("hello.txt", "hello.txt", &old_text, &new_text, 3);
    let patch = Patch::parse(&patch_text)?;
    Ok(hex(patch.format().as_bytes()))
}

fn tree_blob_text(
    git_dir: &Path,
    commit_oid: &OID,
    path: &str,
) -> Result<String, muongit::MuonGitError> {
    let commit = read_object(git_dir, commit_oid)?.as_commit()?;
    let entries = materialize_tree_map(git_dir, &commit.tree_id, "")?;
    let blob_oid = entries
        .get(path)
        .ok_or_else(|| muongit::MuonGitError::NotFound(path.into()))?;
    let blob = read_object(git_dir, blob_oid)?.as_blob()?;
    Ok(String::from_utf8_lossy(&blob.data).into_owned())
}

fn materialize_tree_map(
    git_dir: &Path,
    tree_oid: &OID,
    prefix: &str,
) -> Result<BTreeMap<String, OID>, muongit::MuonGitError> {
    let mut map = BTreeMap::new();
    let tree = read_object(git_dir, tree_oid)?.as_tree()?;
    for entry in tree.entries {
        let path = if prefix.is_empty() {
            entry.name.clone()
        } else {
            format!("{prefix}/{}", entry.name)
        };
        if entry.is_tree() {
            map.extend(materialize_tree_map(git_dir, &entry.oid, &path)?);
        } else {
            map.insert(path, entry.oid);
        }
    }
    Ok(map)
}

fn head_index_entries(
    git_dir: &Path,
) -> Result<BTreeMap<String, muongit::index::IndexEntry>, muongit::MuonGitError> {
    let mut entries = BTreeMap::new();
    let head_oid = match refs::resolve_reference(git_dir, "HEAD") {
        Ok(oid) => oid,
        Err(_) => return Ok(entries),
    };
    let commit = read_object(git_dir, &head_oid)?.as_commit()?;
    collect_head_entries(git_dir, &commit.tree_id, "", &mut entries)?;
    Ok(entries)
}

fn collect_head_entries(
    git_dir: &Path,
    tree_oid: &OID,
    prefix: &str,
    out: &mut BTreeMap<String, muongit::index::IndexEntry>,
) -> Result<(), muongit::MuonGitError> {
    let tree = read_object(git_dir, tree_oid)?.as_tree()?;
    for entry in tree.entries {
        let path = if prefix.is_empty() {
            entry.name.clone()
        } else {
            format!("{prefix}/{}", entry.name)
        };
        if entry.is_tree() {
            collect_head_entries(git_dir, &entry.oid, &path, out)?;
        } else {
            let blob = read_object(git_dir, &entry.oid)?.as_blob()?;
            out.insert(
                path.clone(),
                muongit::index::IndexEntry {
                    ctime_secs: 0,
                    ctime_nanos: 0,
                    mtime_secs: 0,
                    mtime_nanos: 0,
                    dev: 0,
                    ino: 0,
                    mode: entry.mode,
                    uid: 0,
                    gid: 0,
                    file_size: blob.data.len() as u32,
                    oid: entry.oid.clone(),
                    flags: 0,
                    path,
                },
            );
        }
    }
    Ok(())
}

fn commit_options(time: i64) -> CommitOptions {
    let signature = Signature {
        name: "Muon Conformance".into(),
        email: "conformance@muon.ai".into(),
        time,
        offset: 0,
    };
    CommitOptions {
        author: Some(signature.clone()),
        committer: Some(signature),
    }
}

fn basic_auth() -> TransportOptions {
    TransportOptions {
        auth: Some(RemoteAuth::Basic {
            username: "alice".into(),
            password: "s3cret".into(),
        }),
        insecure_skip_tls_verify: false,
    }
}

fn write_text(path: PathBuf, content: &str) -> Result<(), muongit::MuonGitError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), muongit::MuonGitError> {
    let _ = fs::remove_dir_all(dst);
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            fs::copy(from, to)?;
        }
    }
    Ok(())
}

struct GitFixture {
    remote_git_dir: PathBuf,
    seed_workdir: PathBuf,
}

impl GitFixture {
    fn new(root: &Path) -> Result<Self, muongit::MuonGitError> {
        let remote_git_dir = root.join("remote.git");
        let seed_workdir = root.join("seed");
        git(root, &["init", "--bare", remote_git_dir.to_str().unwrap()])?;
        git(root, &["init", seed_workdir.to_str().unwrap()])?;
        git(&seed_workdir, &["config", "user.name", "MuonGit Fixture"])?;
        git(&seed_workdir, &["config", "user.email", "fixture@muon.ai"])?;
        write_text(seed_workdir.join("hello.txt"), "hello\n")?;
        git(&seed_workdir, &["add", "hello.txt"])?;
        git(&seed_workdir, &["commit", "-m", "initial"])?;
        git(&seed_workdir, &["branch", "-M", "main"])?;
        git(
            &seed_workdir,
            &["remote", "add", "origin", remote_git_dir.to_str().unwrap()],
        )?;
        git(&seed_workdir, &["push", "origin", "main"])?;
        git(
            root,
            &[
                "--git-dir",
                remote_git_dir.to_str().unwrap(),
                "symbolic-ref",
                "HEAD",
                "refs/heads/main",
            ],
        )?;
        Ok(Self {
            remote_git_dir,
            seed_workdir,
        })
    }

    fn commit_and_push(
        &self,
        file_name: &str,
        contents: &str,
        message: &str,
    ) -> Result<(), muongit::MuonGitError> {
        write_text(self.seed_workdir.join(file_name), contents)?;
        git(&self.seed_workdir, &["add", file_name])?;
        git(&self.seed_workdir, &["commit", "-m", message])?;
        git(&self.seed_workdir, &["push", "origin", "main"])?;
        git(
            self.remote_git_dir
                .parent()
                .unwrap_or_else(|| Path::new(".")),
            &[
                "--git-dir",
                self.remote_git_dir.to_str().unwrap(),
                "symbolic-ref",
                "HEAD",
                "refs/heads/main",
            ],
        )?;
        Ok(())
    }
}

struct FixtureProcess {
    child: Child,
    url: String,
}

impl FixtureProcess {
    fn http(
        fixture_script: &Path,
        repo: &Path,
        username: &str,
        secret: &str,
    ) -> Result<Self, muongit::MuonGitError> {
        let mut command = Command::new("python3");
        command
            .arg(fixture_script)
            .arg("serve-http")
            .arg("--repo")
            .arg(repo)
            .arg("--auth")
            .arg("basic")
            .arg("--username")
            .arg(username)
            .arg("--secret")
            .arg(secret)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = command.spawn().map_err(muongit::MuonGitError::Io)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| muongit::MuonGitError::Invalid("missing fixture stdout".into()))?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(muongit::MuonGitError::Io)?;
        let url = line
            .split('"')
            .nth(3)
            .ok_or_else(|| muongit::MuonGitError::Invalid("unexpected fixture output".into()))?
            .to_string();
        Ok(Self { child, url })
    }

    fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn git(cwd: &Path, args: &[&str]) -> Result<(), muongit::MuonGitError> {
    let status = Command::new("/usr/bin/git")
        .current_dir(cwd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .map_err(muongit::MuonGitError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(muongit::MuonGitError::Invalid(format!(
            "git command failed: {:?}",
            args
        )))
    }
}

fn join_oids(oids: Vec<OID>) -> String {
    oids.into_iter()
        .map(|oid| oid.hex())
        .collect::<Vec<_>>()
        .join(",")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn escape_json(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn print_checkpoints_json(checkpoints: &[Checkpoint]) {
    println!("{{\"checkpoints\":[");
    for (index, checkpoint) in checkpoints.iter().enumerate() {
        let comma = if index + 1 == checkpoints.len() {
            ""
        } else {
            ","
        };
        println!(
            "  {{\"name\":\"{}\",\"repo\":\"{}\"}}{}",
            escape_json(checkpoint.name),
            escape_json(&checkpoint.repo.to_string_lossy()),
            comma
        );
    }
    println!("]}}");
}

fn print_snapshot_json(snapshot: &Snapshot) {
    println!("{{");
    print_json_field("repo_kind", &snapshot.repo_kind, true);
    print_json_field("head", &snapshot.head, true);
    print_json_field("head_oid", &snapshot.head_oid, true);
    print_json_array("refs", &snapshot.refs, true);
    print_json_array("local_branches", &snapshot.local_branches, true);
    print_json_array("remote_branches", &snapshot.remote_branches, true);
    print_json_array("remotes", &snapshot.remotes, true);
    print_json_array("revisions", &snapshot.revisions, true);
    print_json_array("walks", &snapshot.walks, true);
    print_json_field("head_commit", &snapshot.head_commit, true);
    print_json_array("tree_entries", &snapshot.tree_entries, true);
    print_json_array("worktree_files", &snapshot.worktree_files, true);
    print_json_array("index_entries", &snapshot.index_entries, true);
    print_json_array("status", &snapshot.status, true);
    print_json_field("hello_patch", &snapshot.hello_patch, false);
    println!("}}");
}

fn print_json_field(name: &str, value: &str, trailing_comma: bool) {
    let comma = if trailing_comma { "," } else { "" };
    println!("  \"{}\":\"{}\"{}", name, escape_json(value), comma);
}

fn print_json_array(name: &str, values: &[String], trailing_comma: bool) {
    let comma = if trailing_comma { "," } else { "" };
    println!("  \"{}\":[", name);
    for (index, value) in values.iter().enumerate() {
        let item_comma = if index + 1 == values.len() { "" } else { "," };
        println!("    \"{}\"{}", escape_json(value), item_comma);
    }
    println!("  ]{}", comma);
}
