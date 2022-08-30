use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let num_utxos_created = wallet.create_utxos(online).unwrap();
    assert_eq!(num_utxos_created, UTXO_NUM as u64);
}

#[test]
fn fail() {
    initialize();

    // cannot create utxos for an empty wallet
    let (mut wallet, online) = get_empty_wallet!();
    let result = wallet.create_utxos(online);
    assert!(matches!(result, Err(Error::InsufficientFunds)));

    // cannot create utxos if allocations are already available
    let (mut wallet, online) = get_funded_wallet!();
    let result = wallet.create_utxos(online);
    assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable())));
}
