use super::*;

pub(crate) struct MultisigHubClient {
    client: RestClient,
    base_url: String,
    token: String,
}

pub(crate) enum FileSource {
    Bytes(Vec<u8>),
    Path(PathBuf),
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct APIErrorBody {
    pub(crate) error: String,
    pub(crate) code: u16,
    pub(crate) name: String,
}

// API response/request objects

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct BumpAddressIndicesRequest {
    pub(crate) count: u32,
    pub(crate) internal: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct BumpAddressIndicesResponse {
    pub(crate) first: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmptyResponse {}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct FileMetadata {
    pub(crate) file_id: String,
    pub(crate) r#type: FileType,
    pub(crate) posted_by_xpub: String,
    pub(crate) size_bytes: u64,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) enum FileType {
    Consignment,
    Media,
    OperationData,
    OperationPsbt,
    ResponsePsbt,
    Fascia,
}

impl FileType {
    pub(crate) fn field_name(&self) -> &str {
        match self {
            FileType::OperationPsbt | FileType::ResponsePsbt => "file_psbt",
            FileType::Consignment => "file_consignment",
            FileType::Media => "file_media",
            FileType::OperationData => "file_operation_data",
            FileType::Fascia => "file_fascia",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct GetCurrentAddressIndicesResponse {
    pub(crate) internal: Option<u32>,
    pub(crate) external: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct GetFileRequest {
    pub(crate) file_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct GetOperationByIdxRequest {
    pub(crate) operation_idx: i32,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct InfoResponse {
    pub(crate) min_rgb_lib_version: String,
    pub(crate) max_rgb_lib_version: String,
    pub(crate) rgb_lib_version: String,
    pub(crate) last_operation_idx: Option<i32>,
    pub(crate) user_role: UserRoleResponse,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct MarkOperationProcessedRequest {
    pub(crate) operation_idx: i32,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct OperationResponse {
    pub(crate) operation_idx: i32,
    pub(crate) initiator_xpub: String,
    pub(crate) created_at: i64,
    pub(crate) operation_type: OperationType,
    pub(crate) status: OperationStatus,
    pub(crate) acked_by: HashSet<String>,
    pub(crate) nacked_by: HashSet<String>,
    pub(crate) threshold: Option<u8>,
    pub(crate) my_response: Option<bool>,
    pub(crate) processed_at: Option<i64>,
    pub(crate) files: Vec<FileMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) enum OperationStatus {
    Pending,
    Approved,
    Discarded,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) enum OperationType {
    CreateUtxos = 1,
    Issuance = 2,
    SendRgb = 3,
    SendBtc = 4,
    Inflation = 5,
    BlindReceive = 6,
    WitnessReceive = 7,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct PostOperationResponse {
    pub(crate) operation_idx: i32,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct RespondToOperationRequest {
    pub(crate) operation_idx: i32,
    pub(crate) ack: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct TransferStatusInfo {
    pub(crate) cosigner_xpub: String,
    pub(crate) accepted: bool,
    pub(crate) registered_at: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct TransferStatusRequest {
    pub(crate) batch_transfer_idx: i32,
    pub(crate) accept: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct TransferStatusResponse {
    pub(crate) status: Option<TransferStatusInfo>,
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) enum UserRoleResponse {
    Cosigner(String),
    WatchOnly,
}

impl MultisigHubClient {
    pub(crate) fn new(base_url: &str, token: &str) -> Result<Self, Error> {
        let client = RestClient::builder()
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT))
            .timeout(Duration::from_secs(READ_WRITE_TIMEOUT))
            .build()?;
        Ok(Self {
            client,
            base_url: base_url.to_string(),
            token: token.to_string(),
        })
    }

    fn req_err(e: impl std::fmt::Display) -> Error {
        Error::MultisigHubService {
            details: e.to_string(),
        }
    }

    pub(crate) fn map_hub_error(error_body: APIErrorBody) -> Error {
        match error_body.name.as_str() {
            "CannotPostNewOperation" => Error::MultisigOperationInProgress,
            "CannotRespondToOperation" => Error::MultisigCannotRespondToOperation {
                details: error_body.error,
            },
            "CannotMarkOperationProcessed" => Error::MultisigCannotMarkOperationProcessed {
                details: error_body.error,
            },
            "TransferStatusMismatch" => Error::MultisigTransferStatusMismatch,
            _ => Error::MultisigHubService {
                details: error_body.error,
            },
        }
    }

    pub(crate) fn bump_address_indices(&self, count: u32, internal: bool) -> Result<u32, Error> {
        let response = self
            .client
            .post(format!("{}/bumpaddressindices", self.base_url))
            .bearer_auth(&self.token)
            .json(&BumpAddressIndicesRequest { count, internal })
            .send()
            .map_err(Self::req_err)?;
        if !response.status().is_success() {
            let res = response.json::<APIErrorBody>().map_err(Self::req_err)?;
            return Err(Self::map_hub_error(res));
        }
        let res = response
            .json::<BumpAddressIndicesResponse>()
            .map_err(Self::req_err)?;
        Ok(res.first)
    }

    pub(crate) fn get_current_address_indices(
        &self,
    ) -> Result<GetCurrentAddressIndicesResponse, Error> {
        let response = self
            .client
            .get(format!("{}/getcurrentaddressindices", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .map_err(Self::req_err)?;
        if !response.status().is_success() {
            let res = response.json::<APIErrorBody>().map_err(Self::req_err)?;
            return Err(Self::map_hub_error(res));
        }
        response
            .json::<GetCurrentAddressIndicesResponse>()
            .map_err(Self::req_err)
    }

    pub(crate) fn get_file(&self, file_id: &str, out_path: impl AsRef<Path>) -> Result<(), Error> {
        let mut response = self
            .client
            .post(format!("{}/getfile", self.base_url))
            .bearer_auth(&self.token)
            .json(&GetFileRequest {
                file_id: file_id.to_string(),
            })
            .send()
            .map_err(Self::req_err)?;
        if !response.status().is_success() {
            let res = response.json::<APIErrorBody>().map_err(Self::req_err)?;
            return Err(Self::map_hub_error(res));
        }
        let file = fs::File::create(out_path)?;
        let mut out = io::BufWriter::new(file);
        io::copy(&mut response, &mut out)
            .map_err(|e| Self::req_err(format!("failed to write file: {e}")))?;
        Ok(())
    }

    pub(crate) fn get_operation_by_idx(
        &self,
        operation_idx: i32,
    ) -> Result<Option<OperationResponse>, Error> {
        let response = self
            .client
            .post(format!("{}/getoperationbyidx", self.base_url))
            .bearer_auth(&self.token)
            .json(&GetOperationByIdxRequest { operation_idx })
            .send()
            .map_err(Self::req_err)?;
        if !response.status().is_success() {
            let res = response.json::<APIErrorBody>().map_err(Self::req_err)?;
            return Err(Self::map_hub_error(res));
        }
        response
            .json::<Option<OperationResponse>>()
            .map_err(Self::req_err)
    }

    pub(crate) fn info(&self) -> Result<InfoResponse, Error> {
        let response = self
            .client
            .get(format!("{}/info", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .map_err(Self::req_err)?;
        if !response.status().is_success() {
            let res = response.json::<APIErrorBody>().map_err(Self::req_err)?;
            return Err(Self::map_hub_error(res));
        }
        response.json::<InfoResponse>().map_err(Self::req_err)
    }

    pub(crate) fn mark_operation_processed(
        &self,
        operation_idx: i32,
    ) -> Result<EmptyResponse, Error> {
        let response = self
            .client
            .post(format!("{}/markoperationprocessed", self.base_url))
            .bearer_auth(&self.token)
            .json(&MarkOperationProcessedRequest { operation_idx })
            .send()
            .map_err(Self::req_err)?;
        if !response.status().is_success() {
            let res = response.json::<APIErrorBody>().map_err(Self::req_err)?;
            return Err(Self::map_hub_error(res));
        }
        response.json::<EmptyResponse>().map_err(Self::req_err)
    }

    pub(crate) fn post_operation(
        &self,
        files: Vec<(FileType, FileSource)>,
        operation_type: OperationType,
    ) -> Result<PostOperationResponse, Error> {
        let operation_type_bytes = (operation_type as u8).to_le_bytes().to_vec();
        let operation_type_part = multipart::Part::bytes(operation_type_bytes)
            .mime_str(OCTET_STREAM)
            .expect("OCTET_STREAM is a valid MIME type");
        let mut form = multipart::Form::new().part("operation_type", operation_type_part);
        for (file_type, file_source) in files {
            let field_name = file_type.field_name().to_string();
            let file_part = match file_source {
                FileSource::Bytes(bytes) => multipart::Part::bytes(bytes)
                    .mime_str(OCTET_STREAM)
                    .expect("OCTET_STREAM is a valid MIME type"),
                FileSource::Path(path) => multipart::Part::file(path)?
                    .mime_str(OCTET_STREAM)
                    .expect("OCTET_STREAM is a valid MIME type"),
            };
            form = form.part(field_name, file_part);
        }

        let response = self
            .client
            .post(format!("{}/postoperation", self.base_url))
            .bearer_auth(&self.token)
            .multipart(form)
            .send()
            .map_err(Self::req_err)?;
        if !response.status().is_success() {
            let res = response.json::<APIErrorBody>().map_err(Self::req_err)?;
            return Err(Self::map_hub_error(res));
        }
        response
            .json::<PostOperationResponse>()
            .map_err(Self::req_err)
    }

    pub(crate) fn get_transfer_status(
        &self,
        batch_transfer_idx: i32,
    ) -> Result<Option<TransferStatusInfo>, Error> {
        let response = self
            .client
            .post(format!("{}/transferstatus", self.base_url))
            .bearer_auth(&self.token)
            .json(&TransferStatusRequest {
                batch_transfer_idx,
                accept: None,
            })
            .send()
            .map_err(Self::req_err)?;
        if !response.status().is_success() {
            let res = response.json::<APIErrorBody>().map_err(Self::req_err)?;
            return Err(Self::map_hub_error(res));
        }
        let res = response
            .json::<TransferStatusResponse>()
            .map_err(Self::req_err)?;
        Ok(res.status)
    }

    pub(crate) fn set_transfer_status(
        &self,
        batch_transfer_idx: i32,
        accept: bool,
    ) -> Result<TransferStatusInfo, Error> {
        let response = self
            .client
            .post(format!("{}/transferstatus", self.base_url))
            .bearer_auth(&self.token)
            .json(&TransferStatusRequest {
                batch_transfer_idx,
                accept: Some(accept),
            })
            .send()
            .map_err(Self::req_err)?;
        if !response.status().is_success() {
            let res = response.json::<APIErrorBody>().map_err(Self::req_err)?;
            return Err(Self::map_hub_error(res));
        }
        let res = response
            .json::<TransferStatusResponse>()
            .map_err(Self::req_err)?;
        res.status
            .ok_or_else(|| Self::req_err("expected transfer status in response but got none"))
    }

    pub(crate) fn respond_to_operation(
        &self,
        operation_idx: i32,
        respond_to_operation: RespondToOperation,
    ) -> Result<OperationResponse, Error> {
        let mut form = multipart::Form::new();
        let respond_to_operation_request = match respond_to_operation {
            RespondToOperation::Ack(psbt) => {
                let psbt = Psbt::from_str(&psbt).expect("PSBT already validated by caller");
                let file_bytes = psbt.serialize();
                let file_part = multipart::Part::bytes(file_bytes)
                    .mime_str(OCTET_STREAM)
                    .expect("OCTET_STREAM is a valid MIME type");
                form = form.part("file_psbt", file_part);
                RespondToOperationRequest {
                    operation_idx,
                    ack: true,
                }
            }
            RespondToOperation::Nack => RespondToOperationRequest {
                operation_idx,
                ack: false,
            },
        };
        let json_payload = serde_json::to_string(&respond_to_operation_request)
            .expect("RespondToOperationRequest is serializable");
        let json_part = multipart::Part::text(json_payload)
            .mime_str(JSON)
            .expect("JSON is a valid MIME type");
        form = form.part("request", json_part);

        let response = self
            .client
            .post(format!("{}/respondtooperation", self.base_url))
            .bearer_auth(&self.token)
            .multipart(form)
            .send()
            .map_err(Self::req_err)?;
        if !response.status().is_success() {
            let res = response.json::<APIErrorBody>().map_err(Self::req_err)?;
            return Err(Self::map_hub_error(res));
        }
        response.json::<OperationResponse>().map_err(Self::req_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_api_error(name: &str, error: &str, code: u16) -> APIErrorBody {
        APIErrorBody {
            error: error.to_string(),
            code,
            name: name.to_string(),
        }
    }

    #[test]
    fn map_hub_error() {
        // CannotPostNewOperation
        let body = make_api_error("CannotPostNewOperation", "operation already active", 409);
        let err = MultisigHubClient::map_hub_error(body);
        assert_eq!(err, Error::MultisigOperationInProgress);

        // CannotRespondToOperation
        let body = make_api_error(
            "CannotRespondToOperation",
            "operation already resolved",
            409,
        );
        let err = MultisigHubClient::map_hub_error(body);
        assert_eq!(
            err,
            Error::MultisigCannotRespondToOperation {
                details: "operation already resolved".to_string(),
            }
        );

        // CannotMarkOperationProcessed
        let body = make_api_error(
            "CannotMarkOperationProcessed",
            "operation not approved yet",
            409,
        );
        let err = MultisigHubClient::map_hub_error(body);
        assert_eq!(
            err,
            Error::MultisigCannotMarkOperationProcessed {
                details: "operation not approved yet".to_string(),
            }
        );

        // TransferStatusMismatch
        let body = make_api_error("TransferStatusMismatch", "status mismatch", 409);
        let err = MultisigHubClient::map_hub_error(body);
        assert_eq!(err, Error::MultisigTransferStatusMismatch);

        // Unknown error
        let body = make_api_error("SomeUnknownError", "unexpected failure", 500);
        let err = MultisigHubClient::map_hub_error(body);
        assert_eq!(
            err,
            Error::MultisigHubService {
                details: "unexpected failure".to_string(),
            }
        );

        // Empty name
        let body = make_api_error("", "no name provided", 400);
        let err = MultisigHubClient::map_hub_error(body);
        assert_eq!(
            err,
            Error::MultisigHubService {
                details: "no name provided".to_string(),
            }
        );
    }

    #[test]
    fn file_type_field_names() {
        assert_eq!(FileType::OperationPsbt.field_name(), "file_psbt");
        assert_eq!(FileType::ResponsePsbt.field_name(), "file_psbt");
        assert_eq!(FileType::Consignment.field_name(), "file_consignment");
        assert_eq!(FileType::Media.field_name(), "file_media");
        assert_eq!(FileType::OperationData.field_name(), "file_operation_data");
        assert_eq!(FileType::Fascia.field_name(), "file_fascia");
    }

    #[test]
    fn bump_address_indices_error() {
        // network error
        let client: MultisigHubClient =
            MultisigHubClient::new("http://127.0.0.1:1", "token").unwrap();
        let result = client.bump_address_indices(1, false).unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });

        // malformed JSON response
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/bumpaddressindices")
            .with_status(409)
            .with_header("content-type", JSON)
            .with_body("not valid json")
            .create();
        let client = MultisigHubClient::new(&server.url(), "test-token").unwrap();
        let result = client.bump_address_indices(1, false).unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });
        mock.assert();

        // unexpected JSON
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/bumpaddressindices")
            .with_status(409)
            .with_header("content-type", JSON)
            .with_body(r#"{"unexpected":"unexpected JSON"}"#)
            .create();
        let client = MultisigHubClient::new(&server.url(), "test-token").unwrap();
        let result = client.bump_address_indices(1, false).unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });
        mock.assert();

        // error from hub
        let mut server = mockito::Server::new();
        let body = make_api_error("AnError", "an error", 409);
        let mock = server
            .mock("POST", "/bumpaddressindices")
            .with_status(409)
            .with_header("content-type", JSON)
            .with_body(serde_json::to_string(&body).unwrap())
            .create();
        let client = MultisigHubClient::new(&server.url(), "test-token").unwrap();
        let result = client.bump_address_indices(1, false).unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });
        mock.assert();
    }

    #[test]
    fn get_current_address_indices_error() {
        // network error
        let client: MultisigHubClient =
            MultisigHubClient::new("http://127.0.0.1:1", "token").unwrap();
        let result = client.get_current_address_indices().unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });

        // malformed JSON response
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/getcurrentaddressindices")
            .with_status(200)
            .with_header("content-type", JSON)
            .with_body("not valid json")
            .create();
        let client = MultisigHubClient::new(&server.url(), "test-token").unwrap();
        let result = client.get_current_address_indices().unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });
        mock.assert();

        // JSON error
        let mut server = mockito::Server::new();
        let body = make_api_error("AnError", "an error", 409);
        let mock = server
            .mock("GET", "/getcurrentaddressindices")
            .with_status(409)
            .with_header("content-type", JSON)
            .with_body(serde_json::to_string(&body).unwrap())
            .create();
        let client = MultisigHubClient::new(&server.url(), "test-token").unwrap();
        let result = client.get_current_address_indices().unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });
        mock.assert();

        // malformed error
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/getcurrentaddressindices")
            .with_status(409)
            .with_header("content-type", JSON)
            .with_body("not valid json")
            .create();
        let client = MultisigHubClient::new(&server.url(), "test-token").unwrap();
        let result = client.get_current_address_indices().unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });
        mock.assert();
    }

    #[test]
    fn get_file_error() {
        let tmp_path = tempfile::NamedTempFile::new().unwrap();

        // network error
        let client: MultisigHubClient =
            MultisigHubClient::new("http://127.0.0.1:1", "token").unwrap();
        let result = client.get_file("123", tmp_path.path()).unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });

        // JSON error
        let mut server = mockito::Server::new();
        let body = make_api_error("FileNotFound", "file not found", 404);
        let mock = server
            .mock("POST", "/getfile")
            .with_status(404)
            .with_header("content-type", JSON)
            .with_body(serde_json::to_string(&body).unwrap())
            .create();
        let client = MultisigHubClient::new(&server.url(), "test-token").unwrap();
        let result = client.get_file("123", tmp_path.path()).unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });
        mock.assert();

        // unexpected JSON
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/getfile")
            .with_status(404)
            .with_header("content-type", JSON)
            .with_body(r#"{"unexpected":"unexpected JSON"}"#)
            .create();
        let client = MultisigHubClient::new(&server.url(), "test-token").unwrap();
        let result = client.get_file("123", tmp_path.path()).unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });
        mock.assert();
    }

