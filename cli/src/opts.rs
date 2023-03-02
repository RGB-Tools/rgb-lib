use std::path::PathBuf;

use clap::{Parser, Subcommand};
use rgb_lib::{
    wallet::{AssetType, DatabaseType},
    BitcoinNetwork,
};

#[derive(Parser, Clone, PartialEq, Eq, Debug)]
#[clap(name = "rgblib-tool", bin_name = "rgblib-tool", author, version, about)]
pub struct Opts {
    /// Data directory.
    #[clap(short = 'd', long = "datadir")]
    pub data_dir: PathBuf,

    /// database type
    #[clap(short = 't', long = "dbtype", default_value = "sqlite")]
    pub db_type: DatabaseType,

    /// master xpub for the wallet.
    #[clap(short, long, required = true)]
    pub xpub: String,

    /// mnemonic for the wallet master private key.
    #[clap(short, long)]
    pub mnemonic: Option<String>,

    /// Which bitcoin network to operate.
    #[clap(
        short = 'n',
        long = "network",
        default_value = "regtest",
        global = true
    )]
    pub network: BitcoinNetwork,

    /// Electrum url to connect. It is necessary only for an online operation.
    #[clap(
        long = "electrum-url",
        alias = "electrs-url",
        alias = "electrs",
        alias = "electrum",
        required = false,
        global = true
    )]
    pub electrum_url: Option<String>,

    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Clone, Eq, PartialEq, Debug, amplify::Display)]
pub enum Command {
    #[display("list-unspents")]
    ListUnspents {
        #[display("settled_only: {0}")]
        #[clap(short, long, action = clap::ArgAction::SetTrue)]
        settled_only: bool,
    },

    #[display("asset")]
    #[clap(subcommand)]
    Asset(AssetCommand),

    #[display("transfer")]
    #[clap(subcommand)]
    Transfer(TransferCommand),

    /// Get necessary information to pass to a counterparty to ask funds from them.
    #[display("blind")]
    Blind {
        /// Asset id that you want to receive
        #[display("asset_id: {0}")]
        #[clap(long = "asset-id", alias = "id")]
        asset_id: Option<String>,

        /// amount to blind,
        #[display("amount: {0}")]
        #[clap(short, long)]
        amount: Option<u64>,

        /// Duration until an expiry
        #[display("duration_seconds: {0}")]
        #[clap(short, long, alias = "duration")]
        duration_seconds: Option<u32>,

        /// List of endpoints that you want counterparty to send a response consignments.
        #[display("consignment_endpoints: {0}")]
        #[clap(short, long, required = true)]
        consignment_endpoints: Vec<String>,
    },

    #[display("native-wallet")]
    #[clap(subcommand)]
    NativeWallet(NativeWalletCommand),
}

#[derive(Subcommand, Clone, Eq, PartialEq, Debug, amplify::Display)]
pub enum NativeWalletCommand {
    /// Get bitcoin address
    #[display("get-address")]
    GetAddress,

    /// Send all utxos to an address (funds sweeping.)
    /// By default it does not touch an UTXO which holds RGB-asset.
    #[display("drain")]
    Drain {
        /// Address to send funds.
        #[clap(short, long = "address", alias = "addr")]
        address: String,

        /// DANGEROUS: include UTXOs which holds RGB-asset.
        /// This will destroy all your RGB assets.
        #[clap(long, action = clap::ArgAction::SetTrue)]
        destroy_assets: bool,

        /// feerate for tx (sat/Kvb)
        #[clap(short, long)]
        fee_rate: u64,
    },
}

