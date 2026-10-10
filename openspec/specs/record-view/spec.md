# record-view Specification

## Purpose
Shows a crate one row per record, as diggers think: its cover, artist and album, styles, pressing and what's for sale, opening to its tracks. A record selects, plays, moves and counts as one, the play order follows what's on screen, and only the covers in view are fetched.
## Requirements
### Requirement: Group by record
A crate SHALL be shown either flat (one row per entry, as before) or grouped by record. While it is grouped:
- each album of the crate (see `album-entries`) SHALL be one record row, two rows high;
- an album with one entry SHALL be a record row that acts as that entry, with nothing to open;
- an entry that belongs to no album SHALL be an ordinary row.

The choice SHALL be made with the ▤ button in the playlist title bar (lit while grouped) or with "Group by record" in the ≡ menu. It SHALL be remembered per crate across launches. A crate that has never been toggled SHALL be grouped when it is the user's Discogs wantlist or collection crate, and flat otherwise. Toggling SHALL NOT interrupt playback.

#### Scenario: Toggle
- **WHEN** a flat crate of 12 entries from 4 releases is shown and the user clicks ▤
- **THEN** the crate shows 4 record rows and ▤ is lit; clicking ▤ again shows the 12 entries flat

#### Scenario: Discogs crates default to grouped
- **WHEN** "Collection: digger" is shown for the first time after this change
- **THEN** it is grouped, and the other crates are flat

#### Scenario: Remembered
- **WHEN** the user groups crate "Friday" and restarts the app
- **THEN** "Friday" is still grouped

#### Scenario: No Shift+G
- **WHEN** a flat crate is shown and the user presses Shift+G
- **THEN** the crate stays flat

### Requirement: Record row
A record row SHALL show, from left to right:
- the record's cover, as a square of the row's height, or an empty frame while it loads, or a record icon when there is none (a local album, no image, or a failed fetch);
- an open/close mark (⏵ or ⏷);
- on its first line, artist – album and the OWNED, CART, SOLD, format and ★ marks, and the record's Discogs styles right-aligned and dimmed (its genres when it has no style). In a seller crate, a record with exactly one unsold copy SHALL show that copy's cart pill (+ CART or IN CART, see `discogs-cart`) in place of CART. The artist SHALL be the record's credited artist when known (see `album-entries`), else its first entry's artist. The format mark (FILE, CD, CASS or OTHER) SHALL be shown only for a record with formats but no vinyl (see `discogs-intake`);
- on its second line, dimmed, the catalog number, year, number of tracks and for-sale snapshot, where known. In a seller crate, the for-sale snapshot SHALL be replaced by that seller's unsold copies and their price range ("3 copies €9.00–€18.00", or "1 copy €9.00").

When it holds the playing entry, its second line SHALL name that track with the play or pause sign, and the row SHALL be drawn in the highlight colour. In the column layout, a record row SHALL span the full width, track rows SHALL use the columns, and no column header SHALL be shown while the crate is grouped (its row goes to the list; ☰ › Sort still sorts). The crate sidebar SHALL show a crate's number of records (albums, and entries of no album), whether the crate is grouped or flat, and its tooltip both its records and its tracks. Hovering a record row SHALL show the tooltip of its first entry. For a Discogs record with tracks waiting to be searched or found by search, that tooltip's first detail line, labelled Record, SHALL count the record's entries, its entries with a Discogs clip, and its entries still to search ("11 tracks · 1 clip · 10 to search"); a count of zero SHALL be left out.

#### Scenario: A record
- **WHEN** a grouped crate holds 3 entries of "Glasshouse EP" by Nightcraft, LT-012, 1994, with 6 for sale from €9.00, and its cover is cached
- **THEN** one record row shows the cover, "Nightcraft – Glasshouse EP", and "LT-012 · 1994 · 3 tracks · 6 for sale from €9.00"

#### Scenario: Counting tracks and clips
- **WHEN** a grouped crate holds the 11 entries of "Mezzanine", one with its Discogs clip and 10 to search, and the pointer rests on its record row
- **THEN** the tooltip's first detail line reads Record "11 tracks · 1 clip · 10 to search"

#### Scenario: All clips
- **WHEN** the pointer rests on the record row of "Glasshouse EP", whose 3 entries all have Discogs clips
- **THEN** the tooltip has no Record line

