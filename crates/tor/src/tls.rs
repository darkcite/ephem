//! TLS for Tor channels, with rustls + ring (no C beyond ring's own, no OS certificate store).
//!
//! Same policy as arti's `RustlsProvider` (tor-rtcompat, MIT/Apache-2.0), which arti exports
//! only together with tokio / async-std / smol, none of which runs in a browser: the relay's
//! TLS certificate is merely a container for its link key, so the certificate itself is not
//! validated, but the handshake signature made with it is. The real authentication is the
//! Tor link handshake (CERTS cell) that arti performs inside this TLS session. No resumption.

use crate::net::NoListener;
use crate::stream::SnowflakeStream;
use async_trait::async_trait;
use futures_rustls::rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use futures_rustls::rustls::crypto::{WebPkiSupportedAlgorithms, verify_tls12_signature, verify_tls13_signature};
use futures_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use futures_rustls::rustls::{self, CertificateError, DigitallySignedStruct, Error as TlsError, SignatureScheme};
use std::borrow::Cow;
use std::io::{self, Result as IoResult};
use std::sync::Arc;
use tor_rtcompat::tls::{CertifiedConn, TlsAcceptorSettings, TlsConnector};
use tor_rtcompat::{StreamOps, TlsProvider};

/// A Tor channel's TLS connection (newtype: the tor-rtcompat traits are implemented here, since
/// tor-rtcompat implements them for rustls only together with a native runtime).
pub struct TlsStream(futures_rustls::client::TlsStream<SnowflakeStream>);

impl futures::io::AsyncRead for TlsStream {
    fn poll_read(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &mut [u8]) -> std::task::Poll<IoResult<usize>> {
        std::pin::Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl futures::io::AsyncWrite for TlsStream {
    fn poll_write(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &[u8]) -> std::task::Poll<IoResult<usize>> {
        std::pin::Pin::new(&mut self.0).poll_write(cx, buf)
    }
    fn poll_flush(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<IoResult<()>> {
        std::pin::Pin::new(&mut self.0).poll_flush(cx)
    }
    fn poll_close(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<IoResult<()>> {
        std::pin::Pin::new(&mut self.0).poll_close(cx)
    }
}

impl StreamOps for TlsStream {}

impl CertifiedConn for TlsStream {
    fn export_keying_material(&self, len: usize, label: &[u8], context: Option<&[u8]>) -> IoResult<Vec<u8>> {
        let (_, session) = self.0.get_ref();
        session.export_keying_material(vec![0u8; len], label, context).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn peer_certificate(&self) -> IoResult<Option<Cow<'_, [u8]>>> {
        let (_, session) = self.0.get_ref();
        Ok(session.peer_certificates().and_then(|c| c.first().map(|c| Cow::from(c.as_ref()))))
    }

    fn own_certificate(&self) -> IoResult<Option<Cow<'_, [u8]>>> {
        Ok(None)
    }
}

#[derive(Clone)]
pub struct TorTls {
    config: Arc<rustls::ClientConfig>,
}

impl std::fmt::Debug for TorTls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TorTls")
    }
}

impl Default for TorTls {
    fn default() -> Self {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let algs = provider.signature_verification_algorithms;
        let mut config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("ring supports TLS 1.2 and 1.3")
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Verifier(algs)))
            .with_no_client_auth();
        // tor-spec: implementations SHOULD NOT allow TLS session resumption.
        config.resumption = rustls::client::Resumption::disabled();
        Self { config: Arc::new(config) }
    }
}

#[derive(Debug)]
struct Verifier(WebPkiSupportedAlgorithms);

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(&self, end_entity: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, TlsError> {
        // Only well-formedness; the relay's identity is proven in the Tor link handshake.
        webpki::EndEntityCert::try_from(end_entity).map_err(|_| TlsError::InvalidCertificate(CertificateError::BadEncoding))?;
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(message, cert, dss, &self.0)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(message, cert, dss, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }

    fn root_hint_subjects(&self) -> Option<&[rustls::DistinguishedName]> {
        None
    }
}

pub struct Connector {
    inner: futures_rustls::TlsConnector,
}

#[async_trait]
impl TlsConnector<SnowflakeStream> for Connector {
    type Conn = TlsStream;

    async fn negotiate_unvalidated(&self, stream: SnowflakeStream, sni_hostname: &str) -> IoResult<TlsStream> {
        let name: ServerName<'_> = sni_hostname.try_into().map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        self.inner.connect(name.to_owned(), stream).await.map(TlsStream)
    }
}

/// Tor mode never runs a TLS server (uninhabited).
pub enum NoServer {}

#[async_trait]
impl TlsConnector<SnowflakeStream> for NoServer {
    type Conn = NoListenerStream;

    async fn negotiate_unvalidated(&self, _stream: SnowflakeStream, _sni_hostname: &str) -> IoResult<NoListenerStream> {
        match *self {}
    }
}

/// The (nonexistent) stream of a TLS server.
pub struct NoListenerStream(NoListener);

impl futures::io::AsyncRead for NoListenerStream {
    fn poll_read(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>, _: &mut [u8]) -> std::task::Poll<IoResult<usize>> {
        match self.0 {}
    }
}

impl futures::io::AsyncWrite for NoListenerStream {
    fn poll_write(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>, _: &[u8]) -> std::task::Poll<IoResult<usize>> {
        match self.0 {}
    }
    fn poll_flush(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> std::task::Poll<IoResult<()>> {
        match self.0 {}
    }
    fn poll_close(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> std::task::Poll<IoResult<()>> {
        match self.0 {}
    }
}

impl StreamOps for NoListenerStream {}

impl CertifiedConn for NoListenerStream {
    fn export_keying_material(&self, _: usize, _: &[u8], _: Option<&[u8]>) -> IoResult<Vec<u8>> {
        match self.0 {}
    }
    fn peer_certificate(&self) -> IoResult<Option<Cow<'_, [u8]>>> {
        match self.0 {}
    }
    fn own_certificate(&self) -> IoResult<Option<Cow<'_, [u8]>>> {
        match self.0 {}
    }
}

impl TlsProvider<SnowflakeStream> for TorTls {
    type Connector = Connector;
    type TlsStream = TlsStream;
    type Acceptor = NoServer;
    type TlsServerStream = NoListenerStream;

    fn tls_connector(&self) -> Connector {
        Connector { inner: futures_rustls::TlsConnector::from(Arc::clone(&self.config)) }
    }

    fn tls_acceptor(&self, _settings: TlsAcceptorSettings) -> IoResult<NoServer> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "Tor mode runs no TLS server"))
    }

    fn supports_keying_material_export(&self) -> bool {
        true
    }
}

