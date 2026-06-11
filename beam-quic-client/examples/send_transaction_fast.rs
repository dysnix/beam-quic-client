use beam_quic_client::{BeamQuicClient, Provider, RoutingMode, SendTransactionFastOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint =
        std::env::var("BEAM_QUIC_ENDPOINT").unwrap_or_else(|_| "127.0.0.1:9898".to_string());
    let api_key = std::env::var("BEAM_API_KEY")?;
    let tx_base64 = std::env::var("BEAM_TX_BASE64")?;
    let signature = std::env::var("BEAM_TX_SIGNATURE").ok();
    let request_id = std::env::var("BEAM_REQUEST_ID").ok();

    let client = BeamQuicClient::connect(&endpoint, api_key).await?;

    client
        .send_transaction_fast_base64(
            &tx_base64,
            SendTransactionFastOptions {
                provider: Provider::Bloxroute,
                mode: RoutingMode::Fastest,
                request_id,
                signature,
            },
        )
        .await?;

    println!("fast submission accepted locally");
    Ok(())
}
