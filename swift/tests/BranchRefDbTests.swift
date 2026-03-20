import XCTest
@testable import MuonGit

final class BranchRefDbTests: XCTestCase {
    func testRefDbReadsLoosePackedAndSymbolicRefs() throws {
        let tmp = testDirectory("swift_refdb_reads_loose_packed_and_symbolic_refs")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let gitDir = repo.gitDir
        let mainOID = OID(hex: String(repeating: "a", count: 40))
        let packedOID = OID(hex: String(repeating: "b", count: 40))
        try writeReference(gitDir: gitDir, name: "refs/heads/main", oid: mainOID)
        try "\(packedOID.hex) refs/heads/release\n".write(
            toFile: (gitDir as NSString).appendingPathComponent("packed-refs"),
            atomically: true,
            encoding: .utf8
        )

        let head = try repo.refdb.read(name: "HEAD")
        XCTAssertTrue(head.isSymbolic)
        XCTAssertEqual(head.symbolicTarget, "refs/heads/main")

        let packed = try repo.refdb.read(name: "refs/heads/release")
        XCTAssertEqual(packed.target, packedOID)

        let refs = try repo.refdb.list()
        XCTAssertTrue(refs.contains(where: { $0.name == "refs/heads/main" }))
        XCTAssertTrue(refs.contains(where: { $0.name == "refs/heads/release" }))
    }

    func testRefDbDeleteRemovesPackedRef() throws {
        let tmp = testDirectory("swift_refdb_delete_removes_packed_ref")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let packedOID = OID(hex: String(repeating: "b", count: 40))
        try "\(packedOID.hex) refs/heads/release\n".write(
            toFile: (repo.gitDir as NSString).appendingPathComponent("packed-refs"),
            atomically: true,
            encoding: .utf8
        )

        XCTAssertTrue(try repo.refdb.delete(name: "refs/heads/release"))
        XCTAssertThrowsError(try repo.refdb.read(name: "refs/heads/release"))
    }

    func testBranchCreateLookupListAndUpstream() throws {
        let tmp = testDirectory("swift_branch_create_lookup_list_and_upstream")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let gitDir = repo.gitDir
        let mainOID = OID(hex: String(repeating: "a", count: 40))
        try writeReference(gitDir: gitDir, name: "refs/heads/main", oid: mainOID)

        let branch = try createBranch(gitDir: gitDir, name: "feature")
        XCTAssertEqual(branch.name, "feature")
        XCTAssertEqual(branch.referenceName, "refs/heads/feature")
        XCTAssertEqual(branch.target, mainOID)
        XCTAssertFalse(branch.isHEAD)

        try setBranchUpstream(
            gitDir: gitDir,
            name: "feature",
            upstream: BranchUpstream(remoteName: "origin", mergeRef: "refs/heads/main")
        )
        let lookedUp = try lookupBranch(gitDir: gitDir, name: "feature", kind: .local)
        XCTAssertEqual(lookedUp.upstream, BranchUpstream(remoteName: "origin", mergeRef: "refs/heads/main"))

        let branches = try listBranches(gitDir: gitDir, kind: .local)
        XCTAssertTrue(branches.contains(where: { $0.name == "main" && $0.isHEAD }))
        XCTAssertTrue(branches.contains(where: { $0.name == "feature" }))
    }

    func testBranchDetachedHeadRenameAndDeleteEdgeCases() throws {
        let tmp = testDirectory("swift_branch_detached_head_rename_delete_edge_cases")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let gitDir = repo.gitDir
        let detachedOID = OID(hex: String(repeating: "b", count: 40))
        try "\(detachedOID.hex)\n".write(
            toFile: (gitDir as NSString).appendingPathComponent("HEAD"),
            atomically: true,
            encoding: .utf8
        )
        let detachedBranch = try createBranch(gitDir: gitDir, name: "detached-copy")
        XCTAssertEqual(detachedBranch.target, detachedOID)

        let topicOID = OID(hex: String(repeating: "c", count: 40))
        try "\(topicOID.hex) refs/heads/topic\n".write(
            toFile: (gitDir as NSString).appendingPathComponent("packed-refs"),
            atomically: true,
            encoding: .utf8
        )
        try writeSymbolicReference(gitDir: gitDir, name: "HEAD", target: "refs/heads/topic")
        try setBranchUpstream(
            gitDir: gitDir,
            name: "topic",
            upstream: BranchUpstream(remoteName: "origin", mergeRef: "refs/heads/main")
        )

        let renamed = try renameBranch(gitDir: gitDir, oldName: "topic", newName: "renamed")
        XCTAssertEqual(renamed.name, "renamed")
        XCTAssertEqual(try readReference(gitDir: gitDir, name: "HEAD"), "ref: refs/heads/renamed")
        XCTAssertEqual(try branchUpstream(gitDir: gitDir, name: "renamed"), BranchUpstream(remoteName: "origin", mergeRef: "refs/heads/main"))

        XCTAssertThrowsError(try deleteBranch(gitDir: gitDir, name: "renamed", kind: .local))
    }

    private func testDirectory(_ name: String) -> String {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("tmp/\(name)")
            .path
    }
}
