//! Indexer functionality.
//!
//! This module defines the indexer methods.

use super::*;

/// Indexer for a wallet.
#[non_exhaustive]
pub enum Indexer {
    /// Electrum indexer
    #[cfg(feature = "electrum")]
    Electrum(Box<BdkElectrumClient<ElectrumClient>>),
    /// Esplora indexer
    #[cfg(feature = "esplora")]
    Esplora(Box<EsploraClient>),
}

impl Indexer {
    pub(crate) fn block_hash(&self, height: usize) -> Result<String, IndexerError> {
        Ok(match self {
            #[cfg(feature = "electrum")]
            Indexer::Electrum(client) => {
                client.inner.block_header(height)?.block_hash().to_string()
            }
            #[cfg(feature = "esplora")]
            Indexer::Esplora(client) => client.get_block_hash(height as u32)?.to_string(),
        })
    }

    pub(crate) fn broadcast(&self, tx: &BdkTransaction) -> Result<(), IndexerError> {
        match self {
            #[cfg(feature = "electrum")]
            Indexer::Electrum(client) => {
                client.transaction_broadcast(tx)?;
                Ok(())
            }
            #[cfg(feature = "esplora")]
            Indexer::Esplora(client) => {
                client.broadcast(tx)?;
                Ok(())
            }
        }
    }

    pub(crate) fn fee_estimation(&self, blocks: u16) -> Result<f64, Error> {
        Ok(match self {
            #[cfg(feature = "electrum")]
            Indexer::Electrum(client) => {
                let estimate = client
                    .inner
                    .estimate_fee(blocks as usize)
                    .map_err(IndexerError::from)?; // in BTC/kB
                if estimate == -1.0 {
                    return Err(Error::CannotEstimateFees);
                }
                (estimate * 100_000_000.0) / 1_000.0
            }
            #[cfg(feature = "esplora")]
            Indexer::Esplora(client) => {
                let estimate_map = client.get_fee_estimates().map_err(IndexerError::from)?; // in sat/vB
                if estimate_map.is_empty() {
                    return Err(Error::CannotEstimateFees);
                }
                // map needs to be sorted for interpolation to work
                let estimate_map = BTreeMap::from_iter(estimate_map);
                match estimate_map.get(&blocks) {
                    Some(estimate) => *estimate,
                    None => {
                        // find the two closest keys
                        let mut lower_key = None;
                        let mut upper_key = None;
                        for k in estimate_map.keys() {
                            match k.cmp(&blocks) {
                                Ordering::Less => {
                                    lower_key = Some(k);
                                }
                                Ordering::Greater => {
                                    upper_key = Some(k);
                                    break;
                                }
                                _ => unreachable!("already handled"),
                            }
                        }
                        // use linear interpolation formula
                        match (lower_key, upper_key) {
                            (Some(x1), Some(x2)) => {
                                let y1 = estimate_map[x1];
                                let y2 = estimate_map[x2];
                                y1 + (blocks as f64 - *x1 as f64) / (*x2 as f64 - *x1 as f64)
                                    * (y2 - y1)
                            }
                            _ => {
                                return Err(Error::Internal {
                                    details: s!("esplora map doesn't contain the expected keys"),
                                });
                            }
                        }
                    }
                }
            }
        })
    }

    pub(crate) fn full_scan<K: Ord + Clone, R: Into<FullScanRequest<K>>>(
        &self,
        request: R,
    ) -> Result<FullScanResponse<K>, IndexerError> {
        match self {
            #[cfg(feature = "electrum")]
            Indexer::Electrum(client) => {
                Ok(client.full_scan(request, INDEXER_STOP_GAP, INDEXER_BATCH_SIZE, true)?)
            }
            #[cfg(feature = "esplora")]
            Indexer::Esplora(client) => client
                .full_scan(request, INDEXER_STOP_GAP, INDEXER_PARALLEL_REQUESTS)
                .map_err(|e| IndexerError::from(*e)),
        }
    }

