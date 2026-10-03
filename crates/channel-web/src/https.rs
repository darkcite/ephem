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
    // A new circuit for every request (security audit M-5): the routing service must not see
    // one identity's vault and channel records leave through the same exit.
    let stream = tor.connect(name, port, true).await?;
    let mut tls = TlsConnector::from(Arc::new(cfg)).connect(server, stream).await.map_err(|e| format!("TLS: {e}"))?;
    // `no-cache`: routing answers are cached by URL for minutes (a newer record stayed invisible
    // for ~5 min on delegated-ipfs.dev, 2026-09-29); callers that need the newest also vary the URL.
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nAccept: {content_type}\r\nCache-Control: no-cache\r\nConnection: close\r\n");
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
    ephem_channel::gateway::parse_any_response(&resp).map_err(str::to_owned)
}
