//! The playlist model: entries, selection, reordering, totals, and play order.
//!
//! Entries have stable ids so the current track and pending metadata survive reordering.
//! An entry can exist before its audio does (`Waiting`), or be kept for the record without
//! ever having audio (`Unavailable`); neither ever reaches the engine, so the engine's
//! "track failed" keeps meaning "this file is broken".

use std::collections::{BTreeSet, HashMap, HashSet};

use platform::TrackRef;

use crate::columns::{Dir, Field};
use serde::{Deserialize, Serialize};

pub type EntryId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryStatus {
    /// Metadata not read yet.
    Pending,
    Ready,
    /// Could not be opened or decoded.
    Failed,
    /// The audio isn't local yet, and why.
    Waiting(WaitKind),
    /// Will never play; kept for the record.
    Unavailable(UnavailableKind),
}

/// Why an entry is waiting for its audio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitKind {
    /// Its record is listed; its details haven't arrived yet.
    Listed,
    /// Its preview will download when it nears the playhead.
    Queued,
    /// Its preview is downloading (percent).
    Downloading(u8),
    /// Previews can't download until yt-dlp is found.
    NeedsYtDlp,
    /// A track of a record with no clip: its preview is searched for near the playhead.
    Search,
    Other(String),
}

/// Why an entry will never play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnavailableKind {
    /// Its record has no usable clip.
    NoClip,
    /// Its clip failed to download twice.
    ClipFailed,
    /// A track of a record with no clip, for which a search found no usable video.
    NotFound,
    /// A track whose search found a video another entry of the crate already has.
    AlreadyInCrate,
    Other(String),
}

impl std::fmt::Display for WaitKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WaitKind::Listed => f.write_str("listed"),
            WaitKind::Queued => f.write_str("queued"),
            WaitKind::Downloading(p) => write!(f, "downloading {p}%"),
            WaitKind::NeedsYtDlp => f.write_str("needs yt-dlp"),
            WaitKind::Search => f.write_str("to search"),
            WaitKind::Other(t) => f.write_str(t),
        }
    }
}

impl std::fmt::Display for UnavailableKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnavailableKind::NoClip => f.write_str("no clip"),
            UnavailableKind::ClipFailed => f.write_str("clip failed"),
            UnavailableKind::NotFound => f.write_str("not found by search"),
            UnavailableKind::AlreadyInCrate => f.write_str("already in crate"),
            UnavailableKind::Other(t) => f.write_str(t),
        }
    }
}

/// The wording back to its kind: saved crates store the wording, and the dig crate reports
/// reasons as text. Anything unknown is kept as `Other`.
impl From<&str> for WaitKind {
    fn from(t: &str) -> Self {
        match t {
            "listed" => WaitKind::Listed,
            "queued" => WaitKind::Queued,
            "needs yt-dlp" => WaitKind::NeedsYtDlp,
            "to search" => WaitKind::Search,
            _ => t
                .strip_prefix("downloading ")
                .and_then(|p| p.strip_suffix('%'))
                .and_then(|p| p.parse().ok())
                .map_or_else(|| WaitKind::Other(t.to_owned()), WaitKind::Downloading),
        }
    }
}

impl From<String> for WaitKind {
    fn from(t: String) -> Self {
        t.as_str().into()
    }
}

impl From<&str> for UnavailableKind {
    fn from(t: &str) -> Self {
        match t {
            "no clip" => UnavailableKind::NoClip,
            "clip failed" => UnavailableKind::ClipFailed,
            "not found by search" => UnavailableKind::NotFound,
            "already in crate" => UnavailableKind::AlreadyInCrate,
            _ => UnavailableKind::Other(t.to_owned()),
        }
    }
}

impl From<String> for UnavailableKind {
    fn from(t: String) -> Self {
        t.as_str().into()
    }
}

impl EntryStatus {
    /// Has a local file the engine can be given.
    pub fn is_playable(&self) -> bool {
        matches!(self, EntryStatus::Pending | EntryStatus::Ready)
    }

    /// Playable now or once its audio arrives: it has a place in the play order.
    pub fn in_play_order(&self) -> bool {
        self.is_playable() || matches!(self, EntryStatus::Waiting(_))
    }

    /// What a waiting or unavailable entry is waiting for, or why it won't play, in words
    /// ("downloading 40%", "no clip").
    pub fn note(&self) -> Option<String> {
        match self {
            EntryStatus::Waiting(w) => Some(w.to_string()),
            EntryStatus::Unavailable(u) => Some(u.to_string()),
            _ => None,
        }
    }
}

/// The record an entry belongs to, for entries sent from a catalogue page.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Origin {
    /// The page it was sent from.
    pub page: String,
    pub release: Option<u64>,
    pub master: Option<u64>,
    pub label: String,
    pub catno: String,
    pub year: Option<u16>,
    /// Side, e.g. "A1".
    pub position: String,
    /// The clip the audio comes from, e.g. a video id.
    pub clip: Option<String>,
    /// Copies for sale when the record was last looked up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub for_sale: Option<ForSale>,
    /// The record's title: the album the entry belongs to.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub album: String,
    /// The address of the record's cover thumbnail.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cover: String,
    /// The record's Discogs styles, comma-separated ("Deep House, Minimal").
    #[serde(skip_serializing_if = "String::is_empty")]
    pub styles: String,
    /// The record's credited artist as Discogs shows it ("Various" for a compilation), the
    /// same on every track whatever the track's own credit.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub artist: String,
    /// The record's formats, vinyl first ("Vinyl", "File", "Vinyl, CD"); empty when unknown.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub formats: String,
    /// A track no clip of its record matches: what its search result is found by (the
    /// track key, or `release/<id>/<position>` when saved before track keys).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub search_key: String,
    /// The title of the video a search found for it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub found: String,
    /// The record's full-album upload, held back while its tracks are searched for: added
    /// after the record's entries once one of them is not found.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub album_clip: String,
    /// That upload's title, for its entry.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub album_clip_title: String,
    /// The track's Bandcamp page, when it is on Bandcamp.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub bandcamp: String,
    /// Its Bandcamp clip (`bc.‹track id›`): the clip in use, or the one to switch to.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub bandcamp_clip: String,
    /// Its YouTube clip, set aside while the Bandcamp one is in use.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub youtube_clip: String,
}

/// Where an entry's audio comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipSource {
    YouTube,
    Bandcamp,
}

impl ClipSource {
    /// Bandcamp clips are `bc.‹track id›`; a YouTube id never has a dot.
    pub fn of(clip: &str) -> Self {
        if clip.starts_with("bc.") {
            ClipSource::Bandcamp
        } else {
            ClipSource::YouTube
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ClipSource::YouTube => "YouTube",
            ClipSource::Bandcamp => "Bandcamp",
        }
    }

    /// The row's badge.
    pub fn badge(self) -> &'static str {
        match self {
            ClipSource::YouTube => "YT",
            ClipSource::Bandcamp => "BC",
        }
    }
}

impl Origin {
    /// Where the clip in use comes from (`None`: no clip yet).
    pub fn source(&self) -> Option<ClipSource> {
        self.clip.as_deref().map(ClipSource::of)
    }

    /// The YouTube clip, in use or set aside.
    pub fn youtube(&self) -> Option<&str> {
        match self.source() {
            Some(ClipSource::YouTube) => self.clip.as_deref(),
            _ => Some(self.youtube_clip.as_str()).filter(|c| !c.is_empty()),
        }
    }

    /// The source the entry could switch to: the other one, when it has both.
    pub fn other_source(&self) -> Option<ClipSource> {
        match self.source()? {
            ClipSource::YouTube => (!self.bandcamp_clip.is_empty()).then_some(ClipSource::Bandcamp),
            ClipSource::Bandcamp => self.youtube().map(|_| ClipSource::YouTube),
        }
    }

    /// Switches the clip in use to `to`; true if it changed.
    pub fn switch_to(&mut self, to: ClipSource) -> bool {
        if self.other_source() != Some(to) {
            return false;
        }
        match to {
            ClipSource::Bandcamp => {
                self.youtube_clip = self.clip.take().unwrap_or_default();
                self.clip = Some(self.bandcamp_clip.clone());
            }
            ClipSource::YouTube => {
                self.clip = Some(std::mem::take(&mut self.youtube_clip));
            }
        }
        true
    }
}

/// A marketplace snapshot: how many copies are for sale, and the cheapest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ForSale {
    pub count: u32,
    /// The lowest price in hundredths of `currency` (yen too, so every currency is alike).
    pub lowest_cents: Option<u64>,
    /// ISO code, e.g. "EUR".
    pub currency: String,
    /// Seconds since the Unix epoch.
    pub fetched_at: u64,
}

/// A copy for sale in a seller crate (Top Sellers): one listing of a record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SaleCopy {
    /// The Discogs listing (`/sell/item/{listing}`).
    pub listing: u64,
    pub release: u64,
    /// The price in hundredths of `currency`.
    pub cents: u64,
    /// ISO code, e.g. "EUR".
    pub currency: String,
    /// "VG+ / VG": media, then sleeve.
    pub grades: String,
    pub ships_from: String,
    /// When it was listed ("2026-02-06T08:12:00-08:00").
    pub posted: String,
    pub comments: String,
    /// No longer for sale.
    pub sold: bool,
    /// The price before the last refresh changed it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub was_cents: Option<u64>,
}

impl SaleCopy {
    pub fn url(&self) -> String {
        format!("https://www.discogs.com/sell/item/{}", self.listing)
    }
}

/// An entry to add in place of another (see [`Playlist::replace`]).
#[derive(Debug, Clone, PartialEq)]
pub struct NewEntry {
    pub artist: String,
    pub title: String,
    pub source: Option<String>,
    pub origin: Option<Origin>,
    /// A duration known before the file is read.
    pub duration: Option<f64>,
    /// Why it waits (usually queued).
    pub status: WaitKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: EntryId,
    /// The local file; empty while waiting.
    pub track: TrackRef,
    /// Where the audio comes from (a URL), for remote entries.
    pub source: Option<String>,
    pub origin: Option<Origin>,
    pub title: String,
    pub artist: String,
    /// A local file's album tag (an entry with an origin uses its record's title instead).
    pub album: String,
    pub duration: Option<f64>,
    /// Tempo from analysis, folded into the DJ range (see [`crate::format::dj_bpm`]).
    pub bpm: Option<u16>,
    pub status: EntryStatus,
}

impl Entry {
    /// "(catno) Artist: Title (N BPM)", leaving out what isn't known (messages and the title
    /// line use it; rows add the album, see [`Self::row_name`]).
    pub fn display_name(&self) -> String {
        let catno = self.origin.as_ref().map_or("", |o| o.catno.as_str());
        crate::format::entry_name(catno, &self.artist, &self.title, "", self.bpm)
    }

    /// The playlist row's name: [`Self::display_name`] with the album after the title.
    pub fn row_name(&self) -> String {
        let catno = self.origin.as_ref().map_or("", |o| o.catno.as_str());
        crate::format::entry_name(catno, &self.artist, &self.title, self.album(), self.bpm)
    }

    /// The album it belongs to: its record's title, or a local file's album tag.
    pub fn album(&self) -> &str {
        match &self.origin {
            Some(o) => &o.album,
            None => &self.album,
        }
    }

    /// "Artist - Title", or just the title: the plain name other players expect (M3U).
    pub fn plain_name(&self) -> String {
        if self.artist.is_empty() {
            self.title.clone()
        } else {
            format!("{} - {}", self.artist, self.title)
        }
    }

    /// The album it belongs to, if any (see [`AlbumKey`]).
    pub fn album_key(&self) -> Option<AlbumKey> {
        match &self.origin {
            Some(o) => match (o.release, o.master) {
                (Some(r), _) => Some(AlbumKey::Release(r)),
                (None, Some(m)) => Some(AlbumKey::Master(m)),
                // From Bandcamp only: its album's page.
                _ if !o.bandcamp.is_empty() && !o.page.is_empty() => {
                    Some(AlbumKey::Bandcamp(o.page.clone()))
                }
                _ => None,
            },
            None if self.album.trim().is_empty() => None,
            None => Some(AlbumKey::Local(
                self.artist.trim().to_lowercase(),
                self.album.trim().to_lowercase(),
            )),
        }
    }

    /// The record's values for a filter (see [`Facet`]), none for a local file: its styles
    /// ("Deep House", "Minimal"), its credited artist, or its first label.
    pub fn values(&self, facet: Facet) -> impl Iterator<Item = &str> {
        let o = self.origin.as_ref();
        let (text, list) = match facet {
            Facet::Style => (o.map_or("", |o| o.styles.as_str()), true),
            Facet::Artist => (o.map_or("", |o| o.artist.as_str()), false),
            Facet::Label => (o.map_or("", |o| o.label.as_str()), false),
            Facet::Format => (o.map_or("", |o| o.formats.as_str()), true),
        };
        text.split(move |c| list && c == ',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// The mark of a record known in some format but not on vinyl ("FILE", "CD", "CASS",
    /// "OTHER", from its first format); `None` for vinyl, local files and unknown formats.
    pub fn format_mark(&self) -> Option<&'static str> {
        let formats = &self.origin.as_ref()?.formats;
        if formats.split(',').any(|f| f.trim() == "Vinyl") {
            return None;
        }
        match formats.split(',').next()?.trim() {
            "" => None,
            "File" => Some("FILE"),
            "CD" => Some("CD"),
            "Cassette" => Some("CASS"),
            _ => Some("OTHER"),
        }
    }

    /// What Send to crate compares: the origin's clip when there is one, otherwise the file.
    pub fn duplicate_key(&self) -> DuplicateKey {
        match self.origin.as_ref().and_then(|o| o.clip.clone()) {
            Some(clip) => DuplicateKey::Clip(clip),
            None => DuplicateKey::File(self.track.0.clone()),
        }
    }

    fn to_saved(&self) -> SavedEntry {
        SavedEntry {
            path: self.track.0.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            duration: self.duration,
            bpm: self.bpm,
            source: self.source.clone(),
            origin: self.origin.clone(),
            status: match &self.status {
                EntryStatus::Waiting(w) => SavedStatus::Waiting(w.to_string()),
                EntryStatus::Unavailable(u) => SavedStatus::Unavailable(u.to_string()),
                _ => SavedStatus::Local,
            },
        }
    }
}

/// Which album an entry belongs to: entries with equal keys form one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AlbumKey {
    /// A Discogs release: another pressing of the same master is another album.
    Release(u64),
    /// A master release, for an entry that has no release.
    Master(u64),
    /// A Bandcamp album's page, for an entry that is on Discogs neither way.
    Bandcamp(String),
    /// A local file's artist and album tags, lower-cased, so two "Greatest Hits" stay apart.
    Local(String, String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DuplicateKey {
    Clip(String),
    File(String),
}

/// Saved form of the playlist (metadata cached so a restart shows titles immediately).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedPlaylist {
    pub entries: Vec<SavedEntry>,
    pub current: Option<usize>,
    /// The BPM filter's range, when narrower than the crate's tempos.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm_range: Option<(u16, u16)>,
    /// The styles the style filter shows, when any is selected.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub styles: BTreeSet<String>,
    /// The record artists the artist filter shows, when any is picked.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub artists: BTreeSet<String>,
    /// The labels the label filter shows, when any is picked.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub labels: BTreeSet<String>,
    /// The formats the format filter shows, when any is picked.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub formats: BTreeSet<String>,
    /// A seller crate's copies for sale.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copies: Vec<SaleCopy>,
    /// The CART switch: only records with a copy in the user's cart show.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cart_only: bool,
}

/// What a record can be filtered by, besides its tempo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Facet {
    /// Its Discogs styles (any of them).
    Style,
    /// Its credited artist, as Discogs shows it.
    Artist,
    /// Its first label.
    Label,
    /// Its formats (Vinyl, File, CD, Cassette, Other; any of them).
    Format,
}

