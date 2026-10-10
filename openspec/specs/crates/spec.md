# crates Specification

## Purpose
Lets the player hold several named playlists ("crates"), such as one per label being dug, one for the records you keep and a scratch list, and switch between them without interrupting playback.
## Requirements
### Requirement: Named crates
The player SHALL keep any number of named crates, each an ordered list of entries. The playlist window SHALL show one crate at a time, and its title bar SHALL show that crate's name in the skin's font, shortened to fit. Crate names SHALL be 1 to 40 characters long and unique, ignoring case.

#### Scenario: Name in the title bar
- **WHEN** the shown crate is named "Lowtide Tapes"
- **THEN** the playlist title bar reads LOWTIDE TAPES

#### Scenario: Duplicate name
- **WHEN** the user tries to create a crate named "keepers" while a crate named "Keepers" exists
- **THEN** the name is refused with a message and no crate is created

### Requirement: Crate menu
While the crate sidebar isn't shown (the playlist narrower than 600 skin pixels and not maximized), clicking the playlist title bar without dragging SHALL open a menu listing every crate by name, marking the shown crate and the playing crate, followed by New crate…, Rename crate… and Delete crate…. It SHALL mark the crate the current track comes from with ⏵ only while that track plays or is paused, and list the user's Discogs crates last, under a Discogs heading: the wantlist crate, then the collection crate. While the sidebar is shown, clicking the title bar SHALL NOT open a menu: the sidebar lists, creates, renames and deletes crates. Choosing a crate SHALL show it. Dragging the title bar SHALL still move the window and SHALL NOT open the menu. Deleting a crate that has entries SHALL ask for confirmation first.

#### Scenario: Switch crate
- **WHEN** the playlist is 400 px wide and the user clicks the title bar and chooses "Keepers"
- **THEN** the playlist window shows the Keepers crate and the title bar reads KEEPERS

#### Scenario: No menu beside the sidebar
- **WHEN** the playlist is 700 px wide and the user clicks the title bar
- **THEN** no menu opens

#### Scenario: Drag still moves the window
- **WHEN** the user drags the playlist title bar
- **THEN** the window moves and no menu opens

#### Scenario: New crate
- **WHEN** the user chooses New crate… and enters "Gig 12 Oct"
- **THEN** an empty crate named "Gig 12 Oct" is created and shown

#### Scenario: Delete asks first
- **WHEN** the user chooses Delete crate… for a crate with 40 entries
- **THEN** the crate is deleted only after the user confirms

#### Scenario: Discogs crates last
- **WHEN** the crates are Playlist, Friday, "Wantlist: digger" and "Collection: digger"
- **THEN** the menu lists Playlist and Friday, then under the Discogs heading "Wantlist: digger" and "Collection: digger", in that order

### Requirement: Playback follows its crate
Switching the shown crate SHALL NOT interrupt playback. The playing track SHALL continue, and next, previous, shuffle, repeat and pre-warm SHALL follow the crate the playing track came from (the playing crate) until the user starts a track in another crate, which then becomes the playing crate. Changing a crate that is not playing SHALL NOT affect playback. Deleting the playing crate SHALL stop playback.

#### Scenario: Look elsewhere while listening
- **WHEN** track 3 of crate A is playing and the user switches to crate B
- **THEN** track 3 keeps playing without a gap, and when it ends track 4 of crate A starts

#### Scenario: Start a track in another crate
- **WHEN** the user double-clicks an entry in crate B while crate A is playing
- **THEN** that entry plays, and next and previous now follow crate B

### Requirement: The Playlist crate
A crate named "Playlist" SHALL always exist. Files opened with Eject or on the command line SHALL replace its contents, and it SHALL then be shown and played. It SHALL be possible to clear it, but not to rename or delete it. Opening files SHALL NOT replace any other crate. Adding music (ADD, drag-and-drop, Cmd+O, M3U import) SHALL add to the shown crate.

#### Scenario: Opening files leaves other crates alone
- **WHEN** crate "Lowtide Tapes" is shown and the app is started with two files on the command line
- **THEN** the Playlist crate holds exactly those two files and plays the first one, and "Lowtide Tapes" is unchanged

#### Scenario: Playlist cannot be deleted
- **WHEN** the user opens the crate menu while the Playlist crate is shown
- **THEN** Rename crate… and Delete crate… are unavailable

### Requirement: Send entries to a crate
The entry right-click menu SHALL offer Send to crate, listing the other crates and New crate…. It SHALL copy the selected entries, in their order and with their metadata and origin, to the end of the chosen crate, skipping entries that crate already holds (the same file, or the same origin clip). The source crate SHALL be unchanged.

#### Scenario: Copy without duplicates
- **WHEN** three selected entries are sent to a crate that already holds one of them
- **THEN** the other two are appended to that crate in their order, and the source crate is unchanged

