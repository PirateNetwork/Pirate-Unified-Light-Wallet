use crate::shardtree_store::{Error as ShardStoreError, SqliteShardStore};
use crate::{Database, Error, Result};
use incrementalmerkletree::Position;
use orchard::note::ExtractedNoteCommitment as OrchardExtractedNoteCommitment;
use orchard::tree::{MerkleHashOrchard, MerklePath as OrchardMerklePath};
use pirate_core::selection::{NoteType as SelectableNoteType, SelectableNote};
use rusqlite::OptionalExtension;
use shardtree::{error::ShardTreeError, ShardTree};
use zcash_primitives::sapling::{Node as SaplingNode, NOTE_COMMITMENT_TREE_DEPTH};

const SAPLING_TABLE_PREFIX: &str = "sapling";
const ORCHARD_TABLE_PREFIX: &str = "orchard";
const SHARDTREE_PRUNING_DEPTH: usize = 1000;
const SAPLING_SHARD_HEIGHT: u8 = NOTE_COMMITMENT_TREE_DEPTH / 2;
const ORCHARD_SHARD_HEIGHT: u8 = NOTE_COMMITMENT_TREE_DEPTH / 2;

type SaplingTree<'a> = ShardTree<
    SqliteShardStore<&'a rusqlite::Connection, SaplingNode, SAPLING_SHARD_HEIGHT>,
    { NOTE_COMMITMENT_TREE_DEPTH },
    SAPLING_SHARD_HEIGHT,
>;
type OrchardTree<'a> = ShardTree<
    SqliteShardStore<&'a rusqlite::Connection, MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>,
    { NOTE_COMMITMENT_TREE_DEPTH },
    ORCHARD_SHARD_HEIGHT,
>;

fn map_shardtree_error(context: &str, err: ShardTreeError<ShardStoreError>) -> Error {
    Error::Storage(format!("{}: {}", context, err))
}

fn checkpoint_depth_at_or_below(
    conn: &rusqlite::Connection,
    table_prefix: &'static str,
    anchor_height: u64,
) -> Result<Option<(u32, usize)>> {
    if anchor_height == 0 {
        return Ok(None);
    }
    let anchor_u32 = u32::try_from(anchor_height)
        .map_err(|_| Error::Storage(format!("anchor_height {} exceeds u32::MAX", anchor_height)))?;
    let checkpoint_id: Option<u32> = conn
        .query_row(
            &format!(
                "SELECT MAX(checkpoint_id) FROM {}_tree_checkpoints WHERE checkpoint_id <= ?1",
                table_prefix
            ),
            [anchor_u32],
            |row| row.get(0),
        )
        .optional()?
        .flatten();

    let Some(checkpoint_id) = checkpoint_id else {
        return Ok(None);
    };

    // In shardtree 0.1:
    // - `checkpoint_depth = 0` means "latest tree state" (tip), not "latest checkpoint".
    // - `checkpoint_depth = 1` means "latest checkpoint", then 2, 3, ... for older checkpoints.
    //
    // For a selected checkpoint id, depth must therefore be:
    //   1 + count(checkpoints strictly newer than selected checkpoint).
    let depth: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM {}_tree_checkpoints WHERE checkpoint_id > ?1",
            table_prefix
        ),
        [checkpoint_id],
        |row| row.get(0),
    )?;

    let checkpoint_depth = usize::try_from(depth + 1).map_err(|_| {
        Error::Storage(format!(
            "checkpoint depth {} out of range for {}",
            depth + 1,
            table_prefix
        ))
    })?;

    Ok(Some((checkpoint_id, checkpoint_depth)))
}

fn sapling_witness_for_position(
    tree: &mut SaplingTree<'_>,
    position: u64,
    checkpoint_depth: usize,
) -> Result<Option<incrementalmerkletree::MerklePath<SaplingNode, NOTE_COMMITMENT_TREE_DEPTH>>> {
    match tree.witness_caching(Position::from(position), checkpoint_depth) {
        Ok(path) => Ok(Some(path)),
        Err(ShardTreeError::Query(_)) => Ok(None),
        Err(err) => Err(map_shardtree_error(
            "Failed to compute Sapling witness from shardtree",
            err,
        )),
    }
}

fn sapling_anchor_root_at_depth(
    tree: &mut SaplingTree<'_>,
    checkpoint_depth: usize,
) -> Result<Option<SaplingNode>> {
    match tree.root_at_checkpoint_caching(checkpoint_depth) {
        Ok(root) => Ok(Some(root)),
        Err(ShardTreeError::Query(_)) => Ok(None),
        Err(err) => Err(map_shardtree_error(
            "Failed to compute Sapling root at checkpoint from shardtree",
            err,
        )),
    }
}

