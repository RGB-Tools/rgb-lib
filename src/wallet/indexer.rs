//! Indexer functionality.
//!
//! This module defines the indexer methods.

use super::*;

/// Error code Bitcoin Core returns for an unknown transaction.
#[cfg(feature = "bitcoind")]
const BITCOIND_RPC_INVALID_ADDRESS_OR_KEY: i32 = -5;

/// Indexer for a wallet.
#[non_exhaustive]
pub enum Indexer {
    /// Bitcoin Core node (with a synced `-txindex`)
    #[cfg(feature = "bitcoind")]
    Bitcoind(Box<BitcoindClient>),
    /// Electrum indexer
    #[cfg(feature = "electrum")]
    Electrum(Box<BdkElectrumClient<ElectrumClient>>),
    /// Esplora indexer
    #[cfg(feature = "esplora")]
    Esplora(Box<EsploraClient>),
}

/// Bitcoin Core RPC client with the sync capabilities detected when going online.
#[cfg(feature = "bitcoind")]
pub struct BitcoindClient {
    pub(crate) inner: BitcoindRpcClient,
    /// Whether the node has a synced BIP158 block filter index (`-blockfilterindex`), allowing
    /// the wallet to fetch only the blocks matching its scripts instead of every block
    pub(crate) block_filters: bool,
}

#[cfg(feature = "bitcoind")]
impl std::ops::Deref for BitcoindClient {
    type Target = BitcoindRpcClient;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Status of the Bitcoin Core node indexes relevant to the wallet.
#[cfg(feature = "bitcoind")]
pub(crate) struct BitcoindIndexes {
    pub(crate) txindex: bool,
    pub(crate) block_filters: bool,
}

#[cfg(feature = "bitcoind")]
impl BitcoindClient {
    /// Which indexes the node maintains and has finished building.
    pub(crate) fn indexes(&self) -> Result<BitcoindIndexes, IndexerError> {
        let info = self.inner.get_index_info()?;
        let synced = |status: Option<BitcoindIndexStatus>| matches!(status, Some(s) if s.synced);
        Ok(BitcoindIndexes {
            txindex: synced(info.txindex),
            block_filters: synced(info.basic_block_filter_index),
        })
    }

    /// Bring the BDK wallet up to date with the Bitcoin Core node: apply the blocks since the
    /// wallet's last checkpoint (handling reorgs), then the mempool.
    ///
    /// With a block filter index on the node only the blocks matching the wallet scripts are
    /// fetched, otherwise every block is.
    pub(crate) fn sync_bdk_wallet(&self, bdk_wallet: &mut BdkWallet) -> Result<(), Error> {
        let bdk_sync_error = |e: &dyn fmt::Display| Error::FailedBdkSync {
            details: e.to_string(),
        };

        // the emitter tracks the unconfirmed TXs the wallet knows to report the evicted ones
        let unconfirmed_txs: Vec<_> = bdk_wallet
            .transactions()
            .filter(|tx| !tx.chain_position.is_confirmed())
            .map(|tx| tx.tx_node.tx.clone())
            .collect();

        if self.block_filters {
            self.sync_block_filters(bdk_wallet)?;
        } else {
            let mut emitter = BitcoindEmitter::new(
                &self.inner,
                bdk_wallet.latest_checkpoint(),
                0,
                unconfirmed_txs.clone(),
            );
            while let Some(event) = emitter.next_block().map_err(IndexerError::from)? {
                let height = event.block_height();
                let connected_to = event.connected_to();
                bdk_wallet
                    .apply_block_connected_to(&event.block, height, connected_to)
                    .map_err(|e| bdk_sync_error(&e))?;
            }
        }

        let mut emitter = BitcoindEmitter::new(
            &self.inner,
            bdk_wallet.latest_checkpoint(),
            0,
            unconfirmed_txs,
        );
        let mempool = emitter.mempool().map_err(IndexerError::from)?;
        bdk_wallet.apply_unconfirmed_txs(mempool.update);
        bdk_wallet.apply_evicted_txs(mempool.evicted);
        Ok(())
    }

