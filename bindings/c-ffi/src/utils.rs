use super::*;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("Error converting JSON: {0}")]
    JSONConversion(#[from] serde_json::Error),

    #[error("Error from rgb-lib: {0}")]
    RgbLib(#[from] RgbLibError),

    #[error("Type mismatch")]
    TypeMismatch,
}

impl COpaqueStruct {
    fn new<T: 'static>(other: T) -> Self {
        let mut hasher = DefaultHasher::new();
        TypeId::of::<T>().hash(&mut hasher);
        let ty = hasher.finish();

        COpaqueStruct {
            ptr: Box::into_raw(Box::new(other)) as *const c_void,
            ty,
        }
    }

    fn raw<T>(ptr: *const T) -> Self {
        COpaqueStruct {
            ptr: ptr as *const c_void,
            ty: 0,
        }
    }
}

trait CReturnType: Sized + 'static {
    fn from_opaque(other: &COpaqueStruct) -> Result<&mut Self, Error> {
        let mut hasher = DefaultHasher::new();
        TypeId::of::<Self>().hash(&mut hasher);
        let ty = hasher.finish();

        if other.ty != ty {
            return Err(Error::TypeMismatch);
        }

        let boxed = unsafe { Box::from_raw(other.ptr.clone() as *mut Self) };
        Ok(Box::leak(boxed))
    }
}
impl CReturnType for Wallet {}
impl CReturnType for Online {}

impl<T: 'static, E> From<Result<T, E>> for CResult
where
    E: std::fmt::Debug,
{
    fn from(other: Result<T, E>) -> Self {
        match other {
            Ok(d) => CResult {
                result: CResultValue::Ok,
                inner: COpaqueStruct::new(d),
            },
            Err(e) => CResult {
                result: CResultValue::Err,
                inner: COpaqueStruct::raw(string_to_ptr(format!("{:?}", e))),
            },
        }
    }
}

impl From<Result<String, Error>> for CResultString
where
    Error: std::fmt::Debug,
{
    fn from(other: Result<String, Error>) -> Self {
        match other {
            Ok(d) => CResultString {
                result: CResultValue::Ok,
                inner: string_to_ptr(d),
            },
            Err(e) => CResultString {
                result: CResultValue::Err,
                inner: string_to_ptr(format!("{:?}", e)),
            },
        }
    }
}

fn convert_optional_number<T: serde::de::DeserializeOwned>(
    ptr: *const c_char,
) -> Result<Option<T>, Error> {
    Ok(if let Some(num_str) = convert_optional_string(ptr) {
        serde_json::from_str(&num_str)?
    } else {
        None
    })
}

fn convert_optional_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        None
    } else {
        Some(ptr_to_string(ptr))
    }
}

fn ptr_to_string(ptr: *const c_char) -> String {
    unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() }
}

fn string_to_ptr(other: String) -> *mut c_char {
    let cstr = match CString::new(other) {
        Ok(cstr) => cstr,
        Err(_) => CString::new(String::from(
            "Error converting string: contains a null-char",
        ))
        .unwrap(),
    };

    cstr.into_raw()
}

pub(crate) fn blind_receive(
    wallet: &COpaqueStruct,
    asset_id_opt: *const c_char,
    amount_opt: *const c_char,
    duration_seconds_opt: *const c_char,
    transport_endpoints: *const c_char,
    min_confirmations: c_uchar,
) -> Result<String, Error> {
    let wallet = Wallet::from_opaque(wallet)?;
    let transport_endpoints: Vec<String> =
        serde_json::from_str(&ptr_to_string(transport_endpoints))?;
    let asset_id = convert_optional_string(asset_id_opt);
    let amount: Option<u64> = convert_optional_number(amount_opt)?;
    let duration_seconds: Option<u32> = convert_optional_number(duration_seconds_opt)?;
    let res = wallet.blind_receive(
        asset_id,
        amount,
        duration_seconds,
        transport_endpoints,
        min_confirmations,
    )?;
    Ok(serde_json::to_string(&res)?)
}

pub(crate) fn create_utxos(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    up_to: bool,
    num_opt: *const c_char,
    size_opt: *const c_char,
    fee_rate: c_float,
) -> Result<String, Error> {
    let wallet = Wallet::from_opaque(wallet)?;
    let online = Online::from_opaque(online)?;
    let num: Option<u8> = convert_optional_number(num_opt)?;
    let size: Option<u32> = convert_optional_number(size_opt)?;
    let res = wallet.create_utxos((*online).clone(), up_to, num, size, fee_rate)?;
    Ok(serde_json::to_string(&res)?)
}

pub(crate) fn generate_keys(bitcoin_network: *const c_char) -> Result<String, Error> {
    let bitcoin_network = BitcoinNetwork::from_str(&ptr_to_string(bitcoin_network))?;
    let res = rgb_lib::generate_keys(bitcoin_network.into());
    Ok(serde_json::to_string(&res)?)
}

pub(crate) fn get_address(wallet: &COpaqueStruct) -> Result<String, Error> {
    let wallet = Wallet::from_opaque(wallet)?;
    Ok(wallet.get_address()?)
}

pub(crate) fn go_online(
    wallet: &COpaqueStruct,
    skip_consistency_check: bool,
    electrum_url: *const c_char,
) -> Result<Online, Error> {
    let wallet = Wallet::from_opaque(wallet)?;
    Ok(wallet.go_online(skip_consistency_check, ptr_to_string(electrum_url))?)
}