fn orchard_witness_for_position(
    tree: &mut OrchardTree<'_>,
    position: u64,
    checkpoint_depth: usize,
) -> Result<Option<OrchardMerklePath>> {
    let path = match tree.witness_caching(Position::from(position), checkpoint_depth) {
        Ok(path) => path,
        Err(ShardTreeError::Query(_)) => return Ok(None),
        Err(err) => {
            return Err(map_shardtree_error(
                "Failed to compute Orchard witness from shardtree",
                err,
            ));
        }
    };

    let auth_path: [MerkleHashOrchard; NOTE_COMMITMENT_TREE_DEPTH as usize] = path
        .path_elems()
        .to_vec()
        .try_into()
        .map_err(|v: Vec<MerkleHashOrchard>| {
            Error::Storage(format!(
                "Unexpected Orchard auth path depth from shardtree: {}",
                v.len()
            ))
        })?;
    let position_u64: u64 = path.position().into();
    let position_u32 = u32::try_from(position_u64).map_err(|_| {
        Error::Storage(format!(
            "Orchard witness position {} exceeds u32::MAX",
            position_u64
        ))
    })?;
    Ok(Some(OrchardMerklePath::from_parts(position_u32, auth_path)))
}

fn orchard_anchor_at_depth(
    tree: &mut OrchardTree<'_>,
    checkpoint_depth: usize,
) -> Result<Option<orchard::tree::Anchor>> {
    let root = match tree.root_at_checkpoint_caching(checkpoint_depth) {
        Ok(root) => root,
        Err(ShardTreeError::Query(_)) => return Ok(None),
        Err(err) => {
            return Err(map_shardtree_error(
                "Failed to compute Orchard root at checkpoint from shardtree",
                err,
            ));
        }
    };

    Ok(orchard::tree::Anchor::from_bytes(root.to_bytes()).into())
}

pub(crate) fn resolve_orchard_anchor_from_db_state(
    db: &Database,
    anchor_height: u64,
) -> Result<Option<orchard::tree::Anchor>> {
    let conn = db.conn();
    let Some((_checkpoint_id, checkpoint_depth)) =
        checkpoint_depth_at_or_below(conn, ORCHARD_TABLE_PREFIX, anchor_height)?
    else {
        return Ok(None);
    };

    let store = SqliteShardStore::<_, MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>::from_connection(
        conn,
        ORCHARD_TABLE_PREFIX,
    )?;
    let mut tree: OrchardTree<'_> = ShardTree::new(store, SHARDTREE_PRUNING_DEPTH);
    orchard_anchor_at_depth(&mut tree, checkpoint_depth)
}

pub(crate) fn resolve_sapling_root_from_db_state(
    db: &Database,
    anchor_height: u64,
) -> Result<Option<SaplingNode>> {
    let conn = db.conn();
    let Some((_checkpoint_id, checkpoint_depth)) =
        checkpoint_depth_at_or_below(conn, SAPLING_TABLE_PREFIX, anchor_height)?
    else {
        return Ok(None);
    };

    let store = SqliteShardStore::<_, SaplingNode, SAPLING_SHARD_HEIGHT>::from_connection(
        conn,
        SAPLING_TABLE_PREFIX,
    )?;
    let mut tree: SaplingTree<'_> = ShardTree::new(store, SHARDTREE_PRUNING_DEPTH);
    sapling_anchor_root_at_depth(&mut tree, checkpoint_depth)
}

