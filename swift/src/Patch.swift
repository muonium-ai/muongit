/// Structured unified patch generation, parsing, and worktree apply.
import Foundation

public enum PatchFileStatus: Sendable {
    case added
    case deleted
    case modified
}

public enum PatchLineKind: Sendable {
    case context
    case add
    case delete
}

public struct PatchLine: Sendable, Equatable {
    public let kind: PatchLineKind
    public let text: String

    public init(kind: PatchLineKind, text: String) {
        self.kind = kind
        self.text = text
    }
}

public struct PatchHunk: Sendable, Equatable {
    public let oldStart: Int
    public let oldCount: Int
    public let newStart: Int
    public let newCount: Int
    public let lines: [PatchLine]

    public init(oldStart: Int, oldCount: Int, newStart: Int, newCount: Int, lines: [PatchLine]) {
        self.oldStart = oldStart
        self.oldCount = oldCount
        self.newStart = newStart
        self.newCount = newCount
        self.lines = lines
    }
}

public struct PatchFile: Sendable, Equatable {
    public let oldPath: String?
    public let newPath: String?
    public let status: PatchFileStatus
    public let hunks: [PatchHunk]

    public init(oldPath: String?, newPath: String?, status: PatchFileStatus, hunks: [PatchHunk]) {
        self.oldPath = oldPath
        self.newPath = newPath
        self.status = status
        self.hunks = hunks
    }

    public var path: String {
        newPath ?? oldPath ?? ""
    }

    public static func fromText(
        oldPath: String?,
        newPath: String?,
        oldText: String,
        newText: String,
        context: Int = 3
    ) -> PatchFile {
        let status: PatchFileStatus
        switch (oldPath, newPath) {
        case (nil, .some): status = .added
        case (.some, nil): status = .deleted
        default: status = .modified
        }

        let hunks = makeHunks(edits: diffLines(oldText: oldText, newText: newText), context: context).map { hunk in
            PatchHunk(
                oldStart: hunk.oldStart,
                oldCount: hunk.oldCount,
                newStart: hunk.newStart,
                newCount: hunk.newCount,
                lines: hunk.edits.map { edit in
                    let patchKind: PatchLineKind
                    switch edit.kind {
                    case .equal: patchKind = .context
                    case .insert: patchKind = .add
                    case .delete: patchKind = .delete
                    }
                    return PatchLine(
                        kind: patchKind,
                        text: edit.text
                    )
                }
            )
        }

        return PatchFile(oldPath: oldPath, newPath: newPath, status: status, hunks: hunks)
    }
}

public struct Patch: Sendable, Equatable {
    public let files: [PatchFile]

    public init(files: [PatchFile]) {
        self.files = files
    }

    public static func parse(_ text: String) throws -> Patch {
        try parsePatch(text)
    }

    public static func fromText(
        oldPath: String?,
        newPath: String?,
        oldText: String,
        newText: String,
        context: Int = 3
    ) -> Patch {
        Patch(files: [PatchFile.fromText(
            oldPath: oldPath,
            newPath: newPath,
            oldText: oldText,
            newText: newText,
            context: context
        )])
    }

    public func format() -> String {
        formatPatch(self)
    }
}

public struct PatchReject: Sendable, Equatable {
    public let oldStart: Int
    public let newStart: Int
    public let reason: String

    public init(oldStart: Int, newStart: Int, reason: String) {
        self.oldStart = oldStart
        self.newStart = newStart
        self.reason = reason
    }
}

public struct PatchFileApplyResult: Sendable, Equatable {
    public let path: String
    public let applied: Bool
    public let rejectedHunks: [PatchReject]

    public init(path: String, applied: Bool, rejectedHunks: [PatchReject]) {
        self.path = path
        self.applied = applied
        self.rejectedHunks = rejectedHunks
    }
}

public struct PatchApplyResult: Sendable, Equatable {
    public let files: [PatchFileApplyResult]
    public let hasRejects: Bool

    public init(files: [PatchFileApplyResult], hasRejects: Bool) {
        self.files = files
        self.hasRejects = hasRejects
    }
}

public extension Repository {
    func applyPatch(_ patch: Patch) throws -> PatchApplyResult {
        guard let workdir else {
            throw MuonGitError.bareRepo
        }
        return try applyPatchToWorkdir(workdir, patch: patch)
    }
}

