use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

#[test]
fn cli_local_workflow_smoke() {
    let root = test_dir("cli_local_workflow");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let repo = root.join("repo");
    run_ok(&root, &["init", repo.to_str().unwrap()]);

    write_file(repo.join("hello.txt"), "hello\n");
    let status = run_ok(&repo, &["status"]);
    assert!(status.contains("## main"));
    assert!(status.contains("?? hello.txt"));

    run_ok(&repo, &["add", "hello.txt"]);
    let staged = run_ok(&repo, &["status"]);
    assert!(staged.contains("A  hello.txt"));

    let cached_diff = run_ok(&repo, &["diff", "--cached"]);
    assert!(cached_diff.contains("+++ b/hello.txt"));
    assert!(cached_diff.contains("+hello"));

    let commit = run_ok_env(
        &repo,
        &["commit", "-m", "initial"],
        &[("MUONGIT_AUTHOR_NAME", "MuonGit Test"), ("MUONGIT_AUTHOR_EMAIL", "muongit@example.com")],
    );
    assert!(commit.contains("[main"));
    assert!(commit.contains("initial"));

    run_ok(&repo, &["branch", "feature"]);
    let branches = run_ok(&repo, &["branch"]);
    assert!(branches.contains("* main"));
    assert!(branches.contains("  feature"));

    let switched = run_ok(&repo, &["switch", "feature"]);
    assert!(switched.contains("Switched to branch 'feature'"));

    write_file(repo.join("hello.txt"), "hello feature\n");
    let diff = run_ok(&repo, &["diff"]);
    assert!(diff.contains("--- a/hello.txt"));
    assert!(diff.contains("+hello feature"));

    run_ok(&repo, &["add", "hello.txt"]);
    let after_add = run_ok(&repo, &["status"]);
    assert!(after_add.contains("M  hello.txt"));

    let reset = run_ok(&repo, &["reset", "--mixed", "HEAD"]);
    assert!(reset.contains("Unstaged changes after reset"));
    let after_reset = run_ok(&repo, &["status"]);
    assert!(after_reset.contains(" M hello.txt"));

    run_ok(&repo, &["restore", "hello.txt"]);
    assert_eq!(fs::read_to_string(repo.join("hello.txt")).unwrap(), "hello\n");

    write_file(repo.join("hello.txt"), "hello feature\n");
    run_ok(&repo, &["add", "hello.txt"]);
    run_ok_env(
        &repo,
        &["commit", "-m", "feature work"],
        &[("MUONGIT_AUTHOR_NAME", "MuonGit Test"), ("MUONGIT_AUTHOR_EMAIL", "muongit@example.com")],
    );

    let log = run_ok(&repo, &["log", "--oneline", "--max-count", "2"]);
    assert!(log.contains("feature work"));
    assert!(log.contains("initial"));
}

#[test]
fn cli_http_remote_round_trip_smoke() {
    if !have_tools(&["git", "python3", "curl"]) {
        return;
    }

    let root = test_dir("cli_http_remote_round_trip");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let remote = root.join("remote.git");
    run_ok(&root, &["init", "--bare", remote.to_str().unwrap()]);
    fs::write(
        remote.join("config"),
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = true\n[http]\n\treceivepack = true\n",
    )
    .unwrap();
    let fixture = FixtureProcess::http(&remote);

    let seed = root.join("seed");
    run_ok(&root, &["init", seed.to_str().unwrap()]);
    write_file(seed.join("hello.txt"), "hello\n");
    run_ok(&seed, &["add", "hello.txt"]);
    run_ok_env(
        &seed,
        &["commit", "-m", "initial"],
        &[("MUONGIT_AUTHOR_NAME", "MuonGit Test"), ("MUONGIT_AUTHOR_EMAIL", "muongit@example.com")],
    );
    run_ok(&seed, &["remote", "add", "origin", &fixture.url]);
    let pushed = run_ok(&seed, &["push", "origin"]);
    assert!(pushed.contains("refs/heads/main"));

    let clone = root.join("clone");
    let cloned = run_ok(&root, &["clone", &fixture.url, clone.to_str().unwrap()]);
    assert!(cloned.contains("Cloned"));
    assert_eq!(fs::read_to_string(clone.join("hello.txt")).unwrap(), "hello\n");

    write_file(seed.join("hello.txt"), "hello remote\n");
    run_ok(&seed, &["add", "hello.txt"]);
    run_ok_env(
        &seed,
        &["commit", "-m", "remote update"],
        &[("MUONGIT_AUTHOR_NAME", "MuonGit Test"), ("MUONGIT_AUTHOR_EMAIL", "muongit@example.com")],
    );
    run_ok(&seed, &["push", "origin"]);

    let fetch = run_ok(&clone, &["fetch", "origin"]);
    assert!(fetch.contains("Fetched"));
    let remote_log = run_ok(&clone, &["log", "--oneline", "--max-count", "1", "origin/main"]);
    assert!(remote_log.contains("remote update"));
    run_ok(&clone, &["reset", "--hard", "origin/main"]);

    write_file(clone.join("local.txt"), "local push\n");
    run_ok(&clone, &["add", "local.txt"]);
    run_ok_env(
        &clone,
        &["commit", "-m", "local push"],
        &[("MUONGIT_AUTHOR_NAME", "MuonGit Test"), ("MUONGIT_AUTHOR_EMAIL", "muongit@example.com")],
    );
    run_ok(&clone, &["push", "origin"]);

    let verify = root.join("verify");
    run_ok(&root, &["clone", &fixture.url, verify.to_str().unwrap()]);
    assert_eq!(fs::read_to_string(verify.join("local.txt")).unwrap(), "local push\n");
}

#[test]
fn cli_usage_and_runtime_exit_codes() {
    let unknown = run_raw(Path::new("."), &["wat"], &[]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown command"));

    let outside = run_raw(Path::new("."), &["status"], &[]);
    assert_eq!(outside.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&outside.stderr).contains("could not find repository"));
}

struct FixtureProcess {
    child: Child,
    url: String,
}

impl FixtureProcess {
    fn http(repo: &Path) -> Self {
        let mut command = Command::new("python3");
        command
            .arg(fixture_script())
            .arg("serve-http")
            .arg("--repo")
            .arg(repo);
        spawn_fixture(command)
    }
}

impl Drop for FixtureProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn run_ok(cwd: &Path, args: &[&str]) -> String {
    run_ok_env(cwd, args, &[])
}

fn run_ok_env(cwd: &Path, args: &[&str], envs: &[(&str, &str)]) -> String {
    let output = run_raw(cwd, args, envs);
    assert!(
        output.status.success(),
        "muongit {:?} failed in {}:\nstdout:{}\nstderr:{}",
        args,
        cwd.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn run_raw(cwd: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut command = Command::new(bin_path());
    command.args(args).current_dir(cwd);
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().unwrap()
}

fn bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_muongit"))
}

fn fixture_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/git_remote_fixture.py")
}

fn spawn_fixture(mut command: Command) -> FixtureProcess {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let url = line
        .split('"')
        .nth(3)
        .unwrap_or_else(|| panic!("unexpected fixture output: {line}"))
        .to_string();
    FixtureProcess { child, url }
}

fn test_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp").join(name)
}

fn write_file(path: PathBuf, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn have_tools(tools: &[&str]) -> bool {
    let missing: Vec<_> = tools.iter().copied().filter(|tool| !tool_available(tool)).collect();
    if missing.is_empty() {
        true
    } else {
        eprintln!("skipping cli smoke test; missing tools: {}", missing.join(", "));
        false
    }
}

fn tool_available(tool: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {tool} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}
