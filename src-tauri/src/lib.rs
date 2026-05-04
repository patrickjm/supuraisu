#![allow(unexpected_cfgs)]

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::{DigitallySignedStruct, SignatureScheme};
use rustls_pki_types::{pem::PemObject, CertificateDer, ServerName, UnixTime};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::{
    collections::HashMap,
    future::Future,
    fs,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    process::{Command, Stdio},
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, TlsConnector};
use tonic::{
    codegen::http::Uri,
    transport::{Channel, Endpoint, Identity, Server, ServerTlsConfig},
};
use tauri::{Emitter, Manager};
use tower_service::Service;

pub mod splice_proto {
    tonic::include_proto!("proto");
}

use splice_proto::app_client::AppClient;
use splice_proto::electron_server::{Electron, ElectronServer};
use splice_proto::{
    search_sample_request, Auth, AuthType, DownloadSampleRequest, Empty, GetElectronSessionResponse, GetSessionRequest,
    HelperStartedMessageRequest, HelperStartedMessageResponse, ListSamplePacksRequest, LoggedInRequest, Sample,
    SampleInfoRequest, SamplePack, SearchSampleRequest, UpdatedSessionRequest, User, UserPreferencesRequest,
    ValidateLoginRequest,
};

const SPLICE_GRPC_PORT_START: u16 = 56765;
const SPLICE_GRPC_PORT_END: u16 = 56785;
const AUTH_DOMAIN: &str = "auth.splice.com";
const AUTH_CLIENT_ID: &str = "L2JVBs6YHYe7OeK3eeyOXoPNcNTkPWgv";
const AUTH_AUDIENCE: &str = "https://splice.com";
const AUTH_REDIRECT_URI: &str = "http://localhost/auth-callback";
const AUTH_KEYCHAIN_SERVICE: &str = "Supuraisu Auth";
const AUTH_KEYCHAIN_ACCOUNT: &str = "splice-refresh-token-v1";

#[derive(Debug, Serialize)]
struct SpliceEnvironmentStatus {
    app_path: String,
    app_installed: bool,
    helper_path: String,
    helper_installed: bool,
    user_data_path: Option<String>,
    cert_path: Option<String>,
    key_path: Option<String>,
    cert_exists: bool,
    key_exists: bool,
    grpc_port_start: u16,
    grpc_port_end: u16,
}

#[derive(Debug, Serialize)]
struct HelperProbeResult {
    connected: bool,
    port: Option<u16>,
    errors: Vec<String>,
    session: Option<SessionSummary>,
    user: Option<UserSummary>,
    preferences: Option<UserPreferencesSummary>,
}

#[derive(Debug, Serialize)]
struct UserSummary {
    username: String,
    email: String,
    sounds_status: String,
    credits: u64,
    sounds_plan: i32,
}

#[derive(Debug, Serialize)]
struct SessionSummary {
    has_token: bool,
    auth_type: String,
    sub_channel: String,
}

#[derive(Debug, Serialize, Clone)]
struct OwnedAuthStatus {
    signed_in: bool,
    username: String,
    email: String,
    credits: u64,
    expires_at: Option<u64>,
    keychain_consent: bool,
    helper_connected: bool,
    helper_has_token: bool,
    helper_synced: bool,
}

#[derive(Debug, Serialize)]
struct AuthShakedownStatus {
    keychain_consent: bool,
    owned_signed_in: bool,
    owned_username: String,
    owned_email: String,
    owned_expires_at: Option<u64>,
    helper_connected: bool,
    helper_has_token: bool,
    helper_auth_type: String,
    helper_username: String,
    helper_email: String,
    helper_synced: bool,
    sync_attempted: bool,
    errors: Vec<String>,
}

#[derive(Debug, Clone)]
struct OwnedAuthSession {
    access_token: String,
    expires_at: u64,
    profile: SpliceProfile,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct SpliceProfile {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    uuid: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    avatar_url: String,
    #[serde(default)]
    credits: u64,
    #[serde(default)]
    sounds_plan: i32,
    #[serde(default)]
    sounds_state: String,
    #[serde(default)]
    sounds_status: String,
    #[serde(default)]
    channel: String,
    #[serde(default)]
    pubnub_key: String,
    #[serde(default)]
    features: HashMap<String, bool>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_in: u64,
    #[serde(default)]
    id_token: Option<String>,
}

static OWNED_AUTH: OnceLock<Mutex<Option<OwnedAuthSession>>> = OnceLock::new();
static KEYCHAIN_CONSENT: OnceLock<Mutex<bool>> = OnceLock::new();
static HELPER_LAST_LAUNCH_MS: OnceLock<Mutex<u64>> = OnceLock::new();

fn owned_auth_state() -> &'static Mutex<Option<OwnedAuthSession>> {
    OWNED_AUTH.get_or_init(|| Mutex::new(None))
}

fn keychain_consent_state() -> &'static Mutex<bool> {
    KEYCHAIN_CONSENT.get_or_init(|| Mutex::new(false))
}

fn set_keychain_consent(consent: bool) -> Result<(), String> {
    *keychain_consent_state().lock().map_err(|_| "Auth consent lock poisoned".to_string())? = consent;
    Ok(())
}

fn has_keychain_consent() -> bool {
    keychain_consent_state().lock().map(|consent| *consent).unwrap_or(false)
}

#[derive(Debug, Serialize)]
struct UserPreferencesSummary {
    splice_folder_path: String,
    save_outside_splice_folder: bool,
    sample_import_directories: Vec<String>,
    preset_locations: usize,
}

#[derive(Debug, Serialize)]
struct SampleSearchResult {
    total_hits: i32,
    samples: Vec<SampleSummary>,
    matching_tags: HashMap<String, i32>,
}

#[derive(Debug, Serialize)]
struct SampleSummary {
    file_hash: String,
    filename: String,
    local_path: String,
    bpm: i64,
    key: String,
    sample_type: String,
    genre: String,
    provider_name: String,
    price: i64,
    purchased: bool,
    tags: Vec<String>,
    preview_url: String,
    waveform_url: String,
    duration: i64,
    pack_uuid: String,
    pack_name: String,
    pack_cover_url: String,
}

#[derive(Debug, Serialize)]
struct PackSummary {
    uuid: String,
    name: String,
    cover_url: String,
    banner_url: String,
    demo_url: String,
    genre: String,
    provider_name: String,
    permalink: String,
    sample_count: Option<i64>,
}

#[derive(Debug, Serialize)]
struct CollectionSummary {
    uuid: String,
    name: String,
    description: String,
    cover_url: String,
    permalink: String,
    sample_count: i32,
    pack_count: i32,
    created_by_current_user: bool,
    creator_username: String,
}

#[derive(Debug, Serialize)]
struct DownloadSampleResult {
    requested: bool,
    sample: Option<SampleSummary>,
}