#[derive(Subcommand, Clone, Eq, PartialEq, Debug, amplify::Display)]
pub enum TransferCommand {
    #[display("send")]
    #[clap(alias = "transfer")]
    Send {
        /// Asset id to send
        #[clap(long = "asset-id", alias = "id")]
        asset_id: String,

        /// destination blinded_utxo
        #[clap(long)]
        blinded_utxo: String,

        /// Amount to send
        #[clap(long)]
        amount: u64,

        /// List of endpoints that you want send the consignment data.
        #[display("consignment_endpoints: {0}")]
        #[clap(short, long, required = true)]
        consignment_endpoints: Vec<String>,

        /// If donation is set, It will broadcast the tx without waiting counterparty's ACK.
        #[clap(short, long, action = clap::ArgAction::SetTrue)]
        donation: bool,

        /// feerate for tx (sat/Kvb)
        #[clap(short, long)]
        fee_rate: u64,
    },
    #[display("list ...")]
    List {
        #[clap(long = "asset_id", alias = "id")]
        asset_id: String,
    },
    #[display("fail")]
    Fail {
        /// If this option is specified, it works only for specific transfer txo.
        #[clap(long, conflicts_with = "txid")]
        blinded_utxo: Option<String>,

        /// If this option is specified, it works only for specific transfer tx.
        #[clap(long, conflicts_with = "blinded-utxo")]
        txid: Option<String>,

        /// Fail only transfers those not associated to any asset.
        #[clap(long, action = clap::ArgAction::SetTrue)]
        no_asset_only: bool,
    },
    Delete {
        /// If this option is specified, it works only for specific transfer txo.
        #[clap(long, conflicts_with = "txid")]
        blinded_utxo: Option<String>,

        /// If this option is specified, it works only for specific transfer tx.
        #[clap(long, conflicts_with = "blinded-utxo")]
        txid: Option<String>,

        /// Delete only transfers those not associated to any asset.
        #[clap(long, action = clap::ArgAction::SetTrue)]
        no_asset_only: bool,
    },
}

#[derive(Subcommand, Clone, Eq, PartialEq, Debug, amplify::Display)]
pub enum AssetCommand {
    #[display("issue")]
    #[clap(subcommand)]
    Issue(IssueCommand),

    #[display("list ...")]
    List {
        #[clap(short = 't', long = "asset_type", alias = "asset")]
        filter_asset_types: Vec<AssetType>,
    },

    /// Get balance for specific asset
    #[display("get-balance")]
    GetBalance {
        #[display("asset_id: {0}")]
        #[clap(long = "asset-id", alias = "id")]
        asset_id: String,
    },

    /// Get metadata for asset
    GetMetadata {
        #[display("asset_id: {0}")]
        #[clap(long = "asset-id", alias = "id")]
        asset_id: String,
    },
}

#[derive(Subcommand, Clone, Eq, PartialEq, Debug, amplify::Display)]
pub enum IssueCommand {
    #[display("rgb20")]
    Rgb20 {
        /// Asset name
        #[display("name: {0}")]
        #[clap(short, long, required = true)]
        name: String,

        /// Ticker symbol for the asset. It will be converted to uppercase.
        #[display("ticker: {0}")]
        #[clap(short, long, required = true)]
        ticker: String,

        /// amounts to issue.
        #[display("amounts: {0}")]
        #[clap(short, long, required = true)]
        amounts: Vec<u64>,

        /// Precision (amount divisibility) of the asset.
        #[display("precision: {0}")]
        #[clap(short, long, default_value_t = 1u8)]
        precision: u8,
    },

    #[display("rgb121")]
    Rgb121 {
        /// Asset name
        #[display("name: {0}")]
        #[clap(short, long, required = true)]
        name: String,

        /// amounts to issue.
        #[display("amounts: {0}")]
        #[clap(short, long, required = true)]
        amounts: Vec<u64>,

        #[display("description")]
        #[clap(short, long)]
        description: Option<String>,

        /// Precision (amount divisibility) of the asset.
        #[display("precision: {0}")]
        #[clap(short, long, default_value_t = 1u8)]
        precision: u8,

        #[display("parent_id: {0}")]
        #[clap(long)]
        parent_id: Option<String>,

        #[display("file_path: {0}")]
        #[clap(short, long)]
        file_path: Option<String>,
    },
}
