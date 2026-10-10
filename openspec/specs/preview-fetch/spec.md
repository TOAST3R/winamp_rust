# preview-fetch Specification

## Purpose
Gets a playable preview of each clip just before it is needed, using a yt-dlp that the user installs, and prepares it (sections, drops, waveform) so every preview can be navigated from its first second.
## Requirements
### Requirement: yt-dlp
The app SHALL use a yt-dlp program installed by the user, found on the PATH or at a path set in Options ▸ Discogs…, and SHALL show its version there. The app SHALL NOT bundle, download or update yt-dlp itself. While yt-dlp can't be found, entries SHALL wait with the status "needs yt-dlp", and the main window SHALL say once how to install it. yt-dlp SHALL be looked for again at least every 30 s while entries are waiting for it, and whenever Options ▸ Discogs… is opened.

#### Scenario: Missing yt-dlp
- **WHEN** a label is sent and yt-dlp isn't installed
- **THEN** its entries wait with "needs yt-dlp", and the main window says once how to install it

#### Scenario: Installed later
- **WHEN** the user installs yt-dlp while entries are waiting for it
- **THEN** downloads start within 30 s, without restarting the app

### Requirement: Download ahead
The app SHALL keep previews ready for the playing entry and the next 3 entries in its crate's play order. When playback is stopped, it SHALL do the same for the shown crate's current entry (its first entry when it has none) and the 3 after it. An armed entry SHALL be downloaded first. No other preview SHALL be downloaded, except those of a label crate the user asked to download in full (see `label-crates`), which come after all of these. At most 2 downloads SHALL run at once, audio only, and each entry being downloaded SHALL show its progress (for example "downloading 40%").

#### Scenario: Three ahead
- **WHEN** entry 3 of a 50-entry crate is playing
- **THEN** the previews of entries 4, 5 and 6 are ready or downloading, and no preview after entry 6 is downloaded

#### Scenario: Armed first
- **WHEN** the user double-clicks entry 30 while the previews of entries 4 to 6 are downloading
- **THEN** entry 30's preview starts downloading as soon as a download slot is free, before any other waiting preview

#### Scenario: Play from nothing
- **WHEN** a label is sent with Play while nothing is playing
- **THEN** the previews of the new crate's first 4 entries are downloaded, and the first entry plays as soon as its preview is ready

### Requirement: Safe invocation
yt-dlp SHALL only be given one of:
- **A YouTube clip id:** exactly 11 letters, digits, `-` or `_`.
- **A YouTube search** (see `preview-search`): a query built only from a tracklist's artist and title, without control characters, at most 120 characters, with the `ytsearch5:` prefix, listing results without downloading them.
- **A Bandcamp address** that passes the check in `bandcamp-intake`, rebuilt from its parts, given to read a page without downloading or to download one track.

How it is run:
- Only a search result's id that passes the same check SHALL ever be kept.
- Only a Bandcamp result whose address passes the check and whose track id is 1 to 20 digits SHALL ever be kept.
- The id, query or address SHALL be passed as its own argument after `--`, never through a shell.
- The user's yt-dlp configuration SHALL be ignored.
- Output SHALL be confined to the preview folder, named after the YouTube id or `bc.‹track id›`.

Failures:
- A clip whose id or address doesn't match SHALL be treated as unusable.
- A download that hasn't finished after 120 s SHALL be stopped and retried once.
- A clip that fails twice SHALL make its entry unavailable, with the reason "clip failed".

#### Scenario: Malformed clip id
- **WHEN** a record's clip address carries an id with other characters (for example `abc;rm -rf`)
- **THEN** yt-dlp is not run for it, and the clip is treated as unusable

#### Scenario: Dead clip
- **WHEN** a clip no longer exists
- **THEN** after one retry its entry becomes unavailable with "clip failed", and playback moves on to the next ready entry

#### Scenario: A hostile track title
- **WHEN** a tracklist's title is `--exec rm -rf ~` and its entry is searched
- **THEN** yt-dlp receives it inside one `ytsearch5:` argument after `--`, runs no command, and nothing is downloaded by the search

#### Scenario: A Bandcamp track
- **WHEN** an entry plays from Bandcamp track 3020153053 at `https://analogicalforce.bandcamp.com/track/the-ooze`
- **THEN** yt-dlp receives exactly that address after `--`, and writes `bc.3020153053.mp3` in the preview folder

### Requirement: Preview cache
Previews SHALL be stored in `previews/` in the cache folder. They SHALL use at most the preview cache size, which is 2 GB by default and set in Options ▸ Discogs…. When a new preview doesn't fit, the previews played least recently SHALL be deleted first. A preview downloaded for Download all tracks SHALL never cause a deletion: downloading pauses instead (see `label-crates`). The previews of the playing entry, the armed entry and the next 3 entries SHALL never be deleted. A deleted preview SHALL be downloaded again when it is needed, whether or not its crate was open when it was deleted, and also when its file is missing for any other reason (such as a cache folder from an older version). Such an entry SHALL go back to waiting for its download, never to failed. When the downloaded file is identical, its cached analysis and waveform SHALL be reused.

