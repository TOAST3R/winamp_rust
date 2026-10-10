## MODIFIED Requirements

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
