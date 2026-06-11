# beam-quic-client

Rust client and shared protocol types for Beam QUIC `sendTransaction` and
`sendTransactionFast`.

## Install From Git

```toml
[dependencies]
beam-quic-client = { git = "https://github.com/dysnix/beam-quic-client" }
```

Enable Solana transaction helpers when you want to pass a
`VersionedTransaction` directly:

```toml
[dependencies]
beam-quic-client = { git = "https://github.com/dysnix/beam-quic-client", features = ["solana-transaction"] }
```

See [crates/beam-quic-client/README.md](crates/beam-quic-client/README.md) for
usage examples.
