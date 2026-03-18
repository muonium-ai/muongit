import XCTest
@testable import MuonGit

final class MuonGitTests: XCTestCase {
    func testOIDFromHex() {
        let hex = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let oid = OID(hex: hex)
        XCTAssertEqual(oid.hex, hex)
    }

    func testOIDEquality() {
        let a = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let b = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        XCTAssertEqual(a, b)
    }

    func testSignature() {
        let sig = Signature(name: "Test User", email: "test@example.com")
        XCTAssertEqual(sig.name, "Test User")
        XCTAssertEqual(sig.email, "test@example.com")
    }

    func testVersion() {
        XCTAssertEqual(MuonGitVersion.string, "0.1.0")
        XCTAssertEqual(MuonGitVersion.libgit2Parity, "1.9.0")
    }

    func testObjectType() {
        XCTAssertEqual(ObjectType.commit.rawValue, 1)
        XCTAssertEqual(ObjectType.tree.rawValue, 2)
        XCTAssertEqual(ObjectType.blob.rawValue, 3)
        XCTAssertEqual(ObjectType.tag.rawValue, 4)
    }

    // MARK: - SHA-1 Tests

    func testSHA1Empty() {
        let digest = SHA1.hash([UInt8]())
        let hex = digest.map { String(format: "%02x", $0) }.joined()
        XCTAssertEqual(hex, "da39a3ee5e6b4b0d3255bfef95601890afd80709")
    }

    func testSHA1Hello() {
        let digest = SHA1.hash("hello")
        let hex = digest.map { String(format: "%02x", $0) }.joined()
        XCTAssertEqual(hex, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
    }

    func testSHA1GitBlob() {
        // git hash-object equivalent: "hello\n" as blob
        let data = Array("hello\n".utf8)
        let oid = OID.hash(type: .blob, data: data)
        XCTAssertEqual(oid.hex, "ce013625030ba8dba906f756967f9e9ca394464a")
    }

    func testOIDZero() {
        let z = OID.zero
        XCTAssertTrue(z.isZero)
        XCTAssertEqual(z.hex, "0000000000000000000000000000000000000000")
    }

    // MARK: - Repository Tests

    func testInitAndOpen() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_init"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        XCTAssertFalse(repo.isBare)
        XCTAssertNotNil(repo.workdir)
        XCTAssertTrue(repo.isHeadUnborn)

        let repo2 = try Repository.open(at: tmp)
        XCTAssertFalse(repo2.isBare)
        XCTAssertEqual(try repo2.head(), "ref: refs/heads/main")
    }

    func testInitBare() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_bare"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp, bare: true)
        XCTAssertTrue(repo.isBare)
        XCTAssertNil(repo.workdir)

