package ai.muonium.muongit

import java.io.ByteArrayOutputStream
import kotlin.concurrent.thread

sealed interface RemoteAuth {
    data object None : RemoteAuth
    data class Basic(val username: String, val password: String) : RemoteAuth
    data class BearerToken(val token: String) : RemoteAuth
    data class SshKey(
        val username: String,
        val privateKey: String,
        val port: Int? = null,
        val strictHostKeyChecking: Boolean = true,
    ) : RemoteAuth
    data class SshAgent(
        val username: String,
        val port: Int? = null,
        val strictHostKeyChecking: Boolean = true,
    ) : RemoteAuth
}

data class TransportOptions(
    val auth: RemoteAuth? = null,
    val insecureSkipTLSVerify: Boolean = false,
)

data class ServiceAdvertisement(
    val refs: List<RemoteRef>,
    val capabilities: ServerCapabilities,
)

private enum class RemoteService {
    UploadPack,
    ReceivePack,
}

private data class ParsedRemoteURL(
    val scheme: String,
    val user: String?,
    val host: String,
    val port: Int?,
    val path: String,
)

fun advertiseUploadPack(url: String, options: TransportOptions): ServiceAdvertisement =
    advertiseRefs(url, RemoteService.UploadPack, options)

fun uploadPack(url: String, request: ByteArray, options: TransportOptions): ByteArray =
    statelessRPC(url, RemoteService.UploadPack, request, options)

fun advertiseReceivePack(url: String, options: TransportOptions): ServiceAdvertisement =
    advertiseRefs(url, RemoteService.ReceivePack, options)

fun receivePack(url: String, request: ByteArray, options: TransportOptions): ByteArray =
    statelessRPC(url, RemoteService.ReceivePack, request, options)

private fun advertiseRefs(url: String, service: RemoteService, options: TransportOptions): ServiceAdvertisement {
    val parsed = parseRemoteURL(url)
    val response = when (parsed.scheme) {
        "http", "https" -> advertiseRefsHTTP(url, service, options)
        "ssh" -> advertiseRefsSSH(parsed, service, options)
        else -> throw MuonGitException.InvalidSpec("unsupported remote scheme '${parsed.scheme}'")
    }

    val (refs, capabilities) = parseRefAdvertisement(stripServicePreamble(pktLineDecode(response).first))
    return ServiceAdvertisement(refs, capabilities)
}

private fun statelessRPC(
    url: String,
    service: RemoteService,
    request: ByteArray,
    options: TransportOptions,
): ByteArray {
    val parsed = parseRemoteURL(url)
    return when (parsed.scheme) {
        "http", "https" -> statelessRPCHTTP(url, service, request, options)
        "ssh" -> statelessRPCSSH(parsed, service, request, options)
        else -> throw MuonGitException.InvalidSpec("unsupported remote scheme '${parsed.scheme}'")
    }
}

private fun advertiseRefsHTTP(url: String, service: RemoteService, options: TransportOptions): ByteArray {
    val args = mutableListOf(
        "--silent",
        "--show-error",
        "--fail",
        "--location",
        "--header",
        "Accept: application/x-${service.infoRefsService()}-advertisement",
    )
    applyHTTPAuthArgs(args, options)
    args.add("${trimTrailingSlash(url)}/info/refs?service=${service.infoRefsService()}")
    return runCommand("/usr/bin/curl", args, null)
}

private fun statelessRPCHTTP(
    url: String,
    service: RemoteService,
    request: ByteArray,
    options: TransportOptions,
): ByteArray {
    val args = mutableListOf(
        "--silent",
        "--show-error",
        "--fail",
        "--location",
        "--request",
        "POST",
        "--header",
        "Content-Type: ${service.requestContentType()}",
        "--header",
        "Accept: ${service.resultContentType()}",
        "--data-binary",
        "@-",
    )
    applyHTTPAuthArgs(args, options)
    args.add("${trimTrailingSlash(url)}${service.httpPath()}")
    return runCommand("/usr/bin/curl", args, request)
}

