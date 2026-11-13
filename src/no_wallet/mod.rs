//! No wallet.
//!
//! This module defines the methods that don't require a wallet (available without the `wallet`
//! feature) and the objects they return, some of which are shared with wallet methods (e.g. the
//! inspection ones). It also defines the objects and helpers that don't need a wallet at all (the
//! bitcoin primitives, the recipient ID ones and the invoice and transport endpoint parsing ones),
//! which wallet methods also use. Methods that need to reach the network (e.g. the validation with
//! an indexer) require an indexer validation feature (`bitcoind_validation`,
//! `electrum_validation` or `esplora_validation`), which doesn't enable the `wallet` feature.

pub(crate) mod inspection;
pub(crate) mod validation;

use super::*;

pub use inspection::{
    BundleFilter, ConsignmentInspection, RgbGenesisInfo, RgbInputInfo, RgbInspection, RgbMediaInfo,
    RgbOperationInfo, RgbOutputInfo, RgbTransitionInfo, TransitionGlobal, TransitionMetadata,
    inspect_consignment,
};
#[cfg(feature = "wallet")]
pub(crate) use inspection::{bundle_info, verify_witness_commitment};
#[cfg(feature = "_validation")]
pub use validation::{BitcoindAuth, IndexerConfig, IndexerProtocol, validate_consignment};
pub use validation::{
    ConsignmentValidation, RejectList, RejectListSource, ResolveRejectList, ValidatedWitness,
    ValidationOptions, ValidationWarning, WitnessValidationStatus,
    validate_consignment_with_resolver,
};

// ────────────────────────────────────────────────────────────
// Bitcoin primitives
// ────────────────────────────────────────────────────────────

/// Supported Bitcoin networks.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum BitcoinNetwork {
    /// Bitcoin's mainnet
    Mainnet,
    /// Bitcoin's testnet3
    Testnet,
    /// Bitcoin's testnet4
    Testnet4,
    /// Bitcoin's default signet
    Signet,
    /// Bitcoin's regtest
    Regtest,
    /// Bitcoin's custom signet
    SignetCustom,
}

#[cfg(feature = "wallet")]
impl BitcoinNetwork {
    pub(crate) fn network_kind(&self) -> bitcoin::NetworkKind {
        match self {
            BitcoinNetwork::Mainnet => bitcoin::NetworkKind::Main,
            _ => bitcoin::NetworkKind::Test,
        }
    }
}

impl fmt::Display for BitcoinNetwork {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl FromStr for BitcoinNetwork {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "mainnet" | "bitcoin" => BitcoinNetwork::Mainnet,
            "testnet" | "testnet3" => BitcoinNetwork::Testnet,
            "testnet4" => BitcoinNetwork::Testnet4,
            "regtest" => BitcoinNetwork::Regtest,
            "signet" => BitcoinNetwork::Signet,
            "signetcustom" => BitcoinNetwork::SignetCustom,
            _ => {
                return Err(Error::InvalidBitcoinNetwork {
                    network: s.to_string(),
                });
            }
        })
    }
}

impl TryFrom<ChainNet> for BitcoinNetwork {
    type Error = Error;

    fn try_from(x: ChainNet) -> Result<Self, Self::Error> {
        match x {
            ChainNet::BitcoinMainnet => Ok(BitcoinNetwork::Mainnet),
            ChainNet::BitcoinTestnet3 => Ok(BitcoinNetwork::Testnet),
            ChainNet::BitcoinTestnet4 => Ok(BitcoinNetwork::Testnet4),
            ChainNet::BitcoinSignet => Ok(BitcoinNetwork::Signet),
            ChainNet::BitcoinRegtest => Ok(BitcoinNetwork::Regtest),
            ChainNet::BitcoinSignetCustom => Ok(BitcoinNetwork::SignetCustom),
            _ => Err(Error::UnsupportedLayer1 {
                layer_1: x.layer1().to_string(),
            }),
        }
    }
}

