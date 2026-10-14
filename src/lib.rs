#![allow(clippy::too_many_arguments)]
#![deny(missing_docs)]

//! A library to manage wallets for RGB assets.
//!
//! ## Wallet
//! The main components of the library are the
#![cfg_attr(
    feature = "wallet",
    doc = "[`wallet::Wallet`] and [`wallet::MultisigWallet`]."
)]
#![cfg_attr(
    not(feature = "wallet"),
    doc = "`wallet::Wallet` and `wallet::MultisigWallet`."
)]
//!
//! They allow to create and operate RGB wallets that can issue and operate on NIA, CFA, IFA, BFA
//! and UDA assets. The library also manages UTXOs and asset allocations.
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
//! ## Features
//! The wallet functionality requires the `wallet` feature (enabled by default and by all the
//! indexer features: `bitcoind`, `electrum` and `esplora`). Without it the library only provides
//! the APIs that don't need a wallet, like [`no_wallet::inspect_consignment`], depending on few
//! crates other than the RGB ones. The `validation` feature adds the consignment validation with a
//! witness resolver provided by the caller, `no_wallet::validate_consignment_with_resolver`, and
//! an HTTP client (to confirm mints and read reject lists). Validating with an indexer, like
//! `no_wallet::validate_consignment`, requires an indexer validation feature instead
//! (`bitcoind_validation`, `electrum_validation` or `esplora_validation`, enabled by the
//! corresponding indexer feature), which also adds the indexer client. None of them enables the
//! `wallet` feature.
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
//! # #[cfg(feature = "wallet")]
//! use rgb_lib::keys::{WitnessVersion, generate_keys};
//! # #[cfg(feature = "wallet")]
//! use rgb_lib::wallet::{DatabaseType, SinglesigKeys, Wallet, WalletData};
//! # #[cfg(feature = "wallet")]
//! use rgb_lib::{AssetSchema, BitcoinNetwork};
//!
//! # #[cfg(not(feature = "wallet"))]
//! # fn main() {}
//! # #[cfg(feature = "wallet")]
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
#![cfg_attr(feature = "wallet", doc = "[`wallet::Wallet::new`]")]
#![cfg_attr(not(feature = "wallet"), doc = "`wallet::Wallet::new`")]
//! records the wallet settings in the wallet directory, so an existing
//! wallet can be re-opened with just its location, its master fingerprint and its mnemonic.
//! Passing no mnemonic loads the wallet in watch-only mode.
//! ```
//! # #[cfg(feature = "wallet")]
//! use rgb_lib::keys::{WitnessVersion, generate_keys};
//! # #[cfg(feature = "wallet")]
//! use rgb_lib::wallet::{DatabaseType, SinglesigKeys, Wallet, WalletData};
//! # #[cfg(feature = "wallet")]
//! use rgb_lib::{AssetSchema, BitcoinNetwork};
//!
//! # #[cfg(not(feature = "wallet"))]
//! # fn main() {}
//! # #[cfg(feature = "wallet")]
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

// the `_online` feature is implied by each indexer feature and stands for "this build can reach
// the network": enabling it on its own leaves no indexer to talk to
#[cfg(all(
    feature = "_online",
    not(any(feature = "bitcoind", feature = "electrum", feature = "esplora"))
))]
compile_error!(
    "the `_online` feature cannot be enabled on its own: enable one of `bitcoind`, `electrum` or \
     `esplora` instead"
);
// the `_validation` feature is implied by each indexer validation feature and stands for "this
// build can validate consignments reaching the network": enabling it on its own leaves no indexer
// to talk to
#[cfg(all(
    feature = "_validation",
    not(any(
        feature = "bitcoind_validation",
        feature = "electrum_validation",
        feature = "esplora_validation"
    ))
))]
compile_error!(
    "the `_validation` feature cannot be enabled on its own: enable one of \
     `bitcoind_validation`, `electrum_validation` or `esplora_validation` instead"
);

// defined here, rather than with the wallet test helpers, to be available to the tests of every
// feature set
/// Panic if the given expression doesn't match the provided pattern, logging the unexpected result
#[cfg(test)]
#[macro_export]
macro_rules! assert_matches {
    ($expression:expr, $pattern:pat $(if $guard:expr)? $(,)?) => {{
        #[allow(unreachable_patterns)]
        match $expression {
            $pattern $(if $guard)? => {},
            _ => {
                panic!("received unexpected result: {}", format!("{:?}", $expression));
            }
        }
    }};
}

