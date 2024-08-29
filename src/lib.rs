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

pub use bdk;
pub use bitcoin;
pub use invoice::RgbTransport;
pub use rgbstd::{
    containers::{Contract, Fascia, FileContent, Transfer as RgbTransfer},
    contract::ContractId,
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
    s, ByteArray, FromSliceError,
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use amplify::{hex::ToHex, none, Wrapper};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use base64::{engine::general_purpose, Engine as _};
#[cfg(feature = "electrum")]
use bdk::blockchain::electrum::ElectrumBlockchainConfig;
#[cfg(feature = "esplora")]
use bdk::blockchain::esplora::{
    EsploraBlockchain as EsploraClient, EsploraBlockchainConfig, EsploraError,
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use bdk::{
    bitcoin::Transaction as BdkTransaction,
    blockchain::{
        any::{AnyBlockchain, AnyBlockchainConfig},
        Blockchain, ConfigurableBlockchain,
    },
    database::BatchDatabase,
    database::MemoryDatabase,
    descriptor::IntoWalletDescriptor,
    FeeRate, SyncOptions,
};
use bdk::{
    bitcoin::{
        bip32::{DerivationPath, ExtendedPrivKey, ExtendedPubKey, KeySource},
        psbt::Psbt as BdkPsbt,
        secp256k1::Secp256k1,
        Address as BdkAddress, Network as BdkNetwork, OutPoint as BdkOutPoint,
    },
    database::{
        any::SledDbConfiguration, AnyDatabase, ConfigurableDatabase as BdkConfigurableDatabase,
    },
    descriptor::Segwitv0,
    keys::{
        bip39::{Language, Mnemonic, WordCount},
        DerivableKey, DescriptorKey,
        DescriptorKey::{Public, Secret},
        DescriptorSecretKey, ExtendedKey, GeneratableKey,
    },
    miniscript::DescriptorPublicKey,
    wallet::AddressIndex,
    BlockTime, KeychainKind, LocalUtxo, SignOptions, Wallet as BdkWallet,
};
use bitcoin::{
    bip32::ChildNumber,
    hashes::Hash as Sha256Hash,
    psbt::{raw::ProprietaryKey, Input, Output, PartiallySignedTransaction},
    Address, OutPoint, ScriptBuf, TxOut,
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use bitcoin::{hashes::sha256, Txid};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use bp::seals::txout::{ChainBlindSeal, TxPtr};
use bp::{
    seals::txout::{BlindSeal, CloseMethod, ExplicitSeal},
    Outpoint as RgbOutpoint, ScriptPubkey, Txid as BpTxid,
};
use bpstd::{AddressPayload, Network as RgbNetwork};
use chacha20poly1305::{
    aead::{generic_array::GenericArray, stream},
    Key, KeyInit, XChaCha20Poly1305,
};
use commit_verify::Conceal;
#[cfg(feature = "electrum")]
use electrum_client::{Client as ElectrumClient, ConfigBuilder, ElectrumApi, Param};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use file_format::FileFormat;
use futures::executor::block_on;
#[cfg(any(feature = "electrum", feature = "esplora"))]
use invoice::{Amount, Precision};
use invoice::{Beneficiary, RgbInvoice, RgbInvoiceBuilder, XChainNet};
use psbt::{PropKey, ProprietaryKeyRgb, Psbt as RgbPsbt, RgbPsbt as RgbPsbtTrait};
use rand::{distributions::Alphanumeric, Rng};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use reqwest::{
    blocking::{multipart, Client as RestClient},
    header::CONTENT_TYPE,
};
use rgb::{
    validation::Status, BlindingFactor, Genesis, Layer1, OpId, Opout, SchemaId, SubSchema,
    Transition, WitnessId, XChain, XOutpoint, XOutputSeal,
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use rgb::{validation::Validity, Assign};
use rgb_lib_migration::{Migrator, MigratorTrait};
#[cfg(feature = "electrum")]
use rgb_rt::electrum::Resolver as ElectrumResolver;
#[cfg(feature = "esplora")]
use rgb_rt::esplora_blocking::Resolver as EsploraResolver;
#[cfg(any(feature = "electrum", feature = "esplora"))]
use rgb_rt::AnyResolver;
use rgb_schemata::{cfa_rgb25, cfa_schema, uda_rgb21, uda_schema, NonInflatableAsset};
use rgbfs::StockFs;
use rgbstd::{
    accessors::MergeReveal,
    containers::BuilderSeal,
    containers::CloseMethodSet,
    contract::GraphSeal,
    interface::{
        ContractIface, Iface, IfaceClass, IfaceId, IfaceImpl, IssuerClass, Rgb20, Rgb21, Rgb25,
        TransitionBuilder,
    },
    invoice::{ChainNet, InvoiceState},
    persistence::{Inventory, PersistedState, Stash, Stock},
    resolvers::ResolveHeight,
    stl::{Attachment, MediaType},
    Operation, Txid as RgbTxid,
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use rgbstd::{
    contract::GenesisSeal,
    interface::{
        rgb21::{Allocation, OwnedFraction, TokenData, TokenIndex},
        ContractBuilder,
    },
    stl::{AssetSpec, AssetTerms, Details, Name, RicardianContract, Ticker},
};
use scrypt::{
    password_hash::{PasswordHasher, Salt, SaltString},
    Params, Scrypt,
};
use sea_orm::{
    ActiveValue, ColumnTrait, ConnectOptions, Database, DatabaseConnection, DeriveActiveEnum,
    EntityTrait, EnumIter, IntoActiveValue, QueryFilter, QueryOrder, TryIntoModel,
};
use seals::SecretSeal;
use serde::{Deserialize, Serialize};
use slog::{debug, error, info, o, warn, Drain, Logger};
use slog_async::AsyncGuard;
use slog_term::{FullFormat, PlainDecorator};
use strict_encoding::{
    tn, DecodeError, DeserializeError, FieldName, StrictDeserialize, StrictSerialize, TypeName,
};
use strict_types::StrictVal;
use tempfile::TempDir;
use time::OffsetDateTime;
use typenum::consts::U32;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;

#[cfg(feature = "electrum")]
use crate::utils::get_valid_txid_for_network;
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg(test)]
use crate::wallet::test::{
    mock_asset_terms, mock_contract_details, mock_input_unspents, mock_token_data,
};
#[cfg(test)]
use crate::wallet::test::{mock_chain_net, skip_check_fee_rate};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use crate::{
    api::proxy::{GetConsignmentResponse, Proxy},
    database::{DbData, LocalRecipient, LocalRecipientData, LocalWitnessData},
    utils::get_genesis_hash,
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
        calculate_descriptor_from_xpub, derive_account_xprv_from_mnemonic, get_xpub_from_xprv,
        load_rgb_runtime, now, setup_logger, RgbInExt, RgbOutExt, RgbPsbtExt, RgbRuntime, LOG_FILE,
    },
    wallet::{Balance, Outpoint, NUM_KNOWN_SCHEMAS, SCHEMA_ID_CFA, SCHEMA_ID_NIA, SCHEMA_ID_UDA},
};
