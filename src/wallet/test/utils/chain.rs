use super::*;

const MINE_GRACE_SECS: f32 = 1.0;

static MINER: Lazy<RwLock<Miner>> = Lazy::new(|| {
    RwLock::new(Miner {
        no_mine_count: 0,
        pending_mine_bitcoind: vec![],
        pending_mine_esplora: vec![],
        mines_satisfied_bitcoind: 0,
        mines_satisfied_esplora: 0,
    })
});

#[derive(Debug, Clone)]
pub(crate) struct Miner {
    no_mine_count: u32,
    pending_mine_bitcoind: Vec<OffsetDateTime>,
    pending_mine_esplora: Vec<OffsetDateTime>,
    mines_satisfied_bitcoind: u64,
    mines_satisfied_esplora: u64,
}

pub(crate) struct MinerStopGuard;

impl Drop for MinerStopGuard {
    fn drop(&mut self) {
        let mut miner = MINER.write().unwrap();
        miner.resume_mining();
        if miner.no_mine_count == 0 {
            if !miner.pending_mine_bitcoind.is_empty() {
                println!(
                    "MinerStopGuard drop: mining for {} pending bitcoind requests",
                    miner.pending_mine_bitcoind.len()
                );
                miner.mine(false, 1);
                miner.mines_satisfied_bitcoind += 1;
                miner.pending_mine_bitcoind.clear();
            }
            if !miner.pending_mine_esplora.is_empty() {
                println!(
                    "MinerStopGuard drop: mining for {} pending esplora requests",
                    miner.pending_mine_bitcoind.len()
                );
                miner.mine(true, 1);
                miner.mines_satisfied_esplora += 1;
                miner.pending_mine_esplora.clear();
            }
        }
    }
}

pub(crate) fn bitcoin_cli() -> Vec<String> {
    let compose_file = ["tests", "compose.yaml"].join(MAIN_SEPARATOR_STR);
    vec![
        s!("-f"),
        compose_file,
        s!("exec"),
        s!("-T"),
        s!("-u"),
        s!("blits"),
        s!("bitcoind"),
        s!("bitcoin-cli"),
        s!("-regtest"),
    ]
}

fn esplora_bitcoin_cli() -> Vec<String> {
    let compose_file = ["tests", "compose.yaml"].join(MAIN_SEPARATOR_STR);
    vec![
        s!("-f"),
        compose_file,
        s!("exec"),
        s!("-T"),
        s!("esplora"),
        s!("cli"),
    ]
}

impl Miner {
    fn mine(&self, esplora: bool, blocks: u32) -> bool {
        if self.no_mine_count > 0 {
            return false;
        }
        self.force_mine(esplora, blocks)
    }

    fn mine_after_grace_time(&self, esplora: bool, blocks: u32) -> bool {
        if self.no_mine_count > 0 {
            return false;
        }
        let (count, last) = if esplora {
            (
                self.pending_mine_esplora.len(),
                self.pending_mine_esplora.iter().max(),
            )
        } else {
            (
                self.pending_mine_bitcoind.len(),
                self.pending_mine_bitcoind.iter().max(),
            )
        };
        let Some(last) = last else { return false };
        if (OffsetDateTime::now_utc() - *last).as_seconds_f32() < MINE_GRACE_SECS {
            return false;
        }
        println!(
            "mining after grace time on {} for {count} pending requests",
            if esplora { "esplora" } else { "bitcoind" }
        );
        self.force_mine(esplora, blocks)
    }

    fn force_mine(&self, esplora: bool, blocks: u32) -> bool {
        println!(
            "mining on {}, time: {}",
            if esplora { "esplora" } else { "bitcoind" },
            get_current_time()
        );
        let t_0 = OffsetDateTime::now_utc();
        let bitcoin_cli = if esplora {
            esplora_bitcoin_cli()
        } else {
            bitcoin_cli()
        };
        let cmd = || {
            let output = Command::new("docker")
                .stdin(Stdio::null())
                .arg("compose")
                .args(&bitcoin_cli)
                .arg("-rpcwallet=miner")
                .arg("-generate")
                .arg(blocks.to_string())
                .output()
                .expect("failed to mine");
            if output.status.success() {
                true
            } else if !output.status.success()
                && str::from_utf8(&output.stderr)
                    .unwrap()
                    .contains(QUEUE_DEPTH_EXCEEDED)
            {
                false
            } else {
                println!("stdout: {:?}", output.stdout);
                println!("stderr: {:?}", output.stderr);
                panic!("unexpected error");
            }
        };
        if !wait_for_function(cmd, 120, 500) {
            panic!("could not mine ({QUEUE_DEPTH_EXCEEDED})");
        }
        wait_indexers_sync();
        println!(
            "mined on {} in {} s",
            if esplora { "esplora" } else { "bitcoind" },
            (OffsetDateTime::now_utc() - t_0).as_seconds_f32()
        );
        true
    }

