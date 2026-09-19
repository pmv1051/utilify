pub mod bench;
pub mod diff;
pub mod discography;
pub mod duplicates;
pub mod editor;
pub mod export_import;
pub mod generated;
pub mod matching;
pub mod merge;
pub mod randomizer;
pub mod stats;
pub mod tracks;
pub mod updater;

use crate::db::playlists::PlaylistRow;
use crate::db::{self, now, Db};
use crate::error::Result;
use crate::spotify::models::SimplifiedPlaylist;

/// Store a fresh `/me/playlists` listing in the local cache.
pub fn cache_playlists(database: &Db, list: &[SimplifiedPlaylist]) -> Result<()> {
    let ts = now();
    let rows: Vec<PlaylistRow> = list
        .iter()
        .enumerate()
        .map(|(i, p)| PlaylistRow {
            id: p.id.clone(),
            name: p.name.clone(),
            owner_id: p.owner_id().map(String::from),
            owner_name: p.owner.as_ref().and_then(|o| o.display_name.clone()),
            track_count: p.track_count() as i64,
            snapshot_id: p.snapshot_id.clone(),
            image_url: p.thumbnail(),
            uri: p.uri.clone(),
            is_public: p.public,
            is_collaborative: p.collaborative,
            position: i as i64,
            updated_at: ts,
            is_shadow: false,
            is_own: false,
        })
        .collect();
    database.with_mut(|c| db::playlists::replace_all(c, &rows))
}
