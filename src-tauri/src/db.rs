use chrono::Utc;
use rusqlite::{params, Connection};
use rusqlite_migration::{Migrations, M};

use crate::{
    auth::PACKAGED_MICROSOFT_CLIENT_ID,
    error::AppResult,
    paths::AppPaths,
};

pub fn open_database(paths: &AppPaths) -> AppResult<Connection> {
    let mut connection = Connection::open(&paths.db_path)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    migrations().to_latest(&mut connection)?;
    seed_settings(&connection)?;
    Ok(connection)
}

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(
            "
        CREATE TABLE accounts (
          id TEXT PRIMARY KEY,
          username TEXT NOT NULL,
          uuid TEXT NOT NULL,
          provider TEXT NOT NULL,
          avatar_url TEXT,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );

        CREATE TABLE profiles (
          id TEXT PRIMARY KEY,
          name TEXT NOT NULL,
          profile_type TEXT NOT NULL,
          minecraft_version TEXT NOT NULL,
          loader_version TEXT,
          profile_dir TEXT NOT NULL UNIQUE,
          account_id TEXT REFERENCES accounts(id) ON DELETE SET NULL,
          java_path TEXT,
          memory_min_mb INTEGER,
          memory_max_mb INTEGER,
          jvm_args TEXT NOT NULL DEFAULT '',
          launch_args TEXT NOT NULL DEFAULT '',
          notes TEXT,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          last_played_at TEXT
        );
        CREATE INDEX idx_profiles_updated_at ON profiles(updated_at DESC);

        CREATE TABLE installed_content (
          id TEXT PRIMARY KEY,
          profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
          content_type TEXT NOT NULL,
          install_scope TEXT NOT NULL DEFAULT 'profile',
          provider TEXT NOT NULL,
          project_id TEXT NOT NULL,
          version_id TEXT NOT NULL,
          slug TEXT NOT NULL,
          name TEXT NOT NULL,
          local_file_path TEXT NOT NULL,
          target_rel_path TEXT,
          file_hash TEXT,
          enabled INTEGER NOT NULL DEFAULT 1,
          version_number TEXT,
          installed_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );
        CREATE INDEX idx_installed_content_profile_id ON installed_content(profile_id);

        CREATE TABLE content_dependencies (
          id TEXT PRIMARY KEY,
          installed_content_id TEXT NOT NULL REFERENCES installed_content(id) ON DELETE CASCADE,
          dependency_project_id TEXT NOT NULL,
          dependency_version_id TEXT,
          dependency_kind TEXT NOT NULL
        );

        CREATE TABLE skins (
          id TEXT PRIMARY KEY,
          local_file_path TEXT NOT NULL,
          display_name TEXT NOT NULL,
          model_variant TEXT NOT NULL,
          tags_json TEXT NOT NULL DEFAULT '[]',
          thumbnail_path TEXT,
          imported_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );

        CREATE TABLE profile_exports (
          id TEXT PRIMARY KEY,
          profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
          export_version INTEGER NOT NULL,
          manifest_json TEXT NOT NULL,
          created_at TEXT NOT NULL
        );

        CREATE TABLE launch_history (
          id TEXT PRIMARY KEY,
          profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
          account_id TEXT REFERENCES accounts(id) ON DELETE SET NULL,
          started_at TEXT NOT NULL,
          ended_at TEXT,
          status TEXT NOT NULL,
          log_path TEXT NOT NULL,
          exit_code INTEGER
        );

        CREATE TABLE settings (
          key TEXT PRIMARY KEY,
          value TEXT NOT NULL,
          category TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );
        ",
        ),
        M::up(
            "
        ALTER TABLE accounts ADD COLUMN current_skin_id TEXT;

        CREATE TABLE IF NOT EXISTS account_tokens (
          account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
          token_reference TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );
        ",
        ),
    ])
}

fn seed_settings(connection: &Connection) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let defaults = [
        ("default_java_path", "", "launch"),
        ("download_concurrency", "4", "network"),
        ("ui_density", "compact", "ui"),
        ("log_retention_days", "30", "logging"),
        ("managed_runtime_preference", "auto", "launch"),
        ("microsoft_client_id", PACKAGED_MICROSOFT_CLIENT_ID, "auth"),
        ("launcher_name", "Blocksmith", "launch"),
        ("launcher_version", "0.1.0", "launch"),
    ];

    for (key, value, category) in defaults {
        connection.execute(
            "
            INSERT INTO settings (key, value, category, updated_at)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(key) DO NOTHING
            ",
            params![key, value, category, now],
        )?;
    }

    connection.execute(
        "
        UPDATE settings
        SET value = ?1, updated_at = ?2
        WHERE key = 'microsoft_client_id' AND TRIM(value) = ''
        ",
        params![PACKAGED_MICROSOFT_CLIENT_ID, now],
    )?;

    Ok(())
}
