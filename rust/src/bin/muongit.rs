use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use muongit::blob::read_blob;
use muongit::branch::BranchType;
use muongit::checkout::{ResetMode, RestoreOptions, SwitchOptions};
use muongit::config::Config;
use muongit::diff::{diff_index_to_workdir, diff_stat, diff_trees, format_patch, format_stat};
use muongit::fetch::{CloneOptions, FetchOptions, PushOptions};
use muongit::index::read_index;
use muongit::object::read_object;
use muongit::oid::OID;
use muongit::porcelain::{AddOptions, CommitOptions};
use muongit::refs::{read_reference, resolve_reference};
use muongit::remote::{add_remote, get_remote, list_remotes};
use muongit::remote_transport::{RemoteAuth, TransportOptions};
use muongit::repository::Repository;
use muongit::revparse::resolve_revision;
use muongit::revwalk::{Revwalk, SORT_TIME, SORT_TOPOLOGICAL};
use muongit::status::{workdir_status, FileStatus};
use muongit::tree::{self, TreeEntry};
use muongit::{MuonGitError, Signature};

const HELP: &str = "\
muongit - blessed git-compatible CLI for the Rust port

Usage:
  muongit [-C <path>] <command> [<args>...]

Commands:
  init [--bare] [<path>]               Initialize a repository
  status                               Show short repository status
  add [--include-ignored] [<path>...]  Stage paths
  rm <path>...                         Remove paths from index and worktree
  commit -m <message>                  Create a commit from the current index
  branch [<name> [<start>]]            List or create local branches
  switch [--force] <branch>            Switch to a branch
  switch --detach [--force] <rev>      Detach HEAD at a revision
  reset [--soft|--mixed|--hard] [<rev>] Reset HEAD/index/worktree
  restore [options] <path>...          Restore paths from HEAD or another revision
  log [options] [<rev-or-range>]       Show commit history
  diff [--cached] [--stat]             Show worktree or staged diff
  diff [--stat] <old> <new>            Show diff between two revisions
  remote list                          List remotes
  remote add <name> <url>              Add a remote
  clone [options] <url> <path>         Clone a remote repository
  fetch [options] [<remote>]           Fetch from a remote
  push [options] [<remote>]            Push to a remote
  version                              Show version information
  help [<command>]                     Show help

Global options:
  -C <path>                            Run as if muongit was started in <path>
  --help                               Show help
  --version                            Show version information

Run `muongit help` for the full command list and see docs/cli.md for
output conventions, exit codes, authentication environment variables, and
known gaps.
";

const INIT_HELP: &str = "\
Usage: muongit init [--bare] [<path>]

Initialize a repository at <path>. Defaults to the current directory.
";

const STATUS_HELP: &str = "\
Usage: muongit status

Show short two-column repository status:
  X = staged change against HEAD
  Y = unstaged change in the worktree
  ?? = untracked path
";

const ADD_HELP: &str = "\
Usage: muongit add [--include-ignored] [<path>...]

Stage matching paths. With no paths, stage all tracked and untracked files.
";

const RM_HELP: &str = "\
Usage: muongit rm <path>...

Remove matching paths from the index and worktree.
";

const COMMIT_HELP: &str = "\
Usage: muongit commit -m <message>

Create a commit from the current index. Author and committer default to
repository config user.name/user.email when present and otherwise fall back
to MuonGit defaults. Environment overrides are documented in docs/cli.md.
";

const BRANCH_HELP: &str = "\
Usage:
  muongit branch
  muongit branch <name> [<start>]

List local branches or create a new branch at HEAD or <start>.
";

const SWITCH_HELP: &str = "\
Usage:
  muongit switch [--force] <branch>
  muongit switch --detach [--force] <rev>

Switch to a branch or detach HEAD at a revision.
";

const RESET_HELP: &str = "\
Usage: muongit reset [--soft|--mixed|--hard] [<rev>]

Reset to <rev> (default HEAD). Default mode is --mixed.
";

const RESTORE_HELP: &str = "\
Usage: muongit restore [--source <rev>] [--staged] [--worktree] <path>...

Restore paths from HEAD or another revision. If neither --staged nor
--worktree is provided, --worktree is assumed.
";

const LOG_HELP: &str = "\
Usage: muongit log [--oneline] [--max-count <n>] [--first-parent] [<rev-or-range>]

Walk commits from HEAD by default. Revision ranges like A..B and A...B are
supported.
";

const DIFF_HELP: &str = "\
Usage:
  muongit diff [--cached] [--stat]
  muongit diff [--stat] <old> <new>

Without arguments, diff the index against the worktree.
With --cached, diff HEAD against the index.
With two revisions, diff their commit trees.
";

const REMOTE_HELP: &str = "\
Usage:
  muongit remote list
  muongit remote add <name> <url>

Manage remotes stored in repository config.
";

const CLONE_HELP: &str = "\
Usage: muongit clone [--bare] [--branch <name>] [--remote <name>] [--insecure-skip-tls-verify] <url> <path>

Clone a remote repository. Authentication is configured through the
environment variables documented in docs/cli.md.
";

const FETCH_HELP: &str = "\
Usage: muongit fetch [--refspec <spec>]... [--insecure-skip-tls-verify] [<remote>]

