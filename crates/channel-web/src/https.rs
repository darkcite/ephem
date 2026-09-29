// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! One HTTPS request through a Tor exit (§D.5.2: the owner publishes the channel's IPNS record
//! to `delegated-ipfs.dev` without revealing itself). TLS is rustls with the Mozilla roots
//! (`webpki-roots`); the lab adds its own test root. HTTP/1.1, `Connection: close`.

use ephem_tor::web::Tor;
use futures::{AsyncReadExt, AsyncWriteExt};
use futures_rustls::TlsConnector;
use futures_rustls::rustls::{ClientConfig, RootCertStore, crypto::ring};
use rustls_pki_types::{CertificateDer, ServerName};
use std::sync::Arc;

/// Largest response kept (delegated routing answers are small: an IPNS record is ≤ 10 KiB).
const MAX_RESPONSE: usize = 64 * 1024;

/// One HTTPS request through a Tor exit: `method https://<host><path>` with `body` (empty: no
/// body); returns the status and the response body. `host` may carry a port
/// (`example.org:8443`); `extra_root` (DER, may be empty) is trusted besides the Mozilla roots
/// (the offline lab's test CA). Copies: the response is read into one buffer and the body is
/// cut out of it (≤ 64 KiB, setup path).
pub async fn request(tor: &Tor, method: &str, host: &str, path: &str, content_type: &str, body: &[u8], extra_root: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let (name, port) = match host.rsplit_once(':') {
        Some((h, p)) if p.parse::<u16>().is_ok() => (h, p.parse().unwrap_or(443)),
        _ => (host, 443),
    };
    let mut roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    if !extra_root.is_empty() {
        roots.add(CertificateDer::from(extra_root.to_vec())).map_err(|e| format!("root: {e}"))?;
    }
    let cfg = ClientConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let server = ServerName::try_from(name.to_owned()).map_err(|e| e.to_string())?;
    let stream = tor.connect(name, port, false).await?;
    let mut tls = TlsConnector::from(Arc::new(cfg)).connect(server, stream).await.map_err(|e| format!("TLS: {e}"))?;
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nAccept: {content_type}\r\nConnection: close\r\n");
    if !body.is_empty() {
        head.push_str(&format!("Content-Type: {content_type}\r\nContent-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    tls.write_all(head.as_bytes()).await.map_err(|e| e.to_string())?;
    tls.write_all(body).await.map_err(|e| e.to_string())?;
    tls.flush().await.map_err(|e| e.to_string())?;
    let mut resp = Vec::new();
    let mut buf = [0u8; 4096];
    // `Connection: close`: the response ends with the stream.
    while resp.len() < MAX_RESPONSE {
        match tls.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => resp.extend_from_slice(&buf[..n]),
            Err(e) if !resp.is_empty() => {
                tracing::debug!("https: stream ended: {e}");
                break;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    parse(&resp)
}

/// The status and the body of a whole HTTP/1.1 response (`Content-Length`, chunked, or up to
/// the end of the stream).
fn parse(resp: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let end = resp.windows(4).position(|w| w == b"\r\n\r\n").ok_or("no HTTP response")?;
    let head = std::str::from_utf8(&resp[..end]).map_err(|_| "HTTP head")?;
    let status = head.split(' ').nth(1).and_then(|s| s.parse().ok()).ok_or("no HTTP status")?;
    let rest = &resp[end + 4..];
    let header = |k: &str| head.lines().find_map(|l| l.split_once(':').filter(|(n, _)| n.trim().eq_ignore_ascii_case(k)).map(|(_, v)| v.trim().to_ascii_lowercase()));
    if header("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
        let mut out = Vec::new();
        let mut pos = 0;
        loop {
            let line_end = rest[pos..].windows(2).position(|w| w == b"\r\n").ok_or("chunk")? + pos;
            let size = std::str::from_utf8(&rest[pos..line_end]).ok().and_then(|l| usize::from_str_radix(l.split(';').next()?.trim(), 16).ok()).ok_or("chunk size")?;
            pos = line_end + 2;
            if size == 0 {
                return Ok((status, out));
            }
            out.extend_from_slice(rest.get(pos..pos + size).ok_or("chunk cut short")?);
            pos += size + 2;
        }
    }
    match header("content-length").and_then(|v| v.parse::<usize>().ok()) {
        Some(n) => Ok((status, rest.get(..n).ok_or("body cut short")?.to_vec())),
        None => Ok((status, rest.to_vec())),
    }
}