### Requirement: Crates persist
Every crate SHALL be saved within 2 s of a change, and when the app quits, with its name, its entries (with their metadata and origin) and its current entry. Writes SHALL be atomic. On launch, the crate that was shown when the app closed SHALL be shown again with its current entry. A crate file that cannot be read SHALL NOT prevent launch: it SHALL be left in place and reported once, and the other crates SHALL load.

#### Scenario: Restart
- **WHEN** the app is closed while crate "Lowtide Tapes" is shown with entry 7 current, then started again
- **THEN** "Lowtide Tapes" is shown with the same entries, order and metadata, and entry 7 is current

#### Scenario: Damaged crate file
- **WHEN** one crate file contains invalid data
- **THEN** the app launches, the other crates are available, and a message names the crate that could not be read

### Requirement: Migration from the single playlist
On the first launch with crates, if the config folder holds the single playlist file and no crates yet, its entries, order, metadata and current entry SHALL become the Playlist crate. The single playlist file SHALL be left unchanged.

#### Scenario: Upgrade
- **WHEN** the app starts with a saved 300-entry playlist and no crates
- **THEN** the Playlist crate holds the same 300 entries in the same order, with the same current entry, and the old playlist file is still present and unmodified

### Requirement: Crates do not slow launch
Only the list of crates and the shown crate SHALL be loaded at launch. Other crates SHALL be loaded when they are first shown, played or sent to. The window SHALL be interactive within 300 ms of launch with 50 saved crates of 500 entries each.

#### Scenario: Many crates
- **WHEN** the app is launched with 50 saved crates of 500 entries each
- **THEN** the window is interactive within 300 ms

### Requirement: Crate sidebar
When the playlist is at least 600 skin pixels wide, or maximized, a list of every crate SHALL be shown on the left of the playlist, with each crate's number of records (albums, and entries of no album; see `record-view`), whether it is grouped or flat, and a tooltip naming it. The shown crate SHALL be marked •. The crate the current track comes from SHALL be marked with the player's play sign while it plays and its pause sign while paused, and not marked when playback is stopped; its tooltip SHALL say so. The user's own Discogs crates (the wantlist crate once a token is set, and the collection crate the app makes from their collection, both still recognised after a rename) SHALL be pinned at the bottom of the sidebar under a DISCOGS heading, the wantlist crate above the collection crate, both drawn in the OWNED badge's colour (off-white in the default skin) with a record icon; the other crates are listed from the top in creation order, followed by "+ New crate". Under DISCOGS, below the collection crate, a LABELS heading SHALL list the label crates (see `label-crates`) in the order they were followed, each with its name and number of records; clicking the LABELS heading SHALL fold or unfold the group, showing ⏵ or ⏷ and the number of labels, and the fold SHALL be remembered across restarts. Right-clicking a label crate SHALL offer its menu (see `label-crates`). Below LABELS, a TOP SELLERS heading SHALL list the seller crates (see `seller-crates`) in the list's order, each with its seller's name and number of records. A seller crate never dug SHALL be dimmed and show no count. Clicking the TOP SELLERS heading SHALL fold or unfold the group, showing ⏵ or ⏷ and the number of sellers, and the fold SHALL be remembered across restarts. Right-clicking the heading SHALL offer Add seller…. Double-clicking a seller crate SHALL dig it (see `seller-crates`), and right-clicking it SHALL offer Refresh seller (once dug), Narrow down… (once dug), Rename crate… and Remove seller…. Narrower, it SHALL be hidden. There SHALL be no button or setting to hide it. Clicking a crate SHALL show it. Right-clicking a crate (or Control-clicking it on macOS, which SHALL NOT show it) SHALL offer Rename crate… and Delete crate… for that crate, with the same rules as the crate menu (deleting a crate with entries asks first). For the Playlist crate, which can't be renamed or deleted, the menu SHALL offer Clear crate instead and say why. "+ New crate" SHALL create and show a new crate, after asking its name as the crate menu does. A crate that can't be read SHALL be dimmed and can't be chosen. After a crate is clicked in the sidebar, the Delete key (or Backspace) SHALL delete that crate, with the same rules as Delete crate…, until the list is clicked or its cursor moves. The crate menu on the title bar SHALL keep working. Showing a crate from the sidebar SHALL NOT interrupt playback.

#### Scenario: Switch from the sidebar
- **WHEN** the playlist is maximized and the user clicks "Keepers" in the sidebar
- **THEN** the Keepers crate is shown, the sidebar marks it •, and the playing track plays on

#### Scenario: Too narrow
- **WHEN** the playlist is 400 px wide and not maximized
- **THEN** no sidebar is shown, and the crate menu still works

#### Scenario: Delete from the sidebar
- **WHEN** the user right-clicks "Friday" in the sidebar and chooses Delete crate…
- **THEN** Friday is deleted (after confirming, when it has entries), whichever crate is shown