#[cfg(feature = "validation")]
pub(crate) mod api;
#[cfg(feature = "wallet")]
pub(crate) mod database;
// without the wallet only the database enums are needed, as some are shared with inspection
#[cfg(not(feature = "wallet"))]
pub(crate) mod database {
    use super::*;

    pub(crate) mod enums;
}
#[cfg(feature = "wallet")]
pub(crate) mod encryption;
pub(crate) mod error;
#[cfg(feature = "wallet")]
pub mod keys;
pub mod no_wallet;
#[cfg(feature = "wallet")]
pub mod utils;
#[cfg(feature = "wallet")]
pub mod wallet;

#[cfg(feature = "wallet")]
pub use bdk_wallet;
#[cfg(feature = "wallet")]
pub use bdk_wallet::bitcoin;
#[cfg(not(feature = "wallet"))]
pub use rgbstd::bitcoin;
pub use rgbstd::{
    ContractId, Txid as RgbTxid,
    containers::{ConsignmentExt, Fascia, FileContent, Transfer as RgbTransfer, WitnessBundle},
    indexers::ResolveSpvProof,
    persistence::{RetrievedSpvProofs, UpdateRes},
    schema::SchemaId,
    txout::CloseMethod,
    validation::{ResolveWitness, WitnessResolverError, WitnessStatus},
    vm::WitnessOrd,
};

pub use crate::{
    database::enums::{AssetSchema, Assignment, TypeOfTransition},
    error::Error,
    no_wallet::BitcoinNetwork,
};
#[cfg(feature = "wallet")]
pub use crate::{
    database::enums::{Encryption, TransferStatus, TransportType, WalletTransactionType},
    utils::block_on,
};

#[cfg(any(feature = "wallet", feature = "bitcoind_validation"))]
use std::path::PathBuf;
#[cfg(feature = "validation")]
use std::{cell::RefCell, cmp::max, num::NonZeroU32};
#[cfg(feature = "_online")]
use std::{cmp::Ordering, cmp::min, collections::hash_map::DefaultHasher, hash::Hasher};
#[cfg(feature = "wallet")]
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    hash::Hash,
    io::{self, Write},
    panic,
    sync::{Arc, LazyLock, Mutex},
};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    path::Path,
    str::FromStr,
};
#[cfg(any(feature = "wallet", feature = "validation"))]
use std::{io::Read, time::Duration};

