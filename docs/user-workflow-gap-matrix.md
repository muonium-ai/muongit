# MuonGit User Workflow Gap Matrix

Snapshot date: 2026-03-20

This matrix compares how close each muongit implementation is to the way
developers actually use Git in practice.

Legend:

- `User` answers: can a developer do this directly today without writing glue code?
- `Engine` answers: does the underlying port expose the lower-level capability?
- Labels:
  `usable` = strong direct support today
  `partial` = some workflow support, but meaningful gaps remain
  `library-only` = capability exists as APIs, but not as a blessed end-user tool
  `missing` = no meaningful supported path today

## Matrix

| Workflow | Stock Git | Rust `muongit` CLI | Swift port | Kotlin port |
| --- | --- | --- | --- | --- |
| Clone and sync (`clone`, `remote`, `fetch`, `push`) | User: `usable`  Engine: `usable` | User: `usable`  Engine: `usable`  Notes: `clone`, `remote add/list`, `fetch`, and `push` are documented and smoke-tested. | User: `library-only`  Engine: `usable`  Notes: clone/fetch/push APIs exist and remote transport tests cover HTTP and SSH flows. | User: `library-only`  Engine: `usable`  Notes: clone/fetch/push APIs exist and the same remote transport workflows are implemented. |
| Branch workflow (`branch`, `switch`, detached HEAD, ref state) | User: `usable`  Engine: `usable` | User: `partial`  Engine: `usable`  Notes: create/list/switch/detach/reset/restore are present, but branch rename/delete/upstream flows are not exposed in the CLI. | User: `library-only`  Engine: `usable`  Notes: branch/refdb/switch APIs are present, but there is no blessed Swift CLI. | User: `library-only`  Engine: `usable`  Notes: branch/refdb/switch APIs are present, but there is no blessed Kotlin CLI. |
| Everyday commit flow (`init`, `status`, `add`, `rm`, `commit`, `diff`, `restore`) | User: `usable`  Engine: `usable` | User: `usable`  Engine: `usable`  Notes: this is the strongest Rust path today; the main CLI gap is narrower `diff` porcelain than stock Git. | User: `library-only`  Engine: `usable`  Notes: porcelain/status/object/history APIs exist, but users must write their own wrapper. | User: `library-only`  Engine: `usable`  Notes: porcelain/status/object/history APIs exist, but users must write their own wrapper. |
| Conflict handling (merge conflicts, revert/cherry-pick conflicts, recovery) | User: `usable`  Engine: `usable` | User: `missing`  Engine: `partial`  Notes: merge/cherry-pick/revert/rebase engines exist, but the blessed CLI does not expose them, so there is no normal end-user conflict path. | User: `library-only`  Engine: `partial`  Notes: conflict-capable engines exist in source, but they are not presented as a supported user workflow. | User: `library-only`  Engine: `partial`  Notes: conflict-capable engines exist in source, but they are not presented as a supported user workflow. |
| History inspection (`log`, rev ranges, object reads, patch/diff viewing) | User: `usable`  Engine: `usable` | User: `usable`  Engine: `usable`  Notes: `log`, rev ranges, object reads, and diff forms cover common inspection tasks, though not every stock Git diff shape exists. | User: `library-only`  Engine: `usable`  Notes: revparse/revwalk/object/patch APIs exist and revision-history tests cover them. | User: `library-only`  Engine: `usable`  Notes: revparse/revwalk/object/patch APIs exist and revision-history tests cover them. |
| Advanced repo surgery (`merge`, `rebase`, `cherry-pick`, `revert`, `stash`, `worktree`) | User: `usable`  Engine: `usable` | User: `missing`  Engine: `partial`  Notes: the Rust engine has modules for these areas, but the blessed CLI intentionally does not expose them yet. | User: `library-only`  Engine: `partial`  Notes: source files exist for these advanced areas, but there is no end-user product surface and no single supported workflow story. | User: `library-only`  Engine: `partial`  Notes: source files exist for these advanced areas, but there is no end-user product surface and no single supported workflow story. |

## Findings

1. The main product gap is no longer the core repository engine. It is the gap between engine capability and user-facing workflow coverage.
2. Rust is the only implementation that feels even partially like Git as a tool, because it is the only blessed CLI. Swift and Kotlin are still engine ports, not user products.
3. Everyday commit flow and history inspection are closest to stock Git today. A developer can plausibly use the Rust CLI for normal local work and basic remote sync.
4. Conflict handling is the weakest user workflow. The engines expose pieces of merge/revert/cherry-pick/rebase behavior, but there is no supported direct-user path for resolving conflicts with the blessed CLI.
5. Advanced repo surgery is still materially behind Git across the board. The source tree has building blocks, but the repo does not yet present them as a coherent, supported user experience.

## Highest-Priority Follow-Ups

1. Expand the Rust CLI to expose `merge`, `rebase`, `cherry-pick`, `revert`, `stash`, and `worktree` workflows on top of the existing engine modules.
2. Decide whether Swift and Kotlin are intended to remain embeddable libraries or should also gain blessed user-facing CLIs. Without that decision, “parity with Git” will remain ambiguous at the product level.
3. Add workflow-level validation for conflict-heavy scenarios, not just repository-state conformance, so the project can measure whether conflict resolution behaves like a real Git replacement.
4. Broaden the Rust CLI `diff` and branch-management surface so common Git porcelain habits translate with fewer surprises.

## Basis For Ratings

- Rust user-surface and known CLI gaps: [docs/cli.md](/Users/senthil/github/muonium-ai/muongit/docs/cli.md)
- Rust exported engine modules: [lib.rs](/Users/senthil/github/muonium-ai/muongit/rust/src/lib.rs)
- Cross-implementation workflow validation: [repository_conformance.py](/Users/senthil/github/muonium-ai/muongit/scripts/repository_conformance.py)
- Swift workflow evidence: [BranchRefDbTests.swift](/Users/senthil/github/muonium-ai/muongit/swift/tests/BranchRefDbTests.swift), [RevisionHistoryTests.swift](/Users/senthil/github/muonium-ai/muongit/swift/tests/RevisionHistoryTests.swift), [PatchTests.swift](/Users/senthil/github/muonium-ai/muongit/swift/tests/PatchTests.swift), [RemoteTransportTests.swift](/Users/senthil/github/muonium-ai/muongit/swift/tests/RemoteTransportTests.swift), [SwitchResetRestoreTests.swift](/Users/senthil/github/muonium-ai/muongit/swift/tests/SwitchResetRestoreTests.swift), [StagingCommitPorcelainTests.swift](/Users/senthil/github/muonium-ai/muongit/swift/tests/StagingCommitPorcelainTests.swift)
- Kotlin workflow evidence: [Branch.kt](/Users/senthil/github/muonium-ai/muongit/kotlin/src/main/kotlin/ai/muonium/muongit/Branch.kt), [Checkout.kt](/Users/senthil/github/muonium-ai/muongit/kotlin/src/main/kotlin/ai/muonium/muongit/Checkout.kt), [Fetch.kt](/Users/senthil/github/muonium-ai/muongit/kotlin/src/main/kotlin/ai/muonium/muongit/Fetch.kt), [Patch.kt](/Users/senthil/github/muonium-ai/muongit/kotlin/src/main/kotlin/ai/muonium/muongit/Patch.kt), [Revwalk.kt](/Users/senthil/github/muonium-ai/muongit/kotlin/src/main/kotlin/ai/muonium/muongit/Revwalk.kt), [RefDb.kt](/Users/senthil/github/muonium-ai/muongit/kotlin/src/main/kotlin/ai/muonium/muongit/RefDb.kt)
