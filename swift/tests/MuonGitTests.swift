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
}
