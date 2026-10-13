use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose};
use beam_quic_protocol::{
    DEFAULT_ALPN, Provider as ProtocolProvider, QuicMethod, RoutingMode as ProtocolRoutingMode,
    SubmitRequestHeader, SubmitResponseError, VERSION, decode_response, encode_request,
};
use quinn::{ClientConfig, Connection, Endpoint, crypto::rustls::QuicClientConfig};
use rcgen::{CertificateParams, DistinguishedName, DnType, DnValue, KeyPair};
use rustls::{
    RootCertStore,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
};
use tokio::{sync::Mutex, time::sleep};

const MAX_RESPONSE_BYTES: usize = 16 * 1024;
const DEFAULT_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(10);
const HANDSHAKE_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_CONFIRMATION_POLL_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Debug, Clone)]
pub struct BeamQuicClientConfig {
    pub endpoint: SocketAddr,
    pub api_key: String,
    pub server_name: String,
    pub ca_cert_path: Option<PathBuf>,
    pub danger_accept_invalid_server_cert: bool,
    pub alpn: String,
}

impl Default for BeamQuicClientConfig {
    fn default() -> Self {
        Self {
            endpoint: SocketAddr::from((Ipv4Addr::LOCALHOST, 9898)),
            api_key: String::new(),
            server_name: "localhost".to_string(),
            ca_cert_path: None,
            danger_accept_invalid_server_cert: false,
            alpn: DEFAULT_ALPN.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Bloxroute,
    Astralane,
    Nozomi,
    Falcon,
    Manka,
    Raiden,
    Apex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoutingMode {
    #[default]
    Fastest,
    MevProtect,
}

#[derive(Debug, Clone, Default)]
pub struct SendTransactionOptions {
    pub provider: Option<Provider>,
    pub mode: RoutingMode,
    pub request_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SendTransactionFastOptions {
    pub provider: Provider,
    pub mode: RoutingMode,
    pub request_id: Option<String>,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendTransactionResult {
    pub signature: String,
    pub provider: String,
}

#[derive(Debug, thiserror::Error)]
pub enum BeamQuicClientError {
    #[error("api key must not be empty")]
    EmptyApiKey,
    #[error("server name must not be empty")]
    EmptyServerName,
    #[error("server address must not be empty")]
    EmptyServerAddr,
    #[error("server address did not resolve to any socket address: {0}")]
    EmptyResolvedAddrs(String),
    #[error("failed to resolve Beam QUIC server address {server_addr}: {source}")]
    Resolve {
        server_addr: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to build TLS config: {0}")]
    Tls(String),
    #[error("failed to create QUIC endpoint: {0}")]
    Endpoint(#[source] std::io::Error),
    #[error("failed to connect to Beam QUIC endpoint: {0}")]
    Connect(#[from] quinn::ConnectionError),
    #[error("failed to open QUIC stream: {0}")]
    OpenStream(#[source] quinn::ConnectionError),
    #[error("failed to write request frame: {0}")]
    Write(#[from] quinn::WriteError),
    #[error("failed to finish request stream: {0}")]
    Finish(#[source] quinn::ClosedStream),
    #[error("failed to read response frame: {0}")]
    Read(#[source] quinn::ReadToEndError),
    #[error("invalid Beam QUIC frame: {0}")]
    Frame(#[from] beam_quic_protocol::FrameError),
    #[error("Beam returned error {code}: {message}")]
    Beam { code: i64, message: String },
    #[error("Beam returned malformed response: {0}")]
    BadResponse(String),
    #[error("invalid base64 transaction: {0}")]
    InvalidBase64(#[source] base64::DecodeError),
    #[cfg(any(feature = "solana-sdk", feature = "solana-transaction"))]
    #[error("failed to serialize solana transaction: {0}")]
    SerializeTransaction(#[source] bincode::Error),
}

pub struct BeamQuicClient {
    cfg: BeamQuicClientConfig,
    endpoint: Endpoint,
    connection: Mutex<Option<Connection>>,
}

impl BeamQuicClient {
    pub async fn connect(
        server_addr: impl AsRef<str>,
        api_key: impl Into<String>,
    ) -> Result<Self, BeamQuicClientError> {
        let server_addr = server_addr.as_ref().trim();
        if server_addr.is_empty() {
            return Err(BeamQuicClientError::EmptyServerAddr);
        }
        let endpoint = resolve_endpoint(server_addr).await?;
        Self::connect_with_config(BeamQuicClientConfig {
            endpoint,
            api_key: api_key.into(),
            server_name: inferred_server_name(server_addr),
            ca_cert_path: None,
            danger_accept_invalid_server_cert: true,
            alpn: DEFAULT_ALPN.to_string(),
        })
        .await
    }

    pub async fn connect_with_config(
        cfg: BeamQuicClientConfig,
    ) -> Result<Self, BeamQuicClientError> {
        if cfg.api_key.trim().is_empty() {
            return Err(BeamQuicClientError::EmptyApiKey);
        }
        if cfg.server_name.trim().is_empty() {
            return Err(BeamQuicClientError::EmptyServerName);
        }

        let mut endpoint = Endpoint::client(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0))
            .map_err(BeamQuicClientError::Endpoint)?;
        endpoint.set_default_client_config(build_client_config(&cfg)?);

        let client = Self {
            cfg,
            endpoint,
            connection: Mutex::new(None),
        };
        client.ensure_connected().await?;
        Ok(client)
    }

    pub async fn send_transaction_bytes(
        &self,
        tx_bytes: impl AsRef<[u8]>,
        options: SendTransactionOptions,
    ) -> Result<SendTransactionResult, BeamQuicClientError> {
        let frame = encode_request(
            &SubmitRequestHeader {
                version: VERSION,
                request_id: options.request_id.clone(),
                method: QuicMethod::SendTransaction,
                provider: options.provider.map(Into::into),
                mode: options.mode.into(),
                signature: None,
            },
            tx_bytes.as_ref(),
        )?;
        self.send_frame_with_retry(frame).await
    }

    pub async fn send_transaction_fast_bytes(
        &self,
        tx_bytes: impl AsRef<[u8]>,
        options: SendTransactionFastOptions,
    ) -> Result<(), BeamQuicClientError> {
        let frame = encode_request(
            &SubmitRequestHeader {
                version: VERSION,
                request_id: options.request_id.clone(),
                method: QuicMethod::SendTransactionFast,
                provider: Some(options.provider.into()),
                mode: options.mode.into(),
                signature: options.signature,
            },
            tx_bytes.as_ref(),
        )?;
        self.send_frame_uni_with_retry(frame).await
    }

    pub async fn send_transaction_fast_base64(
        &self,
        encoded_transaction: &str,
        options: SendTransactionFastOptions,
    ) -> Result<(), BeamQuicClientError> {
        let tx_bytes = general_purpose::STANDARD
            .decode(encoded_transaction)
            .map_err(BeamQuicClientError::InvalidBase64)?;
        self.send_transaction_fast_bytes(tx_bytes, options).await
    }

    pub async fn send_transaction_base64(
        &self,
        encoded_transaction: &str,
        options: SendTransactionOptions,
    ) -> Result<SendTransactionResult, BeamQuicClientError> {
        let tx_bytes = general_purpose::STANDARD
            .decode(encoded_transaction)
            .map_err(BeamQuicClientError::InvalidBase64)?;
        self.send_transaction_bytes(tx_bytes, options).await
    }

    #[cfg(any(feature = "solana-sdk", feature = "solana-transaction"))]
    pub async fn send_versioned_transaction(
        &self,
        tx: &solana_transaction::versioned::VersionedTransaction,
        options: SendTransactionOptions,
    ) -> Result<SendTransactionResult, BeamQuicClientError> {
        let tx_bytes = bincode::serialize(tx).map_err(BeamQuicClientError::SerializeTransaction)?;
        self.send_transaction_bytes(tx_bytes, options).await
    }

    #[cfg(any(feature = "solana-sdk", feature = "solana-transaction"))]
    pub async fn send_versioned_transaction_fast(
        &self,
        tx: &solana_transaction::versioned::VersionedTransaction,
        mut options: SendTransactionFastOptions,
    ) -> Result<(), BeamQuicClientError> {
        if options.signature.is_none()
            && let Some(signature) = tx.signatures.first()
        {
            options.signature = Some(signature.to_string());
        }
        let tx_bytes = bincode::serialize(tx).map_err(BeamQuicClientError::SerializeTransaction)?;
        self.send_transaction_fast_bytes(tx_bytes, options).await
    }

    async fn send_frame_with_retry(
        &self,
        frame: Vec<u8>,
    ) -> Result<SendTransactionResult, BeamQuicClientError> {
        match self.send_frame(frame.clone()).await {
            Ok(result) => Ok(result),
            Err(err) if is_reconnectable(&err) => {
                self.drop_connection().await;
                self.ensure_connected().await?;
                self.send_frame(frame).await
            }
            Err(err) => Err(err),
        }
    }

    async fn send_frame(
        &self,
        frame: Vec<u8>,
    ) -> Result<SendTransactionResult, BeamQuicClientError> {
        let connection = self.ensure_connected().await?;
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(BeamQuicClientError::OpenStream)?;
        send.write_all(&frame).await?;
        send.finish().map_err(BeamQuicClientError::Finish)?;

        let response_bytes = recv
            .read_to_end(MAX_RESPONSE_BYTES)
            .await
            .map_err(BeamQuicClientError::Read)?;
        let response = decode_response(&response_bytes)?;
        if !response.ok {
            let SubmitResponseError { code, message } =
                response.error.unwrap_or_else(|| SubmitResponseError {
                    code: -32003,
                    message: "missing error body".to_string(),
                });
            return Err(BeamQuicClientError::Beam { code, message });
        }
        let result = response.result.ok_or_else(|| {
            BeamQuicClientError::BadResponse("missing success result".to_string())
        })?;
        Ok(SendTransactionResult {
            signature: result.signature,
            provider: result.provider,
        })
    }

    async fn send_frame_uni_with_retry(&self, frame: Vec<u8>) -> Result<(), BeamQuicClientError> {
        match self.send_frame_uni(frame.clone()).await {
            Ok(()) => Ok(()),
            Err(err) if is_reconnectable(&err) => {
                self.drop_connection().await;
                self.ensure_connected().await?;
                self.send_frame_uni(frame).await
            }
            Err(err) => Err(err),
        }
    }

    async fn send_frame_uni(&self, frame: Vec<u8>) -> Result<(), BeamQuicClientError> {
        let connection = self.ensure_connected().await?;
        let mut send = connection
            .open_uni()
            .await
            .map_err(BeamQuicClientError::OpenStream)?;
        send.write_all(&frame).await?;
        send.finish().map_err(BeamQuicClientError::Finish)?;
        Ok(())
    }

    async fn ensure_connected(&self) -> Result<Connection, BeamQuicClientError> {
        let mut guard = self.connection.lock().await;
        if let Some(connection) = guard.as_ref()
            && connection.close_reason().is_none()
        {
            return Ok(connection.clone());
        }

        let connection = self
            .endpoint
            .connect(self.cfg.endpoint, self.cfg.server_name.as_str())
            .map_err(|e| BeamQuicClientError::Tls(e.to_string()))?
            .await?;
        wait_for_handshake_confirmation(&connection).await?;
        *guard = Some(connection.clone());
        Ok(connection)
    }

    async fn drop_connection(&self) {
        let mut guard = self.connection.lock().await;
        *guard = None;
    }
}

async fn wait_for_handshake_confirmation(
    connection: &Connection,
) -> Result<(), BeamQuicClientError> {
    let confirmation = async {
        loop {
            if connection.stats().frame_rx.handshake_done > 0 {
                return Ok(());
            }
            if let Some(error) = connection.close_reason() {
                return Err(BeamQuicClientError::Connect(error));
            }
            sleep(HANDSHAKE_CONFIRMATION_POLL_INTERVAL).await;
        }
    };

    match tokio::time::timeout(HANDSHAKE_CONFIRMATION_TIMEOUT, confirmation).await {
        Ok(result) => result,
        Err(_) => {
            connection.close(0_u32.into(), b"handshake confirmation timeout");
            Err(BeamQuicClientError::Tls(format!(
                "Beam QUIC server did not confirm the handshake within {HANDSHAKE_CONFIRMATION_TIMEOUT:?}"
            )))
        }
    }
}

async fn resolve_endpoint(server_addr: &str) -> Result<SocketAddr, BeamQuicClientError> {
    if let Ok(addr) = server_addr.parse::<SocketAddr>() {
        return Ok(addr);
    }
    let mut addrs = tokio::net::lookup_host(server_addr)
        .await
        .map_err(|source| BeamQuicClientError::Resolve {
            server_addr: server_addr.to_string(),
            source,
        })?;
    addrs
        .next()
        .ok_or_else(|| BeamQuicClientError::EmptyResolvedAddrs(server_addr.to_string()))
}

fn inferred_server_name(server_addr: &str) -> String {
    if server_addr.parse::<SocketAddr>().is_ok() {
        return "beam".to_string();
    }
    let host = server_addr
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(server_addr)
        .trim_matches(['[', ']']);
    if host.is_empty() {
        "beam".to_string()
    } else {
        host.to_string()
    }
}

impl From<Provider> for ProtocolProvider {
    fn from(value: Provider) -> Self {
        match value {
            Provider::Bloxroute => Self::Bloxroute,
            Provider::Astralane => Self::Astralane,
            Provider::Nozomi => Self::Nozomi,
            Provider::Falcon => Self::Falcon,
            Provider::Manka => Self::Manka,
            Provider::Raiden => Self::Raiden,
            Provider::Apex => Self::Apex,
        }
    }
}

impl From<RoutingMode> for ProtocolRoutingMode {
    fn from(value: RoutingMode) -> Self {
        match value {
            RoutingMode::Fastest => Self::Fastest,
            RoutingMode::MevProtect => Self::MevProtect,
        }
    }
}

fn is_reconnectable(err: &BeamQuicClientError) -> bool {
    matches!(
        err,
        BeamQuicClientError::Connect(_)
            | BeamQuicClientError::OpenStream(_)
            | BeamQuicClientError::Write(_)
            | BeamQuicClientError::Finish(_)
            | BeamQuicClientError::Read(_)
    )
}

fn build_client_config(cfg: &BeamQuicClientConfig) -> Result<ClientConfig, BeamQuicClientError> {
    let (cert, key) = generate_client_cert_der(&cfg.api_key)?;
    let mut crypto = if cfg.danger_accept_invalid_server_cert {
        rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_client_auth_cert(vec![cert], key)
            .map_err(|e| BeamQuicClientError::Tls(e.to_string()))?
    } else {
        rustls::ClientConfig::builder()
            .with_root_certificates(load_roots(cfg.ca_cert_path.as_ref())?)
            .with_client_auth_cert(vec![cert], key)
            .map_err(|e| BeamQuicClientError::Tls(e.to_string()))?
    };
    crypto.alpn_protocols = vec![cfg.alpn.as_bytes().to_vec()];
    let mut client = ClientConfig::new(Arc::new(
        QuicClientConfig::try_from(crypto).map_err(|e| BeamQuicClientError::Tls(e.to_string()))?,
    ));
    let mut transport = quinn::TransportConfig::default();
    transport.keep_alive_interval(Some(DEFAULT_KEEP_ALIVE_INTERVAL));
    client.transport_config(Arc::new(transport));
    Ok(client)
}

fn load_roots(path: Option<&PathBuf>) -> Result<RootCertStore, BeamQuicClientError> {
    let mut roots = RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().certs {
        roots
            .add(cert)
            .map_err(|e| BeamQuicClientError::Tls(e.to_string()))?;
    }
    if let Some(path) = path {
        let bytes = fs::read(path).map_err(|e| BeamQuicClientError::Tls(e.to_string()))?;
        let certs = rustls_pemfile::certs(&mut bytes.as_slice())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| BeamQuicClientError::Tls(e.to_string()))?;
        for cert in certs {
            roots
                .add(cert)
                .map_err(|e| BeamQuicClientError::Tls(e.to_string()))?;
        }
    }
    Ok(roots)
}

pub fn generate_client_cert_der(
    api_key: &str,
) -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>), BeamQuicClientError> {
    if api_key.trim().is_empty() {
        return Err(BeamQuicClientError::EmptyApiKey);
    }
    let key_pair = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
        .map_err(|e| BeamQuicClientError::Tls(e.to_string()))?;
    let mut params = CertificateParams::new(Vec::<String>::new())
        .map_err(|e| BeamQuicClientError::Tls(e.to_string()))?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, DnValue::Utf8String(api_key.to_string()));
    params.distinguished_name = dn;
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| BeamQuicClientError::Tls(e.to_string()))?;
    Ok((
        cert.der().clone(),
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der())),
    ))
}

#[derive(Debug)]
struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self(
            Arc::new(rustls::crypto::aws_lc_rs::default_provider()),
        ))
    }
}

impl ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::{
        DigitallySignedStruct, DistinguishedName, Error as RustlsError, SignatureScheme,
        server::danger::{ClientCertVerified, ClientCertVerifier},
    };
    use x509_parser::prelude::*;

    #[derive(Debug)]
    struct TestClientCertVerifier {
        accept: bool,
        provider: Arc<rustls::crypto::CryptoProvider>,
    }

    impl TestClientCertVerifier {
        fn new(accept: bool) -> Self {
            Self {
                accept,
                provider: Arc::new(rustls::crypto::aws_lc_rs::default_provider()),
            }
        }
    }

    impl ClientCertVerifier for TestClientCertVerifier {
        fn offer_client_auth(&self) -> bool {
            true
        }

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
        ) -> Result<ClientCertVerified, RustlsError> {
            if self.accept {
                Ok(ClientCertVerified::assertion())
            } else {
                Err(RustlsError::General(
                    "client certificate rejected".to_string(),
                ))
            }
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, RustlsError> {
            rustls::crypto::verify_tls12_signature(
                message,
                cert,
                dss,
                &self.provider.signature_verification_algorithms,
            )
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, RustlsError> {
            rustls::crypto::verify_tls13_signature(
                message,
                cert,
                dss,
                &self.provider.signature_verification_algorithms,
            )
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.provider
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    fn test_server_config(accept_client_cert: bool) -> quinn::ServerConfig {
        let key_pair = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let params = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
        let cert = params.self_signed(&key_pair).unwrap();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));
        let verifier = Arc::new(TestClientCertVerifier::new(accept_client_cert));
        let mut crypto = rustls::ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(vec![cert.der().clone()], key)
            .unwrap();
        crypto.alpn_protocols = vec![DEFAULT_ALPN.as_bytes().to_vec()];
        quinn::ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(crypto).unwrap(),
        ))
    }

    async fn connect_to_test_server(
        accept_client_cert: bool,
    ) -> Result<BeamQuicClient, BeamQuicClientError> {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let server = Endpoint::server(
            test_server_config(accept_client_cert),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        )
        .unwrap();
        let server_addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let incoming = server.accept().await.expect("incoming connection");
            match incoming.await {
                Ok(connection) => {
                    sleep(Duration::from_millis(50)).await;
                    connection.close(0_u32.into(), b"test complete");
                }
                Err(error) => assert!(!accept_client_cert, "unexpected handshake error: {error}"),
            }
        });

        BeamQuicClient::connect_with_config(BeamQuicClientConfig {
            endpoint: server_addr,
            api_key: "beam-key".to_string(),
            server_name: "localhost".to_string(),
            ca_cert_path: None,
            danger_accept_invalid_server_cert: true,
            alpn: DEFAULT_ALPN.to_string(),
        })
        .await
    }

    #[test]
    fn generated_cert_contains_api_key_cn() {
        let (cert, _) = generate_client_cert_der("beam-key").unwrap();
        let (_, parsed) = parse_x509_certificate(cert.as_ref()).unwrap();
        let cn = parsed
            .subject()
            .iter_common_name()
            .next()
            .and_then(|attr| attr.as_str().ok());
        assert_eq!(cn, Some("beam-key"));
    }

    #[test]
    fn insecure_constructor_hides_sni_for_socket_addr() {
        assert_eq!(inferred_server_name("127.0.0.1:9898"), "beam");
        assert_eq!(inferred_server_name("[::1]:9898"), "beam");
        assert_eq!(
            inferred_server_name("beam.example.com:9898"),
            "beam.example.com"
        );
    }

    #[tokio::test]
    async fn waits_for_server_handshake_confirmation() {
        let client = connect_to_test_server(true)
            .await
            .expect("authorized handshake must be confirmed");
        let connection = client
            .connection
            .lock()
            .await
            .clone()
            .expect("confirmed connection must be cached");
        assert!(connection.stats().frame_rx.handshake_done > 0);
    }

    #[tokio::test]
    async fn rejected_client_certificate_never_yields_a_usable_client() {
        let error = match connect_to_test_server(false).await {
            Ok(_) => panic!("rejected handshake must not create a client"),
            Err(error) => error,
        };
        assert!(matches!(error, BeamQuicClientError::Connect(_)));
    }

    #[tokio::test]
    async fn idle_connection_stays_alive_and_is_reused_for_submission() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let mut server_config = test_server_config(true);
        Arc::get_mut(&mut server_config.transport)
            .unwrap()
            .max_idle_timeout(Some(Duration::from_secs(12).try_into().unwrap()));
        let server =
            Endpoint::server(server_config, SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let config = BeamQuicClientConfig {
            endpoint: server.local_addr().unwrap(),
            api_key: "beam-key".into(),
            server_name: "localhost".into(),
            ca_cert_path: None,
            danger_accept_invalid_server_cert: true,
            alpn: DEFAULT_ALPN.into(),
        };
        let (client, peer) = tokio::join!(BeamQuicClient::connect_with_config(config), async {
            server.accept().await.unwrap().await.unwrap()
        });
        let client = client.unwrap();
        let initial_connection = client.connection.lock().await.clone().unwrap();
        let initial_id = initial_connection.stable_id();

        // Exceed the negotiated idle timeout without any application requests.
        sleep(Duration::from_secs(15)).await;
        assert!(peer.close_reason().is_none(), "idle connection was closed");
        assert!(peer.stats().frame_rx.ping > 0, "no keepalive PING received");
        assert!(initial_connection.close_reason().is_none());

        client
            .send_transaction_fast_bytes(
                [1, 2, 3],
                SendTransactionFastOptions {
                    provider: Provider::Bloxroute,
                    mode: RoutingMode::Fastest,
                    request_id: None,
                    signature: None,
                },
            )
            .await
            .unwrap();
        let mut stream = tokio::time::timeout(Duration::from_secs(2), peer.accept_uni())
            .await
            .unwrap()
            .unwrap();
        let bytes = stream.read_to_end(4096).await.unwrap();
        let (header, transaction) = beam_quic_protocol::decode_request(
            &bytes,
            beam_quic_protocol::DEFAULT_MAX_FRAME_HEADER_BYTES,
            beam_quic_protocol::DEFAULT_MAX_TRANSACTION_BYTES,
        )
        .unwrap();
        assert_eq!(header.method, QuicMethod::SendTransactionFast);
        assert_eq!(transaction, [1, 2, 3]);
        assert_eq!(
            client.connection.lock().await.as_ref().unwrap().stable_id(),
            initial_id,
            "submission reconnected instead of reusing the idle connection"
        );
    }
}
