//! Scan queue storage for deterministic rescan/repair scheduling.
//!
//! This models queue-based repair semantics (including `FoundNote` priority)
//! with deterministic wallet scheduling.

use crate::{Database, Error, Result};
use rusqlite::{params, OptionalExtension};

/// Priority used for normal historic scanning.
pub const SCAN_PRIORITY_HISTORIC: i64 = 10;
/// Priority used for witness/note corruption repair rescans.
pub const SCAN_PRIORITY_FOUND_NOTE: i64 = 40;

const STATUS_PENDING: &str = "pending";
const STATUS_IN_PROGRESS: &str = "in_progress";
const STATUS_DONE: &str = "done";

/// A queued scan range.
#[derive(Debug, Clone)]
pub struct ScanRangeRow {
    /// Queue row id.
    pub id: i64,
    /// Inclusive start height.
    pub range_start: u64,
    /// Exclusive end height.
    pub range_end: u64,
    /// Priority (larger is higher).
    pub priority: i64,
    /// Status (`pending`, `in_progress`, `done`).
    pub status: String,
    /// Optional reason metadata.
    pub reason: Option<String>,
    /// Last update timestamp.
    pub updated_at: String,
}

/// Scan queue storage operations.
pub struct ScanQueueStorage<'a> {
    db: &'a Database,
}

impl<'a> ScanQueueStorage<'a> {
    /// Create a new queue wrapper.
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Queue a `FoundNote` repair range.
    ///
    /// `range_end` is exclusive.
    pub fn queue_found_note_range(
        &self,
        range_start: u64,
        range_end: u64,
        reason: Option<&str>,
    ) -> Result<()> {
        self.queue_range(range_start, range_end, SCAN_PRIORITY_FOUND_NOTE, reason)
    }

    /// Queue any range with merge-overlap semantics for pending rows.
    ///
    /// Overlapping `pending` rows of the same priority are merged into a single
    /// pending row. Existing `in_progress` rows are never replaced so active
    /// repair execution remains stable and deterministic.
    pub fn queue_range(
        &self,
        range_start: u64,
        range_end: u64,
        priority: i64,
        reason: Option<&str>,
    ) -> Result<()> {
        if range_start >= range_end {
            return Ok(());
        }

        let range_start_i64 = to_sql_i64(range_start)?;
        let range_end_i64 = to_sql_i64(range_end)?;

        let mut merged_start = range_start_i64;
        let mut merged_end = range_end_i64;
        let mut overlapping_pending_ids = Vec::<i64>::new();
        let mut overlapping_in_progress_ranges = Vec::<(i64, i64)>::new();

        {
            let mut stmt = self.db.conn().prepare(
                r#"
                SELECT id, range_start, range_end, status
                FROM scan_queue
                WHERE priority = ?1
                  AND status IN ('pending', 'in_progress')
                  AND NOT (range_end <= ?2 OR range_start >= ?3)
                "#,
            )?;
            let rows =
                stmt.query_map(params![priority, range_start_i64, range_end_i64], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?;

            for row in rows {
                let (id, start, end, status) = row?;
                if status == STATUS_IN_PROGRESS {
                    overlapping_in_progress_ranges.push((start, end));
                } else {
                    overlapping_pending_ids.push(id);
                    merged_start = merged_start.min(start);
                    merged_end = merged_end.max(end);
                }
            }
        }

        // Avoid re-queueing work that is already fully covered by an active
        // in-progress row when there is nothing pending to merge.
        let fully_covered_by_in_progress = overlapping_in_progress_ranges
            .iter()
            .any(|(start, end)| *start <= range_start_i64 && *end >= range_end_i64);
        if overlapping_pending_ids.is_empty() && fully_covered_by_in_progress {
            return Ok(());
        }

        self.db.conn().execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<()> {
            for id in overlapping_pending_ids {
                self.db
                    .conn()
                    .execute("DELETE FROM scan_queue WHERE id = ?1", params![id])?;
            }

            self.db.conn().execute(
                r#"
                INSERT INTO scan_queue (
                    range_start,
                    range_end,
                    priority,
                    status,
                    reason,
                    created_at,
                    updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), datetime('now'))
                "#,
                params![merged_start, merged_end, priority, STATUS_PENDING, reason],
            )?;

            Ok(())
        })();

        if result.is_err() {
            let _ = self.db.conn().execute_batch("ROLLBACK");
        } else {
            self.db.conn().execute_batch("COMMIT")?;
        }