Fetch from a remote. Defaults to origin.
";

const PUSH_HELP: &str = "\
Usage: muongit push [--refspec <spec>]... [--insecure-skip-tls-verify] [<remote>]

Push to a remote. Defaults to origin and uses the current branch when no
refspecs are provided.
";

#[derive(Debug)]
enum CliError {
    Help(String),
    Usage(String),
    Muon(MuonGitError),
    Io(std::io::Error),
    Message(String),
}

impl From<MuonGitError> for CliError {
    fn from(value: MuonGitError) -> Self {
        Self::Muon(value)
    }
}

impl From<std::io::Error> for CliError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

type CliResult<T> = Result<T, CliError>;

#[derive(Debug, Clone)]
struct Args {
    values: Vec<String>,
    pos: usize,
}

impl Args {
    fn new(values: Vec<String>) -> Self {
        Self { values, pos: 0 }
    }

    fn next(&mut self) -> Option<String> {
        let value = self.values.get(self.pos).cloned();
        if value.is_some() {
            self.pos += 1;
        }
        value
    }

    fn peek(&self) -> Option<&str> {
        self.values.get(self.pos).map(String::as_str)
    }

    fn expect_value(&mut self, flag: &str) -> CliResult<String> {
        self.next()
            .ok_or_else(|| CliError::Usage(format!("missing value for {}\n\n{}", flag, HELP)))
    }

    fn rest(&mut self) -> Vec<String> {
        let rest = self.values[self.pos..].to_vec();
        self.pos = self.values.len();
        rest
    }

    fn is_empty(&self) -> bool {
        self.pos >= self.values.len()
    }

    fn ensure_empty(&self, usage: &str) -> CliResult<()> {
        if self.pos == self.values.len() {
            Ok(())
        } else {
            Err(CliError::Usage(format!(
                "unexpected argument '{}'\n\n{}",
                self.values[self.pos], usage
            )))
        }
    }
}

#[derive(Debug, Clone)]
struct SnapshotEntry {
    path: String,
    oid: OID,
    mode: u32,
}

#[derive(Debug, Default, Clone)]
struct StatusColumns {
    staged: Option<char>,
    unstaged: Option<char>,
    untracked: bool,
}

fn main() {
    let code = match run() {
        Ok(()) => 0,
        Err(CliError::Help(msg)) => {
            println!("{msg}");
            0
        }
        Err(CliError::Usage(msg)) => {
            eprintln!("{msg}");
            2
        }
        Err(CliError::Muon(err)) => {
            eprintln!("muongit: {err}");
            1
        }
        Err(CliError::Io(err)) => {
            eprintln!("muongit: io error: {err}");
            1
        }
        Err(CliError::Message(msg)) => {
            eprintln!("muongit: {msg}");
            1
        }
    };
    process::exit(code);
}

fn run() -> CliResult<()> {
    let raw_args: Vec<String> = env::args().skip(1).collect();
    let args = apply_global_flags(raw_args)?;
    if args.is_empty() {
        return Err(CliError::Help(HELP.to_string()));
    }

    let command = &args[0];
    let mut command_args = Args::new(args[1..].to_vec());
    match command.as_str() {
        "--help" | "help" => cmd_help(&mut command_args),
        "--version" | "version" => cmd_version(&mut command_args),
        "init" => cmd_init(&mut command_args),
        "status" => cmd_status(&mut command_args),
        "add" => cmd_add(&mut command_args),
        "rm" => cmd_rm(&mut command_args),
        "commit" => cmd_commit(&mut command_args),
        "branch" => cmd_branch(&mut command_args),
        "switch" => cmd_switch(&mut command_args),
        "reset" => cmd_reset(&mut command_args),
        "restore" => cmd_restore(&mut command_args),
        "log" => cmd_log(&mut command_args),
        "diff" => cmd_diff(&mut command_args),
        "remote" => cmd_remote(&mut command_args),
        "clone" => cmd_clone(&mut command_args),
        "fetch" => cmd_fetch(&mut command_args),
        "push" => cmd_push(&mut command_args),
        other => Err(CliError::Usage(format!(
            "unknown command '{}'\n\n{}",
            other, HELP
        ))),
    }
}

fn apply_global_flags(raw_args: Vec<String>) -> CliResult<Vec<String>> {
    let mut args = Vec::new();
    let mut pos = 0usize;
    while pos < raw_args.len() {
        match raw_args[pos].as_str() {
            "-C" => {
                let Some(path) = raw_args.get(pos + 1) else {
                    return Err(CliError::Usage(format!("missing path after -C\n\n{}", HELP)));
                };
                env::set_current_dir(path)?;
                pos += 2;
            }
            _ => {
                args.extend_from_slice(&raw_args[pos..]);
                break;
            }
        }
    }
    Ok(args)
}