impl Facet {
    pub const ALL: [Facet; 4] = [Facet::Style, Facet::Artist, Facet::Label, Facet::Format];

    fn index(self) -> usize {
        self as usize
    }

    /// "style", "artist", "label".
    pub fn name(self) -> &'static str {
        match self {
            Facet::Style => "style",
            Facet::Artist => "artist",
            Facet::Label => "label",
            Facet::Format => "format",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedEntry {
    pub path: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub album: String,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    #[serde(default, skip_serializing_if = "SavedStatus::is_local")]
    pub status: SavedStatus,
}

/// Whether a saved entry has local audio; Pending/Ready/Failed are worked out again at load.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SavedStatus {
    #[default]
    Local,
    Waiting(String),
    Unavailable(String),
}

impl SavedStatus {
    fn is_local(&self) -> bool {
        *self == SavedStatus::Local
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClickMods {
    pub shift: bool,
    /// Cmd on macOS, Ctrl elsewhere.
    pub command: bool,
}

#[derive(Debug, Default)]
pub struct Playlist {
    entries: Vec<Entry>,
    selected: BTreeSet<EntryId>,
    anchor: Option<EntryId>,
    current: Option<EntryId>,
    /// The keyboard cursor: an entry, so it stays put while entries arrive around it.
    cursor: Option<EntryId>,
    /// The field the entries were last sorted by, until they are reordered otherwise.
    sorted: Option<(Field, Dir)>,
    /// The BPM filter, as set (see [`Playlist::bpm_filter`] for what applies).
    bpm_range: Option<(u16, u16)>,
    /// The style, artist and label filters, as set (see [`Playlist::filter`] for what
    /// applies), by [`Facet`].
    picked: [BTreeSet<String>; 4],
    next_id: EntryId,
    /// Shown grouped by record: each album's entries are kept together (see
    /// [`Playlist::gather`]).
    grouped: bool,
    /// Entries were added (or an album became known) while grouped: the next
    /// [`Playlist::settle`] places them with their record.
    unsettled: bool,
    /// Bumped whenever the entries, their order, their albums or the filter change, so views
    /// built from them know when to rebuild.
    rev: u64,
    /// A seller crate's copies for sale, in the order they came.
    copies: Vec<SaleCopy>,
    /// The CART switch is on.
    cart_only: bool,
    /// Releases with a copy in the user's cart (kept up to date by the app).
    in_cart: BTreeSet<u64>,
    /// The search's words, folded as the skin font draws them (see [`fold_words`]). Not
    /// saved: a search lasts while its crate is shown.
    search: Vec<Vec<char>>,
}

/// `text` split on whitespace, each word folded with [`crate::skin::fold`] (upper case,
/// accents stripped, char for char).
pub fn fold_words(text: &str) -> Vec<Vec<char>> {
    text.split_whitespace()
        .map(|w| w.chars().map(crate::skin::fold).collect())
        .collect()
}

/// Whether `rest` starts with `word`, comparing folded chars.
fn starts_folded(rest: &str, word: &[char]) -> bool {
    let mut chars = rest.chars().map(crate::skin::fold);
    word.iter().all(|&w| chars.next() == Some(w))
}

/// Whether `hay` contains `word` (folded); no allocation, as it runs over every entry while a
/// search is typed.
fn contains_folded(hay: &str, word: &[char]) -> bool {
    hay.char_indices()
        .any(|(i, _)| starts_folded(&hay[i..], word))
}

/// Which chars of `text` a search word covers, for highlighting a row (folding keeps one char
/// per char, so the marks line up with what is drawn).
pub fn search_marks(text: &str, words: &[Vec<char>]) -> Vec<bool> {
    let mut marks = vec![false; text.chars().count()];
    for (ci, (bi, _)) in text.char_indices().enumerate() {
        for w in words.iter().filter(|w| starts_folded(&text[bi..], w)) {
            marks[ci..ci + w.len()].fill(true);
        }
    }
    marks
}

/// What the filters show: an entry whose tempo is in the BPM range (when one is set) and that
/// has, for each of style, artist and label that filters, one of the picked values. An entry
/// with no tempo, or no value for a filter, shows only while that filter is off.
#[derive(Debug, Clone, Copy)]
pub struct Shown<'a> {
    bpm: Option<(u16, u16)>,
    picks: [Option<&'a BTreeSet<String>>; 4],
    /// The CART switch: the releases in the cart.
    cart: Option<&'a BTreeSet<u64>>,
    /// The search's folded words, every one of which an entry must contain.
    search: &'a [Vec<char>],
}

impl Shown<'_> {
    pub fn shows(&self, e: &Entry) -> bool {
        self.bpm
            .is_none_or(|(lo, hi)| e.bpm.is_some_and(|b| (lo..=hi).contains(&b)))
            && Facet::ALL
                .into_iter()
                .all(|f| self.picks[f.index()].is_none_or(|on| e.values(f).any(|v| on.contains(v))))
            && self.cart.is_none_or(|releases| {
                e.origin
                    .as_ref()
                    .and_then(|o| o.release)
                    .is_some_and(|r| releases.contains(&r))
            })
            && self.search.iter().all(|w| {
                let o = e.origin.as_ref();
                [
                    e.artist.as_str(),
                    &e.title,
                    e.album(),
                    o.map_or("", |o| &o.artist),
                    o.map_or("", |o| &o.label),
                    o.map_or("", |o| &o.catno),
                ]
                .into_iter()
                .any(|field| contains_folded(field, w))
            })
    }

    pub fn is_filtered(&self) -> bool {
        self.bpm.is_some()
            || self.picks.iter().any(Option::is_some)
            || self.cart.is_some()
            || !self.search.is_empty()
    }
}

/// How far the keyboard cursor moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorMove {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
}

impl Playlist {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Changes whenever the entries, their order, their albums or the BPM filter change.
    pub fn rev(&self) -> u64 {
        self.rev
    }

    fn changed(&mut self) {
        self.rev += 1;
    }

    // ---- grouped by record ---------------------------------------------------------------

    pub fn is_grouped(&self) -> bool {
        self.grouped
    }

    /// Groups (or ungroups) the entries by record. Grouping gathers them at once; ungrouping
    /// keeps the order. Returns whether the order changed.
    pub fn set_grouped(&mut self, on: bool) -> bool {
        self.grouped = on;
        self.unsettled = false;
        self.changed();
        let moved = on && self.gather();
        if moved {
            self.sorted = None; // like a sort by record
        }
        moved
    }

    /// While grouped, places entries added since the last call with their record. Returns
    /// whether the order changed (the play order follows it).
    pub fn settle(&mut self) -> bool {
        if !std::mem::take(&mut self.unsettled) || !self.grouped {
            return false;
        }
        let moved = self.gather();
        if moved {
            self.sorted = None;
        }
        moved
    }

    /// Moves each album's entries up to follow that album's first entry, keeping their order;
    /// entries of no album keep their place among the rest. A stable reorder, like a sort: the
    /// crate's order (play, save, export) follows it. Returns whether anything moved.
    pub fn gather(&mut self) -> bool {
        let mut slot: HashMap<AlbumKey, usize> = HashMap::new();
        let mut groups: Vec<Vec<usize>> = Vec::with_capacity(self.entries.len());
        for (i, e) in self.entries.iter().enumerate() {
            match e.album_key() {
                Some(k) => match slot.get(&k) {
                    Some(&g) => groups[g].push(i),
                    None => {
                        slot.insert(k, groups.len());
                        groups.push(vec![i]);
                    }
                },
                None => groups.push(vec![i]),
            }
        }
        if groups.len() == self.entries.len() {
            return false; // every album already in one piece, or none at all
        }
        let order: Vec<usize> = groups.into_iter().flatten().collect();
        if order.iter().enumerate().all(|(i, &j)| i == j) {
            return false;
        }
        let mut old: Vec<Option<Entry>> = std::mem::take(&mut self.entries)
            .into_iter()
            .map(Some)
            .collect();
        self.entries = order
            .into_iter()
            .map(|i| old[i].take().expect("each index once"))
            .collect();
        self.changed();
        true
    }

    /// Moves the entries `ids` (a record), in their order, to just before entry `before` (or
    /// to the end). Nothing happens when `before` is one of them.
    pub fn move_block(&mut self, ids: &[EntryId], before: Option<EntryId>) {
        if before.is_some_and(|b| ids.contains(&b)) || ids.is_empty() {
            return;
        }
        let (block, rest): (Vec<Entry>, Vec<Entry>) = std::mem::take(&mut self.entries)
            .into_iter()
            .partition(|e| ids.contains(&e.id));
        self.entries = rest;
        let at = before
            .and_then(|b| self.index_of(b))
            .unwrap_or(self.entries.len());
        self.entries.splice(at..at, block);
        self.sorted = None;
        self.changed();
    }

    /// Adds or removes `ids` from the selection as one: when all are selected they leave it,
    /// otherwise they all join it. The cursor goes to the first.
    pub fn toggle_ids(&mut self, ids: &[EntryId]) {
        if ids.iter().all(|id| self.selected.contains(id)) {
            for id in ids {
                self.selected.remove(id);
            }
        } else {
            self.selected.extend(ids.iter().copied());
        }
        self.cursor = ids.first().copied();
        self.anchor = self.cursor;
    }

    /// Selects every shown entry from the anchor to index `to`, and `ids` with them (Shift
    /// on a record: up to its end). The cursor goes to `cursor`.
    pub fn extend_to(&mut self, to: usize, ids: &[EntryId], cursor: EntryId) {
        let anchor = self.anchor.and_then(|a| self.index_of(a)).unwrap_or(to);
        let (a, b) = (anchor.min(to), anchor.max(to));
        self.selected = self.shown_ids_between(a, b);
        self.selected.extend(ids.iter().copied());
        self.cursor = Some(cursor);
        if self.anchor.is_none() {
            self.anchor = Some(cursor);
        }
    }

    pub fn index_of(&self, id: EntryId) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }

    pub fn get(&self, id: EntryId) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Every entry of `id`'s album, `id` included, in playlist order; empty when it belongs to
    /// none.
    pub fn album_of(&self, id: EntryId) -> Vec<EntryId> {
        let Some(key) = self.get(id).and_then(Entry::album_key) else {
            return Vec::new();
        };
        self.entries
            .iter()
            .filter(|e| e.album_key().as_ref() == Some(&key))
            .map(|e| e.id)
            .collect()
    }

    /// Appends tracks with file-name titles; returns the new entries for metadata lookup.
    pub fn add(&mut self, tracks: impl IntoIterator<Item = TrackRef>) -> Vec<(EntryId, TrackRef)> {
        let from = self.entries.len();
        let mut added = Vec::new();
        for track in tracks {
            let id = self.alloc_id();
            added.push((id, track.clone()));
            self.entries.push(Entry {
                id,
                title: track.stem().to_owned(),
                artist: String::new(),
                album: String::new(),
                duration: None,
                bpm: None,
                status: EntryStatus::Pending,
                source: None,
                origin: None,
                track,
            });
        }
        self.check_sorted(from);
        self.added();
        added
    }

    /// Appends saved entries (Send to crate) with fresh ids; returns the ones whose metadata
    /// still has to be read.
    pub fn add_saved(
        &mut self,
        saved: impl IntoIterator<Item = SavedEntry>,
    ) -> Vec<(EntryId, TrackRef)> {
        let from = self.entries.len();
        let mut pending = Vec::new();
        for s in saved {
            let id = self.alloc_id();
            let entry = Entry::from_saved(id, s);
            if entry.status == EntryStatus::Pending
                || (entry.status.is_playable() && entry.duration.is_none())
            {
                pending.push((id, entry.track.clone()));
            }
            self.entries.push(entry);
        }
        self.check_sorted(from);
        self.added();
        pending
    }

    /// Entries arrived: a grouped list places them at the next settle.
    fn added(&mut self) {
        self.unsettled |= self.grouped;
        self.changed();
    }

    fn alloc_id(&mut self) -> EntryId {
        self.next_id += 1;
        self.next_id
    }

    fn entry_mut(&mut self, id: EntryId) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Metadata read from the file. An entry with an origin keeps its own artist, title and
    /// album (the record is the truth, not the file's tags) and takes only the duration.
    pub fn set_info(&mut self, id: EntryId, info: audio::TrackInfo) {
        if let Some(e) = self.entry_mut(id)
            && e.status.note().is_none()
        {
            if e.origin.is_none() {
                let album = e.album != info.album || e.artist != info.artist;
                e.title = info.title;
                e.artist = info.artist;
                e.album = info.album;
                if album {
                    self.added();
                }
            }
            if let Some(e) = self.entry_mut(id) {
                e.duration = info.duration_secs;
                e.status = EntryStatus::Ready;
            }
        }
    }

    /// Sets the tempo of every entry with this audio (a clip, or a local file); returns whether
    /// any entry changed.
    pub fn set_bpm(&mut self, key: &DuplicateKey, bpm: u16) -> bool {
        let mut changed = false;
        for e in &mut self.entries {
            if e.bpm != Some(bpm) && e.duplicate_key() == *key {
                e.bpm = Some(bpm);
                changed = true;
            }
        }
        if changed {
            self.changed();
        }
        changed
    }

