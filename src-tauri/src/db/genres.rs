//! Artist → genre tag cache.

use rusqlite::{params, params_from_iter, Connection};
use std::collections::HashMap;

pub struct ArtistGenres {
    pub artist_id: String,
    pub name: Option<String>,
    pub genres: Vec<String>,
}

/// Genres for the given ids that are already cached (newer than `min_cached_at`).
pub fn get_many(conn: &Connection, ids: &[String], min_cached_at: i64) -> rusqlite::Result<HashMap<String, ArtistGenres>> {
    let mut out = HashMap::new();
    for chunk in ids.chunks(400) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT artist_id, name, genres FROM artist_genres
             WHERE cached_at >= {min_cached_at} AND artist_id IN ({placeholders})"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(chunk.iter()), |r| {
            let genres_json: String = r.get(2)?;
            Ok(ArtistGenres {
                artist_id: r.get(0)?,
                name: r.get(1)?,
                genres: serde_json::from_str(&genres_json).unwrap_or_default(),
            })
        })?;
        for row in rows {
            let a = row?;
            out.insert(a.artist_id.clone(), a);
        }
    }
    Ok(out)
}

pub fn put(conn: &Connection, a: &ArtistGenres, now: i64) -> rusqlite::Result<()> {
    let genres_json = serde_json::to_string(&a.genres)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "INSERT INTO artist_genres (artist_id, name, genres, cached_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(artist_id) DO UPDATE SET name = excluded.name, genres = excluded.genres, cached_at = excluded.cached_at",
        params![a.artist_id, a.name, genres_json, now],
    )?;
    Ok(())
}
