# RGB Lib

`rgb-lib` is a Rust library which provides tools to build cross-platform RGB
compatible wallets in a simple fashion, without having to worry about Bitcoin
and RGB internals.

It primarily uses [bdk] to provide Bitcoin walleting functionalities, and
several RGB libraries such as [rgb-core] to provide RGB specific
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

## Language bindings
Bindings for other languages are available. Check the [bindings] directory.

## Tests
In order to run the available tests, execute:
```sh
cargo test --workspace --all-features
```

This command will run a [bitcoind] node, three [electrs] nodes, one [esplora]
node and three [RGB proxy] instances, in order to perform integration tests in
a regtest environment.

Services will not be stopped automatically after the test run. To stop them and
remove all containers, from the project root execute:
```sh
docker compose -f tests/compose.yaml down
```

## Diagrams
The [`docs/`](/docs) directory contains some documents and UML diagrams
to simplify the initial understanding of how rgb-lib operates.

These include typical flows for issuing/sending/receiving assets
and the state transitions of an asset transfer.

## Roadmap
- add an API to extend `BlindData` expiration
- add support for more databases
- improve UTXO management
- improve the library's performance


[RGB proxy]: https://github.com/RGB-Tools/rgb-proxy-server
[bdk]: https://github.com/bitcoindevkit/bdk
[bindings]: bindings/
[bitcoind]: https://github.com/bitcoin/bitcoin
[electrs]: https://github.com/romanz/electrs
[esplora]: https://github.com/Blockstream/esplora
[rgb-core]: https://github.com/RGB-WG/rgb-core
