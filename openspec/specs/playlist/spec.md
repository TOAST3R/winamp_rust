# playlist Specification

## Purpose
TBD - created by archiving change classic-ui. Update Purpose after archive.
## Requirements
### Requirement: Playlist display
The playlist section SHALL list entries as "N. (catno) Artist: Title · Album (T BPM)" with durations, highlight the current track, show total/selected duration, and scroll. "(catno) " SHALL appear only when the entry has a catalog number, "Artist: " only when it has an artist, " · Album" only when it has an album that differs from its title (ignoring case and surrounding spaces), and " (T BPM)" only when its tempo is known. When the row is too narrow, the album SHALL be cut before the title. An entry waiting for its audio SHALL be drawn dimmed, with an icon for its state where its duration would be: listed, queued, downloading (a bar showing the downloaded share), or needs yt-dlp. An unavailable entry SHALL be drawn dimmed, with an unavailable icon where its duration would be. Only an entry whose file could not be opened or decoded SHALL be drawn in the error colour. The state's wording (for example "downloading 40%" or "no clip") SHALL be shown in the entry's tooltip.

#### Scenario: Current highlighted
- **WHEN** track 4 is playing
- **THEN** entry 4 is drawn in the highlight color

#### Scenario: Album in the row
- **WHEN** an entry from "Glasshouse EP" is titled "Glasshouse", with catalog number LT-012, at 124 BPM
- **THEN** it is listed as "12. (LT-012) Nightcraft: Glasshouse · Glasshouse EP (124 BPM)"

#### Scenario: Single
- **WHEN** an entry titled "Static" comes from a release titled "Static"
- **THEN** its row shows no album

#### Scenario: Narrow row
- **WHEN** a row is too narrow for its whole name
- **THEN** the album is cut before any of the title is

#### Scenario: Waiting is not an error
- **WHEN** an entry's audio is 40% downloaded
- **THEN** the entry is drawn dimmed, with a bar 40% full where its duration would be, not in the error colour, and its tooltip says "downloading 40%"

#### Scenario: Unavailable stays visible
- **WHEN** an entry will never have audio because its record has no clip
- **THEN** it stays in the list, dimmed, with the unavailable icon where its duration would be, and its tooltip says "no clip"

#### Scenario: Old crate text
- **WHEN** a crate saved by an older version holds an entry waiting with the text "listed"
- **THEN** it loads and shows the listed icon

### Requirement: Adding and removing tracks
The system SHALL add tracks via file dialog, folder dialog (recursive, supported formats only), and drag-and-drop, and SHALL remove selected entries or clear the list.

#### Scenario: Drop a folder
- **WHEN** a folder with 200 audio files and some images is dropped
- **THEN** the 200 audio files are appended and the images are ignored

### Requirement: Lazy metadata
Entries SHALL appear immediately with file-name titles; tags and durations SHALL be filled by a low-priority background worker without affecting playback.

#### Scenario: Large add during playback
- **WHEN** 2,000 files are added while music plays
- **THEN** playback has no underruns and entries update progressively

### Requirement: Playback order
Double-clicking an entry SHALL play it; shuffle and repeat (off/all/one) SHALL determine the next track, and the Engine's pre-warm SHALL target the actual next track. Entries that are waiting for their audio or unavailable SHALL be skipped by next, previous, shuffle and repeat, and SHALL never be handed to the engine; an entry SHALL join the play order as soon as its audio arrives. Double-clicking an entry that is waiting for its audio SHALL arm it: the current track keeps playing, the main window says it is waiting for that entry, and the entry starts within 100 ms of its audio arriving. Starting another track SHALL cancel the arming.

#### Scenario: Shuffle pre-warm
- **WHEN** shuffle is on and the current track nears its end
- **THEN** the track chosen as next by shuffle is the one pre-warmed and played gaplessly

#### Scenario: Skip what is not ready
- **WHEN** track 3 ends while track 4 is still downloading and track 5 is playable
- **THEN** track 5 plays gaplessly after track 3, and track 4 is not marked as failed

#### Scenario: Arm a waiting entry
- **WHEN** the user double-clicks an entry whose audio is still downloading
- **THEN** the current track keeps playing, the main window says it is waiting for that entry, and the entry starts within 100 ms of its audio arriving

