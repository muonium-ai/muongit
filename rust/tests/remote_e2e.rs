use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use muongit::fetch::{CloneOptions, FetchOptions, PushOptions};
use muongit::refs;
use muongit::remote_transport::{RemoteAuth, TransportOptions};
use muongit::repository::Repository;

#[test]
fn http_basic_clone_fetch_push_round_trip() {
    if !have_tools(&["git", "python3", "curl"]) {
        return;
    }

    let root = test_dir("rust_remote_http_basic_round_trip");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let setup = GitFixture::new(&root);
    let fixture = FixtureProcess::http(
        &setup.remote_git_dir,
        "basic",
        Some("alice"),
        Some("s3cret"),
        false,
        None,
        None,
    );

    let clone_dir = root.join("clone");
    let repo = Repository::clone_with_options(
        &fixture.url,
        clone_dir.to_str().unwrap(),
        &CloneOptions {
            transport: basic_auth("alice", "s3cret"),
            ..CloneOptions::default()
        },
    )
    .unwrap();

    assert_eq!(read_text(clone_dir.join("hello.txt")), "hello\n");
    assert_eq!(
        refs::read_reference(repo.git_dir(), "HEAD").unwrap(),
        "ref: refs/heads/main"
    );
    let initial_oid = oid_hex(&setup.remote_git_dir, "refs/heads/main");
    assert_eq!(
        refs::resolve_reference(repo.git_dir(), "refs/remotes/origin/main")
            .unwrap()
            .hex(),
        initial_oid
    );

    setup.commit_and_push(
        &setup.seed_workdir,
        "hello.txt",
        "hello remote\n",
        "remote update",
        &setup.remote_git_dir,
    );
    let fetched_oid = oid_hex(&setup.remote_git_dir, "refs/heads/main");

    repo.fetch(
        "origin",
        &FetchOptions {
            transport: basic_auth("alice", "s3cret"),
            ..FetchOptions::default()
        },
    )
    .unwrap();

    assert_eq!(
        refs::resolve_reference(repo.git_dir(), "refs/remotes/origin/main")
            .unwrap()
            .hex(),
        fetched_oid
    );
    assert_eq!(read_text(clone_dir.join("hello.txt")), "hello\n");

    configure_identity(&clone_dir);
    git(&clone_dir, &["checkout", "main"]);
    git(&clone_dir, &["reset", "--hard", "refs/remotes/origin/main"]);
    write_file(clone_dir.join("local.txt"), "local push\n");
    git(&clone_dir, &["add", "local.txt"]);
    git(&clone_dir, &["commit", "-m", "local push"]);
    let pushed_oid = git_output(&clone_dir, &["rev-parse", "HEAD"]);

    repo.push(
        "origin",
        &PushOptions {
            transport: basic_auth("alice", "s3cret"),
            ..PushOptions::default()
        },
    )
    .unwrap();

    assert_eq!(oid_hex(&setup.remote_git_dir, "refs/heads/main"), pushed_oid);
    assert_eq!(
        refs::resolve_reference(repo.git_dir(), "refs/remotes/origin/main")
            .unwrap()
            .hex(),
        pushed_oid
    );
}

#[test]
fn https_bearer_clone_smoke() {
    if !have_tools(&["git", "python3", "openssl", "curl"]) {
        return;
    }

    let root = test_dir("rust_remote_https_bearer_clone");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let setup = GitFixture::new(&root);
    let cert = root.join("cert.pem");
    let key = root.join("key.pem");
    openssl_self_signed_cert(&cert, &key);
    let fixture = FixtureProcess::http(
        &setup.remote_git_dir,
        "bearer",
        None,
        Some("top-secret-token"),
        true,
        Some(&cert),
        Some(&key),
    );

    let clone_dir = root.join("clone");
    let repo = Repository::clone_with_options(
        &fixture.url,
        clone_dir.to_str().unwrap(),
        &CloneOptions {
            transport: TransportOptions {
                auth: Some(RemoteAuth::BearerToken("top-secret-token".into())),
                insecure_skip_tls_verify: true,
            },
            ..CloneOptions::default()
        },
    )
    .unwrap();

    assert_eq!(read_text(clone_dir.join("hello.txt")), "hello\n");
    assert_eq!(
        refs::resolve_reference(repo.git_dir(), "refs/remotes/origin/main")
            .unwrap()
            .hex(),
        oid_hex(&setup.remote_git_dir, "refs/heads/main")
    );
}