#[derive(Debug, Serialize)]
struct WasmCandidate {
    path: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct DiagnosticsInfo {
    app_version: String,
    keychain_consent: bool,
    owned_signed_in: bool,
    owned_username: String,
    owned_email: String,
    helper_connected: bool,
    helper_has_token: bool,
    helper_auth_type: String,
    helper_username: String,
    helper_email: String,
    helper_synced: bool,
    environment: SpliceEnvironmentStatus,
    errors: Vec<String>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn splice_user_data_path() -> Option<PathBuf> {
    home_dir().map(|home| home.join("Library/Application Support/com.splice.Splice"))
}

fn splice_cert_path() -> Option<PathBuf> {
    splice_user_data_path().map(|p| p.join(".certs/cert.pem"))
}

fn splice_helper_path() -> PathBuf {
    PathBuf::from("/Applications/Splice.app/Contents/Resources/desktop-helper")
}

fn splice_legacy_helper_path() -> PathBuf {
    PathBuf::from("/Applications/Splice.app/Contents/Resources/Splice Helper.app/Contents/MacOS/Splice Helper")
}

fn helper_last_launch_state() -> &'static Mutex<u64> {
    HELPER_LAST_LAUNCH_MS.get_or_init(|| Mutex::new(0))
}

#[derive(Default)]
struct SupuraisuElectronHost;

fn session_auth_user_from_memory() -> (Option<Auth>, Option<User>) {
    let session = owned_auth_state().lock().ok().and_then(|state| state.clone());
    let Some(session) = session else { return (None, None); };
    let user = User {
        id: session.profile.id,
        username: session.profile.username.clone(),
        bio: String::new(),
        avatar_url: session.profile.avatar_url.clone(),
        location: String::new(),
        email: session.profile.email.clone(),
        sounds_status: if session.profile.sounds_status.is_empty() { session.profile.sounds_state.clone() } else { session.profile.sounds_status.clone() },
        credits: session.profile.credits,
        features: session.profile.features.clone(),
        sounds_plan: session.profile.sounds_plan,
        uuid: session.profile.uuid.clone(),
    };
    let auth = Auth {
        token: session.access_token,
        sub_key: session.profile.pubnub_key,
        sub_channel: session.profile.channel,
        auth_type: AuthType::Auth0 as i32,
    };
    (Some(auth), Some(user))
}

#[tonic::async_trait]
impl Electron for SupuraisuElectronHost {
    async fn helper_started(
        &self,
        request: tonic::Request<HelperStartedMessageRequest>,
    ) -> Result<tonic::Response<HelperStartedMessageResponse>, tonic::Status> {
        let port = request.into_inner().port;
        eprintln!("Supuraisu hosted Splice Helper started on local gRPC port {port}");
        Ok(tonic::Response::new(HelperStartedMessageResponse { message: "ok".to_string() }))
    }

    async fn get_electron_session(
        &self,
        _request: tonic::Request<Empty>,
    ) -> Result<tonic::Response<GetElectronSessionResponse>, tonic::Status> {
        let (auth, user) = session_auth_user_from_memory();
        Ok(tonic::Response::new(GetElectronSessionResponse { auth, user }))
    }

    async fn logout(&self, _request: tonic::Request<Empty>) -> Result<tonic::Response<Empty>, tonic::Status> {
        Ok(tonic::Response::new(Empty {}))
    }

    async fn refresh_auth(&self, _request: tonic::Request<Empty>) -> Result<tonic::Response<Empty>, tonic::Status> {
        Ok(tonic::Response::new(Empty {}))
    }
}

async fn start_supuraisu_electron_server() -> Result<u16, String> {
    let cert_path = splice_cert_path().ok_or_else(|| "Could not resolve Splice cert path".to_string())?;
    let key_path = splice_user_data_path()
        .map(|p| p.join(".certs/key.pem"))
        .ok_or_else(|| "Could not resolve Splice key path".to_string())?;
    let cert = tokio::fs::read(&cert_path)
        .await
        .map_err(|e| format!("Could not read Splice cert {}: {e}", cert_path.display()))?;
    let key = tokio::fs::read(&key_path)
        .await
        .map_err(|e| format!("Could not read Splice key {}: {e}", key_path.display()))?;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Could not bind Supuraisu helper host server: {e}"))?;
    let port = listener.local_addr().map_err(|e| format!("Could not read helper host server port: {e}"))?.port();
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
    let identity = Identity::from_pem(cert, key);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = Server::builder()
            .tls_config(ServerTlsConfig::new().identity(identity))
            .expect("valid helper host TLS config")
            .add_service(ElectronServer::new(SupuraisuElectronHost))
            .serve_with_incoming(incoming)
            .await
        {
            eprintln!("Supuraisu helper host server stopped: {e}");
        }
    });
    Ok(port)
}

async fn launch_splice_helper_once() -> Result<bool, String> {
    let now = now_ms();
    {
        let mut last_launch = helper_last_launch_state()
            .lock()
            .map_err(|_| "Helper launch lock poisoned".to_string())?;
        if now.saturating_sub(*last_launch) < 5_000 {
            return Ok(false);
        }
        *last_launch = now;
    }

    let helper_path = splice_legacy_helper_path();
    if !helper_path.exists() {
        return Err(format!("Splice Helper.app binary not found at {}", helper_path.display()));
    }
    let cert_dir = splice_user_data_path()
        .map(|p| p.join(".certs"))
        .ok_or_else(|| "Could not resolve Splice cert directory".to_string())?;
    let server_port = start_supuraisu_electron_server().await?;

    Command::new(&helper_path)
        .arg("-pid")
        .arg(std::process::id().to_string())
        .arg("-electronPort")
        .arg(server_port.to_string())
        .arg("-certPath")
        .arg(&cert_dir)
        .arg("-releaseChannel")
        .arg("stable")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not launch Splice Helper.app: {e}"))?;
    Ok(true)
}

#[tauri::command]
fn splice_environment_status() -> SpliceEnvironmentStatus {
    let app_path = PathBuf::from("/Applications/Splice.app");
    let helper_path = splice_helper_path();

    let user_data_path = splice_user_data_path();
    let cert_path = user_data_path.as_ref().map(|p| p.join(".certs/cert.pem"));
    let key_path = user_data_path.as_ref().map(|p| p.join(".certs/key.pem"));

    SpliceEnvironmentStatus {
        app_path: app_path.display().to_string(),
        app_installed: app_path.exists(),
        helper_path: helper_path.display().to_string(),
        helper_installed: helper_path.exists(),
        user_data_path: user_data_path.as_ref().map(|p| p.display().to_string()),
        cert_path: cert_path.as_ref().map(|p| p.display().to_string()),
        key_path: key_path.as_ref().map(|p| p.display().to_string()),
        cert_exists: cert_path.as_ref().is_some_and(|p| p.exists()),
        key_exists: key_path.as_ref().is_some_and(|p| p.exists()),
        grpc_port_start: SPLICE_GRPC_PORT_START,
        grpc_port_end: SPLICE_GRPC_PORT_END,
    }
}

#[derive(Debug)]
struct PinnedLocalCertVerifier {
    expected_der: Vec<u8>,
}

impl ServerCertVerifier for PinnedLocalCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if end_entity.as_ref() == self.expected_der.as_slice() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "server certificate did not match pinned Splice local cert".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ED25519,
        ]
    }
}

#[derive(Clone)]
struct SpliceTlsConnector {
    port: u16,
    config: Arc<rustls::ClientConfig>,
}

impl Service<Uri> for SpliceTlsConnector {
    type Response = TokioIo<TlsStream<TcpStream>>;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: Uri) -> Self::Future {
        let port = self.port;
        let config = self.config.clone();
        Box::pin(async move {
            let stream = TcpStream::connect(("127.0.0.1", port)).await?;
            let server_name = ServerName::try_from("127.0.0.1")
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?
                .to_owned();
            let tls_stream = TlsConnector::from(config)
                .connect(server_name, stream)
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(TokioIo::new(tls_stream))
        })
    }
}

async fn connect_helper(port: u16, cert_pem: Vec<u8>) -> anyhow::Result<AppClient<Channel>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let pinned_cert = CertificateDer::from_pem_slice(&cert_pem)?.into_owned();
    let verifier = Arc::new(PinnedLocalCertVerifier {
        expected_der: pinned_cert.as_ref().to_vec(),
    });
    let mut config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec()];

    // The custom connector below performs TLS itself. Tonic's Endpoint must use
    // an `http://` URI here; otherwise it rejects the connection before calling
    // our connector because no built-in Tonic TLS config was supplied.
    let endpoint = Endpoint::from_shared(format!("http://127.0.0.1:{port}"))?
        .connect_timeout(Duration::from_millis(350))
        .timeout(Duration::from_secs(2));
    let channel = endpoint
        .connect_with_connector(SpliceTlsConnector {
            port,
            config: Arc::new(config),
        })
        .await?;
    Ok(AppClient::new(channel))
}

