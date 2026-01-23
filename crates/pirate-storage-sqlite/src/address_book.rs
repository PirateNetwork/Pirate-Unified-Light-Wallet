//! Address book storage with labels, notes, and color tags
//!
//! Provides CRUD operations for contact addresses with rich metadata.

use crate::error::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Maximum label length
pub const MAX_LABEL_LENGTH: usize = 100;

/// Maximum notes length
pub const MAX_NOTES_LENGTH: usize = 500;

/// Color tag for address book entries
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ColorTag {
    /// No color (default)
    #[default]
    None = 0,
    /// Red
    Red = 1,
    /// Orange
    Orange = 2,
    /// Yellow
    Yellow = 3,
    /// Green
    Green = 4,
    /// Blue
    Blue = 5,
    /// Purple
    Purple = 6,
    /// Pink
    Pink = 7,
    /// Gray
    Gray = 8,
}

impl ColorTag {
    /// Get color tag from u8
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => ColorTag::Red,
            2 => ColorTag::Orange,
            3 => ColorTag::Yellow,
            4 => ColorTag::Green,
            5 => ColorTag::Blue,
            6 => ColorTag::Purple,
            7 => ColorTag::Pink,
            8 => ColorTag::Gray,
            _ => ColorTag::None,
        }
    }

    /// Get as u8
    pub fn as_u8(&self) -> u8 {
        *self as u8
    }

    /// Get hex color code
    pub fn hex_color(&self) -> &'static str {
        match self {
            ColorTag::None => "#6B7280",   // Gray-500
            ColorTag::Red => "#EF4444",    // Red-500
            ColorTag::Orange => "#F97316", // Orange-500
            ColorTag::Yellow => "#EAB308", // Yellow-500
            ColorTag::Green => "#22C55E",  // Green-500
            ColorTag::Blue => "#3B82F6",   // Blue-500
            ColorTag::Purple => "#8B5CF6", // Purple-500
            ColorTag::Pink => "#EC4899",   // Pink-500
            ColorTag::Gray => "#6B7280",   // Gray-500
        }
    }

    /// Get display name
    pub fn display_name(&self) -> &'static str {
        match self {
            ColorTag::None => "None",
            ColorTag::Red => "Red",
            ColorTag::Orange => "Orange",
            ColorTag::Yellow => "Yellow",
            ColorTag::Green => "Green",
            ColorTag::Blue => "Blue",
            ColorTag::Purple => "Purple",
            ColorTag::Pink => "Pink",
            ColorTag::Gray => "Gray",
        }
    }

    /// Get all color tags
    pub fn all() -> &'static [ColorTag] {
        &[
            ColorTag::None,
            ColorTag::Red,
            ColorTag::Orange,
            ColorTag::Yellow,
            ColorTag::Green,
            ColorTag::Blue,
            ColorTag::Purple,
            ColorTag::Pink,
            ColorTag::Gray,
        ]
    }
}

/// Address book entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressBookEntry {
    /// Unique ID
    pub id: i64,
    /// Wallet ID this entry belongs to
    pub wallet_id: String,
    /// Sapling address (zs1...)
    pub address: String,
    /// Display label
    pub label: String,
    /// Optional notes
    pub notes: Option<String>,
    /// Color tag
    pub color_tag: ColorTag,
    /// Whether this is a favorite
    pub is_favorite: bool,
    /// Created timestamp
    pub created_at: String,
    /// Last updated timestamp
    pub updated_at: String,
    /// Last used timestamp (for sending)
    pub last_used_at: Option<String>,
    /// Number of times used
    pub use_count: u32,
}

impl AddressBookEntry {
    /// Create new entry (for insertion)
    pub fn new(wallet_id: String, address: String, label: String) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: 0, // Will be set by database
            wallet_id,
            address,
            label,
            notes: None,
            color_tag: ColorTag::None,
            is_favorite: false,
            created_at: now.clone(),
            updated_at: now,
            last_used_at: None,
            use_count: 0,
        }
    }

    /// Set notes
    pub fn with_notes(mut self, notes: String) -> Self {
        self.notes = Some(notes);
        self
    }

    /// Set color tag
    pub fn with_color_tag(mut self, tag: ColorTag) -> Self {
        self.color_tag = tag;
        self
    }

    /// Set favorite
    pub fn with_favorite(mut self, is_favorite: bool) -> Self {
        self.is_favorite = is_favorite;
        self
    }

    /// Get truncated address for display
    pub fn truncated_address(&self) -> String {
        if self.address.len() > 24 {
            format!(
                "{}...{}",
                &self.address[..12],
                &self.address[self.address.len() - 12..]
            )
        } else {
            self.address.clone()
        }
    }
}

