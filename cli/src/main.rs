mod opts;

mod wallet_helpers;

use std::error::Error;

use amplify::map;
use clap::Parser;
use opts::{AssetCommand, Command, IssueCommand, NativeWalletCommand, TransferCommand};
use rgb_lib::{
    wallet::{Online, Recipient, WalletData},
    Wallet,
};

use crate::{opts::Opts, wallet_helpers::assure_utxos_synced};

fn handle_transfer_command(
    mut wallet: Wallet,
    opts: Opts,
    cmd: TransferCommand,
) -> Result<(), Box<dyn Error + 'static>> {
    match cmd {
        TransferCommand::List { asset_id } => {
            let t = wallet.list_transfers(asset_id)?;
            println!("{}", serde_json::to_string(&t)?);
        }
        TransferCommand::Send {
            amount,
            asset_id,
            blinded_utxo,
            consignment_endpoints,
            donation,
            fee_rate,
        } => {
            let online =
                wallet.go_online(false, opts.electrum_url.expect("needs electrum_url option"))?;

            let recipient_map =
                map! { asset_id => vec![Recipient{ blinded_utxo, amount, consignment_endpoints }] };

            let fee_rate: f32 = (fee_rate * 1000) as f32;
            let r = wallet.send(online, recipient_map, donation, fee_rate)?;
            println!("{}", r);
        }
        TransferCommand::Fail {
            blinded_utxo,
            txid,
            no_asset_only,
        } => {
            let online =
                wallet.go_online(false, opts.electrum_url.expect("needs electrum_url option"))?;
            let i = wallet.fail_transfers(online, blinded_utxo, txid, no_asset_only)?;
            println!("{}", i);
        }
        TransferCommand::Delete {
            blinded_utxo,
            txid,
            no_asset_only,
        } => {
            let i = wallet.delete_transfers(blinded_utxo, txid, no_asset_only)?;
            println!("{}", i);
        }
    }
    Ok(())
}

fn handle_asset_command(
    mut wallet: Wallet,
    opts: Opts,
    cmd: AssetCommand,
) -> Result<(), Box<dyn Error + 'static>> {
    match cmd {
        AssetCommand::GetBalance { asset_id } => {
            let b = wallet.get_asset_balance(asset_id)?;
            println!("{}", serde_json::to_string(&b)?);
        }
        AssetCommand::Issue(IssueCommand::Rgb20 {
            ticker,
            name,
            precision,
            amounts,
        }) => {
            let online = Online {
                id: 0,
                electrum_url: opts.electrum_url.unwrap(),
            };
            let r = wallet.issue_asset_rgb20(
                online,
                ticker.to_uppercase(),
                name,
                precision,
                amounts,
            )?;
            println!("{}", serde_json::to_string(&r)?);
        }
        AssetCommand::Issue(IssueCommand::Rgb121 {
            name,
            amounts,
            description,
            precision,
            parent_id,
            file_path,
        }) => {
            let online =
                wallet.go_online(false, opts.electrum_url.expect("needs electrum_url option"))?;
            let asset = wallet.issue_asset_rgb121(
                online,
                name,
                description,
                precision,
                amounts,
                parent_id,
                file_path,
            )?;
            println!("{}", serde_json::to_string(&asset)?);
        }
        AssetCommand::List { filter_asset_types } => {
            let asset = wallet.list_assets(filter_asset_types)?;
            println!("{}", serde_json::to_string(&asset)?);
        }
        AssetCommand::GetMetadata { asset_id } => {
            let online =
                wallet.go_online(false, opts.electrum_url.expect("needs electrum_url option"))?;
            let metadata = wallet.get_asset_metadata(online, asset_id)?;
            println!("{}", serde_json::to_string(&metadata)?);
        }
    };
    Ok(())
}

fn main() -> Result<(), Box<dyn Error + 'static>> {
    let opts = Opts::parse();

    if !opts.data_dir.is_absolute() {
        eprintln!("Please specify absolute path as data dir!");
        std::process::exit(1);
    }

    let wallet_data = WalletData {
        data_dir: opts.data_dir.to_str().unwrap().to_string(),
        bitcoin_network: opts.network,
        database_type: opts.db_type,
        pubkey: opts.xpub.clone(),
        mnemonic: opts.mnemonic.clone(),
    };
    let mut wallet = Wallet::new(wallet_data)?;

    match opts.command.clone() {
        Command::ListUnspents { settled_only } => {
            assure_utxos_synced(
                &mut wallet,
                opts.electrum_url.expect("needs electrum_url option"),
            );
            let u = wallet.list_unspents(settled_only)?;
            println!("{}", serde_json::to_string(&u)?);
        }
        Command::Blind {
            amount,
            asset_id,
            duration_seconds,
            consignment_endpoints,
        } => {
            assure_utxos_synced(
                &mut wallet,
                opts.electrum_url.expect("needs electrum_url option"),
            );
            let b = wallet.blind(asset_id, amount, duration_seconds, consignment_endpoints)?;
            println!("{}", serde_json::to_string(&b)?);
        }
        Command::NativeWallet(NativeWalletCommand::GetAddress) => {
            println!("{}", wallet.get_address())
        }
        Command::NativeWallet(NativeWalletCommand::Drain {
            address,
            destroy_assets,
            fee_rate,
        }) => {
            let online =
                wallet.go_online(false, opts.electrum_url.expect("needs electrum_url option"))?;
            let fee_rate: f32 = (fee_rate * 1000) as f32;
            let txid = wallet.drain_to(online, address, destroy_assets, fee_rate)?;
            println!("{}", txid)
        }
        Command::Asset(cmd) => handle_asset_command(wallet, opts, cmd)?,
        Command::Transfer(cmd) => handle_transfer_command(wallet, opts, cmd)?,
    }
    Ok(())
}
