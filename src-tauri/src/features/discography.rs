//! Artist Discography: search an artist, list their releases, build a
//! deduplicated playlist from the chosen releases.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{AppError, Result};
use crate::features::generated::{self, GeneratedPlaylist};
use crate::features::matching::name_key;
use crate::spotify::artists;
use crate::spotify::models::Track;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistHit {
    pub id: String,
    pub name: String,
    pub image_url: Option<String>,
    pub genres: Vec<String>,
    pub followers: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumInfo {
    pub id: String,
    pub name: String,
    /// `album`, `single`, `compilation`, or `appears_on`.
    pub group: String,
    pub release_date: Option<String>,
    pub total_tracks: u32,
    pub image_url: Option<String>,
    pub artists: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscographyResult {
    pub playlist: GeneratedPlaylist,
    pub albums: usize,
    pub tracks_seen: usize,
    pub duplicates_skipped: usize,
    pub other_artist_skipped: usize,
    pub unplayable_skipped: usize,
    /// What was read but left out, so the result can say *which* tracks are
    /// missing and why. Capped; the counts above stay exact.
    pub skipped: Vec<SkippedTrack>,
}

/// A release the user ticked. The name travels with the id so a skipped track
/// can name its release, and the release that kept the copy, without spending
/// a request to look either up again.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedTrack {
    pub name: String,
    pub artists: String,
    /// Release this copy came from.
    pub release: String,
    /// `same-title`, `same-recording`, `other-artist` or `unplayable`.
    pub reason: String,
    /// Release whose copy was kept instead, for the two duplicate reasons.
    pub kept_from: Option<String>,
}

/// Ceiling on the reported list so a huge catalogue cannot bloat the payload.
const MAX_SKIPPED_REPORTED: usize = 500;

/// Listings are cached for a while so toggling options or revisiting an
/// artist never re-hits the API (a live run exhausted the quota that way).
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);

type Cache<T> = Mutex<HashMap<String, (Instant, Vec<T>)>>;

fn album_cache() -> &'static Cache<AlbumInfo> {
    static C: OnceLock<Cache<AlbumInfo>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn artist_cache() -> &'static Cache<ArtistHit> {
    static C: OnceLock<Cache<ArtistHit>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_get<T: Clone>(cache: &Cache<T>, key: &str) -> Option<Vec<T>> {
    let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .get(key)
        .filter(|(at, _)| at.elapsed() < CACHE_TTL)
        .map(|(_, v)| v.clone())
}

fn cache_put<T>(cache: &Cache<T>, key: String, value: Vec<T>) {
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    guard.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
    guard.insert(key, (Instant::now(), value));
}

pub async fn search(state: &AppState, query: &str) -> Result<Vec<ArtistHit>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let key = q.to_lowercase();
    if let Some(hit) = cache_get(artist_cache(), &key) {
        return Ok(hit);
    }
    let hits: Vec<ArtistHit> = artists::search_artists(&state.spotify, q)
        .await?
        .into_iter()
        .map(|a| ArtistHit {
            image_url: a.images.iter().min_by_key(|i| i.width.unwrap_or(u32::MAX)).map(|i| i.url.clone()),
            followers: a.followers.as_ref().and_then(|f| f.total),
            id: a.id,
            name: a.name,
            genres: a.genres,
        })
        .collect();
    cache_put(artist_cache(), key, hits.clone());
    Ok(hits)
}

const ALLOWED_GROUPS: [&str; 4] = ["album", "single", "compilation", "appears_on"];

/// Releases of an artist for the given `include_groups`. The UI fetches
/// `album,single,compilation` once per artist and `appears_on` only on
/// demand; both results are cached.
pub async fn albums(state: &AppState, artist_id: &str, groups: &[String]) -> Result<Vec<AlbumInfo>> {
    let mut groups: Vec<&str> = groups
        .iter()
        .map(String::as_str)
        .filter(|g| ALLOWED_GROUPS.contains(g))
        .collect();
    groups.sort_unstable();
    groups.dedup();
    if groups.is_empty() {
        return Err(AppError::other("Pick at least one release type."));
    }
    let key = format!("{artist_id}|{}", groups.join(","));
    if let Some(hit) = cache_get(album_cache(), &key) {
        return Ok(hit);
    }

    let mut out: Vec<AlbumInfo> = artists::artist_albums(&state.spotify, artist_id, &groups.join(","))
        .await?
        .into_iter()
        .map(|a| AlbumInfo {
            group: a
                .album_group
                .clone()
                .or_else(|| a.album_type.clone())
                .unwrap_or_else(|| "album".into()),
            image_url: a.images.iter().min_by_key(|i| i.width.unwrap_or(u32::MAX)).map(|i| i.url.clone()),
            artists: a.artists.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(", "),
            total_tracks: a.total_tracks.unwrap_or(0),
            release_date: a.release_date,
            id: a.id,
            name: a.name,
        })
        .collect();
    // Oldest first so the playlist reads chronologically.
    out.sort_by(|x, y| x.release_date.cmp(&y.release_date).then(x.name.cmp(&y.name)));
    cache_put(album_cache(), key, out.clone());
    Ok(out)
}

