// Utility module for multisig tests
//
// Objects are split into sections for better readability

use super::*;

// ----------------------------------------
// multisig hub
// ----------------------------------------

#[derive(Deserialize, Serialize)]
struct AppConfig {
    cosigner_xpubs: Vec<String>,
    threshold_colored: u8,
    threshold_vanilla: u8,
    root_public_key: String,
    rgb_lib_version: String,
}

pub(super) enum Role {
    Cosigner(String),
    WatchOnly,
}

pub(super) fn create_token(
    root: &KeyPair,
    role: Role,
    expiration_date: Option<DateTime<Utc>>,
) -> String {
    let mut authority = biscuit!("");
    match role {
        Role::Cosigner(xpub) => {
            authority = biscuit_merge!(authority, r#"role("cosigner"); xpub({xpub});"#);
        }
        Role::WatchOnly => {
            authority = biscuit_merge!(authority, r#"role("watch-only");"#);
        }
    }
    if let Some(expiration_date) = expiration_date {
        let exp = date(&expiration_date.into());
        authority = biscuit_merge!(authority, r#"check if time($t), $t < {exp};"#);
    }
    authority.build(root).unwrap().to_base64().unwrap()
}

pub(super) fn write_hub_config(
    cosigner_xpubs: &[String],
    threshold_colored: u8,
    threshold_vanilla: u8,
    root_public_key: String,
    rgb_lib_version: Option<String>,
) {
    let rgb_lib_version = rgb_lib_version.unwrap_or_else(local_rgb_lib_version);
    let config = AppConfig {
        cosigner_xpubs: cosigner_xpubs.to_vec(),
        threshold_colored,
        threshold_vanilla,
        root_public_key,
        rgb_lib_version,
    };
    let conf_path = PathBuf::from(join_with_sep(&HUB_DIR_PARTS)).join("config.toml");
    confy::store_path(conf_path, config).unwrap();
}

// ----------------------------------------
// sanitization
// ----------------------------------------
//
// this section defines and implements the Sanitizable trait, which allows to sanitize data
// structures containing variable data (file paths) to uniform them, allowing comparisons via
// PartialEq and thus assert_eq!, instead of having to check each field separately

pub(super) trait Sanitizable {
    fn sanitize(&mut self) {}
}

// replace the variable part of file paths with a fixed string
fn sanitize_path(path: &str) -> String {
    regex::Regex::new(r"tmp/[^/]*")
        .unwrap()
        .replace(path, "tmp/variable")
        .to_string()
}

// convenience macro to avoid duplicate implementation for Token and TokenLight
macro_rules! define_token_sanitizer {
    ($token:ty) => {
        impl Sanitizable for $token {
            fn sanitize(&mut self) {
                self.media.as_mut().map(|m| {
                    m.file_path = sanitize_path(&m.file_path);
                });
                for att in self.attachments.values_mut() {
                    att.file_path = sanitize_path(&att.file_path);
                }
            }
        }
    };
}
define_token_sanitizer!(Token);
define_token_sanitizer!(TokenLight);

impl Sanitizable for NoDetails {
    fn sanitize(&mut self) {}
}

impl Sanitizable for InflateDetails {
    fn sanitize(&mut self) {
        self.fascia_path = sanitize_path(&self.fascia_path);
    }
}

impl Sanitizable for SendDetails {
    fn sanitize(&mut self) {
        self.fascia_path = sanitize_path(&self.fascia_path);
    }
}

impl Sanitizable for Operation {
    fn sanitize(&mut self) {
        match self {
            Operation::InflationCompleted {
                txid: _,
                details,
                status: _,
            } => {
                details.sanitize();
            }
            Operation::InflationDiscarded { details, status: _ } => {
                details.sanitize();
            }
            Operation::InflationPending { details, status: _ } => {
                details.sanitize();
            }
            Operation::InflationToReview {
                psbt: _,
                details,
                status: _,
            } => {
                details.sanitize();
            }
            Operation::SendCompleted {
                txid: _,
                details,
                status: _,
            } => {
                details.sanitize();
            }
            Operation::SendDiscarded { details, status: _ } => {
                details.sanitize();
            }
            Operation::SendPending { details, status: _ } => {
                details.sanitize();
            }
            Operation::SendToReview {
                psbt: _,
                details,
                status: _,
            } => {
                details.sanitize();
            }
            _ => {}
        }
    }
}

fn sanitize_meta(meta: &mut Metadata) {
    if let Some(t) = meta.token.as_mut() {
        t.sanitize()
    }
}

// ----------------------------------------
// wallets and parties
// ----------------------------------------

// singlesig party (allows uniform access to some functionality via SigParty trait)
pub(super) struct SinglesigParty<'a> {
    pub(super) wallet: &'a mut Wallet,
    pub(super) online: Online,
}

// multisig party to be used for cosigners
pub(super) struct MultisigParty<'a> {
    pub(super) signer: &'a Wallet,
    pub(super) multisig: &'a mut MultisigWallet,
    pub(super) online: Online,
    pub(super) xpub: &'a str,
}

// multisig party to be used for watch-only parties
pub(super) struct WatchOnlyParty<'a> {
    pub(super) multisig: &'a mut MultisigWallet,
    pub(super) online: Online,
}

// cosigner-specific functionality
impl<'a> MultisigParty<'a> {
    fn ack(&mut self, psbt: &str, op_idx: i32) -> OperationInfo {
        self.respond_to_operation(op_idx, RespondToOperation::Ack(psbt.to_string()))
    }

    pub(super) fn sign(&mut self, psbt: &str) -> String {
        self.signer.sign_psbt(psbt.to_string(), None).unwrap()
    }

    fn sign_and_ack(&mut self, psbt: &str, op_idx: i32) -> OperationInfo {
        println!(
            "sign and ack {op_idx} {}",
            self.multisig_mut().get_wallet_data().data_dir
        );
        let signed = self.sign(psbt);
        self.ack(&signed, op_idx)
    }
}

// convenience macro to instantiate MultisigParty or WatchOnlyParty based on the number of params
macro_rules! ms_party {
    ($signer:expr, $multisig:expr, $online:expr, $xpub:expr) => {
        MultisigParty {
            signer: $signer,
            multisig: $multisig,
            online: $online,
            xpub: $xpub,
        }
    };
    ($multisig:expr, $online:expr) => {
        WatchOnlyParty {
            multisig: $multisig,
            online: $online,
        }
    };
}

// convenience macro to instantiate SinglesigParty
macro_rules! party {
    ($wallet:expr, $online:expr) => {
        SinglesigParty {
            wallet: $wallet,
            online: $online,
        }
    };
}

// convenience trait to allow uniform access to common functionality from all parties
pub(super) trait SigParty {
    fn get_asset_balance(&self, asset_id: &str) -> Balance;

    fn list_transfers(&self, asset_id: Option<&str>) -> Vec<Transfer>;

    fn refresh(&mut self, asset_id: Option<&str>);

    fn get_data_dir(&self) -> String;
}

impl SigParty for MultisigParty<'_> {
    fn get_asset_balance(&self, asset_id: &str) -> Balance {
        self.multisig
            .get_asset_balance(asset_id.to_string())
            .unwrap()
    }

    fn list_transfers(&self, asset_id: Option<&str>) -> Vec<Transfer> {
        test_list_transfers(self.multisig, asset_id)
    }

    fn refresh(&mut self, asset_id: Option<&str>) {
        wait_for_refresh(self.multisig, self.online, asset_id, None);
    }

    fn get_data_dir(&self) -> String {
        self.multisig.internals.wallet_data.data_dir.clone()
    }
}

impl SigParty for SinglesigParty<'_> {
    fn get_asset_balance(&self, asset_id: &str) -> Balance {
        self.wallet.get_asset_balance(asset_id.to_string()).unwrap()
    }

    fn list_transfers(&self, asset_id: Option<&str>) -> Vec<Transfer> {
        test_list_transfers(self.wallet, asset_id)
    }

    fn refresh(&mut self, asset_id: Option<&str>) {
        wait_for_refresh(self.wallet, self.online, asset_id, None);
    }

    fn get_data_dir(&self) -> String {
        self.wallet.internals.wallet_data.data_dir.clone()
    }
}

