# beam-quic-client

Rust client and shared protocol types for Beam QUIC `sendTransaction` and
`sendTransactionFast`.

## Install From Git

```toml
[dependencies]
beam-quic-client = "0.1.1"
```

Enable Solana transaction helpers when you want to pass a
`VersionedTransaction` directly:

```toml
[dependencies]
beam-quic-client = { version = "0.1.1", features = ["solana-transaction"] }
```

See [beam-quic-client/README.md](beam-quic-client/README.md) for
usage examples.
