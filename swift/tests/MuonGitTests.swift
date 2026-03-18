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

    // MARK: - Commit Tests

    func testParseAndSerializeCommit() throws {
        let treeId = OID(hex: "4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        let author = Signature(name: "Author", email: "author@example.com", time: 1234567890, offset: 0)
        let committer = Signature(name: "Committer", email: "committer@example.com", time: 1234567890, offset: 0)

        let data = serializeCommit(treeId: treeId, parentIds: [], author: author, committer: committer, message: "Initial commit\n")
        let oid = OID.hash(type: .commit, data: Array(data))
        let commit = try parseCommit(oid: oid, data: data)

        XCTAssertEqual(commit.treeId, treeId)
        XCTAssertTrue(commit.parentIds.isEmpty)
        XCTAssertEqual(commit.author.name, "Author")
        XCTAssertEqual(commit.committer.email, "committer@example.com")
        XCTAssertEqual(commit.message, "Initial commit\n")
        XCTAssertNil(commit.messageEncoding)
    }

    func testCommitWithParents() throws {
        let treeId = OID(hex: "4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        let parent1 = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let parent2 = OID(hex: "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let sig = Signature(name: "Test", email: "test@test.com")

        let data = serializeCommit(treeId: treeId, parentIds: [parent1, parent2], author: sig, committer: sig, message: "merge\n")
        let oid = OID.hash(type: .commit, data: Array(data))
        let commit = try parseCommit(oid: oid, data: data)

        XCTAssertEqual(commit.parentIds.count, 2)
        XCTAssertEqual(commit.parentIds[0], parent1)
        XCTAssertEqual(commit.parentIds[1], parent2)
    }

    func testCommitMissingTreeThrows() {
        let raw = Data("author Test <t@t.com> 0 +0000\ncommitter Test <t@t.com> 0 +0000\n\nmsg\n".utf8)
        XCTAssertThrowsError(try parseCommit(oid: OID.zero, data: raw))
    }

    func testCommitWithEncoding() throws {
        let treeId = OID(hex: "4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        let sig = Signature(name: "Test", email: "test@test.com", time: 100)

        let data = serializeCommit(treeId: treeId, parentIds: [], author: sig, committer: sig, message: "msg\n", messageEncoding: "UTF-8")
        let oid = OID.hash(type: .commit, data: Array(data))
        let commit = try parseCommit(oid: oid, data: data)

        XCTAssertEqual(commit.messageEncoding, "UTF-8")
    }

    func testSignatureParsing() {
        let sig = parseSignature("Test User <test@example.com> 1234567890 +0530")
        XCTAssertEqual(sig.name, "Test User")
        XCTAssertEqual(sig.email, "test@example.com")
        XCTAssertEqual(sig.time, 1234567890)
        XCTAssertEqual(sig.offset, 330) // 5*60+30
    }

    func testSignatureFormatNegativeOffset() {
        let sig = Signature(name: "Test", email: "test@test.com", time: 1000, offset: -480)
        let formatted = formatSignature(sig)
        XCTAssertEqual(formatted, "Test <test@test.com> 1000 -0800")
    }

    func testCommitODBRoundTrip() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_commit_odb"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let treeId = OID(hex: "4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        let sig = Signature(name: "Test", email: "test@test.com", time: 1234567890, offset: 0)

        let commitData = serializeCommit(treeId: treeId, parentIds: [], author: sig, committer: sig, message: "test\n")
        let oid = try writeLooseObject(gitDir: repo.gitDir, type: .commit, data: commitData)

        let (readType, readData) = try readLooseObject(gitDir: repo.gitDir, oid: oid)
        XCTAssertEqual(readType, .commit)

        let commit = try parseCommit(oid: oid, data: readData)
        XCTAssertEqual(commit.treeId, treeId)
        XCTAssertEqual(commit.author.name, "Test")
        XCTAssertEqual(commit.message, "test\n")
    }

    // MARK: - Tree Tests

    func testSerializeAndParseTree() throws {
        let blobOid = OID(hex: "ce013625030ba8dba906f756967f9e9ca394464a")
        let entries = [TreeEntry(mode: FileMode.blob.rawValue, name: "hello.txt", oid: blobOid)]

        let data = serializeTree(entries: entries)
        let treeOid = OID.hash(type: .tree, data: Array(data))
        let tree = try parseTree(oid: treeOid, data: data)

        XCTAssertEqual(tree.entries.count, 1)
        XCTAssertEqual(tree.entries[0].name, "hello.txt")
        XCTAssertEqual(tree.entries[0].mode, FileMode.blob.rawValue)
        XCTAssertEqual(tree.entries[0].oid, blobOid)
    }

    func testTreeMultipleEntriesSorted() throws {
        let oid1 = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let oid2 = OID(hex: "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let oid3 = OID(hex: "ccf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

        let entries = [
            TreeEntry(mode: FileMode.blob.rawValue, name: "z.txt", oid: oid1),
            TreeEntry(mode: FileMode.blob.rawValue, name: "a.txt", oid: oid2),
            TreeEntry(mode: FileMode.tree.rawValue, name: "lib", oid: oid3),
        ]

        let data = serializeTree(entries: entries)
        let treeOid = OID.hash(type: .tree, data: Array(data))
        let tree = try parseTree(oid: treeOid, data: data)

        XCTAssertEqual(tree.entries.count, 3)
        XCTAssertEqual(tree.entries[0].name, "a.txt")
        XCTAssertEqual(tree.entries[1].name, "lib")
        XCTAssertTrue(tree.entries[1].isTree)
        XCTAssertEqual(tree.entries[2].name, "z.txt")
    }

    func testTreeEntryTypes() {
        let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

        let blob = TreeEntry(mode: FileMode.blob.rawValue, name: "f", oid: oid)
        XCTAssertTrue(blob.isBlob)
        XCTAssertFalse(blob.isTree)

        let exe = TreeEntry(mode: FileMode.blobExe.rawValue, name: "f", oid: oid)
        XCTAssertTrue(exe.isBlob)

        let tree = TreeEntry(mode: FileMode.tree.rawValue, name: "d", oid: oid)
        XCTAssertTrue(tree.isTree)
        XCTAssertFalse(tree.isBlob)
    }

    func testParseEmptyTree() throws {
        let oid = OID.hash(type: .tree, data: [])
        let tree = try parseTree(oid: oid, data: Data())
        XCTAssertTrue(tree.entries.isEmpty)
    }

    func testTreeODBRoundTrip() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_tree_odb"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let blobOid = OID(hex: "ce013625030ba8dba906f756967f9e9ca394464a")
        let entries = [TreeEntry(mode: FileMode.blob.rawValue, name: "file.txt", oid: blobOid)]

        let treeData = serializeTree(entries: entries)
        let oid = try writeLooseObject(gitDir: repo.gitDir, type: .tree, data: treeData)

        let (readType, readData) = try readLooseObject(gitDir: repo.gitDir, oid: oid)
        XCTAssertEqual(readType, .tree)

        let tree = try parseTree(oid: oid, data: readData)
        XCTAssertEqual(tree.entries.count, 1)
        XCTAssertEqual(tree.entries[0].name, "file.txt")
    }

    // MARK: - Blob Tests

    func testHashBlob() {
        let oid = hashBlob(data: Data("hello\n".utf8))
        XCTAssertEqual(oid.hex, "ce013625030ba8dba906f756967f9e9ca394464a")
    }

    func testHashBlobEmpty() {
        let oid = hashBlob(data: Data())
        XCTAssertEqual(oid.hex, "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391")
    }

    func testWriteAndReadBlob() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_blob_rw"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let content = Data("blob content\n".utf8)
        let oid = try writeBlob(gitDir: repo.gitDir, data: content)
        let blob = try readBlob(gitDir: repo.gitDir, oid: oid)

        XCTAssertEqual(blob.data, content)
        XCTAssertEqual(blob.size, content.count)
        XCTAssertEqual(blob.oid, oid)
    }

    func testWriteBlobFromFile() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_blob_file"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let workdir = repo.workdir!
        let filePath = (workdir as NSString).appendingPathComponent("test.txt")
        try "file content\n".write(toFile: filePath, atomically: true, encoding: .utf8)

        let oid = try writeBlobFromFile(gitDir: repo.gitDir, path: filePath)
        let expected = hashBlob(data: Data("file content\n".utf8))
        XCTAssertEqual(oid, expected)

        let blob = try readBlob(gitDir: repo.gitDir, oid: oid)
        XCTAssertEqual(String(data: blob.data, encoding: .utf8), "file content\n")
    }

    func testReadNonBlobTypeErrors() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_blob_type_err"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let commitData = Data("tree 0000000000000000000000000000000000000000\nauthor T <t@t> 0 +0000\ncommitter T <t@t> 0 +0000\n\nm\n".utf8)
        let oid = try writeLooseObject(gitDir: repo.gitDir, type: .commit, data: commitData)

        XCTAssertThrowsError(try readBlob(gitDir: repo.gitDir, oid: oid))
    }

    // MARK: - Tag Tests

    func testParseAndSerializeTag() throws {
        let targetId = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let tagger = Signature(name: "Tagger", email: "tagger@example.com", time: 1234567890, offset: 0)

        let data = serializeTag(targetId: targetId, targetType: .commit, tagName: "v1.0", tagger: tagger, message: "Release v1.0\n")
        let oid = OID.hash(type: .tag, data: Array(data))
        let tag = try parseTag(oid: oid, data: data)

        XCTAssertEqual(tag.targetId, targetId)
        XCTAssertEqual(tag.targetType, .commit)
        XCTAssertEqual(tag.tagName, "v1.0")
        XCTAssertEqual(tag.tagger?.name, "Tagger")
        XCTAssertEqual(tag.message, "Release v1.0\n")
    }

    func testTagWithoutTagger() throws {
        let targetId = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let data = serializeTag(targetId: targetId, targetType: .commit, tagName: "v0.1", tagger: nil, message: "lightweight\n")
        let oid = OID.hash(type: .tag, data: Array(data))
        let tag = try parseTag(oid: oid, data: data)

        XCTAssertNil(tag.tagger)
        XCTAssertEqual(tag.tagName, "v0.1")
    }

    func testTagMissingObjectThrows() {
        let raw = Data("type commit\ntag v1\n\nmsg\n".utf8)
        XCTAssertThrowsError(try parseTag(oid: OID.zero, data: raw))
    }

    func testTagTargetingTree() throws {
        let targetId = OID(hex: "4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        let data = serializeTag(targetId: targetId, targetType: .tree, tagName: "tree-tag", tagger: nil, message: "tag a tree\n")
        let oid = OID.hash(type: .tag, data: Array(data))
        let tag = try parseTag(oid: oid, data: data)

        XCTAssertEqual(tag.targetType, .tree)
    }

    func testTagODBRoundTrip() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_tag_odb"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let targetId = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let tagger = Signature(name: "T", email: "t@t.com", time: 100, offset: 0)
        let tagData = serializeTag(targetId: targetId, targetType: .commit, tagName: "v1.0", tagger: tagger, message: "msg\n")
        let oid = try writeLooseObject(gitDir: repo.gitDir, type: .tag, data: tagData)

        let (readType, readData) = try readLooseObject(gitDir: repo.gitDir, oid: oid)
        XCTAssertEqual(readType, .tag)

        let tag = try parseTag(oid: oid, data: readData)
        XCTAssertEqual(tag.tagName, "v1.0")
        XCTAssertEqual(tag.targetId, targetId)
    }

    // MARK: - Ref Write/Update/Delete Tests

    func testWriteAndReadReference() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_ref_write"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        try writeReference(gitDir: repo.gitDir, name: "refs/heads/feature", oid: oid)

        let value = try readReference(gitDir: repo.gitDir, name: "refs/heads/feature")
        XCTAssertEqual(value, oid.hex)
    }

    func testWriteSymbolicReference() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_ref_sym"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        try writeSymbolicReference(gitDir: repo.gitDir, name: "refs/heads/alias", target: "refs/heads/main")

        let value = try readReference(gitDir: repo.gitDir, name: "refs/heads/alias")
        XCTAssertEqual(value, "ref: refs/heads/main")
    }

    func testDeleteReference() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_ref_delete"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        try writeReference(gitDir: repo.gitDir, name: "refs/heads/feature", oid: oid)

        let deleted = try deleteReference(gitDir: repo.gitDir, name: "refs/heads/feature")
        XCTAssertTrue(deleted)

        XCTAssertThrowsError(try readReference(gitDir: repo.gitDir, name: "refs/heads/feature"))

        let notDeleted = try deleteReference(gitDir: repo.gitDir, name: "refs/heads/nonexistent")
        XCTAssertFalse(notDeleted)
    }

    func testUpdateReferenceSuccess() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_ref_update"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let oid1 = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let oid2 = OID(hex: "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

        // Create with zero old
        try updateReference(gitDir: repo.gitDir, name: "refs/heads/feature", newOid: oid1, oldOid: OID.zero)
        XCTAssertEqual(try readReference(gitDir: repo.gitDir, name: "refs/heads/feature"), oid1.hex)

        // Update with matching old
        try updateReference(gitDir: repo.gitDir, name: "refs/heads/feature", newOid: oid2, oldOid: oid1)
        XCTAssertEqual(try readReference(gitDir: repo.gitDir, name: "refs/heads/feature"), oid2.hex)
    }

    func testUpdateReferenceConflict() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_ref_cas"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let oid1 = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let oid2 = OID(hex: "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let oidWrong = OID(hex: "ccf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

        try writeReference(gitDir: repo.gitDir, name: "refs/heads/feature", oid: oid1)

        // Wrong old value should fail
        XCTAssertThrowsError(try updateReference(gitDir: repo.gitDir, name: "refs/heads/feature", newOid: oid2, oldOid: oidWrong))

        // Create-only should fail if exists
        XCTAssertThrowsError(try updateReference(gitDir: repo.gitDir, name: "refs/heads/feature", newOid: oid2, oldOid: OID.zero))
    }

    // MARK: - Config Tests

    func testParseSimpleConfig() throws {
        let content = "[core]\n\tbare = false\n\trepositoryformatversion = 0\n"
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_config_parse"
        try? FileManager.default.removeItem(atPath: tmp)
        try FileManager.default.createDirectory(atPath: tmp, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let configPath = (tmp as NSString).appendingPathComponent("config")
        try content.write(toFile: configPath, atomically: true, encoding: .utf8)

        let config = try Config.load(from: configPath)
        XCTAssertEqual(config.get(section: "core", key: "bare"), "false")
        XCTAssertEqual(config.getBool(section: "core", key: "bare"), false)
        XCTAssertEqual(config.getInt(section: "core", key: "repositoryformatversion"), 0)
    }

    func testConfigSubsection() throws {
        let content = "[remote \"origin\"]\n\turl = https://example.com/repo.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n"
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_config_sub"
        try? FileManager.default.removeItem(atPath: tmp)
        try FileManager.default.createDirectory(atPath: tmp, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let configPath = (tmp as NSString).appendingPathComponent("config")
        try content.write(toFile: configPath, atomically: true, encoding: .utf8)

        let config = try Config.load(from: configPath)
        XCTAssertEqual(config.get(section: "remote.origin", key: "url"), "https://example.com/repo.git")
        XCTAssertEqual(config.get(section: "remote.origin", key: "fetch"), "+refs/heads/*:refs/remotes/origin/*")
    }

    func testConfigSetAndUnset() {
        let config = Config()
        config.set(section: "core", key: "bare", value: "true")
        XCTAssertEqual(config.get(section: "core", key: "bare"), "true")

        config.set(section: "core", key: "bare", value: "false")
        XCTAssertEqual(config.get(section: "core", key: "bare"), "false")

        config.unset(section: "core", key: "bare")
        XCTAssertNil(config.get(section: "core", key: "bare"))
    }

    func testConfigCaseInsensitive() {
        let config = Config()
        config.set(section: "Core", key: "Bare", value: "true")
        XCTAssertEqual(config.get(section: "core", key: "bare"), "true")
        XCTAssertEqual(config.get(section: "CORE", key: "BARE"), "true")
    }

    func testConfigIntSuffixes() {
        XCTAssertEqual(parseConfigInt("42"), 42)
        XCTAssertEqual(parseConfigInt("1k"), 1024)
        XCTAssertEqual(parseConfigInt("2m"), 2 * 1024 * 1024)
        XCTAssertEqual(parseConfigInt("1g"), 1024 * 1024 * 1024)
    }

    func testConfigRoundTrip() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_config_rt"
        try? FileManager.default.removeItem(atPath: tmp)
        try FileManager.default.createDirectory(atPath: tmp, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let configPath = (tmp as NSString).appendingPathComponent("config")
        let config = Config(path: configPath)
        config.set(section: "core", key: "bare", value: "false")
        config.set(section: "core", key: "repositoryformatversion", value: "0")
        config.set(section: "remote.origin", key: "url", value: "https://example.com/repo.git")
        try config.save()

        let loaded = try Config.load(from: configPath)
        XCTAssertEqual(loaded.get(section: "core", key: "bare"), "false")
        XCTAssertEqual(loaded.get(section: "remote.origin", key: "url"), "https://example.com/repo.git")
    }

    func testRepoConfig() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_config_repo"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let configPath = (repo.gitDir as NSString).appendingPathComponent("config")
        let config = try Config.load(from: configPath)
        XCTAssertEqual(config.getBool(section: "core", key: "bare"), false)
    }

    // MARK: - Reflog Tests

    func testParseReflogEntry() {
        let content = "0000000000000000000000000000000000000000 aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d Test <test@test.com> 1234567890 +0000\tcommit (initial): first commit\n"
        let entries = parseReflog(content)
        XCTAssertEqual(entries.count, 1)
        XCTAssertTrue(entries[0].oldOid.isZero)
        XCTAssertEqual(entries[0].newOid.hex, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        XCTAssertEqual(entries[0].committer.name, "Test")
        XCTAssertEqual(entries[0].message, "commit (initial): first commit")
    }

    func testAppendAndReadReflog() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_reflog_rw"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let zero = OID.zero
        let oid1 = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let oid2 = OID(hex: "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let sig = Signature(name: "Test", email: "t@t.com", time: 100, offset: 0)

        try appendReflog(gitDir: repo.gitDir, refName: "HEAD", oldOid: zero, newOid: oid1, committer: sig, message: "commit (initial): first")
        try appendReflog(gitDir: repo.gitDir, refName: "HEAD", oldOid: oid1, newOid: oid2, committer: sig, message: "commit: second")

        let entries = try readReflog(gitDir: repo.gitDir, refName: "HEAD")
        XCTAssertEqual(entries.count, 2)
        XCTAssertTrue(entries[0].oldOid.isZero)
        XCTAssertEqual(entries[0].newOid, oid1)
        XCTAssertEqual(entries[0].message, "commit (initial): first")
        XCTAssertEqual(entries[1].oldOid, oid1)
        XCTAssertEqual(entries[1].newOid, oid2)
    }

    func testReadNonexistentReflog() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_reflog_empty"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let entries = try readReflog(gitDir: repo.gitDir, refName: "HEAD")
        XCTAssertTrue(entries.isEmpty)
    }

    func testReflogForBranch() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_reflog_branch"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let sig = Signature(name: "T", email: "t@t", time: 0, offset: 0)

        try appendReflog(gitDir: repo.gitDir, refName: "refs/heads/main", oldOid: OID.zero, newOid: oid, committer: sig, message: "branch: Created")

        let entries = try readReflog(gitDir: repo.gitDir, refName: "refs/heads/main")
        XCTAssertEqual(entries.count, 1)
        XCTAssertEqual(entries[0].message, "branch: Created")
    }

    // MARK: - Index Tests

    func testReadWriteEmptyIndex() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_index_empty"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let index = Index()
        try writeIndex(gitDir: repo.gitDir, index: index)

        let loaded = try readIndex(gitDir: repo.gitDir)
        XCTAssertEqual(loaded.version, 2)
        XCTAssertTrue(loaded.entries.isEmpty)
    }

    func testReadWriteSingleEntry() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_index_single"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let oid = OID(hex: "ce013625030ba8dba906f756967f9e9ca394464a")
        let entry = IndexEntry(mode: 0o100644, fileSize: 6, oid: oid, path: "hello.txt")

        var index = Index()
        index.add(entry)
        try writeIndex(gitDir: repo.gitDir, index: index)

        let loaded = try readIndex(gitDir: repo.gitDir)
        XCTAssertEqual(loaded.entries.count, 1)
        XCTAssertEqual(loaded.entries[0].path, "hello.txt")
        XCTAssertEqual(loaded.entries[0].mode, 0o100644)
        XCTAssertEqual(loaded.entries[0].oid, oid)
        XCTAssertEqual(loaded.entries[0].fileSize, 6)
        XCTAssertEqual(loaded.entries[0].flags & 0xFFF, 9) // "hello.txt".count
    }

    func testReadWriteMultipleEntriesSorted() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_index_multi"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

        var index = Index()
        index.add(IndexEntry(mode: 0o100644, oid: oid, path: "z.txt"))
        index.add(IndexEntry(mode: 0o100644, oid: oid, path: "a.txt"))
        index.add(IndexEntry(mode: 0o100644, oid: oid, path: "lib/main.c"))
        try writeIndex(gitDir: repo.gitDir, index: index)

        let loaded = try readIndex(gitDir: repo.gitDir)
        XCTAssertEqual(loaded.entries.count, 3)
        XCTAssertEqual(loaded.entries[0].path, "a.txt")
        XCTAssertEqual(loaded.entries[1].path, "lib/main.c")
        XCTAssertEqual(loaded.entries[2].path, "z.txt")
    }

    func testIndexAddRemoveFind() {
        let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        var index = Index()
        index.add(IndexEntry(mode: 0o100644, oid: oid, path: "foo.txt"))
        index.add(IndexEntry(mode: 0o100644, oid: oid, path: "bar.txt"))

        XCTAssertNotNil(index.find(path: "foo.txt"))
        XCTAssertNil(index.find(path: "nonexistent"))

        XCTAssertTrue(index.remove(path: "foo.txt"))
        XCTAssertFalse(index.remove(path: "foo.txt"))
        XCTAssertNil(index.find(path: "foo.txt"))
        XCTAssertEqual(index.entries.count, 1)
    }

    func testIndexChecksumValidation() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_index_checksum"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        var index = Index()
        index.add(IndexEntry(mode: 0o100644, fileSize: 10, oid: oid, path: "test.txt"))
        try writeIndex(gitDir: repo.gitDir, index: index)

        // Corrupt the data
        let indexPath = (repo.gitDir as NSString).appendingPathComponent("index")
        var data = Array(try Data(contentsOf: URL(fileURLWithPath: indexPath)))
        data[20] ^= 0xFF
        try Data(data).write(to: URL(fileURLWithPath: indexPath))

        XCTAssertThrowsError(try readIndex(gitDir: repo.gitDir))
    }

    // MARK: - Diff Tests

    private func treeEntry(_ name: String, _ hex: String, _ mode: UInt32) -> TreeEntry {
        TreeEntry(mode: mode, name: name, oid: OID(hex: hex))
    }

    func testDiffIdenticalTrees() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let entries = [treeEntry("a.txt", oid, FileMode.blob.rawValue), treeEntry("b.txt", oid, FileMode.blob.rawValue)]
        let deltas = diffTrees(oldEntries: entries, newEntries: entries)
        XCTAssertTrue(deltas.isEmpty)
    }

    func testDiffAddedFile() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let old = [treeEntry("a.txt", oid, FileMode.blob.rawValue)]
        let new = [treeEntry("a.txt", oid, FileMode.blob.rawValue), treeEntry("b.txt", oid, FileMode.blob.rawValue)]
        let deltas = diffTrees(oldEntries: old, newEntries: new)
        XCTAssertEqual(deltas.count, 1)
        XCTAssertEqual(deltas[0].status, .added)
        XCTAssertEqual(deltas[0].path, "b.txt")
        XCTAssertNil(deltas[0].oldEntry)
        XCTAssertNotNil(deltas[0].newEntry)
    }

    func testDiffDeletedFile() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let old = [treeEntry("a.txt", oid, FileMode.blob.rawValue), treeEntry("b.txt", oid, FileMode.blob.rawValue)]
        let new = [treeEntry("a.txt", oid, FileMode.blob.rawValue)]
        let deltas = diffTrees(oldEntries: old, newEntries: new)
        XCTAssertEqual(deltas.count, 1)
        XCTAssertEqual(deltas[0].status, .deleted)
        XCTAssertEqual(deltas[0].path, "b.txt")
        XCTAssertNotNil(deltas[0].oldEntry)
        XCTAssertNil(deltas[0].newEntry)
    }

    func testDiffModifiedFile() {
        let oid1 = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let oid2 = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let old = [treeEntry("a.txt", oid1, FileMode.blob.rawValue)]
        let new = [treeEntry("a.txt", oid2, FileMode.blob.rawValue)]
        let deltas = diffTrees(oldEntries: old, newEntries: new)
        XCTAssertEqual(deltas.count, 1)
        XCTAssertEqual(deltas[0].status, .modified)
        XCTAssertEqual(deltas[0].path, "a.txt")
        XCTAssertNotNil(deltas[0].oldEntry)
        XCTAssertNotNil(deltas[0].newEntry)
    }

    func testDiffModeChange() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let old = [treeEntry("script.sh", oid, FileMode.blob.rawValue)]
        let new = [treeEntry("script.sh", oid, FileMode.blobExe.rawValue)]
        let deltas = diffTrees(oldEntries: old, newEntries: new)
        XCTAssertEqual(deltas.count, 1)
        XCTAssertEqual(deltas[0].status, .modified)
    }

    func testDiffEmptyToFull() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let new = [treeEntry("a.txt", oid, FileMode.blob.rawValue), treeEntry("b.txt", oid, FileMode.blob.rawValue)]
        let deltas = diffTrees(oldEntries: [], newEntries: new)
        XCTAssertEqual(deltas.count, 2)
        XCTAssertTrue(deltas.allSatisfy { $0.status == .added })
    }

    func testDiffFullToEmpty() {
        let oid = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let old = [treeEntry("a.txt", oid, FileMode.blob.rawValue), treeEntry("b.txt", oid, FileMode.blob.rawValue)]
        let deltas = diffTrees(oldEntries: old, newEntries: [])
        XCTAssertEqual(deltas.count, 2)
        XCTAssertTrue(deltas.allSatisfy { $0.status == .deleted })
    }

    func testDiffMixedChanges() {
        let oid1 = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let oid2 = "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let old = [
            treeEntry("a.txt", oid1, FileMode.blob.rawValue),
            treeEntry("b.txt", oid1, FileMode.blob.rawValue),
            treeEntry("c.txt", oid1, FileMode.blob.rawValue),
        ]
        let new = [
            treeEntry("a.txt", oid1, FileMode.blob.rawValue), // unchanged
            treeEntry("b.txt", oid2, FileMode.blob.rawValue), // modified
            treeEntry("d.txt", oid1, FileMode.blob.rawValue), // added
        ]
        let deltas = diffTrees(oldEntries: old, newEntries: new)
        XCTAssertEqual(deltas.count, 3)
        XCTAssertEqual(deltas[0].status, .modified)
        XCTAssertEqual(deltas[0].path, "b.txt")
        XCTAssertEqual(deltas[1].status, .deleted)
        XCTAssertEqual(deltas[1].path, "c.txt")
        XCTAssertEqual(deltas[2].status, .added)
        XCTAssertEqual(deltas[2].path, "d.txt")
    }

    // MARK: - Status Tests

    private func makeIndexEntry(path: String, oid: OID, fileSize: UInt32) -> IndexEntry {
        IndexEntry(mode: 0o100644, fileSize: fileSize, oid: oid, path: path)
    }

    func testStatusCleanWorkdir() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_status_clean"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let content = Array("hello\n".utf8)
        let filePath = (repo.workdir! as NSString).appendingPathComponent("hello.txt")
        try Data(content).write(to: URL(fileURLWithPath: filePath))

        let oid = OID.hash(type: .blob, data: content)
        var index = Index()
        index.add(makeIndexEntry(path: "hello.txt", oid: oid, fileSize: UInt32(content.count)))
        try writeIndex(gitDir: repo.gitDir, index: index)

        let status = try workdirStatus(gitDir: repo.gitDir, workdir: repo.workdir!)
        XCTAssertTrue(status.isEmpty)
    }

    func testStatusModifiedFile() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_status_modified"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let content = Array("hello\n".utf8)
        let filePath = (repo.workdir! as NSString).appendingPathComponent("hello.txt")
        try Data(content).write(to: URL(fileURLWithPath: filePath))

        let oid = OID.hash(type: .blob, data: content)
        var index = Index()
        index.add(makeIndexEntry(path: "hello.txt", oid: oid, fileSize: UInt32(content.count)))
        try writeIndex(gitDir: repo.gitDir, index: index)

        // Modify the file
        try Data("changed\n".utf8).write(to: URL(fileURLWithPath: filePath))

        let status = try workdirStatus(gitDir: repo.gitDir, workdir: repo.workdir!)
        XCTAssertEqual(status.count, 1)
        XCTAssertEqual(status[0].path, "hello.txt")
        XCTAssertEqual(status[0].status, .modified)
    }

    func testStatusDeletedFile() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_status_deleted"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let content = Array("hello\n".utf8)
        let oid = OID.hash(type: .blob, data: content)
        var index = Index()
        index.add(makeIndexEntry(path: "hello.txt", oid: oid, fileSize: UInt32(content.count)))
        try writeIndex(gitDir: repo.gitDir, index: index)

        // Don't create the file
        let status = try workdirStatus(gitDir: repo.gitDir, workdir: repo.workdir!)
        XCTAssertEqual(status.count, 1)
        XCTAssertEqual(status[0].path, "hello.txt")
        XCTAssertEqual(status[0].status, .deleted)
    }

    func testStatusNewFile() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_status_new"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let index = Index()
        try writeIndex(gitDir: repo.gitDir, index: index)

        let filePath = (repo.workdir! as NSString).appendingPathComponent("new.txt")
        try Data("new\n".utf8).write(to: URL(fileURLWithPath: filePath))

        let status = try workdirStatus(gitDir: repo.gitDir, workdir: repo.workdir!)
        XCTAssertEqual(status.count, 1)
        XCTAssertEqual(status[0].path, "new.txt")
        XCTAssertEqual(status[0].status, .new)
    }

    func testStatusMixed() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_status_mixed"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let contentA = Array("aaa\n".utf8)
        let contentB = Array("bbb\n".utf8)
        let oidA = OID.hash(type: .blob, data: contentA)
        let oidB = OID.hash(type: .blob, data: contentB)

        var index = Index()
        index.add(makeIndexEntry(path: "a.txt", oid: oidA, fileSize: UInt32(contentA.count)))
        index.add(makeIndexEntry(path: "b.txt", oid: oidB, fileSize: UInt32(contentB.count)))
        index.add(makeIndexEntry(path: "c.txt", oid: oidA, fileSize: UInt32(contentA.count)))
        try writeIndex(gitDir: repo.gitDir, index: index)

        // a.txt: unchanged
        try Data(contentA).write(to: URL(fileURLWithPath: (repo.workdir! as NSString).appendingPathComponent("a.txt")))
        // b.txt: modified
        try Data("modified\n".utf8).write(to: URL(fileURLWithPath: (repo.workdir! as NSString).appendingPathComponent("b.txt")))
        // c.txt: deleted (not created)
        // d.txt: new
        try Data("new\n".utf8).write(to: URL(fileURLWithPath: (repo.workdir! as NSString).appendingPathComponent("d.txt")))

        let status = try workdirStatus(gitDir: repo.gitDir, workdir: repo.workdir!)

        let modified = status.filter { $0.status == .modified }
        let deleted = status.filter { $0.status == .deleted }
        let new = status.filter { $0.status == .new }

        XCTAssertEqual(modified.count, 1)
        XCTAssertEqual(modified[0].path, "b.txt")
        XCTAssertEqual(deleted.count, 1)
        XCTAssertEqual(deleted[0].path, "c.txt")
        XCTAssertEqual(new.count, 1)
        XCTAssertEqual(new[0].path, "d.txt")
    }

    // MARK: - Pack Index Tests

    private func sortedTestOids() -> ([OID], [UInt32], [UInt64]) {
        var oids = [
            OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"),
            OID(hex: "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"),
            OID(hex: "ccf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"),
        ]
        oids.sort { compareOidBytes($0.raw, $1.raw) }
        let crcs: [UInt32] = [0x12345678, 0x23456789, 0x3456789A]
        let offsets: [UInt64] = [12, 256, 1024]
        return (oids, crcs, offsets)
    }

    private func compareOidBytes(_ a: [UInt8], _ b: [UInt8]) -> Bool {
        for i in 0..<min(a.count, b.count) {
            if a[i] < b[i] { return true }
            if a[i] > b[i] { return false }
        }
        return a.count < b.count
    }

    func testParsePackIndex() throws {
        let (oids, crcs, offsets) = sortedTestOids()
        let data = buildPackIndex(oids: oids, crcs: crcs, offsets: offsets)
        let idx = try parsePackIndex(data)

        XCTAssertEqual(idx.count, 3)
        XCTAssertEqual(idx.oids.count, 3)
        XCTAssertEqual(idx.crcs.count, 3)
        XCTAssertEqual(idx.offsets.count, 3)
    }

    func testPackIndexFind() throws {
        let (oids, crcs, offsets) = sortedTestOids()
        let data = buildPackIndex(oids: oids, crcs: crcs, offsets: offsets)
        let idx = try parsePackIndex(data)

        XCTAssertEqual(idx.find(oids[0]), offsets[0])
        XCTAssertEqual(idx.find(oids[1]), offsets[1])
        XCTAssertEqual(idx.find(oids[2]), offsets[2])

        let missing = OID(hex: "ddf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        XCTAssertNil(idx.find(missing))
    }

    func testPackIndexContains() throws {
        let (oids, crcs, offsets) = sortedTestOids()
        let data = buildPackIndex(oids: oids, crcs: crcs, offsets: offsets)
        let idx = try parsePackIndex(data)

        XCTAssertTrue(idx.contains(oids[0]))
        XCTAssertTrue(idx.contains(oids[1]))

        let missing = OID(hex: "0000000000000000000000000000000000000001")
        XCTAssertFalse(idx.contains(missing))
    }

    func testPackIndexFanout() throws {
        let (oids, crcs, offsets) = sortedTestOids()
        let data = buildPackIndex(oids: oids, crcs: crcs, offsets: offsets)
        let idx = try parsePackIndex(data)

        XCTAssertEqual(idx.fanout[0xa9], 0)
        XCTAssertEqual(idx.fanout[0xaa], 1)
        XCTAssertEqual(idx.fanout[0xbb], 2)
        XCTAssertEqual(idx.fanout[0xcc], 3)
        XCTAssertEqual(idx.fanout[255], 3)
    }

    func testPackIndexEmpty() throws {
        let data = buildPackIndex(oids: [], crcs: [], offsets: [])
        let idx = try parsePackIndex(data)
        XCTAssertEqual(idx.count, 0)
        XCTAssertTrue(idx.oids.isEmpty)
    }

    func testPackIndexBadMagic() {
        var data = buildPackIndex(oids: [], crcs: [], offsets: [])
        data[0] = 0x00
        XCTAssertThrowsError(try parsePackIndex(data))
    }
}
