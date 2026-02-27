#![allow(clippy::too_many_arguments)]
#![warn(missing_docs)]

//! A library to manage wallets for RGB assets.
//!
//! ## Wallet
//! The main component of the library is the [`Wallet`].
//!
//! It allows to create and operate an RGB wallet that can issue, send and receive NIA, CFA, IFA and
//! UDA assets. The library also manages UTXOs and asset allocations.
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
//! use rgb_lib::{generate_keys, AssetSchema, BitcoinNetwork};
//!
//! fn main() -> Result<(), rgb_lib::Error> {
//!     let data_dir = tempfile::tempdir()?;
//!     let keys = generate_keys(BitcoinNetwork::Regtest);
//!     let wallet_data = WalletData {
//!         data_dir: data_dir.path().to_str().unwrap().to_string(),
//!         bitcoin_network: BitcoinNetwork::Regtest,
//!         database_type: DatabaseType::Sqlite,
//!         max_allocations_per_utxo: 5,
//!         account_xpub_vanilla: keys.account_xpub_vanilla,
//!         account_xpub_colored: keys.account_xpub_colored,
//!         mnemonic: Some(keys.mnemonic),
//!         master_fingerprint: keys.master_fingerprint,
//!         vanilla_keychain: None,
//!         supported_schemas: vec![AssetSchema::Nia],
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
    ContractId, Txid as RgbTxid,
    containers::{ConsignmentExt, Fascia, FileContent, PubWitness, Transfer as RgbTransfer},
    persistence::UpdateRes,
    schema::SchemaId,
    vm::WitnessOrd,
};

pub use crate::{
    database::enums::{AssetSchema, Assignment, TransferStatus, TransportType},
    error::Error,
    keys::{generate_keys, restore_keys},
    utils::{BitcoinNetwork, block_on},
    wallet::{RecipientType, TransactionType, TransferKind, Wallet, backup::restore_backup},
};

#[cfg(any(feature = "electrum", feature = "esplora"))]
use std::{
    cmp::{Ordering, max, min},
    collections::hash_map::DefaultHasher,
    hash::Hasher,
    num::NonZeroU32,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt, fs,
    hash::Hash,
    io::{self, ErrorKind, Read, Write},
    panic,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, LazyLock},
    time::Duration,
};