/// Address book storage
pub struct AddressBookStorage;

impl AddressBookStorage {
    /// Create address_book table
    pub fn create_table(conn: &Connection) -> Result<()> {
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS address_book (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_id TEXT NOT NULL,
                address TEXT NOT NULL,
                label TEXT NOT NULL,
                notes TEXT,
                color_tag INTEGER NOT NULL DEFAULT 0,
                is_favorite INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_used_at TEXT,
                use_count INTEGER NOT NULL DEFAULT 0,
                
                UNIQUE(wallet_id, address)
            )
            "#,
            [],
        )?;

        // Create indices
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_address_book_wallet ON address_book(wallet_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_address_book_label ON address_book(wallet_id, label)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_address_book_favorite ON address_book(wallet_id, is_favorite)",
            [],
        )?;

        Ok(())
    }

    /// Insert new entry
    pub fn insert(conn: &Connection, entry: &AddressBookEntry) -> Result<i64> {
        // Validate
        if entry.label.is_empty() {
            return Err(Error::Validation("Label cannot be empty".to_string()));
        }
        if entry.label.len() > MAX_LABEL_LENGTH {
            return Err(Error::Validation(format!(
                "Label too long: {} (max {})",
                entry.label.len(),
                MAX_LABEL_LENGTH
            )));
        }
        if let Some(ref notes) = entry.notes {
            if notes.len() > MAX_NOTES_LENGTH {
                return Err(Error::Validation(format!(
                    "Notes too long: {} (max {})",
                    notes.len(),
                    MAX_NOTES_LENGTH
                )));
            }
        }
        if !entry.address.starts_with("zs1") {
            return Err(Error::Validation(
                "Address must be a Sapling address (zs1...)".to_string(),
            ));
        }

        conn.execute(
            r#"
            INSERT INTO address_book 
                (wallet_id, address, label, notes, color_tag, is_favorite, 
                 created_at, updated_at, last_used_at, use_count)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            params![
                entry.wallet_id,
                entry.address,
                entry.label,
                entry.notes,
                entry.color_tag.as_u8(),
                entry.is_favorite as i32,
                entry.created_at,
                entry.updated_at,
                entry.last_used_at,
                entry.use_count,
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Update existing entry
    pub fn update(conn: &Connection, entry: &AddressBookEntry) -> Result<()> {
        // Validate
        if entry.label.is_empty() {
            return Err(Error::Validation("Label cannot be empty".to_string()));
        }
        if entry.label.len() > MAX_LABEL_LENGTH {
            return Err(Error::Validation(format!(
                "Label too long: {} (max {})",
                entry.label.len(),
                MAX_LABEL_LENGTH
            )));
        }
        if let Some(ref notes) = entry.notes {
            if notes.len() > MAX_NOTES_LENGTH {
                return Err(Error::Validation(format!(
                    "Notes too long: {} (max {})",
                    notes.len(),
                    MAX_NOTES_LENGTH
                )));
            }
        }

        let now = chrono::Utc::now().to_rfc3339();

        let rows = conn.execute(
            r#"
            UPDATE address_book SET
                label = ?1,
                notes = ?2,
                color_tag = ?3,
                is_favorite = ?4,
                updated_at = ?5
            WHERE id = ?6 AND wallet_id = ?7
            "#,
            params![
                entry.label,
                entry.notes,
                entry.color_tag.as_u8(),
                entry.is_favorite as i32,
                now,
                entry.id,
                entry.wallet_id,
            ],
        )?;

        if rows == 0 {
            return Err(Error::NotFound("Address book entry not found".to_string()));
        }

        Ok(())
    }

    /// Delete entry by ID
    pub fn delete(conn: &Connection, wallet_id: &str, id: i64) -> Result<()> {
        let rows = conn.execute(
            "DELETE FROM address_book WHERE id = ?1 AND wallet_id = ?2",
            params![id, wallet_id],
        )?;

        if rows == 0 {
            return Err(Error::NotFound("Address book entry not found".to_string()));
        }

        Ok(())
    }

    /// Delete entry by address
    pub fn delete_by_address(conn: &Connection, wallet_id: &str, address: &str) -> Result<()> {
        let rows = conn.execute(
            "DELETE FROM address_book WHERE wallet_id = ?1 AND address = ?2",
            params![wallet_id, address],
        )?;

        if rows == 0 {
            return Err(Error::NotFound("Address book entry not found".to_string()));
        }

        Ok(())
    }

    /// Get entry by ID
    pub fn get_by_id(
        conn: &Connection,
        wallet_id: &str,
        id: i64,
    ) -> Result<Option<AddressBookEntry>> {
        let entry = conn
            .query_row(
                r#"
                SELECT id, wallet_id, address, label, notes, color_tag, is_favorite,
                       created_at, updated_at, last_used_at, use_count
                FROM address_book
                WHERE id = ?1 AND wallet_id = ?2
                "#,
                params![id, wallet_id],
                Self::row_to_entry,
            )
            .optional()?;

        Ok(entry)
    }

    /// Get entry by address
    pub fn get_by_address(
        conn: &Connection,
        wallet_id: &str,
        address: &str,
    ) -> Result<Option<AddressBookEntry>> {
        let entry = conn
            .query_row(
                r#"
                SELECT id, wallet_id, address, label, notes, color_tag, is_favorite,
                       created_at, updated_at, last_used_at, use_count
                FROM address_book
                WHERE wallet_id = ?1 AND address = ?2
                "#,
                params![wallet_id, address],
                Self::row_to_entry,
            )
            .optional()?;

        Ok(entry)
    }

    /// Get label for address (for transaction history display)
    pub fn get_label_for_address(
        conn: &Connection,
        wallet_id: &str,
        address: &str,
    ) -> Result<Option<String>> {
        let label = conn
            .query_row(
                "SELECT label FROM address_book WHERE wallet_id = ?1 AND address = ?2",
                params![wallet_id, address],
                |row| row.get(0),
            )
            .optional()?;

        Ok(label)
    }

    /// List all entries for wallet
    pub fn list(conn: &Connection, wallet_id: &str) -> Result<Vec<AddressBookEntry>> {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, wallet_id, address, label, notes, color_tag, is_favorite,
                   created_at, updated_at, last_used_at, use_count
            FROM address_book
            WHERE wallet_id = ?1
            ORDER BY is_favorite DESC, label ASC
            "#,
        )?;

        let entries = stmt
            .query_map(params![wallet_id], Self::row_to_entry)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    /// List favorites
    pub fn list_favorites(conn: &Connection, wallet_id: &str) -> Result<Vec<AddressBookEntry>> {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, wallet_id, address, label, notes, color_tag, is_favorite,
                   created_at, updated_at, last_used_at, use_count
            FROM address_book
            WHERE wallet_id = ?1 AND is_favorite = 1
            ORDER BY label ASC
            "#,
        )?;

        let entries = stmt
            .query_map(params![wallet_id], Self::row_to_entry)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    /// List by color tag
    pub fn list_by_color(
        conn: &Connection,
        wallet_id: &str,
        color_tag: ColorTag,
    ) -> Result<Vec<AddressBookEntry>> {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, wallet_id, address, label, notes, color_tag, is_favorite,
                   created_at, updated_at, last_used_at, use_count
            FROM address_book
            WHERE wallet_id = ?1 AND color_tag = ?2
            ORDER BY label ASC
            "#,
        )?;

        let entries = stmt
            .query_map(params![wallet_id, color_tag.as_u8()], |row| {
                Self::row_to_entry(row)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    /// Search entries
    pub fn search(
        conn: &Connection,
        wallet_id: &str,
        query: &str,
    ) -> Result<Vec<AddressBookEntry>> {
        let search_pattern = format!("%{}%", query.to_lowercase());

        let mut stmt = conn.prepare(
            r#"
            SELECT id, wallet_id, address, label, notes, color_tag, is_favorite,
                   created_at, updated_at, last_used_at, use_count
            FROM address_book
            WHERE wallet_id = ?1 
              AND (LOWER(label) LIKE ?2 OR LOWER(address) LIKE ?2 OR LOWER(notes) LIKE ?2)
            ORDER BY is_favorite DESC, label ASC
            "#,
        )?;

        let entries = stmt
            .query_map(params![wallet_id, search_pattern], |row| {
                Self::row_to_entry(row)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    /// Mark address as used (update last_used_at and use_count)
    pub fn mark_used(conn: &Connection, wallet_id: &str, address: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            r#"
            UPDATE address_book SET
                last_used_at = ?1,
                use_count = use_count + 1,
                updated_at = ?1
            WHERE wallet_id = ?2 AND address = ?3
            "#,
            params![now, wallet_id, address],
        )?;

        Ok(())
    }

    /// Toggle favorite status
    pub fn toggle_favorite(conn: &Connection, wallet_id: &str, id: i64) -> Result<bool> {
        let now = chrono::Utc::now().to_rfc3339();

        // Get current status and toggle
        let new_status: bool = conn.query_row(
            "SELECT is_favorite FROM address_book WHERE id = ?1 AND wallet_id = ?2",
            params![id, wallet_id],
            |row| {
                let current: i32 = row.get(0)?;
                Ok(current == 0)
            },
        )?;

        conn.execute(
            "UPDATE address_book SET is_favorite = ?1, updated_at = ?2 WHERE id = ?3 AND wallet_id = ?4",
            params![new_status as i32, now, id, wallet_id],
        )?;

        Ok(new_status)
    }

    /// Count entries for wallet
    pub fn count(conn: &Connection, wallet_id: &str) -> Result<u32> {
        let count: u32 = conn.query_row(
            "SELECT COUNT(*) FROM address_book WHERE wallet_id = ?1",
            params![wallet_id],
            |row| row.get(0),
        )?;

        Ok(count)
    }

    /// Check if address exists in book
    pub fn exists(conn: &Connection, wallet_id: &str, address: &str) -> Result<bool> {
        let count: u32 = conn.query_row(
            "SELECT COUNT(*) FROM address_book WHERE wallet_id = ?1 AND address = ?2",
            params![wallet_id, address],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    /// Get recently used entries
    pub fn recently_used(
        conn: &Connection,
        wallet_id: &str,
        limit: u32,
    ) -> Result<Vec<AddressBookEntry>> {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, wallet_id, address, label, notes, color_tag, is_favorite,
                   created_at, updated_at, last_used_at, use_count
            FROM address_book
            WHERE wallet_id = ?1 AND last_used_at IS NOT NULL
            ORDER BY last_used_at DESC
            LIMIT ?2
            "#,
        )?;

        let entries = stmt
            .query_map(params![wallet_id, limit], Self::row_to_entry)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    /// Helper to convert row to entry
    fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<AddressBookEntry> {
        Ok(AddressBookEntry {
            id: row.get(0)?,
            wallet_id: row.get(1)?,
            address: row.get(2)?,
            label: row.get(3)?,
            notes: row.get(4)?,
            color_tag: ColorTag::from_u8(row.get::<_, u8>(5)?),
            is_favorite: row.get::<_, i32>(6)? != 0,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
            last_used_at: row.get(9)?,
            use_count: row.get(10)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        AddressBookStorage::create_table(&conn).unwrap();
        conn
    }

    #[test]
    fn test_insert_and_get() {
        let conn = setup_db();
        let wallet_id = "test_wallet";

        let entry = AddressBookEntry::new(
            wallet_id.to_string(),
            "zs1test1234567890abcdef1234567890abcdef1234567890abcdef1234567890abc".to_string(),
            "Alice".to_string(),
        )
        .with_notes("Coffee fund".to_string())
        .with_color_tag(ColorTag::Green);

        let id = AddressBookStorage::insert(&conn, &entry).unwrap();
        assert!(id > 0);

        let retrieved = AddressBookStorage::get_by_id(&conn, wallet_id, id)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.label, "Alice");
        assert_eq!(retrieved.notes, Some("Coffee fund".to_string()));
        assert_eq!(retrieved.color_tag, ColorTag::Green);
    }

    #[test]
    fn test_update() {
        let conn = setup_db();
        let wallet_id = "test_wallet";

        let mut entry = AddressBookEntry::new(
            wallet_id.to_string(),
            "zs1test1234567890abcdef1234567890abcdef1234567890abcdef1234567890abc".to_string(),
            "Alice".to_string(),
        );

        let id = AddressBookStorage::insert(&conn, &entry).unwrap();
        entry.id = id;
        entry.label = "Alice Updated".to_string();
        entry.color_tag = ColorTag::Blue;

        AddressBookStorage::update(&conn, &entry).unwrap();

        let retrieved = AddressBookStorage::get_by_id(&conn, wallet_id, id)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.label, "Alice Updated");
        assert_eq!(retrieved.color_tag, ColorTag::Blue);
    }

    #[test]
    fn test_delete() {
        let conn = setup_db();
        let wallet_id = "test_wallet";

        let entry = AddressBookEntry::new(
            wallet_id.to_string(),
            "zs1test1234567890abcdef1234567890abcdef1234567890abcdef1234567890abc".to_string(),
            "Alice".to_string(),
        );

        let id = AddressBookStorage::insert(&conn, &entry).unwrap();
        AddressBookStorage::delete(&conn, wallet_id, id).unwrap();

        let retrieved = AddressBookStorage::get_by_id(&conn, wallet_id, id).unwrap();
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_list_and_search() {
        let conn = setup_db();
        let wallet_id = "test_wallet";

        let entries = vec![
            AddressBookEntry::new(
                wallet_id.to_string(),
                "zs1alice234567890abcdef1234567890abcdef1234567890abcdef1234567890ab".to_string(),
                "Alice".to_string(),
            ),
            AddressBookEntry::new(
                wallet_id.to_string(),
                "zs1bob56789012345abcdef1234567890abcdef1234567890abcdef1234567890ab".to_string(),
                "Bob".to_string(),
            ),
        ];

        for entry in &entries {
            AddressBookStorage::insert(&conn, entry).unwrap();
        }

        let all = AddressBookStorage::list(&conn, wallet_id).unwrap();
        assert_eq!(all.len(), 2);

        let search = AddressBookStorage::search(&conn, wallet_id, "alice").unwrap();
        assert_eq!(search.len(), 1);
        assert_eq!(search[0].label, "Alice");
    }

    #[test]
    fn test_favorites() {
        let conn = setup_db();
        let wallet_id = "test_wallet";

        let entry = AddressBookEntry::new(
            wallet_id.to_string(),
            "zs1test1234567890abcdef1234567890abcdef1234567890abcdef1234567890abc".to_string(),
            "Alice".to_string(),
        );

        let id = AddressBookStorage::insert(&conn, &entry).unwrap();

        // Toggle on
        let is_fav = AddressBookStorage::toggle_favorite(&conn, wallet_id, id).unwrap();
        assert!(is_fav);

        let favorites = AddressBookStorage::list_favorites(&conn, wallet_id).unwrap();
        assert_eq!(favorites.len(), 1);

        // Toggle off
        let is_fav = AddressBookStorage::toggle_favorite(&conn, wallet_id, id).unwrap();
        assert!(!is_fav);

        let favorites = AddressBookStorage::list_favorites(&conn, wallet_id).unwrap();
        assert_eq!(favorites.len(), 0);
    }

    #[test]
    fn test_mark_used() {
        let conn = setup_db();
        let wallet_id = "test_wallet";
        let address = "zs1test1234567890abcdef1234567890abcdef1234567890abcdef1234567890abc";

        let entry = AddressBookEntry::new(
            wallet_id.to_string(),
            address.to_string(),
            "Alice".to_string(),
        );

        let id = AddressBookStorage::insert(&conn, &entry).unwrap();

        AddressBookStorage::mark_used(&conn, wallet_id, address).unwrap();
        AddressBookStorage::mark_used(&conn, wallet_id, address).unwrap();

        let retrieved = AddressBookStorage::get_by_id(&conn, wallet_id, id)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.use_count, 2);
        assert!(retrieved.last_used_at.is_some());
    }

    #[test]
    fn test_get_label_for_address() {
        let conn = setup_db();
        let wallet_id = "test_wallet";
        let address = "zs1test1234567890abcdef1234567890abcdef1234567890abcdef1234567890abc";

        let entry = AddressBookEntry::new(
            wallet_id.to_string(),
            address.to_string(),
            "Alice".to_string(),
        );

        AddressBookStorage::insert(&conn, &entry).unwrap();

        let label = AddressBookStorage::get_label_for_address(&conn, wallet_id, address).unwrap();
        assert_eq!(label, Some("Alice".to_string()));

        let unknown =
            AddressBookStorage::get_label_for_address(&conn, wallet_id, "zs1unknown").unwrap();
        assert!(unknown.is_none());
    }

    #[test]
    fn test_validation() {
        let conn = setup_db();
        let wallet_id = "test_wallet";

        // Empty label
        let entry = AddressBookEntry::new(
            wallet_id.to_string(),
            "zs1test1234567890abcdef1234567890abcdef1234567890abcdef1234567890abc".to_string(),
            "".to_string(),
        );
        assert!(AddressBookStorage::insert(&conn, &entry).is_err());

        // Invalid address
        let entry = AddressBookEntry::new(
            wallet_id.to_string(),
            "invalid_address".to_string(),
            "Test".to_string(),
        );
        assert!(AddressBookStorage::insert(&conn, &entry).is_err());

        // Label too long
        let entry = AddressBookEntry::new(
            wallet_id.to_string(),
            "zs1test1234567890abcdef1234567890abcdef1234567890abcdef1234567890abc".to_string(),
            "x".repeat(MAX_LABEL_LENGTH + 1),
        );
        assert!(AddressBookStorage::insert(&conn, &entry).is_err());
    }
}
