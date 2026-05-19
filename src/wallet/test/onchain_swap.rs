use std::str::FromStr;

use bdk_wallet::bitcoin::{OutPoint as BdkOutPoint, psbt::Psbt};

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
fn maker_taker_rgb_for_btc_balances() {
    initialize();

    let (mut maker, maker_online) = get_funded_noutxo_wallet!();
    let (mut taker, taker_online) = get_funded_noutxo_wallet!();

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

    let (mut maker, maker_online) = get_funded_noutxo_wallet!();
    let (mut taker, taker_online) = get_funded_noutxo_wallet!();

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
#[ignore = "rgb-lib currently finalizes the RGB commitment after the first coloring; RGB->RGB needs delayed/merged commit support"]
fn maker_taker_rgb_for_rgb_balances() {
    initialize();

    let (mut maker, maker_online) = get_funded_noutxo_wallet!();
    let (mut taker, taker_online) = get_funded_noutxo_wallet!();

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

fn issue_swap_asset(
    wallet: &mut Wallet,
    online: Online,
    ticker: &str,
    name: &str,
    amount: u64,
) -> String {
    test_create_utxos(wallet, online, false, Some(1), Some(10_000), FEE_RATE, None);
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
) {
    let maker_receives_rgb = matches!(maker_receives.kind, OnchainSwapLegKind::Rgb);
    let taker_receives_rgb = matches!(maker_gives.kind, OnchainSwapLegKind::Rgb);

    let offer = maker
        .create_swap_offer(maker_gives, maker_receives, SWAP_FEE, None, None)
        .unwrap();
    let request = taker
        .accept_swap_offer(taker_online, offer, 0, false)
        .unwrap();
    let proposal = maker
        .accept_swap_request(maker_online, request, 0, false)
        .unwrap();
    let completion = taker
        .complete_swap_proposal(taker_online, proposal, 0, false)
        .unwrap();

    let maker_psbt =
        strip_signatures_for_inputs(&completion.psbt, &completion.proposal.maker_inputs);
    let maker_signed_psbt = maker.sign_psbt(maker_psbt, None).unwrap();
    let finalized_psbt = maker.finalize_psbt(maker_signed_psbt, None).unwrap();
    let finalized_psbt = Psbt::from_str(&finalized_psbt).unwrap();
    maker.broadcast_psbt(&finalized_psbt).unwrap();
    mine(false);

    if taker_receives_rgb {
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
        let receive_result = maker
            .accept_swap_transfers(maker_online, completion, OnchainSwapRole::Maker, false)
            .unwrap();
        assert!(!receive_result.assignments.is_empty());
    }
}

fn strip_signatures_for_inputs(psbt: &str, inputs: &[OnchainSwapInput]) -> String {
    let maker_outpoints = inputs
        .iter()
        .map(|input| {
            BdkOutPoint::from_str(&input.outpoint.to_string()).expect("valid swap input outpoint")
        })
        .collect::<Vec<_>>();
    let mut psbt = Psbt::from_str(psbt).unwrap();
    for (txin, input) in psbt.unsigned_tx.input.iter().zip(psbt.inputs.iter_mut()) {
        if maker_outpoints.contains(&txin.previous_output) {
            input.partial_sigs.clear();
            input.tap_key_sig = None;
            input.tap_script_sigs.clear();
            input.final_script_sig = None;
            input.final_script_witness = None;
        }
    }
    psbt.to_string()
}

fn total_btc(wallet: &mut Wallet, online: Online) -> u64 {
    let balance = test_get_btc_balance(wallet, online);
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
