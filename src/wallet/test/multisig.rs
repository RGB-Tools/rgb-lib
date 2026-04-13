use super::*;

static OP_COUNTER: AtomicU64 = AtomicU64::new(0);

// utilities

struct MultisigParty<'a> {
    signer: &'a Wallet,
    multisig: &'a mut MultisigWallet,
    online: Online,
    xpub: &'a str,
}

struct WatchOnlyParty<'a> {
    multisig: &'a mut MultisigWallet,
    online: Online,
}
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

struct SinglesigParty<'a> {
    wallet: &'a mut Wallet,
    online: Online,
}

macro_rules! party {
    ($wallet:expr, $online:expr) => {
        SinglesigParty {
            wallet: $wallet,
            online: $online,
        }
    };
}

trait MultisigOps {
    fn multisig_mut(&mut self) -> &mut MultisigWallet;
    fn multisig_ref(&self) -> &MultisigWallet;
    fn online(&self) -> Online;

    fn assert_up_to_date(&mut self) {
        assert!(self.sync_opt().is_none());
    }

    fn asset_balance(&self, asset_id: &str) -> Balance {
        test_get_asset_balance(self.multisig_ref(), asset_id)
    }

    fn bak_ts(&mut self) -> String {
        self.multisig_ref()
            .database()
            .get_backup_info()
            .unwrap()
            .unwrap()
            .last_operation_timestamp
    }

    fn hub_info(&mut self) -> HubInfo {
        let online = self.online();
        self.multisig_mut().hub_info(online).unwrap()
    }

