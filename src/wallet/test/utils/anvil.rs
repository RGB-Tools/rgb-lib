use super::*;

/// Anvil's first pre-funded account address
const ANVIL_ACCOUNT_0: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
/// Anvil's first pre-funded account private key
const ANVIL_PRIVATE_KEY_0: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

pub(crate) struct DeployedContract {
    pub address: String,
    pub deployer: String,
}

fn anvil_exec() -> Vec<String> {
    let compose_file = ["tests", "compose.yaml"].join(MAIN_SEPARATOR_STR);
    vec![s!("-f"), compose_file, s!("exec"), s!("-T"), s!("anvil")]
}

/// Deploy the TestERC20 contract on anvil and return contract info.
pub(crate) fn deploy_test_erc20(
    name: &str,
    symbol: &str,
    decimals: u8,
    initial_supply: u64,
) -> DeployedContract {
    forge_create(
        "TestERC20.sol:TestERC20",
        &[
            name.to_string(),
            symbol.to_string(),
            decimals.to_string(),
            initial_supply.to_string(),
        ],
    )
}

/// Deploy a contract from the `tests/contracts` directory on anvil and return its info.
///
/// Installs OpenZeppelin deps (if needed), then uses `forge create` via docker compose exec
/// inside the anvil container. `target` is the contract to deploy as `<file>:<name>`.
fn forge_create(target: &str, constructor_args: &[String]) -> DeployedContract {
    ensure_openzeppelin_installed();

    let mut args = anvil_exec();
    args.extend([
        s!("forge"),
        s!("create"),
        format!("/contracts/{target}"),
        s!("--rpc-url"),
        EVM_RPC_URL.to_string(),
        s!("--private-key"),
        ANVIL_PRIVATE_KEY_0.to_string(),
        s!("--cache-path"),
        s!("/tmp/forge-cache"),
        s!("--out"),
        s!("/tmp/forge-out"),
        s!("--root"),
        s!("/tmp"),
        s!("--contracts"),
        s!("/contracts"),
        s!("--remappings=@openzeppelin/=/tmp/lib/openzeppelin-contracts/"),
        s!("--broadcast"),
    ]);
    if !constructor_args.is_empty() {
        args.push(s!("--constructor-args"));
        args.extend(constructor_args.iter().cloned());
    }

    let output = Command::new("docker")
        .stdin(Stdio::null())
        .arg("compose")
        .args(&args)
        .output()
        .unwrap_or_else(|e| panic!("failed to deploy {target}: {e}"));

    assert!(
        output.status.success(),
        "forge create {target} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("invalid utf8 from forge create");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // forge may print to either stdout or stderr; search both
    let all_output = format!("{stdout}\n{stderr}");
    let address = all_output
        .lines()
        .find(|l| l.contains("Deployed to:"))
        .unwrap_or_else(|| panic!("could not find contract address in forge output:\n{all_output}"))
        .split("Deployed to:")
        .nth(1)
        .unwrap()
        .trim()
        .to_string();

    DeployedContract {
        address,
        deployer: ANVIL_ACCOUNT_0.to_string(),
    }
}

/// Install OpenZeppelin contracts in the anvil container (idempotent).
///
/// Tests deploying contracts run in parallel and `forge install` creates the destination directory
/// at the start of the clone, so checking for the directory would let a second caller compile
/// against a half-populated tree. Callers are serialized with a `Once` and the completion marker is
/// only written once the install has succeeded, so a failed one is retried instead of being taken
/// for a complete one.
fn ensure_openzeppelin_installed() {
    static INSTALL_OPENZEPPELIN: std::sync::Once = std::sync::Once::new();

    INSTALL_OPENZEPPELIN.call_once(|| {
        let mut args = anvil_exec();
        args.extend([
            s!("sh"),
            s!("-c"),
            s!("test -f /tmp/lib/.openzeppelin-installed || { forge install OpenZeppelin/openzeppelin-contracts --root /tmp --no-git && touch /tmp/lib/.openzeppelin-installed; }"),
        ]);

        let output = Command::new("docker")
            .stdin(Stdio::null())
            .arg("compose")
            .args(&args)
            .output()
            .expect("failed to install OpenZeppelin");

        assert!(
            output.status.success(),
            "OpenZeppelin install failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    });
}

/// Deploy the MinimalBridge contract on anvil and return contract info.
///
/// `token` is the ERC-20 address the bridge will accept.
pub(crate) fn deploy_bridge(token: &str) -> DeployedContract {
    ensure_arbsys_installed();
    forge_create("MinimalBridge.sol:MinimalBridge", &[token.to_string()])
}

/// Run `cast` with the given arguments in the anvil container, returning its trimmed stdout.
fn cast(cast_args: &[String]) -> String {
    let mut args = anvil_exec();
    args.push(s!("cast"));
    args.extend(cast_args.iter().cloned());
    args.extend([s!("--rpc-url"), EVM_RPC_URL.to_string()]);

    let output = Command::new("docker")
        .stdin(Stdio::null())
        .arg("compose")
        .args(&args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run cast {}: {e}", cast_args[0]));

    assert!(
        output.status.success(),
        "cast {} failed: {}",
        cast_args[0],
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout)
        .expect("invalid utf8 from cast")
        .trim()
        .to_string()
}

/// Send a transaction calling `contract` with the given signature and arguments via `cast send`,
/// from the first anvil account.
fn cast_send(contract: &str, sig: &str, sig_args: &[String]) -> String {
    let mut args = vec![s!("send"), contract.to_string(), sig.to_string()];
    args.extend(sig_args.iter().cloned());
    args.extend([s!("--private-key"), ANVIL_PRIVATE_KEY_0.to_string()]);
    cast(&args)
}

/// Install the MockArbSys code at the address of the Arbitrum system precompile (idempotent),
/// which the MinimalBridge contract reads the block number of a lock from: anvil doesn't have it.
fn ensure_arbsys_installed() {
    static INSTALL_ARBSYS: std::sync::Once = std::sync::Once::new();

    INSTALL_ARBSYS.call_once(|| {
        let mock = forge_create("MockArbSys.sol:MockArbSys", &[]);
        let code = cast(&[s!("code"), mock.address]);
        cast(&[
            s!("rpc"),
            s!("anvil_setCode"),
            s!("0x0000000000000000000000000000000000000064"),
            code,
        ]);
    });
}

/// Deploy the RgbRejectList contract on anvil and return contract info.
///
/// The deployer is both the owner and the appender of the list.
pub(crate) fn deploy_reject_list() -> DeployedContract {
    forge_create(
        "RgbRejectList.sol:RgbRejectList",
        &[ANVIL_ACCOUNT_0.to_string(), ANVIL_ACCOUNT_0.to_string()],
    )
}

/// The ways a `MaliciousRejectList` contract breaks the `IRgbRejectList` read model (see
/// `tests/contracts/MaliciousRejectList.sol`, whose `Mode` enum this mirrors).
#[derive(Clone, Copy, Debug)]
pub(crate) enum MaliciousRejectListMode {
    LengthReverts,
    EntriesReverts,
    ShortPage,
    BadBool,
    LengthOverflow,
    Shrinking,
    HugeLength,
}

/// Deploy a MaliciousRejectList contract behaving as `mode` on anvil and return contract info.
pub(crate) fn deploy_malicious_reject_list(mode: MaliciousRejectListMode) -> DeployedContract {
    forge_create(
        "MaliciousRejectList.sol:MaliciousRejectList",
        &[(mode as u8).to_string()],
    )
}

/// Append entries to the RgbRejectList contract at `reject_list`.
///
/// Each entry is the RGB operation output it applies to and whether it is rejected.
pub(crate) fn reject_list_append(reject_list: &str, entries: &[(Opout, bool)]) {
    let batch = entries
        .iter()
        .map(|(opout, reject)| {
            format!(
                "(0x{},{},{},{reject})",
                hex::encode(opout.op.to_byte_array()),
                opout.ty.to_inner(),
                opout.no
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    cast_send(
        reject_list,
        "append((bytes32,uint16,uint16,bool)[])",
        &[format!("[{batch}]")],
    );
}

/// Query the ERC-20 balance of an account via `cast call`.
pub(crate) fn erc20_balance_of(contract: &str, account: &str) -> u64 {
    let raw = cast(&[
        s!("call"),
        contract.to_string(),
        s!("balanceOf(address)(uint256)"),
        account.to_string(),
    ]);
    // cast may append a human-readable suffix like "1000000 [1e6]"
    let value = raw.split_whitespace().next().unwrap_or("");
    if let Some(hex) = value.strip_prefix("0x") {
        u64::from_str_radix(hex, 16)
    } else {
        value.parse::<u64>()
    }
    .unwrap_or_else(|e| panic!("could not parse balance from {value:?}: {e}"))
}

/// Approve `spender` to transfer `amount` of an ERC-20 token via `cast send`.
pub(crate) fn erc20_approve(token: &str, spender: &str, amount: u64) {
    cast_send(
        token,
        "approve(address,uint256)",
        &[spender.to_string(), amount.to_string()],
    );
}

/// The ID of the lock the mint begun with the result `res` has to be made for, computed from
/// its operation ID and the TXID of its (unsigned) PSBT.
pub(crate) fn mint_lock_id(res: &MintBeginResult) -> String {
    let txid = Psbt::from_str(&res.psbt)
        .unwrap()
        .unsigned_tx
        .compute_txid();
    LockId::with(
        OpId::from_str(&res.details.opid).unwrap(),
        RgbTxid::from_str(&txid.to_string()).unwrap(),
    )
    .to_string()
}

/// Call `fundsIn` on the MinimalBridge contract to lock ERC-20 tokens.
///
/// `lock_id` is the 64-char hex ID of the lock (used as `lockId`, see [`mint_lock_id`]). The lock
/// has no deadline (`beforeBlock` is the maximum block).
pub(crate) fn bridge_funds_in(bridge: &str, amount: u64, lock_id: &str) {
    cast_send(
        bridge,
        "fundsIn(uint256,uint256,uint64)",
        &[
            amount.to_string(),
            format!("0x{lock_id}"),
            u64::MAX.to_string(),
        ],
    );
}

/// Call `fundsOut` on the MinimalBridge contract (as its owner) to release `amount` of the locked
/// ERC-20 tokens to `recipient`.
pub(crate) fn bridge_funds_out(bridge: &str, recipient: &str, amount: u64) {
    cast_send(
        bridge,
        "fundsOut(address,uint256,uint256,string)",
        &[recipient.to_string(), amount.to_string(), s!("1"), s!("")],
    );
}

/// Mine `blocks` empty blocks on anvil via `anvil_mine`.
pub(crate) fn evm_mine(blocks: u64) {
    cast(&[s!("rpc"), s!("anvil_mine"), format!("0x{blocks:x}")]);
}

/// Number of the latest block on anvil.
pub(crate) fn evm_block_number() -> u64 {
    let block = cast(&[s!("block-number")]);
    block
        .parse()
        .unwrap_or_else(|e| panic!("could not parse block number from {block:?}: {e}"))
}

/// A block for a mint to commit to, leaving its lock [`MINT_LOCK_BLOCKS`] blocks after the latest
/// one.
pub(crate) fn default_before_block() -> u64 {
    evm_block_number() + MINT_LOCK_BLOCKS
}
