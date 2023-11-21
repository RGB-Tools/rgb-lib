use super::*;

pub(crate) fn test_blind_receive(wallet: &mut Wallet) -> ReceiveData {
    wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap()
}

pub(crate) fn test_witness_receive(wallet: &mut Wallet) -> ReceiveData {
    wallet
        .witness_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap()
}

pub(crate) fn test_create_utxos_default(wallet: &mut Wallet, online: &Online) -> u8 {
    _test_create_utxos(wallet, online, false, None, None, FEE_RATE)
}

pub(crate) fn test_create_utxos(
    wallet: &mut Wallet,
    online: &Online,
    up_to: bool,
    num: Option<u8>,
    size: Option<u32>,
    fee_rate: f32,
) -> u8 {
    _test_create_utxos(wallet, online, up_to, num, size, fee_rate)
}

pub(crate) fn test_create_utxos_begin_result(
    wallet: &mut Wallet,
    online: &Online,
    up_to: bool,
    num: Option<u8>,
    size: Option<u32>,
    fee_rate: f32,
) -> Result<String, Error> {
    wallet.create_utxos_begin(online.clone(), up_to, num, size, fee_rate)
}

pub(crate) fn _test_create_utxos(
    wallet: &mut Wallet,
    online: &Online,
    up_to: bool,
    num: Option<u8>,
    size: Option<u32>,
    fee_rate: f32,
) -> u8 {
    let delay = 200;
    let mut retries = 3;
    let mut num_utxos_created = 0;
    while retries > 0 {
        retries -= 1;
        let result = wallet.create_utxos(online.clone(), up_to, num, size, fee_rate);
        match result {
            Ok(_) => {
                num_utxos_created = result.unwrap();
                break;
            }
            Err(Error::InsufficientBitcoins {
                needed: _,
                available: _,
            }) => {
                std::thread::sleep(Duration::from_millis(delay));
                continue;
            }
            Err(error) => {
                panic!("error creating UTXOs for wallet: {error:?}");
            }
        }
    }
    if num_utxos_created == 0 {
        panic!("error creating UTXOs for wallet: insufficient bitcoins");
    }
    num_utxos_created
}

pub(crate) fn test_delete_transfers(
    wallet: &Wallet,
    recipient_id: Option<&str>,
    txid: Option<&str>,
    no_asset_only: bool,
) -> bool {
    test_delete_transfers_result(wallet, recipient_id, txid, no_asset_only).unwrap()
}

pub(crate) fn test_delete_transfers_result(
    wallet: &Wallet,
    recipient_id: Option<&str>,
    txid: Option<&str>,
    no_asset_only: bool,
) -> Result<bool, Error> {
    let recipient_id = recipient_id.map(|id| id.to_string());
    let txid = txid.map(|id| id.to_string());
    wallet.delete_transfers(recipient_id, txid, no_asset_only)
}

pub(crate) fn test_drain_to_result(
    wallet: &Wallet,
    online: &Online,
    address: &str,
    destroy_assets: bool,
) -> Result<String, Error> {
    wallet.drain_to(
        online.clone(),
        address.to_string(),
        destroy_assets,
        FEE_RATE,
    )
}

pub(crate) fn test_drain_to_begin_result(
    wallet: &Wallet,
    online: &Online,
    address: &str,
    destroy_assets: bool,
    fee_rate: f32,
) -> Result<String, Error> {
    wallet.drain_to_begin(
        online.clone(),
        address.to_string(),
        destroy_assets,
        fee_rate,
    )
}

pub(crate) fn test_drain_to_destroy(wallet: &Wallet, online: &Online, address: &str) -> String {
    wallet
        .drain_to(online.clone(), address.to_string(), true, FEE_RATE)
        .unwrap()
}

pub(crate) fn test_drain_to_keep(wallet: &Wallet, online: &Online, address: &str) -> String {
    wallet
        .drain_to(online.clone(), address.to_string(), false, FEE_RATE)
        .unwrap()
}

pub(crate) fn test_fail_transfers_all(wallet: &mut Wallet, online: &Online) -> bool {
    wallet
        .fail_transfers(online.clone(), None, None, false)
        .unwrap()
}

pub(crate) fn test_fail_transfers_blind(
    wallet: &mut Wallet,
    online: &Online,
    blinded_utxo: &str,
) -> bool {
    wallet
        .fail_transfers(online.clone(), Some(blinded_utxo.to_string()), None, false)
        .unwrap()
}

pub(crate) fn test_fail_transfers_txid(wallet: &mut Wallet, online: &Online, txid: &str) -> bool {
    wallet
        .fail_transfers(online.clone(), None, Some(txid.to_string()), false)
        .unwrap()
}

pub(crate) fn test_get_address(wallet: &Wallet) -> String {
    wallet.get_address().unwrap()
}

pub(crate) fn test_get_asset_balance(wallet: &Wallet, asset_id: &str) -> Balance {
    test_get_asset_balance_result(wallet, asset_id).unwrap()
}

pub(crate) fn test_get_asset_balance_result(
    wallet: &Wallet,
    asset_id: &str,
) -> Result<Balance, Error> {
    wallet.get_asset_balance(asset_id.to_string())
}

pub(crate) fn test_get_asset_metadata(wallet: &mut Wallet, asset_id: &str) -> Metadata {
    test_get_asset_metadata_result(wallet, asset_id).unwrap()
}

