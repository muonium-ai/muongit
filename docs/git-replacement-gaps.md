# Git Replacement Gap Analysis

Date: 2026-03-20

## Scope

This document captures the remaining gaps that prevent any current muongit implementation
from acting as a practical replacement for the `git` executable.

The goal here is not to restate every libgit2-parity ticket that has already landed.
It is to identify the missing user-facing capabilities that still force users to keep
the stock Git CLI around.

## Current State

The repository has broad low-level coverage for repository access, object parsing,
refs, index, diff, status, merge/rebase primitives, and several advanced features
such as stash, submodule, worktree, blame, describe, notes, grafts, mailmap, and
pathspec.

However, muongit is still primarily a library codebase with benchmark entrypoints,
not a complete Git product.

## Hard Blockers

### 1. No user-facing Git-compatible CLI

There is no end-user `git` replacement binary today.

- Rust exposes a library plus `muongit-bench`
- Swift exposes a library product plus `muongit-bench`
- Kotlin exposes a benchmark task, not a Git CLI

Without a CLI layer, no user can directly swap `git` for any muongit implementation.

### 2. Real clone/fetch/push over authenticated remotes is incomplete

All three top-level repository APIs still leave `clone` unimplemented.

The existing transport/fetch code provides useful plumbing:

- pkt-line encode/decode
- smart-protocol ref advertisement parsing
- refspec mapping
- want/have negotiation assembly
- local ref updates after a hypothetical fetch
- clone setup / clone finish repository wiring

What is still missing is the end-to-end remote implementation:

- actual HTTP/HTTPS request flow
- actual SSH session flow
- authentication and credential callbacks
- packfile upload/download over the network
- private-remote support
- clone/fetch/push behavior that works against real servers

Archived tickets for remote management and transport landed groundwork, but not a
complete real-world remote workflow.

### 3. Porcelain workflows are still missing

The codebase contains low-level object and index primitives, but practical Git
replacement needs high-level workflows.

Missing or incomplete user-facing workflows include:

- stage tracked and untracked files by path/pathspec
- remove or unstage paths
- create commits from the current index and update branch refs
- branch creation / deletion / rename / upstream tracking
- switch or checkout branches in a user-oriented way
- reset and restore workflows

Today, many of these areas exist only as lower-level helpers instead of a cohesive
repository workflow API.

### 4. History and revision plumbing are missing

A usable Git replacement also needs history navigation and revision resolution.

Missing areas from the current code inspection include:

- revision parsing (`HEAD~3`, `main..topic`, ranges, etc.)
- revision walking / log traversal
- history-oriented CLI behaviors like `git log`

Even if object access works, users still need ways to resolve and walk history.

### 5. Several parity targets are still absent as first-class modules

The README still names these parity targets, but there are no corresponding
first-class modules across the ports:

- `branch`
- `refdb`
- `patch`
- `apply`
- `credentials`
- `object`

These gaps matter because they underpin both library parity and end-user workflows.

## Not Primary Blockers Anymore

These recent advanced features are no longer the main blockers for replacing `git`
because they now exist in the ports:

- worktree
- blame
- describe
- notes
- grafts
- mailmap
- pathspec

## Backlog Shape

The remaining work should be tracked as product-level gaps instead of more narrowly
scoped parity fragments.

Recommended backlog groups:

1. Ship a user-facing CLI for each implementation or define one blessed reference CLI.
2. Finish real network transport and end-to-end authenticated clone/fetch/push.
3. Add porcelain workflows for add/commit/branch/switch/reset/restore.
4. Add revision parsing and history walking.
5. Close the remaining parity-module gaps: branch, refdb, patch/apply, credentials, object.

## Proposed Ticket Map

The following ticket set is intended to close the remaining blockers:

- `T-000051` Build a git-compatible CLI for muongit
- `T-000052` Implement end-to-end authenticated remote transport and real clone/fetch/push
- `T-000053` Add staging, add/remove, and commit porcelain workflows
- `T-000054` Add branch and refdb APIs across all ports
- `T-000055` Add switch, checkout, reset, and restore porcelain workflows
- `T-000056` Add revision parsing and history walking
- `T-000057` Add patch and apply APIs across all ports
- `T-000058` Add object API parity across all ports

The CLI ticket (`T-000051`) depends on the underlying feature tickets so the backlog
reflects the real critical path instead of a flat list of disconnected work.