#### Scenario: A compilation
- **WHEN** a grouped crate holds a compilation "Night Moves" credited to "Various", whose first track is by Nightcraft
- **THEN** its record row reads "Various – Night Moves"

#### Scenario: A digital-only record
- **WHEN** a grouped crate holds a record whose only format is File
- **THEN** its record row's first line shows the dim mark FILE

#### Scenario: Styles and no header
- **WHEN** a grouped crate is shown in columns and a record's styles are Deep House and Minimal
- **THEN** no column header is drawn, and the record row's first line ends with "Deep House, Minimal"

#### Scenario: Records counted
- **WHEN** the collection crate holds 4,000 tracks from 1,234 records and is grouped
- **THEN** its sidebar row shows 1,234

#### Scenario: Playing inside a closed record
- **WHEN** track A2 "Tidepool" of a closed record plays
- **THEN** the record row is highlighted and its second line reads "⏵ A2 Tidepool"

#### Scenario: In a seller crate
- **WHEN** a seller crate holds "Glasshouse EP" with copies at €9.00, €12.00 and €18.00, none sold
- **THEN** its second line reads "LT-012 · 1994 · 3 tracks · 3 copies €9.00–€18.00", and its first line shows no cart pill

#### Scenario: One copy in a seller crate
- **WHEN** a seller crate holds "Glasshouse EP" with one unsold copy at €9.00, not in the cart
- **THEN** its first line shows + CART and its second line reads "LT-012 · 1994 · 3 tracks · 1 copy €9.00"

### Requirement: Opening a record
Clicking a record row's ⏵, or pressing Space while the cursor is on it and the playlist has focus, SHALL open it: its tracks are shown as track rows under it, in crate order (indented past the cover in the single-line layout). Doing so again, or pressing Space on one of its track rows, SHALL close it. Records SHALL start closed, except that the record of a track being played or armed from a track row stays open. Which records are open SHALL be kept for the session, per crate. ← and → SHALL keep seeking.

#### Scenario: Open
- **WHEN** the user clicks ⏵ on a record of 3 tracks
- **THEN** its 3 tracks appear under it, indented, and the mark becomes ⏷

#### Scenario: Space
- **WHEN** the playlist has focus, the cursor is on a closed record row, and the user presses Space
- **THEN** the record opens, and pressing Space again closes it

#### Scenario: Arrows still seek
- **WHEN** the cursor is on a record row and the user presses →
- **THEN** playback seeks 5 s forward, and the record doesn't open

### Requirement: Play order follows the records
Turning grouping on SHALL reorder the crate once: each album's entries SHALL follow that album's first entry, keeping their relative order, and every other entry SHALL keep its place. This order SHALL be the crate's order for playback, saving and export, as after a sort. Turning grouping off SHALL keep it. While grouped, an entry added to the crate whose album is already there SHALL be placed after that album's last entry. Sorting a grouped crate SHALL sort its entries and then gather each album's entries again, at its first entry in the sorted order. Gathering a 5,000-entry crate SHALL take less than 16 ms and SHALL NOT interrupt playback.

#### Scenario: Gather
- **WHEN** entries 12, 13, 14 and 40 come from one release and the user groups the crate
- **THEN** the former entry 40 becomes entry 15, the record's tracks play in a row, and the entries that were 15 to 39 move down by one

#### Scenario: Next inside a record
- **WHEN** track A1 of a closed record plays and ends
- **THEN** track A2 of the same record plays next, without a gap

#### Scenario: New clip joins its record
- **WHEN** a grouped crate holds release 123456 in the middle, and another clip of release 123456 is sent to it
- **THEN** the clip is placed after the record's last entry, not at the end of the crate

### Requirement: Acting on a record
In a grouped crate, a record row SHALL act on all of its entries:
- clicking a record row SHALL select all its entries. Shift-click and Cmd- or Ctrl-click SHALL extend or toggle the selection by whole records;
- right-clicking a record row SHALL open the entry menu with the record's entries selected, so that Remove, Send to crate and the Discogs items act on the whole record. Remove album and Select album SHALL NOT be offered there. In the wantlist crate and the collection crate, Remove SHALL NOT be offered either (see `discogs-write`);
- double-clicking a record row, or pressing Enter on it, SHALL play its first playable track, or arm it when none is playable yet;
- dragging a record row SHALL move all its entries together, between other records. Dropping it on a crate in the sidebar SHALL send them all, unless that crate takes no hand edits;
- dragging a track row SHALL reorder it only within its record, and no insertion line SHALL be drawn outside it.