private fun advertiseRefsSSH(parsed: ParsedRemoteURL, service: RemoteService, options: TransportOptions): ByteArray {
    val remoteCommand = "${service.commandName()} --stateless-rpc --advertise-refs ${shellQuote(parsed.path)}"
    return runSSHCommand(parsed, options, remoteCommand, null)
}

private fun statelessRPCSSH(
    parsed: ParsedRemoteURL,
    service: RemoteService,
    request: ByteArray,
    options: TransportOptions,
): ByteArray {
    val remoteCommand = "${service.commandName()} --stateless-rpc ${shellQuote(parsed.path)}"
    return runSSHCommand(parsed, options, remoteCommand, request)
}

private fun runSSHCommand(
    parsed: ParsedRemoteURL,
    options: TransportOptions,
    remoteCommand: String,
    input: ByteArray?,
): ByteArray {
    val (target, port, privateKey, strictHostKeyChecking) = resolveSSHConfig(parsed, options)
    val args = mutableListOf("-o", "BatchMode=yes")
    if (!strictHostKeyChecking) {
        args.addAll(listOf("-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null"))
    }
    if (port != null) {
        args.addAll(listOf("-p", port.toString()))
    }
    if (privateKey != null) {
        args.addAll(listOf("-i", privateKey))
    }
    args.add(target)
    args.add(remoteCommand)
    return runCommand("/usr/bin/ssh", args, input)
}

private fun resolveSSHConfig(
    parsed: ParsedRemoteURL,
    options: TransportOptions,
): Quadruple<String, Int?, String?, Boolean> {
    return when (val auth = options.auth) {
        null, RemoteAuth.None -> Quadruple(
            parsed.user?.let { "$it@${parsed.host}" } ?: parsed.host,
            parsed.port,
            null,
            true,
        )
        is RemoteAuth.SshKey -> Quadruple(
            "${auth.username}@${parsed.host}",
            auth.port ?: parsed.port,
            auth.privateKey,
            auth.strictHostKeyChecking,
        )
        is RemoteAuth.SshAgent -> Quadruple(
            "${auth.username}@${parsed.host}",
            auth.port ?: parsed.port,
            null,
            auth.strictHostKeyChecking,
        )
        is RemoteAuth.Basic, is RemoteAuth.BearerToken ->
            throw MuonGitException.Auth("HTTP credentials cannot be used with ssh remotes")
    }
}

private fun applyHTTPAuthArgs(args: MutableList<String>, options: TransportOptions) {
    if (options.insecureSkipTLSVerify) {
        args.add("--insecure")
    }

    when (val auth = options.auth) {
        is RemoteAuth.Basic -> args.addAll(listOf("--user", "${auth.username}:${auth.password}"))
        is RemoteAuth.BearerToken -> args.addAll(listOf("--header", "Authorization: Bearer ${auth.token}"))
        else -> Unit
    }
}

private fun parseRemoteURL(url: String): ParsedRemoteURL {
    if (!url.contains("://")) {
        val colon = url.indexOf(':')
        if (colon < 0) {
            throw MuonGitException.InvalidSpec("invalid remote url '$url'")
        }
        val (user, host) = parseUserHost(url.substring(0, colon))
        return ParsedRemoteURL(
            scheme = "ssh",
            user = user,
            host = host,
            port = null,
            path = url.substring(colon + 1),
        )
    }

    val schemeEnd = url.indexOf("://")
    val scheme = url.substring(0, schemeEnd)
    val rest = url.substring(schemeEnd + 3)
    val pathStart = rest.indexOf('/')
    val authority = if (pathStart >= 0) rest.substring(0, pathStart) else rest
    val path = if (pathStart >= 0) rest.substring(pathStart) else "/"
    val (user, hostPart) = if ('@' in authority) {
        val at = authority.lastIndexOf('@')
        Pair(authority.substring(0, at), authority.substring(at + 1))
    } else {
        Pair(null, authority)
    }
    val (host, port) = splitHostPort(hostPart)

    return ParsedRemoteURL(
        scheme = scheme,
        user = user,
        host = host,
        port = port,
        path = path,
    )
}

