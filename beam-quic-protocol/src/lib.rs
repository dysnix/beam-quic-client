pub const VERSION: u8 = 1;
pub const DEFAULT_ALPN: &str = "beam-submit-v1";
pub const DEFAULT_MAX_FRAME_HEADER_BYTES: usize = 4096;
pub const DEFAULT_MAX_TRANSACTION_BYTES: usize = 1232;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SubmitRequestHeader {
    pub version: u8,
    #[serde(default)]
    pub request_id: Option<String>,
    pub method: QuicMethod,
    #[serde(default)]
    pub provider: Option<Provider>,
    #[serde(default)]
    pub mode: RoutingMode,
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum QuicMethod {
    #[serde(rename = "sendTransaction")]
    SendTransaction,
    #[serde(rename = "sendTransactionFast")]
    SendTransactionFast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Bloxroute,
    Astralane,
    Nozomi,
    Falcon,
    Manka,
    Raiden,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bloxroute => "bloxroute",
            Self::Astralane => "astralane",
            Self::Nozomi => "nozomi",
            Self::Falcon => "falcon",
            Self::Manka => "manka",
            Self::Raiden => "raiden",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    #[default]
    Fastest,
    MevProtect,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SubmitResponse {
    pub version: u8,
    pub request_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<SubmitResponseResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<SubmitResponseError>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SubmitResponseResult {
    pub signature: String,
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SubmitResponseError {
    pub code: i64,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("frame is too short")]
    TooShort,
    #[error("frame header is too large: {actual} > {max}")]
    HeaderTooLarge { actual: usize, max: usize },
    #[error("transaction is too large: {actual} > {max}")]
    TransactionTooLarge { actual: usize, max: usize },
    #[error("unsupported frame version: {0}")]
    UnsupportedVersion(u8),
    #[error("invalid json header: {0}")]
    InvalidHeader(#[source] serde_json::Error),
    #[error("invalid json response: {0}")]
    InvalidResponse(#[source] serde_json::Error),
}

pub fn encode_request(
    header: &SubmitRequestHeader,
    transaction: &[u8],
) -> Result<Vec<u8>, FrameError> {
    validate_version(header.version)?;
    let header_bytes = serde_json::to_vec(header).map_err(FrameError::InvalidHeader)?;
    let mut frame = Vec::with_capacity(4 + header_bytes.len() + transaction.len());
    frame.extend_from_slice(&(header_bytes.len() as u32).to_be_bytes());
    frame.extend_from_slice(&header_bytes);
    frame.extend_from_slice(transaction);
    Ok(frame)
}

pub fn decode_request(
    frame: &[u8],
    max_header_bytes: usize,
    max_transaction_bytes: usize,
) -> Result<(SubmitRequestHeader, Vec<u8>), FrameError> {
    if frame.len() < 4 {
        return Err(FrameError::TooShort);
    }
    let header_len = u32::from_be_bytes(frame[0..4].try_into().expect("slice len")) as usize;
    if header_len > max_header_bytes {
        return Err(FrameError::HeaderTooLarge {
            actual: header_len,
            max: max_header_bytes,
        });
    }
    if frame.len() < 4 + header_len {
        return Err(FrameError::TooShort);
    }
    let tx_len = frame.len() - 4 - header_len;
    if tx_len > max_transaction_bytes {
        return Err(FrameError::TransactionTooLarge {
            actual: tx_len,
            max: max_transaction_bytes,
        });
    }
    let header = serde_json::from_slice::<SubmitRequestHeader>(&frame[4..4 + header_len])
        .map_err(FrameError::InvalidHeader)?;
    validate_version(header.version)?;
    Ok((header, frame[4 + header_len..].to_vec()))
}

pub fn encode_response(response: &SubmitResponse) -> Result<Vec<u8>, FrameError> {
    validate_version(response.version)?;
    serde_json::to_vec(response).map_err(FrameError::InvalidResponse)
}

pub fn decode_response(response: &[u8]) -> Result<SubmitResponse, FrameError> {
    let response =
        serde_json::from_slice::<SubmitResponse>(response).map_err(FrameError::InvalidResponse)?;
    validate_version(response.version)?;
    Ok(response)
}

fn validate_version(version: u8) -> Result<(), FrameError> {
    if version == VERSION {
        Ok(())
    } else {
        Err(FrameError::UnsupportedVersion(version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_frame_round_trips() {
        let header = SubmitRequestHeader {
            version: VERSION,
            request_id: Some("req-1".to_string()),
            method: QuicMethod::SendTransaction,
            provider: Some(Provider::Bloxroute),
            mode: RoutingMode::Fastest,
            signature: Some("sig".to_string()),
        };
        let frame = encode_request(&header, b"tx").unwrap();
        let (decoded, tx) = decode_request(&frame, 4096, 1232).unwrap();
        assert_eq!(decoded, header);
        assert_eq!(tx, b"tx");
    }

    #[test]
    fn rejects_oversized_header_and_tx() {
        let header = SubmitRequestHeader {
            version: VERSION,
            request_id: None,
            method: QuicMethod::SendTransaction,
            provider: None,
            mode: RoutingMode::Fastest,
            signature: None,
        };
        let frame = encode_request(&header, b"12345").unwrap();
        assert!(matches!(
            decode_request(&frame, 1, 1232).unwrap_err(),
            FrameError::HeaderTooLarge { .. }
        ));
        assert!(matches!(
            decode_request(&frame, 4096, 1).unwrap_err(),
            FrameError::TransactionTooLarge { .. }
        ));
    }

    #[test]
    fn response_round_trips() {
        let response = SubmitResponse {
            version: VERSION,
            request_id: "req-1".to_string(),
            ok: true,
            result: Some(SubmitResponseResult {
                signature: "sig".to_string(),
                provider: "bloxroute".to_string(),
            }),
            error: None,
        };
        let encoded = encode_response(&response).unwrap();
        assert_eq!(decode_response(&encoded).unwrap(), response);
    }
}
