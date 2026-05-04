use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::{DigitallySignedStruct, SignatureScheme};
use rustls_pki_types::{pem::PemObject, CertificateDer, ServerName, UnixTime};
use std::{
    future::Future,
    io,
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, TlsConnector};
use tonic::{codegen::http::Uri, transport::{Channel, Endpoint}};
use tower_service::Service;

pub mod splice_proto {
    tonic::include_proto!("proto");
}

use splice_proto::app_client::AppClient;
use splice_proto::{GetSessionRequest, UserPreferencesRequest};

#[derive(Debug)]
struct PinnedLocalCertVerifier { expected_der: Vec<u8> }

impl ServerCertVerifier for PinnedLocalCertVerifier {
    fn verify_server_cert(&self, end_entity: &CertificateDer<'_>, _intermediates: &[CertificateDer<'_>], _server_name: &ServerName<'_>, _ocsp_response: &[u8], _now: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        if end_entity.as_ref() == self.expected_der.as_slice() { Ok(ServerCertVerified::assertion()) } else { Err(rustls::Error::General("pinned cert mismatch".into())) }
    }
    fn verify_tls12_signature(&self, _message: &[u8], _cert: &CertificateDer<'_>, _dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> { Ok(HandshakeSignatureValid::assertion()) }
    fn verify_tls13_signature(&self, _message: &[u8], _cert: &CertificateDer<'_>, _dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> { Ok(HandshakeSignatureValid::assertion()) }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> { vec![SignatureScheme::RSA_PSS_SHA256, SignatureScheme::RSA_PSS_SHA384, SignatureScheme::RSA_PSS_SHA512, SignatureScheme::RSA_PKCS1_SHA256, SignatureScheme::RSA_PKCS1_SHA384, SignatureScheme::RSA_PKCS1_SHA512, SignatureScheme::ECDSA_NISTP256_SHA256, SignatureScheme::ECDSA_NISTP384_SHA384, SignatureScheme::ED25519] }
}

#[derive(Clone)]
struct SpliceTlsConnector { port: u16, config: Arc<rustls::ClientConfig> }

impl Service<Uri> for SpliceTlsConnector {
    type Response = TokioIo<TlsStream<TcpStream>>;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> { Poll::Ready(Ok(())) }
    fn call(&mut self, _req: Uri) -> Self::Future {
        let port = self.port;
        let config = self.config.clone();
        Box::pin(async move {
            let stream = TcpStream::connect(("127.0.0.1", port)).await?;
            let server_name = ServerName::try_from("127.0.0.1").map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?.to_owned();
            let tls_stream = TlsConnector::from(config).connect(server_name, stream).await.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(TokioIo::new(tls_stream))
        })
    }
}

async fn connect_helper(port: u16, cert_pem: Vec<u8>) -> anyhow::Result<AppClient<Channel>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let pinned_cert = CertificateDer::from_pem_slice(&cert_pem)?.into_owned();
    let verifier = Arc::new(PinnedLocalCertVerifier { expected_der: pinned_cert.as_ref().to_vec() });
    let mut config = rustls::ClientConfig::builder().dangerous().with_custom_certificate_verifier(verifier).with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec()];
    let endpoint = Endpoint::from_shared(format!("http://127.0.0.1:{port}"))?.connect_timeout(Duration::from_millis(350)).timeout(Duration::from_secs(2));
    let channel = endpoint.connect_with_connector(SpliceTlsConnector { port, config: Arc::new(config) }).await?;
    Ok(AppClient::new(channel))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cert_path = PathBuf::from(std::env::var_os("HOME").unwrap()).join("Library/Application Support/com.splice.Splice/.certs/cert.pem");
    let cert_pem = std::fs::read(cert_path)?;
    for port in 56765..=56785 {
        match connect_helper(port, cert_pem.clone()).await {
            Ok(mut client) => {
                println!("connected {port}");
                match client.get_session(GetSessionRequest {}).await {
                    Ok(resp) => {
                        let auth = resp.into_inner().auth;
                        println!("GetSession: has_auth={} has_token={}", auth.is_some(), auth.as_ref().map(|a| !a.token.is_empty()).unwrap_or(false));
                    }
                    Err(e) => println!("GetSession failed: {e:?}"),
                }
                match client.user_preferences(UserPreferencesRequest {}).await {
                    Ok(resp) => println!("UserPreferences: has_preferences={}", resp.into_inner().preferences.is_some()),
                    Err(e) => println!("UserPreferences failed: {e:?}"),
                }
                return Ok(());
            }
            Err(e) => println!("{port}: {e:?}"),
        }
    }
    anyhow::bail!("no helper found")
}
