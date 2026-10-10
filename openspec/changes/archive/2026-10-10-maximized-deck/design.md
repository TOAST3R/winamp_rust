## Context

This change bundles three UI changes and two bugs that came up together while digging in a large collection crate on a big screen:

- The maximized playlist (`App::maximized_layout`, `crates/ui/src/app.rs`) draws the player as a 27-pixel strip (`player_strip`, `layout::STRIP_W`). The waveform is a band across the remaining width, and the playlist sits under it. `layout::maximized()` sizes the playlist from those.
- The columns (`crates/ui/src/columns.rs`, `Field`) have no style. Styles are on `Origin::styles` (a `String`, as Discogs lists them) and are only drawn in record rows (`app.rs`, "The record's styles at the right end of its first line").
- `Playlist::set_search` (`crates/ui/src/playlist.rs`) only stores the folded words. The BPM range and picks are independent of it, so a filter left over from earlier hides search results without any sign.
- `App::search_field` hides an egui `TextEdit` under skin-drawn text by making `text_color`, `selection.bg_fill` and the caret transparent. egui paints *selected* glyphs with `visuals.selection.stroke.color`, which was left at its default, so a selection shows egui's own small font under the skin text.
- When a crate is filled (`digging.rs`, the `have: HashSet<String>` of clip ids), every video already in the crate is skipped. A record whose videos are all used by other records gets no entries and disappears. In the user's collection, 4 of 928 records hit this (e.g. 26559413 shares all 6 videos with another pressing, 26739893).

## Goals / Non-Goals

**Goals:**
- A maximized layout that shows a usable player and the waveform side by side at the top, in the 58-pixel height the waveform band already takes.
- Styles visible and sortable in the column layout.
- A search never silently narrowed by filters set before it.
- Both bugs fixed, each with a test.

**Non-Goals:**
- No EQ in the maximized layout.
- No change to the normal (non-maximized) layout or the classic player column.
- No click-to-filter on the style cell (right-click "Only this style ▸" already exists).
- No restoring filters after a search is cleared.
- The ownership cache and its sync (`dig/src/collection.rs`) are correct and are not touched.

## Decisions

**1. The mini player is a new compact layout, not the main window.** The main window is 116 skin pixels tall and the band is 58, so the main window doesn't fit. Scaling it down would blur the pixel skin. The mini player is 275 × 58 (the main window's width), drawn from existing sprites where possible: the status sprite, the time digits, the title scroller's font, the transport buttons and the volume slider. It has two rows: status, time and title on top; prev / play-pause / stop / next, volume and ⇔ below. Like the strip before it, its positions are fixed in code over existing sprites, so the skin isn't regenerated. Only the volume slider needs a layout rect, which goes into a cloned `SkinDef` for that frame.
   - *Alternative:* keep the strip and widen it. Rejected, because the user wants the player top-left with the waveform beside it.

**2. The band is always shown in maximized mode.** With W off, the right part of the band is panel fill. Then the playlist's rows don't jump when W is toggled, and the transport stays reachable. `layout::maximized()` becomes: width = `w` (no strip), rows from `h − BAND_H − chrome`, with `BAND_H = waveform::HEIGHT`. `STRIP_W` is deleted.

**3. The mini player's controls dispatch the existing `Action`s** (`Prev`, `Play`/`Pause`, `Stop`, `Next`, `SetVolume`, `ToggleMaximized`) and reuse `options_menu` over its non-control area. This is the same wiring the strip and main window use, so there's no new playback path.

**4. A style column stores nothing new.** `Field::Style` reads `Origin::styles`. It sorts on the first comma-separated style, case-folded, with empty values last. It goes after `Format` in `Field::ALL`. Saved `ColumnSettings` without it show it at its default width, the same migration the Format column used (unknown fields in `widths` fall back to the default).

**5. Filters are cleared in `Playlist::set_search`, on the empty → non-empty edge.** When the stored words were empty and the new ones aren't, `set_search` also calls `set_bpm_filter(None)` and `clear_picks(None)`. Doing it in the model covers every way in: typing (`Action::SetSearch`), "Search ‹label›" (`Action::SearchFor`), and paste. It also keeps the rule testable without the UI. `cart_only` is not touched. A whitespace-only search folds to no words, so it doesn't start a search.
   - *Alternative:* clear in the UI's `search_field`. Rejected, because it would miss `SearchFor`.

**6. The search-field fix sets `selection.stroke` transparent too, and draws the selection itself.** `out.cursor_range` already gives the primary and secondary indices. The skin path draws an LCD-coloured block over the selected characters (within the visible tail) and those characters in the field fill colour. The caret logic is unchanged.

**7. Clip de-duplication is per record in the user's own crates.** At the `have.insert(c.clip)` check, when the job's target is the collection crate or the wantlist crate, the key becomes `(record, clip)` instead of `clip`. The existing `places` set (searched tracks) is already keyed by record. Seller, label and other crates keep the crate-wide clip set, because there one video listed under several pressings would otherwise fill the crate with duplicates. The next Refresh collection then adds the 4 missing records through the existing `new = owned − present` path. No migration is needed.

**8. The sidebar always uses `record_count`.** The index already stores `records` per crate (`crates/index.ron`), so the count needs no new data: drop the `is_grouped` branch at the sidebar count. The tooltip shows "924 records · 6,322 tracks". For a crate of local files without albums, records equal entries, so nothing changes there.

**9. Shift+G is removed, not rebound.** Its handler and its help entry go. ▤ and ≡ ▸ Group by record stay. The user wants fewer shortcuts, so no replacement key is added.

## Risks / Trade-offs

- [A 58-pixel mini player with two rows of controls is cramped at 1× on a small display] → Maximized mode is for big screens. The controls keep the 23 × 18 transport sprite size the strip already used.
- [Clearing filters when a search starts could surprise someone who set a filter on purpose to search inside it] → The rule is the user's choice. Setting the filter after typing still narrows the search, and the filter bar shows what's set.
- [Duplicate entries for one video in the collection crate (two pressings of the same tune)] → That's what the user asked for: every owned record is shown. The entries share a cached preview, so no extra download.
- [Headless tests that assert on `STRIP_W` and the strip's buttons break] → They are rewritten against the mini player in the same task.

## Migration Plan

No data migration. Old column settings pick up the Style column at its default width. Users who quit while maximized relaunch into the new layout. Rollback is a revert.

## Open Questions

- Should the mini player also show the BPM / section readout from the main window? Left out for now. Add it if the width allows once the sprites are placed.
