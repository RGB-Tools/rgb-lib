# RGB Lib

`rgb-lib` is a Rust library which provides tools to build cross-platform RGB
compatible wallets in a simple fashion, without having to worry about Bitcoin
and RGB internals.

It primarily uses [bdk] to provide Bitcoin walleting functionalities, and
several RGB libraries such as [rgb-node] to provide RGB specific
functionalities.

The library has been designed to offer an offline usage
(some APIs do not require using Internet) and also a watch-only usage (the
library can work without private keys, sign operations will need to be performed
with another tool).

N.B.: this library is still a work in progress and in its testing phase. Also,
as long as the version is 0.*, API breaking changes should be expected.

## Important remark
> :warning: **Warning: never use the same wallet on more than one device!**
>
> Using the same wallet (mnemonic phrase) on multiple devices can lead to RGB
> asset loss due to improper UTXO management.

This library is intended to exclusively handle all UTXOs for the wallet. Using
the same mnemonic phrase on any other device, including with this same library,
can lead to serious issues and ultimately to RGB asset loss.

Each time the wallet is brought online, a consistency check is carried out to
make sure the UTXO set has not changed since the last synchronization and an
error is returned in case discrepancies are detected.

## Native language bindings
Native language bindings for this library are also available via the
[rgb-lib-ffi] project.

## Tests
In order to run the available tests, execute:
```bash
cargo test
```

This command will run a [bitcoind] node and an [electrs] node in order to
perform integration tests in a regtest environment.

Services will not be stopped automatically after the test run. To stop them and
remove all containers, from the project root execute:
```sh
docker-compose -f tests/docker-compose.yml down
```

## Known issues
- the library doesn't currently work when built in release mode
- running all tests in parallel opens a lot of file descriptors, hitting the
  default 1024 limit, so it needs to be increased (e.g. `ulimit -n 2048`);
  running tests in smaller batches (e.g. `cargo test send` is also possible)

## Roadmap
- add an API to extend `BlindData` expiration
- add a backup/restore system
- add support for more databases
- improve UTXO management
- improve RGB services handling
- improve the library's performance


[bdk]: https://github.com/bitcoindevkit/bdk
[bitcoind]: https://github.com/bitcoin/bitcoin
[electrs]: https://github.com/romanz/electrs
[rgb-lib-ffi]: /rgb-lib-ffi/
[rgb-node]: https://github.com/RGB-WG/rgb-node
