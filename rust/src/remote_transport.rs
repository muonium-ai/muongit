//! Remote transport client helpers for smart HTTP(S) and SSH.

use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::MuonGitError;
use crate::transport::{
    parse_ref_advertisement, pkt_line_decode, PktLine, RemoteRef, ServerCapabilities,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteAuth {
    None,
    Basic {
        username: String,
        password: String,
    },
    BearerToken(String),
    SshKey {
        username: String,
        private_key: String,
        port: Option<u16>,
        strict_host_key_checking: bool,
    },
    SshAgent {
        username: String,
        port: Option<u16>,
        strict_host_key_checking: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TransportOptions {
    pub auth: Option<RemoteAuth>,
    pub insecure_skip_tls_verify: bool,
}

#[derive(Debug, Clone)]
pub struct ServiceAdvertisement {
    pub refs: Vec<RemoteRef>,
    pub capabilities: ServerCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteService {
    UploadPack,
    ReceivePack,
}

impl RemoteService {
    fn http_info_refs_service(self) -> &'static str {
        match self {
            Self::UploadPack => "git-upload-pack",
            Self::ReceivePack => "git-receive-pack",
        }
    }

    fn http_content_type(self) -> &'static str {
        match self {
            Self::UploadPack => "application/x-git-upload-pack-request",
            Self::ReceivePack => "application/x-git-receive-pack-request",
        }
    }

    fn http_result_type(self) -> &'static str {
        match self {
            Self::UploadPack => "application/x-git-upload-pack-result",
            Self::ReceivePack => "application/x-git-receive-pack-result",
        }
    }

    fn command_name(self) -> &'static str {
        match self {
            Self::UploadPack => "git-upload-pack",
            Self::ReceivePack => "git-receive-pack",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedRemoteUrl {
    scheme: String,
    user: Option<String>,
    host: String,
    port: Option<u16>,
    path: String,
}

pub fn advertise_upload_pack(
    url: &str,
    opts: &TransportOptions,
) -> Result<ServiceAdvertisement, MuonGitError> {
    advertise_refs(url, RemoteService::UploadPack, opts)
}

pub fn upload_pack(
    url: &str,
    request: &[u8],
    opts: &TransportOptions,
) -> Result<Vec<u8>, MuonGitError> {
    stateless_rpc(url, RemoteService::UploadPack, request, opts)
}

pub fn advertise_receive_pack(
    url: &str,
    opts: &TransportOptions,
) -> Result<ServiceAdvertisement, MuonGitError> {
    advertise_refs(url, RemoteService::ReceivePack, opts)
}

pub fn receive_pack(
    url: &str,
    request: &[u8],
    opts: &TransportOptions,
) -> Result<Vec<u8>, MuonGitError> {
    stateless_rpc(url, RemoteService::ReceivePack, request, opts)
}

fn advertise_refs(
    url: &str,
    service: RemoteService,
    opts: &TransportOptions,
) -> Result<ServiceAdvertisement, MuonGitError> {
    let parsed = parse_remote_url(url)?;
    let response = match parsed.scheme.as_str() {
        "http" | "https" => advertise_refs_http(url, service, opts)?,
        "ssh" => advertise_refs_ssh(&parsed, service, opts)?,
        scheme => {
            return Err(MuonGitError::InvalidSpec(format!(
                "unsupported remote scheme '{}'",
                scheme
            )))
        }
    };

    let (lines, _) = pkt_line_decode(&response)?;
    let lines = strip_service_preamble(&lines);
    let (refs, capabilities) = parse_ref_advertisement(lines)?;
    Ok(ServiceAdvertisement { refs, capabilities })
}

fn stateless_rpc(
    url: &str,
    service: RemoteService,
    request: &[u8],
    opts: &TransportOptions,
) -> Result<Vec<u8>, MuonGitError> {
    let parsed = parse_remote_url(url)?;
    match parsed.scheme.as_str() {
        "http" | "https" => stateless_rpc_http(url, service, request, opts),
        "ssh" => stateless_rpc_ssh(&parsed, service, request, opts),
        scheme => Err(MuonGitError::InvalidSpec(format!(
            "unsupported remote scheme '{}'",
            scheme
        ))),
    }
}

fn advertise_refs_http(
    url: &str,
    service: RemoteService,
    opts: &TransportOptions,
) -> Result<Vec<u8>, MuonGitError> {
    let endpoint = format!(
        "{}{}?service={}",
        trim_trailing_slash(url),
        "/info/refs",
        service.http_info_refs_service()
    );
    let mut args = vec![
        "--silent".to_string(),
        "--show-error".to_string(),
        "--fail".to_string(),
        "--location".to_string(),
        "--header".to_string(),
        format!(
            "Accept: application/x-{}-advertisement",
            service.http_info_refs_service()
        ),
    ];
    apply_http_auth_args(&mut args, opts);
    args.push(endpoint);
    run_command("/usr/bin/curl", &args, None)
}

fn stateless_rpc_http(
    url: &str,
    service: RemoteService,
    request: &[u8],
    opts: &TransportOptions,
) -> Result<Vec<u8>, MuonGitError> {
    let endpoint = format!("{}{}", trim_trailing_slash(url), service_http_path(service));
    let mut args = vec![
        "--silent".to_string(),
        "--show-error".to_string(),
        "--fail".to_string(),
        "--location".to_string(),
        "--request".to_string(),
        "POST".to_string(),
        "--header".to_string(),
        format!("Content-Type: {}", service.http_content_type()),
        "--header".to_string(),
        format!("Accept: {}", service.http_result_type()),
        "--data-binary".to_string(),
        "@-".to_string(),
    ];
    apply_http_auth_args(&mut args, opts);
    args.push(endpoint);
    run_command("/usr/bin/curl", &args, Some(request))
}

fn advertise_refs_ssh(
    parsed: &ParsedRemoteUrl,
    service: RemoteService,
    opts: &TransportOptions,
) -> Result<Vec<u8>, MuonGitError> {
    let remote_cmd = format!(
        "{} --stateless-rpc --advertise-refs {}",
        service.command_name(),
        shell_quote(&parsed.path)
    );
    run_ssh_command(parsed, opts, &remote_cmd, None)
}

fn stateless_rpc_ssh(
    parsed: &ParsedRemoteUrl,
    service: RemoteService,
    request: &[u8],
    opts: &TransportOptions,
) -> Result<Vec<u8>, MuonGitError> {
    let remote_cmd = format!(
        "{} --stateless-rpc {}",
        service.command_name(),
        shell_quote(&parsed.path)
    );
    run_ssh_command(parsed, opts, &remote_cmd, Some(request))
}

fn run_ssh_command(
    parsed: &ParsedRemoteUrl,
    opts: &TransportOptions,
    remote_cmd: &str,
    input: Option<&[u8]>,
) -> Result<Vec<u8>, MuonGitError> {
    let mut args = Vec::new();

    let (username, port, strict_host_key_checking) = match opts.auth.as_ref() {
        Some(RemoteAuth::SshKey {
            username,
            private_key,
            port,
            strict_host_key_checking,
        }) => {
            args.push("-i".to_string());
            args.push(private_key.clone());
            (Some(username.clone()), port.or(parsed.port), *strict_host_key_checking)
        }
        Some(RemoteAuth::SshAgent {
            username,
            port,
            strict_host_key_checking,
        }) => (
            Some(username.clone()),
            port.or(parsed.port),
            *strict_host_key_checking,
        ),
        Some(RemoteAuth::None) | None => (parsed.user.clone(), parsed.port, true),
        Some(RemoteAuth::Basic { .. }) | Some(RemoteAuth::BearerToken(_)) => {
            return Err(MuonGitError::Auth(
                "HTTP credentials cannot be used with ssh remotes".into(),
            ))
        }
    };

    args.push("-o".to_string());
    args.push("BatchMode=yes".to_string());
    if !strict_host_key_checking {
        args.push("-o".to_string());
        args.push("StrictHostKeyChecking=no".to_string());
        args.push("-o".to_string());
        args.push("UserKnownHostsFile=/dev/null".to_string());
    }
    if let Some(port) = port {
        args.push("-p".to_string());
        args.push(port.to_string());
    }

    let target = match username {
        Some(user) => format!("{}@{}", user, parsed.host),
        None => parsed.host.clone(),
    };
    args.push(target);
    args.push(remote_cmd.to_string());

    run_command("/usr/bin/ssh", &args, input)
}

fn apply_http_auth_args(args: &mut Vec<String>, opts: &TransportOptions) {
    if opts.insecure_skip_tls_verify {
        args.push("--insecure".to_string());
    }

    match opts.auth.as_ref() {
        Some(RemoteAuth::Basic { username, password }) => {
            args.push("--user".to_string());
            args.push(format!("{}:{}", username, password));
        }
        Some(RemoteAuth::BearerToken(token)) => {
            args.push("--header".to_string());
            args.push(format!("Authorization: Bearer {}", token));
        }
        _ => {}
    }
}

fn run_command(
    command: &str,
    args: &[String],
    input: Option<&[u8]>,
) -> Result<Vec<u8>, MuonGitError> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(input) = input {
        let mut stdin = child.stdin.take().ok_or_else(|| {
            MuonGitError::Invalid("failed to open child stdin".into())
        })?;
        stdin.write_all(input)?;
    }

    let output = child.wait_with_output()?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            format!("command '{}' failed", command)
        } else {
            stderr
        };

        if message.to_ascii_lowercase().contains("certificate") {
            Err(MuonGitError::Certificate(message))
        } else if message.to_ascii_lowercase().contains("auth")
            || message.to_ascii_lowercase().contains("permission denied")
            || message.to_ascii_lowercase().contains("unauthorized")
        {
            Err(MuonGitError::Auth(message))
        } else {
            Err(MuonGitError::Invalid(message))
        }
    }
}