async fn scan_helper_client(cert_pem: &[u8]) -> Option<(u16, AppClient<Channel>, Vec<String>)> {
    let mut errors = Vec::new();
    for port in SPLICE_GRPC_PORT_START..=SPLICE_GRPC_PORT_END {
        match connect_helper(port, cert_pem.to_vec()).await {
            Ok(client) => return Some((port, client, errors)),
            Err(e) => errors.push(format!("{port}: {e:?}")),
        }
    }
    None
}

async fn find_helper_client() -> anyhow::Result<(u16, AppClient<Channel>, Vec<String>)> {
    let cert_path = splice_cert_path().ok_or_else(|| anyhow::anyhow!("Could not resolve Splice cert path"))?;
    let cert_pem = std::fs::read(&cert_path).map_err(|e| {
        anyhow::anyhow!("Could not read Splice cert at {}: {e}", cert_path.display())
    })?;

    if let Some(found) = scan_helper_client(&cert_pem).await {
        return Ok(found);
    }

    let mut errors = Vec::new();
    match launch_splice_helper_once().await {
        Ok(true) => errors.push("Launched Splice Helper hosted by Supuraisu".to_string()),
        Ok(false) => errors.push("Splice helper launch was throttled briefly".to_string()),
        Err(e) => errors.push(e),
    }

    for _ in 0..16 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if let Some((port, client, scan_errors)) = scan_helper_client(&cert_pem).await {
            errors.extend(scan_errors);
            return Ok((port, client, errors));
        }
    }

    Err(anyhow::anyhow!("no Splice helper found; errors: {}", errors.join("\n")))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn random_string(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn signed_out_auth_status() -> OwnedAuthStatus {
    OwnedAuthStatus {
        signed_in: false,
        username: String::new(),
        email: String::new(),
        credits: 0,
        expires_at: None,
        keychain_consent: has_keychain_consent(),
        helper_connected: false,
        helper_has_token: false,
        helper_synced: false,
    }
}

fn auth_status_from_session(session: &OwnedAuthSession) -> OwnedAuthStatus {
    OwnedAuthStatus {
        signed_in: true,
        username: session.profile.username.clone(),
        email: session.profile.email.clone(),
        credits: session.profile.credits,
        expires_at: Some(session.expires_at),
        keychain_consent: has_keychain_consent(),
        helper_connected: false,
        helper_has_token: false,
        helper_synced: false,
    }
}

fn refresh_token_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(AUTH_KEYCHAIN_SERVICE, AUTH_KEYCHAIN_ACCOUNT)
        .map_err(|e| format!("Could not open Keychain entry: {e}"))
}

fn read_refresh_token() -> Option<String> {
    refresh_token_entry().ok()?.get_password().ok().filter(|v| !v.is_empty())
}

fn store_refresh_token(token: &str) -> Result<(), String> {
    refresh_token_entry()?.set_password(token).map_err(|e| format!("Could not store refresh token: {e}"))
}

fn delete_refresh_token() -> Result<(), String> {
    match refresh_token_entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Could not delete refresh token: {e}")),
    }
}

fn value_string(value: &serde_json::Value, key: &str) -> String {
    value.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_string()
}

fn profile_from_splice_session_value(value: serde_json::Value) -> SpliceProfile {
    let features = match value.get("features") {
        Some(serde_json::Value::Object(map)) => map
            .iter()
            .filter_map(|(key, value)| value.as_bool().map(|enabled| (key.clone(), enabled)))
            .collect(),
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(|name| (name.to_string(), true)))
            .collect(),
        _ => HashMap::new(),
    };
    SpliceProfile {
        id: value.get("id").and_then(|v| v.as_u64()).unwrap_or_default(),
        uuid: value_string(&value, "uuid"),
        username: value_string(&value, "username"),
        email: value_string(&value, "email"),
        avatar_url: value_string(&value, "avatar_url"),
        credits: value.get("credits").and_then(|v| v.as_u64()).unwrap_or_default(),
        sounds_plan: value.get("sounds_plan").and_then(|v| v.as_i64()).unwrap_or_default() as i32,
        sounds_state: value_string(&value, "sounds_state"),
        sounds_status: value_string(&value, "sounds_status"),
        channel: value_string(&value, "channel"),
        pubnub_key: value_string(&value, "pubnub_key"),
        features,
    }
}

fn profile_from_auth0_value(value: serde_json::Value) -> SpliceProfile {
    let email = value.get("email").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let username = value
        .get("nickname")
        .or_else(|| value.get("name"))
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| email.split('@').next().unwrap_or("Splice user").to_string());
    let sub = value.get("sub").and_then(|v| v.as_str()).unwrap_or_default();
    let uuid = sub.rsplit('|').next().unwrap_or(sub).to_string();
    SpliceProfile {
        uuid,
        username,
        email,
        avatar_url: value.get("picture").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        ..Default::default()
    }
}

fn profile_from_id_token(id_token: Option<&str>) -> Option<SpliceProfile> {
    let payload = id_token?.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let value = serde_json::from_slice::<serde_json::Value>(&bytes).ok()?;
    Some(profile_from_auth0_value(value))
}

async fn fetch_splice_profile(access_token: &str, id_token: Option<&str>) -> Result<SpliceProfile, String> {
    let client = reqwest::Client::new();
    let session_value = client
        .get("https://api.splice.com/session")
        .bearer_auth(access_token)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Profile request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Profile HTTP error: {e}"))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Profile parse failed: {e}"))?;

    if session_value.is_object() {
        return Ok(profile_from_splice_session_value(session_value));
    }

    let userinfo = client
        .get(format!("https://{AUTH_DOMAIN}/userinfo"))
        .bearer_auth(access_token)
        .header("accept", "application/json")
        .send()
        .await
        .ok()
        .and_then(|response| response.error_for_status().ok());
    if let Some(response) = userinfo {
        if let Ok(value) = response.json::<serde_json::Value>().await {
            if value.is_object() {
                return Ok(profile_from_auth0_value(value));
            }
        }
    }

    profile_from_id_token(id_token).ok_or_else(|| "Splice profile was empty after login".to_string())
}

async fn exchange_refresh_token(refresh_token: &str) -> Result<OwnedAuthSession, String> {
    let response = reqwest::Client::new()
        .post(format!("https://{AUTH_DOMAIN}/oauth/token"))
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "client_id": AUTH_CLIENT_ID,
            "refresh_token": refresh_token,
        }))
        .send()
        .await
        .map_err(|e| format!("Token refresh failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Token refresh HTTP error: {e}"))?
        .json::<TokenResponse>()
        .await
        .map_err(|e| format!("Token refresh parse failed: {e}"))?;
    let refresh_token = response.refresh_token.unwrap_or_else(|| refresh_token.to_string());
    store_refresh_token(&refresh_token)?;
    let profile = fetch_splice_profile(&response.access_token, response.id_token.as_deref()).await?;
    Ok(OwnedAuthSession {
        access_token: response.access_token,
        expires_at: now_ms() + response.expires_in.saturating_mul(1000),
        profile,
    })
}

async fn exchange_auth_code(code: &str, verifier: &str) -> Result<OwnedAuthSession, String> {
    let response = reqwest::Client::new()
        .post(format!("https://{AUTH_DOMAIN}/oauth/token"))
        .json(&serde_json::json!({
            "grant_type": "authorization_code",
            "client_id": AUTH_CLIENT_ID,
            "code": code,
            "redirect_uri": AUTH_REDIRECT_URI,
            "code_verifier": verifier,
        }))
        .send()
        .await
        .map_err(|e| format!("Token exchange failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Token exchange HTTP error: {e}"))?
        .json::<TokenResponse>()
        .await
        .map_err(|e| format!("Token exchange parse failed: {e}"))?;
    let refresh_token = response.refresh_token.ok_or_else(|| "Login did not return a refresh token".to_string())?;
    store_refresh_token(&refresh_token)?;
    let profile = fetch_splice_profile(&response.access_token, response.id_token.as_deref()).await?;
    Ok(OwnedAuthSession {
        access_token: response.access_token,
        expires_at: now_ms() + response.expires_in.saturating_mul(1000),
        profile,
    })
}

