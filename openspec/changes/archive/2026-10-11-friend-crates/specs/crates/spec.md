## MODIFIED Requirements

### Requirement: Crate sidebar
When the playlist is at least 600 skin pixels wide, or maximized, a list of every crate SHALL be shown on the left of the playlist, with each crate's number of records (albums, and entries of no album; see `record-view`), whether it is grouped or flat, and a tooltip naming it. The shown crate SHALL be marked •. The crate the current track comes from SHALL be marked with the player's play sign while it plays and its pause sign while paused, and not marked when playback is stopped; its tooltip SHALL say so. The user's own Discogs crates (the wantlist crate once a token is set, and the collection crate the app makes from their collection, both still recognised after a rename) SHALL be pinned at the bottom of the sidebar under a DISCOGS heading, the wantlist crate above the collection crate, both drawn in the OWNED badge's colour (off-white in the default skin) with a record icon; the other crates are listed from the top in creation order, followed by "+ New crate". Under DISCOGS, below the collection crate, a LABELS heading SHALL list the label crates (see `label-crates`) in the order they were followed, each with its name and number of records; clicking the LABELS heading SHALL fold or unfold the group, showing ⏵ or ⏷ and the number of labels, and the fold SHALL be remembered across restarts. Right-clicking a label crate SHALL offer its menu (see `label-crates`). Below LABELS, a TOP SELLERS heading SHALL list the seller crates (see `seller-crates`) in the list's order, each with its seller's name and number of records. A seller crate never dug SHALL be dimmed and show no count. Clicking the TOP SELLERS heading SHALL fold or unfold the group, showing ⏵ or ⏷ and the number of sellers, and the fold SHALL be remembered across restarts. Right-clicking the heading SHALL offer Add seller…. Below TOP SELLERS, a FRIENDS heading SHALL list the friend crates (see `friend-crates`) in the friends list's order, each with its name and number of records; a private friend's crate SHALL be dimmed with no count. Clicking the FRIENDS heading SHALL fold or unfold the group, showing ⏵ or ⏷ and the number of friends, and the fold SHALL be remembered across restarts. FRIENDS SHALL NOT be shown without a token or with no friends, and its heading SHALL offer no menu. Double-clicking a seller crate SHALL dig it (see `seller-crates`), and right-clicking it SHALL offer Refresh seller (once dug), Narrow down… (once dug), Rename crate… and Remove seller…. Narrower, it SHALL be hidden. There SHALL be no button or setting to hide it. Clicking a crate SHALL show it. Right-clicking a crate (or Control-clicking it on macOS, which SHALL NOT show it) SHALL offer Rename crate… and Delete crate… for that crate, with the same rules as the crate menu (deleting a crate with entries asks first). For the Playlist crate, which can't be renamed or deleted, the menu SHALL offer Clear crate instead and say why. "+ New crate" SHALL create and show a new crate, after asking its name as the crate menu does. A crate that can't be read SHALL be dimmed and can't be chosen. After a crate is clicked in the sidebar, the Delete key (or Backspace) SHALL delete that crate, with the same rules as Delete crate…, until the list is clicked or its cursor moves. The crate menu on the title bar SHALL keep working. Showing a crate from the sidebar SHALL NOT interrupt playback.

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

#### Scenario: Friends under the sellers
- **WHEN** the user has the seller decks.de and the friends javimaxilo and Waxport (private)
- **THEN** under DISCOGS the sidebar lists TOP SELLERS with "Seller: decks.de", then FRIENDS with "Friend: javimaxilo" and a dimmed "Friend: Waxport"
