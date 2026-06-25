use std::str::FromStr;

use bdk_wallet::bitcoin::psbt::Psbt;

use super::*;

const SWAP_RGB_AMOUNT: u64 = 1_000;
const SWAP_BTC_PRICE: u64 = 100_000;
const SWAP_FEE: u64 = 2_000;

#[derive(Debug, Clone, Copy)]
struct ExpectedBtcDelta {
    maker: i64,
    taker: i64,
}

#[derive(Debug, Clone, Copy)]
struct ExpectedAssetBalance<'a> {
    asset_id: &'a str,
    maker: u64,
    taker: u64,
}

fn btc(amount: u64) -> OnchainSwapLeg {
    OnchainSwapLeg {
        kind: OnchainSwapLegKind::Btc,
        asset_id: None,
        amount,
    }
}

fn rgb(asset_id: &str, amount: u64) -> OnchainSwapLeg {
    OnchainSwapLeg {
        kind: OnchainSwapLegKind::Rgb,
        asset_id: Some(asset_id.to_string()),
        amount,
    }
}

#[test]
#[parallel]
fn swap_offer_requires_proxy_url() {
    initialize();

    let (mut maker, maker_online) = get_funded_noutxo_wallet(true, None);
    let asset_id = issue_swap_asset(
        &mut maker,
        maker_online,
        "SNOP",
        "Swap No Proxy",
        SWAP_RGB_AMOUNT,
    );

    let result = maker.create_swap_offer(
        rgb(&asset_id, SWAP_RGB_AMOUNT),
        btc(SWAP_BTC_PRICE),
        SWAP_FEE,
        None,
        None,
    );

    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoints { .. })
    ));
}

#[test]
#[parallel]
fn maker_taker_rgb_for_btc_balances() {
    initialize();

    let (mut maker, maker_online) = get_funded_noutxo_wallet(true, None);
    let (mut taker, taker_online) = get_funded_noutxo_wallet(true, None);

    let asset_id = issue_swap_asset(
        &mut maker,
        maker_online,
        "SRGB",
        "Swap RGB For BTC",
        SWAP_RGB_AMOUNT,
    );
    let maker_btc_before = total_btc(&mut maker, maker_online);
    let taker_btc_before = total_btc(&mut taker, taker_online);

    execute_swap(
        &mut maker,
        maker_online,
        &mut taker,
        taker_online,
        rgb(&asset_id, SWAP_RGB_AMOUNT),
        btc(SWAP_BTC_PRICE),
        PROXY_URL,
    );

    assert_btc_delta(
        &mut maker,
        maker_online,
        maker_btc_before,
        &mut taker,
        taker_online,
        taker_btc_before,
        ExpectedBtcDelta {
            maker: SWAP_BTC_PRICE as i64,
            taker: -((SWAP_BTC_PRICE + SWAP_FEE) as i64),
        },
    );
    assert_asset_balances(
        &maker,
        &taker,
        &[ExpectedAssetBalance {
            asset_id: &asset_id,
            maker: 0,
            taker: SWAP_RGB_AMOUNT,
        }],
    );
}

#[test]
#[parallel]
fn maker_taker_btc_for_rgb_balances() {
    initialize();

    let (mut maker, maker_online) = get_funded_noutxo_wallet(true, None);
    let (mut taker, taker_online) = get_funded_noutxo_wallet(true, None);

    let asset_id = issue_swap_asset(
        &mut taker,
        taker_online,
        "SBTC",
        "Swap BTC For RGB",
        SWAP_RGB_AMOUNT,
    );
    let maker_btc_before = total_btc(&mut maker, maker_online);
    let taker_btc_before = total_btc(&mut taker, taker_online);

    execute_swap(
        &mut maker,
        maker_online,
        &mut taker,
        taker_online,
        btc(SWAP_BTC_PRICE),
        rgb(&asset_id, SWAP_RGB_AMOUNT),
        PROXY_URL,
    );

    assert_btc_delta(
        &mut maker,
        maker_online,
        maker_btc_before,
        &mut taker,
        taker_online,
        taker_btc_before,
        ExpectedBtcDelta {
            maker: -(SWAP_BTC_PRICE as i64),
            taker: (SWAP_BTC_PRICE as i64) - (SWAP_FEE as i64),
        },
    );
    assert_asset_balances(
        &maker,
        &taker,
        &[ExpectedAssetBalance {
            asset_id: &asset_id,
            maker: SWAP_RGB_AMOUNT,
            taker: 0,
        }],
    );
}

