//! zk-SNARK parameter loading for Sapling and Orchard.
//!
//! - Sapling proving/verification parameters are loaded from embedded bytes
//!   via `wagyu-zcash-parameters`, so no external download is required.
//! - Orchard proving/verification keys are constructed in-memory via
//!   `orchard::circuit`.
//!
//! The parameters are initialised lazily and cached for reuse.

use bellman::groth16::{Parameters, PreparedVerifyingKey};
use bls12_381::Bls12;
use once_cell::sync::OnceCell;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use zcash_proofs::prover::LocalTxProver;

use orchard::circuit::{ProvingKey as OrchardProvingKey, VerifyingKey as OrchardVerifyingKey};

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

/// Cached Orchard proving and verifying parameters.
pub struct OrchardParams {
    /// Orchard proving key (constructed in-memory).
    pub proving_key: OrchardProvingKey,
    /// Orchard verifying key (constructed in-memory).
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

fn load_orchard_params() -> OrchardParams {
    // These are deterministic builders provided by the Orchard crate.
    let proving_key = OrchardProvingKey::build();
    let verifying_key = OrchardVerifyingKey::build();

    OrchardParams {
        proving_key,
        verifying_key,
    }
}

/// Get shared Sapling parameters (lazy init).
pub fn sapling_params() -> &'static SaplingParams {
    static CELL: OnceCell<SaplingParams> = OnceCell::new();
    CELL.get_or_init(load_sapling_params)
}

/// Get shared Orchard parameters (lazy init).
pub fn orchard_params() -> &'static OrchardParams {
    static CELL: OnceCell<OrchardParams> = OnceCell::new();
    CELL.get_or_init(load_orchard_params)
}

/// Build a `LocalTxProver` using cached Sapling parameters.
///
/// Note: The API now requires file paths, so we write parameters to temporary files.
/// This is less efficient but required by the new API.
pub fn sapling_prover() -> LocalTxProver {
    let (spend_path, output_path) = sapling_param_paths();
    LocalTxProver::new(&spend_path, &output_path)
}

fn sapling_param_paths() -> (PathBuf, PathBuf) {
    static PATHS: OnceCell<(PathBuf, PathBuf)> = OnceCell::new();
    PATHS
        .get_or_init(|| {
            let temp_dir = std::env::temp_dir();
            let pid = std::process::id();
            let spend_path = temp_dir.join(format!("pirate-sapling-spend-{}.params", pid));
            let output_path = temp_dir.join(format!("pirate-sapling-output-{}.params", pid));

            let (spend_bytes, output_bytes) = wagyu_zcash_parameters::load_sapling_parameters();
            write_params_file(&spend_path, &spend_bytes);
            write_params_file(&output_path, &output_bytes);

            (spend_path, output_path)
        })
        .clone()
}

fn write_params_file(path: &PathBuf, bytes: &[u8]) {
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, bytes).expect("Failed to write temporary params file");
    let _ = std::fs::remove_file(path);
    std::fs::rename(&tmp_path, path).expect("Failed to move params file into place");
}