### Requirement: Reordering and selection
Entries SHALL support single, range (Shift), and toggle (Cmd/Ctrl) selection and drag-to-reorder.

#### Scenario: Reorder
- **WHEN** entry 7 is dragged above entry 2
- **THEN** it becomes entry 2 and numbering updates

### Requirement: Persistence and M3U
Each crate SHALL persist across restarts, and the shown crate SHALL import and export M3U/M3U8. An entry whose audio comes from a remote source SHALL be exported as that source's URL, whether or not its audio has arrived; an unavailable entry SHALL be left out of the export.

#### Scenario: Export and reimport
- **WHEN** a crate of local files is exported as M3U8 and imported into an empty crate
- **THEN** the same entries appear in the same order

#### Scenario: Export a remote entry
- **WHEN** a crate holding a local file and then an entry whose audio comes from https://www.youtube.com/watch?v=abcdefghijk is exported
- **THEN** the M3U8 lists the file's path and then that URL, and no downloaded audio file

### Requirement: Entry origin
An entry that has an origin SHALL keep it: the record the entry belongs to (source page, release, label, catalog number, year, side and clip). The origin SHALL survive reordering, sending to another crate, saving and restarting. An entry with an origin SHALL keep its own artist and title, in the playlist and in the main window's title line, even when its file's tags differ; it SHALL take only its duration from the file.

#### Scenario: Origin survives a copy and a restart
- **WHEN** an entry from release 123456 (catalog number LT-012, side A1) is sent to another crate and the app is restarted
- **THEN** the copy still has release 123456, LT-012 and A1

#### Scenario: Tags don't overwrite the record
- **WHEN** the audio of an entry shown as "Nightcraft - Glasshouse" arrives in a file whose tags say "Unknown - glasshouse (vinyl rip)"
- **THEN** the playlist and the title line still show "Nightcraft - Glasshouse", and the entry takes only its duration from the file

### Requirement: Entry tempo
Each entry SHALL carry its tempo once any analysis of its audio is known: from preview preparation, from the playing track's analysis, or from the score cache. The tempo SHALL be the tempo of the score's longest steady-tempo stretch, doubled or halved until it lies within 88–176 BPM, and rounded to a whole number. The tempo SHALL be saved with the crate. The app SHALL NOT download or analyse audio only to learn a tempo, and SHALL NOT take a tempo from file tags.

#### Scenario: Prepared preview
- **WHEN** the next entry's preview is prepared and its analysed tempo is 124.3 BPM
- **THEN** the entry shows "(124 BPM)" within one UI frame of preparation finishing

#### Scenario: Half time folded
- **WHEN** a track is analysed at 87 BPM
- **THEN** its entry shows "(174 BPM)"

#### Scenario: Double time folded
- **WHEN** a track is analysed at 280 BPM
- **THEN** its entry shows "(140 BPM)"

#### Scenario: Survives a restart
- **WHEN** an entry showing "(128 BPM)" is in a crate and the app is restarted
- **THEN** the entry shows "(128 BPM)" in the first frame, without analysis running

#### Scenario: Nothing fetched for a BPM
- **WHEN** a 312-release label crate is shown while entry 1 plays
- **THEN** only previews within the download-ahead horizon are downloaded, and entries outside it show no BPM

### Requirement: Playlist placement and size
The playlist SHALL be drawn to the right of the player column (main, waveform and EQ sections) in the same window. Its corner handle SHALL resize it freely in width, from the skin's playlist width (275 points at 1×) upwards, and in whole rows in height, never shorter than the player column. Its width and rows SHALL be remembered across launches. Hiding the playlist SHALL shrink the window to the player column. A restored width SHALL be reduced if the window wouldn't fit on its monitor. Resizing SHALL NOT affect playback.

#### Scenario: Widen
- **WHEN** the user drags the playlist's corner handle 300 points to the right
- **THEN** the playlist is 300 points wider, long names show in full where they fit, and the player column keeps its size

#### Scenario: Remembered size
- **WHEN** the playlist is resized to 700 × 30 rows and the app is restarted
- **THEN** it opens at 700 points wide with 30 rows

#### Scenario: Hidden playlist
- **WHEN** the user hides the playlist
- **THEN** the window is exactly as wide as the player column

