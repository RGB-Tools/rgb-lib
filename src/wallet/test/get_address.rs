use super::*;

#[test]
#[parallel]
fn success() {
    let data_dir = PrivateDataDir::new();
    let mut party = offline_party!(data_dir.wallet(false, None));
    let bak_info_before = party.db_backup_info_opt();
    assert!(bak_info_before.is_none());
    assert_eq!(
        party.wlt().get_wallet_data().data_dir,
        party.wlt_mut().get_wallet_data().data_dir
    );
    let address = party.wallet.get_address().unwrap();
    let bak_info_after = party.db_backup_info();
    assert!(
        bak_info_after
            .last_operation_timestamp
            .parse::<i128>()
            .unwrap()
            > 0
    );
    assert!(!address.is_empty());
}

// A revealed address index must not be lost when the transaction that wrote it rolls back. This
// is the in-process case: the change stays in the in-memory pending buffer and the next persist
// writes it again.
#[test]
#[parallel]
fn reveal_survives_rolled_back_txn() {
    let data_dir = PrivateDataDir::new();
    let mut wallet = data_dir.wallet(true, None);

    // reveal an address and write it, then roll the transaction back instead of committing
    let txn = wallet.database().begin_transaction().unwrap();
    let address = wallet.get_new_address().unwrap();
    wallet.persist_bdk(&txn).unwrap();
    drop(txn);

    // the reveal didn't reach the DB
    let txn = wallet.database().begin_transaction().unwrap();
    let changeset = txn.get_bdk_changeset().unwrap();
    txn.commit().unwrap();
    assert!(changeset.indexer.last_revealed.is_empty());

    // ...but it's still pending, so the next commit writes it
    let txn = wallet.database().begin_transaction().unwrap();
    wallet.persist_and_commit(txn).unwrap();

    let txn = wallet.database().begin_transaction().unwrap();
    let changeset = txn.get_bdk_changeset().unwrap();
    txn.commit().unwrap();
    assert!(!changeset.indexer.last_revealed.is_empty());

    // reloading the BDK wallet from the DB gives back the same address
    let txn = wallet.database().begin_transaction().unwrap();
    let reloaded = setup_bdk(
        &txn,
        wallet.get_wallet_dir(),
        wallet.get_descriptors().colored,
        wallet.get_descriptors().vanilla,
        false,
        BitcoinNetwork::Regtest,
    )
    .unwrap();
    txn.commit().unwrap();
    assert_eq!(
        reloaded
            .spk_index()
            .last_revealed_index(KeychainKind::External),
        wallet
            .bdk_wallet()
            .spk_index()
            .last_revealed_index(KeychainKind::External),
    );
    assert_eq!(
        reloaded
            .peek_address(
                KeychainKind::External,
                reloaded
                    .spk_index()
                    .last_revealed_index(KeychainKind::External)
                    .unwrap()
            )
            .address,
        address,
    );
}

// A crash after a broadcast must not lose the wallet's view of a TX that is already on the
// network: the pending changes are on disk and are folded back in when the wallet is reloaded.
#[test]
#[parallel]
fn bdk_pending_file_survives_a_crash() {
    let data_dir = PrivateDataDir::new();
    let mut wallet = data_dir.wallet(true, None);
    let wallet_dir = wallet.get_wallet_dir();
    let descriptors = wallet.get_descriptors();

    // reveal an address and flush it the way a broadcast does, then lose the transaction
    let txn = wallet.database().begin_transaction().unwrap();
    let address = wallet.get_new_address().unwrap();
    wallet.flush_bdk_pending().unwrap();
    drop(txn);
    assert!(wallet_dir.join(BDK_PENDING_FILE).exists());

    // nothing reached the DB
    let txn = wallet.database().begin_transaction().unwrap();
    assert!(
        txn.get_bdk_changeset()
            .unwrap()
            .indexer
            .last_revealed
            .is_empty()
    );
    txn.commit().unwrap();

    // reloading the BDK wallet (as a restart would) recovers the reveal from the file
    let txn = wallet.database().begin_transaction().unwrap();
    let reloaded = setup_bdk(
        &txn,
        &wallet_dir,
        descriptors.colored,
        descriptors.vanilla,
        false,
        BitcoinNetwork::Regtest,
    )
    .unwrap();
    txn.commit().unwrap();

    let index = reloaded
        .spk_index()
        .last_revealed_index(KeychainKind::External)
        .expect("the revealed index must have survived");
    assert_eq!(
        reloaded.peek_address(KeychainKind::External, index).address,
        address
    );
    // the file is dropped once its contents are durably in the DB
    assert!(!wallet_dir.join(BDK_PENDING_FILE).exists());
}