fn parse_remote_url(url: &str) -> Result<ParsedRemoteUrl, MuonGitError> {
    if !url.contains("://") {
        let Some((user_host, path)) = url.split_once(':') else {
            return Err(MuonGitError::InvalidSpec(format!(
                "invalid remote url '{}'",
                url
            )));
        };
        let (user, host) = parse_user_host(user_host);
        return Ok(ParsedRemoteUrl {
            scheme: "ssh".into(),
            user,
            host: host.to_string(),
            port: None,
            path: path.to_string(),
        });
    }

    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| MuonGitError::InvalidSpec(format!("invalid remote url '{}'", url)))?;
    let path_start = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..path_start];
    let path = if path_start < rest.len() {
        &rest[path_start..]
    } else {
        "/"
    };

    let (user, host_port) = if let Some((user_info, host_part)) = authority.rsplit_once('@') {
        (Some(user_info.to_string()), host_part)
    } else {
        (None, authority)
    };

    let (host, port) = split_host_port(host_port);
    Ok(ParsedRemoteUrl {
        scheme: scheme.to_string(),
        user,
        host,
        port,
        path: path.to_string(),
    })
}

fn parse_user_host(user_host: &str) -> (Option<String>, &str) {
    if let Some((user, host)) = user_host.split_once('@') {
        (Some(user.to_string()), host)
    } else {
        (None, user_host)
    }
}