pub(super) fn get_test_ms_wallet(keys: &MultisigKeys, dir: String) -> MultisigWallet {
    let data_dir = get_test_data_dir_path()
        .join(dir)
        .to_string_lossy()
        .to_string();
    let _ = fs::create_dir_all(&data_dir);
    let wallet = MultisigWallet::new(
        WalletData {
            data_dir,
            bitcoin_network: BitcoinNetwork::Regtest,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
            supported_schemas: AssetSchema::VALUES.to_vec(),
        },
        keys.clone(),
    )
    .unwrap();
    println!(
        "multisig wallet directory: {:?}",
        test_get_wallet_dir(&wallet)
    );
    wallet
}

pub(super) fn ms_go_online_res(wallet: &mut MultisigWallet, token: &str) -> Result<Online, Error> {
    wallet.go_online(
        false,
        ELECTRUM_URL.to_string(),
        MULTISIG_HUB_URL.to_string(),
        token.to_string(),
    )
}

pub(super) fn ms_go_online(wallet: &mut MultisigWallet, token: &str) -> Online {
    ms_go_online_res(wallet, token).unwrap()
}

pub(super) fn watch_only_wallet_sync(wallet: &mut WatchOnlyParty) {
    let last_processed_op = wallet
        .multisig_ref()
        .get_local_last_processed_operation_idx()
        .unwrap();
    assert_eq!(last_processed_op, 0);
    let hub_info = wallet.hub_info();
    assert_eq!(hub_info.user_role, UserRole::WatchOnly);
    assert!(hub_info.last_operation_idx.is_some());
    wallet.sync_to_head();
    wallet.assert_up_to_date();
}

fn local_rgb_lib_version() -> String {
    env!("CARGO_PKG_VERSION")
        .split('.')
        .take(2)
        .collect::<Vec<_>>()
        .join(".")
}

// ----------------------------------------
// multisig operations
// ----------------------------------------

// global operation counter
// use only in serial tests and initialize to 0 at test start for a deterministic behavior
static OP_COUNTER: AtomicU64 = AtomicU64::new(0);

// convenience trait exposing multisig functionality
pub(super) trait MultisigOps {
    fn multisig_mut(&mut self) -> &mut MultisigWallet;
    fn multisig_ref(&self) -> &MultisigWallet;
    fn online(&self) -> Online;

    fn assert_up_to_date(&mut self) {
        if self.sync_opt().is_some() {
            panic!(
                "wallet {} is not up to date",
                self.multisig_ref().internals.wallet_data.data_dir
            );
        }
        let last_processed_op = self
            .multisig_ref()
            .get_local_last_processed_operation_idx()
            .unwrap();
        let current_op = OP_COUNTER.load(Ordering::SeqCst) as i32;
        assert_eq!(last_processed_op, current_op);
    }

    fn bak_info_opt(&mut self) -> Option<Option<DbBackupInfo>> {
        self.multisig_ref().database().get_backup_info().ok()
    }

    fn bak_ts(&mut self) -> String {
        // using last_operation_timestamp instead of last_backup_timestamp for convenience
        self.bak_info_opt()
            .unwrap()
            .unwrap()
            .last_operation_timestamp
    }

    fn hub_info(&mut self) -> HubInfo {
        let online = self.online();
        self.multisig_mut().hub_info(online).unwrap()
    }

    fn blind_receive(&mut self) -> ReceiveData {
        println!(
            "blind_receive {}",
            self.multisig_mut().get_wallet_data().data_dir
        );
        let bt_before = self.bak_ts();
        let res = self.blind_receive_res().unwrap();
        assert!(self.bak_ts() > bt_before);
        let op_idx = op_counter_bump();
        println!("initiated blind_receive with operation ID {op_idx}");
        res
    }