#[test]
fn ssh_key_clone_and_push_round_trip() {
    if !have_tools(&["git", "python3", "ssh", "ssh-keygen", "sshd"]) {
        return;
    }

    let root = test_dir("rust_remote_ssh_clone_push");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let setup = GitFixture::new(&root);
    let client_key = root.join("client_key");
    ssh_keygen(&client_key);
    let username = current_username();
    let fixture = FixtureProcess::ssh(
        &setup.remote_git_dir,
        &root.join("sshd"),
        &client_key.with_extension("pub"),
        &username,
    );

    let clone_dir = root.join("clone");
    let repo = Repository::clone_with_options(
        &fixture.url,
        clone_dir.to_str().unwrap(),
        &CloneOptions {
            transport: TransportOptions {
                auth: Some(RemoteAuth::SshKey {
                    username: username.clone(),
                    private_key: client_key.to_str().unwrap().to_string(),
                    port: None,
                    strict_host_key_checking: false,
                }),
                insecure_skip_tls_verify: false,
            },
            ..CloneOptions::default()
        },
    )
    .unwrap();

    assert_eq!(read_text(clone_dir.join("hello.txt")), "hello\n");

    configure_identity(&clone_dir);
    git(&clone_dir, &["checkout", "main"]);
    write_file(clone_dir.join("ssh.txt"), "ssh push\n");
    git(&clone_dir, &["add", "ssh.txt"]);
    git(&clone_dir, &["commit", "-m", "ssh push"]);
    let pushed_oid = git_output(&clone_dir, &["rev-parse", "HEAD"]);

    repo.push(
        "origin",
        &PushOptions {
            transport: TransportOptions {
                auth: Some(RemoteAuth::SshKey {
                    username,
                    private_key: client_key.to_str().unwrap().to_string(),
                    port: None,
                    strict_host_key_checking: false,
                }),
                insecure_skip_tls_verify: false,
            },
            ..PushOptions::default()
        },
    )
    .unwrap();

    assert_eq!(oid_hex(&setup.remote_git_dir, "refs/heads/main"), pushed_oid);
}

struct GitFixture {
    remote_git_dir: PathBuf,
    seed_workdir: PathBuf,
}

impl GitFixture {
    fn new(root: &Path) -> Self {
        let remote_git_dir = root.join("remote.git");
        let seed_workdir = root.join("seed");

        git(root, &["init", "--bare", remote_git_dir.to_str().unwrap()]);
        git(root, &["init", seed_workdir.to_str().unwrap()]);
        configure_identity(&seed_workdir);
        write_file(seed_workdir.join("hello.txt"), "hello\n");
        git(&seed_workdir, &["add", "hello.txt"]);
        git(&seed_workdir, &["commit", "-m", "initial"]);
        git(&seed_workdir, &["branch", "-M", "main"]);
        git(
            &seed_workdir,
            &["remote", "add", "origin", remote_git_dir.to_str().unwrap()],
        );
        git(&seed_workdir, &["push", "origin", "main"]);
        git(
            root,
            &[
                "--git-dir",
                remote_git_dir.to_str().unwrap(),
                "symbolic-ref",
                "HEAD",
                "refs/heads/main",
            ],
        );

        Self {
            remote_git_dir,
            seed_workdir,
        }
    }