pub(crate) fn test_get_asset_metadata_result(
    wallet: &mut Wallet,
    asset_id: &str,
) -> Result<Metadata, Error> {
    wallet.get_asset_metadata(asset_id.to_string())
}

pub(crate) fn test_get_btc_balance(wallet: &Wallet, online: &Online) -> BtcBalance {
    wallet.get_btc_balance(online.clone()).unwrap()
}

pub(crate) fn test_get_wallet_data(wallet: &Wallet) -> WalletData {
    wallet.get_wallet_data()
}

pub(crate) fn test_get_wallet_dir(wallet: &Wallet) -> PathBuf {
    wallet.get_wallet_dir()
}

pub(crate) fn test_go_online(
    wallet: &mut Wallet,
    skip_consistency_check: bool,
    electrum_url: Option<&str>,
) -> Online {
    test_go_online_result(wallet, skip_consistency_check, electrum_url).unwrap()
}

pub(crate) fn test_go_online_result(
    wallet: &mut Wallet,
    skip_consistency_check: bool,
    electrum_url: Option<&str>,
) -> Result<Online, Error> {
    let electrum = electrum_url.unwrap_or(ELECTRUM_URL).to_string();
    wallet.go_online(skip_consistency_check, electrum)
}

pub(crate) fn test_issue_asset_cfa(
    wallet: &mut Wallet,
    online: &Online,
    amounts: Option<&[u64]>,
    file_path: Option<String>,
) -> AssetCFA {
    test_issue_asset_cfa_result(wallet, online, amounts, file_path).unwrap()
}

pub(crate) fn test_issue_asset_cfa_result(
    wallet: &mut Wallet,
    online: &Online,
    amounts: Option<&[u64]>,
    file_path: Option<String>,
) -> Result<AssetCFA, Error> {
    let amounts = if let Some(a) = amounts {
        a.to_vec()
    } else {
        vec![AMOUNT]
    };
    wallet.issue_asset_cfa(
        online.clone(),
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        amounts,
        file_path,
    )
}

pub(crate) fn test_issue_asset_nia(
    wallet: &mut Wallet,
    online: &Online,
    amounts: Option<&[u64]>,
) -> AssetNIA {
    test_issue_asset_nia_result(wallet, online, amounts).unwrap()
}

pub(crate) fn test_issue_asset_nia_result(
    wallet: &mut Wallet,
    online: &Online,
    amounts: Option<&[u64]>,
) -> Result<AssetNIA, Error> {
    let amounts = if let Some(a) = amounts {
        a.to_vec()
    } else {
        vec![AMOUNT]
    };
    wallet.issue_asset_nia(
        online.clone(),
        TICKER.to_string(),
        NAME.to_string(),
        PRECISION,
        amounts,
    )
}

pub(crate) fn test_list_assets(
    wallet: &mut Wallet,
    filter_asset_schemas: &[AssetSchema],
) -> Assets {
    wallet.list_assets(filter_asset_schemas.to_vec()).unwrap()
}

pub(crate) fn test_list_transactions(wallet: &Wallet, online: Option<&Online>) -> Vec<Transaction> {
    let online = online.cloned();
    wallet.list_transactions(online).unwrap()
}

pub(crate) fn test_list_transfers(wallet: &Wallet, asset_id: Option<&str>) -> Vec<Transfer> {
    test_list_transfers_result(wallet, asset_id).unwrap()
}

pub(crate) fn test_list_transfers_result(
    wallet: &Wallet,
    asset_id: Option<&str>,
) -> Result<Vec<Transfer>, Error> {
    let asset_id = asset_id.map(|a| a.to_string());
    wallet.list_transfers(asset_id)
}

pub(crate) fn test_list_unspents(
    wallet: &Wallet,
    online: Option<&Online>,
    settled_only: bool,
) -> Vec<Unspent> {
    let online = online.cloned();
    wallet.list_unspents(online, settled_only).unwrap()
}

pub(crate) fn test_refresh_all(wallet: &mut Wallet, online: &Online) -> bool {
    wallet.refresh(online.clone(), None, vec![]).unwrap()
}

pub(crate) fn test_refresh_asset(wallet: &mut Wallet, online: &Online, asset_id: &str) -> bool {
    wallet
        .refresh(online.clone(), Some(asset_id.to_string()), vec![])
        .unwrap()
}

pub(crate) fn test_send(
    wallet: &mut Wallet,
    online: &Online,
    recipient_map: &HashMap<String, Vec<Recipient>>,
) -> String {
    test_send_result(wallet, online, recipient_map).unwrap()
}

pub(crate) fn test_send_result(
    wallet: &mut Wallet,
    online: &Online,
    recipient_map: &HashMap<String, Vec<Recipient>>,
) -> Result<String, Error> {
    wallet.send(
        online.clone(),
        recipient_map.clone(),
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    )
}

pub(crate) fn test_send_begin_result(
    wallet: &mut Wallet,
    online: &Online,
    recipient_map: &HashMap<String, Vec<Recipient>>,
) -> Result<String, Error> {
    wallet.send_begin(
        online.clone(),
        recipient_map.clone(),
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    )
}

pub(crate) fn test_send_btc(
    wallet: &Wallet,
    online: &Online,
    address: &str,
    amount: u64,
) -> String {
    test_send_btc_result(wallet, online, address, amount).unwrap()
}

pub(crate) fn test_send_btc_result(
    wallet: &Wallet,
    online: &Online,
    address: &str,
    amount: u64,
) -> Result<String, Error> {
    wallet.send_btc(online.clone(), address.to_string(), amount, FEE_RATE)
}