    /// Marks `id` as unplayable. A downloaded preview whose file is gone (deleted from the
    /// cache, or left in an old cache folder) waits to download again instead: returns true.
    pub fn set_failed(&mut self, id: EntryId) -> bool {
        let Some(e) = self.entry_mut(id) else {
            return false;
        };
        let clip = e.origin.as_ref().is_some_and(|o| o.clip.is_some());
        if clip && !std::path::Path::new(&e.track.0).is_file() {
            self.set_waiting(id, WaitKind::Queued);
            return true;
        }
        if e.status.note().is_none() {
            e.status = EntryStatus::Failed;
        }
        false
    }

    // ---- entries waiting for their audio (the producer API) ----------------------------

    /// Appends an entry whose audio isn't local yet, with `status` as its short note.
    pub fn add_waiting(
        &mut self,
        artist: impl Into<String>,
        title: impl Into<String>,
        source: Option<String>,
        origin: Option<Origin>,
        status: impl Into<WaitKind>,
    ) -> EntryId {
        let id = self.alloc_id();
        self.entries.push(Entry {
            id,
            track: TrackRef::new(""),
            source,
            origin,
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            duration: None,
            bpm: None,
            status: EntryStatus::Waiting(status.into()),
        });
        self.check_sorted(self.entries.len() - 1);
        self.added();
        id
    }

    /// Updates why a waiting entry waits (downloading 40%).
    pub fn set_status(&mut self, id: EntryId, kind: impl Into<WaitKind>) {
        if let Some(e) = self.entry_mut(id)
            && let EntryStatus::Waiting(w) = &mut e.status
        {
            *w = kind.into();
        }
    }

    /// The entry's audio arrived at `track`: it becomes Pending, so its duration is read and
    /// it joins the engine queue. Returns false if there is no such entry.
    pub fn set_audio(&mut self, id: EntryId, track: TrackRef) -> bool {
        let Some(e) = self.entry_mut(id) else {
            return false;
        };
        e.track = track;
        e.status = EntryStatus::Pending;
        true
    }

    /// Replaces an entry, at its place, by new waiting entries (a listed record by its clips);
    /// with none it is removed. The current entry, the selection and the anchor move to the
    /// first new entry. Returns the new ids.
    pub fn replace(&mut self, id: EntryId, new: Vec<NewEntry>) -> Vec<EntryId> {
        let Some(at) = self.index_of(id) else {
            return Vec::new();
        };
        let ids: Vec<EntryId> = new.iter().map(|_| self.alloc_id()).collect();
        let entries = new.into_iter().zip(&ids).map(|(n, &nid)| Entry {
            id: nid,
            track: TrackRef::new(""),
            source: n.source,
            origin: n.origin,
            title: n.title,
            artist: n.artist,
            album: String::new(),
            duration: n.duration,
            bpm: None,
            status: EntryStatus::Waiting(n.status),
        });
        self.entries.splice(at..=at, entries);
        self.changed();
        let first = ids.first().copied();
        if self.current == Some(id) {
            self.current = first;
        }
        if self.cursor == Some(id) {
            // Onto the entry now in its place.
            self.cursor = first.or_else(|| self.entry_near(at));
        }
        if self.selected.remove(&id)
            && let Some(f) = first
        {
            self.selected.insert(f);
        }
        if self.anchor == Some(id) {
            self.anchor = first;
        }
        ids
    }

    /// Adds a waiting entry right after `after` (at the end when it is gone). Returns its id.
    pub fn insert_after(&mut self, after: EntryId, n: NewEntry) -> EntryId {
        let at = self.index_of(after).map_or(self.entries.len(), |i| i + 1);
        let id = self.alloc_id();
        self.entries.insert(
            at,
            Entry {
                id,
                track: TrackRef::new(""),
                source: n.source,
                origin: n.origin,
                title: n.title,
                artist: n.artist,
                album: String::new(),
                duration: n.duration,
                bpm: None,
                status: EntryStatus::Waiting(n.status),
            },
        );
        self.changed();
        id
    }

    /// Removes one entry (the current one may be it).
    pub fn remove(&mut self, id: EntryId) -> bool {
        let found = self.index_of(id).is_some();
        self.replace(id, Vec::new());
        found
    }

    /// An entry whose audio went away (its preview was deleted from the cache) waits again.
    pub fn set_waiting(&mut self, id: EntryId, kind: impl Into<WaitKind>) {
        if let Some(e) = self.entry_mut(id) {
            e.track = TrackRef::new("");
            e.status = EntryStatus::Waiting(kind.into());
        }
    }

    /// Every entry, for updates that touch many (marketplace numbers of a release).
    pub fn entries_mut(&mut self) -> impl Iterator<Item = &mut Entry> {
        self.changed();
        self.entries.iter_mut()
    }

    /// The entry will never have audio; it stays listed, dimmed, with `reason`.
    pub fn set_unavailable(&mut self, id: EntryId, reason: impl Into<UnavailableKind>) {
        if let Some(e) = self.entry_mut(id) {
            e.status = EntryStatus::Unavailable(reason.into());
        }
    }

    // ---- current track -----------------------------------------------------------------

    pub fn current(&self) -> Option<EntryId> {
        self.current
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current.and_then(|id| self.index_of(id))
    }

    pub fn set_current(&mut self, id: Option<EntryId>) {
        self.current = id;
    }

    // ---- selection ---------------------------------------------------------------------

    pub fn is_selected(&self, id: EntryId) -> bool {
        self.selected.contains(&id)
    }

    pub fn selected_ids(&self) -> Vec<EntryId> {
        self.entries
            .iter()
            .filter(|e| self.selected.contains(&e.id))
            .map(|e| e.id)
            .collect()
    }

    /// Plain click selects one; Shift extends from the anchor; Cmd/Ctrl toggles. The keyboard
    /// cursor goes to the clicked entry.
    pub fn click(&mut self, index: usize, mods: ClickMods) {
        let Some(id) = self.entries.get(index).map(|e| e.id) else {
            return;
        };
        self.cursor = Some(id);
        if mods.shift {
            let anchor = self.anchor.and_then(|a| self.index_of(a)).unwrap_or(index);
            let (a, b) = (anchor.min(index), anchor.max(index));
            if !mods.command {
                self.selected.clear();
            }
            let ids = self.shown_ids_between(a, b);
            self.selected.extend(ids);
            return;
        }
        if mods.command {
            if !self.selected.remove(&id) {
                self.selected.insert(id);
            }
        } else {
            self.selected.clear();
            self.selected.insert(id);
        }
        self.anchor = Some(id);
    }

    /// Selects every entry the BPM filter shows.
    pub fn select_all(&mut self) {
        self.selected = self.shown_ids_between(0, self.entries.len().saturating_sub(1));
    }

    /// The shown entries from index `a` to `b` (inclusive).
    fn shown_ids_between(&self, a: usize, b: usize) -> BTreeSet<EntryId> {
        let shown = self.shown();
        self.entries
            .get(a..=b.min(self.entries.len().saturating_sub(1)))
            .unwrap_or_default()
            .iter()
            .filter(|e| shown.shows(e))
            .map(|e| e.id)
            .collect()
    }

    pub fn select_none(&mut self) {
        self.selected.clear();
    }

    /// Selects exactly `ids` (an album), with the cursor and the anchor on `cursor`.
    pub fn select_only(&mut self, ids: &[EntryId], cursor: EntryId) {
        self.selected = ids.iter().copied().collect();
        self.cursor = Some(cursor).filter(|&c| self.index_of(c).is_some());
        self.anchor = self.cursor;
    }

    /// Inverts the selection among the shown entries (hidden ones end up unselected).
    pub fn invert_selection(&mut self) {
        let shown = self.shown();
        self.selected = self
            .entries
            .iter()
            .filter(|e| shown.shows(e) && !self.selected.contains(&e.id))
            .map(|e| e.id)
            .collect();
    }

    // ---- keyboard cursor ---------------------------------------------------------------

    pub fn cursor(&self) -> Option<EntryId> {
        self.cursor
    }

    pub fn cursor_index(&self) -> Option<usize> {
        self.cursor.and_then(|c| self.index_of(c))
    }

    /// Puts the cursor on an entry without touching the selection.
    pub fn set_cursor(&mut self, id: Option<EntryId>) {
        self.cursor = id.filter(|&i| self.index_of(i).is_some());
    }

    /// Moves the cursor (`page` entries for a page) and selects what it passes: only the new
    /// entry, or with `extend` everything from the anchor to it. Without a cursor, ↑ or ↓ lands
    /// on `start` (the playing entry), or on the first entry; the other moves go from there.
    pub fn move_cursor(
        &mut self,
        mv: CursorMove,
        extend: bool,
        page: usize,
        start: Option<EntryId>,
    ) {
        // The cursor moves over the rows the BPM filter shows.
        let rows = self.shown_rows();
        if rows.is_empty() {
            return;
        }
        let last = rows.len() - 1;
        let page = page.max(1);
        // A row: the entry's own, or the nearest shown one after it (it may be hidden).
        let row_of = |id: EntryId| {
            self.index_of(id)
                .map(|i| rows.partition_point(|&r| r < i).min(last))
        };
        let to = match (self.cursor.and_then(row_of), mv) {
            (None, CursorMove::Up | CursorMove::Down) => start.and_then(row_of).unwrap_or(0),
            (i, _) => {
                let i = i.or_else(|| start.and_then(row_of)).unwrap_or(0);
                match mv {
                    CursorMove::Up => i.saturating_sub(1),
                    CursorMove::Down => (i + 1).min(last),
                    CursorMove::PageUp => i.saturating_sub(page),
                    CursorMove::PageDown => (i + page).min(last),
                    CursorMove::Home => 0,
                    CursorMove::End => last,
                }
            }
        };
        let to = rows[to];
        let id = self.entries[to].id;
        if extend {
            let from = self.anchor.or(self.cursor).unwrap_or(id);
            self.anchor = Some(from);
            let a = self.index_of(from).unwrap_or(to);
            let (a, b) = (a.min(to), a.max(to));
            self.selected = self.shown_ids_between(a, b);
        } else {
            self.selected = [id].into();
            self.anchor = Some(id);
        }
        self.cursor = Some(id);
    }

    /// The entry at `index`, or the last one when the list is shorter.
    fn entry_near(&self, index: usize) -> Option<EntryId> {
        self.entries
            .get(index)
            .or_else(|| self.entries.last())
            .map(|e| e.id)
    }

    // ---- editing -----------------------------------------------------------------------

    /// Removes the selected entries. The current track may be among them.
    pub fn remove_selected(&mut self) -> usize {
        let sel = std::mem::take(&mut self.selected);
        self.remove_set(&sel)
    }

    /// Removes these entries (an album); the rest of the selection stays. The current track
    /// may be among them.
    pub fn remove_ids(&mut self, ids: &[EntryId]) -> usize {
        let set: BTreeSet<EntryId> = ids.iter().copied().collect();
        self.selected.retain(|id| !set.contains(id));
        self.remove_set(&set)
    }

    fn remove_set(&mut self, sel: &BTreeSet<EntryId>) -> usize {
        let before = self.entries.len();
        if let Some(at) = self
            .cursor
            .filter(|c| sel.contains(c))
            .and_then(|c| self.index_of(c))
        {
            // Onto the first entry left after it, else the last one left before it.
            let kept = |e: &&Entry| !sel.contains(&e.id);
            self.cursor = self.entries[at..]
                .iter()
                .find(kept)
                .or_else(|| self.entries[..at].iter().rev().find(kept))
                .map(|e| e.id);
        }
        self.entries.retain(|e| !sel.contains(&e.id));
        self.changed();
        if self.current.is_some_and(|c| sel.contains(&c)) {
            self.current = None;
        }
        before - self.entries.len()
    }

    pub fn clear(&mut self) {
        self.changed();
        self.entries.clear();
        self.selected.clear();
        self.anchor = None;
        self.current = None;
        self.cursor = None;
    }

    /// Moves entry `from` so it ends up at index `to` (drag-to-reorder).
    pub fn move_entry(&mut self, from: usize, to: usize) {
        if from >= self.entries.len() || from == to {
            return;
        }
        let e = self.entries.remove(from);
        let to = to.min(self.entries.len());
        self.entries.insert(to, e);
        self.sorted = None;
        self.changed();
    }

    // ---- sorting -----------------------------------------------------------------------

    /// Reorders the entries by `field` (stable; entries without a value last). The new order
    /// is the crate's order: play order, saving and export follow it.
    pub fn sort_by(&mut self, field: Field, dir: Dir) {
        self.entries
            .sort_by(|a, b| crate::columns::compare(a, b, field, dir));
        // Grouped, each record follows its best-placed track; the mark stays.
        if self.grouped {
            self.gather();
        }
        self.sorted = Some((field, dir));
        self.changed();
    }

    /// What the entries were last sorted by, while that order holds.
    pub fn sorted(&self) -> Option<(Field, Dir)> {
        self.sorted
    }

    /// Entries appended from `from` on keep the sort mark only if they land in order.
    fn check_sorted(&mut self, from: usize) {
        let Some((field, dir)) = self.sorted else {
            return;
        };
        let start = from.max(1);
        if (start..self.entries.len()).any(|i| {
            crate::columns::compare(&self.entries[i - 1], &self.entries[i], field, dir)
                == std::cmp::Ordering::Greater
        }) {
            self.sorted = None;
        }
    }

    // ---- totals ------------------------------------------------------------------------

    /// Sum of known durations and whether any duration is still unknown.
    pub fn total_duration(&self) -> (f64, bool) {
        sum(self.entries.iter())
    }

    pub fn selected_duration(&self) -> (f64, bool) {
        sum(self
            .entries
            .iter()
            .filter(|e| self.selected.contains(&e.id)))
    }

    // ---- persistence -------------------------------------------------------------------

    pub fn to_saved(&self) -> SavedPlaylist {
        SavedPlaylist {
            entries: self.entries.iter().map(Entry::to_saved).collect(),
            current: self.current_index(),
            bpm_range: self.bpm_filter(),
            styles: self.saved_picks(Facet::Style),
            artists: self.saved_picks(Facet::Artist),
            labels: self.saved_picks(Facet::Label),
            formats: self.saved_picks(Facet::Format),
            copies: self.copies.clone(),
            cart_only: self.cart_only,
        }
    }

    /// The saved form of some entries, in playlist order (for Send to crate).
    pub fn saved_entries(&self, ids: &[EntryId]) -> Vec<SavedEntry> {
        self.entries
            .iter()
            .filter(|e| ids.contains(&e.id))
            .map(Entry::to_saved)
            .collect()
    }

