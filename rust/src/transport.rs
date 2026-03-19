//! Git smart protocol and transport abstractions
//! Parity: libgit2 src/libgit2/transports/smart_pkt.c

use crate::error::MuonGitError;
use crate::oid::OID;

// --- Pkt-line encoding/decoding ---

/// Encode data as a pkt-line. Returns the pkt-line bytes.
/// A pkt-line has a 4-hex-digit length prefix (including the 4 bytes themselves).
pub fn pkt_line_encode(data: &[u8]) -> Vec<u8> {
    let len = data.len() + 4;
    let mut out = format!("{:04x}", len).into_bytes();
    out.extend_from_slice(data);
    out
}

/// Encode a flush packet (0000).
pub fn pkt_line_flush() -> Vec<u8> {
    b"0000".to_vec()
}

/// Encode a delimiter packet (0001).
pub fn pkt_line_delim() -> Vec<u8> {
    b"0001".to_vec()
}

/// Decoded pkt-line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PktLine {
    /// A data line.
    Data(Vec<u8>),
    /// Flush packet (0000).
    Flush,
    /// Delimiter packet (0001).
    Delim,
}

/// Parse pkt-lines from a byte buffer.
/// Returns the parsed lines and the number of bytes consumed.
pub fn pkt_line_decode(input: &[u8]) -> Result<(Vec<PktLine>, usize), MuonGitError> {
    let mut lines = Vec::new();
    let mut pos = 0;

    while pos + 4 <= input.len() {
        let hex = std::str::from_utf8(&input[pos..pos + 4])
            .map_err(|_| MuonGitError::InvalidObject("invalid pkt-line header".into()))?;

        if hex == "0000" {
            lines.push(PktLine::Flush);
            pos += 4;
            continue;
        }
        if hex == "0001" {
            lines.push(PktLine::Delim);
            pos += 4;
            continue;
        }

        let len = usize::from_str_radix(hex, 16)
            .map_err(|_| MuonGitError::InvalidObject("invalid pkt-line length".into()))?;

        if len < 4 {
            return Err(MuonGitError::InvalidObject("pkt-line length too small".into()));
        }

        if pos + len > input.len() {
            break; // Incomplete packet
        }

        let data = input[pos + 4..pos + len].to_vec();
        lines.push(PktLine::Data(data));
        pos += len;
    }

    Ok((lines, pos))
}

// --- Smart protocol reference advertisement ---

/// A remote reference from the smart protocol handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRef {
    pub oid: OID,
    pub name: String,
}

/// Server capabilities from the reference advertisement.
#[derive(Debug, Clone, Default)]
pub struct ServerCapabilities {
    pub capabilities: Vec<String>,
}

impl ServerCapabilities {
    pub fn has(&self, cap: &str) -> bool {
        self.capabilities.iter().any(|c| c == cap || c.starts_with(&format!("{}=", cap)))
    }

    pub fn get(&self, cap: &str) -> Option<&str> {
        let prefix = format!("{}=", cap);
        self.capabilities
            .iter()
            .find(|c| c.starts_with(&prefix))
            .map(|c| &c[prefix.len()..])
    }
}

/// Parse the reference advertisement from the smart protocol v1 response.
/// The input should be the decoded pkt-lines from the server.
pub fn parse_ref_advertisement(lines: &[PktLine]) -> Result<(Vec<RemoteRef>, ServerCapabilities), MuonGitError> {
    let mut refs = Vec::new();
    let mut caps = ServerCapabilities::default();

    for (i, line) in lines.iter().enumerate() {
        match line {
            PktLine::Flush => break,
            PktLine::Delim => continue,
            PktLine::Data(data) => {
                let text = String::from_utf8_lossy(data);
                let text = text.trim_end_matches('\n');

                if text.starts_with('#') {
                    continue; // Comment line (e.g., "# service=git-upload-pack")
                }

                // First ref line may contain capabilities after NUL
                let (ref_part, cap_part) = if let Some(nul_pos) = text.find('\0') {
                    (&text[..nul_pos], Some(&text[nul_pos + 1..]))
                } else {
                    (text, None)
                };

                // Parse capabilities from first line
                if i == 0 || caps.capabilities.is_empty() {
                    if let Some(cap_str) = cap_part {
                        caps.capabilities = cap_str.split(' ')
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string())
                            .collect();
                    }
                }

                // Parse ref: "<oid> <refname>"
                if ref_part.len() >= 41 && ref_part.as_bytes()[40] == b' ' {
                    let hex = &ref_part[..40];
                    let name = &ref_part[41..];
                    if let Ok(oid) = OID::from_hex(hex) {
                        refs.push(RemoteRef {
                            oid,
                            name: name.to_string(),
                        });
                    }
                }
            }
        }
    }

    Ok((refs, caps))
}

