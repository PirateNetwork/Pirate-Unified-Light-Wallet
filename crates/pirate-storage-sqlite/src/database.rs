//! Database connection and initialization

use crate::{encryption::EncryptionKey, migrations, security::MasterKey, Result};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpenMode {
    Full,
    ExistingHot,
}

/// Database connection wrapper
pub struct Database {
    conn: Connection,
    master_key: MasterKey,
}

impl Database {
    /// Open database with encryption
    pub fn open<P: AsRef<Path>>(
        path: P,
        key: &EncryptionKey,
        master_key: MasterKey,
    ) -> Result<Self> {
        Self::open_internal(path, key, master_key, OpenMode::Full)
    }

    /// Open an existing encrypted database for hot-path use.
    ///
    /// This skips migration work that has already been performed by wallet attach / startup
    /// paths, while preserving SQLCipher validation and basic readability checks.
    pub fn open_existing<P: AsRef<Path>>(
        path: P,
        key: &EncryptionKey,
        master_key: MasterKey,
    ) -> Result<Self> {
        Self::open_internal(path, key, master_key, OpenMode::ExistingHot)
    }

    fn open_internal<P: AsRef<Path>>(
        path: P,
        key: &EncryptionKey,
        master_key: MasterKey,
        mode: OpenMode,
    ) -> Result<Self> {
        let db_exists = path.as_ref().exists();
        let path_buf = path.as_ref().to_path_buf();

        if mode == OpenMode::ExistingHot && !db_exists {
            return Err(crate::Error::Storage(format!(
                "Database does not exist at {}",
                path_buf.display()
            )));
        }

        let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        if mode == OpenMode::Full {
            flags |= OpenFlags::SQLITE_OPEN_CREATE;
        }
        let conn = Connection::open_with_flags(&path_buf, flags)?;

        // CRITICAL: PRAGMA key MUST be the FIRST statement executed after opening the connection
        // Any other PRAGMA or SQL statement executed before PRAGMA key will cause the database
        // to be created in an unencrypted state, leading to "file is not a database" errors
        let key_hex = hex::encode(key.as_bytes());
        if let Err(e) = conn.execute(
            &format!("PRAGMA key = '{}';", key_hex.replace("'", "''")),
            [],
        ) {
            if !e.to_string().contains("Execute returned results") {
                return Err(crate::Error::Encryption(format!(
                    "Failed to set database encryption key: {}",
                    e
                )));
            }
        }

        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch("PRAGMA foreign_keys=OFF;")?;

        let cipher_version: std::result::Result<String, rusqlite::Error> =
            conn.query_row("PRAGMA cipher_version", [], |row| row.get(0));

        match cipher_version {
            Ok(version) if !version.is_empty() => {
                tracing::debug!("SQLCipher version: {}", version);
            }
            Ok(_) | Err(_) => {
                return Err(crate::Error::Encryption(
                    "SQLCipher encryption verification failed. Database may not be encrypted."
                        .to_string(),
                ));
            }
        }

        if db_exists {
            let test_result: std::result::Result<i64, rusqlite::Error> =
                conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0));