    fn commit_and_push(
        &self,
        repo_dir: &Path,
        file_name: &str,
        contents: &str,
        message: &str,
        remote_git_dir: &Path,
    ) {
        write_file(repo_dir.join(file_name), contents);
        git(repo_dir, &["add", file_name]);
        git(repo_dir, &["commit", "-m", message]);
        git(repo_dir, &["push", "origin", "main"]);
        git(
            repo_dir.parent().unwrap(),
            &[
                "--git-dir",
                remote_git_dir.to_str().unwrap(),
                "symbolic-ref",
                "HEAD",
                "refs/heads/main",
            ],
        );
    }
}

struct FixtureProcess {
    child: Child,
    url: String,
}

impl FixtureProcess {
    fn http(
        repo: &Path,
        auth: &str,
        username: Option<&str>,
        secret: Option<&str>,
        tls: bool,
        cert: Option<&Path>,
        key: Option<&Path>,
    ) -> Self {
        let mut command = Command::new("python3");
        command
            .arg(fixture_script())
            .arg("serve-http")
            .arg("--repo")
            .arg(repo);
        if auth != "none" {
            command.arg("--auth").arg(auth);
        }
        if let Some(username) = username {
            command.arg("--username").arg(username);
        }
        if let Some(secret) = secret {
            command.arg("--secret").arg(secret);
        }
        if tls {
            command.arg("--tls");
            command.arg("--cert").arg(cert.unwrap());
            command.arg("--key").arg(key.unwrap());
        }

        spawn_fixture(command)
    }

    fn ssh(repo: &Path, state_dir: &Path, authorized_key: &Path, username: &str) -> Self {
        let mut command = Command::new("python3");
        command
            .arg(fixture_script())
            .arg("serve-ssh")
            .arg("--repo")
            .arg(repo)
            .arg("--state-dir")
            .arg(state_dir)
            .arg("--authorized-key")
            .arg(authorized_key)
            .arg("--username")
            .arg(username);
        spawn_fixture(command)
    }
}

impl Drop for FixtureProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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

fn basic_auth(username: &str, password: &str) -> TransportOptions {
    TransportOptions {
        auth: Some(RemoteAuth::Basic {
            username: username.to_string(),
            password: password.to_string(),
        }),
        insecure_skip_tls_verify: false,
    }
}

fn fixture_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/git_remote_fixture.py")
}

fn test_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp").join(name)
}

fn current_username() -> String {
    std::env::var("USER").unwrap_or_else(|_| git_output(Path::new("."), &["config", "user.name"]))
}

fn have_tools(tools: &[&str]) -> bool {
    let missing: Vec<_> = tools.iter().copied().filter(|tool| !tool_available(tool)).collect();
    if missing.is_empty() {
        return true;
    }
    eprintln!("skipping remote e2e test; missing tools: {}", missing.join(", "));
    false
}

fn tool_available(tool: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {tool} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn configure_identity(repo_dir: &Path) {
    git(repo_dir, &["config", "user.name", "MuonGit Test"]);
    git(repo_dir, &["config", "user.email", "muongit@example.com"]);
}

fn openssl_self_signed_cert(cert: &Path, key: &Path) {
    let status = Command::new("openssl")
        .arg("req")
        .arg("-x509")
        .arg("-newkey")
        .arg("rsa:2048")
        .arg("-nodes")
        .arg("-keyout")
        .arg(key)
        .arg("-out")
        .arg(cert)
        .arg("-days")
        .arg("1")
        .arg("-subj")
        .arg("/CN=127.0.0.1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
}

fn ssh_keygen(key_path: &Path) {
    let status = Command::new("ssh-keygen")
        .arg("-q")
        .arg("-t")
        .arg("ed25519")
        .arg("-N")
        .arg("")
        .arg("-f")
        .arg(key_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
}

fn oid_hex(remote_git_dir: &Path, ref_name: &str) -> String {
    git_output(
        remote_git_dir.parent().unwrap(),
        &["--git-dir", remote_git_dir.to_str().unwrap(), "rev-parse", ref_name],
    )
}

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed:\nstdout:{}\nstderr:{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed:\nstdout:{}\nstderr:{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn write_file(path: PathBuf, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn read_text(path: PathBuf) -> String {
    fs::read_to_string(path).unwrap()
}
