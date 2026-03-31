use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let mut party = get_funded_party!();

    let before_timestamp = now().unix_timestamp();
    let bak_info_before = party.db_backup_info();
    let num_rights = 3;
    let asset = party.issue_asset_bfa(num_rights, FAKE_EVM_ADDRESS.to_string(), None);
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    // check the asset has been saved with the correct schema
    let bfa_asset_list = party.list_assets(&[AssetSchema::Bfa]).bfa.unwrap();
    assert!(
        bfa_asset_list
            .into_iter()
            .any(|a| a.asset_id == asset.asset_id)
    );

    // add a pending operation to an UTXO so spendable balance will be != settled / future
    let _receive_data = party.blind_receive();
    party.show_unspent_colorings("after issuance + blind receive");

    // checks
    let balance = party.get_asset_balance(&asset.asset_id);
    assert_eq!(asset.ticker, TICKER.to_string());
    assert_eq!(asset.name, NAME.to_string());
    assert_eq!(asset.details, None);
    assert_eq!(asset.precision, PRECISION);
    assert_eq!(asset.initial_supply, 0);
    assert_eq!(
        balance,
        Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        }
    );
    assert!(before_timestamp <= asset.added_at && asset.added_at <= now().unix_timestamp());
    let unspents = party.list_unspents(false);
    let unspents_fungible = unspents.iter().filter(|u| {
        u.rgb_allocations.iter().any(|a| {
            a.asset_id == Some(asset.asset_id.clone())
                && a.assignment == Assignment::Fungible(AMOUNT)
        })
    });
    assert_eq!(unspents_fungible.count(), 0);
    let unspents_mint_right = unspents.iter().filter(|u| {
        u.rgb_allocations.iter().any(|a| {
            a.asset_id == Some(asset.asset_id.clone()) && a.assignment == Assignment::MintRight
        })
    });
    assert_eq!(unspents_mint_right.count(), num_rights as usize);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn multi_success() {
    initialize();

    let mut party = get_funded_party!();

    // create more UTXOs
    party.create_utxos_default();

    let num_rights = 3;
    let asset = party.issue_asset_bfa(num_rights, FAKE_EVM_ADDRESS.to_string(), None);

    // check balance is zero
    assert_eq!(asset.balance.settled, 0);

    // check each allocation ends up on a different UTXO
    let unspents: Vec<Unspent> = party
        .list_unspents(true)
        .into_iter()
        .filter(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| matches!(a.assignment, Assignment::MintRight))
        })
        .collect();
    let mut outpoints: Vec<Outpoint> = vec![];
    for unspent in &unspents {
        let outpoint = unspent.utxo.outpoint.clone();
        assert!(!outpoints.contains(&outpoint));
        outpoints.push(outpoint);
    }
    assert_eq!(outpoints.len(), num_rights as usize);

    // check all allocations are of the same asset
    assert!(
        unspents
            .iter()
            .filter(|u| !u.rgb_allocations.is_empty())
            .all(|u| {
                u.rgb_allocations.len() == 1
                    && u.rgb_allocations.first().unwrap().asset_id == Some(asset.asset_id.clone())
            })
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    let mut party = get_funded_party!();

    // invalid ticker: empty
    let result = party.wlt().issue_asset_bfa(
        s!(""),
        NAME.to_string(),
        PRECISION,
        1,
        ANVIL_CHAIN_ID,
        FAKE_EVM_ADDRESS.to_string(),
        None,
    );
    assert!(matches!(result, Err(Error::InvalidTicker { details: m }) if m == EMPTY_MSG));

    // invalid ticker: too long
    let result = party.wlt().issue_asset_bfa(
        s!("ABCDEFGHI"),
        NAME.to_string(),
        PRECISION,
        1,
        ANVIL_CHAIN_ID,
        FAKE_EVM_ADDRESS.to_string(),
        None,
    );
    assert!(matches!(result, Err(Error::InvalidTicker { details: m }) if m == IDENT_TOO_LONG_MSG));

    // invalid ticker: lowercase
    let result = party.wlt().issue_asset_bfa(
        s!("TiCkEr"),
        NAME.to_string(),
        PRECISION,
        1,
        ANVIL_CHAIN_ID,
        FAKE_EVM_ADDRESS.to_string(),
        None,
    );
    assert!(
        matches!(result, Err(Error::InvalidTicker { details: m }) if m == "ticker needs to be all uppercase")
    );

    // invalid ticker: with space
    let invalid_ticker = "TICKER WITH SPACE";
    let result = party.wlt().issue_asset_bfa(
        invalid_ticker.to_string(),
        NAME.to_string(),
        PRECISION,
        1,
        ANVIL_CHAIN_ID,
        FAKE_EVM_ADDRESS.to_string(),
        None,
    );
    assert!(
        matches!(result, Err(Error::InvalidTicker { details: m }) if m == IDENT_NOT_ASCII_MSG
            .replace("{0}", invalid_ticker).replace("{1}", " ").replace("{2}", "6"))
    );

    // invalid ticker: unicode characters
    let invalid_ticker = "TICKERWITH℧NICODE";
    let result = party.wlt().issue_asset_bfa(
        invalid_ticker.to_string(),
        NAME.to_string(),
        PRECISION,
        1,
        ANVIL_CHAIN_ID,
        FAKE_EVM_ADDRESS.to_string(),
        None,
    );
    assert!(
        matches!(result, Err(Error::InvalidTicker { details: m }) if m == IDENT_NOT_ASCII_MSG
            .replace("{0}", invalid_ticker).replace("{1}", "℧").replace("{2}", "10"))
    );

    // invalid ticker: starting with a number
    let invalid_ticker = "1TICKER";
    let result = party.wlt().issue_asset_bfa(
        invalid_ticker.to_string(),
        NAME.to_string(),
        PRECISION,
        1,
        ANVIL_CHAIN_ID,
        FAKE_EVM_ADDRESS.to_string(),
        None,
    );
    assert!(
        matches!(result, Err(Error::InvalidTicker { details: m }) if m == IDENT_NOT_START_MSG
            .replace("{0}", invalid_ticker).replace("{1}", "1"))
    );

    // invalid name: empty
    let result = party.wlt().issue_asset_bfa(
        TICKER.to_string(),
        s!(""),
        PRECISION,
        1,
        ANVIL_CHAIN_ID,
        FAKE_EVM_ADDRESS.to_string(),
        None,
    );
    assert!(matches!(result, Err(Error::InvalidName { details: m }) if m == EMPTY_MSG));

    // invalid name: too long
    let result = party.wlt().issue_asset_bfa(
        TICKER.to_string(),
        ("a").repeat(257),
        PRECISION,
        1,
        ANVIL_CHAIN_ID,
        FAKE_EVM_ADDRESS.to_string(),
        None,
    );
    assert!(matches!(result, Err(Error::InvalidName { details: m }) if m == IDENT_TOO_LONG_MSG));

    // invalid name: unicode characters
    let invalid_name = "name with ℧nicode characters";
    let result = party.wlt().issue_asset_bfa(
        TICKER.to_string(),
        invalid_name.to_string(),
        PRECISION,
        1,
        ANVIL_CHAIN_ID,
        FAKE_EVM_ADDRESS.to_string(),
        None,
    );
    assert!(
        matches!(result, Err(Error::InvalidName { details: m }) if m == IDENT_NOT_ASCII_MSG
            .replace("{0}", invalid_name).replace("{1}", "℧").replace("{2}", "10"))
    );

    // invalid precision
    let result = party.wlt().issue_asset_bfa(
        TICKER.to_string(),
        NAME.to_string(),
        19,
        1,
        ANVIL_CHAIN_ID,
        FAKE_EVM_ADDRESS.to_string(),
        None,
    );
    assert!(matches!(
        result,
        Err(Error::InvalidPrecision { details: m }) if m == "precision is too high"
    ));

    // invalid contract address: no 0x prefix
    let result = party.issue_asset_bfa_result(1, FAKE_EVM_ADDRESS[2..].to_string(), None);
    assert!(matches!(
        result,
        Err(Error::InvalidEvmContractAddress { details: m }) if m == "address must start with 0x"
    ));

    // invalid contract address: too short
    let result = party.issue_asset_bfa_result(1, s!("0x5FbDB23"), None);
    assert!(matches!(
        result,
        Err(Error::InvalidEvmContractAddress { details: m }) if m == "address must be 20 hex-encoded bytes"
    ));

    // invalid contract address: non-hex characters
    let result = party.issue_asset_bfa_result(1, format!("0x{}", "Z".repeat(40)), None);
    assert!(matches!(
        result,
        Err(Error::InvalidEvmContractAddress { details: m }) if m == "address must be 20 hex-encoded bytes"
    ));

    // invalid reject list contract address
    let result =
        party.issue_asset_bfa_result(1, FAKE_EVM_ADDRESS.to_string(), Some(s!("0x5FbDB23")));
    assert!(matches!(
        result,
        Err(Error::InvalidEvmContractAddress { details: m }) if m == "address must be 20 hex-encoded bytes"
    ));

    // no rights
    let result = party.issue_asset_bfa_result(0, FAKE_EVM_ADDRESS.to_string(), None);
    assert!(matches!(result, Err(Error::NoMintRights)));

    // new wallet
    let mut party = get_empty_party!();

    // insufficient funds
    let result = party.issue_asset_bfa_result(1, FAKE_EVM_ADDRESS.to_string(), None);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    fund_wallet(party.get_address());
    mine(false);

    // insufficient allocations
    let result = party.issue_asset_bfa_result(1, FAKE_EVM_ADDRESS.to_string(), None);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
}
