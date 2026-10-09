use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};

use crate::identity::{Fingerprint, Identity};
use crate::{CoreError, Result};

pub const ALPN: &[u8] = b"wisp/1";
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub struct Transport {
    endpoint: quinn::Endpoint,
    cert: CertificateDer<'static>,
    key_der: Vec<u8>,
    provider: Arc<CryptoProvider>,
}

fn tls_err(e: impl std::fmt::Display) -> CoreError {
    CoreError::Transport(e.to_string())
}

impl Transport {
    pub fn bind(identity: &Identity, addr: SocketAddr) -> Result<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let cert = CertificateDer::from(identity.cert_der.clone());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key_der.clone()));

        let mut server_tls = rustls::ServerConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(tls_err)?
            .with_client_cert_verifier(Arc::new(AcceptAnyClientCert { provider: provider.clone() }))
            .with_single_cert(vec![cert.clone()], key)
            .map_err(tls_err)?;
        server_tls.alpn_protocols = vec![ALPN.to_vec()];
        let quic_server = QuicServerConfig::try_from(server_tls).map_err(tls_err)?;
        let server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_server));

        let endpoint = quinn::Endpoint::server(server_config, addr)?;
        Ok(Self { endpoint, cert, key_der: identity.key_der.clone(), provider })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.endpoint.local_addr()?)
    }

    pub async fn accept(&self) -> Option<Result<quinn::Connection>> {
        let incoming = self.endpoint.accept().await?;
        Some(incoming.await.map_err(tls_err))
    }

    pub async fn connect(&self, addr: SocketAddr, expected: Option<Fingerprint>) -> Result<quinn::Connection> {
        let config = self.client_config(expected)?;
        let connecting = self.endpoint.connect_with(config, addr, "wisp").map_err(tls_err)?;
        match tokio::time::timeout(CONNECT_TIMEOUT, connecting).await {
            Ok(result) => result.map_err(tls_err),
            Err(_) => Err(CoreError::Transport(format!("timeout connecting to {addr}"))),
        }
    }

    /// Races all addresses; the first completed handshake wins, which in practice is the
    /// lowest-latency path (cable beats Wi-Fi).
    pub async fn connect_any(
        &self,
        addrs: &[SocketAddr],
        expected: Option<Fingerprint>,
    ) -> Result<(quinn::Connection, SocketAddr)> {
        let mut attempts = tokio::task::JoinSet::new();
        for &addr in addrs {
            let transport = self.clone();
            attempts.spawn(async move { (addr, transport.connect(addr, expected).await) });
        }
        let mut last_err = CoreError::Transport("no addresses to try".into());
        while let Some(joined) = attempts.join_next().await {
            match joined {
                Ok((addr, Ok(conn))) => {
                    attempts.abort_all();
                    return Ok((conn, addr));
                }
                Ok((_, Err(e))) => last_err = e,
                Err(e) => last_err = tls_err(e),
            }
        }
        Err(last_err)
    }

    pub fn close(&self) {
        self.endpoint.close(0u32.into(), b"bye");
    }

    fn client_config(&self, expected: Option<Fingerprint>) -> Result<quinn::ClientConfig> {
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone()));
        let mut client_tls = rustls::ClientConfig::builder_with_provider(self.provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(tls_err)?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedServerCert {
                expected,
                provider: self.provider.clone(),
            }))
            .with_client_auth_cert(vec![self.cert.clone()], key)
            .map_err(tls_err)?;
        client_tls.alpn_protocols = vec![ALPN.to_vec()];
        let quic_client = QuicClientConfig::try_from(client_tls).map_err(tls_err)?;
        Ok(quinn::ClientConfig::new(Arc::new(quic_client)))
    }
}

pub fn peer_fingerprint(conn: &quinn::Connection) -> Option<Fingerprint> {
    let identity = conn.peer_identity()?;
    let certs = identity.downcast::<Vec<CertificateDer<'static>>>().ok()?;
    certs.first().map(|c| Fingerprint::of(c.as_ref()))
}

