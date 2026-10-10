## ADDED Requirements

### Requirement: Every owned record in the collection crate
In the collection crate and the wantlist crate, every record SHALL get its own entries when it is sent or refreshed, even when some or all of its videos are already used by entries of another record in that crate. A video SHALL be skipped only when the same record already holds it. In other crates (seller, label, artist and other pages), a video the crate already holds SHALL still be skipped. After Refresh collection, the collection crate SHALL hold one record for every distinct release in the synced collection; several copies of one release count as one record.

#### Scenario: Shared videos
- **WHEN** the collection holds release 26559413 and release 26739893, which list the same 6 videos, and the collection crate is refreshed
- **THEN** both records are in the crate, each with its own entries

#### Scenario: Counts match
- **WHEN** the Discogs collection holds 940 copies of 928 distinct releases and Refresh collection finishes
- **THEN** the collection crate holds 928 records

#### Scenario: Seller page still de-duplicated
- **WHEN** a seller page lists two pressings that share a video and is sent into a seller crate
- **THEN** that video appears in the crate once

### Requirement: Refresh progress and Stop
While Refresh collection runs, a "Refresh collection" window SHALL show its progress with a progress bar: while the collection is read, "Reading your collection… page ‹k› of ‹n›" (or that it waits for other sends to finish); then, while the records the crate lacks are fetched, "Adding records… ‹k› of ‹n›". A Stop button SHALL end the refresh at any moment: a read in progress SHALL stop after the current request and keep the previous cached collection; a fill in progress SHALL stop fetching, keep the records already added and remove the ones still listed without details. The main window SHALL say "Refresh collection stopped". The window SHALL close by itself when the refresh ends. Stopping SHALL NOT affect playback.

#### Scenario: Reading
- **WHEN** the user clicks Refresh collection and the collection is read again in 10 pages
- **THEN** the window shows "Reading your collection… page 3 of 10" after the third page, with the bar at 30 %

#### Scenario: Stop while reading
- **WHEN** the user clicks Stop on page 3 of 10
- **THEN** no more collection pages are requested, the previous collection stays in use, and the main window says "Refresh collection stopped"

#### Scenario: Stop while adding
- **WHEN** 40 records are being added and the user clicks Stop after 12
- **THEN** no more records are fetched, the 12 stay in the crate, the other 28 leave it, and the window closes