#[cfg(any(feature = "wallet", feature = "validation"))]
use amplify::s;
use amplify::{ByteArray, Wrapper};
#[cfg(feature = "wallet")]
use amplify::{
    Bytes32, bmap,
    confinement::{Confined, MediumOrdMap},
};
#[cfg(feature = "_online")]
use base64::{Engine as _, engine::general_purpose};
#[cfg(feature = "bitcoind")]
use bdk_bitcoind_rpc::{Emitter as BitcoindEmitter, bip158::FilterIter as BitcoindFilterIter};
#[cfg(feature = "electrum")]
use bdk_electrum::BdkElectrumClient;
#[cfg(feature = "esplora")]
use bdk_esplora::EsploraExt;
#[cfg(feature = "_online")]
use bdk_wallet::Update;
#[cfg(any(feature = "electrum", feature = "esplora"))]
use bdk_wallet::chain::spk_client::{FullScanRequest, FullScanResponse, SyncRequest, SyncResponse};
#[cfg(feature = "bdk_file_store_migration")]
use bdk_wallet::file_store::Store;
#[cfg(feature = "wallet")]
use bdk_wallet::{
    ChangeSet, KeychainKind, LocalOutput, SignOptions, Wallet as BdkWallet,
    bitcoin::{
        Address as BdkAddress, Amount as BdkAmount, BlockHash, Network as BdkNetwork, NetworkKind,
        OutPoint as BdkOutPoint, ScriptBuf, TxOut,
        bip32::{ChildNumber, DerivationPath, Fingerprint, KeySource, Xpriv, Xpub},
        consensus::{Decodable, Encodable},
        hashes::Hash as Sha256Hash,
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
#[cfg(feature = "_online")]
use bdk_wallet::{
    bitcoin::{
        blockdata::fee_rate::FeeRate,
        hashes::{HashEngine, sha256},
    },
    chain::DescriptorExt,
    coin_selection::InsufficientFunds,
};
#[cfg(feature = "wallet")]
use chacha20poly1305::{Key, KeyInit, XChaCha20Poly1305, XNonce, aead::Aead};
#[cfg(feature = "wallet")]
use ed25519_dalek::SigningKey;
#[cfg(feature = "_online")]
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
#[cfg(feature = "wallet")]
use file_format::FileFormat;
#[cfg(feature = "_online")]
use hkdf::Hkdf;
#[cfg(feature = "_online")]
use hmac::{Mac, SimpleHmac};
#[cfg(feature = "wallet")]
use psrgbt::{RgbOutExt, RgbPsbtExt};
#[cfg(feature = "wallet")]
use rand::{RngExt, TryRng, distr::Alphanumeric, rngs::SysRng};
#[cfg(feature = "_online")]
use reqwest::blocking::multipart;
#[cfg(feature = "validation")]
use reqwest::{
    StatusCode,
    blocking::{Client as RestClient, Response},
    header::CONTENT_TYPE,
};
#[cfg(feature = "wallet")]
use rgb_lib_migration::{
    ArrayType, ColumnType, Migrator, MigratorTrait, Nullable, Value, ValueTypeErr,
};
#[cfg(feature = "wallet")]
use rgbinvoice::{AddressPayload, RgbInvoice, RgbInvoiceBuilder, RgbTransport};
use rgbinvoice::{Beneficiary, XChainNet};
#[cfg(feature = "validation")]
use rgbstd::bitcoin::block::Header as BlockHeader;
#[cfg(feature = "_validation")]
use rgbstd::indexers::AnyResolver;
#[cfg(feature = "bitcoind")]
use rgbstd::indexers::bitcoind_blocking::bitcoincore_rpc::jsonrpc::Error as BitcoindJsonRpcError;
#[cfg(feature = "bitcoind_validation")]
use rgbstd::indexers::bitcoind_blocking::bitcoincore_rpc::{
    Auth as BitcoindRpcAuth, Client as BitcoindRpcClient, Error as BitcoindRpcError, RpcApi,
    json::IndexStatus as BitcoindIndexStatus,
};
#[cfg(feature = "electrum")]
use rgbstd::indexers::electrum_blocking::electrum_client::Param;
#[cfg(feature = "electrum_validation")]
use rgbstd::indexers::electrum_blocking::electrum_client::{
    Client as ElectrumClient, Config as ElectrumConfig, ConfigBuilder, ElectrumApi,
    Error as ElectrumError,
};
#[cfg(feature = "esplora_validation")]
use rgbstd::indexers::esplora_blocking::esplora_client::{
    BlockingClient as EsploraClient, Builder as EsploraBuilder, Error as EsploraError,
};
#[cfg(feature = "wallet")]
use rgbstd::{
    Allocation, BundleId, Identity, Layer1, OutputSeal, OwnedFraction, RevealedData, TokenIndex,
    Vout,
    containers::{SealWitness, SpvProof},
    contract::{
        AllocatedState, ContractBuilder, ContractData, FilteredContractState, SchemaWrapper,
        TransitionBuilder,
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
        mpc::{MerkleTree, Message, MultiSource, ProtocolId},
    },
    stl::{
        Attachment, ContractTerms, EmbeddedMedia as RgbEmbeddedMedia, MediaType,
        ProofOfReserves as RgbProofOfReserves, RicardianContract, Ticker, TokenData,
    },
    txout::{BlindSeal, ExplicitSeal, TxPtr},
    validation::{SchemaDefinition, WitnessOrdProvider},
    vm::WitnessPos,
};
use rgbstd::{
    Amount, AssignmentType, Assignments, ChainNet, ExposedSeal, Genesis, GlobalStateType,
    GraphSeal, KnownTransition, OpId, Operation as _, Opout, Precision, Schema, SecretSeal,
    Transition, TransitionBundle, TransitionType, TypedAssigns,
    bitcoin::{OutPoint, Transaction as BdkTransaction, Txid},
    containers::{
        BuilderSeal, Consignment, LoadError, UniversalFile, ValidContract, ValidTransfer,
    },
    contract::IssuerWrapper,
    rgbcore::{commit_verify::mpc::Commitment, dbc::Proof as _},
    stl::{AssetSpec, BlockNumber, BurnReason, Details, Name},
    txout::TxoSeal,
    validation::DbcProof,
};
#[cfg(feature = "_online")]
use rgbstd::{
    containers::PendingConsignment,
    contract::FilterIncludeAll,
    validation::{Status as ValidationStatus, Validity},
};
#[cfg(any(feature = "wallet", feature = "validation"))]
use rgbstd::{
    containers::ValidConsignment,
    stl::{BridgeLocation, EvmAddress, EvmContract, RejectListLocation, RejectListUrl},
};
#[cfg(feature = "validation")]
use rgbstd::{
    containers::{AnchorResolverError, CheckedAnchorResolver, ResolveAnchor},
    daggy::Walker,
    validation::{
        ExternalAnchor, OpoutsDagData, SchemaRules, ValidationConfig, ValidationError, Warning,
    },
};
#[cfg(feature = "wallet")]
use schemata::IfaWrapper;
#[cfg(feature = "_online")]
use schemata::{BfaWrapper, CfaWrapper, NiaWrapper, UdaWrapper};
use schemata::{
    BridgedFungibleAsset, CollectibleFungibleAsset, InflatableFungibleAsset, NonInflatableAsset,
    UniqueDigitalAsset,
};
use schemata::{
    GS_BURN_REASON, GS_BURNED_ASSET, GS_BURNED_INFLATION, GS_DETAILS, GS_ISSUED_SUPPLY, GS_NAME,
    GS_NOMINAL, GS_PRECISION, MS_AFTER_BLOCK, MS_ALLOWED_INFLATION, OS_ASSET, OS_INFLATION,
    OS_MINT, TS_BURN, TS_INFLATION, TS_MINT, TS_TRANSFER,
};
#[cfg(feature = "validation")]
use schemata::{GS_REJECT_LIST, GS_REJECT_LIST_URL};
#[cfg(feature = "wallet")]
use scrypt::{Params, phc::Salt, scrypt};
#[cfg(feature = "wallet")]
use sea_orm::{
    ActiveValue, ColumnTrait, ConnectOptions, Database, DatabaseConnection, DatabaseTransaction,
    DbErr, DeriveActiveEnum, EntityTrait, EnumIter, Iterable, JoinType, JsonValue, QueryFilter,
    QueryOrder, QueryResult, QuerySelect, TransactionTrait, TryGetError, TryGetable, TryIntoModel,
    sea_query::{Alias, Condition, Expr, ExprTrait, Func, OnConflict, SimpleExpr},
};
#[cfg(feature = "wallet")]
use serde::Deserializer;
#[cfg(feature = "wallet")]
use serde::de::{self, Unexpected, Visitor};
use serde::{Deserialize, Serialize};
#[cfg(feature = "_online")]
use sha2::Sha512;
#[cfg(feature = "wallet")]
use sha2::{Digest, Sha256};
#[cfg(feature = "_online")]
use slog::warn;
#[cfg(feature = "wallet")]
use slog::{Drain, Logger, debug, error, info, o};
#[cfg(feature = "wallet")]
use slog_async::AsyncGuard;
#[cfg(feature = "wallet")]
use slog_term::{FullFormat, PlainDecorator};
#[cfg(any(feature = "_online", feature = "rgb_fs_store_migration"))]
use strict_encoding::StrictSerialize;
use strict_encoding::{DeserializeError, StrictDeserialize};
#[cfg(feature = "wallet")]
use strict_encoding::{FieldName, StrictDecode, StrictEncode};
#[cfg(feature = "wallet")]
use strict_types::{TypeLib, TypeLibId};
#[cfg(feature = "wallet")]
use tempfile::TempDir;
#[cfg(feature = "wallet")]
use time::OffsetDateTime;
#[cfg(feature = "_online")]
use url::Url;
#[cfg(feature = "wallet")]
use walkdir::WalkDir;
#[cfg(feature = "_online")]
use x25519_dalek::{PublicKey as X25519Public, StaticSecret as X25519Secret};
#[cfg(feature = "wallet")]
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

#[cfg(feature = "validation")]
use crate::api::{
    evm::{BoundedAnchorResolver, EvmClient},
    reject_list::RejectListClient,
};
#[cfg(feature = "_validation")]
use crate::error::IndexerError;
#[cfg(feature = "bitcoind")]
use crate::no_wallet::validation::checked_bitcoind_indexes;
#[cfg(feature = "electrum")]
use crate::no_wallet::validation::electrum_config;
#[cfg(feature = "esplora")]
use crate::no_wallet::validation::esplora_config;
#[cfg(feature = "_online")]
use crate::no_wallet::validation::{
    ChainResolver, OffchainResolver, RejectList, RejectListSource, checked_evm_client,
    get_resolver, max_mint_scan_blocks, rejected_opouts, unreachable_indexer_details,
    validate_consignment_internal,
};
#[cfg(feature = "wallet")]
use crate::no_wallet::{bundle_info, check_file_exists, verify_witness_commitment};
#[cfg(feature = "electrum")]
use crate::utils::INDEXER_BATCH_SIZE;
#[cfg(feature = "esplora")]
use crate::utils::INDEXER_PARALLEL_REQUESTS;
#[cfg(any(feature = "electrum", feature = "esplora"))]
use crate::utils::INDEXER_STOP_GAP;
#[cfg(feature = "_online")]
use crate::utils::write_fascia;
#[cfg(feature = "bitcoind")]
use crate::wallet::BitcoindClient;
#[cfg(feature = "_online")]
#[cfg(test)]
use crate::wallet::test::{
    mock_chain_tx, mock_clock_skew_margin, mock_confirmation_margin, mock_input_unspents,
    mock_local_version, mock_max_mint_scan_blocks, mock_min_retention_grace,
    mock_mint_after_block_grace, mock_mint_lock_window, mock_retention_grace, mock_send_end_crash,
    skip_build_dag, skip_check_fee_rate, spv_only, strip_terminals,
};
#[cfg(feature = "_online")]
use crate::{
    api::{
        evm::MintAnchorResolver,
        multisig_hub::{
            FileMetadata, FileSource, FileType, InfoResponse, MultisigHubClient,
            OPERATION_TYPES_REQUIRING_SEED, OperationResponse, OperationStatus, OperationType,
            UserRoleResponse,
        },
        proxy::{
            AckTransferData, GetAckResponse, GetConsignmentResponse, InvoiceStatus, JsonRpcError,
            JsonRpcResponse, PostAckResponse, PostConsignmentData, ProxyClient, ProxyTransferState,
            ServerInfoResponse, get_ack_message, list_transfers_message, map_proxy_error,
            post_consignment_message, post_media_message, pow_difficulty_required,
            prepare_ack_auth_message, prepare_invoice_auth_message, transfer_retrieved_message,
        },
    },
    database::{
        DbData,
        entities::{
            pending_witness_script::Model as DbPendingWitnessScript,
            reserved_txo::ActiveModel as DbReservedTxoActMod,
            wallet_transaction::ActiveModel as DbWalletTransactionActMod,
        },
    },
    encryption::{
        decode_hash_hex, decrypt_blob, decrypt_stream, derive_shared, ed25519_from_seed,
        encrypt_blob, encrypt_to_tempfile, invoice_id, sign_ed25519, solve_pow, transfer_id,
        verify_ed25519, verifying_key_from_bytes,
    },
    no_wallet::{BitcoindAuth, IndexerConfig},
    utils::{
        check_proxy, detect_indexer, get_indexer_and_resolver, hash_file, proxy_max_media,
        proxy_retention_grace, script_buf_from_recipient_id,
    },
    wallet::{AssignmentsCollection, Indexer, multisig::RespondToOperation},
};
#[cfg(feature = "wallet")]
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
        enums::{ColoringType, RecipientTypeFull, TransportEndpointKind},
    },
    encryption::{gen_ed25519, sha256_digest},
    error::InternalError,
    keys::{Keys, WitnessVersion},
    utils::{
        ACCOUNT, DumbResolver, KEYCHAIN_BTC, KEYCHAIN_RGB, LOG_FILE, PURPOSE,
        RGB_RUNTIME_DELETING_DIR, RgbStock, adjust_canonicalization, atomic_tmp_path, atomic_write,
        atomic_write_with, beneficiary_from_script_buf, bfa_reject_list_url,
        from_str_or_number_mandatory, from_str_or_number_optional, get_account_xpubs,
        get_coin_type, get_descriptors, get_descriptors_from_xpubs, hash_bytes, hash_bytes_hex,
        now, parse_address_str, read_fascia, setup_logger, str_to_xpub,
    },
    wallet::{
        Balance, LocalRgbAllocation, LocalUnspent, NUM_KNOWN_SCHEMAS, Outpoint,
        RGB_STATE_ASSET_OWNER, RGB_STATE_INFLATION_ALLOWANCE, RGB_STATE_MINT_RIGHT,
        WalletDescriptors,
    },
};
#[cfg(all(test, feature = "wallet"))]
use crate::{
    keys::generate_keys,
    wallet::test::{mock_asset_terms, mock_chain_net, mock_contract_details, mock_token_data},
};

pub(crate) const RGB_GLOBAL_BURNED_ASSET: &str = "burnedAsset";
pub(crate) const RGB_GLOBAL_BURNED_INFLATION: &str = "burnedInflation";
pub(crate) const RGB_GLOBAL_BURN_REASON: &str = "burnReason";
pub(crate) const RGB_GLOBAL_ISSUED_SUPPLY: &str = "issuedSupply";
pub(crate) const RGB_METADATA_AFTER_BLOCK: &str = "afterBlock";
pub(crate) const RGB_METADATA_ALLOWED_INFLATION: &str = "allowedInflation";