    fn blind_receive_res(&mut self) -> Result<ReceiveData, Error> {
        let online = self.online();
        self.multisig_mut().blind_receive(
            online,
            None,
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
    }

    fn create_utxos_init(
        &mut self,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: u64,
    ) -> InitOperationResult {
        println!(
            "create_utxos init {}",
            self.multisig_mut().get_wallet_data().data_dir
        );
        let bt_before = self.bak_ts();
        let res = self
            .create_utxos_init_res(up_to, num, size, fee_rate)
            .unwrap();
        assert!(self.bak_ts() > bt_before);
        let op_idx = op_counter_bump();
        assert_eq!(res.operation_idx, op_idx);
        println!(
            "initiated create_utxos with operation ID {}",
            res.operation_idx
        );
        res
    }

    fn create_utxos_init_res(
        &mut self,
        up_to: bool,
        num: Option<u8>,
        size: Option<u32>,
        fee_rate: u64,
    ) -> Result<InitOperationResult, Error> {
        let online = self.online();
        self.multisig_mut()
            .create_utxos_init(online, up_to, num, size, fee_rate, false)
    }

    fn get_address(&mut self) -> String {
        let online = self.online();
        self.multisig_mut().get_address(online).unwrap()
    }

    fn get_op(&self, idx: i32) -> OperationResponse {
        self.multisig_ref()
            .hub_client()
            .get_operation_by_idx(idx)
            .unwrap()
            .unwrap()
    }

    fn get_op_and_files(&self, op_idx: i32) -> (OperationResponse, Vec<FileResponse>) {
        let op = self.get_op(op_idx);
        let files = self.get_or_download_files(op.files.clone());
        (op, files)
    }

    fn get_or_download_files(&self, files: Vec<FileMetadata>) -> Vec<FileResponse> {
        self.multisig_ref().get_or_download_files(files).unwrap()
    }

    fn inflate_init(&mut self, asset_id: &str, inflation_amounts: &[u64]) -> InitOperationResult {
        println!(
            "inflate init {}",
            self.multisig_mut().get_wallet_data().data_dir
        );
        let bt_before = self.bak_ts();
        let res = self.inflate_init_res(asset_id, inflation_amounts).unwrap();
        assert!(self.bak_ts() > bt_before);
        op_counter_bump();
        println!("initiated inflate with operation ID {}", res.operation_idx);
        res
    }

    fn inflate_init_res(
        &mut self,
        asset_id: &str,
        inflation_amounts: &[u64],
    ) -> Result<InitOperationResult, Error> {
        let online = self.online();
        self.multisig_mut().inflate_init(
            online,
            asset_id.to_string(),
            inflation_amounts.to_vec(),
            FEE_RATE,
            1,
        )
    }

    fn issue_asset_cfa(&mut self, amounts: Option<&[u64]>, file_path: Option<String>) -> AssetCFA {
        println!(
            "issue CFA asset {}",
            self.multisig_mut().get_wallet_data().data_dir
        );
        let bt_before = self.bak_ts();
        let res = self.issue_asset_cfa_res(amounts, file_path).unwrap();
        assert!(self.bak_ts() > bt_before);
        op_counter_bump();
        println!("issued CFA asset with ID {}", res.asset_id);
        res
    }

    fn issue_asset_cfa_res(
        &mut self,
        amounts: Option<&[u64]>,
        file_path: Option<String>,
    ) -> Result<AssetCFA, Error> {
        let amounts = amounts.map_or_else(|| vec![AMOUNT], |a| a.to_vec());
        let online = self.online();
        self.multisig_mut().issue_asset_cfa(
            online,
            NAME.to_string(),
            Some(DETAILS.to_string()),
            PRECISION,
            amounts,
            file_path,
        )
    }

    fn issue_asset_ifa(
        &mut self,
        amounts: Option<&[u64]>,
        inflation_amounts: Option<&[u64]>,
        reject_list_url: Option<String>,
    ) -> AssetIFA {
        println!(
            "issue IFA asset {}",
            self.multisig_mut().get_wallet_data().data_dir
        );
        let bt_before = self.bak_ts();
        let res = self
            .issue_asset_ifa_res(amounts, inflation_amounts, reject_list_url)
            .unwrap();
        assert!(self.bak_ts() > bt_before);
        op_counter_bump();
        println!("issued IFA asset with ID {}", res.asset_id);
        res
    }

    fn issue_asset_ifa_res(
        &mut self,
        amounts: Option<&[u64]>,
        inflation_amounts: Option<&[u64]>,
        reject_list_url: Option<String>,
    ) -> Result<AssetIFA, Error> {
        let amounts = amounts.map_or_else(|| vec![AMOUNT], |a| a.to_vec());
        let inflation_amounts =
            inflation_amounts.map_or_else(|| vec![AMOUNT_INFLATION], |a| a.to_vec());
        let online = self.online();
        self.multisig_mut().issue_asset_ifa(
            online,
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            amounts,
            inflation_amounts,
            reject_list_url,
        )
    }

    fn issue_asset_nia(&mut self, amounts: Option<&[u64]>) -> AssetNIA {
        println!(
            "issue NIA asset {}",
            self.multisig_mut().get_wallet_data().data_dir
        );
        let bt_before = self.bak_ts();
        let res = self.issue_asset_nia_res(amounts).unwrap();
        assert!(self.bak_ts() > bt_before);
        op_counter_bump();
        println!("issued NIA asset with ID {}", res.asset_id);
        res
    }

    fn issue_asset_nia_res(&mut self, amounts: Option<&[u64]>) -> Result<AssetNIA, Error> {
        let amounts = amounts.map_or_else(|| vec![AMOUNT], |a| a.to_vec());
        let online = self.online();
        self.multisig_mut().issue_asset_nia(
            online,
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            amounts,
        )
    }

    fn issue_asset_uda(
        &mut self,
        details: Option<&str>,
        media_file_path: Option<&str>,
        attachments_file_paths: Vec<&str>,
    ) -> AssetUDA {
        println!(
            "issue UDA asset {}",
            self.multisig_mut().get_wallet_data().data_dir
        );
        let bt_before = self.bak_ts();
        let res = self
            .issue_asset_uda_res(details, media_file_path, attachments_file_paths)
            .unwrap();
        assert!(self.bak_ts() > bt_before);
        op_counter_bump();
        println!("issued UDA asset with ID {}", res.asset_id);
        res
    }

    fn issue_asset_uda_res(
        &mut self,
        details: Option<&str>,
        media_file_path: Option<&str>,
        attachments_file_paths: Vec<&str>,
    ) -> Result<AssetUDA, Error> {
        let online = self.online();
        self.multisig_mut().issue_asset_uda(
            online,
            TICKER.to_string(),
            NAME.to_string(),
            details.map(|d| d.to_string()),
            PRECISION,
            media_file_path.map(|m| m.to_string()),
            attachments_file_paths
                .iter()
                .map(|a| a.to_string())
                .collect(),
        )
    }

    fn list_unspents(&mut self, settled_only: bool) -> Vec<Unspent> {
        let online = self.online();
        test_list_unspents(self.multisig_mut(), Some(online), settled_only)
    }

    fn nack(&mut self, op_idx: i32) -> OperationInfo {
        println!(
            "nack {op_idx} {}",
            self.multisig_mut().get_wallet_data().data_dir
        );
        self.respond_to_operation(op_idx, RespondToOperation::Nack)
    }

    fn nack_res(&mut self, op_idx: i32) -> Result<OperationInfo, Error> {
        self.respond_to_operation_res(op_idx, RespondToOperation::Nack)
    }

    fn respond_to_operation(&mut self, op_idx: i32, response: RespondToOperation) -> OperationInfo {
        self.respond_to_operation_res(op_idx, response).unwrap()
    }

    fn respond_to_operation_res(
        &mut self,
        op_idx: i32,
        response: RespondToOperation,
    ) -> Result<OperationInfo, Error> {
        let online = self.online();
        self.multisig_mut()
            .respond_to_operation(online, op_idx, response)
    }

    fn send_btc_init(&mut self, address: &str, amount: u64) -> InitOperationResult {
        println!(
            "send_btc init {}",
            self.multisig_mut().get_wallet_data().data_dir
        );
        let bt_before = self.bak_ts();
        let res = self.send_btc_init_res(address, amount).unwrap();
        assert!(self.bak_ts() > bt_before);
        let op_idx = op_counter_bump();
        assert_eq!(res.operation_idx, op_idx);
        println!("initiated send_btc with operation ID {}", res.operation_idx);
        res
    }

    fn send_btc_init_res(
        &mut self,
        address: &str,
        amount: u64,
    ) -> Result<InitOperationResult, Error> {
        let online = self.online();
        self.multisig_mut()
            .send_btc_init(online, address.to_string(), amount, FEE_RATE, false)
    }

    fn send_init(&mut self, recipient_map: HashMap<String, Vec<Recipient>>) -> InitOperationResult {
        println!(
            "send init {}",
            self.multisig_mut().get_wallet_data().data_dir
        );
        let bt_before = self.bak_ts();
        let res = self.send_init_res(recipient_map).unwrap();
        assert!(self.bak_ts() > bt_before);
        let op_idx = op_counter_bump();
        assert_eq!(res.operation_idx, op_idx);
        println!("initiated send with operation ID {}", res.operation_idx);
        res
    }

    fn send_init_res(
        &mut self,
        recipient_map: HashMap<String, Vec<Recipient>>,
    ) -> Result<InitOperationResult, Error> {
        let online = self.online();
        self.multisig_mut()
            .send_init(online, recipient_map, false, FEE_RATE, 1, None)
    }

    fn sync(&mut self) -> OperationInfo {
        self.sync_opt().unwrap()
    }

    fn sync_opt(&mut self) -> Option<OperationInfo> {
        println!("sync {}", self.multisig_mut().get_wallet_data().data_dir);
        let online = self.online();
        self.multisig_mut().sync_with_hub(online).unwrap()
    }

    fn sync_to_head(&mut self) {
        let online = self.online();
        let last_processed = self
            .multisig_mut()
            .get_local_last_processed_operation_idx()
            .unwrap();
        let last_hub_operation = self
            .multisig_mut()
            .hub_info(online)
            .unwrap()
            .last_operation_idx
            .unwrap();
        assert!(last_hub_operation > last_processed);
        for i in (last_processed + 1)..=last_hub_operation {
            println!("syncing operation {i}");
            let op_info = self.sync();
            assert_eq!(op_info.operation_idx, i);
        }
        let final_processed = self
            .multisig_mut()
            .get_local_last_processed_operation_idx()
            .unwrap();
        assert_eq!(final_processed, last_hub_operation);
        self.assert_up_to_date();
    }

    fn witness_receive(&mut self) -> ReceiveData {
        println!(
            "witness_receive {}",
            self.multisig_mut().get_wallet_data().data_dir
        );
        let bt_before = self.bak_ts();
        let res = self.witness_receive_res().unwrap();
        assert!(self.bak_ts() > bt_before);
        op_counter_bump();
        res
    }

    fn witness_receive_res(&mut self) -> Result<ReceiveData, Error> {
        let online = self.online();
        self.multisig_mut().witness_receive(
            online,
            None,
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
    }
}

impl<'a> MultisigOps for MultisigParty<'a> {
    fn multisig_mut(&mut self) -> &mut MultisigWallet {
        self.multisig
    }

    fn multisig_ref(&self) -> &MultisigWallet {
        self.multisig
    }

    fn online(&self) -> Online {
        self.online
    }
}

impl<'a> MultisigOps for WatchOnlyParty<'a> {
    fn multisig_mut(&mut self) -> &mut MultisigWallet {
        self.multisig
    }

    fn multisig_ref(&self) -> &MultisigWallet {
        self.multisig
    }

    fn online(&self) -> Online {
        self.online
    }
}

// convenience enum to allow for generic asset issuance
pub(super) enum IssuedAsset {
    Cfa(AssetCFA),
    Nia(AssetNIA),
    Uda(AssetUDA),
    Ifa(AssetIFA),
}

impl IssuedAsset {
    fn asset_id(&self) -> &str {
        match self {
            IssuedAsset::Cfa(a) => &a.asset_id,
            IssuedAsset::Nia(a) => &a.asset_id,
            IssuedAsset::Uda(a) => &a.asset_id,
            IssuedAsset::Ifa(a) => &a.asset_id,
        }
    }
}

pub(super) fn issue_asset(
    initiator: &mut MultisigParty,
    others: &mut [&mut MultisigParty],
    schema: AssetSchema,
    amounts: Option<&[u64]>,
    inflation_amounts: Option<&[u64]>,
) -> IssuedAsset {
    let image_str = ["tests", "qrcode.png"].join(MAIN_SEPARATOR_STR);

    let (asset, supply, inflation_supply) = match schema {
        AssetSchema::Cfa => {
            let amounts = amounts.expect("amounts required for CFA");
            println!("issue CFA asset with amounts: {amounts:?}");
            let asset = initiator.issue_asset_cfa(Some(amounts), Some(FILE_STR.to_string()));
            let supply = amounts.iter().sum::<u64>();
            assert_eq!(asset.balance.settled, supply);
            (IssuedAsset::Cfa(asset), supply, None)
        }
        AssetSchema::Nia => {
            let amounts = amounts.expect("amounts required for NIA");
            println!("issue NIA asset with amounts: {amounts:?}");
            let asset = initiator.issue_asset_nia(Some(amounts));
            let supply = amounts.iter().sum::<u64>();
            assert_eq!(asset.balance.settled, supply);
            (IssuedAsset::Nia(asset), supply, None)
        }
        AssetSchema::Uda => {
            println!("issue UDA asset");
            let asset = initiator.issue_asset_uda(
                Some(DETAILS),
                Some(FILE_STR),
                vec![&image_str, FILE_STR],
            );
            assert_eq!(asset.balance.settled, 1);
            (IssuedAsset::Uda(asset), 1, None)
        }
        AssetSchema::Ifa => {
            println!("issue IFA asset with amounts: {amounts:?}, inflation: {inflation_amounts:?}");
            let asset = initiator.issue_asset_ifa(amounts, inflation_amounts, None);
            let supply = amounts
                .expect("amounts required for IFA")
                .iter()
                .sum::<u64>();
            assert_eq!(asset.balance.settled, supply);
            let inflation_supply = inflation_amounts
                .expect("inflation amounts required for IFA")
                .iter()
                .sum::<u64>();
            (IssuedAsset::Ifa(asset), supply, Some(inflation_supply))
        }
    };

    check_issuance(initiator, others, asset.asset_id(), schema);

    let mut all_wallets: Vec<&mut MultisigWallet> = vec![initiator.multisig_mut()];
    all_wallets.extend(others.iter_mut().map(|w| w.multisig_mut()));
    check_asset_metadata(
        &all_wallets,
        asset.asset_id(),
        NAME,
        PRECISION,
        supply,
        inflation_supply,
        schema,
    );

    check_all_parties_up_to_date(initiator, others);
    asset
}

fn get_op_psbt(files: &[FileResponse]) -> Psbt {
    let op_psbt_path = files
        .iter()
        .find(|f| matches!(f.r#type, FileType::OperationPsbt))
        .map(|f| &f.filepath)
        .unwrap();
    wallet::multisig::MultisigWallet::read_psbt_from_file(op_psbt_path).unwrap()
}

// ----------------------------------------
// checks
// ----------------------------------------

fn check_all_parties_up_to_date(initiator: &mut MultisigParty, others: &mut [&mut MultisigParty]) {
    initiator.assert_up_to_date();
    for other in others {
        other.assert_up_to_date();
    }
}

pub(super) fn check_asset_balance(
    wallets: &[&impl SigParty],
    asset_id: &str,
    expected: (u64, u64, u64),
) {
    let expected = Balance {
        settled: expected.0,
        future: expected.1,
        spendable: expected.2,
    };
    for wallet in wallets {
        let balance = wallet.get_asset_balance(asset_id);
        if balance != expected {
            panic!(
                "wallet {} balance {balance:?} is not the expected {expected:?}",
                wallet.get_data_dir()
            )
        }
    }
}

fn check_bak_ts_opt(wallet: &mut MultisigParty, before: Option<Option<DbBackupInfo>>, same: bool) {
    if let Some(Some(before)) = before {
        if before.last_operation_timestamp == "0" {
            eprintln!(
                "wallet {} has last operation timestamp 0",
                wallet.get_data_dir()
            );
        }
        if same {
            assert_eq!(wallet.bak_ts(), before.last_operation_timestamp);
        } else {
            assert!(wallet.bak_ts() > before.last_operation_timestamp);
        }
    }
}

fn check_asset_metadata(
    wallets: &[&mut MultisigWallet],
    asset_id: &str,
    name: &str,
    precision: u8,
    supply: u64,
    inflation_supply: Option<u64>,
    schema: AssetSchema,
) {
    let mut content_1: Vec<u8> = vec![];
    let mut media_1: Option<database::entities::media::Model> = None;
    let mut digests_1: Vec<String> = vec![];
    let mut token_1: Option<TokenLight> = None;
    let mut token_db_1: Option<database::entities::token::Model> = None;

    for wallet in wallets {
        let meta = wallet.get_asset_metadata(asset_id.to_string()).unwrap();
        assert_eq!(meta.asset_schema, schema);
        assert_eq!(meta.initial_supply, supply);
        assert_eq!(meta.known_circulating_supply, supply);
        assert_eq!(meta.name, name);
        assert_eq!(meta.precision, precision);
        assert_eq!(meta.reject_list_url, None);
        match schema {
            AssetSchema::Cfa => assert_eq!(meta.ticker, None),
            _ => assert_eq!(meta.ticker, Some(TICKER.to_string())),
        }
        match schema {
            AssetSchema::Ifa => {
                assert_eq!(
                    meta.max_supply,
                    meta.initial_supply
                        + inflation_supply.expect("inflation supply required for IFA")
                );
            }
            _ => assert_eq!(meta.max_supply, supply),
        }

        match schema {
            AssetSchema::Cfa => {
                let asset_db = wallet
                    .database()
                    .get_asset(asset_id.to_string())
                    .unwrap()
                    .unwrap();
                assert!(asset_db.media_idx.is_some());
                let media = wallet
                    .database()
                    .get_media(asset_db.media_idx.unwrap())
                    .unwrap()
                    .unwrap();
                if let Some(ref media_1) = media_1 {
                    assert_eq!(media_1.digest, media.digest);
                } else {
                    media_1 = Some(media.clone());
                }
                let media_file = wallet.media_dir().join(&media.digest);
                assert!(media_file.exists());
                let content = std::fs::read(&media_file).unwrap();
                if !content_1.is_empty() {
                    assert_eq!(content_1, content);
                } else {
                    content_1 = content;
                }
            }
            AssetSchema::Uda => {
                let meta_token = meta.token.unwrap();
                assert_eq!(meta_token.index, UDA_FIXED_INDEX);
                assert_eq!(meta_token.ticker, None);
                assert_eq!(meta_token.name, None);
                assert_eq!(meta_token.details, None);
                assert_eq!(meta_token.embedded_media, None);
                assert!(meta_token.media.is_some());
                assert!(!meta_token.attachments.is_empty());
                assert_eq!(meta_token.reserves, None);

                let asset_db = wallet
                    .database()
                    .get_asset(asset_id.to_string())
                    .unwrap()
                    .unwrap();
                let assets_uda = wallet
                    .list_assets(vec![AssetSchema::Uda])
                    .unwrap()
                    .uda
                    .unwrap();
                let asset_uda = assets_uda.iter().find(|a| a.asset_id == asset_id).unwrap();
                let token = asset_uda.token.clone().unwrap();
                assert_eq!(token.index, UDA_FIXED_INDEX);
                let tokens = wallet.database().iter_tokens().unwrap();
                let token_db = tokens.iter().find(|t| t.asset_idx == asset_db.idx).unwrap();
                if let Some(ref token_db_1) = token_db_1 {
                    assert_eq!(token_db, token_db_1);
                } else {
                    token_db_1 = Some(token_db.clone());
                }
                let token_medias = wallet.database().iter_token_medias().unwrap();
                let token_media_entries: Vec<_> = token_medias
                    .into_iter()
                    .filter(|tm| tm.token_idx == token_db.idx)
                    .collect();
                assert_eq!(token_media_entries.len(), 3);
                let medias = wallet.database().iter_media().unwrap();
                let mut digests: Vec<String> = token_media_entries
                    .iter()
                    .map(|tm| {
                        medias
                            .iter()
                            .find(|m| m.idx == tm.media_idx)
                            .unwrap()
                            .digest
                            .clone()
                    })
                    .collect();
                digests.sort();
                if !digests_1.is_empty() {
                    assert_eq!(digests_1, digests);
                } else {
                    digests_1 = digests;
                }
                let attachments = &token.attachments;
                assert_eq!(attachments.len(), 2);
                if let Some(ref token_1) = token_1 {
                    let mut san_token = token.clone();
                    let mut san_token_1 = token_1.clone();
                    san_token.sanitize();
                    san_token_1.sanitize();
                    assert_eq!(san_token, san_token_1);
                } else {
                    token_1 = Some(token.clone());
                }
            }
            AssetSchema::Nia | AssetSchema::Ifa => {
                assert_eq!(meta.token, None);
            }
        }
    }
}

pub(super) fn check_btc_balance(
    wallets: &mut [&mut MultisigWallet],
    expected_vanilla: (u64, u64, u64),
    expected_colored: (u64, u64, u64),
) {
    let expected = BtcBalance {
        vanilla: Balance {
            settled: expected_vanilla.0,
            future: expected_vanilla.1,
            spendable: expected_vanilla.2,
        },
        colored: Balance {
            settled: expected_colored.0,
            future: expected_colored.1,
            spendable: expected_colored.2,
        },
    };
    for wallet in wallets.iter_mut() {
        let balance = wallet.get_btc_balance(None, true).unwrap();
        if balance != expected {
            panic!(
                "wallet {} BTC balance {balance:?} is not the expected {expected:?}",
                wallet.get_wallet_data().data_dir
            )
        }
    }
}

pub(super) fn check_change_consistency(wlt_a: &mut MultisigParty, wlt_b: &mut MultisigParty) {
    let wlt_a_txos = wlt_a.multisig.database().iter_txos().unwrap();
    let wlt_b_txos = wlt_b.multisig.database().iter_txos().unwrap();
    let wlt_a_colorings = wlt_a.multisig.database().iter_colorings().unwrap();
    let wlt_b_colorings = wlt_b.multisig.database().iter_colorings().unwrap();
    let resolve_change_outpoints = |txos: &[DbTxo], colorings: &[DbColoring]| -> Vec<String> {
        colorings
            .iter()
            .filter(|c| c.r#type == ColoringType::Change)
            .map(|c| {
                txos.iter()
                    .find(|t| t.idx == c.txo_idx)
                    .unwrap_or_else(|| panic!("coloring txo_idx {} not found in TXOs", c.txo_idx))
                    .outpoint()
                    .to_string()
            })
            .collect()
    };
    let mut wlt_a_outpoints = resolve_change_outpoints(&wlt_a_txos, &wlt_a_colorings);
    let mut wlt_b_outpoints = resolve_change_outpoints(&wlt_b_txos, &wlt_b_colorings);
    assert_eq!(wlt_a_outpoints.len(), wlt_b_outpoints.len());
    wlt_a_outpoints.sort();
    wlt_b_outpoints.sort();
    assert_eq!(wlt_a_outpoints, wlt_b_outpoints);
}

pub(super) fn check_hub_info<'a>(parties: &mut [&mut MultisigParty<'a>]) {
    println!("\n=== Hub info ===");
    for party in parties {
        let info = party.hub_info();
        assert_eq!(info.user_role, UserRole::Cosigner);
        assert_eq!(info.last_operation_idx, None);
        assert_eq!(info.rgb_lib_version, local_rgb_lib_version());
    }
}

fn check_issuance(
    initiator: &mut MultisigParty,
    others: &mut [&mut MultisigParty],
    asset_id: &str,
    schema: AssetSchema,
) {
    fn assert_wallet_has_asset(wallet: &MultisigWallet, schema: AssetSchema, asset_id: &str) {
        let assets = wallet.list_assets(vec![schema]).unwrap();
        let found_ids: Vec<&str> = match schema {
            AssetSchema::Cfa => assets
                .cfa
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|a| a.asset_id.as_str())
                .collect(),
            AssetSchema::Nia => assets
                .nia
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|a| a.asset_id.as_str())
                .collect(),
            AssetSchema::Ifa => assets
                .ifa
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|a| a.asset_id.as_str())
                .collect(),
            AssetSchema::Uda => assets
                .uda
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|a| a.asset_id.as_str())
                .collect(),
        };
        assert_eq!(found_ids, vec![asset_id]);
    }

    let meta_ref = initiator
        .multisig
        .get_asset_metadata(asset_id.to_string())
        .unwrap();
    for wallet in others {
        let bt_before = wallet.bak_ts();
        let op_info = wallet.sync();
        assert!(wallet.bak_ts() > bt_before);
        assert_eq!(
            op_info.operation_idx,
            OP_COUNTER.load(Ordering::SeqCst) as i32
        );
        assert_eq!(op_info.initiator_xpub, initiator.xpub);
        assert_matches!(op_info.operation, Operation::IssuanceCompleted { .. });
        assert_wallet_has_asset(wallet.multisig, schema, asset_id);
        let meta = wallet
            .multisig
            .get_asset_metadata(asset_id.to_string())
            .unwrap();
        let mut san_meta = meta.clone();
        let mut san_meta_ref = meta_ref.clone();
        sanitize_meta(&mut san_meta);
        sanitize_meta(&mut san_meta_ref);
        assert_eq!(san_meta, san_meta_ref);
    }
}

