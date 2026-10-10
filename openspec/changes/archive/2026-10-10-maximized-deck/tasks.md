## 1. Search field draws once (bug)

- [x] 1.1 `App::search_field`: set `selection.stroke` transparent along with `bg_fill`; draw the selection from `out.cursor_range` (primary/secondary, clipped to the visible tail) as an LCD block with the selected chars in the field fill colour
- [x] 1.2 Headless test: with "vigne" typed and all selected, no glyphs other than the skin font's are painted in the field rect, and the selected range is drawn as a block; typing over the selection leaves "A" and no block

## 2. Every owned record in the collection crate (bug)

- [x] 2.1 `digging.rs` crate fill: in the collection and wantlist crates, key the clip de-dup on (record, clip) instead of clip; other crates keep the crate-wide set
- [x] 2.2 Headless tests: two releases sharing all videos sent into the collection crate both get entries; the same two into a seller crate give each video once; refresh with a release missing from the crate adds it

## 3. Record count and Shift+G

- [x] 3.1 Sidebar: show `record_count` for every crate (drop the `is_grouped` branch); tooltip gives records and tracks; headless test: a flat collection crate of 6,322 tracks from 924 records shows 924
- [x] 3.2 Remove the Shift+G handler and its `help.rs` entries (list and test array); headless test: Shift+G on a flat crate leaves it flat; rewrite tests that grouped with Shift+G to click ▤ or use the ≡ item

## 4. Starting a search clears the filters

- [x] 4.1 `Playlist::set_search`: on the empty → non-empty edge of the folded words, also `set_bpm_filter(None)` and `clear_picks(None)`; `cart_only` untouched
- [x] 4.2 Unit tests in `playlist.rs`: start clears BPM and picks; editing a non-empty search keeps a filter set after it; whitespace-only search clears nothing; cart-only survives; headless test: "Search ‹label›" clears a BPM range

## 5. Style column

- [x] 5.1 `columns.rs`: `Field::Style` after `Format` in `ALL`, label "Style", default width share, value from `Origin::styles`; sort by first style case-folded, empty last; old `ColumnSettings` without it show it at default width
- [x] 5.2 Tests: cell text for "Minimal, Deep House"; sort order "Ambient, Drone", "techno", none; loading `(hidden: [Year], widths: {})` shows Style and keeps Year hidden

## 6. Maximized deck layout

- [x] 6.1 Mini player positions (275 × 58: status, time digits, title, prev/play/stop/next, volume, ⇔) placed in code from existing sprites, as the strip did; the volume rect added to a cloned `SkinDef` so `hslider` draws it; no skin regeneration
- [x] 6.2 `layout.rs`: remove `STRIP_W`; `maximized()` gives the full width and rows under a 58-pixel band (always shown); update its unit tests
- [x] 6.3 `App`: replace `player_strip` with `mini_player` (existing `Action`s, `options_menu` over its non-control area, tooltip); `maximized_layout` draws the band (mini player left, waveform or panel fill right) and the playlist under it
- [x] 6.4 Headless tests: rewrite the strip tests (`pl_geometry` = full width, next in the mini player = B, right-click opens Options); waveform overview starts at x = 275 and seeks; W off keeps the band and the row count; restore still returns the previous frame

## 7. Missing previews download again

- [x] 7.1 When opening or reading a Discogs track fails and its preview file is missing, put it back to Waiting(Queued) (not Failed) and refresh the crate's horizon; local files still fail
- [x] 7.2 Headless test: a collection track whose path points at a deleted file goes back to queued when read, and downloads again

## 8. Refresh progress and Stop

- [x] 8.1 `dig::collection`: a stepwise `Syncer` (one page per step, with page k of n); `sync()` runs it to the end; existing tests stay green, plus a step test
- [x] 8.2 Intake: run the sync one page per background step, `Event::CollectionProgress { read, pages }`, `Command::CancelCollectionSync`; `Command::StopJobs(target)` ends a crate's jobs with `Finished` events
- [x] 8.3 UI: Refresh collection window (progress bar, phase text, Stop); Stop cancels the sync or the fill (Listed placeholders removed) and says "Refresh collection stopped"; closes when done
- [x] 8.4 Headless tests: progress shows pages; Stop while reading keeps the cache and sends no more pages; Stop while adding keeps what arrived

## 9. Docs and checks

- [x] 9.1 `README.md`: maximized-mode description (mini player + waveform band), Style column, search clearing filters, sidebar record counts, Shift+G removed from the ▤ line and shortcuts table, refresh window, missing previews; test count in the Tests section
- [x] 9.2 `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, `cargo check -p audio -p platform --target wasm32-unknown-unknown` all clean
- [x] 9.3 Run the app maximized on a big display: check the mini player at 1× and 2×, select all in the search field, and refresh the collection to confirm 928 records, watch the refresh window and try Stop, and play the red 48V Phantom Power record
