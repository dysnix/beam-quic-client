use beam_quic_client::{
    BeamQuicClient, BeamQuicClientConfig, Provider, RoutingMode, SendTransactionOptions,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Install this before building any rustls TLS config. It is OK
    // if another dependency already installed the process-wide provider.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let endpoint =
        std::env::var("BEAM_QUIC_ENDPOINT").unwrap_or_else(|_| "beam.example.com:9898".to_string());
    let server_name =
        std::env::var("BEAM_QUIC_SERVER_NAME").unwrap_or_else(|_| endpoint_host(&endpoint));
    let api_key = std::env::var("BEAM_API_KEY")?;
    let tx_base64 = std::env::var("BEAM_TX_BASE64")?;

    let client = BeamQuicClient::connect_with_config(BeamQuicClientConfig {
        endpoint: resolve_endpoint(&endpoint).await?,
        api_key,
        server_name,
        // None means use native/system roots. Set ca_cert_path when the
        // endpoint uses a private trust root.
        ca_cert_path: None,
        danger_accept_invalid_server_cert: false,
        alpn: "beam-submit-v1".to_string(),
    })
    .await?;

    let result = client
        .send_transaction_base64(
            &tx_base64,
            SendTransactionOptions {
                provider: Some(Provider::Bloxroute),
                mode: RoutingMode::Fastest,
                request_id: None,
            },
        )
        .await?;

    println!(
        "signature={} provider={}",
        result.signature, result.provider
    );
    Ok(())
}

async fn resolve_endpoint(endpoint: &str) -> Result<std::net::SocketAddr, std::io::Error> {
    if let Ok(addr) = endpoint.parse() {
        return Ok(addr);
    }
    tokio::net::lookup_host(endpoint)
        .await?
        .next()
        .ok_or_else(|| std::io::Error::other(format!("no addresses resolved for {endpoint}")))
}

fn endpoint_host(endpoint: &str) -> String {
    if endpoint.parse::<std::net::SocketAddr>().is_ok() {
        return "localhost".to_string();
    }
    endpoint
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(endpoint)
        .trim_matches(['[', ']'])
        .to_string()
}
