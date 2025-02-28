#![allow(clippy::too_many_arguments)]
#![warn(missing_docs)]

//! A library to manage wallets for RGB assets.
//!
//! ## Wallet
//! The main component of the library is the [`Wallet`].
//!
//! It allows to create and operate an RGB wallet that can issue, send and receive NIA, CFA and UDA
//! assets. The library also manages UTXOs and asset allocations.
//!
//! ## Backend
//! The library uses BDK for walleting operations and several components from the RGB ecosystem for
//! RGB asset operations.
//!
//! ## Database
//! A SQLite database is used to persist data to disk.
//!
//! Database support is designed in order to support multiple database backends. At the moment only
//! SQLite is supported but adding more should be relatively easy.
//!
//! ## Api
//! RGB asset transfers require the exchange of off-chain data in the form of consignment or media
//! files.
//!
//! The library currently implements the API for a proxy server to support these data exchanges
//! between sender and receiver.
//!
//! ## Errors
//! Errors are handled with the crate `thiserror`.
//!
//! ## FFI
//! Library functionality is exposed for other languages via FFI bindings.
//!
//! ## Examples
//! ### Create an RGB wallet
//! ```
//! use rgb_lib::wallet::{DatabaseType, Wallet, WalletData};
//! use rgb_lib::{generate_keys, BitcoinNetwork};
//!
//! fn main() -> Result<(), rgb_lib::Error> {
//!     let data_dir = tempfile::tempdir()?;
//!     let keys = generate_keys(BitcoinNetwork::Regtest);
//!     let wallet_data = WalletData {
//!         data_dir: data_dir.path().to_str().unwrap().to_string(),
//!         bitcoin_network: BitcoinNetwork::Regtest,
//!         database_type: DatabaseType::Sqlite,
//!         max_allocations_per_utxo: 5,
//!         pubkey: keys.account_xpub,
//!         mnemonic: Some(keys.mnemonic),
//!         vanilla_keychain: None,
//!     };
//!     let wallet = Wallet::new(wallet_data)?;
//!
//!     Ok(())
//! }
//! ```

pub(crate) mod api;
pub(crate) mod database;
pub(crate) mod error;
pub mod keys;
pub mod utils;
pub mod wallet;

pub use bdk_wallet;
pub use bdk_wallet::bitcoin;
pub use rgbinvoice::RgbTransport;
pub use rgbstd::{
    containers::{
        ConsignmentExt, Contract, Fascia, FileContent, PubWitness, Transfer as RgbTransfer,
    },
    persistence::UpdateRes,
    vm::WitnessOrd,
    ContractId, Txid as RgbTxid,
};

pub use crate::{
    database::enums::{AssetSchema, RecipientType, TransferStatus, TransportType},
    error::Error,
    keys::{generate_keys, restore_keys},
    utils::BitcoinNetwork,
    wallet::{backup::restore_backup, TransactionType, TransferKind, Wallet},
};

