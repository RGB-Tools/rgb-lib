//! RGB multisig wallet module.
//!
//! This module defines the methods of the [`MultisigWallet`] structure.

use super::*;

const HUB_OPS_DIR: &str = "hub_ops";

// BIP-341 NUMS H point (0250929b74...) with 32-zero chain code, formatted as a BIP-32 xpub.
// BIP-388 requires every key in a tr() policy to be a derivable @i/** reference, so a
// bare x-only hex constant is not allowed — a proper xpub with /** is mandatory.
const NUMS_TPUB_TESTNET: &str = "tpubD6NzVbkrYhZ4WLczPJWReQycCJdd6YVWXubbVUFnJ5KgU5MDQrD998ZJLSmaB7GVcCnJSDWprxmrGkJ6SvgQC6QAffVpqSvonXmeizXcrkN";
const NUMS_XPUB_MAINNET: &str = "xpub661MyMwAqRbcEYS8w7XLSVeEsBXy79zSzH1J8vCdxAZningWLdN3zgtU6QgnecKFpJFPpdzxKrwoaZoV44qAJewsc4kX9vGaCaBExuvJH57";

/// A cosigner for the multisig wallet setup.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Cosigner {
    /// Account-level xPub of the vanilla side of the wallet
    pub account_xpub_vanilla: String,
    /// Account-level xPub of the colored side of the wallet
    pub account_xpub_colored: String,
    /// Account index (default: 0)
    pub vanilla_keychain: Option<u8>,
    /// Master fingerprint
    pub master_fingerprint: String,
}

impl Cosigner {
    #[cfg(test)]
    pub(crate) fn from_keys(keys: &Keys, vanilla_keychain: Option<u8>) -> Self {
        Self {
            account_xpub_vanilla: keys.account_xpub_vanilla.clone(),
            account_xpub_colored: keys.account_xpub_colored.clone(),
            vanilla_keychain,
            master_fingerprint: keys.master_fingerprint.clone(),
        }
    }
}

impl fmt::Display for Cosigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}:{}",
            self.master_fingerprint,
            self.account_xpub_vanilla,
            self.account_xpub_colored,
            self.vanilla_keychain.unwrap_or(KEYCHAIN_BTC)
        )?;
        Ok(())
    }
}

impl FromStr for Cosigner {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 4 {
            return Err(Error::InvalidCosigner {
                details: format!("expected 3 or 4 colon-separated parts, got {}", parts.len()),
            });
        }
        let master_fingerprint = parts[0].to_string();
        Fingerprint::from_str(&master_fingerprint).map_err(|_| Error::InvalidCosigner {
            details: format!("invalid master_fingerprint '{}'", master_fingerprint),
        })?;
        let account_xpub_vanilla = parts[1].to_string();
        Xpub::from_str(&account_xpub_vanilla).map_err(|_| Error::InvalidCosigner {
            details: format!("invalid vanilla xpub '{account_xpub_vanilla}'"),
        })?;
        let account_xpub_colored = parts[2].to_string();
        Xpub::from_str(&account_xpub_colored).map_err(|_| Error::InvalidCosigner {
            details: format!("invalid colored xpub '{account_xpub_colored}'"),
        })?;
        let vanilla_keychain = parts[3].parse::<u8>().map_err(|_| Error::InvalidCosigner {
            details: format!("invalid vanilla_keychain value '{}'", parts[3]),
        })?;
        Ok(Cosigner {
            account_xpub_vanilla,
            account_xpub_colored,
            vanilla_keychain: Some(vanilla_keychain),
            master_fingerprint,
        })
    }
}

/// Keys for the multisig wallet.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct MultisigKeys {
    /// Cosigners of the multisig group
    pub cosigners: Vec<Cosigner>,
    /// Threshold for the colored funds
    pub threshold_colored: u8,
    /// Threshold for the vanilla funds
    pub threshold_vanilla: u8,
}

impl MultisigKeys {
    /// Create keys for a multisig wallet.
    pub fn new(cosigners: Vec<Cosigner>, threshold_colored: u8, threshold_vanilla: u8) -> Self {
        Self {
            cosigners,
            threshold_colored,
            threshold_vanilla,
        }
    }

    pub(crate) fn build_descriptors(
        &self,
        bitcoin_network: BitcoinNetwork,
    ) -> Result<WalletDescriptors, Error> {
        if self.cosigners.is_empty() {
            return Err(Error::NoCosignersSupplied);
        }
        let cosigners_len = self.cosigners.len();
        let total = u8::try_from(cosigners_len).map_err(|_| Error::TooManyCosigners)?;
        let check_threshold = |threshold: u8| -> Result<(), Error> {
            if threshold == 0 || threshold > total {
                return Err(Error::InvalidMultisigThreshold {
                    required: threshold,
                    total,
                });
            }
            Ok(())
        };
        check_threshold(self.threshold_colored)?;
        check_threshold(self.threshold_vanilla)?;

        let btc_coin = get_coin_type(&bitcoin_network, false);
        let rgb_coin = get_coin_type(&bitcoin_network, true);

        let key_for = |c: &Cosigner, coin: u32, rgb: bool| -> Result<String, Error> {
            Fingerprint::from_str(&c.master_fingerprint).map_err(|_| Error::InvalidCosigner {
                details: format!("invalid master_fingerprint '{}'", c.master_fingerprint),
            })?;
            let (xpub_str, xpub_type, keychain) = if rgb {
                (&c.account_xpub_colored, "colored", KEYCHAIN_RGB)
            } else {
                (
                    &c.account_xpub_vanilla,
                    "vanilla",
                    c.vanilla_keychain.unwrap_or(KEYCHAIN_BTC),
                )
            };
            let xpub = Xpub::from_str(xpub_str).map_err(|_| Error::InvalidCosigner {
                details: format!("invalid {xpub_type} xpub '{xpub_str}'"),
            })?;
            if xpub.network != bitcoin_network.into() {
                return Err(Error::InvalidCosigner {
                    details: format!("{xpub_type} xpub '{xpub_str}' is for the wrong network"),
                });
            }
            let origin = format!(
                "[{}/{}'/{}'/{}']",
                c.master_fingerprint, PURPOSE, coin, ACCOUNT
            );
            let path = format!("/{keychain}/*");
            Ok(format!("{origin}{xpub}{path}"))
        };

        let mut colored_keys = Vec::with_capacity(cosigners_len);
        let mut vanilla_keys = Vec::with_capacity(cosigners_len);
        for c in &self.cosigners {
            colored_keys.push(key_for(c, rgb_coin, true)?);
            vanilla_keys.push(key_for(c, btc_coin, false)?);
        }

        // deterministic key order so all parties build the same descriptor
        colored_keys.sort();
        vanilla_keys.sort();

        let nums_tpub = match bitcoin_network {
            BitcoinNetwork::Mainnet => NUMS_XPUB_MAINNET,
            _ => NUMS_TPUB_TESTNET,
        };

        // use /0/* (keychain 0) for the NUMS internal key to match the signer
        // keys and to avoid the /** multipath notation, which rust-miniscript
        // v12 does not accept as a raw descriptor string
        let tr_multi_a_desc = |threshold: u8, keys: &[String]| -> String {
            format!(
                "tr({}/0/*,multi_a({},{}))",
                nums_tpub,
                threshold,
                keys.join(",")
            )
        };
        let colored = tr_multi_a_desc(self.threshold_colored, &colored_keys);
        let vanilla = tr_multi_a_desc(self.threshold_vanilla, &vanilla_keys);

        Ok(WalletDescriptors { colored, vanilla })
    }
}

/// An RGB multisig wallet.
///
/// Can be obtained with the [`MultisigWallet::new`] method.
pub struct MultisigWallet {
    pub(crate) internals: WalletInternals,
    pub(crate) keys: MultisigKeys,
}

impl WalletCore for MultisigWallet {
    fn internals(&self) -> &WalletInternals {
        &self.internals
    }