pub(crate) fn construct_anchor_witnesses_from_db_state(
    db: &Database,
    anchor_height: u64,
    notes: Vec<SelectableNote>,
) -> Result<Vec<SelectableNote>> {
    if notes.is_empty() || anchor_height == 0 {
        return Ok(Vec::new());
    }

    let conn = db.conn();
    let sapling_checkpoint =
        checkpoint_depth_at_or_below(conn, SAPLING_TABLE_PREFIX, anchor_height)?;
    let orchard_checkpoint =
        checkpoint_depth_at_or_below(conn, ORCHARD_TABLE_PREFIX, anchor_height)?;

    let mut sapling_tree = if sapling_checkpoint.is_some() {
        let store = SqliteShardStore::<_, SaplingNode, SAPLING_SHARD_HEIGHT>::from_connection(
            conn,
            SAPLING_TABLE_PREFIX,
        )?;
        Some(ShardTree::new(store, SHARDTREE_PRUNING_DEPTH))
    } else {
        None
    };
    let mut orchard_tree = if orchard_checkpoint.is_some() {
        let store =
            SqliteShardStore::<_, MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>::from_connection(
                conn,
                ORCHARD_TABLE_PREFIX,
            )?;
        Some(ShardTree::new(store, SHARDTREE_PRUNING_DEPTH))
    } else {
        None
    };

    let orchard_anchor =
        if let (Some(tree), Some((_, depth))) = (&mut orchard_tree, orchard_checkpoint) {
            orchard_anchor_at_depth(tree, depth)?
        } else {
            None
        };

    let sapling_anchor_root =
        if let (Some(tree), Some((_, depth))) = (&mut sapling_tree, sapling_checkpoint) {
            sapling_anchor_root_at_depth(tree, depth)?
        } else {
            None
        };

    let mut ready_notes = Vec::with_capacity(notes.len());
    let mut missing_notes = Vec::new();

    for mut note in notes {
        let hydrated = match note.note_type {
            SelectableNoteType::Sapling => {
                if note.diversifier.is_none() || note.note.is_none() {
                    false
                } else if note.merkle_path.is_some() {
                    true
                } else if let (Some(position), Some(tree), Some((_, depth))) = (
                    note.sapling_position,
                    sapling_tree.as_mut(),
                    sapling_checkpoint,
                ) {
                    if let Some(witness) = sapling_witness_for_position(tree, position, depth)? {
                        // Validate the witness against the anchor root for this checkpoint to
                        // avoid constructing spends that will be rejected as "unknown-anchor".
                        let leaf = if note.commitment.len() == 32 {
                            let mut cmu = [0u8; 32];
                            cmu.copy_from_slice(&note.commitment);
                            let cmu: Option<zcash_primitives::sapling::note::ExtractedNoteCommitment> =
                                zcash_primitives::sapling::note::ExtractedNoteCommitment::from_bytes(&cmu)
                                    .into();
                            cmu.map(|cmu| SaplingNode::from_cmu(&cmu))
                        } else {
                            None
                        };

                        match (leaf, sapling_anchor_root.as_ref()) {
                            (Some(leaf), Some(root)) => {
                                let path_root = witness.root(leaf);
                                if path_root.to_bytes() != root.to_bytes() {
                                    false
                                } else {
                                    note.merkle_path = Some(witness);
                                    true
                                }
                            }
                            _ => false,
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            SelectableNoteType::Orchard => {
                match (note.orchard_position, note.orchard_note.as_ref()) {
                    (Some(position), Some(orchard_note)) => {
                        if note.orchard_merkle_path.is_some() && note.orchard_anchor.is_some() {
                            true
                        } else if let (Some(tree), Some((_, depth)), Some(anchor)) =
                            (orchard_tree.as_mut(), orchard_checkpoint, orchard_anchor)
                        {
                            if let Some(merkle_path) =
                                orchard_witness_for_position(tree, position, depth)?
                            {
                                // Keep root consistency check explicit to fail closed on malformed state.
                                let leaf =
                                    OrchardExtractedNoteCommitment::from(orchard_note.commitment());
                                let path_root = merkle_path.root(leaf);
                                if path_root.to_bytes() != anchor.to_bytes() {
                                    false
                                } else {
                                    note.orchard_merkle_path = Some(merkle_path);
                                    note.orchard_anchor = Some(anchor);
                                    true
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                    _ => false,
                }
            }
        };

        if hydrated {
            ready_notes.push(note);
        } else {
            missing_notes.push(note);
        }
    }

    if !missing_notes.is_empty() {
        let missing_sapling = missing_notes
            .iter()
            .filter(|note| note.note_type == SelectableNoteType::Sapling)
            .count();
        let missing_orchard = missing_notes.len().saturating_sub(missing_sapling);
        let sapling_checkpoint_id = sapling_checkpoint.map(|(id, _)| id).unwrap_or(0);
        let orchard_checkpoint_id = orchard_checkpoint.map(|(id, _)| id).unwrap_or(0);
        tracing::debug!(
            "Anchor witness construction incomplete at anchor {} using shardtree checkpoints sapling={} orchard={}: ready={}, missing_sapling={}, missing_orchard={}",
            anchor_height,
            sapling_checkpoint_id,
            orchard_checkpoint_id,
            ready_notes.len(),
            missing_sapling,
            missing_orchard
        );
    }

    Ok(ready_notes)
}

#[cfg(test)]
mod tests {
    use super::checkpoint_depth_at_or_below;

    #[test]
    fn checkpoint_depth_matches_shardtree_checkpoint_semantics() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        conn.execute(
            "CREATE TABLE sapling_tree_checkpoints (checkpoint_id INTEGER PRIMARY KEY)",
            [],
        )
        .unwrap();

        // Checkpoints are monotonic in block height.
        for id in [100u32, 101u32, 105u32, 110u32] {
            conn.execute(
                "INSERT INTO sapling_tree_checkpoints (checkpoint_id) VALUES (?1)",
                [id],
            )
            .unwrap();
        }

        // Latest checkpoint is depth 1 (depth 0 is latest tree state in shardtree 0.1).
        let (_, depth) = checkpoint_depth_at_or_below(&conn, "sapling", 200)
            .unwrap()
            .expect("checkpoint");
        assert_eq!(depth, 1);

        // Exact checkpoint depth is 1 + number of strictly newer checkpoints.
        let (_, depth) = checkpoint_depth_at_or_below(&conn, "sapling", 105)
            .unwrap()
            .expect("checkpoint");
        // Newer checkpoints: 110
        assert_eq!(depth, 2);

        // Anchor between checkpoints selects MAX <= anchor.
        let (_, depth) = checkpoint_depth_at_or_below(&conn, "sapling", 109)
            .unwrap()
            .expect("checkpoint");
        assert_eq!(depth, 2);

        // Oldest checkpoint has depth 1 + count(newer checkpoints).
        let (_, depth) = checkpoint_depth_at_or_below(&conn, "sapling", 100)
            .unwrap()
            .expect("checkpoint");
        assert_eq!(depth, 4);
    }
}