async fn send_owned_session_to_helper(session: &OwnedAuthSession) -> Result<(), String> {
    let (_port, mut client, _errors) = find_helper_client().await.map_err(|e| format!("{e:?}"))?;
    let user = User {
        id: session.profile.id,
        username: session.profile.username.clone(),
        bio: String::new(),
        avatar_url: session.profile.avatar_url.clone(),
        location: String::new(),
        email: session.profile.email.clone(),
        sounds_status: if session.profile.sounds_status.is_empty() { session.profile.sounds_state.clone() } else { session.profile.sounds_status.clone() },
        credits: session.profile.credits,
        features: session.profile.features.clone(),
        sounds_plan: session.profile.sounds_plan,
        uuid: session.profile.uuid.clone(),
    };
    let auth = Auth {
        token: session.access_token.clone(),
        sub_key: session.profile.pubnub_key.clone(),
        sub_channel: session.profile.channel.clone(),
        auth_type: AuthType::Auth0 as i32,
    };
    let logged_in = client
        .logged_in(LoggedInRequest { auth: Some(auth.clone()), user: Some(user.clone()) })
        .await
        .map(|_| ())
        .map_err(|e| format!("Helper LoggedIn failed: {e:?}"));
    let updated = client
        .updated_session(UpdatedSessionRequest { auth: Some(auth), user: Some(user) })
        .await
        .map(|_| ())
        .map_err(|e| format!("Helper UpdatedSession failed: {e:?}"));

    match (logged_in, updated) {
        (Ok(()), _) | (_, Ok(())) => Ok(()),
        (Err(a), Err(b)) => Err(format!("{a}; {b}")),
    }
}

async fn ensure_owned_auth_session() -> Result<Option<OwnedAuthSession>, String> {
    if let Some(session) = owned_auth_state().lock().map_err(|_| "Auth state lock poisoned".to_string())?.clone() {
        if now_ms() + 30_000 < session.expires_at {
            return Ok(Some(session));
        }
    }
    if !has_keychain_consent() {
        return Ok(None);
    }
    let Some(refresh_token) = read_refresh_token() else { return Ok(None); };
    match exchange_refresh_token(&refresh_token).await {
        Ok(session) => {
            *owned_auth_state().lock().map_err(|_| "Auth state lock poisoned".to_string())? = Some(session.clone());
            let _ = send_owned_session_to_helper(&session).await;
            Ok(Some(session))
        }
        Err(e) => {
            // Keep the refresh token on transient network/server failures. The
            // user can explicitly sign out to clear Keychain; a later launch may
            // refresh successfully without forcing a new login.
            *owned_auth_state().lock().map_err(|_| "Auth state lock poisoned".to_string())? = None;
            Err(e)
        }
    }
}

async fn helper_access_token() -> Result<String, String> {
    let (_port, mut client, _errors) = find_helper_client().await.map_err(|e| format!("{e:?}"))?;
    let session = client
        .get_session(GetSessionRequest {})
        .await
        .map_err(|e| format!("GetSession failed: {e:?}"))?
        .into_inner();
    session
        .auth
        .map(|auth| auth.token)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| "No authenticated Splice session token available".to_string())
}

async fn splice_access_token() -> Result<String, String> {
    match ensure_owned_auth_session().await {
        Ok(Some(session)) => Ok(session.access_token),
        Ok(None) => helper_access_token().await,
        Err(owned_error) => helper_access_token()
            .await
            .map_err(|helper_error| format!("Supuraisu auth failed: {owned_error}; helper auth failed: {helper_error}")),
    }
}

fn summarize_sample_with_context(
    sample: Sample,
    local_files: Option<&HashMap<String, Vec<String>>>,
    packs: Option<&HashMap<String, SamplePack>>,
) -> SampleSummary {
    let pack = packs.and_then(|packs| packs.get(&sample.pack_uuid));
    let local_path = if sample.local_path.is_empty() {
        local_files
            .and_then(|files| files.get(&sample.filename))
            .and_then(|paths| {
                if paths.len() == 1 {
                    return paths.first().cloned();
                }
                let pack_name = pack.map(|pack| pack.name.to_ascii_lowercase()).unwrap_or_default();
                if pack_name.is_empty() {
                    return None;
                }
                let matches: Vec<_> = paths
                    .iter()
                    .filter(|path| path.to_ascii_lowercase().contains(&pack_name))
                    .cloned()
                    .collect();
                (matches.len() == 1).then(|| matches[0].clone())
            })
            .unwrap_or_default()
    } else {
        sample.local_path
    };

    SampleSummary {
        file_hash: sample.file_hash,
        filename: sample.filename,
        local_path,
        bpm: sample.bpm,
        key: sample.audio_key,
        sample_type: sample.sample_type,
        genre: sample.genre,
        provider_name: sample.provider_name,
        price: sample.price,
        purchased: sample.purchased_at > 0,
        tags: sample.tags,
        preview_url: sample.preview_url,
        waveform_url: sample.waveform_url,
        duration: sample.duration,
        pack_uuid: sample.pack_uuid,
        pack_name: pack.map(|pack| pack.name.clone()).unwrap_or_default(),
        pack_cover_url: pack.map(|pack| pack.cover_url.clone()).unwrap_or_default(),
    }
}

fn summarize_sample(sample: Sample) -> SampleSummary {
    summarize_sample_with_context(sample, None, None)
}

fn summarize_helper_pack(pack: SamplePack) -> PackSummary {
    PackSummary {
        uuid: pack.uuid,
        name: pack.name,
        cover_url: pack.cover_url,
        banner_url: String::new(),
        demo_url: String::new(),
        genre: pack.genre,
        provider_name: pack.provider_name,
        permalink: pack.permalink,
        sample_count: None,
    }
}

async fn fetch_all_helper_packs(client: &mut AppClient<Channel>) -> Vec<SamplePack> {
    let mut next_token = 0;
    let mut packs = Vec::new();
    for _ in 0..50 {
        let Ok(response) = client
            .list_sample_packs(ListSamplePacksRequest { next_token })
            .await
            .map(|r| r.into_inner())
        else {
            break;
        };
        packs.extend(response.sample_packs);
        if response.next_token == 0 || response.next_token == next_token {
            break;
        }
        next_token = response.next_token;
    }
    packs
}

fn index_local_audio_files(splice_folder: &str) -> HashMap<String, Vec<String>> {
    fn visit(dir: &Path, files: &mut HashMap<String, Vec<String>>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return; };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, files);
            } else if path.is_file() {
                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    files.entry(name.to_string()).or_default().push(path.display().to_string());
                }
            }
        }
    }

    let mut files = HashMap::new();
    let root = Path::new(splice_folder).join("sounds");
    visit(&root, &mut files);
    files
}

