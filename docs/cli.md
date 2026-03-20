# muongit CLI

`muongit` is the blessed user-facing CLI for this repo. It is implemented once in Rust and shipped on top of the shared MuonGit library APIs rather than duplicating command frontends in Swift and Kotlin.

## Install

```bash
cargo install --path rust --bin muongit
```

For local development without installing:

```bash
cargo run --manifest-path rust/Cargo.toml --bin muongit -- <command> ...
```

## Command Surface

Supported commands in this ticket:

- `init [--bare] [<path>]`
- `status`
- `add [--include-ignored] [<path>...]`
- `rm <path>...`
- `commit -m <message>`
- `branch [<name> [<start>]]`
- `switch [--force] <branch>`
- `switch --detach [--force] <rev>`
- `reset [--soft|--mixed|--hard] [<rev>]`
- `restore [--source <rev>] [--staged] [--worktree] <path>...`
- `log [--oneline] [--max-count <n>] [--first-parent] [<rev-or-range>]`
- `diff [--cached] [--stat]`
- `diff [--stat] <old> <new>`
- `remote list`
- `remote add <name> <url>`
- `clone [--bare] [--branch <name>] [--remote <name>] [--insecure-skip-tls-verify] <url> <path>`
- `fetch [--refspec <spec>]... [--insecure-skip-tls-verify] [<remote>]`
- `push [--refspec <spec>]... [--insecure-skip-tls-verify] [<remote>]`

Global options:

- `-C <path>` changes the working directory before command execution.
- `--help` and `help [<command>]` print help text.
- `--version` and `version` print the MuonGit version.

## Output And Exit Codes

- Success output is written to `stdout`.
- Usage and parse failures are written to `stderr` and exit with code `2`.
- Repository, transport, auth, and runtime failures are written to `stderr` and exit with code `1`.
- Success exits with code `0`.

Conventions used by this ticket:

- `status` prints a git-style short format:
  `X` = staged change against `HEAD`
  `Y` = unstaged worktree change
  `??` = untracked path
- `commit` prints `[branch short-oid] summary`
- `switch` prints either `Switched to branch 'name'` or `HEAD is now at <oid>`
- `fetch` and `push` print human-readable summaries and push reports

## Identity And Authentication

Commit identity resolution order:

1. `MUONGIT_AUTHOR_NAME` / `MUONGIT_AUTHOR_EMAIL`
2. `GIT_AUTHOR_NAME` / `GIT_AUTHOR_EMAIL`
3. repository config `user.name` / `user.email`
4. built-in MuonGit defaults

Committer resolution follows the same pattern with `MUONGIT_COMMITTER_*` and `GIT_COMMITTER_*`, then falls back to the resolved author identity.

Remote authentication is environment-driven:

- HTTP basic auth:
  `MUONGIT_HTTP_USERNAME` and `MUONGIT_HTTP_PASSWORD`
  or `MUONGIT_HTTP_BASIC=username:password`
- HTTP bearer auth:
  `MUONGIT_HTTP_BEARER_TOKEN`
- SSH private-key auth:
  `MUONGIT_SSH_PRIVATE_KEY`
  optional `MUONGIT_SSH_USERNAME`
  optional `MUONGIT_SSH_PORT`
  optional `MUONGIT_SSH_STRICT_HOST_KEY_CHECKING=1`
- SSH agent auth:
  `MUONGIT_SSH_AGENT=1`
  optional `MUONGIT_SSH_USERNAME`
  optional `MUONGIT_SSH_PORT`
  optional `MUONGIT_SSH_STRICT_HOST_KEY_CHECKING=1`

`clone`, `fetch`, and `push` also accept `--insecure-skip-tls-verify` for local test fixtures and self-signed TLS endpoints.

## Smoke Coverage

The Rust integration smoke tests cover:

- local repository workflows: `init`, `status`, `add`, `commit`, `branch`, `switch`, `reset`, `restore`, `log`, and `diff`
- HTTP remote workflows: `init --bare`, `remote add`, `push`, `clone`, and `fetch`
- CLI exit-code behavior for usage and runtime failures

## Known Gaps

- This ticket blesses one Rust CLI implementation; Swift and Kotlin remain library ports.
- `diff` currently supports worktree diff, staged diff, and explicit two-revision diff forms; it does not yet implement every porcelain form that stock `git diff` accepts.
- The CLI is intentionally narrow and does not yet expose merge, rebase, cherry-pick, revert, stash, or worktree subcommands.