private fun parseUserHost(text: String): Pair<String?, String> {
    val at = text.indexOf('@')
    return if (at >= 0) {
        Pair(text.substring(0, at), text.substring(at + 1))
    } else {
        Pair(null, text)
    }
}

private fun splitHostPort(authority: String): Pair<String, Int?> {
    val colon = authority.lastIndexOf(':')
    if (colon >= 0) {
        val host = authority.substring(0, colon)
        val portText = authority.substring(colon + 1)
        if (portText.isNotEmpty() && portText.all(Char::isDigit)) {
            return Pair(host, portText.toInt())
        }
    }
    return Pair(authority, null)
}

private fun stripServicePreamble(lines: List<PktLine>): List<PktLine> {
    if (lines.size >= 2) {
        val first = lines[0]
        if (first is PktLine.Data &&
            startsWithBytes(first.bytes, "# service=".toByteArray()) &&
            lines[1] is PktLine.Flush
        ) {
            return lines.drop(2)
        }
    }
    return lines
}

private fun trimTrailingSlash(url: String): String {
    var value = url
    while (value.endsWith("/")) {
        value = value.dropLast(1)
    }
    return value
}

private fun shellQuote(value: String): String =
    "'${value.replace("'", "'\\''")}'"

private fun startsWithBytes(bytes: ByteArray, prefix: ByteArray): Boolean {
    if (bytes.size < prefix.size) {
        return false
    }
    for (index in prefix.indices) {
        if (bytes[index] != prefix[index]) {
            return false
        }
    }
    return true
}

private fun runCommand(command: String, args: List<String>, input: ByteArray?): ByteArray {
    val process = try {
        ProcessBuilder(listOf(command) + args).start()
    } catch (error: Exception) {
        throw MuonGitException.InvalidObject(error.message ?: "failed to launch remote command")
    }

    val stdout = ByteArrayOutputStream()
    val stderr = ByteArrayOutputStream()
    val stdoutThread = thread(start = true) {
        process.inputStream.use { it.copyTo(stdout) }
    }
    val stderrThread = thread(start = true) {
        process.errorStream.use { it.copyTo(stderr) }
    }

    process.outputStream.use { stream ->
        if (input != null) {
            stream.write(input)
        }
    }

    val exitCode = process.waitFor()
    stdoutThread.join()
    stderrThread.join()

    if (exitCode != 0) {
        val message = stderr.toString(Charsets.UTF_8.name()).trim().ifEmpty { "command failed" }
        val lower = message.lowercase()
        if ("certificate" in lower) {
            throw MuonGitException.Certificate(message)
        }
        if ("auth" in lower || "permission denied" in lower || "unauthorized" in lower) {
            throw MuonGitException.Auth(message)
        }
        throw MuonGitException.InvalidObject(message)
    }

    return stdout.toByteArray()
}

private fun RemoteService.infoRefsService(): String = when (this) {
    RemoteService.UploadPack -> "git-upload-pack"
    RemoteService.ReceivePack -> "git-receive-pack"
}

private fun RemoteService.requestContentType(): String = when (this) {
    RemoteService.UploadPack -> "application/x-git-upload-pack-request"
    RemoteService.ReceivePack -> "application/x-git-receive-pack-request"
}

private fun RemoteService.resultContentType(): String = when (this) {
    RemoteService.UploadPack -> "application/x-git-upload-pack-result"
    RemoteService.ReceivePack -> "application/x-git-receive-pack-result"
}

private fun RemoteService.commandName(): String = when (this) {
    RemoteService.UploadPack -> "git-upload-pack"
    RemoteService.ReceivePack -> "git-receive-pack"
}

private fun RemoteService.httpPath(): String = when (this) {
    RemoteService.UploadPack -> "/git-upload-pack"
    RemoteService.ReceivePack -> "/git-receive-pack"
}

private data class Quadruple<A, B, C, D>(
    val first: A,
    val second: B,
    val third: C,
    val fourth: D,
)