pub struct BuildRequest<'a> {
    pub artist_id: &'a str,
    pub artist_name: &'a str,
    pub albums: &'a [AlbumRef],
    pub name: &'a str,
    pub only_this_artist: bool,
    pub dedupe_by_name: bool,
    pub randomize: bool,
}

/// Decides which tracks reach the playlist and records every one it drops.
/// Separate from the fetching so the rules can be tested without the API.
#[derive(Default)]
struct Selection {
    seen_uri: HashMap<String, String>,
    seen_name: HashMap<String, String>,
    uris: Vec<String>,
    skipped: Vec<SkippedTrack>,
    tracks_seen: usize,
    duplicates: usize,
    other_artist: usize,
    unplayable: usize,
}

impl Selection {
    fn note(&mut self, t: &Track, release: &str, reason: &str, kept_from: Option<String>) {
        if self.skipped.len() >= MAX_SKIPPED_REPORTED {
            return;
        }
        self.skipped.push(SkippedTrack {
            name: t.name.clone(),
            artists: t.artist_names(),
            release: release.to_string(),
            reason: reason.to_string(),
            kept_from,
        });
    }

    fn consider(&mut self, t: &Track, release: &str, artist_id: &str, only_this_artist: bool, dedupe_by_name: bool) {
        self.tracks_seen += 1;

        if !t.is_playable_catalog_track() {
            self.unplayable += 1;
            self.note(t, release, "unplayable", None);
            return;
        }
        if only_this_artist && !t.artists.iter().any(|a| a.id.as_deref() == Some(artist_id)) {
            self.other_artist += 1;
            self.note(t, release, "other-artist", None);
            return;
        }
        let Some(uri) = t.uri.clone() else {
            self.unplayable += 1;
            self.note(t, release, "unplayable", None);
            return;
        };

        if let Some(kept) = self.seen_uri.get(&uri).cloned() {
            self.duplicates += 1;
            self.note(t, release, "same-recording", Some(kept));
            return;
        }
        if dedupe_by_name {
            let key = name_key(&t.name, &t.artist_names());
            if let Some(kept) = self.seen_name.get(&key).cloned() {
                self.duplicates += 1;
                self.note(t, release, "same-title", Some(kept));
                return;
            }
            self.seen_name.insert(key, release.to_string());
        }
        self.seen_uri.insert(uri.clone(), release.to_string());
        self.uris.push(uri);
    }
}

