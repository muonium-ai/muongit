package ai.muonium.muongit

import java.io.File

enum class PatchFileStatus {
    ADDED,
    DELETED,
    MODIFIED,
}

enum class PatchLineKind {
    CONTEXT,
    ADD,
    DELETE,
}

data class PatchLine(
    val kind: PatchLineKind,
    val text: String,
)

data class PatchHunk(
    val oldStart: Int,
    val oldCount: Int,
    val newStart: Int,
    val newCount: Int,
    val lines: List<PatchLine>,
)

data class PatchFile(
    val oldPath: String?,
    val newPath: String?,
    val status: PatchFileStatus,
    val hunks: List<PatchHunk>,
) {
    val path: String get() = newPath ?: oldPath ?: ""

    companion object {
        fun fromText(
            oldPath: String?,
            newPath: String?,
            oldText: String,
            newText: String,
            context: Int = 3,
        ): PatchFile {
            val status = when {
                oldPath == null && newPath != null -> PatchFileStatus.ADDED
                oldPath != null && newPath == null -> PatchFileStatus.DELETED
                else -> PatchFileStatus.MODIFIED
            }

            val hunks = makeHunks(diffLines(oldText, newText), context).map { hunk ->
                PatchHunk(
                    oldStart = hunk.oldStart,
                    oldCount = hunk.oldCount,
                    newStart = hunk.newStart,
                    newCount = hunk.newCount,
                    lines = hunk.edits.map { edit ->
                        PatchLine(
                            kind = when (edit.kind) {
                                EditKind.EQUAL -> PatchLineKind.CONTEXT
                                EditKind.INSERT -> PatchLineKind.ADD
                                EditKind.DELETE -> PatchLineKind.DELETE
                            },
                            text = edit.text,
                        )
                    },
                )
            }

            return PatchFile(oldPath = oldPath, newPath = newPath, status = status, hunks = hunks)
        }
    }
}

data class Patch(
    val files: List<PatchFile>,
) {
    fun format(): String = formatPatch(this)

    companion object {
        fun parse(text: String): Patch = parsePatch(text)

        fun fromText(
            oldPath: String?,
            newPath: String?,
            oldText: String,
            newText: String,
            context: Int = 3,
        ): Patch = Patch(
            files = listOf(PatchFile.fromText(oldPath, newPath, oldText, newText, context))
        )
    }
}

data class PatchReject(
    val oldStart: Int,
    val newStart: Int,
    val reason: String,
)

data class PatchFileApplyResult(
    val path: String,
    val applied: Boolean,
    val rejectedHunks: List<PatchReject>,
)

data class PatchApplyResult(
    val files: List<PatchFileApplyResult>,
    val hasRejects: Boolean,
)

fun Repository.applyPatch(patch: Patch): PatchApplyResult {
    val workdir = workdir ?: throw MuonGitException.BareRepo()
    return applyPatch(workdir, patch)
}