pub(super) fn check_last_transaction(
    wallets: &mut [&mut MultisigWallet],
    psbt: &str,
    last_tx_type: &TransactionType,
) {
    let txid = Psbt::from_str(psbt).unwrap().get_txid().to_string();
    for wallet in wallets {
        let transactions = test_list_transactions(*wallet, None);
        let transaction = transactions.first().unwrap();
        assert_eq!(transaction.txid, txid);
        assert_eq!(
            transaction.transaction_type,
            *last_tx_type,
            "wallet {} last transaction type {:?} != expected {last_tx_type:?}",
            wallet.get_wallet_data().data_dir,
            transaction.transaction_type
        );
    }
}

pub(super) fn check_transfer_status(
    parties: &[&dyn SigParty],
    asset_ids: &[Option<&str>],
    batch_transfer_idx: Option<i32>,
    status: TransferStatus,
) {
    assert!(!asset_ids.is_empty(), "not checking anything");
    for asset_id in asset_ids {
        for party in parties {
            let transfers = party.list_transfers(*asset_id);
            let transfer = if let Some(idx) = batch_transfer_idx {
                transfers
                    .iter()
                    .find(|t| t.batch_transfer_idx == idx)
                    .unwrap()
            } else {
                transfers.last().unwrap()
            };
            eprintln!(
                "checking xfer {} for asset {asset_id:?} for {}",
                transfer.batch_transfer_idx,
                party.get_data_dir()
            );
            assert_eq!(transfer.status, status);
        }
    }
}