#[tauri::command]
async fn probe_splice_helper() -> Result<HelperProbeResult, String> {
    let (port, mut client, mut errors) = match find_helper_client().await {
        Ok(found) => found,
        Err(e) => {
            return Ok(HelperProbeResult {
                connected: false,
                port: None,
                errors: vec![format!("{e:?}")],
                session: None,
                user: None,
                preferences: None,
            })
        }
    };

    let session = match client.get_session(GetSessionRequest {}).await {
        Ok(response) => response.into_inner().auth.map(|auth| SessionSummary {
            has_token: !auth.token.is_empty(),
            auth_type: match auth.auth_type {
                1 => "AUTH0".to_string(),
                0 => "SPLICE".to_string(),
                other => format!("UNKNOWN({other})"),
            },
            sub_channel: auth.sub_channel,
        }),
        Err(e) => {
            errors.push(format!("{port}: GetSession failed: {e:?}"));
            None
        }
    };

    let user = match client.validate_login(ValidateLoginRequest {}).await {
        Ok(response) => response.into_inner().user.map(|user| UserSummary {
            username: user.username,
            email: user.email,
            sounds_status: user.sounds_status,
            credits: user.credits,
            sounds_plan: user.sounds_plan,
        }),
        Err(e) => {
            errors.push(format!("{port}: ValidateLogin failed: {e:?}"));
            None
        }
    };

    let preferences = match client.user_preferences(UserPreferencesRequest {}).await {
        Ok(response) => response.into_inner().preferences.map(|prefs| {
            UserPreferencesSummary {
                splice_folder_path: prefs.splice_folder_path,
                save_outside_splice_folder: prefs.save_outside_splice_folder,
                sample_import_directories: prefs.sample_import_directories,
                preset_locations: prefs.presets.len(),
            }
        }),
        Err(e) => {
            errors.push(format!("{port}: UserPreferences failed: {e:?}"));
            None
        }
    };

    Ok(HelperProbeResult {
        connected: true,
        port: Some(port),
        errors,
        session,
        user,
        preferences,
    })
}

fn normalize_search_query(query: &str) -> String {
    let trimmed = query.trim();
    for ext in [".wav", ".aif", ".aiff", ".mp3"] {
        if trimmed.to_ascii_lowercase().ends_with(ext) {
            return trimmed[..trimmed.len() - ext.len()].to_string();
        }
    }
    trimmed.to_string()
}

#[tauri::command]
async fn search_splice_samples(
    query: String,
    only_purchased: Option<bool>,
    page: Option<i32>,
    per_page: Option<i32>,
    pack_uuid: Option<String>,
    liked: Option<bool>,
    genre: Option<String>,
    instrument: Option<String>,
    key: Option<String>,
    sample_type: Option<String>,
    bpm_min: Option<i32>,
    bpm_max: Option<i32>,
    sort_fn: Option<String>,
    tags: Option<Vec<String>>,
) -> Result<SampleSearchResult, String> {
    let (_port, mut client, _errors) = find_helper_client().await.map_err(|e| format!("{e:?}"))?;
    let purchased = if only_purchased.unwrap_or(false) {
        search_sample_request::PurchasedTypes::OnlyPurchased
    } else {
        search_sample_request::PurchasedTypes::All
    };

    let local_files = client
        .user_preferences(UserPreferencesRequest {})
        .await
        .ok()
        .and_then(|r| r.into_inner().preferences)
        .map(|prefs| index_local_audio_files(&prefs.splice_folder_path));

    let pack_uuid = pack_uuid.unwrap_or_default();

    let response = client
        .search_samples(SearchSampleRequest {
            liked: liked.unwrap_or(false),
            purchased: purchased as i32,
            matching_tags_and_packs: true,
            search_term: normalize_search_query(&query),
            collection_uuid: String::new(),
            sort_fn: sort_fn.unwrap_or_default(),
            bpm_min: bpm_min.unwrap_or_default(),
            bpm_max: bpm_max.unwrap_or_default(),
            tags: tags.unwrap_or_default(),
            file_hash: String::new(),
            genre: genre.unwrap_or_default(),
            instrument: instrument.unwrap_or_default(),
            key: key.unwrap_or_default(),
            sample_type: sample_type.unwrap_or_default(),
            pack_uuid,
            chord_type: String::new(),
            per_page: per_page.unwrap_or(12).clamp(1, 50),
            page: page.unwrap_or(1).max(1),
            random_seed: String::new(),
        })
        .await
        .map_err(|e| format!("SearchSamples failed: {e:?}"))?
        .into_inner();

    let pack_index: HashMap<String, SamplePack> = response
        .matching_packs
        .into_iter()
        .map(|pack| (pack.uuid.clone(), pack))
        .collect();

    Ok(SampleSearchResult {
        total_hits: response.total_hits,
        matching_tags: response.matching_tags,
        samples: response
            .samples
            .into_iter()
            .map(|sample| summarize_sample_with_context(sample, local_files.as_ref(), Some(&pack_index)))
            .collect(),
    })
}

#[cfg(target_os = "macos")]
#[allow(deprecated, unexpected_cfgs)]
mod macos_file_drag {
    use cocoa::{base::nil, foundation::{NSPoint, NSRect, NSSize, NSString}};
    use objc::{
        class, msg_send, sel, sel_impl,
        declare::ClassDecl,
        runtime::{Class, Object, Protocol, Sel},
    };
    use std::{path::Path, sync::OnceLock};

    type Id = *mut Object;

    extern "C" fn source_operation_mask(_: &Object, _: Sel, _: Id, _: i64) -> u64 {
        1 // NSDragOperationCopy
    }

    fn dragging_source_class() -> &'static Class {
        static CLASS: OnceLock<&'static Class> = OnceLock::new();
        CLASS.get_or_init(|| {
            let superclass = class!(NSObject);
            let mut decl = ClassDecl::new("SupuraisuDraggingSource", superclass)
                .expect("SupuraisuDraggingSource class should only be registered once");
            let protocol = Protocol::get("NSDraggingSource").expect("NSDraggingSource protocol");
            decl.add_protocol(protocol);
            unsafe {
                decl.add_method(
                    sel!(draggingSession:sourceOperationMaskForDraggingContext:),
                    source_operation_mask as extern "C" fn(&Object, Sel, Id, i64) -> u64,
                );
            }
            decl.register()
        })
    }

    pub unsafe fn begin_file_drag(ns_view: Id, path: &str) -> Result<(), String> {
        if !Path::new(path).is_file() {
            return Err(format!("Not a file: {path}"));
        }

        let app: Id = msg_send![class!(NSApplication), sharedApplication];
        let event: Id = msg_send![app, currentEvent];
        if event.is_null() {
            return Err("No current AppKit mouse event available for drag".to_string());
        }

        let ns_path = NSString::alloc(nil).init_str(path);
        let url: Id = msg_send![class!(NSURL), fileURLWithPath: ns_path];
        let item: Id = msg_send![class!(NSDraggingItem), alloc];
        let item: Id = msg_send![item, initWithPasteboardWriter: url];

        let workspace: Id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let icon: Id = msg_send![workspace, iconForFile: ns_path];
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(64.0, 64.0));
        let _: () = msg_send![item, setDraggingFrame: frame contents: icon];

        let items: Id = msg_send![class!(NSArray), arrayWithObject: item];
        let source: Id = msg_send![dragging_source_class(), new];
        let _: Id = msg_send![ns_view, beginDraggingSessionWithItems: items event: event source: source];
        Ok(())
    }
}

