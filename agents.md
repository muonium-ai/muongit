# agents.md — Agent Coordination Guide

## Overview

muongit uses AI agents to parallelize development across three language ports. Each agent works on a specific language or cross-cutting concern, coordinated via MuonTickets.

## Agent Roles

### agent-swift
- **Scope**: `swift/` directory
- **Stack**: Swift 5.9+, Swift Package Manager
- **Branch prefix**: `swift/`
- **Focus**: macOS/iOS native idioms, Swift concurrency (Sendable, actors)

### agent-kotlin
- **Scope**: `kotlin/` directory
- **Stack**: Kotlin 2.0+, Gradle, Kotlin Multiplatform
- **Branch prefix**: `kotlin/`
- **Focus**: JVM + Kotlin/Native targets, coroutine-friendly APIs

### agent-rust
- **Scope**: `rust/` directory
- **Stack**: Rust 1.75+, Cargo
- **Branch prefix**: `rust/`
- **Focus**: Zero-copy where possible, no-std compatible core, ownership-driven API

### agent-parity
- **Scope**: Cross-cutting, all ports
- **Branch prefix**: `parity/`
- **Focus**: API parity audits, conformance test generation, ensuring consistent behavior across ports

## Workflow

### Commit Rules

**Each ticket = exactly one commit.** This is a hard rule.

- One ticket's implementation must be contained in a single, self-contained commit
- Do NOT batch multiple tickets into one commit
- Do NOT split one ticket across multiple commits
- Commit message format: `T-NNNNNN: <ticket title>`
  ```
  T-000002: Implement SHA-1 hashing for OID
  ```
- The commit must include all code, tests, and ticket status changes for that ticket
- Tests must pass before committing
- Run tests for the affected port(s) before creating the commit

### Ticket Lifecycle (per commit)

```bash
MT="python3 tickets/mt/muontickets/muontickets/mt.py"

# 1. Claim the ticket
$MT claim T-000042 --owner agent-swift

# 2. Implement the code and tests

# 3. Run tests to verify
make -C swift test   # (or kotlin/rust)

# 4. Stage all changes for this ticket
git add swift/src/NewModule.swift swift/tests/... tickets/T-000042.md

# 5. Mark ticket done
$MT set-status T-000042 needs_review
$MT done T-000042

# 6. Commit (one ticket = one commit)
git commit -m "T-000042: Implement feature X"
```

### Picking Work
```bash
# Claim next available ticket for your role
$MT pick --owner agent-swift --label swift
$MT pick --owner agent-rust --label rust
$MT pick --owner agent-kotlin --label kotlin
```

### Progress Updates
```bash
# Log progress
$MT comment T-000042 "Implemented OID hex parsing, tests passing"

# Mark for review
$MT set-status T-000042 needs_review

# Mark done
$MT done T-000042
```

### Handling Failures
```bash
# Record failure (auto-retries up to limit)
$MT fail-task T-000042 --error "SHA-256 not yet available in stdlib"
```

## Temp Directory

All temporary files, test repositories, and scratch work **must** use the project-local `tmp/` directory (gitignored). This keeps everything inside the project sandbox.

```bash
# Use for test repos in code
TMP_DIR="$(git rev-parse --show-toplevel)/tmp"

# Rust tests
let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/test_name");

# Swift tests
let tmp = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent()
    .appendingPathComponent("tmp/test_name").path

# Kotlin tests
val tmp = java.io.File(System.getProperty("user.dir")).resolve("../tmp/test_name")
```

**Never** use `/tmp`, `NSTemporaryDirectory()`, or `System.getProperty("java.io.tmpdir")` — those are outside the project sandbox and require extra permissions.

## Parity Rules

1. **API surface**: Every public function in libgit2's `include/git2/*.h` must have a corresponding function in each port
2. **Behavior**: Ports must produce identical results for the same inputs (same OIDs, same diffs, same merge outcomes)
3. **Error codes**: Map libgit2 error codes to idiomatic error types in each language
4. **Tests**: Each port must pass equivalent test cases — use `vendor/libgit2/tests/` as reference

## Reference

- libgit2 headers: `vendor/libgit2/include/git2/`
- libgit2 source: `vendor/libgit2/src/libgit2/`
- libgit2 tests: `vendor/libgit2/tests/`
- Tickets: `tickets/`
- mt.py: `tickets/mt/muontickets/muontickets/mt.py`
- Temp/scratch: `tmp/` (gitignored, use for all test repos and scratch work)