pub(super) fn check_wallet_state(
    wallet: &mut MultisigWallet,
    op_last_successful: &InitOperationResult,
    op_last: &InitOperationResult,
    btc_vanilla: (u64, u64, u64),
    btc_colored: (u64, u64, u64),
    last_tx_type: &TransactionType,
    asset_expectations: &HashMap<&str, (u64, u64, u64, usize, TransferStatus)>,
) {
    // check BTC balance
    check_btc_balance(&mut [wallet], btc_vanilla, btc_colored);
    // get all assets
    let mut all_assets: Vec<String> = vec![];
    let assets = wallet.list_assets(vec![]).unwrap();
    #[rustfmt::skip]
    all_assets.extend(assets.cfa.unwrap_or_default().into_iter().map(|a| a.asset_id));
    #[rustfmt::skip]
    all_assets.extend(assets.nia.unwrap_or_default().into_iter().map(|a| a.asset_id));
    #[rustfmt::skip]
    all_assets.extend(assets.uda.unwrap_or_default().into_iter().map(|a| a.asset_id));
    #[rustfmt::skip]
    all_assets.extend(assets.ifa.unwrap_or_default().into_iter().map(|a| a.asset_id));
    // check asset state
    let data_dir = wallet.get_wallet_data().data_dir;
    for (asset_id, (settled, future, spendable, transfer_count, status)) in asset_expectations {
        assert!(
            all_assets.contains(&asset_id.to_string()),
            "asset {asset_id} not found in wallet {data_dir}"
        );
        // asset balance
        let expected = Balance {
            settled: *settled,
            future: *future,
            spendable: *spendable,
        };
        let balance = wallet.get_asset_balance(asset_id.to_string()).unwrap();
        assert_eq!(
            balance, expected,
            "wallet {data_dir} asset {asset_id} balance {balance:?} != expected {expected:?}",
        );
        // asset transfer number and last transfer status
        let transfers = test_list_transfers(wallet, Some(asset_id));
        assert_eq!(
            transfers.len(),
            *transfer_count,
            "wallet {data_dir} asset {asset_id} transfer count {} != expected {transfer_count}",
            transfers.len()
        );
        assert_eq!(
            transfers.last().unwrap().status,
            *status,
            "wallet {data_dir} asset {asset_id} last transfer status {:?} != expected {status:?}",
            transfers.last().unwrap().status
        );
    }
    // check last TX ID and type
    check_last_transaction(&mut [wallet], &op_last_successful.psbt, last_tx_type);
    // check last processed op ID
    let last_processed_op = wallet.get_local_last_processed_operation_idx().unwrap();
    assert_eq!(
        last_processed_op, op_last.operation_idx,
        "wallet {data_dir} op idx {last_processed_op} != expected {}",
        op_last.operation_idx
    );
}

