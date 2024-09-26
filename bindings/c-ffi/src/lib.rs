mod utils;

use crate::utils::*;

use std::{
    any::TypeId,
    collections::{hash_map::DefaultHasher, HashMap},
    ffi::{c_char, c_float, c_uchar, c_void, CStr, CString},
    hash::{Hash, Hasher},
    str::FromStr,
};

use rgb_lib::{
    wallet::{Online, Recipient, RefreshFilter, Wallet, WalletData},
    AssetSchema, BitcoinNetwork, Error as RgbLibError,
};

#[repr(C)]
pub struct COpaqueStruct {
    ptr: *const c_void,
    ty: u64,
}

#[repr(C)]
pub enum CResultValue {
    Ok,
    Err,
}

#[repr(C)]
pub struct CResult {
    result: CResultValue,
    inner: COpaqueStruct,
}

#[repr(C)]
pub struct CResultString {
    result: CResultValue,
    inner: *mut c_char,
}

#[no_mangle]
pub extern "C" fn free_online(obj: COpaqueStruct) {
    unsafe {
        let _ = Box::from_raw(obj.ptr as *mut Online);
    }
}

#[no_mangle]
pub extern "C" fn free_wallet(obj: COpaqueStruct) {
    unsafe {
        let _ = Box::from_raw(obj.ptr as *mut Wallet);
    }
}

#[no_mangle]
pub extern "C" fn rgblib_blind_receive(
    wallet: &COpaqueStruct,
    asset_id_opt: *const c_char,
    amount_opt: *const c_char,
    duration_seconds_opt: *const c_char,
    transport_endpoints: *const c_char,
    min_confirmations: c_uchar,
) -> CResultString {
    blind_receive(
        wallet,
        asset_id_opt,
        amount_opt,
        duration_seconds_opt,
        transport_endpoints,
        min_confirmations,
    )
    .into()
}

#[no_mangle]
pub extern "C" fn rgblib_create_utxos(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    up_to: bool,
    num_opt: *const c_char,
    size_opt: *const c_char,
    fee_rate: c_float,
) -> CResultString {
    create_utxos(wallet, online, up_to, num_opt, size_opt, fee_rate).into()
}

#[no_mangle]
pub extern "C" fn rgblib_generate_keys(bitcoin_network: *const c_char) -> CResultString {
    generate_keys(bitcoin_network).into()
}

#[no_mangle]
pub extern "C" fn rgblib_get_address(wallet: &COpaqueStruct) -> CResultString {
    get_address(wallet).into()
}

#[no_mangle]
pub extern "C" fn rgblib_get_asset_balance(
    wallet: &COpaqueStruct,
    asset_id: *const c_char,
) -> CResultString {
    get_asset_balance(wallet, asset_id).into()
}

#[no_mangle]
pub extern "C" fn rgblib_get_btc_balance(
    wallet: &COpaqueStruct,
    online: *const COpaqueStruct,
) -> CResultString {
    get_btc_balance(wallet, online).into()
}

#[no_mangle]
pub extern "C" fn rgblib_go_online(
    wallet: &COpaqueStruct,
    skip_consistency_check: bool,
    electrum_url: *const c_char,
) -> CResult {
    go_online(wallet, skip_consistency_check, electrum_url).into()
}

#[no_mangle]
pub extern "C" fn rgblib_issue_asset_cfa(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    name: *const c_char,
    details_opt: *const c_char,
    precision: c_uchar,
    amounts: *const c_char,
    file_path_opt: *const c_char,
) -> CResultString {
    issue_asset_cfa(
        wallet,
        online,
        name,
        details_opt,
        precision,
        amounts,
        file_path_opt,
    )
    .into()
}

#[no_mangle]
pub extern "C" fn rgblib_issue_asset_nia(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    ticker: *const c_char,
    name: *const c_char,
    precision: c_uchar,
    amounts: *const c_char,
) -> CResultString {
    issue_asset_nia(wallet, online, ticker, name, precision, amounts).into()
}

#[no_mangle]
pub extern "C" fn rgblib_issue_asset_uda(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    ticker: *const c_char,
    name: *const c_char,
    details_opt: *const c_char,
    precision: c_uchar,
    media_file_path_opt: *const c_char,
    attachments_file_paths: *const c_char,
) -> CResultString {
    issue_asset_uda(
        wallet,
        online,
        ticker,
        name,
        details_opt,
        precision,
        media_file_path_opt,
        attachments_file_paths,
    )
    .into()
}

#[no_mangle]
pub extern "C" fn rgblib_list_assets(
    wallet: &COpaqueStruct,
    filter_asset_schemas: *const c_char,
) -> CResultString {
    list_assets(wallet, filter_asset_schemas).into()
}

#[no_mangle]
pub extern "C" fn rgblib_list_transactions(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
) -> CResultString {
    list_transactions(wallet, online).into()
}

#[no_mangle]
pub extern "C" fn rgblib_list_transfers(
    wallet: &COpaqueStruct,
    asset_id: *const c_char,
) -> CResultString {
    list_transfers(wallet, asset_id).into()
}

#[no_mangle]
pub extern "C" fn rgblib_list_unspents(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    settled_only: bool,
) -> CResultString {
    list_unspents(wallet, online, settled_only).into()
}

#[no_mangle]
pub extern "C" fn rgblib_new_wallet(wallet_data: *const c_char) -> CResult {
    new_wallet(wallet_data).into()
}

#[no_mangle]
pub extern "C" fn rgblib_refresh(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    asset_id_opt: *const c_char,
    filter: *const c_char,
) -> CResultString {
    refresh(wallet, online, asset_id_opt, filter).into()
}

#[no_mangle]
pub extern "C" fn rgblib_restore_keys(
    bitcoin_network: *const c_char,
    mnemonic: *const c_char,
) -> CResultString {
    restore_keys(bitcoin_network, mnemonic).into()
}

#[no_mangle]
pub extern "C" fn rgblib_send(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    recipient_map: *const c_char,
    donation: bool,
    fee_rate: c_float,
    min_confirmations: c_uchar,
) -> CResultString {
    send(
        wallet,
        online,
        recipient_map,
        donation,
        fee_rate,
        min_confirmations,
    )
    .into()
}

#[no_mangle]
pub extern "C" fn rgblib_send_btc(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    address: *const c_char,
    amount: u64,
    fee_rate: c_float,
) -> CResultString {
    send_btc(wallet, online, address, amount, fee_rate).into()
}

#[no_mangle]
pub extern "C" fn rgblib_witness_receive(
    wallet: &COpaqueStruct,
    asset_id_opt: *const c_char,
    amount_opt: *const c_char,
    duration_seconds_opt: *const c_char,
    transport_endpoints: *const c_char,
    min_confirmations: c_uchar,
) -> CResultString {
    witness_receive(
        wallet,
        asset_id_opt,
        amount_opt,
        duration_seconds_opt,
        transport_endpoints,
        min_confirmations,
    )
    .into()
}