    pub(crate) fn get_latest_block_height(&self) -> Result<u32, Error> {
        Ok(match self {
            #[cfg(feature = "electrum")]
            Indexer::Electrum(client) => {
                let header = client
                    .inner
                    .block_headers_subscribe()
                    .map_err(IndexerError::from)?;
                u32::try_from(header.height).map_err(|_| Error::Indexer {
                    details: s!("electrs returned invalid height"),
                })?
            }
            #[cfg(feature = "esplora")]
            Indexer::Esplora(client) => client.get_height().map_err(IndexerError::from)?,
        })
    }

    pub(crate) fn get_tx_confirmations(&self, txid: &str) -> Result<Option<u64>, Error> {
        Ok(match self {
            #[cfg(feature = "electrum")]
            Indexer::Electrum(client) => {
                let tx_details = match client.inner.raw_call(
                    "blockchain.transaction.get",
                    vec![Param::String(txid.to_string()), Param::Bool(true)],
                ) {
                    Ok(td) => Ok(td),
                    Err(e) => {
                        if e.to_string()
                            .contains("No such mempool or blockchain transaction")
                        {
                            return Ok(None);
                        } else if e.to_string().contains(
                            "genesis block coinbase is not considered an ordinary transaction",
                        ) {
                            return Ok(Some(u64::MAX));
                        } else {
                            Err(IndexerError::from(e))
                        }
                    }
                }?;
                if let Some(confirmations) = tx_details.get("confirmations") {
                    Some(
                        confirmations
                            .as_u64()
                            .expect("confirmations to be a valid u64 number"),
                    )
                } else {
                    Some(0)
                }
            }
            #[cfg(feature = "esplora")]
            Indexer::Esplora(client) => {
                let txid = Txid::from_str(txid).unwrap();
                let tx_status = client.get_tx_status(&txid).map_err(IndexerError::from)?;
                if let Some(tx_height) = tx_status.block_height {
                    let height = self.get_latest_block_height()?;
                    // a tx height greater than the tip should never happen: it points to
                    // an inconsistent indexer (e.g. the two values served from different
                    // sources/replicas, or a reorg between the reads), so surface it as an
                    // error rather than underflowing
                    let blocks_since = height.checked_sub(tx_height).ok_or_else(|| {
                        Error::Indexer {
                            details: s!(
                                "indexer reported a transaction height greater than the chain tip"
                            ),
                        }
                    })?;
                    Some(u64::from(blocks_since) + 1)
                } else if client.get_tx(&txid).map_err(IndexerError::from)?.is_none() {
                    None
                } else {
                    Some(0)
                }
            }
        })
    }

    pub(crate) fn populate_tx_cache(
        &self,
        #[cfg_attr(feature = "esplora", allow(unused))] bdk_wallet: &PersistedWallet<BdkStore>,
    ) {
        match self {
            #[cfg(feature = "electrum")]
            Indexer::Electrum(client) => {
                client.populate_tx_cache(bdk_wallet.tx_graph().full_txs().map(|tx_node| tx_node.tx))
            }
            #[cfg(feature = "esplora")]
            Indexer::Esplora(_) => {}
        }
    }