fn cmd_help(args: &mut Args) -> CliResult<()> {
    if args.is_empty() {
        return Err(CliError::Help(HELP.to_string()));
    }
    let topic = args.next().unwrap();
    args.ensure_empty(HELP)?;
    let text = match topic.as_str() {
        "init" => INIT_HELP,
        "status" => STATUS_HELP,
        "add" => ADD_HELP,
        "rm" => RM_HELP,
        "commit" => COMMIT_HELP,
        "branch" => BRANCH_HELP,
        "switch" => SWITCH_HELP,
        "reset" => RESET_HELP,
        "restore" => RESTORE_HELP,
        "log" => LOG_HELP,
        "diff" => DIFF_HELP,
        "remote" => REMOTE_HELP,
        "clone" => CLONE_HELP,
        "fetch" => FETCH_HELP,
        "push" => PUSH_HELP,
        "version" => "Usage: muongit version\n",
        _ => HELP,
    };
    Err(CliError::Help(text.to_string()))
}

fn cmd_version(args: &mut Args) -> CliResult<()> {
    args.ensure_empty("Usage: muongit version\n")?;
    println!("muongit {}", muongit::version::STRING);
    Ok(())
}

fn cmd_init(args: &mut Args) -> CliResult<()> {
    if matches!(args.peek(), Some("--help")) {
        return Err(CliError::Help(INIT_HELP.to_string()));
    }

    let mut bare = false;
    let mut path = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bare" => bare = true,
            _ if arg.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option '{}'\n\n{}", arg, INIT_HELP)));
            }
            _ if path.is_none() => path = Some(arg),
            _ => return Err(CliError::Usage(INIT_HELP.to_string())),
        }
    }

    let path = path.unwrap_or_else(|| ".".to_string());
    let repo = Repository::init(path.clone(), bare)?;
    if bare {
        println!(
            "Initialized empty bare MuonGit repository in {}",
            repo.git_dir().display()
        );
    } else {
        println!(
            "Initialized empty MuonGit repository in {}",
            repo.git_dir().display()
        );
    }
    Ok(())
}

fn cmd_status(args: &mut Args) -> CliResult<()> {
    if matches!(args.peek(), Some("--help")) {
        return Err(CliError::Help(STATUS_HELP.to_string()));
    }
    args.ensure_empty(STATUS_HELP)?;

    let repo = open_repo()?;
    let workdir = require_workdir(&repo)?;
    let head = describe_head(repo.git_dir())?;
    println!("## {}", head);

    let mut statuses = BTreeMap::<String, StatusColumns>::new();
    let head_entries = head_snapshot(repo.git_dir())?;
    let index = read_index(repo.git_dir())?;
    let index_paths: BTreeSet<&str> = index.entries.iter().map(|entry| entry.path.as_str()).collect();

    for entry in &index.entries {
        let cols = statuses.entry(entry.path.clone()).or_default();
        match head_entries.get(&entry.path) {
            Some(head_entry) => {
                if head_entry.oid != entry.oid || head_entry.mode != entry.mode {
                    cols.staged = Some('M');
                }
            }
            None => cols.staged = Some('A'),
        }
    }

    for path in head_entries.keys() {
        if !index_paths.contains(path.as_str()) {
            statuses.entry(path.clone()).or_default().staged = Some('D');
        }
    }

    for entry in workdir_status(repo.git_dir(), workdir)? {
        let cols = statuses.entry(entry.path.clone()).or_default();
        match entry.status {
            FileStatus::Deleted => cols.unstaged = Some('D'),
            FileStatus::Modified => cols.unstaged = Some('M'),
            FileStatus::New => cols.untracked = true,
        }
    }

    if statuses.is_empty() {
        println!("nothing to commit, working tree clean");
        return Ok(());
    }

    for (path, cols) in statuses {
        if cols.untracked && cols.staged.is_none() && cols.unstaged.is_none() {
            println!("?? {}", path);
        } else {
            let x = cols.staged.unwrap_or(' ');
            let y = cols.unstaged.unwrap_or(' ');
            println!("{}{} {}", x, y, path);
        }
    }
    Ok(())
}

fn cmd_add(args: &mut Args) -> CliResult<()> {
    if matches!(args.peek(), Some("--help")) {
        return Err(CliError::Help(ADD_HELP.to_string()));
    }

    let mut include_ignored = false;
    let mut patterns = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--include-ignored" => include_ignored = true,
            _ if arg.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option '{}'\n\n{}", arg, ADD_HELP)));
            }
            _ => patterns.push(arg),
        }
    }

    let repo = open_repo()?;
    let pattern_refs = string_refs(&patterns);
    let result = repo.add(
        &pattern_refs,
        &AddOptions {
            include_ignored,
        },
    )?;

    for path in result.staged_paths {
        println!("A {}", path);
    }
    for path in result.removed_paths {
        println!("D {}", path);
    }
    Ok(())
}

fn cmd_rm(args: &mut Args) -> CliResult<()> {
    if matches!(args.peek(), Some("--help")) {
        return Err(CliError::Help(RM_HELP.to_string()));
    }

    let patterns = args.rest();
    if patterns.is_empty() {
        return Err(CliError::Usage(RM_HELP.to_string()));
    }

    let repo = open_repo()?;
    let pattern_refs = string_refs(&patterns);
    let result = repo.remove(&pattern_refs)?;
    let mut removed = BTreeSet::new();
    removed.extend(result.removed_from_index);
    removed.extend(result.removed_from_workdir);
    for path in removed {
        println!("rm '{}'", path);
    }
    Ok(())
}

