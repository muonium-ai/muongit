# muongit TODO

## Phase 1: Foundation
- [x] Project scaffolding (swift, kotlin, rust)
- [x] Build systems (SPM, Gradle, Cargo)
- [x] Core types: OID, ObjectType, Signature
- [x] Error types matching libgit2 error codes
- [x] Version/parity tracking
- [x] OID: full SHA-1 implementation
- [x] OID: SHA-256 support (experimental parity)
- [x] Repository: open existing repo
- [x] Repository: init new repo (bare + non-bare)
- [x] Repository: discover (walk up to find .git)

## Phase 2: Object Database
- [x] Loose object read (deflate + parse)
- [x] Loose object write
- [x] Pack file index parsing
- [x] Pack file object lookup
- [x] Object type detection and parsing
- [x] Commit object read/write
- [x] Tree object read/write
- [x] Blob object read/write
- [x] Tag object read/write

## Phase 3: References & Index
- [x] Reference read (loose + packed-refs)
- [x] Reference write/update/delete
- [x] Symbolic references
- [x] Reflog read/write
- [x] Index file read (.git/index)
- [x] Index file write
- [x] Index entry staging

## Phase 4: Diff & Status
- [x] Tree-to-tree diff
- [x] Index-to-workdir diff
- [x] Diff formatting (patch, stat)
- [x] Status (combined index + workdir)
- [x] Ignore (.gitignore) support
- [ ] Attributes (.gitattributes)

## Phase 5: Merge & Checkout
- [x] Checkout (index to workdir)
- [x] Merge base computation
- [x] Three-way merge
- [x] Merge conflict detection
- [ ] Cherry-pick
- [ ] Revert
- [ ] Rebase

## Phase 6: Network
- [x] Config file read/write
- [x] Remote management
- [x] Smart protocol (pack negotiation)
- [x] HTTP/HTTPS transport
- [x] SSH transport
- [x] Fetch
- [x] Push
- [x] Clone

## Phase 7: Advanced
- [ ] Submodule support
- [ ] Worktree support
- [ ] Stash
- [ ] Blame
- [ ] Describe
- [ ] Notes
- [ ] Grafts
- [ ] Mailmap
- [ ] Pathspec
- [ ] Filter system
