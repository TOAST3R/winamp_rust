# Diggr

Diggr is a music player in Rust with the look of a classic 2000s desktop player, built for
**speed and zero perceived latency**, with a fullscreen fractal visualizer that follows the
rhythm and structure of the music.

Progress:

 **audio-core** ✅ → **classic-ui** ✅ → **music-analysis** ✅ → **visual-engine** ✅ → web-target

Implemented so far: the audio engine (`audio-core`), the classic skinned player window
(`classic-ui`), music analysis ahead of the playhead (`music-analysis`), and the fullscreen
visual engine (`visual-engine`), which holds a steady 60 fps on an M2 MacBook. All four are
archived in `openspec/changes/archive/`. `web-target` is planned and not started yet.

Working on the code (or pointing an AI agent at it)? Start with [`AGENTS.md`](AGENTS.md).

## Launch the app

Starting from nothing on macOS, run these from the project folder:

```sh
xcode-select --install                                           # 1. once: the C linker (skip if already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # 2. once: install Rust
source "$HOME/.cargo/env"                                        # 3. put cargo on PATH in this terminal
cargo run --release -p diggr                             # 4. build (a few minutes the first time) and launch
```

The player window opens. Drag music files or folders onto it, or start with the bundled
test tones:

```sh
cargo run --release -p diggr -- crates/audio/tests/fixtures/tone.*
cargo run --release -p diggr -- ~/Music/album/*.flac    # your own files
```

After the first build you can also start the program directly, without `cargo`:

```sh
./target/release/diggr
```

Press `F` for fullscreen visuals, `Esc` to leave, and Cmd+Q to quit. Your playlist and settings
are kept for next time.