#[derive(Debug)]
struct PinnedServerCert {
    expected: Option<Fingerprint>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedServerCert {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        match self.expected {
            Some(fp) if fp != Fingerprint::of(end_entity.as_ref()) => {
                Err(rustls::Error::General("certificate fingerprint mismatch".into()))
            }
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

#[derive(Debug)]
struct AcceptAnyClientCert {
    provider: Arc<CryptoProvider>,
}

impl ClientCertVerifier for AcceptAnyClientCert {
    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> std::result::Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{read_frame, write_frame, Message};

    fn localhost() -> SocketAddr {
        "127.0.0.1:0".parse().unwrap()
    }

    #[tokio::test]
    async fn pinned_connection_exchanges_frames_and_fingerprints() {
        let a_id = Identity::generate("A").unwrap();
        let b_id = Identity::generate("B").unwrap();
        let a = Transport::bind(&a_id, localhost()).unwrap();
        let b = Transport::bind(&b_id, localhost()).unwrap();
        let b_addr = b.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let conn = b.accept().await.unwrap().unwrap();
            let client_fp = peer_fingerprint(&conn).unwrap();
            let (mut send, mut recv) = conn.accept_bi().await.unwrap();
            let msg = read_frame(&mut recv).await.unwrap();
            assert_eq!(msg, Message::Ping { nonce: 1 });
            write_frame(&mut send, &Message::Pong { nonce: 1 }).await.unwrap();
            send.finish().unwrap();
            conn.closed().await;
            client_fp
        });

        let conn = a.connect(b_addr, Some(b_id.fingerprint())).await.unwrap();
        assert_eq!(peer_fingerprint(&conn), Some(b_id.fingerprint()));
        let (mut send, mut recv) = conn.open_bi().await.unwrap();
        write_frame(&mut send, &Message::Ping { nonce: 1 }).await.unwrap();
        assert_eq!(read_frame(&mut recv).await.unwrap(), Message::Pong { nonce: 1 });
        conn.close(0u32.into(), b"done");
        assert_eq!(server.await.unwrap(), a_id.fingerprint());
    }

    #[tokio::test]
    async fn wrong_expected_fingerprint_is_refused() {
        let a = Transport::bind(&Identity::generate("A").unwrap(), localhost()).unwrap();
        let b = Transport::bind(&Identity::generate("B").unwrap(), localhost()).unwrap();
        let b_addr = b.local_addr().unwrap();
        tokio::spawn(async move { while let Some(_) = b.accept().await {} });
        let other = Identity::generate("Other").unwrap().fingerprint();
        assert!(a.connect(b_addr, Some(other)).await.is_err());
    }

    #[tokio::test]
    async fn connect_any_skips_dead_addresses() {
        let a = Transport::bind(&Identity::generate("A").unwrap(), localhost()).unwrap();
        let b_id = Identity::generate("B").unwrap();
        let b = Transport::bind(&b_id, localhost()).unwrap();
        let b_addr = b.local_addr().unwrap();
        tokio::spawn(async move { while let Some(_) = b.accept().await {} });

        let dead: SocketAddr = "192.0.2.1:9".parse().unwrap();
        let started = std::time::Instant::now();
        let (_conn, used) = a.connect_any(&[dead, b_addr], Some(b_id.fingerprint())).await.unwrap();
        assert_eq!(used, b_addr);
        assert!(started.elapsed() < CONNECT_TIMEOUT);
    }

    #[tokio::test]
    async fn connect_any_with_no_reachable_address_errors() {
        let a = Transport::bind(&Identity::generate("A").unwrap(), localhost()).unwrap();
        let dead: SocketAddr = "192.0.2.1:9".parse().unwrap();
        assert!(a.connect_any(&[dead], None).await.is_err());
        assert!(a.connect_any(&[], None).await.is_err());
    }
}
