# playlist-columns Specification

## Purpose
Once the playlist is wide, shows entries as aligned columns (catalog number, artist, title, BPM, side, year, for sale, time) that can be resized and hidden, and lets the crate be sorted by any of them.
## Requirements
### Requirement: Column layout
When the playlist is at least 480 points wide (at 1×), each entry SHALL be drawn as aligned columns:
- number, catalog number, artist, title, album, format, style and BPM;
- side, year and for sale ("6 · €9.00", "none");
- time: the duration, or the entry's status where the duration would be.

A header row SHALL name the columns. The album SHALL appear only in its column, not in the title. The format column SHALL show the record's formats ("Vinyl", "File", "Vinyl, CD"), and sorting by it SHALL order Vinyl first, then File, CD, Cassette and Other, with unknown formats last. The style column SHALL show the record's styles as Discogs lists them ("Deep House, Minimal"), cut to the column's width, and sorting by it SHALL order entries by their first style, ignoring case, with entries without styles last. Narrower than 480 points, entries SHALL be drawn in the single-line format, without a header. Empty values SHALL leave their cell blank. Column settings saved before the format or style column existed SHALL show it at its default width.

#### Scenario: Wide playlist
- **WHEN** the playlist is 700 points wide and shows a Discogs entry and a local file without an album tag
- **THEN** both are drawn in columns, and the local file's catalog number, album, format, style, side, year and for-sale cells are blank

#### Scenario: Narrow playlist
- **WHEN** the playlist is resized to 400 points wide
- **THEN** the header disappears and entries use the single-line format

#### Scenario: Sort by album
- **WHEN** a crate holds entries from albums "b", "A" and none, and the user clicks the Album header
- **THEN** the order is "A", "b", then the entry without an album

#### Scenario: Sort by format
- **WHEN** a crate holds File, Vinyl, unknown and CD entries, and the user clicks the Format header
- **THEN** the order is Vinyl, File, CD, then the unknown one

#### Scenario: Style column
- **WHEN** a record with styles "Minimal, Deep House" is shown in a playlist 700 points wide
- **THEN** its rows show "Minimal, Deep House" in the Style column, right of Format

#### Scenario: Sort by style
- **WHEN** a crate holds records styled "techno", "Ambient, Drone" and none, and the user clicks the Style header
- **THEN** the order is "Ambient, Drone", "techno", then the record without styles

#### Scenario: Old column settings
- **WHEN** column settings saved before the style column existed are loaded
- **THEN** the Style column is shown at its default width, and the hidden columns stay hidden

### Requirement: Column widths and visibility
Dragging a header divider SHALL resize the column to its left. Right-clicking the header SHALL offer to show or hide each column except number, title and time. Widths and visibility SHALL be remembered across launches. When the playlist is resized, the columns SHALL scale with it, and the title SHALL take the remaining width.

#### Scenario: Hide a column
- **WHEN** the user right-clicks the header and hides Year, then restarts the app
- **THEN** the Year column is still hidden and the title is wider

#### Scenario: Resize a column
- **WHEN** the user drags the divider right of Artist 40 points to the right
- **THEN** the Artist column is 40 points wider and the title narrower

### Requirement: Sort the crate by a column
Clicking a column header (other than the number column), or choosing that field in the playlist's ≡ ▸ Sort, SHALL reorder the shown crate by that field, ascending, and a second click SHALL reorder it descending. The rules:
- The sort SHALL be stable.
- Entries without a value SHALL go last in both directions.
- Catalog numbers and sides SHALL sort in natural order (LT-2 before LT-10, A2 before A10).
- For sale SHALL sort by lowest price, with "none for sale" after priced entries and before entries with no snapshot.

The new order SHALL be the crate's order for playback, saving and export. The playing entry SHALL keep playing, and the next track SHALL follow the new order. The header SHALL mark the sorted column and direction until the crate is reordered by hand. Sorting a 1,000-entry crate SHALL take less than 16 ms and SHALL NOT affect playback.

#### Scenario: Sort by BPM
- **WHEN** a crate holds entries at 128, 122, unknown and 140 BPM and the user clicks the BPM header
- **THEN** the order is 122, 128, 140, unknown, and the header marks BPM ascending

#### Scenario: Descending keeps unknowns last
- **WHEN** the user clicks the BPM header again
- **THEN** the order is 140, 128, 122, unknown

#### Scenario: Natural catalog order
- **WHEN** entries have catalog numbers LT-10, LT-2 and LT-1 and the user sorts by Cat#
- **THEN** the order is LT-1, LT-2, LT-10

#### Scenario: Next follows the sort
- **WHEN** entry "A" plays and a sort moves entry "B" directly after it
- **THEN** "A" keeps playing without interruption, and B plays next

#### Scenario: Manual reorder clears the mark
- **WHEN** the crate is sorted by Year and the user drags an entry to another position
- **THEN** the header no longer marks Year as sorted

#### Scenario: Big crate
- **WHEN** a 1,000-entry crate is sorted by Artist during playback
- **THEN** the sort takes less than 16 ms and playback has zero underruns