pub(crate) fn issue_asset_cfa(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    name: *const c_char,
    details_opt: *const c_char,
    precision: c_uchar,
    amounts: *const c_char,
    file_path_opt: *const c_char,
) -> Result<String, Error> {
    let wallet = Wallet::from_opaque(wallet)?;
    let online = Online::from_opaque(online)?;
    let amounts: Vec<u64> = serde_json::from_str(&ptr_to_string(amounts))?;
    let details = convert_optional_string(details_opt);
    let file_path = convert_optional_string(file_path_opt);
    let res = wallet.issue_asset_cfa(
        (*online).clone().into(),
        ptr_to_string(name),
        details,
        precision,
        amounts,
        file_path,
    )?;
    Ok(serde_json::to_string(&res)?)
}

pub(crate) fn issue_asset_nia(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    ticker: *const c_char,
    name: *const c_char,
    precision: c_uchar,
    amounts: *const c_char,
) -> Result<String, Error> {
    let wallet = Wallet::from_opaque(wallet)?;
    let online = Online::from_opaque(online)?;
    let amounts: Vec<u64> = serde_json::from_str(&ptr_to_string(amounts))?;
    let res = wallet.issue_asset_nia(
        (*online).clone(),
        ptr_to_string(ticker),
        ptr_to_string(name),
        precision,
        amounts,
    )?;
    Ok(serde_json::to_string(&res)?)
}

pub(crate) fn issue_asset_uda(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    ticker: *const c_char,
    name: *const c_char,
    details_opt: *const c_char,
    precision: c_uchar,
    media_file_path_opt: *const c_char,
    attachments_file_paths: *const c_char,
) -> Result<String, Error> {
    let wallet = Wallet::from_opaque(wallet)?;
    let online = Online::from_opaque(online)?;
    let details = convert_optional_string(details_opt);
    let media_file_path = convert_optional_string(media_file_path_opt);
    let attachments_file_paths: Vec<String> =
        serde_json::from_str(&ptr_to_string(attachments_file_paths))?;
    let res = wallet.issue_asset_uda(
        (*online).clone().into(),
        ptr_to_string(ticker),
        ptr_to_string(name),
        details,
        precision,
        media_file_path,
        attachments_file_paths,
    )?;
    Ok(serde_json::to_string(&res)?)
}

pub(crate) fn list_assets(
    wallet: &COpaqueStruct,
    filter_asset_schemas: *const c_char,
) -> Result<String, Error> {
    let wallet = Wallet::from_opaque(wallet)?;
    let filter_asset_schemas: Vec<AssetSchema> =
        serde_json::from_str(&ptr_to_string(filter_asset_schemas))?;
    let res = wallet.list_assets(filter_asset_schemas)?;
    Ok(serde_json::to_string(&res)?)
}

pub(crate) fn new_wallet(wallet_data: *const c_char) -> Result<Wallet, Error> {
    let wallet_data: WalletData = serde_json::from_str(&ptr_to_string(wallet_data))?;
    Ok(Wallet::new(wallet_data)?)
}

pub(crate) fn refresh(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    asset_id_opt: *const c_char,
    filter: *const c_char,
) -> Result<String, Error> {
    let wallet = Wallet::from_opaque(wallet)?;
    let online = Online::from_opaque(online)?;
    let filter: Vec<RefreshFilter> = serde_json::from_str(&ptr_to_string(filter))?;
    let asset_id = convert_optional_string(asset_id_opt);
    let res = wallet.refresh((*online).clone(), asset_id, filter)?;
    Ok(serde_json::to_string(&res)?)
}

pub(crate) fn send(
    wallet: &COpaqueStruct,
    online: &COpaqueStruct,
    recipient_map: *const c_char,
    donation: bool,
    fee_rate: c_float,
    min_confirmations: c_uchar,
) -> Result<String, Error> {
    let wallet = Wallet::from_opaque(wallet)?;
    let online = Online::from_opaque(online)?;
    let recipient_map: HashMap<String, Vec<Recipient>> =
        serde_json::from_str(&ptr_to_string(recipient_map))?;
    let res = wallet.send(
        (*online).clone(),
        recipient_map,
        donation,
        fee_rate,
        min_confirmations,
    )?;
    Ok(serde_json::to_string(&res)?)
}

pub(crate) fn witness_receive(
    wallet: &COpaqueStruct,
    asset_id_opt: *const c_char,
    amount_opt: *const c_char,
    duration_seconds_opt: *const c_char,
    transport_endpoints: *const c_char,
    min_confirmations: c_uchar,
) -> Result<String, Error> {
    let wallet = Wallet::from_opaque(wallet)?;
    let transport_endpoints: Vec<String> =
        serde_json::from_str(&ptr_to_string(transport_endpoints))?;
    let asset_id = convert_optional_string(asset_id_opt);
    let amount: Option<u64> = convert_optional_number(amount_opt)?;
    let duration_seconds: Option<u32> = convert_optional_number(duration_seconds_opt)?;
    let res = wallet.witness_receive(
        asset_id,
        amount,
        duration_seconds,
        transport_endpoints,
        min_confirmations,
    )?;
    Ok(serde_json::to_string(&res)?)
}