fn cmd_commit(args: &mut Args) -> CliResult<()> {
    if matches!(args.peek(), Some("--help")) {
        return Err(CliError::Help(COMMIT_HELP.to_string()));
    }

    let mut message = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-m" | "--message" => message = Some(args.expect_value(arg.as_str())?),
            _ if arg.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option '{}'\n\n{}", arg, COMMIT_HELP)));
            }
            _ => return Err(CliError::Usage(COMMIT_HELP.to_string())),
        }
    }

    let Some(message) = message else {
        return Err(CliError::Usage(COMMIT_HELP.to_string()));
    };

    let repo = open_repo()?;
    let commit_options = resolve_commit_options(&repo)?;
    let result = repo.commit(&message, &commit_options)?;
    println!(
        "[{} {}] {}",
        short_ref_name(&result.reference),
        short_oid(&result.oid),
        result.summary
    );
    Ok(())
}

fn cmd_branch(args: &mut Args) -> CliResult<()> {
    if matches!(args.peek(), Some("--help")) {
        return Err(CliError::Help(BRANCH_HELP.to_string()));
    }

    let mut force = false;
    let mut positionals = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-f" | "--force" => force = true,
            _ if arg.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option '{}'\n\n{}", arg, BRANCH_HELP)));
            }
            _ => positionals.push(arg),
        }
    }

    let repo = open_repo()?;
    match positionals.len() {
        0 => {
            for branch in repo.list_branches(Some(BranchType::Local))? {
                let marker = if branch.is_head { '*' } else { ' ' };
                println!("{} {}", marker, branch.name);
            }
            Ok(())
        }
        1 | 2 => {
            let start_oid = if let Some(start) = positionals.get(1) {
                Some(resolve_revision(repo.git_dir(), start)?)
            } else {
                None
            };
            let branch = repo.create_branch(positionals[0].as_str(), start_oid.as_ref(), force)?;
            println!("{}", branch.name);
            Ok(())
        }
        _ => Err(CliError::Usage(BRANCH_HELP.to_string())),
    }
}

fn cmd_switch(args: &mut Args) -> CliResult<()> {
    if matches!(args.peek(), Some("--help")) {
        return Err(CliError::Help(SWITCH_HELP.to_string()));
    }

    let mut force = false;
    let mut detach = false;
    let mut target = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--force" => force = true,
            "--detach" => detach = true,
            _ if arg.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option '{}'\n\n{}", arg, SWITCH_HELP)));
            }
            _ if target.is_none() => target = Some(arg),
            _ => return Err(CliError::Usage(SWITCH_HELP.to_string())),
        }
    }
    let Some(target) = target else {
        return Err(CliError::Usage(SWITCH_HELP.to_string()));
    };

    let repo = open_repo()?;
    let opts = SwitchOptions { force };
    if detach {
        let result = repo.checkout_revision(&target, &opts)?;
        println!("HEAD is now at {}", short_oid(&result.head_oid));
    } else {
        let _ = repo.switch_branch(&target, &opts)?;
        println!("Switched to branch '{}'", target);
    }
    Ok(())
}

fn cmd_reset(args: &mut Args) -> CliResult<()> {
    if matches!(args.peek(), Some("--help")) {
        return Err(CliError::Help(RESET_HELP.to_string()));
    }

    let mut mode = ResetMode::Mixed;
    let mut spec = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--soft" => mode = ResetMode::Soft,
            "--mixed" => mode = ResetMode::Mixed,
            "--hard" => mode = ResetMode::Hard,
            _ if arg.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option '{}'\n\n{}", arg, RESET_HELP)));
            }
            _ if spec.is_none() => spec = Some(arg),
            _ => return Err(CliError::Usage(RESET_HELP.to_string())),
        }
    }

    let repo = open_repo()?;
    let spec = spec.unwrap_or_else(|| "HEAD".to_string());
    let result = repo.reset(&spec, mode)?;
    match mode {
        ResetMode::Hard => println!("HEAD is now at {}", short_oid(&result.head_oid)),
        ResetMode::Mixed => println!("Unstaged changes after reset to {}", short_oid(&result.head_oid)),
        ResetMode::Soft => println!("Moved HEAD to {}", short_oid(&result.head_oid)),
    }
    Ok(())
}

fn cmd_restore(args: &mut Args) -> CliResult<()> {
    if matches!(args.peek(), Some("--help")) {
        return Err(CliError::Help(RESTORE_HELP.to_string()));
    }

    let mut source = None;
    let mut staged = false;
    let mut worktree = false;
    let mut paths = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--source" => source = Some(args.expect_value("--source")?),
            "--staged" => staged = true,
            "--worktree" => worktree = true,
            _ if arg.starts_with('-') => {
                return Err(CliError::Usage(format!(
                    "unknown option '{}'\n\n{}",
                    arg, RESTORE_HELP
                )));
            }
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        return Err(CliError::Usage(RESTORE_HELP.to_string()));
    }
    if !staged && !worktree {
        worktree = true;
    }

    let repo = open_repo()?;
    let path_refs = string_refs(&paths);
    let result = repo.restore(
        &path_refs,
        &RestoreOptions {
            source,
            staged,
            worktree,
        },
    )?;
    for path in result.staged_paths {
        println!("staged {}", path);
    }
    for path in result.removed_from_index {
        println!("unstaged {}", path);
    }
    for path in result.restored_paths {
        println!("restored {}", path);
    }
    for path in result.removed_from_workdir {
        println!("removed {}", path);
    }
    Ok(())
}

