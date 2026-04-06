//! zk-SNARK parameter loading for Sapling and Orchard.
//!
//! - Sapling proving/verification parameters are loaded from embedded bytes
//!   via an embedded parameter crate, so no external download is required.
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
use tempfile::{Builder, NamedTempFile};
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
    ensure_sapling_param_files(&spend_path, &output_path);
    LocalTxProver::new(&spend_path, &output_path)
}

fn sapling_param_paths() -> (PathBuf, PathBuf) {
    static PATHS: OnceCell<(PathBuf, PathBuf)> = OnceCell::new();
    PATHS
        .get_or_init(|| {
            let spend_path = Builder::new()
                .prefix("pirate-sapling-spend-")
                .suffix(".params")
                .tempfile()
                .expect("failed to create secure Sapling spend params temp file")
                .into_temp_path()
                .keep()
                .expect("failed to persist secure Sapling spend params temp file");
            let output_path = Builder::new()
                .prefix("pirate-sapling-output-")
                .suffix(".params")
                .tempfile()
                .expect("failed to create secure Sapling output params temp file")
                .into_temp_path()
                .keep()
                .expect("failed to persist secure Sapling output params temp file");

            ensure_sapling_param_files(&spend_path, &output_path);

            (spend_path, output_path)
        })
        .clone()
}

fn ensure_sapling_param_files(spend_path: &PathBuf, output_path: &PathBuf) {
    let (spend_bytes, output_bytes) = wagyu_zcash_parameters::load_sapling_parameters();
    ensure_params_file(spend_path, &spend_bytes);
    ensure_params_file(output_path, &output_bytes);
}

fn ensure_params_file(path: &PathBuf, bytes: &[u8]) {
    let expected_len = bytes.len() as u64;
    let needs_write = match std::fs::metadata(path) {
        Ok(meta) => meta.len() != expected_len,
        Err(_) => true,
    };
    if needs_write {
        write_params_file(path, bytes);
    }
}

fn write_params_file(path: &PathBuf, bytes: &[u8]) {
    let parent = path
        .parent()
        .expect("params path should always have a parent directory");
    let mut tmp_file =
        NamedTempFile::new_in(parent).expect("Failed to create secure temporary params file");
    use std::io::Write as _;
    tmp_file
        .write_all(bytes)
        .expect("Failed to write temporary params file");
    tmp_file
        .flush()
        .expect("Failed to flush temporary params file");

    let temp_path = tmp_file.into_temp_path();
    temp_path
        .persist(path)
        .or_else(|e| {
            let temp_path = e.path;
            let _ = std::fs::remove_file(path);
            temp_path.persist(path)
        })
        .expect("Failed to move params file into place");
}