impl From<BitcoinNetwork> for bitcoin::Network {
    fn from(x: BitcoinNetwork) -> bitcoin::Network {
        match x {
            BitcoinNetwork::Mainnet => bitcoin::Network::Bitcoin,
            BitcoinNetwork::Testnet => bitcoin::Network::Testnet,
            BitcoinNetwork::Testnet4 => bitcoin::Network::Testnet4,
            BitcoinNetwork::Signet => bitcoin::Network::Signet,
            BitcoinNetwork::Regtest => bitcoin::Network::Regtest,
            BitcoinNetwork::SignetCustom => bitcoin::Network::Signet,
        }
    }
}

impl From<BitcoinNetwork> for bitcoin::NetworkKind {
    fn from(x: BitcoinNetwork) -> Self {
        match x {
            BitcoinNetwork::Mainnet => Self::Main,
            _ => Self::Test,
        }
    }
}

impl From<BitcoinNetwork> for ChainNet {
    fn from(x: BitcoinNetwork) -> ChainNet {
        match x {
            BitcoinNetwork::Mainnet => ChainNet::BitcoinMainnet,
            BitcoinNetwork::Testnet => ChainNet::BitcoinTestnet3,
            BitcoinNetwork::Testnet4 => ChainNet::BitcoinTestnet4,
            BitcoinNetwork::Signet => ChainNet::BitcoinSignet,
            BitcoinNetwork::Regtest => ChainNet::BitcoinRegtest,
            BitcoinNetwork::SignetCustom => ChainNet::BitcoinSignetCustom,
        }
    }
}

/// Bitcoin transaction outpoint.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Outpoint {
    /// ID of the transaction
    pub txid: String,
    /// Output index
    pub vout: u32,
}

impl fmt::Display for Outpoint {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}:{}", self.txid, self.vout)
    }
}

impl From<OutPoint> for Outpoint {
    fn from(x: OutPoint) -> Outpoint {
        Outpoint {
            txid: x.txid.to_string(),
            vout: x.vout,
        }
    }
}

#[cfg(feature = "wallet")]
impl From<DbReservedTxo> for Outpoint {
    fn from(x: DbReservedTxo) -> Outpoint {
        Outpoint {
            txid: x.txid,
            vout: x.vout,
        }
    }
}

#[cfg(feature = "wallet")]
impl From<DbTxo> for Outpoint {
    fn from(x: DbTxo) -> Outpoint {
        Outpoint {
            txid: x.txid,
            vout: x.vout,
        }
    }
}

impl From<Outpoint> for OutPoint {
    fn from(x: Outpoint) -> OutPoint {
        OutPoint::from_str(&x.to_string()).expect("outpoint should be parsable")
    }
}

// ────────────────────────────────────────────────────────────
// Recipients
// ────────────────────────────────────────────────────────────

/// The type of an RGB recipient
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum RecipientType {
    /// Receive via blinded UTXO
    Blind,
    /// Receive via witness TX
    Witness,
}

impl fmt::Display for RecipientType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

/// RGB recipient information used to be paid
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct RecipientInfo {
    /// Recipient ID
    pub recipient_id: String,
    /// Recipient type
    pub recipient_type: RecipientType,
    /// Recipient network
    pub network: BitcoinNetwork,
}

impl RecipientInfo {
    /// Builds a new [`RecipientInfo`] from the provided string, checking that it is valid.
    pub fn new(recipient_id: String) -> Result<Self, Error> {
        let xchainnet_beneficiary = XChainNet::<Beneficiary>::from_str(&recipient_id)
            .map_err(|_| Error::InvalidRecipientID)?;
        let recipient_type = match xchainnet_beneficiary.into_inner() {
            Beneficiary::WitnessVout(_, _) => RecipientType::Witness,
            Beneficiary::BlindedSeal(_) => RecipientType::Blind,
        };
        Ok(Self {
            recipient_id,
            recipient_type,
            network: xchainnet_beneficiary.chain_network().try_into()?,
        })
    }
}

/// Extract the witness script if recipient is a Witness one
pub fn script_buf_from_recipient_id(recipient_id: String) -> Result<Option<ScriptBuf>, Error> {
    let xchainnet_beneficiary =
        XChainNet::<Beneficiary>::from_str(&recipient_id).map_err(|_| Error::InvalidRecipientID)?;
    match xchainnet_beneficiary.into_inner() {
        Beneficiary::WitnessVout(pay_2_vout, _) => {
            let script_buf = pay_2_vout.to_script();
            Ok(Some(script_buf))
        }
        Beneficiary::BlindedSeal(_) => Ok(None),
    }
}