fn cmd_log(args: &mut Args) -> CliResult<()> {
    if matches!(args.peek(), Some("--help")) {
        return Err(CliError::Help(LOG_HELP.to_string()));
    }

    let mut oneline = false;
    let mut first_parent = false;
    let mut max_count = None;
    let mut spec = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--oneline" => oneline = true,
            "--first-parent" => first_parent = true,
            "--max-count" => {
                let raw = args.expect_value("--max-count")?;
                max_count = Some(raw.parse::<usize>().map_err(|_| {
                    CliError::Usage(format!("invalid max count '{}'\n\n{}", raw, LOG_HELP))
                })?);
            }
            _ if arg.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option '{}'\n\n{}", arg, LOG_HELP)));
            }
            _ if spec.is_none() => spec = Some(arg),
            _ => return Err(CliError::Usage(LOG_HELP.to_string())),
        }
    }

    let repo = open_repo()?;
    let mut walk = Revwalk::new(repo.git_dir());
    walk.sorting(SORT_TOPOLOGICAL | SORT_TIME);
    if first_parent {
        walk.simplify_first_parent();
    }
    if let Some(spec) = spec {
        if spec.contains("..") {
            walk.push_range(&spec)?;
        } else {
            walk.push(resolve_revision(repo.git_dir(), &spec)?);
        }
    } else {
        walk.push_head()?;
    }

    let limit = max_count.unwrap_or(usize::MAX);
    for oid in walk.collect_all()?.into_iter().take(limit) {
        let commit = read_object(repo.git_dir(), &oid)?.as_commit()?;
        if oneline {
            println!("{} {}", short_oid(&oid), commit_summary(&commit.message));
        } else {
            println!("commit {}", oid.hex());
            println!("Author: {} <{}>", commit.author.name, commit.author.email);
            println!("Date:   {} {}", commit.author.time, format_offset(commit.author.offset));
            println!();
            for line in commit.message.lines() {
                println!("    {}", line);
            }
            println!();
        }
    }
    Ok(())
}

fn cmd_diff(args: &mut Args) -> CliResult<()> {
    if matches!(args.peek(), Some("--help")) {
        return Err(CliError::Help(DIFF_HELP.to_string()));
    }

    let mut cached = false;
    let mut stat = false;
    let mut revisions = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cached" => cached = true,
            "--stat" => stat = true,
            _ if arg.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option '{}'\n\n{}", arg, DIFF_HELP)));
            }
            _ => revisions.push(arg),
        }
    }

    let repo = open_repo()?;
    match (cached, revisions.len()) {
        (false, 0) => print_workdir_diff(&repo, stat),
        (true, 0) => print_cached_diff(&repo, stat),
        (false, 2) => print_revision_diff(&repo, &revisions[0], &revisions[1], stat),
        _ => Err(CliError::Usage(DIFF_HELP.to_string())),
    }
}

fn cmd_remote(args: &mut Args) -> CliResult<()> {
    if matches!(args.peek(), Some("--help")) {
        return Err(CliError::Help(REMOTE_HELP.to_string()));
    }

    let subcommand = args.next().unwrap_or_else(|| "list".to_string());
    let repo = open_repo()?;
    match subcommand.as_str() {
        "list" => {
            args.ensure_empty(REMOTE_HELP)?;
            for name in list_remotes(repo.git_dir())? {
                let remote = get_remote(repo.git_dir(), &name)?;
                println!("{}\t{}", remote.name, remote.url);
            }
            Ok(())
        }
        "add" => {
            let name = args.expect_value("remote add <name>")?;
            let url = args.expect_value("remote add <url>")?;
            args.ensure_empty(REMOTE_HELP)?;
            let remote = add_remote(repo.git_dir(), &name, &url)?;
            println!("added remote {} {}", remote.name, remote.url);
            Ok(())
        }
        _ => Err(CliError::Usage(REMOTE_HELP.to_string())),
    }
}

fn cmd_clone(args: &mut Args) -> CliResult<()> {
    if matches!(args.peek(), Some("--help")) {
        return Err(CliError::Help(CLONE_HELP.to_string()));
    }

    let mut bare = false;
    let mut branch = None;
    let mut remote_name = "origin".to_string();
    let mut insecure = false;
    let mut positionals = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bare" => bare = true,
            "--branch" => branch = Some(args.expect_value("--branch")?),
            "--remote" => remote_name = args.expect_value("--remote")?,
            "--insecure-skip-tls-verify" => insecure = true,
            _ if arg.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option '{}'\n\n{}", arg, CLONE_HELP)));
            }
            _ => positionals.push(arg),
        }
    }
    if positionals.len() != 2 {
        return Err(CliError::Usage(CLONE_HELP.to_string()));
    }

    let url = &positionals[0];
    let path = &positionals[1];
    Repository::clone_with_options(
        url,
        path,
        &CloneOptions {
            remote_name,
            branch,
            bare,
            transport: transport_from_env(insecure)?,
        },
    )?;
    println!("Cloned {} into {}", url, path);
    Ok(())
}

