# beam-quic-client

Rust client and shared protocol types for Beam QUIC `sendTransaction` and
`sendTransactionFast`.

## Install From crates.io

```toml
[dependencies]
beam-quic-client = "0.2.0"
```

Enable Solana transaction helpers when you want to pass a
`VersionedTransaction` directly:

```toml
[dependencies]
beam-quic-client = { version = "0.2.0", features = ["solana-transaction"] }
```

See [beam-quic-client/README.md](beam-quic-client/README.md) for
usage examples.

The client enables QUIC keepalive with a 10-second interval so idle connections
remain usable between submissions. Keepalive sends QUIC PING frames, not Solana
transactions.

## Providers and 0.2.0 migration

Supported providers: `Bloxroute`, `Astralane`, `Nozomi`, and `Manka`.
Use `Provider::Manka` and `RoutingMode::Fastest` for either submission method.
Manka accepts up to 1232 wire bytes per transaction. Connect to Beam with your
Beam API key; no Manka credentials are needed in the client.

Version 0.2.0 removes `Falcon`, `Raiden`, and `Apex` from the public provider
enums. Update matching application code and tip destinations before upgrading.
The wire format remains version 1 (`beam-submit-v1`) for retained providers.

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
