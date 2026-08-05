# beam-quic-client

Rust client for Beam QUIC `sendTransaction` and `sendTransactionFast`.

## Quick Start

```rust
use beam_quic_client::{BeamQuicClient, Provider, RoutingMode, SendTransactionOptions};

# async fn example(tx_bytes: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
let client = BeamQuicClient::connect(
    "beam.example.com:9898",
    "YOUR_BEAM_API_KEY",
).await?;

let result = client.send_transaction_bytes(
    tx_bytes,
    SendTransactionOptions {
        provider: Some(Provider::Bloxroute),
        mode: RoutingMode::Fastest,
        request_id: None,
    },
).await?;

println!("{} via {}", result.signature, result.provider);
# Ok(())
# }
```

## Fast Fire-and-Forget

`sendTransactionFast` uses a unidirectional QUIC stream and Beam does not send a response. It requires an explicit provider and skips Beam-side tip validation/signature extraction. Pass `signature` only when you want Beam to collect landing metrics for the submitted transaction.

```rust
use beam_quic_client::{
    BeamQuicClient, Provider, RoutingMode, SendTransactionFastOptions,
};

# async fn example(tx_bytes: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
let client = BeamQuicClient::connect(
    "beam.example.com:9898",
    "YOUR_BEAM_API_KEY",
).await?;

client.send_transaction_fast_bytes(
    tx_bytes,
    SendTransactionFastOptions {
        provider: Provider::Bloxroute,
        mode: RoutingMode::Fastest,
        request_id: None,
        signature: Some("OPTIONAL_TX_SIGNATURE".to_string()),
    },
).await?;
# Ok(())
# }
```

If `signature` is omitted, Beam still submits the raw bytes but does not track that transaction in the DB.

## Authentication

The client generates an ephemeral self-signed ECDSA client certificate for each client instance. The API key is placed in the certificate Common Name (`CN`), so it is not sent in the request frame body.

Connection setup waits for the server's QUIC `HANDSHAKE_DONE` confirmation
before any request stream can be opened. This ensures that an API key or client
IP rejected during mutual TLS authorization is reported by the client instead
of allowing a fire-and-forget send to return success first.

## Verified TLS

Use `connect_with_config` when you want normal server certificate verification or a custom CA bundle.

```rust
use beam_quic_client::{
    BeamQuicClient, BeamQuicClientConfig, Provider, RoutingMode, SendTransactionOptions,
};

# async fn example(tx_bytes: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

let client = BeamQuicClient::connect_with_config(BeamQuicClientConfig {
    endpoint: "203.0.113.10:9898".parse()?,
    api_key: "YOUR_BEAM_API_KEY".to_string(),
    server_name: "beam.example.com".to_string(),
    ca_cert_path: None,
    danger_accept_invalid_server_cert: false,
    alpn: "beam-submit-v1".to_string(),
}).await?;

let result = client.send_transaction_bytes(
    tx_bytes,
    SendTransactionOptions {
        provider: Some(Provider::Bloxroute),
        mode: RoutingMode::Fastest,
        request_id: None,
    },
).await?;
# Ok(())
# }
```

`ca_cert_path: None` uses native/system roots. Set `ca_cert_path` to a PEM CA bundle when the endpoint uses a private trust root.

See `examples/send_transaction.rs`, `examples/send_transaction_fast.rs`, and `examples/send_transaction_tls.rs` for complete examples.