    /// Restores a saved playlist; entries without cached metadata are returned for lookup.
    pub fn from_saved(saved: SavedPlaylist) -> (Self, Vec<(EntryId, TrackRef)>) {
        let mut pl = Playlist::default();
        let pending = pl.add_saved(saved.entries);
        pl.current = saved.current.and_then(|i| pl.entries.get(i)).map(|e| e.id);
        pl.bpm_range = saved.bpm_range;
        pl.picked = [saved.styles, saved.artists, saved.labels, saved.formats];
        pl.copies = saved.copies;
        pl.cart_only = saved.cart_only;
        (pl, pending)
    }

    // ---- copies for sale (seller crates) -------------------------------------------------

    pub fn copies(&self) -> &[SaleCopy] {
        &self.copies
    }

    /// A record's copies: unsold cheapest first, then the sold ones.
    pub fn copies_of(&self, release: u64) -> Vec<&SaleCopy> {
        let mut v: Vec<&SaleCopy> = self
            .copies
            .iter()
            .filter(|c| c.release == release)
            .collect();
        v.sort_by_key(|c| (c.sold, c.cents, c.listing));
        v
    }

    pub fn copy(&self, listing: u64) -> Option<&SaleCopy> {
        self.copies.iter().find(|c| c.listing == listing)
    }

    /// Adds copies not held yet (by listing); returns how many were new.
    pub fn add_copies(&mut self, copies: impl IntoIterator<Item = SaleCopy>) -> usize {
        let mut n = 0;
        for c in copies {
            if self.copy(c.listing).is_none() {
                self.copies.push(c);
                n += 1;
            }
        }
        if n > 0 {
            self.changed();
        }
        n
    }

    /// Replaces the copies (after a refresh).
    pub fn set_copies(&mut self, copies: Vec<SaleCopy>) {
        if self.copies != copies {
            self.copies = copies;
            self.changed();
        }
    }

    /// Marks a copy no longer for sale; returns whether it changed.
    pub fn mark_sold(&mut self, listing: u64) -> bool {
        match self
            .copies
            .iter_mut()
            .find(|c| c.listing == listing && !c.sold)
        {
            Some(c) => {
                c.sold = true;
                self.changed();
                true
            }
            None => false,
        }
    }

    // ---- BPM filter ----------------------------------------------------------------------

    /// The lowest and highest known tempo, when there are at least two different ones (a
    /// range means nothing otherwise).
    pub fn tempo_span(&self) -> Option<(u16, u16)> {
        let mut known = self.entries.iter().filter_map(|e| e.bpm);
        let first = known.next()?;
        let (lo, hi) = known.fold((first, first), |(lo, hi), b| (lo.min(b), hi.max(b)));
        (lo < hi).then_some((lo, hi))
    }

    /// The range that applies now: the one set, within the crate's tempos; `None` (no
    /// filter) when it covers them all, or misses them all (the tempos moved away).
    pub fn bpm_filter(&self) -> Option<(u16, u16)> {
        let (lo, hi) = self.bpm_range?;
        let (span_lo, span_hi) = self.tempo_span()?;
        let (lo, hi) = (lo.max(span_lo), hi.min(span_hi));
        (lo <= hi && (lo, hi) != (span_lo, span_hi)).then_some((lo, hi))
    }

    /// Sets the range (`None`, or the whole span, turns the filter off). Returns whether what
    /// applies changed.
    pub fn set_bpm_filter(&mut self, range: Option<(u16, u16)>) -> bool {
        let before = self.bpm_filter();
        self.bpm_range = range.map(|(a, b)| (a.min(b), a.max(b)));
        self.bpm_range = self.bpm_filter();
        self.changed();
        before != self.bpm_range
    }

    /// Whether the filters show `e` (see [`Shown`]). For many entries, take [`Self::shown`]
    /// once instead.
    pub fn shows(&self, e: &Entry) -> bool {
        self.shown().shows(e)
    }

    /// What the BPM, style, artist and label filters show, worked out once for a pass over
    /// the entries.
    pub fn shown(&self) -> Shown<'_> {
        Shown {
            bpm: self.bpm_filter(),
            picks: Facet::ALL.map(|f| self.filter(f)),
            cart: self.cart_only.then_some(&self.in_cart),
            search: &self.search,
        }
    }

    /// The search's folded words (empty: no search).
    pub fn search(&self) -> &[Vec<char>] {
        &self.search
    }

    /// Sets the search from the typed text. Returns whether what it matches changed.
    pub fn set_search(&mut self, text: &str) -> bool {
        let words = fold_words(text);
        if words == self.search {
            return false;
        }
        // A new search covers the whole crate: filters set before it would hide matches.
        if self.search.is_empty() {
            if self.bpm_range.is_some() {
                self.set_bpm_filter(None);
            }
            // One facet at a time: `None` would turn the CART switch off too.
            for f in Facet::ALL {
                if !self.picked[f.index()].is_empty() {
                    self.clear_picks(Some(f));
                }
            }
        }
        self.search = words;
        self.changed();
        true
    }

    /// Whether a filter (BPM, style, artist or label) hides anything.
    pub fn is_filtered(&self) -> bool {
        self.shown().is_filtered()
    }

    /// Turns every filter and the search off. Returns whether that shows more.
    pub fn clear_filters(&mut self) -> bool {
        let bpm = self.set_bpm_filter(None);
        let search = self.set_search("");
        self.clear_picks(None) || bpm || search
    }

    /// The crate indices of the entries the filters show, in order: row `r` of the list is
    /// entry `shown_rows()[r]`, still numbered by its crate position.
    pub fn shown_rows(&self) -> Vec<usize> {
        let shown = self.shown();
        if !shown.is_filtered() {
            return (0..self.entries.len()).collect();
        }
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| shown.shows(e))
            .map(|(i, _)| i)
            .collect()
    }

    // ---- style, artist and label filters -------------------------------------------------

    /// The crate's values for `facet`, each with the number of records (albums, and entries of
    /// no album) that have it: the most first, then by name.
    pub fn counts(&self, facet: Facet) -> Vec<(String, usize)> {
        let mut seen: HashMap<&str, HashSet<AlbumKey>> = HashMap::new();
        let mut loose: HashMap<&str, usize> = HashMap::new();
        for e in &self.entries {
            let key = e.album_key();
            for v in e.values(facet) {
                match &key {
                    Some(k) => {
                        seen.entry(v).or_default().insert(k.clone());
                    }
                    None => *loose.entry(v).or_default() += 1,
                }
            }
        }
        let mut out: Vec<(String, usize)> = seen
            .keys()
            .chain(loose.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|&v| {
                let n = seen.get(v).map_or(0, HashSet::len) + loose.get(v).copied().unwrap_or(0);
                (v.to_owned(), n)
            })
            .collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }

    /// The values of `facet` that filter now: the ones picked, when some entry still has one
    /// of them; `None` (no filter) otherwise.
    pub fn filter(&self, facet: Facet) -> Option<&BTreeSet<String>> {
        let on = &self.picked[facet.index()];
        let any = !on.is_empty()
            && self
                .entries
                .iter()
                .any(|e| e.values(facet).any(|v| on.contains(v)));
        any.then_some(on)
    }

    /// Whether a style, artist, label or format filter hides anything (not the BPM range).
    pub fn picks_filter(&self) -> bool {
        Facet::ALL.into_iter().any(|f| self.filter(f).is_some()) || self.cart_only
    }

    /// The CART switch.
    pub fn cart_only(&self) -> bool {
        self.cart_only
    }

    pub fn set_cart_only(&mut self, on: bool) {
        if self.cart_only != on {
            self.cart_only = on;
            self.changed();
        }
    }

    /// The releases with a copy in the cart; the view changes only when the switch is on.
    pub fn set_in_cart(&mut self, releases: &BTreeSet<u64>) {
        if self.in_cart != *releases {
            self.in_cart = releases.clone();
            if self.cart_only {
                self.changed();
            }
        }
    }

    /// Whether `value` is picked in `facet`.
    pub fn picked(&self, facet: Facet, value: &str) -> bool {
        self.picked[facet.index()].contains(value)
    }

    /// Picks or unpicks one value of `facet`.
    pub fn set_pick(&mut self, facet: Facet, value: &str, on: bool) {
        let set = &mut self.picked[facet.index()];
        let changed = if on {
            set.insert(value.to_owned())
        } else {
            set.remove(value)
        };
        if changed {
            self.changed();
        }
    }

    /// Unpicks every value of `facet`, or of all of them with `None`. Returns whether one of
    /// them filtered.
    pub fn clear_picks(&mut self, facet: Option<Facet>) -> bool {
        let facets: Vec<Facet> = facet.map_or(Facet::ALL.to_vec(), |f| vec![f]);
        let mut was = facets.iter().any(|&f| self.filter(f).is_some());
        let mut changed = false;
        // Show all records turns the CART switch off too.
        if facet.is_none() && self.cart_only {
            self.cart_only = false;
            (was, changed) = (true, true);
        }
        for f in facets {
            let set = &mut self.picked[f.index()];
            changed |= !set.is_empty();
            set.clear();
        }
        if changed {
            self.changed();
        }
        was
    }

    /// What is saved of a facet's filter: the picks while they filter.
    fn saved_picks(&self, facet: Facet) -> BTreeSet<String> {
        self.filter(facet).cloned().unwrap_or_default()
    }

    /// Entries without a known tempo (hidden while a range is set).
    pub fn without_bpm(&self) -> usize {
        self.entries.iter().filter(|e| e.bpm.is_none()).count()
    }

    // ---- play order ----------------------------------------------------------------------

    /// Every entry that can eventually play (playable or waiting) and that the BPM filter
    /// shows, in playlist order or shuffled starting with `first`. The current entry stays in
    /// it even when the filter hides it, so it plays on and the next shown entry follows it.
    /// A waiting entry keeps its place when its audio arrives, because the set of entries
    /// shuffled doesn't change. Audio producers read what comes next from this order.
    pub fn play_order(&self, shuffle: bool, first: Option<EntryId>, seed: u64) -> Vec<EntryId> {
        let shown = self.shown();
        let ids: Vec<EntryId> = self
            .entries
            .iter()
            .filter(|e| e.status.in_play_order())
            .filter(|e| Some(e.id) == self.current || shown.shows(e))
            .map(|e| e.id)
            .collect();
        let first = first.and_then(|f| ids.iter().position(|&id| id == f));
        play_order(ids.len(), shuffle, first, seed)
            .into_iter()
            .map(|i| ids[i])
            .collect()
    }

    /// The engine queue: the play order without the entries still waiting for their audio.
    pub fn queue(
        &self,
        shuffle: bool,
        first: Option<EntryId>,
        seed: u64,
    ) -> Vec<(EntryId, TrackRef)> {
        self.play_order(shuffle, first, seed)
            .into_iter()
            .filter_map(|id| self.get(id))
            .filter(|e| e.status.is_playable())
            .map(|e| (e.id, e.track.clone()))
            .collect()
    }
}

/// File tags as the metadata worker reports them, for tests.
#[cfg(test)]
pub(crate) fn tags(title: String, artist: String, duration: Option<f64>) -> audio::TrackInfo {
    audio::TrackInfo {
        title,
        artist,
        duration_secs: duration,
        ..Default::default()
    }
}

impl Entry {
    fn from_saved(id: EntryId, s: SavedEntry) -> Self {
        let track = TrackRef::new(s.path);
        let known = !s.title.is_empty();
        let status = match s.status {
            SavedStatus::Waiting(t) => EntryStatus::Waiting(t.into()),
            SavedStatus::Unavailable(t) => EntryStatus::Unavailable(t.into()),
            SavedStatus::Local if known => EntryStatus::Ready,
            SavedStatus::Local => EntryStatus::Pending,
        };
        Entry {
            id,
            title: if known {
                s.title
            } else {
                track.stem().to_owned()
            },
            artist: s.artist,
            album: s.album,
            duration: s.duration,
            bpm: s.bpm,
            status,
            source: s.source,
            origin: s.origin,
            track,
        }
    }
}

fn sum<'a>(entries: impl Iterator<Item = &'a Entry>) -> (f64, bool) {
    entries.fold((0.0, false), |(total, unknown), e| match e.duration {
        Some(d) => (total + d, unknown),
        None => (total, true),
    })
}