### Requirement: Keyboard navigation
While the playlist has focus it SHALL show a keyboard cursor, drawn as an outline distinct from the selection highlight. The keys SHALL work as follows:
- ↑ and ↓ SHALL move the cursor by one entry and select only that entry;
- Shift+↑ and Shift+↓ SHALL extend the selection;
- PgUp and PgDn SHALL move by one visible page;
- Home and End SHALL move to the first and last entries;
- Enter SHALL play the cursor's entry (or arm it when it is waiting for audio).

The list SHALL scroll so the cursor stays visible. When there is no cursor yet, the first ↑ or ↓ SHALL put it on the playing entry, or on the first entry when nothing in the shown crate plays. The cursor SHALL follow its entry when entries are added, replaced or reordered around it.

#### Scenario: Walk down
- **WHEN** the playlist has focus, the cursor is on entry 9 of 40 with 10 visible rows ending at entry 10, and the user presses ↓ twice
- **THEN** the cursor and the selection are on entry 11, and the list has scrolled by one row so entry 11 is visible

#### Scenario: First press lands on the playing entry
- **WHEN** entry 23 plays, the playlist has just been focused with no cursor, and the user presses ↓
- **THEN** the cursor is on entry 23 and the list shows it

#### Scenario: Extend
- **WHEN** the cursor is on entry 5 and the user presses Shift+↓ three times
- **THEN** entries 5 to 8 are selected and the cursor is on entry 8

#### Scenario: Stable under dig expansion
- **WHEN** the cursor is on entry 30 and a "listed" entry above it is replaced by three clip entries
- **THEN** the cursor stays on the same entry, now numbered 32

### Requirement: Show the playing entry
Pressing P SHALL show the playing crate, scroll it so the playing entry is visible, and put the cursor on it. When the playing entry changes and the previous playing entry was visible, the list SHALL scroll by the least amount that shows the new one; when it wasn't visible, the list SHALL NOT scroll.

#### Scenario: P
- **WHEN** entry 180 plays while the list shows entries 1 to 20
- **THEN** pressing P scrolls to show entry 180 and puts the cursor on it

#### Scenario: Follow
- **WHEN** entry 20 is visible in the last row and playing, and the next track starts
- **THEN** the list scrolls by one row so entry 21 is visible

#### Scenario: Don't steal the scroll
- **WHEN** the user has scrolled to entries 200 to 220 while entry 5 plays, and the next track starts
- **THEN** the list stays at entries 200 to 220

### Requirement: Entry tooltip
Hovering an entry SHALL show a tooltip with everything known about it:
- its full name, even when the row truncates it;
- for an entry from Discogs, its record's cover (see Cover on hover);
- its album;
- its label, catalog number, side and year;
- its tempo and duration;
- its state and the reason;
- whether it is kept or passed, and whether a wantlist change is pending;
- its for-sale snapshot, with how long ago it was fetched.

Information that isn't known SHALL be left out. For a local file, the tooltip SHALL show its path. Building the tooltip SHALL NOT read files or make requests. A cover is fetched only by the background cover worker.

#### Scenario: Discogs entry
- **WHEN** the pointer rests on an entry from album Glasshouse EP, label Lowtide Tapes, catalog number LT-012, side A1, 1994, at 124 BPM, with 6 for sale from €9.00 fetched 3 hours ago
- **THEN** the tooltip shows all of these, including "Album: Glasshouse EP" and "fetched 3 h ago"

#### Scenario: Truncated name
- **WHEN** an entry's name is cut off in its row
- **THEN** the tooltip shows the whole name

#### Scenario: Local file
- **WHEN** the pointer rests on a local file entry with the album tag "Geogaddi"
- **THEN** the tooltip shows its album and its path, and no Discogs fields and no cover

### Requirement: Entry context menu
Right-clicking an entry (or Control-clicking it on macOS) SHALL open a menu with:
- Play, or Arm when the entry is waiting for its audio;
- Remove;
- Remove album (N tracks) and Select album, when the entry belongs to an album;
- Send to crate;
- for an entry from Discogs: Add to wantlist or Remove from wantlist (or "In collection", disabled, when the record is owned), Add to collection (or "In collection", disabled, when this pressing is owned), Retry wantlist or Retry add to collection after a failure, Pass or Undo pass, Open for-sale page, Open release on Discogs, and Copy Discogs link.

