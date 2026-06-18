//! Client for the RGB proxy protocol's JSON-RPC API.
//!
//! The protocol (v1.0) is specified in the `README.md` of
//! [rgb-http-json-rpc](https://github.com/RGB-Tools/rgb-http-json-rpc), while the rationale
//! behind it and the expected client behavior are detailed in `v1.0-changes-spec.md`, whose
//! sections the comments in this crate refer to.
//!
//! All timestamps are absolute Unix timestamps in seconds, UTC

use super::*;

/// Number of transfers an invoice is reserved for: invoices are single-use.
pub(crate) const INVOICE_QUOTA: u32 = 1;

pub struct ProxyClient {
    client: RestClient,
    base_url: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct JsonRpcError {
    pub(crate) code: i64,
    pub(crate) message: String,
    // -100 (cannot change), -200 (invalid) and -300 (missing) errors list the offending fields
    // under data.fields
    #[serde(default)]
    pub(crate) data: Option<serde_json::Value>,
}

impl JsonRpcError {
    /// Whether the error reports `field` among its offending ones (`data.fields`).
    fn has_field(&self, field: &str) -> bool {
        self.data
            .as_ref()
            .and_then(|d| d.get("fields"))
            .and_then(|f| f.as_array())
            .is_some_and(|fields| fields.iter().any(|f| f.as_str() == Some(field)))
    }

    /// Whether `data.fields` reports exactly `field` as the sole offending one.
    fn only_field(&self, field: &str) -> bool {
        self.data
            .as_ref()
            .and_then(|d| d.get("fields"))
            .and_then(|f| f.as_array())
            .is_some_and(|fields| fields.len() == 1 && fields[0].as_str() == Some(field))
    }