#[cfg(any(feature = "electrum", feature = "esplora"))]
use std::{
    cmp::{min, Ordering},
    collections::hash_map::DefaultHasher,
    hash::Hasher,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt, fs,
    hash::Hash,
    io::{self, ErrorKind, Read, Write},
    panic,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use amplify::{
    bmap,
    confinement::{Confined, U24},
    s, FromSliceError,
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use amplify::{hex::ToHex, none, ByteArray, Wrapper};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use base64::{engine::general_purpose, Engine as _};
#[cfg(feature = "electrum")]
use bdk_electrum::{
    electrum_client::{
        Client as ElectrumClient, ConfigBuilder, ElectrumApi, Error as ElectrumError, Param,
    },
    BdkElectrumClient,
};
#[cfg(feature = "esplora")]
use bdk_esplora::{
    esplora_client::{
        BlockingClient as EsploraClient, Builder as EsploraBuilder, Error as EsploraError,
    },
    EsploraExt,
};
use bdk_wallet::{
    bitcoin::{
        bip32::ChildNumber,
        bip32::{DerivationPath, KeySource, Xpriv, Xpub},
        hashes::Hash as Sha256Hash,
        psbt::Psbt,
        psbt::{raw::ProprietaryKey, ExtractTxError, Input, Output},
        secp256k1::Secp256k1,
        Address as BdkAddress, Amount as BdkAmount, BlockHash, Network as BdkNetwork, NetworkKind,
        OutPoint, OutPoint as BdkOutPoint, ScriptBuf, TxOut,
    },
    chain::ChainPosition,
    descriptor::Segwitv0,
    file_store::Store,
    keys::{
        bip39::{Language, Mnemonic, WordCount},
        DerivableKey, DescriptorKey,
        DescriptorKey::{Public, Secret},
        DescriptorSecretKey, ExtendedKey, GeneratableKey,
    },
    miniscript::DescriptorPublicKey,
    ChangeSet, KeychainKind, LocalOutput, PersistedWallet, SignOptions, Wallet as BdkWallet,
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use bdk_wallet::{
    bitcoin::{blockdata::fee_rate::FeeRate, hashes::sha256, Transaction as BdkTransaction, Txid},
    chain::spk_client::{FullScanRequest, FullScanResponse, SyncRequest, SyncResponse},
    coin_selection::InsufficientFunds,
    Update,
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use bp::seals::txout::TxPtr;
use bp::{
    seals::txout::{BlindSeal, CloseMethod, ExplicitSeal},
    Outpoint as RgbOutpoint, ScriptPubkey, Tx,
};
use bpstd::{AddressPayload, Network as RgbNetwork};
use chacha20poly1305::{
    aead::{generic_array::GenericArray, stream},
    Key, KeyInit, XChaCha20Poly1305,
};
use commit_verify::Conceal;
#[cfg(feature = "electrum")]
use electrum::Config as BpElectrumConfig;
#[cfg(feature = "esplora")]
use esplora::Config as BpEsploraConfig;
#[cfg(any(feature = "electrum", feature = "esplora"))]
use file_format::FileFormat;
use futures::executor::block_on;
use ifaces::{
    rgb21::{EmbeddedMedia as RgbEmbeddedMedia, TokenData},
    IssuerWrapper, Rgb20, Rgb21, Rgb25,
};
use psrgbt::{
    PropKey, ProprietaryKeyRgb, Psbt as RgbPsbt, RgbExt, RgbPsbt as RgbPsbtTrait, RgbPsbtError,
};
use rand::{distributions::Alphanumeric, Rng};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use reqwest::{
    blocking::{multipart, Client as RestClient},
    header::CONTENT_TYPE,
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use rgb::resolvers::AnyResolver;
use rgb_lib_migration::{Migrator, MigratorTrait};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use rgbinvoice::{Amount, Precision};
use rgbinvoice::{Beneficiary, RgbInvoice, RgbInvoiceBuilder, XChainNet};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use rgbstd::{
    containers::IndexedConsignment, stl::ContractTerms, validation::Validity, Assign, GenesisSeal,
    Identity,
};
use rgbstd::{
    containers::{BuilderSeal, Kit, ValidContract, ValidKit, ValidTransfer},
    info::{ContractInfo, SchemaInfo},
    interface::{IfaceClass, IfaceRef, TransitionBuilder},
    invoice::{InvoiceState, Pay2Vout},
    persistence::{
        fs::FsBinStore, ContractIfaceError, MemContract, MemContractState, PersistedState,
        StashDataError, StashReadProvider, Stock, StockError,
    },
    stl::{Attachment, ProofOfReserves as RgbProofOfReserves},
    validation::{ResolveWitness, Status, WitnessResolverError},
    ChainNet, Genesis, GraphSeal, Layer1, MergeReveal, OpId, Operation, Opout, OutputSeal,
    SecretSeal, Transition,
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use rgbstd::{
    interface::ContractBuilder,
    stl::{AssetSpec, Details, MediaType, Name, RicardianContract, Ticker},
    Allocation, OwnedFraction, TokenIndex,
};
use schemata::{CollectibleFungibleAsset, NonInflatableAsset, UniqueDigitalAsset};
use scrypt::{
    password_hash::{PasswordHasher, Salt, SaltString},
    Params, Scrypt,
};
use sea_orm::{
    ActiveValue, ColumnTrait, ConnectOptions, Database, DatabaseConnection, DeriveActiveEnum,
    EntityTrait, EnumIter, IntoActiveValue, QueryFilter, QueryOrder, TryIntoModel,
};
use serde::de::{self, Unexpected, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use slog::{debug, error, info, o, warn, Drain, Logger};
use slog_async::AsyncGuard;
use slog_term::{FullFormat, PlainDecorator};
use strict_encoding::{
    tn, DecodeError, DeserializeError, FieldName, StrictDeserialize, StrictSerialize, TypeName,
};
use tempfile::TempDir;
use time::OffsetDateTime;
use typenum::consts::U32;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;

#[cfg(feature = "electrum")]
use crate::utils::INDEXER_BATCH_SIZE;
#[cfg(feature = "esplora")]
use crate::utils::INDEXER_PARALLEL_REQUESTS;
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg(test)]
use crate::wallet::test::{
    mock_asset_terms, mock_contract_details, mock_input_unspents, mock_token_data, mock_vout,
};
#[cfg(test)]
use crate::wallet::test::{mock_chain_net, skip_check_fee_rate};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use crate::{
    api::proxy::{GetConsignmentResponse, Proxy},
    database::{DbData, LocalRecipient, LocalRecipientData, LocalWitnessData},
    error::IndexerError,
    utils::{
        check_proxy, get_indexer, get_proxy_client, script_buf_from_recipient_id, OffchainResolver,
        INDEXER_RETRIES, INDEXER_STOP_GAP, INDEXER_TIMEOUT,
    },
    wallet::Indexer,
};
use crate::{
    database::{
        entities::{
            asset::{ActiveModel as DbAssetActMod, Model as DbAsset},
            asset_transfer::{ActiveModel as DbAssetTransferActMod, Model as DbAssetTransfer},
            backup_info::{ActiveModel as DbBackupInfoActMod, Model as DbBackupInfo},
            batch_transfer::{ActiveModel as DbBatchTransferActMod, Model as DbBatchTransfer},
            coloring::{ActiveModel as DbColoringActMod, Model as DbColoring},
            media::{ActiveModel as DbMediaActMod, Model as DbMedia},
            pending_witness_outpoint::{
                ActiveModel as DbPendingWitnessOutpointActMod, Model as DbPendingWitnessOutpoint,
            },
            pending_witness_script::{
                ActiveModel as DbPendingWitnessScriptActMod, Model as DbPendingWitnessScript,
            },
            token::{ActiveModel as DbTokenActMod, Model as DbToken},
            token_media::{ActiveModel as DbTokenMediaActMod, Model as DbTokenMedia},
            transfer::{ActiveModel as DbTransferActMod, Model as DbTransfer},
            transfer_transport_endpoint::{
                ActiveModel as DbTransferTransportEndpointActMod,
                Model as DbTransferTransportEndpoint,
            },
            transport_endpoint::{
                ActiveModel as DbTransportEndpointActMod, Model as DbTransportEndpoint,
            },
            txo::{ActiveModel as DbTxoActMod, Model as DbTxo},
            wallet_transaction::{
                ActiveModel as DbWalletTransactionActMod, Model as DbWalletTransaction,
            },
        },
        enums::{ColoringType, WalletTransactionType},
        LocalRgbAllocation, LocalTransportEndpoint, LocalUnspent, RgbLibDatabase, TransferData,
    },
    error::InternalError,
    utils::{
        adjust_canonicalization, beneficiary_from_script_buf, calculate_descriptor_from_xprv,
        calculate_descriptor_from_xpub, derive_account_xprv_from_mnemonic,
        from_str_or_number_mandatory, from_str_or_number_optional, get_genesis_hash,
        get_xpub_from_xprv, load_rgb_runtime, now, parse_address_str, setup_logger, RgbInExt,
        RgbOutExt, RgbPsbtExt, RgbRuntime, LOG_FILE,
    },
    wallet::{Balance, Outpoint, NUM_KNOWN_SCHEMAS, SCHEMA_ID_CFA, SCHEMA_ID_NIA, SCHEMA_ID_UDA},
};