fn cmd_fetch(args: &mut Args) -> CliResult<()> {
    if matches!(args.peek(), Some("--help")) {
        return Err(CliError::Help(FETCH_HELP.to_string()));
    }

    let mut insecure = false;
    let mut refspecs = Vec::new();
    let mut remote_name = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--refspec" => refspecs.push(args.expect_value("--refspec")?),
            "--insecure-skip-tls-verify" => insecure = true,
            _ if arg.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option '{}'\n\n{}", arg, FETCH_HELP)));
            }
            _ if remote_name.is_none() => remote_name = Some(arg),
            _ => return Err(CliError::Usage(FETCH_HELP.to_string())),
        }
    }

    let repo = open_repo()?;
    let remote_name = remote_name.unwrap_or_else(|| "origin".to_string());
    let result = repo.fetch(
        &remote_name,
        &FetchOptions {
            refspecs: if refspecs.is_empty() { None } else { Some(refspecs) },
            transport: transport_from_env(insecure)?,
        },
    )?;
    println!(
        "Fetched {} ref(s) from {}",
        result.updated_refs, remote_name
    );
    if let Some(pack) = result.indexed_pack {
        println!("Indexed pack {} ({} objects)", pack.pack_name, pack.object_count);
    }
    Ok(())
}

fn cmd_push(args: &mut Args) -> CliResult<()> {
    if matches!(args.peek(), Some("--help")) {
        return Err(CliError::Help(PUSH_HELP.to_string()));
    }

    let mut insecure = false;
    let mut refspecs = Vec::new();
    let mut remote_name = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--refspec" => refspecs.push(args.expect_value("--refspec")?),
            "--insecure-skip-tls-verify" => insecure = true,
            _ if arg.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option '{}'\n\n{}", arg, PUSH_HELP)));
            }
            _ if remote_name.is_none() => remote_name = Some(arg),
            _ => return Err(CliError::Usage(PUSH_HELP.to_string())),
        }
    }

    let repo = open_repo()?;
    let remote_name = remote_name.unwrap_or_else(|| "origin".to_string());
    let result = repo.push(
        &remote_name,
        &PushOptions {
            refspecs: if refspecs.is_empty() { None } else { Some(refspecs) },
            transport: transport_from_env(insecure)?,
        },
    )?;
    print!("{}", result.report);
    if !result.report.ends_with('\n') {
        println!();
    }
    println!(
        "Updated {} tracking ref(s)",
        result.updated_tracking_refs
    );
    Ok(())
}

fn open_repo() -> CliResult<Repository> {
    Ok(Repository::discover(".")?)
}

fn require_workdir(repo: &Repository) -> CliResult<&Path> {
    repo.workdir()
        .ok_or(CliError::Muon(MuonGitError::BareRepo))
}

fn string_refs(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}

fn short_oid(oid: &OID) -> String {
    oid.hex().chars().take(7).collect()
}

fn short_ref_name(reference: &str) -> &str {
    reference
        .strip_prefix("refs/heads/")
        .or_else(|| reference.strip_prefix("refs/remotes/"))
        .unwrap_or(reference)
}

fn describe_head(git_dir: &Path) -> Result<String, MuonGitError> {
    let head = read_reference(git_dir, "HEAD")?;
    if let Some(target) = head.strip_prefix("ref: ") {
        let target = target.trim();
        Ok(target
            .strip_prefix("refs/heads/")
            .unwrap_or(target)
            .to_string())
    } else {
        Ok(format!("HEAD (detached at {})", short_oid(&OID::from_hex(head.trim())?)))
    }
}

fn commit_summary(message: &str) -> String {
    message.lines().next().unwrap_or("").to_string()
}

fn format_offset(offset: i32) -> String {
    let sign = if offset >= 0 { '+' } else { '-' };
    let abs = offset.unsigned_abs();
    let hours = abs / 60;
    let minutes = abs % 60;
    format!("{}{:02}{:02}", sign, hours, minutes)
}

fn resolve_commit_options(repo: &Repository) -> CliResult<CommitOptions> {
    let config = Config::load(&repo.git_dir().join("config")).ok();
    let author = resolve_signature("AUTHOR", config.as_ref(), None);
    let committer = resolve_signature("COMMITTER", config.as_ref(), Some(&author));
    Ok(CommitOptions {
        author: Some(author),
        committer: Some(committer),
    })
}

