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
        provider: Some(Provider::Manka),
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
        provider: Provider::Manka,
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

## Connection Keepalive

QUIC keepalive is enabled by default with a 10-second interval. When a connection
is idle, the transport sends QUIC PING frames to keep it usable between submissions.
These are transport control packets, not Solana transactions or Beam submission
requests. The setting applies to both submission methods and to connections created
after a reconnect.

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
        provider: Some(Provider::Manka),
        mode: RoutingMode::Fastest,
        request_id: None,
    },
).await?;
# Ok(())
# }
```

`ca_cert_path: None` uses native/system roots. Set `ca_cert_path` to a PEM CA bundle when the endpoint uses a private trust root.

See `examples/send_transaction.rs`, `examples/send_transaction_fast.rs`, and `examples/send_transaction_tls.rs` for complete examples.

## Providers and 0.2.0 migration

Supported providers: `Bloxroute`, `Astralane`, `Nozomi`, and `Manka`.
Use `Provider::Manka` and `RoutingMode::Fastest` for either submission method.
Manka accepts up to 1232 wire bytes per transaction. Connect to Beam with your
Beam API key; no Manka credentials are needed in the client.

Version 0.2.0 removes `Falcon`, `Raiden`, and `Apex` from the public provider
enums. Update matching application code and tip destinations before upgrading.
The wire format remains version 1 (`beam-submit-v1`) for retained providers.

Manka minimum tip: **0.001 SOL (1,000,000 lamports)**.

Manka tip destinations:

```text
rfMKugRuMGjfeodunVPy4bzQBjnPsGozwjaV3QUeG2x
rfMWRbvWQJ9933LY4GaQwdmoV8qqdkbUGFXkDDxtNvE
rfMafrD8chZGcWFo3y7oxNjbgzLbGYio595jb8eJw1B
rfMqGwHkxUxyA9AKLYBfA7ZG3Hwbt64jdDHr2PUMMF5
rfMyAcoGpSuBgnCuy6akRUkcBbBNQ1SomSG2g6JRTC4
```

Include a System Program SOL transfer to one of these addresses. Regular
submission validates the tip/provider match; fast submission skips synchronous
tip validation and requires the explicit provider.
