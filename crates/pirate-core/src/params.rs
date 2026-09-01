//! zk-SNARK parameter loading for Sapling and Ironwood.
//!
//! - Sapling proving/verification parameters are loaded directly from embedded
//!   bytes, so no external download or writable filesystem location is required.
//! - Ironwood proving/verification keys are constructed in-memory via
//!   `orchard::circuit`.
//!
//! The parameters are initialised lazily and cached for reuse.

use bellman::groth16::{Parameters, PreparedVerifyingKey};
use bls12_381::Bls12;
use once_cell::sync::OnceCell;
use std::io::Cursor;
use std::sync::Arc;
use zcash_proofs::prover::LocalTxProver;

use orchard::circuit::{
    OrchardCircuitVersion, ProvingKey as OrchardProvingKey, VerifyingKey as OrchardVerifyingKey,
};

/// Cached Sapling proving and verifying parameters.
pub struct SaplingParams {
    /// Sapling spend proving parameters.
    pub spend_params: Arc<Parameters<Bls12>>,
    /// Sapling output proving parameters.
    pub output_params: Arc<Parameters<Bls12>>,
    /// Prepared spend verifying key.
    pub spend_vk: Arc<PreparedVerifyingKey<Bls12>>,
    /// Prepared output verifying key.
    pub output_vk: Arc<PreparedVerifyingKey<Bls12>>,
}

/// Cached Ironwood proving and verifying parameters.
pub struct IronwoodParams {
    /// Ironwood proving key (constructed in-memory).
    pub proving_key: OrchardProvingKey,
    /// Ironwood verifying key (constructed in-memory).
    pub verifying_key: OrchardVerifyingKey,
}

fn load_sapling_params() -> SaplingParams {
    let (spend_bytes, output_bytes) = wagyu_zcash_parameters::load_sapling_parameters();

    let spend_params = Parameters::<Bls12>::read(&mut Cursor::new(spend_bytes), false)
        .expect("couldn't deserialize Sapling spend parameters");
    let output_params = Parameters::<Bls12>::read(&mut Cursor::new(output_bytes), false)
        .expect("couldn't deserialize Sapling output parameters");

    // Prepare verifying keys for efficient verification
    use bellman::groth16::prepare_verifying_key;
    let spend_vk = prepare_verifying_key(&spend_params.vk);
    let output_vk = prepare_verifying_key(&output_params.vk);

    SaplingParams {
        spend_params: Arc::new(spend_params),
        output_params: Arc::new(output_params),
        spend_vk: Arc::new(spend_vk),
        output_vk: Arc::new(output_vk),
    }
}

fn load_ironwood_params() -> IronwoodParams {
    let circuit_version = OrchardCircuitVersion::PostNu6_3;
    let proving_key = OrchardProvingKey::build(circuit_version);
    let verifying_key = OrchardVerifyingKey::build(circuit_version);

    IronwoodParams {
        proving_key,
        verifying_key,
    }
}

/// Get shared Sapling parameters (lazy init).
pub fn sapling_params() -> &'static SaplingParams {
    static CELL: OnceCell<SaplingParams> = OnceCell::new();
    CELL.get_or_init(load_sapling_params)
}

/// Get shared Ironwood parameters (lazy init).
pub fn ironwood_params() -> &'static IronwoodParams {
    static CELL: OnceCell<IronwoodParams> = OnceCell::new();
    CELL.get_or_init(load_ironwood_params)
}

/// Build a `LocalTxProver` directly from the embedded Sapling parameters.
///
/// Keeping this path entirely in memory avoids platform-specific temporary
/// directory behavior and leaves no proving-parameter files behind on disk.
pub fn sapling_prover() -> LocalTxProver {
    let (spend_bytes, output_bytes) = wagyu_zcash_parameters::load_sapling_parameters();
    LocalTxProver::from_bytes(&spend_bytes, &output_bytes)
}

#[cfg(test)]
mod tests {
    #[test]
    fn constructs_sapling_prover_from_embedded_parameters() {
        let _prover = super::sapling_prover();
    }
}