#### Scenario: Control-click on a Mac
- **WHEN** the user Control-clicks "Friday" in the sidebar on macOS
- **THEN** Friday's menu opens with Rename crate… and Delete crate…, and the shown crate doesn't change

#### Scenario: The Discogs crates stand apart
- **WHEN** the crates are Playlist, Friday, "Wantlist: digger" (the user's wantlist) and "Collection: digger" (the user's collection)
- **THEN** the sidebar lists Playlist, Friday and + New crate from the top, and under DISCOGS at the bottom "Wantlist: digger" then "Collection: digger", both in the OWNED badge's colour with a record icon

#### Scenario: Wantlist before connecting
- **WHEN** no token is set and the crates are Playlist and Wantlist
- **THEN** Wantlist is listed among the user's crates from the top, not under DISCOGS

#### Scenario: Playing mark follows playback
- **WHEN** a track from Keepers plays, is paused, and is stopped
- **THEN** Keepers shows the play sign, then the pause sign, then no mark

#### Scenario: Playlist clears
- **WHEN** the user right-clicks Playlist in the sidebar
- **THEN** the menu offers Clear crate and says Playlist can't be renamed or deleted; Clear crate empties it

#### Scenario: Delete key
- **WHEN** the user clicks "Friday" in the sidebar and presses Delete (or Backspace)
- **THEN** Friday is deleted, after confirming when it has entries; after a click or a cursor move in the list, Delete removes the selected entries instead, and the Playlist crate is never deleted this way

#### Scenario: Always there when it fits
- **WHEN** the playlist is widened from 400 to 700 px
- **THEN** the sidebar appears, with no button to hide it

#### Scenario: An old setting
- **WHEN** the app starts with a `settings.ron` that has `crate_sidebar: false` and the playlist maximized
- **THEN** the settings load, and the sidebar is shown

#### Scenario: Top Sellers
- **WHEN** the list holds decks.de (dug, 420 records) and logon (never dug)
- **THEN** under DISCOGS, below the collection crate, "⏷ TOP SELLERS (2)" lists "Seller: decks.de" with 420 and "Seller: logon" dimmed with no count

#### Scenario: Folded
- **WHEN** the user clicks the TOP SELLERS heading and restarts the app
- **THEN** the heading reads "⏵ TOP SELLERS (2)" and its crates stay hidden

#### Scenario: Seller menu
- **WHEN** the user right-clicks "Seller: decks.de", which has been dug
- **THEN** the menu offers Refresh seller, Narrow down…, Rename crate… and Remove seller…

#### Scenario: Labels between the collection and the sellers
- **WHEN** the user follows Siesta Records and has the seller decks.de
- **THEN** under DISCOGS the sidebar lists the wantlist and collection crates, then LABELS with "Label: Siesta Records", then TOP SELLERS with "Seller: decks.de"

#### Scenario: Records, not tracks
- **WHEN** the collection crate holds 6,322 tracks from 924 records and is shown flat
- **THEN** the sidebar shows 924 for it, and its tooltip reads "Collection: 301 (924 records, 6322 entries)"

### Requirement: Drop entries on a crate
Dragging entries onto a crate in the sidebar SHALL send them to that crate exactly as Send to crate does: the whole selection when the dragged entry is part of it, skipping entries the crate already holds, with the same message. The crate under the pointer SHALL be highlighted while dragging. Dropping on the shown crate SHALL do nothing. Dropping inside the list SHALL still reorder.

#### Scenario: Drop a selection
- **WHEN** entries 3 to 5 are selected and dragged onto "Friday", which already holds entry 4's clip
- **THEN** entries 3 and 5 are added to Friday, and the message says 2 were sent (1 already there)

#### Scenario: Drop in the list
- **WHEN** an entry is dragged and dropped between two rows of the list
- **THEN** it is reordered there, and no crate receives it

### Requirement: Keepers becomes the wantlist crate
On the first launch after this change, the Keepers crate (the one recorded in the Discogs settings, else the crate named "Keepers") SHALL be renamed "Wantlist" and become the wantlist crate, keeping its entries. When a crate named "Wantlist" already exists, the Keepers crate SHALL keep its name and still become the wantlist crate. A crate marked as the user's collection in an older index SHALL still be recognised as the collection crate.

#### Scenario: Upgrade
- **WHEN** the app starts after this change with a Keepers crate of 12 entries and no crate named "Wantlist"
- **THEN** the crate is named "Wantlist", still holds its 12 entries, and Add to wantlist adds to it

#### Scenario: Name taken
- **WHEN** the app starts after this change with both a Keepers crate and a crate named "Wantlist"
- **THEN** Keepers keeps its name and becomes the wantlist crate, and "Wantlist" is left as it is