fun parsePatch(text: String): Patch {
    val lines = text.lineSequence().toList()
    val files = mutableListOf<PatchFile>()
    var index = 0

    while (index < lines.size) {
        if (lines[index].isEmpty()) {
            index += 1
            continue
        }
        val oldHeader = lines[index]
        if (!oldHeader.startsWith("--- ")) {
            throw MuonGitException.InvalidSpec("expected file header at line ${index + 1}")
        }
        index += 1
        if (index >= lines.size || !lines[index].startsWith("+++ ")) {
            throw MuonGitException.InvalidSpec("missing new-file header after line $index")
        }

        val oldPath = parsePatchPath(oldHeader.removePrefix("--- "))
        val newPath = parsePatchPath(lines[index].removePrefix("+++ "))
        index += 1

        val status = when {
            oldPath == null && newPath != null -> PatchFileStatus.ADDED
            oldPath != null && newPath == null -> PatchFileStatus.DELETED
            else -> PatchFileStatus.MODIFIED
        }

        val hunks = mutableListOf<PatchHunk>()
        while (index < lines.size && lines[index].startsWith("@@ ")) {
            val (oldStart, oldCount, newStart, newCount) = parseHunkHeader(lines[index])
            index += 1

            var oldSeen = 0
            var newSeen = 0
            val patchLines = mutableListOf<PatchLine>()

            while (oldSeen < oldCount || newSeen < newCount) {
                if (index >= lines.size) {
                    throw MuonGitException.InvalidSpec("unexpected end of patch while reading hunk")
                }
                val line = lines[index]
                if (line == "\\ No newline at end of file") {
                    index += 1
                    continue
                }

                val marker = line.firstOrNull()
                    ?: throw MuonGitException.InvalidSpec("empty hunk line")
                val payload = line.drop(1)
                when (marker) {
                    ' ' -> {
                        oldSeen += 1
                        newSeen += 1
                        patchLines += PatchLine(PatchLineKind.CONTEXT, payload)
                    }
                    '-' -> {
                        oldSeen += 1
                        patchLines += PatchLine(PatchLineKind.DELETE, payload)
                    }
                    '+' -> {
                        newSeen += 1
                        patchLines += PatchLine(PatchLineKind.ADD, payload)
                    }
                    else -> throw MuonGitException.InvalidSpec(
                        "unsupported hunk marker '$marker' at line ${index + 1}"
                    )
                }
                index += 1
            }

            hunks += PatchHunk(
                oldStart = oldStart,
                oldCount = oldCount,
                newStart = newStart,
                newCount = newCount,
                lines = patchLines,
            )
        }

        files += PatchFile(oldPath = oldPath, newPath = newPath, status = status, hunks = hunks)
    }

    return Patch(files)
}

fun formatPatch(patch: Patch): String = buildString {
    for (file in patch.files) {
        if (file.hunks.isEmpty()) continue
        val oldHeader = file.oldPath?.let { "a/$it" } ?: "/dev/null"
        val newHeader = file.newPath?.let { "b/$it" } ?: "/dev/null"
        append("--- $oldHeader\n")
        append("+++ $newHeader\n")
        for (hunk in file.hunks) {
            append("@@ -${hunk.oldStart},${hunk.oldCount} +${hunk.newStart},${hunk.newCount} @@\n")
            for (line in hunk.lines) {
                val marker = when (line.kind) {
                    PatchLineKind.CONTEXT -> ' '
                    PatchLineKind.ADD -> '+'
                    PatchLineKind.DELETE -> '-'
                }
                append(marker)
                append(line.text)
                append('\n')
            }
        }
    }
}

fun applyPatch(workdir: File, patch: Patch): PatchApplyResult {
    val fileResults = mutableListOf<PatchFileApplyResult>()
    var hasRejects = false

    for (file in patch.files) {
        val relativePath = file.path
        val targetFile = File(workdir, relativePath)
        val rejects = mutableListOf<PatchReject>()

        val original = when (file.status) {
            PatchFileStatus.ADDED -> {
                if (targetFile.exists()) {
                    rejects += fileLevelReject("target file already exists")
                }
                ""
            }
            PatchFileStatus.DELETED,
            PatchFileStatus.MODIFIED -> {
                if (!targetFile.exists()) {
                    rejects += fileLevelReject("target file does not exist")
                    ""
                } else {
                    targetFile.readText()
                }
            }
        }

        if (rejects.isEmpty()) {
            val (updated, hunkRejects) = applyPatchFileToText(original, file)
            if (hunkRejects.isEmpty()) {
                when (file.status) {
                    PatchFileStatus.DELETED -> {
                        if (!updated.isNullOrEmpty()) {
                            rejects += fileLevelReject("delete patch did not consume full file content")
                        } else {
                            targetFile.delete()
                        }
                    }
                    PatchFileStatus.ADDED,
                    PatchFileStatus.MODIFIED -> {
                        targetFile.parentFile?.mkdirs()
                        targetFile.writeText(updated ?: "")
                    }
                }
            } else {
                rejects += hunkRejects
            }
        }

        if (rejects.isNotEmpty()) {
            hasRejects = true
        }
        fileResults += PatchFileApplyResult(
            path = relativePath,
            applied = rejects.isEmpty(),
            rejectedHunks = rejects,
        )
    }

    return PatchApplyResult(files = fileResults, hasRejects = hasRejects)
}

