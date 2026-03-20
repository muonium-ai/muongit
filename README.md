# muongit

Native git port of libgit2 to Swift, Kotlin, and Rust.

## Overview

muongit is a multi-language native reimplementation of [libgit2](https://libgit2.org/) (v1.9.0). Each port is a standalone library — no C bindings, no FFI — targeting full API parity with the reference implementation.

| Port | Directory | Build System | Status |
|------|-----------|-------------|--------|
| Swift | `swift/` | Swift Package Manager | Scaffolding |
| Kotlin | `kotlin/` | Gradle (Kotlin Multiplatform) | Scaffolding |
| Rust | `rust/` | Cargo | Scaffolding |

## Project Structure

```
muongit/
├── swift/          # Swift port (macOS, iOS, watchOS, tvOS)
├── kotlin/         # Kotlin port (JVM, macOS, Linux native)
├── rust/           # Rust port
├── vendor/
│   └── libgit2/    # Reference implementation (submodule)
├── tickets/        # MuonTickets task tracking
├── TODO.md         # High-level roadmap
└── agents.md       # Agent coordination guide
```

## Building

Each port has its own Makefile with consistent targets:

```bash
# Build
make -C swift build
make -C kotlin build
make -C rust build

# Test
make -C swift test
make -C kotlin test
make -C rust test

# Check API parity against libgit2
make -C swift check-parity
make -C kotlin check-parity
make -C rust check-parity
```

## CLI

The blessed user-facing CLI is the Rust `muongit` binary.

```bash
# install
cargo install --path rust --bin muongit

# run without installing
cargo run --manifest-path rust/Cargo.toml --bin muongit -- status
```

The command surface, exit codes, authentication environment variables, and
known gaps are documented in [docs/cli.md](docs/cli.md).

## Conformance

The cross-implementation repository conformance matrix lives in
`scripts/repository_conformance.py`.

```bash
python3 scripts/repository_conformance.py
```

That workflow:

- authors the same canonical repository scenario with each writer implementation
- persists checkpoints under `tmp/repository_conformance/`
- has every implementation reopen each checkpoint and compare normalized semantic snapshots
- covers objects, porcelain, refs/branches, revision walking, checkout/reset/restore, patch/apply, and remote clone/fetch/push

When the matrix fails, it reports the writer implementation, checkpoint, and
reader implementation that disagreed.

The current product-readiness summary by user workflow lives in
[docs/user-workflow-gap-matrix.md](docs/user-workflow-gap-matrix.md).

## libgit2 API Modules

The following core modules are targeted for parity (68 public APIs):

- **Core Objects**: repository, commit, tree, blob, tag, object, oid
- **References**: refs, reflog, refdb, refspec, branch
- **Index**: index, staging, conflict resolution
- **Configuration**: config file parsing (INI format)
- **Diff & Patch**: diff, patch, blame, apply
- **Merge & Rebase**: merge, cherrypick, revert, rebase
- **Network**: remote, fetch, push, transport, credentials
- **Working Directory**: checkout, status, ignore, attributes
- **Advanced**: submodule, worktree, stash, notes, describe, grafts

## Tickets

Task tracking uses [MuonTickets](https://github.com/muonium-ai/muontickets), a Git-native file-based ticketing system.

```bash
# List open tickets
python3 tickets/mt/muontickets/muontickets/mt.py ls

# View a ticket
python3 tickets/mt/muontickets/muontickets/mt.py show T-000001
```

## License

MIT