fn strip_service_preamble(lines: &[PktLine]) -> &[PktLine] {
    if matches!(lines.first(), Some(PktLine::Data(data)) if data.starts_with(b"# service="))
        && matches!(lines.get(1), Some(PktLine::Flush))
    {
        &lines[2..]
    } else {
        lines
    }
}

fn split_host_port(host_port: &str) -> (String, Option<u16>) {
    if let Some((host, port)) = host_port.rsplit_once(':') {
        if port.chars().all(|ch| ch.is_ascii_digit()) {
            return (host.to_string(), port.parse::<u16>().ok());
        }
    }
    (host_port.to_string(), None)
}

fn trim_trailing_slash(url: &str) -> &str {
    url.trim_end_matches('/')
}

fn service_http_path(service: RemoteService) -> &'static str {
    match service {
        RemoteService::UploadPack => "/git-upload-pack",
        RemoteService::ReceivePack => "/git-receive-pack",
    }
}

fn shell_quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_remote_url_https() {
        let parsed = parse_remote_url("https://github.com/user/repo.git").unwrap();
        assert_eq!(parsed.scheme, "https");
        assert_eq!(parsed.host, "github.com");
        assert_eq!(parsed.port, None);
        assert_eq!(parsed.path, "/user/repo.git");
    }

    #[test]
    fn test_parse_remote_url_ssh_shorthand() {
        let parsed = parse_remote_url("git@github.com:user/repo.git").unwrap();
        assert_eq!(parsed.scheme, "ssh");
        assert_eq!(parsed.user.as_deref(), Some("git"));
        assert_eq!(parsed.host, "github.com");
        assert_eq!(parsed.path, "user/repo.git");
    }

    #[test]
    fn test_parse_remote_url_ssh_with_port() {
        let parsed = parse_remote_url("ssh://git@example.com:2222/repo.git").unwrap();
        assert_eq!(parsed.scheme, "ssh");
        assert_eq!(parsed.user.as_deref(), Some("git"));
        assert_eq!(parsed.host, "example.com");
        assert_eq!(parsed.port, Some(2222));
        assert_eq!(parsed.path, "/repo.git");
    }
}
