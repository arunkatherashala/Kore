//! Optional TLS support for cluster RPC (feature `tls`).
//!
//! Wraps `tokio-rustls` so both coord and worker can serve/dial over TLS
//! streams that satisfy the same `AsyncRead + AsyncWrite` bounds `KoreFrame`
//! already requires. No changes to the wire codec — TLS is orthogonal.
//!
//! # Usage
//!
//! ```ignore
//! // Server:
//! let acceptor = kore_net::tls::server_acceptor_from_pem(&cert_pem, &key_pem)?;
//! let (raw, _) = listener.accept().await?;
//! let tls_stream = acceptor.accept(raw).await?;
//! let msg = KoreFrame::read(&mut tls_stream).await?;
//!
//! // Client:
//! let connector = kore_net::tls::client_connector_trust_roots(&[ca_pem])?;
//! let raw = TcpStream::connect(addr).await?;
//! let mut tls = connector.connect(server_name, raw).await?;
//! KoreFrame::write(&mut tls, &msg).await?;
//! ```

use std::io::Cursor;
use std::sync::Arc;

use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio_rustls::rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio_rustls::{TlsAcceptor, TlsConnector};

/// Wrap a raw PEM certificate chain + private key into a `TlsAcceptor` that
/// coord/worker can use to upgrade an inbound TCP connection.
pub fn server_acceptor_from_pem(
    cert_pem: &[u8],
    key_pem: &[u8],
) -> std::io::Result<TlsAcceptor> {
    let cert_chain = load_certs(cert_pem)?;
    let key = load_private_key(key_pem)?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .map_err(|e| std::io::Error::new(
            std::io::ErrorKind::InvalidInput, format!("server cert: {e}")
        ))?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

/// Build a client connector that trusts the given PEM CA bundle(s).
/// If no CAs are provided, uses the system webpki-roots (via `rustls-native-certs`
/// — for now we require an explicit CA to avoid pulling extra deps and keep
/// the trust surface small).
pub fn client_connector_trust_roots(
    ca_pems: &[&[u8]],
) -> std::io::Result<TlsConnector> {
    let mut roots = RootCertStore::empty();
    for pem in ca_pems {
        for cert in load_certs(pem)? {
            roots.add(cert).map_err(|e| std::io::Error::new(
                std::io::ErrorKind::InvalidInput, format!("root cert: {e}")
            ))?;
        }
    }
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

/// Parse a `ServerName` from a hostname or IP string.
pub fn server_name(hostname: &str) -> std::io::Result<ServerName<'static>> {
    ServerName::try_from(hostname.to_string())
        .map_err(|e| std::io::Error::new(
            std::io::ErrorKind::InvalidInput, format!("bad hostname: {e}")
        ))
}

fn load_certs(pem: &[u8]) -> std::io::Result<Vec<CertificateDer<'static>>> {
    let mut r = Cursor::new(pem);
    rustls_pemfile::certs(&mut r)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| std::io::Error::new(
            std::io::ErrorKind::InvalidInput, format!("cert PEM: {e}")
        ))
}

fn load_private_key(pem: &[u8]) -> std::io::Result<PrivateKeyDer<'static>> {
    let mut r = Cursor::new(pem);
    if let Some(key) = rustls_pemfile::private_key(&mut r)
        .map_err(|e| std::io::Error::new(
            std::io::ErrorKind::InvalidInput, format!("key PEM: {e}")
        ))?
    {
        return Ok(key);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "no private key found in PEM",
    ))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KoreFrame, KoreMsg};
    use tokio::net::{TcpListener, TcpStream};

    fn gen_self_signed() -> (Vec<u8>, Vec<u8>) {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("gen cert");
        let cert_pem = cert.cert.pem();
        let key_pem  = cert.key_pair.serialize_pem();
        (cert_pem.into_bytes(), key_pem.into_bytes())
    }

    #[test]
    fn server_acceptor_builds() {
        let (cert, key) = gen_self_signed();
        let _ = server_acceptor_from_pem(&cert, &key).expect("acceptor");
    }

    #[test]
    fn client_connector_builds() {
        let (cert, _) = gen_self_signed();
        let _ = client_connector_trust_roots(&[cert.as_slice()]).expect("connector");
    }

    #[tokio::test]
    async fn tls_koreframe_roundtrip() {
        // Full TLS handshake, then a KoreFrame message round-trips over it.
        let (cert, key) = gen_self_signed();
        let acceptor = server_acceptor_from_pem(&cert, &key).unwrap();
        let connector = client_connector_trust_roots(&[cert.as_slice()]).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let acc = acceptor.clone();
        let server_task = tokio::spawn(async move {
            let (raw, _) = listener.accept().await.unwrap();
            let mut tls = acc.accept(raw).await.expect("server handshake");
            let msg = KoreFrame::read(&mut tls).await.unwrap();
            KoreFrame::write(&mut tls, &msg).await.unwrap();
        });

        let raw = TcpStream::connect(addr).await.unwrap();
        let sn = server_name("localhost").unwrap();
        let mut tls = connector.connect(sn, raw).await.expect("client handshake");
        KoreFrame::write(&mut tls, &KoreMsg::Ping).await.unwrap();
        let reply = KoreFrame::read(&mut tls).await.unwrap();
        assert!(matches!(reply, KoreMsg::Ping));

        server_task.await.unwrap();
    }
}