    fn internals_mut(&mut self) -> &mut WalletInternals {
        &mut self.internals
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn sync_db_txos(&mut self, full_scan: bool, include_spent: bool) -> Result<(), Error> {
        // sync addresses
        let response = self.hub_client().get_current_address_indices()?;
        let (bdk_wallet, bdk_database) = self.bdk_wallet_db_mut();
        let mut persist = false;
        let mut reveal = |keychain_kind: KeychainKind, index: Option<u32>| {
            if let Some(hub_index) = index {
                let local_index = bdk_wallet
                    .derivation_index(keychain_kind)
                    .map(|i| i as i64)
                    .unwrap_or(-1);
                if local_index < hub_index as i64 {
                    for _ in local_index..hub_index as i64 {
                        bdk_wallet.reveal_next_address(keychain_kind);
                    }
                    persist = true;
                }
            }
        };
        reveal(KeychainKind::Internal, response.internal);
        reveal(KeychainKind::External, response.external);
        if persist {
            bdk_wallet.persist(bdk_database)?;
        }
        // sync UTXOs
        self.sync_db_txos_with_bdk(full_scan, include_spent)
    }
}

impl WalletBackup for MultisigWallet {}

impl WalletOffline for MultisigWallet {
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn get_new_addresses(
        &mut self,
        keychain: KeychainKind,
        count: u32,
    ) -> Result<BdkAddress, Error> {
        let is_internal = keychain == KeychainKind::Internal;
        let start_index = self.hub_client().bump_address_indices(count, is_internal)?;
        let local_index = self.bdk_wallet().derivation_index(keychain).unwrap_or(0);
        let target_index = start_index + count;
        let (bdk_wallet, bdk_database) = self.bdk_wallet_db_mut();
        for _ in local_index..target_index {
            bdk_wallet.reveal_next_address(keychain);
        }
        let first_address = bdk_wallet.peek_address(keychain, start_index).address;
        bdk_wallet.persist(bdk_database)?;
        Ok(first_address)
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl WalletOnline for MultisigWallet {
    fn wallet_specific_consistency_checks(&mut self) -> Result<(), Error> {
        Ok(())
    }

    fn list_internal_for_broadcast(&self) -> impl Iterator<Item = LocalOutput> + '_ {
        self.internal_outputs()
    }

    fn get_hub_fail_status(&self, batch_transfer_idx: i32) -> Result<bool, Error> {
        Ok(self
            .hub_client()
            .get_transfer_status(batch_transfer_idx)?
            .is_some_and(|s| !s.accepted))
    }

    fn set_hub_accept_status(&self, batch_transfer_idx: i32) -> Result<Option<bool>, Error> {
        if self.check_is_cosigner().is_ok() {
            match self
                .hub_client()
                .set_transfer_status(batch_transfer_idx, true)
            {
                Ok(_) => Ok(Some(true)),
                Err(Error::MultisigTransferStatusMismatch) => Ok(Some(false)),
                Err(e) => Err(e),
            }
        } else {
            // watch-only must follow the cosigners' decision if registered
            match self.hub_client().get_transfer_status(batch_transfer_idx)? {
                Some(s) => Ok(Some(s.accepted)),
                None => Ok(None),
            }
        }
    }

    fn set_hub_fail_status(&self, batch_transfer_idx: i32) -> Result<(), Error> {
        if self.check_is_cosigner().is_ok() {
            self.hub_client()
                .set_transfer_status(batch_transfer_idx, false)?;
        }
        Ok(())
    }
}

/// Common offline APIs of the wallet.
impl RgbWalletOpsOffline for MultisigWallet {}

/// Common online APIs of the wallet.
#[cfg(any(feature = "electrum", feature = "esplora"))]
impl RgbWalletOpsOnline for MultisigWallet {
    fn fail_transfers(
        &mut self,
        online: Online,
        batch_transfer_idx: Option<i32>,
        no_asset_only: bool,
        skip_sync: bool,
    ) -> Result<bool, Error> {
        self.check_is_cosigner()?;
        info!(
            self.logger(),
            "Failing batch transfer with idx {:?}...", batch_transfer_idx
        );
        self.check_online(online)?;
        let changed = self.fail_transfers_impl(batch_transfer_idx, no_asset_only, skip_sync)?;
        info!(self.logger(), "Fail transfers completed");
        Ok(changed)
    }
}

/// Voting status for multisig operations.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct MultisigVotingStatus {
    /// xPubs of cosigners that ACKed
    pub acked_by: HashSet<String>,
    /// xPubs of cosigners that NACKed
    pub nacked_by: HashSet<String>,
    /// Number of signatures needed
    pub threshold: u8,
    /// My response (true=ACK, false=NACK), if given
    pub my_response: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
struct ReceiveMetadata {
    invoice: String,
    min_confirmations: u8,
    expiration_timestamp: Option<i64>,
    secret_seal: Option<GraphSeal>,
}

/// Operations for multisig wallets.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub enum Operation {
    // CreateUtxos variants
    /// Create UTXOs operation waiting for user's response (ACK/NACK)
    CreateUtxosToReview {
        /// PSBT to sign
        psbt: String,
        /// Operation voting status
        status: MultisigVotingStatus,
    },
    /// Create UTXOs operation already responded to, waiting for threshold to be met
    CreateUtxosPending {
        /// Operation voting status
        status: MultisigVotingStatus,
    },
    /// Create UTXOs operation approved and finalized (threshold reached)
    CreateUtxosCompleted {
        /// Operation TXID
        txid: String,
        /// Operation voting status
        status: MultisigVotingStatus,
    },
    /// Create UTXOs operation rejected (NACKs exceeded threshold)
    CreateUtxosDiscarded {
        /// Operation voting status
        status: MultisigVotingStatus,
    },

    // SendBtc variants
    /// Send BTC operation waiting for user's response (ACK/NACK)
    SendBtcToReview {
        /// PSBT to sign
        psbt: String,
        /// Operation voting status
        status: MultisigVotingStatus,
    },
    /// Send BTC operation already responded to, waiting for threshold to be met
    SendBtcPending {
        /// Operation voting status
        status: MultisigVotingStatus,
    },
    /// Send BTC operation approved and finalized (threshold reached)
    SendBtcCompleted {
        /// Operation TXID
        txid: String,
        /// Operation voting status
        status: MultisigVotingStatus,
    },
    /// Send BTC operation rejected (NACKs exceeded threshold)
    SendBtcDiscarded {
        /// Operation voting status
        status: MultisigVotingStatus,
    },

    // Send variants
    /// Send operation waiting for user's response (ACK/NACK)
    SendToReview {
        /// PSBT to sign
        psbt: String,
        /// Operation details
        details: SendDetails,
        /// Operation voting status
        status: MultisigVotingStatus,
    },
    /// Send operation already responded to, waiting for threshold to be met
    SendPending {
        /// Operation details
        details: SendDetails,
        /// Operation voting status
        status: MultisigVotingStatus,
    },
    /// Send operation approved and finalized (threshold reached)
    SendCompleted {
        /// Operation TXID
        txid: String,
        /// Operation details
        details: SendDetails,
        /// Operation voting status
        status: MultisigVotingStatus,
    },
    /// Send operation rejected (NACKs exceeded threshold)
    SendDiscarded {
        /// Operation details
        details: SendDetails,
        /// Operation voting status
        status: MultisigVotingStatus,
    },

    // Inflation variants
    /// Inflation operation waiting for user's response (ACK/NACK)
    InflationToReview {
        /// PSBT to sign
        psbt: String,
        /// Operation details
        details: InflateDetails,
        /// Operation voting status
        status: MultisigVotingStatus,
    },
    /// Inflation operation already responded to, waiting for threshold to be met
    InflationPending {
        /// Operation details
        details: InflateDetails,
        /// Operation voting status
        status: MultisigVotingStatus,
    },
    /// Inflation operation approved and finalized (threshold reached)
    InflationCompleted {
        /// Operation TXID
        txid: String,
        /// Operation details
        details: InflateDetails,
        /// Operation voting status
        status: MultisigVotingStatus,
    },
    /// Inflation operation rejected (NACKs exceeded threshold)
    InflationDiscarded {
        /// Operation details
        details: InflateDetails,
        /// Operation voting status
        status: MultisigVotingStatus,
    },

