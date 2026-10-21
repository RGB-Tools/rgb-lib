# RGB Lib

`rgb-lib` is a Rust library which provides tools to build cross-platform RGB
compatible wallets in a simple fashion, without having to worry about Bitcoin
and RGB internals.

It primarily uses [bdk] to provide Bitcoin walleting functionalities, and
several RGB libraries to provide RGB specific functionalities that can be found
in the [rgb-protocol] organization.

The library has been designed to offer an offline usage
(some APIs do not require using Internet) and also a watch-only usage (the
library can work without private keys, sign operations will need to be performed
with another tool).

Multisig wallets are supported: each cosigner uses rgb-lib with the same
multisig setup and a separate singlesig wallet for signing.
To coordinate cosigners (propose operations, review, approve or refuse them),
use the [RGB multisig hub]. See [Multisig][docs-multisig] for more info.

N.B.: this library is still a work in progress and in its testing phase. Also,
as long as the version is 0.*, API breaking changes should be expected.

## Important remark
> :warning: **Warning: never instantiate a wallet in multiple places!**
>
> Using the same wallet (mnemonic phrase) on multiple directories or devices
> can lead to RGB asset loss due to improper UTXO management.
>
> This includes keeping two instances live on a single data directory: an
> instance loads its BDK state from the database when it is created and does not
> re-read it afterwards, so the two immediately drift apart. They will hand out
> the same addresses, causing address reuse, and each will persist state the
> other is unaware of. A watch-only wallet and its signing counterpart are no
> exception: keep a single instance performing wallet operations and use the
> other one only to sign.

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

This command will run four [bitcoind] nodes, four [electrs] nodes, one
[esplora] node, four [RGB proxy] instances, one [RGB multisig hub] instance, an
[nginx] server (serving reject lists) and an EVM ([anvil]) node, in order to
perform integration tests in regtest and custom signet environments.

Services will not be stopped automatically after the test run. To stop them and
remove all containers, from the project root execute:
```sh
docker compose -f tests/compose.yaml down
```

## Diagrams
The [`docs/`](/docs) directory contains some documents and UML diagrams
to simplify the initial understanding of how rgb-lib operates.

These include typical flows for issuing/sending/receiving assets, the state
transitions of an asset transfer and [multisig][docs-multisig] coordination.

## Roadmap
- add an API to extend `BlindData` expiration
- add support for more databases
- improve UTXO management
- improve the library's performance


[RGB multisig hub]: https://github.com/RGB-Tools/rgb-multisig-hub
[RGB proxy]: https://github.com/RGB-Tools/rgb-proxy-server
[anvil]: https://github.com/foundry-rs/foundry
[bdk]: https://github.com/bitcoindevkit/bdk
[bindings]: bindings/
[docs-multisig]: docs/multisig.md
[bitcoind]: https://github.com/bitcoin/bitcoin
[electrs]: https://github.com/romanz/electrs
[esplora]: https://github.com/Blockstream/esplora
[nginx]: https://nginx.org
[rgb-protocol]: https://github.com/rgb-protocol
