//! Spendability state storage.
//!
//! Tracks wallet-level anchor/witness readiness used by the send path.

use crate::{
    scan_queue::{ScanQueueStorage, SCAN_PRIORITY_HISTORIC},
    Database, Error, Result,
};
use rusqlite::{params, OptionalExtension};

/// Wallet spendability state persisted in SQLite.
#[derive(Debug, Clone)]
pub struct SpendabilityStateRow {
    /// Whether the wallet can spend at the current validated anchor epoch.
    pub spendable: bool,
    /// Whether a full rescan is required before spending.
    pub rescan_required: bool,
    /// Earliest height a verified imported key still requires replay from.
    ///
    /// Zero means there is no imported-key replay obligation.
    pub required_rescan_from_height: u64,
    /// Monotonic generation for verified spending-key imports.
    pub key_import_generation: u64,
    /// Latest target height known to the wallet when state was saved.
    pub target_height: u64,
    /// Latest anchor height observed by sync.
    pub anchor_height: u64,
    /// Anchor height validated for spending.
    pub validated_anchor_height: u64,
    /// Whether a repair/rescan has been queued.
    pub repair_queued: bool,
    /// Earliest height requested for queued repair.
    pub repair_from_height: u64,
    /// Deterministic reason code exposed over FFI.
    pub reason_code: String,
    /// Last update timestamp (ISO 8601).
    pub updated_at: String,
}

/// Canonical target height with independently snapped Sapling and Ironwood anchors.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PerPoolAnchorHeights {
    /// Latest target height derived from scan queue extrema.
    pub target_height: u64,
    /// Sapling anchor snapped to the highest Sapling checkpoint at-or-below the ideal anchor.
    pub sapling_anchor_height: u64,
    /// Ironwood anchor snapped to the highest Ironwood checkpoint at-or-below the ideal anchor.
    pub ironwood_anchor_height: u64,
    /// Conservative anchor equal to `min(sapling_anchor_height, ironwood_anchor_height)`.
    pub conservative_anchor_height: u64,
}

