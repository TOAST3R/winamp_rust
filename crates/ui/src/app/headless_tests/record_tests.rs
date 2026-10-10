//! The playlist grouped by record, headless: the toggle, gathering, record rows that open,
//! select, play and move as one, the keyboard over rows, and the BPM filter.

use super::*;
use crate::playlist::Origin;

/// A playlist of playable entries from these releases (one entry per item), titled `T<i>`,
/// by Nightcraft, on "Album <release>".
fn records_rig(name: &str, releases: &[u64]) -> (Rig, Vec<EntryId>) {
    let mut rig = Rig::new(name, Vec::new(), |_| {});
    let ids = rig.fill_playlist(releases.len());
    for (i, (e, &r)) in rig
        .app
        .crates
        .shown_mut()
        .entries_mut()
        .zip(releases)
        .enumerate()
    {
        e.title = format!("T{i}");
        e.artist = "Nightcraft".into();
        e.album = format!("Album {r}");
        e.origin = Some(Origin {
            release: Some(r),
            catno: format!("LT-{r:03}"),
            year: Some(1994),
            position: format!("A{}", i + 1),
            album: format!("Album {r}"),
            styles: "Deep House, Minimal".into(),
            ..Default::default()
        });
    }
    rig.until(
        |r| {
            r.app
                .crates
                .shown()
                .entries()
                .iter()
                .all(|e| e.status.is_playable())
        },
        "the files are read",
    );
    (rig, ids)
}

/// The middle of the record row that starts at list row `unit` (record rows are two high).
fn record_at(unit: usize) -> Pos2 {
    pos2(
        PL_LEFT + 120.0,
        PL_TOP + LIST_TOP + (unit as f32 + 1.0) * 13.0,
    )
}

/// The record row's ▸ / ▾ mark, starting at list row `unit`.
fn open_mark_at(unit: usize) -> Pos2 {
    pos2(PL_LEFT + 42.0, PL_TOP + LIST_TOP + unit as f32 * 13.0 + 6.5)
}

fn titles(rig: &Rig) -> Vec<String> {
    rig.app
        .crates
        .shown()
        .entries()
        .iter()
        .map(|e| e.title.clone())
        .collect()
}

fn group(rig: &mut Rig) {
    let ctx = rig.ctx.clone();
    rig.app.apply(Action::ToggleGrouped, &ctx);
    assert!(rig.app.crates.shown().is_grouped());
}

#[test]
fn grouping_makes_record_rows_and_shift_g_does_nothing() {
    let (mut rig, _) = records_rig("group-toggle", &[1, 1, 1, 2, 2, 3]);
    group(&mut rig);
    let out = rig.frame(Vec::new());
    assert!(shows(&out, "Nightcraft – Album 1"), "{:?}", text_list(&out));
    assert!(
        text_list(&out)
            .iter()
            .any(|t| t == "LT-001 · 1994 · 3 tracks"),
        "{:?}",
        text_list(&out)
    );
    assert!(
        !shows(&out, "1. Nightcraft: T0"),
        "no track rows while closed"
    );
    assert_eq!(rig.app.pl_list().len(), 3);
    // Shift+G is no shortcut: the ▤ button and the ≡ menu group.
    rig.key(Key::G, Modifiers::SHIFT);
    assert!(rig.app.crates.shown().is_grouped());
    rig.click(footer_button(&rig, "pl_menu"));
    rig.click_text("Group by record");
    assert!(!rig.app.crates.shown().is_grouped());
    assert_eq!(rig.app.pl_list().len(), 6);
}