    pub(crate) fn sync<I: 'static>(
        &self,
        request: impl Into<SyncRequest<I>>,
    ) -> Result<SyncResponse, IndexerError> {
        match self {
            #[cfg(feature = "electrum")]
            Indexer::Electrum(client) => Ok(client.sync(request, INDEXER_BATCH_SIZE, true)?),
            #[cfg(feature = "esplora")]
            Indexer::Esplora(client) => client
                .sync(request, INDEXER_PARALLEL_REQUESTS)
                .map_err(|e| IndexerError::from(*e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    const TEST_TXID: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    #[cfg(feature = "electrum")]
    const GENESIS_HEADER_HEX: &str = "0100000000000000000000000000000000000000000000000000000000000000000000003ba3edfd7a7b12b27ac72c3e67768f617fc81bc3888a51323a9fb8aa4b1e5e4a29ab5f49ffff001d1dac2b7c";

    #[cfg(feature = "electrum")]
    fn electrum_indexer(url: &str) -> Indexer {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let opts = ConfigBuilder::new().retry(0).timeout(Some(1)).build();
        let client = ElectrumClient::from_config(url, opts).expect("electrum client");
        Indexer::Electrum(Box::new(BdkElectrumClient::new(client)))
    }

    #[cfg(feature = "esplora")]
    fn esplora_indexer(url: &str) -> Indexer {
        let opts = EsploraBuilder::new(url).max_retries(0).timeout(1);
        Indexer::Esplora(Box::new(EsploraClient::from_builder(opts)))
    }

    #[cfg(feature = "electrum")]
    fn electrum_tx_get_result(req: &serde_json::Value, result: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":{},"result":{result}}}"#,
            req["id"]
        )
    }

    #[cfg(feature = "electrum")]
    fn electrum_tx_get_error(req: &serde_json::Value, message: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":{},"error":{{"code":-32603,"message":"{message}"}}}}"#,
            req["id"]
        )
    }

    #[cfg(feature = "electrum")]
    fn start_electrum_mock(
        handler: impl Fn(&str, &serde_json::Value) -> String + Send + 'static,
    ) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock electrum");
        let port = listener.local_addr().expect("mock electrum addr").port();
        let url = format!("tcp://127.0.0.1:{port}");

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("mock electrum accept");
            let mut reader = BufReader::new(stream.try_clone().expect("clone mock stream"));
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .expect("read mock electrum request");
            let req: serde_json::Value =
                serde_json::from_str(line.trim()).expect("parse mock electrum request");
            let method = req["method"]
                .as_str()
                .expect("mock electrum request method");
            let id = req["id"].clone();
            let response = handler(method, &req);
            if response.is_empty() {
                return;
            }
            let body = if response.starts_with('{') {
                response
            } else {
                format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{response}}}"#)
            };
            let _ = writeln!(stream, "{body}");
        });

        (url, handle)
    }

    #[cfg(feature = "esplora")]
    fn assert_fee_rate_approx(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

    #[cfg(feature = "esplora")]
    fn run_esplora_fee_estimation_case(
        fee_estimates_body: &str,
        blocks: u16,
    ) -> Result<f64, Error> {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/fee-estimates")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(fee_estimates_body)
            .create();

        let result = esplora_indexer(&server.url()).fee_estimation(blocks);
        mock.assert();
        result
    }

    #[cfg(feature = "esplora")]
    fn minimal_tx_raw_bytes() -> Vec<u8> {
        use bdk_wallet::bitcoin::{
            Amount, OutPoint, ScriptBuf, Sequence, TxIn, TxOut, Witness, absolute::LockTime,
            transaction::Version,
        };

        let tx = BdkTransaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::new(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(50_000_000_000),
                script_pubkey: ScriptBuf::new(),
            }],
        };
        bdk_wallet::bitcoin::consensus::encode::serialize(&tx)
    }

    #[cfg(feature = "esplora")]
    fn run_esplora_get_tx_confirmations_case(
        txid: &str,
        status_body: &str,
        tip_height: Option<u32>,
        raw_tx: Option<Option<Vec<u8>>>,
    ) -> Result<Option<u64>, Error> {
        let mut server = mockito::Server::new();
        let mock_status = server
            .mock("GET", format!("/tx/{txid}/status").as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(status_body)
            .create();

        let mock_height = tip_height.map(|height| {
            server
                .mock("GET", "/blocks/tip/height")
                .with_status(200)
                .with_header("content-type", "text/plain")
                .with_body(height.to_string())
                .create()
        });

        let mock_raw = raw_tx.map(|raw_tx| {
            let mock = server.mock("GET", format!("/tx/{txid}/raw").as_str());
            match raw_tx {
                Some(bytes) => mock
                    .with_status(200)
                    .with_header("content-type", "application/octet-stream")
                    .with_body(bytes)
                    .create(),
                None => mock.with_status(404).create(),
            }
        });

        let result = esplora_indexer(&server.url()).get_tx_confirmations(txid);

        mock_status.assert();
        if let Some(mock) = mock_height {
            mock.assert();
        }
        if let Some(mock) = mock_raw {
            mock.assert();
        }

        result
    }

    #[cfg(feature = "electrum")]
    #[test]
    fn test_electrum_fee_estimation_unavailable() {
        let (url, handle) = start_electrum_mock(|method, _| {
            assert_eq!(method, "blockchain.estimatefee");
            "-1.0".into()
        });
        let indexer = electrum_indexer(&url);

        let res = indexer.fee_estimation(6).unwrap_err();
        assert_matches!(res, Error::CannotEstimateFees);
        handle.join().expect("mock electrum thread");
    }

    #[cfg(feature = "electrum")]
    #[test]
    fn test_electrum_fee_estimation_protocol_error() {
        let (url, handle) = start_electrum_mock(|method, req| {
            assert_eq!(method, "blockchain.estimatefee");
            format!(
                r#"{{"jsonrpc":"2.0","id":{},"error":{{"code":-32603,"message":"internal error"}}}}"#,
                req["id"]
            )
        });
        let indexer = electrum_indexer(&url);

        let res = indexer.fee_estimation(6).unwrap_err();
        assert_matches!(res, Error::Indexer { .. });
        handle.join().expect("mock electrum thread");
    }

    #[cfg(feature = "electrum")]
    #[test]
    fn test_electrum_fee_estimation_success() {
        let (url, handle) = start_electrum_mock(|method, req| {
            assert_eq!(method, "blockchain.estimatefee");
            assert_eq!(req["params"], serde_json::json!([6]));
            "0.0001".into()
        });
        let indexer = electrum_indexer(&url);

        let res = indexer.fee_estimation(6).unwrap();
        assert_eq!(res, 10.0);
        handle.join().expect("mock electrum thread");
    }

    #[cfg(feature = "electrum")]
    #[test]
    fn test_electrum_get_tx_confirmations_not_found() {
        let (url, handle) = start_electrum_mock(|method, req| {
            assert_eq!(method, "blockchain.transaction.get");
            assert_eq!(req["params"], serde_json::json!([TEST_TXID, true]));
            electrum_tx_get_error(req, "No such mempool or blockchain transaction")
        });
        let indexer = electrum_indexer(&url);

        let res = indexer.get_tx_confirmations(TEST_TXID).unwrap();
        assert_eq!(res, None);
        handle.join().expect("mock electrum thread");
    }

    #[cfg(feature = "electrum")]
    #[test]
    fn test_electrum_get_tx_confirmations_confirmed() {
        let (url, handle) = start_electrum_mock(|method, req| {
            assert_eq!(method, "blockchain.transaction.get");
            electrum_tx_get_result(req, r#"{"confirmations":42,"hex":"00"}"#)
        });
        let indexer = electrum_indexer(&url);

        let res = indexer.get_tx_confirmations(TEST_TXID).unwrap();
        assert_eq!(res, Some(42));
        handle.join().expect("mock electrum thread");
    }

    #[cfg(feature = "electrum")]
    #[test]
    fn test_electrum_get_tx_confirmations_mempool() {
        let (url, handle) = start_electrum_mock(|method, req| {
            assert_eq!(method, "blockchain.transaction.get");
            electrum_tx_get_result(req, r#"{"hex":"00"}"#)
        });
        let indexer = electrum_indexer(&url);

        let res = indexer.get_tx_confirmations(TEST_TXID).unwrap();
        assert_eq!(res, Some(0));
        handle.join().expect("mock electrum thread");
    }

    #[cfg(feature = "electrum")]
    #[test]
    fn test_electrum_get_tx_confirmations_genesis_coinbase() {
        let (url, handle) = start_electrum_mock(|method, req| {
            assert_eq!(method, "blockchain.transaction.get");
            electrum_tx_get_error(
                req,
                "genesis block coinbase is not considered an ordinary transaction",
            )
        });
        let indexer = electrum_indexer(&url);

        let res = indexer.get_tx_confirmations(TEST_TXID).unwrap();
        assert_eq!(res, Some(u64::MAX));
        handle.join().expect("mock electrum thread");
    }

    #[cfg(feature = "electrum")]
    #[test]
    fn test_electrum_get_tx_confirmations_protocol_error() {
        let (url, handle) = start_electrum_mock(|method, req| {
            assert_eq!(method, "blockchain.transaction.get");
            electrum_tx_get_error(req, "internal error")
        });
        let indexer = electrum_indexer(&url);

        let res = indexer.get_tx_confirmations(TEST_TXID).unwrap_err();
        assert_matches!(res, Error::Indexer { .. });
        handle.join().expect("mock electrum thread");
    }

    #[cfg(feature = "electrum")]
    #[test]
    fn test_electrum_network_error() {
        let (url, handle) = start_electrum_mock(|_, _| String::new());
        let indexer = electrum_indexer(&url);

        let res = indexer.get_latest_block_height().unwrap_err();
        assert_matches!(res, Error::Indexer { .. });
        handle.join().expect("mock electrum thread");
    }

    #[cfg(feature = "electrum")]
    #[test]
    fn test_electrum_invalid_height_response() {
        let (url, handle) = start_electrum_mock(|method, req| {
            assert_eq!(method, "blockchain.headers.subscribe");
            format!(
                r#"{{"jsonrpc":"2.0","id":{},"result":{{"height":{},"hex":"{GENESIS_HEADER_HEX}"}}}}"#,
                req["id"],
                u64::from(u32::MAX) + 1
            )
        });
        let indexer = electrum_indexer(&url);

        let res = indexer.get_latest_block_height().unwrap_err();
        assert_matches!(
            res,
            Error::Indexer { details } if details.contains("electrs returned invalid height")
        );
        handle.join().expect("mock electrum thread");
    }

    #[cfg(feature = "electrum")]
    #[test]
    fn test_electrum_broadcast_failure() {
        let (url, handle) = start_electrum_mock(|method, req| {
            assert_eq!(method, "blockchain.transaction.broadcast");
            format!(
                r#"{{"jsonrpc":"2.0","id":{},"error":{{"code":-32603,"message":"broadcast failed"}}}}"#,
                req["id"]
            )
        });
        let indexer = electrum_indexer(&url);

        let tx = BdkTransaction {
            version: bdk_wallet::bitcoin::transaction::Version::TWO,
            lock_time: bdk_wallet::bitcoin::locktime::absolute::LockTime::ZERO,
            input: vec![],
            output: vec![],
        };
        let res = indexer.broadcast(&tx).unwrap_err();
        assert_matches!(res, IndexerError::Electrum { .. });
        handle.join().expect("mock electrum thread");
    }

    #[cfg(feature = "esplora")]
    #[test]
    fn test_esplora_fee_estimation_empty_estimates() {
        let res = run_esplora_fee_estimation_case("{}", 6).unwrap_err();
        assert_matches!(res, Error::CannotEstimateFees);
    }

    #[cfg(feature = "esplora")]
    #[test]
    fn test_esplora_fee_estimation_exact_match() {
        let res = run_esplora_fee_estimation_case(r#"{"6": 7.5}"#, 6).unwrap();
        assert_eq!(res, 7.5);
    }

    #[cfg(feature = "esplora")]
    #[test]
    fn test_esplora_fee_estimation_interpolation() {
        // no exact fee-estimate key; interpolate between lower and upper bounds
        // fee decreases between bounds
        let res = run_esplora_fee_estimation_case(r#"{"1": 10.0, "10": 2.0}"#, 6).unwrap();
        assert_fee_rate_approx(res, 50.0 / 9.0);

        // fee increases between bounds
        let res = run_esplora_fee_estimation_case(r#"{"1": 2.0, "10": 10.0}"#, 6).unwrap();
        assert_fee_rate_approx(res, 58.0 / 9.0);

        // at the lower bound side of the interval
        let res = run_esplora_fee_estimation_case(r#"{"1": 10.0, "10": 2.0}"#, 2).unwrap();
        assert_fee_rate_approx(res, 82.0 / 9.0);

        // at the upper bound side of the interval
        let res = run_esplora_fee_estimation_case(r#"{"1": 10.0, "10": 2.0}"#, 9).unwrap();
        assert_fee_rate_approx(res, 26.0 / 9.0);

        // blocks above the highest key: lower bound only
        let res = run_esplora_fee_estimation_case(r#"{"1": 10.0, "3": 5.0}"#, 6).unwrap_err();
        assert_matches!(
            res,
            Error::Internal { details } if details.contains("esplora map doesn't contain the expected keys")
        );

        // blocks below the lowest key: upper bound only
        let res = run_esplora_fee_estimation_case(r#"{"3": 5.0, "10": 2.0}"#, 1).unwrap_err();
        assert_matches!(
            res,
            Error::Internal { details } if details.contains("esplora map doesn't contain the expected keys")
        );
    }

    #[cfg(feature = "esplora")]
    #[test]
    fn test_esplora_fee_estimation_network_error() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/fee-estimates")
            .with_status(500)
            .create();

        let indexer = esplora_indexer(&server.url());

        let res = indexer.fee_estimation(6).unwrap_err();
        assert_matches!(res, Error::Indexer { .. });
        mock.assert();
    }

    #[cfg(feature = "esplora")]
    #[test]
    fn test_esplora_get_tx_confirmations_confirmed() {
        let res = run_esplora_get_tx_confirmations_case(
            TEST_TXID,
            r#"{"confirmed": true, "block_height": 95, "block_hash": "0000000000000000000000000000000000000000000000000000000000000000", "block_time": 1600000000}"#,
            Some(100),
            None,
        )
        .unwrap();
        assert_eq!(res, Some(6));
    }

    #[cfg(feature = "esplora")]
    #[test]
    fn test_esplora_get_tx_confirmations_mempool() {
        let raw_tx = minimal_tx_raw_bytes();
        let res = run_esplora_get_tx_confirmations_case(
            TEST_TXID,
            r#"{"confirmed": false}"#,
            None,
            Some(Some(raw_tx)),
        )
        .unwrap();
        assert_eq!(res, Some(0));
    }

    #[cfg(feature = "esplora")]
    #[test]
    fn test_esplora_get_tx_confirmations_not_found() {
        let res = run_esplora_get_tx_confirmations_case(
            TEST_TXID,
            r#"{"confirmed": false}"#,
            None,
            Some(None),
        )
        .unwrap();
        assert_eq!(res, None);
    }

    #[cfg(feature = "esplora")]
    #[test]
    fn test_esplora_get_tx_confirmations_inconsistent_height() {
        let txid = TEST_TXID;
        let mut server = mockito::Server::new();
        let mock_status = server
            .mock("GET", format!("/tx/{}/status", txid).as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"confirmed": true, "block_height": 101, "block_hash": "0000000000000000000000000000000000000000000000000000000000000000", "block_time": 1600000000}"#)
            .create();
        let mock_height = server
            .mock("GET", "/blocks/tip/height")
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body("100")
            .create();

        let indexer = esplora_indexer(&server.url());

        let res = indexer.get_tx_confirmations(txid).unwrap_err();
        assert_matches!(
            res,
            Error::Indexer { details } if details.contains("indexer reported a transaction height greater than the chain tip")
        );
        mock_status.assert();
        mock_height.assert();
    }

    #[cfg(feature = "esplora")]
    #[test]
    fn test_esplora_network_error() {
        let indexer = esplora_indexer("http://127.0.0.1:1");

        let res = indexer.get_latest_block_height().unwrap_err();
        assert_matches!(res, Error::Indexer { .. });
    }

    #[cfg(feature = "esplora")]
    #[test]
    fn test_esplora_invalid_height_response() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/blocks/tip/height")
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body("invalid")
            .create();

        let indexer = esplora_indexer(&server.url());

        let res = indexer.get_latest_block_height().unwrap_err();
        assert_matches!(res, Error::Indexer { .. });
        mock.assert();
    }

    #[cfg(feature = "esplora")]
    #[test]
    fn test_esplora_broadcast_failure() {
        let mut server = mockito::Server::new();
        let mock = server.mock("POST", "/tx").with_status(500).create();

        let indexer = esplora_indexer(&server.url());

        let tx = BdkTransaction {
            version: bdk_wallet::bitcoin::transaction::Version::TWO,
            lock_time: bdk_wallet::bitcoin::locktime::absolute::LockTime::ZERO,
            input: vec![],
            output: vec![],
        };
        let res = indexer.broadcast(&tx).unwrap_err();
        assert_matches!(res, IndexerError::Esplora { .. });
        mock.assert();
    }
}