/// Queue order for the engine: playlist order, or a shuffle that starts with `first`.
pub fn play_order(len: usize, shuffle: bool, first: Option<usize>, seed: u64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    if !shuffle || len < 2 {
        return order;
    }
    // Fisher–Yates with a small xorshift generator (no dependency needed for a shuffle).
    let mut state = seed | 1;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for i in (1..len).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    if let Some(f) = first.filter(|&f| f < len) {
        let at = order.iter().position(|&i| i == f).expect("permutation");
        order.swap(0, at);
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_are_saved_with_the_crate() {
        let mut p = Playlist::default();
        let c = |listing, release, cents, sold| SaleCopy {
            listing,
            release,
            cents,
            sold,
            ..SaleCopy::default()
        };
        assert_eq!(
            p.add_copies([
                c(1, 10, 1800, false),
                c(2, 10, 900, false),
                c(3, 11, 500, false)
            ]),
            3
        );
        assert_eq!(p.add_copies([c(1, 10, 1800, false)]), 0, "by listing, once");
        assert!(p.mark_sold(2));
        assert!(!p.mark_sold(2));
        let (back, _) = Playlist::from_saved(p.to_saved());
        assert_eq!(back.copies(), p.copies());
        let order: Vec<u64> = back.copies_of(10).iter().map(|c| c.listing).collect();
        assert_eq!(order, [1, 2], "unsold cheapest first, sold last");
    }

    fn pl(n: usize) -> Playlist {
        let mut p = Playlist::default();
        p.add((0..n).map(|i| TrackRef::new(format!("/m/{i}.mp3"))));
        p
    }

    fn sel(p: &Playlist) -> Vec<usize> {
        p.selected_ids()
            .iter()
            .map(|id| p.index_of(*id).unwrap())
            .collect()
    }

    #[test]
    fn add_uses_file_names_until_metadata_arrives() {
        let mut p = Playlist::default();
        let added = p.add([TrackRef::new("/music/M83 - Midnight City.mp3")]);
        assert_eq!(p.entries()[0].title, "M83 - Midnight City");
        assert_eq!(p.entries()[0].status, EntryStatus::Pending);
        p.set_info(
            added[0].0,
            tags("Midnight City".into(), "M83".into(), Some(243.0)),
        );
        assert_eq!(p.entries()[0].display_name(), "M83: Midnight City");
        assert_eq!(p.entries()[0].status, EntryStatus::Ready);
    }

    #[test]
    fn selection_click_shift_and_command() {
        let mut p = pl(10);
        p.click(2, ClickMods::default());
        assert_eq!(sel(&p), [2]);
        p.click(
            5,
            ClickMods {
                shift: true,
                command: false,
            },
        );
        assert_eq!(sel(&p), [2, 3, 4, 5]);
        p.click(
            8,
            ClickMods {
                shift: false,
                command: true,
            },
        );
        assert_eq!(sel(&p), [2, 3, 4, 5, 8]);
        p.click(
            3,
            ClickMods {
                shift: false,
                command: true,
            },
        );
        assert_eq!(sel(&p), [2, 4, 5, 8]);
        p.click(0, ClickMods::default());
        assert_eq!(sel(&p), [0]);
        p.invert_selection();
        assert_eq!(sel(&p).len(), 9);
        p.select_none();
        assert!(sel(&p).is_empty());
    }

    #[test]
    fn reorder_moves_entry_and_keeps_identity() {
        let mut p = pl(10);
        let seventh = p.entries()[6].id; // "entry 7"
        p.set_current(Some(seventh));
        p.move_entry(6, 1); // dragged above entry 2
        assert_eq!(p.entries()[1].id, seventh);
        assert_eq!(p.current_index(), Some(1), "current follows the entry");
        let order: Vec<_> = p
            .entries()
            .iter()
            .map(|e| e.track.stem().to_owned())
            .collect();
        assert_eq!(order, ["0", "6", "1", "2", "3", "4", "5", "7", "8", "9"]);
    }

    #[test]
    fn remove_selected_and_clear() {
        let mut p = pl(5);
        p.set_current(Some(p.entries()[1].id));
        p.click(1, ClickMods::default());
        p.click(
            3,
            ClickMods {
                shift: true,
                command: false,
            },
        );
        assert_eq!(p.remove_selected(), 3);
        assert_eq!(p.len(), 2);
        assert_eq!(p.current(), None, "removed current track is forgotten");
        p.clear();
        assert!(p.is_empty());
    }

    #[test]
    fn durations_sum_known_and_flag_unknown() {
        let mut p = pl(3);
        let ids: Vec<_> = p.entries().iter().map(|e| e.id).collect();
        p.set_info(ids[0], tags("a".into(), "".into(), Some(60.0)));
        p.set_info(ids[1], tags("b".into(), "".into(), Some(30.5)));
        assert_eq!(p.total_duration(), (90.5, true));
        p.set_info(ids[2], tags("c".into(), "".into(), Some(9.5)));
        assert_eq!(p.total_duration(), (100.0, false));
        p.click(1, ClickMods::default());
        assert_eq!(p.selected_duration(), (30.5, false));
    }

    #[test]
    fn a_tempo_reaches_every_entry_with_that_audio_and_is_saved() {
        let mut p = pl(2);
        let clip = |c: &str| Origin {
            catno: "LT-012".into(),
            clip: Some(c.into()),
            ..Default::default()
        };
        let a = p.add_waiting(
            "Nightcraft",
            "Glasshouse",
            None,
            Some(clip("aaaaaaaaaaa")),
            "listed",
        );
        let b = p.add_waiting(
            "Nightcraft",
            "Glasshouse",
            None,
            Some(clip("aaaaaaaaaaa")),
            "listed",
        );
        let other = p.add_waiting(
            "Nightcraft",
            "Undertow",
            None,
            Some(clip("bbbbbbbbbbb")),
            "listed",
        );
        assert!(p.set_bpm(&DuplicateKey::Clip("aaaaaaaaaaa".into()), 124));
        assert!(
            !p.set_bpm(&DuplicateKey::Clip("aaaaaaaaaaa".into()), 124),
            "no change"
        );
        assert_eq!(p.get(a).unwrap().bpm, Some(124));
        assert_eq!(p.get(b).unwrap().bpm, Some(124));
        assert_eq!(p.get(other).unwrap().bpm, None);
        let file = p.entries()[0].track.clone();
        assert!(p.set_bpm(&DuplicateKey::File(file.0.clone()), 128));
        assert_eq!(p.entries()[0].bpm, Some(128));
        assert_eq!(p.entries()[1].bpm, None);

        let text = ron::to_string(&p.to_saved()).unwrap();
        let (restored, _) = Playlist::from_saved(ron::from_str(&text).unwrap());
        assert_eq!(
            restored.get(a).map(Entry::display_name).as_deref(),
            Some("(LT-012) Nightcraft: Glasshouse (124 BPM)")
        );
        assert_eq!(restored.entries()[0].bpm, Some(128));
        assert!(
            !ron::to_string(&restored.entries()[1].to_saved())
                .unwrap()
                .contains("bpm"),
            "an unknown tempo isn't written"
        );
    }

    fn cursor_at(p: &Playlist) -> Option<usize> {
        p.cursor_index()
    }

    #[test]
    fn the_cursor_walks_pages_and_ends_and_selects_as_it_goes() {
        let mut p = pl(40);
        let playing = p.entries()[22].id;
        p.move_cursor(CursorMove::Down, false, 10, Some(playing));
        assert_eq!(
            cursor_at(&p),
            Some(22),
            "the first press lands on the playing entry"
        );
        assert_eq!(sel(&p), [22]);
        p.move_cursor(CursorMove::Down, false, 10, None);
        p.move_cursor(CursorMove::Down, false, 10, None);
        assert_eq!((cursor_at(&p), sel(&p)), (Some(24), vec![24]));
        p.move_cursor(CursorMove::PageUp, false, 10, None);
        assert_eq!(cursor_at(&p), Some(14));
        p.move_cursor(CursorMove::End, false, 10, None);
        assert_eq!(cursor_at(&p), Some(39));
        p.move_cursor(CursorMove::Down, false, 10, None);
        assert_eq!(cursor_at(&p), Some(39), "stops at the end");
        p.move_cursor(CursorMove::PageDown, false, 10, None);
        assert_eq!(cursor_at(&p), Some(39));
        p.move_cursor(CursorMove::Home, false, 10, None);
        p.move_cursor(CursorMove::Up, false, 10, None);
        assert_eq!(cursor_at(&p), Some(0), "stops at the start");

        let mut fresh = pl(3);
        fresh.move_cursor(CursorMove::Up, false, 10, None);
        assert_eq!(
            cursor_at(&fresh),
            Some(0),
            "nothing playing: the first entry"
        );
    }

    #[test]
    fn shift_extends_the_selection_from_the_anchor() {
        let mut p = pl(10);
        p.click(4, ClickMods::default());
        assert_eq!(cursor_at(&p), Some(4), "a click moves the cursor");
        for _ in 0..3 {
            p.move_cursor(CursorMove::Down, true, 10, None);
        }
        assert_eq!((cursor_at(&p), sel(&p)), (Some(7), vec![4, 5, 6, 7]));
        p.move_cursor(CursorMove::Up, true, 10, None);
        assert_eq!(sel(&p), [4, 5, 6]);
        p.move_cursor(CursorMove::Down, false, 10, None);
        assert_eq!(sel(&p), [7], "a plain move selects one again");
    }

    #[test]
    fn the_cursor_stays_on_its_entry_while_entries_change_around_it() {
        let mut p = pl(3);
        let listed = p.add_waiting("Nightcraft", "Glasshouse EP", None, None, "listed");
        let after = p.add_waiting("Nightcraft", "Undertow", None, None, "listed");
        p.move_cursor(CursorMove::End, false, 10, None);
        assert_eq!(p.cursor(), Some(after));
        // A listed record above it becomes three clips.
        let clip = |t: &str| NewEntry {
            artist: "Nightcraft".into(),
            title: t.into(),
            source: None,
            origin: None,
            duration: None,
            status: "queued".into(),
        };
        p.replace(listed, vec![clip("A1"), clip("A2"), clip("B1")]);
        assert_eq!((p.cursor(), cursor_at(&p)), (Some(after), Some(6)));
        // Reordering carries it along.
        p.move_entry(6, 0);
        assert_eq!((p.cursor(), cursor_at(&p)), (Some(after), Some(0)));
        // A replaced cursor entry hands over to its first replacement; a removed one to the
        // entry now in its place.
        let a1 = p.entries()[4].id;
        p.set_cursor(Some(a1));
        let a2 = p.entries()[5].id;
        p.remove(a1);
        assert_eq!(p.cursor(), Some(a2));
        p.click(4, ClickMods::default());
        let next = p.entries()[5].id;
        p.remove_selected();
        assert_eq!(p.cursor(), Some(next));
        p.clear();
        assert_eq!(p.cursor(), None);
    }

    #[test]
    fn statuses_read_back_from_their_wording() {
        for w in [
            WaitKind::Listed,
            WaitKind::Queued,
            WaitKind::Downloading(40),
            WaitKind::NeedsYtDlp,
            WaitKind::Other("paused by Discogs".into()),
        ] {
            assert_eq!(WaitKind::from(w.to_string()), w);
        }
        for u in [
            UnavailableKind::NoClip,
            UnavailableKind::ClipFailed,
            UnavailableKind::Other("not found".into()),
        ] {
            assert_eq!(UnavailableKind::from(u.to_string()), u);
        }
        assert_eq!(
            WaitKind::from("downloading lots%"),
            WaitKind::Other("downloading lots%".into())
        );
        assert_eq!(
            EntryStatus::Waiting(WaitKind::Downloading(7))
                .note()
                .as_deref(),
            Some("downloading 7%")
        );
    }

    #[test]
    fn a_crate_saved_with_status_words_loads_their_kinds() {
        let text = r#"(entries: [
            (path: "", title: "A", status: Waiting("listed")),
            (path: "", title: "B", status: Waiting("downloading 40%")),
            (path: "", title: "C", status: Waiting("needs yt-dlp")),
            (path: "", title: "D", status: Unavailable("no clip")),
            (path: "", title: "E", status: Unavailable("failed: 502")),
            (path: "", title: "F", status: Unavailable("already in crate")),
        ], current: None)"#;
        let (p, _) = Playlist::from_saved(ron::from_str(text).unwrap());
        let got: Vec<_> = p.entries().iter().map(|e| e.status.clone()).collect();
        assert_eq!(
            got,
            [
                EntryStatus::Waiting(WaitKind::Listed),
                EntryStatus::Waiting(WaitKind::Downloading(40)),
                EntryStatus::Waiting(WaitKind::NeedsYtDlp),
                EntryStatus::Unavailable(UnavailableKind::NoClip),
                EntryStatus::Unavailable(UnavailableKind::Other("failed: 502".into())),
                EntryStatus::Unavailable(UnavailableKind::AlreadyInCrate),
            ]
        );
        // And they are written back in the same words.
        assert_eq!(
            ron::to_string(&p.to_saved()).unwrap(),
            ron::to_string(&ron::from_str::<SavedPlaylist>(text).unwrap()).unwrap()
        );
    }

    #[test]
    fn a_held_back_album_clip_is_saved_and_old_crates_load_without_it() {
        let mut p = Playlist::default();
        let held = Origin {
            search_key: "track/nightcraft/glasshouse".into(),
            album_clip: "fullalbum01".into(),
            album_clip_title: "Glasshouse EP (Full Album)".into(),
            release: Some(1),
            position: "A1".into(),
            ..Origin::default()
        };
        let id = p.add_waiting(
            "Nightcraft",
            "Glasshouse",
            None,
            Some(held.clone()),
            "to search",
        );
        let text = ron::to_string(&p.to_saved()).unwrap();
        let (restored, _) = Playlist::from_saved(ron::from_str(&text).unwrap());
        assert_eq!(restored.get(id).unwrap().origin.as_ref(), Some(&held));

        let old = r#"(entries: [(path: "", title: "A", status: Waiting("to search"),
            origin: Some((release: Some(1), position: "A1", search_key: "release/1/A1")))],
            current: None)"#;
        let (p, _) = Playlist::from_saved(ron::from_str(old).unwrap());
        let o = p.entries()[0].origin.as_ref().unwrap();
        assert_eq!(
            (o.search_key.as_str(), o.album_clip.as_str()),
            ("release/1/A1", "")
        );
        assert!(
            !ron::to_string(&p.to_saved())
                .unwrap()
                .contains("album_clip")
        );
    }

    #[test]
    fn an_entry_goes_in_right_after_another() {
        let mut p = Playlist::default();
        let a = p.add_waiting("N", "A", None, None, "queued");
        let _b = p.add_waiting("N", "B", None, None, "queued");
        let new = |t: &str| NewEntry {
            artist: "N".into(),
            title: t.into(),
            source: None,
            origin: None,
            duration: None,
            status: WaitKind::Queued,
        };
        let x = p.insert_after(a, new("X"));
        assert_eq!(sorted_titles(&p), ["A", "X", "B"]);
        assert_eq!(p.index_of(x), Some(1));
        p.remove(a);
        p.insert_after(a, new("Y"));
        assert_eq!(
            sorted_titles(&p),
            ["X", "B", "Y"],
            "after a gone entry: at the end"
        );
    }

    fn sorted_titles(p: &Playlist) -> Vec<String> {
        p.entries().iter().map(|e| e.title.clone()).collect()
    }

    /// Entries titled by name, with the given BPM, catalog number, side, year and for-sale
    /// snapshot (count, lowest cents).
    fn dig_entry(
        p: &mut Playlist,
        title: &str,
        bpm: Option<u16>,
        catno: &str,
        side: &str,
        year: Option<u16>,
        sale: Option<(u32, Option<u64>)>,
    ) -> EntryId {
        let id = p.add_waiting(
            "",
            title,
            None,
            Some(Origin {
                catno: catno.into(),
                position: side.into(),
                year,
                for_sale: sale.map(|(count, lowest_cents)| ForSale {
                    count,
                    lowest_cents,
                    currency: "EUR".into(),
                    fetched_at: 0,
                }),
                ..Default::default()
            }),
            "listed",
        );
        p.entries_mut().find(|e| e.id == id).unwrap().bpm = bpm;
        id
    }

    #[test]
    fn sorting_by_bpm_keeps_unknown_tempos_last_both_ways() {
        let mut p = Playlist::default();
        for (t, b) in [
            ("a", Some(128)),
            ("b", Some(122)),
            ("c", None),
            ("d", Some(140)),
        ] {
            dig_entry(&mut p, t, b, "", "", None, None);
        }
        p.sort_by(Field::Bpm, Dir::Asc);
        assert_eq!(sorted_titles(&p), ["b", "a", "d", "c"]);
        assert_eq!(p.sorted(), Some((Field::Bpm, Dir::Asc)));
        p.sort_by(Field::Bpm, Dir::Desc);
        assert_eq!(sorted_titles(&p), ["d", "a", "b", "c"]);
    }

    #[test]
    fn sorting_by_text_fields_is_natural_and_stable() {
        let mut p = Playlist::default();
        for (t, cat, side) in [
            ("x", "LT-10", "B1"),
            ("y", "LT-2", "A10"),
            ("z", "LT-1", "A2"),
            ("w", "LT-2", ""),
        ] {
            dig_entry(&mut p, t, None, cat, side, None, None);
        }
        p.sort_by(Field::CatNo, Dir::Asc);
        assert_eq!(
            sorted_titles(&p),
            ["z", "y", "w", "x"],
            "ties keep their order"
        );
        p.sort_by(Field::Side, Dir::Asc);
        assert_eq!(sorted_titles(&p), ["z", "y", "x", "w"]);
        p.sort_by(Field::Title, Dir::Desc);
        assert_eq!(sorted_titles(&p), ["z", "y", "x", "w"]);
    }

    #[test]
    fn sorting_by_year_time_and_artist() {
        let mut p = Playlist::default();
        let a = dig_entry(&mut p, "a", None, "", "", Some(1994), None);
        let b = dig_entry(&mut p, "b", None, "", "", Some(1989), None);
        let c = dig_entry(&mut p, "c", None, "", "", None, None);
        p.sort_by(Field::Year, Dir::Asc);
        assert_eq!(sorted_titles(&p), ["b", "a", "c"]);
        for (id, d, artist) in [
            (a, 300.0, "Mira Sol"),
            (b, 200.0, ""),
            (c, 100.0, "nightcraft"),
        ] {
            let e = p.entries_mut().find(|e| e.id == id).unwrap();
            e.duration = Some(d);
            e.artist = artist.into();
        }
        p.sort_by(Field::Time, Dir::Desc);
        assert_eq!(sorted_titles(&p), ["a", "b", "c"]);
        p.sort_by(Field::Artist, Dir::Asc);
        assert_eq!(
            sorted_titles(&p),
            ["a", "c", "b"],
            "case-insensitive, no artist last"
        );
    }

    #[test]
    fn for_sale_sorts_by_price_then_none_then_unknown() {
        let mut p = Playlist::default();
        for (t, sale) in [
            ("unknown", None),
            ("none", Some((0, None))),
            ("cheap", Some((3, Some(500)))),
            ("dear", Some((1, Some(4000)))),
            ("unpriced", Some((2, None))),
        ] {
            dig_entry(&mut p, t, None, "", "", None, sale);
        }
        p.sort_by(Field::ForSale, Dir::Asc);
        assert_eq!(
            sorted_titles(&p),
            ["cheap", "dear", "unpriced", "none", "unknown"]
        );
        p.sort_by(Field::ForSale, Dir::Desc);
        assert_eq!(
            sorted_titles(&p),
            ["dear", "cheap", "unpriced", "none", "unknown"]
        );
    }

    #[test]
    fn the_sort_mark_lasts_until_the_order_is_changed_otherwise() {
        let mut p = Playlist::default();
        for (t, b) in [("a", Some(128)), ("b", Some(122))] {
            dig_entry(&mut p, t, b, "", "", None, None);
        }
        p.sort_by(Field::Title, Dir::Asc);
        dig_entry(&mut p, "c", None, "", "", None, None);
        assert_eq!(
            p.sorted(),
            Some((Field::Title, Dir::Asc)),
            "an add in order keeps it"
        );
        dig_entry(&mut p, "0", None, "", "", None, None);
        assert_eq!(p.sorted(), None, "an add out of order clears it");
        p.sort_by(Field::Title, Dir::Asc);
        p.move_entry(0, 2);
        assert_eq!(p.sorted(), None, "a drag clears it");
    }

    #[test]
    fn sorting_a_thousand_entries_is_quick() {
        let mut p = Playlist::default();
        for i in 0..1000u32 {
            let cat = format!("LT-{}", (i * 7919) % 1000);
            dig_entry(
                &mut p,
                &format!("t{i}"),
                Some((i % 90 + 90) as u16),
                &cat,
                "A1",
                None,
                None,
            );
        }
        let t = std::time::Instant::now();
        p.sort_by(Field::CatNo, Dir::Asc);
        p.sort_by(Field::Bpm, Dir::Desc);
        // < 16 ms each in release; debug builds get a looser bound.
        let limit = if cfg!(debug_assertions) { 200 } else { 32 };
        assert!(t.elapsed().as_millis() < limit, "{:?}", t.elapsed());
    }

    #[test]
    fn save_and_restore() {
        let mut p = pl(3);
        let id = p.entries()[0].id;
        let mut info = tags("Echo".into(), "Crusher-P".into(), Some(230.0));
        info.album = "Vocaloid Hits".into();
        p.set_info(id, info);
        p.set_current(Some(p.entries()[2].id));
        let saved = p.to_saved();
        let text = ron::to_string(&saved).unwrap();
        assert_eq!(
            text.matches("album").count(),
            1,
            "an empty album isn't written"
        );
        let (restored, pending) = Playlist::from_saved(ron::from_str(&text).unwrap());
        assert_eq!(restored.len(), 3);
        assert_eq!(restored.entries()[0].album(), "Vocaloid Hits");
        assert_eq!(restored.current_index(), Some(2));
        assert_eq!(
            pending.len(),
            2,
            "entries without cached metadata are re-read"
        );
    }

    #[test]
    fn play_order_is_a_permutation_starting_with_first() {
        assert_eq!(play_order(4, false, Some(2), 1), [0, 1, 2, 3]);
        let order = play_order(50, true, Some(17), 12345);
        assert_eq!(order[0], 17);
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, (0..50).collect::<Vec<_>>());
        assert_ne!(order, (0..50).collect::<Vec<_>>(), "actually shuffled");
        assert_eq!(
            play_order(50, true, Some(17), 12345),
            order,
            "deterministic per seed"
        );
    }

    fn origin(release: u64, clip: &str) -> Origin {
        Origin {
            page: "https://www.discogs.com/label/1".into(),
            release: Some(release),
            label: "Lowtide Tapes".into(),
            catno: "LT-012".into(),
            year: Some(1994),
            position: "A1".into(),
            clip: Some(clip.into()),
            ..Default::default()
        }
    }

    #[test]
    fn a_playlist_file_in_the_current_format_still_loads() {
        // Written by the previous version: no source, origin or status fields.
        let text = r#"(
    entries: [
        (
            path: "/m/a.mp3",
            title: "Echo",
            artist: "Crusher-P",
            duration: Some(230.0),
        ),
        (
            path: "/m/b.flac",
        ),
    ],
    current: Some(1),
)"#;
        let saved: SavedPlaylist = ron::from_str(text).unwrap();
        let (p, pending) = Playlist::from_saved(saved.clone());
        assert_eq!(p.entries()[0].display_name(), "Crusher-P: Echo");
        assert_eq!(p.entries()[0].status, EntryStatus::Ready);
        assert_eq!(p.entries()[1].status, EntryStatus::Pending);
        assert!(
            p.entries()
                .iter()
                .all(|e| e.source.is_none() && e.origin.is_none())
        );
        assert_eq!(p.current_index(), Some(1));
        assert_eq!(pending.len(), 1);
        // And it saves back the same way (no new fields for local entries).
        assert_eq!(p.to_saved().entries[0], saved.entries[0]);
        assert!(!ron::to_string(&p.to_saved()).unwrap().contains("status"));
    }

    #[test]
    fn waiting_and_unavailable_entries_survive_a_restart() {
        let mut p = pl(1);
        let w = p.add_waiting(
            "Nightcraft",
            "Glasshouse",
            Some("https://www.youtube.com/watch?v=abcdefghijk".into()),
            Some(origin(123456, "abcdefghijk")),
            "listed",
        );
        p.set_status(w, "downloading 40%");
        let u = p.add_waiting(
            "Nightcraft",
            "Untitled",
            None,
            Some(origin(123456, "")),
            "listed",
        );
        p.set_unavailable(u, "no clip");
        let text = ron::to_string(&p.to_saved()).unwrap();
        let (r, pending) = Playlist::from_saved(ron::from_str(&text).unwrap());
        assert_eq!(pending.len(), 1, "only the local file is looked up");
        let e = &r.entries()[1];
        assert_eq!(e.status, EntryStatus::Waiting("downloading 40%".into()));
        assert_eq!(e.origin.as_ref().unwrap().catno, "LT-012");
        assert_eq!(
            e.source.as_deref(),
            Some("https://www.youtube.com/watch?v=abcdefghijk")
        );
        assert_eq!(
            r.entries()[2].status,
            EntryStatus::Unavailable("no clip".into())
        );
    }

    #[test]
    fn a_listed_record_is_replaced_in_place_by_its_clips() {
        let mut p = pl(2);
        let listed = p.add_waiting("Nightcraft", "Glasshouse EP", None, None, "listed");
        p.add([TrackRef::new("/m/after.mp3")]);
        p.set_current(Some(listed));
        p.click(2, ClickMods::default());
        let clip = |t: &str| NewEntry {
            artist: "Nightcraft".into(),
            title: t.into(),
            source: Some(format!("https://www.youtube.com/watch?v={t:x<11}")),
            origin: None,
            duration: Some(300.0),
            status: "queued".into(),
        };
        let ids = p.replace(listed, vec![clip("a"), clip("b")]);
        let titles: Vec<String> = p.entries().iter().map(|e| e.title.clone()).collect();
        assert_eq!(titles, ["0", "1", "a", "b", "after"]);
        assert_eq!(
            p.current(),
            Some(ids[0]),
            "the current entry moves to the first clip"
        );
        assert_eq!(p.selected_ids(), [ids[0]]);
        assert_eq!(
            p.get(ids[1]).unwrap().status,
            EntryStatus::Waiting("queued".into())
        );
        assert_eq!(p.get(ids[1]).unwrap().duration, Some(300.0));
        // A record with nothing to add leaves the crate.
        let gone = p.add_waiting("", "CD only", None, None, "listed");
        assert!(p.replace(gone, Vec::new()).is_empty());
        assert!(p.get(gone).is_none());
        assert!(p.remove(ids[1]));
        assert_eq!(p.len(), 4);
        // Audio that went away: waiting again.
        p.set_audio(ids[0], TrackRef::new("/cache/previews/a.m4a"));
        p.set_waiting(ids[0], "queued");
        assert_eq!(
            p.get(ids[0]).unwrap().status,
            EntryStatus::Waiting("queued".into())
        );
        assert!(p.get(ids[0]).unwrap().track.0.is_empty());
    }

    #[test]
    fn a_for_sale_snapshot_is_optional_in_saved_crates() {
        let mut o = origin(1, "x");
        let text = ron::to_string(&o).unwrap();
        assert!(!text.contains("for_sale"), "left out when unknown");
        o.for_sale = Some(ForSale {
            count: 6,
            lowest_cents: Some(900),
            currency: "EUR".into(),
            fetched_at: 1_790_000_000,
        });
        let back: Origin = ron::from_str(&ron::to_string(&o).unwrap()).unwrap();
        assert_eq!(back, o);
    }

    #[test]
    fn tags_never_overwrite_an_origin() {
        let mut p = Playlist::default();
        let id = p.add_waiting(
            "Nightcraft",
            "Glasshouse",
            None,
            Some(origin(1, "x")),
            "listed",
        );
        // Metadata for a waiting entry is ignored: it has no file yet.
        p.set_info(id, tags("t".into(), "a".into(), Some(1.0)));
        assert_eq!(
            p.get(id).unwrap().status,
            EntryStatus::Waiting("listed".into())
        );
        assert!(p.set_audio(id, TrackRef::new("/cache/abcdefghijk.m4a")));
        assert_eq!(p.get(id).unwrap().status, EntryStatus::Pending);
        let mut info = tags(
            "glasshouse (vinyl rip)".into(),
            "Unknown".into(),
            Some(301.0),
        );
        info.album = "Rips".into();
        p.set_info(id, info);
        let e = p.get(id).unwrap();
        assert_eq!(e.display_name(), "(LT-012) Nightcraft: Glasshouse");
        assert_eq!(e.album(), "", "the record's (empty) album, not the tag's");
        assert_eq!((e.duration, &e.status), (Some(301.0), &EntryStatus::Ready));
        // Entries without an origin still take the tags.
        let plain = p.add([TrackRef::new("/m/x.mp3")])[0].0;
        let mut info = tags("Title".into(), "Artist".into(), None);
        info.album = "Album".into();
        p.set_info(plain, info);
        assert_eq!(p.get(plain).unwrap().plain_name(), "Artist - Title");
        assert_eq!(p.get(plain).unwrap().album(), "Album");
    }

    /// Entries 0..6, with 3 waiting and 4 unavailable.
    fn mixed() -> (Playlist, Vec<EntryId>) {
        let mut p = pl(3);
        p.add_waiting("", "three", None, None, "listed");
        let u = p.add_waiting("", "four", None, None, "listed");
        p.set_unavailable(u, "no clip");
        p.add((5..7).map(|i| TrackRef::new(format!("/m/{i}.mp3"))));
        let ids = p.entries().iter().map(|e| e.id).collect();
        (p, ids)
    }

    #[test]
    fn the_engine_queue_skips_waiting_and_unavailable_entries() {
        let (p, ids) = mixed();
        assert_eq!(
            p.play_order(false, None, 1),
            [ids[0], ids[1], ids[2], ids[3], ids[5], ids[6]],
            "unavailable entries have no place in the order"
        );
        let queue: Vec<EntryId> = p.queue(false, None, 1).iter().map(|(id, _)| *id).collect();
        assert_eq!(queue, [ids[0], ids[1], ids[2], ids[5], ids[6]]);
        // Next/previous/repeat run over the engine queue: 2 is followed by 5, and 5 is preceded
        // by 2. Shuffle too: whatever the seed, the queue never holds 3 or 4.
        for seed in 1..50 {
            let q: Vec<EntryId> = p
                .queue(true, Some(ids[5]), seed)
                .iter()
                .map(|(id, _)| *id)
                .collect();
            assert_eq!(q[0], ids[5]);
            assert_eq!(q.len(), 5);
            assert!(!q.contains(&ids[3]) && !q.contains(&ids[4]));
        }
        // A waiting entry can't be the first of a queue.
        assert_eq!(p.queue(true, Some(ids[3]), 7).len(), 5);
    }

    #[test]
    fn an_entry_joins_the_queue_in_place_when_its_audio_arrives() {
        let (mut p, ids) = mixed();
        let order = p.play_order(true, Some(ids[0]), 99);
        let before: Vec<EntryId> = p
            .queue(true, Some(ids[0]), 99)
            .iter()
            .map(|(id, _)| *id)
            .collect();
        assert!(!before.contains(&ids[3]));
        p.set_audio(ids[3], TrackRef::new("/cache/three.m4a"));
        assert_eq!(
            p.play_order(true, Some(ids[0]), 99),
            order,
            "its place in the shuffled order doesn't change"
        );
        let after: Vec<EntryId> = p
            .queue(true, Some(ids[0]), 99)
            .iter()
            .map(|(id, _)| *id)
            .collect();
        let expected: Vec<EntryId> = order.iter().copied().filter(|&id| id != ids[4]).collect();
        assert_eq!(after, expected, "it is queued where the order puts it");
    }

    #[test]
    fn albums_group_by_release_master_or_local_tags() {
        let mut p = Playlist::default();
        let rel = |r: Option<u64>, m: Option<u64>, clip: &str| Origin {
            release: r,
            master: m,
            clip: Some(clip.into()),
            ..Default::default()
        };
        let a1 = p.add_waiting("N", "a1", None, Some(rel(Some(1), Some(9), "a")), "queued");
        let other = p.add_waiting("N", "x", None, Some(rel(Some(2), Some(9), "b")), "queued");
        let a2 = p.add_waiting("N", "a2", None, Some(rel(Some(1), Some(9), "c")), "listed");
        p.set_unavailable(a2, "no clip");
        let m1 = p.add_waiting("N", "m1", None, Some(rel(None, Some(7), "d")), "queued");
        let m2 = p.add_waiting("N", "m2", None, Some(rel(None, Some(7), "e")), "queued");
        let none = p.add_waiting("N", "n", None, Some(rel(None, None, "f")), "queued");

        assert_eq!(
            p.album_of(a1),
            [a1, a2],
            "same release, unavailable included"
        );
        assert_eq!(p.album_of(other), [other], "another pressing of master 9");
        assert_eq!(p.album_of(m2), [m1, m2], "same master, no release");
        assert!(p.album_of(none).is_empty(), "no record: no album");

        let local: Vec<EntryId> = p
            .add((0..4).map(|i| TrackRef::new(format!("/m/{i}.mp3"))))
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        for (id, artist, album) in [
            (local[0], "Artist A", "Greatest Hits"),
            (local[1], "Artist B", "Greatest Hits"),
            (local[2], "artist a ", "greatest hits"),
            (local[3], "Artist A", ""),
        ] {
            let mut info = tags("t".into(), artist.into(), None);
            info.album = album.into();
            p.set_info(id, info);
        }
        assert_eq!(
            p.album_of(local[0]),
            [local[0], local[2]],
            "case and spaces aside"
        );
        assert_eq!(p.album_of(local[1]), [local[1]], "same name, other artist");
        assert!(p.album_of(local[3]).is_empty(), "no album tag");
    }

    /// Entries at these tempos (`None`: unknown), from /m/0.mp3 on.
    fn tempos(bpms: &[Option<u16>]) -> Playlist {
        let mut p = pl(bpms.len());
        for (e, b) in p.entries.iter_mut().zip(bpms) {
            e.bpm = *b;
        }
        p
    }

    #[test]
    fn a_bpm_range_shows_the_entries_inside_it() {
        let mut p = tempos(&[Some(124), Some(128), Some(137), Some(139)]);
        assert_eq!(p.tempo_span(), Some((124, 139)));
        assert_eq!(p.shown_rows(), [0, 1, 2, 3], "no filter: everything");
        assert!(p.set_bpm_filter(Some((140, 130))), "either order");
        assert_eq!(
            p.bpm_filter(),
            Some((130, 139)),
            "within the crate's tempos"
        );
        assert_eq!(p.shown_rows(), [2, 3]);
        assert!(p.set_bpm_filter(Some((100, 200))), "the whole span: off");
        assert_eq!(p.bpm_filter(), None);
        assert!(!p.set_bpm_filter(None), "already off");
    }

    #[test]
    fn entries_without_a_tempo_show_only_without_a_filter() {
        let mut p = tempos(&[Some(124), None, Some(139), None, None]);
        assert_eq!(p.without_bpm(), 3);
        assert_eq!(p.shown_rows().len(), 5);
        p.set_bpm_filter(Some((130, 140)));
        assert_eq!(p.shown_rows(), [2]);
        assert!(!p.shows(&p.entries[1].clone()));
    }

    #[test]
    fn a_range_needs_two_tempos_and_follows_them() {
        let mut p = tempos(&[Some(128), None]);
        assert_eq!(p.tempo_span(), None, "one tempo: no range");
        p.set_bpm_filter(Some((120, 125)));
        assert_eq!(p.bpm_filter(), None);
        let mut p = tempos(&[Some(124), Some(128), Some(137), Some(139)]);
        p.set_bpm_filter(Some((130, 139)));
        // The fast tracks go: the range no longer reaches any tempo, so it is off again.
        p.remove_ids(&[p.entries[2].id, p.entries[3].id]);
        assert_eq!(p.bpm_filter(), None);
        assert_eq!(p.shown_rows(), [0, 1]);
    }

    #[test]
    fn the_range_is_saved_with_the_crate() {
        let mut p = tempos(&[Some(124), Some(128), Some(137), Some(139)]);
        let text = ron::to_string(&p.to_saved()).unwrap();
        assert!(
            !text.contains("bpm_range"),
            "nothing saved without a filter"
        );
        p.set_bpm_filter(Some((130, 140)));
        let text = ron::to_string(&p.to_saved()).unwrap();
        let (restored, _) = Playlist::from_saved(ron::from_str(&text).unwrap());
        assert_eq!(restored.bpm_filter(), Some((130, 139)));
        // A hand-edited, upside-down or far-off range applies as nothing.
        let mut saved = p.to_saved();
        saved.bpm_range = Some((300, 400));
        assert_eq!(Playlist::from_saved(saved).0.bpm_filter(), None);
    }

    #[test]
    fn filtering_a_big_crate_is_quick() {
        let bpms: Vec<Option<u16>> = (0..1000)
            .map(|i| (i % 7 != 0).then_some(90 + (i % 80) as u16))
            .collect();
        let mut p = tempos(&bpms);
        let t = std::time::Instant::now();
        p.set_bpm_filter(Some((120, 140)));
        let rows = p.shown_rows();
        assert!(!rows.is_empty());
        assert!(
            t.elapsed() < std::time::Duration::from_millis(16),
            "{:?}",
            t.elapsed()
        );
    }

    #[test]
    fn the_play_order_keeps_to_the_filter_and_the_current_entry() {
        let mut p = tempos(&[Some(124), Some(134), Some(124), Some(124), Some(138)]);
        let ids: Vec<EntryId> = p.entries.iter().map(|e| e.id).collect();
        p.set_current(Some(ids[1]));
        p.set_bpm_filter(Some((130, 140)));
        assert_eq!(
            p.play_order(false, None, 0),
            [ids[1], ids[4]],
            "next after 2 is 5"
        );
        // The playing entry hidden by a new range plays on, and the next shown one follows.
        p.set_bpm_filter(Some((136, 140)));
        assert_eq!(p.play_order(false, None, 0), [ids[1], ids[4]]);
        let shuffled = p.play_order(true, Some(ids[1]), 7);
        assert_eq!(shuffled.len(), 2);
        assert_eq!(shuffled[0], ids[1]);
    }

    /// A crate of one entry per record, release `i + 1` with these styles ("" for none).
    fn styled(styles: &[&str]) -> (Playlist, Vec<EntryId>) {
        let mut p = Playlist::default();
        let ids = styles
            .iter()
            .enumerate()
            .map(|(i, st)| {
                let o = Origin {
                    styles: (*st).into(),
                    ..origin(i as u64 + 1, &format!("c{i}"))
                };
                p.add_waiting("A", format!("t{i}"), None, Some(o), "listed")
            })
            .collect();
        (p, ids)
    }

    /// A crate of one entry per record, release `i + 1`, with these record artists and labels
    /// (and the style "Techno"); the entries' own artists are "Track <i>".
    fn credited(records: &[(&str, &str)]) -> (Playlist, Vec<EntryId>) {
        let mut p = Playlist::default();
        let ids = records
            .iter()
            .enumerate()
            .map(|(i, (artist, label))| {
                let o = Origin {
                    styles: "Techno".into(),
                    artist: (*artist).into(),
                    label: (*label).into(),
                    ..origin(i as u64 + 1, &format!("c{i}"))
                };
                p.add_waiting(
                    format!("Track {i}"),
                    format!("t{i}"),
                    None,
                    Some(o),
                    "listed",
                )
            })
            .collect();
        (p, ids)
    }

    #[test]
    fn the_artist_filter_matches_the_record_credit_exactly() {
        let (mut p, _) = credited(&[
            ("Theo Parrish", "Sound Signature"),
            ("Theo Parrish & Marcellus Pittman", "Unirhythm"),
            ("Various", "Sound Signature"),
            ("Theo Parrish", "Ugly Edits"),
        ]);
        p.set_pick(Facet::Artist, "Theo Parrish", true);
        assert_eq!(
            p.shown_rows(),
            [0, 3],
            "not the joint credit, nor a compilation"
        );
        // Any of the picked artists.
        p.set_pick(Facet::Artist, "Various", true);
        assert_eq!(p.shown_rows(), [0, 2, 3]);
        // And the label filter on top: both must pass.
        p.set_pick(Facet::Label, "Sound Signature", true);
        assert_eq!(p.shown_rows(), [0, 2]);
        assert!(p.picks_filter());
        // The track's own credit plays no part.
        p.clear_picks(None);
        p.set_pick(Facet::Artist, "Track 0", true);
        assert_eq!(p.filter(Facet::Artist), None, "no record is credited so");
        assert_eq!(p.shown_rows(), [0, 1, 2, 3]);
    }

    #[test]
    fn labels_and_artists_are_counted_by_record_and_saved() {
        let (mut p, _) = credited(&[
            ("Nightcraft", "Lowtide Tapes"),
            ("Nightcraft", "Lowtide Tapes"),
            ("Lumen", "Analogical Force"),
            ("", ""),
        ]);
        assert_eq!(
            p.counts(Facet::Label),
            [
                ("Lowtide Tapes".to_owned(), 2),
                ("Analogical Force".to_owned(), 1)
            ]
        );
        assert_eq!(p.counts(Facet::Artist)[0], ("Nightcraft".to_owned(), 2));
        // A record without a label shows only while the label filter is off.
        p.set_pick(Facet::Label, "Analogical Force", true);
        assert_eq!(p.shown_rows(), [2]);
        p.set_pick(Facet::Artist, "Lumen", true);
        let text = ron::to_string(&p.to_saved()).unwrap();
        let (restored, _) = Playlist::from_saved(ron::from_str(&text).unwrap());
        assert!(restored.picked(Facet::Label, "Analogical Force"));
        assert!(restored.picked(Facet::Artist, "Lumen"));
        assert_eq!(restored.shown_rows(), [2]);
        // Show all records: the three filters off, the BPM range kept.
        let mut p = restored;
        for (e, b) in p.entries.iter_mut().zip([124, 134, 134, 134]) {
            e.bpm = Some(b);
        }
        p.set_bpm_filter(Some((130, 140)));
        assert!(p.clear_picks(None));
        assert!(!p.picks_filter());
        assert_eq!(p.shown_rows(), [1, 2, 3]);
    }

    #[test]
    fn a_record_not_on_vinyl_is_marked_with_its_first_format() {
        let mark = |formats: &str| {
            let mut p = Playlist::default();
            let o = Origin {
                formats: formats.into(),
                ..Default::default()
            };
            let id = p.add_waiting("A", "t", None, Some(o), "listed");
            p.get(id).unwrap().format_mark()
        };
        assert_eq!(mark("File"), Some("FILE"));
        assert_eq!(mark("CD, File"), Some("CD"));
        assert_eq!(mark("Cassette"), Some("CASS"));
        assert_eq!(mark("Other"), Some("OTHER"));
        assert_eq!(mark("Vinyl"), None);
        assert_eq!(mark("Vinyl, CD"), None, "vinyl is the norm");
        assert_eq!(mark(""), None, "unknown");
    }

    #[test]
    fn a_style_filter_shows_records_with_any_selected_style() {
        let (mut p, ids) = styled(&["Deep House", "Minimal, Techno", "Electro", ""]);
        assert!(!p.is_filtered());
        p.set_pick(Facet::Style, "Deep House", true);
        p.set_pick(Facet::Style, "Minimal", true);
        assert_eq!(p.shown_rows(), [0, 1], "any of them; none for no style");
        assert!(p.is_filtered());
        assert!(!p.shows(&p.entries[3].clone()));
        // Both filters: a tempo in the range, and a selected style.
        for (e, b) in p.entries.iter_mut().zip([124, 134, 134, 134]) {
            e.bpm = Some(b);
        }
        p.set_bpm_filter(Some((130, 140)));
        assert_eq!(p.shown_rows(), [1]);
        p.set_current(Some(ids[0]));
        assert_eq!(
            p.play_order(false, None, 0),
            [ids[0], ids[1]],
            "the current plays on"
        );
        assert!(p.clear_filters());
        assert_eq!(p.shown_rows(), [0, 1, 2, 3]);
        assert!(!p.clear_filters(), "already off");
    }

    #[test]
    fn styles_are_counted_by_record_most_first() {
        // Two entries of release 1 count once.
        let (mut p, _) = styled(&["Minimal, Deep House", "Deep House", "Electro"]);
        let more = Origin {
            styles: "Minimal, Deep House".into(),
            ..origin(1, "c9")
        };
        p.add_waiting("A", "t9", None, Some(more), "listed");
        assert_eq!(
            p.counts(Facet::Style),
            [
                ("Deep House".to_owned(), 2),
                ("Electro".to_owned(), 1),
                ("Minimal".to_owned(), 1)
            ]
        );
    }

    #[test]
    fn a_style_gone_from_the_crate_stops_filtering() {
        let (mut p, ids) = styled(&["Deep House", "Electro"]);
        p.set_pick(Facet::Style, "Electro", true);
        assert_eq!(p.shown_rows(), [1]);
        p.remove_ids(&[ids[1]]);
        assert_eq!(p.filter(Facet::Style), None);
        assert_eq!(p.shown_rows(), [0], "everything shows again");
    }

    #[test]
    fn the_style_filter_is_saved_with_the_crate() {
        let (mut p, _) = styled(&["Deep House", "Electro"]);
        let text = ron::to_string(&p.to_saved()).unwrap();
        assert!(
            !text.contains("styles: ["),
            "nothing saved without a filter"
        );
        p.set_pick(Facet::Style, "Deep House", true);
        let text = ron::to_string(&p.to_saved()).unwrap();
        let (restored, _) = Playlist::from_saved(ron::from_str(&text).unwrap());
        assert!(restored.picked(Facet::Style, "Deep House"));
        assert_eq!(restored.shown_rows(), [0]);
    }

    /// A crate of one entry per record: (record artist, label, catalogue number, track title).
    fn searchable(records: &[(&str, &str, &str, &str)]) -> Playlist {
        let mut p = Playlist::default();
        for (i, (artist, label, catno, title)) in records.iter().enumerate() {
            let o = Origin {
                artist: (*artist).into(),
                label: (*label).into(),
                catno: (*catno).into(),
                ..origin(i as u64 + 1, &format!("c{i}"))
            };
            p.add_waiting(*artist, *title, None, Some(o), "listed");
        }
        p
    }

    #[test]
    fn the_search_needs_every_word_in_some_field() {
        let mut p = searchable(&[
            (
                "Theo Parrish",
                "Sound Signature",
                "SS-001",
                "Summertime Is Here",
            ),
            ("Nightcraft", "Lowtide Tapes", "LT-012", "Night Moves"),
            ("Nightcraft", "Lowtide Tapes", "LT-007", "Dawn"),
            ("Âme", "Innervisions", "IV-09", "Rej"),
        ]);
        assert!(p.set_search("parrish"));
        assert_eq!(p.shown_rows(), [0]);
        assert!(p.is_filtered());
        // Words in different fields, all of them needed.
        p.set_search("lowtide 012");
        assert_eq!(p.shown_rows(), [1]);
        p.set_search("  night   ");
        assert_eq!(p.shown_rows(), [1, 2], "artist or title");
        // Case and accents are folded as the skin font draws them.
        p.set_search("AME");
        assert_eq!(p.shown_rows(), [3]);
        p.set_search("innervisions rej");
        assert_eq!(p.shown_rows(), [3]);
        assert!(!p.set_search("innervisions  rej"), "the same words");
        // Nothing matches; then the empty search shows every entry.
        p.set_search("electro");
        assert!(p.shown_rows().is_empty());
        p.set_search("");
        assert_eq!(p.shown_rows(), [0, 1, 2, 3]);
        assert!(!p.is_filtered());
    }

    #[test]
    fn starting_a_search_clears_the_filters_set_before_it() {
        let mut p = searchable(&[
            ("Theo Parrish", "Sound Signature", "SS-001", "Summer"),
            ("Nightcraft", "Lowtide Tapes", "LT-012", "Night"),
        ]);
        p.set_pick(Facet::Label, "Lowtide Tapes", true);
        p.set_cart_only(true);
        assert!(!p.set_search("   "), "blanks start no search");
        assert!(p.filter(Facet::Label).is_some());
        p.set_search("parrish");
        assert!(p.filter(Facet::Label).is_none(), "the label pick is gone");
        assert!(p.cart_only(), "the CART switch stays");
        // A filter set during the search narrows it, and editing the search keeps it.
        p.set_cart_only(false);
        p.set_pick(Facet::Label, "Lowtide Tapes", true);
        assert!(p.shown_rows().is_empty());
        p.set_search("parrish summer");
        assert!(p.filter(Facet::Label).is_some());
        // Clearing the search brings nothing back.
        p.set_search("");
        assert_eq!(p.shown_rows(), [1]);
    }

    #[test]
    fn the_search_finds_a_local_file_by_its_album_and_is_not_saved() {
        let mut p = Playlist::default();
        p.add([TrackRef::new("/m/a.flac"), TrackRef::new("/m/b.flac")]);
        p.entries[1].album = "Dub Housing".into();
        p.set_search("housing");
        assert_eq!(p.shown_rows(), [1]);
        let text = ron::to_string(&p.to_saved()).unwrap();
        let (restored, _) = Playlist::from_saved(ron::from_str(&text).unwrap());
        assert_eq!(restored.shown_rows(), [0, 1], "a search isn't saved");
        // Clearing the filters clears the search too.
        assert!(p.clear_filters());
        assert_eq!(p.shown_rows(), [0, 1]);
    }

    #[test]
    fn search_marks_cover_the_matched_chars() {
        let words = fold_words("par ère");
        let marks = search_marks("Théo Parrish · Père", &words);
        let lit: String = "Théo Parrish · Père"
            .chars()
            .zip(&marks)
            .map(|(c, &m)| if m { c } else { '.' })
            .collect();
        assert_eq!(lit, ".....Par........ère");
        assert!(search_marks("abc", &[]).iter().all(|&m| !m));
    }

    #[test]
    fn next_follows_the_search_and_the_playing_entry_stays() {
        let mut p = searchable(&[
            ("Theo Parrish", "Sound Signature", "SS-001", "A"),
            ("Nightcraft", "Lowtide Tapes", "LT-012", "B"),
            ("Theo Parrish", "Sound Signature", "SS-002", "C"),
        ]);
        let ids: Vec<EntryId> = p.entries().iter().map(|e| e.id).collect();
        p.set_current(Some(ids[1]));
        p.set_search("parrish");
        assert_eq!(p.shown_rows(), [0, 2]);
        // The hidden playing entry plays on, and the next shown one follows it.
        assert_eq!(p.play_order(false, None, 0), [ids[0], ids[1], ids[2]]);
        p.set_current(Some(ids[0]));
        assert_eq!(p.play_order(false, None, 0), [ids[0], ids[2]]);
    }

    #[test]
    fn searching_a_big_crate_is_quick() {
        let names = [
            "Theo Parrish",
            "Nightcraft",
            "Âme",
            "Moodymann",
            "DJ Sprinkles",
        ];
        let mut p = Playlist::default();
        for i in 0..5000u64 {
            let o = Origin {
                artist: names[i as usize % 5].into(),
                label: format!("Label {}", i % 120),
                catno: format!("CAT-{i:04}"),
                ..origin(i + 1, &format!("c{i}"))
            };
            p.add_waiting(
                names[i as usize % 5],
                format!("Track number {i}"),
                None,
                Some(o),
                "listed",
            );
        }
        let best = |f: &mut dyn FnMut()| {
            (0..3)
                .map(|_| {
                    let t = std::time::Instant::now();
                    f();
                    t.elapsed()
                })
                .min()
                .unwrap()
        };
        // The target is for release builds; debug ones are several times slower.
        let budget = if cfg!(debug_assertions) { 150 } else { 16 };
        let took = best(&mut || {
            p.set_search("");
            p.set_search("sprinkles label 7");
            let rows = p.shown_rows();
            let order = p.play_order(false, None, 0);
            assert!(!rows.is_empty() && !order.is_empty());
        });
        assert!(took < std::time::Duration::from_millis(budget), "{took:?}");
    }

    #[test]
    fn style_filtering_a_big_crate_is_quick() {
        let names = [
            "Deep House",
            "Minimal",
            "Techno",
            "Dub Techno",
            "Electro",
            "Ambient",
        ];
        let st: Vec<String> = (0..5000)
            .map(|i| format!("{}, {}", names[i % 6], names[(i / 6) % 6]))
            .collect();
        let refs: Vec<&str> = st.iter().map(String::as_str).collect();
        let (mut p, _) = styled(&refs);
        // The best of three, so a busy machine doesn't decide it.
        let best = |f: &mut dyn FnMut()| {
            (0..3)
                .map(|_| {
                    let t = std::time::Instant::now();
                    f();
                    t.elapsed()
                })
                .min()
                .unwrap()
        };
        // The target is for release builds; debug ones are several times slower.
        let budget = if cfg!(debug_assertions) { 100 } else { 16 };
        let took = best(&mut || {
            p.set_pick(Facet::Style, "Electro", false);
            p.set_pick(Facet::Style, "Electro", true);
            let rows = p.shown_rows();
            let order = p.play_order(false, None, 0);
            assert!(!rows.is_empty() && !order.is_empty());
        });
        assert!(took < std::time::Duration::from_millis(budget), "{took:?}");
        // Counting styles runs again only when the crate changes, not per frame.
        let budget = if cfg!(debug_assertions) { 200 } else { 16 };
        let took = best(&mut || assert_eq!(p.counts(Facet::Style).len(), 6));
        assert!(took < std::time::Duration::from_millis(budget), "{took:?}");
    }

    /// A crate of `releases` (one entry each, clip `c<i>`), plus a local file without an album
    /// where `None` is given.
    fn records(releases: &[Option<u64>]) -> (Playlist, Vec<EntryId>) {
        let mut p = Playlist::default();
        let ids = releases
            .iter()
            .enumerate()
            .map(|(i, r)| match r {
                Some(r) => p.add_waiting(
                    "A",
                    format!("t{i}"),
                    None,
                    Some(origin(*r, &format!("c{i}"))),
                    "listed",
                ),
                None => p.add([TrackRef::new(format!("/m/{i}.mp3"))])[0].0,
            })
            .collect();
        (p, ids)
    }

    fn order(p: &Playlist) -> Vec<String> {
        p.entries().iter().map(|e| e.title.clone()).collect()
    }

    #[test]
    fn gathering_moves_each_record_up_to_its_first_entry() {
        // 12, 13, 14 and 40 are one release (here t0, t1, t2, t5); t3 has no album.
        let (mut p, ids) = records(&[Some(1), Some(1), Some(1), None, Some(2), Some(1), Some(3)]);
        p.set_current(Some(ids[5]));
        let rev = p.rev();
        assert!(p.set_grouped(true));
        assert_eq!(
            order(&p),
            ["t0", "t1", "t2", "t5", "/m/3", "t4", "t6"]
                .map(|t| t.trim_start_matches("/m/").to_owned())
        );
        assert_eq!(
            p.current(),
            Some(ids[5]),
            "the playing entry is the same one"
        );
        assert!(p.rev() > rev);
        // Already gathered: nothing moves, and ungrouping keeps the order.
        assert!(!p.gather());
        p.set_grouped(false);
        assert_eq!(order(&p)[3], "t5");
    }

    #[test]
    fn entries_added_while_grouped_join_their_record_at_the_next_settle() {
        let (mut p, _) = records(&[Some(1), Some(2), Some(3)]);
        p.set_grouped(true);
        p.add_waiting("A", "t3", None, Some(origin(1, "c3")), "listed");
        assert_eq!(order(&p).last().unwrap(), "t3", "appended until settled");
        assert!(p.settle());
        assert_eq!(order(&p), ["t0", "t3", "t1", "t2"]);
        assert!(!p.settle(), "nothing more to place");
        // Flat crates never move.
        let (mut flat, _) = records(&[Some(1), Some(2)]);
        flat.add_waiting("A", "t2", None, Some(origin(1, "c2")), "listed");
        assert!(!flat.settle());
        assert_eq!(order(&flat), ["t0", "t1", "t2"]);
    }

    #[test]
    fn sorting_a_grouped_crate_gathers_after_sorting() {
        let (mut p, _) = records(&[Some(1), Some(2), Some(1)]);
        for (e, bpm) in p.entries.iter_mut().zip([130, 120, 110]) {
            e.bpm = Some(bpm);
        }
        p.set_grouped(true); // t0, t2, t1
        p.sort_by(Field::Bpm, Dir::Asc);
        // By tempo t2 (110) comes first, so its record leads: t2, t0, then t1.
        assert_eq!(order(&p), ["t2", "t0", "t1"]);
        assert_eq!(p.sorted(), Some((Field::Bpm, Dir::Asc)));
    }

    #[test]
    fn a_record_moves_as_a_block_and_selections_take_whole_records() {
        let (mut p, ids) = records(&[Some(1), Some(1), Some(2), Some(3)]);
        p.move_block(&[ids[0], ids[1]], Some(ids[3]));
        assert_eq!(order(&p), ["t2", "t0", "t1", "t3"]);
        p.move_block(&[ids[0], ids[1]], Some(ids[1]));
        assert_eq!(order(&p), ["t2", "t0", "t1", "t3"], "onto itself: nothing");
        p.move_block(&[ids[2]], None);
        assert_eq!(order(&p), ["t0", "t1", "t3", "t2"]);
        p.toggle_ids(&[ids[0], ids[1]]);
        assert_eq!(p.selected_ids(), [ids[0], ids[1]]);
        p.toggle_ids(&[ids[0], ids[1]]);
        assert!(p.selected_ids().is_empty());
        p.select_only(&[ids[0]], ids[0]);
        p.extend_to(3, &[ids[2]], ids[2]);
        assert_eq!(p.selected_ids().len(), 4);
    }

    #[test]
    fn the_cart_switch_shows_only_records_in_the_cart() {
        let releases: Vec<Option<u64>> = [1, 1, 2, 3].map(Some).to_vec();
        let (mut p, _) = records(&releases);
        p.set_in_cart(&BTreeSet::from([1]));
        assert!(!p.is_filtered(), "off: everything shows");
        p.set_cart_only(true);
        assert_eq!(p.shown_rows(), [0, 1]);
        assert!(p.is_filtered() && p.picks_filter());
        let before = p.rev();
        p.set_in_cart(&BTreeSet::from([1, 3]));
        assert!(p.rev() > before, "the view follows the cart");
        assert_eq!(p.shown_rows(), [0, 1, 3]);
        let (back, _) = Playlist::from_saved(p.to_saved());
        assert!(back.cart_only(), "remembered with the crate");
        assert!(p.clear_picks(None), "Show all records turns it off");
        assert!(!p.cart_only() && !p.is_filtered());
    }

    #[test]
    fn the_cart_switch_on_five_thousand_entries_is_quick() {
        let releases: Vec<Option<u64>> = (0..5000u64).map(|i| Some(i % 1500)).collect();
        let (mut p, _) = records(&releases);
        p.set_grouped(true);
        p.set_in_cart(&(0..40).collect());
        let mut took = std::time::Duration::MAX;
        for on in [true, false, true, false, true] {
            let t = std::time::Instant::now();
            p.set_cart_only(on);
            let rows = crate::records::build(&p, &Default::default());
            took = took.min(t.elapsed());
            assert!(!rows.is_empty());
        }
        let budget = if cfg!(debug_assertions) { 40 } else { 16 };
        assert!(took < std::time::Duration::from_millis(budget), "{took:?}");
    }

    #[test]
    fn gathering_five_thousand_entries_is_quick() {
        let releases: Vec<Option<u64>> = (0..5000u64).map(|i| Some(i % 1500)).collect();
        let (scattered, _) = records(&releases);
        // The best of a few runs (each from the same scattered order).
        let mut took = std::time::Duration::MAX;
        for _ in 0..5 {
            let mut p = Playlist::default();
            p.add_saved(
                scattered
                    .saved_entries(&scattered.entries().iter().map(|e| e.id).collect::<Vec<_>>()),
            );
            let t = std::time::Instant::now();
            assert!(p.set_grouped(true));
            took = took.min(t.elapsed());
        }
        let budget = if cfg!(debug_assertions) { 40 } else { 16 };
        assert!(took < std::time::Duration::from_millis(budget), "{took:?}");
    }
}
