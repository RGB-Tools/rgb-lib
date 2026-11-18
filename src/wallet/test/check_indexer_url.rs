use super::*;

#[cfg(feature = "bitcoind")]
#[test]
#[parallel]
fn success_bitcoind() {
    initialize();

    let auth = bitcoind_pass_auth();
    let result = check_indexer_url(BITCOIND_URL, Some(&auth), BitcoinNetwork::Regtest);
    assert_matches!(result, Ok(IndexerProtocol::Bitcoind));

    // cookie file auth
    let auth = bitcoind_cookie_auth();
    let result = check_indexer_url(
        BITCOIND_NO_FILTERS_URL,
        Some(&auth),
        BitcoinNetwork::Regtest,
    );
    assert_matches!(result, Ok(IndexerProtocol::Bitcoind));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success_electrum() {
    initialize();

    let result = check_indexer_url(ELECTRUM_URL, None, BitcoinNetwork::Regtest);
    assert_matches!(result, Ok(IndexerProtocol::Electrum));

    let result = check_indexer_url(ELECTRUM_2_URL, None, BitcoinNetwork::Regtest);
    assert_matches!(result, Ok(IndexerProtocol::Electrum));
}

#[cfg(feature = "esplora")]
#[test]
#[parallel]
fn success_esplora() {
    initialize();

    let result = check_indexer_url(ESPLORA_URL, None, BitcoinNetwork::Regtest);
    assert_matches!(result, Ok(IndexerProtocol::Esplora));
}

#[cfg(feature = "bitcoind")]
#[test]
#[parallel]
fn fail_bitcoind() {
    initialize();

    // bitcoind URL without credentials is not an electrum nor esplora server
    let result = check_indexer_url(BITCOIND_URL, None, BitcoinNetwork::Regtest);
    let invalid_indexer = s!("not a valid electrum nor esplora server");
    assert_matches!(result, Err(Error::InvalidIndexer { details: m }) if m == invalid_indexer);

    // wrong credentials
    let auth = BitcoindAuth::UserPass {
        username: BITCOIND_USER.to_string(),
        password: s!("wrong"),
    };
    let result = check_indexer_url(BITCOIND_URL, Some(&auth), BitcoinNetwork::Regtest);
    assert_matches!(result, Err(Error::InvalidIndexer { details: m }) if m.contains("cannot connect"));

    // credentials with a non-bitcoind URL
    let auth = bitcoind_pass_auth();
    let result = check_indexer_url(PROXY_URL, Some(&auth), BitcoinNetwork::Regtest);
    assert_matches!(result, Err(Error::InvalidIndexer { details: m }) if m.contains("cannot connect"));

    // node without -txindex
    let result = check_indexer_url(
        BITCOIND_NO_TXINDEX_URL,
        Some(&auth),
        BitcoinNetwork::Regtest,
    );
    let details = "bitcoind has no synced transaction index (-txindex)";
    assert_matches!(result, Err(Error::InvalidIndexer { details: m }) if m == details);

    // wrong network
    let result = check_indexer_url(BITCOIND_URL, Some(&auth), BitcoinNetwork::Testnet);
    let details = "resolver is for another chain-network pair";
    assert_matches!(result, Err(Error::InvalidIndexer { details: m }) if m == details);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail_electrum() {
    initialize();

    // verbose transactions are unsupported
    let result = check_indexer_url(ELECTRUM_BLOCKSTREAM_URL, None, BitcoinNetwork::Regtest);
    let verbose_unsupported =
        "verbose transactions are unsupported by the provided electrum service";
    assert_matches!(result, Err(Error::InvalidIndexer { details: m }) if m.contains(verbose_unsupported));

    // wrong network
    let result = check_indexer_url(ELECTRUM_URL, None, BitcoinNetwork::Testnet);
    let details = "resolver is for another chain-network pair";
    assert_matches!(result, Err(Error::InvalidIndexer { details: m }) if m == details);
}

#[cfg(feature = "esplora")]
#[test]
#[parallel]
fn fail_esplora() {
    initialize();

    // wrong network
    let result = check_indexer_url(ESPLORA_URL, None, BitcoinNetwork::Testnet);
    let details = "resolver is for another chain-network pair";
    assert_matches!(result, Err(Error::InvalidIndexer { details: m }) if m == details);
}

#[test]
#[parallel]
fn fail_detection() {
    initialize();

    // a URL speaking no supported protocol: the electrum probe fails and, when enabled, the
    // esplora fallback then fails to answer
    let result = check_indexer_url(PROXY_URL, None, BitcoinNetwork::Regtest);
    let invalid_indexer = s!("not a valid electrum nor esplora server");
    assert_matches!(result, Err(Error::InvalidIndexer { details: m }) if m == invalid_indexer);
}
