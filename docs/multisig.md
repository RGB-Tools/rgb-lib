# Multisig

`rgb-lib` supports multisig wallets where a group of cosigners must cooperate
to perform operations.

## Overview

Each cosigner runs:

- a `MultisigWallet` with the same multisig keys, used to perform operations
- a separate singlesig wallet (software or hardware), used only for
  signing

Third parties can optionally be given watch-only access to the multisig wallet
with a restricted set of APIs.

## Coordination

Cosigners need to exchange the information required to coordinate operation
proposals, reviews, approvals, and refusals. The
[RGB multisig hub][hub repository] supports this workflow:

- cosigners create new operations, auto-approved or requiring approval, and
  retrieve operations created by others
- cosigners review operations that require approval and respond by approving or
  refusing them
- once an operation receives the required number of approvals, it becomes
  approved; if it can no longer reach the threshold, it is discarded
- cosigners process approved operations and skip discarded ones

The hub handles authentication (Biscuit tokens), configuration (xPubs,
thresholds, `rgb-lib` version), tracks operation, cosigner progress and
address indexes (to avoid address reuse), and coordinates transfer failures
across cosigners.
See the [hub repository](https://github.com/RGB-Tools/rgb-multisig-hub)
for installation, configuration, and API details.

## Setup and operation flow

Create a `MultisigWallet` for each cosigner using `MultisigWallet::new`, with:

- `WalletData`: the same structure used for singlesig wallets
- `MultisigKeys`: built providing the cosigner list plus vanilla and colored
  thresholds. Each cosigner is a `Cosigner` created from singlesig keys

Cosigners can sign PSBTs with their singlesig wallets (software or hardware).
Note: a singlesig software wallet can be created calling `Wallet::new`
providing the cosigners's mnemonic and used to sign via its `sign_psbt` method.

When going online, pass the hub URL and the cosigner's token to the
`go_online` method.

Call `sync_with_hub` repeatedly until it returns `None` (no more
operations to handle). Only one operation can be pending at a time; operations
must be processed in order.

For operations that require approval, the initiator calls the corresponding
`*_init` method (which builds the operation and posts it to the hub with the
unsigned PSBT), then signs the PSBT and responds with `respond_to_operation`,
in the same way as other cosigners will do after retrieving the PSBT by calling
`sync_with_hub`.

Operations like receive and issuance are auto-approved, meaning they do not
produce PSBTs and as a result do not require cosigner signatures nor responses.
One cosigner initiates the operation and the others process it by calling
`sync_with_hub`.

For send, inflate and receive operations, the `refresh` API might be needed to
complete the transfer on each cosigner side.

## Backup and recovery

Because all cosigners share the same multisig descriptor, any cosigner's wallet
backup is sufficient to fully restore the multisig wallet. If a cosigner loses
their local state, they can recover by importing the backup of any other
cosigner.

Each cosigner should therefore keep their own backup up to date, but in an
emergency a single surviving backup is enough to restore access for all
participants.


[hub repository]: https://github.com/RGB-Tools/rgb-multisig-hub
