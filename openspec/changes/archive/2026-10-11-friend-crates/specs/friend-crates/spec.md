## ADDED Requirements

### Requirement: Friends list from Discogs
With a Discogs token set, the app SHALL read the user's friends from Discogs (`/users/‹username›/friends`, every page) and keep a crate per friend named "Friend: ‹username›". It SHALL read the list the first time a token is set (once the window is interactive), and then at most once every 7 days, at a moment with no send waiting. The list, when it was last read, which friends' collections are private, and whether FRIENDS is folded SHALL be saved in the config folder and survive restarts. There SHALL be no way to add or remove a friend in the app. A friend's crate SHALL still be recognised as that friend's after it is renamed. Without a token nothing SHALL be read and FRIENDS SHALL NOT be shown.

#### Scenario: First read
- **WHEN** a token for a user with 12 friends is saved
- **THEN** FRIENDS lists 12 crates named "Friend: ‹username›", none of them dug, after one request

#### Scenario: Within a week
- **WHEN** the app is launched and the list was read 3 days ago
- **THEN** no friends request is sent, and FRIENDS shows the saved list

#### Scenario: A week later
- **WHEN** the app is launched and the list was read 8 days ago, and the user has made a new friend since
- **THEN** the list is read again and the new friend's crate appears

#### Scenario: Renamed
- **WHEN** the user renames "Friend: javier.camunas" to "Javier"
- **THEN** it stays under FRIENDS, and double-clicking it digs javier.camunas's collection

### Requirement: The list fails soft
If reading the friends list fails (offline, an error status, or an answer without a friends list), the last saved list SHALL stay as it is and FRIENDS SHALL keep showing it. Hovering the FRIENDS heading SHALL say when the list was last read and why the last read failed. The next attempt SHALL be at the next launch, or after a failed attempt at least a day later.

#### Scenario: Endpoint gone
- **WHEN** Discogs answers 404 to the friends request and FRIENDS holds 12 friends
- **THEN** the 12 crates stay, and the heading's tooltip says the list couldn't be refreshed and when it was last read

### Requirement: A friend's crate holds their collection
Double-clicking a friend's crate that was never dug SHALL send that friend's collection page (`/user/‹username›/collection`) into the crate, like any send: listed newest first, details fetched nearest the playhead first, behind other work, and cached. The crate SHALL be grouped by record when first shown. Records the user owns SHALL show the OWNED badge. A single click SHALL show the crate from what is saved, without any request; a never-dug crate SHALL show "Double-click to dig ‹username›'s collection". Double-clicking a dug crate SHALL only show it. Without a token, a double-click SHALL open the Connect to Discogs dialog instead.

#### Scenario: First dig
- **WHEN** the user double-clicks "Friend: javimaxilo", never dug, whose collection holds 117 records
- **THEN** the crate fills with those 117 records, grouped, and records the user owns show OWNED

#### Scenario: Click
- **WHEN** the user clicks "Friend: javimaxilo", dug last week
- **THEN** its saved records are shown, and no request is sent to Discogs

#### Scenario: Never dug
- **WHEN** the user clicks "Friend: JAMESSUBURBAN", never dug
- **THEN** the empty crate says "Double-click to dig JAMESSUBURBAN's collection"

### Requirement: Private collections
When Discogs answers 403 to a friend's collection, the friend SHALL be remembered as private: their crate SHALL be dimmed, show no count, and its tooltip SHALL say "Private collection". Double-clicking it SHALL say "‹username›'s collection is private" and send nothing. Each read of the friends list SHALL check every friend's collection with one small request: a 403 SHALL mark the friend private, and any other answer SHALL clear the mark. Being offline SHALL fail the read (the last list stays).

#### Scenario: Private
- **WHEN** the user double-clicks "Friend: Waxport" and Discogs answers 403
- **THEN** the crate is dimmed with the tooltip "Private collection", stays empty, and later double-clicks send nothing

#### Scenario: Known before digging
- **WHEN** the friends list is read and Waxport's collection answers 403
- **THEN** "Friend: Waxport" is dimmed with the tooltip "Private collection" before anyone double-clicks it

#### Scenario: Made public
- **WHEN** Waxport is marked private and, at the next weekly read, their collection answers 200
- **THEN** the crate is no longer dimmed and can be dug

### Requirement: Refresh friend
Right-clicking a dug friend's crate, in the sidebar or the title-bar crate menu, SHALL offer Refresh friend, alongside Rename crate… and Delete crate…. It SHALL read the friend's collection again: records added since come in, and records no longer in it leave the crate. While it runs, the refresh window (see `discogs-collection` "Refresh progress and Stop") SHALL show its progress with Stop, and the item SHALL read "Refreshing…" and be disabled. The main window SHALL then summarise ("javimaxilo: 3 new, 1 gone", or "javimaxilo: up to date"). A failed refresh SHALL change nothing and say why. Refresh friend SHALL only read from Discogs.

#### Scenario: New and gone
- **WHEN** since the last dig javimaxilo added 3 records and sold 1, and the user chooses Refresh friend
- **THEN** the 3 come in, the sold one leaves, and the main window says "javimaxilo: 3 new, 1 gone"

#### Scenario: Stop
- **WHEN** a refresh is adding 40 records and the user clicks Stop
- **THEN** the records that arrived stay, the rest are not fetched, and the main window says the refresh stopped

### Requirement: Friends who leave
When a read of the friends list no longer includes someone, their crate SHALL leave FRIENDS and become an ordinary crate, with its name and records unchanged. If they appear in the list again later, a new friend's crate SHALL be made for them.

#### Scenario: Unfriended
- **WHEN** the weekly read no longer lists javimaxilo, whose crate holds 117 records
- **THEN** "Friend: javimaxilo" is listed with the other crates at the top of the sidebar, with its 117 records, and is no longer under FRIENDS

### Requirement: Friends never touch playback
Reading the friends list, digging and refreshing a friend SHALL NOT interrupt playback or cause underruns, and SHALL wait behind sends already running.

#### Scenario: Digging while playing
- **WHEN** a track plays and the user double-clicks a friend's crate
- **THEN** playback continues without a gap while the crate fills
