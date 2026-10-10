//! Playlist columns and sorting a crate by one of them. A sort is a one-off reorder, as in
//! classic players: the crate's order changes, and play order, saving and export follow it. Entries
//! without a value go last whichever the direction, so a fresh dig crate sorted by BPM shows
//! the known tempos first.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::playlist::Entry;

/// From this playlist width (skin pixels) on, entries are drawn as columns under a header.
pub const COLUMNS_FROM_WIDTH: u16 = 480;
/// The narrowest a column gets, in skin pixels.
pub const MIN_COLUMN_W: f32 = 18.0;

/// A sortable column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Field {
    CatNo,
    Artist,
    Title,
    Album,
    Bpm,
    Side,
    Year,
    ForSale,
    Time,
    /// The record's formats ("Vinyl", "File", "Vinyl, CD").
    Format,
    /// The record's styles, as Discogs lists them ("Minimal, Deep House").
    Style,
}

impl Field {
    pub const ALL: [Field; 11] = [
        Field::CatNo,
        Field::Artist,
        Field::Title,
        Field::Album,
        Field::Format,
        Field::Style,
        Field::Bpm,
        Field::Side,
        Field::Year,
        Field::ForSale,
        Field::Time,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::CatNo => "Cat#",
            Field::Artist => "Artist",
            Field::Title => "Title",
            Field::Album => "Album",
            Field::Bpm => "BPM",
            Field::Side => "Side",
            Field::Year => "Year",
            Field::ForSale => "For sale",
            Field::Time => "Time",
            Field::Format => "Format",
            Field::Style => "Style",
        }
    }
}

/// A column: the entry's number, or a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
    Number,
    Field(Field),
}

impl Field {
    /// Number, title and time always show.
    pub fn hideable(self) -> bool {
        !matches!(self, Field::Title | Field::Time)
    }

    /// Its share of the list width when nothing else is set (the title takes what's left).
    fn default_width(self) -> f32 {
        match self {
            Field::CatNo => 0.10,
            Field::Artist => 0.15,
            Field::Title => 0.0,
            Field::Album => 0.12,
            Field::Bpm => 0.07,
            Field::Side => 0.05,
            Field::Year => 0.06,
            Field::ForSale => 0.10,
            Field::Time => 0.08,
            Field::Format => 0.06,
            Field::Style => 0.08,
        }
    }
}

/// Which columns show and how wide they are, as shares of the list's width (so they scale
/// with the playlist). The title takes the rest.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnSettings {
    pub hidden: Vec<Field>,
    pub widths: BTreeMap<Field, f32>,
}

impl ColumnSettings {
    pub fn shows(&self, f: Field) -> bool {
        !(f.hideable() && self.hidden.contains(&f))
    }

    pub fn width(&self, f: Field) -> f32 {
        self.widths.get(&f).copied().unwrap_or(f.default_width())
    }

    pub fn toggle(&mut self, f: Field) {
        if !f.hideable() {
            return;
        }
        if let Some(i) = self.hidden.iter().position(|h| *h == f) {
            self.hidden.remove(i);
        } else {
            self.hidden.push(f);
        }
    }

    /// Widens `f` by `share` of the list width (narrows it when negative), within limits.
    pub fn resize(&mut self, f: Field, share: f32) {
        if f == Field::Title {
            return;
        }
        let w = (self.width(f) + share).clamp(0.02, 0.5);
        self.widths.insert(f, w);
    }

    /// Clamps values a hand-edited file could break.
    pub fn sanitized(mut self) -> Self {
        self.hidden.retain(|f| f.hideable());
        self.hidden.dedup();
        self.widths
            .retain(|f, w| *f != Field::Title && w.is_finite());
        for w in self.widths.values_mut() {
            *w = w.clamp(0.02, 0.5);
        }
        self
    }

