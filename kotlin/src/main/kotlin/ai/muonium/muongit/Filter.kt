// Filter.kt - Clean/smudge filter system
// Parity: libgit2 src/libgit2/filter.c, crlf.c, ident.c

package ai.muonium.muongit

import java.io.File

/** Direction of filtering. Parity: git_filter_mode_t */
enum class FilterMode {
    /** Working directory → ODB (clean) */
    TO_ODB,
    /** ODB → working directory (smudge) */
    TO_WORKTREE
}

/** Metadata about the file being filtered. Parity: git_filter_source */
data class FilterSource(
    val path: String,
    val mode: FilterMode,
    val oid: OID? = null
)

/** Result of applying a filter. */
sealed class FilterResult {
    data class Applied(val data: ByteArray) : FilterResult()
    data object Passthrough : FilterResult()
}

/** A single filter implementation. */
interface Filter {
    val name: String
    fun check(source: FilterSource, attrs: Attributes): Boolean
    fun apply(input: ByteArray, source: FilterSource): FilterResult
}

/** A chain of filters to apply to a file. Parity: git_filter_list */
class FilterList(val source: FilterSource) {
    private val filters = mutableListOf<Filter>()

    companion object {
        /** Load the applicable filters for a path in a repository. Parity: git_filter_list_load */
        fun load(
            gitDir: File,
            workdir: File?,
            path: String,
            mode: FilterMode,
            oid: OID? = null
        ): FilterList {
            val attrs = Attributes.loadForRepo(gitDir, workdir)
            val source = FilterSource(path = path, mode = mode, oid = oid)
            val list = FilterList(source)

            val crlf = CrlfFilter(gitDir)
            val ident = IdentFilter()

            when (mode) {
                FilterMode.TO_WORKTREE -> {
                    // Smudge: CRLF(0) → Ident(100)
                    if (crlf.check(source, attrs)) list.filters.add(crlf)
                    if (ident.check(source, attrs)) list.filters.add(ident)
                }
                FilterMode.TO_ODB -> {
                    // Clean: Ident(100) → CRLF(0) (reverse order)
                    if (ident.check(source, attrs)) list.filters.add(ident)
                    if (crlf.check(source, attrs)) list.filters.add(crlf)
                }
            }

            return list
        }
    }

    /** Apply all filters in the chain to the input data. Parity: git_filter_list_apply_to_buffer */
    fun apply(input: ByteArray): ByteArray {
        var data = input
        for (filter in filters) {
            when (val result = filter.apply(data, source)) {
                is FilterResult.Applied -> data = result.data
                is FilterResult.Passthrough -> {}
            }
        }
        return data
    }

    /** Number of active filters. */
    val size: Int get() = filters.size

    /** Whether the filter list is empty. */
    val isEmpty: Boolean get() = filters.isEmpty()

    /** Check if a named filter is in the list. */
    fun contains(name: String): Boolean = filters.any { it.name == name }
}

// ── CRLF Filter (priority 0) ──
// Parity: libgit2 src/libgit2/crlf.c

private enum class CrlfAction { NONE, CRLF_TO_LF, LF_TO_CRLF, AUTO }
private enum class EolStyle { LF, CRLF, NATIVE }

/** CRLF / text / eol filter. */
class CrlfFilter : Filter {
    private val autoCrlf: String?
    private val coreEol: String?

    constructor(gitDir: File) {
        val configFile = File(gitDir, "config")
        val config = if (configFile.exists()) Config.load(configFile.absolutePath) else Config()
        this.autoCrlf = config.get("core", "autocrlf")
        this.coreEol = config.get("core", "eol")
    }

    internal constructor(autoCrlf: String?, coreEol: String?) {
        this.autoCrlf = autoCrlf
        this.coreEol = coreEol
    }

    override val name: String = "crlf"

    override fun check(source: FilterSource, attrs: Attributes): Boolean {
        val action = resolveAction(attrs, source.path, source.mode)
        return action != CrlfAction.NONE
    }

    override fun apply(input: ByteArray, source: FilterSource): FilterResult {
        if (isBinaryData(input)) return FilterResult.Passthrough
        return when (source.mode) {
            FilterMode.TO_ODB -> crlfToLf(input)
            FilterMode.TO_WORKTREE -> lfToCrlf(input)
        }
    }

    private fun resolveAction(attrs: Attributes, path: String, mode: FilterMode): CrlfAction {
        val textAttr = attrs.get(path, "text")
        val crlfAttr = attrs.get(path, "crlf")
        val eolAttr = attrs.get(path, "eol")

        // text attribute takes priority
        var isText: Boolean? = when (textAttr) {
            AttrValue.Set -> true
            AttrValue.Unset -> false
            is AttrValue.Value -> if (textAttr.value == "auto") return CrlfAction.AUTO else null
            else -> null
        }

        // Fall back to crlf attribute
        if (isText == null) {
            isText = when (crlfAttr) {
                AttrValue.Set -> true
                AttrValue.Unset -> false
                else -> null
            }
        }

        // Fall back to core.autocrlf config
        if (isText == null) {
            isText = when (autoCrlf) {
                "true", "input" -> true
                else -> null
            }
        }

        if (isText != true) return CrlfAction.NONE

        val outputEol = resolveEol(eolAttr, mode)

        return when (mode) {
            FilterMode.TO_ODB -> CrlfAction.CRLF_TO_LF
            FilterMode.TO_WORKTREE -> when (outputEol) {
                EolStyle.CRLF -> CrlfAction.LF_TO_CRLF
                EolStyle.LF -> CrlfAction.NONE
                EolStyle.NATIVE -> {
                    if (System.getProperty("os.name").lowercase().contains("win"))
                        CrlfAction.LF_TO_CRLF
                    else
                        CrlfAction.NONE
                }
            }
        }
    }

