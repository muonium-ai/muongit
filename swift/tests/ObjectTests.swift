import XCTest
@testable import MuonGit

final class ObjectTests: XCTestCase {
    func testReadLooseObjectAndConvertToBlob() throws {
        let tmp = testDirectory("swift_object_loose_lookup")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let blobData = Data("object api loose blob\n".utf8)
        let blobOID = try writeBlob(gitDir: repo.gitDir, data: blobData)

        let object = try repo.readObject(blobOID)
        XCTAssertEqual(object.oid, blobOID)
        XCTAssertEqual(object.objectType, .blob)
        XCTAssertEqual(object.size, blobData.count)

        let blob = try object.asBlob()
        XCTAssertEqual(blob.data, blobData)
    }

    func testReadPackedObjectByOID() throws {
        let tmp = testDirectory("swift_object_pack_lookup")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let packDir = (repo.gitDir as NSString).appendingPathComponent("objects/pack")
        try FileManager.default.createDirectory(atPath: packDir, withIntermediateDirectories: true)

        let blobData = Array("packed object payload\n".utf8)
        let blobOID = OID.hash(type: .blob, data: blobData)
        let packData = buildTestPack(objects: [(.blob, blobData)])
        let idxData = buildPackIndex(oids: [blobOID], crcs: [0], offsets: [12])

        let packPath = (packDir as NSString).appendingPathComponent("test.pack")
        let idxPath = (packDir as NSString).appendingPathComponent("test.idx")
        try Data(packData).write(to: URL(fileURLWithPath: packPath))
        try Data(idxData).write(to: URL(fileURLWithPath: idxPath))

        let object = try readObject(gitDir: repo.gitDir, oid: blobOID)
        XCTAssertEqual(object.objectType, .blob)
        XCTAssertEqual(object.size, blobData.count)
        XCTAssertEqual(object.data, Data(blobData))
    }

    func testPeelTagToTargetObject() throws {
        let tmp = testDirectory("swift_object_peel_tag")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let blobData = Data("peeled blob\n".utf8)
        let blobOID = try writeBlob(gitDir: repo.gitDir, data: blobData)
        let tagData = serializeTag(
            targetId: blobOID,
            targetType: .blob,
            tagName: "v1.0",
            tagger: nil,
            message: "annotated blob tag\n"
        )
        let tagOID = try writeLooseObject(gitDir: repo.gitDir, type: .tag, data: tagData)

        let tagObject = try readObject(gitDir: repo.gitDir, oid: tagOID)
        let tag = try tagObject.asTag()
        XCTAssertEqual(tag.targetId, blobOID)
        XCTAssertEqual(tag.targetType, .blob)

        let peeled = try tagObject.peel(gitDir: repo.gitDir)
        XCTAssertEqual(peeled.oid, blobOID)
        XCTAssertEqual(peeled.objectType, .blob)
        XCTAssertEqual(peeled.data, blobData)
    }

    private func testDirectory(_ name: String) -> String {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("tmp/\(name)")
            .path
    }
}
