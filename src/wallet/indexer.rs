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
                    let height = client.get_height().map_err(IndexerError::from)?;
                    Some((height - tx_height + 1) as u64)
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
        #[cfg_attr(feature = "esplora", allow(unused))] bdk_wallet: &PersistedWallet<
            Store<ChangeSet>,
        >,
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