// Recovery of the pending file happens while the wallet is being built, not at some later commit:
// a fresh instance must see the recovered state immediately, before any operation of its own.
#[test]
#[parallel]
fn recovered_pending_is_visible_right_after_load() {
    let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet_keys = SinglesigKeys::from_keys(&keys, None);
    let data_dir = PrivateDataDir::new();

    // a wallet that revealed an address, flushed it as a broadcast does, then died
    let (wallet_dir, address) = {
        let mut wallet = data_dir.wallet_raw(&wallet_keys, None, BitcoinNetwork::Regtest);
        let txn = wallet.database().begin_transaction().unwrap();
        let address = wallet.get_new_address().unwrap();
        wallet.flush_bdk_pending().unwrap();
        drop(txn);
        (wallet.get_wallet_dir(), address)
    };
    assert!(wallet_dir.join(BDK_PENDING_FILE).exists());

    // restart: no commit of our own, just construction
    let wallet = data_dir.wallet_raw(&wallet_keys, None, BitcoinNetwork::Regtest);
    assert_eq!(wallet.get_wallet_dir(), wallet_dir);

    let index = wallet
        .bdk_wallet()
        .spk_index()
        .last_revealed_index(KeychainKind::External)
        .expect("the recovered index must be visible immediately after load");
    assert_eq!(
        wallet
            .bdk_wallet()
            .peek_address(KeychainKind::External, index)
            .address,
        address
    );
    // and it is already durable, so the file is gone
    assert!(!wallet_dir.join(BDK_PENDING_FILE).exists());
}

// A crash between the temporary write and the rename leaves the newest changes in the .tmp file
// only: they must still be recovered, and must not be silently truncated by the next flush.
#[test]
#[parallel]
fn pending_tmp_file_left_by_a_crash_is_recovered() {
    let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet_keys = SinglesigKeys::from_keys(&keys, None);
    let data_dir = PrivateDataDir::new();

    let (wallet_dir, address) = {
        let mut wallet = data_dir.wallet_raw(&wallet_keys, None, BitcoinNetwork::Regtest);
        let txn = wallet.database().begin_transaction().unwrap();
        let address = wallet.get_new_address().unwrap();
        wallet.flush_bdk_pending().unwrap();
        drop(txn);
        (wallet.get_wallet_dir(), address)
    };

    // simulate the crash: the write and fsync completed, the rename did not
    let pending = wallet_dir.join(BDK_PENDING_FILE);
    let tmp = atomic_tmp_path(&pending).unwrap();
    fs::rename(&pending, &tmp).unwrap();
    assert!(!pending.exists() && tmp.exists());

    // restart: the reveal is recovered from the temporary file alone
    let wallet = data_dir.wallet_raw(&wallet_keys, None, BitcoinNetwork::Regtest);
    assert_eq!(wallet.get_wallet_dir(), wallet_dir);
    let index = wallet
        .bdk_wallet()
        .spk_index()
        .last_revealed_index(KeychainKind::External)
        .expect("the index written to the temporary file must have survived");
    assert_eq!(
        wallet
            .bdk_wallet()
            .peek_address(KeychainKind::External, index)
            .address,
        address
    );
    assert!(!tmp.exists());
}
