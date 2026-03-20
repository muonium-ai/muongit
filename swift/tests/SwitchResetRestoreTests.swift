import XCTest
@testable import MuonGit

final class SwitchResetRestoreTests: XCTestCase {
    func testSwitchBranchUpdatesHeadAndWorktree() throws {
        let tmp = testDirectory("swift_switch_branch_updates_head_and_worktree")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let mainTree = try writeTree(gitDir: repo.gitDir, files: [
            ("shared.txt", "main\n"),
            ("only-main.txt", "remove me\n"),
        ])
        let mainCommit = try writeCommit(gitDir: repo.gitDir, treeOID: mainTree, parents: [], message: "main", time: 1)
        let featureTree = try writeTree(gitDir: repo.gitDir, files: [
            ("shared.txt", "feature\n"),
            ("only-feature.txt", "add me\n"),
        ])
        let featureCommit = try writeCommit(gitDir: repo.gitDir, treeOID: featureTree, parents: [mainCommit], message: "feature", time: 2)

        try writeReference(gitDir: repo.gitDir, name: "refs/heads/main", oid: mainCommit)
        try writeReference(gitDir: repo.gitDir, name: "refs/heads/feature", oid: featureCommit)
        try writeSymbolicReference(gitDir: repo.gitDir, name: "HEAD", target: "refs/heads/main")
        try seedWorkdir(from: mainCommit, repo: repo)

        let result = try repo.switchBranch(name: "feature")

        XCTAssertEqual(result.previousHead, mainCommit)
        XCTAssertEqual(result.headOID, featureCommit)
        XCTAssertEqual(result.headRef, "refs/heads/feature")
        XCTAssertEqual(try readReference(gitDir: repo.gitDir, name: "HEAD"), "ref: refs/heads/feature")
        XCTAssertEqual(try String(contentsOfFile: (tmp as NSString).appendingPathComponent("shared.txt"), encoding: .utf8), "feature\n")
        XCTAssertFalse(FileManager.default.fileExists(atPath: (tmp as NSString).appendingPathComponent("only-main.txt")))
        XCTAssertEqual(try String(contentsOfFile: (tmp as NSString).appendingPathComponent("only-feature.txt"), encoding: .utf8), "add me\n")
        XCTAssertTrue(result.updatedPaths.contains("shared.txt"))
        XCTAssertTrue(result.removedPaths.contains("only-main.txt"))
    }

    func testCheckoutRevisionDetachesHead() throws {
        let tmp = testDirectory("swift_checkout_revision_detaches_head")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let mainTree = try writeTree(gitDir: repo.gitDir, files: [("shared.txt", "main\n")])
        let mainCommit = try writeCommit(gitDir: repo.gitDir, treeOID: mainTree, parents: [], message: "main", time: 1)
        let featureTree = try writeTree(gitDir: repo.gitDir, files: [("shared.txt", "detached\n")])
        let featureCommit = try writeCommit(gitDir: repo.gitDir, treeOID: featureTree, parents: [mainCommit], message: "feature", time: 2)

        try writeReference(gitDir: repo.gitDir, name: "refs/heads/main", oid: mainCommit)
        try writeReference(gitDir: repo.gitDir, name: "refs/heads/feature", oid: featureCommit)
        try writeSymbolicReference(gitDir: repo.gitDir, name: "HEAD", target: "refs/heads/main")
        try seedWorkdir(from: mainCommit, repo: repo)

        let result = try repo.checkoutRevision(spec: featureCommit.hex)

        XCTAssertEqual(result.previousHead, mainCommit)
        XCTAssertEqual(result.headOID, featureCommit)
        XCTAssertNil(result.headRef)
        XCTAssertEqual(try readReference(gitDir: repo.gitDir, name: "HEAD"), featureCommit.hex)
        XCTAssertEqual(try String(contentsOfFile: (tmp as NSString).appendingPathComponent("shared.txt"), encoding: .utf8), "detached\n")
    }

