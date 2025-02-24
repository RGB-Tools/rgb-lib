use super::*;

static MINER: Lazy<RwLock<Miner>> = Lazy::new(|| RwLock::new(Miner { no_mine_count: 0 }));

#[derive(Clone, Debug)]
pub(crate) struct Miner {
    no_mine_count: u32,
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

fn _esplora_bitcoin_cli() -> Vec<String> {
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
    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn mine(&self, esplora: bool, blocks: u32) -> bool {
        if self.no_mine_count > 0 {
            return false;
        }
        self.force_mine(esplora, blocks)
    }

    #[cfg(any(feature = "electrum", feature = "esplora"))]
    fn force_mine(&self, esplora: bool, blocks: u32) -> bool {
        println!("mining (esplora: {esplora}), time: {}", get_current_time());
        let bitcoin_cli = if esplora {
            _esplora_bitcoin_cli()
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
                && String::from_utf8(output.stderr.clone())
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

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn mine_blocks(esplora: bool, blocks: u32, resume: bool) {
    let t_0 = OffsetDateTime::now_utc();
    if resume {
        resume_mining();
    }
    loop {
        if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 120.0 {
            panic!("unable to mine");
        }
        let mined = MINER.read().as_ref().unwrap().mine(esplora, blocks);
        if mined {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn mine(esplora: bool, resume: bool) {
    mine_blocks(esplora, 1, resume)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
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

pub(crate) fn stop_mining() {
    MINER.write().unwrap().stop_mining()
}

pub(crate) fn stop_mining_when_alone() {
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
}

pub(crate) fn resume_mining() {
    MINER.write().unwrap().resume_mining()
}

pub(crate) fn estimate_smart_fee(esplora: bool) -> bool {
    let bitcoin_cli = if esplora {
        _esplora_bitcoin_cli()
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
            && String::from_utf8(output.stderr.clone())
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

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn wait_indexers_sync() {
    let t_0 = OffsetDateTime::now_utc();
    let mut max_blockcount = 0;
    for bitcoin_cli in [bitcoin_cli(), _esplora_bitcoin_cli()] {
        let t_0 = OffsetDateTime::now_utc();
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
                && String::from_utf8(output.stderr.clone())
                    .unwrap()
                    .contains(QUEUE_DEPTH_EXCEEDED)
            {
                eprintln!("work queue depth exceeded");
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
            let indexer = build_indexer(indexer_url).expect("cannot get indexer {indexer}");
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