    #[test]
    fn get_operation_by_idx_error() {
        // network error
        let client: MultisigHubClient =
            MultisigHubClient::new("http://127.0.0.1:1", "token").unwrap();
        let result = client.get_operation_by_idx(1).unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });

        // malformed JSON response
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/getoperationbyidx")
            .with_status(200)
            .with_header("content-type", JSON)
            .with_body("not valid json")
            .create();
        let client = MultisigHubClient::new(&server.url(), "test-token").unwrap();
        let result = client.get_operation_by_idx(1).unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });
        mock.assert();

        // unexpected JSON
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/getoperationbyidx")
            .with_status(404)
            .with_header("content-type", JSON)
            .with_body(r#"{"unexpected":"unexpected JSON"}"#)
            .create();
        let client = MultisigHubClient::new(&server.url(), "test-token").unwrap();
        let result = client.get_operation_by_idx(1).unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });
        mock.assert();

        // error from hub
        let mut server = mockito::Server::new();
        let body = make_api_error("AnError", "an error", 409);
        let mock = server
            .mock("POST", "/getoperationbyidx")
            .with_status(409)
            .with_header("content-type", JSON)
            .with_body(serde_json::to_string(&body).unwrap())
            .create();
        let client = MultisigHubClient::new(&server.url(), "test-token").unwrap();
        let result = client.get_operation_by_idx(1).unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });
        mock.assert();
    }

    #[test]
    fn info_error() {
        // network error
        let client = MultisigHubClient::new("http://127.0.0.1:1", "token").unwrap();
        let result = client.info().unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });

        // malformed JSON response
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/info")
            .with_status(200)
            .with_header("content-type", JSON)
            .with_body("not valid json")
            .create();
        let client = MultisigHubClient::new(&server.url(), "test-token").unwrap();
        let result = client.info().unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });
        mock.assert();
    }

    #[test]
    fn mark_operation_processed_error() {
        // network error
        let client = MultisigHubClient::new("http://127.0.0.1:1", "token").unwrap();
        let result = client.mark_operation_processed(1).unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });

        // malformed JSON response
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/markoperationprocessed")
            .with_status(409)
            .with_header("content-type", JSON)
            .with_body("not valid json")
            .create();
        let client = MultisigHubClient::new(&server.url(), "test-token").unwrap();
        let result = client.mark_operation_processed(1).unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });
        mock.assert();
    }

    #[test]
    fn post_operation_error() {
        // network error
        let client: MultisigHubClient =
            MultisigHubClient::new("http://127.0.0.1:1", "token").unwrap();
        let result = client
            .post_operation(vec![], OperationType::CreateUtxos)
            .unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });

        // malformed JSON response
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/postoperation")
            .with_status(200)
            .with_header("content-type", JSON)
            .with_body("not valid json")
            .create();
        let client = MultisigHubClient::new(&server.url(), "test-token").unwrap();
        let result = client
            .post_operation(vec![], OperationType::CreateUtxos)
            .unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });
        mock.assert();
    }

    #[test]
    fn respond_to_operation_error() {
        // network error
        let client: MultisigHubClient =
            MultisigHubClient::new("http://127.0.0.1:1", "token").unwrap();
        let result = client
            .respond_to_operation(1, RespondToOperation::Nack)
            .unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });

        // malformed JSON response
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/respondtooperation")
            .with_status(200)
            .with_header("content-type", JSON)
            .with_body("not valid json")
            .create();
        let client = MultisigHubClient::new(&server.url(), "test-token").unwrap();
        let result = client
            .respond_to_operation(1, RespondToOperation::Nack)
            .unwrap_err();
        assert_matches!(result, Error::MultisigHubService { .. });
        mock.assert();
    }
}
