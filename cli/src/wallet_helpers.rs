use rgb_lib::Wallet;

pub fn assure_utxos_synced(wallet: &mut Wallet, electrum_url: String) {
    let online = wallet.go_online(false, electrum_url).unwrap();
    match wallet.create_utxos(online, true, None, None, 10.0001) {
        Err(rgb_lib::Error::AllocationsAlreadyAvailable) => (),
        Ok(_num) => (),
        Err(x) => panic!("{}", x.to_string()),
    }
}