public func parsePatch(_ text: String) throws -> Patch {
    let lines = text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
    var files: [PatchFile] = []
    var index = 0

    while index < lines.count {
        if lines[index].isEmpty {
            index += 1
            continue
        }
        let oldHeader = lines[index]
        guard oldHeader.hasPrefix("--- ") else {
            throw MuonGitError.invalidSpec("expected file header at line \(index + 1)")
        }
        index += 1
        guard index < lines.count, lines[index].hasPrefix("+++ ") else {
            throw MuonGitError.invalidSpec("missing new-file header after line \(index)")
        }

        let oldPath = parsePatchPath(String(oldHeader.dropFirst(4)))
        let newPath = parsePatchPath(String(lines[index].dropFirst(4)))
        index += 1

        let status: PatchFileStatus
        switch (oldPath, newPath) {
        case (nil, .some): status = .added
        case (.some, nil): status = .deleted
        default: status = .modified
        }

        var hunks: [PatchHunk] = []
        while index < lines.count, lines[index].hasPrefix("@@ ") {
            let (oldStart, oldCount, newStart, newCount) = try parseHunkHeader(lines[index])
            index += 1

            var oldSeen = 0
            var newSeen = 0
            var patchLines: [PatchLine] = []

            while oldSeen < oldCount || newSeen < newCount {
                guard index < lines.count else {
                    throw MuonGitError.invalidSpec("unexpected end of patch while reading hunk")
                }
                let line = lines[index]
                if line == #"\\ No newline at end of file"# || line == #"\ No newline at end of file"# {
                    index += 1
                    continue
                }

                guard let marker = line.first else {
                    throw MuonGitError.invalidSpec("empty hunk line")
                }
                let text = String(line.dropFirst())
                switch marker {
                case " ":
                    oldSeen += 1
                    newSeen += 1
                    patchLines.append(PatchLine(kind: .context, text: text))
                case "-":
                    oldSeen += 1
                    patchLines.append(PatchLine(kind: .delete, text: text))
                case "+":
                    newSeen += 1
                    patchLines.append(PatchLine(kind: .add, text: text))
                default:
                    throw MuonGitError.invalidSpec("unsupported hunk marker '\(marker)' at line \(index + 1)")
                }
                index += 1
            }

            hunks.append(PatchHunk(
                oldStart: oldStart,
                oldCount: oldCount,
                newStart: newStart,
                newCount: newCount,
                lines: patchLines
            ))
        }

        files.append(PatchFile(oldPath: oldPath, newPath: newPath, status: status, hunks: hunks))
    }

    return Patch(files: files)
}

public func formatPatch(_ patch: Patch) -> String {
    patch.files.compactMap { file in
        guard !file.hunks.isEmpty else { return nil }

        let oldHeader = file.oldPath.map { "a/\($0)" } ?? "/dev/null"
        let newHeader = file.newPath.map { "b/\($0)" } ?? "/dev/null"

        var out = "--- \(oldHeader)\n+++ \(newHeader)\n"
        for hunk in file.hunks {
            out += "@@ -\(hunk.oldStart),\(hunk.oldCount) +\(hunk.newStart),\(hunk.newCount) @@\n"
            for line in hunk.lines {
                let marker: Character
                switch line.kind {
                case .context: marker = " "
                case .add: marker = "+"
                case .delete: marker = "-"
                }
                out.append(marker)
                out += line.text
                out += "\n"
            }
        }
        return out
    }.joined()
}

public func applyPatchToWorkdir(_ workdir: String, patch: Patch) throws -> PatchApplyResult {
    let fm = FileManager.default
    var fileResults: [PatchFileApplyResult] = []
    var hasRejects = false

    for file in patch.files {
        let relativePath = file.path
        let targetPath = (workdir as NSString).appendingPathComponent(relativePath)
        var rejects: [PatchReject] = []

        let original: String
        switch file.status {
        case .added:
            if fm.fileExists(atPath: targetPath) {
                rejects.append(fileLevelReject("target file already exists"))
            }
            original = ""
        case .deleted, .modified:
            guard fm.fileExists(atPath: targetPath) else {
                rejects.append(fileLevelReject("target file does not exist"))
                original = ""
                fileResults.append(PatchFileApplyResult(path: relativePath, applied: false, rejectedHunks: rejects))
                hasRejects = true
                continue
            }
            original = try String(contentsOfFile: targetPath, encoding: .utf8)
        }

        if rejects.isEmpty {
            switch applyPatchFileToText(original: original, file: file) {
            case (let updated?, _):
                switch file.status {
                case .deleted:
                    if !updated.isEmpty {
                        rejects.append(fileLevelReject("delete patch did not consume full file content"))
                    } else {
                        try fm.removeItem(atPath: targetPath)
                    }
                case .added, .modified:
                    let parent = (targetPath as NSString).deletingLastPathComponent
                    try fm.createDirectory(atPath: parent, withIntermediateDirectories: true)
                    try updated.write(toFile: targetPath, atomically: true, encoding: .utf8)
                }
            case (_, let hunkRejects):
                rejects.append(contentsOf: hunkRejects)
            }
        }

        if !rejects.isEmpty {
            hasRejects = true
        }
        fileResults.append(PatchFileApplyResult(path: relativePath, applied: rejects.isEmpty, rejectedHunks: rejects))
    }

    return PatchApplyResult(files: fileResults, hasRejects: hasRejects)
}