#[test]
fn grouping_gathers_each_record_and_playback_follows() {
    let (mut rig, ids) = records_rig("group-gather", &[1, 2, 1, 3, 1]);
    group(&mut rig);
    assert_eq!(titles(&rig), ["T0", "T2", "T4", "T1", "T3"]);
    // Track T0 of record 1 plays; the next is T2 of the same record.
    rig.app.apply(Action::PlayEntry(ids[0]), &rig.ctx.clone());
    rig.until(|r| r.app.position.state == PlayState::Playing, "it plays");
    rig.until(
        |r| r.engine_queue().len() == 5,
        "the queue follows the new order",
    );
    let queue = rig.engine_queue();
    let p = rig.app.crates.shown();
    assert_eq!(
        queue[1],
        p.get(ids[2]).unwrap().track,
        "the record's next track"
    );
    // The closed record row shows what plays in it.
    let out = rig.frame(Vec::new());
    assert!(
        text_list(&out).iter().any(|t| t.starts_with("⏵ A1 T0")),
        "{:?}",
        text_list(&out)
    );
}

#[test]
fn a_record_opens_with_its_mark_or_space_and_arrows_still_seek() {
    let (mut rig, _) = records_rig("group-open", &[1, 1, 2]);
    group(&mut rig);
    rig.click(open_mark_at(0));
    let out = rig.frame(Vec::new());
    assert!(shows(&out, "⏷"), "{:?}", text_list(&out));
    assert!(
        text_list(&out).iter().any(|t| t.contains("Nightcraft: T0")),
        "its tracks show: {:?}",
        text_list(&out)
    );
    assert_eq!(rig.app.pl_list().len(), 4, "record, two tracks, record");
    rig.click(open_mark_at(0));
    assert_eq!(rig.app.pl_list().len(), 2);
    // Space on the cursor's record row, with the playlist focused.
    rig.app.focus = Focus::Playlist;
    rig.click(record_at(0));
    rig.key(Key::Space, Modifiers::NONE);
    assert_eq!(rig.app.pl_list().len(), 4);
    rig.key(Key::Space, Modifiers::NONE);
    assert_eq!(rig.app.pl_list().len(), 2);
    // → seeks (nothing plays here) and opens nothing.
    rig.key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(rig.app.pl_list().len(), 2);
}

#[test]
fn a_record_row_selects_plays_and_removes_its_whole_record() {
    let (mut rig, ids) = records_rig("group-act", &[1, 1, 1, 2]);
    group(&mut rig);
    rig.click(record_at(0));
    assert_eq!(rig.app.crates.shown().selected_ids(), &ids[..3]);
    // Cmd-click on the other record adds it.
    rig.click_with_mods(record_at(2), Modifiers::COMMAND);
    assert_eq!(rig.app.crates.shown().selected_ids().len(), 4);
    // The menu acts on the record; Remove album isn't there (the row is the album).
    rig.click(record_at(0));
    rig.click_with(record_at(0), PointerButton::Secondary);
    let out = rig.frame(Vec::new());
    assert!(shows(&out, "Remove"), "{:?}", text_list(&out));
    assert!(
        !text_list(&out)
            .iter()
            .any(|t| t.starts_with("Remove album"))
    );
    assert!(!shows(&out, "Select album"));
    rig.click_text("Remove");
    assert_eq!(rig.ids(PLAYLIST), [ids[3]], "all three removed");
}

#[test]
fn a_double_click_plays_a_record_from_its_first_playable_track() {
    // The rig's clicks share one double-click window, so this is the test's first click.
    let (mut rig, ids) = records_rig("group-play", &[1, 1, 2]);
    rig.app.crates.shown_mut().set_waiting(ids[0], "queued");
    group(&mut rig);
    rig.double_click(record_at(0));
    rig.until(|r| r.app.position.state == PlayState::Playing, "it plays");
    assert_eq!(
        rig.app.crates.shown().current(),
        Some(ids[1]),
        "the first one still waits for its audio"
    );
}

#[test]
fn a_dragged_record_moves_as_a_block() {
    let (mut rig, ids) = records_rig("group-drag", &[1, 2, 2, 3]);
    group(&mut rig);
    // Records 1 (one entry), 2 (two) and 3 (one): drag record 2 above record 1.
    let (from, to) = (record_at(2), record_at(0));
    rig.frame(vec![Event::PointerMoved(from)]);
    rig.press(from, PointerButton::Primary, true);
    for y in [from.y - 4.0, from.y - 12.0, to.y] {
        rig.frame(vec![Event::PointerMoved(pos2(from.x, y))]);
    }
    rig.press(to, PointerButton::Primary, false);
    rig.frame(Vec::new());
    assert_eq!(rig.ids(PLAYLIST), [ids[1], ids[2], ids[0], ids[3]]);
}

