#![allow(clippy::too_many_arguments)]
#![deny(missing_docs)]

//! A library to manage wallets for RGB assets.
//!
//! ## Wallet
//! The main components of the library are the [`wallet::Wallet`] and [`wallet::MultisigWallet`].
//!
//! They allow to create and operate RGB wallets that can issue and operate on NIA, CFA, IFA and
//! UDA assets. The library also manages UTXOs and asset allocations.
//!
//! ## Backend
//! The library uses BDK for walleting operations and several components from the RGB ecosystem for
//! RGB asset operations.
//!
//! ## Database
//! A SQLite database is used to persist data to disk, including the BDK wallet data.
//!
//! Database support is designed in order to support multiple database backends. At the moment only
//! SQLite is supported but adding more should be relatively easy.
//!
//! ## External services
//! The library uses an external indexer to get information about the Bitcoin network.
//! At the moment Electrum and Esplora are supported.
//!
//! RGB asset operations may require the exchange of off-chain data in the form of consignment or
//! media files with a counterparty. To do so the library is able to interact with a proxy server to
//! support these data exchanges between the parties.
//!
//! The library also supports the use of a multisig hub to support multisig wallets and the use
//! of a reject list service.
//!
//! ## Errors
//! Errors are handled with the crate `thiserror`.
//!
//! ## FFI
//! Library functionality is exposed for other languages via FFI bindings.
//!
//! ## Examples
//! ### Create an RGB singlesig wallet
//! ```
//! use rgb_lib::keys::{WitnessVersion, generate_keys};
//! use rgb_lib::wallet::{DatabaseType, SinglesigKeys, Wallet, WalletData};
//! use rgb_lib::{AssetSchema, BitcoinNetwork};
//!
//! fn main() -> Result<(), rgb_lib::Error> {
//!     let data_dir = tempfile::tempdir()?;
//!     let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
//!     let single_sig_keys = SinglesigKeys::from_keys(&keys, None);
//!     let wallet_data = WalletData {
//!         data_dir: data_dir.path().to_str().unwrap().to_string(),
//!         bitcoin_network: BitcoinNetwork::Regtest,
//!         database_type: DatabaseType::Sqlite,
//!         max_allocations_per_utxo: 5,
//!         supported_schemas: vec![AssetSchema::Nia],
//!     };
//!     let wallet = Wallet::new(wallet_data, single_sig_keys)?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ### Load an existing RGB singlesig wallet
//! [`wallet::Wallet::new`] records the wallet settings in the wallet directory, so an existing
//! wallet can be re-opened with just its location, its master fingerprint and its mnemonic.
//! Passing no mnemonic loads the wallet in watch-only mode.
//! ```
//! use rgb_lib::keys::{WitnessVersion, generate_keys};
//! use rgb_lib::wallet::{DatabaseType, SinglesigKeys, Wallet, WalletData};
//! use rgb_lib::{AssetSchema, BitcoinNetwork};
//!
//! fn main() -> Result<(), rgb_lib::Error> {
//!     let data_dir = tempfile::tempdir()?;
//!     let data_dir = data_dir.path().to_str().unwrap();
//!     let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
//!     let wallet_data = WalletData {
//!         data_dir: data_dir.to_string(),
//!         bitcoin_network: BitcoinNetwork::Regtest,
//!         database_type: DatabaseType::Sqlite,
//!         max_allocations_per_utxo: 5,
//!         supported_schemas: vec![AssetSchema::Nia],
//!     };
//!     let wallet = Wallet::new(wallet_data, SinglesigKeys::from_keys(&keys, None))?;
//!     drop(wallet);
//!
//!     let wallet = Wallet::load(
//!         data_dir,
//!         &keys.master_fingerprint,
//!         Some(keys.mnemonic.clone()),
//!     )?;
//!
//!     Ok(())
//! }
//! ```

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) mod api;
pub(crate) mod database;
pub(crate) mod error;
pub mod keys;
pub mod utils;
pub mod wallet;