    func testSwitchBranchRejectsLocalChanges() throws {
        let tmp = testDirectory("swift_switch_branch_rejects_local_changes")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let mainTree = try writeTree(gitDir: repo.gitDir, files: [("shared.txt", "main\n")])
        let mainCommit = try writeCommit(gitDir: repo.gitDir, treeOID: mainTree, parents: [], message: "main", time: 1)
        let featureTree = try writeTree(gitDir: repo.gitDir, files: [("shared.txt", "feature\n")])
        let featureCommit = try writeCommit(gitDir: repo.gitDir, treeOID: featureTree, parents: [mainCommit], message: "feature", time: 2)

        try writeReference(gitDir: repo.gitDir, name: "refs/heads/main", oid: mainCommit)
        try writeReference(gitDir: repo.gitDir, name: "refs/heads/feature", oid: featureCommit)
        try writeSymbolicReference(gitDir: repo.gitDir, name: "HEAD", target: "refs/heads/main")
        try seedWorkdir(from: mainCommit, repo: repo)
        try "dirty\n".write(toFile: (tmp as NSString).appendingPathComponent("shared.txt"), atomically: true, encoding: .utf8)

        XCTAssertThrowsError(try repo.switchBranch(name: "feature")) { error in
            guard case MuonGitError.conflict(let message) = error else {
                return XCTFail("expected conflict, got \(error)")
            }
            XCTAssertTrue(message.contains("shared.txt"))
        }
        XCTAssertEqual(try readReference(gitDir: repo.gitDir, name: "HEAD"), "ref: refs/heads/main")
        XCTAssertEqual(try String(contentsOfFile: (tmp as NSString).appendingPathComponent("shared.txt"), encoding: .utf8), "dirty\n")
    }

    func testResetModesUpdateRefsIndexAndWorktree() throws {
        let tmp = testDirectory("swift_reset_modes_update_refs_index_and_worktree")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let baseTree = try writeTree(gitDir: repo.gitDir, files: [("file.txt", "base\n")])
        let baseCommit = try writeCommit(gitDir: repo.gitDir, treeOID: baseTree, parents: [], message: "base", time: 1)
        let changedTree = try writeTree(gitDir: repo.gitDir, files: [
            ("file.txt", "changed\n"),
            ("new.txt", "new\n"),
        ])
        let changedCommit = try writeCommit(gitDir: repo.gitDir, treeOID: changedTree, parents: [baseCommit], message: "changed", time: 2)

        try writeReference(gitDir: repo.gitDir, name: "refs/heads/main", oid: changedCommit)
        try writeSymbolicReference(gitDir: repo.gitDir, name: "HEAD", target: "refs/heads/main")
        try seedWorkdir(from: changedCommit, repo: repo)

        let baseEntries = try materializeEntries(gitDir: repo.gitDir, commitOID: baseCommit)
        let changedEntries = try materializeEntries(gitDir: repo.gitDir, commitOID: changedCommit)

        try "dirty soft\n".write(toFile: (tmp as NSString).appendingPathComponent("file.txt"), atomically: true, encoding: .utf8)
        _ = try repo.reset(spec: baseCommit.hex, mode: .soft)
        XCTAssertEqual(try resolveReference(gitDir: repo.gitDir, name: "HEAD"), baseCommit)
        XCTAssertEqual(try readIndex(gitDir: repo.gitDir).find(path: "file.txt")?.oid, changedEntries["file.txt"]?.oid)
        XCTAssertEqual(try String(contentsOfFile: (tmp as NSString).appendingPathComponent("file.txt"), encoding: .utf8), "dirty soft\n")

        try writeReference(gitDir: repo.gitDir, name: "refs/heads/main", oid: changedCommit)
        try seedWorkdir(from: changedCommit, repo: repo)
        try "dirty mixed\n".write(toFile: (tmp as NSString).appendingPathComponent("file.txt"), atomically: true, encoding: .utf8)
        _ = try repo.reset(spec: baseCommit.hex, mode: .mixed)
        XCTAssertEqual(try resolveReference(gitDir: repo.gitDir, name: "HEAD"), baseCommit)
        XCTAssertEqual(try readIndex(gitDir: repo.gitDir).find(path: "file.txt")?.oid, baseEntries["file.txt"]?.oid)
        XCTAssertEqual(try String(contentsOfFile: (tmp as NSString).appendingPathComponent("file.txt"), encoding: .utf8), "dirty mixed\n")
        XCTAssertTrue(FileManager.default.fileExists(atPath: (tmp as NSString).appendingPathComponent("new.txt")))

        try writeReference(gitDir: repo.gitDir, name: "refs/heads/main", oid: changedCommit)
        try seedWorkdir(from: changedCommit, repo: repo)
        try "dirty hard\n".write(toFile: (tmp as NSString).appendingPathComponent("file.txt"), atomically: true, encoding: .utf8)
        let hard = try repo.reset(spec: baseCommit.hex, mode: .hard)
        XCTAssertEqual(try resolveReference(gitDir: repo.gitDir, name: "HEAD"), baseCommit)
        XCTAssertEqual(try String(contentsOfFile: (tmp as NSString).appendingPathComponent("file.txt"), encoding: .utf8), "base\n")
        XCTAssertFalse(FileManager.default.fileExists(atPath: (tmp as NSString).appendingPathComponent("new.txt")))
        XCTAssertTrue(hard.removedPaths.contains("new.txt"))
        XCTAssertEqual(try readIndex(gitDir: repo.gitDir).find(path: "file.txt")?.oid, baseEntries["file.txt"]?.oid)
    }