fn resolve_signature(role: &str, config: Option<&Config>, fallback: Option<&Signature>) -> Signature {
    let env_name = env_var_any(&[
        format!("MUONGIT_{}_NAME", role),
        format!("GIT_{}_NAME", role),
    ])
    .or_else(|| fallback.map(|sig| sig.name.clone()))
    .or_else(|| config.and_then(|cfg| cfg.get("user", "name").map(str::to_string)))
    .unwrap_or_else(|| "MuonGit".to_string());

    let env_email = env_var_any(&[
        format!("MUONGIT_{}_EMAIL", role),
        format!("GIT_{}_EMAIL", role),
    ])
    .or_else(|| fallback.map(|sig| sig.email.clone()))
    .or_else(|| config.and_then(|cfg| cfg.get("user", "email").map(str::to_string)))
    .unwrap_or_else(|| "muongit@example.invalid".to_string());

    let time = env_var_any(&[format!("MUONGIT_{}_TIME", role)])
        .and_then(|raw| raw.parse::<i64>().ok())
        .or_else(|| fallback.map(|sig| sig.time))
        .unwrap_or_else(unix_now);

    let offset = env_var_any(&[format!("MUONGIT_{}_OFFSET", role)])
        .and_then(|raw| parse_offset(&raw))
        .or_else(|| fallback.map(|sig| sig.offset))
        .unwrap_or(0);

    Signature {
        name: env_name,
        email: env_email,
        time,
        offset,
    }
}

fn env_var_any(keys: &[String]) -> Option<String> {
    keys.iter().find_map(|key| env::var(key).ok())
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn parse_offset(raw: &str) -> Option<i32> {
    if let Ok(minutes) = raw.parse::<i32>() {
        return Some(minutes);
    }
    let sign = if raw.starts_with('-') {
        -1
    } else if raw.starts_with('+') {
        1
    } else {
        return None;
    };
    let digits = &raw[1..];
    if digits.len() != 4 {
        return None;
    }
    let hours = digits[..2].parse::<i32>().ok()?;
    let minutes = digits[2..].parse::<i32>().ok()?;
    Some(sign * (hours * 60 + minutes))
}

fn transport_from_env(insecure_skip_tls_verify: bool) -> CliResult<TransportOptions> {
    let auth = if let Ok(token) = env::var("MUONGIT_HTTP_BEARER_TOKEN") {
        Some(RemoteAuth::BearerToken(token))
    } else if let (Ok(username), Ok(password)) = (
        env::var("MUONGIT_HTTP_USERNAME"),
        env::var("MUONGIT_HTTP_PASSWORD"),
    ) {
        Some(RemoteAuth::Basic { username, password })
    } else if let Ok(basic) = env::var("MUONGIT_HTTP_BASIC") {
        let Some((username, password)) = basic.split_once(':') else {
            return Err(CliError::Message(
                "MUONGIT_HTTP_BASIC must be in username:password form".into(),
            ));
        };
        Some(RemoteAuth::Basic {
            username: username.to_string(),
            password: password.to_string(),
        })
    } else if let Ok(private_key) = env::var("MUONGIT_SSH_PRIVATE_KEY") {
        let username = env::var("MUONGIT_SSH_USERNAME")
            .or_else(|_| env::var("USER"))
            .unwrap_or_else(|_| "git".to_string());
        let port = env::var("MUONGIT_SSH_PORT")
            .ok()
            .and_then(|raw| raw.parse::<u16>().ok());
        let strict_host_key_checking = env_bool("MUONGIT_SSH_STRICT_HOST_KEY_CHECKING");
        Some(RemoteAuth::SshKey {
            username,
            private_key,
            port,
            strict_host_key_checking,
        })
    } else if env_bool("MUONGIT_SSH_AGENT") {
        let username = env::var("MUONGIT_SSH_USERNAME")
            .or_else(|_| env::var("USER"))
            .unwrap_or_else(|_| "git".to_string());
        let port = env::var("MUONGIT_SSH_PORT")
            .ok()
            .and_then(|raw| raw.parse::<u16>().ok());
        let strict_host_key_checking = env_bool("MUONGIT_SSH_STRICT_HOST_KEY_CHECKING");
        Some(RemoteAuth::SshAgent {
            username,
            port,
            strict_host_key_checking,
        })
    } else {
        None
    };

    Ok(TransportOptions {
        auth,
        insecure_skip_tls_verify,
    })
}

fn env_bool(key: &str) -> bool {
    env::var(key)
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "on"))
        .unwrap_or(false)
}

fn head_snapshot(git_dir: &Path) -> CliResult<BTreeMap<String, SnapshotEntry>> {
    match resolve_reference(git_dir, "HEAD") {
        Ok(oid) => commit_snapshot(git_dir, &oid),
        Err(MuonGitError::NotFound(_)) => Ok(BTreeMap::new()),
        Err(err) => Err(err.into()),
    }
}

fn commit_snapshot(git_dir: &Path, commit_oid: &OID) -> CliResult<BTreeMap<String, SnapshotEntry>> {
    let commit = read_object(git_dir, commit_oid)?.as_commit()?;
    let mut entries = BTreeMap::new();
    collect_tree_snapshot(git_dir, &commit.tree_id, "", &mut entries)?;
    Ok(entries)
}

fn revision_snapshot(git_dir: &Path, spec: &str) -> CliResult<BTreeMap<String, SnapshotEntry>> {
    let oid = resolve_revision(git_dir, spec)?;
    commit_snapshot(git_dir, &oid)
}