pub(crate) fn beneficiary_from_script_buf(script_buf: ScriptBuf) -> Beneficiary {
    let address_payload = AddressPayload::from_script(&script_buf).unwrap();
    Beneficiary::WitnessVout(Pay2Vout::new(address_payload), None)
}

/// Return the recipient ID for a specific script buf
pub fn recipient_id_from_script_buf(
    script_buf: ScriptBuf,
    bitcoin_network: BitcoinNetwork,
) -> String {
    let beneficiary = beneficiary_from_script_buf(script_buf);
    XChainNet::with(bitcoin_network.into(), beneficiary).to_string()
}

// ────────────────────────────────────────────────────────────
// Invoices & transport
// ────────────────────────────────────────────────────────────

/// An RGB invoice.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct Invoice {
    /// The RGB invoice string
    invoice_string: String,
    /// The data of the RGB invoice
    pub(crate) invoice_data: InvoiceData,
}

impl Invoice {
    /// Parse the provided `invoice_string`.
    /// Throws an error if the provided string is not a valid RGB invoice.
    pub fn new(invoice_string: String) -> Result<Self, Error> {
        let decoded = RgbInvoice::from_str(&invoice_string).map_err(|e| Error::InvalidInvoice {
            details: e.to_string(),
        })?;
        let asset_id = decoded.contract.map(|cid| cid.to_string());
        let asset_schema = if let Some(schema_id) = decoded.schema {
            Some(AssetSchema::try_from(schema_id)?)
        } else {
            None
        };
        let assignment_name = decoded.assignment_name.map(|a| a.to_string());
        let assignment = match asset_schema {
            None => match (decoded.assignment_state, assignment_name.as_deref()) {
                (Some(InvoiceState::Amount(v)), Some(RGB_STATE_ASSET_OWNER)) => {
                    Assignment::Fungible(v.value())
                }
                (Some(InvoiceState::Amount(v)), Some(RGB_STATE_INFLATION_ALLOWANCE)) => {
                    Assignment::InflationRight(v.value())
                }
                (Some(InvoiceState::Amount(_)), _) => Assignment::Any,
                (Some(InvoiceState::Data(_)), Some(RGB_STATE_ASSET_OWNER) | None) => {
                    Assignment::NonFungible
                }
                (None, None) => Assignment::Any,
                (_, _) => {
                    return Err(Error::InvalidInvoice {
                        details: s!("unsupported assignment"),
                    });
                }
            },
            Some(AssetSchema::Nia) | Some(AssetSchema::Cfa) => {
                match (decoded.assignment_state, assignment_name.as_deref()) {
                    (Some(InvoiceState::Amount(v)), Some(RGB_STATE_ASSET_OWNER) | None) => {
                        Assignment::Fungible(v.value())
                    }
                    (None, Some(RGB_STATE_ASSET_OWNER) | None) => Assignment::Fungible(0),
                    (_, _) => {
                        return Err(Error::InvalidInvoice {
                            details: s!("invalid assignment"),
                        });
                    }
                }
            }
            Some(AssetSchema::Uda) => {
                match (decoded.assignment_state, assignment_name.as_deref()) {
                    (Some(InvoiceState::Data(_)) | None, Some(RGB_STATE_ASSET_OWNER) | None) => {
                        Assignment::NonFungible
                    }
                    (_, _) => {
                        return Err(Error::InvalidInvoice {
                            details: s!("invalid assignment"),
                        });
                    }
                }
            }
            Some(AssetSchema::Ifa) => {
                match (decoded.assignment_state, assignment_name.as_deref()) {
                    (Some(InvoiceState::Amount(v)), Some(RGB_STATE_ASSET_OWNER)) => {
                        Assignment::Fungible(v.value())
                    }
                    (None, Some(RGB_STATE_ASSET_OWNER)) => Assignment::Fungible(0),
                    (Some(InvoiceState::Amount(v)), Some(RGB_STATE_INFLATION_ALLOWANCE)) => {
                        Assignment::InflationRight(v.value())
                    }
                    (None, Some(RGB_STATE_INFLATION_ALLOWANCE)) => Assignment::InflationRight(0),
                    (Some(InvoiceState::Void) | None, Some(RGB_STATE_LINK_RIGHT)) => {
                        Assignment::LinkRight
                    }
                    (Some(InvoiceState::Amount(_)), None) => Assignment::Any,
                    (None, None) => Assignment::Any,
                    (_, _) => {
                        return Err(Error::InvalidInvoice {
                            details: s!("invalid assignment"),
                        });
                    }
                }
            }
            Some(AssetSchema::Bfa) => {
                match (decoded.assignment_state, assignment_name.as_deref()) {
                    (Some(InvoiceState::Amount(v)), Some(RGB_STATE_ASSET_OWNER)) => {
                        Assignment::Fungible(v.value())
                    }
                    (None, Some(RGB_STATE_ASSET_OWNER)) => Assignment::Fungible(0),
                    (Some(InvoiceState::Void) | None, Some(RGB_STATE_LINK_RIGHT)) => {
                        Assignment::LinkRight
                    }
                    (Some(InvoiceState::Void) | None, Some(RGB_STATE_MINT_RIGHT)) => {
                        Assignment::MintRight
                    }
                    (Some(InvoiceState::Amount(_)), None) => Assignment::Any,
                    (None, None) => Assignment::Any,
                    (_, _) => {
                        return Err(Error::InvalidInvoice {
                            details: s!("invalid assignment"),
                        });
                    }
                }
            }
        };
        let recipient_id = decoded.beneficiary.to_string();
        let transport_endpoints: Vec<String> =
            decoded.transports.iter().map(|t| t.to_string()).collect();
        // rgb-ops parses the invoice's public key into a 64-char hex string
        let public_key = decoded.public_key;
        let encryption = Encryption::from(decoded.encryption);

        let layer_1 = decoded.beneficiary.layer1();
        let network = match layer_1 {
            Layer1::Bitcoin => decoded.beneficiary.chain_network().try_into().unwrap(),
            _ => {
                return Err(Error::UnsupportedLayer1 {
                    layer_1: layer_1.to_string(),
                });
            }
        };

        Ok(Invoice {
            invoice_string,
            invoice_data: InvoiceData {
                recipient_id,
                asset_schema,
                asset_id,
                assignment,
                assignment_name,
                expiration_timestamp: decoded.expiry.map(|t| t as u64),
                transport_endpoints,
                network,
                public_key,
                encryption,
                unknown_query_params: decoded.unknown_query.into_iter().collect(),
            },
        })
    }

