import XCTest
@testable import MuonGit

final class StagingCommitPorcelainTests: XCTestCase {
    func testAddStagesModifiedAndUntrackedPathspecMatches() throws {
        let tmp = testDirectory("swift_porcelain_add_pathspec")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let index = try buildIndex(gitDir: repo.gitDir, files: [
            ("src/one.txt", "base\n"),
            ("notes.md", "keep\n"),
        ])
        try writeIndex(gitDir: repo.gitDir, index: index)
        try writeWorkdirFile(tmp, path: "src/one.txt", content: "changed\n")
        try writeWorkdirFile(tmp, path: "src/two.txt", content: "new\n")
        try writeWorkdirFile(tmp, path: "docs/readme.md", content: "skip\n")
        try writeWorkdirFile(tmp, path: "notes.md", content: "keep\n")

        let result = try repo.add(paths: ["src/*.txt"])

        XCTAssertEqual(result.stagedPaths, ["src/one.txt", "src/two.txt"])
        XCTAssertEqual(result.removedPaths, [])

        let updated = try readIndex(gitDir: repo.gitDir)
        let one = try XCTUnwrap(updated.find(path: "src/one.txt"))
        let two = try XCTUnwrap(updated.find(path: "src/two.txt"))
        XCTAssertNil(updated.find(path: "docs/readme.md"))
        XCTAssertEqual(try readBlob(gitDir: repo.gitDir, oid: one.oid).data, Data("changed\n".utf8))
        XCTAssertEqual(try readBlob(gitDir: repo.gitDir, oid: two.oid).data, Data("new\n".utf8))
    }

    func testRemoveDeletesTrackedPathsFromIndexAndWorkdir() throws {
        let tmp = testDirectory("swift_porcelain_remove")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        _ = try seedHead(repo: repo, files: [("tracked.txt", "tracked\n"), ("keep.txt", "keep\n")], message: "base")

        let result = try repo.remove(paths: ["tracked.txt"])

        XCTAssertEqual(result.removedFromIndex, ["tracked.txt"])
        XCTAssertEqual(result.removedFromWorkdir, ["tracked.txt"])
        XCTAssertNil(try readIndex(gitDir: repo.gitDir).find(path: "tracked.txt"))
        XCTAssertFalse(FileManager.default.fileExists(atPath: (tmp as NSString).appendingPathComponent("tracked.txt")))
        XCTAssertTrue(FileManager.default.fileExists(atPath: (tmp as NSString).appendingPathComponent("keep.txt")))
    }

    func testUnstageRestoresHeadEntriesAndDropsNewPaths() throws {
        let tmp = testDirectory("swift_porcelain_unstage")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        _ = try seedHead(repo: repo, files: [("tracked.txt", "base\n")], message: "base")
        try writeWorkdirFile(tmp, path: "tracked.txt", content: "staged\n")
        try writeWorkdirFile(tmp, path: "new.txt", content: "new\n")
        _ = try repo.add(paths: ["tracked.txt", "new.txt"])

        let result = try repo.unstage(paths: ["tracked.txt", "new.txt"])

        XCTAssertEqual(result.restoredPaths, ["tracked.txt"])
        XCTAssertEqual(result.removedPaths, ["new.txt"])

        let updated = try readIndex(gitDir: repo.gitDir)
        let tracked = try XCTUnwrap(updated.find(path: "tracked.txt"))
        XCTAssertEqual(try readBlob(gitDir: repo.gitDir, oid: tracked.oid).data, Data("base\n".utf8))
        XCTAssertNil(updated.find(path: "new.txt"))
        XCTAssertTrue(FileManager.default.fileExists(atPath: (tmp as NSString).appendingPathComponent("new.txt")))
    }

    func testUnstageOnUnbornBranchRemovesNewEntries() throws {
        let tmp = testDirectory("swift_porcelain_unstage_unborn")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        try writeWorkdirFile(tmp, path: "new.txt", content: "new\n")
        _ = try repo.add(paths: ["new.txt"])

        let result = try repo.unstage(paths: ["new.txt"])

        XCTAssertEqual(result.restoredPaths, [])
        XCTAssertEqual(result.removedPaths, ["new.txt"])
        XCTAssertTrue(try readIndex(gitDir: repo.gitDir).entries.isEmpty)
    }