#### Scenario: Over the limit
- **WHEN** a new preview would take the cache over its limit
- **THEN** the least recently played previews are deleted until it fits, and the playing, armed and next 3 previews are kept

#### Scenario: Downloaded again
- **WHEN** a preview that was deleted is downloaded again as an identical file
- **THEN** it starts with its whole waveform and sections, without being analyzed again

#### Scenario: Missing file downloads again
- **WHEN** a collection-crate track whose preview file no longer exists is played or its details are read
- **THEN** the track waits for its preview again (not red, not failed), its preview is downloaded, and it plays

### Requirement: Ready to navigate
Right after a preview is downloaded, its sections, drops and waveform SHALL be computed in the background and cached. This SHALL start only once the playing track's own analysis covers its first 32 bars, so that it never competes with a track start.
- A preview whose preparation has finished before it starts SHALL show its whole waveform, section bands and drop markers in its first frame of playback, and section jumps, drop jumps and loops SHALL work from its first second.
- Preparing a 6-minute preview SHALL take no more than 20 s on an M-series Mac while another track plays.
- Seeking within a preview SHALL take less than 50 ms, as for a local file.

#### Scenario: Next track opens ready
- **WHEN** the next entry's preview was downloaded and prepared while the current track played, and the user presses B
- **THEN** the new track's whole waveform, section bands and drop markers are drawn in its first frame, and Shift+] jumps to its first drop

#### Scenario: Out of order
- **WHEN** the user plays an entry whose preview has just been downloaded and isn't prepared yet
- **THEN** it plays at once, and its waveform and sections fill in as they do for a local file played for the first time

### Requirement: Previews are for listening
Previews SHALL only be played. The app SHALL NOT offer to save, export or copy preview audio. Exporting a crate SHALL write each preview entry's clip address, never a file from the preview folder.

#### Scenario: Export a dig crate
- **WHEN** a crate of downloaded previews is exported as M3U8
- **THEN** it lists the clips' addresses and no file from the preview folder

### Requirement: YouTube limiting
When yt-dlp's error for a download or a search says that YouTube is limiting requests (HTTP 429, "Too Many Requests", or a request to sign in to confirm the user isn't a bot), the app SHALL treat it as limited, not as a failure of that track: the track SHALL NOT be marked "clip failed" or "not found", and its tries SHALL NOT be counted. While limited, no preview download or search SHALL start. The app SHALL try again after 10 minutes, then after 20, 40 and at most 60 minutes while YouTube keeps limiting, and the wait SHALL start again at 10 minutes once a download or search succeeds. The main window SHALL say once per limited period "YouTube is limiting requests: trying again in ‹N› min". Entries waiting for their preview meanwhile SHALL keep waiting, and their tooltip SHALL say "waiting for YouTube". Being limited SHALL never touch playback of previews already downloaded.

#### Scenario: Too many requests
- **WHEN** yt-dlp fails a download with "HTTP Error 429: Too Many Requests"
- **THEN** the track stays waiting (not "clip failed"), no other download or search starts, and the main window says YouTube is limiting requests and in how many minutes it tries again

#### Scenario: Bot check on a search
- **WHEN** a search fails with "Sign in to confirm you're not a bot"
- **THEN** the track stays waiting to be searched, it isn't remembered as not found, and searching pauses like downloading

#### Scenario: Longer waits
- **WHEN** YouTube is still limiting when the app tries again after 10 minutes
- **THEN** it waits 20 minutes, then 40, then 60 minutes between tries

#### Scenario: Back to normal
- **WHEN** a try after the wait succeeds
- **THEN** downloads and searches carry on, and the next limited period waits 10 minutes again

#### Scenario: A broken video is still a failure
- **WHEN** yt-dlp fails a download with "Video unavailable"
- **THEN** it counts as a failed try as before, and after two the track is "clip failed"

### Requirement: Bandcamp limiting
When yt-dlp's error for a Bandcamp read or download says that Bandcamp is limiting requests (HTTP 429 or "Too Many Requests"), it SHALL be handled as YouTube limiting is: not a failure, with the same 10 → 20 → 40 → 60 minute waits. The pause SHALL be kept apart from YouTube's: while one source is limited, the other SHALL go on. The main window, the tooltips ("waiting for Bandcamp") and Download all tracks' window SHALL name the source that is limited.

#### Scenario: Only Bandcamp limited
- **WHEN** Bandcamp answers 429 while a crate mixes YT and BC entries
- **THEN** BC entries keep waiting with "waiting for Bandcamp", YT entries keep downloading, and the main window says Bandcamp is limiting requests