/// Build a want/have negotiation request for fetch.
/// `wants` are OIDs we need, `haves` are OIDs we already have.
pub fn build_want_have(wants: &[OID], haves: &[OID], caps: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();

    for (i, want) in wants.iter().enumerate() {
        let line = if i == 0 && !caps.is_empty() {
            format!("want {} {}\n", want.hex(), caps.join(" "))
        } else {
            format!("want {}\n", want.hex())
        };
        out.extend_from_slice(&pkt_line_encode(line.as_bytes()));
    }

    out.extend_from_slice(&pkt_line_flush());

    for have in haves {
        let line = format!("have {}\n", have.hex());
        out.extend_from_slice(&pkt_line_encode(line.as_bytes()));
    }

    out.extend_from_slice(&pkt_line_encode(b"done\n"));
    out.extend_from_slice(&pkt_line_flush());

    out
}

/// Parse a URL into (scheme, host, path).
pub fn parse_git_url(url: &str) -> Option<(&str, &str, &str)> {
    // Handle SSH shorthand: user@host:path
    if !url.contains("://") {
        if let Some(colon) = url.find(':') {
            if url[..colon].contains('@') {
                let host = &url[..colon];
                let path = &url[colon + 1..];
                return Some(("ssh", host, path));
            }
        }
        return None;
    }

    let scheme_end = url.find("://")?;
    let scheme = &url[..scheme_end];
    let rest = &url[scheme_end + 3..];

    let path_start = rest.find('/').unwrap_or(rest.len());
    let host = &rest[..path_start];
    let path = if path_start < rest.len() {
        &rest[path_start..]
    } else {
        "/"
    };

    Some((scheme, host, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkt_line_encode() {
        let encoded = pkt_line_encode(b"hello\n");
        assert_eq!(encoded, b"000ahello\n");
    }

    #[test]
    fn test_pkt_line_flush() {
        assert_eq!(pkt_line_flush(), b"0000");
    }

    #[test]
    fn test_pkt_line_decode() {
        let input = b"000ahello\n0000";
        let (lines, consumed) = pkt_line_decode(input).unwrap();
        assert_eq!(consumed, 14);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], PktLine::Data(b"hello\n".to_vec()));
        assert_eq!(lines[1], PktLine::Flush);
    }

    #[test]
    fn test_pkt_line_roundtrip() {
        let data = b"test data here";
        let encoded = pkt_line_encode(data);
        let (lines, _) = pkt_line_decode(&encoded).unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], PktLine::Data(data.to_vec()));
    }

    #[test]
    fn test_parse_ref_advertisement() {
        let oid_hex = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
        let line1 = format!("{} HEAD\0multi_ack thin-pack side-band\n", oid_hex);
        let line2 = format!("{} refs/heads/main\n", oid_hex);

        let mut input = Vec::new();
        input.extend_from_slice(&pkt_line_encode(line1.as_bytes()));
        input.extend_from_slice(&pkt_line_encode(line2.as_bytes()));
        input.extend_from_slice(&pkt_line_flush());

        let (lines, _) = pkt_line_decode(&input).unwrap();
        let (refs, caps) = parse_ref_advertisement(&lines).unwrap();

        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].name, "HEAD");
        assert_eq!(refs[1].name, "refs/heads/main");
        assert!(caps.has("multi_ack"));
        assert!(caps.has("thin-pack"));
        assert!(caps.has("side-band"));
        assert!(!caps.has("ofs-delta"));
    }

    #[test]
    fn test_build_want_have() {
        let want = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let have = OID::from_hex("bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();

        let data = build_want_have(&[want], &[have], &["multi_ack", "thin-pack"]);
        let text = String::from_utf8_lossy(&data);

        assert!(text.contains("want aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d multi_ack thin-pack"));
        assert!(text.contains("have bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"));
        assert!(text.contains("done"));
    }

    #[test]
    fn test_parse_git_url_https() {
        let (scheme, host, path) = parse_git_url("https://github.com/user/repo.git").unwrap();
        assert_eq!(scheme, "https");
        assert_eq!(host, "github.com");
        assert_eq!(path, "/user/repo.git");
    }

    #[test]
    fn test_parse_git_url_ssh() {
        let (scheme, host, path) = parse_git_url("git@github.com:user/repo.git").unwrap();
        assert_eq!(scheme, "ssh");
        assert_eq!(host, "git@github.com");
        assert_eq!(path, "user/repo.git");
    }

    #[test]
    fn test_parse_git_url_ssh_protocol() {
        let (scheme, host, path) = parse_git_url("ssh://git@github.com/user/repo.git").unwrap();
        assert_eq!(scheme, "ssh");
        assert_eq!(host, "git@github.com");
        assert_eq!(path, "/user/repo.git");
    }

    #[test]
    fn test_server_capabilities_get() {
        let caps = ServerCapabilities {
            capabilities: vec![
                "multi_ack".into(),
                "agent=git/2.30.0".into(),
                "symref=HEAD:refs/heads/main".into(),
            ],
        };
        assert!(caps.has("multi_ack"));
        assert!(caps.has("agent"));
        assert_eq!(caps.get("agent"), Some("git/2.30.0"));
        assert_eq!(caps.get("symref"), Some("HEAD:refs/heads/main"));
        assert_eq!(caps.get("multi_ack"), None);
    }
}