    fn stop_mining(&mut self) {
        self.no_mine_count += 1;
    }

    fn resume_mining(&mut self) {
        if self.no_mine_count > 0 {
            self.no_mine_count -= 1;
        }
    }
}

pub(crate) fn mine_blocks(esplora: bool, blocks: u32) {
    let t_0 = OffsetDateTime::now_utc();
    let elapsed = |t: &OffsetDateTime, m: &str| {
        let lapse = (OffsetDateTime::now_utc() - *t).as_seconds_f32();
        println!(
            "waited {lapse} s to {m} on {}",
            if esplora { "esplora" } else { "bitcoind" }
        );
    };

    // register as a pending mine request; all concurrent callers pool here so that
    // mine_after_grace_time() can fire a single mine() on behalf of the whole burst
    let ts = OffsetDateTime::now_utc();
    let satisfied_start = {
        let mut miner = MINER.write().unwrap();
        if esplora {
            miner.pending_mine_esplora.push(ts);
            miner.mines_satisfied_esplora
        } else {
            miner.pending_mine_bitcoind.push(ts);
            miner.mines_satisfied_bitcoind
        }
    };
    elapsed(&t_0, "register");

    let t_1 = OffsetDateTime::now_utc();
    loop {
        if (OffsetDateTime::now_utc() - t_1).as_seconds_f32() > 120.0 {
            panic!("unable to mine");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        let mut miner = MINER.write().unwrap();
        let satisfied = if esplora {
            miner.mines_satisfied_esplora > satisfied_start
        } else {
            miner.mines_satisfied_bitcoind > satisfied_start
        };
        if satisfied {
            elapsed(&t_1, "be satisfied");
            return;
        }
        // first thread past the grace period mines once for the whole batch
        if miner.mine_after_grace_time(esplora, blocks) {
            if esplora {
                miner.mines_satisfied_esplora += 1;
                miner.pending_mine_esplora.clear();
            } else {
                miner.mines_satisfied_bitcoind += 1;
                miner.pending_mine_bitcoind.clear();
            }
            elapsed(&t_1, "mine after grace");
            return;
        }
        drop(miner)
    }
}

pub(crate) fn mine(esplora: bool) {
    mine_blocks(esplora, 1)
}

pub fn get_tx_height(esplora: bool, txid: &str) -> Option<u64> {
    let indexer_url = match esplora {
        true => ESPLORA_URL,
        false => ELECTRUM_URL,
    };
    let indexer = build_indexer(indexer_url).expect("cannot get indexer");
    indexer.get_tx_confirmations(txid).unwrap()
}

pub fn mine_tx(esplora: bool, txid: &str) {
    println!("trying to have TX {txid} mined");
    for _ in 0..10 {
        if let Some(conf_num) = get_tx_height(esplora, txid)
            && conf_num > 0
        {
            println!("TX with ID {txid} has been mined");
            return;
        }
        mine(esplora);
    }
    panic!("TX is not getting mined");
}

// this allows a test that called stop_mining_when_alone() to mine a block without resuming mining
// for other tests.
//
// it works because a block is only mined if the no mine counter is <= 1, which makes sure no other
// test has called the regular stop_mining() and is assuming no blocks will be mined.
//
// this needs to be called only by tests that stopped mining via stop_mining_when_alone() in order
// to avoid deadlocks, since otherwise two tests could increment the no mine counter, making it
// impossible for it to get back to 1
pub(crate) fn force_mine_no_resume_when_alone(esplora: bool) {
    let t_0 = OffsetDateTime::now_utc();
    loop {
        if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 120.0 {
            panic!("unable to mine but no resume when alone");
        }
        let miner = MINER.write().unwrap();
        if miner.no_mine_count <= 1 {
            miner.force_mine(esplora, 1);
            break;
        }
        drop(miner);
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

pub(crate) fn stop_mining() -> MinerStopGuard {
    loop {
        let now = OffsetDateTime::now_utc();
        let mut miner = MINER.write().unwrap();

        // if there are pending mines since > 60s wait for the counter to
        // get back to 0 to avoid mining starvation
        let stale = |v: &Vec<OffsetDateTime>| v.iter().any(|&t| (now - t).as_seconds_f32() > 60.0);
        let stale_bitcoind = stale(&miner.pending_mine_bitcoind);
        let stale_esplora = stale(&miner.pending_mine_esplora);
        if stale_bitcoind || stale_esplora {
            drop(miner);
            println!("blocking stop_mining request due to stale pending mine requests");
            std::thread::sleep(std::time::Duration::from_millis(500));
            continue;
        }

        // multiple tests can increment the no mine counter and proceed with their no mining section
        // in parallel
        miner.stop_mining();
        return MinerStopGuard;
    }
}

// this is needed when a test needs to mine while keeping mining stopped for all other tests.
//
// it works because it waits for the no mine counter to be 0, so further tests calling this will
// block until the first is done.
//
// other tests calling the regular stop_mining() won't have issues as the only way to mine a block
// in this condition is by calling force_mine_no_resume_when_alone(), which waits for the no mine
// counter to be <= 1, meaning other tests have exited the code region where 0 blocks are expected.
pub(crate) fn stop_mining_when_alone() -> MinerStopGuard {
    let t_0 = OffsetDateTime::now_utc();
    loop {
        if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 120.0 {
            panic!("unable to stop mining when alone");
        }
        let mut miner = MINER.write().unwrap();
        if miner.no_mine_count == 0 {
            miner.stop_mining();
            break;
        }
        drop(miner);
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    MinerStopGuard
}

pub(crate) fn estimate_smart_fee(esplora: bool) -> bool {
    let bitcoin_cli = if esplora {
        esplora_bitcoin_cli()
    } else {
        bitcoin_cli()
    };
    let mut json_output: Option<Value> = None;
    let cmd = || {
        let output = Command::new("docker")
            .stdin(Stdio::null())
            .arg("compose")
            .args(&bitcoin_cli)
            .arg("estimatesmartfee")
            .arg("1")
            .output()
            .expect("failed to estimate fee");
        if output.status.success() {
            let output_str = String::from_utf8(output.stdout).unwrap();
            json_output = serde_json::from_str(&output_str).unwrap();
            true
        } else if !output.status.success()
            && str::from_utf8(&output.stderr)
                .unwrap()
                .contains(QUEUE_DEPTH_EXCEEDED)
        {
            false
        } else {
            println!("stdout: {:?}", output.stdout);
            println!("stderr: {:?}", output.stderr);
            panic!("unexpected error");
        }
    };
    if !wait_for_function(cmd, 120, 500) {
        panic!("could not request fee estimation ({QUEUE_DEPTH_EXCEEDED})");
    }
    json_output.unwrap().get("errors").is_none()
}

pub(crate) fn wait_indexers_sync() {
    let t_0 = OffsetDateTime::now_utc();
    let mut max_blockcount = 0;
    for bitcoin_cli in [bitcoin_cli(), esplora_bitcoin_cli()] {
        let output = loop {
            if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 120.0 {
                panic!("could not get blockcount ({QUEUE_DEPTH_EXCEEDED})");
            }
            let output = Command::new("docker")
                .stdin(Stdio::null())
                .arg("compose")
                .args(&bitcoin_cli)
                .arg("getblockcount")
                .output()
                .expect("failed to call getblockcount");
            if !output.status.success()
                && str::from_utf8(&output.stderr)
                    .unwrap()
                    .contains(QUEUE_DEPTH_EXCEEDED)
            {
                println!("work queue depth exceeded");
                std::thread::sleep(std::time::Duration::from_millis(500));
                continue;
            }
            assert!(output.status.success());
            break output;
        };
        let blockcount_str =
            std::str::from_utf8(&output.stdout).expect("could not parse blockcount output");
        let blockcount = blockcount_str
            .trim()
            .parse::<u32>()
            .expect("could not parse blockcount");
        max_blockcount = std::cmp::max(blockcount, max_blockcount);
    }
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let mut all_synced = true;

        let mut indexer_urls = vec![];
        #[cfg(feature = "electrum")]
        indexer_urls.extend([ELECTRUM_URL, ELECTRUM_2_URL, ELECTRUM_BLOCKSTREAM_URL]);
        #[cfg(feature = "esplora")]
        indexer_urls.push(ESPLORA_URL);

        for indexer_url in indexer_urls {
            let err_msg = format!("cannot get indexer {indexer_url}");
            let indexer = build_indexer(indexer_url).expect(&err_msg);
            if indexer.block_hash(max_blockcount as usize).is_err() {
                all_synced = false;
            }
        }

        if all_synced {
            break;
        };
        if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 60.0 {
            panic!("indexers not syncing with bitcoind");
        }
    }
}