#[test]
#[parallel]
fn maker_taker_rgb_for_rgb_balances() {
    initialize();

    let (mut maker, maker_online) = get_funded_noutxo_wallet(true, None);
    let (mut taker, taker_online) = get_funded_noutxo_wallet(true, None);

    let maker_asset_id = issue_swap_asset(
        &mut maker,
        maker_online,
        "SRGA",
        "Maker RGB Swap Asset",
        SWAP_RGB_AMOUNT,
    );
    let taker_asset_id = issue_swap_asset(
        &mut taker,
        taker_online,
        "SRGB",
        "Taker RGB Swap Asset",
        SWAP_RGB_AMOUNT,
    );
    let maker_btc_before = total_btc(&mut maker, maker_online);
    let taker_btc_before = total_btc(&mut taker, taker_online);

    execute_swap(
        &mut maker,
        maker_online,
        &mut taker,
        taker_online,
        rgb(&maker_asset_id, SWAP_RGB_AMOUNT),
        rgb(&taker_asset_id, SWAP_RGB_AMOUNT),
        PROXY_URL,
    );

    assert_btc_delta(
        &mut maker,
        maker_online,
        maker_btc_before,
        &mut taker,
        taker_online,
        taker_btc_before,
        ExpectedBtcDelta {
            maker: 0,
            taker: -(SWAP_FEE as i64),
        },
    );
    assert_asset_balances(
        &maker,
        &taker,
        &[
            ExpectedAssetBalance {
                asset_id: &maker_asset_id,
                maker: 0,
                taker: SWAP_RGB_AMOUNT,
            },
            ExpectedAssetBalance {
                asset_id: &taker_asset_id,
                maker: SWAP_RGB_AMOUNT,
                taker: 0,
            },
        ],
    );
}

#[test]
#[parallel]
fn maker_rejects_request_with_mutated_offer() {
    initialize();

    let (mut maker, maker_online) = get_funded_noutxo_wallet(true, None);
    let (mut taker, taker_online) = get_funded_noutxo_wallet(true, None);
    let asset_id = issue_swap_asset(
        &mut maker,
        maker_online,
        "SMOF",
        "Swap Mutated Offer",
        SWAP_RGB_AMOUNT,
    );

    let offer = maker
        .create_swap_offer(
            rgb(&asset_id, SWAP_RGB_AMOUNT),
            btc(SWAP_BTC_PRICE),
            SWAP_FEE,
            None,
            Some(PROXY_URL.to_string()),
        )
        .unwrap();
    let mut request = taker
        .accept_swap_offer(taker_online, offer, 0, false)
        .unwrap();
    request.offer.network_fee_sat += 1;

    assert!(
        maker
            .accept_swap_request(maker_online, request, 0, false)
            .is_err()
    );
}

#[test]
#[parallel]
fn taker_rejects_proposal_with_mutated_request() {
    initialize();

    let (mut maker, maker_online) = get_funded_noutxo_wallet(true, None);
    let (mut taker, taker_online) = get_funded_noutxo_wallet(true, None);
    let asset_id = issue_swap_asset(
        &mut maker,
        maker_online,
        "SMRQ",
        "Swap Mutated Request",
        SWAP_RGB_AMOUNT,
    );

    let offer = maker
        .create_swap_offer(
            rgb(&asset_id, SWAP_RGB_AMOUNT),
            btc(SWAP_BTC_PRICE),
            SWAP_FEE,
            None,
            Some(PROXY_URL.to_string()),
        )
        .unwrap();
    let request = taker
        .accept_swap_offer(taker_online, offer, 0, false)
        .unwrap();
    let mut proposal = maker
        .accept_swap_request(maker_online, request, 0, false)
        .unwrap();
    proposal.request.taker_change_script_pubkey_hex = "00".to_string();

    assert!(
        taker
            .complete_swap_proposal(taker_online, proposal, 0, false)
            .is_err()
    );
}