pub async fn build(state: &AppState, req: BuildRequest<'_>) -> Result<DiscographyResult> {
    if req.albums.is_empty() {
        return Err(AppError::other("Pick at least one release."));
    }
    let description = if req.name.trim().is_empty() {
        format!("{} discography", req.artist_name)
    } else {
        req.name.trim().to_string()
    };

    let mut sel = Selection::default();

    for (i, album) in req.albums.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
        for t in artists::album_tracks(&state.spotify, &album.id).await? {
            sel.consider(&t, &album.name, req.artist_id, req.only_this_artist, req.dedupe_by_name);
        }
    }

    let Selection {
        uris,
        skipped,
        tracks_seen,
        duplicates,
        other_artist,
        unplayable,
        ..
    } = sel;
    if !skipped.is_empty() {
        log::info!(
            "discography: {} track(s) left out ({duplicates} duplicate, {other_artist} other artist, {unplayable} unplayable)",
            duplicates + other_artist + unplayable
        );
    }

    let playlist = generated::create_from_uris(state, &description, uris, req.randomize).await?;
    Ok(DiscographyResult {
        playlist,
        albums: req.albums.len(),
        tracks_seen,
        duplicates_skipped: duplicates,
        other_artist_skipped: other_artist,
        unplayable_skipped: unplayable,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spotify::models::Artist;

    const MAC: &str = "artist-mac";

    fn track(name: &str, uri: &str, artists: &[(&str, &str)]) -> Track {
        Track {
            id: Some(uri.to_string()),
            uri: Some(format!("spotify:track:{uri}")),
            name: name.to_string(),
            kind: Some("track".into()),
            is_local: false,
            artists: artists
                .iter()
                .map(|(id, n)| Artist {
                    id: Some(id.to_string()),
                    name: n.to_string(),
                })
                .collect(),
            album: None,
            duration_ms: Some(180_000),
            linked_from: None,
        }
    }

    fn mac(name: &str, uri: &str) -> Track {
        track(name, uri, &[(MAC, "Mac Miller")])
    }

    /// The reported reason for the bug this was built for: a new single loses
    /// its title to an older release and vanishes with no explanation.
    #[test]
    fn a_later_single_loses_its_title_to_an_earlier_release_and_says_so() {
        let mut sel = Selection::default();
        // Releases are walked oldest first.
        sel.consider(&mac("Butterflies", "old"), "Swimming", MAC, true, true);
        sel.consider(&mac("Butterflies", "new"), "Butterflies", MAC, true, true);

        assert_eq!(sel.uris.len(), 1, "only the first copy is kept");
        assert_eq!(sel.duplicates, 1);
        assert_eq!(sel.skipped.len(), 1);
        let s = &sel.skipped[0];
        assert_eq!(s.name, "Butterflies");
        assert_eq!(s.release, "Butterflies");
        assert_eq!(s.reason, "same-title");
        assert_eq!(s.kept_from.as_deref(), Some("Swimming"));
    }

    #[test]
    fn title_dedupe_off_keeps_both_copies() {
        let mut sel = Selection::default();
        sel.consider(&mac("Butterflies", "old"), "Swimming", MAC, true, false);
        sel.consider(&mac("Butterflies", "new"), "Butterflies", MAC, true, false);
        assert_eq!(sel.uris.len(), 2);
        assert!(sel.skipped.is_empty());
    }

    #[test]
    fn the_same_recording_twice_is_named_as_such() {
        let mut sel = Selection::default();
        sel.consider(&mac("Circles", "c1"), "Circles", MAC, true, true);
        sel.consider(&mac("Circles", "c1"), "Best Of", MAC, true, true);
        assert_eq!(sel.uris.len(), 1);
        assert_eq!(sel.skipped[0].reason, "same-recording");
        assert_eq!(sel.skipped[0].kept_from.as_deref(), Some("Circles"));
    }

    #[test]
    fn edition_noise_collapses_but_different_songs_do_not() {
        let mut sel = Selection::default();
        sel.consider(&mac("Self Care", "a"), "Swimming", MAC, true, true);
        sel.consider(&mac("Self Care - Live", "b"), "Live EP", MAC, true, true);
        sel.consider(&mac("Self Care (feat. Someone)", "c"), "Deluxe", MAC, true, true);
        sel.consider(&mac("Selfish", "d"), "Other", MAC, true, true);
        assert_eq!(sel.uris.len(), 2, "two distinct songs survive");
        assert_eq!(sel.duplicates, 2);
        assert!(sel.skipped.iter().all(|s| s.reason == "same-title"));
    }

    #[test]
    fn a_guest_only_track_is_reported_not_silently_dropped() {
        let mut sel = Selection::default();
        sel.consider(
            &track("Someone Else's Song", "x", &[("other", "Another Artist")]),
            "Compilation",
            MAC,
            true,
            true,
        );
        assert!(sel.uris.is_empty());
        assert_eq!(sel.other_artist, 1);
        assert_eq!(sel.skipped[0].reason, "other-artist");
        assert_eq!(sel.skipped[0].artists, "Another Artist");

        // With the filter off it is kept.
        let mut open = Selection::default();
        open.consider(
            &track("Someone Else's Song", "x", &[("other", "Another Artist")]),
            "Compilation",
            MAC,
            false,
            true,
        );
        assert_eq!(open.uris.len(), 1);
        assert!(open.skipped.is_empty());
    }

    #[test]
    fn a_local_file_is_reported_as_unplayable() {
        let mut sel = Selection::default();
        let mut local = mac("Bootleg", "b");
        local.is_local = true;
        sel.consider(&local, "Mixtape", MAC, true, true);
        assert!(sel.uris.is_empty());
        assert_eq!(sel.unplayable, 1);
        assert_eq!(sel.skipped[0].reason, "unplayable");
        assert_eq!(sel.tracks_seen, 1, "it was still read");
    }

    #[test]
    fn the_reported_list_is_capped_but_the_counts_are_not() {
        let mut sel = Selection::default();
        sel.consider(&mac("Keeper", "k"), "Album", MAC, true, true);
        for i in 0..(MAX_SKIPPED_REPORTED + 50) {
            sel.consider(&mac("Keeper", &format!("dup{i}")), "Reissue", MAC, true, true);
        }
        assert_eq!(sel.skipped.len(), MAX_SKIPPED_REPORTED);
        assert_eq!(sel.duplicates, MAX_SKIPPED_REPORTED + 50);
    }
}