#[test]
fn the_keyboard_walks_rows_and_p_finds_a_track_in_a_closed_record() {
    let (mut rig, ids) = records_rig("group-keys", &[1, 1, 1, 1, 2, 3]);
    group(&mut rig);
    rig.app.focus = Focus::Playlist;
    rig.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(
        rig.app.crates.shown().selected_ids(),
        &ids[..4],
        "the first record, all of it"
    );
    rig.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(
        rig.app.crates.shown().selected_ids(),
        [ids[4]],
        "one step past it"
    );
    rig.key(Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!(rig.app.crates.shown().selected_ids(), [ids[4], ids[5]]);
    // Enter on a closed record row plays the record.
    rig.key(Key::Home, Modifiers::NONE);
    rig.key(Key::Enter, Modifiers::NONE);
    rig.until(|r| r.app.position.state == PlayState::Playing, "it plays");
    assert_eq!(rig.app.crates.shown().current(), Some(ids[0]));
    // P puts the cursor on the playing track's row: its closed record.
    rig.key(Key::End, Modifiers::NONE);
    rig.key(Key::P, Modifiers::NONE);
    assert_eq!(rig.app.pl_cursor_row().map(|r| r.first()), Some(0));
}

#[test]
fn under_a_filter_a_record_shows_what_matches() {
    let (mut rig, _) = records_rig("group-filter", &[1, 1, 1, 1, 2]);
    for (e, bpm) in rig
        .app
        .crates
        .shown_mut()
        .entries_mut()
        .zip([134, 124, 134, 124, 124])
    {
        e.bpm = Some(bpm);
    }
    group(&mut rig);
    rig.app
        .apply(Action::SetBpmFilter(Some((130, 140))), &rig.ctx.clone());
    let out = rig.frame(Vec::new());
    assert!(
        text_list(&out)
            .iter()
            .any(|t| t == "LT-001 · 1994 · 2 of 4 tracks"),
        "{:?}",
        text_list(&out)
    );
    assert_eq!(rig.app.pl_list().len(), 1, "record 2 has nothing shown");
    rig.click(open_mark_at(0));
    assert_eq!(rig.app.pl_list().len(), 3, "only its two matching tracks");
}

#[test]
fn grouping_is_remembered_per_crate() {
    let (mut rig, _) = records_rig("group-remember", &[1, 1]);
    group(&mut rig);
    let friday = rig.crate_with("Friday", &["tone.wav"]);
    rig.app.show_crate(friday);
    assert!(
        !rig.app.crates.shown().is_grouped(),
        "another crate stays flat"
    );
    rig.app.show_crate(PLAYLIST);
    assert!(rig.app.crates.shown().is_grouped());
    rig.app.crates.save_due(true, Duration::ZERO);
    let crates = Crates::open(&Store::new(rig.dir.join("config")));
    assert!(crates.is_grouped(PLAYLIST), "saved");
}

#[test]
fn grouped_has_no_column_header_and_record_rows_show_their_styles() {
    let (mut rig, _) = records_rig("group-header", &[1, 1, 2]);
    rig.app.settings.playlist_width = 700;
    let out = rig.frame(Vec::new());
    assert!(
        shows(&out, "Cat#"),
        "flat and wide: a header {:?}",
        text_list(&out)
    );
    let flat_rows = rig.app.pl_visible_rows();
    group(&mut rig);
    let out = rig.frame(Vec::new());
    assert!(!shows(&out, "Cat#"), "grouped: none {:?}", text_list(&out));
    assert_eq!(
        rig.app.pl_visible_rows(),
        flat_rows + 1,
        "its row goes to the list"
    );
    assert!(shows(&out, "Deep House, Minimal"), "{:?}", text_list(&out));
}

#[test]
fn a_record_row_names_the_record_artist() {
    // Album 1 is a compilation credited to "Various", its first track by Nightcraft; album 2
    // carries no record artist (saved before it existed) and takes its first track's.
    let (mut rig, _) = records_rig("group-artist", &[1, 1, 2]);
    for (i, e) in rig.app.crates.shown_mut().entries_mut().enumerate() {
        if i < 2 {
            e.origin.as_mut().unwrap().artist = "Various".into();
        }
        if i == 1 {
            e.artist = "Lumen".into();
        }
    }
    group(&mut rig);
    let out = rig.frame(Vec::new());
    let list = text_list(&out);
    assert!(
        list.iter().any(|t| t.starts_with("Various – Album 1")),
        "{list:?}"
    );
    assert!(
        list.iter().any(|t| t.starts_with("Nightcraft – Album 2")),
        "{list:?}"
    );
}

#[test]
fn the_sidebar_counts_records_grouped_or_flat() {
    let (mut rig, _) = records_rig("group-count", &[1, 1, 1, 2, 2, 3]);
    rig.app.settings.playlist_width = 700;
    let count_of = |out: &egui::FullOutput| {
        // The Playlist crate's count, on its sidebar row.
        let name = texts(out)
            .into_iter()
            .find(|t| t.text == "Playlist")
            .unwrap();
        texts(out)
            .into_iter()
            .filter(|t| (t.rect.center().y - name.rect.center().y).abs() < 2.0)
            .find_map(|t| t.text.parse::<usize>().ok())
    };
    let out = rig.frame(Vec::new());
    assert_eq!(count_of(&out), Some(3), "flat: its records");
    group(&mut rig);
    let out = rig.frame(Vec::new());
    assert_eq!(count_of(&out), Some(3), "grouped: its records");
    // Saved with the crate, for when it isn't loaded.
    rig.app.crates.save_due(true, Duration::ZERO);
    let index = std::fs::read_to_string(rig.dir.join("config/crates/index.ron")).unwrap();
    assert!(index.contains("records: 3"), "{index}");
}

/// Every character in the UI's strings (outside tests) has a glyph in the fonts egui draws
/// with, so none shows as an empty box. Main-window messages use the skin's own font, which
/// draws `›` as `>`.
#[test]
fn every_character_the_ui_draws_has_a_glyph() {
    fn sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                if !p.ends_with("headless_tests") {
                    sources(&p, out);
                }
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    sources(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut used = std::collections::BTreeMap::<char, String>::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        // Test modules sit at the end of each file (`#[cfg(test)]` also marks single items).
        let end = text
            .match_indices("#[cfg(test)]")
            .map(|(i, _)| i)
            .find(|&i| {
                text[i..]
                    .trim_start_matches("#[cfg(test)]")
                    .trim_start()
                    .starts_with("mod ")
            })
            .unwrap_or(text.len());
        let code = &text[..end];
        // Comments and terminal output (eprintln!) aren't drawn.
        let drawn = |l: &&str| {
            let l = l.trim_start();
            !l.starts_with("//") && !l.contains("println!(")
        };
        for line in code.lines().filter(drawn) {
            for (i, lit) in line.split('"').enumerate() {
                if i % 2 == 1 {
                    for c in lit.chars().filter(|c| !c.is_ascii()) {
                        used.entry(c).or_insert_with(|| f.display().to_string());
                    }
                }
            }
        }
    }
    assert!(used.contains_key(&'⏵'), "the scan finds the UI's strings");
    let (mut rig, _) = records_rig("glyphs", &[1]);
    rig.frame(Vec::new());
    let font = egui::FontId::proportional(9.5);
    let missing: Vec<String> = used
        .iter()
        .filter(|(c, _)| !rig.ctx.fonts_mut(|f| f.has_glyphs(&font, &c.to_string())))
        .map(|(c, f)| format!("{c} (in {f})"))
        .collect();
    assert!(missing.is_empty(), "no glyph for: {missing:?}");
}