private fun applyPatchFileToText(original: String, file: PatchFile): Pair<String?, List<PatchReject>> {
    val lines = splitPatchText(original).toMutableList()
    var offset = 0
    val rejects = mutableListOf<PatchReject>()

    for (hunk in file.hunks) {
        val expectedOld = hunk.lines.filter { it.kind != PatchLineKind.ADD }.map { it.text }
        val replacement = hunk.lines.filter { it.kind != PatchLineKind.DELETE }.map { it.text }
        val baseIndex = maxOf(0, hunk.oldStart - 1 + offset)

        if (!matchesPatchSlice(lines, baseIndex, expectedOld)) {
            rejects += PatchReject(hunk.oldStart, hunk.newStart, "hunk context mismatch")
            continue
        }

        repeat(expectedOld.size) {
            lines.removeAt(baseIndex)
        }
        lines.addAll(baseIndex, replacement)
        offset += replacement.size - expectedOld.size
    }

    return if (rejects.isEmpty()) {
        joinPatchText(lines) to emptyList()
    } else {
        null to rejects
    }
}

private fun splitPatchText(text: String): List<String> {
    if (text.isEmpty()) return emptyList()
    val parts = mutableListOf<String>()
    var start = 0
    for (i in text.indices) {
        if (text[i] == '\n') {
            parts += text.substring(start, i)
            start = i + 1
        }
    }
    parts += text.substring(start)
    return parts
}

private fun joinPatchText(lines: List<String>): String {
    if (lines.isEmpty()) return ""
    return lines.joinToString("\n")
}

private fun matchesPatchSlice(lines: List<String>, index: Int, expected: List<String>): Boolean {
    if (index < 0 || index + expected.size > lines.size) return false
    return lines.subList(index, index + expected.size) == expected
}

private fun parsePatchPath(raw: String): String? {
    val token = raw.substringBefore(' ')
    return when {
        token == "/dev/null" -> null
        token.startsWith("a/") || token.startsWith("b/") -> token.drop(2)
        else -> token
    }
}

private fun parseHunkHeader(line: String): List<Int> {
    if (!line.startsWith("@@ -") || !line.endsWith(" @@")) {
        throw MuonGitException.InvalidSpec("invalid hunk header '$line'")
    }
    val trimmed = line.removePrefix("@@ -").removeSuffix(" @@")
    val parts = trimmed.split(" ", limit = 2)
    if (parts.size != 2 || !parts[1].startsWith("+")) {
        throw MuonGitException.InvalidSpec("invalid hunk header '$line'")
    }

    val (oldStart, oldCount) = parseHunkRange(parts[0])
    val (newStart, newCount) = parseHunkRange(parts[1].drop(1))
    return listOf(oldStart, oldCount, newStart, newCount)
}

private fun parseHunkRange(spec: String): Pair<Int, Int> {
    val parts = spec.split(",", limit = 2)
    return if (parts.size == 2) {
        val start = parts[0].toIntOrNull()
            ?: throw MuonGitException.InvalidSpec("invalid range '$spec'")
        val count = parts[1].toIntOrNull()
            ?: throw MuonGitException.InvalidSpec("invalid range '$spec'")
        start to count
    } else {
        val start = spec.toIntOrNull()
            ?: throw MuonGitException.InvalidSpec("invalid range '$spec'")
        start to 1
    }
}

private fun fileLevelReject(reason: String): PatchReject =
    PatchReject(oldStart = 0, newStart = 0, reason = reason)