#[test]
#[parallel]
fn maker_rejects_tampered_rgb_consignment_before_signing_btc() {
    initialize();

    let (mut maker, maker_online) = get_funded_noutxo_wallet(true, None);
    let (mut taker, taker_online) = get_funded_noutxo_wallet(true, None);
    let asset_id = issue_swap_asset(
        &mut taker,
        taker_online,
        "SMCS",
        "Swap Mutated Consignment",
        SWAP_RGB_AMOUNT,
    );

    let offer = maker
        .create_swap_offer(
            btc(SWAP_BTC_PRICE),
            rgb(&asset_id, SWAP_RGB_AMOUNT),
            SWAP_FEE,
            None,
            Some(PROXY_URL.to_string()),
        )
        .unwrap();
    let request = taker
        .accept_swap_offer(taker_online, offer, 0, false)
        .unwrap();
    let proposal = maker
        .accept_swap_request(maker_online, request, 0, false)
        .unwrap();
    assert_eq!(signature_count(&proposal.psbt), 0);

    let completion = taker
        .complete_swap_proposal(taker_online, proposal, 0, false)
        .unwrap();
    let mut tampered_completion = completion.clone();
    tampered_completion.consignments[0].blinding += 1;
    assert!(
        maker
            .process_swap_completion(maker_online, tampered_completion)
            .is_err()
    );

    let completion = maker
        .process_swap_completion(maker_online, completion)
        .unwrap();
    assert!(completion.finalized_psbt.is_some());
    assert!(signature_count(&completion.psbt) > 0);
}

fn issue_swap_asset(
    wallet: &mut Wallet,
    online: Online,
    ticker: &str,
    name: &str,
    amount: u64,
) -> String {
    wallet
        .create_utxos(online, false, Some(1), Some(10_000), FEE_RATE, false)
        .unwrap();
    let asset = wallet
        .issue_asset_nia(ticker.to_string(), name.to_string(), 0, vec![amount])
        .unwrap();
    assert_eq!(asset_amount(wallet, &asset.asset_id), amount);
    asset.asset_id
}