    // Auto-approved operations
    /// Issuance operation completed (auto-approved)
    IssuanceCompleted {
        /// ID of the issued asset
        asset_id: String,
    },
    /// Blind receive operation completed (auto-approved)
    BlindReceiveCompleted {
        /// Operation details
        details: ReceiveData,
    },
    /// Witness receive operation completed (auto-approved)
    WitnessReceiveCompleted {
        /// Operation details
        details: ReceiveData,
    },
}

#[derive(Debug, Clone)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
struct FileResponse {
    r#type: FileType,
    filepath: PathBuf,
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl InfoBatchTransfer {
    fn extract_from_files(files: &[FileResponse]) -> Result<Self, Error> {
        let transfer_data_file = files
            .iter()
            .find(|f| matches!(f.r#type, FileType::OperationData))
            .ok_or(Error::MultisigUnexpectedData {
                details: s!("operation data not found"),
            })?;
        let file = fs::File::open(&transfer_data_file.filepath)?;
        let reader = io::BufReader::new(file);
        serde_json::from_reader(reader).map_err(|_| Error::MultisigUnexpectedData {
            details: s!("invalid operation data"),
        })
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
fn extract_fascia_path(files: &[FileResponse]) -> Result<String, Error> {
    files
        .iter()
        .find(|f| matches!(f.r#type, FileType::Fascia))
        .map(|f| f.filepath.to_string_lossy().to_string())
        .ok_or(Error::MultisigUnexpectedData {
            details: s!("cannot find fascia"),
        })
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
fn extract_fascia_from_files(files: &[FileResponse]) -> Result<Fascia, Error> {
    let fascia_file = files
        .iter()
        .find(|f| matches!(f.r#type, FileType::Fascia))
        .ok_or(Error::MultisigUnexpectedData {
            details: s!("fascia not found"),
        })?;
    let file = fs::File::open(&fascia_file.filepath)?;
    let reader = io::BufReader::new(file);
    serde_json::from_reader(reader).map_err(|_| Error::MultisigUnexpectedData {
        details: s!("invalid fascia"),
    })
}

#[derive(Debug, Clone)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
struct NoDetails;

#[cfg(any(feature = "electrum", feature = "esplora"))]
trait OperationHandler {
    type Details: Clone;

    fn extract_details(files: &[FileResponse]) -> Result<Self::Details, Error>;

    fn to_review(psbt: String, details: Self::Details, status: MultisigVotingStatus) -> Operation;

    fn pending(details: Self::Details, status: MultisigVotingStatus) -> Operation;

    fn completed(txid: String, details: Self::Details, status: MultisigVotingStatus) -> Operation;

    fn discarded(details: Self::Details, status: MultisigVotingStatus) -> Operation;

    fn finalize_and_execute(
        wallet: &mut MultisigWallet,
        combined_psbt: &Psbt,
    ) -> Result<String, Error>;

    fn reconstruct_transfer_directory(
        _wallet: &MultisigWallet,
        _txid: &str,
        _files: &[FileResponse],
    ) -> Result<(), Error> {
        Ok(())
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
struct CreateUtxosHandler;

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl OperationHandler for CreateUtxosHandler {
    type Details = NoDetails;

    fn extract_details(_files: &[FileResponse]) -> Result<Self::Details, Error> {
        Ok(NoDetails)
    }

    fn to_review(psbt: String, _details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::CreateUtxosToReview { psbt, status }
    }

    fn pending(_details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::CreateUtxosPending { status }
    }

    fn completed(txid: String, _details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::CreateUtxosCompleted { txid, status }
    }

    fn discarded(_details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::CreateUtxosDiscarded { status }
    }

    fn finalize_and_execute(
        wallet: &mut MultisigWallet,
        combined_psbt: &Psbt,
    ) -> Result<String, Error> {
        wallet.create_utxos_end_impl(combined_psbt, false)?;
        Ok(combined_psbt.unsigned_tx.compute_txid().to_string())
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
struct SendBtcHandler;

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl OperationHandler for SendBtcHandler {
    type Details = NoDetails;

    fn extract_details(_files: &[FileResponse]) -> Result<Self::Details, Error> {
        Ok(NoDetails)
    }

    fn to_review(psbt: String, _details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::SendBtcToReview { psbt, status }
    }

    fn pending(_details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::SendBtcPending { status }
    }

    fn completed(txid: String, _details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::SendBtcCompleted { txid, status }
    }

    fn discarded(_details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::SendBtcDiscarded { status }
    }

    fn finalize_and_execute(
        wallet: &mut MultisigWallet,
        combined_psbt: &Psbt,
    ) -> Result<String, Error> {
        wallet.send_btc_end_impl(combined_psbt, false)?;
        Ok(combined_psbt.unsigned_tx.compute_txid().to_string())
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
struct SendRgbHandler;

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl OperationHandler for SendRgbHandler {
    type Details = SendDetails;

    fn extract_details(files: &[FileResponse]) -> Result<Self::Details, Error> {
        let fascia_path = extract_fascia_path(files)?;
        let info_batch_transfer = InfoBatchTransfer::extract_from_files(files)?;
        Ok(SendDetails {
            fascia_path,
            min_confirmations: info_batch_transfer.min_confirmations,
            entropy: info_batch_transfer.entropy,
            is_donation: info_batch_transfer.donation,
        })
    }

    fn to_review(psbt: String, details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::SendToReview {
            psbt,
            details,
            status,
        }
    }

    fn pending(details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::SendPending { details, status }
    }

    fn completed(txid: String, details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::SendCompleted {
            txid,
            details,
            status,
        }
    }

    fn discarded(details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::SendDiscarded { details, status }
    }

    fn finalize_and_execute(
        wallet: &mut MultisigWallet,
        combined_psbt: &Psbt,
    ) -> Result<String, Error> {
        let res = wallet.send_end_impl(combined_psbt, false)?;
        Ok(res.txid)
    }

    fn reconstruct_transfer_directory(
        wallet: &MultisigWallet,
        txid: &str,
        files: &[FileResponse],
    ) -> Result<(), Error> {
        wallet.reconstruct_rgb_transfer_directory(txid, files)
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
struct InflateHandler;

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl OperationHandler for InflateHandler {
    type Details = InflateDetails;

    fn extract_details(files: &[FileResponse]) -> Result<Self::Details, Error> {
        let fascia_path = extract_fascia_path(files)?;
        let info_batch_transfer = InfoBatchTransfer::extract_from_files(files)?;
        if info_batch_transfer.transfers.len() != 1 {
            return Err(Error::MultisigUnexpectedData {
                details: format!(
                    "expected 1 transfer for inflation, got {} transfers",
                    info_batch_transfer.transfers.len()
                ),
            });
        }
        Ok(InflateDetails {
            fascia_path,
            min_confirmations: info_batch_transfer.min_confirmations,
            entropy: info_batch_transfer.entropy,
        })
    }

    fn to_review(psbt: String, details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::InflationToReview {
            psbt,
            details,
            status,
        }
    }

    fn pending(details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::InflationPending { details, status }
    }

    fn completed(txid: String, details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::InflationCompleted {
            txid,
            details,
            status,
        }
    }

    fn discarded(details: Self::Details, status: MultisigVotingStatus) -> Operation {
        Operation::InflationDiscarded { details, status }
    }

    fn finalize_and_execute(
        wallet: &mut MultisigWallet,
        combined_psbt: &Psbt,
    ) -> Result<String, Error> {
        let res = wallet.inflate_end_impl(combined_psbt)?;
        Ok(res.txid)
    }

    fn reconstruct_transfer_directory(
        wallet: &MultisigWallet,
        txid: &str,
        files: &[FileResponse],
    ) -> Result<(), Error> {
        wallet.reconstruct_rgb_transfer_directory(txid, files)
    }
}

/// Information about an operation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct OperationInfo {
    /// Index of the operation
    pub operation_idx: i32,
    /// xPub of the initiator of the operation
    pub initiator_xpub: String,
    /// Operation details
    pub operation: Operation,
}

/// Response to an operation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub enum RespondToOperation {
    /// ACK the operation with a signed PSBT
    Ack(String),
    /// NACK the operation
    Nack,
}

/// Result of an operation initialization.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct InitOperationResult {
    /// PSBT of the operation
    pub psbt: String,
    /// Index of the operation on the hub
    pub operation_idx: i32,
}

/// The role of the user on the hub.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub enum UserRole {
    /// A cosigner
    Cosigner,
    /// A watch-only user
    WatchOnly,
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl From<UserRoleResponse> for UserRole {
    fn from(orig: UserRoleResponse) -> Self {
        match orig {
            UserRoleResponse::Cosigner(_) => UserRole::Cosigner,
            UserRoleResponse::WatchOnly => UserRole::WatchOnly,
        }
    }
}

/// Information about the hub.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct HubInfo {
    /// The minimum supported rgb-lib version
    pub min_rgb_lib_version: String,
    /// The maximum supported rgb-lib version
    pub max_rgb_lib_version: String,
    /// The current rgb-lib version set on the hub
    pub rgb_lib_version: String,
    /// The last operation index on the hub
    pub last_operation_idx: Option<i32>,
    /// The role of the user on the hub
    pub user_role: UserRole,
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
impl From<InfoResponse> for HubInfo {
    fn from(orig: InfoResponse) -> Self {
        Self {
            min_rgb_lib_version: orig.min_rgb_lib_version,
            max_rgb_lib_version: orig.max_rgb_lib_version,
            rgb_lib_version: orig.rgb_lib_version,
            last_operation_idx: orig.last_operation_idx,
            user_role: orig.user_role.into(),
        }
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
enum PostData {
    BeginOperationData(Box<BeginOperationData>),
    Psbt(Psbt),
}

/// Offline APIs of the wallet
impl MultisigWallet {
    /// Create a new RGB multisig wallet based on the provided [`WalletData`] and
    /// [`MultisigKeys`].
    pub fn new(wallet_data: WalletData, keys: MultisigKeys) -> Result<Self, Error> {
        let wdata = wallet_data.clone();
        let bdk_network = BdkNetwork::from(wdata.bitcoin_network);

        // wallet keys
        let descs = keys.build_descriptors(wdata.bitcoin_network)?;

        // wallet directory and file logging setup
        let fingerprint = hash_bytes_hex(format!("{}|{}", descs.colored, descs.vanilla).as_bytes())
            [..8]
            .to_string();
        let (wallet_dir, logger, _logger_guard) = setup_new_wallet(&wallet_data, &fingerprint)?;
        fs::create_dir_all(wallet_dir.join(HUB_OPS_DIR))?;

        // setup the BDK wallet
        let (bdk_wallet, bdk_database) = setup_bdk(
            &wdata,
            &wallet_dir,
            descs.colored,
            descs.vanilla,
            true,
            bdk_network,
        )?;

        // setup RGB
        setup_rgb(&wallet_dir, wdata.supported_schemas, wdata.bitcoin_network)?;

        // setup rgb-lib DB
        let database = setup_db(&wallet_dir)?;

        info!(logger, "New multisig wallet completed");
        Ok(Self {
            internals: WalletInternals {
                wallet_data,
                logger,
                _logger_guard,
                database: Arc::new(database),
                wallet_dir,
                bdk_wallet,
                bdk_database,
                #[cfg(any(feature = "electrum", feature = "esplora"))]
                online_data: None,
            },
            keys,
        })
    }

    /// Return the bitcoin keys of the wallet.
    pub fn get_keys(&self) -> MultisigKeys {
        self.keys.clone()
    }

    /// Return the descriptors of the wallet.
    pub fn get_descriptors(&self) -> WalletDescriptors {
        self.keys
            .build_descriptors(self.internals.wallet_data.bitcoin_network)
            .expect("already succeeded at wallet creation")
    }

    /// Get the last hub processed operation index that the wallet has stored in the database.
    pub fn get_local_last_processed_operation_idx(&self) -> Result<i32, Error> {
        info!(
            self.logger(),
            "Getting local last processed operation IDX..."
        );
        let idx = self
            .database()
            .get_backup_info()?
            .and_then(|b| b.last_processed_operation_idx)
            .unwrap_or(0);
        info!(
            self.logger(),
            "Get local last processed operation IDX completed"
        );
        Ok(idx)
    }
}

/// Online APIs of the wallet
#[cfg(any(feature = "electrum", feature = "esplora"))]
impl MultisigWallet {
    fn is_cosigner(&self) -> Result<bool, Error> {
        Ok(matches!(
            self.online_data()
                .as_ref()
                .unwrap()
                .user_role
                .as_ref()
                .expect("always set"),
            UserRole::Cosigner
        ))
    }

    fn check_is_cosigner(&self) -> Result<(), Error> {
        if !self.is_cosigner()? {
            return Err(Error::MultisigUserNotCosigner);
        }
        Ok(())
    }

    fn hub_client(&self) -> &MultisigHubClient {
        self.online_data()
            .as_ref()
            .unwrap()
            .hub_client
            .as_ref()
            .unwrap()
    }

    fn get_hub_ops_dir(&self) -> PathBuf {
        self.get_wallet_dir().join(HUB_OPS_DIR)
    }

    fn get_cached_file_path(&self, file_id: &str) -> PathBuf {
        self.get_hub_ops_dir().join(file_id)
    }

    fn get_or_download_file(&self, file_metadata: &FileMetadata) -> Result<PathBuf, Error> {
        let filepath = self.get_cached_file_path(&file_metadata.file_id);
        if filepath.exists() {
            return Ok(filepath);
        }
        if matches!(file_metadata.r#type, FileType::Media) {
            let media_path = self.get_media_dir().join(&file_metadata.file_id);
            if media_path.exists() {
                return Ok(media_path);
            }
        }
        self.hub_client()
            .get_file(&file_metadata.file_id, &filepath)?;
        Ok(filepath)
    }

    fn get_or_download_files(
        &self,
        file_metadata: Vec<FileMetadata>,
    ) -> Result<Vec<FileResponse>, Error> {
        let mut files = Vec::new();
        for metadata in file_metadata {
            let filepath = self.get_or_download_file(&metadata)?;
            files.push(FileResponse {
                r#type: metadata.r#type,
                filepath,
            });
        }
        Ok(files)
    }

    fn reconstruct_rgb_transfer_directory(
        &self,
        txid: &str,
        files: &[FileResponse],
    ) -> Result<(), Error> {
        let transfer_dir = self.get_transfer_dir(txid);
        fs::create_dir_all(&transfer_dir)?;
        let batch_transfer = InfoBatchTransfer::extract_from_files(files)?;
        let batch_data_str = serde_json::to_string(&batch_transfer).expect("serializable");
        fs::write(transfer_dir.join(TRANSFER_DATA_FILE), batch_data_str)?;
        let fascia = extract_fascia_from_files(files)?;
        let fascia_str = serde_json::to_string(&fascia).expect("serializable");
        let fascia_path = transfer_dir.join(FASCIA_FILE);
        fs::write(fascia_path, fascia_str)?;
        Ok(())
    }

    /// Return a new Bitcoin address from the vanilla wallet.
    ///
    /// This method generates a new address using the index atomically retrieved from the hub.
    /// This ensures all cosigners maintain consistent address derivation indices.
    pub fn get_address(&mut self, online: Online) -> Result<String, Error> {
        info!(self.logger(), "Getting address...");
        self.check_online(online)?;
        self.check_is_cosigner()?;
        let address = self.get_new_addresses(KeychainKind::Internal, 1)?;
        self.update_backup_info(false)?;
        info!(self.logger(), "Get address completed");
        Ok(address.to_string())
    }

    /// Return the existing or freshly generated wallet [`Online`] data.
    ///
    /// Setting `skip_consistency_check` to false runs a check on assets (RGB vs rgb-lib DB) and
    /// medias (DB vs actual files) to try and detect possible inconsistencies in the wallet.
    /// Setting `skip_consistency_check` to true bypasses the check and allows operating an
    /// inconsistent wallet.
    ///
    /// <div class="warning">Warning: setting <tt>skip_consistency_check</tt> to true is dangerous,
    /// only do this if you know what you're doing!</div>
    pub fn go_online(
        &mut self,
        skip_consistency_check: bool,
        indexer_url: String,
        hub_url: String,
        hub_token: String,
    ) -> Result<Online, Error> {
        info!(self.logger(), "Going online...");
        // check hub URL validity
        let valid_url = match Url::parse(&hub_url) {
            Ok(url) => matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
            Err(_) => false,
        };
        if !valid_url {
            return Err(Error::MultisigHubService {
                details: s!("URL must be valid and start with http:// or https://"),
            });
        }

        // check hub connectivity and configuration
        let hub_client = MultisigHubClient::new(&hub_url, &hub_token)?;
        let info = hub_client.info()?;
        const RGB_LIB_VERSION: &str = env!("CARGO_PKG_VERSION");
        let local_version = RGB_LIB_VERSION
            .split('.')
            .take(2)
            .collect::<Vec<_>>()
            .join(".");
        #[cfg(test)]
        let local_version = mock_local_version(&local_version);
        if local_version != info.rgb_lib_version {
            return Err(Error::MultisigHubService {
                details: format!(
                    "rgb-lib version mismatch: local version is {} but hub requires {}",
                    local_version, info.rgb_lib_version
                ),
            });
        }

        // shared go online logic
        let online = self.go_online_impl(skip_consistency_check, &indexer_url)?;

        // set multisig-specific OnlineData fields
        self.online_data_mut().as_mut().unwrap().hub_client = Some(hub_client);
        self.online_data_mut().as_mut().unwrap().user_role = Some(info.user_role.into());

        info!(self.logger(), "Go online completed");
        Ok(online)
    }

    /// Get information about the hub.
    pub fn hub_info(&self, online: Online) -> Result<HubInfo, Error> {
        info!(self.logger(), "Hub info...");
        self.check_online(online)?;
        let info = self.hub_client().info()?.into();
        info!(self.logger(), "Hub info completed");
        Ok(info)
    }

    fn mark_operation_as_processed(&self, operation_idx: i32) -> Result<(), Error> {
        if self.is_cosigner()?
            && let Err(e) = self.hub_client().mark_operation_processed(operation_idx)
        {
            // ignore to enable multiple instances and restore from old backups
            if !matches!(&e, Error::MultisigCannotMarkOperationProcessed { details: d }
                if d == "Cannot mark operation as processed: already marked this operation as processed")
            {
                return Err(e);
            }
        }
        self.update_backup_info_with_op_idx(false, Some(operation_idx))?;
        Ok(())
    }

    fn upload_and_process_issuance<T: IssuedAssetDetails>(
        &self,
        issue_data: &IssueData,
        mut additional_files: Vec<(FileType, FileSource)>,
    ) -> Result<T, Error> {
        let mut files = vec![(
            FileType::Consignment,
            FileSource::Path(issue_data.contract_path.clone()),
        )];
        files.append(&mut additional_files);
        let response = self
            .hub_client()
            .post_operation(files, OperationType::Issuance)?;
        let mut runtime = self.rgb_runtime()?;
        let asset = self.import_and_save_contract(issue_data, &mut runtime)?;
        self.mark_operation_as_processed(response.operation_idx)?;
        T::from_issuance(self, &asset, issue_data)
    }

    /// Issue a new RGB NIA asset with the provided `ticker`, `name`, `precision` and `amounts`,
    /// post the issuance to the hub and return the asset.
    ///
    /// At least 1 amount needs to be provided and the sum of all amounts cannot exceed the maximum
    /// `u64` value.
    ///
    /// If `amounts` contains more than 1 element, each one will be issued as a separate allocation
    /// for the same asset (on a separate UTXO that needs to be already available).
    pub fn issue_asset_nia(
        &self,
        online: Online,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
    ) -> Result<AssetNIA, Error> {
        self.check_online(online)?;
        self.check_is_cosigner()?;
        self.issue_asset_nia_with_impl(ticker, name, precision, amounts, |issue_data| {
            self.upload_and_process_issuance(&issue_data, vec![])
        })
    }

    /// Issue a new RGB UDA asset with the provided `ticker`, `name`, optional `details` and
    /// `precision`, post the issuance to the hub and return the asset.
    ///
    /// An optional `media_file_path` containing the path to a media file can be provided. Its hash
    /// and mime type will be encoded in the contract.
    ///
    /// An optional `attachments_file_paths` containing paths to extra media files can be provided.
    /// Their hash and mime type will be encoded in the contract.
    pub fn issue_asset_uda(
        &self,
        online: Online,
        ticker: String,
        name: String,
        details: Option<String>,
        precision: u8,
        media_file_path: Option<String>,
        attachments_file_paths: Vec<String>,
    ) -> Result<AssetUDA, Error> {
        self.check_online(online)?;
        self.check_is_cosigner()?;
        self.issue_asset_uda_with_impl(
            ticker,
            name,
            details,
            precision,
            media_file_path,
            attachments_file_paths,
            |issue_data| {
                let mut files = vec![];
                if let Some(media) = &issue_data.asset_data.token.as_ref().unwrap().media {
                    files.push((
                        FileType::Media,
                        FileSource::Path(media.file_path.clone().into()),
                    ))
                }
                for media in issue_data
                    .asset_data
                    .token
                    .as_ref()
                    .unwrap()
                    .attachments
                    .values()
                {
                    files.push((
                        FileType::Media,
                        FileSource::Path(media.file_path.clone().into()),
                    ))
                }
                self.upload_and_process_issuance(&issue_data, files)
            },
        )
    }

    /// Issue a new RGB CFA asset with the provided `name`, optional `details`, `precision` and
    /// `amounts`, post the issuance to the hub and return the asset.
    ///
    /// An optional `file_path` containing the path to a media file can be provided. Its hash and
    /// mime type will be encoded in the contract.
    ///
    /// At least 1 amount needs to be provided and the sum of all amounts cannot exceed the maximum
    /// `u64` value.
    ///
    /// If `amounts` contains more than 1 element, each one will be issued as a separate allocation
    /// for the same asset (on a separate UTXO that needs to be already available).
    pub fn issue_asset_cfa(
        &self,
        online: Online,
        name: String,
        details: Option<String>,
        precision: u8,
        amounts: Vec<u64>,
        file_path: Option<String>,
    ) -> Result<AssetCFA, Error> {
        self.check_online(online)?;
        self.check_is_cosigner()?;
        self.issue_asset_cfa_with_impl(name, details, precision, amounts, file_path, |issue_data| {
            let mut files = vec![];
            if let Some(media) = &issue_data.asset_data.media {
                files.push((
                    FileType::Media,
                    FileSource::Path(media.file_path.clone().into()),
                ))
            }
            self.upload_and_process_issuance(&issue_data, files)
        })
    }

    /// Issue a new RGB IFA asset with the provided `ticker`, `name`, `precision`, `amounts` and
    /// `inflation_amounts`, post the issuance to the hub and return the asset.
    ///
    /// At least 1 amount needs to be provided and the sum of all amounts cannot exceed the maximum
    /// `u64` value.
    ///
    /// If `amounts` contains more than 1 element, each one will be issued as a separate allocation
    /// for the same asset (on a separate UTXO that needs to be already available).
    ///
    /// The `inflation_amounts` can be empty. If provided the sum of its elements plus the sum of
    /// `amounts` cannot exceed the maximum `u64` value.
    pub fn issue_asset_ifa(
        &self,
        online: Online,
        ticker: String,
        name: String,
        precision: u8,
        amounts: Vec<u64>,
        inflation_amounts: Vec<u64>,
        reject_list_url: Option<String>,
    ) -> Result<AssetIFA, Error> {
        self.check_online(online)?;
        self.check_is_cosigner()?;
        self.issue_asset_ifa_with_impl(
            ticker,
            name,
            precision,
            amounts,
            inflation_amounts,
            reject_list_url,
            |issue_data| self.upload_and_process_issuance(&issue_data, vec![]),
        )
    }

    fn receive_impl(
        &mut self,
        asset_id: Option<String>,
        assignment: Assignment,
        expiration_timestamp: Option<u64>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
        recipient_type: RecipientType,
        operation_type: OperationType,
    ) -> Result<ReceiveData, Error> {
        // shared receive data creation logic
        let receive_data_internal = self.create_receive_data(
            asset_id,
            assignment,
            expiration_timestamp.map(|t| t as i64),
            transport_endpoints,
            recipient_type,
        )?;

        // post operation and metadata to hub
        let expiration_timestamp = receive_data_internal.expiration_timestamp;
        let receive_metadata = ReceiveMetadata {
            invoice: receive_data_internal.invoice_string.clone(),
            min_confirmations,
            expiration_timestamp,
            secret_seal: receive_data_internal.blind_seal,
        };
        let metadata_json = serde_json::to_vec(&receive_metadata).expect("serializable");
        let files = vec![(FileType::OperationData, FileSource::Bytes(metadata_json))];
        let response = self.hub_client().post_operation(files, operation_type)?;

        // store transfer
        let batch_transfer_idx =
            self.store_receive_transfer(&receive_data_internal, min_confirmations)?;

        self.update_backup_info(false)?;

        self.mark_operation_as_processed(response.operation_idx)?;

        Ok(ReceiveData {
            invoice: receive_data_internal.invoice_string,
            recipient_id: receive_data_internal.recipient_id,
            expiration_timestamp: expiration_timestamp.map(|t| t as u64),
            batch_transfer_idx,
        })
    }

    /// Blind an UTXO to receive RGB assets, post the operation to the hub and return the
    /// resulting [`ReceiveData`].
    ///
    /// An optional asset ID can be specified, which will be embedded in the invoice, resulting in
    /// the refusal of the transfer is the asset doesn't match.
    ///
    /// An optional amount can be specified, which will be embedded in the invoice. It will not be
    /// checked when accepting the transfer.
    ///
    /// An optional expiration UTC timestamp can be specified, which will set the expiration of the
    /// invoice and the transfer.
    ///
    /// Each endpoint in the provided `transport_endpoints` list will be used as RGB data exchange
    /// medium. The list needs to contain at least 1 endpoint and a maximum of 3. Strings
    /// specifying invalid endpoints and duplicate ones will cause an error to be raised. A valid
    /// endpoint string encodes an
    /// [`RgbTransport`](https://docs.rs/rgb-invoicing/latest/rgbinvoice/enum.RgbTransport.html).
    /// At the moment the only supported variant is JsonRpc (e.g. `rpc://127.0.0.1` or
    /// `rpcs://example.com`).
    ///
    /// The `min_confirmations` number determines the minimum number of confirmations needed for
    /// the transaction anchoring the transfer for it to be considered final and move (while
    /// refreshing) to the [`TransferStatus::Settled`] status.
    pub fn blind_receive(
        &mut self,
        online: Online,
        asset_id: Option<String>,
        assignment: Assignment,
        expiration_timestamp: Option<u64>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
    ) -> Result<ReceiveData, Error> {
        info!(
            self.logger(),
            "Receiving via blinded UTXO for asset '{:?}' with expiration '{:?}'...",
            asset_id,
            expiration_timestamp,
        );
        self.check_online(online)?;
        self.check_is_cosigner()?;
        let receive_data = self.receive_impl(
            asset_id,
            assignment,
            expiration_timestamp,
            transport_endpoints,
            min_confirmations,
            RecipientType::Blind,
            OperationType::BlindReceive,
        )?;
        info!(self.logger(), "Blind receive completed");
        Ok(receive_data)
    }

    /// Create an address to receive RGB assets, post the operation to the hub and return the
    /// resulting [`ReceiveData`].
    ///
    /// An optional asset ID can be specified, which will be embedded in the invoice, resulting in
    /// the refusal of the transfer is the asset doesn't match.
    ///
    /// An optional amount can be specified, which will be embedded in the invoice. It will not be
    /// checked when accepting the transfer.
    ///
    /// An optional expiration UTC timestamp can be specified, which will set the expiration of the
    /// invoice and the transfer.
    ///
    /// Each endpoint in the provided `transport_endpoints` list will be used as RGB data exchange
    /// medium. The list needs to contain at least 1 endpoint and a maximum of 3. Strings
    /// specifying invalid endpoints and duplicate ones will cause an error to be raised. A valid
    /// endpoint string encodes an
    /// [`RgbTransport`](https://docs.rs/rgb-invoicing/latest/rgbinvoice/enum.RgbTransport.html).
    /// At the moment the only supported variant is JsonRpc (e.g. `rpc://127.0.0.1` or
    /// `rpcs://example.com`).
    ///
    /// The `min_confirmations` number determines the minimum number of confirmations needed for
    /// the transaction anchoring the transfer for it to be considered final and move (while
    /// refreshing) to the [`TransferStatus::Settled`] status.
    pub fn witness_receive(
        &mut self,
        online: Online,
        asset_id: Option<String>,
        assignment: Assignment,
        expiration_timestamp: Option<u64>,
        transport_endpoints: Vec<String>,
        min_confirmations: u8,
    ) -> Result<ReceiveData, Error> {
        info!(
            self.logger(),
            "Receiving via witness TX for asset '{:?}' with expiration '{:?}'...",
            asset_id,
            expiration_timestamp,
        );
        self.check_online(online)?;
        self.check_is_cosigner()?;
        let receive_data = self.receive_impl(
            asset_id,
            assignment,
            expiration_timestamp,
            transport_endpoints,
            min_confirmations,
            RecipientType::Witness,
            OperationType::WitnessReceive,
        )?;
        info!(self.logger(), "Witness receive completed");
        Ok(receive_data)
    }

    fn accept_issuance_consignment(&mut self, files: &[FileResponse]) -> Result<String, Error> {
        // get and validate contract
        let consignment_file = files
            .iter()
            .find(|f| matches!(f.r#type, FileType::Consignment))
            .ok_or(Error::MultisigUnexpectedData {
                details: s!("issuance consignment not found"),
            })?;
        let contract =
            Contract::load_file(&consignment_file.filepath).map_err(InternalError::from)?;
        let asset_schema: AssetSchema = contract.schema_id().try_into()?;
        let validation_config = ValidationConfig {
            chain_net: self.chain_net(),
            trusted_typesystem: asset_schema.types(),
            ..Default::default()
        };
        let valid_contract = contract
            .clone()
            .validate(&DumbResolver, &validation_config)
            .unwrap();

        // import and save contract
        let mut runtime = self.rgb_runtime()?;
        runtime
            .import_contract(valid_contract.clone(), &DumbResolver)
            .expect("importing issued contract should work");
        let contract_id = valid_contract.contract_id();
        let asset_id = contract_id.to_string();
        let contract_path = self.get_issue_consignment_path(&asset_id);
        valid_contract.save_file(&contract_path)?;

        // handle media files, if any
        let media_files = files
            .iter()
            .filter(|f| matches!(f.r#type, FileType::Media))
            .collect::<Vec<_>>();
        let attachments = self.extract_attachments(&valid_contract, asset_schema);
        if media_files.len() != attachments.len() {
            return Err(Error::MultisigUnexpectedData {
                details: format!(
                    "expected {} media files, got {}",
                    attachments.len(),
                    media_files.len()
                ),
            });
        }
        let attachment_digests = attachments
            .iter()
            .map(|a| hex::encode(a.digest))
            .collect::<Vec<_>>();
        let media_dir = self.get_media_dir();
        for media_file in media_files {
            let media_digest = hash_file(&media_file.filepath)?;
            if !attachment_digests.contains(&media_digest) {
                return Err(Error::MultisigUnexpectedData {
                    details: s!("media digest mismatch"),
                });
            }
            let media_path = media_dir.join(&media_digest);
            if !media_path.exists() {
                fs::rename(&media_file.filepath, media_path)?;
            }
        }

        let asset_data = self.extract_asset_data(
            &runtime,
            contract_id,
            asset_schema,
            valid_contract.clone(),
            None,
        )?;

        let mut issue_utxos: HashMap<i32, Vec<Assignment>> = HashMap::new();
        for a in valid_contract
            .contract_data()
            .allocations(&FilterIncludeAll)
        {
            let outpoint = a.seal.to_outpoint().into();
            let txo = match self.database().get_txo(&outpoint)? {
                Some(txo) => txo,
                None => {
                    self.sync_db_txos(false, true)?;
                    self.database().get_txo(&outpoint)?.expect("should exist")
                }
            };
            issue_utxos
                .entry(txo.idx)
                .or_default()
                .push(Assignment::from_opout_and_state(a.opout, &a.state));
        }

        let issue_data = IssueData {
            asset_data,
            valid_contract,
            contract_path,
            issue_utxos,
        };

        self.import_and_save_contract(&issue_data, &mut runtime)?;

        self.update_backup_info(false)?;

        Ok(asset_id)
    }

    fn import_receive_data(
        &mut self,
        files: &[FileResponse],
        operation_type: &OperationType,
    ) -> Result<ReceiveData, Error> {
        // get and parse receive data
        let metadata_file = files
            .iter()
            .find(|f| matches!(f.r#type, FileType::OperationData))
            .ok_or(Error::MultisigUnexpectedData {
                details: s!("receive data not found"),
            })?;
        let file = fs::File::open(&metadata_file.filepath)?;
        let reader = io::BufReader::new(file);
        let receive_metadata: ReceiveMetadata =
            serde_json::from_reader(reader).map_err(|_| Error::MultisigUnexpectedData {
                details: s!("invalid receive data"),
            })?;
        let min_confirmations = receive_metadata.min_confirmations;
        let invoice = Invoice::new(receive_metadata.invoice.clone())?;
        let invoice_data = invoice.invoice_data();
        let recipient_id = invoice_data.recipient_id.clone();
        let endpoints = self.convert_transport_endpoints(&invoice_data.transport_endpoints)?;

        // parse data based on operation type
        let (blind_seal, recipient_type_full, script_pubkey) = match operation_type {
            OperationType::BlindReceive => {
                let blind_seal =
                    receive_metadata
                        .secret_seal
                        .ok_or(Error::MultisigUnexpectedData {
                            details: s!("secret seal not found"),
                        })?;
                let unblinded_utxo = Outpoint {
                    txid: blind_seal.txid.to_string(),
                    vout: blind_seal.vout.into_u32(),
                };
                (
                    Some(blind_seal),
                    RecipientTypeFull::Blind { unblinded_utxo },
                    None,
                )
            }
            OperationType::WitnessReceive => (
                None,
                RecipientTypeFull::Witness { vout: None },
                Some(script_buf_from_recipient_id(invoice_data.recipient_id.clone())?.unwrap()),
            ),
            _ => unreachable!("only receive operations"),
        };

        // store transfer
        let receive_data_internal = ReceiveDataInternal {
            asset_id: invoice_data.asset_id.clone(),
            detected_assignment: invoice_data.assignment.clone(),
            invoice_string: receive_metadata.invoice.clone(),
            recipient_id: invoice_data.recipient_id.clone(),
            endpoints,
            created_at: now().unix_timestamp(),
            expiration_timestamp: invoice_data.expiration_timestamp.map(|t| t as i64),
            blind_seal,
            recipient_type_full,
            script_pubkey,
        };
        let batch_transfer_idx =
            self.store_receive_transfer(&receive_data_internal, min_confirmations)?;

        Ok(ReceiveData {
            invoice: receive_metadata.invoice,
            recipient_id,
            expiration_timestamp: receive_data_internal.expiration_timestamp.map(|t| t as u64),
            batch_transfer_idx,
        })
    }

    /// Sync the wallet with the hub.
    ///
    /// Try to fetch the next operation the cosigner has not processed yet (based on
    /// local index) from the hub.
    /// If the operation is found, it is processed and the operation info is returned.
    /// If the operation isn't found (i.e. the cosigner is already in sync), None is returned.
    pub fn sync_with_hub(&mut self, online: Online) -> Result<Option<OperationInfo>, Error> {
        info!(self.logger(), "Syncing with hub...");
        self.check_online(online)?;

        // make sure the wallet is synced and transfers are up-to-date
        self.sync_db_txos(false, false)?;
        self.refresh_impl(None, vec![], true)?;
        self.refresh_impl(None, vec![], true)?;

        let op_idx = self.get_local_last_processed_operation_idx()?;
        let Some(op) = self.hub_client().get_operation_by_idx(op_idx + 1)? else {
            return Ok(None);
        };

        let operation = self.process_operation(&op)?;

        // refresh when needed
        let needs_refresh = op.status == OperationStatus::Approved
            && matches!(
                op.operation_type,
                OperationType::SendRgb
                    | OperationType::Inflation
                    | OperationType::WitnessReceive
                    | OperationType::BlindReceive
            );
        if needs_refresh {
            let _ = self.refresh_impl(None, vec![], true);
            if !matches!(op.operation_type, OperationType::Inflation) {
                let _ = self.refresh_impl(None, vec![], true);
            }
        }

        // cleanup cache for approved or discarded operations
        if op.status == OperationStatus::Approved || op.status == OperationStatus::Discarded {
            let ops_dir = self.get_hub_ops_dir();
            if ops_dir.exists() {
                for entry in fs::read_dir(&ops_dir)? {
                    fs::remove_file(entry?.path())?;
                }
            }
        }

        self.update_backup_info(false)?;
        info!(self.logger(), "Sync with hub completed");
        Ok(Some(OperationInfo {
            operation_idx: op.operation_idx,
            initiator_xpub: op.initiator_xpub,
            operation,
        }))
    }

    fn read_psbt_from_file(path: &Path) -> Result<Psbt, Error> {
        let file = fs::File::open(path)?;
        let mut reader = io::BufReader::new(file);
        Psbt::deserialize_from_reader(&mut reader).map_err(|_| Error::MultisigUnexpectedData {
            details: s!("invalid PSBT"),
        })
    }

    fn extract_psbt_string(files: &[FileResponse]) -> Result<String, Error> {
        let psbt_file = files
            .iter()
            .find(|f| matches!(f.r#type, FileType::OperationPsbt))
            .ok_or(Error::MultisigUnexpectedData {
                details: s!("PSBT not found"),
            })?;
        let psbt = Self::read_psbt_from_file(&psbt_file.filepath)?;
        Ok(psbt.to_string())
    }

    fn build_voting_status(
        op: &OperationResponse,
        my_response: Option<bool>,
    ) -> Result<MultisigVotingStatus, Error> {
        Ok(MultisigVotingStatus {
            acked_by: op.acked_by.clone(),
            nacked_by: op.nacked_by.clone(),
            threshold: op.threshold.ok_or(Error::MultisigUnexpectedData {
                details: s!("operation with status should have a threshold"),
            })?,
            my_response,
        })
    }

    fn combine_psbts_from_files(files: &[FileResponse]) -> Result<Psbt, Error> {
        let mut combined_psbt: Option<Psbt> = None;
        let mut combined = false;
        for file in files {
            if matches!(file.r#type, FileType::ResponsePsbt) {
                let psbt = Self::read_psbt_from_file(&file.filepath)?;
                if let Some(ref mut combined_psbt) = combined_psbt {
                    combined_psbt
                        .combine(psbt.clone())
                        .map_err(|_| Error::CannotCombinePsbts)?;
                    combined = true;
                } else {
                    combined_psbt = Some(psbt);
                }
            }
        }
        if !combined {
            return Err(Error::MultisigUnexpectedData {
                details: s!("insufficient PSBTs supplied"),
            });
        }
        Ok(combined_psbt.expect("should exist"))
    }

    fn handle_operation<H: OperationHandler>(
        &mut self,
        op: &OperationResponse,
        files: &[FileResponse],
    ) -> Result<Operation, Error> {
        let details = H::extract_details(files)?;
        match (op.status.clone(), op.my_response) {
            (OperationStatus::Pending, None) => {
                let psbt = Self::extract_psbt_string(files)?;
                let status = Self::build_voting_status(op, None)?;
                Ok(H::to_review(psbt, details, status))
            }
            (OperationStatus::Pending, my_response) => {
                let status = Self::build_voting_status(op, my_response)?;
                Ok(H::pending(details, status))
            }
            (OperationStatus::Approved, _) => {
                let mut combined_psbt = Self::combine_psbts_from_files(files)?;
                self.finalize_psbt_impl(&mut combined_psbt, None)?;
                let txid = combined_psbt.unsigned_tx.compute_txid().to_string();
                H::reconstruct_transfer_directory(self, &txid, files)?;
                let txid = H::finalize_and_execute(self, &combined_psbt)?;
                self.mark_operation_as_processed(op.operation_idx)?;
                let status = Self::build_voting_status(op, op.my_response)?;
                Ok(H::completed(txid, details, status))
            }
            (OperationStatus::Discarded, my_response) => {
                self.mark_operation_as_processed(op.operation_idx)?;
                let status = Self::build_voting_status(op, my_response)?;
                Ok(H::discarded(details, status))
            }
        }
    }

    fn process_operation(&mut self, op: &OperationResponse) -> Result<Operation, Error> {
        let files = self.get_or_download_files(op.files.clone())?;
        Ok(match op.operation_type {
            OperationType::CreateUtxos => {
                self.handle_operation::<CreateUtxosHandler>(op, &files)?
            }
            OperationType::SendBtc => self.handle_operation::<SendBtcHandler>(op, &files)?,
            OperationType::SendRgb => self.handle_operation::<SendRgbHandler>(op, &files)?,
            OperationType::Inflation => self.handle_operation::<InflateHandler>(op, &files)?,
            OperationType::Issuance => match op.status {
                OperationStatus::Approved => {
                    let asset_id = self.accept_issuance_consignment(&files)?;
                    self.mark_operation_as_processed(op.operation_idx)?;
                    Operation::IssuanceCompleted { asset_id }
                }
                _ => {
                    return Err(Error::MultisigUnexpectedData {
                        details: s!("unexpected issuance status"),
                    });
                }
            },
            OperationType::BlindReceive | OperationType::WitnessReceive => match op.status {
                OperationStatus::Approved => {
                    let details = self.import_receive_data(&files, &op.operation_type)?;
                    self.mark_operation_as_processed(op.operation_idx)?;
                    match op.operation_type {
                        OperationType::BlindReceive => Operation::BlindReceiveCompleted { details },
                        _ => Operation::WitnessReceiveCompleted { details },
                    }
                }
                _ => {
                    return Err(Error::MultisigUnexpectedData {
                        details: s!("unexpected receive status"),
                    });
                }
            },
        })
    }

    /// Respond to an operation by posting an ACK (with a signed PSBT) or a NACK to the hub and
    /// return the corresponding [`OperationInfo`].
    ///
    /// If this response changes the status of the operation, it is handled and marked as processed.
    pub fn respond_to_operation(
        &mut self,
        online: Online,
        operation_idx: i32,
        respond_to_operation: RespondToOperation,
    ) -> Result<OperationInfo, Error> {
        info!(self.logger(), "Responding to operation...");
        self.check_online(online)?;
        self.check_is_cosigner()?;

        // check we can respond to operation
        let op = self
            .hub_client()
            .get_operation_by_idx(operation_idx)?
            .ok_or(Error::MultisigOperationNotFound { operation_idx })?;
        if op.status != OperationStatus::Pending {
            return Err(Error::MultisigCannotRespondToOperation {
                details: s!("not pending"),
            });
        }
        if op.my_response.is_some() {
            return Err(Error::MultisigCannotRespondToOperation {
                details: s!("already responded"),
            });
        }

        // extract and check PSBT
        if let RespondToOperation::Ack(psbt) = &respond_to_operation {
            // check PSBT is valid and has been signed
            let psbt = Psbt::from_str(psbt)?;
            if self.psbt_signature_count(&psbt)? == 0 {
                return Err(Error::InvalidPsbt {
                    details: s!("PSBT has no signatures"),
                });
            }

            // check PSBT is the one from the operation we are responding
            let psbt_file = op
                .files
                .iter()
                .find(|f| f.r#type == FileType::OperationPsbt);
            let Some(psbt_file) = psbt_file else {
                return Err(Error::MultisigUnexpectedData {
                    details: s!("operation should have a PSBT"),
                });
            };
            let op_psbt_path = self.get_or_download_file(psbt_file)?;
            let op_psbt = Self::read_psbt_from_file(&op_psbt_path)?;
            if op_psbt.unsigned_tx.compute_txid() != psbt.unsigned_tx.compute_txid() {
                return Err(Error::InvalidPsbt {
                    details: s!("PSBT unrelated to operation"),
                });
            }
        }

        // send response to hub
        let operation_response = self
            .hub_client()
            .respond_to_operation(operation_idx, respond_to_operation)?;

        // process operation
        let operation = self.process_operation(&operation_response)?;

        self.update_backup_info(false)?;
        info!(self.logger(), "Responding to operation...");
        Ok(OperationInfo {
            operation_idx: operation_response.operation_idx,
            initiator_xpub: operation_response.initiator_xpub,
            operation,
        })
    }

    fn post_operation(
        &self,
        operation_type: OperationType,
        post_data: PostData,
    ) -> Result<InitOperationResult, Error> {
        // collect operation files
        let mut files = vec![];
        let psbt = match post_data {
            PostData::Psbt(psbt) => psbt,
            PostData::BeginOperationData(begin_operation_data) => {
                let fascia_path = begin_operation_data.transfer_dir.join(FASCIA_FILE);
                files.push((FileType::Fascia, FileSource::Path(fascia_path)));
                let transfer_metadata_bytes =
                    serde_json::to_vec(&begin_operation_data.info_batch_transfer)
                        .expect("serializable");
                files.push((
                    FileType::OperationData,
                    FileSource::Bytes(transfer_metadata_bytes),
                ));
                begin_operation_data.psbt
            }
        };
        files.push((FileType::OperationPsbt, FileSource::Bytes(psbt.serialize())));

        // post operation and its files
        let response = self.hub_client().post_operation(files, operation_type)?;

        Ok(InitOperationResult {
            psbt: psbt.to_string(),
            operation_idx: response.operation_idx,
        })
    }

    /// Prepare the PSBT to create new UTXOs to hold RGB allocations with the provided `fee_rate`
    /// (in sat/vB) and post the operation to the hub.
    ///
    /// If `up_to` is false, just create the required UTXOs, if it is true, create as many UTXOs as
    /// needed to reach the requested number or return an error if none need to be created.
    ///
    /// Providing the optional `num` parameter requests that many UTXOs, if it's not specified the
    /// default number (5<!--UTXO_NUM-->) is used.
    ///
    /// Providing the optional `size` parameter requests that UTXOs be created of that size (in
    /// sats), if it's not specified the default one (1000<!--UTXO_SIZE-->) is used.
    ///
    /// If not enough bitcoin funds are available to create the requested (or default) number of
    /// UTXOs, the number is decremented by one until it is possible to complete the operation. If
    /// the number reaches zero, an error is returned.
    ///
    /// Returns a PSBT ready to be signed and the operation index on the hub.
    pub fn create_utxos_init(
        &mut self,
        online: Online,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: u64,
        skip_sync: bool,
    ) -> Result<InitOperationResult, Error> {
        info!(self.logger(), "Initiate creating UTXOs...");
        self.check_online(online)?;
        self.check_is_cosigner()?;
        let psbt = self.create_utxos_begin_impl(up_to, num, size, fee_rate, skip_sync)?;
        let res = self.post_operation(OperationType::CreateUtxos, PostData::Psbt(psbt))?;
        self.update_backup_info(false)?;
        info!(self.logger(), "Initiate creating UTXOs completed");
        Ok(res)
    }

    /// Prepare the PSBT to send the specified `amount` of bitcoins (in sats) using the vanilla
    /// wallet to the specified Bitcoin `address` with the specified `fee_rate` (in sat/vB) and post
    /// the operation to the hub.
    ///
    /// Returns a PSBT ready to be signed and the operation index on the hub.
    pub fn send_btc_init(
        &mut self,
        online: Online,
        address: String,
        amount: u64,
        fee_rate: u64,
        skip_sync: bool,
    ) -> Result<InitOperationResult, Error> {
        info!(self.logger(), "Initiate sending BTC...");
        self.check_online(online)?;
        self.check_is_cosigner()?;
        let psbt = self.send_btc_begin_impl(address, amount, fee_rate, skip_sync)?;
        let res = self.post_operation(OperationType::SendBtc, PostData::Psbt(psbt))?;
        self.update_backup_info(false)?;
        info!(self.logger(), "Initiate sending BTC completed");
        Ok(res)
    }

    /// Prepare the PSBT to send RGB assets according to the given recipient map, with the provided
    /// `fee_rate` (in sat/vB) and post the operation to the hub.
    ///
    /// The `recipient_map` maps asset IDs to a vector of [`Recipient`]s. When multiple recipients
    /// are provided, a batch transfer will be performed, meaning a single Bitcoin transaction will
    /// be used to move all assets to the respective recipients. Each asset being sent will result
    /// in the creation of a single consignment, which will then be posted to the RGB proxy server
    /// for each of its recipients.
    ///
    /// If `donation` is true, the resulting transaction will be broadcast (by
    /// [`send_end`](Wallet::send_end)) as soon as it's ready, without the need for recipients to
    /// ACK the transfer.
    /// If `donation` is false, all recipients will need to ACK the transfer before the transaction
    /// is broadcast.
    ///
    /// The `min_confirmations` number determines the minimum number of confirmations needed for
    /// the transaction anchoring the transfer for it to be considered final and move (while
    /// refreshing) to the [`TransferStatus::Settled`] status.
    ///
    /// An optional expiration UTC timestamp can be specified, which will set the expiration of the
    /// transfer.
    ///
    /// Returns a PSBT ready to be signed and the operation index on the hub.
    pub fn send_init(
        &mut self,
        online: Online,
        recipient_map: HashMap<String, Vec<Recipient>>,
        donation: bool,
        fee_rate: u64,
        min_confirmations: u8,
        expiration_timestamp: Option<u64>,
    ) -> Result<InitOperationResult, Error> {
        info!(self.logger(), "Initiate sending...");
        self.check_online(online)?;
        self.check_is_cosigner()?;
        let data = self.send_begin_impl(
            recipient_map,
            donation,
            fee_rate,
            min_confirmations,
            expiration_timestamp.map(|t| t as i64),
            true,
        )?;
        let res = self.post_operation(
            OperationType::SendRgb,
            PostData::BeginOperationData(Box::new(data)),
        )?;
        self.update_backup_info(false)?;
        info!(self.logger(), "Initiate sending completed");
        Ok(res)
    }

    /// Prepare the PSBT to inflate RGB assets according to the given inflation amounts, with the
    /// provided `fee_rate` (in sat/vB) and post the operation to the hub.
    ///
    /// For every amount in `inflation_amounts` a new UTXO allocating the requested
    /// asset amount will be created. The sum of its elements plus the known circulating supply
    /// cannot exceed the maximum `u64` value.
    ///
    /// The `min_confirmations` number determines the minimum number of confirmations needed for
    /// the transaction anchoring the transfer for it to be considered final and move (while
    /// refreshing) to the [`TransferStatus::Settled`] status.
    ///
    /// Returns a PSBT ready to be signed and the operation index on the hub.
    pub fn inflate_init(
        &mut self,
        online: Online,
        asset_id: String,
        inflation_amounts: Vec<u64>,
        fee_rate: u64,
        min_confirmations: u8,
    ) -> Result<InitOperationResult, Error> {
        info!(self.logger(), "Initiate inflating...");
        self.check_online(online)?;
        self.check_is_cosigner()?;

        let data = self.inflate_begin_impl(
            asset_id,
            inflation_amounts,
            fee_rate,
            min_confirmations,
            true,
        )?;
        let res = self.post_operation(
            OperationType::Inflation,
            PostData::BeginOperationData(Box::new(data)),
        )?;
        self.update_backup_info(false)?;
        info!(self.logger(), "Initiate inflating completed");
        Ok(res)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn cosigner_display_and_parse() {
        let keys = generate_keys(BitcoinNetwork::Regtest);

        // vanilla_keychain None
        let cosigner = Cosigner::from_keys(&keys, None);
        let cosigner_str = cosigner.to_string();
        let parsed_cosigner = Cosigner::from_str(&cosigner_str).unwrap();
        assert_eq!(
            parsed_cosigner.master_fingerprint,
            cosigner.master_fingerprint
        );
        assert_eq!(
            parsed_cosigner.account_xpub_vanilla,
            cosigner.account_xpub_vanilla
        );
        assert_eq!(
            parsed_cosigner.account_xpub_colored,
            cosigner.account_xpub_colored
        );
        assert_eq!(parsed_cosigner.vanilla_keychain, Some(0));

        // vanilla_keychain Some(2)
        let cosigner2 = Cosigner::from_keys(&keys, Some(2));
        let cosigner2_str = cosigner2.to_string();
        let parsed_cosigner2 = Cosigner::from_str(&cosigner2_str).unwrap();
        assert_eq!(
            parsed_cosigner2.master_fingerprint,
            cosigner2.master_fingerprint
        );
        assert_eq!(
            parsed_cosigner2.account_xpub_vanilla,
            cosigner2.account_xpub_vanilla
        );
        assert_eq!(
            parsed_cosigner2.account_xpub_colored,
            cosigner2.account_xpub_colored
        );
        assert_eq!(parsed_cosigner2.vanilla_keychain, Some(2));
    }

    #[test]
    fn cosigner_parse_invalid() {
        // invalid number of parts
        let result = Cosigner::from_str("invalid");
        assert_matches!(result.as_ref().unwrap_err(), Error::InvalidCosigner { details: d } if d == "expected 3 or 4 colon-separated parts, got 1");

        // invalid master fingerprint
        let result = Cosigner::from_str(
            "invalid:tpubDDu9rJ9wzT7eueYDT2S6SLRHekx2nzth5tEFDMa4kp9yV3KwrLxdSMBhcFYbvN4i9fCiMg9xaXhd13zEENAi47jZuEwFHQfR6qzzEoVVvgk:tpubDDWKRHFqLA3YRTVAVMaxdS7EZHV1y6BTuHozCExoyXXRQBF92kvAi5d7xog5Mg4jfy8HK1cMMUYQNSZmEtLhz9gAWyDLvK74Vz9oJA3xBSo:invalid",
        );
        assert_matches!(result.as_ref().unwrap_err(), Error::InvalidCosigner { details: d } if d == "invalid master_fingerprint 'invalid'");

        // invalid vanilla xpub
        let result = Cosigner::from_str(
            "abcd1234:invalid:tpubDDWKRHFqLA3YRTVAVMaxdS7EZHV1y6BTuHozCExoyXXRQBF92kvAi5d7xog5Mg4jfy8HK1cMMUYQNSZmEtLhz9gAWyDLvK74Vz9oJA3xBSo:3",
        );
        assert_matches!(result.as_ref().unwrap_err(), Error::InvalidCosigner { details: d } if d == "invalid vanilla xpub 'invalid'");

        // invalid colored xpub
        let result = Cosigner::from_str(
            "abcd1234:tpubDDu9rJ9wzT7eueYDT2S6SLRHekx2nzth5tEFDMa4kp9yV3KwrLxdSMBhcFYbvN4i9fCiMg9xaXhd13zEENAi47jZuEwFHQfR6qzzEoVVvgk:invalid:3",
        );
        assert_matches!(result.as_ref().unwrap_err(), Error::InvalidCosigner { details: d } if d == "invalid colored xpub 'invalid'");

        // invalid vanilla_keychain
        let result = Cosigner::from_str(
            "abcd1234:tpubDDu9rJ9wzT7eueYDT2S6SLRHekx2nzth5tEFDMa4kp9yV3KwrLxdSMBhcFYbvN4i9fCiMg9xaXhd13zEENAi47jZuEwFHQfR6qzzEoVVvgk:tpubDDWKRHFqLA3YRTVAVMaxdS7EZHV1y6BTuHozCExoyXXRQBF92kvAi5d7xog5Mg4jfy8HK1cMMUYQNSZmEtLhz9gAWyDLvK74Vz9oJA3xBSo:invalid",
        );
        assert_matches!(result.as_ref().unwrap_err(), Error::InvalidCosigner { details: d } if d == "invalid vanilla_keychain value 'invalid'");
    }
}
