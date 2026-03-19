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
        XCTAssertEqual(MuonGitVersion.string, "0.9.0")
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

    // MARK: - Diff Formatting Tests

    func testDiffLinesIdentical() {
        let edits = diffLines(oldText: "a\nb\nc\n", newText: "a\nb\nc\n")
        XCTAssert(edits.allSatisfy { $0.kind == .equal })
    }

    func testDiffLinesInsert() {
        let edits = diffLines(oldText: "a\nc\n", newText: "a\nb\nc\n")
        let inserts = edits.filter { $0.kind == .insert }
        XCTAssertEqual(inserts.count, 1)
        XCTAssertEqual(inserts[0].text, "b")
    }

    func testDiffLinesDelete() {
        let edits = diffLines(oldText: "a\nb\nc\n", newText: "a\nc\n")
        let deletes = edits.filter { $0.kind == .delete }
        XCTAssertEqual(deletes.count, 1)
        XCTAssertEqual(deletes[0].text, "b")
    }

    func testDiffLinesModify() {
        let edits = diffLines(oldText: "a\nb\nc\n", newText: "a\nB\nc\n")
        let deletes = edits.filter { $0.kind == .delete }
        let inserts = edits.filter { $0.kind == .insert }
        XCTAssertEqual(deletes.count, 1)
        XCTAssertEqual(deletes[0].text, "b")
        XCTAssertEqual(inserts.count, 1)
        XCTAssertEqual(inserts[0].text, "B")
    }

    func testFormatPatchBasic() {
        let old = "line1\nline2\nline3\n"
        let new = "line1\nmodified\nline3\n"
        let patch = formatPatch(oldPath: "file.txt", newPath: "file.txt", oldText: old, newText: new)
        XCTAssert(patch.contains("--- a/file.txt"))
        XCTAssert(patch.contains("+++ b/file.txt"))
        XCTAssert(patch.contains("@@"))
        XCTAssert(patch.contains("-line2"))
        XCTAssert(patch.contains("+modified"))
    }

    func testFormatPatchNoChanges() {
        let text = "same\n"
        let patch = formatPatch(oldPath: "f.txt", newPath: "f.txt", oldText: text, newText: text)
        XCTAssert(patch.isEmpty)
    }

    func testFormatPatchAddedFile() {
        let patch = formatPatch(oldPath: "new.txt", newPath: "new.txt", oldText: "", newText: "hello\nworld\n")
        XCTAssert(patch.contains("+hello"))
        XCTAssert(patch.contains("+world"))
    }

    func testFormatPatchDeletedFile() {
        let patch = formatPatch(oldPath: "old.txt", newPath: "old.txt", oldText: "goodbye\nworld\n", newText: "")
        XCTAssert(patch.contains("-goodbye"))
        XCTAssert(patch.contains("-world"))
    }

    func testDiffStatBasic() {
        let stat = diffStat(path: "file.txt", oldText: "a\nb\nc\n", newText: "a\nB\nc\nd\n")
        XCTAssertEqual(stat.path, "file.txt")
        XCTAssertEqual(stat.deletions, 1)
        XCTAssertEqual(stat.insertions, 2)
    }

    func testFormatStatOutput() {
        let stats = [
            DiffStatEntry(path: "file.txt", insertions: 3, deletions: 1),
            DiffStatEntry(path: "other.rs", insertions: 0, deletions: 5),
        ]
        let output = formatStat(stats: stats)
        XCTAssert(output.contains("file.txt"))
        XCTAssert(output.contains("other.rs"))
        XCTAssert(output.contains("2 files changed"))
        XCTAssert(output.contains("3 insertions(+)"))
        XCTAssert(output.contains("6 deletions(-)"))
    }

    func testFormatStatEmpty() {
        let output = formatStat(stats: [])
        XCTAssert(output.isEmpty)
    }

    // MARK: - Index-to-Workdir Diff Tests

    func testDiffWorkdirClean() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_diff_workdir_clean"
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

        let deltas = try diffIndexToWorkdir(gitDir: repo.gitDir, workdir: repo.workdir!)
        XCTAssert(deltas.isEmpty)
    }

    func testDiffWorkdirModified() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_diff_workdir_mod"
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

        let deltas = try diffIndexToWorkdir(gitDir: repo.gitDir, workdir: repo.workdir!)
        XCTAssertEqual(deltas.count, 1)
        XCTAssertEqual(deltas[0].status, .modified)
        XCTAssertEqual(deltas[0].path, "hello.txt")
        XCTAssertNotNil(deltas[0].oldEntry)
        XCTAssertNotNil(deltas[0].newEntry)
        XCTAssertNotEqual(deltas[0].oldEntry!.oid, deltas[0].newEntry!.oid)
    }

    func testDiffWorkdirDeleted() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_diff_workdir_del"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let content = Array("hello\n".utf8)
        let oid = OID.hash(type: .blob, data: content)
        var index = Index()
        index.add(makeIndexEntry(path: "hello.txt", oid: oid, fileSize: UInt32(content.count)))
        try writeIndex(gitDir: repo.gitDir, index: index)

        // Don't create the file — it's deleted
        let deltas = try diffIndexToWorkdir(gitDir: repo.gitDir, workdir: repo.workdir!)
        XCTAssertEqual(deltas.count, 1)
        XCTAssertEqual(deltas[0].status, .deleted)
        XCTAssertEqual(deltas[0].path, "hello.txt")
        XCTAssertNotNil(deltas[0].oldEntry)
        XCTAssertNil(deltas[0].newEntry)
    }

    func testDiffWorkdirNewFile() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_diff_workdir_new"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let index = Index()
        try writeIndex(gitDir: repo.gitDir, index: index)

        // Create a file not in the index
        try Data("new\n".utf8).write(to: URL(fileURLWithPath: (repo.workdir! as NSString).appendingPathComponent("new.txt")))

        let deltas = try diffIndexToWorkdir(gitDir: repo.gitDir, workdir: repo.workdir!)
        XCTAssertEqual(deltas.count, 1)
        XCTAssertEqual(deltas[0].status, .added)
        XCTAssertEqual(deltas[0].path, "new.txt")
        XCTAssertNil(deltas[0].oldEntry)
        XCTAssertNotNil(deltas[0].newEntry)
    }

    func testDiffWorkdirMixed() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_diff_workdir_mixed"
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

        let wd = repo.workdir!
        // a.txt: unchanged
        try Data(contentA).write(to: URL(fileURLWithPath: (wd as NSString).appendingPathComponent("a.txt")))
        // b.txt: modified
        try Data("modified\n".utf8).write(to: URL(fileURLWithPath: (wd as NSString).appendingPathComponent("b.txt")))
        // c.txt: deleted (not created)
        // d.txt: new
        try Data("new\n".utf8).write(to: URL(fileURLWithPath: (wd as NSString).appendingPathComponent("d.txt")))

        let deltas = try diffIndexToWorkdir(gitDir: repo.gitDir, workdir: wd)

        let modified = deltas.filter { $0.status == .modified }
        let deleted = deltas.filter { $0.status == .deleted }
        let added = deltas.filter { $0.status == .added }

        XCTAssertEqual(modified.count, 1)
        XCTAssertEqual(modified[0].path, "b.txt")
        XCTAssertEqual(deleted.count, 1)
        XCTAssertEqual(deleted[0].path, "c.txt")
        XCTAssertEqual(added.count, 1)
        XCTAssertEqual(added[0].path, "d.txt")
    }

    // MARK: - Ignore / Glob Tests

    func testGlobMatchBasic() {
        XCTAssertTrue(globMatch("*.txt", "hello.txt"))
        XCTAssertFalse(globMatch("*.txt", "hello.rs"))
        XCTAssertTrue(globMatch("hello.*", "hello.txt"))
        XCTAssertTrue(globMatch("?ello.txt", "hello.txt"))
        XCTAssertFalse(globMatch("?ello.txt", "hhello.txt"))
    }

    func testGlobMatchStarNoSlash() {
        XCTAssertFalse(globMatch("*.txt", "dir/hello.txt"))
        XCTAssertTrue(globMatch("*.txt", "hello.txt"))
    }

    func testGlobMatchDoubleStar() {
        XCTAssertTrue(globMatch("**/*.txt", "hello.txt"))
        XCTAssertTrue(globMatch("**/*.txt", "dir/hello.txt"))
        XCTAssertTrue(globMatch("**/*.txt", "a/b/c/hello.txt"))
        XCTAssertTrue(globMatch("**/build", "build"))
        XCTAssertTrue(globMatch("**/build", "src/build"))
    }

    func testGlobMatchCharClass() {
        XCTAssertTrue(globMatch("[abc].txt", "a.txt"))
        XCTAssertTrue(globMatch("[abc].txt", "b.txt"))
        XCTAssertFalse(globMatch("[abc].txt", "d.txt"))
        XCTAssertTrue(globMatch("[a-z].txt", "m.txt"))
        XCTAssertFalse(globMatch("[a-z].txt", "M.txt"))
        XCTAssertTrue(globMatch("[!abc].txt", "d.txt"))
        XCTAssertFalse(globMatch("[!abc].txt", "a.txt"))
    }

    func testIgnoreBasic() {
        var ignore = Ignore()
        ignore.addPatterns("*.o\n*.log\nbuild/\n", baseDir: "")

        XCTAssertTrue(ignore.isIgnored("main.o", isDir: false))
        XCTAssertTrue(ignore.isIgnored("debug.log", isDir: false))
        XCTAssertTrue(ignore.isIgnored("src/test.o", isDir: false))
        XCTAssertFalse(ignore.isIgnored("main.c", isDir: false))
        XCTAssertTrue(ignore.isIgnored("build", isDir: true))
        XCTAssertFalse(ignore.isIgnored("build", isDir: false))
    }

    func testIgnoreNegation() {
        var ignore = Ignore()
        ignore.addPatterns("*.log\n!important.log\n", baseDir: "")

        XCTAssertTrue(ignore.isIgnored("debug.log", isDir: false))
        XCTAssertFalse(ignore.isIgnored("important.log", isDir: false))
    }

    func testIgnoreDoubleStar() {
        var ignore = Ignore()
        ignore.addPatterns("**/build\nlogs/**/*.log\n", baseDir: "")

        XCTAssertTrue(ignore.isIgnored("build", isDir: false))
        XCTAssertTrue(ignore.isIgnored("src/build", isDir: false))
        XCTAssertTrue(ignore.isIgnored("logs/2024/error.log", isDir: false))
    }

    func testIgnoreWithPath() {
        var ignore = Ignore()
        ignore.addPatterns("doc/*.html\n", baseDir: "")

        XCTAssertTrue(ignore.isIgnored("doc/index.html", isDir: false))
        XCTAssertFalse(ignore.isIgnored("src/index.html", isDir: false))
    }

    func testIgnoreLoadFromRepo() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_ignore_load"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let workdir = repo.workdir!

        try "*.o\nbuild/\n".write(toFile: (workdir as NSString).appendingPathComponent(".gitignore"), atomically: true, encoding: .utf8)

        let ignore = Ignore.load(gitDir: repo.gitDir, workdir: workdir)
        XCTAssertTrue(ignore.isIgnored("main.o", isDir: false))
        XCTAssertTrue(ignore.isIgnored("build", isDir: true))
        XCTAssertFalse(ignore.isIgnored("main.c", isDir: false))
    }

    func testIgnoreSubdir() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_ignore_subdir"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let workdir = repo.workdir!

        try "*.o\n".write(toFile: (workdir as NSString).appendingPathComponent(".gitignore"), atomically: true, encoding: .utf8)
        let vendorDir = (workdir as NSString).appendingPathComponent("vendor")
        try FileManager.default.createDirectory(atPath: vendorDir, withIntermediateDirectories: true)
        try "*.tmp\n".write(toFile: (vendorDir as NSString).appendingPathComponent(".gitignore"), atomically: true, encoding: .utf8)

        var ignore = Ignore.load(gitDir: repo.gitDir, workdir: workdir)
        ignore.loadForPath(workdir: workdir, relDir: "vendor")

        XCTAssertTrue(ignore.isIgnored("main.o", isDir: false))
        XCTAssertTrue(ignore.isIgnored("vendor/cache.tmp", isDir: false))
        XCTAssertFalse(ignore.isIgnored("src/cache.tmp", isDir: false))
    }

    // MARK: - Merge Base Tests

    private func makeCommit(gitDir: String, treeOid: OID, parents: [OID], msg: String) throws -> OID {
        var data = "tree \(treeOid.hex)\n"
        for p in parents {
            data += "parent \(p.hex)\n"
        }
        data += "author Test <test@test.com> 1000000000 +0000\n"
        data += "committer Test <test@test.com> 1000000000 +0000\n"
        data += "\n\(msg)"
        return try writeLooseObject(gitDir: gitDir, type: .commit, data: Data(data.utf8))
    }

    private func makeEmptyTree(gitDir: String) throws -> OID {
        return try writeLooseObject(gitDir: gitDir, type: .tree, data: Data())
    }

    func testMergeBaseSameCommit() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_mb_same"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let tree = try makeEmptyTree(gitDir: repo.gitDir)
        let c1 = try makeCommit(gitDir: repo.gitDir, treeOid: tree, parents: [], msg: "initial")

        let result = try mergeBase(gitDir: repo.gitDir, oid1: c1, oid2: c1)
        XCTAssertEqual(result, c1)
    }

    func testMergeBaseLinearHistory() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_mb_linear"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let tree = try makeEmptyTree(gitDir: repo.gitDir)
        let a = try makeCommit(gitDir: repo.gitDir, treeOid: tree, parents: [], msg: "A")
        let b = try makeCommit(gitDir: repo.gitDir, treeOid: tree, parents: [a], msg: "B")
        let c = try makeCommit(gitDir: repo.gitDir, treeOid: tree, parents: [b], msg: "C")

        XCTAssertEqual(try mergeBase(gitDir: repo.gitDir, oid1: b, oid2: c), b)
        XCTAssertEqual(try mergeBase(gitDir: repo.gitDir, oid1: a, oid2: c), a)
        XCTAssertEqual(try mergeBase(gitDir: repo.gitDir, oid1: a, oid2: b), a)
    }

    func testMergeBaseForkAndMerge() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_mb_fork"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let tree = try makeEmptyTree(gitDir: repo.gitDir)
        let a = try makeCommit(gitDir: repo.gitDir, treeOid: tree, parents: [], msg: "A")
        let b = try makeCommit(gitDir: repo.gitDir, treeOid: tree, parents: [a], msg: "B")
        let c = try makeCommit(gitDir: repo.gitDir, treeOid: tree, parents: [a], msg: "C")
        let d = try makeCommit(gitDir: repo.gitDir, treeOid: tree, parents: [b, c], msg: "D")

        XCTAssertEqual(try mergeBase(gitDir: repo.gitDir, oid1: b, oid2: c), a)
        XCTAssertEqual(try mergeBase(gitDir: repo.gitDir, oid1: b, oid2: d), b)
    }

    func testMergeBaseNoCommonAncestor() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_mb_disjoint"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let tree = try makeEmptyTree(gitDir: repo.gitDir)
        let a = try makeCommit(gitDir: repo.gitDir, treeOid: tree, parents: [], msg: "A")
        let b = try makeCommit(gitDir: repo.gitDir, treeOid: tree, parents: [a], msg: "B")
        let c = try makeCommit(gitDir: repo.gitDir, treeOid: tree, parents: [], msg: "C")
        let d = try makeCommit(gitDir: repo.gitDir, treeOid: tree, parents: [c], msg: "D")

        XCTAssertNil(try mergeBase(gitDir: repo.gitDir, oid1: b, oid2: d))
    }

    func testMergeBasesMultiple() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_mb_multi"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let tree = try makeEmptyTree(gitDir: repo.gitDir)
        let a = try makeCommit(gitDir: repo.gitDir, treeOid: tree, parents: [], msg: "A")
        let b = try makeCommit(gitDir: repo.gitDir, treeOid: tree, parents: [a], msg: "B")
        let c = try makeCommit(gitDir: repo.gitDir, treeOid: tree, parents: [a], msg: "C")

        let bases = try mergeBases(gitDir: repo.gitDir, oid1: b, oid2: c)
        XCTAssertEqual(bases.count, 1)
        XCTAssertEqual(bases[0], a)
    }

    // MARK: - Checkout Tests

    private func addBlobToIndex(gitDir: String, index: inout Index, path: String, content: Data, executable: Bool) throws {
        let oid = try writeLooseObject(gitDir: gitDir, type: .blob, data: content)
        let mode: UInt32 = executable ? 0o100755 : 0o100644
        index.add(IndexEntry(
            ctimeSecs: 0, ctimeNanos: 0, mtimeSecs: 0, mtimeNanos: 0,
            dev: 0, ino: 0, mode: mode, uid: 0, gid: 0,
            fileSize: UInt32(content.count), oid: oid,
            flags: UInt16(min(path.count, 0xFFF)), path: path
        ))
    }

    func testCheckoutBasic() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_checkout_basic"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let workdir = repo.workdir!
        var index = Index()
        try addBlobToIndex(gitDir: repo.gitDir, index: &index, path: "hello.txt", content: Data("Hello, world!\n".utf8), executable: false)
        try addBlobToIndex(gitDir: repo.gitDir, index: &index, path: "src/main.rs", content: Data("fn main() {}\n".utf8), executable: false)
        try writeIndex(gitDir: repo.gitDir, index: index)

        let result = try checkoutIndex(gitDir: repo.gitDir, workdir: workdir, options: CheckoutOptions(force: true))
        XCTAssertEqual(result.updated.count, 2)
        XCTAssertTrue(result.conflicts.isEmpty)
        XCTAssertEqual(try String(contentsOfFile: (workdir as NSString).appendingPathComponent("hello.txt"), encoding: .utf8), "Hello, world!\n")
        XCTAssertEqual(try String(contentsOfFile: (workdir as NSString).appendingPathComponent("src/main.rs"), encoding: .utf8), "fn main() {}\n")
    }

    func testCheckoutCreatesDirectories() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_checkout_dirs"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let workdir = repo.workdir!
        var index = Index()
        try addBlobToIndex(gitDir: repo.gitDir, index: &index, path: "a/b/c/deep.txt", content: Data("deep content".utf8), executable: false)
        try writeIndex(gitDir: repo.gitDir, index: index)

        let result = try checkoutIndex(gitDir: repo.gitDir, workdir: workdir, options: CheckoutOptions(force: true))
        XCTAssertEqual(result.updated.count, 1)
        XCTAssertTrue(FileManager.default.fileExists(atPath: (workdir as NSString).appendingPathComponent("a/b/c/deep.txt")))
    }

    func testCheckoutConflictDetection() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_checkout_conflict"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let workdir = repo.workdir!

        try "local changes".write(toFile: (workdir as NSString).appendingPathComponent("existing.txt"), atomically: true, encoding: .utf8)

        var index = Index()
        try addBlobToIndex(gitDir: repo.gitDir, index: &index, path: "existing.txt", content: Data("index content".utf8), executable: false)
        try writeIndex(gitDir: repo.gitDir, index: index)

        let result1 = try checkoutIndex(gitDir: repo.gitDir, workdir: workdir, options: CheckoutOptions(force: false))
        XCTAssertTrue(result1.updated.isEmpty)
        XCTAssertEqual(result1.conflicts.count, 1)
        XCTAssertEqual(try String(contentsOfFile: (workdir as NSString).appendingPathComponent("existing.txt"), encoding: .utf8), "local changes")

        let result2 = try checkoutIndex(gitDir: repo.gitDir, workdir: workdir, options: CheckoutOptions(force: true))
        XCTAssertEqual(result2.updated.count, 1)
        XCTAssertEqual(try String(contentsOfFile: (workdir as NSString).appendingPathComponent("existing.txt"), encoding: .utf8), "index content")
    }

    func testCheckoutExecutableMode() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_checkout_exec"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let workdir = repo.workdir!
        var index = Index()
        try addBlobToIndex(gitDir: repo.gitDir, index: &index, path: "script.sh", content: Data("#!/bin/sh\necho hi\n".utf8), executable: true)
        try writeIndex(gitDir: repo.gitDir, index: index)

        _ = try checkoutIndex(gitDir: repo.gitDir, workdir: workdir, options: CheckoutOptions(force: true))
        let attrs = try FileManager.default.attributesOfItem(atPath: (workdir as NSString).appendingPathComponent("script.sh"))
        let perms = (attrs[.posixPermissions] as! Int)
        XCTAssertTrue(perms & 0o111 != 0, "file should be executable")
    }

    func testCheckoutPaths() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_checkout_paths"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let workdir = repo.workdir!
        var index = Index()
        try addBlobToIndex(gitDir: repo.gitDir, index: &index, path: "a.txt", content: Data("aaa".utf8), executable: false)
        try addBlobToIndex(gitDir: repo.gitDir, index: &index, path: "b.txt", content: Data("bbb".utf8), executable: false)
        try addBlobToIndex(gitDir: repo.gitDir, index: &index, path: "c.txt", content: Data("ccc".utf8), executable: false)
        try writeIndex(gitDir: repo.gitDir, index: index)

        let result = try checkoutPaths(gitDir: repo.gitDir, workdir: workdir, paths: ["a.txt", "c.txt"], options: CheckoutOptions(force: true))
        XCTAssertEqual(result.updated.count, 2)
        XCTAssertTrue(FileManager.default.fileExists(atPath: (workdir as NSString).appendingPathComponent("a.txt")))
        XCTAssertFalse(FileManager.default.fileExists(atPath: (workdir as NSString).appendingPathComponent("b.txt")))
        XCTAssertTrue(FileManager.default.fileExists(atPath: (workdir as NSString).appendingPathComponent("c.txt")))
    }

    func testCheckoutPathNotInIndex() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_checkout_notfound"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let workdir = repo.workdir!
        let index = Index()
        try writeIndex(gitDir: repo.gitDir, index: index)

        XCTAssertThrowsError(try checkoutPaths(gitDir: repo.gitDir, workdir: workdir, paths: ["nonexistent.txt"], options: CheckoutOptions(force: true)))
    }

    // MARK: - Remote Tests

    func testAddAndGetRemote() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_remote_add"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let remote = try addRemote(gitDir: repo.gitDir, name: "origin", url: "https://example.com/repo.git")

        XCTAssertEqual(remote.name, "origin")
        XCTAssertEqual(remote.url, "https://example.com/repo.git")
        XCTAssertEqual(remote.fetchRefspecs.count, 1)
        XCTAssertEqual(remote.fetchRefspecs[0], "+refs/heads/*:refs/remotes/origin/*")

        let loaded = try getRemote(gitDir: repo.gitDir, name: "origin")
        XCTAssertEqual(loaded.url, "https://example.com/repo.git")
    }

    func testListRemotes() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_remote_list"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        try addRemote(gitDir: repo.gitDir, name: "origin", url: "https://example.com/repo.git")
        try addRemote(gitDir: repo.gitDir, name: "upstream", url: "https://example.com/upstream.git")

        let names = try listRemotes(gitDir: repo.gitDir)
        XCTAssertTrue(names.contains("origin"))
        XCTAssertTrue(names.contains("upstream"))
        XCTAssertEqual(names.count, 2)
    }

    func testRemoveRemote() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_remote_rm"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        try addRemote(gitDir: repo.gitDir, name: "origin", url: "https://example.com/repo.git")
        try removeRemote(gitDir: repo.gitDir, name: "origin")

        XCTAssertThrowsError(try getRemote(gitDir: repo.gitDir, name: "origin"))
    }

    func testRenameRemote() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_remote_rename"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        try addRemote(gitDir: repo.gitDir, name: "origin", url: "https://example.com/repo.git")
        try renameRemote(gitDir: repo.gitDir, oldName: "origin", newName: "upstream")

        XCTAssertThrowsError(try getRemote(gitDir: repo.gitDir, name: "origin"))
        let remote = try getRemote(gitDir: repo.gitDir, name: "upstream")
        XCTAssertEqual(remote.url, "https://example.com/repo.git")
        XCTAssertEqual(remote.fetchRefspecs[0], "+refs/heads/*:refs/remotes/upstream/*")
    }

    func testAddDuplicateRemote() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_remote_dup"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        try addRemote(gitDir: repo.gitDir, name: "origin", url: "https://example.com/repo.git")
        XCTAssertThrowsError(try addRemote(gitDir: repo.gitDir, name: "origin", url: "https://other.com/repo.git"))
    }

    func testParseRefspec() {
        let result1 = parseRefspec("+refs/heads/*:refs/remotes/origin/*")!
        XCTAssertTrue(result1.force)
        XCTAssertEqual(result1.src, "refs/heads/*")
        XCTAssertEqual(result1.dst, "refs/remotes/origin/*")

        let result2 = parseRefspec("refs/heads/main:refs/heads/main")!
        XCTAssertFalse(result2.force)
        XCTAssertEqual(result2.src, "refs/heads/main")
        XCTAssertEqual(result2.dst, "refs/heads/main")

        XCTAssertNil(parseRefspec("no-colon"))
    }

    func testGetNonexistentRemote() throws {
        let tmp = NSTemporaryDirectory() + "muongit_test_remote_noexist"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        XCTAssertThrowsError(try getRemote(gitDir: repo.gitDir, name: "nope"))
    }

    // MARK: - Three-Way Merge Tests

    func testMerge3NoChanges() {
        let base = "line1\nline2\nline3"
        let result = merge3(base: base, ours: base, theirs: base)
        XCTAssertFalse(result.hasConflicts)
        XCTAssertEqual(result.toCleanString(), "line1\nline2\nline3\n")
    }

    func testMerge3OursOnly() {
        let result = merge3(base: "line1\nline2\nline3", ours: "line1\nmodified\nline3", theirs: "line1\nline2\nline3")
        XCTAssertFalse(result.hasConflicts)
        XCTAssertEqual(result.toCleanString(), "line1\nmodified\nline3\n")
    }

    func testMerge3TheirsOnly() {
        let result = merge3(base: "line1\nline2\nline3", ours: "line1\nline2\nline3", theirs: "line1\nline2\nchanged")
        XCTAssertFalse(result.hasConflicts)
        XCTAssertEqual(result.toCleanString(), "line1\nline2\nchanged\n")
    }

    func testMerge3BothDifferentRegions() {
        let result = merge3(base: "line1\nline2\nline3", ours: "changed1\nline2\nline3", theirs: "line1\nline2\nchanged3")
        XCTAssertFalse(result.hasConflicts)
        XCTAssertEqual(result.toCleanString(), "changed1\nline2\nchanged3\n")
    }

    func testMerge3SameChangeBothSides() {
        let both = "line1\nSAME\nline3"
        let result = merge3(base: "line1\nline2\nline3", ours: both, theirs: both)
        XCTAssertFalse(result.hasConflicts)
        XCTAssertEqual(result.toCleanString(), "line1\nSAME\nline3\n")
    }

    func testMerge3Conflict() {
        let result = merge3(base: "line1\nline2\nline3", ours: "line1\nours\nline3", theirs: "line1\ntheirs\nline3")
        XCTAssertTrue(result.hasConflicts)
        XCTAssertNil(result.toCleanString())
        let text = result.toStringWithMarkers()
        XCTAssertTrue(text.contains("<<<<<<< ours"))
        XCTAssertTrue(text.contains("======="))
        XCTAssertTrue(text.contains(">>>>>>> theirs"))
    }

    func testMerge3OursAddsLines() {
        let result = merge3(base: "line1\nline3", ours: "line1\nline2\nline3", theirs: "line1\nline3")
        XCTAssertFalse(result.hasConflicts)
        XCTAssertEqual(result.toCleanString(), "line1\nline2\nline3\n")
    }

    func testMerge3TheirsDeletesLines() {
        let result = merge3(base: "line1\nline2\nline3", ours: "line1\nline2\nline3", theirs: "line1\nline3")
        XCTAssertFalse(result.hasConflicts)
        XCTAssertEqual(result.toCleanString(), "line1\nline3\n")
    }

    func testMerge3EmptyBase() {
        let result = merge3(base: "", ours: "added", theirs: "")
        XCTAssertFalse(result.hasConflicts)
        XCTAssertEqual(result.toCleanString(), "added\n")
    }

    // MARK: - Transport Tests

    func testPktLineEncode() {
        let encoded = pktLineEncode("hello\n".data(using: .utf8)!)
        XCTAssertEqual(encoded, "000ahello\n".data(using: .ascii)!)
    }

    func testPktLineFlushValue() {
        XCTAssertEqual(pktLineFlush(), "0000".data(using: .ascii)!)
    }

    func testPktLineDecode() throws {
        let input = "000ahello\n0000".data(using: .ascii)!
        let result = try pktLineDecode(input).get()
        let (lines, consumed) = result
        XCTAssertEqual(consumed, 14)
        XCTAssertEqual(lines.count, 2)
        XCTAssertEqual(lines[0], PktLine.data("hello\n".data(using: .utf8)!))
        XCTAssertEqual(lines[1], PktLine.flush)
    }

    func testPktLineRoundtrip() throws {
        let data = "test data here".data(using: .utf8)!
        let encoded = pktLineEncode(data)
        let result = try pktLineDecode(encoded).get()
        XCTAssertEqual(result.0.count, 1)
        XCTAssertEqual(result.0[0], PktLine.data(data))
    }

    func testParseRefAdvertisement() throws {
        let oidHex = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        let line1 = "\(oidHex) HEAD\0multi_ack thin-pack side-band\n"
        let line2 = "\(oidHex) refs/heads/main\n"

        var input = Data()
        input.append(pktLineEncode(line1.data(using: .utf8)!))
        input.append(pktLineEncode(line2.data(using: .utf8)!))
        input.append(pktLineFlush())

        let decoded = try pktLineDecode(input).get()
        let parsed = try parseRefAdvertisement(decoded.0).get()
        let (refs, caps) = parsed

        XCTAssertEqual(refs.count, 2)
        XCTAssertEqual(refs[0].name, "HEAD")
        XCTAssertEqual(refs[1].name, "refs/heads/main")
        XCTAssertTrue(caps.has("multi_ack"))
        XCTAssertTrue(caps.has("thin-pack"))
        XCTAssertTrue(caps.has("side-band"))
        XCTAssertFalse(caps.has("ofs-delta"))
    }

    func testBuildWantHave() throws {
        let want = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let have = OID(hex: "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

        let data = buildWantHave(wants: [want], haves: [have], caps: ["multi_ack", "thin-pack"])
        let text = String(data: data, encoding: .utf8)!

        XCTAssertTrue(text.contains("want aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d multi_ack thin-pack"))
        XCTAssertTrue(text.contains("have bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"))
        XCTAssertTrue(text.contains("done"))
    }

    func testParseGitURLHttps() {
        let result = parseGitURL("https://github.com/user/repo.git")!
        XCTAssertEqual(result.scheme, "https")
        XCTAssertEqual(result.host, "github.com")
        XCTAssertEqual(result.path, "/user/repo.git")
    }

    func testParseGitURLSsh() {
        let result = parseGitURL("git@github.com:user/repo.git")!
        XCTAssertEqual(result.scheme, "ssh")
        XCTAssertEqual(result.host, "git@github.com")
        XCTAssertEqual(result.path, "user/repo.git")
    }

    func testParseGitURLSshProtocol() {
        let result = parseGitURL("ssh://git@github.com/user/repo.git")!
        XCTAssertEqual(result.scheme, "ssh")
        XCTAssertEqual(result.host, "git@github.com")
        XCTAssertEqual(result.path, "/user/repo.git")
    }

    func testServerCapabilitiesGet() {
        let caps = ServerCapabilities(capabilities: [
            "multi_ack",
            "agent=git/2.30.0",
            "symref=HEAD:refs/heads/main",
        ])
        XCTAssertTrue(caps.has("multi_ack"))
        XCTAssertTrue(caps.has("agent"))
        XCTAssertEqual(caps.get("agent"), "git/2.30.0")
        XCTAssertEqual(caps.get("symref"), "HEAD:refs/heads/main")
        XCTAssertNil(caps.get("multi_ack"))
    }

    // MARK: - Fetch/Push/Clone Tests

    func testRefspecMatchGlob() {
        XCTAssertEqual(refspecMatch("refs/heads/main", pattern: "refs/heads/*"), "main")
        XCTAssertEqual(refspecMatch("refs/heads/feature/x", pattern: "refs/heads/*"), "feature/x")
        XCTAssertNil(refspecMatch("refs/tags/v1", pattern: "refs/heads/*"))
    }

    func testRefspecMatchExact() {
        XCTAssertEqual(refspecMatch("refs/heads/main", pattern: "refs/heads/main"), "")
        XCTAssertNil(refspecMatch("refs/heads/dev", pattern: "refs/heads/main"))
    }

    func testApplyRefspecGlob() {
        let result = applyRefspec("refs/heads/main", refspec: "+refs/heads/*:refs/remotes/origin/*")
        XCTAssertEqual(result, "refs/remotes/origin/main")

        let result2 = applyRefspec("refs/heads/feature/x", refspec: "+refs/heads/*:refs/remotes/origin/*")
        XCTAssertEqual(result2, "refs/remotes/origin/feature/x")
    }

    func testApplyRefspecNoMatch() {
        let result = applyRefspec("refs/tags/v1", refspec: "+refs/heads/*:refs/remotes/origin/*")
        XCTAssertNil(result)
    }

    func testComputeFetchWants() throws {
        let tmp = NSTemporaryDirectory() + "test_fetch_wants_\(UUID().uuidString)"
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let oid1 = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let oid2 = OID(hex: "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

        let remoteRefs = [
            RemoteRef(oid: oid1, name: "refs/heads/main"),
            RemoteRef(oid: oid2, name: "refs/heads/dev"),
            RemoteRef(oid: oid1, name: "refs/tags/v1"),
        ]
        let refspecs = ["+refs/heads/*:refs/remotes/origin/*"]

        let neg = computeFetchWants(remoteRefs: remoteRefs, refspecs: refspecs, gitDir: repo.gitDir)
        XCTAssertEqual(neg.wants.count, 2)
        XCTAssertEqual(neg.matchedRefs.count, 2)
        XCTAssertEqual(neg.matchedRefs[0].localName, "refs/remotes/origin/main")
        XCTAssertEqual(neg.matchedRefs[1].localName, "refs/remotes/origin/dev")
    }

    func testComputeFetchWantsSkipsExisting() throws {
        let tmp = NSTemporaryDirectory() + "test_fetch_skip_\(UUID().uuidString)"
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        try writeReference(gitDir: repo.gitDir, name: "refs/remotes/origin/main", oid: oid)

        let remoteRefs = [RemoteRef(oid: oid, name: "refs/heads/main")]
        let refspecs = ["+refs/heads/*:refs/remotes/origin/*"]

        let neg = computeFetchWants(remoteRefs: remoteRefs, refspecs: refspecs, gitDir: repo.gitDir)
        XCTAssertEqual(neg.wants.count, 0)
        XCTAssertEqual(neg.matchedRefs.count, 1)
    }

    func testUpdateRefsFromFetch() throws {
        let tmp = NSTemporaryDirectory() + "test_fetch_update_\(UUID().uuidString)"
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let matched = [MatchedRef(remoteName: "refs/heads/main", localName: "refs/remotes/origin/main", oid: oid)]

        let count = try updateRefsFromFetch(gitDir: repo.gitDir, matchedRefs: matched)
        XCTAssertEqual(count, 1)

        let resolved = try resolveReference(gitDir: repo.gitDir, name: "refs/remotes/origin/main")
        XCTAssertEqual(resolved, oid)
    }

    func testComputePushUpdates() throws {
        let tmp = NSTemporaryDirectory() + "test_push_updates_\(UUID().uuidString)"
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let localOid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let remoteOid = OID(hex: "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

        try writeReference(gitDir: repo.gitDir, name: "refs/heads/main", oid: localOid)

        let remoteRefs = [RemoteRef(oid: remoteOid, name: "refs/heads/main")]
        let updates = try computePushUpdates(
            pushRefspecs: ["refs/heads/main:refs/heads/main"],
            gitDir: repo.gitDir,
            remoteRefs: remoteRefs
        )

        XCTAssertEqual(updates.count, 1)
        XCTAssertEqual(updates[0].srcOid, localOid)
        XCTAssertEqual(updates[0].dstOid, remoteOid)
        XCTAssertFalse(updates[0].force)
    }

    func testBuildPushReportOutput() {
        let oid1 = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let oid2 = OID(hex: "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let updates = [PushUpdate(srcRef: "refs/heads/main", dstRef: "refs/heads/main", srcOid: oid1, dstOid: oid2, force: false)]
        let report = buildPushReport(updates)
        XCTAssertTrue(report.contains(oid1.hex))
        XCTAssertTrue(report.contains(oid2.hex))
        XCTAssertTrue(report.contains("refs/heads/main"))
    }

    func testCloneSetupCreatesRepo() throws {
        let tmp = NSTemporaryDirectory() + "test_clone_setup_\(UUID().uuidString)"
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try cloneSetup(path: tmp, url: "https://example.com/repo.git")
        let remote = try getRemote(gitDir: repo.gitDir, name: "origin")
        XCTAssertEqual(remote.url, "https://example.com/repo.git")
    }

    func testCloneFinishSetsUpRefs() throws {
        let tmp = NSTemporaryDirectory() + "test_clone_finish_\(UUID().uuidString)"
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        try cloneFinish(gitDir: repo.gitDir, remoteName: "origin", defaultBranch: "main", headOid: oid)

        let resolved = try resolveReference(gitDir: repo.gitDir, name: "refs/heads/main")
        XCTAssertEqual(resolved, oid)

        let head = try String(contentsOfFile: (repo.gitDir as NSString).appendingPathComponent("HEAD"), encoding: .utf8)
        XCTAssertTrue(head.contains("refs/heads/main"))
    }

    func testDefaultBranchFromCapsValue() {
        let caps = ServerCapabilities(capabilities: ["multi_ack", "symref=HEAD:refs/heads/main"])
        XCTAssertEqual(defaultBranchFromCaps(caps), "main")

        let caps2 = ServerCapabilities(capabilities: ["multi_ack"])
        XCTAssertNil(defaultBranchFromCaps(caps2))
    }

    func testCloneSetupWithBranch() throws {
        let tmp = NSTemporaryDirectory() + "test_clone_branch_\(UUID().uuidString)"
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let opts = CloneOptions(branch: "develop")
        let repo = try cloneSetup(path: tmp, url: "https://example.com/repo.git", options: opts)
        let head = try String(contentsOfFile: (repo.gitDir as NSString).appendingPathComponent("HEAD"), encoding: .utf8)
        XCTAssertTrue(head.contains("refs/heads/develop"))
    }

    // MARK: - Attributes Tests

    func testParseSimpleAttrs() {
        let attrs = Attributes()
        attrs.parse("*.txt text\n*.bin binary\n")

        XCTAssertEqual(attrs.get("hello.txt", attr: "text"), .set)
        XCTAssertTrue(attrs.isBinary("image.bin"))
        XCTAssertFalse(attrs.isBinary("hello.txt"))
    }

    func testParseUnsetAndValue() {
        let attrs = Attributes()
        attrs.parse("*.md text eol=lf\n*.png -text -diff\n")

        XCTAssertEqual(attrs.get("README.md", attr: "text"), .set)
        XCTAssertEqual(attrs.get("README.md", attr: "eol"), .value("lf"))
        XCTAssertEqual(attrs.eol("README.md"), "lf")
        XCTAssertEqual(attrs.get("image.png", attr: "text"), .unset)
        XCTAssertTrue(attrs.isBinary("image.png"))
    }

    func testBinaryMacro() {
        let attrs = Attributes()
        attrs.parse("*.jpg binary\n")

        XCTAssertTrue(attrs.isBinary("photo.jpg"))
        XCTAssertEqual(attrs.get("photo.jpg", attr: "diff"), .unset)
        XCTAssertEqual(attrs.get("photo.jpg", attr: "merge"), .unset)
        XCTAssertEqual(attrs.get("photo.jpg", attr: "text"), .unset)
    }

    func testLastMatchWins() {
        let attrs = Attributes()
        attrs.parse("* text\n*.bin -text\n")

        XCTAssertEqual(attrs.get("file.txt", attr: "text"), .set)
        XCTAssertEqual(attrs.get("file.bin", attr: "text"), .unset)
    }

    func testPathWithDirectory() {
        let attrs = Attributes()
        attrs.parse("src/*.rs text eol=lf\n")

        XCTAssertEqual(attrs.get("src/main.rs", attr: "text"), .set)
        XCTAssertNil(attrs.get("main.rs", attr: "text"))
    }

    func testGetAllAttrs() {
        let attrs = Attributes()
        attrs.parse("*.rs text eol=lf diff\n")

        let all = attrs.getAll("main.rs")
        XCTAssertEqual(all.count, 3)
        XCTAssertTrue(all.contains(where: { $0.0 == "text" && $0.1 == .set }))
        XCTAssertTrue(all.contains(where: { $0.0 == "eol" && $0.1 == .value("lf") }))
        XCTAssertTrue(all.contains(where: { $0.0 == "diff" && $0.1 == .set }))
    }

    func testCommentAndEmptyLines() {
        let attrs = Attributes()
        attrs.parse("# comment\n\n*.txt text\n  # another comment\n")

        XCTAssertEqual(attrs.get("file.txt", attr: "text"), .set)
        XCTAssertEqual(attrs.ruleCount, 1)
    }

    func testGlobPatterns() {
        let attrs = Attributes()
        attrs.parse("*.txt text\n*.[ch] diff\nMakefile export-ignore\n")

        XCTAssertEqual(attrs.get("file.txt", attr: "text"), .set)
        XCTAssertEqual(attrs.get("main.c", attr: "diff"), .set)
        XCTAssertEqual(attrs.get("util.h", attr: "diff"), .set)
        XCTAssertNil(attrs.get("main.rs", attr: "diff"))
        XCTAssertEqual(attrs.get("Makefile", attr: "export-ignore"), .set)
    }

    func testLoadAttrsFile() throws {
        let tmp = NSTemporaryDirectory() + "test_attrs_load_\(UUID().uuidString)"
        try FileManager.default.createDirectory(atPath: tmp, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let path = (tmp as NSString).appendingPathComponent(".gitattributes")
        try "*.txt text\n*.bin binary\n".write(toFile: path, atomically: true, encoding: .utf8)

        let attrs = Attributes.load(path: path)
        XCTAssertEqual(attrs.get("file.txt", attr: "text"), .set)
        XCTAssertTrue(attrs.isBinary("data.bin"))
    }

    func testLoadForRepo() throws {
        let tmp = NSTemporaryDirectory() + "test_attrs_repo_\(UUID().uuidString)"
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let workdir = repo.workdir!

        try "*.txt text\n".write(toFile: (workdir as NSString).appendingPathComponent(".gitattributes"), atomically: true, encoding: .utf8)

        let infoDir = (repo.gitDir as NSString).appendingPathComponent("info")
        try FileManager.default.createDirectory(atPath: infoDir, withIntermediateDirectories: true)
        try "*.bin binary\n".write(toFile: (infoDir as NSString).appendingPathComponent("attributes"), atomically: true, encoding: .utf8)

        let attrs = Attributes.loadForRepo(gitDir: repo.gitDir, workdir: workdir)
        XCTAssertEqual(attrs.get("file.txt", attr: "text"), .set)
        XCTAssertTrue(attrs.isBinary("data.bin"))
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

    // MARK: - Pack Object Tests

    func testApplyDeltaCopy() throws {
        let base = Array("hello world".utf8)
        let cmd: UInt8 = 0x80 | 0x01 | 0x10
        let delta: [UInt8] = [11, 11, cmd, 0, 11]
        let result = try applyDelta(base: base, delta: delta)
        XCTAssertEqual(result, base)
    }

    func testApplyDeltaInsert() throws {
        let base = Array("hello".utf8)
        let delta: [UInt8] = [5, 6, 6] + Array("world!".utf8)
        let result = try applyDelta(base: base, delta: delta)
        XCTAssertEqual(result, Array("world!".utf8))
    }

    func testApplyDeltaMixed() throws {
        let base = Array("hello cruel".utf8)
        let cmd2: UInt8 = 0x80 | 0x01 | 0x10
        let delta: [UInt8] = [11, 11, cmd2, 0, 5, 6] + Array(" world".utf8)
        let result = try applyDelta(base: base, delta: delta)
        XCTAssertEqual(result, Array("hello world".utf8))
    }

    func testBuildAndReadPack() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_pack_read"
        try? FileManager.default.removeItem(atPath: tmp)
        try FileManager.default.createDirectory(atPath: tmp, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let blobData = Array("hello pack\n".utf8)
        let packData = buildTestPack(objects: [(.blob, blobData)])
        let packPath = (tmp as NSString).appendingPathComponent("test.pack")
        try Data(packData).write(to: URL(fileURLWithPath: packPath))

        let oid = OID.hash(type: .blob, data: blobData)
        let idxData = buildPackIndex(oids: [oid], crcs: [0], offsets: [12])
        let idx = try parsePackIndex(idxData)

        let obj = try readPackObject(packPath: packPath, offset: 12, index: idx)
        XCTAssertEqual(obj.objType, .blob)
        XCTAssertEqual(Array(obj.data), blobData)
    }

    func testBuildAndReadMultipleObjects() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_pack_multi"
        try? FileManager.default.removeItem(atPath: tmp)
        try FileManager.default.createDirectory(atPath: tmp, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let blob1 = Array("first blob\n".utf8)
        let blob2 = Array("second blob\n".utf8)
        let packData = buildTestPack(objects: [(.blob, blob1), (.blob, blob2)])
        let packPath = (tmp as NSString).appendingPathComponent("test.pack")
        try Data(packData).write(to: URL(fileURLWithPath: packPath))

        let oid1 = OID.hash(type: .blob, data: blob1)
        var oids = [oid1]
        oids.sort { compareOidBytes($0.raw, $1.raw) }
        let idxData = buildPackIndex(oids: oids, crcs: [0], offsets: [12])
        let idx = try parsePackIndex(idxData)

        let obj1 = try readPackObject(packPath: packPath, offset: 12, index: idx)
        XCTAssertEqual(obj1.objType, .blob)
        XCTAssertEqual(Array(obj1.data), blob1)
    }

    func testReadPackCommit() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_pack_commit"
        try? FileManager.default.removeItem(atPath: tmp)
        try FileManager.default.createDirectory(atPath: tmp, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let commitData = Array("tree 0000000000000000000000000000000000000000\nauthor Test <t@t> 0 +0000\ncommitter Test <t@t> 0 +0000\n\ntest\n".utf8)
        let packData = buildTestPack(objects: [(.commit, commitData)])
        let packPath = (tmp as NSString).appendingPathComponent("test.pack")
        try Data(packData).write(to: URL(fileURLWithPath: packPath))

        let oid = OID.hash(type: .commit, data: commitData)
        let idxData = buildPackIndex(oids: [oid], crcs: [0], offsets: [12])
        let idx = try parsePackIndex(idxData)

        let obj = try readPackObject(packPath: packPath, offset: 12, index: idx)
        XCTAssertEqual(obj.objType, .commit)
        XCTAssertEqual(Array(obj.data), commitData)
    }

    // MARK: - Conformance Tests
    // These tests use identical inputs and expected outputs across all three ports
    // to verify cross-language consistency.

    func testConformanceSHA1Vectors() {
        // Vector 1: empty string
        let d1 = SHA1.hash([UInt8]())
        XCTAssertEqual(d1.map { String(format: "%02x", $0) }.joined(), "da39a3ee5e6b4b0d3255bfef95601890afd80709")

        // Vector 2: "hello"
        let d2 = SHA1.hash(Array("hello".utf8))
        XCTAssertEqual(d2.map { String(format: "%02x", $0) }.joined(), "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")

        // Vector 3: longer string
        let d3 = SHA1.hash(Array("The quick brown fox jumps over the lazy dog".utf8))
        XCTAssertEqual(d3.map { String(format: "%02x", $0) }.joined(), "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12")

        // Vector 4: with newline
        let d4 = SHA1.hash(Array("hello world\n".utf8))
        XCTAssertEqual(d4.map { String(format: "%02x", $0) }.joined(), "22596363b3de40b06f981fb85d82312e8c0ed511")
    }

    func testConformanceBlobOID() {
        // All ports must compute identical OIDs for the same blob content
        let oid1 = OID.hash(type: .blob, data: Array("hello\n".utf8))
        XCTAssertEqual(oid1.hex, "ce013625030ba8dba906f756967f9e9ca394464a")

        let oid2 = OID.hash(type: .blob, data: [UInt8]())
        XCTAssertEqual(oid2.hex, "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391")

        let oid3 = OID.hash(type: .blob, data: Array("test content\n".utf8))
        XCTAssertEqual(oid3.hex, "d670460b4b4aece5915caf5c68d12f560a9fe3e4")
    }

    func testConformanceCommitOID() throws {
        // Serialize a commit with fixed inputs and verify OID matches across ports
        let treeId = OID(hex: "4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        let author = Signature(name: "Conf Author", email: "author@conf.test", time: 1700000000, offset: 0)
        let committer = Signature(name: "Conf Committer", email: "committer@conf.test", time: 1700000000, offset: 0)

        let data = serializeCommit(treeId: treeId, parentIds: [], author: author, committer: committer, message: "conformance test commit\n")
        let oid = OID.hash(type: .commit, data: Array(data))

        // Verify round-trip
        let parsed = try parseCommit(oid: oid, data: data)
        XCTAssertEqual(parsed.treeId, treeId)
        XCTAssertEqual(parsed.author.name, "Conf Author")
        XCTAssertEqual(parsed.committer.email, "committer@conf.test")
        XCTAssertEqual(parsed.message, "conformance test commit\n")

        // The OID must be deterministic — same across all ports
        // Store this value and verify it matches Kotlin and Rust
        XCTAssertFalse(oid.isZero)
        XCTAssertEqual(oid.hex.count, 40)
    }

    func testConformanceTreeOID() throws {
        // Build a tree with known entries and verify OID
        let blobOid = OID(hex: "ce013625030ba8dba906f756967f9e9ca394464a")
        let entries = [
            TreeEntry(mode: FileMode.blob.rawValue, name: "hello.txt", oid: blobOid),
        ]
        let data = serializeTree(entries: entries)
        let oid = OID.hash(type: .tree, data: Array(data))

        let parsed = try parseTree(oid: oid, data: data)
        XCTAssertEqual(parsed.entries.count, 1)
        XCTAssertEqual(parsed.entries[0].name, "hello.txt")
        XCTAssertEqual(parsed.entries[0].oid, blobOid)

        XCTAssertFalse(oid.isZero)
        XCTAssertEqual(oid.hex.count, 40)
    }

    func testConformanceTagOID() throws {
        let targetId = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let tagger = Signature(name: "Conf Tagger", email: "tagger@conf.test", time: 1700000000, offset: 0)

        let data = serializeTag(targetId: targetId, targetType: .commit, tagName: "v1.0-conf", tagger: tagger, message: "conformance tag\n")
        let oid = OID.hash(type: .tag, data: Array(data))

        let parsed = try parseTag(oid: oid, data: data)
        XCTAssertEqual(parsed.targetId, targetId)
        XCTAssertEqual(parsed.tagName, "v1.0-conf")
        XCTAssertEqual(parsed.tagger?.name, "Conf Tagger")

        XCTAssertFalse(oid.isZero)
    }

    func testConformanceSHA256Vectors() {
        // Vector 1: empty string
        let d1 = SHA256Hash.hash([UInt8]())
        XCTAssertEqual(d1.map { String(format: "%02x", $0) }.joined(), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")

        // Vector 2: "hello"
        let d2 = SHA256Hash.hash(Array("hello".utf8))
        XCTAssertEqual(d2.map { String(format: "%02x", $0) }.joined(), "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")

        // Vector 3: longer string
        let d3 = SHA256Hash.hash(Array("The quick brown fox jumps over the lazy dog".utf8))
        XCTAssertEqual(d3.map { String(format: "%02x", $0) }.joined(), "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592")
    }

    func testConformanceSHA256BlobOID() {
        let oid1 = OID.hashSHA256(type: .blob, data: Array("hello\n".utf8))
        XCTAssertEqual(oid1.hex.count, 64)
        XCTAssertFalse(oid1.isZero)

        let oid2 = OID.hashSHA256(type: .blob, data: [UInt8]())
        XCTAssertEqual(oid2.hex.count, 64)

        // SHA-256 and SHA-1 should produce different OIDs
        let oidSha1 = OID.hash(type: .blob, data: Array("hello\n".utf8))
        XCTAssertNotEqual(oid1.hex, oidSha1.hex)
    }

    func testConformanceHashAlgorithm() {
        XCTAssertEqual(HashAlgorithm.sha1.digestLength, 20)
        XCTAssertEqual(HashAlgorithm.sha256.digestLength, 32)
        XCTAssertEqual(HashAlgorithm.sha1.hexLength, 40)
        XCTAssertEqual(HashAlgorithm.sha256.hexLength, 64)
    }

    func testConformanceSignatureFormat() {
        // Positive offset
        let sig1 = Signature(name: "Test User", email: "test@example.com", time: 1234567890, offset: 330)
        let fmt1 = formatSignature(sig1)
        XCTAssertEqual(fmt1, "Test User <test@example.com> 1234567890 +0530")

        // Negative offset
        let sig2 = Signature(name: "Test", email: "test@test.com", time: 1000, offset: -480)
        let fmt2 = formatSignature(sig2)
        XCTAssertEqual(fmt2, "Test <test@test.com> 1000 -0800")

        // Zero offset
        let sig3 = Signature(name: "Zero", email: "zero@test.com", time: 0, offset: 0)
        let fmt3 = formatSignature(sig3)
        XCTAssertEqual(fmt3, "Zero <zero@test.com> 0 +0000")
    }

    func testConformanceDeltaApply() throws {
        // Copy entire base
        let base1 = Array("hello world".utf8)
        let cmd1: UInt8 = 0x80 | 0x01 | 0x10
        let delta1: [UInt8] = [11, 11, cmd1, 0, 11]
        let result1 = try applyDelta(base: base1, delta: delta1)
        XCTAssertEqual(String(bytes: result1, encoding: .utf8), "hello world")

        // Insert only
        let base2 = Array("hello".utf8)
        let delta2: [UInt8] = [5, 6, 6] + Array("world!".utf8)
        let result2 = try applyDelta(base: base2, delta: delta2)
        XCTAssertEqual(String(bytes: result2, encoding: .utf8), "world!")

        // Copy + insert
        let base3 = Array("hello cruel".utf8)
        let cmd3: UInt8 = 0x80 | 0x01 | 0x10
        let delta3: [UInt8] = [11, 11, cmd3, 0, 5, 6] + Array(" world".utf8)
        let result3 = try applyDelta(base: base3, delta: delta3)
        XCTAssertEqual(String(bytes: result3, encoding: .utf8), "hello world")
    }

    // SHA-256 Tests

    func testSHA256Empty() {
        let digest = SHA256Hash.hash([UInt8]())
        XCTAssertEqual(digest.map { String(format: "%02x", $0) }.joined(), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    }

    func testSHA256Hello() {
        let digest = SHA256Hash.hash(Array("hello".utf8))
        XCTAssertEqual(digest.map { String(format: "%02x", $0) }.joined(), "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
    }

    func testSHA256GitBlob() {
        let data = Array("hello\n".utf8)
        let oid = OID.hashSHA256(type: .blob, data: data)
        XCTAssertEqual(oid.hex.count, 64)
        XCTAssertFalse(oid.isZero)
    }

    func testSHA256Longer() {
        let digest = SHA256Hash.hash(Array("The quick brown fox jumps over the lazy dog".utf8))
        XCTAssertEqual(digest.map { String(format: "%02x", $0) }.joined(), "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592")
    }

    func testHashAlgorithm() {
        XCTAssertEqual(HashAlgorithm.sha1.digestLength, 20)
        XCTAssertEqual(HashAlgorithm.sha256.digestLength, 32)
        XCTAssertEqual(HashAlgorithm.sha1.hexLength, 40)
        XCTAssertEqual(HashAlgorithm.sha256.hexLength, 64)
    }

    func testZeroSHA256() {
        let z = OID.zeroSHA256
        XCTAssertTrue(z.isZero)
        XCTAssertEqual(z.hex.count, 64)
    }

    func testConformanceIndexRoundTrip() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_conf_index"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let oid = OID(hex: "ce013625030ba8dba906f756967f9e9ca394464a")

        var index = Index()
        index.add(IndexEntry(mode: 0o100644, fileSize: 6, oid: oid, path: "hello.txt"))
        index.add(IndexEntry(mode: 0o100755, fileSize: 100, oid: OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"), path: "script.sh"))
        try writeIndex(gitDir: repo.gitDir, index: index)

        let loaded = try readIndex(gitDir: repo.gitDir)
        XCTAssertEqual(loaded.entries.count, 2)
        XCTAssertEqual(loaded.entries[0].path, "hello.txt")
        XCTAssertEqual(loaded.entries[1].path, "script.sh")
        XCTAssertEqual(loaded.entries[0].mode, 0o100644)
        XCTAssertEqual(loaded.entries[1].mode, 0o100755)
    }

    // ── libgit2 Parity Tests ──

    // OID parity (libgit2 core/oid.c)

    func testParityOIDFromValidHex() {
        let oid = OID(hex: "ae90f12eea699729ed24555e40b9fd669da12a12")
        XCTAssertEqual(oid.hex, "ae90f12eea699729ed24555e40b9fd669da12a12")
    }

    func testParityOIDZeroIsZero() {
        XCTAssertTrue(OID.zero.isZero)
        XCTAssertEqual(OID.zero.hex, "0000000000000000000000000000000000000000")
    }

    func testParityOIDNonzeroIsNotZero() {
        let oid = OID(hex: "ae90f12eea699729ed24555e40b9fd669da12a12")
        XCTAssertFalse(oid.isZero)
    }

    func testParityOIDEquality() {
        let a = OID(hex: "ae90f12eea699729ed24555e40b9fd669da12a12")
        let b = OID(hex: "ae90f12eea699729ed24555e40b9fd669da12a12")
        let c = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        XCTAssertEqual(a, b)
        XCTAssertNotEqual(a, c)
    }

    func testParityOIDSHA256Roundtrip() {
        let hex64 = "d3e63d2f2e43d1fee23a74bf19a0ede156cba2d1bd602eba13de433cea1bb512"
        let oid = OID(hex: hex64)
        XCTAssertEqual(oid.hex, hex64)
        XCTAssertEqual(oid.raw.count, 32)
    }

    func testParityOIDSHA1vsSHA256Different() {
        let data = Array("test content\n".utf8)
        let sha1Oid = OID.hash(type: .blob, data: data)
        let sha256Oid = OID.hashSHA256(type: .blob, data: data)
        XCTAssertNotEqual(sha1Oid.hex, sha256Oid.hex)
        XCTAssertEqual(sha1Oid.hex.count, 40)
        XCTAssertEqual(sha256Oid.hex.count, 64)
    }

    func testParityHashAlgorithmProperties() {
        XCTAssertEqual(HashAlgorithm.sha1.digestLength, 20)
        XCTAssertEqual(HashAlgorithm.sha256.digestLength, 32)
        XCTAssertEqual(HashAlgorithm.sha1.hexLength, 40)
        XCTAssertEqual(HashAlgorithm.sha256.hexLength, 64)
    }

    // Signature parity (libgit2 commit/signature.c)

    func testParitySignaturePositiveOffset() {
        let sig = Signature(name: "Test User", email: "test@test.tt", time: 1461698487, offset: 120)
        XCTAssertEqual(formatSignature(sig), "Test User <test@test.tt> 1461698487 +0200")
    }

    func testParitySignatureNegativeOffset() {
        let sig = Signature(name: "Test", email: "test@test.com", time: 1000, offset: -300)
        XCTAssertEqual(formatSignature(sig), "Test <test@test.com> 1000 -0500")
    }

    func testParitySignatureZeroOffset() {
        let sig = Signature(name: "A", email: "a@b.c", time: 0, offset: 0)
        XCTAssertEqual(formatSignature(sig), "A <a@b.c> 0 +0000")
    }

    func testParitySignatureLargeOffset() {
        let sig = Signature(name: "A", email: "a@example.com", time: 1461698487, offset: 754)
        XCTAssertEqual(formatSignature(sig), "A <a@example.com> 1461698487 +1234")
    }

    func testParitySignatureSingleCharName() {
        let sig = Signature(name: "x", email: "x@y.z", time: 100, offset: 0)
        XCTAssertEqual(formatSignature(sig), "x <x@y.z> 100 +0000")
    }

    // Commit parity (libgit2 object/validate.c)

    func testParityCommitNoParents() throws {
        let treeId = OID(hex: "4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        let author = Signature(name: "Author", email: "a@a.com", time: 1638286404, offset: -300)
        let committer = Signature(name: "Committer", email: "c@c.com", time: 1638324642, offset: -300)
        let data = serializeCommit(treeId: treeId, parentIds: [], author: author, committer: committer, message: "initial commit\n")
        let oid = OID.hash(type: .commit, data: Array(data))
        let parsed = try parseCommit(oid: oid, data: data)
        XCTAssertEqual(parsed.treeId, treeId)
        XCTAssertTrue(parsed.parentIds.isEmpty)
        XCTAssertEqual(parsed.author.name, "Author")
        XCTAssertEqual(parsed.message, "initial commit\n")
    }

    func testParityCommitMultipleParents() throws {
        let treeId = OID(hex: "4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        let p1 = OID(hex: "ae90f12eea699729ed24555e40b9fd669da12a12")
        let p2 = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let sig = Signature(name: "M", email: "m@m.com", time: 1000, offset: 0)
        let data = serializeCommit(treeId: treeId, parentIds: [p1, p2], author: sig, committer: sig, message: "merge\n")
        let oid = OID.hash(type: .commit, data: Array(data))
        let parsed = try parseCommit(oid: oid, data: data)
        XCTAssertEqual(parsed.parentIds.count, 2)
        XCTAssertEqual(parsed.parentIds[0], p1)
        XCTAssertEqual(parsed.parentIds[1], p2)
    }

    func testParityCommitWithEncoding() throws {
        let treeId = OID(hex: "4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        let sig = Signature(name: "UTF8", email: "u@u.com", time: 0, offset: 0)
        let data = serializeCommit(treeId: treeId, parentIds: [], author: sig, committer: sig, message: "msg\n", messageEncoding: "ISO-8859-1")
        let oid = OID.hash(type: .commit, data: Array(data))
        let parsed = try parseCommit(oid: oid, data: data)
        XCTAssertEqual(parsed.messageEncoding, "ISO-8859-1")
    }

    func testParityCommitRoundtripPreservesOID() throws {
        let treeId = OID(hex: "bdd24e358576f1baa275df98cdcaf3ac9a3f4233")
        let parentId = OID(hex: "d6d956f1d66210bfcd0484166befab33b5987a39")
        let author = Signature(name: "Edward Thomson", email: "ethomson@edwardthomson.com", time: 1638286404, offset: -300)
        let committer = Signature(name: "Edward Thomson", email: "ethomson@edwardthomson.com", time: 1638324642, offset: -300)
        let data1 = serializeCommit(treeId: treeId, parentIds: [parentId], author: author, committer: committer, message: "commit go here.\n")
        let oid1 = OID.hash(type: .commit, data: Array(data1))
        let parsed = try parseCommit(oid: oid1, data: data1)
        let data2 = serializeCommit(treeId: parsed.treeId, parentIds: parsed.parentIds, author: parsed.author, committer: parsed.committer, message: parsed.message, messageEncoding: parsed.messageEncoding)
        let oid2 = OID.hash(type: .commit, data: Array(data2))
        XCTAssertEqual(oid1, oid2)
    }

    // Tree parity (libgit2 object/tree/parse.c)

    func testParityTreeEmpty() throws {
        let data = serializeTree(entries: [])
        XCTAssertTrue(data.isEmpty)
        let oid = OID.hash(type: .tree, data: Array(data))
        let parsed = try parseTree(oid: oid, data: data)
        XCTAssertTrue(parsed.entries.isEmpty)
        let oid2 = OID.hash(type: .tree, data: [])
        XCTAssertEqual(oid, oid2)
    }

    func testParityTreeSingleBlob() throws {
        let blobOid = OID(hex: "ae90f12eea699729ed24555e40b9fd669da12a12")
        let entries = [TreeEntry(mode: FileMode.blob.rawValue, name: "foo", oid: blobOid)]
        let data = serializeTree(entries: entries)
        let parsed = try parseTree(oid: OID.zero, data: data)
        XCTAssertEqual(parsed.entries.count, 1)
        XCTAssertEqual(parsed.entries[0].name, "foo")
        XCTAssertEqual(parsed.entries[0].mode, FileMode.blob.rawValue)
        XCTAssertEqual(parsed.entries[0].oid, blobOid)
    }

    func testParityTreeSingleSubtree() throws {
        let treeOid = OID(hex: "ae90f12eea699729ed24555e40b9fd669da12a12")
        let entries = [TreeEntry(mode: FileMode.tree.rawValue, name: "subdir", oid: treeOid)]
        let data = serializeTree(entries: entries)
        let parsed = try parseTree(oid: OID.zero, data: data)
        XCTAssertEqual(parsed.entries.count, 1)
        XCTAssertTrue(parsed.entries[0].isTree)
        XCTAssertFalse(parsed.entries[0].isBlob)
    }

    func testParityTreeMultipleModes() throws {
        let oid1 = OID(hex: "ae90f12eea699729ed24555e40b9fd669da12a12")
        let oid2 = OID(hex: "e8bfe5af39579a7e4898bb23f3a76a72c368cee6")
        let entries = [
            TreeEntry(mode: FileMode.blob.rawValue, name: "file.txt", oid: oid1),
            TreeEntry(mode: FileMode.blobExe.rawValue, name: "run.sh", oid: oid2),
            TreeEntry(mode: FileMode.link.rawValue, name: "sym", oid: oid1),
            TreeEntry(mode: FileMode.tree.rawValue, name: "dir", oid: oid2),
        ]
        let data = serializeTree(entries: entries)
        let parsed = try parseTree(oid: OID.zero, data: data)
        XCTAssertEqual(parsed.entries.count, 4)
        XCTAssertEqual(parsed.entries[0].name, "dir")
        XCTAssertEqual(parsed.entries[0].mode, FileMode.tree.rawValue)
        XCTAssertEqual(parsed.entries[1].name, "file.txt")
        XCTAssertEqual(parsed.entries[2].name, "run.sh")
        XCTAssertEqual(parsed.entries[3].name, "sym")
    }

    func testParityTreeRoundtripPreservesOID() throws {
        let oid = OID(hex: "ce013625030ba8dba906f756967f9e9ca394464a")
        let entries = [
            TreeEntry(mode: FileMode.blob.rawValue, name: "hello.txt", oid: oid),
            TreeEntry(mode: FileMode.blobExe.rawValue, name: "script.sh", oid: oid),
        ]
        let data1 = serializeTree(entries: entries)
        let treeOid1 = OID.hash(type: .tree, data: Array(data1))
        let parsed = try parseTree(oid: treeOid1, data: data1)
        let data2 = serializeTree(entries: parsed.entries)
        let treeOid2 = OID.hash(type: .tree, data: Array(data2))
        XCTAssertEqual(treeOid1, treeOid2)
    }

    // Tag parity

    func testParityTagTargetingDifferentTypes() throws {
        let target = OID(hex: "ae90f12eea699729ed24555e40b9fd669da12a12")
        let tagger = Signature(name: "T", email: "t@t", time: 0, offset: 0)
        for objType in [ObjectType.commit, ObjectType.tree, ObjectType.blob] {
            let data = serializeTag(targetId: target, targetType: objType, tagName: "v1.0", tagger: tagger, message: "tag msg\n")
            let oid = OID.hash(type: .tag, data: Array(data))
            let parsed = try parseTag(oid: oid, data: data)
            XCTAssertEqual(parsed.targetType, objType)
            XCTAssertEqual(parsed.tagName, "v1.0")
        }
    }

    func testParityTagWithoutTagger() throws {
        let target = OID(hex: "ae90f12eea699729ed24555e40b9fd669da12a12")
        let data = serializeTag(targetId: target, targetType: .commit, tagName: "lightweight", tagger: nil, message: "no tagger\n")
        let oid = OID.hash(type: .tag, data: Array(data))
        let parsed = try parseTag(oid: oid, data: data)
        XCTAssertNil(parsed.tagger)
        XCTAssertEqual(parsed.tagName, "lightweight")
    }

    // Config parity (libgit2 config/read.c)

    func testParityConfigBooleanValues() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_parity_config_bool"
        try? FileManager.default.removeItem(atPath: tmp)
        try FileManager.default.createDirectory(atPath: tmp, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: tmp) }
        let path = (tmp as NSString).appendingPathComponent("config")
        try "[core]\n\tfilemode = true\n\tbare = false\n\tyes = yes\n\tno = no\n\ton = on\n\toff = off\n\tone = 1\n\tzero = 0\n".write(toFile: path, atomically: true, encoding: .utf8)
        let cfg = try Config.load(from: path)
        XCTAssertEqual(cfg.getBool(section: "core", key: "filemode"), true)
        XCTAssertEqual(cfg.getBool(section: "core", key: "bare"), false)
        XCTAssertEqual(cfg.getBool(section: "core", key: "yes"), true)
        XCTAssertEqual(cfg.getBool(section: "core", key: "no"), false)
        XCTAssertEqual(cfg.getBool(section: "core", key: "on"), true)
        XCTAssertEqual(cfg.getBool(section: "core", key: "off"), false)
        XCTAssertEqual(cfg.getBool(section: "core", key: "one"), true)
        XCTAssertEqual(cfg.getBool(section: "core", key: "zero"), false)
    }

    func testParityConfigIntSuffixes() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_parity_config_int"
        try? FileManager.default.removeItem(atPath: tmp)
        try FileManager.default.createDirectory(atPath: tmp, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: tmp) }
        let path = (tmp as NSString).appendingPathComponent("config")
        try "[core]\n\tplain = 42\n\tkilo = 1k\n\tmega = 1m\n\tgiga = 1g\n".write(toFile: path, atomically: true, encoding: .utf8)
        let cfg = try Config.load(from: path)
        XCTAssertEqual(cfg.getInt(section: "core", key: "plain"), 42)
        XCTAssertEqual(cfg.getInt(section: "core", key: "kilo"), 1024)
        XCTAssertEqual(cfg.getInt(section: "core", key: "mega"), 1048576)
        XCTAssertEqual(cfg.getInt(section: "core", key: "giga"), 1073741824)
    }

    func testParityConfigCaseInsensitive() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_parity_config_case"
        try? FileManager.default.removeItem(atPath: tmp)
        try FileManager.default.createDirectory(atPath: tmp, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: tmp) }
        let path = (tmp as NSString).appendingPathComponent("config")
        try "[Core]\n\tFileMode = true\n".write(toFile: path, atomically: true, encoding: .utf8)
        let cfg = try Config.load(from: path)
        XCTAssertEqual(cfg.getBool(section: "core", key: "filemode"), true)
        XCTAssertEqual(cfg.getBool(section: "CORE", key: "FILEMODE"), true)
    }

    func testParityConfigComments() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_parity_config_comments"
        try? FileManager.default.removeItem(atPath: tmp)
        try FileManager.default.createDirectory(atPath: tmp, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: tmp) }
        let path = (tmp as NSString).appendingPathComponent("config")
        try "# Comment\n; Also comment\n[core]\n\t# in section\n\tbare = false\n".write(toFile: path, atomically: true, encoding: .utf8)
        let cfg = try Config.load(from: path)
        XCTAssertEqual(cfg.getBool(section: "core", key: "bare"), false)
    }

    // Blob parity

    func testParityBlobEmptyOID() {
        let oid = OID.hash(type: .blob, data: [])
        XCTAssertEqual(oid.hex, "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391")
    }

    func testParityBlobKnownContent() {
        let oid = OID.hash(type: .blob, data: Array("hello\n".utf8))
        XCTAssertEqual(oid.hex, "ce013625030ba8dba906f756967f9e9ca394464a")
    }

    func testParityBlobNewlineOnly() {
        let oid = OID.hash(type: .blob, data: [0x0a])
        XCTAssertEqual(oid.hex, "8b137891791fe96927ad78e64b0aad7bded08bdc")
    }

    // Index parity

    func testParityIndexSortedByPath() {
        let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        var index = Index()
        index.add(IndexEntry(mode: 0o100644, oid: oid, path: "z.txt"))
        index.add(IndexEntry(mode: 0o100644, oid: oid, path: "a.txt"))
        index.add(IndexEntry(mode: 0o100644, oid: oid, path: "m/file.c"))
        XCTAssertEqual(index.entries[0].path, "a.txt")
        XCTAssertEqual(index.entries[1].path, "m/file.c")
        XCTAssertEqual(index.entries[2].path, "z.txt")
    }

    func testParityIndexDuplicatePathReplaces() {
        let oid1 = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let oid2 = OID(hex: "ce013625030ba8dba906f756967f9e9ca394464a")
        var index = Index()
        index.add(IndexEntry(mode: 0o100644, fileSize: 10, oid: oid1, path: "file.txt"))
        index.add(IndexEntry(mode: 0o100644, fileSize: 20, oid: oid2, path: "file.txt"))
        XCTAssertEqual(index.entries.count, 1)
        XCTAssertEqual(index.entries[0].oid, oid2)
        XCTAssertEqual(index.entries[0].fileSize, 20)
    }

    func testParityIndexManyEntriesRoundtrip() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_parity_index_many"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }
        let repo = try Repository.create(at: tmp)
        let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        var index = Index()
        for i in 0..<100 {
            index.add(IndexEntry(mode: 0o100644, fileSize: UInt32(i), oid: oid, path: String(format: "file_%04d.txt", i)))
        }
        try writeIndex(gitDir: repo.gitDir, index: index)
        let loaded = try readIndex(gitDir: repo.gitDir)
        XCTAssertEqual(loaded.entries.count, 100)
        for i in 1..<loaded.entries.count {
            XCTAssertTrue(loaded.entries[i - 1].path < loaded.entries[i].path)
        }
    }

    // Diff parity

    func testParityDiffSortedOutput() {
        let oid1 = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let oid2 = OID(hex: "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let old = [
            TreeEntry(mode: FileMode.blob.rawValue, name: "a.txt", oid: oid1),
            TreeEntry(mode: FileMode.blob.rawValue, name: "c.txt", oid: oid1),
            TreeEntry(mode: FileMode.blob.rawValue, name: "e.txt", oid: oid1),
        ]
        let new = [
            TreeEntry(mode: FileMode.blob.rawValue, name: "b.txt", oid: oid2),
            TreeEntry(mode: FileMode.blob.rawValue, name: "c.txt", oid: oid2),
            TreeEntry(mode: FileMode.blob.rawValue, name: "d.txt", oid: oid2),
        ]
        let deltas = diffTrees(oldEntries: old, newEntries: new)
        let paths = deltas.map { $0.path }
        XCTAssertEqual(paths, ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"])
        XCTAssertEqual(deltas[0].status, .deleted)
        XCTAssertEqual(deltas[1].status, .added)
        XCTAssertEqual(deltas[2].status, .modified)
        XCTAssertEqual(deltas[3].status, .added)
        XCTAssertEqual(deltas[4].status, .deleted)
    }

    func testParityDiffModeChangeIsModified() {
        let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let old = [TreeEntry(mode: FileMode.blob.rawValue, name: "f", oid: oid)]
        let new = [TreeEntry(mode: FileMode.blobExe.rawValue, name: "f", oid: oid)]
        let deltas = diffTrees(oldEntries: old, newEntries: new)
        XCTAssertEqual(deltas.count, 1)
        XCTAssertEqual(deltas[0].status, .modified)
    }

    // Delta parity

    func testParityDeltaEmptyInsert() throws {
        let base = Array("base".utf8)
        var delta: [UInt8] = [4, 3, 3]
        delta.append(contentsOf: Array("new".utf8))
        let result = try applyDelta(base: base, delta: delta)
        XCTAssertEqual(String(bytes: result, encoding: .utf8), "new")
    }

    func testParityDeltaInvalidOpcodeZero() {
        let base = Array("base".utf8)
        let delta: [UInt8] = [4, 1, 0]
        XCTAssertThrowsError(try applyDelta(base: base, delta: delta))
    }

    // SHA NIST vectors

    func testParitySHA1NISTVectors() {
        XCTAssertEqual(SHA1.hash(Array("abc".utf8)).map { String(format: "%02x", $0) }.joined(), "a9993e364706816aba3e25717850c26c9cd0d89d")
        XCTAssertEqual(SHA1.hash([UInt8]()).map { String(format: "%02x", $0) }.joined(), "da39a3ee5e6b4b0d3255bfef95601890afd80709")
    }

    func testParitySHA256NISTVectors() {
        XCTAssertEqual(SHA256Hash.hash(Array("abc".utf8)).map { String(format: "%02x", $0) }.joined(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        XCTAssertEqual(SHA256Hash.hash([UInt8]()).map { String(format: "%02x", $0) }.joined(), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    }

    // Repository parity

    func testParityRepoInitCreatesStructure() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_parity_repo_init"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }
        let repo = try Repository.create(at: tmp)
        XCTAssertFalse(repo.isBare)
        XCTAssertTrue(FileManager.default.fileExists(atPath: (repo.gitDir as NSString).appendingPathComponent("HEAD")))
        XCTAssertTrue(FileManager.default.fileExists(atPath: (repo.gitDir as NSString).appendingPathComponent("objects")))
        XCTAssertTrue(FileManager.default.fileExists(atPath: (repo.gitDir as NSString).appendingPathComponent("refs")))
        let head = try String(contentsOfFile: (repo.gitDir as NSString).appendingPathComponent("HEAD"), encoding: .utf8)
        XCTAssertTrue(head.contains("ref: refs/heads/main"))
    }

    func testParityRepoInitBare() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_parity_repo_bare"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }
        let repo = try Repository.create(at: tmp, bare: true)
        XCTAssertTrue(repo.isBare)
        XCTAssertTrue(FileManager.default.fileExists(atPath: (repo.gitDir as NSString).appendingPathComponent("HEAD")))
    }

    func testParityRepoReinitPreserves() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_parity_repo_reinit"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }
        let _ = try Repository.create(at: tmp)
        let gitDir = (tmp as NSString).appendingPathComponent(".git")
        try "ae90f12eea699729ed24555e40b9fd669da12a12\n".write(toFile: ((gitDir as NSString).appendingPathComponent("refs/heads/main")), atomically: true, encoding: .utf8)
        let _ = try Repository.create(at: tmp)
        let refContent = try String(contentsOfFile: ((gitDir as NSString).appendingPathComponent("refs/heads/main")), encoding: .utf8)
        XCTAssertTrue(refContent.contains("ae90f12eea699729ed24555e40b9fd669da12a12"))
    }

    // ── Performance Tests ──

    private func measureMs(_ block: () -> Void) -> Double {
        let start = Date()
        block()
        return Date().timeIntervalSince(start) * 1000.0
    }

    func testPerfSHA1Throughput1MB() {
        let data = [UInt8](repeating: 0xAB, count: 1_000_000)
        let ms = measureMs { let _ = SHA1.hash(data) }
        print("[perf] SHA-1 1MB: \(String(format: "%.2f", ms))ms")
        XCTAssertTrue(ms < 1000.0, "SHA-1 1MB took \(ms)ms")
    }

    func testPerfSHA256Throughput1MB() {
        let data = [UInt8](repeating: 0xAB, count: 1_000_000)
        let ms = measureMs { let _ = SHA256Hash.hash(data) }
        print("[perf] SHA-256 1MB: \(String(format: "%.2f", ms))ms")
        XCTAssertTrue(ms < 1000.0, "SHA-256 1MB took \(ms)ms")
    }

    func testPerfOIDCreation10K() {
        let ms = measureMs {
            for i in 0..<10_000 {
                let data = Array("blob content \(i)".utf8)
                let _ = OID.hash(type: .blob, data: data)
            }
        }
        print("[perf] OID creation 10K: \(String(format: "%.2f", ms))ms")
        XCTAssertTrue(ms < 5000.0, "OID creation 10K took \(ms)ms")
    }

    func testPerfTreeSerialize1KEntries() throws {
        let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let entries = (0..<1000).map { TreeEntry(mode: FileMode.blob.rawValue, name: String(format: "file_%04d.txt", $0), oid: oid) }
        let ms = measureMs {
            let data = serializeTree(entries: entries)
            let _ = try! parseTree(oid: OID.zero, data: data)
        }
        print("[perf] Tree serialize+parse 1K: \(String(format: "%.2f", ms))ms")
        XCTAssertTrue(ms < 1000.0, "Tree 1K took \(ms)ms")
    }

    func testPerfCommitSerialize10K() {
        let treeId = OID(hex: "4b825dc642cb6eb9a060e54bf899d69f7cb46237")
        let sig = Signature(name: "Perf Test", email: "perf@test", time: 1000000, offset: 0)
        let ms = measureMs {
            for i in 0..<10_000 {
                let _ = serializeCommit(treeId: treeId, parentIds: [], author: sig, committer: sig, message: "commit \(i)\n")
            }
        }
        print("[perf] Commit serialize 10K: \(String(format: "%.2f", ms))ms")
        XCTAssertTrue(ms < 5000.0, "Commit serialize 10K took \(ms)ms")
    }

    func testPerfIndexReadWrite1K() throws {
        let tmp = NSTemporaryDirectory() + "muongit_swift_test_perf_index"
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }
        let repo = try Repository.create(at: tmp)
        let oid = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        var index = Index()
        for i in 0..<1000 {
            index.add(IndexEntry(mode: 0o100644, fileSize: UInt32(i), oid: oid, path: String(format: "src/file_%04d.txt", i)))
        }
        let ms = measureMs {
            try! writeIndex(gitDir: repo.gitDir, index: index)
            let _ = try! readIndex(gitDir: repo.gitDir)
        }
        print("[perf] Index write+read 1K: \(String(format: "%.2f", ms))ms")
        XCTAssertTrue(ms < 5000.0, "Index 1K took \(ms)ms")
    }

    func testPerfDiffLargeTrees() {
        let oid1 = OID(hex: "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let oid2 = OID(hex: "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        let old = (0..<1000).map { TreeEntry(mode: FileMode.blob.rawValue, name: String(format: "file_%04d.txt", $0), oid: oid1) }
        let new = (0..<1000).map { i -> TreeEntry in
            let oid = i % 10 == 0 ? oid2 : oid1
            return TreeEntry(mode: FileMode.blob.rawValue, name: String(format: "file_%04d.txt", i), oid: oid)
        }
        let ms = measureMs {
            let deltas = diffTrees(oldEntries: old, newEntries: new)
            XCTAssertEqual(deltas.count, 100)
        }
        print("[perf] Diff 1K-entry trees: \(String(format: "%.2f", ms))ms")
        XCTAssertTrue(ms < 1000.0, "Diff 1K took \(ms)ms")
    }

    func testPerfBlobHashing10K() {
        let ms = measureMs {
            for i in 0..<10_000 {
                let content = Array("line \(i)\nmore content here\n".utf8)
                let _ = OID.hash(type: .blob, data: content)
            }
        }
        print("[perf] Blob hashing 10K: \(String(format: "%.2f", ms))ms")
        XCTAssertTrue(ms < 5000.0, "Blob hashing 10K took \(ms)ms")
    }

    func testPerfSHA1vsSHA256Comparison() {
        let data = [UInt8](repeating: 0xAB, count: 1_000_000)
        let msSha1 = measureMs { let _ = SHA1.hash(data) }
        let msSha256 = measureMs { let _ = SHA256Hash.hash(data) }
        print("[perf] SHA-1 1MB: \(String(format: "%.2f", msSha1))ms, SHA-256 1MB: \(String(format: "%.2f", msSha256))ms, ratio: \(String(format: "%.2f", msSha256 / max(msSha1, 0.001)))x")
        XCTAssertTrue(msSha1 < 1000.0)
        XCTAssertTrue(msSha256 < 1000.0)
    }
}