    func testCommitUpdatesBranchAndReflogs() throws {
        let tmp = testDirectory("swift_porcelain_commit")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let base = try seedHead(repo: repo, files: [("tracked.txt", "base\n"), ("remove.txt", "remove me\n")], message: "base")
        try writeWorkdirFile(tmp, path: "tracked.txt", content: "changed\n")
        try writeWorkdirFile(tmp, path: "new.txt", content: "new\n")
        _ = try repo.add(paths: ["tracked.txt", "new.txt"])
        _ = try repo.remove(paths: ["remove.txt"])

        let result = try repo.commit(message: "second")

        XCTAssertEqual(result.reference, "refs/heads/main")
        XCTAssertEqual(result.parentIDs, [base])
        XCTAssertEqual(result.summary, "second")
        XCTAssertEqual(try resolveReference(gitDir: repo.gitDir, name: "HEAD"), result.oid)
        XCTAssertEqual(try resolveReference(gitDir: repo.gitDir, name: "refs/heads/main"), result.oid)

        let commit = try readObject(gitDir: repo.gitDir, oid: result.oid).asCommit()
        XCTAssertEqual(commit.parentIds, [base])
        let headLog = try readReflog(gitDir: repo.gitDir, refName: "HEAD")
        let branchLog = try readReflog(gitDir: repo.gitDir, refName: "refs/heads/main")
        XCTAssertEqual(headLog.last?.message, "commit: second")
        XCTAssertEqual(branchLog.last?.message, "commit: second")
    }

    func testCommitRejectsDetachedHead() throws {
        let tmp = testDirectory("swift_porcelain_commit_detached")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let base = try seedHead(repo: repo, files: [("tracked.txt", "base\n")], message: "base")
        try writeReference(gitDir: repo.gitDir, name: "HEAD", oid: base)

        XCTAssertThrowsError(try repo.commit(message: "detached")) { error in
            guard case MuonGitError.invalidSpec(let message) = error else {
                return XCTFail("expected invalidSpec, got \(error)")
            }
            XCTAssertTrue(message.contains("detached HEAD"))
        }
    }

    private func buildIndex(gitDir: String, files: [(String, String)]) throws -> Index {
        var index = Index()
        for (path, content) in files {
            let oid = try writeLooseObject(gitDir: gitDir, type: .blob, data: Data(content.utf8))
            index.add(
                IndexEntry(
                    mode: FileMode.blob.rawValue,
                    fileSize: UInt32(content.utf8.count),
                    oid: oid,
                    flags: UInt16(min(path.utf8.count, 0x0FFF)),
                    path: path
                )
            )
        }
        return index
    }

    private func seedHead(repo: Repository, files: [(String, String)], message: String) throws -> OID {
        let commit = try writeCommitSnapshot(gitDir: repo.gitDir, files: files, parents: [], message: message, time: 1)
        try writeReference(gitDir: repo.gitDir, name: "refs/heads/main", oid: commit)
        try writeIndex(gitDir: repo.gitDir, index: try buildIndex(gitDir: repo.gitDir, files: files))
        for (path, content) in files {
            try writeWorkdirFile(repo.workdir!, path: path, content: content)
        }
        return commit
    }

    private func writeCommitSnapshot(
        gitDir: String,
        files: [(String, String)],
        parents: [OID],
        message: String,
        time: Int64
    ) throws -> OID {
        let entries = try files.map { path, content in
            let oid = try writeLooseObject(gitDir: gitDir, type: .blob, data: Data(content.utf8))
            return TreeEntry(mode: FileMode.blob.rawValue, name: path, oid: oid)
        }
        let treeOID = try writeLooseObject(gitDir: gitDir, type: .tree, data: serializeTree(entries: entries))
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

    private func writeWorkdirFile(_ workdir: String, path: String, content: String) throws {
        let fullPath = (workdir as NSString).appendingPathComponent(path)
        try FileManager.default.createDirectory(
            atPath: (fullPath as NSString).deletingLastPathComponent,
            withIntermediateDirectories: true
        )
        try content.write(toFile: fullPath, atomically: true, encoding: .utf8)
    }

    private func testDirectory(_ name: String) -> String {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("tmp")
            .appendingPathComponent(name)
            .path
    }
}