        let repo2 = try Repository.open(at: tmp)
        XCTAssertTrue(repo2.isBare)
    }

    func testOpenNonexistent() {
        XCTAssertThrowsError(try Repository.open(at: "/tmp/muongit_does_not_exist_12345"))
    }

    func testDiscover() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_discover"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let _ = try Repository.create(at: tmp)
        let subdir = (tmp as NSString).appendingPathComponent("a/b/c")
        try FileManager.default.createDirectory(atPath: subdir, withIntermediateDirectories: true)

        let found = try Repository.discover(at: subdir)
        XCTAssertFalse(found.isBare)
    }

    // MARK: - ODB Tests

    func testWriteAndReadLooseObject() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_odb"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let blobData = Data("hello, muongit!\n".utf8)

        // Write a blob
        let oid = try writeLooseObject(gitDir: repo.gitDir, type: .blob, data: blobData)
        XCTAssertFalse(oid.isZero)

        // Verify the OID matches what we'd compute directly
        let expectedOID = OID.hash(type: .blob, data: Array(blobData))
        XCTAssertEqual(oid, expectedOID)

        // Read it back
        let (readType, readData) = try readLooseObject(gitDir: repo.gitDir, oid: oid)
        XCTAssertEqual(readType, .blob)
        XCTAssertEqual(readData, blobData)
    }

    func testWriteAndReadCommitObject() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_odb_commit"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let commitData = Data("tree 0000000000000000000000000000000000000000\nauthor Test <test@test.com> 0 +0000\ncommitter Test <test@test.com> 0 +0000\n\ntest commit\n".utf8)

        let oid = try writeLooseObject(gitDir: repo.gitDir, type: .commit, data: commitData)
        let (readType, readData) = try readLooseObject(gitDir: repo.gitDir, oid: oid)
        XCTAssertEqual(readType, .commit)
        XCTAssertEqual(readData, commitData)
    }

    func testWriteIdempotent() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_odb_idempotent"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let data = Data("idempotent test\n".utf8)

        let oid1 = try writeLooseObject(gitDir: repo.gitDir, type: .blob, data: data)
        let oid2 = try writeLooseObject(gitDir: repo.gitDir, type: .blob, data: data)
        XCTAssertEqual(oid1, oid2)
    }

    func testReadNonexistentObject() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_odb_missing"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let fakeOid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        XCTAssertThrowsError(try readLooseObject(gitDir: repo.gitDir, oid: fakeOid))
    }

    // MARK: - Refs Tests

    func testReadReference() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_refs"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let headValue = try readReference(gitDir: repo.gitDir, name: "HEAD")
        XCTAssertEqual(headValue, "ref: refs/heads/main")
    }

    func testResolveReferenceUnbornThrows() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_refs_unborn"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        XCTAssertThrowsError(try resolveReference(gitDir: repo.gitDir, name: "HEAD")) { error in
            guard case MuonGitError.notFound = error else {
                XCTFail("Expected MuonGitError.notFound, got \(error)")
                return
            }
        }
    }

    func testResolveHeadWithCommit() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_refs_resolve"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let fakeOid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let refPath = (repo.gitDir as NSString).appendingPathComponent("refs/heads/main")
        try fakeOid.write(toFile: refPath, atomically: true, encoding: .utf8)

        let resolved = try resolveReference(gitDir: repo.gitDir, name: "HEAD")
        XCTAssertEqual(resolved.hex, fakeOid)
    }

    func testPackedRefs() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_refs_packed"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let packedOid = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let packedContent = "# pack-refs with: peeled fully-peeled sorted\n\(packedOid) refs/tags/v1.0\n"
        try packedContent.write(
            toFile: (repo.gitDir as NSString).appendingPathComponent("packed-refs"),
            atomically: true, encoding: .utf8
        )

        let tagValue = try readReference(gitDir: repo.gitDir, name: "refs/tags/v1.0")
        XCTAssertEqual(tagValue, packedOid)
    }

    func testListReferences() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_refs_list"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let oid1 = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let oid2 = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"

        let mainPath = (repo.gitDir as NSString).appendingPathComponent("refs/heads/main")
        try oid1.write(toFile: mainPath, atomically: true, encoding: .utf8)

        let featurePath = (repo.gitDir as NSString).appendingPathComponent("refs/heads/feature")
        try oid2.write(toFile: featurePath, atomically: true, encoding: .utf8)

        let refs = try listReferences(gitDir: repo.gitDir)
        let refMap = Dictionary(uniqueKeysWithValues: refs)
        XCTAssertEqual(refMap["refs/heads/main"], oid1)
        XCTAssertEqual(refMap["refs/heads/feature"], oid2)
    }

    func testLooseOverridesPacked() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_refs_override"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let packedOid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let looseOid = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"

        try "# pack-refs\n\(packedOid) refs/tags/v1.0\n".write(
            toFile: (repo.gitDir as NSString).appendingPathComponent("packed-refs"),
            atomically: true, encoding: .utf8
        )

        let tagsDir = ((repo.gitDir as NSString).appendingPathComponent("refs") as NSString).appendingPathComponent("tags")
        try FileManager.default.createDirectory(atPath: tagsDir, withIntermediateDirectories: true)
        try looseOid.write(
            toFile: (tagsDir as NSString).appendingPathComponent("v1.0"),
            atomically: true, encoding: .utf8
        )

        let refs = try listReferences(gitDir: repo.gitDir)
        let refMap = Dictionary(uniqueKeysWithValues: refs)
        XCTAssertEqual(refMap["refs/tags/v1.0"], looseOid)
    }
}