    /// The columns and where they sit in a list `list_w` skin pixels wide: (column, x, width),
    /// left to right. The number column is `number_w` wide; the title gets what's left (at
    /// least [`MIN_COLUMN_W`]), the others their share.
    pub fn layout(&self, list_w: f32, number_w: f32) -> Vec<(Col, f32, f32)> {
        let shown: Vec<Field> = Field::ALL.into_iter().filter(|f| self.shows(*f)).collect();
        let widths: Vec<f32> = shown
            .iter()
            .map(|f| (self.width(*f) * list_w).max(MIN_COLUMN_W))
            .collect();
        let others: f32 = shown
            .iter()
            .zip(&widths)
            .filter(|(f, _)| **f != Field::Title)
            .map(|(_, w)| w)
            .sum();
        let title = (list_w - number_w - others).max(MIN_COLUMN_W);
        let mut x = 0.0;
        let mut out = vec![(Col::Number, x, number_w)];
        x += number_w;
        for (f, w) in shown.into_iter().zip(widths) {
            let w = if f == Field::Title { title } else { w };
            out.push((Col::Field(f), x, w));
            x += w;
        }
        out
    }
}

/// What a cell shows (the time column is drawn by the row, with its status icon).
pub fn cell_text(e: &Entry, f: Field) -> String {
    let o = e.origin.as_ref();
    match f {
        Field::CatNo => o.map(|o| o.catno.clone()).unwrap_or_default(),
        Field::Artist => e.artist.clone(),
        Field::Title => e.title.clone(),
        Field::Album => e.album().to_owned(),
        Field::Bpm => e.bpm.map(|b| b.to_string()).unwrap_or_default(),
        Field::Side => o.map(|o| o.position.clone()).unwrap_or_default(),
        Field::Year => o
            .and_then(|o| o.year)
            .map(|y| y.to_string())
            .unwrap_or_default(),
        Field::ForSale => match o.and_then(|o| o.for_sale.as_ref()) {
            Some(fs) if fs.count == 0 => "none".into(),
            Some(fs) => match fs.lowest_cents {
                Some(c) => format!("{} · {}", fs.count, crate::format::price(c, &fs.currency)),
                None => fs.count.to_string(),
            },
            None => String::new(),
        },
        Field::Time => String::new(),
        Field::Format => o.map(|o| o.formats.clone()).unwrap_or_default(),
        Field::Style => o.map(|o| o.styles.clone()).unwrap_or_default(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dir {
    Asc,
    Desc,
}

impl Dir {
    pub fn flip(self) -> Self {
        match self {
            Dir::Asc => Dir::Desc,
            Dir::Desc => Dir::Asc,
        }
    }
}

/// Compares text the way people count: runs of digits by value, the rest case-insensitively
/// (`LT-2` < `LT-10`, `A2` < `A10` < `B1`).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (da, db) = (take(&mut a), take(&mut b));
                let (ta, tb) = (da.trim_start_matches('0'), db.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// Where an entry falls for one field: a known value, or one of the "last" groups (in order),
/// which stay after all known values in both directions.
#[derive(Debug, Clone, PartialEq)]
enum Key {
    Text(String),
    Num(f64),
    Last(u8),
}

const UNKNOWN: Key = Key::Last(9);

fn key(e: &Entry, field: Field) -> Key {
    let text = |s: &str| {
        let s = s.trim();
        if s.is_empty() {
            UNKNOWN
        } else {
            Key::Text(s.to_owned())
        }
    };
    let o = e.origin.as_ref();
    match field {
        Field::CatNo => text(o.map_or("", |o| o.catno.as_str())),
        Field::Artist => text(&e.artist),
        Field::Title => text(&e.title),
        Field::Album => text(e.album()),
        Field::Side => text(o.map_or("", |o| o.position.as_str())),
        Field::Bpm => e.bpm.map_or(UNKNOWN, |b| Key::Num(b as f64)),
        Field::Year => o
            .and_then(|o| o.year)
            .map_or(UNKNOWN, |y| Key::Num(y as f64)),
        Field::Time => e.duration.map_or(UNKNOWN, Key::Num),
        // Vinyl first, then File, CD, Cassette and Other (by the record's first format).
        Field::Format => {
            let first = o.map_or("", |o| o.formats.split(',').next().unwrap_or("").trim());
            ["Vinyl", "File", "CD", "Cassette", "Other"]
                .iter()
                .position(|f| *f == first)
                .map_or(UNKNOWN, |i| Key::Num(i as f64))
        }
        Field::Style => text(o.map_or("", |o| o.styles.split(',').next().unwrap_or(""))),
        // Priced first (by the lowest price), then copies with no price, then "none for
        // sale", then no snapshot at all.
        Field::ForSale => match o.and_then(|o| o.for_sale.as_ref()) {
            Some(fs) if fs.count > 0 => {
                fs.lowest_cents.map_or(Key::Last(0), |c| Key::Num(c as f64))
            }
            Some(_) => Key::Last(1),
            None => Key::Last(2),
        },
    }
}

/// How two entries order by `field` in `dir`. Only known values follow the direction.
pub fn compare(a: &Entry, b: &Entry, field: Field, dir: Dir) -> Ordering {
    let known = |o: Ordering| match dir {
        Dir::Asc => o,
        Dir::Desc => o.reverse(),
    };
    match (key(a, field), key(b, field)) {
        (Key::Last(x), Key::Last(y)) => x.cmp(&y),
        (Key::Last(_), _) => Ordering::Greater,
        (_, Key::Last(_)) => Ordering::Less,
        (Key::Num(x), Key::Num(y)) => known(x.total_cmp(&y)),
        (Key::Text(x), Key::Text(y)) => known(natural_cmp(&x, &y)),
        // One field never mixes kinds.
        (Key::Num(_), Key::Text(_)) => Ordering::Less,
        (Key::Text(_), Key::Num(_)) => Ordering::Greater,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_fill_the_list_and_the_title_takes_the_rest() {
        let mut c = ColumnSettings::default();
        let cols = c.layout(600.0, 24.0);
        let names: Vec<Col> = cols.iter().map(|c| c.0).collect();
        assert_eq!(names[0], Col::Number);
        assert_eq!(names.len(), 12);
        let (_, x, w) = cols.last().copied().unwrap();
        assert!((x + w - 600.0).abs() < 0.01, "they fill the list exactly");
        let title_w = |c: &ColumnSettings| {
            c.layout(600.0, 24.0)
                .into_iter()
                .find(|c| c.0 == Col::Field(Field::Title))
                .unwrap()
                .2
        };
        let before = title_w(&c);
        c.toggle(Field::Year);
        assert_eq!(
            c.layout(600.0, 24.0).len(),
            11,
            "a hidden column is left out"
        );
        assert!(
            (title_w(&c) - (before + 0.06 * 600.0)).abs() < 0.01,
            "and the title widens"
        );
        c.toggle(Field::Title);
        assert!(c.shows(Field::Title), "the title can't be hidden");
        // Artist 40 pixels wider: the title 40 narrower.
        let artist = |c: &ColumnSettings| {
            c.layout(600.0, 24.0)
                .into_iter()
                .find(|c| c.0 == Col::Field(Field::Artist))
                .unwrap()
                .2
        };
        let (a0, t0) = (artist(&c), title_w(&c));
        c.resize(Field::Artist, 40.0 / 600.0);
        assert!((artist(&c) - (a0 + 40.0)).abs() < 0.01);
        assert!((title_w(&c) - (t0 - 40.0)).abs() < 0.01);
    }

    #[test]
    fn column_settings_are_sanitized() {
        let mut widths = BTreeMap::new();
        widths.insert(Field::Bpm, 9.0);
        widths.insert(Field::Title, 0.3);
        widths.insert(Field::Year, f32::NAN);
        let c = ColumnSettings {
            hidden: vec![Field::Time, Field::Side, Field::Side],
            widths,
        }
        .sanitized();
        assert_eq!(c.hidden, [Field::Side]);
        assert_eq!(c.widths.len(), 1);
        assert_eq!(c.width(Field::Bpm), 0.5);
    }

    #[test]
    fn natural_order_counts_numbers() {
        let mut v = vec!["LT-10", "lt-2", "LT-1", "B1", "A10", "A2", "LT-02"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["A2", "A10", "B1", "LT-1", "lt-2", "LT-02", "LT-10"]);
        assert_eq!(natural_cmp("abc", "ABC"), Ordering::Equal);
        assert_eq!(natural_cmp("a", "a1"), Ordering::Less);
    }

    #[test]
    fn album_sorts_without_regard_to_case_and_empties_go_last() {
        use crate::playlist::{Origin, Playlist};
        let mut p = Playlist::default();
        for album in ["b", "", "A"] {
            let origin = Origin {
                album: album.into(),
                ..Default::default()
            };
            p.add_waiting("x", "t", None, Some(origin), "listed");
        }
        let mut v: Vec<&Entry> = p.entries().iter().collect();
        for dir in [Dir::Asc, Dir::Desc] {
            v.sort_by(|a, b| compare(a, b, Field::Album, dir));
            let albums: Vec<String> = v.iter().map(|e| cell_text(e, Field::Album)).collect();
            let want = if dir == Dir::Asc {
                ["A", "b", ""]
            } else {
                ["b", "A", ""]
            };
            assert_eq!(albums, want, "{dir:?}");
        }
        assert!(Field::Album.hideable());
        let order: Vec<Col> = ColumnSettings::default()
            .layout(700.0, 24.0)
            .into_iter()
            .map(|c| c.0)
            .collect();
        let at = |f| order.iter().position(|c| *c == Col::Field(f)).unwrap();
        assert_eq!(at(Field::Album), at(Field::Title) + 1, "after the title");
        assert_eq!(at(Field::Format), at(Field::Album) + 1, "then the format");
        assert_eq!(at(Field::Style), at(Field::Format) + 1, "then the styles");
        assert_eq!(at(Field::Bpm), at(Field::Style) + 1, "before the BPM");
    }

    #[test]
    fn style_shows_the_styles_and_sorts_by_the_first() {
        use crate::playlist::{Origin, Playlist};
        let mut p = Playlist::default();
        for styles in ["techno", "", "Ambient, Drone", "Minimal, Deep House"] {
            let origin = Origin {
                styles: styles.into(),
                ..Default::default()
            };
            p.add_waiting("x", "t", None, Some(origin), "listed");
        }
        let mut v: Vec<&Entry> = p.entries().iter().collect();
        v.sort_by(|a, b| compare(a, b, Field::Style, Dir::Asc));
        let cells: Vec<String> = v.iter().map(|e| cell_text(e, Field::Style)).collect();
        assert_eq!(
            cells,
            ["Ambient, Drone", "Minimal, Deep House", "techno", ""]
        );
        // Saved settings from before the column existed show it, and keep what was hidden.
        let old: ColumnSettings = ron::from_str("(hidden: [Year], widths: {})").unwrap();
        assert!(old.shows(Field::Style));
        assert!(!old.shows(Field::Year));
        assert_eq!(old.width(Field::Style), 0.08);
    }

    #[test]
    fn format_sorts_vinyl_first_and_unknown_last() {
        use crate::playlist::{Origin, Playlist};
        let mut p = Playlist::default();
        for formats in ["File", "Vinyl", "", "CD", "Vinyl, CD"] {
            let origin = Origin {
                formats: formats.into(),
                ..Default::default()
            };
            p.add_waiting("x", "t", None, Some(origin), "listed");
        }
        let mut v: Vec<&Entry> = p.entries().iter().collect();
        v.sort_by(|a, b| compare(a, b, Field::Format, Dir::Asc));
        let cells: Vec<String> = v.iter().map(|e| cell_text(e, Field::Format)).collect();
        assert_eq!(cells, ["Vinyl", "Vinyl, CD", "File", "CD", ""]);
        // Saved settings from before the column existed show it.
        let old: ColumnSettings = ron::from_str("(hidden: [Year], widths: {})").unwrap();
        assert!(old.shows(Field::Format));
        assert_eq!(old.width(Field::Format), 0.06);
    }
}