fn execute_swap(
    maker: &mut Wallet,
    maker_online: Online,
    taker: &mut Wallet,
    taker_online: Online,
    maker_gives: OnchainSwapLeg,
    maker_receives: OnchainSwapLeg,
    proxy_url: &str,
) {
    let maker_receives_rgb = matches!(maker_receives.kind, OnchainSwapLegKind::Rgb);
    let taker_receives_rgb = matches!(maker_gives.kind, OnchainSwapLegKind::Rgb);
    let maker_gives_btc = matches!(maker_gives.kind, OnchainSwapLegKind::Btc);
    let rgb_for_rgb = maker_receives_rgb && taker_receives_rgb;
    let maker_gives_asset_id = maker_gives.asset_id.clone();
    let maker_receives_asset_id = maker_receives.asset_id.clone();

    let offer = maker
        .create_swap_offer(
            maker_gives,
            maker_receives,
            SWAP_FEE,
            None,
            Some(proxy_url.to_string()),
        )
        .unwrap();
    let request = taker
        .accept_swap_offer(taker_online, offer, 0, false)
        .unwrap();
    let proposal = maker
        .accept_swap_request(maker_online, request, 0, false)
        .unwrap();
    if rgb_for_rgb || maker_gives_btc {
        assert_eq!(signature_count(&proposal.psbt), 0);
    } else {
        assert!(signature_count(&proposal.psbt) > 0);
    }
    assert_consignment_transport(&proposal.consignments);
    if taker_receives_rgb {
        assert!(
            proposal.maker_history.is_some(),
            "maker RGB history should be exposed as a high-level history ref"
        );
        assert_asset_history_transport(proposal.maker_history.as_ref());
    }
    let completion = taker
        .complete_swap_proposal(taker_online, proposal, 0, false)
        .unwrap();
    if rgb_for_rgb {
        assert!(
            completion.taker_history.is_some(),
            "taker RGB history should be exposed as a high-level history ref"
        );
        assert_asset_history_transport(completion.taker_history.as_ref());
        let mut missing_history_endpoint = completion.clone();
        missing_history_endpoint
            .taker_history
            .as_mut()
            .unwrap()
            .endpoint = None;
        assert!(
            maker
                .process_swap_completion(maker_online, missing_history_endpoint)
                .is_err()
        );
        let mut tampered_completion = completion.clone();
        tampered_completion.txid =
            "0000000000000000000000000000000000000000000000000000000000000000".to_string();
        assert!(
            maker
                .process_swap_completion(maker_online, tampered_completion)
                .is_err()
        );
    }
    // Maker resumes the swap on their side: for RGB-for-RGB this consumes the fascia and emits
    // the maker's consignment; for the single-RGB cases this is a no-op.
    let completion = maker
        .process_swap_completion(maker_online, completion)
        .unwrap();
    if rgb_for_rgb || maker_gives_btc {
        assert!(completion.finalized_psbt.is_some());
        assert!(signature_count(&completion.psbt) > 0);
    }
    assert_consignment_transport(&completion.consignments);

    let broadcast_txid = maker
        .broadcast_swap_completion(maker_online, completion.clone())
        .unwrap();
    assert_eq!(broadcast_txid, completion.txid);
    mine(false);

    if taker_receives_rgb {
        let mut bad_endpoint = completion.clone();
        bad_endpoint
            .consignments
            .iter_mut()
            .filter(|c| Some(c.asset_id.as_str()) == maker_gives_asset_id.as_deref())
            .for_each(|c| c.endpoint = Some(format!("rpc://{PROXY_HOST_MOD_PROTO}")));
        assert!(
            taker
                .accept_swap_transfers(taker_online, bad_endpoint, OnchainSwapRole::Taker, false)
                .is_err()
        );

        let mut missing_endpoint = completion.clone();
        missing_endpoint
            .consignments
            .iter_mut()
            .filter(|c| Some(c.asset_id.as_str()) == maker_gives_asset_id.as_deref())
            .for_each(|c| c.endpoint = None);
        assert!(
            taker
                .accept_swap_transfers(
                    taker_online,
                    missing_endpoint,
                    OnchainSwapRole::Taker,
                    false,
                )
                .is_err()
        );

        let mut wrong_recipient = completion.clone();
        let wrong_id = alternate_swap_recipient_id(taker);
        wrong_recipient
            .consignments
            .iter_mut()
            .filter(|c| Some(c.asset_id.as_str()) == maker_gives_asset_id.as_deref())
            .for_each(|c| c.recipient_id = wrong_id.clone());
        assert!(
            taker
                .accept_swap_transfers(taker_online, wrong_recipient, OnchainSwapRole::Taker, false)
                .is_err()
        );

        let mut missing_consignment = completion.clone();
        missing_consignment
            .consignments
            .retain(|c| Some(c.asset_id.as_str()) != maker_gives_asset_id.as_deref());
        assert!(
            taker
                .accept_swap_transfers(
                    taker_online,
                    missing_consignment,
                    OnchainSwapRole::Taker,
                    false,
                )
                .is_err()
        );
        let receive_result = taker
            .accept_swap_transfers(
                taker_online,
                completion.clone(),
                OnchainSwapRole::Taker,
                false,
            )
            .unwrap();
        assert!(!receive_result.assignments.is_empty());
    }
    if maker_receives_rgb {
        let mut bad_endpoint = completion.clone();
        bad_endpoint
            .consignments
            .iter_mut()
            .filter(|c| Some(c.asset_id.as_str()) == maker_receives_asset_id.as_deref())
            .for_each(|c| c.endpoint = Some(format!("rpc://{PROXY_HOST_MOD_PROTO}")));
        assert!(
            maker
                .accept_swap_transfers(maker_online, bad_endpoint, OnchainSwapRole::Maker, false)
                .is_err()
        );

        let mut missing_endpoint = completion.clone();
        missing_endpoint
            .consignments
            .iter_mut()
            .filter(|c| Some(c.asset_id.as_str()) == maker_receives_asset_id.as_deref())
            .for_each(|c| c.endpoint = None);
        assert!(
            maker
                .accept_swap_transfers(
                    maker_online,
                    missing_endpoint,
                    OnchainSwapRole::Maker,
                    false,
                )
                .is_err()
        );

        let mut wrong_recipient = completion.clone();
        let wrong_id = alternate_swap_recipient_id(maker);
        wrong_recipient
            .consignments
            .iter_mut()
            .filter(|c| Some(c.asset_id.as_str()) == maker_receives_asset_id.as_deref())
            .for_each(|c| c.recipient_id = wrong_id.clone());
        assert!(
            maker
                .accept_swap_transfers(maker_online, wrong_recipient, OnchainSwapRole::Maker, false)
                .is_err()
        );

        let mut missing_consignment = completion.clone();
        missing_consignment
            .consignments
            .retain(|c| Some(c.asset_id.as_str()) != maker_receives_asset_id.as_deref());
        assert!(
            maker
                .accept_swap_transfers(
                    maker_online,
                    missing_consignment,
                    OnchainSwapRole::Maker,
                    false,
                )
                .is_err()
        );
        let receive_result = maker
            .accept_swap_transfers(maker_online, completion, OnchainSwapRole::Maker, false)
            .unwrap();
        assert!(!receive_result.assignments.is_empty());
    }
}