    /// Return the data associated with this [`Invoice`].
    pub fn invoice_data(&self) -> InvoiceData {
        self.invoice_data.clone()
    }

    /// Return the string associated with this [`Invoice`].
    pub fn invoice_string(&self) -> String {
        self.invoice_string.clone()
    }
}

/// The data of an RGB invoice.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct InvoiceData {
    /// ID of the receive operation (blinded UTXO or Bitcoin script)
    pub recipient_id: String,
    /// RGB schema
    pub asset_schema: Option<AssetSchema>,
    /// RGB asset ID
    pub asset_id: Option<String>,
    /// RGB assignment
    pub assignment: Assignment,
    /// RGB assignment name
    pub assignment_name: Option<String>,
    /// Bitcoin network
    pub network: BitcoinNetwork,
    /// Invoice expiration
    pub expiration_timestamp: Option<u64>,
    /// Transport endpoints
    pub transport_endpoints: Vec<String>,
    /// Recipient's Ed25519 public key (hex-encoded), from the invoice `pk` field (if present)
    pub public_key: Option<String>,
    /// Consignment/media encryption preference, from the invoice `enc` field
    pub encryption: Encryption,
    /// Unknown query parameters carried by the invoice
    pub unknown_query_params: HashMap<String, String>,
}

/// An RGB transport endpoint.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct TransportEndpoint {
    /// Endpoint address
    pub endpoint: String,
    /// Endpoint transport type
    pub transport_type: TransportType,
}