    /// The error message, followed by the offending fields (`data.fields`) when reported.
    fn details(&self) -> String {
        let fields: Vec<&str> = self
            .data
            .as_ref()
            .and_then(|d| d.get("fields"))
            .and_then(|f| f.as_array())
            .map(|fields| fields.iter().filter_map(|f| f.as_str()).collect())
            .unwrap_or_default();
        if fields.is_empty() {
            self.message.clone()
        } else {
            format!("{}: {}", self.message, fields.join(", "))
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct JsonRpcRequest<P> {
    method: String,
    jsonrpc: String,
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<P>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct JsonRpcResponse<R> {
    jsonrpc: String,
    id: Option<String>,
    pub(crate) result: Option<R>,
    pub(crate) error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct NullRequest;

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ServerInfoResponse {
    pub(crate) protocol_version: String,
    pub(crate) version: String,
    pub(crate) uptime: i64,
    // optional so the protocol-version check still works against older/non-compliant proxies whose
    // server.info response lacks v1.0 fields
    #[serde(default)]
    pub(crate) current_time: Option<String>,
    #[serde(default)]
    pub(crate) max_expiry_secs: Option<String>,
    #[serde(default)]
    pub(crate) max_quota: Option<String>,
    #[serde(default)]
    pub(crate) max_pow_difficulty: Option<String>,
    #[serde(default)]
    pub(crate) retention_grace_secs: Option<String>,
    #[serde(default)]
    pub(crate) max_media: Option<String>,
}

/// Status of an invoice on a recipient proxy, as reported by `invoice.check`.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum InvoiceStatus {
    /// the invoice is not known to the proxy (not reserved, or its data has been deleted)
    Unknown,
    /// the invoice is reserved and can still accept transfers
    Available,
    /// the invoice is reserved and its quota is full
    Full,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct InvoiceCheckResponse {
    pub(crate) status: InvoiceStatus,
    pub(crate) pow_difficulty: String,
}

/// State of a transfer on a recipient proxy, as reported by `transfer.list`.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProxyTransferState {
    /// the transfer data (consignment and media) is available
    Pending,
    /// the transfer data has been deleted via `transfer.retrieved`
    Retrieved,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GetConsignmentResponse {
    pub(crate) consignment: String,
    pub(crate) sender_transport_endpoint: String,
    pub(crate) sender_pk: String,
    pub(crate) consignment_hash: String,
    pub(crate) batch: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PostAckResponse {
    pub(crate) result: bool,
    pub(crate) sender_pk: Option<String>,
    pub(crate) signed_tx: Option<String>,
    pub(crate) signed_tx_hash: Option<String>,
    // the operation expiration, which together with the sender proxy retention grace sets the
    // broadcast deadline; optional so responses from non-compliant proxies are still accepted
    #[serde(default)]
    pub(crate) expires_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct GetAckResponse {
    // transfers that have not been responded to yet (pending) are absent from both maps
    pub(crate) acks: HashMap<String, bool>,
    pub(crate) signatures: HashMap<String, String>,
}

/// Per-transfer data registered with `ack.prepare`, whose signed TX travels in the uploaded file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct AckTransferData {
    pub(crate) recipient_pk: String,
    pub(crate) consignment_hash: String,
    pub(crate) signed_tx: String,
}

/// Data of a transfer posted with `consignment.post`, besides the consignment file.
#[derive(Debug, Clone, Serialize)]
pub struct PostConsignmentData {
    pub(crate) invoice_id: String,
    pub(crate) transfer_id: String,
    pub(crate) sender_transport_endpoint: String,
    pub(crate) sender_pk: String,
    pub(crate) consignment_hash: String,
    pub(crate) batch: bool,
    /// signature of `transfer_id ‖ consignment_hash ‖ blob_hash ‖ "consignment"` with the sender
    /// key
    pub(crate) signature: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct TransferIDParam {
    transfer_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct InvoiceIDsParam {
    invoice_ids: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct MediaGetParams {
    transfer_id: String,
    media_id: String,
}

/// Message authenticating an `invoice.prepare` call on a proxy with proxy-level authentication
/// enabled: `invoice_id ‖ expires_at ‖ recipient_pk ‖ quota`.
pub(crate) fn prepare_invoice_auth_message(
    invoice_id: &str,
    expires_at: &str,
    recipient_pk: &str,
) -> String {
    format!("{invoice_id}{expires_at}{recipient_pk}{INVOICE_QUOTA}")
}

/// Message authenticating an `ack.prepare` call on a proxy with proxy-level authentication
/// enabled: `sender_pk ‖ expires_at ‖ signed_tx_hash` followed, for each transfer sorted
/// lexicographically by transfer ID, by `transfer_id ‖ recipient_pk ‖ consignment_hash`.
pub(crate) fn prepare_ack_auth_message(
    sender_pk: &str,
    expires_at: &str,
    signed_tx_hash: &str,
    transfer_data: &HashMap<String, AckTransferData>,
) -> String {
    let mut message = format!("{sender_pk}{expires_at}{signed_tx_hash}");
    let mut transfer_ids: Vec<&String> = transfer_data.keys().collect();
    transfer_ids.sort();
    for transfer_id in transfer_ids {
        let data = &transfer_data[transfer_id];
        message.push_str(transfer_id);
        message.push_str(&data.recipient_pk);
        message.push_str(&data.consignment_hash);
    }
    message
}

/// Message signed by the sender to post a consignment:
/// `transfer_id ‖ consignment_hash ‖ blob_hash ‖ "consignment"`, the literal keeping it distinct
/// from the `media.post` one.
pub(crate) fn post_consignment_message(
    transfer_id: &str,
    consignment_hash: &str,
    blob_hash: &str,
) -> String {
    format!("{transfer_id}{consignment_hash}{blob_hash}consignment")
}

/// Message signed by the sender to post a media: `transfer_id ‖ media_id ‖ blob_hash ‖ "media"`,
/// the literal keeping it distinct from the `consignment.post` one.
pub(crate) fn post_media_message(transfer_id: &str, media_id: &str, blob_hash: &str) -> String {
    format!("{transfer_id}{media_id}{blob_hash}media")
}

/// Message signed by the recipient to list the transfers of an invoice: `invoice_id ‖ timestamp`.
pub(crate) fn list_transfers_message(invoice_id: &str, timestamp: &str) -> String {
    format!("{invoice_id}{timestamp}")
}

/// Message signed by the sender to get the ACKs of an operation: `sorted transfer_ids ‖ timestamp`.
pub(crate) fn get_ack_message(transfer_ids: &[String], timestamp: &str) -> String {
    let mut sorted_ids = transfer_ids.to_vec();
    sorted_ids.sort();
    format!("{}{timestamp}", sorted_ids.concat())
}

/// Message signed by the recipient to mark a transfer as retrieved: `transfer_id ‖ "retrieved"`.
pub(crate) fn transfer_retrieved_message(transfer_id: &str) -> String {
    format!("{transfer_id}retrieved")
}

#[derive(Debug, Deserialize, Serialize)]
struct PrepareInvoiceParams {
    invoice_id: String,
    expires_at: String,
    recipient_pk: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
}

#[derive(Debug, Serialize)]
struct PostConsignmentParams {
    #[serde(flatten)]
    data: PostConsignmentData,
    file_size: String,
    pow_timestamp: String,
    pow_nonce: String,
}

#[derive(Debug, Serialize)]
struct PostMediaParams {
    transfer_id: String,
    media_id: String,
    file_size: String,
    signature: String,
}

#[derive(Debug, Serialize)]
struct ListTransfersParams {
    invoice_id: String,
    signature: String,
    timestamp: String,
}

#[derive(Debug, Serialize)]
struct TransferRetrievedParams {
    transfer_id: String,
    signature: String,
}

#[derive(Debug, Serialize)]
struct AckTransferParams<'a> {
    recipient_pk: &'a str,
    consignment_hash: &'a str,
}

#[derive(Debug, Serialize)]
struct PrepareAckParams<'a> {
    transfer_data: HashMap<&'a str, AckTransferParams<'a>>,
    expires_at: String,
    signed_tx_hash: String,
    sender_pk: String,
    file_size: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
}

#[derive(Debug, Serialize)]
struct PostAckParams {
    transfer_id: String,
    ack: bool,
    signature: String,
}

#[derive(Debug, Serialize)]
struct GetAckParams {
    transfer_ids: Vec<String>,
    signature: String,
    timestamp: String,
}

#[derive(Debug, Serialize)]
struct CleanAckParams {
    transfer_ids: Vec<String>,
    signature: String,
}

/// The proof of work difficulty required by a proxy refusing a `consignment.post` call with an
/// "invalid proof of work" error, if that is the error.
pub(crate) fn pow_difficulty_required(err: &JsonRpcError) -> Option<u32> {
    if err.code != -114 {
        return None;
    }
    err.data
        .as_ref()
        .and_then(|d| d.get("pow_difficulty"))
        .and_then(|d| d.as_str())
        .and_then(|d| d.parse().ok())
}

/// Map a proxy JSON-RPC error to a typed [`Error`].
pub(crate) fn map_proxy_error(err: JsonRpcError) -> Error {
    match err.code {
        // ack.clean nulls only signed_tx (rest of the operation is untouched); a later
        // ack.prepare re-supplying it is a legitimate recovery, not a conflict
        -100 if err.only_field("signed_tx") => Error::SignedTxCleared,
        -100 => Error::ProxyCannotChange {
            details: err.details(),
        },
        -101 => Error::InvoiceNotReserved,
        -102 => Error::TransferIDInUse,
        -104 => Error::OperationNotFound,
        -105 => Error::OperationExpired,
        // all the recipients have ACKed an operation, so its TX was released and can no longer be
        // revoked
        -107 => Error::ProxyTxAlreadyReleased,
        -111 => Error::InvoiceExpired,
        -112 => Error::InvoiceQuotaExceeded,
        -113 => Error::TransferIDDeleted,
        // the proxy refuses calls starting a new transfer
        -115 => Error::ProxyDraining,
        // the transfer already has the max number of media the proxy accepts
        -116 => Error::ProxyTooManyMedia,
        // the consignment is not (or no longer, after transfer.retrieved) available
        -400 | -402 | -403 => Error::NoConsignment,
        -401 => Error::NoMedia,
        -501 => Error::InvalidProxyAuthSignature,
        -502 => Error::InvalidEndpointAuthSignature,
        -503 => Error::InvalidContentSignature,
        // an expiration in the past or beyond the proxy's max_expiry_secs
        -200 if err.has_field("expires_at") => Error::ExpirationInvalid,
        // other -200 (invalid), -300 (missing), -301 (missing file) and anything else
        _ => Error::Proxy {
            details: err.details(),
        },
    }
}

impl ProxyClient {
    pub(crate) fn new(base_url: &str) -> Result<Self, Error> {
        let client = RestClient::builder()
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT))
            .timeout(Duration::from_secs(READ_WRITE_TIMEOUT))
            .build()?;
        Ok(Self {
            client,
            base_url: base_url.to_string(),
        })
    }

    fn req_err(e: impl std::fmt::Display) -> Error {
        Error::Proxy {
            details: e.to_string(),
        }
    }

    fn file_size(path: &Path) -> Result<u64, Error> {
        Ok(std::fs::metadata(path).map_err(Self::req_err)?.len())
    }

    fn call<P: Serialize, R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Option<P>,
    ) -> Result<JsonRpcResponse<R>, Error> {
        let body = JsonRpcRequest {
            method: method.to_string(),
            jsonrpc: s!("2.0"),
            id: None,
            params,
        };
        self.client
            .post(&self.base_url)
            .header(CONTENT_TYPE, JSON)
            .json(&body)
            .send()
            .map_err(Self::req_err)?
            .json::<JsonRpcResponse<R>>()
            .map_err(Self::req_err)
    }

    fn call_multipart<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params_json: String,
        file: multipart::Part,
    ) -> Result<JsonRpcResponse<R>, Error> {
        let form = multipart::Form::new()
            .text("method", method.to_string())
            .text("jsonrpc", "2.0")
            .text("id", "null")
            .text("params", params_json)
            .part("file", file);
        self.client
            .post(&self.base_url)
            .multipart(form)
            .send()
            .map_err(Self::req_err)?
            .json::<JsonRpcResponse<R>>()
            .map_err(Self::req_err)
    }

    pub(crate) fn get_info(&self) -> Result<JsonRpcResponse<ServerInfoResponse>, Error> {
        self.call("server.info", None::<NullRequest>)
    }

    pub(crate) fn check_invoices(
        &self,
        invoice_ids: &[String],
    ) -> Result<JsonRpcResponse<HashMap<String, InvoiceCheckResponse>>, Error> {
        self.call(
            "invoice.check",
            Some(InvoiceIDsParam {
                invoice_ids: invoice_ids.to_vec(),
            }),
        )
    }

    pub(crate) fn prepare_invoice(
        &self,
        invoice_id: &str,
        expiration: &str,
        recipient_pk: &str,
        signature: Option<String>,
    ) -> Result<JsonRpcResponse<bool>, Error> {
        self.call(
            "invoice.prepare",
            Some(PrepareInvoiceParams {
                invoice_id: invoice_id.to_string(),
                expires_at: expiration.to_string(),
                recipient_pk: recipient_pk.to_string(),
                signature,
            }),
        )
    }

    pub(crate) fn get_consignment(
        &self,
        transfer_id: &str,
    ) -> Result<JsonRpcResponse<GetConsignmentResponse>, Error> {
        self.call(
            "consignment.get",
            Some(TransferIDParam {
                transfer_id: transfer_id.to_string(),
            }),
        )
    }

    pub(crate) fn post_consignment<P: AsRef<Path>>(
        &self,
        data: &PostConsignmentData,
        consignment_path: P,
        pow_timestamp: &str,
        pow_nonce: &str,
    ) -> Result<JsonRpcResponse<bool>, Error> {
        let file_size = Self::file_size(consignment_path.as_ref())?;
        let params = serde_json::to_string(&PostConsignmentParams {
            data: data.clone(),
            file_size: file_size.to_string(),
            pow_timestamp: pow_timestamp.to_string(),
            pow_nonce: pow_nonce.to_string(),
        })
        .expect("serializable");
        self.call_multipart(
            "consignment.post",
            params,
            multipart::Part::file(consignment_path)?,
        )
    }

    pub(crate) fn get_media(
        &self,
        transfer_id: &str,
        media_id: &str,
    ) -> Result<JsonRpcResponse<String>, Error> {
        self.call(
            "media.get",
            Some(MediaGetParams {
                transfer_id: transfer_id.to_string(),
                media_id: media_id.to_string(),
            }),
        )
    }

    pub(crate) fn post_media<P: AsRef<Path>>(
        &self,
        transfer_id: &str,
        media_id: &str,
        media_path: P,
        signature: &str,
    ) -> Result<JsonRpcResponse<bool>, Error> {
        let file_size = Self::file_size(media_path.as_ref())?;
        let params = serde_json::to_string(&PostMediaParams {
            transfer_id: transfer_id.to_string(),
            media_id: media_id.to_string(),
            file_size: file_size.to_string(),
            signature: signature.to_string(),
        })
        .expect("serializable");
        self.call_multipart("media.post", params, multipart::Part::file(media_path)?)
    }

    pub(crate) fn list_transfers(
        &self,
        invoice_id: &str,
        signature: &str,
        timestamp: &str,
    ) -> Result<JsonRpcResponse<HashMap<String, ProxyTransferState>>, Error> {
        self.call(
            "transfer.list",
            Some(ListTransfersParams {
                invoice_id: invoice_id.to_string(),
                signature: signature.to_string(),
                timestamp: timestamp.to_string(),
            }),
        )
    }

    pub(crate) fn transfer_retrieved(
        &self,
        transfer_id: &str,
        signature: &str,
    ) -> Result<JsonRpcResponse<bool>, Error> {
        self.call(
            "transfer.retrieved",
            Some(TransferRetrievedParams {
                transfer_id: transfer_id.to_string(),
                signature: signature.to_string(),
            }),
        )
    }

    pub(crate) fn prepare_ack(
        &self,
        transfer_data: HashMap<String, AckTransferData>,
        expiration: &str,
        signed_tx_hash: &str,
        sender_pk: &str,
        signature: Option<String>,
    ) -> Result<JsonRpcResponse<bool>, Error> {
        // the signed TXs are uploaded as a file mapping each transfer ID to its signed TX
        let signed_txs: HashMap<&str, &str> = transfer_data
            .iter()
            .map(|(tid, data)| (tid.as_str(), data.signed_tx.as_str()))
            .collect();
        let file = serde_json::to_vec(&signed_txs).expect("serializable");
        let params = serde_json::to_string(&PrepareAckParams {
            transfer_data: transfer_data
                .iter()
                .map(|(tid, data)| {
                    (
                        tid.as_str(),
                        AckTransferParams {
                            recipient_pk: &data.recipient_pk,
                            consignment_hash: &data.consignment_hash,
                        },
                    )
                })
                .collect(),
            expires_at: expiration.to_string(),
            signed_tx_hash: signed_tx_hash.to_string(),
            sender_pk: sender_pk.to_string(),
            file_size: file.len().to_string(),
            signature,
        })
        .expect("serializable");
        let file = multipart::Part::bytes(file)
            .file_name("signed_txs.json")
            .mime_str(JSON)
            .expect("valid mime");
        self.call_multipart("ack.prepare", params, file)
    }

    pub(crate) fn post_ack(
        &self,
        transfer_id: &str,
        ack: bool,
        signature: &str,
    ) -> Result<JsonRpcResponse<PostAckResponse>, Error> {
        self.call(
            "ack.post",
            Some(PostAckParams {
                transfer_id: transfer_id.to_string(),
                ack,
                signature: signature.to_string(),
            }),
        )
    }

    pub(crate) fn get_ack(
        &self,
        transfer_ids: &[String],
        signature: &str,
        timestamp: &str,
    ) -> Result<JsonRpcResponse<GetAckResponse>, Error> {
        self.call(
            "ack.get",
            Some(GetAckParams {
                transfer_ids: transfer_ids.to_vec(),
                signature: signature.to_string(),
                timestamp: timestamp.to_string(),
            }),
        )
    }

    pub(crate) fn clean_ack(
        &self,
        transfer_ids: &[String],
        signature: &str,
    ) -> Result<JsonRpcResponse<bool>, Error> {
        self.call(
            "ack.clean",
            Some(CleanAckParams {
                transfer_ids: transfer_ids.to_vec(),
                signature: signature.to_string(),
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_proxy_errors<T: std::fmt::Debug, F: Fn(&ProxyClient) -> Result<T, Error>>(f: F) {
        // network error
        let client = ProxyClient::new("http://127.0.0.1:1").unwrap();
        assert_matches!(f(&client).unwrap_err(), Error::Proxy { .. });

        // malformed JSON response
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/")
            .with_status(409)
            .with_header("content-type", JSON)
            .with_body("not valid json")
            .create();
        let client = ProxyClient::new(&server.url()).unwrap();
        assert_matches!(f(&client).unwrap_err(), Error::Proxy { .. });
        mock.assert();

        // unexpected JSON
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/")
            .with_status(409)
            .with_header("content-type", JSON)
            .with_body(r#"{"unexpected": "json"}"#)
            .create();
        let client = ProxyClient::new(&server.url()).unwrap();
        assert_matches!(f(&client).unwrap_err(), Error::Proxy { .. });
        mock.assert();
    }

    #[test]
    fn map_proxy_error_expiration() {
        let map = |code: i64, data: Option<serde_json::Value>| {
            map_proxy_error(JsonRpcError {
                code,
                message: s!("msg"),
                data,
            })
        };

        // invalid expires_at, alone or among other invalid fields
        let err = map(-200, Some(serde_json::json!({"fields": ["expires_at"]})));
        assert_matches!(err, Error::ExpirationInvalid);
        let fields = serde_json::json!({"fields": ["invoice_id", "expires_at"]});
        let err = map(-200, Some(fields));
        assert_matches!(err, Error::ExpirationInvalid);

        // other invalid fields, no data, unexpected data
        let err = map(-200, Some(serde_json::json!({"fields": ["signature"]})));
        assert_matches!(err, Error::Proxy { .. });
        let err = map(-200, None);
        assert_matches!(err, Error::Proxy { .. });
        let err = map(-200, Some(serde_json::json!("expires_at")));
        assert_matches!(err, Error::Proxy { .. });

        // a missing expires_at is not an invalid expiration
        let err = map(-300, Some(serde_json::json!({"fields": ["expires_at"]})));
        assert_matches!(err, Error::Proxy { .. });
    }

    // confirm the error mapping is correct
    //
    // -502 (InvalidEndpointAuthSignature, ack.clean/ack.get) has no reachable integration
    // test today: the auto-generated per-operation key it authenticates has no public API to
    // corrupt. The wallet's handling of it is unit-tested in wallet::online::tests
    #[test]
    fn map_proxy_error_endpoint_auth_signature() {
        let err = map_proxy_error(JsonRpcError {
            code: -502,
            message: s!("Invalid endpoint auth signature"),
            data: None,
        });
        assert_matches!(err, Error::InvalidEndpointAuthSignature);
    }

    #[test]
    fn map_proxy_error_signed_tx_cleared() {
        let map = |data: Option<serde_json::Value>| {
            map_proxy_error(JsonRpcError {
                code: -100,
                message: s!("Cannot change"),
                data,
            })
        };

        // signed_tx is the sole reported change: a benign post-ack.clean recovery
        let err = map(Some(serde_json::json!({"fields": ["signed_tx"]})));
        assert_matches!(err, Error::SignedTxCleared);

        // any other field, alone or together with signed_tx, is a genuine conflict
        let err = map(Some(serde_json::json!({"fields": ["sender_pk"]})));
        assert_matches!(err, Error::ProxyCannotChange { .. });
        let err = map(Some(
            serde_json::json!({"fields": ["signed_tx", "sender_pk"]}),
        ));
        assert_matches!(err, Error::ProxyCannotChange { .. });

        // no or unexpected data also falls back to the generic error
        let err = map(None);
        assert_matches!(err, Error::ProxyCannotChange { .. });
        let err = map(Some(serde_json::json!("signed_tx")));
        assert_matches!(err, Error::ProxyCannotChange { .. });
    }

    #[test]
    fn get_info_error() {
        assert_proxy_errors(|c| c.get_info());
    }

    #[test]
    fn check_invoices_error() {
        assert_proxy_errors(|c| c.check_invoices(&[s!("123")]));
    }

    #[test]
    fn prepare_invoice_error() {
        assert_proxy_errors(|c| c.prepare_invoice("123", "999", "pk", None));
    }

    #[test]
    fn list_transfers_error() {
        assert_proxy_errors(|c| c.list_transfers("123", "00", "999"));
    }

    #[test]
    fn transfer_retrieved_error() {
        assert_proxy_errors(|c| c.transfer_retrieved("123", "00"));
    }

    #[test]
    fn map_proxy_error_invoice_reuse() {
        let map = |code: i64| {
            map_proxy_error(JsonRpcError {
                code,
                message: s!("msg"),
                data: None,
            })
        };
        assert_matches!(map(-101), Error::InvoiceNotReserved);
        assert_matches!(map(-102), Error::TransferIDInUse);
        assert_matches!(map(-111), Error::InvoiceExpired);
        assert_matches!(map(-112), Error::InvoiceQuotaExceeded);
        assert_matches!(map(-113), Error::TransferIDDeleted);
        assert_matches!(map(-115), Error::ProxyDraining);
        assert_matches!(map(-116), Error::ProxyTooManyMedia);
        assert_matches!(map(-402), Error::NoConsignment);
        assert_matches!(map(-403), Error::NoConsignment);
    }

    #[test]
    fn pow_difficulty_from_error() {
        let err = |code: i64, data: Option<serde_json::Value>| JsonRpcError {
            code,
            message: s!("msg"),
            data,
        };
        let data = Some(serde_json::json!({"pow_difficulty": "12"}));
        assert_eq!(pow_difficulty_required(&err(-114, data.clone())), Some(12));
        assert_eq!(pow_difficulty_required(&err(-200, data)), None);
        assert_eq!(pow_difficulty_required(&err(-114, None)), None);
        let bad = Some(serde_json::json!({"pow_difficulty": "x"}));
        assert_eq!(pow_difficulty_required(&err(-114, bad)), None);
    }

    #[test]
    fn get_consignment_error() {
        assert_proxy_errors(|c| c.get_consignment("123"));
    }

    #[test]
    fn get_media_error() {
        assert_proxy_errors(|c| c.get_media("123", "456"));
    }

    #[test]
    fn post_ack_error() {
        assert_proxy_errors(|c| c.post_ack("123", true, "00"));
    }

    #[test]
    fn get_ack_error() {
        assert_proxy_errors(|c| c.get_ack(&[s!("123")], "00", "999"));
    }

    #[test]
    fn clean_ack_error() {
        assert_proxy_errors(|c| c.clean_ack(&[s!("123")], "00"));
    }

    #[test]
    fn post_consignment_error() {
        let tmp_path = tempfile::NamedTempFile::new().unwrap();
        let data = PostConsignmentData {
            invoice_id: s!("iid"),
            transfer_id: s!("123"),
            sender_transport_endpoint: s!("rpc://x"),
            sender_pk: s!("pk"),
            consignment_hash: s!("h"),
            batch: false,
            signature: s!("00"),
        };
        assert_proxy_errors(|c| c.post_consignment(&data, tmp_path.path(), "999", "0"));
    }

    #[test]
    fn post_media_error() {
        let tmp_path = tempfile::NamedTempFile::new().unwrap();
        assert_proxy_errors(|c| c.post_media("123", "456", tmp_path.path(), "00"));
    }

    #[test]
    fn prepare_ack_error() {
        assert_proxy_errors(|c| c.prepare_ack(HashMap::new(), "999", "h", "pk", None));
    }
}
