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

/// Largest response kept (delegated routing answers are tiny).
const MAX_RESPONSE: usize = 64 * 1024;

/// `PUT https://<host>/<path>` with `body`; returns the HTTP status. `host` may carry a port
/// (`example.org:8443`); `extra_root` (DER, may be empty) is trusted besides the Mozilla roots
/// (the offline lab's test CA).
pub async fn put(tor: &Tor, host: &str, path: &str, content_type: &str, body: &[u8], extra_root: &[u8]) -> Result<u16, String> {
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
    let head = format!("PUT {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    tls.write_all(head.as_bytes()).await.map_err(|e| e.to_string())?;
    tls.write_all(body).await.map_err(|e| e.to_string())?;
    tls.flush().await.map_err(|e| e.to_string())?;
    let mut resp = Vec::new();
    let mut buf = [0u8; 4096];
    // The status line is all we need; stop at the end of the head.
    while !resp.windows(4).any(|w| w == b"\r\n\r\n") && resp.len() < MAX_RESPONSE {
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
    let line = resp.split(|&b| b == b'\n').next().unwrap_or_default();
    std::str::from_utf8(line).ok().and_then(|l| l.split(' ').nth(1)).and_then(|s| s.parse().ok()).ok_or_else(|| "no HTTP response".into())
}