    fn blind_receive(&mut self) -> ReceiveData {
        self.blind_receive_res().unwrap()
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
        self.create_utxos_init_res(up_to, num, size, fee_rate)
            .unwrap()
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

    fn inflate_init(
        &mut self,
        asset_id: String,
        inflation_amounts: Vec<u64>,
    ) -> InitOperationResult {
        self.inflate_init_res(asset_id, inflation_amounts).unwrap()
    }

    fn inflate_init_res(
        &mut self,
        asset_id: String,
        inflation_amounts: Vec<u64>,
    ) -> Result<InitOperationResult, Error> {
        let online = self.online();
        self.multisig_mut()
            .inflate_init(online, asset_id, inflation_amounts, FEE_RATE, 1)
    }

    fn issue_asset_cfa(&mut self, amounts: Option<&[u64]>, file_path: Option<String>) -> AssetCFA {
        self.issue_asset_cfa_res(amounts, file_path).unwrap()
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
        self.issue_asset_ifa_res(amounts, inflation_amounts, reject_list_url)
            .unwrap()
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
        self.issue_asset_nia_res(amounts).unwrap()
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
        self.issue_asset_uda_res(details, media_file_path, attachments_file_paths)
            .unwrap()
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

    fn list_transfers(&self, asset_id: &str) -> Vec<Transfer> {
        test_list_transfers(self.multisig_ref(), Some(asset_id))
    }

    fn list_unspents(&mut self, settled_only: bool) -> Vec<Unspent> {
        let online = self.online();
        test_list_unspents(self.multisig_mut(), Some(online), settled_only)
    }

    fn nack(&mut self, op_idx: i32) -> OperationInfo {
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
        self.send_btc_init_res(address, amount).unwrap()
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
        self.send_init_res(recipient_map).unwrap()
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

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn wait_refresh(&mut self, asset_id: Option<&str>, transfer_ids: Option<&[i32]>) {
        let online = self.online();
        wait_for_refresh(self.multisig_mut(), online, asset_id, transfer_ids)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn witness_receive(&mut self) -> ReceiveData {
        self.witness_receive_res().unwrap()
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
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

impl<'a> MultisigParty<'a> {
    fn sign_and_ack(&mut self, psbt: String, op_idx: i32) -> OperationInfo {
        let signed = self.signer.sign_psbt(psbt, None).unwrap();
        self.respond_to_operation(op_idx, RespondToOperation::Ack(signed))
    }
}

#[derive(Deserialize, Serialize)]
pub(crate) struct AppConfig {
    pub(crate) cosigner_xpubs: Vec<String>,
    pub(crate) threshold_colored: u8,
    pub(crate) threshold_vanilla: u8,
    pub(crate) root_public_key: String,
    pub(crate) rgb_lib_version: String,
}

enum Role {
    Cosigner(String),
    WatchOnly,
}

fn assert_last_transfer_settled(transfers: &[Transfer]) {
    assert_eq!(transfers.last().unwrap().status, TransferStatus::Settled);
}

fn assert_synced_wallet_state(wallet: &MultisigWallet) {
    let assets = wallet.list_assets(vec![]).unwrap();
    let nia_assets = assets.nia.unwrap();
    assert_eq!(nia_assets.len(), 2);
    let ifa_assets = assets.ifa.unwrap();
    assert_eq!(ifa_assets.len(), 1);
    let cfa_assets = assets.cfa.unwrap();
    assert_eq!(cfa_assets.len(), 1);
    let uda_assets = assets.uda.unwrap();
    assert_eq!(uda_assets.len(), 1);
    let mut nia_counts: Vec<usize> = [&nia_assets[0].asset_id, &nia_assets[1].asset_id]
        .iter()
        .map(|id| test_list_transfers(wallet, Some(id)).len())
        .collect();
    nia_counts.sort_unstable();
    assert_eq!(nia_counts, [3, 4]);
    for asset_id in [
        &ifa_assets[0].asset_id,
        &cfa_assets[0].asset_id,
        &uda_assets[0].asset_id,
    ] {
        assert_eq!(test_list_transfers(wallet, Some(asset_id)).len(), 3);
    }
}

fn create_token(root: &KeyPair, role: Role, expiration_date: Option<DateTime<Utc>>) -> String {
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

fn get_test_ms_wallet(keys: &MultisigKeys, dir: String) -> MultisigWallet {
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

fn issuance_assertions(
    issuer: &mut MultisigParty,
    observer_1: &mut MultisigParty,
    observer_2: &mut MultisigParty,
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
    let bi = observer_1.bak_ts();
    let op_info = observer_1.sync();
    assert!(observer_1.bak_ts() > bi);
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    assert_eq!(op_info.initiator_xpub, issuer.xpub);
    assert_matches!(op_info.operation, Operation::IssuanceCompleted { .. });
    let op_info = observer_2.sync();
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    assert_eq!(op_info.initiator_xpub, issuer.xpub);
    assert_matches!(op_info.operation, Operation::IssuanceCompleted { .. });
    assert_wallet_has_asset(issuer.multisig, schema, asset_id);
    assert_wallet_has_asset(observer_1.multisig, schema, asset_id);
    assert_wallet_has_asset(observer_2.multisig, schema, asset_id);
    let meta_ref = observer_1
        .multisig
        .get_asset_metadata(asset_id.to_string())
        .unwrap();
    for meta in [
        issuer
            .multisig
            .get_asset_metadata(asset_id.to_string())
            .unwrap(),
        observer_2
            .multisig
            .get_asset_metadata(asset_id.to_string())
            .unwrap(),
    ] {
        assert_eq!(meta.ticker, meta_ref.ticker);
        assert_eq!(meta.name, meta_ref.name);
        assert_eq!(meta.precision, meta_ref.precision);
        assert_eq!(meta.initial_supply, meta_ref.initial_supply);
        assert_eq!(meta.asset_schema, meta_ref.asset_schema);
    }
}

fn local_rgb_lib_version() -> String {
    env!("CARGO_PKG_VERSION")
        .split('.')
        .take(2)
        .collect::<Vec<_>>()
        .join(".")
}

fn ms_go_online_res(wallet: &mut MultisigWallet, token: &str) -> Result<Online, Error> {
    wallet.go_online(
        false,
        ELECTRUM_URL.to_string(),
        MULTISIG_HUB_URL.to_string(),
        token.to_string(),
    )
}

fn ms_go_online(wallet: &mut MultisigWallet, token: &str) -> Online {
    ms_go_online_res(wallet, token).unwrap()
}

fn write_hub_config(
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

// test helpers

fn backup(multisig: &MultisigWallet, label: &str) -> (i32, String) {
    println!("\n=== Backup ===");
    let last_processed = multisig.get_local_last_processed_operation_idx().unwrap();
    let bak_fpath = get_test_data_dir_path().join(format!("{label}_backup.rgb-lib_backup"));
    let backup_file = bak_fpath.to_str().unwrap();
    let _ = std::fs::remove_file(backup_file);
    multisig.backup(backup_file, PASSWORD).unwrap();
    (last_processed, backup_file.to_string())
}

fn backup_restore(
    backup_file: &str,
    random_str: &str,
    multisig_wlt_keys: MultisigKeys,
    wlt_last_processed_before_backup: i32,
    token: &str,
) {
    println!("\n=== Restore backup ===");
    let target_dir_path = get_restore_dir_path(Some(format!("{random_str}_1")));
    let target_dir = target_dir_path.to_str().unwrap();
    restore_backup(backup_file, PASSWORD, target_dir).unwrap();
    let mut wlt_restored =
        MultisigWallet::new(get_test_wallet_data(target_dir), multisig_wlt_keys).unwrap();
    let wlt_restored_last_processed = wlt_restored
        .get_local_last_processed_operation_idx()
        .unwrap();
    assert_eq!(
        wlt_restored_last_processed,
        wlt_last_processed_before_backup
    );
    let wlt_restored_online = ms_go_online(&mut wlt_restored, token);
    ms_party!(&mut wlt_restored, wlt_restored_online).sync_to_head();
}

fn blind_receive_completed(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
    singlesig_wlt: &mut SinglesigParty,
    nia_asset_1: AssetNIA,
) {
    println!("\n=== Blind receive ===");
    let bi = wlt_2.bak_ts();
    let receive_data = wlt_2.blind_receive();
    assert!(wlt_2.bak_ts() > bi);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let recipient_map = HashMap::from([(
        nia_asset_1.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT_SMALL),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let _txid = test_send(singlesig_wlt.wallet, singlesig_wlt.online, &recipient_map);
    wlt_2.wait_refresh(None, None);
    wait_for_refresh(
        singlesig_wlt.wallet,
        singlesig_wlt.online,
        Some(&nia_asset_1.asset_id),
        None,
    );
    mine(false, false);
    wlt_2.wait_refresh(Some(&nia_asset_1.asset_id), None);
    wait_for_refresh(
        singlesig_wlt.wallet,
        singlesig_wlt.online,
        Some(&nia_asset_1.asset_id),
        None,
    );
    let transfers = wlt_2.list_transfers(&nia_asset_1.asset_id);
    assert_last_transfer_settled(&transfers);
    let transfers = test_list_transfers(singlesig_wlt.wallet, Some(&nia_asset_1.asset_id));
    assert_last_transfer_settled(&transfers);
    let bi = wlt_1.bak_ts();
    wlt_1.sync();
    assert!(wlt_1.bak_ts() > bi);
    let transfers = wlt_1.list_transfers(&nia_asset_1.asset_id);
    assert_last_transfer_settled(&transfers);
    let bi = wlt_3.bak_ts();
    wlt_3.sync();
    assert!(wlt_3.bak_ts() > bi);
    let transfers = wlt_3.list_transfers(&nia_asset_1.asset_id);
    assert_last_transfer_settled(&transfers);
    let expected_nia_asset_1_balance = nia_asset_1.balance.settled;
    let balance = wlt_2.asset_balance(&nia_asset_1.asset_id);
    assert_eq!(balance.settled, expected_nia_asset_1_balance);
    let balance = wlt_1.asset_balance(&nia_asset_1.asset_id);
    assert_eq!(balance.settled, expected_nia_asset_1_balance);
    let balance = wlt_3.asset_balance(&nia_asset_1.asset_id);
    assert_eq!(balance.settled, expected_nia_asset_1_balance);
}

fn blind_receive_unknown_asset(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
    singlesig_wlt: &mut SinglesigParty,
) -> AssetNIA {
    println!("\n=== Blind receive unknown asset ===");
    let nia_asset_2 = test_issue_asset_nia(singlesig_wlt.wallet, singlesig_wlt.online, None);
    let bi = wlt_1.bak_ts();
    let receive_data = wlt_1.blind_receive();
    assert!(wlt_1.bak_ts() > bi);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let recipient_map = HashMap::from([(
        nia_asset_2.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT_SMALL),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let _txid = test_send(singlesig_wlt.wallet, singlesig_wlt.online, &recipient_map);
    wlt_1.wait_refresh(None, None);
    wait_for_refresh(
        singlesig_wlt.wallet,
        singlesig_wlt.online,
        Some(&nia_asset_2.asset_id),
        None,
    );
    mine(false, false);
    wlt_1.wait_refresh(Some(&nia_asset_2.asset_id), None);
    wait_for_refresh(
        singlesig_wlt.wallet,
        singlesig_wlt.online,
        Some(&nia_asset_2.asset_id),
        None,
    );
    let transfers = wlt_1.list_transfers(&nia_asset_2.asset_id);
    assert_last_transfer_settled(&transfers);
    let transfers = test_list_transfers(singlesig_wlt.wallet, Some(&nia_asset_2.asset_id));
    assert_last_transfer_settled(&transfers);
    let bi = wlt_2.bak_ts();
    wlt_2.sync();
    assert!(wlt_2.bak_ts() > bi);
    let bi = wlt_3.bak_ts();
    wlt_3.sync();
    assert!(wlt_3.bak_ts() > bi);
    let balance = wlt_1.asset_balance(&nia_asset_2.asset_id);
    assert_eq!(balance.settled, AMOUNT_SMALL);
    let balance = wlt_2.asset_balance(&nia_asset_2.asset_id);
    assert_eq!(balance.settled, AMOUNT_SMALL);
    let balance = wlt_3.asset_balance(&nia_asset_2.asset_id);
    assert_eq!(balance.settled, AMOUNT_SMALL);
    nia_asset_2
}

fn check_cosigner_hub_info<'a>(
    wlt_1: &mut MultisigParty<'a>,
    wlt_2: &mut MultisigParty<'a>,
    wlt_3: &mut MultisigParty<'a>,
    wlt_4: &mut MultisigParty<'a>,
) {
    println!("\n=== Hub info ===");
    for wlt in [wlt_1, wlt_2, wlt_3, wlt_4] {
        let info = wlt.hub_info();
        assert_eq!(info.user_role, UserRole::Cosigner);
        assert_eq!(info.last_operation_idx, None);
        assert_eq!(info.rgb_lib_version, local_rgb_lib_version());
    }
}

fn check_change_consistency(wlt_1: &mut MultisigParty, wlt_4: &mut MultisigParty) {
    println!("\n=== Check change_utxo_idx consistency (wlt_1 vs wlt_4) ===");
    let wlt_1_txos = wlt_1.multisig.database().iter_txos().unwrap();
    let wlt_1_colorings = wlt_1.multisig.database().iter_colorings().unwrap();
    let wlt_4_txos = wlt_4.multisig.database().iter_txos().unwrap();
    let wlt_4_colorings = wlt_4.multisig.database().iter_colorings().unwrap();
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
    let mut wlt_1_outpoints = resolve_change_outpoints(&wlt_1_txos, &wlt_1_colorings);
    let mut wlt_4_outpoints = resolve_change_outpoints(&wlt_4_txos, &wlt_4_colorings);
    assert_eq!(wlt_1_outpoints.len(), wlt_4_outpoints.len());
    wlt_1_outpoints.sort();
    wlt_4_outpoints.sort();
    assert_eq!(wlt_1_outpoints, wlt_4_outpoints);
}

fn create_utxos_completed(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
    sats: u64,
) {
    println!("\n=== Create UTXOs ===");
    let unspents = wlt_2.list_unspents(false);
    let outpoints = unspents
        .into_iter()
        .map(|u| u.utxo.outpoint)
        .collect::<Vec<_>>();
    let num_utxos = 20;
    let utxo_size = 1000;
    let bi = wlt_1.bak_ts();
    let init_res = wlt_1.create_utxos_init(false, Some(num_utxos), Some(utxo_size), FEE_RATE);
    assert!(wlt_1.bak_ts() > bi);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    wlt_1.sign_and_ack(init_res.psbt.clone(), init_res.operation_idx);
    let bi = wlt_2.bak_ts();
    let op_info = wlt_2.sync();
    assert!(wlt_2.bak_ts() > bi);
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    assert_eq!(op_info.initiator_xpub, wlt_1.xpub);
    let Operation::CreateUtxosToReview { psbt, status } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(status.acked_by.len(), 1);
    assert_eq!(status.nacked_by.len(), 0);
    assert_eq!(status.threshold, 3);
    assert_eq!(status.my_response, None);
    let psbt_info = wlt_2.multisig.inspect_psbt(psbt.clone()).unwrap();
    assert!(!psbt_info.txid.is_empty());
    assert_eq!(psbt_info.total_input_sat, sats);
    assert!(psbt_info.size_vbytes > 0);
    assert!(psbt_info.fee_sat > 0);
    assert_eq!(psbt_info.inputs.len(), 1);
    for inp in &psbt_info.inputs {
        assert_eq!(inp.amount_sat, sats);
        assert!(outpoints.contains(&inp.outpoint));
    }
    assert_eq!(psbt_info.outputs.len() as u8, num_utxos + 1);
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
    assert_eq!(psbt_info.signature_count, 0);
    let signed_2 = wlt_2.signer.sign_psbt(psbt.clone(), None).unwrap();
    let psbt_info = wlt_2.multisig.inspect_psbt(signed_2.clone()).unwrap();
    assert_eq!(psbt_info.signature_count, 1);
    let op_info = wlt_2.respond_to_operation(
        op_info.operation_idx,
        RespondToOperation::Ack(signed_2.to_string()),
    );
    let Operation::CreateUtxosPending { status } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(status.acked_by.len(), 2);
    assert_eq!(status.nacked_by.len(), 0);
    assert_eq!(status.threshold, 3);
    assert_eq!(status.my_response, Some(true));
    let op_info = wlt_3.sync();
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    assert_eq!(op_info.initiator_xpub, wlt_1.xpub);
    let Operation::CreateUtxosToReview { psbt, status } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(status.acked_by.len(), 2);
    assert_eq!(status.nacked_by.len(), 0);
    assert_eq!(status.threshold, 3);
    assert_eq!(status.my_response, None);
    let bi = wlt_3.bak_ts();
    let op_info = wlt_3.sign_and_ack(psbt, op_info.operation_idx);
    assert!(wlt_3.bak_ts() > bi);
    assert_matches!(op_info.operation, Operation::CreateUtxosCompleted { .. });
    let unspents = test_list_unspents(wlt_3.multisig, None, false);
    assert_eq!(unspents.len(), (num_utxos + 1) as usize);
    let op_info = wlt_1.sync();
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    assert_eq!(op_info.initiator_xpub, wlt_1.xpub);
    assert_matches!(op_info.operation, Operation::CreateUtxosCompleted { .. });
    let op_info = wlt_2.sync();
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    assert_eq!(op_info.initiator_xpub, wlt_1.xpub);
    assert_matches!(op_info.operation, Operation::CreateUtxosCompleted { .. });
    wlt_3.assert_up_to_date();
    let unspents = test_list_unspents(wlt_3.multisig, None, false);
    assert_eq!(unspents.len(), (num_utxos + 1) as usize);
    mine(false, false);
}

fn create_utxos_discarded(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
    wlt_4: &mut MultisigParty,
) {
    println!("\n=== Create UTXOs discarded ===");
    let init_res = wlt_1.create_utxos_init(false, None, None, FEE_RATE);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    wlt_1.sign_and_ack(init_res.psbt.clone(), init_res.operation_idx);
    let op_info = wlt_2.sync();
    let op_info = wlt_2.nack(op_info.operation_idx);
    assert_matches!(
        op_info.operation,
        Operation::CreateUtxosPending { status: _ }
    );
    let op_info = wlt_3.nack(op_info.operation_idx);
    assert_matches!(
        op_info.operation,
        Operation::CreateUtxosDiscarded { status: _ }
    );
    let op_info = wlt_1.sync();
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    assert_eq!(op_info.initiator_xpub, wlt_1.xpub);
    let Operation::CreateUtxosDiscarded { status } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(
        status.nacked_by,
        set![wlt_2.xpub.to_string(), wlt_3.xpub.to_string()]
    );
    assert_eq!(status.my_response, Some(true));
    let op_info = wlt_2.sync();
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    assert_eq!(op_info.initiator_xpub, wlt_1.xpub);
    let Operation::CreateUtxosDiscarded { status } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(
        status.nacked_by,
        set![wlt_2.xpub.to_string(), wlt_3.xpub.to_string()]
    );
    assert_eq!(status.my_response, Some(false));
    let op_info = wlt_4.sync();
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    assert_eq!(op_info.initiator_xpub, wlt_1.xpub);
    let Operation::CreateUtxosDiscarded { status } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(
        status.nacked_by,
        set![wlt_2.xpub.to_string(), wlt_3.xpub.to_string()]
    );
    assert_eq!(status.my_response, None);
}

fn inflate_completed(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
) -> AssetIFA {
    println!("\n=== Inflation ===");
    let ifa_amounts = vec![100, 50];
    let initial_supply = ifa_amounts.iter().sum::<u64>();
    let bi = wlt_1.bak_ts();
    let ifa_asset = wlt_1.issue_asset_ifa(Some(&ifa_amounts), Some(&[AMOUNT_INFLATION]), None);
    assert!(wlt_1.bak_ts() > bi);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    assert_eq!(ifa_asset.balance.settled, initial_supply);
    issuance_assertions(wlt_1, wlt_2, wlt_3, &ifa_asset.asset_id, AssetSchema::Ifa);
    let ifa_balance_1 = wlt_1.asset_balance(&ifa_asset.asset_id);
    let ifa_balance_2 = wlt_2.asset_balance(&ifa_asset.asset_id);
    let ifa_balance_3 = wlt_3.asset_balance(&ifa_asset.asset_id);
    assert_eq!(ifa_balance_1.settled, initial_supply);
    assert_eq!(ifa_balance_2.settled, initial_supply);
    assert_eq!(ifa_balance_3.settled, initial_supply);
    wlt_2.assert_up_to_date();
    let inflation_amounts = vec![25, 26];
    let bi = wlt_2.bak_ts();
    let init_res = wlt_2.inflate_init(ifa_asset.asset_id.clone(), inflation_amounts.clone());
    assert!(wlt_2.bak_ts() > bi);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    wlt_2.sign_and_ack(init_res.psbt.clone(), init_res.operation_idx);
    let op_info = wlt_3.sync();
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    let Operation::InflationToReview {
        psbt,
        details,
        status: _,
    } = op_info.operation
    else {
        panic!("unexpected operation {op_info:?}");
    };
    let rgb_inspection = wlt_3
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
    let mut sorted_expected = inflation_amounts.clone();
    sorted_expected.sort();
    assert_eq!(sorted_inflate_outputs, sorted_expected);
    let op_info = wlt_3.sign_and_ack(psbt, op_info.operation_idx);
    let Operation::InflationPending { status, details: _ } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(status.acked_by.len(), 2);
    assert_eq!(status.nacked_by.len(), 0);
    assert_eq!(status.threshold, 3);
    assert_eq!(status.my_response, Some(true));
    let op_info = wlt_1.sync();
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    let Operation::InflationToReview {
        psbt,
        details: _,
        status: _,
    } = op_info.operation
    else {
        panic!("unexpected operation {op_info:?}");
    };
    let bi = wlt_1.bak_ts();
    let op_info = wlt_1.sign_and_ack(psbt, op_info.operation_idx);
    assert!(wlt_1.bak_ts() > bi);
    assert_matches!(op_info.operation, Operation::InflationCompleted { .. });
    let op_info = wlt_2.sync();
    assert_matches!(op_info.operation, Operation::InflationCompleted { .. });
    let op_info = wlt_3.sync();
    assert_matches!(op_info.operation, Operation::InflationCompleted { .. });
    mine(false, false);
    wlt_1.wait_refresh(Some(&ifa_asset.asset_id), None);
    wlt_2.wait_refresh(Some(&ifa_asset.asset_id), None);
    wlt_3.wait_refresh(Some(&ifa_asset.asset_id), None);
    let inflate_transfers_1 = wlt_1.list_transfers(&ifa_asset.asset_id);
    assert_last_transfer_settled(&inflate_transfers_1);
    let inflate_transfers_2 = wlt_2.list_transfers(&ifa_asset.asset_id);
    assert_last_transfer_settled(&inflate_transfers_2);
    let inflate_transfers_3 = wlt_3.list_transfers(&ifa_asset.asset_id);
    assert_last_transfer_settled(&inflate_transfers_3);
    let inflation_amount_total = inflation_amounts.iter().sum::<u64>();
    let new_supply = initial_supply + inflation_amount_total;
    let final_balance_1 = wlt_1.asset_balance(&ifa_asset.asset_id);
    let final_balance_2 = wlt_2.asset_balance(&ifa_asset.asset_id);
    let final_balance_3 = wlt_3.asset_balance(&ifa_asset.asset_id);
    assert_eq!(final_balance_1.settled, new_supply);
    assert_eq!(final_balance_2.settled, new_supply);
    assert_eq!(final_balance_3.settled, new_supply);
    ifa_asset
}

fn inflate_discarded(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
    ifa_asset: AssetIFA,
) {
    println!("\n=== Inflation discarded ===");
    let init_res = wlt_3.inflate_init(ifa_asset.asset_id.clone(), vec![1]);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        init_res.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    wlt_3.nack(init_res.operation_idx);
    let op_info = wlt_2.nack(init_res.operation_idx);
    let Operation::InflationDiscarded { status, .. } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(
        status.nacked_by,
        set![wlt_2.xpub.to_string(), wlt_3.xpub.to_string()]
    );
    assert_eq!(status.my_response, Some(false));
    let op_info = wlt_1.sync();
    assert_eq!(op_info.initiator_xpub, wlt_3.xpub);
    let Operation::InflationDiscarded { status, .. } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(
        status.nacked_by,
        set![wlt_2.xpub.to_string(), wlt_3.xpub.to_string()]
    );
    assert_eq!(status.my_response, None);
    let op_info = wlt_3.sync();
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    let Operation::InflationDiscarded { status, .. } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(status.nacked_by.len(), 2);
    assert_eq!(status.acked_by.len(), 0);
    assert_eq!(status.threshold, 3);
    assert_eq!(status.my_response, Some(false));
}

fn issue_cfa_completed(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
) -> AssetCFA {
    println!("\n=== Issue CFA ===");
    let amts = vec![200, AMOUNT_SMALL];
    let bi = wlt_2.bak_ts();
    let cfa_asset = wlt_2.issue_asset_cfa(Some(&amts), Some(FILE_STR.to_string()));
    assert!(wlt_2.bak_ts() > bi);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let settled = amts.iter().sum::<u64>();
    assert_eq!(cfa_asset.balance.settled, settled);
    issuance_assertions(wlt_2, wlt_1, wlt_3, &cfa_asset.asset_id, AssetSchema::Cfa);
    let meta_1 = wlt_1
        .multisig
        .get_asset_metadata(cfa_asset.asset_id.clone())
        .unwrap();
    assert_eq!(meta_1.ticker, None);
    assert_eq!(meta_1.name, NAME);
    assert_eq!(meta_1.precision, PRECISION);
    assert_eq!(meta_1.initial_supply, settled);
    assert_eq!(meta_1.asset_schema, AssetSchema::Cfa);
    let asset_db_1 = wlt_1
        .multisig
        .database()
        .get_asset(cfa_asset.asset_id.clone())
        .unwrap()
        .unwrap();
    let asset_db_2 = wlt_2
        .multisig
        .database()
        .get_asset(cfa_asset.asset_id.clone())
        .unwrap()
        .unwrap();
    let asset_db_3 = wlt_3
        .multisig
        .database()
        .get_asset(cfa_asset.asset_id.clone())
        .unwrap()
        .unwrap();
    assert!(asset_db_1.media_idx.is_some());
    assert!(asset_db_2.media_idx.is_some());
    assert!(asset_db_3.media_idx.is_some());
    let media_1 = wlt_1
        .multisig
        .database()
        .get_media(asset_db_1.media_idx.unwrap())
        .unwrap()
        .unwrap();
    let media_2 = wlt_2
        .multisig
        .database()
        .get_media(asset_db_2.media_idx.unwrap())
        .unwrap()
        .unwrap();
    let media_3 = wlt_3
        .multisig
        .database()
        .get_media(asset_db_3.media_idx.unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(media_1.digest, media_2.digest);
    assert_eq!(media_2.digest, media_3.digest);
    let media_file_1 = wlt_1.multisig.media_dir().join(&media_1.digest);
    let media_file_2 = wlt_2.multisig.media_dir().join(&media_2.digest);
    let media_file_3 = wlt_3.multisig.media_dir().join(&media_3.digest);
    assert!(media_file_1.exists());
    assert!(media_file_2.exists());
    assert!(media_file_3.exists());
    let content_1 = std::fs::read(&media_file_1).unwrap();
    let content_2 = std::fs::read(&media_file_2).unwrap();
    let content_3 = std::fs::read(&media_file_3).unwrap();
    assert_eq!(content_1, content_2);
    assert_eq!(content_2, content_3);
    wlt_2.assert_up_to_date();
    wlt_1.assert_up_to_date();
    cfa_asset
}

fn issue_nia_completed(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
) -> AssetNIA {
    println!("\n=== Issue NIA ===");
    let amts = vec![50, 70, 30];
    let bi = wlt_2.bak_ts();
    let nia_asset_1 = wlt_2.issue_asset_nia(Some(&amts));
    assert!(wlt_2.bak_ts() > bi);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let settled = amts.iter().sum::<u64>();
    assert_eq!(nia_asset_1.balance.settled, settled);
    issuance_assertions(wlt_2, wlt_1, wlt_3, &nia_asset_1.asset_id, AssetSchema::Nia);
    let meta_1 = wlt_1
        .multisig
        .get_asset_metadata(nia_asset_1.asset_id.clone())
        .unwrap();
    assert_eq!(meta_1.ticker, Some(TICKER.to_string()));
    assert_eq!(meta_1.name, NAME);
    assert_eq!(meta_1.precision, PRECISION);
    assert_eq!(meta_1.initial_supply, settled);
    assert_eq!(meta_1.asset_schema, AssetSchema::Nia);
    wlt_2.assert_up_to_date();
    wlt_1.assert_up_to_date();
    nia_asset_1
}

fn issue_uda_completed(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
) -> AssetUDA {
    println!("\n=== Issue UDA ===");
    let image_str = ["tests", "qrcode.png"].join(MAIN_SEPARATOR_STR);
    let bi = wlt_3.bak_ts();
    let uda_asset =
        wlt_3.issue_asset_uda(Some(DETAILS), Some(FILE_STR), vec![&image_str, FILE_STR]);
    assert!(wlt_3.bak_ts() > bi);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    assert_eq!(uda_asset.balance.settled, 1);
    issuance_assertions(wlt_3, wlt_1, wlt_2, &uda_asset.asset_id, AssetSchema::Uda);
    let meta_1 = wlt_1
        .multisig
        .get_asset_metadata(uda_asset.asset_id.clone())
        .unwrap();
    assert_eq!(meta_1.ticker, Some(TICKER.to_string()));
    assert_eq!(meta_1.name, NAME);
    assert_eq!(meta_1.precision, PRECISION);
    assert_eq!(meta_1.initial_supply, 1);
    assert_eq!(meta_1.asset_schema, AssetSchema::Uda);
    let token_1 = uda_asset.token.as_ref().unwrap();
    assert_eq!(token_1.index, UDA_FIXED_INDEX);
    let asset_db_1 = wlt_1
        .multisig
        .database()
        .get_asset(uda_asset.asset_id.clone())
        .unwrap()
        .unwrap();
    let asset_db_2 = wlt_2
        .multisig
        .database()
        .get_asset(uda_asset.asset_id.clone())
        .unwrap()
        .unwrap();
    let asset_db_3 = wlt_3
        .multisig
        .database()
        .get_asset(uda_asset.asset_id.clone())
        .unwrap()
        .unwrap();
    assert!(asset_db_1.media_idx.is_none());
    assert!(asset_db_2.media_idx.is_none());
    assert!(asset_db_3.media_idx.is_none());
    let tokens_1 = wlt_1.multisig.database().iter_tokens().unwrap();
    let token_db_1 = tokens_1
        .into_iter()
        .find(|t| t.asset_idx == asset_db_1.idx)
        .unwrap();
    let tokens_2 = wlt_2.multisig.database().iter_tokens().unwrap();
    let token_db_2 = tokens_2
        .into_iter()
        .find(|t| t.asset_idx == asset_db_2.idx)
        .unwrap();
    let tokens_3 = wlt_3.multisig.database().iter_tokens().unwrap();
    let token_db_3 = tokens_3
        .into_iter()
        .find(|t| t.asset_idx == asset_db_3.idx)
        .unwrap();
    assert_eq!(token_db_1.index, UDA_FIXED_INDEX);
    assert_eq!(token_db_2.index, UDA_FIXED_INDEX);
    assert_eq!(token_db_3.index, UDA_FIXED_INDEX);
    let token_medias_1 = wlt_1.multisig.database().iter_token_medias().unwrap();
    let token_media_entries_1: Vec<_> = token_medias_1
        .into_iter()
        .filter(|tm| tm.token_idx == token_db_1.idx)
        .collect();
    let token_medias_2 = wlt_2.multisig.database().iter_token_medias().unwrap();
    let token_media_entries_2: Vec<_> = token_medias_2
        .into_iter()
        .filter(|tm| tm.token_idx == token_db_2.idx)
        .collect();
    let token_medias_3 = wlt_3.multisig.database().iter_token_medias().unwrap();
    let token_media_entries_3: Vec<_> = token_medias_3
        .into_iter()
        .filter(|tm| tm.token_idx == token_db_3.idx)
        .collect();
    assert_eq!(token_media_entries_1.len(), 3);
    assert_eq!(token_media_entries_2.len(), 3);
    assert_eq!(token_media_entries_3.len(), 3);
    let medias_1 = wlt_1.multisig.database().iter_media().unwrap();
    let medias_2 = wlt_2.multisig.database().iter_media().unwrap();
    let medias_3 = wlt_3.multisig.database().iter_media().unwrap();
    let mut digests_1: Vec<String> = token_media_entries_1
        .iter()
        .map(|tm| {
            medias_1
                .iter()
                .find(|m| m.idx == tm.media_idx)
                .unwrap()
                .digest
                .clone()
        })
        .collect();
    digests_1.sort();
    let mut digests_2: Vec<String> = token_media_entries_2
        .iter()
        .map(|tm| {
            medias_2
                .iter()
                .find(|m| m.idx == tm.media_idx)
                .unwrap()
                .digest
                .clone()
        })
        .collect();
    digests_2.sort();
    let mut digests_3: Vec<String> = token_media_entries_3
        .iter()
        .map(|tm| {
            medias_3
                .iter()
                .find(|m| m.idx == tm.media_idx)
                .unwrap()
                .digest
                .clone()
        })
        .collect();
    digests_3.sort();
    assert_eq!(digests_1, digests_2);
    assert_eq!(digests_2, digests_3);
    let attachments = &token_1.attachments;
    assert_eq!(attachments.len(), 2);
    assert!(token_1.media.is_some());
    wlt_3.assert_up_to_date();
    uda_asset
}

fn receive_failed(wlt_1: &mut MultisigParty, wlt_2: &mut MultisigParty, wlt_3: &mut MultisigParty) {
    println!("\n=== Receive RGB failed ===");
    let receive_data = wlt_1.blind_receive();
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let batch_transfer_idx = receive_data.batch_transfer_idx;
    let recipient_id = receive_data.recipient_id.clone();
    let online = wlt_1.online();
    let changed = wlt_1
        .multisig
        .fail_transfers(online, Some(batch_transfer_idx), false, false)
        .unwrap();
    assert!(changed);
    let transfers = test_list_transfers(wlt_1.multisig_ref(), None);
    let t = transfers
        .iter()
        .find(|t| t.batch_transfer_idx == batch_transfer_idx)
        .unwrap();
    assert_eq!(t.status, TransferStatus::Failed);
    wlt_2.sync();
    let transfers = test_list_transfers(wlt_2.multisig_ref(), None);
    let t = transfers
        .iter()
        .find(|t| t.recipient_id.as_deref() == Some(&recipient_id))
        .unwrap();
    assert_eq!(t.status, TransferStatus::Failed);
    wlt_3.sync();
    let transfers = test_list_transfers(wlt_3.multisig_ref(), None);
    let t = transfers
        .iter()
        .find(|t| t.recipient_id.as_deref() == Some(&recipient_id))
        .unwrap();
    assert_eq!(t.status, TransferStatus::Failed);
}

fn send_btc_completed(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
    singlesig_wlt: &mut SinglesigParty,
) {
    println!("\n=== Send BTC ===");
    let prev_balance = test_get_btc_balance(singlesig_wlt.wallet, singlesig_wlt.online);
    let addr = test_get_address(singlesig_wlt.wallet);
    let amount = 1000;
    let bi = wlt_1.bak_ts();
    let init_res = wlt_1.send_btc_init(&addr, amount);
    assert!(wlt_1.bak_ts() > bi);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        init_res.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    let op_info = wlt_1.sync();
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    let Operation::SendBtcToReview { psbt, .. } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    wlt_1.sign_and_ack(psbt.clone(), init_res.operation_idx);
    let bi = wlt_2.bak_ts();
    let op_info = wlt_2.sync();
    assert!(wlt_2.bak_ts() > bi);
    assert_eq!(op_info.initiator_xpub, wlt_1.xpub);
    let Operation::SendBtcToReview { psbt, status } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(status.acked_by.len(), 1);
    assert_eq!(status.nacked_by.len(), 0);
    assert_eq!(status.threshold, 3);
    assert_eq!(status.my_response, None);
    let op_info = wlt_2.sign_and_ack(psbt.clone(), op_info.operation_idx);
    let Operation::SendBtcPending { status } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(status.acked_by.len(), 2);
    assert_eq!(status.nacked_by.len(), 0);
    assert_eq!(status.threshold, 3);
    assert_eq!(status.my_response, Some(true));
    let op_info = wlt_3.sync();
    assert_eq!(op_info.initiator_xpub, wlt_1.xpub);
    let Operation::SendBtcToReview { psbt, status } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(status.acked_by.len(), 2);
    assert_eq!(status.nacked_by.len(), 0);
    assert_eq!(status.threshold, 3);
    assert_eq!(status.my_response, None);
    let bi = wlt_3.bak_ts();
    let op_info = wlt_3.sign_and_ack(psbt.clone(), op_info.operation_idx);
    assert!(wlt_3.bak_ts() > bi);
    let Operation::SendBtcCompleted { txid, status } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(status.acked_by.len(), 3);
    assert_eq!(status.nacked_by.len(), 0);
    assert_eq!(status.threshold, 3);
    assert_eq!(status.my_response, Some(true));
    wlt_1.sync();
    wlt_2.sync();
    let transactions = test_list_transactions(wlt_1.multisig, Some(wlt_1.online));
    let transaction = transactions.first().unwrap();
    assert_eq!(transaction.txid, txid);
    assert_matches!(transaction.transaction_type, TransactionType::User);
    let transactions = test_list_transactions(wlt_2.multisig, Some(wlt_2.online));
    let transaction = transactions.first().unwrap();
    assert_eq!(transaction.txid, txid);
    assert_matches!(transaction.transaction_type, TransactionType::User);
    let transactions = test_list_transactions(wlt_3.multisig, Some(wlt_3.online));
    let transaction = transactions.first().unwrap();
    assert_eq!(transaction.txid, txid);
    assert_matches!(transaction.transaction_type, TransactionType::User);
    mine(false, false);
    let balance = test_get_btc_balance(singlesig_wlt.wallet, singlesig_wlt.online);
    assert_eq!(
        balance.vanilla.settled,
        prev_balance.vanilla.settled + amount
    );
}

fn send_btc_discarded(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
) {
    println!("\n=== Send BTC discarded ===");
    let addr = wlt_1.get_address();
    let init_res = wlt_3.send_btc_init(&addr, 999);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        init_res.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    wlt_3.nack(init_res.operation_idx);
    let op_info = wlt_2.nack(init_res.operation_idx);
    let Operation::SendBtcDiscarded { status } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(
        status.nacked_by,
        set![wlt_2.xpub.to_string(), wlt_3.xpub.to_string()]
    );
    assert_eq!(status.my_response, Some(false));
    let op_info = wlt_1.sync();
    assert_eq!(op_info.initiator_xpub, wlt_3.xpub);
    let Operation::SendBtcDiscarded { status } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(
        status.nacked_by,
        set![wlt_2.xpub.to_string(), wlt_3.xpub.to_string()]
    );
    assert_eq!(status.my_response, None);
    let op_info = wlt_3.sync();
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    let Operation::SendBtcDiscarded { status } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(status.nacked_by.len(), 2);
    assert_eq!(status.acked_by.len(), 0);
    assert_eq!(status.threshold, 3);
    assert_eq!(status.my_response, Some(false));
}

fn send_discarded(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
    singlesig_wlt: &mut SinglesigParty,
    cfa_asset: AssetCFA,
    nia_asset_1: AssetNIA,
) {
    println!("\n=== Send RGB discarded from multisig to singlesig ===");
    let rcv_data_1 = test_witness_receive(singlesig_wlt.wallet);
    let rcv_data_2 = test_blind_receive(singlesig_wlt.wallet);
    let rcv_data_3 = test_blind_receive(singlesig_wlt.wallet);
    let cfa_amount_witness = AMOUNT_SMALL;
    let cfa_amount_blind = 20;
    let send_recipient_map = HashMap::from([
        (
            cfa_asset.asset_id.clone(),
            vec![
                Recipient {
                    assignment: Assignment::Fungible(cfa_amount_witness),
                    recipient_id: rcv_data_1.recipient_id.clone(),
                    witness_data: Some(WitnessData {
                        amount_sat: 1000,
                        blinding: None,
                    }),
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    assignment: Assignment::Fungible(cfa_amount_blind),
                    recipient_id: rcv_data_3.recipient_id.clone(),
                    witness_data: None,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
            ],
        ),
        (
            nia_asset_1.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(AMOUNT_SMALL),
                recipient_id: rcv_data_2.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    wlt_1.assert_up_to_date();
    let init_res = wlt_1.send_init(send_recipient_map);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    wlt_1.nack(init_res.operation_idx);
    let op_info = wlt_2.sync();
    let op_info = wlt_2.nack(op_info.operation_idx);
    assert_matches!(op_info.operation, Operation::SendDiscarded { .. });
    let op_info = wlt_3.sync();
    assert_matches!(op_info.operation, Operation::SendDiscarded { .. });
    let op_info = wlt_1.sync();
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    assert_eq!(op_info.initiator_xpub, wlt_1.xpub);
    let Operation::SendDiscarded { status, .. } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(
        status.nacked_by,
        set![wlt_1.xpub.to_string(), wlt_2.xpub.to_string()]
    );
    assert_eq!(status.my_response, Some(false));
}

fn send_extra_completed(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
    singlesig_wlt: &mut SinglesigParty,
    nia_asset_2: &AssetNIA,
    cfa_asset: &AssetCFA,
    nia_asset_1: &AssetNIA,
) {
    println!("\n=== Send with extra ===");
    let nia_asset_2_id = &nia_asset_2.asset_id;
    let cfa_asset_id = &cfa_asset.asset_id;
    let nia_asset_1_id = &nia_asset_1.asset_id;
    let cfa_balance_before = wlt_1.asset_balance(cfa_asset_id).settled;
    let nia_balance_before = wlt_1.asset_balance(nia_asset_1_id).settled;
    let nia_asset_2_balance_before = wlt_1.asset_balance(nia_asset_2_id).settled;
    let extra_amount = 10u64;
    let rcv_data = test_blind_receive(singlesig_wlt.wallet);
    let send_recipient_map = HashMap::from([(
        nia_asset_2_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(extra_amount),
            recipient_id: rcv_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let bi = wlt_1.bak_ts();
    let init_res = wlt_1.send_init(send_recipient_map);
    assert!(wlt_1.bak_ts() > bi);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    wlt_1.sign_and_ack(init_res.psbt, init_res.operation_idx);
    let bi = wlt_2.bak_ts();
    let op_info = wlt_2.sync();
    assert!(wlt_2.bak_ts() > bi);
    let Operation::SendToReview { psbt, .. } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    wlt_2.sign_and_ack(psbt, op_info.operation_idx);
    let op_info = wlt_3.sync();
    let Operation::SendToReview { psbt, .. } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    let bi = wlt_3.bak_ts();
    let op_info = wlt_3.sign_and_ack(psbt, op_info.operation_idx);
    assert!(wlt_3.bak_ts() > bi);
    assert_matches!(op_info.operation, Operation::SendCompleted { .. });
    wlt_1.sync();
    wlt_2.sync();
    wait_for_refresh(singlesig_wlt.wallet, singlesig_wlt.online, None, None);
    wlt_1.wait_refresh(Some(nia_asset_2_id), None);
    wlt_2.wait_refresh(Some(nia_asset_2_id), None);
    wlt_3.wait_refresh(Some(nia_asset_2_id), None);
    mine(false, false);
    wait_for_refresh(
        singlesig_wlt.wallet,
        singlesig_wlt.online,
        Some(nia_asset_2_id),
        None,
    );
    wlt_1.wait_refresh(Some(nia_asset_2_id), None);
    wlt_2.wait_refresh(Some(nia_asset_2_id), None);
    wlt_3.wait_refresh(Some(nia_asset_2_id), None);
    let expected_nia_asset_2 = nia_asset_2_balance_before - extra_amount;
    assert_eq!(
        wlt_1.asset_balance(nia_asset_2_id).settled,
        expected_nia_asset_2,
    );
    let cfa_balance_after = wlt_1.asset_balance(cfa_asset_id).settled;
    assert_eq!(cfa_balance_after, cfa_balance_before);
    let nia_balance_after = wlt_1.asset_balance(nia_asset_1_id).settled;
    assert_eq!(nia_balance_after, nia_balance_before);
    let unspents_1 = wlt_1.list_unspents(true);
    let cfa_found = unspents_1.iter().any(|u| {
        u.rgb_allocations
            .iter()
            .any(|a| a.asset_id.as_deref() == Some(cfa_asset_id))
    });
    assert!(cfa_found);
    let nia_found = unspents_1.iter().any(|u| {
        u.rgb_allocations
            .iter()
            .any(|a| a.asset_id.as_deref() == Some(nia_asset_1_id))
    });
    assert!(nia_found);
    assert_eq!(
        wlt_2.asset_balance(nia_asset_2_id).settled,
        expected_nia_asset_2,
    );
    assert_eq!(
        wlt_2.asset_balance(cfa_asset_id).settled,
        cfa_balance_before
    );
    assert_eq!(
        wlt_2.asset_balance(nia_asset_1_id).settled,
        nia_balance_before
    );
    let unspents_2 = wlt_2.list_unspents(true);
    assert!(unspents_2.iter().any(|u| {
        u.rgb_allocations
            .iter()
            .any(|a| a.asset_id.as_deref() == Some(cfa_asset_id))
    }));
    assert!(unspents_2.iter().any(|u| {
        u.rgb_allocations
            .iter()
            .any(|a| a.asset_id.as_deref() == Some(nia_asset_1_id))
    }));
    assert_eq!(
        wlt_3.asset_balance(nia_asset_2_id).settled,
        expected_nia_asset_2,
    );
    assert_eq!(
        wlt_3.asset_balance(cfa_asset_id).settled,
        cfa_balance_before
    );
    assert_eq!(
        wlt_3.asset_balance(nia_asset_1_id).settled,
        nia_balance_before
    );
    let unspents_3 = wlt_3.list_unspents(true);
    assert!(unspents_3.iter().any(|u| {
        u.rgb_allocations
            .iter()
            .any(|a| a.asset_id.as_deref() == Some(cfa_asset_id))
    }));
    assert!(unspents_3.iter().any(|u| {
        u.rgb_allocations
            .iter()
            .any(|a| a.asset_id.as_deref() == Some(nia_asset_1_id))
    }));
}

fn send_failed(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
    singlesig_wlt: &mut SinglesigParty,
    nia_asset_2: &AssetNIA,
) {
    println!("\n=== Send RGB failed from multisig ===");
    let send_amount = 10u64;
    let rcv_data = test_blind_receive(singlesig_wlt.wallet);
    let send_recipient_map = HashMap::from([(
        nia_asset_2.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(send_amount),
            recipient_id: rcv_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    wlt_1.assert_up_to_date();
    let init_res = wlt_1.send_init(send_recipient_map);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    wlt_1.sign_and_ack(init_res.psbt, init_res.operation_idx);
    let op_info = wlt_2.sync();
    let Operation::SendToReview { psbt, .. } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    wlt_2.sign_and_ack(psbt, op_info.operation_idx);
    let op_info = wlt_3.sync();
    let Operation::SendToReview { psbt, .. } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    let op_info = wlt_3.sign_and_ack(psbt, op_info.operation_idx);
    assert_matches!(op_info.operation, Operation::SendCompleted { .. });
    wlt_1.sync();
    wlt_2.sync();
    let transfers = wlt_1.list_transfers(&nia_asset_2.asset_id);
    let last = transfers.last().unwrap();
    assert_eq!(last.status, TransferStatus::WaitingCounterparty);
    let batch_transfer_idx = last.batch_transfer_idx;
    let online = wlt_1.online();
    let changed = wlt_1
        .multisig
        .fail_transfers(online, Some(batch_transfer_idx), false, false)
        .unwrap();
    assert!(changed);
    let transfers = wlt_1.list_transfers(&nia_asset_2.asset_id);
    assert_eq!(transfers.last().unwrap().status, TransferStatus::Failed);
    wlt_2.wait_refresh(Some(&nia_asset_2.asset_id), None);
    let transfers = wlt_2.list_transfers(&nia_asset_2.asset_id);
    assert_eq!(transfers.last().unwrap().status, TransferStatus::Failed);
    wlt_3.wait_refresh(Some(&nia_asset_2.asset_id), None);
    let transfers = wlt_3.list_transfers(&nia_asset_2.asset_id);
    assert_eq!(transfers.last().unwrap().status, TransferStatus::Failed);
}

fn send_to_singlesig(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
    singlesig_wlt: &mut SinglesigParty,
    cfa_asset: &AssetCFA,
    nia_asset_1: &AssetNIA,
    nia_asset_2: &AssetNIA,
) {
    println!("\n=== Send RGB from multisig to singlesig ===");
    let rcv_data_1 = test_witness_receive(singlesig_wlt.wallet);
    let rcv_data_2 = test_blind_receive(singlesig_wlt.wallet);
    let rcv_data_3 = test_blind_receive(singlesig_wlt.wallet);
    let rcv_data_4 = test_blind_receive(singlesig_wlt.wallet);
    let cfa_amount_witness = AMOUNT_SMALL;
    let cfa_amount_blind = 20;
    let nia_asset_2_amount = 30;
    let send_recipient_map = HashMap::from([
        (
            cfa_asset.asset_id.clone(),
            vec![
                Recipient {
                    assignment: Assignment::Fungible(cfa_amount_witness),
                    recipient_id: rcv_data_1.recipient_id.clone(),
                    witness_data: Some(WitnessData {
                        amount_sat: 1000,
                        blinding: None,
                    }),
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    assignment: Assignment::Fungible(cfa_amount_blind),
                    recipient_id: rcv_data_3.recipient_id.clone(),
                    witness_data: None,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
            ],
        ),
        (
            nia_asset_1.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(AMOUNT_SMALL),
                recipient_id: rcv_data_2.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            nia_asset_2.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(nia_asset_2_amount),
                recipient_id: rcv_data_4.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    wlt_1.assert_up_to_date();
    let bi = wlt_1.bak_ts();
    let init_res = wlt_1.send_init(send_recipient_map);
    assert!(wlt_1.bak_ts() > bi);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    wlt_1.sign_and_ack(init_res.psbt, init_res.operation_idx);
    let bi = wlt_2.bak_ts();
    let op_info = wlt_2.sync();
    assert!(wlt_2.bak_ts() > bi);
    assert_eq!(
        op_info.operation_idx,
        OP_COUNTER.load(Ordering::SeqCst) as i32
    );
    let Operation::SendToReview {
        psbt,
        details,
        status,
    } = op_info.operation
    else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(status.acked_by.len(), 1);
    assert_eq!(status.nacked_by.len(), 0);
    assert_eq!(status.threshold, 3);
    assert_eq!(status.my_response, None);
    assert!(!details.is_donation);
    let psbt_info = wlt_2.multisig.inspect_psbt(psbt.clone()).unwrap();
    assert!(psbt_info.inputs.len() >= 2);
    assert!(psbt_info.outputs.len() >= 2);
    let op_return_out = &psbt_info.outputs[0];
    assert!(op_return_out.is_op_return);
    for out in &psbt_info.outputs[1..] {
        assert!(out.address.is_some());
    }
    let rgb_inspection = wlt_2
        .multisig
        .inspect_rgb_transfer(psbt.clone(), details.fascia_path, details.entropy)
        .unwrap();
    assert_eq!(rgb_inspection.close_method, CloseMethod::OpretFirst);
    assert_eq!(rgb_inspection.commitment_hex.len(), 64);
    assert_eq!(rgb_inspection.operations.len(), 3);
    let cfa_transfer = rgb_inspection
        .operations
        .iter()
        .find(|op| op.asset_id == cfa_asset.asset_id)
        .unwrap();
    let nia_transfer = rgb_inspection
        .operations
        .iter()
        .find(|op| op.asset_id == nia_asset_1.asset_id)
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
    let nia_asset_2_transfer = rgb_inspection
        .operations
        .iter()
        .find(|op| op.asset_id == nia_asset_2.asset_id)
        .unwrap();
    let nia_asset_2_inputs: Vec<_> = nia_asset_2_transfer
        .transitions
        .iter()
        .flat_map(|t| &t.inputs)
        .collect();
    let nia_asset_2_outputs: Vec<_> = nia_asset_2_transfer
        .transitions
        .iter()
        .flat_map(|t| &t.outputs)
        .collect();
    assert_eq!(nia_asset_2_outputs.len(), 2);
    let nia_asset_2_sent: Vec<_> = nia_asset_2_outputs.iter().filter(|o| !o.is_ours).collect();
    let nia_asset_2_change: Vec<_> = nia_asset_2_outputs.iter().filter(|o| o.is_ours).collect();
    assert_eq!(nia_asset_2_sent.len(), 1);
    assert_eq!(nia_asset_2_change.len(), 1);
    assert_eq!(
        nia_asset_2_sent[0].assignment.main_amount(),
        nia_asset_2_amount
    );
    assert!(nia_asset_2_sent[0].is_concealed);
    assert!(!nia_asset_2_sent[0].is_ours);
    let expected_nia_asset_2_change = AMOUNT_SMALL - nia_asset_2_amount;
    assert_eq!(
        nia_asset_2_change[0].assignment.main_amount(),
        expected_nia_asset_2_change
    );
    assert!(!nia_asset_2_change[0].is_concealed);
    assert!(nia_asset_2_change[0].is_ours);
    let nia_asset_2_total_output: u64 = nia_asset_2_outputs
        .iter()
        .map(|o| o.assignment.main_amount())
        .sum();
    assert_eq!(nia_asset_2_total_output, AMOUNT_SMALL);
    assert!(!nia_asset_2_inputs.is_empty());
    let nia_asset_2_total_input: u64 = nia_asset_2_inputs
        .iter()
        .map(|i| i.assignment.main_amount())
        .sum();
    assert_eq!(nia_asset_2_total_input, nia_asset_2_total_output);
    assert_eq!(nia_asset_2_total_input, AMOUNT_SMALL);
    for input in &nia_asset_2_inputs {
        assert!((input.vin as usize) < psbt_info.inputs.len());
        assert_matches!(input.assignment, Assignment::Fungible(_));
    }
    let op_info = wlt_2.sign_and_ack(psbt, op_info.operation_idx);
    let Operation::SendPending { status, details: _ } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    assert_eq!(status.acked_by.len(), 2);
    assert_eq!(status.nacked_by.len(), 0);
    assert_eq!(status.threshold, 3);
    assert_eq!(status.my_response, Some(true));
    let op_info = wlt_3.sync();
    let Operation::SendToReview {
        psbt,
        details: _,
        status: _,
    } = op_info.operation
    else {
        panic!("unexpected operation {op_info:?}");
    };
    let bi = wlt_3.bak_ts();
    let op_info = wlt_3.sign_and_ack(psbt, op_info.operation_idx);
    assert!(wlt_3.bak_ts() > bi);
    assert_matches!(op_info.operation, Operation::SendCompleted { .. });
    let op_info = wlt_1.sync();
    assert_matches!(op_info.operation, Operation::SendCompleted { .. });
    let op_info = wlt_2.sync();
    assert_matches!(op_info.operation, Operation::SendCompleted { .. });
    wait_for_refresh(singlesig_wlt.wallet, singlesig_wlt.online, None, None);
    wlt_1.wait_refresh(Some(&cfa_asset.asset_id), None);
    wlt_2.wait_refresh(Some(&cfa_asset.asset_id), None);
    wlt_3.wait_refresh(Some(&cfa_asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(
        singlesig_wlt.wallet,
        singlesig_wlt.online,
        Some(&cfa_asset.asset_id),
        None,
    );
    wlt_1.wait_refresh(Some(&cfa_asset.asset_id), None);
    wlt_2.wait_refresh(Some(&cfa_asset.asset_id), None);
    wlt_3.wait_refresh(Some(&cfa_asset.asset_id), None);
    let rcv_transfers = test_list_transfers(singlesig_wlt.wallet, Some(&cfa_asset.asset_id));
    assert_last_transfer_settled(&rcv_transfers);
    let send_transfers_1 = wlt_1.list_transfers(&cfa_asset.asset_id);
    assert_last_transfer_settled(&send_transfers_1);
    let send_transfers_2 = wlt_2.list_transfers(&cfa_asset.asset_id);
    assert_last_transfer_settled(&send_transfers_2);
    let send_transfers_3 = wlt_3.list_transfers(&cfa_asset.asset_id);
    assert_last_transfer_settled(&send_transfers_3);
    let balance_1 = wlt_1.asset_balance(&cfa_asset.asset_id);
    let balance_2 = wlt_2.asset_balance(&cfa_asset.asset_id);
    let balance_3 = wlt_3.asset_balance(&cfa_asset.asset_id);
    assert!(balance_1.settled < 300);
    assert!(balance_2.settled < 300);
    assert!(balance_3.settled < 300);
    let rcv_balance = test_get_asset_balance(singlesig_wlt.wallet, &cfa_asset.asset_id);
    let expected_rcv_balance = cfa_amount_witness + cfa_amount_blind;
    assert_eq!(rcv_balance.settled, expected_rcv_balance);
    wait_for_refresh(
        singlesig_wlt.wallet,
        singlesig_wlt.online,
        Some(&nia_asset_1.asset_id),
        None,
    );
    wait_for_refresh(
        singlesig_wlt.wallet,
        singlesig_wlt.online,
        Some(&nia_asset_2.asset_id),
        None,
    );
    let rcv_transfers = test_list_transfers(singlesig_wlt.wallet, Some(&nia_asset_2.asset_id));
    assert_last_transfer_settled(&rcv_transfers);
    let send_transfers = wlt_1.list_transfers(&nia_asset_2.asset_id);
    assert_last_transfer_settled(&send_transfers);
    let expected_nia_asset_2_remaining = AMOUNT_SMALL - nia_asset_2_amount;
    let balance_1 = wlt_1.asset_balance(&nia_asset_2.asset_id);
    let balance_2 = wlt_2.asset_balance(&nia_asset_2.asset_id);
    let balance_3 = wlt_3.asset_balance(&nia_asset_2.asset_id);
    assert_eq!(balance_1.settled, expected_nia_asset_2_remaining);
    assert_eq!(balance_2.settled, expected_nia_asset_2_remaining);
    assert_eq!(balance_3.settled, expected_nia_asset_2_remaining);
    let rcv_balance = test_get_asset_balance(singlesig_wlt.wallet, &nia_asset_2.asset_id);
    let expected_singlesig_nia_asset_2 = AMOUNT - AMOUNT_SMALL + nia_asset_2_amount;
    assert_eq!(rcv_balance.settled, expected_singlesig_nia_asset_2);
}

fn send_uda_to_singlesig(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
    singlesig_wlt: &mut SinglesigParty,
    uda_asset: &AssetUDA,
) {
    println!("\n=== Send UDA to singlesig ===");
    let rcv_data = test_blind_receive(singlesig_wlt.wallet);
    let send_recipient_map = HashMap::from([(
        uda_asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::NonFungible,
            recipient_id: rcv_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    wlt_1.assert_up_to_date();
    let bi = wlt_1.bak_ts();
    let init_res = wlt_1.send_init(send_recipient_map);
    assert!(wlt_1.bak_ts() > bi);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    wlt_1.sign_and_ack(init_res.psbt, init_res.operation_idx);
    let bi = wlt_2.bak_ts();
    let op_info = wlt_2.sync();
    assert!(wlt_2.bak_ts() > bi);
    let Operation::SendToReview { psbt, .. } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    wlt_2.sign_and_ack(psbt, op_info.operation_idx);
    let op_info = wlt_3.sync();
    let Operation::SendToReview { psbt, .. } = op_info.operation else {
        panic!("unexpected operation {op_info:?}");
    };
    let bi = wlt_3.bak_ts();
    let op_info = wlt_3.sign_and_ack(psbt, op_info.operation_idx);
    assert!(wlt_3.bak_ts() > bi);
    assert_matches!(op_info.operation, Operation::SendCompleted { .. });
    wlt_1.sync();
    wlt_2.sync();
    wait_for_refresh(singlesig_wlt.wallet, singlesig_wlt.online, None, None);
    wlt_1.wait_refresh(Some(&uda_asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(
        singlesig_wlt.wallet,
        singlesig_wlt.online,
        Some(&uda_asset.asset_id),
        None,
    );
    wlt_1.wait_refresh(Some(&uda_asset.asset_id), None);
    let transfers = test_list_transfers(singlesig_wlt.wallet, Some(&uda_asset.asset_id));
    assert_last_transfer_settled(&transfers);
    let balance = test_get_asset_balance(singlesig_wlt.wallet, &uda_asset.asset_id);
    assert_eq!(balance.settled, 1);
    let balance = wlt_1.asset_balance(&uda_asset.asset_id);
    assert_eq!(balance.settled, 0);
}

fn watch_only_wallet_sync(root: &KeyPair, keys: &MultisigKeys, dir: String) {
    println!("\n=== Watch-only sync ===");
    let token = create_token(root, Role::WatchOnly, None);
    let mut watch_only_wlt = get_test_ms_wallet(keys, dir);
    let watch_only_wlt_last_processed = watch_only_wlt
        .get_local_last_processed_operation_idx()
        .unwrap();
    assert_eq!(watch_only_wlt_last_processed, 0);
    let online = ms_go_online(&mut watch_only_wlt, &token);
    let info = watch_only_wlt.hub_info(online).unwrap();
    assert_eq!(info.user_role, UserRole::WatchOnly);
    assert!(info.last_operation_idx.is_some());
    ms_party!(&mut watch_only_wlt, online).sync_to_head();
    assert_synced_wallet_state(&watch_only_wlt);
}

fn witness_receive_completed(
    wlt_1: &mut MultisigParty,
    wlt_2: &mut MultisigParty,
    wlt_3: &mut MultisigParty,
    singlesig_wlt: &mut SinglesigParty,
    uda_asset: &AssetUDA,
) {
    println!("\n=== Witness receive ===");
    let bi = wlt_3.bak_ts();
    let receive_data = wlt_3.witness_receive();
    assert!(wlt_3.bak_ts() > bi);
    OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let recipient_map = HashMap::from([(
        uda_asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::NonFungible,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let _txid = test_send(singlesig_wlt.wallet, singlesig_wlt.online, &recipient_map);
    wlt_3.wait_refresh(None, None);
    wait_for_refresh(
        singlesig_wlt.wallet,
        singlesig_wlt.online,
        Some(&uda_asset.asset_id),
        None,
    );
    mine(false, false);
    wlt_3.wait_refresh(Some(&uda_asset.asset_id), None);
    wait_for_refresh(
        singlesig_wlt.wallet,
        singlesig_wlt.online,
        Some(&uda_asset.asset_id),
        None,
    );
    let transfers = wlt_3.list_transfers(&uda_asset.asset_id);
    assert_last_transfer_settled(&transfers);
    let transfers = test_list_transfers(singlesig_wlt.wallet, Some(&uda_asset.asset_id));
    assert_last_transfer_settled(&transfers);
    let bi = wlt_1.bak_ts();
    wlt_1.sync();
    assert!(wlt_1.bak_ts() > bi);
    let transfers = wlt_1.list_transfers(&uda_asset.asset_id);
    assert_last_transfer_settled(&transfers);
    let bi = wlt_2.bak_ts();
    wlt_2.sync();
    assert!(wlt_2.bak_ts() > bi);
    let transfers = wlt_2.list_transfers(&uda_asset.asset_id);
    assert_last_transfer_settled(&transfers);
    let balance = wlt_3.asset_balance(&uda_asset.asset_id);
    assert_eq!(balance.settled, 1);
    let balance = wlt_1.asset_balance(&uda_asset.asset_id);
    assert_eq!(balance.settled, 1);
    let balance = wlt_2.asset_balance(&uda_asset.asset_id);
    assert_eq!(balance.settled, 1);
    let balance = test_get_asset_balance(singlesig_wlt.wallet, &uda_asset.asset_id);
    assert_eq!(balance.settled, 0);
}

fn wlt_4_sync(wlt_4: &mut MultisigParty) {
    println!("\n=== Wallet 4 sync ===");
    let wlt_4_last_processed = wlt_4
        .multisig
        .get_local_last_processed_operation_idx()
        .unwrap();
    assert_eq!(wlt_4_last_processed, 1);
    wlt_4.sync_to_head();
    assert_synced_wallet_state(wlt_4.multisig_ref());
}

// tests

#[cfg(feature = "electrum")]
#[test]
#[serial]
fn success() {
    initialize();

    let bitcoin_network = BitcoinNetwork::Regtest;
    let threshold_colored = 3;
    let threshold_vanilla = 3;

    let wlt_1_keys = generate_keys(bitcoin_network);
    let wlt_2_keys = generate_keys(bitcoin_network);
    let wlt_3_keys = generate_keys(bitcoin_network);
    let wlt_4_keys = generate_keys(bitcoin_network);

    let cosigners = vec![
        Cosigner::from_keys(&wlt_1_keys, None),
        Cosigner::from_keys(&wlt_2_keys, None),
        Cosigner::from_keys(&wlt_3_keys, None),
        Cosigner::from_keys(&wlt_4_keys, None),
    ];
    let cosigner_xpubs: Vec<String> = cosigners
        .iter()
        .map(|c| c.account_xpub_colored.clone())
        .collect();

    // write hub configuration file and restart hub
    let root_keypair = KeyPair::new();
    let root_public_key = root_keypair.public();
    write_hub_config(
        &cosigner_xpubs,
        threshold_colored,
        threshold_vanilla,
        root_public_key.to_bytes_hex(),
        None,
    );
    restart_multisig_hub();

    // create biscuit tokens for cosigners
    let mut cosigner_tokens = vec![];
    for cosigner_xpub in &cosigner_xpubs {
        cosigner_tokens.push(create_token(
            &root_keypair,
            Role::Cosigner(cosigner_xpub.clone()),
            None,
        ));
    }

    // single-sig wallets for signing
    let wlt_1_singlesig = get_test_wallet_with_keys(&wlt_1_keys);
    let wlt_2_singlesig = get_test_wallet_with_keys(&wlt_2_keys);
    let wlt_3_singlesig = get_test_wallet_with_keys(&wlt_3_keys);
    let wlt_4_singlesig = get_test_wallet_with_keys(&wlt_4_keys);

    // multi-sig wallets
    let multisig_wlt_keys =
        MultisigKeys::new(cosigners.clone(), threshold_colored, threshold_vanilla);
    let random_str: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(6)
        .map(char::from)
        .collect();
    let mut wlt_1_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_1"));
    let wlt_1_multisig_online = ms_go_online(&mut wlt_1_multisig, &cosigner_tokens[0]);
    let mut wlt_2_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_2"));
    let wlt_2_multisig_online = ms_go_online(&mut wlt_2_multisig, &cosigner_tokens[1]);
    let mut wlt_3_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_3"));
    let wlt_3_multisig_online = ms_go_online(&mut wlt_3_multisig, &cosigner_tokens[2]);
    let mut wlt_4_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_4"));
    let wlt_4_multisig_online = ms_go_online(&mut wlt_4_multisig, &cosigner_tokens[3]);

    let mut wlt_1 = ms_party!(
        &wlt_1_singlesig,
        &mut wlt_1_multisig,
        wlt_1_multisig_online,
        &cosigner_xpubs[0]
    );
    let mut wlt_2 = ms_party!(
        &wlt_2_singlesig,
        &mut wlt_2_multisig,
        wlt_2_multisig_online,
        &cosigner_xpubs[1]
    );
    let mut wlt_3 = ms_party!(
        &wlt_3_singlesig,
        &mut wlt_3_multisig,
        wlt_3_multisig_online,
        &cosigner_xpubs[2]
    );
    let mut wlt_4 = ms_party!(
        &wlt_4_singlesig,
        &mut wlt_4_multisig,
        wlt_4_multisig_online,
        &cosigner_xpubs[3]
    );

    let descriptors = multisig_wlt_keys
        .build_descriptors(bitcoin_network)
        .unwrap();
    for wlt in [&wlt_1, &wlt_2, &wlt_3, &wlt_4] {
        let wlt_keys = wlt.multisig.get_keys();
        assert_eq!(wlt_keys, multisig_wlt_keys);
        let wlt_descriptors = wlt.multisig.get_descriptors();
        assert_eq!(wlt_descriptors, descriptors);
    }

    let sats = 30_000;
    send_sats_to_address(wlt_1.get_address(), Some(sats));
    mine(false, false);

    check_cosigner_hub_info(&mut wlt_1, &mut wlt_2, &mut wlt_3, &mut wlt_4);

    create_utxos_discarded(&mut wlt_1, &mut wlt_2, &mut wlt_3, &mut wlt_4);

    create_utxos_completed(&mut wlt_1, &mut wlt_2, &mut wlt_3, sats);

    let cfa_asset = issue_cfa_completed(&mut wlt_1, &mut wlt_2, &mut wlt_3);

    send_btc_discarded(&mut wlt_1, &mut wlt_2, &mut wlt_3);

    let nia_asset_1 = issue_nia_completed(&mut wlt_1, &mut wlt_2, &mut wlt_3);

    let uda_asset = issue_uda_completed(&mut wlt_1, &mut wlt_2, &mut wlt_3);

    let (mut singlesig_wlt, singlesig_wlt_online) = get_funded_wallet!();
    let mut singlesig_wlt = party!(&mut singlesig_wlt, singlesig_wlt_online);

    send_uda_to_singlesig(
        &mut wlt_1,
        &mut wlt_2,
        &mut wlt_3,
        &mut singlesig_wlt,
        &uda_asset,
    );

    witness_receive_completed(
        &mut wlt_1,
        &mut wlt_2,
        &mut wlt_3,
        &mut singlesig_wlt,
        &uda_asset,
    );

    send_discarded(
        &mut wlt_1,
        &mut wlt_2,
        &mut wlt_3,
        &mut singlesig_wlt,
        cfa_asset.clone(),
        nia_asset_1.clone(),
    );

    let nia_asset_2 =
        blind_receive_unknown_asset(&mut wlt_1, &mut wlt_2, &mut wlt_3, &mut singlesig_wlt);

    send_to_singlesig(
        &mut wlt_1,
        &mut wlt_2,
        &mut wlt_3,
        &mut singlesig_wlt,
        &cfa_asset,
        &nia_asset_1,
        &nia_asset_2,
    );

    let (wlt_1_last_processed_before_backup, backup_file) =
        backup(wlt_1.multisig, &format!("{random_str}_1"));

    send_extra_completed(
        &mut wlt_1,
        &mut wlt_2,
        &mut wlt_3,
        &mut singlesig_wlt,
        &nia_asset_2,
        &cfa_asset,
        &nia_asset_1,
    );

    let ifa_asset = inflate_completed(&mut wlt_1, &mut wlt_2, &mut wlt_3);
    inflate_discarded(&mut wlt_1, &mut wlt_2, &mut wlt_3, ifa_asset);

    blind_receive_completed(
        &mut wlt_1,
        &mut wlt_2,
        &mut wlt_3,
        &mut singlesig_wlt,
        nia_asset_1,
    );

    send_btc_completed(&mut wlt_1, &mut wlt_2, &mut wlt_3, &mut singlesig_wlt);

    receive_failed(&mut wlt_1, &mut wlt_2, &mut wlt_3);

    send_failed(
        &mut wlt_1,
        &mut wlt_2,
        &mut wlt_3,
        &mut singlesig_wlt,
        &nia_asset_2,
    );

    wlt_4_sync(&mut wlt_4);

    check_change_consistency(&mut wlt_1, &mut wlt_4);

    watch_only_wallet_sync(
        &root_keypair,
        &multisig_wlt_keys,
        format!("{random_str}_watch_only"),
    );

    backup_restore(
        &backup_file,
        &random_str,
        multisig_wlt_keys,
        wlt_1_last_processed_before_backup,
        &cosigner_tokens[0],
    );
}

#[cfg(feature = "electrum")]
#[test]
#[serial]
fn fail() {
    initialize();

    let bitcoin_network = BitcoinNetwork::Regtest;
    let threshold_colored = 2;
    let threshold_vanilla = 2;

    let wlt_1_keys = generate_keys(bitcoin_network);
    let wlt_2_keys = generate_keys(bitcoin_network);
    let wlt_3_keys = generate_keys(bitcoin_network);

    let cosigners = vec![
        Cosigner::from_keys(&wlt_1_keys, None),
        Cosigner::from_keys(&wlt_2_keys, None),
        Cosigner::from_keys(&wlt_3_keys, None),
    ];
    let num_cosigners = cosigners.len() as u8;
    let cosigner_xpubs: Vec<String> = cosigners
        .iter()
        .map(|c| c.account_xpub_colored.clone())
        .collect();

    // write hub configuration file and restart hub
    let root_keypair = KeyPair::new();
    let root_public_key = root_keypair.public();
    write_hub_config(
        &cosigner_xpubs,
        threshold_colored,
        threshold_vanilla,
        root_public_key.to_bytes_hex(),
        None,
    );
    restart_multisig_hub();

    // create biscuit tokens for cosigners
    let mut cosigner_tokens = vec![];
    for cosigner_xpub in &cosigner_xpubs {
        cosigner_tokens.push(create_token(
            &root_keypair,
            Role::Cosigner(cosigner_xpub.clone()),
            None,
        ));
    }

    let random_str: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(6)
        .map(char::from)
        .collect();
    let data_dir = get_test_data_dir_path()
        .join(format!("{random_str}_1"))
        .to_string_lossy()
        .to_string();
    let _ = fs::create_dir_all(&data_dir);

    // no cosigners supplied
    let invalid_multisig_wlt_keys = MultisigKeys::new(vec![], threshold_colored, threshold_vanilla);
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    let Err(err) = res else {
        panic!("expected Err, got Ok")
    };
    assert_eq!(err, Error::NoCosignersSupplied);

    // invalid thresholds: higher than total cosigners
    let invalid_threshold = num_cosigners + 1;
    // - colored threshold
    let invalid_multisig_wlt_keys =
        MultisigKeys::new(cosigners.clone(), invalid_threshold, threshold_vanilla);
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    let Err(err) = res else {
        panic!("expected Err, got Ok")
    };
    assert_matches!(err, Error::InvalidMultisigThreshold { required, total } if required == invalid_threshold && total == num_cosigners);
    // - vanilla threshold
    let invalid_multisig_wlt_keys =
        MultisigKeys::new(cosigners.clone(), threshold_colored, invalid_threshold);
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    let Err(err) = res else {
        panic!("expected Err, got Ok")
    };
    assert_matches!(err, Error::InvalidMultisigThreshold { required, total } if required == invalid_threshold && total == num_cosigners);

    // invalid thresholds: k=0
    let invalid_threshold = 0;
    // - colored threshold
    let invalid_multisig_wlt_keys =
        MultisigKeys::new(cosigners.clone(), invalid_threshold, threshold_vanilla);
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    let Err(err) = res else {
        panic!("expected Err, got Ok")
    };
    assert_matches!(err, Error::InvalidMultisigThreshold { required, total } if required == invalid_threshold && total == num_cosigners);
    // - vanilla threshold
    let invalid_multisig_wlt_keys =
        MultisigKeys::new(cosigners.clone(), threshold_colored, invalid_threshold);
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    let Err(err) = res else {
        panic!("expected Err, got Ok")
    };
    assert_matches!(err, Error::InvalidMultisigThreshold { required, total } if required == invalid_threshold && total == num_cosigners);

    // invalid fingerprint
    let mut invalid_cosigners = cosigners.clone();
    let invalid_fingerprint = s!("invalid");
    invalid_cosigners[1].master_fingerprint = invalid_fingerprint.clone();
    let invalid_multisig_wlt_keys = MultisigKeys::new(
        invalid_cosigners.clone(),
        threshold_colored,
        threshold_vanilla,
    );
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    let Err(err) = res else {
        panic!("expected Err, got Ok")
    };
    assert_matches!(err, Error::InvalidCosigner { details: d } if d == format!("invalid master_fingerprint '{invalid_fingerprint}'"));

    // invalid xpub content
    let invalid_xpub = s!("invalid");
    // - colored xpub
    let mut invalid_cosigners = cosigners.clone();
    invalid_cosigners[1].account_xpub_colored = invalid_xpub.clone();
    let invalid_multisig_wlt_keys = MultisigKeys::new(
        invalid_cosigners.clone(),
        threshold_colored,
        threshold_vanilla,
    );
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    let Err(err) = res else {
        panic!("expected Err, got Ok")
    };
    assert_matches!(err, Error::InvalidCosigner { details: d } if d == format!("invalid colored xpub '{invalid_xpub}'"));
    // - vanilla xpub
    let mut invalid_cosigners = cosigners.clone();
    invalid_cosigners[1].account_xpub_vanilla = invalid_xpub.clone();
    let invalid_multisig_wlt_keys = MultisigKeys::new(
        invalid_cosigners.clone(),
        threshold_colored,
        threshold_vanilla,
    );
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    let Err(err) = res else {
        panic!("expected Err, got Ok")
    };
    assert_matches!(err, Error::InvalidCosigner { details: d } if d == format!("invalid vanilla xpub '{invalid_xpub}'"));

    // invalid xpub network
    let invalid_keys = generate_keys(BitcoinNetwork::Mainnet);
    let invalid_cosigner = Cosigner::from_keys(&invalid_keys, None);
    // - colored xpub
    let mut invalid_cosigners = cosigners.clone();
    invalid_cosigners[1].account_xpub_colored = invalid_cosigner.account_xpub_colored.clone();
    let invalid_multisig_wlt_keys = MultisigKeys::new(
        invalid_cosigners.clone(),
        threshold_colored,
        threshold_vanilla,
    );
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    let Err(err) = res else {
        panic!("expected Err, got Ok")
    };
    assert_matches!(err, Error::InvalidCosigner { details: d } if d == format!("colored xpub '{}' is for the wrong network", invalid_cosigner.account_xpub_colored));
    // - vanilla xpub
    let mut invalid_cosigners = cosigners.clone();
    invalid_cosigners[1].account_xpub_vanilla = invalid_cosigner.account_xpub_vanilla.clone();
    let invalid_multisig_wlt_keys = MultisigKeys::new(
        invalid_cosigners.clone(),
        threshold_colored,
        threshold_vanilla,
    );
    let res = MultisigWallet::new(get_test_wallet_data(&data_dir), invalid_multisig_wlt_keys);
    let Err(err) = res else {
        panic!("expected Err, got Ok")
    };
    assert_matches!(err, Error::InvalidCosigner { details: d } if d == format!("vanilla xpub '{}' is for the wrong network", invalid_cosigner.account_xpub_vanilla));

    // invalid rgb-lib version
    println!("setting MOCK_LOCAL_VERSION");
    MOCK_LOCAL_VERSION.replace(Some(s!("0.2")));
    let multisig_wlt_keys =
        MultisigKeys::new(cosigners.clone(), threshold_colored, threshold_vanilla);
    let mut wlt_1_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_1"));
    let res = ms_go_online_res(&mut wlt_1_multisig, &cosigner_tokens[0]).unwrap_err();
    assert_matches!(res, Error::MultisigHubService { details: d } if d == "rgb-lib version mismatch: local version is 0.2 but hub requires 0.3");

    // expired token
    let multisig_wlt_keys =
        MultisigKeys::new(cosigners.clone(), threshold_colored, threshold_vanilla);
    let mut wlt_1_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_1"));
    let expired_token = create_token(
        &root_keypair,
        Role::Cosigner(cosigner_xpubs[0].clone()),
        Some(Utc::now() - Duration::from_secs(1)),
    );
    let res = ms_go_online_res(&mut wlt_1_multisig, &expired_token).unwrap_err();
    assert_matches!(res, Error::MultisigHubService { details: d } if d == "Missing or invalid credentials");

    // invalid token
    let invalid_token = s!("invalid");
    let res = ms_go_online_res(&mut wlt_1_multisig, &invalid_token).unwrap_err();
    assert_matches!(res, Error::MultisigHubService { details: d } if d == "Missing or invalid credentials");

    // token for cosigner not in hub config
    let wlt_3_keys = generate_keys(bitcoin_network);
    let invalid_cosigner_token =
        create_token(&root_keypair, Role::Cosigner(wlt_3_keys.xpub.clone()), None);
    let res = ms_go_online_res(&mut wlt_1_multisig, &invalid_cosigner_token).unwrap_err();
    assert_matches!(res, Error::MultisigHubService { details: d } if d == "Missing or invalid credentials");

    // token with no xpub nor role
    let invalid_token = biscuit!("")
        .build(&root_keypair)
        .unwrap()
        .to_base64()
        .unwrap();
    let res = ms_go_online_res(&mut wlt_1_multisig, &invalid_token).unwrap_err();
    assert_matches!(res, Error::MultisigHubService { details: d } if d == "Missing or invalid credentials");

    // invalid hub URL
    let res = wlt_1_multisig
        .go_online(
            false,
            ELECTRUM_URL.to_string(),
            s!("invalid"),
            cosigner_tokens[0].to_string(),
        )
        .unwrap_err();
    assert_matches!(res, Error::MultisigHubService { details: d } if d == "URL must be valid and start with http:// or https://");

    // respond with PSBT that has no signatures
    let wlt_1_multisig_online = ms_go_online(&mut wlt_1_multisig, &cosigner_tokens[0]);
    send_sats_to_address(
        wlt_1_multisig.get_address(wlt_1_multisig_online).unwrap(),
        Some(10_000),
    );
    mine(false, false);
    let mut wlt_1 = ms_party!(&mut wlt_1_multisig, wlt_1_multisig_online);
    let init_res = wlt_1.create_utxos_init(false, None, None, FEE_RATE);
    let op_idx_1 = init_res.operation_idx;
    let unsigned_psbt = init_res.psbt.clone();
    let res = wlt_1
        .respond_to_operation_res(op_idx_1, RespondToOperation::Ack(unsigned_psbt.clone()))
        .unwrap_err();
    assert_matches!(
        res,
        Error::InvalidPsbt { details: d } if d == "PSBT has no signatures"
    );

    // cannot initiate a new operation if another is pending
    let res = wlt_1.create_utxos_init_res(false, None, None, FEE_RATE);
    assert_matches!(res, Err(Error::MultisigOperationInProgress));

    // respond to a non-pending operation
    let wlt_1_singlesig = get_test_wallet_with_keys(&wlt_1_keys);
    let signed_psbt = wlt_1_singlesig
        .sign_psbt(unsigned_psbt.clone(), None)
        .unwrap();
    wlt_1.respond_to_operation(op_idx_1, RespondToOperation::Ack(signed_psbt));
    let wlt_2_singlesig = get_test_wallet_with_keys(&wlt_2_keys);
    let signed_psbt = wlt_2_singlesig
        .sign_psbt(unsigned_psbt.clone(), None)
        .unwrap();
    let mut wlt_2_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_2"));
    let wlt_2_multisig_online = ms_go_online(&mut wlt_2_multisig, &cosigner_tokens[1]);
    let mut wlt_2 = ms_party!(&mut wlt_2_multisig, wlt_2_multisig_online);
    let op_2 = wlt_2.sync();
    wlt_2.respond_to_operation(
        op_2.operation_idx,
        RespondToOperation::Ack(signed_psbt.clone()),
    );
    let mut wlt_3_multisig = get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_3"));
    let wlt_3_multisig_online = ms_go_online(&mut wlt_3_multisig, &cosigner_tokens[2]);
    let mut wlt_3 = ms_party!(&mut wlt_3_multisig, wlt_3_multisig_online);
    let res = wlt_3
        .respond_to_operation_res(op_idx_1, RespondToOperation::Nack)
        .unwrap_err();
    assert_matches!(
        res,
        Error::MultisigCannotRespondToOperation { details: d } if d == "not pending"
    );

    // respond with PSBT that has the wrong TXID
    wlt_1.sync();
    wlt_2.assert_up_to_date();
    let init_res = wlt_1.create_utxos_init(false, Some(5), None, FEE_RATE);
    let op_idx_2 = init_res.operation_idx;
    let res = wlt_1
        .respond_to_operation_res(op_idx_2, RespondToOperation::Ack(signed_psbt.to_string()))
        .unwrap_err();
    assert_matches!(
        res,
        Error::InvalidPsbt { details: d } if d == "PSBT unrelated to operation"
    );

    // respond to already responded
    wlt_1.respond_to_operation(op_idx_2, RespondToOperation::Nack);
    let res = wlt_1
        .respond_to_operation_res(op_idx_2, RespondToOperation::Nack)
        .unwrap_err();
    assert_matches!(
        res,
        Error::MultisigCannotRespondToOperation { details: d } if d == "already responded"
    );

    // respond to a non-next operation
    wlt_2.respond_to_operation(op_idx_2, RespondToOperation::Nack);
    wlt_1.sync();
    wlt_2.assert_up_to_date();
    let init_res = wlt_1.create_utxos_init(false, Some(3), None, FEE_RATE);
    let op_idx_3 = init_res.operation_idx;
    let res = wlt_3
        .respond_to_operation_res(op_idx_3, RespondToOperation::Nack)
        .unwrap_err();
    assert_matches!(
        res,
        Error::MultisigCannotRespondToOperation { details: d } if d == "Cannot respond to operation: operation is not the next one to be processed"
    );

    // watch-only forbidden
    let token = create_token(&root_keypair, Role::WatchOnly, None);
    let mut watch_only_wlt =
        get_test_ms_wallet(&multisig_wlt_keys, format!("{random_str}_watch_only"));
    let online = ms_go_online(&mut watch_only_wlt, &token);
    let res = watch_only_wlt.get_address(online).unwrap_err();
    assert_eq!(res, Error::MultisigUserNotCosigner);
    let mut wlt_wo = ms_party!(&mut watch_only_wlt, online);
    let res = wlt_wo.issue_asset_cfa_res(None, None).unwrap_err();
    assert_eq!(res, Error::MultisigUserNotCosigner);
    let res = wlt_wo.issue_asset_nia_res(None).unwrap_err();
    assert_eq!(res, Error::MultisigUserNotCosigner);
    let res = wlt_wo.issue_asset_ifa_res(None, None, None).unwrap_err();
    assert_eq!(res, Error::MultisigUserNotCosigner);
    let res = wlt_wo.issue_asset_uda_res(None, None, vec![]).unwrap_err();
    assert_eq!(res, Error::MultisigUserNotCosigner);
    let res = wlt_wo.blind_receive_res().unwrap_err();
    assert_eq!(res, Error::MultisigUserNotCosigner);
    let res = wlt_wo.witness_receive_res().unwrap_err();
    assert_eq!(res, Error::MultisigUserNotCosigner);
    let res = wlt_wo.nack_res(0).unwrap_err();
    assert_eq!(res, Error::MultisigUserNotCosigner);
    let res = wlt_wo
        .create_utxos_init_res(false, None, None, FEE_RATE)
        .unwrap_err();
    assert_eq!(res, Error::MultisigUserNotCosigner);
    let res = wlt_wo.send_btc_init_res("address", AMOUNT).unwrap_err();
    assert_eq!(res, Error::MultisigUserNotCosigner);
    let res = wlt_wo.send_init_res(HashMap::new()).unwrap_err();
    assert_eq!(res, Error::MultisigUserNotCosigner);
    let res = wlt_wo.inflate_init_res(s!("asset_id"), vec![]).unwrap_err();
    assert_eq!(res, Error::MultisigUserNotCosigner);
}
