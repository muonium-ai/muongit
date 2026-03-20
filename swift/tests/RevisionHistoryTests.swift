import XCTest
@testable import MuonGit

final class RevisionHistoryTests: XCTestCase {
    private struct Fixture {
        let a: OID
        let b: OID
        let c: OID
        let d: OID
        let e: OID
    }

    func testResolveRevisionExpressions() throws {
        let (repo, fixture, tmp) = try setupFixture("swift_revision_resolve")
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        XCTAssertEqual(try resolveRevision(gitDir: repo.gitDir, spec: "HEAD"), fixture.e)
        XCTAssertEqual(try resolveRevision(gitDir: repo.gitDir, spec: "mainline"), fixture.c)
        XCTAssertEqual(try resolveRevision(gitDir: repo.gitDir, spec: fixture.d.hex), fixture.d)
        XCTAssertEqual(try resolveRevision(gitDir: repo.gitDir, spec: "HEAD~1"), fixture.c)
        XCTAssertEqual(try resolveRevision(gitDir: repo.gitDir, spec: "HEAD^2"), fixture.d)
    }

    func testRevparseRanges() throws {
        let (repo, fixture, tmp) = try setupFixture("swift_revision_ranges")
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let twoDot = try revparse(gitDir: repo.gitDir, spec: "mainline..feature")
        XCTAssertTrue(twoDot.isRange)
        XCTAssertFalse(twoDot.usesMergeBase)
        XCTAssertEqual(twoDot.from, fixture.c)
        XCTAssertEqual(twoDot.to, fixture.d)

        let threeDot = try revparse(gitDir: repo.gitDir, spec: "mainline...feature")
        XCTAssertTrue(threeDot.isRange)
        XCTAssertTrue(threeDot.usesMergeBase)
        XCTAssertEqual(threeDot.from, fixture.c)
        XCTAssertEqual(threeDot.to, fixture.d)
    }

    func testRevwalkDefaultOrderAndFirstParent() throws {
        let (repo, fixture, tmp) = try setupFixture("swift_revwalk_default")
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let walker = Revwalk(gitDir: repo.gitDir)
        try walker.pushHead()
        XCTAssertEqual(try walker.allOids(), [fixture.e, fixture.d, fixture.c, fixture.b, fixture.a])

        let firstParent = Revwalk(gitDir: repo.gitDir)
        try firstParent.pushHead()
        firstParent.simplifyFirstParent()
        XCTAssertEqual(try firstParent.allOids(), [fixture.e, fixture.c, fixture.b, fixture.a])
    }

    func testRevwalkRangeSemantics() throws {
        let (repo, fixture, tmp) = try setupFixture("swift_revwalk_ranges")
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let twoDot = Revwalk(gitDir: repo.gitDir)
        try twoDot.pushRange("mainline..feature")
        XCTAssertEqual(try twoDot.allOids(), [fixture.d])

        let threeDot = Revwalk(gitDir: repo.gitDir)
        try threeDot.pushRange("mainline...feature")
        XCTAssertEqual(try threeDot.allOids(), [fixture.d, fixture.c])
    }

    func testRevwalkTopologicalTimeOrder() throws {
        let (repo, fixture, tmp) = try setupFixture("swift_revwalk_topo")
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let walker = Revwalk(gitDir: repo.gitDir)
        try walker.pushHead()
        walker.sorting([.topological, .time])
        XCTAssertEqual(try walker.allOids(), [fixture.e, fixture.d, fixture.c, fixture.b, fixture.a])
    }

    private func setupFixture(_ name: String) throws -> (Repository, Fixture, String) {
        let tmp = testDirectory(name)
        try? FileManager.default.removeItem(atPath: tmp)
        let repo = try Repository.create(at: tmp)
        let tree = try writeLooseObject(gitDir: repo.gitDir, type: .tree, data: Data())

        let a = try makeCommit(gitDir: repo.gitDir, tree: tree, parents: [], time: 1, message: "A\n")
        let b = try makeCommit(gitDir: repo.gitDir, tree: tree, parents: [a], time: 2, message: "B\n")
        let c = try makeCommit(gitDir: repo.gitDir, tree: tree, parents: [b], time: 3, message: "C\n")
        let d = try makeCommit(gitDir: repo.gitDir, tree: tree, parents: [b], time: 4, message: "D\n")
        let e = try makeCommit(gitDir: repo.gitDir, tree: tree, parents: [c, d], time: 5, message: "E\n")

        try writeReference(gitDir: repo.gitDir, name: "refs/heads/main", oid: e)
        try writeReference(gitDir: repo.gitDir, name: "refs/heads/mainline", oid: c)
        try writeReference(gitDir: repo.gitDir, name: "refs/heads/feature", oid: d)

        return (repo, Fixture(a: a, b: b, c: c, d: d, e: e), tmp)
    }

    private func makeCommit(
        gitDir: String,
        tree: OID,
        parents: [OID],
        time: Int64,
        message: String
    ) throws -> OID {
        let signature = Signature(name: "Muon Test", email: "test@muon.ai", time: time, offset: 0)
        let data = serializeCommit(
            treeId: tree,
            parentIds: parents,
            author: signature,
            committer: signature,
            message: message
        )
        return try writeLooseObject(gitDir: gitDir, type: .commit, data: data)
    }

    private func testDirectory(_ name: String) -> String {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("tmp/\(name)")
            .path
    }
}
