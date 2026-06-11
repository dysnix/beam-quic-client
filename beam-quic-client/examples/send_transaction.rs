use beam_quic_client::{BeamQuicClient, Provider, RoutingMode, SendTransactionOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint =
        std::env::var("BEAM_QUIC_ENDPOINT").unwrap_or_else(|_| "127.0.0.1:9898".to_string());
    let api_key = std::env::var("BEAM_API_KEY")?;
    let tx_base64 = std::env::var("BEAM_TX_BASE64")?;

    let client = BeamQuicClient::connect(&endpoint, api_key).await?;

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
