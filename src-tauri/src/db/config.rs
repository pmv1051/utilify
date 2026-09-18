use rusqlite::{params, Connection, OptionalExtension};

pub const CLIENT_ID: &str = "client_id";
pub const ACCESS_TOKEN: &str = "access_token";
pub const REFRESH_TOKEN: &str = "refresh_token";
pub const TOKEN_EXPIRES_AT: &str = "token_expires_at";
pub const USER_ID: &str = "user_id";
pub const USER_DISPLAY_NAME: &str = "user_display_name";
pub const USER_PRODUCT: &str = "user_product";
pub const MINIMIZE_TO_TRAY: &str = "minimize_to_tray";
/// Seconds a track must be heard before it counts as a play (else a skip).
pub const PLAY_THRESHOLD_SECS: &str = "play_threshold_secs";

pub fn get(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM config WHERE key = ?1", [key], |r| r.get(0))
        .optional()
}

pub fn set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO config (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, key: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM config WHERE key = ?1", [key])?;
    Ok(())
}