fn collect_tree_snapshot(
    git_dir: &Path,
    tree_oid: &OID,
    prefix: &str,
    entries: &mut BTreeMap<String, SnapshotEntry>,
) -> CliResult<()> {
    let tree = read_object(git_dir, tree_oid)?.as_tree()?;
    for entry in tree.entries {
        let path = if prefix.is_empty() {
            entry.name.clone()
        } else {
            format!("{}/{}", prefix, entry.name)
        };
        if entry.mode == tree::file_mode::TREE {
            collect_tree_snapshot(git_dir, &entry.oid, &path, entries)?;
        } else {
            entries.insert(
                path.clone(),
                SnapshotEntry {
                    path,
                    oid: entry.oid,
                    mode: entry.mode,
                },
            );
        }
    }
    Ok(())
}

fn index_snapshot(git_dir: &Path) -> CliResult<BTreeMap<String, SnapshotEntry>> {
    let mut entries = BTreeMap::new();
    for entry in read_index(git_dir)?.entries {
        entries.insert(
            entry.path.clone(),
            SnapshotEntry {
                path: entry.path,
                oid: entry.oid,
                mode: entry.mode,
            },
        );
    }
    Ok(entries)
}

fn snapshot_to_tree_entries(snapshot: &BTreeMap<String, SnapshotEntry>) -> Vec<TreeEntry> {
    snapshot
        .values()
        .map(|entry| TreeEntry {
            mode: entry.mode,
            name: entry.path.clone(),
            oid: entry.oid.clone(),
        })
        .collect()
}

fn load_blob_text(git_dir: &Path, oid: &OID) -> CliResult<String> {
    let blob = read_blob(git_dir, oid)?;
    Ok(String::from_utf8_lossy(&blob.data).into_owned())
}

fn load_workdir_text(workdir: &Path, path: &str) -> CliResult<String> {
    let bytes = fs::read(workdir.join(path))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn print_workdir_diff(repo: &Repository, stat: bool) -> CliResult<()> {
    let workdir = require_workdir(repo)?;
    let deltas = diff_index_to_workdir(repo.git_dir(), workdir)?;
    if stat {
        let mut stats = Vec::new();
        for delta in deltas {
            let old_text = match delta.old_entry {
                Some(entry) => load_blob_text(repo.git_dir(), &entry.oid)?,
                None => String::new(),
            };
            let new_text = match delta.status {
                muongit::diff::DiffStatus::Deleted => String::new(),
                _ => load_workdir_text(workdir, &delta.path)?,
            };
            stats.push(diff_stat(&delta.path, &old_text, &new_text));
        }
        print!("{}", format_stat(&stats));
        return Ok(());
    }

    for delta in deltas {
        let old_text = match delta.old_entry {
            Some(entry) => load_blob_text(repo.git_dir(), &entry.oid)?,
            None => String::new(),
        };
        let new_text = match delta.status {
            muongit::diff::DiffStatus::Deleted => String::new(),
            _ => load_workdir_text(workdir, &delta.path)?,
        };
        print!("{}", format_patch(&delta.path, &delta.path, &old_text, &new_text, 3));
    }
    Ok(())
}

fn print_cached_diff(repo: &Repository, stat: bool) -> CliResult<()> {
    let head = head_snapshot(repo.git_dir())?;
    let index = index_snapshot(repo.git_dir())?;
    let deltas = diff_trees(&snapshot_to_tree_entries(&head), &snapshot_to_tree_entries(&index));
    print_snapshot_diff(repo.git_dir(), &head, &index, deltas, stat)
}

fn print_revision_diff(repo: &Repository, old_spec: &str, new_spec: &str, stat: bool) -> CliResult<()> {
    let old_snapshot = revision_snapshot(repo.git_dir(), old_spec)?;
    let new_snapshot = revision_snapshot(repo.git_dir(), new_spec)?;
    let deltas = diff_trees(
        &snapshot_to_tree_entries(&old_snapshot),
        &snapshot_to_tree_entries(&new_snapshot),
    );
    print_snapshot_diff(repo.git_dir(), &old_snapshot, &new_snapshot, deltas, stat)
}

fn print_snapshot_diff(
    git_dir: &Path,
    old_snapshot: &BTreeMap<String, SnapshotEntry>,
    new_snapshot: &BTreeMap<String, SnapshotEntry>,
    deltas: Vec<muongit::diff::DiffDelta>,
    stat: bool,
) -> CliResult<()> {
    if stat {
        let mut stats = Vec::new();
        for delta in &deltas {
            let old_text = old_snapshot
                .get(&delta.path)
                .map(|entry| load_blob_text(git_dir, &entry.oid))
                .transpose()?
                .unwrap_or_default();
            let new_text = new_snapshot
                .get(&delta.path)
                .map(|entry| load_blob_text(git_dir, &entry.oid))
                .transpose()?
                .unwrap_or_default();
            stats.push(diff_stat(&delta.path, &old_text, &new_text));
        }
        print!("{}", format_stat(&stats));
        return Ok(());
    }

    for delta in deltas {
        let old_text = old_snapshot
            .get(&delta.path)
            .map(|entry| load_blob_text(git_dir, &entry.oid))
            .transpose()?
            .unwrap_or_default();
        let new_text = new_snapshot
            .get(&delta.path)
            .map(|entry| load_blob_text(git_dir, &entry.oid))
            .transpose()?
            .unwrap_or_default();
        print!("{}", format_patch(&delta.path, &delta.path, &old_text, &new_text, 3));
    }
    Ok(())
}
