# beam-quic-protocol

Shared Beam QUIC frame types and codecs. Install with
`beam-quic-protocol = "0.2.0"`.

Providers: `Bloxroute`, `Astralane`, `Nozomi`, and `Manka`. Manka uses
`RoutingMode::Fastest`. Version 0.2.0 removes the Falcon, Raiden, and Apex enum
variants. Framing remains version 1 and ALPN remains `beam-submit-v1`;
retained providers remain wire-compatible with previous clients.

For submission examples and Manka tip addresses, see the
[client documentation](https://github.com/dysnix/beam-quic-client/blob/main/beam-quic-client/README.md).