When the clicked entry is selected, Remove, Send to crate, Add to wantlist, Remove from wantlist and Add to collection SHALL act on the whole selection (the wantlist and collection items on its distinct releases, see `discogs-write`). Otherwise the selection SHALL first become the clicked entry. Remove album and Select album SHALL act on the clicked entry's album. All other items SHALL act on the clicked entry only.

#### Scenario: Remove a selection
- **WHEN** entries 3 to 6 are selected and the user right-clicks entry 4 and chooses Remove
- **THEN** entries 3 to 6 are removed

#### Scenario: Right-click outside the selection
- **WHEN** entries 3 to 6 are selected and the user right-clicks entry 10 and chooses Remove
- **THEN** only entry 10 is removed

#### Scenario: Open the release
- **WHEN** the user chooses Open release on Discogs on an entry from release 123456
- **THEN** the default browser opens https://www.discogs.com/release/123456

#### Scenario: Arm from the menu
- **WHEN** the user chooses Arm on an entry whose preview is downloading
- **THEN** the entry is armed exactly as by a double-click

#### Scenario: Album items
- **WHEN** the user right-clicks an entry of a release with 4 entries in the crate
- **THEN** the menu offers "Remove album (4 tracks)" and "Select album" after Remove

#### Scenario: Discogs items
- **WHEN** the user right-clicks an entry from release 123456, which is neither wanted nor owned
- **THEN** the menu offers "Add to wantlist (Y)" and "Add to collection", and no Keep item

#### Scenario: No Render show
- **WHEN** the user right-clicks any entry, in any crate
- **THEN** the menu offers no Render show item

### Requirement: Maximized playlist
A toggle SHALL maximize the playlist: the ⇔ button in the playlist's title bar, or Shift+P. The title bar SHALL hold, from the right: the close button, ⇔, and the ▤ button that groups the shown crate by record (see `record-view`), all at the classic 275 skin pixel width. While maximized:
- the window SHALL fill the screen's usable area (below the menu bar), as the operating system maximizes it;
- a band as tall as the waveform section (58 skin pixels) SHALL run across the top of the window. On its left, at the main window's width (275 skin pixels), it SHALL hold a mini player showing the play state, the elapsed time, the playing entry's title, previous, play or pause, stop, next, a volume slider and the ⇔ button. The waveform SHALL fill the rest of the band when it is on (see `waveform-view`); when it is off, the rest SHALL be skin panel. The band SHALL be shown whether the waveform is on or off;
- the player SHALL NOT be drawn as a strip on the left;
- the playlist SHALL take the full window width under the band, its width and rows following the window, with columns when it is wide enough;
- the EQ SHALL be hidden, and the keyboard SHALL go to the playlist.

Toggling again SHALL restore the previous window frame, layout, playlist width and rows exactly. When the playlist is hidden, the toggle SHALL show it first. The mode SHALL be remembered across launches, and a launch in this mode SHALL stay within the 300 ms launch target. Entering or leaving the mode SHALL NOT affect playback.

#### Scenario: Maximize
- **WHEN** the window is 1100 × 580 points at 2× on a 1440 × 900 display, and the user presses Shift+P
- **THEN** the window fills the area below the menu bar, the mini player is at the top left, the waveform at the top right, and the playlist fills the full width under them, with columns

#### Scenario: Restore
- **WHEN** the user clicks ⇔ while maximized
- **THEN** the window returns to 1100 × 580 points at its previous position, with the player column and the playlist at their previous width and rows

#### Scenario: Mini player controls
- **WHEN** the playlist is maximized and the user clicks next in the mini player
- **THEN** the next track starts, exactly as with B

#### Scenario: Waveform off
- **WHEN** the playlist is maximized and the waveform is hidden with W
- **THEN** the mini player is still shown at the top left, the rest of the band is skin panel, and the playlist keeps the same rows

#### Scenario: Playlist width
- **WHEN** the playlist is maximized in a window 1100 skin pixels wide
- **THEN** the playlist is 1100 skin pixels wide, and its rows fill the height under the 58-pixel band