    /// Apply the blocks since the wallet's last checkpoint using the node BIP158 filters, fetching
    /// only the ones matching the wallet scripts.
    ///
    /// The filters are matched against the revealed scripts plus the lookahead ones: if a pass
    /// reveals new scripts (as the used ones get discovered), the filters are scanned again from
    /// the same starting point so no gap in the derivation is left unchecked.
    fn sync_block_filters(&self, bdk_wallet: &mut BdkWallet) -> Result<(), Error> {
        let bdk_sync_error = |e: &dyn fmt::Display| Error::FailedBdkSync {
            details: e.to_string(),
        };
        let next_indices = |bdk_wallet: &BdkWallet| -> Vec<(KeychainKind, u32)> {
            bdk_wallet
                .keychains()
                .map(|(kc, _)| (kc, bdk_wallet.next_derivation_index(kc)))
                .collect()
        };
        let start_cp = bdk_wallet.latest_checkpoint();
        loop {
            let indices = next_indices(bdk_wallet);
            let lookahead = bdk_wallet.spk_index().lookahead();
            let spks: Vec<ScriptBuf> = indices
                .iter()
                .flat_map(|(kc, next_index)| {
                    bdk_wallet
                        .unbounded_spk_iter(*kc)
                        .take((next_index + lookahead) as usize)
                        .map(|(_, spk)| spk)
                })
                .collect();
            for event in BitcoindFilterIter::new(&self.inner, start_cp.clone(), spks) {
                let event = event.map_err(|e| Error::Indexer {
                    details: e.to_string(),
                })?;
                // advance the chain to this block whether it matched or not, so the blocks
                // that did match always have their predecessor in the chain to connect to
                let block_id = event.cp.block_id();
                bdk_wallet
                    .apply_update(Update {
                        chain: Some(event.cp),
                        ..Default::default()
                    })
                    .map_err(|e| bdk_sync_error(&e))?;
                if let Some(block) = &event.block {
                    bdk_wallet
                        .apply_block_connected_to(block, block_id.height, block_id)
                        .map_err(|e| bdk_sync_error(&e))?;
                }
            }
            if next_indices(bdk_wallet) == indices {
                break;
            }
        }
        Ok(())
    }
}

impl Indexer {
    pub(crate) fn block_hash(&self, height: usize) -> Result<String, IndexerError> {
        Ok(match self {
            #[cfg(feature = "bitcoind")]
            Indexer::Bitcoind(client) => client.get_block_hash(height as u64)?.to_string(),
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
            #[cfg(feature = "bitcoind")]
            Indexer::Bitcoind(client) => {
                client.send_raw_transaction(tx)?;
                Ok(())
            }
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
            #[cfg(feature = "bitcoind")]
            Indexer::Bitcoind(client) => {
                let estimate = client
                    .estimate_smart_fee(blocks, None)
                    .map_err(IndexerError::from)?;
                // fee_rate is in BTC/kvB
                let Some(fee_rate) = estimate.fee_rate else {
                    return Err(Error::CannotEstimateFees);
                };
                fee_rate.to_sat() as f64 / 1_000.0
            }
            #[cfg(feature = "electrum")]
            Indexer::Electrum(client) => {
                let estimate = client
                    .inner
                    .estimate_fee(blocks as usize, None)
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

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn full_scan<K: Ord + Clone, R: Into<FullScanRequest<K>>>(
        &self,
        request: R,
    ) -> Result<FullScanResponse<K>, IndexerError> {
        match self {
            #[cfg(feature = "bitcoind")]
            Indexer::Bitcoind(_) => {
                unreachable!("bitcoind syncs via BitcoindClient::sync_bdk_wallet")
            }
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
            #[cfg(feature = "bitcoind")]
            Indexer::Bitcoind(client) => {
                let height = client.get_block_count().map_err(IndexerError::from)?;
                u32::try_from(height).map_err(|_| Error::Indexer {
                    details: s!("bitcoind returned invalid height"),
                })?
            }
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
        #[cfg(test)]
        if let Some((_, height)) = mock_chain_tx(txid) {
            let tip = self.get_latest_block_height()?;
            return Ok(Some(u64::from(tip.saturating_sub(height) + 1)));
        }
        Ok(match self {
            #[cfg(feature = "bitcoind")]
            Indexer::Bitcoind(client) => {
                let txid = Txid::from_str(txid).unwrap();
                match client.get_raw_transaction_info(&txid, None) {
                    Ok(info) => Some(u64::from(info.confirmations.unwrap_or(0))),
                    Err(BitcoindRpcError::JsonRpc(BitcoindJsonRpcError::Rpc(e)))
                        if e.code == BITCOIND_RPC_INVALID_ADDRESS_OR_KEY =>
                    {
                        if e.message.contains("genesis block coinbase") {
                            Some(u64::MAX)
                        } else {
                            // the node has a synced txindex (checked when going online), so
                            // an unknown TX is neither mined nor in the mempool
                            None
                        }
                    }
                    Err(e) => return Err(IndexerError::from(e).into()),
                }
            }
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
        #[cfg_attr(any(feature = "esplora", feature = "bitcoind"), allow(unused))]
        bdk_wallet: &BdkWallet,
    ) {
        match self {
            #[cfg(feature = "bitcoind")]
            Indexer::Bitcoind(_) => {}
            #[cfg(feature = "electrum")]
            Indexer::Electrum(client) => {
                client.populate_tx_cache(bdk_wallet.tx_graph().full_txs().map(|tx_node| tx_node.tx))
            }
            #[cfg(feature = "esplora")]
            Indexer::Esplora(_) => {}
        }
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    pub(crate) fn sync<I: 'static>(
        &self,
        request: impl Into<SyncRequest<I>>,
    ) -> Result<SyncResponse, IndexerError> {
        match self {
            #[cfg(feature = "bitcoind")]
            Indexer::Bitcoind(_) => {
                unreachable!("bitcoind syncs via BitcoindClient::sync_bdk_wallet")
            }
            #[cfg(feature = "electrum")]
            Indexer::Electrum(client) => Ok(client.sync(request, INDEXER_BATCH_SIZE, true)?),
            #[cfg(feature = "esplora")]
            Indexer::Esplora(client) => client
                .sync(request, INDEXER_PARALLEL_REQUESTS)
                .map_err(|e| IndexerError::from(*e)),
        }
    }
}

#[cfg(all(test, feature = "_online"))]
mod tests {
    use super::*;

    #[cfg(feature = "_online")]
    const TEST_TXID: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    #[cfg(feature = "electrum")]
    const GENESIS_HEADER_HEX: &str = "0100000000000000000000000000000000000000000000000000000000000000000000003ba3edfd7a7b12b27ac72c3e67768f617fc81bc3888a51323a9fb8aa4b1e5e4a29ab5f49ffff001d1dac2b7c";

    #[cfg(feature = "bitcoind")]
    fn bitcoind_indexer(url: &str) -> Indexer {
        let client =
            BitcoindRpcClient::new(url, BitcoindRpcAuth::UserPass(s!("user"), s!("password")))
                .expect("bitcoind client");
        Indexer::Bitcoind(Box::new(BitcoindClient {
            inner: client,
            block_filters: false,
        }))
    }

    #[cfg(feature = "electrum")]
    fn electrum_indexer(url: &str) -> Indexer {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let opts = ConfigBuilder::new()
            .retry(0)
            .timeout(Some(Duration::from_secs(1)))
            .build();
        let client = ElectrumClient::from_config(url, opts).expect("electrum client");
        Indexer::Electrum(Box::new(BdkElectrumClient::new(client)))
    }

    #[cfg(feature = "esplora")]
    fn esplora_indexer(url: &str) -> Indexer {
        let opts = EsploraBuilder::new(url).max_retries(0).timeout(1);
        Indexer::Esplora(Box::new(EsploraClient::from_builder(opts)))
    }

    #[cfg(feature = "bitcoind")]
    fn run_bitcoind_get_tx_confirmations_error_case(
        error_code: i32,
        error_message: &str,
    ) -> Result<Option<u64>, Error> {
        let mut server = mockito::Server::new();
        let error_message = error_message.to_string();
        let mock = server
            .mock("POST", "/")
            .with_header("content-type", "application/json")
            .with_body_from_request(move |request| {
                let req: serde_json::Value =
                    serde_json::from_slice(request.body().expect("mock bitcoind request body"))
                        .expect("parse mock bitcoind request");
                assert_eq!(req["method"], "getrawtransaction");
                format!(
                    r#"{{"jsonrpc":"2.0","id":{},"result":null,"error":{{"code":{error_code},"message":"{error_message}"}}}}"#,
                    req["id"]
                )
                .into()
            })
            .create();

        let result = bitcoind_indexer(&server.url()).get_tx_confirmations(TEST_TXID);
        mock.assert();
        result
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
    fn electrum_server_version_response(id: &serde_json::Value) -> String {
        format!(r#"{{"jsonrpc":"2.0","id":{id},"result":["mock-electrum","1.6"]}}"#)
    }

    #[cfg(feature = "electrum")]
    fn start_electrum_mock(
        handler: impl Fn(&str, &serde_json::Value) -> String + Send + 'static,
    ) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{BufRead, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock electrum");
        let port = listener.local_addr().expect("mock electrum addr").port();
        let url = format!("tcp://127.0.0.1:{port}");

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("mock electrum accept");
            let reader_stream = stream.try_clone().expect("clone mock electrum stream");
            let mut reader = std::io::BufReader::new(reader_stream);
            loop {
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
                let response = if method == "server.version" {
                    electrum_server_version_response(&id)
                } else {
                    handler(method, &req)
                };
                if response.is_empty() {
                    return;
                }
                let body = if response.starts_with('{') {
                    response
                } else {
                    format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{response}}}"#)
                };
                writeln!(stream, "{body}").expect("write mock electrum response");
                if method != "server.version" {
                    return;
                }
            }
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

    #[cfg(feature = "bitcoind")]
    #[test]
    fn test_bitcoind_get_tx_confirmations_not_found() {
        let res = run_bitcoind_get_tx_confirmations_error_case(
            BITCOIND_RPC_INVALID_ADDRESS_OR_KEY,
            "No such mempool or blockchain transaction",
        )
        .unwrap();
        assert_eq!(res, None);
    }

    #[cfg(feature = "bitcoind")]
    #[test]
    fn test_bitcoind_get_tx_confirmations_genesis_coinbase() {
        let res = run_bitcoind_get_tx_confirmations_error_case(
            BITCOIND_RPC_INVALID_ADDRESS_OR_KEY,
            "The genesis block coinbase is not considered an ordinary transaction",
        )
        .unwrap();
        assert_eq!(res, Some(u64::MAX));
    }

    #[cfg(feature = "bitcoind")]
    #[test]
    fn test_bitcoind_get_tx_confirmations_protocol_error() {
        let res =
            run_bitcoind_get_tx_confirmations_error_case(-32603, "internal error").unwrap_err();
        assert_matches!(res, Error::Indexer { .. });
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