    func testRestoreStagedAndWorktreePaths() throws {
        let tmp = testDirectory("swift_restore_staged_and_worktree_paths")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let commitTree = try writeTree(gitDir: repo.gitDir, files: [("file.txt", "committed\n")])
        let commit = try writeCommit(gitDir: repo.gitDir, treeOID: commitTree, parents: [], message: "commit", time: 1)

        try writeReference(gitDir: repo.gitDir, name: "refs/heads/main", oid: commit)
        try writeSymbolicReference(gitDir: repo.gitDir, name: "HEAD", target: "refs/heads/main")
        try seedWorkdir(from: commit, repo: repo)

        let headEntry = try XCTUnwrap(materializeEntries(gitDir: repo.gitDir, commitOID: commit)["file.txt"])

        try "worktree\n".write(toFile: (tmp as NSString).appendingPathComponent("file.txt"), atomically: true, encoding: .utf8)
        var index = try readIndex(gitDir: repo.gitDir)
        let stagedOID = try writeLooseObject(gitDir: repo.gitDir, type: .blob, data: Data("staged\n".utf8))
        index.add(IndexEntry(mode: headEntry.mode, fileSize: UInt32("staged\n".utf8.count), oid: stagedOID, flags: UInt16("file.txt".utf8.count), path: "file.txt"))
        try writeIndex(gitDir: repo.gitDir, index: index)

        let result = try repo.restore(paths: ["file.txt"], options: RestoreOptions(staged: true, worktree: true))

        XCTAssertEqual(try readIndex(gitDir: repo.gitDir).find(path: "file.txt")?.oid, headEntry.oid)
        XCTAssertEqual(try String(contentsOfFile: (tmp as NSString).appendingPathComponent("file.txt"), encoding: .utf8), "committed\n")
        XCTAssertEqual(result.stagedPaths, ["file.txt"])
        XCTAssertEqual(result.restoredPaths, ["file.txt"])
    }

    func testRestoreFromSourceUpdatesWorktreeOnly() throws {
        let tmp = testDirectory("swift_restore_from_source_updates_worktree_only")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let baseTree = try writeTree(gitDir: repo.gitDir, files: [("file.txt", "base\n")])
        let baseCommit = try writeCommit(gitDir: repo.gitDir, treeOID: baseTree, parents: [], message: "base", time: 1)
        let changedTree = try writeTree(gitDir: repo.gitDir, files: [("file.txt", "changed\n")])
        let changedCommit = try writeCommit(gitDir: repo.gitDir, treeOID: changedTree, parents: [baseCommit], message: "changed", time: 2)

        try writeReference(gitDir: repo.gitDir, name: "refs/heads/main", oid: changedCommit)
        try writeSymbolicReference(gitDir: repo.gitDir, name: "HEAD", target: "refs/heads/main")
        try seedWorkdir(from: changedCommit, repo: repo)
        try "dirty\n".write(toFile: (tmp as NSString).appendingPathComponent("file.txt"), atomically: true, encoding: .utf8)

        _ = try repo.restore(paths: ["file.txt"], options: RestoreOptions(source: baseCommit.hex, staged: false, worktree: true))

        XCTAssertEqual(try String(contentsOfFile: (tmp as NSString).appendingPathComponent("file.txt"), encoding: .utf8), "base\n")
        XCTAssertEqual(
            try readIndex(gitDir: repo.gitDir).find(path: "file.txt")?.oid,
            try materializeEntries(gitDir: repo.gitDir, commitOID: changedCommit)["file.txt"]?.oid
        )
    }