If you get `zsh: command not found: cargo`, repeat step 3. To make it permanent, run
`echo 'source "$HOME/.cargo/env"' >> ~/.zshrc`. Other platforms and details are under
[Setup](#setup), and everything the window can do is under [Try it](#try-it).

## Setup

### 1. Prerequisites

| Platform | Needs |
|---|---|
| macOS | Xcode command-line tools (the linker): `xcode-select --install` |
| Linux | a C toolchain plus ALSA headers: `sudo apt install build-essential pkg-config libasound2-dev`; runs under X11 or Wayland |
| Windows | Visual Studio Build Tools (C++); rustup offers to install them |

### 2. Rust

The project uses stable Rust, edition 2024. It needs **Rust 1.95 or newer** (egui 0.36 requires
it) and is built and tested with 1.98. On an older toolchain, `rustup update stable`.

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"                     # puts cargo on PATH in this shell
rustup component add clippy rustfmt           # for the lint/format commands below
rustup target add wasm32-unknown-unknown      # only for the web-portability check
```

> **`zsh: command not found: cargo`?** Cargo is installed in `~/.cargo/bin`, but that folder
> isn't on your `PATH`. Fix it for the current shell with `source "$HOME/.cargo/env"`, or for
> every new shell with:
>
> ```sh
> echo 'source "$HOME/.cargo/env"' >> ~/.zshrc    # bash: ~/.bashrc
> ```
>
> Check with `cargo --version`. Conda environments (`(base)`) don't matter here.

### 3. Build

```sh
cargo build --release
```

The first build downloads and compiles all dependencies, which takes a few minutes; later
builds are incremental. Dependencies are compiled with optimizations even in debug builds
(see `Cargo.toml`), because unoptimized decoders can't keep up with real-time playback.

## Try it

### The player

```sh
cargo run --release -p diggr                          # opens with the crate you last had open
cargo run --release -p diggr -- ~/Music/album/*.flac  # replaces the Playlist crate and plays
```

No music handy? The repo includes short test tones:
`cargo run --release -p diggr -- crates/audio/tests/fixtures/tone.*`

The window has the classic player's parts, side by side: the player column on the left (main
player, then the waveform and the equalizer when they show) and the playlist on its right, at
least as tall as the player column. It's drawn from an original pixel-art skin dressed like a
DJ's gear (warm graphite panels, an amber LCD, title bars with groove lines), at double size
by default (switch in **Options**: the gear ⚙ in the playlist's footer, or right-click the main
window anywhere that isn't a control, or the mini player while the playlist is maximized;
Options also has the spectrogram, Discogs… and Browser…).

Click in the player or in the playlist (or press `Tab`) to give it the keyboard: its title bar
lights up and the other side's dims. With the player focused, `↑` / `↓` set the volume. With the
playlist focused, they move a cursor through the list (Shift extends the selection; PgUp/PgDn
and Home/End jump), and Enter plays the entry under it.

- **Add music:** drag files or folders onto the window (folders are scanned recursively; `.m3u`
  playlists are expanded), use **+** in the playlist's footer, or press Cmd+O. These add to the crate on screen.
  An empty crate says so in its list: *Paste a Discogs link · Cmd+V, or drop files*, and *Press H
  for help*.
  **Eject** (and files given on the command line) replace the Playlist crate and play it.
- **Crates:** the playlist window shows one of several named playlists, and its title bar shows
  that crate's name.
  - Below 600 pixels wide, click the title bar for the crate menu: switch crate (• marks the
    one shown, ⏵ the one playing), **New crate…**, **Rename crate…** and **Delete crate…**
    (which asks first when the crate has entries). Wider, the crate sidebar does this instead
    and the title bar opens no menu. Dragging the title bar still moves the window.
  - Switching crates never interrupts playback: next, previous, shuffle and repeat follow the
    crate the playing track came from, until you start a track in another crate.
  - **Playlist** is the scratch crate. It always exists, can be cleared but not renamed or
    deleted, and is the only crate that Eject and command-line files replace. Your playlist
    from earlier versions becomes this crate on the first launch.
  - **Send to crate** (in an entry's right-click menu) copies the selection, in order, to
    another crate or a new one, skipping entries that crate already holds.
  - **Crate sidebar:** once the playlist is at least 600 pixels wide (or maximized), every crate
    is listed on its left with its number of records (grouped or flat; its tooltip also
    counts the tracks): • marks the one shown, and the player's
    play (or pause) sign marks the one your track comes from while it plays (or is paused).
    Your Discogs collection crate is pinned at the bottom under **DISCOGS**, in amber with a
    record icon, even if you rename it, followed by the labels you follow (**LABELS**, see
    [Labels](#labels)) and **TOP SELLERS**. **Playlist**'s menu offers Clear crate (it can't be
    renamed or deleted: Eject and opened files use it). Click one to
    show it, right-click it (Control-click on a Mac) for **Rename crate…** and **Delete crate…**, or
    click a crate and press Delete, or click **+ New crate**. Drag entries onto
    a crate to send them there, exactly as Send to crate does. Narrower than that, it's hidden
    and the title bar's crate menu does the same job.
- **Playlist window:**
  - **The footer:** **+** adds (files, a folder, or an M3U playlist), and **≡** is the crate's
    menu: select all / none / invert, remove selected, clear the crate, **Sort ▸**, Group by
    record and Export M3U…; the gear ⚙ opens **Options** (size, spectrogram, Discogs…,
    Browser…). The "selected/total" time sits on the right, beside the grip;
  - **The filter bar**, under the title bar, holds everything that narrows the crate on
    screen: the search field, the BPM filter, the record filters and the CART switch. It is
    part of the frame, so it takes no row of the list. **×** at its end clears the search and
    every filter. Whatever narrows the crate, only what's shown plays (next, previous,
    shuffle, the previews downloaded ahead), the playing track finishes even if it's hidden,
    entries keep their crate numbers, and the title bar reads `NAME · 42/301`. `P` on a
    hidden playing track clears the search and every filter. Sorting and M3U export always
    take the whole crate;
  - **Search:** type in the bar's field (**Cmd+F** gets there from the keyboard) and the list
    narrows as you type to the entries whose artist, title, record, record artist, label or
    catalog number hold every word, in any order, ignoring case and accents ("ame" finds
    Âme; "lowtide 012" finds LT-012 on Lowtide Tapes). Matches are lit in the rows. Enter
    plays the first match, ↓ moves to the list, **Esc** clears the search (the filters stay).
    Starting a search clears the BPM range and the record filters (not CART), so it always
    looks through the whole crate; a filter set while searching still narrows it.
    Letters typed there never act as shortcuts. The search is forgotten when another crate is
    shown and on restart;
  - **BPM filter:** once the crate on screen has two different known tempos, the bar shows
    `BPM ◂━●━━●━▸ 124-139` after the search field. Drag a handle to keep only the tracks in
    that range. Tracks without a known BPM are hidden while a range is set; the slider's
    tooltip says how many (and shows the range when the bar is too tight for its text). A
    double-click on the slider, or the bar's **×**, shows everything again. The range is
    remembered per crate;
  - **Style, artist and label filters:** in your Discogs wantlist and collection crates you
    can keep only the records of some styles, artists or labels. Pick several and any of them
    shows (Deep House and Minimal show both). A record's artist is its credit as Discogs shows
    it ("Theo Parrish", "Various", "Theo Parrish & Marcellus Pittman" are three artists), and
    its label is its first label. Each list has a search box and each value's number of
    records, the most first. The filters add up with each other and with the BPM range (a
    track must pass them all), play follows what's shown, and each crate remembers them.
    - In the filter bar, after the BPM filter: the style chips (lit ones are on), then
      **ARTISTS** and **LABELS**, when all of it fits. Otherwise **STYLES**, **ARTISTS** and
      **LABELS** buttons, when they fit; otherwise one **FILTERS** button ("FILTERS 2" while
      two are set). A button is lit while its filter is on and counts its picks ("LABELS 2");
      a double-click on it, or on a chip, turns that filter off.
    - Every button opens the **filter panel**, one tab per filter (STYLE, ARTIST, LABEL,
      FORMAT) and, in a seller crate with a cart, the CART switch.
    - A **format** filter (Vinyl, File, CD, Cassette, Other) works the same way, in every crate
      dug from Discogs, not only those two: its **FORMATS** button follows the others.
    - Right-click a record: **Only this artist**, **Only this label** and **Only this style ▸**
      keep just that value in its filter. In other crates it offers **Search ‹label›** and
      **Search ‹catalog number›** instead;
  - entries read `(catno) Artist: Title · Album (124 BPM)`. The catalog number appears for
    entries from Discogs. The album is the Discogs release's title, or a local file's album tag;
    it's left out when it's the same as the title (a single), and a row too narrow for
    everything loses it before any of the title. The BPM appears once the track has been analysed (when a preview is prepared, when
    a track plays, or from the analysis cache), folded into 88–176 so half and double time read
    alike (87 shows as 174). Nothing is downloaded just to find a BPM;
  - double-click an entry to play it;
  - Shift/Cmd-click to select several;
  - drag to reorder;
  - Delete removes the selection;
  - drag the bottom-right grip to make the playlist wider (any width) or taller (whole rows);
    its size is remembered, and narrowed to fit a smaller screen;
  - once the playlist is at least 480 pixels wide (at 1×), entries are drawn as columns: #,
    Cat#, Artist, Title, Album, Format, Style, BPM, Side, Year, For sale and Time (Style sorts by
    a record's first style). Drag a divider in the header to
    resize a column, and right-click the header to show or hide columns (#, Title and Time
    always show); both are remembered. Click a column's name to **sort** the crate by it, and
    click again for the other way (**≡ ▸ Sort** in the footer does the same at any width). A sort reorders
    the crate itself, as in classic players: the playing track plays on, and next, saving and export
    follow the new order. Entries without a value (no BPM yet, no catalog number) go last
    either way, and catalog numbers and sides sort naturally (LT-2 before LT-10);
  - **⇔** in the playlist's title bar (or `Shift+P`) maximizes the playlist: the window fills
    the screen below the menu bar, a band runs across the top with a mini player on the left
    (play state, elapsed time and title on an LCD line; previous, play/pause, stop, next,
    volume and ⇔ to restore under it) and the waveform beside it when it's on, the EQ hides,
    and the playlist takes the full width under the band. Press
    ⇔ or `Shift+P` again to get the previous window and sizes back. It's remembered, and the
    app opens maximized next time;
  - **▤** in the title bar (or **≡ ▸ Group by record**) shows the crate one row
    per record: its cover, "Artist – Album" (the record's credit, so a compilation reads
    "Various – …") with its Discogs styles at the right, the catalog
    number, year, number of tracks and what's for sale (or, while one of its tracks plays, that
    track). The column header is hidden while grouped (☰ › Sort still sorts). **⏵** (or `Space` on it)
    opens a record to show its tracks. A click selects the whole record, a double-click (or
    `Enter`) plays it from its first playable track, its menu acts on all of it, and dragging
    it moves it whole (a track moves only within its record). The arrows step over records.
    Turning it on gathers each record's tracks together, like a sort, so what plays next is
    what you see; tracks added later join their record. Each crate remembers its choice; your
    Discogs wantlist and collection crates start grouped. Covers load for the record rows in
    view, top first, from the same cache as the tooltips; under a BPM filter a record says how
    many of its tracks match ("2 of 4 tracks");
  - scrolling over the list works anywhere on it, and slow two-finger trackpad scrolling adds
    up row by row;
  - `P` scrolls to the playing entry. When the next track starts, the list follows it if the
    previous one was on screen;
  - **+ ▸ Import M3U…** and **≡ ▸ Export M3U…** read and write M3U/M3U8 (of the crate on
    screen).
  - Entries can wait for their audio or be unavailable. Both are dimmed, with an icon where the
    duration goes: a hollow dot (listed), a clock (queued), a bar that fills as it downloads, a
    warning sign (needs yt-dlp), or a barred circle (no clip, the clip failed, not found by
    search, or already in crate). Both are
    skipped by next, previous and shuffle. Double-clicking a waiting entry arms it: the current
    track plays on, the main window says it is waiting, and the entry starts as soon as its
    audio arrives. Only files that can't be opened are drawn in red.
  - Hover an entry to see everything known about it: its full name, album, label, catalog
    number, side, year, BPM, duration, status, wanted or passed (and how a wantlist or
    collection change is going), and what's for sale (with how
    long ago that was fetched). A local file shows its path. A Discogs entry also shows its
    record's cover once the pointer has rested on it for a moment. Covers come from Discogs'
    image host (not the API, so they don't slow digging down), one at a time, and are kept in
    the cache's `covers/`, so each is fetched once. Crates dug before albums were shown get
    their albums and covers from the cache when they're shown, without asking Discogs.
  - Right-click an entry for **Play** (or **Arm**, when it's waiting), **Remove**, **Remove
    album (N tracks)**, **Select album**, **Send to crate**, and for a Discogs entry **Add to
    wantlist** (or **Remove from wantlist**), **Add to collection**, Pass, Open for-sale page,
    **Open release on Discogs** and **Copy Discogs link**. In your Discogs wantlist and
    collection crates the menu leaves out what makes no sense for a record you want or own
    (Remove, Remove album and Pass; in the collection, the wantlist and Add to collection
    items too), and the collection's has **Remove from collection…** instead. Remove, Send to crate and the
    wantlist and collection items act on the whole selection when the entry is part of it (the
    wantlist and collection items once per record: "Add 3 records to wantlist"); otherwise the
    entry you
    clicked becomes the selection. While the menu is open, the rest of the entry's album is
    tinted, wherever it is in the crate. An album is a Discogs release (another pressing is
    another album), or local files with the same artist and album tags.
- **Main window:**
  - click the time to switch between elapsed and remaining;
  - click the mini visualizer to cycle spectrum → oscilloscope → off;
  - **SHUFFLE**;
  - **REP** cycles off → all → one (all gapless);
  - **WAVE**, **EQ** and **PL** show or hide the other sections. There is no balance
    control: playback is always centred.
- **Equalizer:**
  - **ON** enables it;
  - drag the sliders, or double-click one to reset it to 0 dB;
  - **LP** is a DJ filter: drag the knob down to sweep a resonant low-pass over everything
    that plays (and over the visuals), from 20 kHz down to 60 Hz, smoothly and without clicks.
    Fully up is off (the audio passes untouched); double-click turns it off. It always starts
    off;
  - **PRESETS** loads the built-in presets, and can save or delete your own.
- **Move the window** by dragging any title bar. Your crates, settings and presets are saved
  in the config folder (see [Where files are kept](#where-files-are-kept)). Set
  `DIGGR_CONFIG_DIR=/some/dir` to use another folder, for example for testing.

| Key | Action | | Key | Action |
|---|---|---|---|---|
| `Z` | previous | | `←` / `→` | seek −5 s / +5 s |
| `X` | play | | `↑` / `↓` | volume (playlist focused: move in the list) |
| `C` | pause / resume | | `F` | fullscreen visuals (`F`/`Esc` to leave) |
| `V` | stop | | `Delete` / `Backspace` | remove selected entries |
| `B` | next | | `Enter` | play the entry under the cursor (or the first selected) |
| `[` / `]` | previous / next section | | `Cmd+O` / `Cmd+A` | add files / select all |
| `Shift+]` | jump to the next drop | | `W` | show/hide the waveform |
| `L` | loop the current section | | `Shift+L` | loop 4 bars (press again: 8, 16) |
| `H` or `F1` | all shortcuts (help panel) | | `S` | spectrogram window |
| `Y` | add the playing track's record to the wantlist (again: remove it) | | `Cmd+V` | paste a Discogs page into the crate on screen |
| `N` | pass the playing track | | `I` | open the playing release's for-sale page |
| `Tab` | switch the keyboard between player and playlist | | `P` | show the playing entry |
| `Shift+P` | maximize the playlist (again: restore) | | | |
| `Space` | open or close the record under the cursor (grouped) | | `Cmd+F` | search the crate on screen |

On Linux and Windows, `Cmd` is `Ctrl`.

**Fullscreen (`F`)** shows the fractal visuals (see [Visuals](#visuals)). Transport keys,
including the section jumps and loops, and `Y`, `N` and `I`, keep working in fullscreen.

### Digging Discogs

Paste a Discogs address (Cmd+V, anywhere in the player) and the page's tracks go into the crate
on screen as previews. They're the clips the page links to, fetched with yt-dlp a few tracks
ahead of what plays. Other pasted text is ignored.

- **Pages:** a label, an artist (their own and remix credits, oldest first), a release, a
  master, a user's wantlist or collection (`/user/‹name›/collection`), a list, or a marketplace
  item (`/shop/item/…` or `/sell/item/…`),
  which is dug as the release it sells: one lookup, remembered for good. Addresses with or
  without a language prefix, the name part, a query or a fragment all work. Any other Discogs
  page shows which ones do.
- **Every format, vinyl first:** a page brings all its records, on vinyl (multi-disc ones
  such as `3x12"` too), as files, on CD or cassette. When a record exists on vinyl and in
  another format (the same master release, or the same catalog number and title), the vinyl
  release brings the tunes: its entries carry its catalog number, cover, wantlist and
  collection actions and what's for sale, and the digital twin adds nothing. The vinyl release
  is fetched first; if the twin was fetched first, its tracks become the vinyl release's in
  place (a playing one keeps playing), and the twin's other tracks leave. A record with no
  vinyl release stays, with a dim **FILE**, **CD**, **CASS** or **OTHER** mark before its
  title. Your wantlist and collection crates keep whatever releases they hold.
- **Every track of a record:** a record comes in as one entry per track of its tracklist,
  "Artist - Title" with its side and length: the track's video when Discogs has one, else "to
  search". So *Mezzanine* (eleven tracks, one video of "Teardrop") brings eleven entries, not
  one. A video Discogs lists twice counts once, and videos that match no track (a whole side, a
  mix) come after the tracks. For a remix credit, only the remix tracks come in, searched for
  when Discogs has no video of them. When one gets near the playhead (the armed entry, the
  playing one and the next 3), your yt-dlp lists a few YouTube results for "artist title"
  (nothing is downloaded for that), one search at a time. A result is used only when its title
  holds the track's title, the artist is in its title or channel, and its length is within
  10 s (or 5 %) of the tracklist's; otherwise the track says "not found by search". A found
  video then downloads and plays like any clip, and the tooltip names it ("Preview: found by
  search (…)"); a video the crate has already (one track listed twice) says "already in
  crate". Results are remembered by the track in the cache's `searches.ron` (a track not found
  is tried again after a week), so a track is searched once, whichever release, page or crate
  it comes from. A full-album upload (its title says "full album", or it is the record's only
  video and matches no track) is held back while the tracks are searched, and comes in after
  them if one is not found. A grouped record's tooltip counts what it holds ("11 tracks · 1
  clip · 10 to search"). Sending a record again adds nothing twice. A record with neither
  clips nor a tracklist is still one "no clip" entry.
- **Entries appear at once:** each listed record waits, dimmed, until its details arrive
  (records near the selected or playing entry are fetched first). It then becomes one entry per
  track (its clip, or to search) and its other clips, or "no clip". A send that's still going when you quit resumes
  when you next show or play its crate. The main window shows the progress
  ("12 of 250 releases").
- **Previews:** the playing entry and the next 3 download two at a time, with progress where
  the duration goes. While stopped, it's the current entry of the crate on screen and the next
  3. Each downloaded preview is analyzed and its waveform built before it plays, so section
  jumps and loops work from its first second. A label's **Download all tracks** fetches the
  rest behind these (see [Labels](#labels)).
- **Verdicts on the playing track:** `Y` adds its record to your wantlist: all its tracks go
  to the **Wantlist** crate, they're marked ★ wherever they appear, and (with a token) the
  release goes on your Discogs wantlist. Pressing `Y` again takes the record off, here and on
  Discogs, whoever put it there. Without a token the record is wanted here, and a dialog says
  once how connecting your Discogs account keeps your wantlist and collection up to date from
  the player (**Don't show this again** turns it into a one-line message). `N` passes it: it's
  dimmed, the next track starts, and later sends leave it out. Each verdict flashes on the main
  window's title line for a moment (`WANTED`, `UNWANTED`, `PASS`, and `OWNED` once Discogs takes
  a collection add; `WANTED 3` for several records). Nothing flashes when nothing changed.
  `I` opens the release's for-sale page in your browser. A Discogs entry's title line shows its
  catalog number and BPM, then its side, year and what's for sale (`(LT-012) Nightcraft:
  Glasshouse (124 BPM) (6:12) · A1 · 1994 · 6 for sale from €9.00`). The entry menu (right-click) has the same, and **Add to collection**.
- **Bought it:** **Add to collection** adds one copy of the release to your Discogs collection
  (in Uncategorized, so it's under All), marks it OWNED at once, takes it off your wantlist
  and moves it from the Wantlist crate to your collection's crate. It needs a token.
- **Your wantlist on Discogs:** once you're connected, the Wantlist crate is "Wantlist: ‹you›",
  in amber under DISCOGS in the sidebar, above your collection. It follows your Discogs
  wantlist: records wanted before you connected are added to it, records you add on
  discogs.com come in, and ones you remove there leave. Records leave it only with **Remove
  from wantlist** (`Y`), **Add to collection** or a sync: it has no Remove, Delete does nothing
  there, and nothing goes into it by hand (it isn't under Send to crate, and drops, added
  files and pasted pages are refused with a line saying to use Add to wantlist). The same goes
  for your collection crate. Deleting the crate doesn't touch Discogs. You never want what you own: Add to wantlist is
  off for a record you own in any pressing (the menu says "In collection"), and when a sync finds you
  bought a wanted record elsewhere, it comes off your wantlist and the main window says so.
- **When Discogs can't take a change:** a wantlist change made while Discogs is offline waits
  ("wantlist pending") and goes out once it answers. After server errors it's tried again 1,
  2, 5, 15 and 60 minutes later, then the entry shows ⚑ and the menu offers **Retry
  wantlist**. A collection add is never repeated by itself, as each try can add a copy: after a
  failure the menu offers **Retry add to collection**, which first asks Discogs whether the add
  went through.
- **Sold it:** in the collection crate, **Remove from collection…** on one record (never a
  selection of several) asks "Remove 1 copy of ‹record› (LT-012, 1994) from your Discogs
  collection?", as its notes and rating there are lost. Remove takes out one copy, the one
  added last: one request finds it (and its folder), one removes it. When it was your last
  copy, the record leaves the crate and its OWNED badge goes (unless you own another pressing
  of it); the cached collection follows at once, so the next sync stays at one request. A
  removal waits while Discogs is offline and is retried like a wantlist change, then ⚑ and
  **Retry remove from collection**; a copy already gone counts as removed.
- **Records you already own:** with a token, entries whose record is in your Discogs
  collection get an amber **OWNED** badge before the title. That covers the same release and
  also another pressing of the same master; the tooltip says which ("Owned: another pressing
  (AF014, 2018)").
  - Saving a token also sends your whole collection into a crate "Collection: ‹you›" and
    shows it (once; it isn't made again while it exists). Its entries carry no OWNED badge.
    It can be sent again from your collection page with the browser extension. It's a
    normal dig: one request per 100 records, then one per record, nearest the playhead first.
  - The collection is synced only when a crate from Discogs is on screen and the cached copy
    is missing or a week old, or from **Refresh collection** in Options ▸ Discogs…, which also
    shows how many records it holds and how old it is.
  - **Refresh collection** and **Refresh wantlist**, on a right-click of those crates in the
    sidebar (or in the title-bar crate menu when the playlist is narrow), make the crate match
    Discogs: records added there come in, records gone from there leave, and the main window
    says what changed ("Collection: 3 new, 1 gone"). They only read from Discogs, never change
    it. A collection refresh shows a window with a progress bar ("Reading your collection…
    page 3 of 10", then "Adding records… 2 of 4") and **Stop**, which keeps the previous
    collection, or the records that already came in.
  - The first sync reads 100 records a request. After that only what was added since is
    read, newest first, which is usually one request; everything is read again only when
    records were removed. Other pressings are never looked up, and without a token nothing
    is fetched or marked (the main window says once that a token would do it).
- **Setup (Options ▸ Discogs…):**
  - Pages work without an account, at Discogs' lower rate limit (25 requests a minute
    instead of 60).
  - For the wantlist and the full rate, paste a personal access token (discogs.com ▸
    Settings ▸ Developers, <https://www.discogs.com/settings/developers>, **Generate new
    token**; the dialog's **Open that page** goes there). It's checked before it's saved, is
    readable only by you, and is shown only by its last 4 characters.
  - Previews need [yt-dlp](https://github.com/yt-dlp/yt-dlp): `brew install yt-dlp` (or your
    package manager). The dialog shows the version found, and can take a path if it isn't on
    the `PATH`. Until it's found, entries say "needs yt-dlp". If clips keep failing, try
    `yt-dlp -U`.
  - The dialog also sets the default filter for every send (skip what you've passed) and the
    preview cache size (2 GB by default; the least recently played go first, except for
    Download all tracks, which never deletes anything: see [Labels](#labels)). A track whose
    preview was deleted, or left in an older version's cache folder, downloads again when
    it's played instead of turning red.
- Previews are for listening while you dig. They stay in the cache and are never exported.

### Labels

Under DISCOGS in the sidebar, **LABELS** holds a crate per label you follow ("Label: ‹name›"),
grouped by record. Click the heading to fold or unfold it.

- **Follow a label** from the browser: on a Discogs label page, or a link to one, the
  extension offers only **Diggr: Send label**. The label's crate is made and fills in the
  background: the crate on screen, playback and the window stay as they are, so you can send
  several labels in a row. Sending a followed label again refreshes it. Pasting a label's
  address in the player (Cmd+V) still adds its tracks to the crate on screen instead.
- **Move to Labels:** a crate filled from one label's page (every entry from it) offers Move to
  Labels in its right-click menu, and becomes that label's crate.
- **A label crate fills only from its label.** Paste, drops, Send to crate, Delete, Remove and
  Clear crate are refused, with a message that says so. **N** (pass) dims a track as anywhere
  else, and Y, I and copying entries out work as usual.
- **Right-click a label crate** for:
  - **Delete label…**: asks, stops following the label and deletes its crate. What you passed
    stays passed if you follow it again.
  - **Export to crate ▸**: copies every entry into one of your crates, or a new one named after
    the label, skipping what that crate holds. The label stays followed.
  - **Refresh label**: reads the label's page again. Only new records come in, and the main
    window says how many ("Label: Siesta Records: 4 new records", or "up to date").
  - **Download all tracks**: downloads every preview of the label to the preview cache in the
    background, behind what's playing, so the whole label plays instantly. A window shows the
    progress ("120 of 300 tracks"), what's downloading now, and **Stop**, which ends it at any
    moment. Closing the window only hides it: while it runs, the label's menu reads
    **Downloading (N of M)…** and shows it again. It never deletes other previews: when the cache
    is full it pauses, and the window asks to **Raise cache** to twice its size or stop. At the
    end the main window says "Label: Siesta Records: all 40 tracks downloaded".
  - **Retry failed tracks (N)**: downloads the "clip failed" tracks again and searches the
    "not found" ones again, forgetting that they weren't found. It runs as Download all tracks,
    whose window also offers **Retry failed (N)**. "No clip" and "already in crate" tracks
    aren't retried: nothing on YouTube can change them.
- **When YouTube limits requests** (too many, or "confirm you're not a bot", which can happen
  on a big label), nothing is marked failed. Downloads and searches pause, and try again after
  10 minutes, then 20, 40 and at most 60, back to 10 once YouTube answers. The main window says
  so once, waiting tracks say "waiting for YouTube" in their tooltip, and Download all tracks'
  window shows the pause with **Try now**. Previews already downloaded keep playing.

### Bandcamp

Tracks YouTube doesn't have, and digital-only releases Discogs doesn't list, are often on the
label's or artist's Bandcamp. Bandcamp pages go into Diggr the way Discogs pages do, read through
your yt-dlp (nothing is downloaded until a track is about to play):

- **Which pages:** a label or artist (`‹name›.bandcamp.com`, or `/music`), an album
  (`/album/…`) or a track (`/track/…`). Paste the address (Cmd+V), or use the extension on the
  page or on a link. Other Bandcamp pages are refused, and so is anything that only looks like
  Bandcamp: the address is checked and rebuilt before yt-dlp sees it.
- **Merged, not piled on:** each track of an album is matched with the crate's entries by artist
  and title (ignoring "(Original Mix)", "feat." and accents), and only among the entries of the
  same catalogue number when the album's title starts with one ("[AF070] The Ooze EP"):
  - a track the crate plays already is **skipped**;
  - a track the crate has without a preview ("not found", "clip failed", "no clip") gets the
    Bandcamp audio **in place**, keeping its Discogs record and its pass;
  - a track the crate lacks is **added**, after the entries with its catalogue number.

  The main window sums it up ("af070 the ooze ep: 1 added, 1 fixed, 1 skipped"). A label's own
  account often credits every track to the label and names the artist in the album's title
  ("Gioele Menoni - Mental Roots", "Flits - Advance [TOBAS 006]"): the artist and the catalogue
  number are taken from there, and "[Vinyl]"-style format words are dropped. A track Bandcamp
  doesn't stream (a pre-order) comes in as "no clip (not streamable)".
- **Labels:** a Bandcamp label page sent from the browser is followed under LABELS, like a
  Discogs label page. When a followed Discogs label has the same name (compared without spaces,
  punctuation or a trailing "Records", "Music", "Ltd"…, so `analogicalforce` is "Analogical
  Force"), that crate follows both and the main window says "Merged into Label: …". A close name
  ("Lowtide" and "Lowtide Tapes") asks **Merge** or **Separate**, once. Following a Discogs label
  whose Bandcamp crate exists joins them the same way. **Refresh label** reads both, and on
  Bandcamp only the albums it hasn't read before. Albums of a followed Bandcamp go to its label
  crate whatever the send's mode.
- **Discogs and Bandcamp together:** a Discogs record sent into a crate that has some of its
  tracks from Bandcamp takes those entries over (they get the release, catalogue number and side,
  keep playing from Bandcamp, and can switch to the record's video) instead of adding them twice.
- **Bandcamp when YouTube has nothing:** a Discogs track that ends "not found" or "clip failed"
  is looked for on Bandcamp, once: on the label's Bandcamp its crate follows, or on the one its
  label's name suggests ("Lowtide Tapes" → `lowtidetapes.bandcamp.com`). Only the albums whose
  address holds the record's catalogue number are read (or its title, on a followed Bandcamp), and
  only the failed tracks change: "Dig: 2 tracks found on Bandcamp". Retry failed tracks and
  Refresh label look again.
- **Records:** a label crate (or any grouped crate) groups Bandcamp tracks by album, as it does
  Discogs releases, with the album's cover from Bandcamp's image host.
- **Where the sound comes from:** each row ends with **YT** or **BC** (nothing for a local
  file), and the tooltip says "Source: Bandcamp · ‹album›". An entry with both offers **Play from
  YouTube** or **Play from Bandcamp** in its menu; the choice is kept, and the other preview stays
  in the cache. **Open on Bandcamp** opens the track's page, to buy it.
- **Not on Discogs:** a Bandcamp-only entry has Add to wantlist, Add to collection and Open
  for-sale page greyed out; Y and I say it isn't from Discogs. N passes it as usual.
- **Limits:** when Bandcamp limits requests, it waits like YouTube does (10, 20, 40, 60 min),
  separately: one source waiting never stops the other. Tooltips say "waiting for Bandcamp".

### Top Sellers

Under DISCOGS in the sidebar, **TOP SELLERS** holds a crate per seller ("Seller: ‹name›"), for
listening through a shop's stock and putting the copies you want in your Discogs cart. Click
the heading to fold or unfold it; right-click it for **Add seller…**.

- **The first list:** the first time a token is saved (or at the first launch with one), the
  sellers you bought from most in your last 100 purchases go in the list, 10 of them, ties
  going to the latest order. Shops with nothing for sale are skipped. That happens once: after
  that the list is yours.
- **Add seller…** takes a seller's page (`/seller/‹name›/profile`, `/seller/‹name›`,
  `/user/‹name›`) or a bare name, looks it up once you stop typing, and shows how many copies
  they have for sale. Pasting a seller's page in the player, or the extension's **Add seller**
  on a seller's page, does the same (or refreshes a seller already in the list).
- **A click shows, a double-click digs:** clicking a seller crate only shows what it holds,
  with no request to Discogs. A double-click on one never dug counts its copies (one request)
  and asks: **Dig** for 1,000 copies or fewer, **Narrow down** for more. Narrow down takes
  search text and "only the newest N" (Discogs applies both, so the count is instant), and
  format, price, minimum condition and ships-from (read from the listings: a progress line
  shows them coming). Dig is offered once 1 to 1,000 copies match. Discogs only lets anyone
  read the first 10,000 copies of another user's stock, and the dialog says so. The criteria
  are kept for refreshes. A double-click on a crate dug more than a day ago refreshes it;
  within a day it just shows it.
- **One row per copy:** a seller crate is grouped by record. A record's second line gives its
  copies and their price range ("3 copies €9.00–€18.00"); open it (⏵) to see each copy, cheapest
  first, with its price, conditions and country ("€9.00 · VG+ / VG · Germany"), above the
  record's tracks, which come in once (clips, searches and vinyl first, as for any dig). Copy rows
  hold no audio: play order, the arrows and the previews skip them. Double-click a copy to
  open its listing on discogs.com.
- **Refresh seller** (right-click the crate) reads the listings again with the saved criteria:
  new copies come in, copies gone stay with a **SOLD** badge (dimmed, listed last; a record
  whose copies all sold is dimmed and still plays), and prices follow. The main window says
  what changed ("decks.de: 14 new, 6 sold, 3 cheaper"). A refresh that would bring more than
  1,000 copies changes nothing and opens Narrow down; a failed one changes nothing.
  **Remove seller…** takes it out of the list and deletes its crate.
- **Your cart:** in a seller crate, each copy row has a cart button. Click **+ CART** to put
  that copy in your real Discogs cart; it then reads **IN CART**, which turns into a red
  **REMOVE** under the pointer, and a click takes the copy out again. A record with only one
  copy for sale has the button on its own row, so you don't need to open it; a record with
  several shows a cyan **CART** badge when one of them is in, and its copy rows have the
  buttons. Any entry of that release in any other crate, your wantlist and collection
  included, shows the **CART** badge (the tooltip says from whom and for how much). If the
  copy sold meanwhile it gets SOLD; if the cart can't be reached, the listing opens on
  discogs.com instead. The cart is read at launch,
  after each dig and cart change, and kept in the cache, so badges show at once and follow
  what you change on discogs.com. The app never empties your cart, and you always pay on
  discogs.com.
- **CART n · subtotal:** in a seller crate with copies in your cart, this switch in the filter
  bar (or in its filter panel when the bar is too narrow) shows and plays only those records, so you can listen to the order once
  more before paying; ☰ › Open cart on discogs.com goes to the cart. The subtotal is Discogs',
  in the seller's currency.

The cart and purchases calls aren't part of Discogs' published API (they are what discogs.com
itself uses, checked in October 2026). If Discogs changes them, the first list stays empty
(add sellers by hand), and + CART opens the listing instead.

### From the browser

A Chrome extension (in `extensions/chrome/`) adds a button to Discogs and Bandcamp pages: Play
in Diggr, Enqueue in Diggr and Send to crate. It talks to the player through a small **browser bridge**
that the player starts once its window is up.

- **Local only:** the bridge listens on `127.0.0.1`, port 47800 by default. Other computers
  can't reach it. If the port is taken, the player works without the bridge and
  Options ▸ Browser… says so; you can pick another port there (the extension's options need the
  same one).
- **Pairing (Options ▸ Browser…):** the dialog shows a 6-digit code, valid for 2 minutes and only
  while the dialog is open. Enter it in the extension's options. The extension receives a long
  random key and sends it with every request; the player keeps only its hash, in
  `dig/bridge.ron`. After 5 wrong codes, pairing is locked for a minute. **Forget browsers**
  revokes every key, and each browser then asks to be paired again.
- **What it accepts:** only a Discogs or Bandcamp page address (the pages that paste accepts),
  a mode (Play, Enqueue, or a crate of 1 to 40 characters), the skip-passed switch, and for a
  Bandcamp page its title (at most 200 characters, to name a label), in a body of at most 16 KB
  (a vinyl-only switch from an older extension is accepted and ignored).
  Anything else, including file paths and other addresses, is refused and changes nothing. A send is exactly a paste: it is answered at
  once, and the crate fills in afterwards. The extension can also read the crate names, what's
  playing, and the progress of sends.
- **No web pages:** requests from web pages (a web origin, another host name, a preflight) are
  refused, and no answer allows other origins.

**Install the extension** (Chrome, or any Chromium browser: Brave, Edge, Arc):

1. Open `chrome://extensions`, turn on **Developer mode**, click **Load unpacked** and choose
   the `extensions/chrome/` folder. It asks for site access to discogs.com, bandcamp.com and
   127.0.0.1 only. (An older copy needs reloading in `chrome://extensions` for Bandcamp, and Chrome
   asks you to accept the new site access.)
2. Start the player and open Options ▸ Browser…. The extension's options page opens on install (or
   right-click its toolbar button ▸ Options): enter the 6-digit code and click **Pair**.
3. On a seller's page (`/seller/‹name›/profile`), the button offers only **Add seller to
   Diggr**, which adds the seller to Top Sellers (or refreshes it) and asks the player to
   come to the front (macOS may only bounce its Dock icon).
   On a label's page, the button offers only **Diggr: Send label**, which follows the label
   under LABELS in the player (or refreshes it), without bringing the player to the front
   (see [Labels](#labels)).
   On a Discogs release, master, artist, wantlist, list or marketplace item page, the
   button after the title (or in the bottom-right corner) offers **Play in Diggr**, **Enqueue
   in Diggr** and **Send to crate** (the player's crates, or New crate…, which suggests a name
   from the page, such as "D'Arcangelo - TimeLss", that you can edit), and the skip-passed
   switch, which it remembers. On a release, master or marketplace item page
   of a record you own, a line under the button says "✓ In your collection" (or "✓ Another
   pressing in your collection (AF014, 2018)"). The extension asks the player, which answers
   from its cached collection: no Discogs request for a release or master, and one lookup the
   first time a marketplace item is seen. Without a token in the player, a dimmed line says to
   add one. A crate created from the browser (New crate…) comes on screen in the player, with
   the playlist opened if it was hidden. A confirmation shows for 3 s ("Sent to Diggr: Label: Lowtide
   Tapes → Playlist").
   On a Bandcamp album or track page, the button (after the album's or track's name) offers
   the same as on a release; on a Bandcamp label or artist page, only **Diggr: Send label**,
   which sends the page's title too, to name the label (see [Bandcamp](#bandcamp)).
4. On any site, right-click a Discogs or Bandcamp link for Play in Diggr or Enqueue in Diggr (a
   label link offers only Diggr: Send label); the toolbar
   button shows ✓ or ! for 3 s. Clicking the toolbar button shows whether the player is running
   and paired, what's playing, and sends in progress.

The labels use the name the player reports (`dig::APP_NAME`, Diggr), so they follow it if it ever
changes. The extension is plain JavaScript with no build step and no dependencies:
`manifest.json`, `background.js` (the only code that calls the player, with the key),
`content.js` (the button), `pages.js` (which pages are supported),
`options.*`, `popup.*` and `icons/`. `pages.js` is plain enough to check with Node:
`suggestName(title, kind, address)` is a pure function.

**Manual checklist** (the extension has no automated tests):

- [ ] Load unpacked in a fresh Chrome profile: the site access listed is discogs.com,
      bandcamp.com and 127.0.0.1 only.
- [ ] Pair with the code from Options ▸ Browser…; the dialog says a browser was paired, and the same
      code no longer works.
- [ ] Release, master, artist, wantlist, list and marketplace item pages each show the
      button, and each of Play, Enqueue, Send to crate and New crate… works; a forum thread
      shows no button.
- [ ] A label's page shows the button with only Diggr: Send label; it adds the label under
      LABELS in the player without changing the shown crate, and a second time says
      "Refreshed label ‹name›". Right-clicking a label link offers only Diggr: Send label.
- [ ] A Bandcamp album and a track page show the button by their name, with Play, Enqueue and
      Send to crate (New crate… suggests "Artist - Album"); a label page (`/` and `/music`) shows
      only Diggr: Send label, which follows it as "Label: ‹its name›"; a merch page shows none.
      Right-clicking a Bandcamp album link offers Play and Enqueue.
- [ ] A seller's page shows the button with only Add seller to Diggr; it adds the seller in the
      player, and a second time says "Refreshed seller ‹name›".
- [ ] New crate… suggests "Artist - Title" on a release (no `*`, no `(2)`), the name on an
      artist, and "Wantlist: user" on a wantlist, within 40 characters.
- [ ] Moving between pages without a reload (Discogs' own links) shows and hides the button.
- [ ] Right-click a Discogs release link on another site (a forum post): Enqueue in Diggr adds
      it and the toolbar shows ✓.
- [ ] With the player closed, an action says that it isn't running, and nothing else happens.
- [ ] After Forget browsers, an action opens the pairing screen.
- [ ] A hostile page cannot use the bridge: serve this file from `python3 -m http.server` and
      open it; every line should read "refused" or "blocked", and no crate changes:
      ```html
      <pre id=o></pre><script>
      for (const [m, p] of [["GET","hello"],["POST","send"],["POST","pair"]])
        fetch(`http://127.0.0.1:47800/v1/${p}`, {method: m, body: m == "POST" ? "{}" : undefined,
          headers: {"Content-Type": "application/json"}})
          .then(r => o.textContent += `${p}: ${r.status >= 400 ? "refused" : "ACCEPTED"} (${r.status})\n`)
          .catch(() => o.textContent += `${p}: blocked\n`);
      </script>
      ```
- [ ] From another computer on the network, `curl http://<this computer>:47800/v1/hello` fails
      to connect.

### Waveform and structure navigation

Under the main window, the **waveform** (`W`) has a title bar like the equalizer's (drag it to
move the window, × to hide the waveform) and two rows:

- **Overview** of the whole track: coloured by frequency, with section bands, red markers where
  the energy jumps (the drops), and the playhead. Click or drag to seek.
- **Zoom** around the playhead: bass is red, mids green, highs blue, so kicks and hats are easy
  to tell apart. Beat ticks are shown, taller on bar starts. The scroll wheel zooms from 1 to
  64 bars.

The waveform fills in within a few seconds of a track starting. It's saved in the cache, so a
replayed track shows its whole waveform at once.

**Jump by structure:**
- `]` goes to the next section, and `[` to the start of this one (or the previous one if you're
  in its first bar).
- `Shift+]` goes to the next drop, meaning the next section that is at least 4 dB louder than
  the one before.

Jumps land exactly on a bar line, so the beat never stumbles. A yellow line on the waveform
shows where the jump will happen. The player uses the first bar line it can still reach: the
next one, or the one after if the next is less than half a second away.

**Loops:**
- `L` loops the current section, at most 32 bars. Press it again to stop.
- `Shift+L` loops 4 bars from the current bar line; press again for 8, then 16.

Loops repeat without a gap and show in yellow on the waveform. A seek, stop or track change
ends them.

`[` and `]` work by key position, next to `P`, so they work on any keyboard layout.

### Spectrogram

`S` (or **Spectrogram (S)** in **Options**, a right-click on the main window) opens a separate, resizable window
with the current track's spectrogram: time runs left to right, frequency goes up on a log scale
from 20 Hz to the file's Nyquist frequency, and brighter means louder.

- **Track** shows the whole track with the playhead. Click to seek. Scroll to zoom time around
  the cursor, Shift+scroll to zoom frequency, drag to pan, and double-click to see the whole
  track again. Once you zoom past what the overview holds, the visible range is recomputed at
  full resolution in the background (the header shows the FFT size: short ranges use short
  windows so drum hits stay sharp, long ranges up to 8192 points for fine pitch detail).
- **Live** is a scrolling waterfall of what you hear right now, in step with the audio.
- **Mid / Side / L / R** picks the signal: mid is the sum of both channels, side their
  difference (stereo width). The whole-track view has mid and side; L and R need a zoomed or
  live view.
- **dB** sets the range the colours span (−120 to 0 dB by default), to bring out quiet detail.
- Hovering shows the time, frequency with the nearest note (e.g. `440 Hz A4`) and the level.

**Quality check:** the bottom right shows where the content ends. For a lossless file (FLAC,
WAV, ALAC) whose highs stop at a hard wall below 19.5 kHz it reads, for example, *Content ends
at 16.0 kHz: likely from a lossy source (≈128 kbps MP3)*: a sign that the file was made from an
MP3 or similar. Music that just gets quieter towards the top isn't flagged, and lossy files
(MP3, AAC, Vorbis) never are.

The whole-track view comes from the same background pass as the waveform, so it appears as the
waveform fills in and is instant on replay. That pass's cache format changed with this feature,
so tracks you played before are analyzed once more the first time you play them again.

To look at a file without playing it (for example a FLAC next to a transcoded copy):

```sh
cargo run --release -p ui --example spectrogram_probe -- song.flac
```

It prints the quality verdict and opens the window; clicks move the playhead.

### Visuals

Fullscreen visuals run on musical time: beats, bars and phrases from the analysis, with kicks,
snares and hats fired as the playhead crosses them, so motion lands on the beat you hear. At
each section change the **director** picks what to show. A big rise in energy cuts to your
highest-rated look with a flash. A drop in energy crossfades to something calm. A section that
comes back returns to the look it had before. Other changes morph to a sibling look, and the
`stretch` macro follows the track's tension. The same track always gets the same show.

Six scenes ship with it, each with at least two variants:

- **Julia Tunnel** (2D fractal);
- **Liquid Feedback** (the MilkDrop feel);
- **KIFS Cathedral** (raymarched 3D);
- **Flame** (a compute-shader fractal flame);
- **Polar Life** (cellular automata on the walls of a tunnel): spectrum onsets and kicks give birth
  at the centre, and the tunnel flies outward eight rings per beat. Cells are lit, bevelled tiles
  with halos and comet tails. Each kind of section runs its own rule: Life in grooves, Brian's
  Brain in builds, Star Wars in drops, Day & Night in breakdowns. Every kick or beat hits the tunnel with
  a burst of interference, and drops speed it up with their energy. The `hyperdrive` variant bends
  the tunnel into a swaying 3D pipe;
- **Coral Tunnel** (reaction-diffusion): onsets and kicks seed chemistry that grows into glossy
  coral or dividing cells as it streams outward.

The artist, title and progress (with section ticks) show for 5 s on entering fullscreen, on each
new track, and when you move the mouse or press a key.

| Key | Action |
|---|---|
| `D` | show/hide the fader deck: 6 macros (intensity, chaos, stretch, speed, hue, feedback) and the scene's parameters |
| `M` / `Shift+M` | mutate the current look (small / big step) |
| `K` | keep: save the current look, fader positions included, as a new variant |
| `Backspace` | undo back through the looks you had |
| `1`–`5` | rate the current look (the director prefers higher ratings) |

On the deck, dragging a fader switches it to MANUAL. When you let go, it holds, then glides back
to automation after the RETURN time (1 beat, 1 bar, 4 bars, a phrase, or ∞), landing on a bar
line. Click RETURN in the deck header to change the global setting. Right-click a fader to give it
its own RETURN, and double-click it to hand it back to automation now. Speed snaps to ¼, ½, 1, 2
and 4×.

**Make it yours:** on first use the scenes are copied to `visuals/` in the config folder
(`~/Library/Application Support/Diggr/visuals/` on macOS), and any file you save there is picked up
while the music plays:

```
visuals/
  director.ron              the rules for what happens at section changes (commented)
  prelude/*.wgsl            helpers every scene can call: complex math, noise, palettes, SDFs, tunnels
  scenes/<id>/scene.ron     name, tags, parameters (type, default, range), macro mappings, routes
  scenes/<id>/scene.wgsl    fn scene(uv: vec2f, m: Music, p: Params) -> vec4f
  variants/<id>/<name>.ron  saved looks (K writes these; ratings live here too)
```

A new look is one `.wgsl` and one `.ron` file in a new `scenes/<id>/` folder. You write only
`fn scene`. The engine generates `p.<param>` from your manifest and passes the music as `m`: for
example `m.beat` (the phase within the beat), `m.motion` (beats, scaled by the speed macro), `m.kick`
(beats since the last kick, so use `pulse(m.kick, 4.0)` for a punch), `m.energy`, `m.tension`,
and `band(m, i)` for the 19 spectrum bars. Routes in the manifest connect signals to parameters
without writing code, for example
`(source: Kick, shapers: [Envelope(attack: Ms(5), release: Ms(120)), Range(0, 0.35)], target: "zoom")`.
A scene can also be a cellular automaton: declare
`kind: Automaton(theta: 128, rings: 64, steps_per_beat: 4.0)` and write
`fn rule(c: vec2<i32>, m: Music, p: Params) -> vec4f` next to `fn scene`. `rule` returns a cell's
next state from the previous one, read with `cell(c)`: θ wraps around, and rings outside the grid
are empty. `inject_level(m, theta)` spreads the spectrum around the circle, `tick()` numbers
the steps, and `since_reset()` is 0 on the first step after a reset (a rule can seed its start
state there; `preroll: N` in the kind runs N steps right away, 16 by default). In `fn scene`, `state_at(depth, theta)` samples the grid and `tick_phase()` glides
between steps. The grid persists between frames, one per drawn layer. It steps on musical time,
so the show is the same at any frame rate, and it starts over on a seek, a new track or a reload.
A `step_rate` param, if the scene has one, is read once per beat and snapped to ×0.5, ×1, ×2 or
×4. For continuous rules such as diffusion, `substeps: N` in the kind runs `rule` N times per step
(up to 32; `substeps()` returns N). Any scene can fly through a tunnel with
`tube_hit(uv, focal, bend)` from `prelude/tunnel.wgsl`: it returns the depth, the angle and the
distance from the axis of a tube whose far end sits at `bend`.

Bundled scenes are copied only when missing, so after an update that changes a scene you've used
before, delete its folder under `visuals/scenes/` (and `visuals/variants/`) to get the new version.
If you save a broken shader or manifest, the previous version keeps running, and a message
shows the file, line and error for 8 seconds. To reset a file, delete it and it is restored from
the bundled copy the next time you enter fullscreen.

Resolution adapts to hold the frame rate. You can measure the scenes on your GPU:

```sh
cargo run -p visuals --example visual_bench --release        # offscreen, native Retina size
DIGGR_VISUAL_BENCH=1 cargo run -p diggr --release   # in the app: press F; 15 s with no visuals,
                                                             # then 15 s per scene; results in .../visuals/bench.txt
DIGGR_FRAME_STATS=1 cargo run -p diggr --release    # per-second frame timings (app, visuals, present)
cargo run -p ui --example fullscreen_probe --release         # what a blank eframe window can present
```

### Render a show to video

Any track's show can be rendered to an MP4 (H.264 video with the track's own audio). The show
is deterministic, so the file matches what you'd see in fullscreen with your hands off the
deck. Rendering needs ffmpeg (`brew install ffmpeg`).

```sh
diggr --render-show track.flac -o show.mp4                 # 1920×1080, 60 fps, the whole track
diggr --render-show track.flac -o clip.mp4 --from 1:00 --to 1:30 --size 1280x720 --fps 30
diggr --render-show track.flac -o card.mp4 --overlay       # with the artist/title card at the start
diggr --render-show track.flac -o one.mp4 --look julia_tunnel/solar   # one look, director off
```

(With cargo: `cargo run --release -p diggr -- --render-show …`.)

- **Frame timing:** frame n shows the music at exactly `start + n/fps`, so every kick lands on
  its frame.
- **Size:** the width must be divisible by 8 and the height even; vertical `1080x1920` works.
- **Progress:** the time left is shown as it renders. Ctrl-C stops it, and a cancelled or
  failed render leaves no file behind.
- **Analysis:** the track is analyzed first if it isn't in the cache yet.
- **Matching the live show:** a render matches the live show of a replay, because a first live
  play can react to sections the analyzer later corrects.

To measure rendering speed without encoding:
`cargo run -p visuals --example render_speed --release -- track.flac`.

### Music analysis

While a track plays, the app analyzes it about 2 minutes ahead of what you hear: tempo, the beat
grid, bars, and sections (intro, build, drop, breakdown, groove, outro), with a countdown to the
next drop. The first 32 bars are ready about half a second after you press play, and results are
cached in the cache folder (`~/Library/Caches/Diggr/` on macOS), so a second play is instant. Set `DIGGR_CACHE_DIR`
to use another folder. It runs at low priority and never delays playback.

In fullscreen:

| Key | Action |
|---|---|
| `T` | show/hide the analysis strip: BPM, current section, drop countdown, section bands, beats (taller on bar starts), the tension curve and the playhead |
| `A` | annotation mode, for teaching the analyzer your music |
| `Space` *(annotating)* | tap along with the beat |
| `1`–`6` *(annotating)* | mark where a section starts: 1 intro, 2 build, 3 drop, 4 breakdown, 5 groove, 6 outro |

Annotations are saved immediately to `annotations/` in the cache folder. To see how the
analyzer scores against them:

```sh
cargo run --release -p analysis --bin analysis-eval
```

This prints each track's beat accuracy and how many of your section marks it found. The targets
are ≥ 90% and ≥ 70%.

No music annotated yet? You can try it on generated tracks:

```sh
cargo run --release -p analysis --example make_annotated -- /tmp/eval
cargo run --release -p analysis --bin analysis-eval -- /tmp/eval/annotations
```

### Terminal player

The earlier terminal harness is still available:

```sh
cargo run --release -p diggr -- --tui ~/Music/*.mp3
```

Run it in a real terminal (Terminal, iTerm, or the VS Code terminal): it reads keys directly,
so it won't work with piped input or in a non-interactive shell. By default it plays at 80%
volume; add `--volume 0.3` to start quieter.

| Key | Action | | Key | Action |
|---|---|---|---|---|
| `z` | previous | | `←` / `→` | seek −5 s / +5 s |
| `x` | play | | `↑` / `↓` | volume |
| `c` | pause / resume | | `e` | EQ on/off |
| `v` | stop | | `q` / `Esc` | quit |
| `b` | next | | | |

The status line shows the audible position (from the playback clock), track, volume, EQ, device
rate, the last start/seek latency, underruns, and, in debug builds, allocations detected
inside the audio callback (should always be 0).

Supported formats: MP3, FLAC, WAV, OGG Vorbis, AAC/M4A (pure-Rust decoding via symphonia).

### Measure latency on your machine

```sh
cargo run --release -p diggr -- --bench --volume 0 file1.mp3 file2.flac …
cargo run --release -p diggr -- --bench crates/audio/tests/fixtures/tone.*   # quick check
```

For each file this measures press-play → first audio at the device and three seeks. It then
plays the whole queue gaplessly. Underruns are counted over the whole session, including the
seeks. It exits non-zero if a target is missed:
start < 30 ms, seek < 50 ms, 0 underruns, 0 callback allocations (the allocation check needs a
debug build: drop `--release`). `--volume 0` runs the full pipeline silently (it is also the
default for `--bench`). Add `--analysis` to run the music analyzer during the whole bench and
confirm it doesn't cost underruns or latency.

Last measured on an M-series Mac (CoreAudio, 44.1 kHz, 512-frame buffer), 60 s files:
start 7.9–20.8 ms, seek 12.1–20.0 ms.

### Check A/V clock accuracy acoustically

```sh
cargo run --release -p diggr -- --click-test
```

Plays 16 clicks and records them with the default microphone. It reports how far the heard
clicks are from when the playback clock said they'd be audible (target ±2 ms). This needs your
speakers audible to the mic, and macOS will ask for microphone permission.

### Where files are kept

| | macOS | Linux | Windows | Override |
|---|---|---|---|---|
| config (settings, presets, `crates/`, `dig/`, `visuals/`) | `~/Library/Application Support/Diggr/` | `~/.config/diggr/` | `%APPDATA%\Diggr\` | `DIGGR_CONFIG_DIR`* |
| cache (analysis scores, waveform `overviews/`, `annotations/`, Discogs responses in `discogs/`, `previews/`, record `covers/`, your Discogs `collection.ron`, preview `searches.ron`, your Discogs `cart.ron`) | `~/Library/Caches/Diggr/` | `~/.cache/diggr/` | `%LOCALAPPDATA%\Diggr\` | `DIGGR_CACHE_DIR` |

\* `DIGGR_CONFIG_DIR` covers settings, crates and presets; the editable `visuals/` folder
always lives in the platform config folder.

Crates are kept in `crates/`: `index.ron` lists them (with whether each is grouped by record) and each crate is `<id>.ron`. A crate file
that can't be read is reported once and left as it is; a damaged `index.ron` is rebuilt from the
crate files. The single `playlist.ron` of earlier versions is read once, on the first launch
with crates, to create the Playlist crate, and is then left untouched as a backup (the previous
version still opens it); the Playlist crate is now the one that counts.

Digging keeps its state in the config folder's `dig/`: `settings.ron` (filters, cache size,
yt-dlp path, the Wantlist crate, whether the Connect to Discogs dialog was turned off), `token`
(readable only by you), `memory.ron` (wanted records, passed tracks, and wantlist changes still
to be sent, with their retries), `jobs.ron` (sends still in progress), `bandcamp.ron` (the
Bandcamp albums each label crate has read, and the labels you kept separate), `sellers.ron` (Top
Sellers: their order, each one's dig criteria, when it was last dug, and whether the first list
was made) and
`bridge.ron` (the browser bridge's port and the SHA-256 of each paired browser's key, readable
only by you). In the
cache, `discogs/` keeps API responses: record details for good, listings for a day, and
for-sale numbers refreshed once a day when their track plays. `previews/` holds the downloaded
clips, and `covers/` the record covers shown in tooltips (150 px PNGs, about 10 KB each; safe to
delete). `collection.ron` is your Discogs collection (release ids, and each owned pressing's
master, catalog number and year), synced as described in [Digging Discogs](#digging-discogs).
`cart.ron` is your Discogs cart as last read (listing ids, releases, sellers, prices and
subtotals), for the CART badges at launch.

Other environment variables, mostly for unattended runs and measurements:

| Variable | Effect |
|---|---|
| `DIGGR_AUTO_FULLSCREEN=1` | enter fullscreen as soon as playback starts |
| `DIGGR_AUTO_QUIT_SECS=n` | close the app after `n` seconds |
| `DIGGR_VISUAL_BENCH=1` | benchmark every scene on the first fullscreen (see [Visuals](#visuals)) |
| `DIGGR_FRAME_STATS=1` | print per-second frame timings |

`diggr --help` prints all command-line modes.

## Tests

```sh
cargo test --workspace            # 842 tests, under a minute after the first build; no audio hardware or display needed
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo check -p audio -p platform --target wasm32-unknown-unknown   # core stays web-portable
```

Run a single test file or test with `cargo test -p audio --test engine` or
`cargo test -p audio gapless`.

Tests write their files under `<temp>/diggr-tests/` and delete them when they finish.
Anything a background worker writes late, or a killed run leaves, is deleted by a later run
once it's an hour old.

The test fixtures in `crates/audio/tests/fixtures/` were generated with ffmpeg (2 s, 440 Hz, tagged
`M83 / Midnight_City`). You only need ffmpeg if you want to regenerate them.

What's covered:

- **Unit tests** per module: lock-free ring, seqlock clock (interpolation, latency, gapless
  boundary, monotonicity, concurrency), EQ (±0.5 dB at band centers, bit-identical when off, no
  clicks when sweeping, Nyquist bypass), the LP filter (bit-identical when off, highs gone within
  20 ms, no jumps when sweeping or switching), tap, renderer, resampler.
- **`crates/audio/tests/decode_formats.rs`**: every format at 44.1 and 48 kHz (pitch, length,
  tags), accurate seeking, corrupt and garbage files. Fixtures are in
  `crates/audio/tests/fixtures/`.
- **`crates/audio/tests/engine.rs`**: the full engine against `platform::testing::ManualSink`, a
  sink the test drives by hand with fake time. Covers:
  - bit-exact playback, pause and seek;
  - bit-exact gapless playback, and gapless playback with resampling;
  - the clock hitting a click within ±2 ms with 12 ms of output latency;
  - device loss that comes back at a new sample rate;
  - the tap, EQ, volume and balance.
- **`crates/audio/tests/rt_alloc.rs`**: a counting global allocator proves the audio callback
  never allocates on any path.
- **`crates/ui`** (unit tests):
  - playlist selection, reordering and totals;
  - shuffle order, and a play order that skips entries waiting for their audio;
  - the crate search: every word in some field, folded case and accents, the play order
    following it, lit matches, and a 5,000-entry search within 16 ms; headless, typed letters
    never acting as shortcuts, Cmd+F, Enter, Esc, a search forgotten with its crate, the
    filter bar's tiers, its FILTERS panel and ×, and the row menu's Only this… / Search…;
  - folder scanning and M3U round trip (remote entries exported as their source URL);
  - settings, crate and preset persistence;
  - crates: name rules, lazy loading, unreadable crate files, a damaged index, migration of a
    300-entry `playlist.ron`, and Send to crate without duplicates;
  - spectrum bars following the *audible* frame;
  - EQ curve;
  - skin validation, that the committed skin matches its generator, and that a skin without
    an LCD colour keeps green (and one without a filter bar height gets the default);
  - the empty crate hint and the verdict flash wording (and, headless, which verdicts flash);
  - label crates: the label field and `is_locked`, every refused edit while pass still works,
    Move to Labels, the LABELS group and its fold, the label crate menu, Export to crate,
    Delete label, Refresh label's summary, a label sent from the browser (followed, then
    refreshed, nothing doubled), and Download all tracks (all downloaded behind what plays,
    its progress window and Stop, the cache-full question, the menu showing the window again,
    a limited YouTube pausing it with Try now, and Retry failed tracks);
  - repaint policy;
  - headless egui click/drag tests of the skinned widgets;
  - the whole player driven headlessly against `ManualSink`: switching crates leaves playback
    alone, starting a track re-targets next/previous, an armed entry is audible within 100 ms
    of its audio arriving, Eject replaces only the Playlist crate, dimmed rows, the title bar's
    click (crate menu) versus drag (move), and the crate and entry menus.
- **`crates/dig`**: the Discogs client against recorded JSON (`crates/dig/tests/fixtures/`),
  covering:
  - request headers, and a rate limit that never exceeds 60 requests in any minute;
  - backing off on 429;
  - the disk cache;
  - every supported address form;
  - listings for each page kind;
  - clip extraction and matching;
  - focus-first expansion;
  - jobs resumed after a restart;
  - offline and back;
  - token checks, wantlist changes, collection adds (one copy, and a checked retry that never
    adds a second), and wanted records found in the collection after a sync;
  - albums and covers from the cache with no request, and covers: fetched once, shrunk,
    paced to 4 a second, only from Discogs' and Bandcamp's image hosts, a stale address looked up again.

  The browser bridge (`crates/dig/tests/bridge.rs`) runs on an ephemeral loopback port:
  pairing (expiry, single use, lockout after 5 wrong codes), keys and Forget browsers, refusals
  by host, origin, preflight, size, unknown fields and other addresses (look-alike Bandcamp hosts
  too), Bandcamp albums and labels (added, merged or refreshed), no
  `Access-Control-*` header on any answer, the crates and status snapshot, a taken port, and a
  send answered in under 100 ms while Discogs is slow.

  The preview scheduler runs against a fake yt-dlp: the horizon, 2 slots, the armed entry
  first, cancelling, retries, the timeout, yt-dlp appearing later, the cache limit, and the
  background list of Download all tracks (after the horizon, paused instead of evicting,
  resumed by a larger cache, stopped by an empty list), YouTube limiting (yt-dlp's 429 and
  bot-check errors told apart from a broken video, nothing failed, waits of 10, 20, 40 and
  60 min, Try now), Retry (given-up clips and not-found searches tried again), and Bandcamp:
  a track downloaded as MP3 by its located page, a label read album by album (leaving out
  albums read before), and a Bandcamp limit that leaves YouTube going. `crates/dig` also checks
  Bandcamp addresses, reads yt-dlp's answers from fixtures, and runs a real read and download by
  hand (`--ignored real_bandcamp`). Also
  covered: the dig memory, and preparing a preview's score and overview behind the gate.
- **Digging in the player** (`crates/ui`, headless, with a fake Discogs, yt-dlp and browser):
  - a pasted release filling the crate with playable previews;
  - a record with one video for eleven tracks (*Mezzanine*): every track, in order, and nothing
    twice when sent again; one video found for two tracks; the full-album fallback, after a
    reload too; the vinyl release taking over a track found for its digital twin;
  - Play mode naming its crate and playing;
  - a missing page taking its crate away again;
  - `Y`, `N` and `I` with and without a token;
  - the entry menu;
  - Options ▸ Discogs… checking a token;
  - "needs yt-dlp";
  - no request to Discogs before the window is interactive;
  - the browser bridge: started only after the first frame, a send answered in under 100 ms
    while Discogs is slow and then filling the crate, Options ▸ Browser… pairing once and Forget
    browsers, and a taken port that leaves the player working.
- **Bandcamp in the player** (`crates/ui`, headless, `bandcamp_tests.rs`, with fake pages): a
  pasted album playing from Bandcamp, a merge that skips, fixes and adds, a label followed from
  the browser and refreshed with new albums only, merged by name into a Discogs label (and the
  other way round), Merge or Separate asked once, the YT/BC badges, switching sources and Open on
  Bandcamp, and the Discogs items off on a Bandcamp-only entry. Merging, name matching and
  switching are also unit-tested.
- **Top Sellers and the cart** (`crates/ui`, headless, `seller_tests.rs`; and `crates/dig`):
  the first list from purchases (once, closed shops skipped), the sidebar group and its fold,
  click versus double-click (no request on a click), Dig and Narrow down (search text, newest N,
  criteria read from the listings), refreshes (new, sold, cheaper, past the limit, failed),
  Add seller, a pasted or browser-sent seller page, Remove seller, copy rows and the cursor
  stepping over them, SOLD, the CART badge from a cached and a fresh cart, Add and Remove from
  cart with their outcomes, and the CART switch. `crates/dig/tests/seller_api.rs` runs the real
  inventory, purchases and cart reads by hand (`--ignored`, with `DIGGR_DISCOGS_TOKEN`).
- **`crates/ui/tests/dig_playback.rs`**: an 800-release label is expanded, and previews are
  downloaded and prepared, while the engine plays in real time. Zero underruns, and a prepared
  preview starts as fast as a local file.
- **`crates/ui/tests/waiting_skip.rs`**: track 3 is followed sample-exactly by track 5 while
  track 4 waits for its audio, and track 4 is not marked failed.
- **`crates/ui/tests/large_add.rs`**: 2,000 files get their metadata read while the engine plays
  in real time, with zero underruns.
- **`crates/analysis`**: generated electronica with exact ground truth, testing:
  - kicks within 20 ms;
  - tempo at 124/128/140/174 BPM, and tempo changes;
  - beatless intros;
  - downbeats and section boundaries on the right bar;
  - section kinds, and repeated drops sharing a label;
  - streaming, seek, cache and pre-warm.
- **`crates/ui/tests/analysis_playback.rs`**: real-time playback while two tracks are analyzed,
  with zero underruns.
- **Spectrogram** (`crates/analysis` spectral and detail modules, `crates/ui` spectrogram):
  - a tone lands on its row, and a full-scale sine reads 0 dB;
  - the whole-track overview stays between 2048 and 4096 columns for any length, and a 2-hour
    mix's complete overview within 16 MB;
  - zoomed detail resolves clicks 10 ms apart, and skips gaps by seeking in sparse views;
  - a brick wall at 16 kHz is flagged, a gradual roll-off and full-band content aren't, and
    lossy codecs never are;
  - `lossless` follows the codec for every fixture format;
  - note names, cursor readout, zoom limits, live columns lined up with the audio, and a
    headless click that seeks to the time under the pointer.
- **`crates/visuals`** (unit tests, plus GPU tests on a headless device that skip when there is
  no GPU):
  - musical time and triggers locked to the analyzed beats, including pause and seek;
  - every shaper, route determinism, and the base → manual → macros → routes → clamp order;
  - manifests, and WGSL generation checked with naga, with errors mapped to the author's line;
  - every bundled scene compiling and rendering;
  - crossfade, feedback trails, compute accumulation, hue, and broken shaders not panicking;
  - automata: steps on musical time at 30 and 144 fps, per-beat step rate with hysteresis
    (drops ×2 and ×4), catch-up, pre-roll and reset on seeks; grids that persist, wrap in θ and
    stay separate per layer; Polar Life starting from a soup, growing from kicks and emptying in
    silence; Coral Tunnel growing a kick splash with its chemistry staying in range, and its dividing cells outlasting the flow;
  - variants loading when params change, mutation, lineage and undo;
  - the director's default show (rise, fall, repeat, idle, track change, provisional boundaries);
  - fader RETURN glides landing on bar lines;
  - the overlay fade;
  - hot reload keeping the last good scene, and the file watcher;
  - the engine end to end (keys, crossfades, re-init);
  - offline show rendering: a kick at 12.500 s hitting frame 750 at 60 fps, identical frames
    across renders, BT.709 YUV conversion, the overlay card, pinned looks, an MP4 end to end
    through ffmpeg (skipped without it), and cancel leaving no file.
- **`crates/visuals/tests/render_playback.rs`**: real-time playback while a show renders, with
  zero underruns.
- **`apps/native/tests/no_old_name.rs`**: no tracked file names the player that inspired Diggr's
  look, in any letter case (skipped outside a git checkout).

Measure the player's launch time (the target is under 300 ms):

```sh
cargo run --release -p diggr -- --startup-time
```

On an M-series Mac it takes 142–171 ms, including with a 500-entry saved playlist, and
148–182 ms with a Discogs token and a send still in progress (nothing is sent to Discogs
before the window is up). The very
first launch after a build takes about 0.7 s while macOS compiles and caches GPU shaders.

## How it works

```
 decode thread ──lock-free ring (~0.5 s)──▶ audio callback ──▶ device (opened once, always running)
   symphonia → stereo → rubato             EQ → tap → volume/balance
   gapless + pre-warm next track           publishes the playback clock
                                                  │
                   visuals / analysis ◀── clock + tap (lock-free, never block audio)
```

- **Instant start:** the output device opens at launch and stays open (emitting silence), so
  pressing play only has to decode one packet.
- **Zero-offset visuals:** the callback publishes *which frame is at the speaker right now*
  (callback time + output latency). Consumers interpolate it at any frame rate.
- **Gapless:** the next track is opened and pre-decoded 30 s before the end. One continuous
  resampler carries across track boundaries, so even 44.1 kHz files on a 48 kHz device join
  seamlessly.
- **Real-time safe:** no allocations, locks, or waiting in the callback. Seeks flush by
  generation number instead of locking.
- **Resilient:** corrupt frames and unplayable files are skipped. If the output device
  disappears, the stream is rebuilt at the same position, even at a new sample rate.

Full details and the reasoning behind each decision:
[`openspec/changes/archive/2026-09-26-audio-core/design.md`](openspec/changes/archive/2026-09-26-audio-core/design.md).

## Layout

```
crates/platform   seam traits (AudioSink, Spawner, FileSource) + native impls + ManualSink for tests
crates/audio      decode, ring, renderer (callback), clock, EQ, tap, decode worker, Engine API
crates/analysis   music analysis: beat grid, tempo segments, sections, tension, cache, eval tools;
                  track overview (waveform + spectrogram, cutoff check) and zoomed spectrogram detail
crates/ui         the player window: skin, main/EQ/playlist sections, fullscreen host, playlist model,
                  waveform, spectrogram window
crates/dig        digging Discogs (native only): API client, rate limit and cache, page expansion,
                  send jobs, yt-dlp previews and their scheduler, preparing previews, dig memory,
                  the browser bridge (loopback server, pairing)
crates/visuals    the visual engine: signals, modulation, scenes (WGSL + RON), variants, director, GPU compositor, overlay, deck
crates/visuals/assets  the bundled scenes, variants, prelude and director rules
extensions/chrome the Chrome extension (plain JS, loaded unpacked) that sends Discogs pages to the bridge
assets/skin       the bundled original skin (atlas.png + skin.ron), generated by `cargo run -p ui --bin skin-gen`
apps/native       the desktop app: GUI (default), --tui, --bench, --click-test, --startup-time, --render-show
openspec/         specs and plans for every milestone (see below)
.claude/, .opencode/  OpenSpec agent commands (/opsx:propose, :explore, :apply, :archive)
AGENTS.md         conventions and checks for anyone (human or AI) changing the code
```

## Known limitations

- **AAC/M4A is not sample-exact gapless:** symphonia 0.6.1 ignores MP4 edit lists, so about
  43 ms of encoder priming and padding plays. MP3, FLAC, Vorbis and WAV are exact.
- **WAV tags** (RIFF `INFO`) are read by our own small parser, because symphonia 0.6.1 drops them.
- **Still to verify:** a 1-hour playlist with zero underruns, and the acoustic clock check.
- Surround files are played as their front left/right channels.
- **Top Sellers rely on undocumented Discogs calls** (`/purchases`, `/cart`): see
  [Top Sellers](#top-sellers) for what happens if they change. Bringing the player to the front
  from the browser asks the system; macOS often only bounces the Dock icon. A
  `diggr://` link that macOS would honour needs the app bundle (`macos-release`).

## Roadmap (OpenSpec)

Each milestone is an OpenSpec change with a proposal, design, specs and tasks in
`openspec/changes/`:

1. `audio-core`: the audio engine ✅ (done and archived, apart from the two checks above)
2. `classic-ui`: the classic skinned player, EQ and playlist (egui/wgpu), and the
   fullscreen key ✅ (done and archived)
3. `music-analysis`: beat grid, phrases, build/drop/breakdown detection that analyzes ahead of
   the playhead ✅ (done and archived)
4. `visual-engine`: fractal scenes (WGSL), a modulation matrix, a director, a fader deck, and an
   auto-fading track overlay ✅ (done and archived)
5. `web-target`: the same app in Chrome via WebAssembly, AudioWorklet and WebGPU (proposed in
   `openspec/changes/web-target/`, not started)
6. `waveform-navigation`: the coloured waveform, section/drop jumps on the beat, bar loops,
   and the shortcuts help ✅ (done and archived)
7. `spectrogram-window`: a spectrogram window with whole-track, zoomed and live views, and a
   check for files made from lossy sources ✅ (done and archived)
8. `beatmatch-automix`: tempo-matched, phrase-aligned DJ mixes between tracks (proposed)
9. `show-render`: render a track's visual show to an MP4, from the command line
   ✅ (done and archived)
10. `crates`: named playlists ("crates") switched from the playlist's title bar, with entries that
    remember the record they came from and can wait for their audio ✅ (done and archived)
11. `discogs-digging`: paste a Discogs page (label, artist, release, master, wantlist or list) to
    dig it in a crate. Previews are downloaded and analyzed a few tracks ahead, so each one opens
    ready to navigate. `Y` keeps a track (and adds it to your Discogs wantlist), `N` passes and
    `I` opens its for-sale page ✅ (done and archived)
12. `browser-bridge`: a Chrome extension with Play in / Enqueue in / Send to crate on Discogs pages
    and links, talking to the player through a paired local bridge (proposed; needs
    `discogs-digging`)
13. `seller-crates`: Top Sellers, a crate per seller dug on a double-click, one row per copy,
    SOLD, and your Discogs cart with the CART badge and switch

Finished changes move to `openspec/changes/archive/`, and their requirements become the living
specs in `openspec/specs/`.

The `openspec` CLI is optional; it's only needed to browse or advance the plans:

```sh
npm install -g @fission-ai/openspec   # requires Node.js
openspec list                         # active changes and task progress
openspec list --specs                 # specs of what's already built
openspec show classic-ui              # read the next milestone
openspec show audio-playback --type spec
```
