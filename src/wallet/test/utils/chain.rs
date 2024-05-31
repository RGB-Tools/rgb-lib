use super::*;

static MINER: Lazy<RwLock<Miner>> = Lazy::new(|| RwLock::new(Miner { no_mine_count: 0 }));

#[derive(Clone, Debug)]
pub(crate) struct Miner {
    no_mine_count: u32,
}

pub(crate) fn bitcoin_cli() -> Vec<String> {
    let compose_file = ["tests", "docker-compose.yml"].join(&MAIN_SEPARATOR.to_string());
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
    let compose_file = ["tests", "docker-compose.yml"].join(&MAIN_SEPARATOR.to_string());
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
    fn mine(&self) -> bool {
        if self.no_mine_count > 0 {
            return false;
        }
        self.force_mine(false)
    }

    fn force_mine(&self, esplora: bool) -> bool {
        let bitcoin_cli = if esplora {
            _esplora_bitcoin_cli()
        } else {
            bitcoin_cli()
        };
        let t_0 = OffsetDateTime::now_utc();
        loop {
            if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 120.0 {
                panic!("could not mine ({QUEUE_DEPTH_EXCEEDED})");
            }
            let output = Command::new("docker")
                .stdin(Stdio::null())
                .arg("compose")
                .args(&bitcoin_cli)
                .arg("-rpcwallet=miner")
                .arg("-generate")
                .arg("1")
                .output()
                .expect("failed to mine");
            if !output.status.success()
                && String::from_utf8(output.stderr)
                    .unwrap()
                    .contains(QUEUE_DEPTH_EXCEEDED)
            {
                eprintln!("work queue depth exceeded");
                std::thread::sleep(std::time::Duration::from_millis(500));
                continue;
            }
            assert!(output.status.success());
            wait_indexers_sync();
            break;
        }
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

pub(crate) fn mine(resume: bool) {
    let t_0 = OffsetDateTime::now_utc();
    if resume {
        resume_mining();
    }
    loop {
        if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 120.0 {
            println!("forcibly breaking mining wait");
            resume_mining();
        }
        let mined = MINER.read().as_ref().unwrap().mine();
        if mined {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

pub(crate) fn mine_but_no_resume(esplora: bool) {
    let t_0 = OffsetDateTime::now_utc();
    loop {
        if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 120.0 {
            println!("forcibly breaking mining wait");
            resume_mining();
        }
        let miner = MINER.write().unwrap();
        if miner.no_mine_count <= 1 {
            miner.force_mine(esplora);
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
            println!("forcibly breaking stop wait");
            stop_mining();
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

        #[cfg(feature = "electrum")]
        for indexer_url in [ELECTRUM_URL, ELECTRUM_2_URL, ELECTRUM_BLOCKSTREAM_URL] {
            let electrum =
                electrum_client::Client::new(indexer_url).expect("cannot get electrum client");
            if electrum.block_header(max_blockcount as usize).is_err() {
                all_synced = false;
            }
        }

        #[cfg(feature = "esplora")]
        {
            let esplora =
                bdk::blockchain::esplora::EsploraBlockchain::new(ESPLORA_URL, INDEXER_STOP_GAP);
            if esplora.get_block_hash(max_blockcount).is_err() {
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
