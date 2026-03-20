package ai.muonium.muongit

import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class PatchTest {
    @Test
    fun testPatchRoundtripParseAndFormat() {
        val patch = Patch.fromText(
            oldPath = "file.txt",
            newPath = "file.txt",
            oldText = "line1\nline2\n",
            newText = "line1\nline2 changed\nline3\n",
        )

        val text = patch.format()
        assertEquals(patch, Patch.parse(text))
    }

    @Test
    fun testApplyPatchModifiesExistingFile() {
        val tmp = testDirectory("kotlin_patch_modify")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val file = File(repo.workdir!!, "file.txt")
            file.writeText("line1\nline2\n")

            val patch = Patch.fromText(
                oldPath = "file.txt",
                newPath = "file.txt",
                oldText = "line1\nline2\n",
                newText = "line1\nline2 changed\nline3\n",
            )

            val result = repo.applyPatch(patch)
            assertFalse(result.hasRejects)
            assertEquals("line1\nline2 changed\nline3\n", file.readText())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testApplyPatchAddsNewFile() {
        val tmp = testDirectory("kotlin_patch_add")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val patch = Patch.fromText(
                oldPath = null,
                newPath = "nested/new.txt",
                oldText = "",
                newText = "hello\nworld\n",
            )

            val result = repo.applyPatch(patch)
            val file = File(repo.workdir!!, "nested/new.txt")
            assertFalse(result.hasRejects)
            assertEquals("hello\nworld\n", file.readText())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testApplyPatchDeletesFile() {
        val tmp = testDirectory("kotlin_patch_delete")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val file = File(repo.workdir!!, "gone.txt")
            file.writeText("goodbye\nworld\n")

            val patch = Patch.fromText(
                oldPath = "gone.txt",
                newPath = null,
                oldText = "goodbye\nworld\n",
                newText = "",
            )

            val result = repo.applyPatch(patch)
            assertFalse(result.hasRejects)
            assertFalse(file.exists())
        } finally {
            tmp.deleteRecursively()
        }
    }

    @Test
    fun testApplyPatchRejectsContextMismatch() {
        val tmp = testDirectory("kotlin_patch_reject")
        tmp.deleteRecursively()
        try {
            val repo = Repository.init(tmp.path)
            val file = File(repo.workdir!!, "file.txt")
            file.writeText("line1\nDIFFERENT\n")

            val patch = Patch.fromText(
                oldPath = "file.txt",
                newPath = "file.txt",
                oldText = "line1\nline2\n",
                newText = "line1\nline2 changed\n",
            )

            val result = repo.applyPatch(patch)
            assertTrue(result.hasRejects)
            assertFalse(result.files[0].applied)
            assertEquals("hunk context mismatch", result.files[0].rejectedHunks[0].reason)
            assertEquals("line1\nDIFFERENT\n", file.readText())
        } finally {
            tmp.deleteRecursively()
        }
    }

    private fun testDirectory(name: String): File =
        File(System.getProperty("user.dir")).resolve("../tmp/$name")
}
