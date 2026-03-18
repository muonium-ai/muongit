# muongit TODO

## Phase 1: Foundation (Current)
- [x] Project scaffolding (swift, kotlin, rust)
- [x] Build systems (SPM, Gradle, Cargo)
- [x] Core types: OID, ObjectType, Signature
- [x] Error types matching libgit2 error codes
- [x] Version/parity tracking
- [ ] OID: full SHA-1 implementation
- [ ] OID: SHA-256 support (experimental parity)
- [ ] Repository: open existing repo
- [ ] Repository: init new repo (bare + non-bare)
- [ ] Repository: discover (walk up to find .git)

## Phase 2: Object Database
- [ ] Loose object read (deflate + parse)
- [ ] Loose object write
- [ ] Pack file index parsing
- [ ] Pack file object lookup
- [ ] Object type detection and parsing
- [ ] Commit object read/write
- [ ] Tree object read/write
- [ ] Blob object read/write
- [ ] Tag object read/write

## Phase 3: References & Index
- [ ] Reference read (loose + packed-refs)
- [ ] Reference write/update/delete
- [ ] Symbolic references
- [ ] Reflog read/write
- [ ] Index file read (.git/index)
- [ ] Index file write
- [ ] Index entry staging

## Phase 4: Diff & Status
- [ ] Tree-to-tree diff
- [ ] Index-to-workdir diff
- [ ] Diff formatting (patch, stat)
- [ ] Status (combined index + workdir)
- [ ] Ignore (.gitignore) support
- [ ] Attributes (.gitattributes)

## Phase 5: Merge & Checkout
- [ ] Checkout (index to workdir)
- [ ] Merge base computation
- [ ] Three-way merge
- [ ] Merge conflict detection
- [ ] Cherry-pick
- [ ] Revert
- [ ] Rebase

## Phase 6: Network
- [ ] Config file read/write
- [ ] Remote management
- [ ] Smart protocol (pack negotiation)
- [ ] HTTP/HTTPS transport
- [ ] SSH transport
- [ ] Fetch
- [ ] Push
- [ ] Clone

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
