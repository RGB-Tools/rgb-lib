mod utils;

use crate::utils::*;

use std::{
    any::TypeId,
    collections::{HashMap, hash_map::DefaultHasher},
    ffi::{CStr, CString, c_char, c_void},
    hash::{Hash, Hasher},
    ptr::null_mut,
    str::FromStr,
};

use rgb_lib::{
    AssetSchema, Assignment, BitcoinNetwork, Error as RgbLibError,
    wallet::{Online, Recipient, RefreshFilter, Wallet, WalletData},
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

#[unsafe(no_mangle)]
pub extern "C" fn free_online(obj: COpaqueStruct) {
    unsafe {
        let _ = Box::from_raw(obj.ptr as *mut Online);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn free_wallet(obj: COpaqueStruct) {
    unsafe {
        let _ = Box::from_raw(obj.ptr as *mut Wallet);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_backup(
    wallet: &COpaqueStruct,
    backup_path: *const c_char,
    password: *const c_char,
) -> CResult {
    backup(wallet, backup_path, password).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_backup_info(wallet: &COpaqueStruct) -> CResultString {
    backup_info(wallet).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_blind_receive(
    wallet: &COpaqueStruct,
    asset_id_opt: *const c_char,
    assignment: *const c_char,
    duration_seconds_opt: *const c_char,
    transport_endpoints: *const c_char,
    min_confirmations: *const c_char,
) -> CResultString {
    blind_receive(
        wallet,
        asset_id_opt,
        assignment,
        duration_seconds_opt,
        transport_endpoints,
        min_confirmations,
    )
    .into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_create_utxos(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    up_to: bool,
    num_opt: *const c_char,
    size_opt: *const c_char,
    fee_rate: *const c_char,
    skip_sync: bool,
) -> CResultString {
    create_utxos(
        wallet, online, up_to, num_opt, size_opt, fee_rate, skip_sync,
    )
    .into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_finalize_psbt(
    wallet: &COpaqueStruct,
    signed_psbt: *const c_char,
) -> CResultString {
    finalize_psbt(wallet, signed_psbt).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_generate_keys(bitcoin_network: *const c_char) -> CResultString {
    generate_keys(bitcoin_network).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_get_address(wallet: &COpaqueStruct) -> CResultString {
    get_address(wallet).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_get_asset_balance(
    wallet: &COpaqueStruct,
    asset_id: *const c_char,
) -> CResultString {
    get_asset_balance(wallet, asset_id).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_get_btc_balance(
    wallet: &COpaqueStruct,
    online: *const COpaqueStruct,
    skip_sync: bool,
) -> CResultString {
    get_btc_balance(wallet, online, skip_sync).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_get_fee_estimation(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    blocks: *const c_char,
) -> CResultString {
    get_fee_estimation(wallet, online, blocks).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_go_online(
    wallet: &COpaqueStruct,
    skip_consistency_check: bool,
    electrum_url: *const c_char,
) -> CResult {
    go_online(wallet, skip_consistency_check, electrum_url).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_issue_asset_cfa(
    wallet: &COpaqueStruct,
    name: *const c_char,
    details_opt: *const c_char,
    precision: *const c_char,
    amounts: *const c_char,
    file_path_opt: *const c_char,
) -> CResultString {
    issue_asset_cfa(wallet, name, details_opt, precision, amounts, file_path_opt).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_issue_asset_ifa(
    wallet: &COpaqueStruct,
    ticker: *const c_char,
    name: *const c_char,
    precision: *const c_char,
    amounts: *const c_char,
    inflation_amounts: *const c_char,
    replace_rights_num: *const c_char,
) -> CResultString {
    issue_asset_ifa(
        wallet,
        ticker,
        name,
        precision,
        amounts,
        inflation_amounts,
        replace_rights_num,
    )
    .into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_issue_asset_nia(
    wallet: &COpaqueStruct,
    ticker: *const c_char,
    name: *const c_char,
    precision: *const c_char,
    amounts: *const c_char,
) -> CResultString {
    issue_asset_nia(wallet, ticker, name, precision, amounts).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_issue_asset_uda(
    wallet: &COpaqueStruct,
    ticker: *const c_char,
    name: *const c_char,
    details_opt: *const c_char,
    precision: *const c_char,
    media_file_path_opt: *const c_char,
    attachments_file_paths: *const c_char,
) -> CResultString {
    issue_asset_uda(
        wallet,
        ticker,
        name,
        details_opt,
        precision,
        media_file_path_opt,
        attachments_file_paths,
    )
    .into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_list_assets(
    wallet: &COpaqueStruct,
    filter_asset_schemas: *const c_char,
) -> CResultString {
    list_assets(wallet, filter_asset_schemas).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_list_transactions(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    skip_sync: bool,
) -> CResultString {
    list_transactions(wallet, online, skip_sync).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_list_transfers(
    wallet: &COpaqueStruct,
    asset_id: *const c_char,
) -> CResultString {
    list_transfers(wallet, asset_id).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_list_unspents(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    settled_only: bool,
    skip_sync: bool,
) -> CResultString {
    list_unspents(wallet, online, settled_only, skip_sync).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_new_wallet(wallet_data: *const c_char) -> CResult {
    new_wallet(wallet_data).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_refresh(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    asset_id_opt: *const c_char,
    filter: *const c_char,
    skip_sync: bool,
) -> CResultString {
    refresh(wallet, online, asset_id_opt, filter, skip_sync).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_restore_backup(
    backup_path: *const c_char,
    password: *const c_char,
    target_dir: *const c_char,
) -> CResult {
    restore_backup(backup_path, password, target_dir).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_restore_keys(
    bitcoin_network: *const c_char,
    mnemonic: *const c_char,
) -> CResultString {
    restore_keys(bitcoin_network, mnemonic).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_send(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    recipient_map: *const c_char,
    donation: bool,
    fee_rate: *const c_char,
    min_confirmations: *const c_char,
    skip_sync: bool,
) -> CResultString {
    send(
        wallet,
        online,
        recipient_map,
        donation,
        fee_rate,
        min_confirmations,
        skip_sync,
    )
    .into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_send_btc(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    address: *const c_char,
    amount: *const c_char,
    fee_rate: *const c_char,
    skip_sync: bool,
) -> CResultString {
    send_btc(wallet, online, address, amount, fee_rate, skip_sync).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_sign_psbt(
    wallet: &COpaqueStruct,
    unsigned_psbt: *const c_char,
) -> CResultString {
    sign_psbt(wallet, unsigned_psbt).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_sync(wallet: &COpaqueStruct, online: &COpaqueStruct) -> CResultString {
    sync(wallet, online).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgblib_witness_receive(
    wallet: &COpaqueStruct,
    asset_id_opt: *const c_char,
    assignment: *const c_char,
    duration_seconds_opt: *const c_char,
    transport_endpoints: *const c_char,
    min_confirmations: *const c_char,
) -> CResultString {
    witness_receive(
        wallet,
        asset_id_opt,
        assignment,
        duration_seconds_opt,
        transport_endpoints,
        min_confirmations,
    )
    .into()
}