#### Scenario: Remembered
- **WHEN** the app is quit while maximized and launched again
- **THEN** its first frame is already maximized, within 300 ms of launch

#### Scenario: Title bar buttons
- **WHEN** the playlist is shown at 275 skin pixels wide
- **THEN** its title bar shows ▤, ⇔ and the close button at its right end, and the crate name is shortened to fit before them

### Requirement: Trackpad scrolling
Scrolling the playlist SHALL add up partial scroll steps, so that slow two-finger trackpad scrolling, and its momentum, move the list one row per row-height of scrolling. Scrolling SHALL work anywhere over the list, entries included. Leftover partial steps SHALL be dropped when the pointer leaves the list or scrolling pushes past its top or bottom.

#### Scenario: Slow scroll
- **WHEN** the user scrolls the playlist down by 3 points per frame for 10 frames with 13-point rows
- **THEN** the list moves down by 2 rows

#### Scenario: No jump later
- **WHEN** 10 points of scrolling are left over and the pointer leaves the list and comes back
- **THEN** the next 3 points of scrolling don't move the list

### Requirement: Playlist footer
The playlist footer SHALL hold, from left to right: a `+` button, a `≡` button, a gear button, and the "selected/total" time readout, right-aligned in its LCD box beside the resize grip (only the total when both don't fit in the box). The footer SHALL hold no filter control (see `filter-bar`). Everything SHALL fit at the classic 275 skin pixel width. The buttons SHALL open menus:
- `+`: Add files…, Add folder…, Import M3U…;
- `≡`: Select all, Select none, Invert selection, Remove selected, Clear crate, Sort ▸ (every column's field), Group by record (a checkbox, see `record-view`), Export M3U…;
- the gear: the Options menu (see the player window's Options menu), with the same items as the right-click.

Each item SHALL act as the same item did in the footer's earlier menus. No app setting SHALL be in the `+` and `≡` menus.

#### Scenario: Three buttons
- **WHEN** the playlist is shown at 275 skin pixels wide
- **THEN** the footer shows `+`, `≡`, the gear and the time readout, and no ADD, REM, SEL, MISC or OPT button

#### Scenario: No filters in the footer
- **WHEN** the collection crate, with tempos and styles, is shown 700 pixels wide
- **THEN** the footer shows `+`, `≡`, the gear and the time readout only, and the BPM control and style chips are in the filter bar

#### Scenario: Options from the footer
- **WHEN** the user clicks the gear and chooses Discogs…
- **THEN** the Discogs dialog opens, as from the right-click Options menu

#### Scenario: Add
- **WHEN** the user clicks `+` and chooses Add folder…
- **THEN** the folder dialog opens, as ADD ▸ Add folder… did

#### Scenario: Sort from the footer
- **WHEN** the user clicks `≡` and chooses Sort ▸ BPM
- **THEN** the crate is sorted by BPM, as by clicking the BPM column header

#### Scenario: Group from the menu
- **WHEN** the user clicks `≡` and ticks Group by record
- **THEN** the shown crate is grouped, as with ▤

### Requirement: Empty crate hint
When the shown crate has no entries and is not a Discogs crate (wantlist or collection), its list SHALL show, centred in the list area under the column header, "PASTE A DISCOGS LINK · CMD+V" over "OR DROP FILES", then after a blank line "PRESS H FOR HELP", in the playlist text colour, with CTRL+V instead of CMD+V off macOS. It SHALL show in the normal and maximized layouts, SHALL NOT move, and SHALL go away as soon as the crate has an entry.

#### Scenario: New crate
- **WHEN** the user creates the crate "asdf" on macOS and it is shown
- **THEN** its list shows "PASTE A DISCOGS LINK · CMD+V", "OR DROP FILES" and, after a gap, "PRESS H FOR HELP", centred

#### Scenario: Maximized
- **WHEN** the playlist is maximized and the shown crate is empty
- **THEN** the hint shows in the middle of the list

#### Scenario: Filled
- **WHEN** a Discogs link is pasted into the empty crate and its first entry arrives
- **THEN** the hint is gone

#### Scenario: Empty wantlist crate
- **WHEN** the wantlist crate is shown and empty
- **THEN** no hint is shown, since a paste can't fill it

