//! Search, a page at a time. It goes out signed out, as every search of this provider does,
//! because YouTube files each signed-in search in the account's search history and the app
//! searches while the query is still being typed.

use anyhow::Result;
use music::{Found, SearchKind, Track};
use serde_json::{Value, json};
use ytmusic::nav::Nav as _;
use ytmusic::{Client, YtMusic, parse};

use crate::{subscriptions, wire};

/// The rows of one page of a filtered search and the token for the page after it.
pub struct Rows {
    rows: Vec<Value>,
    next: Option<String>,
}

/// The first page of `kind` for `query`.
pub async fn first(api: &YtMusic, query: &str, kind: SearchKind) -> Result<Rows> {
    let response = api
        .execute_with(
            "search",
            Client::Music,
            json!({ "query": query, "params": filter(kind) }),
            false,
        )
        .await?;
    Ok(rows(&response))
}

/// The page a continuation `token` of an earlier page names.
pub async fn more(api: &YtMusic, token: &str) -> Result<Rows> {
    let response = api
        .execute_with(
            "search",
            Client::Music,
            json!({ "continuation": token }),
            false,
        )
        .await?;
    Ok(rows(&response))
}

/// Files a page of `kind` into `found`: what it holds, or why it failed, and the token for the
/// page after it.
pub fn file(found: &mut Found, kind: SearchKind, page: Result<Rows>) {
    let page = match page {
        Ok(page) => page,
        Err(error) => {
            let error = error.context(format!("cannot search youtube music for {kind:?}"));
            match kind {
                SearchKind::Tracks => found.tracks = Err(error),
                SearchKind::Videos => found.videos = Err(error),
                SearchKind::Albums => found.albums = Err(error),
                SearchKind::Playlists => found.playlists = Err(error),
                SearchKind::Artists => found.artists = Err(error),
            }
            return;
        }
    };
    if let Some(next) = page.next {
        found.more.insert(kind, next);
    }
    let rows = page.rows.iter();
    match kind {
        SearchKind::Tracks => found.tracks = Ok(tracks(rows)),
        SearchKind::Videos => found.videos = Ok(tracks(rows)),
        SearchKind::Albums => {
            found.albums = Ok(rows
                .filter_map(parse::list_item_album)
                .map(wire::album)
                .collect());
        }
        SearchKind::Playlists => {
            found.playlists = Ok(rows
                .filter_map(parse::list_item_playlist)
                .map(|playlist| wire::playlist(playlist, false, false))
                .collect());
        }
        SearchKind::Artists => {
            found.artists = Ok(rows
                .filter_map(|row| row.at(&["musicResponsiveListItemRenderer"]))
                .filter_map(subscriptions::saved_artist)
                .collect());
        }
    }
}

/// The search filter that asks YouTube Music for `kind` alone.
fn filter(kind: SearchKind) -> &'static str {
    match kind {
        SearchKind::Tracks => ytmusic::search::SONGS,
        SearchKind::Videos => ytmusic::search::VIDEOS,
        SearchKind::Albums => ytmusic::search::ALBUMS,
        SearchKind::Playlists => ytmusic::search::PLAYLISTS,
        SearchKind::Artists => ytmusic::search::ARTISTS,
    }
}

/// The rows of a first page's shelves or a continuation's, and the next page's token.
fn rows(response: &Value) -> Rows {
    let shelves: Vec<&Value> = parse::find_renderers(response, "musicShelfRenderer")
        .into_iter()
        .chain(parse::find_renderers(response, "musicShelfContinuation"))
        .collect();
    Rows {
        rows: shelves
            .iter()
            .flat_map(|shelf| shelf.items(&["contents"]))
            .cloned()
            .collect(),
        next: shelves
            .iter()
            .find_map(|shelf| parse::shelf_continuation(shelf)),
    }
}

/// The playable tracks among `rows`, numbered in their order.
fn tracks<'a>(rows: impl Iterator<Item = &'a Value>) -> Vec<Track> {
    rows.filter_map(parse::list_item_track)
        .filter(|track| track.video_id.is_some())
        .enumerate()
        .map(|(index, track)| wire::track(track, index as u32))
        .collect()
}