            if test_result.is_err() {
                let file_size = std::fs::metadata(&path_buf).map(|m| m.len()).unwrap_or(0);

                if file_size < 100 {
                    tracing::warn!(
                        "Database file exists but is too small ({} bytes), may be corrupted",
                        file_size
                    );
                    return Err(crate::Error::Encryption(
                        "Database file appears to be corrupted. Please delete it and try again."
                            .to_string(),
                    ));
                }

                return Err(crate::Error::Encryption(
                    "Database encryption verification failed: cannot read from encrypted database. The database may have been created with a different encryption key.".to_string()
                ));
            }
        }

        if mode == OpenMode::Full {
            migrations::run_migrations(&conn)?;
        }

        Ok(Self { conn, master_key })
    }

    /// Get connection
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Get master key for field-level encryption
    pub fn master_key(&self) -> &MasterKey {
        &self.master_key
    }

    /// Rekey database with a new encryption key
    pub fn rekey(&self, new_key: &EncryptionKey) -> Result<()> {
        let key_hex = hex::encode(new_key.as_bytes());
        if let Err(e) = self.conn.execute(
            &format!("PRAGMA rekey = '{}';", key_hex.replace("'", "''")),
            [],
        ) {
            if !e.to_string().contains("Execute returned results") {
                return Err(crate::Error::Encryption(format!(
                    "Failed to rekey database: {}",
                    e
                )));
            }
        }
        Ok(())
    }

    /// Begin transaction
    pub fn transaction(&mut self) -> Result<rusqlite::Transaction<'_>> {
        Ok(self.conn.transaction()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::{EncryptionAlgorithm, MasterKey};
    use tempfile::NamedTempFile;

    #[test]
    fn test_open_database() {
        let file = NamedTempFile::new().unwrap();
        let salt = crate::security::generate_salt();
        let key = EncryptionKey::from_passphrase("test", &salt).unwrap();
        let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
        let result = Database::open(file.path(), &key, master_key);
        assert!(result.is_ok());
    }

    #[test]
    fn test_sqlcipher_verification() {
        let file = NamedTempFile::new().unwrap();
        let salt = crate::security::generate_salt();
        let key = EncryptionKey::from_passphrase("test-passphrase", &salt).unwrap();
        let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);

        // Create encrypted database
        let db = Database::open(file.path(), &key, master_key).unwrap();

        // Verify SQLCipher is active by checking cipher_version
        let version: String = db
            .conn()
            .query_row("PRAGMA cipher_version", [], |row| row.get(0))
            .unwrap();
        assert!(!version.is_empty(), "SQLCipher version should be non-empty");
    }

    #[test]
    fn test_wrong_database_key_fails() {
        let file = NamedTempFile::new().unwrap();
        let salt = crate::security::generate_salt();
        let key1 = EncryptionKey::from_passphrase("correct-key", &salt).unwrap();
        let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);

        // Create database with key1
        let db = Database::open(file.path(), &key1, master_key.clone()).unwrap();
        db.conn()
            .execute("CREATE TABLE test (id INTEGER)", [])
            .unwrap();
        db.conn()
            .execute("INSERT INTO test (id) VALUES (1)", [])
            .unwrap();
        drop(db); // Close database

        // Try to open with wrong key
        let key2 = EncryptionKey::from_passphrase("wrong-key", &salt).unwrap();
        let result = Database::open(file.path(), &key2, master_key);

        // Should fail or return garbage data
        // SQLCipher behavior: wrong key may succeed but return garbage or fail
        // We verify by trying to read - if it fails or returns wrong data, encryption is working
        match result {
            Ok(db) => {
                // If it opens, try to read - should fail or return garbage
                let read_result: rusqlite::Result<i64> =
                    db.conn()
                        .query_row("SELECT id FROM test", [], |row| row.get(0));
                // Either read fails or we verify encryption is working
                assert!(
                    read_result.is_err() || read_result.unwrap() != 1,
                    "Wrong key should not allow reading correct data"
                );
            }
            Err(_) => {
                // Database correctly rejected wrong key
            }
        }
    }

    #[test]
    fn test_database_file_is_encrypted() {
        let file = NamedTempFile::new().unwrap();
        let salt = crate::security::generate_salt();
        let key = EncryptionKey::from_passphrase("test-passphrase", &salt).unwrap();
        let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);

        // Create database and write some data
        let db = Database::open(file.path(), &key, master_key).unwrap();
        db.conn()
            .execute("CREATE TABLE test (data TEXT)", [])
            .unwrap();
        db.conn()
            .execute("INSERT INTO test (data) VALUES ('sensitive data')", [])
            .unwrap();
        drop(db);

        // Read raw database file
        let file_contents = std::fs::read(file.path()).unwrap();

        // Encrypted database should not contain plaintext "sensitive data"
        let file_string = String::from_utf8_lossy(&file_contents);
        assert!(
            !file_string.contains("sensitive data"),
            "Database file should not contain plaintext data"
        );

        // Encrypted file should appear mostly random (high entropy)
        // Check that file doesn't have long strings of readable text
        let readable_chars = file_string
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || c.is_whitespace())
            .count();
        let total_chars = file_string.len();
        let readable_ratio = readable_chars as f64 / total_chars as f64;

        // Encrypted data should have low readable character ratio (< 0.5)
        assert!(
            readable_ratio < 0.5,
            "Encrypted database should have low readable character ratio, got: {}",
            readable_ratio
        );
    }
}
