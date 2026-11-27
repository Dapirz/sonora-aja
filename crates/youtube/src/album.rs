//! The album version of a music video: the plain recording YouTube Music files as a song, found
//! by a signed-out song search for the video's title and artist. Only a sure match counts, so a
//! live take, a remix or another artist's cover never stands in for the video.

use std::time::Duration;

use anyhow::Result;
use music::{SearchKind, Track};
use ytmusic::YtMusic;

use crate::search;

/// How many of the search's first hits are weighed. Further down, a match is more likely
/// another recording of the same song.
const WEIGHED: usize = 5;
/// How much longer than its song a music video may run, at the least: an intro or a skit.
const LONGER: Duration = Duration::from_secs(30);

/// Words in a bracketed part of a video title that only mark it as a video, so the part says
/// nothing about the recording.
const VIDEO_MARKS: &[&str] = &[
    "official",
    "video",
    "mv",
    "visualizer",
    "visualiser",
    "lyric",
    "lyrics",
    "audio",
    "hd",
    "4k",
    "hq",
    "clip",
    "music",
];

/// The song `video` is the music video of, or `None` when no hit is surely the same recording.
pub async fn version(api: &YtMusic, video: &Track) -> Result<Option<Track>> {
    let title = song_title(&video.name, &video.artists);
    let Some(artist) = artists(video).into_iter().next() else {
        return Ok(None);
    };
    let query = format!("{title} {artist}");
    let mut found = music::Found::empty();
    search::file(
        &mut found,
        SearchKind::Tracks,
        search::first(api, &query, SearchKind::Tracks).await,
    );
    Ok(found
        .tracks?
        .into_iter()
        .take(WEIGHED)
        .find(|hit| same_song(video, hit)))
}

/// The official music video of `song`, or `None` when no video is surely of the same
/// recording. Only an upload credited to the song's own artists counts, so a lyric video from
/// another channel never does.
pub async fn video(api: &YtMusic, song: &Track) -> Result<Option<Track>> {
    let Some(artist) = artists(song).into_iter().next() else {
        return Ok(None);
    };
    let query = format!("{} {artist}", song.name);
    let mut found = music::Found::empty();
    search::file(
        &mut found,
        SearchKind::Videos,
        search::first(api, &query, SearchKind::Videos).await,
    );
    Ok(found
        .videos?
        .into_iter()
        .take(WEIGHED)
        .find(|hit| hit.album_id.is_none() && same_song(hit, song)))
}

/// Whether `hit`, a song, is the recording `video` shows: the same title once the video's own
/// markings are off, every artist of the video credited on the song, and a length that fits.
/// A video can run longer than its song, or be cut down from it.
fn same_song(video: &Track, hit: &Track) -> bool {
    let wanted = plain(&song_title(&video.name, &video.artists));
    let fits = !video.duration.is_zero()
        && !hit.duration.is_zero()
        && hit.duration + LONGER.max(video.duration / 5) >= video.duration
        && hit.duration <= video.duration * 2;
    let credited = artists(video)
        .iter()
        .all(|artist| artists(hit).iter().any(|other| other == artist));
    hit.album_id.is_some() && !wanted.is_empty() && wanted == plain(&hit.name) && credited && fits
}

/// A video's title as its song would be called: without the "Artist - " some lead with and
/// without the brackets that only say it is a video.
fn song_title(title: &str, artists: &str) -> String {
    let title = match title.split_once(" - ") {
        Some((lead, rest)) if !rest.trim().is_empty() && names(artists).contains(&words(lead)) => {
            rest
        }
        _ => title,
    };
    let mut kept = String::new();
    let mut part = String::new();
    let mut depth = 0u32;
    for c in title.chars() {
        match c {
            '(' | '[' => {
                depth += 1;
                if depth == 1 {
                    part.clear();
                    continue;
                }
                part.push(c);
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                match depth {
                    0 if marks_video(&part) => {}
                    0 => {
                        kept.push('(');
                        kept.push_str(&part);
                        kept.push(')');
                    }
                    _ => part.push(c),
                }
            }
            _ if depth > 0 => part.push(c),
            _ => kept.push(c),
        }
    }
    kept.trim().to_owned()
}

/// Whether a bracketed part only marks a title as a video, as "Official Music Video" does.
fn marks_video(part: &str) -> bool {
    // "M/V" comes apart into two letters once its slash is gone.
    let part = words(part).replace("m v", "mv");
    !part.is_empty() && part.split(' ').all(|word| VIDEO_MARKS.contains(&word))
}

/// A title reduced to what tells recordings apart: lowercase words, with the brackets that do
/// not change the recording left out, as a featured artist or a remaster.
fn plain(title: &str) -> String {
    let mut kept = String::new();
    let mut part = String::new();
    let mut depth = 0u32;
    for c in title.chars() {
        match c {
            '(' | '[' => {
                depth += 1;
                part.clear();
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 && !harmless(&part) {
                    kept.push(' ');
                    kept.push_str(&part);
                }
            }
            _ if depth > 0 => part.push(c),
            _ => kept.push(c),
        }
    }
    words(&kept)
}

/// Whether a bracketed part leaves the recording as it is: a guest credit or a remaster.
fn harmless(part: &str) -> bool {
    let part = words(part);
    let first = part.split(' ').next().unwrap_or_default();
    matches!(first, "feat" | "ft" | "featuring" | "with") || part.contains("remaster")
}

/// The artists credited on `track`, as compared names. A channel name's "VEVO" or " - Topic"
/// is left off, since the same artist goes by the plain name on their songs.
fn artists(track: &Track) -> Vec<String> {
    let mut names: Vec<String> = match track.artist_refs.is_empty() {
        true => names(&track.artists),
        false => track
            .artist_refs
            .iter()
            .map(|artist| artist_name(&artist.name))
            .collect(),
    };
    names.retain(|name| !name.is_empty());
    names
}

/// Each name in a credit line such as "A, B & C".
fn names(line: &str) -> Vec<String> {
    line.split([',', '&'])
        .map(artist_name)
        .filter(|name| !name.is_empty())
        .collect()
}

fn artist_name(name: &str) -> String {
    let name = name.trim();
    let name = name.strip_suffix(" - Topic").unwrap_or(name);
    let name = name.strip_suffix("VEVO").unwrap_or(name);
    words(name)
}

/// Lowercase letters and digits, one space between words, so punctuation and case never tell
/// two names apart.
fn words(text: &str) -> String {
    text.chars()
        .map(|c| match c.is_alphanumeric() {
            true => c.to_lowercase().next().unwrap_or(c),
            false => ' ',
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
