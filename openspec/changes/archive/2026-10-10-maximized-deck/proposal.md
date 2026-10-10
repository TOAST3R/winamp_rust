## Why

On a big screen the maximized playlist hides almost all of the player behind a 27-pixel strip, while the waveform band stretches across the whole width. In the column view a record's styles can't be seen at all. Searching while a filter is set silently hides records: a search for "Cortini" found nothing because a BPM range from an earlier dig was still set. Two bugs make it worse. Selecting text in the search field draws it a second time in egui's small default font. The collection crate drops a record outright when every one of its videos is already used by another record (4 of 928 records are missing).

## What Changes

- **BREAKING (layout)**: the maximized playlist no longer has the 27-pixel player strip. A band as tall as the waveform (58 skin pixels) runs across the top. It holds a compact mini player on the left, at the main window's width (275 skin pixels), with the waveform filling the rest. The playlist takes the full window width under the band. The band is shown even with the waveform off, the EQ stays hidden, and the playlist's rows follow the window as before.
- The mini player shows the play state, the elapsed time, the playing title, previous / play-pause / stop / next, volume and the ⇔ restore button. Right-clicking it opens the Options menu, as the strip did.
- A new **Style** column in the column layout, after Format, shows the record's styles ("Deep House, Minimal"). It sorts by the record's first style, and it can be hidden and resized like the other columns.
- Starting a search clears the BPM range and every record filter (styles, artists, labels, formats). Starting means the field goes from empty to text, by typing or from the "Search ‹label›" menu items. Filters set while a search is shown still narrow it, and clearing the search doesn't bring cleared filters back. The CART switch is left alone.
- The crate sidebar always shows a crate's number of records, grouped or flat (the collection read 6,322 tracks instead of 924 records after being switched to flat). Its tooltip gives both counts.
- **BREAKING (keys)**: Shift+G is removed. Grouping stays on ▤ and ≡ ▸ Group by record.
- Refresh collection shows a progress window (reading pages, then adding records) with a Stop button.
- Fix: a Discogs track whose preview file is missing (evicted while its crate was closed, or left in the pre-rename cache folder) downloads again instead of turning red.
- Fix: selected text in the search field is drawn once, in the skin font, with the selection shown as an LCD-coloured block.
- Fix: in the collection and wantlist crates, a record gets its own entries even when its videos are already used by another record of the crate. Every owned record then appears. Seller, label and other pages keep skipping a video the crate already holds.

## Capabilities

### New Capabilities
<!-- none -->

### Modified Capabilities
- `playlist`: "Maximized playlist" now has a top band (mini player + waveform) in place of the left strip.
- `waveform-view`: while maximized, the waveform is the right part of the top band, not a band over the full width.
- `player-window`: the Options right-click applies to the maximized mini player, not the strip.
- `playlist-columns`: new Style column.
- `crate-search`: starting a search clears the filters; selected text is drawn once in the skin.
- `crates`: the sidebar counts records in every crate.
- `record-view`: grouping only from ▤ and the ≡ menu; the record count no longer depends on grouping.
- `player-window`: Shift+G leaves the keyboard shortcuts.
- `preview-fetch`: a missing preview file sends the track back to the download queue.
- `discogs-collection`: refresh progress and Stop; the collection crate holds every owned record, even ones whose videos are shared with another record.

## Impact

- `crates/ui/src/app.rs`: `maximized_layout`, `player_strip` replaced by a mini player, `search_field` (selection colours, clearing filters), the filter-clearing path (`Action::SetSearch`).
- `crates/ui/src/layout.rs`: `maximized()` and `STRIP_W` (removed); its tests.
- `crates/ui/src/columns.rs`: `Field::Style`; old saved column settings get it at its default width.
- `crates/ui/src/app/digging.rs`: the clip de-duplication when filling a crate (`have.insert`), per record for the collection and wantlist crates.
- `crates/ui/src/skin/`: the mini player's layout rects (and any new sprites) in `skin-gen`, so `assets/skin/default/` gets regenerated.
- Headless UI tests for the maximized layout and the strip (e.g. `pl_geometry` = 1100 − `STRIP_W`) change.
- `crates/ui/src/app.rs` sidebar count (`record_count` regardless of `is_grouped`), the Shift+G key handler; `crates/ui/src/help.rs` entry.
- `README.md`: the maximized-mode description, the ▤ line and the shortcuts table without Shift+G.
- No changes to audio, analysis or the playback path.