    func testRestoreMissingPathFails() throws {
        let tmp = testDirectory("swift_restore_missing_path_fails")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let commitTree = try writeTree(gitDir: repo.gitDir, files: [("file.txt", "committed\n")])
        let commit = try writeCommit(gitDir: repo.gitDir, treeOID: commitTree, parents: [], message: "commit", time: 1)

        try writeReference(gitDir: repo.gitDir, name: "refs/heads/main", oid: commit)
        try writeSymbolicReference(gitDir: repo.gitDir, name: "HEAD", target: "refs/heads/main")
        try seedWorkdir(from: commit, repo: repo)

        XCTAssertThrowsError(try repo.restore(paths: ["missing.txt"])) { error in
            guard case MuonGitError.notFound(let message) = error else {
                return XCTFail("expected notFound, got \(error)")
            }
            XCTAssertTrue(message.contains("missing.txt"))
        }
    }

    private struct MaterializedEntry {
        let path: String
        let oid: OID
        let mode: UInt32
        let data: Data
    }

    private func writeTree(gitDir: String, files: [(String, String)]) throws -> OID {
        let entries = try files.map { name, content in
            let blobOID = try writeLooseObject(gitDir: gitDir, type: .blob, data: Data(content.utf8))
            return TreeEntry(mode: FileMode.blob.rawValue, name: name, oid: blobOID)
        }
        return try writeLooseObject(gitDir: gitDir, type: .tree, data: serializeTree(entries: entries))
    }

    private func writeCommit(gitDir: String, treeOID: OID, parents: [OID], message: String, time: Int64) throws -> OID {
        let signature = Signature(name: "Muon Test", email: "test@muon.ai", time: time, offset: 0)
        let data = serializeCommit(
            treeId: treeOID,
            parentIds: parents,
            author: signature,
            committer: signature,
            message: "\(message)\n"
        )
        return try writeLooseObject(gitDir: gitDir, type: .commit, data: data)
    }

    private func materializeEntries(gitDir: String, commitOID: OID) throws -> [String: MaterializedEntry] {
        let commit = try readObject(gitDir: gitDir, oid: commitOID).asCommit()
        var result: [String: MaterializedEntry] = [:]
        try collectEntries(gitDir: gitDir, treeOID: commit.treeId, prefix: "", result: &result)
        return result
    }

    private func collectEntries(
        gitDir: String,
        treeOID: OID,
        prefix: String,
        result: inout [String: MaterializedEntry]
    ) throws {
        let tree = try readObject(gitDir: gitDir, oid: treeOID).asTree()
        for entry in tree.entries {
            let path = prefix.isEmpty ? entry.name : "\(prefix)/\(entry.name)"
            if entry.mode == FileMode.tree.rawValue {
                try collectEntries(gitDir: gitDir, treeOID: entry.oid, prefix: path, result: &result)
            } else {
                let blob = try readBlob(gitDir: gitDir, oid: entry.oid)
                result[path] = MaterializedEntry(path: path, oid: entry.oid, mode: entry.mode, data: blob.data)
            }
        }
    }

    private func seedWorkdir(from commitOID: OID, repo: Repository) throws {
        let entries = try materializeEntries(gitDir: repo.gitDir, commitOID: commitOID)
        try clearWorkdir(repo.workdir!)
        var index = Index()
        for path in entries.keys.sorted() {
            let entry = try XCTUnwrap(entries[path])
            index.add(IndexEntry(
                mode: entry.mode,
                fileSize: UInt32(entry.data.count),
                oid: entry.oid,
                flags: UInt16(min(path.utf8.count, 0x0FFF)),
                path: path
            ))
        }
        try writeIndex(gitDir: repo.gitDir, index: index)
        _ = try checkoutIndex(gitDir: repo.gitDir, workdir: try XCTUnwrap(repo.workdir), options: CheckoutOptions(force: true))
    }

    private func clearWorkdir(_ workdir: String) throws {
        for entry in try FileManager.default.contentsOfDirectory(atPath: workdir) where entry != ".git" {
            try FileManager.default.removeItem(atPath: (workdir as NSString).appendingPathComponent(entry))
        }
    }

    private func testDirectory(_ name: String) -> String {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("tmp/\(name)")
            .path
    }
}