use amplify::{Wrapper, bmap, confinement::Confined, s};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use base64::{Engine as _, engine::general_purpose};
#[cfg(feature = "electrum")]
use bdk_electrum::{
    BdkElectrumClient,
    electrum_client::{Client as ElectrumClient, ElectrumApi, Error as ElectrumError, Param},
};
#[cfg(feature = "esplora")]
use bdk_esplora::{
    EsploraExt,
    esplora_client::{
        BlockingClient as EsploraClient, Builder as EsploraBuilder, Error as EsploraError,
    },
};
#[cfg(feature = "esplora")]
use bdk_wallet::bitcoin::Txid;
use bdk_wallet::{
    ChangeSet, KeychainKind, LocalOutput, PersistedWallet, SignOptions, Wallet as BdkWallet,
    bitcoin::{
        Address as BdkAddress, Amount as BdkAmount, BlockHash, Network as BdkNetwork, NetworkKind,
        OutPoint, OutPoint as BdkOutPoint, ScriptBuf, TxOut,
        bip32::{ChildNumber, DerivationPath, Fingerprint, KeySource, Xpriv, Xpub},
        constants::ChainHash,
        hashes::{Hash as Sha256Hash, sha256},
        psbt::{ExtractTxError, Psbt},
        secp256k1::Secp256k1,
    },
    chain::{CanonicalizationParams, ChainPosition},
    descriptor::Segwitv0,
    file_store::Store,
    keys::{
        DerivableKey, DescriptorKey,
        DescriptorKey::{Public, Secret},
        ExtendedKey, GeneratableKey,
        bip39::{Language, Mnemonic, WordCount},
    },
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use bdk_wallet::{
    Update,
    bitcoin::{Transaction as BdkTransaction, blockdata::fee_rate::FeeRate},
    chain::{
        DescriptorExt,
        spk_client::{FullScanRequest, FullScanResponse, SyncRequest, SyncResponse},
    },
    coin_selection::InsufficientFunds,
};
use chacha20poly1305::{
    Key, KeyInit, XChaCha20Poly1305,
    aead::{generic_array::GenericArray, stream},
};
use file_format::FileFormat;
use psrgbt::{RgbOutExt, RgbPsbtExt};
use rand::{Rng, distr::Alphanumeric};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use reqwest::{
    blocking::{Client as RestClient, multipart},
    header::CONTENT_TYPE,
};
use rgb_lib_migration::{
    ArrayType, ColumnType, Migrator, MigratorTrait, Nullable, Value, ValueType, ValueTypeErr,
};
use rgbinvoice::{AddressPayload, Beneficiary, RgbInvoice, RgbInvoiceBuilder, XChainNet};
#[cfg(feature = "electrum")]
use rgbstd::indexers::electrum_blocking::electrum_client::ConfigBuilder;
use rgbstd::{
    Allocation, Amount, ChainNet, Genesis, GraphSeal, Identity, Layer1, Operation, Opout,
    OutputSeal, OwnedFraction, Precision, Schema, SecretSeal, TokenIndex, Transition,
    TransitionType, TypeSystem,
    containers::{BuilderSeal, Kit, ValidContract, ValidKit, ValidTransfer},
    contract::{AllocatedState, ContractBuilder, IssuerWrapper, TransitionBuilder},
    info::{ContractInfo, SchemaInfo},
    invoice::{InvoiceState, Pay2Vout},
    persistence::{MemContract, MemContractState, StashReadProvider, Stock, fs::FsBinStore},
    rgbcore::commit_verify::Conceal,
    stl::{
        AssetSpec, Attachment, ContractTerms, Details, EmbeddedMedia as RgbEmbeddedMedia,
        MediaType, Name, ProofOfReserves as RgbProofOfReserves, RejectListUrl, RicardianContract,
        Ticker, TokenData,
    },
    txout::{BlindSeal, CloseMethod, ExplicitSeal},
    validation::{
        ResolveWitness, Scripts, Status, WitnessOrdProvider, WitnessResolverError, WitnessStatus,
    },
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use rgbstd::{
    Assign, KnownTransition,
    containers::Consignment,
    contract::SchemaWrapper,
    daggy::Walker,
    indexers::AnyResolver,
    txout::TxPtr,
    validation::{OpoutsDagData, ValidationConfig, ValidationError, Validity, Warning},
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use schemata::{
    CfaWrapper, IfaWrapper, NiaWrapper, OS_ASSET, OS_INFLATION, OS_REPLACE, UdaWrapper,
};
use schemata::{
    CollectibleFungibleAsset, InflatableFungibleAsset, NonInflatableAsset, UniqueDigitalAsset,
};
use scrypt::{
    Params, Scrypt,
    password_hash::{PasswordHasher, Salt, SaltString, rand_core::OsRng},
};
use sea_orm::{
    ActiveValue, ColumnTrait, ConnectOptions, Database, DatabaseConnection, DbErr,
    DeriveActiveEnum, EntityTrait, EnumIter, IntoActiveValue, JsonValue, QueryFilter, QueryOrder,
    QueryResult, TryGetError, TryGetable, TryIntoModel,
};
use serde::de::{self, Unexpected, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use slog::{Drain, Logger, debug, error, info, o, warn};
use slog_async::AsyncGuard;
use slog_term::{FullFormat, PlainDecorator};
use strict_encoding::{DecodeError, DeserializeError, FieldName};
use tempfile::TempDir;
use time::OffsetDateTime;
use typenum::consts::U32;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;

#[cfg(feature = "electrum")]
use crate::utils::INDEXER_BATCH_SIZE;
#[cfg(feature = "esplora")]
use crate::utils::INDEXER_PARALLEL_REQUESTS;
#[cfg(test)]
use crate::wallet::test::{mock_asset_terms, mock_contract_details, mock_token_data};
#[cfg(test)]
use crate::wallet::test::{mock_chain_net, skip_build_dag, skip_check_fee_rate};
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg(test)]
use crate::wallet::test::{mock_input_unspents, mock_vout};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use crate::{
    api::proxy::{GetConsignmentResponse, Proxy},
    api::reject_list::RejectList,
    database::{DbData, LocalRecipient, LocalRecipientData, LocalWitnessData},
    error::IndexerError,
    utils::{
        INDEXER_STOP_GAP, OffchainResolver, check_proxy, get_indexer_and_resolver, get_rest_client,
        script_buf_from_recipient_id,
    },
    wallet::{AssignmentsCollection, Indexer},
};
use crate::{
    database::{
        LocalRgbAllocation, LocalTransportEndpoint, LocalUnspent, RgbLibDatabase, TransferData,
        entities::{
            asset::{ActiveModel as DbAssetActMod, Model as DbAsset},
            asset_transfer::{ActiveModel as DbAssetTransferActMod, Model as DbAssetTransfer},
            backup_info::{ActiveModel as DbBackupInfoActMod, Model as DbBackupInfo},
            batch_transfer::{ActiveModel as DbBatchTransferActMod, Model as DbBatchTransfer},
            coloring::{ActiveModel as DbColoringActMod, Model as DbColoring},
            media::{ActiveModel as DbMediaActMod, Model as DbMedia},
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
        enums::{ColoringType, RecipientTypeFull, WalletTransactionType},
    },
    error::InternalError,
    utils::{
        DumbResolver, LOG_FILE, RgbRuntime, adjust_canonicalization, beneficiary_from_script_buf,
        from_str_or_number_mandatory, from_str_or_number_optional, get_account_xpubs,
        get_descriptors, get_descriptors_from_xpubs, load_rgb_runtime, now, parse_address_str,
        setup_logger, str_to_xpub,
    },
    wallet::{
        Balance, NUM_KNOWN_SCHEMAS, Outpoint, SCHEMA_ID_CFA, SCHEMA_ID_IFA, SCHEMA_ID_NIA,
        SCHEMA_ID_UDA,
    },
};
