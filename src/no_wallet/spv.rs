//! SPV proofs.
//!
//! This module defines the filling in of the Bitcoin SPV proofs the witness transactions of a
//! consignment are missing, which doesn't require a wallet but needs an indexer to retrieve them,
//! so it requires an indexer validation feature.

use super::*;

/// Report of [`fill_spv_proofs`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct SpvProofFill {
    /// Witness transactions still carrying no SPV proof, as the indexer couldn't provide one,
    /// sorted by TXID. Empty if every witness of the consignment carries one
    pub without_proof: Vec<WitnessWithoutProof>,
    /// Whether the consignment file has been rewritten, which only happens if the operation
    /// retrieved at least one proof
    pub rewritten: bool,
}

/// A witness transaction of a consignment the indexer couldn't provide an SPV proof for, see
/// [`SpvProofFill::without_proof`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "camel_case", serde(rename_all = "camelCase"))]
pub struct WitnessWithoutProof {
    /// TXID of the witness transaction
    pub txid: String,
    /// Why the indexer couldn't provide a proof for this transaction, e.g. because it's not mined
    /// yet. A failure that isn't about a single transaction, e.g. the indexer being unreachable,
    /// fails the whole operation instead
    pub details: String,
}

/// Fill in the RGB transfer consignment at the given path, without a wallet, with the Bitcoin SPV
/// proofs its witness transactions are missing, retrieved from the indexer described by
/// `indexer_config`, and write it back to the same path.
///
/// A consignment carries the SPV proofs of the witness transactions its sender could retrieve when
/// generating it, so a witness mined afterwards has none, and neither has one whose sender used an
/// indexer that couldn't provide it. Filling in the missing proofs lets a party validating the
/// consignment (see [`validate_consignment`]) establish where the witnesses are mined from the
/// block headers alone, without looking up the transactions themselves, which is what a validating
/// party with no indexer of its own (e.g. one holding a header chain it has validated itself)
/// needs.
///
/// Only the witnesses carrying no proof are looked up, so the indexer is asked nothing about the
/// ones already proven. A proof the consignment already carries is therefore kept as it is, even
/// if a reorg has moved its witness and left it no longer verifying: validation falls back to
/// resolving such a witness without it (reporting a
/// [`ValidationWarning::InvalidSpvProof`](crate::no_wallet::ValidationWarning::InvalidSpvProof)),
/// and refreshing it means dropping the stale proof before filling in again.
///
/// The indexer is not trusted: a proof is checked against the block header the validating party
/// reports for its height, so one the indexer made up fails to verify (and the witness is resolved
/// without it). The indexer must be for the network of the consignment's asset, taken from its
/// genesis, or an [`Error::InvalidIndexer`] is returned.
///
/// Returns the report of which witnesses are still without a proof, so the caller can tell whether
/// the consignment is fully proven, and whether the file has been rewritten: it's left untouched if
/// no proof has been retrieved, so filling in twice in a row writes once.
///
/// An [`Error::InvalidConsignment`] is returned if the file is not a transfer consignment: a
/// contract consignment holds the genesis only, so it has no witness to prove.
pub fn fill_spv_proofs(
    consignment_path: &str,
    indexer_config: &IndexerConfig,
) -> Result<SpvProofFill, Error> {
    check_file_exists(consignment_path)?;
    let Ok(UniversalFile::Transfer(mut consignment)) = UniversalFile::load_file(consignment_path)
    else {
        return Err(Error::InvalidConsignment);
    };
    let bitcoin_network = BitcoinNetwork::try_from(consignment.genesis().chain_net)?;
    let resolver = validation::get_resolver(indexer_config, bitcoin_network)?;

    // a witness is looked up once, even if the consignment holds more than one bundle anchored to
    // it: the retrieved proof then goes to the bundles carrying none, leaving the ones that
    // already have one as they are
    let missing: BTreeSet<RgbTxid> = consignment
        .bundled_witnesses()
        .filter(|witness_bundle| witness_bundle.spv_proof.is_none())
        .map(|witness_bundle| witness_bundle.witness_id())
        .collect();

    let mut fill = SpvProofFill {
        without_proof: vec![],
        rewritten: false,
    };
    for witness_id in missing {
        match resolver.resolve_spv_proof(witness_id) {
            Ok(proof) => {
                consignment.modify_bundle(witness_id, |witness_bundle| {
                    if witness_bundle.spv_proof.is_none() {
                        witness_bundle.spv_proof = Some(proof.clone());
                    }
                });
                fill.rewritten = true;
            }
            // only a failure about this witness transaction, e.g. that it's not mined yet, leaves
            // the consignment as it is for it: one that isn't, e.g. an unreachable indexer, would
            // otherwise be reported as a missing proof for each of them
            Err(WitnessResolverError::ResolverIssue(Some(txid), details)) if txid == witness_id => {
                fill.without_proof.push(WitnessWithoutProof {
                    txid: witness_id.to_string(),
                    details,
                })
            }
            Err(e) => {
                return Err(Error::Indexer {
                    details: e.to_string(),
                });
            }
        }
    }
    // the witnesses are looked up in the internal byte order of their TXIDs, which is the reverse
    // of the hex the reported ones are, so the report is sorted again
    fill.without_proof.sort_by(|a, b| a.txid.cmp(&b.txid));

    if fill.rewritten {
        atomic_write_with(Path::new(consignment_path), |tmp| {
            Ok(consignment.save_file(tmp)?)
        })?;
    }

    Ok(fill)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_spv_proofs_invalid_file() {
        // the indexer is never reached, as the consignment is read first
        let indexer_config = IndexerConfig::Electrum { url: s!("") };

        // inexistent file
        let result = fill_spv_proofs("inexistent", &indexer_config);
        assert!(
            matches!(result, Err(Error::InvalidFilePath { file_path: f }) if f == "inexistent")
        );

        // not a consignment
        let file = tempfile::NamedTempFile::with_prefix("fill_spv_proofs::").unwrap();
        std::fs::write(file.path(), b"not a consignment").unwrap();
        let result = fill_spv_proofs(&file.path().to_string_lossy(), &indexer_config);
        assert_matches!(result, Err(Error::InvalidConsignment));
    }
}
