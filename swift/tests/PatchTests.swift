import XCTest
@testable import MuonGit

final class PatchTests: XCTestCase {
    func testPatchRoundtripParseAndFormat() throws {
        let patch = Patch.fromText(
            oldPath: "file.txt",
            newPath: "file.txt",
            oldText: "line1\nline2\n",
            newText: "line1\nline2 changed\nline3\n"
        )

        let text = patch.format()
        XCTAssertEqual(try Patch.parse(text), patch)
    }

    func testApplyPatchModifiesExistingFile() throws {
        let tmp = testDirectory("swift_patch_modify")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let path = (repo.workdir! as NSString).appendingPathComponent("file.txt")
        try "line1\nline2\n".write(toFile: path, atomically: true, encoding: .utf8)

        let patch = Patch.fromText(
            oldPath: "file.txt",
            newPath: "file.txt",
            oldText: "line1\nline2\n",
            newText: "line1\nline2 changed\nline3\n"
        )

        let result = try repo.applyPatch(patch)
        XCTAssertFalse(result.hasRejects)
        XCTAssertEqual(try String(contentsOfFile: path, encoding: .utf8), "line1\nline2 changed\nline3\n")
    }

    func testApplyPatchAddsNewFile() throws {
        let tmp = testDirectory("swift_patch_add")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let patch = Patch.fromText(
            oldPath: nil,
            newPath: "nested/new.txt",
            oldText: "",
            newText: "hello\nworld\n"
        )

        let result = try repo.applyPatch(patch)
        let path = (repo.workdir! as NSString).appendingPathComponent("nested/new.txt")
        XCTAssertFalse(result.hasRejects)
        XCTAssertEqual(try String(contentsOfFile: path, encoding: .utf8), "hello\nworld\n")
    }

    func testApplyPatchDeletesFile() throws {
        let tmp = testDirectory("swift_patch_delete")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let path = (repo.workdir! as NSString).appendingPathComponent("gone.txt")
        try "goodbye\nworld\n".write(toFile: path, atomically: true, encoding: .utf8)

        let patch = Patch.fromText(
            oldPath: "gone.txt",
            newPath: nil,
            oldText: "goodbye\nworld\n",
            newText: ""
        )

        let result = try repo.applyPatch(patch)
        XCTAssertFalse(result.hasRejects)
        XCTAssertFalse(FileManager.default.fileExists(atPath: path))
    }

    func testApplyPatchRejectsContextMismatch() throws {
        let tmp = testDirectory("swift_patch_reject")
        try? FileManager.default.removeItem(atPath: tmp)
        defer { try? FileManager.default.removeItem(atPath: tmp) }

        let repo = try Repository.create(at: tmp)
        let path = (repo.workdir! as NSString).appendingPathComponent("file.txt")
        try "line1\nDIFFERENT\n".write(toFile: path, atomically: true, encoding: .utf8)

        let patch = Patch.fromText(
            oldPath: "file.txt",
            newPath: "file.txt",
            oldText: "line1\nline2\n",
            newText: "line1\nline2 changed\n"
        )

        let result = try repo.applyPatch(patch)
        XCTAssertTrue(result.hasRejects)
        XCTAssertFalse(result.files[0].applied)
        XCTAssertEqual(result.files[0].rejectedHunks[0].reason, "hunk context mismatch")
        XCTAssertEqual(try String(contentsOfFile: path, encoding: .utf8), "line1\nDIFFERENT\n")
    }

    private func testDirectory(_ name: String) -> String {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("tmp/\(name)")
            .path
    }
}