impl TransportEndpoint {
    /// Builds a new [`TransportEndpoint::endpoint`] from the provided string, checking that it is
    /// valid.
    pub fn new(transport_endpoint: String) -> Result<Self, Error> {
        let rgb_transport = RgbTransport::from_str(&transport_endpoint)?;
        TransportEndpoint::try_from(rgb_transport)
    }

    /// Return the transport type of this transport endpoint.
    pub fn transport_type(&self) -> TransportType {
        self.transport_type
    }
}

impl TryFrom<RgbTransport> for TransportEndpoint {
    type Error = Error;

    fn try_from(x: RgbTransport) -> Result<Self, Self::Error> {
        match x {
            RgbTransport::JsonRpc { tls, host } => Ok(TransportEndpoint {
                endpoint: format!("http{}://{host}", if tls { "s" } else { "" }),
                transport_type: TransportType::JsonRpc,
            }),
            _ => Err(Error::UnsupportedTransportType),
        }
    }
}

// ────────────────────────────────────────────────────────────
// Files
// ────────────────────────────────────────────────────────────

/// Check the file at the given path exists.
pub(crate) fn check_file_exists(file_path: &str) -> Result<(), Error> {
    if !Path::new(file_path).exists() {
        return Err(Error::InvalidFilePath {
            file_path: file_path.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLINDED_RECIPIENT_ID: &str =
        "bcrt:utxob:tjVmHbI2-U0_umHn-bU4cmP6-l3VW00H-ewoi2uz-XZG6O3i-wUFBW";

    #[test]
    fn new_recipient_info() {
        let recipient_info = RecipientInfo::new(BLINDED_RECIPIENT_ID.to_string()).unwrap();
        assert_eq!(recipient_info.recipient_id, BLINDED_RECIPIENT_ID);
        assert_eq!(recipient_info.recipient_type, RecipientType::Blind);
        assert_eq!(recipient_info.network, BitcoinNetwork::Regtest);

        let result = RecipientInfo::new(s!("invalid"));
        assert!(matches!(result, Err(Error::InvalidRecipientID)));
    }

    #[test]
    fn new_invoice() {
        let invoice = Invoice::new(format!(
            "rgb:~/~/ae/{BLINDED_RECIPIENT_ID}?assignment_name=assetOwner"
        ))
        .unwrap();
        let invoice_data = invoice.invoice_data();
        assert_eq!(invoice_data.recipient_id, BLINDED_RECIPIENT_ID);
        assert_eq!(invoice_data.assignment, Assignment::Fungible(1));
        assert_eq!(
            invoice_data.assignment_name.as_deref(),
            Some(RGB_STATE_ASSET_OWNER)
        );
        assert_eq!(invoice_data.network, BitcoinNetwork::Regtest);
        assert_eq!(invoice_data.encryption, Encryption::Required);

        let result = Invoice::new(s!("invalid"));
        assert!(matches!(result, Err(Error::InvalidInvoice { details: _ })));
    }

    #[test]
    fn new_transport_endpoint() {
        let transport_endpoint =
            TransportEndpoint::new(s!("rpc://127.0.0.1:3000/json-rpc")).unwrap();
        assert_eq!(
            transport_endpoint.endpoint,
            "http://127.0.0.1:3000/json-rpc"
        );
        assert_eq!(transport_endpoint.transport_type(), TransportType::JsonRpc);

        let result = TransportEndpoint::new(s!("ws://127.0.0.1:3000/json-rpc"));
        assert!(matches!(result, Err(Error::UnsupportedTransportType)));

        let result = TransportEndpoint::new(s!("127.0.0.1:3000/json-rpc"));
        assert!(matches!(
            result,
            Err(Error::InvalidTransportEndpoint { details: _ })
        ));
    }

    #[test]
    fn outpoint_conversions() {
        let outpoint = Outpoint {
            txid: s!("0000000000000000000000000000000000000000000000000000000000000001"),
            vout: 7,
        };
        assert_eq!(
            outpoint.to_string(),
            "0000000000000000000000000000000000000000000000000000000000000001:7"
        );
        assert_eq!(Outpoint::from(OutPoint::from(outpoint.clone())), outpoint);
    }
}