#[tauri::command]
async fn download_splice_sample(file_hash: String) -> Result<DownloadSampleResult, String> {
    if file_hash.trim().is_empty() {
        return Err("file_hash is required".to_string());
    }

    let (_port, mut client, _errors) = find_helper_client().await.map_err(|e| format!("{e:?}"))?;
    client
        .download_sample(DownloadSampleRequest {
            file_hash: file_hash.clone(),
        })
        .await
        .map_err(|e| format!("DownloadSample failed: {e:?}"))?;

    // DownloadSample returns when the helper has accepted the request, not
    // necessarily when the file is already present on disk. Poll SampleInfo so
    // the UI updates after the first click instead of requiring a second click.
    let mut last_sample = None;
    for _ in 0..60 {
        let sample = client
            .sample_info(SampleInfoRequest {
                local_path: String::new(),
                file_hash: file_hash.clone(),
                audio_hash: String::new(),
            })
            .await
            .ok()
            .and_then(|r| r.into_inner().sample);

        if let Some(sample) = sample {
            let local_path = sample.local_path.clone();
            last_sample = Some(sample);
            if !local_path.is_empty() && std::path::Path::new(&local_path).exists() {
                break;
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Ok(DownloadSampleResult {
        requested: true,
        sample: last_sample.map(summarize_sample),
    })
}

#[tauri::command]
async fn list_collections(page: Option<i32>, per_page: Option<i32>) -> Result<Vec<CollectionSummary>, String> {
    let token = splice_access_token().await?;

    let query = r#"
      query SupuraisuCollectionsList($page: Int = 1, $limit: Int = 60) {
        assetsSearch(
          filter: {legacy: true, asset_type_slug: collection, liked: true}
          pagination: {page: $page, limit: $limit}
          sort: {sort: relevance}
        ) {
          items {
            __typename
            ... on CollectionAsset {
              uuid
              name
              public
              permalink_slug
              creator { username }
              child_asset_counts { type count }
              files { asset_file_type_slug url uuid }
            }
          }
        }
      }
    "#;

    let body = serde_json::json!({
        "operationName": "SupuraisuCollectionsList",
        "query": query,
        "variables": {
            "page": page.unwrap_or(1).max(1),
            "limit": per_page.unwrap_or(60).clamp(1, 100),
        }
    });

    let response: serde_json::Value = reqwest::Client::new()
        .post("https://surfaces-graphql.splice.com/graphql")
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Collections request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Collections HTTP error: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Collections response parse failed: {e}"))?;

    if let Some(errors) = response.get("errors") {
        return Err(format!("Collections returned errors: {errors}"));
    }

    let items = response
        .pointer("/data/assetsSearch/items")
        .and_then(|items| items.as_array())
        .ok_or_else(|| "Collections response did not include assetsSearch.items".to_string())?;

    Ok(items
        .iter()
        .filter_map(|item| {
            let files = item.get("files").and_then(|files| files.as_array());
            let cover_url = files
                .and_then(|files| {
                    files
                        .iter()
                        .find(|file| file.get("asset_file_type_slug").and_then(|v| v.as_str()) == Some("cover_image"))
                })
                .and_then(|file| file.get("url"))
                .and_then(|url| url.as_str())
                .unwrap_or_default()
                .to_string();
            let sample_count = item
                .get("child_asset_counts")
                .and_then(|counts| counts.as_array())
                .and_then(|counts| counts.iter().find(|count| count.get("type").and_then(|v| v.as_str()) == Some("sample")))
                .and_then(|count| count.get("count"))
                .and_then(|count| count.as_i64())
                .unwrap_or_default() as i32;
            let pack_count = item
                .get("child_asset_counts")
                .and_then(|counts| counts.as_array())
                .and_then(|counts| counts.iter().find(|count| count.get("type").and_then(|v| v.as_str()) == Some("pack")))
                .and_then(|count| count.get("count"))
                .and_then(|count| count.as_i64())
                .unwrap_or_default() as i32;

            Some(CollectionSummary {
                uuid: item.get("uuid")?.as_str()?.to_string(),
                name: item.get("name")?.as_str()?.to_string(),
                description: String::new(),
                cover_url,
                permalink: item.get("permalink_slug").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                sample_count,
                pack_count,
                created_by_current_user: item.get("public").and_then(|v| v.as_bool()).map(|public| !public).unwrap_or(false),
                creator_username: item.pointer("/creator/username").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
            })
        })
        .collect())
}

#[tauri::command]
async fn collection_samples(
    uuid: String,
    page: Option<i32>,
    per_page: Option<i32>,
    sample_type: Option<String>,
    bpm_min: Option<i32>,
    bpm_max: Option<i32>,
    sort_fn: Option<String>,
    tags: Option<Vec<String>>,
) -> Result<SampleSearchResult, String> {
    let (_port, mut client, _errors) = find_helper_client().await.map_err(|e| format!("{e:?}"))?;
    let local_files = client
        .user_preferences(UserPreferencesRequest {})
        .await
        .ok()
        .and_then(|r| r.into_inner().preferences)
        .map(|prefs| index_local_audio_files(&prefs.splice_folder_path));
    let response = client
        .search_samples(SearchSampleRequest {
            liked: false,
            purchased: search_sample_request::PurchasedTypes::All as i32,
            matching_tags_and_packs: true,
            search_term: String::new(),
            collection_uuid: uuid,
            sort_fn: sort_fn.unwrap_or_default(),
            bpm_min: bpm_min.unwrap_or_default(),
            bpm_max: bpm_max.unwrap_or_default(),
            tags: tags.unwrap_or_default(),
            file_hash: String::new(),
            genre: String::new(),
            instrument: String::new(),
            key: String::new(),
            sample_type: sample_type.unwrap_or_default(),
            pack_uuid: String::new(),
            chord_type: String::new(),
            per_page: per_page.unwrap_or(50).clamp(1, 100),
            page: page.unwrap_or(1).max(1),
            random_seed: String::new(),
        })
        .await
        .map_err(|e| format!("Collection search failed: {e:?}"))?
        .into_inner();

    let pack_index: HashMap<String, SamplePack> = response
        .matching_packs
        .into_iter()
        .map(|pack| (pack.uuid.clone(), pack))
        .collect();

    Ok(SampleSearchResult {
        total_hits: response.total_hits,
        matching_tags: response.matching_tags,
        samples: response
            .samples
            .into_iter()
            .map(|sample| summarize_sample_with_context(sample, local_files.as_ref(), Some(&pack_index)))
            .collect(),
    })
}

#[tauri::command]
async fn list_helper_packs() -> Result<Vec<PackSummary>, String> {
    let (_port, mut client, _errors) = find_helper_client().await.map_err(|e| format!("{e:?}"))?;
    Ok(fetch_all_helper_packs(&mut client)
        .await
        .into_iter()
        .map(summarize_helper_pack)
        .collect())
}

#[tauri::command]
async fn liked_splice_packs(limit: Option<i32>) -> Result<Vec<PackSummary>, String> {
    let (_port, mut client, _errors) = find_helper_client().await.map_err(|e| format!("{e:?}"))?;
    let mut seen = HashMap::<String, PackSummary>::new();
    let pages = limit.unwrap_or(500).clamp(50, 1_000);
    let per_page = 100;
    let max_pages = (pages + per_page - 1) / per_page;

    for page in 1..=max_pages {
        let response = client
            .search_samples(SearchSampleRequest {
                liked: true,
                purchased: search_sample_request::PurchasedTypes::OnlyPurchased as i32,
                matching_tags_and_packs: true,
                search_term: String::new(),
                collection_uuid: String::new(),
                sort_fn: "recency".to_string(),
                bpm_min: 0,
                bpm_max: 0,
                tags: Vec::new(),
                file_hash: String::new(),
                genre: String::new(),
                instrument: String::new(),
                key: String::new(),
                sample_type: String::new(),
                pack_uuid: String::new(),
                chord_type: String::new(),
                per_page,
                page,
                random_seed: String::new(),
            })
            .await
            .map_err(|e| format!("Liked packs search failed: {e:?}"))?
            .into_inner();

        for pack in response.matching_packs {
            seen.entry(pack.uuid.clone()).or_insert_with(|| summarize_helper_pack(pack));
        }
        if response.samples.len() < per_page as usize || seen.len() >= limit.unwrap_or(500) as usize {
            break;
        }
    }

    let mut packs: Vec<_> = seen.into_values().collect();
    packs.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    Ok(packs)
}

#[tauri::command]
async fn explore_splice_packs(limit: Option<i32>) -> Result<Vec<PackSummary>, String> {
    let token = splice_access_token().await?;

    let query = r#"
      query SupuraisuPackExplore($limit: Int = 12) {
        assetsSearch(
          filter: {asset_type_slug: pack}
          pagination: {page: 1, limit: $limit}
          sort: {sort: recommended, order: DESC}
        ) {
          items {
            __typename
            ... on PackAsset {
              uuid
              name
              permalink_base_url
              main_genre
              files { asset_file_type_slug url path }
              child_asset_counts { type count }
            }
          }
        }
      }
    "#;

    let body = serde_json::json!({
        "operationName": "SupuraisuPackExplore",
        "query": query,
        "variables": { "limit": limit.unwrap_or(12).clamp(1, 50) }
    });

    let response: serde_json::Value = reqwest::Client::new()
        .post("https://surfaces-graphql.splice.com/graphql")
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("GraphQL request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("GraphQL HTTP error: {e}"))?
        .json()
        .await
        .map_err(|e| format!("GraphQL response parse failed: {e}"))?;

    if let Some(errors) = response.get("errors") {
        return Err(format!("GraphQL returned errors: {errors}"));
    }

    let items = response
        .pointer("/data/assetsSearch/items")
        .and_then(|items| items.as_array())
        .ok_or_else(|| "GraphQL response did not include assetsSearch.items".to_string())?;

    Ok(items
        .iter()
        .filter_map(|item| {
            let files = item.get("files")?.as_array()?;
            let file_url = |kind: &str| {
                files
                    .iter()
                    .find(|file| file.get("asset_file_type_slug").and_then(|v| v.as_str()) == Some(kind))
                    .and_then(|file| file.get("url"))
                    .and_then(|url| url.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            let sample_count = item
                .get("child_asset_counts")
                .and_then(|counts| counts.as_array())
                .and_then(|counts| {
                    counts.iter().find(|count| {
                        count.get("type").and_then(|v| v.as_str()) == Some("sample")
                    })
                })
                .and_then(|count| count.get("count"))
                .and_then(|count| count.as_i64());

            Some(PackSummary {
                uuid: item.get("uuid")?.as_str()?.to_string(),
                name: item.get("name")?.as_str()?.to_string(),
                cover_url: file_url("cover_image"),
                banner_url: file_url("banner_image"),
                demo_url: file_url("preview_mp3"),
                genre: item.get("main_genre").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                provider_name: String::new(),
                permalink: item.get("permalink_base_url").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                sample_count,
            })
        })
        .collect())
}

#[tauri::command]
async fn reveal_in_finder(path: String) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("path is required".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open")
            .arg("-R")
            .arg(&path)
            .status()
            .map_err(|e| format!("Failed to run Finder reveal: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("Finder reveal failed with status {status}"))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err("Reveal is currently implemented for macOS only".to_string())
    }
}

#[tauri::command]
async fn read_local_audio_bytes(path: String) -> Result<Vec<u8>, String> {
    if path.trim().is_empty() {
        return Err("path is required".to_string());
    }
    tokio::fs::read(&path)
        .await
        .map_err(|e| format!("Failed to read local audio file: {e}"))
}

#[tauri::command]
async fn fetch_preview_bytes(url: String) -> Result<Vec<u8>, String> {
    if !(url.starts_with("https://spliceproduction.s3.") || url.starts_with("https://spliceblob.splice.com/")) {
        return Err("Refusing to fetch non-Splice preview URL".to_string());
    }

    let response = reqwest::get(&url)
        .await
        .map_err(|e| format!("Preview fetch failed: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Preview fetch failed: HTTP {status}"));
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|e| format!("Preview read failed: {e}"))
}

#[tauri::command]
async fn helper_auth_snapshot() -> (bool, bool, String, String, String, Vec<String>) {
    let mut errors = Vec::new();
    let Ok((_port, mut client, helper_errors)) = find_helper_client().await.map_err(|e| format!("{e:?}")) else {
        errors.push("Splice helper is not running".to_string());
        return (false, false, String::new(), String::new(), String::new(), errors);
    };
    errors.extend(helper_errors);
    let mut helper_has_token = false;
    let mut helper_auth_type = String::new();
    match client.get_session(GetSessionRequest {}).await {
        Ok(response) => {
            if let Some(auth) = response.into_inner().auth {
                helper_has_token = !auth.token.is_empty();
                helper_auth_type = format!("{}", auth.auth_type);
            }
        }
        Err(e) => errors.push(format!("Helper GetSession failed: {e:?}")),
    }
    let mut helper_username = String::new();
    let mut helper_email = String::new();
    match client.validate_login(ValidateLoginRequest {}).await {
        Ok(response) => {
            if let Some(user) = response.into_inner().user {
                helper_username = user.username;
                helper_email = user.email;
            }
        }
        Err(e) => errors.push(format!("Helper ValidateLogin failed: {e:?}")),
    }
    (true, helper_has_token, helper_auth_type, helper_username, helper_email, errors)
}

fn auth_status_with_helper(mut status: OwnedAuthStatus, helper: &(bool, bool, String, String, String, Vec<String>)) -> OwnedAuthStatus {
    status.helper_connected = helper.0;
    status.helper_has_token = helper.1;
    status.helper_synced = status.signed_in
        && helper.0
        && helper.1
        && (!status.username.is_empty() && helper.3 == status.username || !status.email.is_empty() && helper.4 == status.email);
    status
}

#[tauri::command]
async fn supuraisu_auth_status(keychain_consent: bool) -> Result<OwnedAuthStatus, String> {
    set_keychain_consent(keychain_consent)?;
    let status = match ensure_owned_auth_session().await? {
        Some(session) => auth_status_from_session(&session),
        None => signed_out_auth_status(),
    };
    let helper = helper_auth_snapshot().await;
    Ok(auth_status_with_helper(status, &helper))
}

#[tauri::command]
async fn supuraisu_auth_shakedown(keychain_consent: bool) -> Result<AuthShakedownStatus, String> {
    set_keychain_consent(keychain_consent)?;
    let mut errors = Vec::new();
    let owned = match ensure_owned_auth_session().await {
        Ok(session) => session,
        Err(e) => {
            errors.push(e);
            None
        }
    };
    let mut sync_attempted = false;
    if let Some(session) = owned.as_ref() {
        sync_attempted = true;
        if let Err(e) = send_owned_session_to_helper(session).await {
            errors.push(e);
        }
    }
    let helper = helper_auth_snapshot().await;
    errors.extend(helper.5.clone());
    let helper_synced = owned
        .as_ref()
        .map(|session| {
            helper.0
                && helper.1
                && (!session.profile.username.is_empty() && helper.3 == session.profile.username
                    || !session.profile.email.is_empty() && helper.4 == session.profile.email)
        })
        .unwrap_or(false);
    Ok(AuthShakedownStatus {
        keychain_consent: has_keychain_consent(),
        owned_signed_in: owned.is_some(),
        owned_username: owned.as_ref().map(|s| s.profile.username.clone()).unwrap_or_default(),
        owned_email: owned.as_ref().map(|s| s.profile.email.clone()).unwrap_or_default(),
        owned_expires_at: owned.as_ref().map(|s| s.expires_at),
        helper_connected: helper.0,
        helper_has_token: helper.1,
        helper_auth_type: helper.2,
        helper_username: helper.3,
        helper_email: helper.4,
        helper_synced,
        sync_attempted,
        errors,
    })
}

#[tauri::command]
async fn supuraisu_auth_logout() -> Result<(), String> {
    delete_refresh_token()?;
    *owned_auth_state().lock().map_err(|_| "Auth state lock poisoned".to_string())? = None;
    if let Ok((_port, mut client, _errors)) = find_helper_client().await {
        let _ = client.logout(splice_proto::LogoutRequest {}).await;
    }
    Ok(())
}

#[tauri::command]
async fn supuraisu_diagnostics() -> Result<DiagnosticsInfo, String> {
    let environment = splice_environment_status();
    let owned = owned_auth_state().lock().map_err(|_| "Auth state lock poisoned".to_string())?.clone();
    let helper = helper_auth_snapshot().await;
    let mut errors = helper.5.clone();
    let helper_synced = owned
        .as_ref()
        .map(|session| {
            helper.0
                && helper.1
                && (!session.profile.username.is_empty() && helper.3 == session.profile.username
                    || !session.profile.email.is_empty() && helper.4 == session.profile.email)
        })
        .unwrap_or(false);
    if !environment.helper_installed {
        errors.push("Splice Helper is not installed at the expected path".to_string());
    }
    if !environment.cert_exists || !environment.key_exists {
        errors.push("Splice local TLS certificate/key is missing".to_string());
    }
    Ok(DiagnosticsInfo {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        keychain_consent: has_keychain_consent(),
        owned_signed_in: owned.is_some(),
        owned_username: owned.as_ref().map(|s| s.profile.username.clone()).unwrap_or_default(),
        owned_email: owned.as_ref().map(|s| s.profile.email.clone()).unwrap_or_default(),
        helper_connected: helper.0,
        helper_has_token: helper.1,
        helper_auth_type: helper.2,
        helper_username: helper.3,
        helper_email: helper.4,
        helper_synced,
        environment,
        errors,
    })
}

fn collect_asar_wasm_entries(prefix: &str, node: &serde_json::Value, out: &mut Vec<(String, u64, usize)>) {
    let Some(files) = node.get("files").and_then(|value| value.as_object()) else {
        return;
    };
    for (name, child) in files {
        let path = if prefix.is_empty() { format!("/{name}") } else { format!("{prefix}/{name}") };
        if path.ends_with(".wasm") {
            if let (Some(offset), Some(size)) = (
                child.get("offset").and_then(|value| value.as_str()).and_then(|value| value.parse::<u64>().ok()),
                child.get("size").and_then(|value| value.as_u64()).and_then(|value| usize::try_from(value).ok()),
            ) {
                out.push((path.clone(), offset, size));
            }
        }
        collect_asar_wasm_entries(&path, child, out);
    }
}

fn read_asar_wasm_candidates(asar_path: &Path) -> Result<Vec<WasmCandidate>, String> {
    let bytes = fs::read(asar_path).map_err(|e| format!("Could not read {}: {e}", asar_path.display()))?;
    if bytes.len() < 16 {
        return Err(format!("{} is too small to be an ASAR archive", asar_path.display()));
    }
    let header_size = u32::from_le_bytes(bytes[4..8].try_into().map_err(|_| "Invalid ASAR header".to_string())?) as usize;
    let json_size = u32::from_le_bytes(bytes[12..16].try_into().map_err(|_| "Invalid ASAR header".to_string())?) as usize;
    let json_end = 16usize.checked_add(json_size).ok_or_else(|| "Invalid ASAR header size".to_string())?;
    if json_end > bytes.len() {
        return Err(format!("{} has an invalid ASAR header", asar_path.display()));
    }
    let header: serde_json::Value = serde_json::from_slice(&bytes[16..json_end]).map_err(|e| format!("Could not parse ASAR header: {e}"))?;
    let data_start = 8usize.checked_add(header_size).ok_or_else(|| "Invalid ASAR data offset".to_string())?;
    let mut entries = Vec::new();
    collect_asar_wasm_entries("", &header, &mut entries);
    entries.sort_by_key(|(path, _, _)| {
        if path.starts_with("/desktop-main/") || path.starts_with("/desktop-companion/") {
            0
        } else {
            1
        }
    });

    let mut candidates = Vec::new();
    for (path, offset, size) in entries.into_iter().take(12) {
        let start = data_start.checked_add(usize::try_from(offset).map_err(|_| "Invalid ASAR file offset".to_string())?).ok_or_else(|| "Invalid ASAR file range".to_string())?;
        let end = start.checked_add(size).ok_or_else(|| "Invalid ASAR file range".to_string())?;
        if end <= bytes.len() {
            candidates.push(WasmCandidate { path, bytes: bytes[start..end].to_vec() });
        }
    }
    Ok(candidates)
}

#[tauri::command]
async fn splice_decoder_wasm_candidates() -> Result<Vec<WasmCandidate>, String> {
    let asar_path = PathBuf::from("/Applications/Splice.app/Contents/Resources/app.asar");
    let candidates = read_asar_wasm_candidates(&asar_path)?;
    if candidates.is_empty() {
        return Err("No WASM files found in the installed Splice app".to_string());
    }
    Ok(candidates)
}

#[tauri::command]
async fn supuraisu_auth_login(app: tauri::AppHandle, keychain_consent: bool) -> Result<(), String> {
    if !keychain_consent {
        return Err("Please approve Supuraisu using macOS Keychain before signing in.".to_string());
    }
    set_keychain_consent(true)?;
    if let Some(window) = app.get_webview_window("supuraisu-auth") {
        let _ = window.set_focus();
        return Ok(());
    }

    let verifier = random_string(96);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_string(32);
    let mut url = url::Url::parse(&format!("https://{AUTH_DOMAIN}/authorize")).map_err(|e| e.to_string())?;
    url.query_pairs_mut()
        .append_pair("scope", "openid profile offline_access")
        .append_pair("response_type", "code")
        .append_pair("client_id", AUTH_CLIENT_ID)
        .append_pair("redirect_uri", AUTH_REDIRECT_URI)
        .append_pair("audience", AUTH_AUDIENCE)
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");

    let app_for_nav = app.clone();
    tauri::WebviewWindowBuilder::new(&app, "supuraisu-auth", tauri::WebviewUrl::External(url))
        .title("Sign in to Splice")
        .inner_size(960.0, 760.0)
        .on_navigation(move |nav_url| {
            if nav_url.as_str().starts_with(AUTH_REDIRECT_URI) {
                let code = nav_url.query_pairs().find(|(key, _)| key == "code").map(|(_, value)| value.to_string());
                let returned_state = nav_url.query_pairs().find(|(key, _)| key == "state").map(|(_, value)| value.to_string());
                let error = nav_url.query_pairs().find(|(key, _)| key == "error_description" || key == "error").map(|(_, value)| value.to_string());
                let app = app_for_nav.clone();
                let verifier = verifier.clone();
                let expected_state = state.clone();
                tauri::async_runtime::spawn(async move {
                    let result = async {
                        if let Some(error) = error {
                            return Err(format!("Splice login failed: {error}"));
                        }
                        if returned_state.as_deref() != Some(expected_state.as_str()) {
                            return Err("Splice login state mismatch".to_string());
                        }
                        let code = code.ok_or_else(|| "Splice login callback did not include a code".to_string())?;
                        let session = exchange_auth_code(&code, &verifier).await?;
                        *owned_auth_state().lock().map_err(|_| "Auth state lock poisoned".to_string())? = Some(session.clone());
                        let _ = send_owned_session_to_helper(&session).await;
                        Ok(auth_status_from_session(&session))
                    }
                    .await;
                    let _ = app.emit("supuraisu-auth-complete", &result);
                    if let Some(window) = app.get_webview_window("supuraisu-auth") {
                        let _ = window.close();
                    }
                });
                return false;
            }
            true
        })
        .build()
        .map_err(|e| format!("Could not open login window: {e}"))?;

    Ok(())
}

#[tauri::command]
async fn start_file_drag(window: tauri::Window, path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let ns_view = window.ns_view().map_err(|e| format!("Could not get NSView: {e}"))? as usize;
        let (tx, rx) = std::sync::mpsc::channel();
        window
            .run_on_main_thread(move || {
                let result = unsafe { macos_file_drag::begin_file_drag((ns_view as *mut std::ffi::c_void).cast(), &path) };
                let _ = tx.send(result);
            })
            .map_err(|e| format!("Could not dispatch drag to main thread: {e}"))?;
        rx.recv()
            .map_err(|e| format!("Drag result channel failed: {e}"))?
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        let _ = path;
        Err("Native file drag is currently implemented for macOS only".to_string())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            splice_environment_status,
            supuraisu_auth_status,
            supuraisu_auth_shakedown,
            supuraisu_auth_login,
            supuraisu_auth_logout,
            supuraisu_diagnostics,
            probe_splice_helper,
            search_splice_samples,
            download_splice_sample,
            list_collections,
            collection_samples,
            list_helper_packs,
            liked_splice_packs,
            explore_splice_packs,
            reveal_in_finder,
            read_local_audio_bytes,
            fetch_preview_bytes,
            splice_decoder_wasm_candidates,
            start_file_drag
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
