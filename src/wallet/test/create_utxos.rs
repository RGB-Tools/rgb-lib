use super::*;

#[test]
fn success() {
    initialize();

    // up_to version with 0 allocatable UTXOs
    println!("\n=== up_to true, 0 allocatable");
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let num_utxos_created = wallet.create_utxos(online.clone(), true, None).unwrap();
    assert_eq!(num_utxos_created, UTXO_NUM);
    let unspents = wallet.list_unspents(false).unwrap();
    assert_eq!(unspents.len(), (UTXO_NUM + 1) as usize);

    // up_to version with allocatable UTXOs partially available (1 missing)
    println!("\n=== up_to true, need to create 1 more");
    let num_utxos_created = wallet
        .create_utxos(online.clone(), true, Some(UTXO_NUM + 1))
        .unwrap();
    assert_eq!(num_utxos_created, 1);
    let unspents = wallet.list_unspents(false).unwrap();
    assert_eq!(unspents.len(), (UTXO_NUM + 2) as usize);

    // forced version always creates UTXOs
    println!("\n=== up_to false");
    let num_utxos_created = wallet.create_utxos(online, false, None).unwrap();
    assert_eq!(num_utxos_created, UTXO_NUM);
    let unspents = wallet.list_unspents(false).unwrap();
    assert_eq!(unspents.len(), (UTXO_NUM * 2 + 2) as usize);
}

#[test]
fn fail() {
    initialize();

    // cannot create utxos for an empty wallet
    let (mut wallet, online) = get_empty_wallet!();
    let result = wallet.create_utxos(online, true, None);
    assert!(matches!(result, Err(Error::InsufficientFunds)));

    // don't create utxos if enough allocations are already available
    let (mut wallet, online) = get_funded_wallet!();
    let result = wallet.create_utxos(online, true, None);
    assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));
}
