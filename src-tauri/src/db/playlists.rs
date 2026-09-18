use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::features::randomizer::SHADOW_SUFFIX;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistRow {
    pub id: String,
    pub name: String,
    pub owner_id: Option<String>,
    pub owner_name: Option<String>,
    pub track_count: i64,
    pub snapshot_id: Option<String>,
    pub image_url: Option<String>,
    pub uri: Option<String>,
    pub is_public: Option<bool>,
    pub is_collaborative: Option<bool>,
    pub position: i64,
    pub updated_at: i64,
    /// Derived: name ends with `-Utilify`.
    pub is_shadow: bool,
    /// Derived: owned by the signed-in user.
    pub is_own: bool,
}

fn from_row(row: &Row<'_>, current_user: Option<&str>) -> rusqlite::Result<PlaylistRow> {
    let name: String = row.get("name")?;
    let owner_id: Option<String> = row.get("owner_id")?;
    Ok(PlaylistRow {
        is_shadow: name.ends_with(SHADOW_SUFFIX),
        is_own: current_user.is_some() && owner_id.as_deref() == current_user,
        id: row.get("id")?,
        name,
        owner_id,
        owner_name: row.get("owner_name")?,
        track_count: row.get("track_count")?,
        snapshot_id: row.get("snapshot_id")?,
        image_url: row.get("image_url")?,
        uri: row.get("uri")?,
        is_public: row.get::<_, Option<i64>>("is_public")?.map(|v| v != 0),
        is_collaborative: row.get::<_, Option<i64>>("is_collaborative")?.map(|v| v != 0),
        position: row.get("position")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Replace the whole playlist cache with a fresh listing.
pub fn replace_all(conn: &mut Connection, rows: &[PlaylistRow]) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM playlists", [])?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO playlists
                (id, name, owner_id, owner_name, track_count, snapshot_id, image_url, uri,
                 is_public, is_collaborative, position, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )?;
        for p in rows {
            stmt.execute(params![
                p.id,
                p.name,
                p.owner_id,
                p.owner_name,
                p.track_count,
                p.snapshot_id,
                p.image_url,
                p.uri,
                p.is_public.map(|b| b as i64),
                p.is_collaborative.map(|b| b as i64),
                p.position,
                p.updated_at,
            ])?;
        }
    }
    tx.commit()
}

pub fn list(conn: &Connection, current_user: Option<&str>) -> rusqlite::Result<Vec<PlaylistRow>> {
    let mut stmt = conn.prepare("SELECT * FROM playlists ORDER BY position ASC")?;
    let rows = stmt.query_map([], |r| from_row(r, current_user))?;
    rows.collect()
}

pub fn get(conn: &Connection, id: &str, current_user: Option<&str>) -> rusqlite::Result<Option<PlaylistRow>> {
    conn.query_row("SELECT * FROM playlists WHERE id = ?1", [id], |r| from_row(r, current_user))
        .optional()
}

pub fn count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM playlists", [], |r| r.get(0))
}