        result
    }

    /// Record the observed historic scanned range.
    ///
    /// Keeps a single merged `SCAN_PRIORITY_HISTORIC` row so queue extrema can
    /// be used as canonical target/anchor input.
    pub fn record_historic_scanned_range(
        &self,
        range_start: u64,
        range_end: u64,
        reason: Option<&str>,
    ) -> Result<()> {
        if range_start >= range_end {
            return Ok(());
        }

        let range_start_i64 = to_sql_i64(range_start)?;
        let range_end_i64 = to_sql_i64(range_end)?;
        let (existing_start, existing_end): (Option<i64>, Option<i64>) = self.db.conn().query_row(
            r#"
            SELECT MIN(range_start), MAX(range_end)
            FROM scan_queue
            WHERE priority = ?1
            "#,
            params![SCAN_PRIORITY_HISTORIC],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        let merged_start = existing_start
            .unwrap_or(range_start_i64)
            .min(range_start_i64);
        let merged_end = existing_end.unwrap_or(range_end_i64).max(range_end_i64);

        self.db.conn().execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<()> {
            self.db.conn().execute(
                "DELETE FROM scan_queue WHERE priority = ?1",
                params![SCAN_PRIORITY_HISTORIC],
            )?;
            self.db.conn().execute(
                r#"
                INSERT INTO scan_queue (
                    range_start,
                    range_end,
                    priority,
                    status,
                    reason,
                    created_at,
                    updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), datetime('now'))
                "#,
                params![
                    merged_start,
                    merged_end,
                    SCAN_PRIORITY_HISTORIC,
                    STATUS_DONE,
                    reason
                ],
            )?;
            Ok(())
        })();

        if result.is_err() {
            let _ = self.db.conn().execute_batch("ROLLBACK");
        } else {
            self.db.conn().execute_batch("COMMIT")?;
        }

        result
    }

    /// Returns the next active `FoundNote` range.
    ///
    /// Prefers `in_progress` over `pending` so interrupted runs can resume
    /// deterministically.
    pub fn next_found_note_range(&self) -> Result<Option<ScanRangeRow>> {
        self.db
            .conn()
            .query_row(
                r#"
                SELECT
                    id,
                    range_start,
                    range_end,
                    priority,
                    status,
                    reason,
                    updated_at
                FROM scan_queue
                WHERE priority = ?1
                  AND status IN ('in_progress', 'pending')
                ORDER BY
                    CASE status
                        WHEN 'in_progress' THEN 0
                        ELSE 1
                    END,
                    range_start ASC
                LIMIT 1
                "#,
                params![SCAN_PRIORITY_FOUND_NOTE],
                |row| {
                    let range_start_i64: i64 = row.get(1)?;
                    let range_end_i64: i64 = row.get(2)?;
                    Ok(ScanRangeRow {
                        id: row.get(0)?,
                        range_start: to_u64(1, range_start_i64)?,
                        range_end: to_u64(2, range_end_i64)?,
                        priority: row.get(3)?,
                        status: row.get(4)?,
                        reason: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Mark a queued range as in-progress.
    pub fn mark_in_progress(&self, id: i64) -> Result<()> {
        self.db.conn().execute(
            r#"
            UPDATE scan_queue
            SET status = ?1, updated_at = datetime('now')
            WHERE id = ?2
            "#,
            params![STATUS_IN_PROGRESS, id],
        )?;
        Ok(())
    }

    /// Mark a queued range as done.
    pub fn mark_done(&self, id: i64) -> Result<()> {
        self.db.conn().execute(
            r#"
            UPDATE scan_queue
            SET status = ?1, updated_at = datetime('now')
            WHERE id = ?2
            "#,
            params![STATUS_DONE, id],
        )?;
        Ok(())
    }

    /// Mark active FoundNote ranges as done once their end is at-or-before
    /// `end_exclusive`.
    ///
    /// Queue-driven FoundNote repairs must never be retired before execution.
    /// Only rows already marked `in_progress` are eligible for completion here.
    /// This prevents pending repairs from being dropped before the replay worker
    /// processes them.
    pub fn mark_found_note_done_through(&self, end_exclusive: u64) -> Result<usize> {
        let end_exclusive = to_sql_i64(end_exclusive)?;
        let changed = self.db.conn().execute(
            r#"
            UPDATE scan_queue
            SET status = ?1, updated_at = datetime('now')
            WHERE priority = ?2
              AND status = 'in_progress'
              AND range_end <= ?3
            "#,
            params![STATUS_DONE, SCAN_PRIORITY_FOUND_NOTE, end_exclusive],
        )?;
        Ok(changed)
    }

    /// Remove all queued scan ranges.
    ///
    /// Used by destructive rescan resets to ensure stale pending FoundNote rows
    /// cannot block fixed-anchor spendability after the chain-derived state is
    /// rebuilt from scratch.
    pub fn clear_all(&self) -> Result<()> {
        self.db.conn().execute("DELETE FROM scan_queue", [])?;
        Ok(())
    }

    /// Remove all `FoundNote` repair ranges.
    pub fn clear_found_note_ranges(&self) -> Result<()> {
        self.db.conn().execute(
            "DELETE FROM scan_queue WHERE priority = ?1",
            params![SCAN_PRIORITY_FOUND_NOTE],
        )?;
        Ok(())
    }
}

fn to_sql_i64(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::Storage(format!("value {} exceeds i64::MAX", value)))
}

fn to_u64(index: usize, value: i64) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}