pub(super) fn check_wallets_up_to_date(wallets: &mut [&mut MultisigParty]) {
    for wallet in wallets {
        wallet.assert_up_to_date();
    }
}

// ----------------------------------------
// test
// ----------------------------------------

pub(super) fn backup(multisig: &MultisigParty, label: &str) -> String {
    println!("backup wallet {}", multisig.get_data_dir());
    let bak_fpath = get_test_data_dir_path().join(format!("{label}_backup.rgb-lib_backup"));
    let backup_file = bak_fpath.to_str().unwrap();
    let _ = std::fs::remove_file(backup_file);
    multisig.multisig.backup(backup_file, PASSWORD).unwrap();
    backup_file.to_string()
}

pub(super) fn backup_restore(backup_file: &str, path: &str, keys: MultisigKeys) -> MultisigWallet {
    println!("restore wallet from backup {backup_file}");
    let target_dir_path = get_restore_dir_path(Some(format!("{path}_1")));
    let target_dir = target_dir_path.to_str().unwrap();
    restore_backup(backup_file, PASSWORD, target_dir).unwrap();
    MultisigWallet::new(get_test_wallet_data(target_dir), keys).unwrap()
}

pub(super) fn inspect_create_utxos(
    cosigner: &mut MultisigParty,
    psbt: &str,
    txid: Option<bool>,
    input_map: &HashMap<u32, u64>,
    utxo_num: Option<u8>,
    utxo_size: Option<u32>,
    sig_num: u16,
) {
    let psbt_info = cosigner.multisig.inspect_psbt(psbt.to_string()).unwrap();
    if txid.is_none() {
        assert!(!psbt_info.txid.is_empty());
    }
    let total_input_sat = input_map.values().sum::<u64>();
    assert_eq!(psbt_info.total_input_sat, total_input_sat);
    assert!(psbt_info.size_vbytes > 0);
    assert!(psbt_info.fee_sat > 0);
    assert_eq!(psbt_info.inputs.len(), input_map.len());
    let outpoints: Vec<_> = cosigner
        .list_unspents(false)
        .into_iter()
        .map(|u| u.utxo.outpoint)
        .collect();
    for (i, inp) in psbt_info.inputs.iter().enumerate() {
        assert_eq!(inp.amount_sat, *input_map.get(&(i as u32)).unwrap());
        assert!(outpoints.contains(&inp.outpoint));
    }
    let utxo_num = utxo_num.unwrap_or(UTXO_NUM);
    let utxo_size = utxo_size.unwrap_or(UTXO_SIZE);
    assert_eq!(psbt_info.outputs.len() as u8, utxo_num + 1);
    let mut change_out = false;
    for out in &psbt_info.outputs {
        assert!(out.address.is_some());
        assert!(out.is_mine);
        assert!(!out.is_op_return);
        if !change_out && out.amount_sat != utxo_size as u64 {
            change_out = true
        } else {
            assert_eq!(out.amount_sat, utxo_size as u64);
        }
    }
    assert_eq!(
        psbt_info.total_input_sat - psbt_info.total_output_sat,
        psbt_info.fee_sat
    );
    assert_eq!(psbt_info.signature_count, sig_num);
}