pub use bdk_wallet;
pub use bdk_wallet::bitcoin;
pub use rgbstd::{
    ContractId, Txid as RgbTxid,
    containers::{ConsignmentExt, Fascia, FileContent, Transfer as RgbTransfer, WitnessBundle},
    persistence::{RetrievedSpvProofs, UpdateRes},
    schema::SchemaId,
    txout::CloseMethod,
    vm::WitnessOrd,
};

pub use crate::{
    database::enums::{
        AssetSchema, Assignment, TransferStatus, TransportType, WalletTransactionType,
    },
    error::Error,
    utils::{BitcoinNetwork, block_on},
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
    io::{self, Read, Write},
    panic,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use amplify::{
    Bytes32, Wrapper, bmap,
    confinement::{Confined, MediumOrdMap},
    s,
};
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
#[cfg(feature = "bdk_file_store_migration")]
use bdk_wallet::file_store::Store;
use bdk_wallet::{
    ChangeSet, KeychainKind, LocalOutput, SignOptions, Wallet as BdkWallet,
    bitcoin::{
        Address as BdkAddress, Amount as BdkAmount, BlockHash, Network as BdkNetwork, NetworkKind,
        OutPoint, OutPoint as BdkOutPoint, ScriptBuf, Transaction as BdkTransaction, TxOut, Txid,
        bip32::{ChildNumber, DerivationPath, Fingerprint, KeySource, Xpriv, Xpub},
        consensus::{Decodable, Encodable},
        hashes::{Hash as Sha256Hash, sha256},
        psbt::{ExtractTxError, Psbt},
        secp256k1::Secp256k1,
    },
    chain::{
        BlockId, CanonicalizationParams, ChainPosition, ConfirmationBlockTime, DescriptorId, Merge,
    },
    descriptor::{IntoWalletDescriptor, Segwitv0},
    keys::{
        DerivableKey, DescriptorKey,
        DescriptorKey::{Public, Secret},
        ExtendedKey, GeneratableKey,
        bip39::{Language, Mnemonic, WordCount},
    },
    miniscript::{Descriptor, DescriptorPublicKey},
    signer::SignersContainer,
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use bdk_wallet::{
    Update,
    bitcoin::{blockdata::fee_rate::FeeRate, hashes::HashEngine},
    chain::{
        DescriptorExt,
        spk_client::{FullScanRequest, FullScanResponse, SyncRequest, SyncResponse},
    },
    coin_selection::InsufficientFunds,
};
use chacha20poly1305::{Key, KeyInit, XChaCha20Poly1305, XNonce, aead::Aead};
use file_format::FileFormat;
use psrgbt::{RgbOutExt, RgbPsbtExt};
use rand::{RngExt, distr::Alphanumeric};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use reqwest::{
    blocking::{Client as RestClient, multipart},
    header::CONTENT_TYPE,
};
use rgb_lib_migration::{
    ArrayType, ColumnType, Migrator, MigratorTrait, Nullable, Value, ValueTypeErr,
};
use rgbinvoice::{
    AddressPayload, Beneficiary, RgbInvoice, RgbInvoiceBuilder, RgbTransport, XChainNet,
};
#[cfg(feature = "electrum")]
use rgbstd::indexers::electrum_blocking::electrum_client::ConfigBuilder;
use rgbstd::{
    Allocation, Amount, AssignmentType, BundleId, ChainNet, Genesis, GlobalStateType, GraphSeal,
    Identity, KnownTransition, Layer1, OpId, Operation as _, Opout, OutputSeal, OwnedFraction,
    Precision, RevealedData, Schema, SecretSeal, TokenIndex, Transition, TransitionBundle,
    TransitionType, Vout,
    containers::{
        BuilderSeal, SealWitness, SpvProof, ValidConsignment, ValidContract, ValidTransfer,
    },
    contract::{
        AllocatedState, ContractBuilder, ContractData, FilteredContractState, IssuerWrapper,
        SchemaWrapper, TransitionBuilder,
    },
    info::SchemaInfo,
    invoice::{InvoiceState, Pay2Vout},
    persistence::{
        AllocKind, AllocSeal, AllocationFilter, AllocationRow, AllocationWrite,
        ContractAssignments, ContractStateSnapshot, GlobalStateRow, GlobalStateWrite, Lib, LibId,
        RgbStore, Stock, StockError, TxBegin, TxMode, Visibility, decode, encode,
    },
    rgbcore::commit_verify::{
        CommitId, Conceal, TryCommitVerify,
        mpc::{Commitment, MerkleTree, Message, MultiSource, ProtocolId},
    },
    stl::{
        AssetSpec, Attachment, ContractTerms, Details, EmbeddedMedia as RgbEmbeddedMedia,
        MediaType, Name, ProofOfReserves as RgbProofOfReserves, RejectListUrl, RicardianContract,
        Ticker, TokenData,
    },
    txout::{BlindSeal, ExplicitSeal, TxPtr},
    validation::{
        ResolveWitness, SchemaDefinition, WitnessOrdProvider, WitnessResolverError, WitnessStatus,
    },
    vm::WitnessPos,
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use rgbstd::{
    containers::Consignment,
    contract::FilterIncludeAll,
    daggy::Walker,
    indexers::AnyResolver,
    validation::{
        OpoutsDagData, SchemaRules, ValidationConfig, ValidationError, Validity, Warning,
    },
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use schemata::{CfaWrapper, NiaWrapper, UdaWrapper};
use schemata::{
    CollectibleFungibleAsset, IfaWrapper, InflatableFungibleAsset, NonInflatableAsset, OS_ASSET,
    OS_INFLATION, TS_BURN, TS_INFLATION, TS_TRANSFER, UniqueDigitalAsset,
};
use scrypt::{Params, phc::Salt, scrypt};
use sea_orm::{
    ActiveValue, ColumnTrait, ConnectOptions, Database, DatabaseConnection, DatabaseTransaction,
    DbErr, DeriveActiveEnum, EntityTrait, EnumIter, Iterable, JoinType, JsonValue, QueryFilter,
    QueryOrder, QueryResult, QuerySelect, TransactionTrait, TryGetError, TryGetable, TryIntoModel,
    sea_query::{Alias, Condition, Expr, ExprTrait, Func, OnConflict, SimpleExpr},
};
use serde::de::{self, Unexpected, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use slog::warn;
use slog::{Drain, Logger, debug, error, info, o};
use slog_async::AsyncGuard;
use slog_term::{FullFormat, PlainDecorator};
#[cfg(any(
    feature = "electrum",
    feature = "esplora",
    feature = "rgb_fs_store_migration"
))]
use strict_encoding::StrictSerialize;
use strict_encoding::{FieldName, StrictDecode, StrictDeserialize, StrictEncode};
use strict_types::{TypeLib, TypeLibId};
use tempfile::TempDir;
use time::OffsetDateTime;
#[cfg(any(feature = "electrum", feature = "esplora"))]
use url::Url;
use walkdir::WalkDir;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

#[cfg(feature = "electrum")]
use crate::utils::INDEXER_BATCH_SIZE;
#[cfg(feature = "esplora")]
use crate::utils::INDEXER_PARALLEL_REQUESTS;
#[cfg(any(feature = "electrum", feature = "esplora"))]
use crate::utils::write_fascia;
#[cfg(any(feature = "electrum", feature = "esplora"))]
#[cfg(test)]
use crate::wallet::test::{
    mock_input_unspents, mock_local_version, mock_send_end_crash, mock_vout, skip_build_dag,
    skip_check_fee_rate, strip_terminals,
};
#[cfg(any(feature = "electrum", feature = "esplora"))]
use crate::{
    api::{
        multisig_hub::{
            FileMetadata, FileSource, FileType, InfoResponse, MultisigHubClient, OperationResponse,
            OperationStatus, OperationType, UserRoleResponse,
        },
        proxy::{GetConsignmentResponse, ProxyClient},
        reject_list::RejectListClient,
    },
    database::{
        DbData,
        entities::{
            pending_witness_script::Model as DbPendingWitnessScript,
            reserved_txo::ActiveModel as DbReservedTxoActMod,
            wallet_transaction::ActiveModel as DbWalletTransactionActMod,
        },
    },
    error::IndexerError,
    utils::{
        INDEXER_STOP_GAP, OffchainResolver, check_proxy, get_indexer_and_resolver, hash_file,
        script_buf_from_recipient_id,
    },
    wallet::{AssignmentsCollection, Indexer, multisig::RespondToOperation},
};
use crate::{
    database::rgb::migrate_rgb_fs_to_db,
    database::{
        DbTxn, RgbLibDatabase,
        entities::{
            asset::{ActiveModel as DbAssetActMod, Model as DbAsset},
            asset_transfer::{ActiveModel as DbAssetTransferActMod, Model as DbAssetTransfer},
            backup_info::{ActiveModel as DbBackupInfoActMod, Model as DbBackupInfo},
            batch_transfer::{ActiveModel as DbBatchTransferActMod, Model as DbBatchTransfer},
            coloring::{ActiveModel as DbColoringActMod, Model as DbColoring},
            media::{ActiveModel as DbMediaActMod, Model as DbMedia},
            pending_witness_script::ActiveModel as DbPendingWitnessScriptActMod,
            reserved_txo::Model as DbReservedTxo,
            rgb_aluvm_lib::ActiveModel as DbAluvmLibActMod,
            rgb_assignment::{ActiveModel as DbAssignmentActMod, Model as DbAssignment},
            rgb_bundle::ActiveModel as DbBundleActMod,
            rgb_bundle_contract::ActiveModel as DbBundleContractActMod,
            rgb_bundle_witness::ActiveModel as DbBundleWitnessActMod,
            rgb_genesis::{ActiveModel as DbGenesisActMod, Model as DbGenesis},
            rgb_global_state::{ActiveModel as DbGlobalStateActMod, Model as DbGlobalState},
            rgb_invalid_op::ActiveModel as DbInvalidOpActMod,
            rgb_op_bundle::ActiveModel as DbOpBundleActMod,
            rgb_op_input::{ActiveModel as DbOpInputActMod, Model as DbOpInput},
            rgb_outpoint_opout::ActiveModel as DbOutpointOpoutActMod,
            rgb_schema::{ActiveModel as DbSchemaActMod, Model as DbSchema},
            rgb_schema_type_lib::ActiveModel as DbSchemaTypeLibActMod,
            rgb_secret_seal::ActiveModel as DbSecretSealActMod,
            rgb_secret_seal_opout::ActiveModel as DbSecretSealOpoutActMod,
            rgb_type_lib::ActiveModel as DbTypeLibActMod,
            rgb_witness::{ActiveModel as DbWitnessActMod, Model as DbWitness},
            rgb_witness_ord::{ActiveModel as DbWitnessOrdActMod, Model as DbWitnessOrd},
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
            wallet_transaction::Model as DbWalletTransaction,
        },
        enums::{ColoringType, RecipientTypeFull},
    },
    error::InternalError,
    keys::{Keys, WitnessVersion},
    utils::{
        ACCOUNT, DumbResolver, KEYCHAIN_BTC, KEYCHAIN_RGB, LOG_FILE, PURPOSE,
        RGB_RUNTIME_DELETING_DIR, RgbStock, adjust_canonicalization, atomic_tmp_path, atomic_write,
        atomic_write_with, beneficiary_from_script_buf, from_str_or_number_mandatory,
        from_str_or_number_optional, get_account_xpubs, get_coin_type, get_descriptors,
        get_descriptors_from_xpubs, hash_bytes, hash_bytes_hex, now, parse_address_str,
        read_fascia, setup_logger, str_to_xpub,
    },
    wallet::{
        Balance, LocalRgbAllocation, LocalUnspent, NUM_KNOWN_SCHEMAS, Outpoint, SCHEMA_ID_CFA,
        SCHEMA_ID_IFA, SCHEMA_ID_NIA, SCHEMA_ID_UDA, WalletDescriptors,
    },
};
#[cfg(test)]
use crate::{
    keys::generate_keys,
    wallet::test::{mock_asset_terms, mock_chain_net, mock_contract_details, mock_token_data},
};