fn alternate_swap_recipient_id(wallet: &mut Wallet) -> String {
    let address = wallet.get_address().unwrap();
    let script = wallet.get_script_pubkey(&address).unwrap();
    recipient_id_from_script_buf(script, BitcoinNetwork::Regtest)
}

fn assert_consignment_transport(consignments: &[OnchainSwapConsignment]) {
    if consignments.is_empty() {
        return;
    }
    for consignment in consignments {
        assert_eq!(
            consignment.endpoint.as_deref(),
            Some(PROXY_ENDPOINT.as_str())
        );
        assert!(
            consignment.path.is_empty(),
            "proxy-mode consignments must not expose sender-local paths"
        );
    }
}

fn assert_asset_history_transport(history: Option<&OnchainSwapAssetHistory>) {
    let Some(history) = history else {
        return;
    };
    assert_eq!(history.endpoint.as_deref(), Some(PROXY_ENDPOINT.as_str()));
    assert!(
        history.path.is_empty(),
        "proxy-mode asset histories must not expose sender-local paths"
    );
}

fn signature_count(psbt: &str) -> usize {
    Psbt::from_str(psbt)
        .unwrap()
        .inputs
        .iter()
        .map(|input| {
            input.partial_sigs.len()
                + input.tap_script_sigs.len()
                + usize::from(input.tap_key_sig.is_some())
                + usize::from(input.final_script_sig.is_some())
                + usize::from(input.final_script_witness.is_some())
        })
        .sum()
}

fn total_btc(wallet: &mut Wallet, online: Online) -> u64 {
    let balance = wallet.get_btc_balance(Some(online), false).unwrap();
    balance.vanilla.future + balance.colored.future
}

fn asset_amount(wallet: &Wallet, asset_id: &str) -> u64 {
    wallet
        .get_asset_balance(asset_id.to_string())
        .map(|balance| balance.future)
        .unwrap_or(0)
}

fn assert_btc_delta(
    maker: &mut Wallet,
    maker_online: Online,
    maker_before: u64,
    taker: &mut Wallet,
    taker_online: Online,
    taker_before: u64,
    expected: ExpectedBtcDelta,
) {
    let maker_after = total_btc(maker, maker_online);
    let taker_after = total_btc(taker, taker_online);
    assert_eq!(
        maker_after as i64 - maker_before as i64,
        expected.maker,
        "maker BTC delta mismatch"
    );
    assert_eq!(
        taker_after as i64 - taker_before as i64,
        expected.taker,
        "taker BTC delta mismatch"
    );
    assert_eq!(
        maker_after + taker_after + SWAP_FEE,
        maker_before + taker_before,
        "total BTC should only decrease by the swap fee"
    );
}

fn assert_asset_balances(
    maker: &Wallet,
    taker: &Wallet,
    expected_balances: &[ExpectedAssetBalance<'_>],
) {
    for expected in expected_balances {
        let maker_amount = asset_amount(maker, expected.asset_id);
        let taker_amount = asset_amount(taker, expected.asset_id);
        assert_eq!(
            maker_amount, expected.maker,
            "maker asset balance mismatch for {}",
            expected.asset_id
        );
        assert_eq!(
            taker_amount, expected.taker,
            "taker asset balance mismatch for {}",
            expected.asset_id
        );
        assert_eq!(
            maker_amount + taker_amount,
            SWAP_RGB_AMOUNT,
            "asset amount should be conserved for {}",
            expected.asset_id
        );
    }
}