    private fun resolveEol(eolAttr: AttrValue?, mode: FilterMode): EolStyle {
        if (eolAttr is AttrValue.Value) {
            return when (eolAttr.value) {
                "lf" -> EolStyle.LF
                "crlf" -> EolStyle.CRLF
                else -> EolStyle.NATIVE
            }
        }

        if (mode == FilterMode.TO_ODB && autoCrlf == "input") {
            return EolStyle.LF
        }

        return when (coreEol) {
            "lf" -> EolStyle.LF
            "crlf" -> EolStyle.CRLF
            else -> EolStyle.NATIVE
        }
    }
}

// ── Ident Filter (priority 100) ──
// Parity: libgit2 src/libgit2/ident.c

/** $Id$ expansion/contraction filter. */
class IdentFilter : Filter {
    override val name: String = "ident"

    override fun check(source: FilterSource, attrs: Attributes): Boolean {
        return attrs.get(source.path, "ident") == AttrValue.Set
    }

    override fun apply(input: ByteArray, source: FilterSource): FilterResult {
        if (isBinaryData(input)) return FilterResult.Passthrough
        return when (source.mode) {
            FilterMode.TO_WORKTREE -> identSmudge(input, source.oid)
            FilterMode.TO_ODB -> identClean(input)
        }
    }
}

// ── Internal Helpers ──

/** Convert CRLF to LF (clean direction). */
internal fun crlfToLf(input: ByteArray): FilterResult {
    var hasCrlf = false
    for (i in 0 until input.size - 1) {
        if (input[i] == 0x0D.toByte() && input[i + 1] == 0x0A.toByte()) {
            hasCrlf = true
            break
        }
    }
    if (!hasCrlf) return FilterResult.Passthrough

    val output = mutableListOf<Byte>()
    var i = 0
    while (i < input.size) {
        if (i + 1 < input.size && input[i] == 0x0D.toByte() && input[i + 1] == 0x0A.toByte()) {
            output.add(0x0A.toByte())
            i += 2
        } else {
            output.add(input[i])
            i++
        }
    }
    return FilterResult.Applied(output.toByteArray())
}

/** Convert LF to CRLF (smudge direction). */
internal fun lfToCrlf(input: ByteArray): FilterResult {
    var hasBareLf = false
    for (i in input.indices) {
        if (input[i] == 0x0A.toByte() && (i == 0 || input[i - 1] != 0x0D.toByte())) {
            hasBareLf = true
            break
        }
    }
    if (!hasBareLf) return FilterResult.Passthrough

    val output = mutableListOf<Byte>()
    for (i in input.indices) {
        if (input[i] == 0x0A.toByte() && (i == 0 || input[i - 1] != 0x0D.toByte())) {
            output.add(0x0D.toByte())
        }
        output.add(input[i])
    }
    return FilterResult.Applied(output.toByteArray())
}

/** Simple binary detection: check for NUL bytes in the first 8000 bytes. */
internal fun isBinaryData(data: ByteArray): Boolean {
    val checkLen = minOf(data.size, 8000)
    for (i in 0 until checkLen) {
        if (data[i] == 0.toByte()) return true
    }
    return false
}

/** Smudge: Replace `$Id$` with `$Id: <hex> $` */
internal fun identSmudge(input: ByteArray, oid: OID?): FilterResult {
    oid ?: return FilterResult.Passthrough
    val str = input.toString(Charsets.UTF_8)
    val needle = "\$Id\$"
    if (!str.contains(needle)) return FilterResult.Passthrough
    val replacement = "\$Id: ${oid.hex} \$"
    val result = str.replace(needle, replacement)
    return FilterResult.Applied(result.toByteArray(Charsets.UTF_8))
}

/** Clean: Replace `$Id: <anything> $` back to `$Id$` */
internal fun identClean(input: ByteArray): FilterResult {
    val str = try { input.toString(Charsets.UTF_8) } catch (_: Exception) { return FilterResult.Passthrough }
    if (!str.contains("\$Id:")) return FilterResult.Passthrough

    val sb = StringBuilder()
    var remaining = str
    while (true) {
        val startIdx = remaining.indexOf("\$Id:")
        if (startIdx < 0) break
        sb.append(remaining.substring(0, startIdx))
        val afterStart = remaining.substring(startIdx + 4)
        val endIdx = afterStart.indexOf('$')
        if (endIdx >= 0) {
            val content = afterStart.substring(0, endIdx)
            if (!content.contains('\n')) {
                sb.append("\$Id\$")
                remaining = afterStart.substring(endIdx + 1)
            } else {
                sb.append("\$Id:")
                remaining = afterStart
            }
        } else {
            sb.append(remaining.substring(startIdx))
            remaining = ""
            break
        }
    }
    sb.append(remaining)

    val result = sb.toString().toByteArray(Charsets.UTF_8)
    return if (result.contentEquals(input)) FilterResult.Passthrough
    else FilterResult.Applied(result)
}
