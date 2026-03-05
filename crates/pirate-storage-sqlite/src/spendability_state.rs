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

impl Default for SpendabilityStateRow {
    fn default() -> Self {
        Self {
            spendable: false,
            rescan_required: true,
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
                    let target_height_i64: i64 = row.get(2)?;
                    let anchor_height_i64: i64 = row.get(3)?;
                    let validated_anchor_height_i64: i64 = row.get(4)?;
                    let repair_from_height_i64: i64 = row.get(6)?;
                    Ok(SpendabilityStateRow {
                        spendable: row.get::<_, i64>(0)? != 0,
                        rescan_required: row.get::<_, i64>(1)? != 0,
                        target_height: u64::try_from(target_height_i64).map_err(|_| {
                            rusqlite::Error::IntegralValueOutOfRange(2, target_height_i64)
                        })?,
                        anchor_height: u64::try_from(anchor_height_i64).map_err(|_| {
                            rusqlite::Error::IntegralValueOutOfRange(3, anchor_height_i64)
                        })?,
                        validated_anchor_height: u64::try_from(validated_anchor_height_i64)
                            .map_err(|_| {
                                rusqlite::Error::IntegralValueOutOfRange(
                                    4,
                                    validated_anchor_height_i64,
                                )
                            })?,
                        repair_queued: row.get::<_, i64>(5)? != 0,
                        repair_from_height: u64::try_from(repair_from_height_i64).map_err(
                            |_| rusqlite::Error::IntegralValueOutOfRange(6, repair_from_height_i64),
                        )?,
                        reason_code: row.get(7)?,
                        updated_at: row.get(8)?,
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
    /// Queries both Sapling and Orchard checkpoint tables and returns the more
    /// conservative (lower) of the two, ensuring both pools can produce valid
    /// witnesses at the returned height.
    fn snap_to_checkpoint(&self, ceiling: u64, floor: u64) -> Result<Option<u64>> {
        let ceiling_u32 = match u32::try_from(ceiling) {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };
        let floor_u32 = u32::try_from(floor).unwrap_or(0);

        let sapling_max: Option<u32> = self
            .db
            .conn()
            .query_row(
                "SELECT MAX(checkpoint_id) FROM sapling_tree_checkpoints \
                 WHERE checkpoint_id <= ?1 AND checkpoint_id >= ?2",
                params![ceiling_u32, floor_u32],
                |row| row.get(0),
            )
            .map_err(|e| Error::Storage(format!("sapling checkpoint query: {}", e)))?;

        let orchard_max: Option<u32> = self
            .db
            .conn()
            .query_row(
                "SELECT MAX(checkpoint_id) FROM orchard_tree_checkpoints \
                 WHERE checkpoint_id <= ?1 AND checkpoint_id >= ?2",
                params![ceiling_u32, floor_u32],
                |row| row.get(0),
            )
            .map_err(|e| Error::Storage(format!("orchard checkpoint query: {}", e)))?;

        let snapped = match (sapling_max, orchard_max) {
            (Some(s), Some(o)) => Some(s.min(o)),
            (Some(s), None) => Some(s),
            (None, Some(o)) => Some(o),
            (None, None) => None,
        };

        Ok(snapped.map(u64::from))
    }

    /// Persist the full state.
    pub fn save_state(&self, state: &SpendabilityStateRow) -> Result<()> {
        let target_height = to_sql_i64(state.target_height)?;
        let anchor_height = to_sql_i64(state.anchor_height)?;
        let validated_anchor_height = to_sql_i64(state.validated_anchor_height)?;
        let repair_from_height = to_sql_i64(state.repair_from_height)?;
        self.db.conn().execute(
            r#"
            UPDATE spendability_state SET
                spendable = ?1,
                rescan_required = ?2,
                target_height = ?3,
                anchor_height = ?4,
                validated_anchor_height = ?5,
                repair_queued = ?6,
                repair_from_height = ?7,
                reason_code = ?8,
                updated_at = ?9
            WHERE id = 1
            "#,
            params![
                bool_to_int(state.spendable),
                bool_to_int(state.rescan_required),
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

    /// Mark state as sync-finalizing (not spendable yet, but no mandatory full rescan).
    pub fn mark_sync_finalizing(&self, target_height: u64, anchor_height: u64) -> Result<()> {
        let target_height = to_sql_i64(target_height)?;
        let anchor_height = to_sql_i64(anchor_height)?;
        self.db.conn().execute(
            r#"
            UPDATE spendability_state SET
                spendable = 0,
                rescan_required = 0,
                target_height = ?1,
                anchor_height = ?2,
                repair_queued = 0,
                repair_from_height = 0,
                reason_code = 'ERR_SYNC_FINALIZING',
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

    /// Mark state as validated/spendable.
    pub fn mark_validated(&self, target_height: u64, anchor_height: u64) -> Result<()> {
        let target_height = to_sql_i64(target_height)?;
        let anchor_height = to_sql_i64(anchor_height)?;
        self.db.conn().execute(
            r#"
            UPDATE spendability_state SET
                spendable = 1,
                rescan_required = 0,
                target_height = ?1,
                anchor_height = ?2,
                validated_anchor_height = ?2,
                repair_queued = 0,
                repair_from_height = 0,
                reason_code = 'OK',
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
                rescan_required = 0,
                repair_queued = 1,
                repair_from_height = CASE
                    WHEN repair_from_height > 0 AND repair_from_height < ?1 THEN repair_from_height
                    ELSE ?1
                END,
                reason_code = ?2,
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
                rescan_required = 0,
                repair_queued = 1,
                repair_from_height = CASE
                    WHEN repair_from_height > 0 AND repair_from_height < ?1 THEN repair_from_height
                    ELSE ?1
                END,
                reason_code = ?2,
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