#### Scenario: Remove a record
- **WHEN** the user right-clicks a record row of 3 tracks in a dig crate and chooses Remove
- **THEN** its 3 entries are removed

#### Scenario: No Remove in the collection crate
- **WHEN** the user right-clicks a record row in the collection crate
- **THEN** the menu offers no Remove

#### Scenario: Play a record
- **WHEN** the user double-clicks a record row whose first track is still downloading and whose second is playable
- **THEN** the second track plays

#### Scenario: Move a record
- **WHEN** the user drags the third record row above the first
- **THEN** all of that record's entries come first in the crate, in their order

### Requirement: Keyboard over rows
While a grouped crate is shown and the playlist has focus, the keyboard SHALL work over the rows shown:
- ↑, ↓, PgUp, PgDn, Home and End SHALL move the cursor over the rows shown: record rows, and the track rows of open records;
- Shift with them SHALL extend the selection, a record row counting as all its entries;
- P SHALL show the playing entry's row, its track row when its record is open and its record row otherwise.

When the playing entry changes and the previous one's row was in view, the list SHALL scroll by the least amount that shows the new one's row.

#### Scenario: Walk past a closed record
- **WHEN** the cursor is on a closed record row of 4 tracks and the user presses ↓
- **THEN** the cursor moves to the next record row

#### Scenario: P in a closed record
- **WHEN** a track of a closed record far down the list plays and the user presses P
- **THEN** the list scrolls to that record row and the cursor is on it

### Requirement: Grouped view under a filter
A grouped crate SHALL show only the entries that the BPM filter shows. A record with no shown entry SHALL have no row. A record with some entries hidden SHALL say "k of N tracks" on its second line, and opening it SHALL show only its shown tracks. Play, selection and the keyboard SHALL follow the shown entries, as in the flat view.

#### Scenario: Partly hidden
- **WHEN** the range is 130–140 and a record has 4 tracks, 2 of them at 134 BPM
- **THEN** its row says "2 of 4 tracks", and opening it shows the 2 tracks at 134 BPM

### Requirement: Grouped view stays fast
Building the rows of a grouped 5,000-entry crate SHALL take less than 5 ms, and SHALL happen only when the crate, the filter or the open records change, not on every frame. Drawing, scrolling and hovering SHALL read no file and make no request. Grouped view SHALL NOT delay launch past 300 ms, and SHALL cause zero underruns.

#### Scenario: Big collection
- **WHEN** a grouped crate of 1,500 records and 5,000 entries is scrolled from top to bottom while a track plays
- **THEN** playback has zero underruns, and no frame rebuilds the rows unless an entry or the filter changed

### Requirement: Copy rows
In a grouped seller crate, an open record SHALL list its copies as copy rows above its tracks, cheapest first and sold ones last. Each copy row SHALL show its cart pill (+ CART or IN CART, see `discogs-cart`) when it is unsold, or the SOLD badge when it is sold, then the price, the media and sleeve condition, and the country it ships from. The pill's fixed width SHALL keep the prices of all unsold copy rows aligned. A copy row SHALL hold no audio:
- it SHALL be skipped by play order, shuffle, the next track, the preview horizon, and Enter or double-click to play;
- double-clicking it outside the pill SHALL open its listing on discogs.com in the default browser;
- its tooltip SHALL give the listing's date and comments;
- its right-click menu SHALL offer Open on discogs.com.

In the flat view, copies SHALL NOT be rows: a track's tooltip SHALL list its record's copies.

#### Scenario: Open record
- **WHEN** the user opens "Glasshouse EP" in a seller crate with copies at €12.00 (VG+/VG+) and €9.00 (VG+/VG), the €9.00 one in the cart
- **THEN** the record shows "IN CART €9.00 · VG+ / VG · Germany", then "+ CART €12.00 · VG+ / VG+ · Germany", both prices starting at the same x, then its tracks

#### Scenario: Skipped by play
- **WHEN** the last track of a record plays and the next record in the crate is open with two copy rows
- **THEN** that record's first track plays next, not a copy row

#### Scenario: Flat view
- **WHEN** the same seller crate is shown flat
- **THEN** no copy rows are shown, and a track's tooltip lists "€9.00 VG+/VG, €12.00 VG+/VG+"