private func applyPatchFileToText(original: String, file: PatchFile) -> (String?, [PatchReject]) {
    var lines = splitPatchText(original)
    var offset = 0
    var rejects: [PatchReject] = []

    for hunk in file.hunks {
        let expectedOld = hunk.lines
            .filter { $0.kind != .add }
            .map(\.text)
        let replacement = hunk.lines
            .filter { $0.kind != .delete }
            .map(\.text)
        let baseIndex = max(0, hunk.oldStart - 1 + offset)

        if !matchesPatchSlice(lines: lines, index: baseIndex, expected: expectedOld) {
            rejects.append(PatchReject(oldStart: hunk.oldStart, newStart: hunk.newStart, reason: "hunk context mismatch"))
            continue
        }

        lines.replaceSubrange(baseIndex..<(baseIndex + expectedOld.count), with: replacement)
        offset += replacement.count - expectedOld.count
    }

    return rejects.isEmpty ? (joinPatchText(lines), []) : (nil, rejects)
}

private func splitPatchText(_ text: String) -> [String] {
    if text.isEmpty { return [] }
    return text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
}

private func joinPatchText(_ lines: [String]) -> String {
    guard !lines.isEmpty else { return "" }
    return lines.joined(separator: "\n")
}

private func matchesPatchSlice(lines: [String], index: Int, expected: [String]) -> Bool {
    guard index >= 0, index + expected.count <= lines.count else {
        return false
    }
    return Array(lines[index..<(index + expected.count)]) == expected
}

private func parsePatchPath(_ raw: String) -> String? {
    let token = raw.split(separator: " ", maxSplits: 1).first.map(String.init) ?? raw
    if token == "/dev/null" {
        return nil
    }
    if token.hasPrefix("a/") || token.hasPrefix("b/") {
        return String(token.dropFirst(2))
    }
    return token
}

private func parseHunkHeader(_ line: String) throws -> (Int, Int, Int, Int) {
    guard line.hasPrefix("@@ -"), line.hasSuffix(" @@") else {
        throw MuonGitError.invalidSpec("invalid hunk header '\(line)'")
    }
    let trimmed = String(line.dropFirst(4).dropLast(3))
    guard let (oldPart, newPart) = trimmed.split(separator: " ", maxSplits: 1).map(String.init).asPair,
          newPart.hasPrefix("+") else {
        throw MuonGitError.invalidSpec("invalid hunk header '\(line)'")
    }
    let oldRange = try parseHunkRange(oldPart)
    let newRange = try parseHunkRange(String(newPart.dropFirst()))
    return (oldRange.0, oldRange.1, newRange.0, newRange.1)
}

private func parseHunkRange(_ spec: String) throws -> (Int, Int) {
    if let (start, count) = spec.split(separator: ",", maxSplits: 1).map(String.init).asPair {
        guard let parsedStart = Int(start), let parsedCount = Int(count) else {
            throw MuonGitError.invalidSpec("invalid range '\(spec)'")
        }
        return (parsedStart, parsedCount)
    }

    guard let parsedStart = Int(spec) else {
        throw MuonGitError.invalidSpec("invalid range '\(spec)'")
    }
    return (parsedStart, 1)
}

private func fileLevelReject(_ reason: String) -> PatchReject {
    PatchReject(oldStart: 0, newStart: 0, reason: reason)
}

private extension Array {
    var asPair: (Element, Element)? {
        count == 2 ? (self[0], self[1]) : nil
    }
}