impl Default for SpendabilityStateRow {
    fn default() -> Self {
        Self {
            spendable: false,
            rescan_required: true,
            required_rescan_from_height: 0,
            key_import_generation: 0,
            target_height: 0,
            anchor_height: 0,
            validated_anchor_height: 0,
            repair_queued: false,
            repair_from_height: 0,
            reason_code: "ERR_RESCAN_REQUIRED".to_string(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Spendability-state storage operations.
pub struct SpendabilityStateStorage<'a> {
    db: &'a Database,
}

impl<'a> SpendabilityStateStorage<'a> {
    fn birthday_height_for_account(&self, account_id: i64) -> Result<u64> {
        let birthday_i64: Option<i64> = self.db.conn().query_row(
            r#"
            SELECT MIN(birthday_height)
            FROM account_keys
            WHERE account_id = ?1 AND birthday_height > 0
            "#,
            params![account_id],
            |row| row.get(0),
        )?;
        match birthday_i64 {
            Some(value) => u64::try_from(value)
                .map_err(|_| Error::Storage(format!("birthday_height out of range: {}", value))),
            None => Ok(0),
        }
    }

    /// Create a new storage wrapper.
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Load current spendability state.
    pub fn load_state(&self) -> Result<SpendabilityStateRow> {
        let row = self
            .db
            .conn()
            .query_row(
                r#"
                SELECT
                    spendable,
                    rescan_required,
                    required_rescan_from_height,
                    key_import_generation,
                    target_height,
                    anchor_height,
                    validated_anchor_height,
                    repair_queued,
                    repair_from_height,
                    reason_code,
                    updated_at
                FROM spendability_state
                WHERE id = 1
                "#,
                [],
                |row| {
                    let required_rescan_from_height_i64: i64 = row.get(2)?;
                    let key_import_generation_i64: i64 = row.get(3)?;
                    let target_height_i64: i64 = row.get(4)?;
                    let anchor_height_i64: i64 = row.get(5)?;
                    let validated_anchor_height_i64: i64 = row.get(6)?;
                    let repair_from_height_i64: i64 = row.get(8)?;
                    Ok(SpendabilityStateRow {
                        spendable: row.get::<_, i64>(0)? != 0,
                        rescan_required: row.get::<_, i64>(1)? != 0,
                        required_rescan_from_height: u64::try_from(required_rescan_from_height_i64)
                            .map_err(|_| {
                                rusqlite::Error::IntegralValueOutOfRange(
                                    2,
                                    required_rescan_from_height_i64,
                                )
                            })?,
                        key_import_generation: u64::try_from(key_import_generation_i64).map_err(
                            |_| {
                                rusqlite::Error::IntegralValueOutOfRange(
                                    3,
                                    key_import_generation_i64,
                                )
                            },
                        )?,
                        target_height: u64::try_from(target_height_i64).map_err(|_| {
                            rusqlite::Error::IntegralValueOutOfRange(4, target_height_i64)
                        })?,
                        anchor_height: u64::try_from(anchor_height_i64).map_err(|_| {
                            rusqlite::Error::IntegralValueOutOfRange(5, anchor_height_i64)
                        })?,
                        validated_anchor_height: u64::try_from(validated_anchor_height_i64)
                            .map_err(|_| {
                                rusqlite::Error::IntegralValueOutOfRange(
                                    6,
                                    validated_anchor_height_i64,
                                )
                            })?,
                        repair_queued: row.get::<_, i64>(7)? != 0,
                        repair_from_height: u64::try_from(repair_from_height_i64).map_err(
                            |_| rusqlite::Error::IntegralValueOutOfRange(8, repair_from_height_i64),
                        )?,
                        reason_code: row.get(9)?,
                        updated_at: row.get(10)?,
                    })
                },
            )
            .optional()?;

        Ok(row.unwrap_or_default())
    }

    /// Returns the minimum and maximum chain heights considered scannable.
    ///
    /// Uses queue extrema directly for canonical height derivation.
    pub fn scan_queue_extrema(&self) -> Result<Option<(u64, u64)>> {
        let queue_row = self
            .db
            .conn()
            .query_row(
                r#"
                SELECT MIN(range_start), MAX(range_end)
                FROM scan_queue
                WHERE priority = ?1
                "#,
                params![SCAN_PRIORITY_HISTORIC],
                |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .optional()?;

        if let Some((Some(range_start_i64), Some(range_end_i64))) = queue_row {
            let range_start = u64::try_from(range_start_i64).map_err(|_| {
                Error::Storage(format!(
                    "scan_queue.range_start out of range: {}",
                    range_start_i64
                ))
            })?;
            let range_end = u64::try_from(range_end_i64).map_err(|_| {
                Error::Storage(format!(
                    "scan_queue.range_end out of range: {}",
                    range_end_i64
                ))
            })?;
            if range_end > range_start {
                return Ok(Some((range_start.max(1), range_end.saturating_sub(1))));
            }
        }

        Ok(None)
    }

    /// Compute canonical target/anchor heights.
    ///
    /// Derivation model:
    /// - target = max_scannable_height + 1
    /// - anchor = latest checkpoint <= target - min_confirmations, per pool
    /// - if both pools have checkpoints, use the lower (more conservative) height
    pub fn get_target_and_anchor_heights(
        &self,
        min_confirmations: u32,
    ) -> Result<Option<(u64, u64)>> {
        self.get_target_and_anchor_heights_for_account_opt(min_confirmations, None)
    }

    /// Compute canonical target/anchor heights for a specific account.
    ///
    /// Account birthday is used as a floor to prevent stale pre-birthday
    /// checkpoints from pinning spendability.
    pub fn get_target_and_anchor_heights_for_account(
        &self,
        min_confirmations: u32,
        account_id: i64,
    ) -> Result<Option<(u64, u64)>> {
        self.get_target_and_anchor_heights_for_account_opt(min_confirmations, Some(account_id))
    }

    /// Compute canonical target height plus per-pool snapped anchors for an account.
    pub fn get_target_and_anchor_heights_by_pool_for_account(
        &self,
        min_confirmations: u32,
        account_id: i64,
    ) -> Result<Option<PerPoolAnchorHeights>> {
        let min_confirmations = min_confirmations.max(1) as u64;
        let Some((min_height, max_height)) = self.scan_queue_extrema()? else {
            return Ok(None);
        };
        let target_height = max_height.saturating_add(1);
        let anchor_floor = min_height
            .max(1)
            .max(self.birthday_height_for_account(account_id)?.max(1));
        let ideal_anchor = target_height
            .saturating_sub(min_confirmations)
            .max(anchor_floor);
        if ideal_anchor > target_height {
            return Ok(None);
        }

        let sapling_anchor_height = self
            .snap_to_checkpoint_for_table("sapling_tree_checkpoints", ideal_anchor, anchor_floor)?
            .unwrap_or(ideal_anchor);
        let ironwood_anchor_height = self
            .snap_to_checkpoint_for_table("orchard_tree_checkpoints", ideal_anchor, anchor_floor)?
            .unwrap_or(ideal_anchor);
        let conservative_anchor_height = sapling_anchor_height.min(ironwood_anchor_height);

        Ok(Some(PerPoolAnchorHeights {
            target_height,
            sapling_anchor_height,
            ironwood_anchor_height,
            conservative_anchor_height,
        }))
    }

    fn get_target_and_anchor_heights_for_account_opt(
        &self,
        min_confirmations: u32,
        account_id: Option<i64>,
    ) -> Result<Option<(u64, u64)>> {
        let min_confirmations = min_confirmations.max(1) as u64;
        let Some((min_height, max_height)) = self.scan_queue_extrema()? else {
            return Ok(None);
        };

        let target_height = max_height.saturating_add(1);
        let mut anchor_floor = min_height.max(1);
        if let Some(account_id) = account_id {
            anchor_floor = anchor_floor.max(self.birthday_height_for_account(account_id)?.max(1));
        }
        let ideal_anchor = target_height
            .saturating_sub(min_confirmations)
            .max(anchor_floor);

        if ideal_anchor > target_height {
            return Ok(None);
        }

        // Snap the anchor to an actual ShardTree checkpoint so that witness/root
        // computation uses a real tree state rather than falling back to the nearest
        // older checkpoint (which produces a different root and causes
        // "unknown-anchor" rejections at broadcast).
        //
        // `root_at_checkpoint_id(height)` requires exact checkpoint presence.
        // We find the maximum checkpoint <= ideal_anchor for each pool and use
        // the more conservative (lower) one.
        let anchor_height = self
            .snap_to_checkpoint(ideal_anchor, anchor_floor)?
            .unwrap_or(ideal_anchor);

        if anchor_height > target_height {
            return Ok(None);
        }

        Ok(Some((target_height, anchor_height)))
    }

    /// Find the highest ShardTree checkpoint at-or-below `ceiling` that is >= `floor`.
    ///
    /// Queries both Sapling and Ironwood checkpoint tables and returns the more
    /// conservative (lower) of the two, ensuring both pools can produce valid
    /// witnesses at the returned height.
    fn snap_to_checkpoint(&self, ceiling: u64, floor: u64) -> Result<Option<u64>> {
        let sapling_max =
            self.snap_to_checkpoint_for_table("sapling_tree_checkpoints", ceiling, floor)?;
        let orchard_max =
            self.snap_to_checkpoint_for_table("orchard_tree_checkpoints", ceiling, floor)?;

        let snapped = match (sapling_max, orchard_max) {
            (Some(s), Some(o)) => Some(s.min(o)),
            (Some(s), None) => Some(s),
            (None, Some(o)) => Some(o),
            (None, None) => None,
        };

        Ok(snapped)
    }

    fn snap_to_checkpoint_for_table(
        &self,
        table_name: &str,
        ceiling: u64,
        floor: u64,
    ) -> Result<Option<u64>> {
        let ceiling_u32 = match u32::try_from(ceiling) {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };
        let floor_u32 = u32::try_from(floor).unwrap_or(0);
        let sql = format!(
            "SELECT MAX(checkpoint_id) FROM {table_name} WHERE checkpoint_id <= ?1 AND checkpoint_id >= ?2"
        );
        self.db
            .conn()
            .query_row(&sql, params![ceiling_u32, floor_u32], |row| {
                row.get::<_, Option<u32>>(0)
            })
            .map(|value| value.map(u64::from))
            .map_err(|e| Error::Storage(format!("{table_name} checkpoint query: {}", e)))
    }

    /// Persist the full state.
    pub fn save_state(&self, state: &SpendabilityStateRow) -> Result<()> {
        let target_height = to_sql_i64(state.target_height)?;
        let anchor_height = to_sql_i64(state.anchor_height)?;
        let validated_anchor_height = to_sql_i64(state.validated_anchor_height)?;
        let repair_from_height = to_sql_i64(state.repair_from_height)?;
        let required_rescan_from_height = to_sql_i64(state.required_rescan_from_height)?;
        let key_import_generation = to_sql_i64(state.key_import_generation)?;
        self.db.conn().execute(
            r#"
            UPDATE spendability_state SET
                spendable = ?1,
                rescan_required = ?2,
                required_rescan_from_height = ?3,
                key_import_generation = ?4,
                target_height = ?5,
                anchor_height = ?6,
                validated_anchor_height = ?7,
                repair_queued = ?8,
                repair_from_height = ?9,
                reason_code = ?10,
                updated_at = ?11
            WHERE id = 1
            "#,
            params![
                bool_to_int(state.spendable),
                bool_to_int(state.rescan_required),
                required_rescan_from_height,
                key_import_generation,
                target_height,
                anchor_height,
                validated_anchor_height,
                bool_to_int(state.repair_queued),
                repair_from_height,
                state.reason_code,
                chrono::Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Record a chain height learned at sync startup without weakening any
    /// existing rescan, imported-key replay, or witness-repair obligation.
    /// Existing non-zero heights are monotonic; a fresh zero-height state also
    /// receives an initial anchor so current-tip wallets retain the known tip
    /// even when there are no blocks to scan.
    pub fn record_known_sync_height(&self, height: u64) -> Result<()> {
        if height == 0 {
            return Ok(());
        }
        let height = to_sql_i64(height)?;
        self.db.conn().execute(
            r#"
            UPDATE spendability_state SET
                target_height = CASE
                    WHEN target_height > ?1 THEN target_height
                    ELSE ?1
                END,
                anchor_height = CASE
                    WHEN anchor_height = 0 THEN ?1
                    ELSE anchor_height
                END,
                updated_at = ?2
            WHERE id = 1 AND spendable = 0
            "#,
            params![height, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Atomically establish the durable rescan gate before wallet state is
    /// rewound. Imported-key replay obligations and their generation are
    /// preserved; an explicit rescan supersedes any ordinary repair request.
    /// The known target is monotonic so controller restarts cannot turn a
    /// current chain tip into the lower replay floor.
    pub fn begin_rescan(
        &self,
        target_height: u64,
        anchor_height: u64,
        reason_code: &str,
    ) -> Result<()> {
        let target_height = to_sql_i64(target_height)?;
        let anchor_height = to_sql_i64(anchor_height)?;
        self.db.conn().execute(
            r#"
            UPDATE spendability_state SET
                spendable = 0,
                rescan_required = 1,
                target_height = CASE
                    WHEN target_height > ?1 THEN target_height
                    ELSE ?1
                END,
                anchor_height = ?2,
                repair_queued = 0,
                repair_from_height = 0,
                reason_code = CASE
                    WHEN required_rescan_from_height > 0 THEN reason_code
                    ELSE ?3
                END,
                updated_at = ?4
            WHERE id = 1
            "#,
            params![
                target_height,
                anchor_height,
                reason_code,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    /// Record an interrupted sync without erasing its last known heights or
    /// any stronger replay/repair gate.
    pub fn mark_sync_interrupted(&self) -> Result<()> {
        self.db.conn().execute(
            r#"
            UPDATE spendability_state SET
                spendable = 0,
                reason_code = CASE
                    WHEN rescan_required = 0
                     AND required_rescan_from_height = 0
                     AND repair_queued = 0
                    THEN 'ERR_SYNC_FINALIZING'
                    ELSE reason_code
                END,
                updated_at = ?1
            WHERE id = 1
            "#,
            params![chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Mark state as requiring a full rescan.
    pub fn mark_rescan_required(&self, reason_code: &str) -> Result<()> {
        self.db.conn().execute(
            r#"
            UPDATE spendability_state SET
                spendable = 0,
                rescan_required = 1,
                repair_queued = 0,
                repair_from_height = 0,
                reason_code = ?1,
                updated_at = ?2
            WHERE id = 1
            "#,
            params![reason_code, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Complete a verified-key replay when it covered the durable floor for the
    /// same import generation.
    ///
    /// Returns `false` without changing the gate when a newer import arrived or
    /// the replay began above the required height.
    pub fn complete_required_rescan(
        &self,
        expected_generation: u64,
        replayed_from_height: u64,
    ) -> Result<bool> {
        let expected_generation = to_sql_i64(expected_generation)?;
        let replayed_from_height = to_sql_i64(replayed_from_height)?;
        let changed = self.db.conn().execute(
            r#"
            UPDATE spendability_state SET
                spendable = CASE
                    WHEN repair_queued = 0
                         AND anchor_height > 0
                         AND validated_anchor_height >= anchor_height
                    THEN 1
                    ELSE 0
                END,
                rescan_required = 0,
                required_rescan_from_height = 0,
                reason_code = CASE
                    WHEN repair_queued != 0 THEN 'ERR_WITNESS_REPAIR_QUEUED'
                    WHEN anchor_height > 0 AND validated_anchor_height >= anchor_height THEN 'OK'
                    ELSE 'ERR_SYNC_FINALIZING'
                END,
                updated_at = ?3
            WHERE id = 1
              AND rescan_required != 0
              AND required_rescan_from_height > 0
              AND key_import_generation = ?1
              AND ?2 <= required_rescan_from_height
            "#,
            params![
                expected_generation,
                replayed_from_height,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        Ok(changed == 1)
    }

    /// Mark state as sync-finalizing.
    ///
    /// A verified-key replay obligation is stronger than this ordinary sync
    /// transition and remains gated until an eligible rescan completes it.
    pub fn mark_sync_finalizing(&self, target_height: u64, anchor_height: u64) -> Result<()> {
        let target_height = to_sql_i64(target_height)?;
        let anchor_height = to_sql_i64(anchor_height)?;
        self.db.conn().execute(
            r#"
            UPDATE spendability_state SET
                spendable = 0,
                rescan_required = CASE
                    WHEN required_rescan_from_height > 0 THEN 1
                    ELSE 0
                END,
                target_height = ?1,
                anchor_height = ?2,
                reason_code = CASE
                    WHEN required_rescan_from_height > 0 THEN reason_code
                    WHEN repair_queued != 0 THEN 'ERR_WITNESS_REPAIR_QUEUED'
                    ELSE 'ERR_SYNC_FINALIZING'
                END,
                updated_at = ?3
            WHERE id = 1
            "#,
            params![
                target_height,
                anchor_height,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    /// Record a validated anchor and finish any durable witness repair.
    ///
    /// This is the only ordinary sync transition that clears `repair_queued`:
    /// reaching the finalization phase alone does not prove that the rebuilt
    /// witnesses and selected anchor are valid for spending.
    pub fn mark_validated(&self, target_height: u64, anchor_height: u64) -> Result<()> {
        let target_height = to_sql_i64(target_height)?;
        let anchor_height = to_sql_i64(anchor_height)?;
        self.db.conn().execute(
            r#"
            UPDATE spendability_state SET
                spendable = CASE
                    WHEN required_rescan_from_height > 0 THEN 0
                    ELSE 1
                END,
                rescan_required = CASE
                    WHEN required_rescan_from_height > 0 THEN 1
                    ELSE 0
                END,
                target_height = ?1,
                anchor_height = ?2,
                validated_anchor_height = ?2,
                repair_queued = 0,
                repair_from_height = 0,
                reason_code = CASE
                    WHEN required_rescan_from_height > 0 THEN reason_code
                    ELSE 'OK'
                END,
                updated_at = ?3
            WHERE id = 1
            "#,
            params![
                target_height,
                anchor_height,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    /// Queue a witness repair/rescan request.
    pub fn queue_repair(&self, from_height: u64, reason_code: &str) -> Result<()> {
        let queue_start = from_height.max(1);
        let previous_state = self.load_state().unwrap_or_default();
        let queue_extrema_end = self
            .scan_queue_extrema()?
            .map(|(_, max_height)| max_height.saturating_add(1))
            .unwrap_or(0);
        let queue_end = previous_state
            .target_height
            .max(previous_state.anchor_height)
            .saturating_add(1)
            .max(queue_extrema_end)
            .max(queue_start.saturating_add(1));
        self.queue_repair_range(queue_start, queue_end, reason_code)
    }

    /// Queue a witness repair over an explicit range.
    ///
    /// `range_end_exclusive` is exclusive.
    pub fn queue_repair_range(
        &self,
        from_height: u64,
        range_end_exclusive: u64,
        reason_code: &str,
    ) -> Result<()> {
        let queue_start = from_height.max(1);
        let queue_end = range_end_exclusive.max(queue_start.saturating_add(1));
        let from_height = to_sql_i64(queue_start)?;
        self.db.conn().execute(
            r#"
            UPDATE spendability_state SET
                spendable = 0,
                rescan_required = CASE
                    WHEN required_rescan_from_height > 0 THEN 1
                    ELSE 0
                END,
                repair_queued = 1,
                repair_from_height = CASE
                    WHEN repair_from_height > 0 AND repair_from_height < ?1 THEN repair_from_height
                    ELSE ?1
                END,
                reason_code = CASE
                    WHEN required_rescan_from_height > 0 THEN reason_code
                    ELSE ?2
                END,
                updated_at = ?3
            WHERE id = 1
            "#,
            params![from_height, reason_code, chrono::Utc::now().to_rfc3339()],
        )?;
        let scan_queue = ScanQueueStorage::new(self.db);
        scan_queue.queue_found_note_range(queue_start, queue_end, Some(reason_code))?;
        Ok(())
    }

    /// Mark repair as queued in spendability state without mutating scan queue rows.
    ///
    /// Use this when queue work was already enqueued by another path and we only need
    /// state gating (`ERR_WITNESS_REPAIR_QUEUED`) to remain deterministic.
    pub fn mark_repair_pending_without_enqueue(
        &self,
        from_height: u64,
        reason_code: &str,
    ) -> Result<()> {
        let from_height = to_sql_i64(from_height.max(1))?;
        self.db.conn().execute(
            r#"
            UPDATE spendability_state SET
                spendable = 0,
                rescan_required = CASE
                    WHEN required_rescan_from_height > 0 THEN 1
                    ELSE 0
                END,
                repair_queued = 1,
                repair_from_height = CASE
                    WHEN repair_from_height > 0 AND repair_from_height < ?1 THEN repair_from_height
                    ELSE ?1
                END,
                reason_code = CASE
                    WHEN required_rescan_from_height > 0 THEN reason_code
                    ELSE ?2
                END,
                updated_at = ?3
            WHERE id = 1
            "#,
            params![from_height, reason_code, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }
}

fn bool_to_int(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn to_sql_i64(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::Storage(format!("value {} exceeds i64::MAX", value)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        encryption::EncryptionKey,
        security::{EncryptionAlgorithm, MasterKey},
    };
    use tempfile::NamedTempFile;

    fn test_db() -> Database {
        let file = NamedTempFile::new().unwrap();
        let salt = crate::security::generate_salt();
        let key = EncryptionKey::from_passphrase("test", &salt).unwrap();
        let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
        Database::open(file.path(), &key, master_key).unwrap()
    }

    #[test]
    fn known_sync_height_initializes_fresh_target_and_anchor() {
        let db = test_db();
        let storage = SpendabilityStateStorage::new(&db);

        storage.record_known_sync_height(152_849).unwrap();
        let current = storage.load_state().unwrap();
        assert_eq!(current.target_height, 152_849);
        assert_eq!(current.anchor_height, 152_849);
        assert!(current.rescan_required);
        assert_eq!(current.reason_code, "ERR_RESCAN_REQUIRED");
    }

    #[test]
    fn known_sync_height_initializes_missing_anchor_without_lowering_target() {
        let db = test_db();
        let storage = SpendabilityStateStorage::new(&db);
        let mut state = storage.load_state().unwrap();
        state.target_height = 152_860;
        state.anchor_height = 0;
        storage.save_state(&state).unwrap();

        storage.record_known_sync_height(152_849).unwrap();
        let current = storage.load_state().unwrap();
        assert_eq!(current.target_height, 152_860);
        assert_eq!(current.anchor_height, 152_849);
    }

    #[test]
    fn known_sync_height_is_monotonic_and_preserves_independent_obligations() {
        let db = test_db();
        let storage = SpendabilityStateStorage::new(&db);
        let mut state = storage.load_state().unwrap();
        state.target_height = 152_860;
        state.anchor_height = 152_850;
        state.rescan_required = true;
        state.required_rescan_from_height = 152_855;
        state.key_import_generation = 7;
        state.repair_queued = true;
        state.repair_from_height = 152_856;
        state.reason_code = "ERR_RESCAN_REQUIRED".to_string();
        storage.save_state(&state).unwrap();

        storage.record_known_sync_height(152_849).unwrap();
        let current = storage.load_state().unwrap();
        assert_eq!(current.target_height, 152_860);
        assert_eq!(current.anchor_height, 152_850);
        assert!(current.rescan_required);
        assert_eq!(current.required_rescan_from_height, 152_855);
        assert_eq!(current.key_import_generation, 7);
        assert!(current.repair_queued);
        assert_eq!(current.repair_from_height, 152_856);
        assert_eq!(current.reason_code, "ERR_RESCAN_REQUIRED");

        storage.record_known_sync_height(152_861).unwrap();
        let raised = storage.load_state().unwrap();
        assert_eq!(raised.target_height, 152_861);
        assert_eq!(raised.anchor_height, 152_850);
        assert_eq!(raised.required_rescan_from_height, 152_855);
        assert_eq!(raised.key_import_generation, 7);
        assert!(raised.repair_queued);
    }

    #[test]
    fn begin_rescan_is_atomic_and_preserves_import_replay_obligation() {
        let db = test_db();
        let storage = SpendabilityStateStorage::new(&db);
        let mut state = storage.load_state().unwrap();
        state.target_height = 152_860;
        state.anchor_height = 152_860;
        state.rescan_required = true;
        state.required_rescan_from_height = 152_855;
        state.key_import_generation = 3;
        state.repair_queued = true;
        state.repair_from_height = 152_856;
        state.reason_code = "ERR_RESCAN_REQUIRED".to_string();
        storage.save_state(&state).unwrap();

        storage
            .begin_rescan(152_849, 152_849, "ERR_ORDINARY_RESCAN")
            .unwrap();
        let current = storage.load_state().unwrap();
        assert_eq!(current.target_height, 152_860);
        assert_eq!(current.anchor_height, 152_849);
        assert!(current.rescan_required);
        assert_eq!(current.required_rescan_from_height, 152_855);
        assert_eq!(current.key_import_generation, 3);
        assert!(!current.repair_queued);
        assert_eq!(current.repair_from_height, 0);
        assert_eq!(current.reason_code, "ERR_RESCAN_REQUIRED");
    }

    #[test]
    fn begin_rescan_forces_an_ordinary_rescan_gate_and_can_raise_target() {
        let db = test_db();
        let storage = SpendabilityStateStorage::new(&db);
        let mut state = storage.load_state().unwrap();
        state.spendable = true;
        state.rescan_required = false;
        state.target_height = 152_850;
        state.anchor_height = 152_850;
        state.reason_code = "OK".to_string();
        storage.save_state(&state).unwrap();

        storage
            .begin_rescan(152_860, 152_849, "ERR_RESCAN_REQUIRED")
            .unwrap();
        let current = storage.load_state().unwrap();
        assert!(!current.spendable);
        assert!(current.rescan_required);
        assert_eq!(current.target_height, 152_860);
        assert_eq!(current.anchor_height, 152_849);
        assert_eq!(current.reason_code, "ERR_RESCAN_REQUIRED");
    }

    #[test]
    fn sync_interruption_preserves_heights_and_marks_an_ordinary_sync_finalizing() {
        let db = test_db();
        let storage = SpendabilityStateStorage::new(&db);
        let mut state = storage.load_state().unwrap();
        state.spendable = true;
        state.rescan_required = false;
        state.target_height = 152_860;
        state.anchor_height = 152_850;
        state.validated_anchor_height = 152_850;
        state.reason_code = "OK".to_string();
        storage.save_state(&state).unwrap();

        storage.mark_sync_interrupted().unwrap();
        let current = storage.load_state().unwrap();
        assert!(!current.spendable);
        assert!(!current.rescan_required);
        assert_eq!(current.target_height, 152_860);
        assert_eq!(current.anchor_height, 152_850);
        assert_eq!(current.validated_anchor_height, 152_850);
        assert_eq!(current.reason_code, "ERR_SYNC_FINALIZING");
    }

    #[test]
    fn sync_interruption_preserves_stronger_replay_and_repair_obligations() {
        let db = test_db();
        let storage = SpendabilityStateStorage::new(&db);
        let mut state = storage.load_state().unwrap();
        state.spendable = true;
        state.rescan_required = true;
        state.target_height = 152_860;
        state.anchor_height = 152_850;
        state.validated_anchor_height = 152_840;
        state.required_rescan_from_height = 152_855;
        state.key_import_generation = 3;
        state.repair_queued = true;
        state.repair_from_height = 152_845;
        state.reason_code = "ERR_RESCAN_REQUIRED".to_string();
        storage.save_state(&state).unwrap();

        storage.mark_sync_interrupted().unwrap();
        let current = storage.load_state().unwrap();
        assert!(!current.spendable);
        assert!(current.rescan_required);
        assert_eq!(current.target_height, 152_860);
        assert_eq!(current.anchor_height, 152_850);
        assert_eq!(current.validated_anchor_height, 152_840);
        assert_eq!(current.required_rescan_from_height, 152_855);
        assert_eq!(current.key_import_generation, 3);
        assert!(current.repair_queued);
        assert_eq!(current.repair_from_height, 152_845);
        assert_eq!(current.reason_code, "ERR_RESCAN_REQUIRED");
    }
}