pub(super) fn inspect_inflate(
    wallet: &MultisigParty,
    op_init: &InitOperationResult,
    ifa_asset: &AssetIFA,
    inflation_amounts: &[u64],
) {
    let psbt = &op_init.psbt;
    let (_, files) = wallet.get_op_and_files(op_init.operation_idx);
    let details = InflateHandler::extract_details(&files).unwrap();
    let rgb_inspection = wallet
        .multisig
        .inspect_rgb_transfer(psbt.clone(), details.fascia_path, details.entropy)
        .unwrap();
    assert_eq!(rgb_inspection.close_method, CloseMethod::OpretFirst);
    assert_eq!(rgb_inspection.operations.len(), 1);
    let ifa_op = &rgb_inspection.operations[0];
    assert_eq!(ifa_op.asset_id, ifa_asset.asset_id);
    let inflate_transitions: Vec<_> = ifa_op
        .transitions
        .iter()
        .filter(|t| t.r#type == TypeOfTransition::Inflate)
        .collect();
    assert_eq!(inflate_transitions.len(), 1);
    let inflate_outputs: Vec<_> = inflate_transitions[0]
        .outputs
        .iter()
        .filter(|o| matches!(o.assignment, Assignment::Fungible(_)))
        .map(|o| o.assignment.main_amount())
        .collect();
    let mut sorted_inflate_outputs = inflate_outputs.clone();
    sorted_inflate_outputs.sort();
    let mut sorted_expected = inflation_amounts.to_vec();
    sorted_expected.sort();
    assert_eq!(sorted_inflate_outputs, sorted_expected);
}

pub(super) fn inspect_send(
    wallet: &MultisigParty,
    op_init: &InitOperationResult,
    cfa: &AssetCFA,
    nia_1: &AssetNIA,
    nia_2: &AssetNIA,
    cfa_amount_blind: u64,
    cfa_amount_witness: u64,
    nia_2_amount: u64,
) {
    // PSBT inspection
    let psbt = &op_init.psbt;
    let psbt_info = wallet.multisig.inspect_psbt(psbt.to_string()).unwrap();
    assert!(psbt_info.inputs.len() >= 2);
    assert!(psbt_info.outputs.len() >= 2);
    let op_return_out = &psbt_info.outputs[0];
    assert!(op_return_out.is_op_return);
    for out in &psbt_info.outputs[1..] {
        assert!(out.address.is_some());
    }

    // RGB inspection
    let (_, files) = wallet.get_op_and_files(op_init.operation_idx);
    let details = SendRgbHandler::extract_details(&files).unwrap();
    let rgb_inspection = wallet
        .multisig
        .inspect_rgb_transfer(psbt.to_string(), details.fascia_path, details.entropy)
        .unwrap();
    assert_eq!(rgb_inspection.close_method, CloseMethod::OpretFirst);
    assert_eq!(rgb_inspection.commitment_hex.len(), 64);
    assert_eq!(rgb_inspection.operations.len(), 3);
    let cfa_transfer = rgb_inspection
        .operations
        .iter()
        .find(|op| op.asset_id == cfa.asset_id)
        .unwrap();
    let nia_transfer = rgb_inspection
        .operations
        .iter()
        .find(|op| op.asset_id == nia_1.asset_id)
        .unwrap();
    let cfa_inputs: Vec<_> = cfa_transfer
        .transitions
        .iter()
        .flat_map(|t| &t.inputs)
        .collect();
    let cfa_outputs: Vec<_> = cfa_transfer
        .transitions
        .iter()
        .flat_map(|t| &t.outputs)
        .collect();
    assert_eq!(cfa_outputs.len(), 3);
    let cfa_sent_outputs: Vec<_> = cfa_outputs.iter().filter(|o| !o.is_ours).collect();
    let cfa_change_outputs: Vec<_> = cfa_outputs.iter().filter(|o| o.is_ours).collect();
    assert_eq!(cfa_sent_outputs.len(), 2);
    assert_eq!(cfa_change_outputs.len(), 1);
    let cfa_witness_output = cfa_sent_outputs.iter().find(|o| !o.is_concealed).unwrap();
    assert_eq!(
        cfa_witness_output.assignment.main_amount(),
        cfa_amount_witness
    );
    assert!(!cfa_witness_output.is_concealed);
    assert!(!cfa_witness_output.is_ours);
    let cfa_blind_output = cfa_sent_outputs.iter().find(|o| o.is_concealed).unwrap();
    assert_eq!(cfa_blind_output.assignment.main_amount(), cfa_amount_blind);
    assert!(cfa_blind_output.is_concealed);
    assert!(!cfa_blind_output.is_ours);
    let cfa_change_amount: u64 = cfa_change_outputs
        .iter()
        .map(|o| o.assignment.main_amount())
        .sum();
    for o in &cfa_change_outputs {
        assert!(!o.is_concealed);
        assert!(o.is_ours);
    }
    let cfa_sent_amount = cfa_amount_witness + cfa_amount_blind;
    let total_cfa_output: u64 = cfa_outputs.iter().map(|o| o.assignment.main_amount()).sum();
    assert_eq!(total_cfa_output, cfa_sent_amount + cfa_change_amount);
    assert!(!cfa_inputs.is_empty());
    let total_cfa_input: u64 = cfa_inputs.iter().map(|i| i.assignment.main_amount()).sum();
    assert_eq!(total_cfa_input, total_cfa_output);
    for input in &cfa_inputs {
        assert!((input.vin as usize) < psbt_info.inputs.len());
        assert_matches!(input.assignment, Assignment::Fungible(_));
    }
    let nia_inputs: Vec<_> = nia_transfer
        .transitions
        .iter()
        .flat_map(|t| &t.inputs)
        .collect();
    let nia_outputs: Vec<_> = nia_transfer
        .transitions
        .iter()
        .flat_map(|t| &t.outputs)
        .collect();
    let nia_sent_outputs: Vec<_> = nia_outputs.iter().filter(|o| !o.is_ours).collect();
    let nia_change_outputs: Vec<_> = nia_outputs.iter().filter(|o| o.is_ours).collect();
    assert_eq!(nia_sent_outputs.len(), 1);
    assert!(!nia_change_outputs.is_empty());
    assert_eq!(nia_sent_outputs[0].assignment.main_amount(), AMOUNT_SMALL);
    assert!(nia_sent_outputs[0].is_concealed);
    assert!(!nia_sent_outputs[0].is_ours);
    let total_nia_output: u64 = nia_outputs.iter().map(|o| o.assignment.main_amount()).sum();
    let total_nia_input: u64 = nia_inputs.iter().map(|i| i.assignment.main_amount()).sum();
    assert_eq!(total_nia_input, total_nia_output);
    assert!(total_nia_input >= AMOUNT_SMALL);
    for input in &nia_inputs {
        assert!((input.vin as usize) < psbt_info.inputs.len());
        assert_matches!(input.assignment, Assignment::Fungible(_));
    }
    let nia_2_transfer = rgb_inspection
        .operations
        .iter()
        .find(|op| op.asset_id == nia_2.asset_id)
        .unwrap();
    let nia_2_inputs: Vec<_> = nia_2_transfer
        .transitions
        .iter()
        .flat_map(|t| &t.inputs)
        .collect();
    let nia_2_outputs: Vec<_> = nia_2_transfer
        .transitions
        .iter()
        .flat_map(|t| &t.outputs)
        .collect();
    assert_eq!(nia_2_outputs.len(), 2);
    let nia_2_sent: Vec<_> = nia_2_outputs.iter().filter(|o| !o.is_ours).collect();
    let nia_2_change: Vec<_> = nia_2_outputs.iter().filter(|o| o.is_ours).collect();
    assert_eq!(nia_2_sent.len(), 1);
    assert_eq!(nia_2_change.len(), 1);
    assert_eq!(nia_2_sent[0].assignment.main_amount(), nia_2_amount);
    assert!(nia_2_sent[0].is_concealed);
    assert!(!nia_2_sent[0].is_ours);
    let expected_nia_2_change = AMOUNT_SMALL - nia_2_amount;
    assert_eq!(
        nia_2_change[0].assignment.main_amount(),
        expected_nia_2_change
    );
    assert!(!nia_2_change[0].is_concealed);
    assert!(nia_2_change[0].is_ours);
    let nia_2_total_output: u64 = nia_2_outputs
        .iter()
        .map(|o| o.assignment.main_amount())
        .sum();
    assert_eq!(nia_2_total_output, AMOUNT_SMALL);
    assert!(!nia_2_inputs.is_empty());
    let nia_2_total_input: u64 = nia_2_inputs
        .iter()
        .map(|i| i.assignment.main_amount())
        .sum();
    assert_eq!(nia_2_total_input, nia_2_total_output);
    assert_eq!(nia_2_total_input, AMOUNT_SMALL);
    for input in &nia_2_inputs {
        assert!((input.vin as usize) < psbt_info.inputs.len());
        assert_matches!(input.assignment, Assignment::Fungible(_));
    }
}

fn op_counter_bump() -> i32 {
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    OP_COUNTER.load(Ordering::SeqCst) as i32
}

pub(super) fn op_counter_reset() {
    OP_COUNTER.store(0, Ordering::SeqCst);
}

pub(super) fn operation_complete<H>(
    op_idx: i32,
    ackers: &mut [&mut MultisigParty],
    nackers: &mut [&mut MultisigParty],
    others: &mut [&mut MultisigParty],
    approve: bool,
) where
    H: OperationHandler,
    H::Details: Sanitizable,
{
    let (op, files) = if !ackers.is_empty() {
        let party = ackers.first().unwrap();
        party.get_op_and_files(op_idx)
    } else {
        let party = nackers.first().unwrap();
        party.get_op_and_files(op_idx)
    };
    println!(
        "{} {:?} operation with id {op_idx}",
        if approve { "approve" } else { "disacrd" },
        op.operation_type
    );

    let op_psbt = get_op_psbt(&files);
    let op_txid = op_psbt.get_txid();
    let mut details = H::extract_details(&files).unwrap();
    details.sanitize();
    let threshold = op.threshold.unwrap();

    let last_acker = if !ackers.is_empty() {
        ackers.len() - 1
    } else {
        0
    };
    let last_nacker = if !nackers.is_empty() {
        nackers.len() - 1
    } else {
        0
    };
    let mut acked_by = set![];
    let mut nacked_by = set![];

    let get_op_review = |status: &MultisigVotingStatus| {
        H::to_review(op_psbt.to_string(), details.clone(), status.clone())
    };
    let get_op_pending =
        |status: &MultisigVotingStatus| H::pending(details.clone(), status.clone());
    let get_op_final = |status: &MultisigVotingStatus| {
        if approve {
            H::completed(op_txid.to_string(), details.clone(), status.clone())
        } else {
            H::discarded(details.clone(), status.clone())
        }
    };

    // approve or discard
    if approve {
        // nack with nacker cosigners
        for nacker in nackers.iter_mut() {
            let bt_before = nacker.bak_info_opt();
            let mut op_info = nacker.sync();
            check_bak_ts_opt(nacker, bt_before, false);
            op_info.operation.sanitize();
            let status = MultisigVotingStatus {
                acked_by: acked_by.clone(),
                nacked_by: nacked_by.clone(),
                threshold,
                my_response: None,
            };
            assert_eq!(op_info.operation, get_op_review(&status));
            let bt_before = nacker.bak_info_opt();
            let mut response = nacker.nack(op_info.operation_idx);
            check_bak_ts_opt(nacker, bt_before, false);
            nacked_by.insert(nacker.xpub.to_string());
            let status = MultisigVotingStatus {
                acked_by: acked_by.clone(),
                nacked_by: nacked_by.clone(),
                threshold,
                my_response: Some(false),
            };
            response.operation.sanitize();
            assert_eq!(response.operation, get_op_pending(&status));
        }
        // ack with acker cosigners
        for (i, acker) in ackers.iter_mut().enumerate() {
            let bt_before = acker.bak_info_opt();
            let mut op_info = acker.sync();
            check_bak_ts_opt(acker, bt_before, false);
            op_info.operation.sanitize();
            let status = MultisigVotingStatus {
                acked_by: acked_by.clone(),
                nacked_by: nacked_by.clone(),
                threshold,
                my_response: None,
            };
            assert_eq!(op_info.operation, get_op_review(&status));
            let bt_before = acker.bak_info_opt();
            let mut response = acker.sign_and_ack(&op_psbt.to_string(), op_info.operation_idx);
            check_bak_ts_opt(acker, bt_before, false);
            acked_by.insert(acker.xpub.to_string());
            let status = MultisigVotingStatus {
                acked_by: acked_by.clone(),
                nacked_by: nacked_by.clone(),
                threshold,
                my_response: Some(true),
            };
            response.operation.sanitize();
            if i < last_acker {
                assert_eq!(response.operation, get_op_pending(&status));
            } else {
                assert_eq!(response.operation, get_op_final(&status));
            }
        }
    } else {
        // ack with acker cosigners
        for acker in ackers.iter_mut() {
            let bt_before = acker.bak_info_opt();
            let mut op_info = acker.sync();
            check_bak_ts_opt(acker, bt_before, false);
            let status = MultisigVotingStatus {
                acked_by: acked_by.clone(),
                nacked_by: nacked_by.clone(),
                threshold,
                my_response: None,
            };
            op_info.operation.sanitize();
            assert_eq!(op_info.operation, get_op_review(&status));
            let bt_before = acker.bak_info_opt();
            let mut response = acker.sign_and_ack(&op_psbt.to_string(), op_info.operation_idx);
            check_bak_ts_opt(acker, bt_before, false);
            acked_by.insert(acker.xpub.to_string());
            let status = MultisigVotingStatus {
                acked_by: acked_by.clone(),
                nacked_by: nacked_by.clone(),
                threshold,
                my_response: Some(true),
            };
            response.operation.sanitize();
            assert_eq!(response.operation, get_op_pending(&status));
        }
        // nack with nacker cosigners
        for (i, nacker) in nackers.iter_mut().enumerate() {
            let bt_before = nacker.bak_info_opt();
            let mut op_info = nacker.sync();
            check_bak_ts_opt(nacker, bt_before, false);
            op_info.operation.sanitize();
            let status = MultisigVotingStatus {
                acked_by: acked_by.clone(),
                nacked_by: nacked_by.clone(),
                threshold,
                my_response: None,
            };
            assert_eq!(op_info.operation, get_op_review(&status));
            let bt_before = nacker.bak_info_opt();
            let mut response = nacker.nack(op_info.operation_idx);
            check_bak_ts_opt(nacker, bt_before, false);
            nacked_by.insert(nacker.xpub.to_string());
            let status = MultisigVotingStatus {
                acked_by: acked_by.clone(),
                nacked_by: nacked_by.clone(),
                threshold,
                my_response: Some(false),
            };
            response.operation.sanitize();
            if i < last_nacker {
                assert_eq!(response.operation, get_op_pending(&status));
            } else {
                assert_eq!(response.operation, get_op_final(&status));
            }
        }
    }

    // final situation
    // - ackers
    let status = MultisigVotingStatus {
        acked_by: acked_by.clone(),
        nacked_by: nacked_by.clone(),
        threshold,
        my_response: Some(true),
    };
    for (i, acker) in ackers.iter_mut().enumerate() {
        let bt_before = acker.bak_info_opt();
        let op_info_opt = acker.sync_opt();
        if approve && i == last_acker {
            assert!(op_info_opt.is_none());
            check_bak_ts_opt(acker, bt_before, true);
            continue;
        }
        check_bak_ts_opt(acker, bt_before, false);
        let mut op_info = op_info_opt.unwrap();
        op_info.operation.sanitize();
        assert_eq!(op_info.operation, get_op_final(&status));
    }
    // - nackers
    let status = MultisigVotingStatus {
        acked_by: acked_by.clone(),
        nacked_by: nacked_by.clone(),
        threshold,
        my_response: Some(false),
    };
    for (i, nacker) in nackers.iter_mut().enumerate() {
        let bt_before = nacker.bak_info_opt();
        let op_info_opt = nacker.sync_opt();
        if !approve && i == last_nacker {
            assert!(op_info_opt.is_none());
            check_bak_ts_opt(nacker, bt_before, true);
            continue;
        }
        check_bak_ts_opt(nacker, bt_before, false);
        let mut op_info = op_info_opt.unwrap();
        op_info.operation.sanitize();
        assert_eq!(op_info.operation, get_op_final(&status));
    }
    // - others
    let status = MultisigVotingStatus {
        acked_by: acked_by.clone(),
        nacked_by: nacked_by.clone(),
        threshold,
        my_response: None,
    };
    for other in others.iter_mut() {
        let bt_before = other.bak_info_opt();
        let mut op_info = other.sync();
        check_bak_ts_opt(other, bt_before, false);
        op_info.operation.sanitize();
        assert_eq!(op_info.operation, get_op_final(&status));
    }
}

pub(super) fn settle_transfer(
    senders: &mut [&mut impl SigParty],
    receivers: &mut [&mut impl SigParty],
    asset_id: Option<&str>,
    txid: Option<&str>,
    psbt: Option<&str>,
    stage_1: bool,
) {
    if stage_1 {
        for wallet in &mut *receivers {
            wallet.refresh(None); // always None as the recipient might not know the asset yet
        }
        for wallet in &mut *senders {
            wallet.refresh(asset_id);
        }
    }
    if let Some(psbt) = psbt {
        let txid = Psbt::from_str(psbt).unwrap().get_txid().to_string();
        mine_tx(false, false, &txid);
    } else if let Some(txid) = txid {
        mine_tx(false, false, txid);
    } else {
        mine(false, false);
    }
    for wallet in &mut *receivers {
        wallet.refresh(asset_id);
    }
    for wallet in &mut *senders {
        wallet.refresh(asset_id);
    }
}

pub(super) fn sync_wallets_full(wallets: &mut [&mut MultisigParty]) {
    for wallet in wallets {
        eprintln!("syncing wallet {}", wallet.get_data_dir());
        let online = wallet.online();
        let last_processed = wallet
            .multisig_mut()
            .get_local_last_processed_operation_idx()
            .unwrap();
        let last_hub_operation = wallet
            .multisig_mut()
            .hub_info(online)
            .unwrap()
            .last_operation_idx
            .unwrap();
        assert!(
            last_hub_operation > last_processed,
            "wallet already in sync"
        );
        for i in (last_processed + 1)..=last_hub_operation {
            println!("syncing operation {i}");
            let op_info = wallet.sync();
            assert_eq!(op_info.operation_idx, i);
        }
        let final_processed = wallet
            .multisig_mut()
            .get_local_last_processed_operation_idx()
            .unwrap();
        assert_eq!(final_processed, last_hub_operation);
        wallet.assert_up_to_date();
    }
}
