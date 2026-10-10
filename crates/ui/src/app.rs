//! The eframe application: main player, equalizer and playlist stacked in one borderless
//! window, plus fullscreen visual mode.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use audio::{Engine, EngineEvent, EqPreset, EqPresets, PlayState, Position, TapReader, TrackInfo};
use egui::{
    Color32, Id, Key, Modifiers, Pos2, Rect, Sense, TextureHandle, Ui, ViewportCommand, pos2, vec2,
};
use platform::{FileSource, Spawner, TrackRef};

use crate::columns::{Col, Dir, Field};
use crate::crates::{CrateId, Crates, MetaKey, PLAYLIST};
use crate::eqcurve;
use crate::files;
use crate::format;
use crate::fullscreen::{SceneFrame, VisualScene};
use crate::metadata::{MetaResult, MetaWorker};
use crate::playlist::{AlbumKey, ClickMods, CursorMove, EntryId, EntryStatus, Facet};
use crate::records::{self, ListRow};
use crate::settings::{PRESETS_FILE, Repeat, SETTINGS_FILE, Settings, Store, VisMode};
use crate::skin::LoadedSkin;
use crate::spectrum::{Analyzer, BARS};
use crate::widgets::{self, Skinned, SliderSprites, color};

#[cfg(not(target_arch = "wasm32"))]
mod bandcamp;
#[cfg(not(target_arch = "wasm32"))]
mod covers;
#[cfg(not(target_arch = "wasm32"))]
mod digging;
#[cfg(not(target_arch = "wasm32"))]
mod sellers;
#[cfg(not(target_arch = "wasm32"))]
pub use digging::{BridgeSetup, DigAction, DigSetup, SendMode};

pub type EngineFactory = Box<dyn FnOnce() -> Result<Engine, String> + Send>;

/// What the host app provides.
pub struct AppContext {
    pub engine: EngineFactory,
    pub spawner: Arc<dyn Spawner>,
    pub files: Arc<dyn FileSource>,
    /// Where settings/playlist/presets live; `None` disables persistence.
    pub store: Option<Store>,
    /// Files from the command line: replace the Playlist crate and play.
    pub open: Vec<PathBuf>,
    pub scene: Box<dyn VisualScene>,
    pub startup: Startup,
    /// Music analysis ahead of the playhead (beats, sections) for the visuals.
    pub analysis: Option<analysis::AnalysisService>,
    /// Where annotation mode saves its JSON files.
    pub annotations_dir: Option<PathBuf>,
    /// Native-rate track overviews for the waveform section.
    pub overviews: Option<analysis::overview::OverviewService>,
    /// Digging Discogs pages into crates of previews; `None` turns it off.
    #[cfg(not(target_arch = "wasm32"))]
    pub dig: Option<DigSetup>,
}

#[derive(Clone, Copy)]
pub struct Startup {
    pub process_start: Instant,
    /// Print time-to-first-frame.
    pub report: bool,
    /// Quit right after the first frame (for launch-time measurements).
    pub exit_after_first_frame: bool,
}

enum EngineSlot {
    Starting(Receiver<Result<Engine, String>>),
    Ready(Box<Engine>),
    Failed(String),
}

struct Fullscreen {
    restore: Option<Rect>,
    last_pointer: Instant,
    pointer_at: Option<Pos2>,
}

pub struct DiggrApp {
    skin: LoadedSkin,
    def: Arc<crate::skin::SkinDef>,
    tex: Option<TextureHandle>,
    store: Option<Store>,
    settings: Settings,
    presets: EqPresets,
    /// The crates: the window edits the shown one, the engine queue comes from the playing one.
    crates: Crates,
    /// Entry ids in engine-queue order, and the crate they belong to.
    queue: Vec<EntryId>,
    queue_crate: CrateId,
    queue_dirty: bool,
    shuffle_seed: u64,
    engine: EngineSlot,
    /// Play once the engine is up: an entry, or whatever Play would start.
    pending_play: Option<Option<(CrateId, EntryId)>>,
    /// A waiting entry that starts as soon as its audio arrives.
    armed: Option<(CrateId, EntryId)>,
    tap: Option<TapReader>,
    analyzer: Analyzer,
    meta: Option<MetaWorker<MetaKey>>,
    now_playing: Option<TrackInfo>,
    position: Position,
    seek_drag: Option<f32>,
    /// The LP knob, 0..=1 (1 is off); not remembered, so every launch starts with it off.
    lp_knob: f32,
    title_offset: usize,
    title_tick: f64,
    /// The first row in view (a record row counts as one).
    pl_scroll: usize,
    pl_drag_from: Option<usize>,
    /// A record row being dragged: all its entries move.
    pl_drag_block: Option<Vec<EntryId>>,
    /// A track row of an open record being dragged: it moves within that record only.
    pl_drag_track: Option<AlbumKey>,
    /// The records open in each grouped crate (for the session).
    pl_open: HashMap<CrateId, HashSet<AlbumKey>>,
    /// Bumped when a record opens or closes.
    pl_open_rev: u64,
    /// The shown crate's rows, and what they were built from.
    pl_rows_cache: RefCell<Option<(RowsKey, Rc<Vec<ListRow>>)>>,
    /// The rest of the album of the entry whose menu is open (tinted; the selection stays).
    pl_tint: Vec<EntryId>,
    /// The BPM range handle being dragged: 0 the low one, 1 the high one.
    bpm_drag: Option<u8>,
    /// The crate last clicked in the sidebar, until the list is clicked: Delete deletes it.
    side_crate: Option<CrateId>,
    /// Where the footer's BPM slider was last drawn: its track's ends and whether the range
    /// text fit (skin pixels, footer coordinates), for tests.
    #[cfg(test)]
    bpm_slider: Option<(f32, f32, bool)>,
    /// The shown crate's values per filter (style, artist, label) with their record counts,
    /// and what they were counted from.
    facet_cache: [Option<(CrateId, u64, FacetCounts)>; 4],
    /// What each filter's list is narrowed to.
    facet_search: [String; 4],
    /// "Records leave this crate with…" was said this session.
    discogs_hint_shown: bool,
    /// Where the footer's filters were last drawn, and how, for tests.
    #[cfg(test)]
    filters_drawn: Option<(f32, FooterFilters)>,
    /// The CART switch as last drawn (its label and where, in points), for tests.
    #[cfg(test)]
    cart_drawn: Option<(String, Pos2)>,
    pl_resize_acc: egui::Vec2,
    /// The filter bar's search text as typed (the shown crate holds its folded words).
    pl_search: String,
    /// Cmd+F was pressed: the search field takes the keyboard on the next frame.
    search_focus: bool,
    /// The filter panel's tab.
    panel_tab: Facet,
    /// Scrolling not yet enough for a whole row, in points.
    pl_scroll_acc: f32,
    /// The window's content size in skin pixels (the maximized layout follows it).
    win_px: egui::Vec2,
    /// The OS has reported the window maximized since the playlist was maximized (so a
    /// later "not maximized" means it was restored behind our back).
    max_seen: bool,
    /// Which side the arrow keys work on.
    focus: Focus,
    /// The shown crate's playing entry when the list last looked (see `follow_playing_entry`).
    pl_follow: Option<(CrateId, EntryId)>,
    /// The playlist width has been fitted to the monitor.
    width_fitted: bool,
    name_dialog: Option<NameDialog>,
    /// A crate with entries waiting for "Delete crate…" to be confirmed.
    confirm_delete: Option<CrateId>,
    fullscreen: Option<Fullscreen>,
    scene: Box<dyn VisualScene>,
    scene_ready: bool,
    render_state: Option<eframe::egui_wgpu::RenderState>,
    last_frame: Instant,
    startup: Startup,
    first_frame_done: bool,
    /// The shortcuts help panel (`H` / `F1`).
    help: bool,
    overviews: Option<analysis::overview::OverviewService>,
    /// The audible track's overview, as far as it is built.
    overview: Option<Arc<analysis::overview::Overview>>,
    nav: Nav,
    /// The spectrogram window (`S`) and whether it is open.
    spectro: crate::spectrogram::SpectrogramWindow,
    spectro_open: bool,
    /// `DIGGR_FRAME_STATS=1`: per-second frame phase timings on stderr and in
    /// `$TMPDIR/diggr_frame_stats.log`.
    profile: Option<FrameProfile>,
    /// `DIGGR_AUTO_FULLSCREEN=1`: enter fullscreen once playback starts (for unattended runs).
    auto_fullscreen: bool,
    /// `DIGGR_AUTO_QUIT_SECS=n`: close the app after n seconds.
    auto_quit: Option<Instant>,
    settings_dirty: Option<Instant>,
    last_size: Option<egui::Vec2>,
    message: Option<(String, Instant)>,
    /// A verdict just landed: the title line shows it for [`FLASH_SECS`] (WANTED, PASS, …).
    flash: Option<(String, Instant)>,
    analysis: Option<analysis::AnalysisService>,
    files: Arc<dyn FileSource>,
    /// Engine track instances → files, from `TrackLoaded` events.
    track_refs: std::collections::HashMap<audio::TrackId, TrackRef>,
    score: Option<Arc<analysis::SongScore>>,
    /// The last playing track whose tempo was given to its entries.
    bpm_track: Option<TrackRef>,
    strip: bool,
    annotating: Option<analysis::eval::Annotations>,
    annotations_dir: Option<PathBuf>,
    #[cfg(not(target_arch = "wasm32"))]
    dig: Option<digging::Dig>,
}

const SAVE_DELAY: Duration = Duration::from_millis(800);

impl DiggrApp {
    pub fn new(cc: &eframe::CreationContext<'_>, ctx: AppContext) -> Self {
        Self::build(cc.egui_ctx.clone(), cc.wgpu_render_state.clone(), ctx)
    }

    fn build(
        egui_ctx: egui::Context,
        render_state: Option<eframe::egui_wgpu::RenderState>,
        ctx: AppContext,
    ) -> Self {
        let store = ctx.store;
        let settings = store.as_ref().map(Store::load_settings).unwrap_or_default();
        let presets = store.as_ref().map(Store::load_presets).unwrap_or_default();
        let mut crates = store.as_ref().map_or_else(Crates::in_memory, Crates::open);

        // Open the audio device in the background: the window must not wait for it.
        let (tx, rx) = std::sync::mpsc::channel();
        let factory = ctx.engine;
        let wake_ctx = egui_ctx.clone();
        std::thread::Builder::new()
            .name("engine-start".into())
            .spawn(move || {
                let _ = tx.send(factory());
                wake_ctx.request_repaint();
            })
            .expect("spawn engine starter");

        let wake_ctx = egui_ctx.clone();
        let scores = ctx.analysis.as_ref().and_then(|a| a.cache().cloned());
        let meta = MetaWorker::start(&*ctx.spawner, ctx.files.clone(), scores, move || {
            wake_ctx.request_repaint()
        })
        .ok();

        let mut pending_play = None;
        if !ctx.open.is_empty() {
            let added = crates.replace_playlist(
                files::expand(&ctx.open)
                    .into_iter()
                    .map(|p| TrackRef::new(p.to_string_lossy())),
            );
            pending_play = Some(added.first().map(|(key, _)| *key));
            if let Some(m) = &meta {
                m.request(added);
            }
        }
        if let Some(m) = &meta {
            m.request(crates.take_pending_meta());
        }
        let message = Some(crates.take_messages().join("\n"))
            .filter(|m| !m.is_empty())
            .map(|m| (m, Instant::now()));
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);

        let spectro = crate::spectrogram::SpectrogramWindow::new(
            settings.spectrogram.clone(),
            Some(Arc::new(analysis::detail::DetailService::new(
                ctx.spawner.clone(),
                ctx.files.clone(),
            ))),
        );
        #[cfg(not(target_arch = "wasm32"))]
        let dig = ctx.dig.map(|setup| {
            digging::Dig::new(
                setup,
                ctx.spawner.clone(),
                ctx.files.clone(),
                store.as_ref().map(|s| s.dir().to_path_buf()),
                egui_ctx.clone(),
            )
        });
        let skin = LoadedSkin::default_skin();
        Self {
            def: Arc::new(skin.def.clone()),
            skin,
            tex: None,
            store,
            analyzer: Analyzer::new(48_000),
            settings,
            presets,
            queue: Vec::new(),
            queue_crate: crates.playing_id(),
            crates,
            queue_dirty: true,
            shuffle_seed: seed,
            engine: EngineSlot::Starting(rx),
            pending_play,
            armed: None,
            tap: None,
            meta,
            now_playing: None,
            position: Position {
                track: 0,
                frame: 0,
                sample_rate: 0,
                state: PlayState::Stopped,
                discontinuity: false,
            },
            seek_drag: None,
            lp_knob: audio::filter::OFF,
            title_offset: 0,
            title_tick: 0.0,
            pl_scroll: 0,
            pl_drag_from: None,
            pl_drag_block: None,
            pl_drag_track: None,
            pl_open: HashMap::new(),
            pl_open_rev: 0,
            pl_rows_cache: RefCell::new(None),
            pl_tint: Vec::new(),
            bpm_drag: None,
            side_crate: None,
            #[cfg(test)]
            bpm_slider: None,
            facet_cache: [None, None, None, None],
            facet_search: Default::default(),
            discogs_hint_shown: false,
            #[cfg(test)]
            filters_drawn: None,
            #[cfg(test)]
            cart_drawn: None,
            pl_resize_acc: egui::Vec2::ZERO,
            pl_search: String::new(),
            search_focus: false,
            panel_tab: Facet::Style,
            pl_scroll_acc: 0.0,
            win_px: egui::Vec2::ZERO,
            max_seen: false,
            focus: Focus::Player,
            pl_follow: None,
            width_fitted: false,
            name_dialog: None,
            confirm_delete: None,
            fullscreen: None,
            scene: ctx.scene,
            scene_ready: false,
            render_state,
            last_frame: Instant::now(),
            startup: ctx.startup,
            first_frame_done: false,
            profile: std::env::var_os("DIGGR_FRAME_STATS").map(|_| FrameProfile::new()),
            auto_fullscreen: std::env::var_os("DIGGR_AUTO_FULLSCREEN").is_some(),
            auto_quit: std::env::var("DIGGR_AUTO_QUIT_SECS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .map(|s| Instant::now() + Duration::from_secs(s)),
            settings_dirty: None,
            last_size: None,
            message,
            flash: None,
            analysis: ctx.analysis,
            files: ctx.files.clone(),
            track_refs: std::collections::HashMap::new(),
            score: None,
            bpm_track: None,
            strip: false,
            annotating: None,
            annotations_dir: ctx.annotations_dir,
            help: false,
            overviews: ctx.overviews,
            overview: None,
            nav: Nav::default(),
            spectro,
            spectro_open: false,
            #[cfg(not(target_arch = "wasm32"))]
            dig,
        }
    }

    /// Window size in points for the current settings.
    pub fn window_size(settings: &Settings, skin: &LoadedSkin) -> egui::Vec2 {
        let (w, h) = crate::layout::window_size(settings, &skin.def);
        vec2(w as f32, h as f32) * settings.scale as f32
    }

    fn engine(&mut self) -> Option<&mut Engine> {
        match &mut self.engine {
            EngineSlot::Ready(e) => Some(e),
            _ => None,
        }
    }

    fn mark_settings(&mut self) {
        self.settings_dirty.get_or_insert_with(Instant::now);
    }

    /// A crate's entries changed: save it soon, and rebuild the engine queue if it is playing.
    fn mark_crate(&mut self, id: CrateId) {
        self.crates.touch(id);
        if id == self.crates.playing_id() {
            self.queue_dirty = true;
        }
    }

    /// A tempo is known for this audio: every entry with the same audio (the same clip, or the
    /// same file), in every loaded crate, shows it.
    fn set_track_bpm(&mut self, track: &TrackRef, raw_bpm: f64) {
        let Some(bpm) = format::dj_bpm(raw_bpm) else {
            return;
        };
        let ids = self.crates.loaded_ids();
        let keys: std::collections::HashSet<_> = ids
            .iter()
            .filter_map(|&c| self.crates.get(c))
            .flat_map(|p| p.entries().iter().filter(|e| e.track == *track))
            .map(crate::playlist::Entry::duplicate_key)
            .collect();
        for c in ids {
            let changed = self
                .crates
                .get_mut(c)
                .is_some_and(|p| keys.iter().fold(false, |ch, k| p.set_bpm(k, bpm) | ch));
            // Saved soon. The play order changes only when a BPM filter decides what plays.
            if changed {
                let filtered = self.crates.get(c).is_some_and(|p| p.is_filtered());
                if filtered {
                    self.mark_crate(c);
                } else {
                    self.crates.touch(c);
                }
            }
        }
    }

    /// Once the playing track's score is whole (at once for a cached one), its entries show
    /// its tempo. Done once per track.
    fn show_playing_bpm(&mut self) {
        if self.position.state == PlayState::Stopped {
            return;
        }
        let Some(track) = self.track_refs.get(&self.position.track) else {
            return;
        };
        if self.bpm_track.as_ref() == Some(track) {
            return;
        }
        let Some(bpm) = self
            .score
            .as_ref()
            .filter(|s| s.complete)
            .map(|s| s.dominant_bpm())
        else {
            return;
        };
        let track = track.clone();
        self.bpm_track = Some(track.clone());
        if let Some(bpm) = bpm {
            self.set_track_bpm(&track, bpm);
        }
    }

    fn mark_shown(&mut self) {
        self.mark_crate(self.crates.shown_id());
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), Instant::now()));
    }

    /// Shows `verdict` on the title line for a moment, for `count` records.
    fn flash(&mut self, verdict: Verdict, count: usize) {
        if count > 0 {
            self.flash = Some((flash_text(verdict, count), Instant::now()));
        }
    }

    // ---- engine glue ---------------------------------------------------------------------

    fn poll_engine_start(&mut self) {
        let EngineSlot::Starting(rx) = &self.engine else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(mut engine)) => {
                let s = &self.settings;
                engine.set_volume(s.volume);
                engine.set_eq(s.eq);
                engine.set_repeat(s.repeat.to_engine());
                engine.set_av_offset_ms(s.av_offset_ms);
                self.tap = engine.take_tap();
                self.analyzer.set_rate(engine.stats().format.sample_rate);
                self.engine = EngineSlot::Ready(Box::new(engine));
                self.queue_dirty = true;
            }
            Ok(Err(e)) => self.engine = EngineSlot::Failed(e),
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(_) => self.engine = EngineSlot::Failed("audio engine did not start".into()),
        }
    }

    /// Rebuilds the engine queue from the playing crate (and shuffle order).
    fn sync_queue(&mut self, first: Option<EntryId>) {
        let playing = self.crates.playing();
        let queue = playing.queue(
            self.settings.shuffle,
            first.or(playing.current()),
            self.shuffle_seed,
        );
        let tracks: Vec<TrackRef>;
        (self.queue, tracks) = queue.into_iter().unzip();
        self.queue_crate = self.crates.playing_id();
        self.queue_dirty = false;
        if let EngineSlot::Ready(e) = &mut self.engine {
            e.set_queue(tracks);
        }
    }

    /// Starts an entry, which makes its crate the playing one. A waiting entry is armed
    /// instead: it starts when its audio arrives, and the current track plays on meanwhile.
    fn play_entry(&mut self, crate_id: CrateId, id: EntryId) {
        let Some(entry) = self.crates.get(crate_id).and_then(|p| p.get(id)) else {
            return;
        };
        match &entry.status {
            EntryStatus::Waiting(_) => {
                self.armed = Some((crate_id, id));
                return;
            }
            EntryStatus::Unavailable(reason) => {
                let text = format!("{}: {reason}", entry.display_name());
                self.notify(text);
                return;
            }
            _ => {}
        }
        if self.engine().is_none() {
            self.pending_play = Some(Some((crate_id, id)));
            return;
        }
        self.armed = None;
        if self.crates.playing_id() != crate_id && self.crates.set_playing(crate_id) {
            self.queue_dirty = true;
        }
        // In shuffle mode the chosen track starts a fresh shuffled order.
        if self.queue_dirty || self.settings.shuffle || !self.queue.contains(&id) {
            self.shuffle_seed = self
                .shuffle_seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1);
            self.sync_queue(Some(id));
        }
        if let Some(q) = self.queue.iter().position(|&x| x == id) {
            self.crates.playing_mut().set_current(Some(id));
            if let Some(e) = self.engine() {
                e.play_index(q);
            }
        }
    }

    /// Play: resumes when paused, restarts the playing track, and when stopped starts the
    /// shown crate (at its current entry).
    fn play(&mut self) {
        if self.crates.shown().is_empty() {
            self.open_files_dialog(Open::AddAndPlay);
            return;
        }
        let Some(e) = self.engine() else {
            self.pending_play = Some(None);
            return;
        };
        match e.state() {
            PlayState::Paused => e.resume(),
            PlayState::Playing => {
                if let Some(id) = self.crates.playing().current() {
                    self.play_entry(self.crates.playing_id(), id);
                }
            }
            PlayState::Stopped => {
                let shown = self.crates.shown();
                let id = shown
                    .current()
                    .or_else(|| shown.queue(false, None, 0).first().map(|(id, _)| *id));
                if let Some(id) = id {
                    self.play_entry(self.crates.shown_id(), id);
                }
            }
        }
    }

    fn with_engine(&mut self, f: impl FnOnce(&mut Engine)) {
        if let Some(e) = self.engine() {
            f(e);
        }
    }

    // ---- structure navigation ----------------------------------------------------------

    /// Jumps to `target(score, now)`, on the next downbeat when the beat grid allows.
    fn structure_jump(&mut self, target: fn(&analysis::SongScore, f64) -> Option<f64>) {
        let Some(score) = self.score.clone() else {
            self.message = Some(("Not analyzed yet".into(), Instant::now()));
            return;
        };
        let now = self.position.seconds();
        let Some(t) = target(&score, now) else { return };
        let track = self.position.track;
        if self.nav.loop_region.is_some() {
            self.with_engine(|e| e.set_loop(track, None));
        }
        let at = crate::navigation::downbeats_after(&score, now, crate::navigation::CANDIDATES);
        if crate::navigation::grid_ok(&score, now) && !at.is_empty() {
            self.nav.jump_target = Some(t);
            self.with_engine(|e| e.seek_at(track, at, t));
        } else {
            self.with_engine(|e| e.seek(t));
        }
    }

    fn set_loop(&mut self, region: Option<(f64, f64)>) {
        let track = self.position.track;
        self.with_engine(|e| e.set_loop(track, region));
    }

    /// `L`: loop the current section (at most 32 bars), or stop looping.
    fn section_loop(&mut self) {
        if self.nav.loop_region.is_some() {
            self.set_loop(None);
            return;
        }
        let now = self.position.seconds();
        match self
            .score
            .as_deref()
            .and_then(|s| crate::navigation::section_loop(s, now))
        {
            Some(region) => self.set_loop(Some(region)),
            None => self.message = Some(("Not analyzed yet".into(), Instant::now())),
        }
    }

    /// `Shift+L`: loop 4, 8, 16 bars from the current downbeat (each press doubles).
    fn bar_loop(&mut self) {
        let bars = crate::navigation::next_loop_bars(self.nav.loop_bars);
        let now = self.position.seconds();
        match self
            .score
            .as_deref()
            .and_then(|s| crate::navigation::bar_loop(s, now, bars))
        {
            Some(region) => {
                self.nav.loop_bars = Some(bars);
                self.set_loop(Some(region));
            }
            None => self.message = Some(("Not analyzed yet".into(), Instant::now())),
        }
    }

    fn waveform_section(&mut self, ui: &mut Ui, rect: Rect) {
        let drops = self
            .score
            .as_deref()
            .map(crate::navigation::rise_marks)
            .unwrap_or_default();
        let overview = self.overview.clone();
        let duration = self
            .now_playing
            .as_ref()
            .and_then(|i| i.duration_secs)
            .or(overview
                .as_ref()
                .filter(|o| o.complete)
                .map(|o| o.seconds()));
        let playing = self.position.state != PlayState::Stopped;
        let view = crate::waveform::View {
            overview: overview.as_deref().filter(|_| playing),
            score: self.score.as_deref(),
            now: self.position.seconds(),
            duration: duration.filter(|_| playing),
            zoom_bars: self.settings.waveform_bars,
            loop_region: self.nav.loop_region,
            pending_jump: self.nav.pending_jump,
            drops: &drops,
        };
        match crate::waveform::draw(ui, rect, self.settings.scale as f32, &view) {
            Some(crate::waveform::Action::Seek(t)) => self.with_engine(|e| e.seek(t)),
            Some(crate::waveform::Action::Zoom(bars)) => {
                self.settings.waveform_bars = bars;
                self.mark_settings();
            }
            None => {}
        }
    }

    fn update_audio(&mut self, dt: f32) {
        self.poll_engine_start();
        if self.queue_dirty && self.engine().is_some() {
            self.sync_queue(None);
        }
        if let Some(p) = self.pending_play.take() {
            if self.engine().is_some() {
                match p {
                    Some((c, id)) => self.play_entry(c, id),
                    None => self.play(),
                }
            } else {
                self.pending_play = Some(p);
            }
        }
        if let Some(meta) = &self.meta {
            meta.request(self.crates.take_pending_meta());
            for r in meta.poll() {
                let (c, id) = match r {
                    MetaResult::Info(key, _)
                    | MetaResult::Failed(key)
                    | MetaResult::Bpm(key, _) => key,
                };
                let Some(playlist) = self.crates.get_mut(c) else {
                    continue; // deleted meanwhile
                };
                match r {
                    MetaResult::Info(_, i) => playlist.set_info(id, i),
                    MetaResult::Failed(_) => {
                        if playlist.set_failed(id) {
                            self.mark_crate(c);
                        }
                    }
                    MetaResult::Bpm(_, bpm) => {
                        if let Some(key) = playlist.get(id).map(|e| e.duplicate_key())
                            && let Some(bpm) = format::dj_bpm(bpm)
                        {
                            playlist.set_bpm(&key, bpm);
                        }
                    }
                }
                // A tempo may decide what plays under a BPM filter.
                if self.crates.get(c).is_some_and(|p| p.is_filtered()) {
                    self.mark_crate(c);
                } else {
                    self.crates.touch(c);
                }
            }
        }
        let messages = self.crates.take_messages();
        if !messages.is_empty() {
            self.notify(messages.join("\n"));
        }
        self.check_armed();
        let queue = self.queue.clone();
        let queue_crate = self.queue_crate;
        let mut requeued = None;
        let EngineSlot::Ready(engine) = &mut self.engine else {
            return;
        };
        for ev in engine.poll_events() {
            match ev {
                EngineEvent::TrackInfo { index, info, .. } => {
                    if let (Some(&id), Some(p)) =
                        (queue.get(index), self.crates.get_mut(queue_crate))
                    {
                        p.set_info(id, info.clone());
                    }
                }
                EngineEvent::TrackFailed { index, .. } => {
                    if let (Some(&id), Some(p)) =
                        (queue.get(index), self.crates.get_mut(queue_crate))
                        && p.set_failed(id)
                    {
                        requeued = Some(id);
                    }
                }
                EngineEvent::TrackLoaded { id, track, .. } => {
                    // Keep the recent instances only (ids grow monotonically).
                    self.track_refs.retain(|&k, _| k + 256 > id);
                    self.track_refs.insert(id, track);
                }
                EngineEvent::JumpScheduled { at_secs, .. } => self.nav.pending_jump = Some(at_secs),
                EngineEvent::JumpMissed { .. } => {
                    // Too late to quantize: jump now rather than not at all.
                    self.nav.pending_jump = None;
                    if let Some(t) = self.nav.jump_target.take() {
                        engine.seek(t);
                    }
                }
                EngineEvent::LoopChanged { region, .. } => {
                    self.nav.loop_region = region;
                    if region.is_none() {
                        self.nav.loop_bars = None;
                    }
                }
                EngineEvent::LoopRejected { .. } => {
                    self.nav.loop_bars = None;
                    self.message = Some(("Too late to loop there".into(), Instant::now()));
                }
                EngineEvent::PreWarm { track, .. } => {
                    if let Some(a) = &self.analysis {
                        a.prewarm(&track);
                    }
                    if let Some(o) = &self.overviews {
                        o.request(&track);
                    }
                }
            }
        }
        let live = self.spectro_open && self.spectro.live_mode();
        let mut chunks = Vec::new();
        if let Some(tap) = &mut self.tap {
            while let Some(chunk) = tap.pop() {
                self.analyzer.push(&chunk);
                if live {
                    chunks.push(chunk);
                }
            }
        }
        if live {
            self.spectro
                .push_tap(&chunks, engine.stats().format.sample_rate);
        }
        self.position = engine.position();
        if self.position.discontinuity {
            self.analyzer.set_rate(engine.stats().format.sample_rate);
        }
        self.analyzer.update(&self.position, dt);
        self.now_playing = engine.track_info(self.position.track).cloned();
        // Drive the analyzer with the audible position and pick up its latest score.
        self.score = None;
        if self.position.state != PlayState::Stopped
            && let (Some(a), Some(track)) =
                (&self.analysis, self.track_refs.get(&self.position.track))
        {
            a.playhead(track, self.position.seconds());
            self.score = a.score(track);
        }
        // The overview is built only once the analyzer has its first results, so it never
        // competes with playback start or the playhead analysis.
        self.overview = None;
        if self.position.state != PlayState::Stopped
            && let (Some(ov), Some(track)) =
                (&self.overviews, self.track_refs.get(&self.position.track))
        {
            if self.score.is_some() {
                ov.request(track);
            }
            self.overview = ov.get(track);
        }
        // A scheduled jump has happened once its landing point is audible.
        if let Some(at) = self.nav.pending_jump
            && (self.position.discontinuity || self.position.seconds() >= at + 0.25)
        {
            self.nav.pending_jump = None;
            self.nav.jump_target = None;
        }
        if self.position.state != PlayState::Stopped
            && let Some(&id) = engine.current_index().and_then(|q| queue.get(q))
            && let Some(p) = self.crates.get_mut(queue_crate)
        {
            p.set_current(Some(id));
        }
        // A track that was to play but whose preview is gone plays once downloaded again.
        if let Some(id) = requeued {
            self.armed = Some((queue_crate, id));
            self.mark_crate(queue_crate);
        }
    }

    // ---- entries waiting for their audio ---------------------------------------------------

    /// An audio producer delivered a waiting entry's file: it becomes playable, joins the
    /// queue if its crate is playing, and starts at once if it was armed.
    pub fn set_audio(&mut self, crate_id: CrateId, id: EntryId, track: TrackRef) {
        let Some(p) = self.crates.get_mut(crate_id) else {
            return;
        };
        if !p.set_audio(id, track.clone()) {
            return;
        }
        if let Some(m) = &self.meta {
            m.request(vec![((crate_id, id), track)]);
        }
        self.mark_crate(crate_id);
        self.check_armed();
    }

    /// Starts the armed entry once it can play, and forgets it if it never will.
    fn check_armed(&mut self) {
        let Some((c, id)) = self.armed else {
            return;
        };
        let status = self
            .crates
            .get(c)
            .and_then(|p| p.get(id))
            .map(|e| &e.status);
        let (waiting, playable) = (
            matches!(status, Some(EntryStatus::Waiting(_))),
            status.is_some_and(EntryStatus::is_playable),
        );
        if playable && matches!(self.engine, EngineSlot::Ready(_)) {
            self.play_entry(c, id);
        } else if !waiting && !playable {
            self.armed = None; // removed, or it will never play
        }
    }

    /// "Waiting for ‹title› (downloading 40%)" while an entry is armed.
    /// Digging progress for the status line.
    fn dig_line(&self) -> Option<String> {
        #[cfg(not(target_arch = "wasm32"))]
        return self.dig.as_ref().and_then(|d| d.line());
        #[cfg(target_arch = "wasm32")]
        None
    }

    /// Kept, passed and wantlist-pending marks for a playlist row.
    /// "this pressing", or "another pressing (AF014, 2018)", when the entry's record is in
    /// the user's collection.
    /// What of the entry's record the user owns: "this pressing", "another pressing (…)".
    fn dig_owned(&self, e: &crate::playlist::Entry) -> Option<String> {
        #[cfg(not(target_arch = "wasm32"))]
        return self.dig.as_ref()?.owned(e).map(|o| digging::owned_text(&o));
        #[cfg(target_arch = "wasm32")]
        {
            let _ = e;
            None
        }
    }

    /// An entry's dig marks in the shown crate. In the collection's own crate (or entries
    /// sent from the collection page) everything is owned, so no OWNED badge there.
    fn dig_marks(&self, e: &crate::playlist::Entry) -> format::DigMarks {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(d) = &self.dig else {
                return format::DigMarks::default();
            };
            let m = d.marks(e);
            let from_collection = e.origin.as_ref().is_some_and(|o| {
                ::dig::discogs::url::parse(&o.page)
                    .is_ok_and(|p| matches!(p.kind, ::dig::discogs::url::PageKind::Collection(_)))
            });
            let badge = !from_collection && !self.crates.is_collection(self.crates.shown_id());
            format::DigMarks {
                wanted: m.wanted,
                passed: m.passed,
                wantlist_pending: m.wantlist_pending,
                wantlist_failed: m.wantlist_failed,
                collection_failed: m.collection_failed,
                discard_pending: m.discard_pending,
                discard_failed: m.discard_failed,
                owned: if badge { self.dig_owned(e) } else { None },
                cart: e
                    .origin
                    .as_ref()
                    .and_then(|o| o.release)
                    .and_then(|r| d.cart.for_release(r))
                    .map(|i| format!("from {}, {}", i.seller, i.price)),
                copies: e.origin.as_ref().and_then(|o| o.release).and_then(|r| {
                    let copies = self.crates.shown().copies_of(r);
                    let text: Vec<String> = copies
                        .iter()
                        .map(|c| {
                            let sold = if c.sold { " (sold)" } else { "" };
                            format!("{} {}{sold}", format::price(c.cents, &c.currency), c.grades)
                        })
                        .collect();
                    (!text.is_empty()).then(|| text.join(", "))
                }),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = e;
            format::DigMarks::default()
        }
    }

    fn armed_line(&self) -> Option<String> {
        let (c, id) = self.armed?;
        let e = self.crates.get(c)?.get(id)?;
        Some(match e.status.note() {
            Some(note) => format!("Waiting for {} ({note})", e.display_name()),
            None => format!("Waiting for {}", e.display_name()),
        })
    }

    // ---- files -----------------------------------------------------------------------------

    /// Adds music to the shown crate (ADD, drag-and-drop, Cmd+O, M3U import), or replaces the
    /// Playlist crate (Eject). Adding to an empty crate plays it if nothing else is playing.
    fn add_paths(&mut self, paths: Vec<PathBuf>, open: Open) {
        let found: Vec<TrackRef> = files::expand(&paths)
            .into_iter()
            .map(|p| TrackRef::new(p.to_string_lossy()))
            .collect();
        if found.is_empty() {
            return;
        }
        if open != Open::Replace && self.refuse_discogs_insert(self.crates.shown_id()) {
            return;
        }
        let (crate_id, added, play) = if open == Open::Replace {
            let added = self.crates.replace_playlist(found);
            self.queue_dirty = true;
            (PLAYLIST, added, true)
        } else {
            let crate_id = self.crates.shown_id();
            let was_empty = self.crates.shown().is_empty();
            let added = self.crates.shown_mut().add(found);
            let added: Vec<_> = added.into_iter().map(|(e, t)| ((crate_id, e), t)).collect();
            self.mark_shown();
            let idle = self.position.state == PlayState::Stopped;
            (
                crate_id,
                added,
                open == Open::AddAndPlay || (was_empty && idle),
            )
        };
        let first = added.first().map(|((_, id), _)| *id);
        if let Some(m) = &self.meta {
            m.request(added);
        }
        if play && let Some(id) = first {
            self.play_entry(crate_id, id);
        }
    }

    fn open_files_dialog(&mut self, open: Open) {
        let exts: Vec<&str> = files::AUDIO_EXTENSIONS
            .iter()
            .copied()
            .chain(["m3u", "m3u8"])
            .collect();
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("Audio", &exts)
            .pick_files()
        {
            self.add_paths(paths, open);
        }
    }

    fn open_folder_dialog(&mut self) {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.add_paths(vec![dir], Open::Add);
        }
    }

    fn export_m3u(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Playlist", &["m3u8", "m3u"])
            .set_file_name(format!("{}.m3u8", self.crates.name(self.crates.shown_id())))
            .save_file()
        else {
            return;
        };
        let entries = files::m3u_entries(self.crates.shown());
        match std::fs::write(&path, files::write_m3u(&entries)) {
            Ok(()) => self.notify(format!("Saved {}", path.display())),
            Err(e) => self.notify(format!("Could not save: {e}")),
        }
    }

    // ---- keyboard ----------------------------------------------------------------------------

    fn handle_keys(&mut self, ctx: &egui::Context) {
        // Tab switches between the player and the playlist. egui would otherwise move its
        // widget focus, and a focused widget holds the keyboard until Esc.
        if !ctx.text_edit_focused() && ctx.input(|i| i.key_pressed(Key::Tab) && !i.modifiers.any())
        {
            ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
            if self.fullscreen.is_none() && self.settings.show_playlist {
                self.focus = self.focus.other();
            }
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        // A question is open (keeping a record already owned): its own keys only.
        #[cfg(not(target_arch = "wasm32"))]
        if self.dig_asking() {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        for text in ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Paste(t) => Some(t.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        }) {
            self.dig_paste(&text);
        }
        let (pressed, mods): (Vec<Key>, Modifiers) = ctx.input(|i| {
            let keys = i
                .events
                .iter()
                .filter_map(|e| match e {
                    // Brackets by position (right of P), so they work on any keyboard layout.
                    egui::Event::Key {
                        key,
                        physical_key,
                        pressed: true,
                        ..
                    } => Some(match physical_key {
                        Some(k @ (Key::OpenBracket | Key::CloseBracket)) => *k,
                        _ => *key,
                    }),
                    _ => None,
                })
                .collect();
            (keys, i.modifiers)
        });
        for key in pressed {
            // Cmd+F (Ctrl+F) finds in the shown crate, never fullscreen.
            if key == Key::F && mods.command {
                if self.fullscreen.is_none() && self.settings.show_playlist {
                    self.search_focus = true;
                }
                continue;
            }
            // The help panel takes its own close keys first (Esc must not leave fullscreen).
            if matches!(key, Key::H | Key::F1) && !mods.command {
                self.help = !self.help;
                continue;
            }
            if self.help && key == Key::Escape {
                self.help = false;
                continue;
            }
            if !self.transport_key(ctx, key, mods) {
                if self.fullscreen.is_some() {
                    if !self.host_key(key) {
                        self.scene.key(key, mods);
                    }
                } else {
                    self.window_key(ctx, key, mods);
                }
            }
        }
    }

    /// Keys the fullscreen host keeps for itself (see [`host_action`]); others go to the scene.
    fn host_key(&mut self, key: Key) -> bool {
        let Some(action) = host_action(key, self.annotating.is_some()) else {
            return false;
        };
        let t = self.position.seconds();
        match action {
            HostAction::ToggleStrip => self.strip = !self.strip,
            HostAction::ToggleAnnotating => self.toggle_annotating(),
            HostAction::Tap => {
                if let Some(a) = &mut self.annotating {
                    a.tap(t);
                }
                self.save_annotations();
            }
            HostAction::Boundary(kind) => {
                if let Some(a) = &mut self.annotating {
                    a.boundary(t, kind);
                }
                self.save_annotations();
            }
        }
        true
    }

    fn toggle_annotating(&mut self) {
        if self.annotating.take().is_some() {
            return; // every mark is saved as it is made
        }
        let Some(track) = self.track_refs.get(&self.position.track).cloned() else {
            self.notify("Play a track to annotate it");
            return;
        };
        let Some(dir) = self.annotations_dir.clone() else {
            self.notify("Annotations need a cache directory");
            return;
        };
        let hash = self
            .score
            .as_ref()
            .map(|s| s.content_hash)
            .filter(|&h| h != 0)
            .or_else(|| analysis::cache::content_hash(&*self.files, &track))
            .unwrap_or(0);
        self.annotating = Some(analysis::eval::Annotations::load_or_new(&dir, &track, hash));
        self.strip = true;
    }

    fn save_annotations(&mut self) {
        if let (Some(ann), Some(dir)) = (&self.annotating, &self.annotations_dir)
            && let Err(e) = ann.save(dir)
        {
            self.message = Some((format!("Could not save annotations: {e}"), Instant::now()));
        }
    }

    /// Keys that work in both windowed and fullscreen mode.
    fn transport_key(&mut self, ctx: &egui::Context, key: Key, mods: Modifiers) -> bool {
        if mods.command {
            return false;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.dig_key(key) {
            return true;
        }
        let secs = self.position.seconds();
        match key {
            Key::Z => self.with_engine(|e| e.previous()),
            Key::X => self.play(),
            Key::C => self.with_engine(|e| e.toggle_pause()),
            Key::V => self.with_engine(|e| e.stop()),
            Key::B => self.with_engine(|e| e.next()),
            Key::ArrowLeft => self.with_engine(|e| e.seek((secs - 5.0).max(0.0))),
            Key::ArrowRight => self.with_engine(|e| e.seek(secs + 5.0)),
            // With the playlist focused, the arrows move its cursor instead (`window_key`).
            Key::ArrowUp | Key::ArrowDown if self.arrows_move_cursor() => return false,
            Key::ArrowUp => self.set_volume(self.settings.volume + 0.05),
            Key::ArrowDown => self.set_volume(self.settings.volume - 0.05),
            Key::F => self.toggle_fullscreen(ctx),
            Key::Escape if self.fullscreen.is_some() => self.toggle_fullscreen(ctx),
            Key::CloseBracket if mods.shift => self.structure_jump(crate::navigation::next_rise),
            Key::CloseBracket => self.structure_jump(crate::navigation::next_section),
            Key::OpenBracket => self.structure_jump(crate::navigation::previous_section),
            Key::L if mods.shift => self.bar_loop(),
            Key::L => self.section_loop(),
            _ => return false,
        }
        true
    }

    fn window_key(&mut self, ctx: &egui::Context, key: Key, mods: Modifiers) {
        match key {
            Key::W if !mods.command => self.apply(Action::ToggleWaveform, ctx),
            Key::S if !mods.command => self.spectro_open = !self.spectro_open,
            // After a click on a crate in the sidebar, Delete deletes that crate (asking first
            // when it has entries); otherwise it removes the selected entries.
            Key::Delete | Key::Backspace => match self.side_crate.take() {
                Some(PLAYLIST) => self.notify("The Playlist crate can't be deleted"),
                Some(c) if self.pl_sidebar() => self.apply(Action::DeleteCrate(c), ctx),
                _ => self.remove_selected(),
            },
            Key::O if mods.command => self.open_files_dialog(Open::Add),
            Key::A if mods.command => self.crates.shown_mut().select_all(),
            Key::Enter => {
                // On a closed record row, the record plays.
                if let Some(ListRow::Record(rec)) = self.pl_cursor_row()
                    && !rec.single
                    && !rec.open
                {
                    let shown = self.crates.shown();
                    let ids = rec.members.iter().map(|&i| shown.entries()[i].id).collect();
                    return self.apply(Action::PlayRecord(ids), ctx);
                }
                let shown = self.crates.shown();
                if let Some(id) = shown
                    .cursor()
                    .or_else(|| shown.selected_ids().first().copied())
                {
                    self.play_entry(self.crates.shown_id(), id);
                }
            }
            // Space opens or closes the record under the cursor (grouped): its record row, or
            // one of its tracks while it's open.
            Key::Space if self.arrows_move_cursor() => match self.pl_cursor_row() {
                Some(ListRow::Record(rec)) if !rec.single => {
                    self.apply(Action::ToggleRecord(rec.key), ctx);
                }
                Some(ListRow::Entry { idx, track: true }) => {
                    if let Some(key) = self.crates.shown().entries()[idx].album_key() {
                        self.apply(Action::ToggleRecord(key), ctx);
                    }
                }
                _ => {}
            },
            Key::P if mods.shift && !mods.command => self.apply(Action::ToggleMaximized, ctx),
            Key::P if !mods.command => self.show_playing_entry(),
            _ if !mods.command && self.arrows_move_cursor() => {
                let mv = match key {
                    Key::ArrowUp => CursorMove::Up,
                    Key::ArrowDown => CursorMove::Down,
                    Key::PageUp => CursorMove::PageUp,
                    Key::PageDown => CursorMove::PageDown,
                    Key::Home => CursorMove::Home,
                    Key::End => CursorMove::End,
                    _ => return,
                };
                let page = self.pl_visible_rows();
                let start = (self.crates.shown_id() == self.crates.playing_id()
                    && self.position.state != PlayState::Stopped)
                    .then(|| self.crates.shown().current())
                    .flatten();
                self.side_crate = None;
                if self.crates.shown().is_grouped() {
                    return self.move_row_cursor(mv, mods.shift, start);
                }
                let p = self.crates.shown_mut();
                p.move_cursor(mv, mods.shift, page, start);
                if let Some(i) = p.cursor_index() {
                    self.scroll_into_view(i);
                }
            }
            _ => {}
        }
    }

    /// The row the keyboard cursor is on (grouped: a closed record's row for any of its
    /// entries).
    fn pl_cursor_row(&self) -> Option<ListRow> {
        let i = self.crates.shown().cursor_index()?;
        let rows = self.pl_list();
        let r = records::row_of(&rows, i);
        rows.get(r)
            .filter(|row| row.indices().contains(&i))
            .cloned()
    }

    /// Grouped, the arrows move over the rows shown: a record row is one step, and selecting
    /// it selects its entries. Shift extends from the anchor by whole rows.
    fn move_row_cursor(&mut self, mv: CursorMove, extend: bool, start: Option<EntryId>) {
        let rows = self.pl_list();
        if rows.is_empty() {
            return;
        }
        let units = self.pl_visible_rows();
        let last = rows.len() - 1;
        let p = self.crates.shown();
        let row_of_id = |id: EntryId| p.index_of(id).map(|i| records::row_of(&rows, i));
        let at = p.cursor().and_then(row_of_id);
        let page = records::fit(&rows, self.pl_scroll, units).max(1);
        let to = match (at, mv) {
            (None, CursorMove::Up | CursorMove::Down) => start.and_then(row_of_id).unwrap_or(0),
            (at, _) => {
                let at = at.or_else(|| start.and_then(row_of_id)).unwrap_or(0);
                match mv {
                    CursorMove::Up => at.saturating_sub(1),
                    CursorMove::Down => (at + 1).min(last),
                    CursorMove::PageUp => at.saturating_sub(page),
                    CursorMove::PageDown => (at + page).min(last),
                    CursorMove::Home => 0,
                    CursorMove::End => last,
                }
            }
        };
        // Copy rows hold no audio: the cursor steps over them the way it was going.
        let down = matches!(
            mv,
            CursorMove::Down | CursorMove::PageDown | CursorMove::End
        );
        let mut to = to;
        while matches!(rows[to], ListRow::Copy { .. }) {
            match (down, to) {
                (true, t) if t < last => to += 1,
                (_, t) if t > 0 => to -= 1,
                _ => return,
            }
        }
        let ids: Vec<EntryId> = rows[to]
            .indices()
            .iter()
            .map(|&i| p.entries()[i].id)
            .collect();
        let first = rows[to].first();
        let last_idx = rows[to].indices().iter().copied().max().unwrap_or(first);
        let p = self.crates.shown_mut();
        if extend {
            p.extend_to(last_idx, &ids, ids[0]);
        } else {
            p.select_only(&ids, ids[0]);
        }
        self.scroll_into_view(first);
    }

    /// ↑/↓ move the playlist cursor while the playlist has the keyboard (not in fullscreen).
    fn arrows_move_cursor(&self) -> bool {
        self.focus == Focus::Playlist && self.fullscreen.is_none() && self.settings.show_playlist
    }

    /// Maximizes the window for the playlist (showing it, and giving it the keyboard), or
    /// restores the previous frame and layout.
    fn set_maximized(&mut self, ctx: &egui::Context, on: bool) {
        self.settings.playlist_maximized = on;
        if on {
            self.settings.show_playlist = true;
            self.focus = Focus::Playlist;
        }
        self.max_seen = false;
        self.last_size = None;
        ctx.send_viewport_cmd(ViewportCommand::Maximized(on));
        self.mark_settings();
    }

    /// The shown crate's rows (see [`records`]), rebuilt only when the crate, its filter, its
    /// grouping or its open records change.
    fn pl_list(&self) -> Rc<Vec<ListRow>> {
        let id = self.crates.shown_id();
        let p = self.crates.shown();
        let key = (id, p.rev(), p.is_grouped(), self.pl_open_rev);
        if let Some((k, rows)) = &*self.pl_rows_cache.borrow()
            && *k == key
        {
            return rows.clone();
        }
        let empty = HashSet::new();
        let rows = Rc::new(records::build(p, self.pl_open.get(&id).unwrap_or(&empty)));
        *self.pl_rows_cache.borrow_mut() = Some((key, rows.clone()));
        rows
    }

    /// Scrolls the playlist by the least amount that shows entry `index` (a crate index): its
    /// row, its closed record's row, or under a BPM filter where it would be.
    fn scroll_into_view(&mut self, index: usize) {
        let rows = self.pl_list();
        let row = records::row_of(&rows, index);
        self.pl_scroll = records::scroll_to(&rows, self.pl_scroll, row, self.pl_visible_rows());
    }

    /// The list row of crate index `index` (see [`Self::scroll_into_view`]).
    fn pl_row_of(&self, index: usize) -> usize {
        records::row_of(&self.pl_list(), index)
    }

    /// Whether list row `row` is in view.
    fn pl_row_in_view(&self, row: usize) -> bool {
        let rows = self.pl_list();
        row >= self.pl_scroll
            && row < self.pl_scroll + records::fit(&rows, self.pl_scroll, self.pl_visible_rows())
    }

    /// `P`: the playing crate, scrolled to its playing entry, with the cursor on it.
    /// A style, artist or label filter changed: saved soon, the play order follows, and the
    /// list keeps a row in view.
    fn filter_changed(&mut self) {
        self.mark_shown();
        let rows = self.pl_list().len();
        self.pl_scroll = self.pl_scroll.min(rows.saturating_sub(1));
    }

    fn show_playing_entry(&mut self) {
        let playing = self.crates.playing_id();
        let Some(id) = self.crates.playing().current() else {
            return;
        };
        if self.crates.shown_id() != playing {
            self.show_crate(playing);
        }
        // Filters that hide it are turned off.
        let p = self.crates.shown_mut();
        if p.get(id).is_some_and(|e| !p.shows(e)) {
            p.clear_filters();
            self.mark_shown();
        }
        let p = self.crates.shown_mut();
        p.set_cursor(Some(id));
        if let Some(i) = p.index_of(id) {
            self.scroll_into_view(i);
        }
    }

    fn remove_selected(&mut self) {
        let ids = self.crates.shown().selected_ids();
        self.remove_entries(&ids);
    }

    /// Removes entries of the shown crate (Remove, Remove album, Delete), except from the
    /// Discogs crates, which records leave only through the Discogs items, and label crates.
    fn remove_entries(&mut self, ids: &[EntryId]) {
        if self.refuse_discogs_edit(self.crates.shown_id()) {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.dig_before_remove(ids);
        if self.crates.shown_mut().remove_ids(ids) > 0 {
            self.mark_shown();
        }
    }

    /// The crates Send to crate offers: every other crate but the Discogs ones, which the
    /// Discogs items alone fill.
    fn send_targets(&self) -> Vec<(CrateId, String)> {
        self.crates
            .list()
            .iter()
            .filter(|c| c.id != self.crates.shown_id() && !self.crates.is_locked(c.id))
            .map(|c| (c.id, c.name.clone()))
            .collect()
    }

    /// Whether crate `c` is a Discogs crate, which only the Discogs items fill, or a label
    /// crate, which only its label fills; says how.
    fn refuse_discogs_insert(&mut self, c: CrateId) -> bool {
        if !self.crates.is_locked(c) {
            return false;
        }
        self.notify(if self.crates.is_label(c) {
            LABEL_CRATE_HINT
        } else if self.crates.is_wantlist(c) {
            "Records come into this crate with Add to wantlist (Y)"
        } else {
            "Records come into this crate with Add to collection"
        });
        true
    }

    /// Whether crate `c` is a Discogs crate or a label crate, which take no hand removals; the
    /// first refusal of the session says how records leave a Discogs crate, and every one says
    /// how a label crate works.
    fn refuse_discogs_edit(&mut self, c: CrateId) -> bool {
        if !self.crates.is_locked(c) {
            return false;
        }
        if self.crates.is_label(c) {
            self.notify(LABEL_CRATE_HINT);
        } else if !self.discogs_hint_shown {
            self.discogs_hint_shown = true;
            self.notify(if self.crates.is_wantlist(c) {
                "Records leave this crate with Remove from wantlist (Y)"
            } else {
                "Records leave this crate with Remove from collection…"
            });
        }
        true
    }

    fn set_volume(&mut self, v: f32) {
        self.settings.volume = v.clamp(0.0, 1.0);
        let v = self.settings.volume;
        self.with_engine(|e| e.set_volume(v));
        self.mark_settings();
    }

    // ---- fullscreen ------------------------------------------------------------------------

    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        match self.fullscreen.take() {
            Some(fs) => {
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
                ctx.send_viewport_cmd(ViewportCommand::CursorVisible(true));
                if self.settings.playlist_maximized {
                    ctx.send_viewport_cmd(ViewportCommand::Maximized(true));
                    self.max_seen = false;
                } else if let Some(r) = fs.restore {
                    ctx.send_viewport_cmd(ViewportCommand::InnerSize(Self::window_size(
                        &self.settings,
                        &self.skin,
                    )));
                    ctx.send_viewport_cmd(ViewportCommand::OuterPosition(r.min));
                }
                self.last_size = None;
            }
            None => {
                if !self.scene_ready
                    && let Some(rs) = &self.render_state
                {
                    self.scene.init(rs);
                    self.scene_ready = true;
                }
                self.scene.entered();
                let restore = ctx.input(|i| i.viewport().outer_rect);
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
                self.fullscreen = Some(Fullscreen {
                    restore,
                    last_pointer: Instant::now(),
                    pointer_at: None,
                });
            }
        }
    }

    fn fullscreen_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
        let (artist, title, duration) = self
            .now_playing_names()
            .unwrap_or_else(|| (String::new(), "Nothing playing".into(), None));
        let bars = *self.analyzer.bars();
        let frame = SceneFrame {
            position: self.position,
            bars: &bars,
            artist: &artist,
            title: &title,
            score: self.score.as_deref(),
            duration,
            strip_visible: self.strip || self.annotating.is_some(),
        };
        if let Some(cb) = self.scene.paint(rect, &frame) {
            ui.painter().add(cb);
        }
        self.scene.ui(ui, &frame);
        if self.strip || self.annotating.is_some() {
            crate::timeline::draw(
                ui,
                rect,
                &self.position,
                self.score.as_deref(),
                self.annotating.as_ref(),
            );
        }

        // Hide the cursor after 2 s without movement.
        if let Some(fs) = &mut self.fullscreen {
            let pointer = ctx.input(|i| i.pointer.latest_pos());
            if pointer != fs.pointer_at {
                fs.pointer_at = pointer;
                fs.last_pointer = Instant::now();
                ctx.send_viewport_cmd(ViewportCommand::CursorVisible(true));
            } else if fs.last_pointer.elapsed() > Duration::from_secs(2) {
                ctx.send_viewport_cmd(ViewportCommand::CursorVisible(false));
            }
        }
        ctx.request_repaint(); // one frame per vsync
    }

    // ---- sections ----------------------------------------------------------------------------

    /// A painter for a section; borrows only the (shared) skin definition, not the app.
    fn skinned<'a>(&self, def: &'a crate::skin::SkinDef, ui: &Ui, origin: Pos2) -> Skinned<'a> {
        Skinned {
            painter: ui.painter().clone(),
            tex: self.tex.as_ref().expect("texture loaded").id(),
            def,
            origin,
            scale: self.settings.scale as f32,
        }
    }

    fn titlebar_drag(ui: &mut Ui, sk: &Skinned, id: &str, layout: &str) {
        let resp = ui.interact(sk.at(layout), Id::new(id), Sense::click_and_drag());
        if resp.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
    }

    fn main_section(&mut self, ui: &mut Ui, origin: Pos2) {
        let state = self.position.state;
        let info = self.now_playing.clone();
        let duration = info.as_ref().and_then(|i| i.duration_secs);
        let elapsed = self.position.seconds();
        let mut actions: Vec<Action> = Vec::new();
        {
            let def = self.def.clone();
            let sk = self.skinned(&def, ui, origin);
            sk.sprite("main_bg", 0.0, 0.0);
            // Under every control: a right-click anywhere else opens Options.
            let (mw, mh) = def.main_size;
            self.options_menu(
                ui,
                sk.rect(0.0, 0.0, mw as f32, mh as f32),
                "main_options",
                &mut actions,
            );
            dim_title(&sk, "titlebar", self.focus == Focus::Player);
            Self::titlebar_drag(ui, &sk, "main_title", "titlebar");
            if widgets::button(ui, &sk, "min", "btn_min", "btn_min").clicked() {
                ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
            }
            if widgets::button(ui, &sk, "close", "btn_close", "btn_close").clicked() {
                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
            }

            // LCD: status, time, visualizer.
            let st = sk.def.at("status");
            let status = match state {
                PlayState::Playing => "status_play",
                PlayState::Paused => "status_pause",
                PlayState::Stopped => "status_stop",
            };
            sk.sprite(status, st.x as f32, st.y as f32);
            let t = sk.def.at("time");
            let show_time = state != PlayState::Stopped;
            let remaining = self.settings.time_remaining && duration.is_some();
            let secs = if remaining {
                (duration.unwrap_or(0.0) - elapsed).max(0.0)
            } else {
                elapsed
            };
            let (mm, ss) = format::lcd(secs);
            let (tx, ty) = (t.x as f32, t.y as f32);
            sk.sprite(
                if show_time && remaining {
                    "digit_minus"
                } else {
                    "digit_blank"
                },
                tx,
                ty,
            );
            let digit = |c: char| {
                if show_time {
                    format!("digit_{c}")
                } else {
                    "digit_blank".to_owned()
                }
            };
            let mins: Vec<char> = mm.chars().collect();
            for (i, c) in mins.iter().rev().take(3).enumerate() {
                sk.sprite(&digit(*c), tx + 21.0 - 10.0 * i as f32, ty);
            }
            if show_time {
                sk.sprite("digit_colon", tx + 31.0, ty);
            }
            for (i, c) in ss.chars().enumerate() {
                sk.sprite(&digit(c), tx + 35.0 + 10.0 * i as f32, ty);
            }
            if ui
                .interact(sk.at("time"), Id::new("time"), Sense::click())
                .clicked()
            {
                actions.push(Action::ToggleRemaining);
            }
            self.draw_vis(&sk);
            if ui
                .interact(sk.at("vis"), Id::new("vis"), Sense::click())
                .clicked()
            {
                actions.push(Action::CycleVis);
            }

            // Title, kbps, kHz, mono/stereo.
            let tt = sk.def.at("title_text");
            let lcd = color(sk.def.colors.lcd);
            let left = self
                .flash
                .as_ref()
                .and_then(|(_, at)| FLASH_SECS.checked_sub(at.elapsed()))
                .filter(|d| !d.is_zero());
            if let (Some(left), Some((text, _))) = (left, &self.flash) {
                // Centred and still; the track line comes back by itself.
                let x = tt.x as f32 + ((tt.w as f32 - sk.text_width(text)) / 2.0).round();
                sk.text(x, tt.y as f32, text, lcd);
                ui.ctx().request_repaint_after(left);
            } else {
                self.flash = None;
                let title = self
                    .now_playing_line()
                    .unwrap_or_else(|| engine_status(&self.engine));
                let width = (tt.w / sk.def.font.advance) as usize;
                let shown = format::scroll(&title, width, self.title_offset);
                sk.text(tt.x as f32, tt.y as f32, &shown, lcd);
            }
            if let Some(i) = &info {
                let k = sk.def.at("kbps");
                if let Some(kbps) = i.bitrate_kbps {
                    let s = kbps.min(999).to_string();
                    sk.text(
                        k.x as f32 + k.w as f32 - sk.text_width(&s),
                        k.y as f32,
                        &s,
                        lcd,
                    );
                }
                let k = sk.def.at("khz");
                let s = ((i.sample_rate as f32 / 1000.0).round() as u32).to_string();
                sk.text(
                    k.x as f32 + k.w as f32 - sk.text_width(&s),
                    k.y as f32,
                    &s,
                    lcd,
                );
            }
            let channels = info.as_ref().map_or(0, |i| i.channels);
            let m = sk.def.at("mono");
            sk.sprite(
                if channels == 1 { "mono_on" } else { "mono_off" },
                m.x as f32,
                m.y as f32,
            );
            let s = sk.def.at("stereo");
            sk.sprite(
                if channels >= 2 {
                    "stereo_on"
                } else {
                    "stereo_off"
                },
                s.x as f32,
                s.y as f32,
            );

            // Sliders.
            let vol = SliderSprites {
                track: "volume_track",
                fill: Some("volume_fill"),
                thumb: "volume_thumb",
            };
            if let (_, Some(v)) =
                widgets::hslider(ui, &sk, "volume", "volume", vol, self.settings.volume)
            {
                actions.push(Action::Volume(v));
            }
            // Where the classic player had balance: the waveform section, as with `W`.
            if widgets::toggle(
                ui,
                &sk,
                "wave_tog",
                "wave_toggle",
                "tog_wave",
                self.settings.show_waveform,
            )
            .clicked()
            {
                actions.push(Action::ToggleWaveform);
            }
            if widgets::toggle(
                ui,
                &sk,
                "eq_tog",
                "eq_toggle",
                "tog_eq",
                self.settings.show_eq,
            )
            .clicked()
            {
                actions.push(Action::ToggleEq);
            }
            if widgets::toggle(
                ui,
                &sk,
                "pl_tog",
                "pl_toggle",
                "tog_pl",
                self.settings.show_playlist,
            )
            .clicked()
            {
                actions.push(Action::TogglePlaylist);
            }
            let r = sk.def.at("seek");
            if let Some(d) = duration.filter(|d| *d > 0.0 && state != PlayState::Stopped) {
                let value = self.seek_drag.unwrap_or((elapsed / d) as f32);
                let seek = SliderSprites {
                    track: "seek_track",
                    fill: None,
                    thumb: "seek_thumb",
                };
                let (resp, new) = widgets::hslider(ui, &sk, "seek", "seek", seek, value);
                if let Some(v) = new {
                    self.seek_drag = Some(v);
                }
                if (resp.drag_stopped() || (resp.clicked() && new.is_some()))
                    && let Some(v) = self.seek_drag.take()
                {
                    actions.push(Action::Seek(v as f64 * d));
                }
            } else {
                sk.sprite("seek_track", r.x as f32, r.y as f32);
            }

            // Transport.
            for (name, action) in [
                ("prev", Action::Prev),
                ("play", Action::Play),
                ("pause", Action::Pause),
                ("stop", Action::Stop),
                ("next", Action::Next),
                ("eject", Action::Eject),
            ] {
                if widgets::button(ui, &sk, name, name, name).clicked() {
                    actions.push(action);
                }
            }
            let shuffle = if self.settings.shuffle {
                "shuffle_on"
            } else {
                "shuffle_off"
            };
            if widgets::button(ui, &sk, "shuffle", "shuffle", shuffle).clicked() {
                actions.push(Action::Shuffle);
            }
            let repeat = match self.settings.repeat {
                Repeat::Off => "repeat_off",
                Repeat::All => "repeat_all",
                Repeat::One => "repeat_one",
            };
            if widgets::button(ui, &sk, "repeat", "repeat", repeat).clicked() {
                actions.push(Action::Repeat);
            }
        }
        for a in actions {
            self.apply(a, ui.ctx());
        }
    }

    /// Artist, title and duration of what plays: the file's tags, except that an entry with
    /// an origin keeps its own artist and title (the record, not the file, is the truth).
    fn now_playing_names(&self) -> Option<(String, String, Option<f64>)> {
        let playing = self.crates.playing();
        let entry = playing.current().and_then(|id| playing.get(id));
        match (&self.now_playing, entry) {
            (Some(i), Some(e)) if e.origin.is_some() => Some((
                e.artist.clone(),
                e.title.clone(),
                i.duration_secs.or(e.duration),
            )),
            (Some(i), _) => Some((i.artist.clone(), i.title.clone(), i.duration_secs)),
            (None, Some(e)) => Some((e.artist.clone(), e.title.clone(), e.duration)),
            (None, None) => None,
        }
    }

    /// The main window's title line: `N. (catno) Artist: Title (N BPM) (m:ss)`, then the
    /// record's side, year and for-sale summary for an entry from a catalogue page.
    fn now_playing_line(&self) -> Option<String> {
        let (artist, title, duration) = self.now_playing_names()?;
        let playing = self.crates.playing();
        let number = playing.current_index().map_or(0, |i| i + 1);
        let entry = playing.current().and_then(|id| playing.get(id));
        let origin = entry.and_then(|e| e.origin.as_ref());
        let catno = origin.map_or("", |o| o.catno.as_str());
        let name = format::entry_name(catno, &artist, &title, "", entry.and_then(|e| e.bpm));
        let mut line = format::title_line(number, &name, duration);
        if let Some(o) = origin {
            line += &format::origin_details(o);
        }
        Some(line)
    }

    fn draw_vis(&self, sk: &Skinned) {
        let v = sk.def.at("vis");
        let c = &sk.def.colors;
        match self.settings.vis {
            VisMode::Off => {}
            VisMode::Spectrum => {
                let bars = self.analyzer.bars();
                let peaks = self.analyzer.peaks();
                let h = v.h as f32;
                for i in 0..BARS {
                    let x = v.x as f32 + i as f32 * 4.0;
                    let bh = (bars[i] * h).round();
                    for row in 0..bh as u32 {
                        let t = row as f32 / (h - 1.0);
                        let col = lerp_color(c.vis_bar_low, c.vis_bar_high, t);
                        sk.fill(sk.rect(x, v.y as f32 + h - 1.0 - row as f32, 3.0, 1.0), col);
                    }
                    let py = (peaks[i] * h).round();
                    if py >= 1.0 {
                        sk.fill(sk.rect(x, v.y as f32 + h - py, 3.0, 1.0), color(c.vis_peak));
                    }
                }
            }
            VisMode::Scope => {
                let samples = self.analyzer.scope(&self.position, v.w as usize);
                let mid = v.y as f32 + v.h as f32 / 2.0;
                for (i, s) in samples.iter().enumerate() {
                    let y = (mid + s.clamp(-1.0, 1.0) * (v.h as f32 / 2.0 - 1.0)).round();
                    sk.fill(
                        sk.rect(v.x as f32 + i as f32, y, 1.0, 1.0),
                        color(c.vis_scope),
                    );
                }
            }
        }
    }

    /// The waveform's title bar in the player column: drags the window like the others, and
    /// its close button hides the waveform.
    fn wave_title(&mut self, ui: &mut Ui, origin: Pos2) {
        let def = self.def.clone();
        let sk = self.skinned(&def, ui, origin);
        sk.sprite("wave_title", 0.0, 0.0);
        dim_title(&sk, "wave_titlebar", self.focus == Focus::Player);
        Self::titlebar_drag(ui, &sk, "wave_title", "wave_titlebar");
        if widgets::button(ui, &sk, "wave_close", "wave_close", "btn_close").clicked() {
            self.apply(Action::ToggleWaveform, ui.ctx());
        }
    }

    fn eq_section(&mut self, ui: &mut Ui, origin: Pos2) {
        let mut eq = self.settings.eq;
        let mut lp = self.lp_knob;
        let mut changed = false;
        let mut actions = Vec::new();
        {
            let def = self.def.clone();
            let sk = self.skinned(&def, ui, origin);
            sk.sprite("eq_bg", 0.0, 0.0);
            dim_title(&sk, "eq_titlebar", self.focus == Focus::Player);
            Self::titlebar_drag(ui, &sk, "eq_title", "eq_titlebar");
            if widgets::button(ui, &sk, "eq_close", "eq_close", "btn_close").clicked() {
                actions.push(Action::ToggleEq);
            }
            if widgets::toggle(ui, &sk, "eq_on", "eq_on", "eq_on", eq.enabled).clicked() {
                eq.enabled = !eq.enabled;
                changed = true;
            }
            // The LP knob: drag up or down (100 skin pixels for the whole turn), double-click
            // to turn it off.
            let kr = sk.at("eq_lp");
            let (resp, delta) = widgets::drag_area(ui, &sk, "eq_lp", kr);
            if resp.double_clicked() {
                lp = audio::filter::OFF;
            } else if delta.y != 0.0 {
                lp = (lp - delta.y / 100.0).clamp(0.0, audio::filter::OFF);
            }
            sk.sprite_in("eq_lp_knob", kr);
            let angle = (-135.0 + 270.0 * lp).to_radians();
            let c = kr.center();
            let tip = c + vec2(angle.sin(), -angle.cos()) * 4.5 * sk.scale;
            let on = lp < audio::filter::OFF;
            sk.painter.line_segment(
                [c + (tip - c) * 0.2, tip],
                egui::Stroke::new(
                    1.5 * sk.scale,
                    if on {
                        color(sk.def.colors.lcd)
                    } else {
                        Color32::from_rgb(26, 26, 38)
                    },
                ),
            );
            resp.on_hover_text(format::lp_label(lp));
            let presets_resp = widgets::button(ui, &sk, "eq_presets", "eq_presets", "eq_presets");
            egui::Popup::menu(&presets_resp).show(|ui| {
                ui.label("Load");
                for p in &self.presets.presets {
                    if ui.button(&p.name).clicked() {
                        eq = p.apply_to(&eq);
                        changed = true;
                    }
                }
                ui.separator();
                if ui.button("Save as…").clicked() {
                    actions.push(Action::SavePreset);
                }
                ui.menu_button("Delete", |ui| {
                    for p in &self.presets.presets {
                        if ui.button(&p.name).clicked() {
                            actions.push(Action::DeletePreset(p.name.clone()));
                        }
                    }
                });
            });

            // Response curve.
            let g = sk.def.at("eq_graph");
            let pts = eqcurve::curve(&eq.bands_db, (g.w - 2) as usize);
            let cx = sk.def.colors.eq_curve;
            for (i, db) in pts.iter().enumerate() {
                let y = g.y as f32 + 1.0 + ((12.0 - db) / 24.0 * (g.h - 3) as f32).round();
                sk.fill(sk.rect(g.x as f32 + 1.0 + i as f32, y, 1.0, 1.0), color(cx));
            }

            // Sliders (double-click resets to 0 dB).
            let slider = |ui: &mut Ui, id: &str, layout: &str, db: &mut f32| {
                let (resp, new) = widgets::vslider(
                    ui,
                    &sk,
                    id,
                    layout,
                    "eq_track",
                    "eq_thumb",
                    (*db + 12.0) / 24.0,
                );
                if resp.double_clicked() {
                    *db = 0.0;
                    return true;
                }
                if let Some(v) = new {
                    *db = (v * 24.0 - 12.0).round().clamp(-12.0, 12.0);
                    return true;
                }
                false
            };
            changed |= slider(ui, "eq_pre", "eq_preamp", &mut eq.preamp_db);
            for i in 0..10 {
                changed |= slider(
                    ui,
                    &format!("eq_b{i}"),
                    &format!("eq_band{i}"),
                    &mut eq.bands_db[i],
                );
            }
        }
        if lp != self.lp_knob {
            self.lp_knob = lp;
            self.with_engine(|e| e.set_filter(lp));
        }
        if changed && eq != self.settings.eq {
            self.settings.eq = eq;
            self.with_engine(|e| e.set_eq(eq));
            self.mark_settings();
        }
        for a in actions {
            self.apply(a, ui.ctx());
        }
    }

    /// The Options menu on a right-click (or Control-click on a Mac) in `rect`, which sits
    /// under the section's controls so they keep their own clicks.
    fn options_menu(&self, ui: &mut Ui, rect: Rect, id: &str, actions: &mut Vec<Action>) {
        let resp = ui
            .interact(rect, Id::new(id), Sense::click())
            .on_hover_text("Right-click for options");
        let open = opens_context_menu(
            resp.secondary_clicked(),
            resp.clicked(),
            ui.input(|i| i.modifiers),
        );
        let command = if open {
            Some(egui::SetOpenCommand::Bool(true))
        } else if resp.clicked() {
            Some(egui::SetOpenCommand::Bool(false))
        } else {
            None
        };
        egui::Popup::context_menu(&resp)
            .open_memory(command)
            .show(|ui| self.options_items(ui, actions));
    }

    /// The Options menu's items, for the right-click and the footer's gear alike.
    fn options_items(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let label = if self.settings.scale >= 2 {
            "Classic size (1×)"
        } else {
            "Double size (2×)"
        };
        if ui.button(label).clicked() {
            actions.push(Action::ToggleScale);
            ui.close();
        }
        if ui.button("Spectrogram (S)").clicked() {
            actions.push(Action::ToggleSpectrogram);
            ui.close();
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.dig.is_some() {
            ui.separator();
            if ui.button("Discogs…").clicked() {
                actions.push(Action::Dig(DigAction::OpenDialog));
                ui.close();
            }
            if ui.button("Browser…").clicked() {
                actions.push(Action::Dig(DigAction::OpenBrowserDialog));
                ui.close();
            }
        }
    }

    /// The playlist's width and rows now: the chosen ones, or, while maximized, what the
    /// window leaves under the band (the chosen ones are kept for restoring).
    fn pl_geometry(&self) -> (u16, usize) {
        if self.settings.playlist_maximized {
            let (w, rows) = crate::layout::maximized(&self.skin.def, self.win_px.x, self.win_px.y);
            (w, rows as usize)
        } else {
            (
                self.settings.playlist_width,
                crate::layout::playlist_rows(&self.settings, &self.skin.def) as usize,
            )
        }
    }

    /// Rows the playlist shows now.
    fn pl_rows(&self) -> usize {
        self.pl_geometry().1
    }

    /// The playlist is wide enough for columns (under a header row).
    fn pl_columns(&self) -> bool {
        self.pl_geometry().0 >= crate::columns::COLUMNS_FROM_WIDTH
    }

    /// The column header shows: wide enough for columns, and not grouped by record (record
    /// rows span the whole width, so the header's labels would name nothing).
    fn pl_header(&self) -> bool {
        self.pl_columns() && !self.crates.shown().is_grouped()
    }

    /// Rows of entries on screen: the playlist's rows, less the column header.
    fn pl_visible_rows(&self) -> usize {
        (self.pl_rows() - self.pl_header() as usize).max(1)
    }

    /// The playlist is wide enough for the crate sidebar (whether or not it is on).
    fn pl_sidebar_fits(&self) -> bool {
        self.settings.playlist_maximized || self.pl_geometry().0 >= SIDEBAR_FROM_WIDTH
    }

    /// The seller crates for the sidebar (crate, dug, refreshing), in Top Sellers order, and
    /// whether TOP SELLERS is folded.
    fn sidebar_sellers(&self) -> (Vec<(CrateId, bool, bool)>, bool) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (rows, folded) = self.seller_rows();
            (
                rows.into_iter()
                    .map(|r| (r.crate_id, r.dug, r.refreshing))
                    .collect(),
                folded,
            )
        }
        #[cfg(target_arch = "wasm32")]
        (Vec::new(), false)
    }

    /// The crate sidebar shows exactly when it fits.
    fn pl_sidebar(&self) -> bool {
        self.pl_sidebar_fits()
    }

    /// The crate sidebar in `area` (skin pixels: x, y, w, h): the crates with their counts,
    /// then "+ New crate", and the user's Discogs collection pinned at the bottom under
    /// DISCOGS. Returns the crate under the pointer while entries are dragged (not the shown
    /// one), which is highlighted.
    fn crate_sidebar(
        &mut self,
        ui: &mut Ui,
        sk: &Skinned,
        area: (f32, f32, f32, f32),
        row_h: f32,
        font: &egui::FontId,
        actions: &mut Vec<Action>,
    ) -> Option<CrateId> {
        let (x, y, w, h) = area;
        let colors = sk.def.colors.clone();
        let scale = sk.scale;
        sk.fill(
            sk.rect(x, y, w, h),
            lerp_color(colors.pl_bg, colors.pl_text, 0.06),
        );
        sk.fill(
            sk.rect(x + w - 1.0, y, 1.0, h),
            lerp_color(colors.pl_bg, colors.pl_text, 0.3),
        );
        let painter = sk.painter.with_clip_rect(sk.rect(x, y, w - 1.0, h));
        let rows = ((h / row_h).floor() as usize).max(1);
        let (sellers, folded) = self.sidebar_sellers();
        let seller_ids: HashSet<CrateId> = sellers.iter().map(|s| s.0).collect();
        let labels: Vec<_> = self.crates.labels().cloned().collect();
        let (mine, collection): (Vec<_>, Vec<_>) = self
            .crates
            .list()
            .iter()
            .filter(|c| !seller_ids.contains(&c.id) && !c.is_label())
            .cloned()
            .partition(|c| !c.discogs());
        // The Discogs group is anchored to the bottom: DISCOGS, the wantlist then the
        // collection, then LABELS and the label crates, then TOP SELLERS and the seller crates
        // (each unless folded). When it doesn't all fit, the crates at its end are left out
        // first.
        let mut collection = collection;
        collection.sort_by_key(|c| !c.wantlist);
        let mut bottom: Vec<SideRow> = Vec::new();
        if !collection.is_empty() || !labels.is_empty() || !sellers.is_empty() {
            bottom.push(SideRow::Discogs);
        }
        bottom.extend(collection.into_iter().map(|c| SideRow::Crate(c, None)));
        if !labels.is_empty() {
            let folded = self.settings.labels_folded;
            bottom.push(SideRow::Labels(labels.len(), folded));
            if !folded {
                bottom.extend(labels.into_iter().map(|c| SideRow::Crate(c, None)));
            }
        }
        if !sellers.is_empty() {
            bottom.push(SideRow::Sellers(sellers.len(), folded));
            if !folded {
                for (id, dug, busy) in &sellers {
                    if let Some(c) = self.crates.info(*id) {
                        bottom.push(SideRow::Crate(c.clone(), Some((*dug, *busy))));
                    }
                }
            }
        }
        bottom.truncate(rows.saturating_sub(1));
        let group = bottom.len();
        let top_rows = rows - group;
        let mut placed: Vec<Placed> = mine
            .into_iter()
            .take(top_rows.saturating_sub(1))
            .enumerate()
            .map(|(i, c)| (c, i, None))
            .collect();
        let new_row = placed.len();
        let heading = |row: usize, text: &str| {
            let dr = sk.rect(x, y + row as f32 * row_h, w - 1.0, row_h);
            let width = painter
                .text(
                    pos2(dr.left() + 3.0 * scale, dr.center().y),
                    egui::Align2::LEFT_CENTER,
                    text,
                    egui::FontId::proportional(font.size * 0.75),
                    color(colors.pl_owned),
                )
                .width();
            painter.hline(
                dr.left() + width + 6.0 * scale..=dr.right() - 3.0 * scale,
                dr.center().y,
                egui::Stroke::new(1.0, lerp_color(colors.pl_owned, colors.pl_bg, 0.5)),
            );
            dr
        };
        for (i, row) in bottom.into_iter().enumerate() {
            let r = top_rows + i;
            match row {
                SideRow::Discogs => {
                    heading(r, "DISCOGS");
                }
                SideRow::Labels(n, folded) => {
                    let mark = if folded { "⏵" } else { "⏷" };
                    let dr = heading(r, &format!("{mark} LABELS ({n})"));
                    if ui
                        .interact(dr, Id::new("pl_side_labels"), Sense::click())
                        .on_hover_text("The labels you follow: click to fold")
                        .clicked()
                    {
                        actions.push(Action::ToggleLabelsFold);
                    }
                }
                SideRow::Sellers(n, folded) => {
                    let mark = if folded { "⏵" } else { "⏷" };
                    let dr = heading(r, &format!("{mark} TOP SELLERS ({n})"));
                    let resp = ui
                        .interact(dr, Id::new("pl_side_sellers"), Sense::click())
                        .on_hover_text("Your Top Sellers: click to fold, right-click to add one");
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        use crate::app::sellers::SellerAction;
                        let menu_click = opens_context_menu(
                            resp.secondary_clicked(),
                            resp.clicked(),
                            ui.input(|i| i.modifiers),
                        );
                        if resp.clicked() && !menu_click {
                            actions.push(Action::Dig(DigAction::Seller(SellerAction::ToggleFold)));
                        }
                        let open = menu_click.then_some(egui::SetOpenCommand::Bool(true));
                        egui::Popup::context_menu(&resp)
                            .open_memory(open)
                            .show(|ui| {
                                if ui.button("Add seller…").clicked() {
                                    actions.push(Action::Dig(DigAction::Seller(SellerAction::Add)));
                                    ui.close();
                                }
                            });
                    }
                    #[cfg(target_arch = "wasm32")]
                    let _ = resp;
                }
                SideRow::Crate(c, seller) => placed.push((c, r, seller)),
            }
        }
        let (shown, playing) = (self.crates.shown_id(), self.crates.playing_id());
        let state = self.position.state;
        let dragging = self.pl_drag_from.is_some();
        let mut target = None;
        for (c, row, seller) in &placed {
            let rr = sk.rect(x, y + *row as f32 * row_h, w - 1.0, row_h);
            let readable = !self.crates.is_unreadable(c.id);
            // A seller crate never dug is dimmed and shows no count.
            let undug = seller.is_some_and(|(dug, _)| !dug);
            let over = dragging && readable && c.id != shown && ui.rect_contains_pointer(rr);
            // A Discogs crate isn't lit as a target: a drop there only says how to fill it.
            if over {
                target = Some(c.id);
            }
            if over && !self.crates.is_locked(c.id) && seller.is_none() {
                painter.rect_filled(rr, 0.0, color(colors.pl_selected_bg));
            } else if c.id == shown {
                painter.rect_filled(
                    rr,
                    0.0,
                    lerp_color(colors.pl_selected_bg, colors.pl_bg, 0.5),
                );
            }
            let base = if c.discogs() {
                colors.pl_owned
            } else if c.id == shown {
                colors.pl_current
            } else {
                colors.pl_text
            };
            let col = if readable && !undug {
                color(base)
            } else {
                lerp_color(base, colors.pl_bg, 0.55)
            };
            // The left slot: ▶ / ‖ while this crate plays or is paused, a record for the
            // collection, a dot for the shown crate.
            let live = (c.id == playing)
                .then_some(state)
                .filter(|s| *s != PlayState::Stopped);
            let slot = pos2(rr.left() + 2.0 * scale, rr.center().y);
            match live {
                Some(s) => {
                    let name = if s == PlayState::Playing {
                        "status_play"
                    } else {
                        "status_pause"
                    };
                    let r = sk.def.sprite(name);
                    sk.sprite_in(
                        name,
                        Rect::from_min_size(
                            slot - vec2(0.0, r.h as f32 * scale / 2.0),
                            vec2(r.w as f32, r.h as f32) * scale,
                        ),
                    );
                }
                None if c.discogs() => {
                    let centre = slot + vec2(4.0 * scale, 0.0);
                    painter.circle_stroke(centre, 3.5 * scale, egui::Stroke::new(scale, col));
                    painter.circle_filled(centre, scale, col);
                }
                None if c.id == shown => {
                    painter.text(slot, egui::Align2::LEFT_CENTER, "•", font.clone(), col);
                }
                None => {}
            }
            // Every crate counts records, grouped or flat.
            let shown_count = self.crates.record_count(c.id);
            let count_text = if undug {
                String::new()
            } else {
                shown_count.to_string()
            };
            let count = painter
                .text(
                    pos2(rr.right() - 3.0 * scale, rr.center().y),
                    egui::Align2::RIGHT_CENTER,
                    count_text,
                    font.clone(),
                    col,
                )
                .width();
            painter
                .with_clip_rect(Rect::from_min_max(
                    rr.min,
                    pos2(rr.right() - count - 6.0 * scale, rr.max.y),
                ))
                .text(
                    pos2(rr.left() + 13.0 * scale, rr.center().y),
                    egui::Align2::LEFT_CENTER,
                    &c.name,
                    font.clone(),
                    col,
                );
            let mut resp = ui.interact(rr, Id::new(("pl_side", c.id)), Sense::click());
            if resp.hovered() && !dragging {
                let tracks = entries_label(self.crates.entry_count(c.id));
                let records = match shown_count {
                    1 => "1 record".to_owned(),
                    n => format!("{n} records"),
                };
                let mut tip = format!("{} ({records}, {tracks})", c.name);
                if c.collection {
                    tip += "\nYour Discogs collection";
                } else if c.wantlist {
                    tip += "\nYour Discogs wantlist";
                } else if c.is_label() {
                    tip += "\nA label you follow: it fills from the label's page";
                } else if undug {
                    tip += "\nTop Sellers: double-click to dig it";
                } else if seller.is_some() {
                    tip += "\nTop Sellers: double-click to refresh it (once a day)";
                }
                match live {
                    Some(PlayState::Playing) => tip += "\nPlaying from this crate",
                    Some(_) => tip += "\nPaused in this crate",
                    None => {}
                }
                resp = resp.on_hover_text(tip);
            }
            // A right-click, or a Control-click on a Mac, opens the crate's menu.
            let menu_click = opens_context_menu(
                resp.secondary_clicked(),
                resp.clicked(),
                ui.input(|i| i.modifiers),
            );
            if resp.clicked() && !menu_click && readable {
                actions.push(Action::ShowCrate(c.id));
            }
            #[cfg(not(target_arch = "wasm32"))]
            if resp.double_clicked() && seller.is_some() && readable {
                actions.push(Action::Dig(DigAction::Seller(
                    crate::app::sellers::SellerAction::Dig(c.id),
                )));
            }
            if resp.clicked() || menu_click {
                self.side_crate = Some(c.id);
            }
            // Move to Labels needs the crate's entries: a right-click loads it.
            if menu_click && readable {
                self.crates.load(c.id);
            }
            #[cfg(not(target_arch = "wasm32"))]
            let movable = self.label_to_move(c.id);
            let open = if menu_click {
                Some(egui::SetOpenCommand::Bool(true))
            } else if resp.clicked() {
                Some(egui::SetOpenCommand::Bool(false))
            } else {
                None
            };
            let entries = self.crates.entry_count(c.id);
            egui::Popup::context_menu(&resp)
                .open_memory(open)
                .show(|ui| {
                    if c.id == PLAYLIST {
                        // The scratch crate stays (Eject and opened files fill it): it clears.
                        if ui
                            .add_enabled(entries > 0, egui::Button::new("Clear crate"))
                            .clicked()
                        {
                            actions.push(Action::ClearCrate(c.id));
                            ui.close();
                        }
                        ui.label(
                            egui::RichText::new(
                                "Playlist can't be renamed or deleted:\nEject and opened files use it",
                            )
                            .weak()
                            .small(),
                        );
                        return;
                    }
                    if c.is_label() {
                        self.label_menu(ui, c.id, actions);
                        return;
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    if c.discogs() {
                        self.dig_refresh_item(ui, c, actions);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some((dug, busy)) = *seller {
                        use crate::app::sellers::SellerAction;
                        if dug {
                            let label = if busy { "Refreshing…" } else { "Refresh seller" };
                            if ui
                                .add_enabled(!busy, egui::Button::new(label))
                                .on_hover_text("Read this seller's copies for sale again")
                                .clicked()
                            {
                                actions.push(Action::Dig(DigAction::Seller(
                                    SellerAction::Refresh(c.id),
                                )));
                                ui.close();
                            }
                            if ui.button("Narrow down…").clicked() {
                                actions.push(Action::Dig(DigAction::Seller(
                                    SellerAction::Narrow(c.id),
                                )));
                                ui.close();
                            }
                        }
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(label) = movable
                        && ui
                            .button("Move to Labels")
                            .on_hover_text(
                                "Follow this crate's label: it moves under LABELS and fills \
                                 only from the label from then on",
                            )
                            .clicked()
                    {
                        actions.push(Action::MoveToLabels(c.id, label));
                        ui.close();
                    }
                    if ui
                        .add_enabled(readable, egui::Button::new("Rename crate…"))
                        .clicked()
                    {
                        actions.push(Action::RenameCrate(c.id));
                        ui.close();
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    if seller.is_some() {
                        if ui.button("Remove seller…").clicked() {
                            actions.push(Action::Dig(DigAction::Seller(
                                crate::app::sellers::SellerAction::Remove(c.id),
                            )));
                            ui.close();
                        }
                        return;
                    }
                    if ui.button("Delete crate…").clicked() {
                        actions.push(Action::DeleteCrate(c.id));
                        ui.close();
                    }
                });
        }
        let nr = sk.rect(x, y + new_row as f32 * row_h, w - 1.0, row_h);
        painter.text(
            pos2(nr.left() + 3.0 * scale, nr.center().y),
            egui::Align2::LEFT_CENTER,
            "+ New crate",
            font.clone(),
            lerp_color(colors.pl_text, colors.pl_bg, 0.3),
        );
        if ui
            .interact(nr, Id::new("pl_side_new"), Sense::click())
            .clicked()
        {
            actions.push(Action::NewCrate);
        }
        target
    }

    /// The filter bar's BPM control, from `x` to `x_end` (skin pixels, in the bar at `y`, `h`
    /// tall): "BPM", a two-handle range over the crate's tempos and the range as text (the
    /// bar's × clears it). Nothing when the crate has fewer than two different tempos.
    /// Returns where it ends (`x` when nothing is drawn).
    fn bpm_control(
        &mut self,
        ui: &mut Ui,
        sk: &Skinned,
        (x, y, x_end, h): (f32, f32, f32, f32),
        actions: &mut Vec<Action>,
    ) -> f32 {
        let shown = self.crates.shown();
        let Some((lo, hi)) = shown.tempo_span() else {
            return x;
        };
        let filter = shown.bpm_filter();
        let (a, b) = filter.unwrap_or((lo, hi));
        let colors = &sk.def.colors;
        let lcd = color(colors.lcd);
        let ty = y + ((h - sk.def.font.glyph_h as f32) / 2.0).round();

        // A short slider of a fixed width, then the range: "BPM" goes in front when there's
        // room for it too. The slider starts at a fixed place, so it never moves under a
        // dragged handle.
        let (label, core) = bpm_widths(sk);
        let with_label = x_end - x >= label + core;
        if with_label {
            sk.text(x, ty, "BPM", lcd);
        }
        let x0 = if with_label { x + label } else { x };
        let x1 = x0 + BPM_SLIDER_W;
        let with_text = x_end - x >= core;
        if x_end - x0 < BPM_SLIDER_W {
            return x;
        }
        #[cfg(test)]
        {
            self.bpm_slider = Some((x0, x1, with_text));
        }
        let range = format!("{a}-{b}");
        let mut end = x1;
        if with_text {
            sk.text(x1 + 3.0, ty, &range, lcd);
            end = x1 + 3.0 + sk.text_width(&range);
        }

        // The track, its selected part, and the two handles.
        let span = (hi - lo) as f32;
        let at = |v: u16| x0 + (v - lo) as f32 / span * (x1 - x0);
        let mid = y + (h / 2.0).floor();
        sk.fill(
            sk.rect(x0, mid, x1 - x0, 1.0),
            lerp_color(colors.pl_text, colors.pl_bg, 0.7),
        );
        sk.fill(sk.rect(at(a), mid - 1.0, at(b) - at(a), 3.0), lcd);
        let handle = sk.def.sprite("bpm_handle");
        let (hw, hh) = (handle.w as f32, handle.h as f32);
        for v in [a, b] {
            sk.sprite(
                "bpm_handle",
                (at(v) - hw / 2.0).round(),
                y + ((h - hh) / 2.0).round(),
            );
        }
        let resp = ui.interact(
            sk.rect(x0 - hw / 2.0, y, x1 - x0 + hw, h),
            Id::new("bpm_range"),
            Sense::click_and_drag(),
        );
        if resp.double_clicked() {
            actions.push(Action::SetBpmFilter(None));
        } else if let Some(p) = resp.interact_pointer_pos() {
            let px = (p.x - sk.origin.x) / sk.scale;
            let v = lo + ((px - x0) / (x1 - x0) * span).round().clamp(0.0, span) as u16;
            // The handle nearest the pointer when the drag starts is the one that moves.
            let which = *self
                .bpm_drag
                .get_or_insert(if (px - at(a)).abs() <= (px - at(b)).abs() {
                    0
                } else {
                    1
                });
            let (na, nb) = if which == 0 {
                (v.min(b), b)
            } else {
                (a, v.max(a))
            };
            if (na, nb) != (a, b) {
                actions.push(Action::SetBpmFilter(Some((na, nb))));
            }
        } else {
            self.bpm_drag = None;
        }
        let mut tip = format!("{range} BPM: drag a handle to keep only these tempos");
        if filter.is_some() {
            tip += "; double-click to show all";
        }
        let missing = shown.without_bpm();
        if missing > 0 {
            tip += &format!(
                " ({} without a BPM {} hidden while a range is set)",
                entries_label(missing),
                if missing == 1 { "is" } else { "are" }
            );
        }
        resp.on_hover_text(tip);
        end
    }

    /// The shown crate's values for `facet` with their record counts, counted again only when
    /// the crate changes.
    fn facet_counts(&mut self, facet: Facet) -> FacetCounts {
        let c = self.crates.shown_id();
        let rev = self.crates.shown().rev();
        let slot = &mut self.facet_cache[facet as usize];
        match slot {
            Some((sc, sr, list)) if (*sc, *sr) == (c, rev) => list.clone(),
            _ => {
                let list = Rc::new(self.crates.shown().counts(facet));
                *slot = Some((c, rev, list.clone()));
                list
            }
        }
    }

    /// The filters the shown crate offers, of those with at least two values: style, artist
    /// and label in the Discogs crates, format in any crate with entries from Discogs.
    fn offered_facets(&mut self) -> Vec<Facet> {
        let discogs = self.crates.is_discogs(self.crates.shown_id());
        let dug = discogs
            || self
                .crates
                .shown()
                .entries()
                .iter()
                .any(|e| e.origin.is_some());
        Facet::ALL
            .into_iter()
            .filter(|&f| if f == Facet::Format { dug } else { discogs })
            .filter(|&f| self.facet_counts(f).len() >= 2)
            .collect()
    }

    /// The widths the filter buttons want: with style chips (when the styles are few enough)
    /// and as buttons only; `None` when the crate offers no filter.
    fn filter_widths(&mut self, sk: &Skinned) -> Option<(Option<f32>, f32)> {
        let facets = self.offered_facets();
        if facets.is_empty() {
            return None;
        }
        let counts: Vec<FacetCounts> = facets.iter().map(|&f| self.facet_counts(f)).collect();
        let shown = self.crates.shown();
        let width = |items: &[f32]| {
            items.iter().sum::<f32>() + STYLE_GAP * items.len().saturating_sub(1) as f32
        };
        let buttons: Vec<f32> = facets
            .iter()
            .map(|&f| sk.text_width(&facet_button_label(shown, f)))
            .collect();
        let chips: Option<Vec<f32>> = facets
            .iter()
            .position(|&f| f == Facet::Style)
            .filter(|&i| counts[i].len() <= STYLE_CHIPS_MAX)
            .map(|i| {
                counts[i]
                    .iter()
                    .map(|(s, _)| sk.text_width(&s.to_uppercase()))
                    .chain(
                        facets
                            .iter()
                            .zip(&buttons)
                            .filter(|(f, _)| **f != Facet::Style)
                            .map(|(_, w)| *w),
                    )
                    .collect()
            });
        Some((chips.map(|c| width(&c)), width(&buttons)))
    }

    /// The filter bar's style, artist, label and format filters, from `x` to `x_end` (skin
    /// pixels, like [`Self::bpm_control`]): the style chips (at most [`STYLE_CHIPS_MAX`]) then
    /// the other buttons when all of it fits, else one button per filter when they fit, else
    /// one FILTERS button. Each opens the filter panel. Returns where they end.
    fn filter_controls(
        &mut self,
        ui: &mut Ui,
        sk: &Skinned,
        (x, y, x_end, h): (f32, f32, f32, f32),
        actions: &mut Vec<Action>,
    ) -> f32 {
        // With no filter but a CART switch that doesn't fit, FILTERS still opens the panel.
        let cart_w = self
            .cart_label()
            .map(|l| 2.0 * STYLE_GAP + sk.text_width(&l));
        let (chips_w, buttons_w) = match self.filter_widths(sk) {
            Some(w) => w,
            None if cart_w.is_some_and(|w| x + w > x_end) => (None, f32::INFINITY),
            None => return x,
        };
        let facets = self.offered_facets();
        let counts: Vec<FacetCounts> = facets.iter().map(|&f| self.facet_counts(f)).collect();
        let shown = self.crates.shown();
        let lcd = color(sk.def.colors.lcd);
        let dim = lerp_color(sk.def.colors.lcd, sk.def.colors.pl_bg, 0.6);
        let ty = y + ((h - sk.def.font.glyph_h as f32) / 2.0).round();
        let x = x + STYLE_GAP;
        let room = x_end - x;
        let set = facets
            .iter()
            .filter(|&&f| shown.filter(f).is_some())
            .count()
            + shown.cart_only() as usize;
        let folded = if set > 0 {
            format!("FILTERS {set}")
        } else {
            "FILTERS".to_owned()
        };
        let tier = if chips_w.is_some_and(|w| w <= room) {
            FooterFilters::Chips
        } else if buttons_w <= room {
            FooterFilters::Buttons
        } else if sk.text_width(&folded) <= room {
            FooterFilters::Folded
        } else {
            FooterFilters::None
        };
        #[cfg(test)]
        {
            self.filters_drawn = Some((x, tier));
        }
        if tier == FooterFilters::None {
            return x;
        }
        if tier == FooterFilters::Folded {
            let w = sk.text_width(&folded);
            sk.text(x, ty, &folded, if set > 0 { lcd } else { dim });
            let button = ui.interact(
                sk.rect(x - 1.0, y, w + 2.0, h),
                Id::new("filters_button"),
                Sense::click(),
            );
            if button.clicked() {
                // On the first tab with a set filter, or the first tab.
                self.panel_tab = facets
                    .iter()
                    .copied()
                    .find(|&f| self.crates.shown().filter(f).is_some())
                    .or(facets.first().copied())
                    .unwrap_or(self.panel_tab);
            }
            egui::Popup::from_toggle_button_response(&button)
                .id(filters_popup_id())
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| self.filter_panel(ui, actions));
            button.on_hover_text("Filters: click to choose which records show");
            return x + w;
        }
        // What to draw, worked out before the lists borrow the app.
        let texts: Vec<(String, bool)> = facets
            .iter()
            .map(|&f| (facet_button_label(shown, f), shown.filter(f).is_some()))
            .collect();
        let styles_on: Vec<bool> = facets
            .iter()
            .position(|&f| f == Facet::Style)
            .map(|i| {
                counts[i]
                    .iter()
                    .map(|(s, _)| shown.picked(Facet::Style, s))
                    .collect()
            })
            .unwrap_or_default();
        let mut cx = x;
        for ((&f, _), (text, on)) in facets.iter().zip(&counts).zip(texts) {
            if f == Facet::Style && tier == FooterFilters::Chips {
                let list = &counts[facets.iter().position(|&g| g == f).unwrap_or(0)];
                for ((style, n), &on) in list.iter().zip(&styles_on) {
                    let name = style.to_uppercase();
                    let w = sk.text_width(&name);
                    sk.text(cx, ty, &name, if on { lcd } else { dim });
                    let r = ui.interact(
                        sk.rect(cx - 1.0, y, w + 2.0, h),
                        Id::new(("style_chip", style.as_str())),
                        Sense::click(),
                    );
                    if r.double_clicked() {
                        actions.push(Action::ClearPicks(Some(f)));
                    } else if r.clicked() {
                        actions.push(Action::TogglePick(f, style.clone()));
                    }
                    r.on_hover_text(format!(
                        "{style}: {} — click to {} it; double-click shows every style",
                        records_label(*n),
                        if on { "unselect" } else { "select" }
                    ));
                    cx += w + STYLE_GAP;
                }
                continue;
            }
            let w = sk.text_width(&text);
            sk.text(cx, ty, &text, if on { lcd } else { dim });
            let button = ui.interact(
                sk.rect(cx - 1.0, y, w + 2.0, h),
                Id::new(("facet_button", f.name())),
                Sense::click(),
            );
            if button.double_clicked() {
                actions.push(Action::ClearPicks(Some(f)));
            } else if button.clicked() {
                self.panel_tab = f;
            }
            let n = self.facet_counts(f).len();
            let popup = egui::Popup::from_toggle_button_response(&button)
                .id(facet_popup_id(f))
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
            popup.show(|ui| self.filter_panel(ui, actions));
            button.on_hover_text(format!(
                "{n} {}s: click to choose which records show; double-click shows them all",
                f.name()
            ));
            cx += w + STYLE_GAP;
        }
        cx - STYLE_GAP
    }

    /// The filter panel: a tab per filter the crate offers (its list: search, checkboxes with
    /// record counts, Clear) and, in a seller crate with a cart, the CART switch.
    fn filter_panel(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let facets = self.offered_facets();
        let tab = if facets.contains(&self.panel_tab) {
            Some(self.panel_tab)
        } else {
            facets.first().copied()
        };
        let cart = self.cart_label();
        let mut chosen = None;
        ui.horizontal(|ui| {
            for &f in &facets {
                if ui
                    .selectable_label(tab == Some(f), f.name().to_uppercase())
                    .clicked()
                {
                    chosen = Some(f);
                }
            }
            if let Some(label) = &cart {
                let mut on = self.crates.shown().cart_only();
                if ui.checkbox(&mut on, label.as_str()).clicked() {
                    actions.push(Action::ToggleCartOnly);
                }
            }
        });
        if let Some(f) = chosen {
            self.panel_tab = f;
        }
        if let Some(f) = chosen.or(tab) {
            ui.separator();
            let list = self.facet_counts(f);
            self.facet_list(ui, f, &list, actions);
        }
    }

    /// The filter bar's CART switch, from `x` (skin pixels, like [`Self::filter_controls`]),
    /// in a seller crate with copies in the cart: "CART 3 · €41.20", lit while on. Nothing
    /// when it doesn't fit (the filter panel holds it too).
    fn cart_switch(
        &mut self,
        ui: &mut Ui,
        sk: &Skinned,
        (x, y, x_end, h): (f32, f32, f32, f32),
        actions: &mut Vec<Action>,
    ) {
        let on = self.crates.shown().cart_only();
        #[cfg(test)]
        {
            self.cart_drawn = None;
        }
        let Some(label) = self.cart_label() else {
            return;
        };
        let x = x + STYLE_GAP;
        let w = sk.text_width(&label);
        if x + w > x_end {
            return;
        }
        let lcd = color(sk.def.colors.lcd);
        let dim = lerp_color(sk.def.colors.lcd, sk.def.colors.pl_bg, 0.6);
        let ty = y + ((h - sk.def.font.glyph_h as f32) / 2.0).round();
        sk.text(x, ty, &label, if on { lcd } else { dim });
        let r = ui
            .interact(
                sk.rect(x - 1.0, y, w + 2.0, h),
                Id::new("cart_switch"),
                Sense::click(),
            )
            .on_hover_text(if on {
                "Only what's in your cart: click to show every record"
            } else {
                "Click to show and play only the records in your cart"
            });
        #[cfg(test)]
        {
            self.cart_drawn = Some((label, r.rect.center()));
        }
        if r.clicked() {
            actions.push(Action::ToggleCartOnly);
        }
    }

    /// "CART 3 · €41.20": the shown seller crate's part of the cart, when it has one (or the
    /// switch is on).
    fn cart_label(&self) -> Option<String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let seller = self.crates.seller_of(self.crates.shown_id())?;
            let d = self.dig.as_ref()?;
            let part = d
                .cart
                .sellers
                .iter()
                .find(|(s, _)| s.eq_ignore_ascii_case(seller))
                .map(|(_, c)| c.clone())
                .unwrap_or_default();
            if part.count == 0 && !self.crates.shown().cart_only() {
                return None;
            }
            Some(if part.subtotal.is_empty() {
                format!("CART {}", part.count)
            } else {
                format!("CART {} · {}", part.count, part.subtotal)
            })
        }
        #[cfg(target_arch = "wasm32")]
        None
    }

    /// A filter's list: a search field, one checkbox per value with its number of records,
    /// and Clear.
    fn facet_list(&mut self, ui: &mut Ui, f: Facet, list: &FacetCounts, actions: &mut Vec<Action>) {
        ui.set_min_width(220.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.facet_search[f as usize])
                .hint_text(format!("Filter {}s…", f.name()))
                .desired_width(f32::INFINITY),
        );
        let needle = self.facet_search[f as usize].trim().to_lowercase();
        let shown = self.crates.shown();
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .show(ui, |ui| {
                for (value, n) in list.iter() {
                    if !needle.is_empty() && !value.to_lowercase().contains(&needle) {
                        continue;
                    }
                    let mut on = shown.picked(f, value);
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut on, value.as_str()).clicked() {
                            actions.push(Action::TogglePick(f, value.clone()));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.weak(n.to_string())
                        });
                    });
                }
            });
        ui.separator();
        if ui
            .add_enabled(shown.filter(f).is_some(), egui::Button::new("Clear"))
            .clicked()
        {
            actions.push(Action::ClearPicks(Some(f)));
        }
    }

    /// The column header: names (with the sort arrow), a click sorts, dragging a divider
    /// resizes the column to its left, and a right-click shows or hides columns.
    fn column_header(
        &mut self,
        ui: &mut Ui,
        sk: &Skinned,
        head: Rect,
        cols: &[(Col, f32, f32)],
        font: &egui::FontId,
        actions: &mut Vec<Action>,
    ) {
        let d = sk.def;
        let s = sk.scale;
        let painter = sk.painter.with_clip_rect(head);
        painter.rect_filled(
            head,
            0.0,
            lerp_color(d.colors.pl_text, d.colors.pl_bg, 0.85),
        );
        let label_col = lerp_color(d.colors.pl_text, d.colors.pl_bg, 0.3);
        let sorted = self.crates.shown().sorted();
        let cell = |x: f32, w: f32| {
            Rect::from_min_size(
                pos2(head.left() + x * s, head.top()),
                vec2(w * s, head.height()),
            )
        };
        for &(c, x, w) in cols {
            let name = match c {
                Col::Number => "#",
                Col::Field(f) => f.label(),
            };
            let r = cell(x, w);
            let cell_painter = painter.with_clip_rect(r.shrink2(vec2(1.5 * s, 0.0)));
            let text = cell_painter.text(
                pos2(r.left() + 3.0 * s, r.center().y),
                egui::Align2::LEFT_CENTER,
                name,
                font.clone(),
                label_col,
            );
            // The sort direction, drawn as a small triangle (the font has no arrows).
            if let (Col::Field(f), Some((sf, dir))) = (c, sorted)
                && sf == f
            {
                let (cx, cy, h) = (text.right() + 5.0 * s, r.center().y, 2.5 * s);
                let points = if dir == Dir::Asc {
                    vec![pos2(cx - h, cy + h), pos2(cx + h, cy + h), pos2(cx, cy - h)]
                } else {
                    vec![pos2(cx - h, cy - h), pos2(cx + h, cy - h), pos2(cx, cy + h)]
                };
                cell_painter.add(egui::Shape::convex_polygon(
                    points,
                    label_col,
                    egui::Stroke::NONE,
                ));
            }
        }
        let resp = ui.interact(head, Id::new("pl_head"), Sense::click());
        if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
            && let Some(&(Col::Field(f), _, _)) =
                cols.iter().find(|(_, x, w)| cell(*x, *w).contains(p))
        {
            actions.push(Action::Sort(f));
        }
        egui::Popup::context_menu(&resp)
            .id(Id::new("pl_head_menu"))
            .show(|ui| {
                for f in Field::ALL.into_iter().filter(|f| f.hideable()) {
                    let mut on = self.settings.columns.shows(f);
                    if ui.checkbox(&mut on, f.label()).clicked() {
                        actions.push(Action::ToggleColumn(f));
                    }
                }
            });
        // Dividers on each column's right edge (none after the last). Dragging the title's
        // edge resizes the column after it instead, since the title takes what's left.
        for (i, &(c, x, w)) in cols.iter().enumerate().take(cols.len().saturating_sub(1)) {
            let Col::Field(f) = c else { continue };
            let edge = head.left() + (x + w) * s;
            let grip =
                Rect::from_center_size(pos2(edge, head.center().y), vec2(4.0 * s, head.height()));
            painter.vline(
                edge,
                head.y_range(),
                egui::Stroke::new(1.0, label_col.gamma_multiply(0.5)),
            );
            let g = ui
                .interact(grip, Id::new(("pl_col_edge", i)), Sense::drag())
                .on_hover_cursor(egui::CursorIcon::ResizeColumn);
            let dx = g.drag_delta().x / s;
            if dx != 0.0 {
                let share = dx / head.width() * s;
                match (f, cols.get(i + 1)) {
                    (Field::Title, Some(&(Col::Field(next), _, _))) => {
                        actions.push(Action::ResizeColumn(next, -share))
                    }
                    _ => actions.push(Action::ResizeColumn(f, share)),
                }
            }
        }
    }

    /// When the playing entry of the shown crate changes and the previous one was on screen,
    /// the list follows the new one; otherwise the user's scroll is left alone.
    fn follow_playing_entry(&mut self) {
        let shown_id = self.crates.shown_id();
        let now = (shown_id == self.crates.playing_id())
            .then(|| self.crates.shown().current())
            .flatten()
            .map(|id| (shown_id, id));
        if now == self.pl_follow {
            return;
        }
        let shown = self.crates.shown();
        let target = match (self.pl_follow, now) {
            (Some((c0, prev)), Some((c1, new))) if c0 == c1 => shown
                .index_of(prev)
                .map(|i| self.pl_row_of(i))
                .filter(|&r| self.pl_row_in_view(r))
                .and_then(|_| shown.index_of(new)),
            _ => None,
        };
        self.pl_follow = now;
        if let Some(i) = target {
            self.scroll_into_view(i);
        }
    }

    /// The entry menu (right-click) for `e`, or for a record row whose entries are `record`
    /// (selected, so the selection items act on all of them).
    fn entry_menu(
        &self,
        ui: &mut Ui,
        e: &crate::playlist::Entry,
        record: Option<&[EntryId]>,
        actions: &mut Vec<Action>,
    ) {
        let shown = self.crates.shown();
        let waiting = matches!(e.status, EntryStatus::Waiting(_));
        let playable = match record {
            Some(ids) => ids
                .iter()
                .any(|&id| shown.get(id).is_some_and(|x| x.status.in_play_order())),
            None => e.status.in_play_order(),
        };
        if ui
            .add_enabled(
                playable,
                egui::Button::new(if waiting { "Arm" } else { "Play" }),
            )
            .clicked()
        {
            actions.push(match record {
                Some(ids) => Action::PlayRecord(ids.to_vec()),
                None => Action::PlayEntry(e.id),
            });
            ui.close();
        }
        // The Discogs crates mirror the account: records leave them with Remove from wantlist
        // or Remove from collection, never one track at a time; a label crate keeps them all.
        let discogs = self.crates.is_locked(self.crates.shown_id());
        if !discogs && ui.button("Remove").clicked() {
            actions.push(Action::RemoveEntry(e.id));
            ui.close();
        }
        // A record row already is its album.
        let album = shown.album_of(e.id).len();
        if album > 0 && record.is_none() {
            let more = album > 1;
            let tracks = if album == 1 { "track" } else { "tracks" };
            if !discogs
                && ui
                    .add_enabled(
                        more,
                        egui::Button::new(format!("Remove album ({album} {tracks})")),
                    )
                    .clicked()
            {
                actions.push(Action::RemoveAlbum(e.id));
                ui.close();
            }
            if ui
                .add_enabled(more, egui::Button::new("Select album"))
                .clicked()
            {
                actions.push(Action::SelectAlbum(e.id));
                ui.close();
            }
        }
        self.filter_items(ui, e, actions);
        ui.separator();
        ui.menu_button("Send to crate", |ui| {
            for (id, name) in self.send_targets() {
                let readable = !self.crates.is_unreadable(id);
                if ui.add_enabled(readable, egui::Button::new(name)).clicked() {
                    actions.push(Action::SendTo(e.id, Some(id)));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("New crate…").clicked() {
                actions.push(Action::SendTo(e.id, None));
                ui.close();
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        self.dig_entry_menu(ui, e, actions);
    }

    /// The entry menu's filter shortcuts: in the Discogs crates, keep only this record's
    /// artist, label or a style; elsewhere, search its label or catalogue number.
    fn filter_items(&self, ui: &mut Ui, e: &crate::playlist::Entry, actions: &mut Vec<Action>) {
        let Some(o) = e.origin.as_ref() else {
            return;
        };
        let mut items = Vec::new();
        if self.crates.is_discogs(self.crates.shown_id()) {
            for (f, what) in [(Facet::Artist, "artist"), (Facet::Label, "label")] {
                if let Some(v) = e.values(f).next() {
                    items.push((
                        format!("Only this {what}"),
                        Action::OnlyPick(f, v.to_owned()),
                    ));
                }
            }
            let styles: Vec<&str> = e.values(Facet::Style).collect();
            if !items.is_empty() || !styles.is_empty() {
                ui.separator();
            }
            for (label, a) in items {
                if ui.button(label).clicked() {
                    actions.push(a);
                    ui.close();
                }
            }
            if !styles.is_empty() {
                ui.menu_button("Only this style", |ui| {
                    for st in styles {
                        if ui.button(st).clicked() {
                            actions.push(Action::OnlyPick(Facet::Style, st.to_owned()));
                            ui.close();
                        }
                    }
                });
            }
            return;
        }
        for text in [o.label.trim(), o.catno.trim()] {
            if !text.is_empty() {
                items.push((format!("Search {text}"), Action::SearchFor(text.to_owned())));
            }
        }
        if !items.is_empty() {
            ui.separator();
        }
        for (label, a) in items {
            if ui.button(label).clicked() {
                actions.push(a);
                ui.close();
            }
        }
    }

    /// Whether a listing is in the user's Discogs cart.
    fn copy_in_cart(&self, listing: u64) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return self
            .dig
            .as_ref()
            .is_some_and(|d| d.cart.has_listing(listing));
        #[cfg(target_arch = "wasm32")]
        {
            let _ = listing;
            false
        }
    }

    /// A copy for sale under its open record: its cart pill (or SOLD), price, grades and
    /// country. It holds no audio; a double-click opens its listing, and the pill the cart.
    #[allow(clippy::too_many_arguments)]
    fn copy_row(
        &self,
        ui: &mut Ui,
        clip: &egui::Painter,
        rr: Rect,
        listing: u64,
        font: &egui::FontId,
        colors: &crate::skin::Colors,
        scale: f32,
        actions: &mut Vec<Action>,
    ) {
        let Some(c) = self.crates.shown().copy(listing) else {
            return;
        };
        let in_cart = self.copy_in_cart(listing);
        let col = if c.sold {
            lerp_color(colors.pl_text, colors.pl_bg, 0.5)
        } else {
            color(colors.pl_text)
        };
        let row_clip = clip.with_clip_rect(rr);
        let at = pos2(rr.left() + 3.0 * scale, rr.center().y);
        // An unsold copy carries its cart pill; a sold one only SOLD.
        let pill = (!c.sold).then(|| Pill {
            in_cart,
            pointer: self.pill_pointer(ui, rr),
        });
        let marks = Badges {
            owned: false,
            cart: false,
            sold: c.sold,
            pill,
        };
        let (w, pill_rect) = entry_badges(&row_clip, at, marks, None, font, colors, scale);
        row_clip.text(
            at + vec2(w, 0.0),
            egui::Align2::LEFT_CENTER,
            format::copy_line(c),
            font.clone(),
            col,
        );
        let mut tip = String::new();
        if let Some(day) = c.posted.get(..10) {
            tip += &format!("Listed {day}");
        }
        if let Some(was) = c.was_cents {
            tip += &format!("\nWas {}", format::price(was, &c.currency));
        }
        if !c.comments.trim().is_empty() {
            tip += &format!("\n{}", c.comments.trim());
        }
        tip += "\nDouble-click to open it on discogs.com";
        let resp = ui
            .interact(rr, Id::new(("pl_copy", listing)), Sense::click())
            .on_hover_text(tip.trim_start());
        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::app::sellers::SellerAction;
            if resp.double_clicked() {
                actions.push(Action::Dig(DigAction::Seller(SellerAction::OpenCopy(
                    listing,
                ))));
            }
            egui::Popup::context_menu(&resp).show(|ui| {
                if ui.button("Open on discogs.com").clicked() {
                    actions.push(Action::Dig(DigAction::Seller(SellerAction::OpenCopy(
                        listing,
                    ))));
                    ui.close();
                }
            });
        }
        #[cfg(target_arch = "wasm32")]
        let _ = resp;
        if let Some(rect) = pill_rect {
            self.cart_pill_click(ui, rect, rr, c, in_cart, actions);
        }
    }

    /// The pointer for a cart pill on the band `rr` of its row, when it is over it and nothing
    /// covers it.
    fn pill_pointer(&self, ui: &Ui, rr: Rect) -> Option<Pos2> {
        ui.rect_contains_pointer(rr)
            .then(|| ui.ctx().pointer_interact_pos())
            .flatten()
    }

    /// A cart pill's click over the band `rr` of its row, registered after the row's so it
    /// takes its own clicks (the row isn't selected, played or opened): + CART adds the copy,
    /// IN CART takes it out.
    fn cart_pill_click(
        &self,
        ui: &mut Ui,
        pill: Rect,
        rr: Rect,
        copy: &crate::playlist::SaleCopy,
        in_cart: bool,
        actions: &mut Vec<Action>,
    ) {
        let hit = Rect::from_x_y_ranges(pill.x_range(), rr.y_range());
        let what = format!(
            "{} · {}",
            format::price(copy.cents, &copy.currency),
            copy.grades
        );
        let tip = if in_cart {
            format!("Remove {what} from your cart")
        } else {
            format!("Add {what} to your cart")
        };
        let resp = ui
            .interact(hit, Id::new(("pl_cart_pill", copy.listing)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tip);
        #[cfg(not(target_arch = "wasm32"))]
        if resp.clicked() {
            use crate::app::sellers::SellerAction;
            actions.push(Action::Dig(DigAction::Seller(if in_cart {
                SellerAction::CartRemove(copy.listing)
            } else {
                SellerAction::CartAdd(vec![copy.listing])
            })));
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (resp, actions);
    }

    /// A record row: its cover, ⏵/⏷, "Artist – Album" with its marks, and a dimmed line with
    /// the pressing, its tracks and what's for sale (or the track playing in it). A click
    /// selects the record, a double-click plays it, ⏵ opens it, and its menu acts on it all; a
    /// record of one entry acts as that entry. Returns the row's response (drag, hover).
    #[allow(clippy::too_many_arguments)]
    fn record_row(
        &self,
        ui: &mut Ui,
        sk: &Skinned,
        clip: &egui::Painter,
        rr: Rect,
        rec: &records::RecordRow,
        cover: Option<CoverSlot>,
        look: &RecordLook,
        actions: &mut Vec<Action>,
    ) -> egui::Response {
        let shown = self.crates.shown();
        let colors = look.colors;
        let scale = sk.scale;
        let font = look.font;
        let first = &shown.entries()[rec.members[0]];
        let ids: Vec<EntryId> = rec.members.iter().map(|&i| shown.entries()[i].id).collect();
        let playing_here = shown
            .current()
            .filter(|c| ids.contains(c))
            .and_then(|c| shown.get(c))
            .filter(|_| self.crates.shown_id() == self.crates.playing_id());
        if ids.iter().all(|&id| shown.is_selected(id)) {
            clip.rect_filled(rr, 0.0, color(colors.pl_selected_bg));
        }
        if self.focus == Focus::Playlist && shown.cursor().is_some_and(|c| ids.contains(&c)) {
            clip.rect_stroke(
                rr,
                0.0,
                egui::Stroke::new(scale, color(colors.pl_text)),
                egui::StrokeKind::Inside,
            );
        }
        let copies = match rec.key {
            AlbumKey::Release(r) => shown.copies_of(r),
            _ => Vec::new(),
        };
        let all_sold = !copies.is_empty() && copies.iter().all(|c| c.sold);
        let (col, _) = row_look(first, playing_here.is_some(), colors);
        let col = if playing_here.is_some() {
            color(colors.pl_current)
        } else if all_sold {
            // Every copy sold: the record stays (it still plays), dimmed.
            lerp_color(
                col.to_array()[..3].try_into().unwrap_or([0; 3]),
                colors.pl_bg,
                0.45,
            )
        } else {
            col
        };
        let dim = lerp_color(
            col.to_array()[..3].try_into().unwrap_or([0; 3]),
            colors.pl_bg,
            0.4,
        );
        // The cover: a square of the row's height.
        let side = rr.height() - 2.0 * scale;
        let cr = Rect::from_min_size(rr.min + vec2(scale, scale), vec2(side, side));
        match cover {
            Some(CoverSlot::Loaded(tex, size)) => {
                let fit = size * (side / size.x.max(size.y).max(1.0));
                clip.image(
                    tex,
                    Rect::from_center_size(cr.center(), fit),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            Some(CoverSlot::Waiting) => {
                clip.rect_stroke(
                    cr,
                    0.0,
                    egui::Stroke::new(scale, dim),
                    egui::StrokeKind::Inside,
                );
            }
            None => {
                clip.circle_stroke(cr.center(), side * 0.4, egui::Stroke::new(scale, dim));
                clip.circle_filled(cr.center(), side * 0.08, dim);
            }
        }
        // The row's area first, so ▸ (added after) takes its own clicks.
        let resp = ui.interact(
            rr,
            Id::new(("pl_record", first.id)),
            Sense::click_and_drag(),
        );
        let x0 = cr.right() + 3.0 * scale;
        let line1 = rr.top() + rr.height() * 0.27;
        let line2 = rr.top() + rr.height() * 0.73;
        let text_clip = clip.with_clip_rect(Rect::from_min_max(pos2(x0, rr.top()), rr.max));
        let mut at = pos2(x0, line1);
        if !rec.single {
            let mark = if rec.open { "⏷" } else { "⏵" };
            let w = text_clip
                .text(at, egui::Align2::LEFT_CENTER, mark, font.clone(), col)
                .width();
            at.x += w + 3.0 * scale;
            let tri = Rect::from_min_max(
                pos2(x0 - 2.0 * scale, rr.top()),
                pos2(at.x, line1 * 2.0 - rr.top()),
            );
            if ui
                .interact(tri, Id::new(("pl_record_open", first.id)), Sense::click())
                .on_hover_text(if rec.open {
                    "Close the record"
                } else {
                    "Open the record"
                })
                .clicked()
            {
                actions.push(Action::ToggleRecord(rec.key.clone()));
            }
        }
        let marks = self.dig_marks(first);
        // A record with one copy left for sale carries that copy's cart pill; with several,
        // CART only says one is in the cart (the copy rows have the pills).
        let mut unsold = copies.iter().filter(|c| !c.sold);
        // The pill sits on the first line: it takes the clicks of that line's half only.
        let band = Rect::from_min_max(rr.min, pos2(rr.right(), rr.center().y));
        let lone = match (unsold.next(), unsold.next()) {
            (Some(c), None) => Some((*c, self.copy_in_cart(c.listing))),
            _ => None,
        };
        let badges = Badges {
            owned: marks.owned.is_some(),
            cart: marks.cart.is_some(),
            sold: all_sold,
            pill: lone.map(|(_, in_cart)| Pill {
                in_cart,
                pointer: self.pill_pointer(ui, band),
            }),
        };
        let (w, pill_rect) = entry_badges(
            &text_clip,
            at,
            badges,
            first.format_mark(),
            font,
            colors,
            scale,
        );
        at.x += w;
        if let (Some(rect), Some((c, in_cart))) = (pill_rect, lone) {
            self.cart_pill_click(ui, rect, band, c, in_cart, actions);
        }
        let album = if first.album().is_empty() {
            first.title.as_str()
        } else {
            first.album()
        };
        // The record's own credit ("Various" for a compilation), else its first track's.
        let artist = match first.origin.as_ref().map(|o| o.artist.as_str()) {
            Some(a) if !a.is_empty() => a,
            _ => first.artist.as_str(),
        };
        let name = if artist.is_empty() {
            album.to_owned()
        } else {
            format!("{artist} – {album}")
        };
        let failed = marks.wantlist_failed.is_some()
            || marks.collection_failed.is_some()
            || marks.discard_failed.is_some();
        let name = format!(
            "{}{}{name}",
            if failed { "⚑ " } else { "" },
            if marks.wanted { "★ " } else { "" }
        );
        // The record's styles at the right end of its first line; the name stops short of them.
        let styles = first.origin.as_ref().map_or("", |o| o.styles.as_str());
        let mut name_right = rr.right() - 3.0 * scale;
        if !styles.is_empty() {
            let small = egui::FontId::proportional(font.size * 0.9);
            let room = (rr.right() - at.x) * 0.45;
            let style_clip = clip.with_clip_rect(Rect::from_min_max(
                pos2(rr.right() - 3.0 * scale - room, rr.top()),
                rr.max,
            ));
            let w = style_clip
                .text(
                    pos2(rr.right() - 3.0 * scale, line1),
                    egui::Align2::RIGHT_CENTER,
                    styles,
                    small,
                    dim,
                )
                .width()
                .min(room);
            name_right -= w + 8.0 * scale;
        }
        let name_clip = text_clip.with_clip_rect(Rect::from_min_max(
            pos2(x0, rr.top()),
            pos2(name_right.max(at.x), rr.bottom()),
        ));
        let hl = color(colors.pl_current);
        text_marked(&name_clip, at, name, font, col, hl, shown.search());
        let second = match playing_here {
            Some(p) => {
                let sign = if self.position.state == PlayState::Paused {
                    "⏸"
                } else {
                    "⏵"
                };
                let side = p.origin.as_ref().map(|o| o.position.trim()).unwrap_or("");
                [sign, side, p.title.as_str()]
                    .into_iter()
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            None if !copies.is_empty() => format::seller_record_line(
                first.origin.as_ref(),
                rec.members.len(),
                rec.total,
                &copies,
            ),
            None => format::record_line(first.origin.as_ref(), rec.members.len(), rec.total),
        };
        text_clip.text(
            pos2(x0, line2),
            egui::Align2::LEFT_CENTER,
            second,
            egui::FontId::proportional(font.size * 0.9),
            dim,
        );
        let menu_click = opens_context_menu(
            resp.secondary_clicked(),
            resp.clicked(),
            ui.input(|i| i.modifiers),
        );
        if resp.double_clicked() {
            actions.push(if rec.single {
                Action::PlayEntry(first.id)
            } else {
                Action::PlayRecord(ids.clone())
            });
        } else if resp.clicked() && !menu_click {
            actions.push(if rec.single {
                Action::Select(rec.members[0], look.mods)
            } else {
                Action::SelectRecord(ids.clone(), look.mods)
            });
        } else if menu_click && !ids.iter().all(|&id| shown.is_selected(id)) {
            // The menu acts on the selection, which becomes the record.
            actions.push(Action::SelectRecord(ids.clone(), ClickMods::default()));
        }
        let open = if menu_click {
            Some(egui::SetOpenCommand::Bool(true))
        } else if resp.clicked() {
            Some(egui::SetOpenCommand::Bool(false))
        } else {
            None
        };
        let record = (!rec.single).then_some(ids.as_slice());
        egui::Popup::context_menu(&resp)
            .open_memory(open)
            .show(|ui| self.entry_menu(ui, first, record, actions));
        resp
    }

    /// The filter bar's search field, `w` skin pixels wide from `x`: an invisible egui text
    /// field takes the typing (so no shortcut fires), and the text is drawn in the skin font in
    /// its sunken box.
    fn search_field(
        &mut self,
        ui: &mut Ui,
        fsk: &Skinned,
        x: f32,
        w: f32,
        actions: &mut Vec<Action>,
    ) {
        let h = fsk.def.sprite("pl_field_fill").h as f32;
        let y = ((fsk.def.pl_filter_h as f32 - h) / 2.0).floor();
        fsk.sprite("pl_field_l", x, y);
        fsk.sprite_in("pl_field_fill", fsk.rect(x + 1.0, y, w - 2.0, h));
        fsk.sprite("pl_field_r", x + w - 1.0, y);
        // Cleared elsewhere (another crate shown, P, ×): the field follows.
        if !crate::playlist::fold_words(&self.pl_search).is_empty()
            && self.crates.shown().search().is_empty()
        {
            self.pl_search.clear();
        }
        let before = self.pl_search.clone();
        let rect = fsk.rect(x, y, w, h);
        let out = ui
            .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                let v = ui.visuals_mut();
                // egui paints selected glyphs in the selection stroke colour, not `text_color`.
                v.selection.bg_fill = Color32::TRANSPARENT;
                v.selection.stroke = egui::Stroke::NONE;
                v.text_cursor.stroke = egui::Stroke::NONE;
                egui::TextEdit::singleline(&mut self.pl_search)
                    .id(Id::new("pl_search"))
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .text_color(Color32::TRANSPARENT)
                    .desired_width(rect.width())
                    .show(ui)
            })
            .inner;
        let resp = &out.response;
        if std::mem::take(&mut self.search_focus) {
            resp.request_focus();
        }
        let (enter, esc, down) = ui.input(|i| {
            (
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::ArrowDown),
            )
        });
        if resp.lost_focus() && esc {
            self.pl_search.clear();
        }
        if resp.has_focus() && down {
            resp.surrender_focus();
            actions.push(Action::SearchToList);
        }
        if self.pl_search != before {
            actions.push(Action::SetSearch(self.pl_search.clone()));
        }
        if resp.lost_focus() && enter {
            actions.push(Action::PlayFirstShown);
        }

        // The text (its tail, when longer than the box) and the caret, in the LCD colour.
        let colors = &fsk.def.colors;
        let adv = fsk.def.font.advance as f32;
        let ty = y + ((h - fsk.def.font.glyph_h as f32) / 2.0).round();
        let fits = ((w - 6.0) / adv).floor().max(1.0) as usize;
        let focused = resp.has_focus();
        if self.pl_search.is_empty() && !focused {
            let hint = format!("SEARCH · {PASTE_MODIFIER}+F");
            let hint: String = hint.chars().take(fits).collect();
            fsk.text(x + 3.0, ty, &hint, lerp_color(colors.lcd, [0, 0, 0], 0.6));
            return;
        }
        let chars: Vec<char> = self.pl_search.chars().collect();
        let (caret, other) = out.cursor_range.map_or((chars.len(), chars.len()), |r| {
            (
                r.primary.index.0.min(chars.len()),
                r.secondary.index.0.min(chars.len()),
            )
        });
        let start = caret.saturating_sub(fits.saturating_sub(1));
        let shown: String = chars[start..].iter().take(fits).collect();
        fsk.text(x + 3.0, ty, &shown, color(colors.lcd));
        // The selection: an LCD block, its characters in the field's dark.
        let end = (start + fits).min(chars.len());
        let (lo, hi) = (caret.min(other).max(start), caret.max(other).min(end));
        if focused && lo < hi {
            let sx = x + 3.0 + (lo - start) as f32 * adv;
            fsk.fill(
                fsk.rect(sx - 1.0, y + 2.0, (hi - lo) as f32 * adv + 1.0, h - 4.0),
                color(colors.lcd),
            );
            let sel: String = chars[lo..hi].iter().collect();
            fsk.text(sx, ty, &sel, color(colors.pl_bg));
        }
        if focused {
            let cx = x + 3.0 + (caret - start) as f32 * adv - 1.0;
            fsk.fill(fsk.rect(cx, y + 2.0, 1.0, h - 4.0), color(colors.lcd));
        }
    }

    /// The filter bar's contents across `width` skin pixels: the search field, taking what the
    /// others leave (at least [`SEARCH_MIN_W`], at most [`SEARCH_MAX_SHARE`]), the BPM control, the filter buttons, the CART
    /// switch, and × while the search or a filter is set.
    fn filter_bar(&mut self, ui: &mut Ui, fsk: &Skinned, width: f32, actions: &mut Vec<Action>) {
        let h = fsk.def.pl_filter_h as f32;
        let shown = self.crates.shown();
        // ×'s place is kept while it is hidden, so nothing moves under a dragged handle.
        let clear_w = fsk.def.sprite("bpm_clear").w as f32;
        let (x0, x_end) = (4.0, width - 8.0 - clear_w);
        if shown.is_filtered() {
            let cy = ((h - clear_w) / 2.0).round();
            fsk.sprite("bpm_clear", x_end + 4.0, cy);
            let clear = ui.interact(
                fsk.rect(x_end + 2.0, 0.0, clear_w + 4.0, h),
                Id::new("filters_clear"),
                Sense::click(),
            );
            if clear.clicked() {
                actions.push(Action::ClearFilters);
            }
            clear.on_hover_text("Clear the search and every filter");
        }
        // What the controls want, so the search field takes only the rest.
        let mut want = 0.0;
        if shown.tempo_span().is_some() {
            let (label, core) = bpm_widths(fsk);
            want += label + core;
        }
        if let Some((chips, buttons)) = self.filter_widths(fsk) {
            want += STYLE_GAP + chips.unwrap_or(buttons);
        }
        if let Some(cart) = self.cart_label() {
            want += STYLE_GAP + fsk.text_width(&cart);
        }
        let search_w = (x_end - x0 - 4.0 - want)
            .min(width * SEARCH_MAX_SHARE)
            .max(SEARCH_MIN_W);
        self.search_field(ui, fsk, x0, search_w, actions);
        let x = x0 + search_w + 4.0;
        let bpm_end = self.bpm_control(ui, fsk, (x, 0.0, x_end, h), actions);
        let filters_end = self.filter_controls(ui, fsk, (bpm_end, 0.0, x_end, h), actions);
        self.cart_switch(ui, fsk, (filters_end, 0.0, x_end, h), actions);
    }

    fn playlist_section(&mut self, ui: &mut Ui, origin: Pos2) {
        self.follow_playing_entry();
        let rows = self.pl_rows();
        let d = crate::layout::playlist_def(&self.def, self.pl_geometry().0);
        let width = d.pl_width as f32;
        let list_h = (rows * d.pl_row_h as usize) as f32;
        let mut actions = Vec::new();
        // The rows the BPM filter shows: row r is crate entry `row_map[r]`.
        let row_map = self.crates.shown().shown_rows();
        // The search's words, lit in the rows.
        let words = self.crates.shown().search().to_vec();
        let hl = color(self.def.colors.pl_current);
        // Wide enough: columns under a header row, which takes the first row.
        let columns = self.pl_columns();
        let visible = self.pl_visible_rows();
        // What the list draws: entries, or grouped, record rows and the tracks of open ones.
        let list_rows = self.pl_list();
        let max_start = records::max_start(&list_rows, visible);
        self.pl_scroll = self.pl_scroll.min(max_start);
        {
            let def = d.clone();
            let sk = self.skinned(&def, ui, origin);
            let scale = sk.scale;
            draw_stretched_bar(&sk, "pl_top", 0.0, width, d.pl_top_h as f32);
            // The title bar names the shown crate: a click opens the crate menu, a drag still
            // moves the window.
            let title = ui.interact(
                sk.at("pl_titlebar"),
                Id::new("pl_title"),
                Sense::click_and_drag(),
            );
            if title.drag_started() {
                ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
            }
            let shown = self.crates.shown();
            let count = shown.is_filtered().then(|| (row_map.len(), shown.len()));
            draw_crate_name(&sk, self.crates.name(self.crates.shown_id()), count);
            dim_title(&sk, "pl_titlebar", self.focus == Focus::Playlist);
            // The sidebar does the crate menu's job when it shows; narrower, the title bar opens
            // the menu instead.
            if !self.pl_sidebar() {
                egui::Popup::menu(&title)
                    .id(Id::new("crate_menu"))
                    .show(|ui| self.crate_menu(ui, &mut actions));
            }
            if widgets::button(ui, &sk, "pl_close", "pl_close", "btn_close").clicked() {
                actions.push(Action::TogglePlaylist);
            }
            if widgets::button(ui, &sk, "pl_max", "pl_max", "btn_max").clicked() {
                actions.push(Action::ToggleMaximized);
            }
            // ▤: one row per record (lit while grouped).
            let on = self.crates.shown().is_grouped();
            let sprite = if on { "btn_group_on" } else { "btn_group" };
            if widgets::button(ui, &sk, "pl_group", "pl_group", sprite)
                .on_hover_text("Group by record (Shift+G)")
                .clicked()
            {
                actions.push(Action::ToggleGrouped);
            }
            // The filter bar, between the title bar and the list.
            let fsk = Skinned {
                origin: sk.origin + vec2(0.0, d.pl_top_h as f32 * scale),
                ..self.skinned(&def, ui, origin)
            };
            draw_stretched_bar(&fsk, "pl_filter", 0.0, width, d.pl_filter_h as f32);
            self.filter_bar(ui, &fsk, width, &mut actions);
            let top = (d.pl_top_h + d.pl_filter_h) as f32;
            sk.sprite_in("pl_left", sk.rect(0.0, top, 12.0, list_h));
            sk.sprite_in(
                "pl_right",
                sk.rect(d.pl_width as f32 - 20.0, top, 20.0, list_h),
            );
            // The crate sidebar takes the left of the list area.
            let mut l = d.at("pl_list");
            let side = self.pl_sidebar().then(|| {
                let area = (l.x as f32, top, SIDEBAR_W as f32, list_h);
                l.x += SIDEBAR_W;
                l.w -= SIDEBAR_W;
                area
            });
            let list = sk.rect(l.x as f32, l.y as f32, l.w as f32, list_h);
            sk.fill(list, color(d.colors.pl_bg));

            // Rows.
            let font = egui::FontId::proportional(9.5 * scale);
            let clip = sk.painter.with_clip_rect(list);
            let row_h = d.pl_row_h as f32;
            let mods = ui.input(|i| ClickMods {
                shift: i.modifiers.shift,
                command: i.modifiers.command,
            });
            let mut drop_target = None;
            let side_drop = side.and_then(|area| {
                self.crate_sidebar(ui, &sk, area, d.pl_row_h as f32, &font, &mut actions)
            });
            // The column header, then the rows.
            let head_top = top;
            // Grouped, the record rows span the width: no header (☰ › Sort still sorts).
            let header = self.pl_header();
            let rows_top = head_top + header as usize as f32 * row_h;
            let cols = if columns {
                // The number column fits the biggest number (entries keep their crate numbers).
                let digits = self.crates.shown().len().max(1).to_string().len() as f32;
                let cols = self
                    .settings
                    .columns
                    .layout(l.w as f32, digits * 6.0 + 10.0);
                if header {
                    let head = sk.rect(l.x as f32, head_top, l.w as f32, row_h);
                    self.column_header(ui, &sk, head, &cols, &font, &mut actions);
                }
                cols
            } else {
                Vec::new()
            };
            let shown = self.crates.shown();
            if row_map.is_empty() && shown.is_filtered() {
                let y = rows_top + ((row_h - sk.def.font.glyph_h as f32) / 2.0).round();
                sk.text(
                    l.x as f32 + 3.0,
                    y,
                    "NO TRACKS MATCH",
                    color(d.colors.pl_text),
                );
                ui.interact(
                    sk.rect(l.x as f32, rows_top, l.w as f32, row_h),
                    Id::new("pl_no_match"),
                    Sense::hover(),
                )
                .on_hover_text(if shown.picks_filter() {
                    "☰ › Show all records shows every style, artist, label and format"
                } else {
                    "× after the BPM range, or ☰ › Show all tempos, shows every track"
                });
            }
            // An empty crate says how to fill it (not the Discogs crates: a paste can't).
            if shown.is_empty() && !self.crates.is_locked(self.crates.shown_id()) {
                let lines = empty_crate_hint(PASTE_MODIFIER);
                let line_h = sk.def.font.glyph_h as f32 + 4.0;
                let body = list_h - (rows_top - top);
                let mut y = rows_top + ((body - line_h * lines.len() as f32) / 2.0).round();
                for line in lines {
                    let x = l.x as f32 + ((l.w as f32 - sk.text_width(&line)) / 2.0).round();
                    sk.text(x, y, &line, color(d.colors.pl_text));
                    y += line_h;
                }
            }
            // While an entry's menu is open, the rest of its album is tinted.
            self.pl_tint = list_rows
                .iter()
                .skip(self.pl_scroll)
                .take(records::fit(&list_rows, self.pl_scroll, visible))
                .filter_map(|r| match r {
                    ListRow::Entry { idx, .. } => Some(*idx),
                    ListRow::Record(_) | ListRow::Copy { .. } => None,
                })
                .find(|&idx| egui::Popup::is_id_open(ui.ctx(), row_menu_id(idx)))
                .map(|idx| {
                    let id = shown.entries()[idx].id;
                    let mut album = shown.album_of(id);
                    album.retain(|&a| a != id);
                    album
                })
                .unwrap_or_default();
            let tint = lerp_color(d.colors.pl_selected_bg, d.colors.pl_bg, 0.65);
            let mut y = 0usize;
            // The record rows drawn, for their covers.
            let mut in_view: Vec<usize> = Vec::new();
            for row in list_rows.iter().skip(self.pl_scroll) {
                if y > 0 && y + row.units() > visible {
                    break;
                }
                let at_y = y;
                y += row.units();
                let (idx, track) = match row {
                    ListRow::Entry { idx, track } => (*idx, *track),
                    ListRow::Copy { listing, .. } => {
                        let indent = if columns {
                            0.0
                        } else {
                            records::RECORD_UNITS as f32 * row_h
                        };
                        let rr = sk.rect(
                            l.x as f32 + indent,
                            rows_top + at_y as f32 * row_h,
                            l.w as f32 - indent,
                            row_h,
                        );
                        self.copy_row(
                            ui,
                            &clip,
                            rr,
                            *listing,
                            &font,
                            &d.colors,
                            scale,
                            &mut actions,
                        );
                        continue;
                    }
                    ListRow::Record(rec) => {
                        in_view.push(rec.members[0]);
                        let top = rows_top + at_y as f32 * row_h;
                        let rr = sk.rect(
                            l.x as f32,
                            top,
                            l.w as f32,
                            records::RECORD_UNITS as f32 * row_h,
                        );
                        let first = &shown.entries()[rec.members[0]];
                        #[cfg(not(target_arch = "wasm32"))]
                        let cover = self.dig.as_mut().and_then(|d| d.covers.row_slot(first));
                        #[cfg(target_arch = "wasm32")]
                        let cover = None;
                        let look = RecordLook {
                            colors: &d.colors,
                            font: &font,
                            mods,
                        };
                        let resp =
                            self.record_row(ui, &sk, &clip, rr, rec, cover, &look, &mut actions);
                        if resp.drag_started() {
                            self.pl_drag_from = Some(rec.members[0]);
                            self.pl_drag_track = None;
                            self.pl_drag_block = (!rec.single).then(|| {
                                rec.members.iter().map(|&i| shown.entries()[i].id).collect()
                            });
                        }
                        if resp.hovered() && self.pl_drag_from.is_none() {
                            let mut details =
                                format::entry_details(first, self.dig_marks(first), unix_now());
                            if let Some(counts) = format::record_counts(
                                rec.members.iter().map(|&i| &shown.entries()[i]),
                            ) {
                                details.insert(1, ("Record", counts));
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            let hover = self
                                .dig
                                .as_mut()
                                .and_then(|d| d.covers.slot(ui.ctx(), first));
                            #[cfg(target_arch = "wasm32")]
                            let hover = None;
                            resp.on_hover_ui(move |ui| entry_tooltip(ui, &details, hover));
                        }
                        if self.pl_drag_from.is_some()
                            && self.pl_drag_track.is_none()
                            && ui.rect_contains_pointer(rr)
                        {
                            drop_target = Some(rec.members[0]);
                            clip.rect_filled(
                                Rect::from_min_size(rr.min, vec2(rr.width(), scale)),
                                0.0,
                                Color32::WHITE,
                            );
                        }
                        continue;
                    }
                };
                let Some(e) = shown.entries().get(idx) else {
                    break;
                };
                // A track of an open record sits under its record, past the cover.
                let indent = if track && !columns {
                    records::RECORD_UNITS as f32 * row_h
                } else {
                    0.0
                };
                let rr = sk.rect(
                    l.x as f32 + indent,
                    rows_top + at_y as f32 * row_h,
                    l.w as f32 - indent,
                    row_h,
                );
                if shown.is_selected(e.id) {
                    clip.rect_filled(rr, 0.0, color(d.colors.pl_selected_bg));
                } else if self.pl_tint.contains(&e.id) {
                    clip.rect_filled(rr, 0.0, tint);
                }
                if self.focus == Focus::Playlist && shown.cursor() == Some(e.id) {
                    clip.rect_stroke(
                        rr,
                        0.0,
                        egui::Stroke::new(scale, color(d.colors.pl_text)),
                        egui::StrokeKind::Inside,
                    );
                }
                let current = shown.current() == Some(e.id);
                let (mut col, dur) = row_look(e, current, &d.colors);
                // Where the audio comes from: "YT", "BC" (nothing for local files).
                let source = e
                    .origin
                    .as_ref()
                    .and_then(|o| o.source())
                    .map(|s| s.badge());
                let marks = self.dig_marks(e);
                let owned = marks.owned.clone();
                let badges = Badges {
                    owned: owned.is_some(),
                    cart: marks.cart.is_some(),
                    sold: false,
                    pill: None,
                };
                if marks.passed {
                    let [r, g, b, _] = col.to_array();
                    col = lerp_color([r, g, b], d.colors.pl_bg, 0.55);
                }
                let failed = marks.wantlist_failed.is_some()
                    || marks.collection_failed.is_some()
                    || marks.discard_failed.is_some();
                let marked = |name: String| {
                    format!(
                        "{}{}{name}{}",
                        if failed { "⚑ " } else { "" },
                        if marks.wanted { "★ " } else { "" },
                        if marks.wantlist_pending {
                            " (wantlist pending)"
                        } else {
                            ""
                        }
                    )
                };
                if columns {
                    for &(c, x, w) in &cols {
                        let cell = Rect::from_min_size(
                            pos2(rr.left() + x * scale, rr.top()),
                            vec2(w * scale, rr.height()),
                        );
                        let cell_clip = clip.with_clip_rect(cell.shrink2(vec2(1.5 * scale, 0.0)));
                        let text = match c {
                            Col::Number => format!("{}.", idx + 1),
                            Col::Field(Field::Time) => {
                                let w = draw_row_end(&sk, &cell_clip, cell, &dur, &font, col);
                                if let Some(src) = source {
                                    let right = cell.right() - w - 6.0 * scale;
                                    source_badge(
                                        &cell_clip,
                                        right,
                                        cell.center().y,
                                        src,
                                        &font,
                                        col,
                                        scale,
                                    );
                                }
                                continue;
                            }
                            Col::Field(Field::Title)
                                if badges.any() || e.format_mark().is_some() =>
                            {
                                let at = pos2(cell.left() + 3.0 * scale, cell.center().y);
                                let (w, _) = entry_badges(
                                    &cell_clip,
                                    at,
                                    badges,
                                    e.format_mark(),
                                    &font,
                                    &d.colors,
                                    scale,
                                );
                                text_marked(
                                    &cell_clip,
                                    at + vec2(w, 0.0),
                                    marked(e.title.clone()),
                                    &font,
                                    col,
                                    hl,
                                    &words,
                                );
                                continue;
                            }
                            Col::Field(Field::Title) => marked(e.title.clone()),
                            Col::Field(f) => crate::columns::cell_text(e, f),
                        };
                        text_marked(
                            &cell_clip,
                            pos2(cell.left() + 3.0 * scale, cell.center().y),
                            text,
                            &font,
                            col,
                            hl,
                            &words,
                        );
                    }
                } else {
                    let mut dur_w = draw_row_end(&sk, &clip, rr, &dur, &font, col);
                    if let Some(src) = source {
                        let right = rr.right() - dur_w - 9.0 * scale;
                        dur_w += source_badge(&clip, right, rr.center().y, src, &font, col, scale);
                    }
                    let name_clip = clip.with_clip_rect(Rect::from_min_max(
                        rr.min,
                        pos2(rr.right() - dur_w - 8.0 * scale, rr.max.y),
                    ));
                    let at = pos2(rr.left() + 3.0 * scale, rr.center().y);
                    if badges.any() || e.format_mark().is_some() {
                        // The number, the badges, then the name.
                        let num = name_clip
                            .text(
                                at,
                                egui::Align2::LEFT_CENTER,
                                format!("{}. ", idx + 1),
                                font.clone(),
                                col,
                            )
                            .width();
                        let at = at + vec2(num, 0.0);
                        let (w, _) = entry_badges(
                            &name_clip,
                            at,
                            badges,
                            e.format_mark(),
                            &font,
                            &d.colors,
                            scale,
                        );
                        text_marked(
                            &name_clip,
                            at + vec2(w, 0.0),
                            marked(e.row_name()),
                            &font,
                            col,
                            hl,
                            &words,
                        );
                    } else {
                        text_marked(
                            &name_clip,
                            at,
                            format!("{}. {}", idx + 1, marked(e.row_name())),
                            &font,
                            col,
                            hl,
                            &words,
                        );
                    }
                }
                let mut resp = ui.interact(rr, row_id(idx), Sense::click_and_drag());
                // Everything known about the entry, built only for the row under the pointer.
                if resp.hovered() && self.pl_drag_from.is_none() {
                    let mut details = format::entry_details(e, marks.clone(), unix_now());
                    // While a source limits requests, what waits for it says so.
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(source) = self.waiting_for(e)
                        && let Some((_, status)) = details.iter_mut().find(|(k, _)| *k == "Status")
                    {
                        *status += &format!(" · waiting for {}", source.name());
                    }
                    // Only what's in memory: the cover worker reads files and fetches.
                    #[cfg(not(target_arch = "wasm32"))]
                    let cover = self.dig.as_mut().and_then(|d| d.covers.slot(ui.ctx(), e));
                    #[cfg(target_arch = "wasm32")]
                    let cover = None;
                    resp = resp.on_hover_ui(move |ui| entry_tooltip(ui, &details, cover));
                }
                let menu_click = opens_context_menu(
                    resp.secondary_clicked(),
                    resp.clicked(),
                    ui.input(|i| i.modifiers),
                );
                if menu_click {
                    // The album's tint shows from the next frame on.
                    ui.ctx().request_repaint();
                }
                if resp.double_clicked() {
                    actions.push(Action::PlayEntry(e.id));
                } else if resp.clicked() && !menu_click {
                    actions.push(Action::Select(idx, mods));
                } else if menu_click && !shown.is_selected(e.id) {
                    // The menu acts on the selection, which becomes the clicked entry.
                    actions.push(Action::Select(idx, ClickMods::default()));
                }
                // egui's context menu opens only on the secondary button; on a Mac,
                // Control-click is the usual right-click too.
                let open = if menu_click {
                    Some(egui::SetOpenCommand::Bool(true))
                } else if resp.clicked() {
                    Some(egui::SetOpenCommand::Bool(false))
                } else {
                    None
                };
                egui::Popup::context_menu(&resp)
                    .open_memory(open)
                    .show(|ui| self.entry_menu(ui, e, None, &mut actions));
                if resp.drag_started() {
                    self.pl_drag_from = Some(idx);
                    self.pl_drag_block = None;
                    // A track of an open record stays in its record.
                    self.pl_drag_track = track.then(|| e.album_key()).flatten();
                }
                // Where a drag may land: a record moves between rows, a track within its own.
                let lands = match (&self.pl_drag_block, &self.pl_drag_track) {
                    (Some(_), _) => !track,
                    (None, Some(k)) => track && e.album_key().as_ref() == Some(k),
                    (None, None) => !track,
                };
                if self.pl_drag_from.is_some() && lands && ui.rect_contains_pointer(rr) {
                    drop_target = Some(idx);
                    clip.rect_filled(
                        Rect::from_min_size(rr.min, vec2(rr.width(), scale)),
                        0.0,
                        Color32::WHITE,
                    );
                }
            }
            if let Some(from) = self.pl_drag_from
                && ui.input(|i| i.pointer.any_released())
            {
                let shown = self.crates.shown();
                match (drop_target, self.pl_drag_block.take()) {
                    (Some(to), Some(block)) => {
                        let before = shown.entries().get(to).map(|e| e.id);
                        actions.push(Action::MoveBlock(block, before));
                    }
                    (Some(to), None) => actions.push(Action::Move(from, to)),
                    (None, _) => {
                        if let Some(c) = side_drop
                            && let Some(e) = shown.entries().get(from)
                        {
                            actions.push(Action::DropOnCrate(e.id, c));
                        }
                    }
                }
                self.pl_drag_from = None;
                self.pl_drag_track = None;
            }
            // The covers of the record rows in view, top first.
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(dig) = &mut self.dig {
                let shown = self.crates.shown();
                let es: Vec<&crate::playlist::Entry> = in_view
                    .iter()
                    .filter_map(|&i| shown.entries().get(i))
                    .collect();
                dig.covers.want_in_view(&es);
            }
            #[cfg(target_arch = "wasm32")]
            let _ = in_view;
            // Scrolling adds up, so a trackpad's small steps move the list too. The rows sit
            // on top of the list, so "over the list" is where the pointer is, not what egui
            // reports as hovered.
            if ui.rect_contains_pointer(list) {
                let dy = ui.input(|i| i.smooth_scroll_delta.y);
                self.pl_scroll = scroll_rows(
                    &mut self.pl_scroll_acc,
                    self.pl_scroll,
                    max_start,
                    dy,
                    row_h * scale,
                );
            } else {
                self.pl_scroll_acc = 0.0;
            }

            // Scrollbar.
            let sc = d.at("pl_scroll");
            let thumb = d.sprite("pl_scroll_thumb");
            let travel = (list_h - thumb.h as f32).max(0.0);
            let max_scroll = max_start;
            let frac = if max_scroll == 0 {
                0.0
            } else {
                self.pl_scroll as f32 / max_scroll as f32
            };
            let sc_rect = sk.rect(sc.x as f32, top, sc.w as f32, list_h);
            let sresp = ui.interact(sc_rect, Id::new("pl_scroll"), Sense::click_and_drag());
            if let Some(p) = sresp.interact_pointer_pos() {
                let f = (((p.y - sc_rect.top()) / scale - thumb.h as f32 / 2.0) / travel)
                    .clamp(0.0, 1.0);
                self.pl_scroll = (f * max_scroll as f32).round() as usize;
            }
            sk.sprite(
                "pl_scroll_thumb",
                sc.x as f32,
                top + (frac * travel).round(),
            );

            // Bottom bar.
            let bottom_y = top + list_h;
            let bsk = Skinned {
                origin: sk.origin + vec2(0.0, bottom_y * scale),
                ..self.skinned(&def, ui, origin)
            };
            draw_stretched_bar(&bsk, "pl_bottom", 0.0, width, d.pl_bottom_h as f32);
            // + adds to the crate; ≡ is the crate's menu.
            let plus = widgets::button(ui, &bsk, "pl_plus", "pl_plus", "pl_plus");
            egui::Popup::menu(&plus).show(|ui| {
                if ui.button("Add files…").clicked() {
                    actions.push(Action::AddFiles);
                }
                if ui.button("Add folder…").clicked() {
                    actions.push(Action::AddFolder);
                }
                ui.separator();
                if ui.button("Import M3U…").clicked() {
                    actions.push(Action::AddFiles);
                }
            });
            plus.on_hover_text("Add");
            let discogs = self.crates.is_locked(self.crates.shown_id());
            let seller_crate = self.crates.seller_of(self.crates.shown_id()).is_some();
            let menu = widgets::button(ui, &bsk, "pl_menu", "pl_menu", "pl_menu");
            egui::Popup::menu(&menu).show(|ui| {
                if ui.button("Select all").clicked() {
                    actions.push(Action::SelectAll);
                }
                if ui.button("Select none").clicked() {
                    actions.push(Action::SelectNone);
                }
                if ui.button("Invert selection").clicked() {
                    actions.push(Action::InvertSelection);
                }
                ui.separator();
                // The Discogs crates lose records only through the Discogs items.
                if !discogs && ui.button("Remove selected").clicked() {
                    actions.push(Action::RemoveSelected);
                }
                if !discogs && ui.button("Clear crate").clicked() {
                    actions.push(Action::Clear);
                }
                ui.separator();
                ui.menu_button("Sort", |ui| {
                    for f in Field::ALL {
                        if ui.button(f.label()).clicked() {
                            actions.push(Action::Sort(f));
                            ui.close();
                        }
                    }
                });
                #[cfg(not(target_arch = "wasm32"))]
                if seller_crate && ui.button("Open cart on discogs.com").clicked() {
                    actions.push(Action::Dig(DigAction::Seller(
                        crate::app::sellers::SellerAction::OpenCart,
                    )));
                    ui.close();
                }
                let mut grouped = self.crates.shown().is_grouped();
                if ui
                    .checkbox(&mut grouped, "Group by record")
                    .on_hover_text("One row per record, opening to its tracks (Shift+G)")
                    .clicked()
                {
                    actions.push(Action::ToggleGrouped);
                    ui.close();
                }
                ui.separator();
                if ui.button("Export M3U…").clicked() {
                    actions.push(Action::ExportM3u);
                }
            });
            menu.on_hover_text("Crate");
            // The gear: the same Options as a right-click on the player.
            let opts = widgets::button(ui, &bsk, "pl_opts", "pl_opts", "pl_opts");
            egui::Popup::menu(&opts).show(|ui| self.options_items(ui, &mut actions));
            opts.on_hover_text("Options");

            // "selected/total" time, classic style, right-aligned beside the grip.
            let (total, t_unknown) = self.crates.shown().total_duration();
            let (seltime, s_unknown) = self.crates.shown().selected_duration();
            let info = format!(
                "{}{}/{}{}",
                format::clock(seltime),
                if s_unknown { "+" } else { "" },
                format::clock(total),
                if t_unknown { "+" } else { "" }
            );
            // In the skin's LCD box, right-aligned; just the total when both don't fit.
            let pi = d.at("pl_info");
            let info = if bsk.text_width(&info) > pi.w as f32 {
                format!(
                    "{}{}",
                    format::clock(total),
                    if t_unknown { "+" } else { "" }
                )
            } else {
                info
            };
            let info_x = (pi.x + pi.w) as f32 - bsk.text_width(&info);
            bsk.text(info_x, pi.y as f32, &info, color(bsk.def.colors.lcd));

            // The resize grip: whole rows down, any width sideways.
            let rz = bsk.at("pl_resize");
            bsk.sprite(
                "pl_resize",
                d.at("pl_resize").x as f32,
                d.at("pl_resize").y as f32,
            );
            let (_, delta) = widgets::drag_area(ui, &bsk, "pl_resize", rz);
            self.pl_resize_acc += delta;
            let rows = (self.pl_resize_acc.y / row_h).trunc();
            let width = self.pl_resize_acc.x.trunc();
            if self.settings.playlist_maximized {
                self.pl_resize_acc = egui::Vec2::ZERO;
            } else if rows != 0.0 || width != 0.0 {
                self.pl_resize_acc -= vec2(width, rows * row_h);
                actions.push(Action::ResizePlaylist {
                    rows: rows as i32,
                    width: width as i32,
                });
            }
        }
        for a in actions {
            self.apply(a, ui.ctx());
        }
    }

    /// The name dialog: EQ preset names, and new or renamed crates. A refused name keeps the
    /// dialog open with the reason.
    fn name_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.name_dialog else {
            return;
        };
        let (title, verb) = dialog.purpose.labels();
        let mut done = None;
        egui::Window::new(title)
            .id(Id::new("name-dialog"))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let r = ui.text_edit_singleline(&mut dialog.text);
                // Enter makes the field lose focus; check that before taking focus back.
                let entered = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                r.request_focus();
                if let Some(e) = &dialog.error {
                    ui.colored_label(Color32::from_rgb(230, 90, 90), e);
                }
                ui.horizontal(|ui| {
                    if ui.button(verb).clicked() || entered {
                        done = Some(true);
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                        done = Some(false);
                    }
                });
            });
        match done {
            Some(true) => {
                let Some(dialog) = self.name_dialog.take() else {
                    return;
                };
                if let Err(e) = self.apply_name(&dialog.purpose, &dialog.text) {
                    self.name_dialog = Some(NameDialog {
                        error: Some(e),
                        ..dialog
                    });
                }
            }
            Some(false) => self.name_dialog = None,
            None => {}
        }
    }

    fn apply_name(&mut self, purpose: &NameFor, name: &str) -> Result<(), String> {
        match purpose {
            NameFor::Preset => {
                if !name.trim().is_empty() {
                    self.presets
                        .save(EqPreset::from_settings(name.trim(), &self.settings.eq));
                    self.save_presets();
                }
            }
            NameFor::NewCrate => {
                let id = self.crates.create(name)?;
                self.show_crate(id);
            }
            NameFor::RenameCrate(id) => self.crates.rename(*id, name)?,
            NameFor::SendToNew(ids) => {
                let to = self.crates.create(name)?;
                self.send_to(ids, to);
            }
            NameFor::ExportLabel(from) => {
                let to = self.crates.create(name)?;
                self.export_label(*from, to);
                self.show_crate(to);
            }
        }
        Ok(())
    }

    /// "Delete crate "X" (40 entries)?" before deleting a crate that has entries.
    fn delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.confirm_delete else {
            return;
        };
        let label = self.crates.is_label(id);
        let question = if label {
            format!(
                "Stop following \"{}\" and delete its crate ({})?",
                self.crates.name(id),
                entries_label(self.crates.entry_count(id))
            )
        } else {
            format!(
                "Delete crate \"{}\" ({})?",
                self.crates.name(id),
                entries_label(self.crates.entry_count(id))
            )
        };
        let mut choice = None;
        let modal = egui::Modal::new(Id::new("delete-crate")).show(ctx, |ui| {
            ui.label(question);
            ui.horizontal(|ui| {
                if ui
                    .button(if label { "Delete label" } else { "Delete" })
                    .clicked()
                {
                    choice = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(false);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(false);
        }
        if let Some(delete) = choice {
            self.confirm_delete = None;
            if delete {
                self.delete_crate(id);
            }
        }
    }

    // ---- crates ------------------------------------------------------------------------------

    /// The crate menu under the playlist title bar.
    fn crate_menu(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let (shown, playing) = (self.crates.shown_id(), self.crates.playing_id());
        // ⏵ marks the crate only while its track plays or is paused.
        let live = self.position.state != PlayState::Stopped;
        // The user's crates, then their Discogs collection under its own heading.
        let (mine, collection): (Vec<_>, Vec<_>) =
            self.crates.list().iter().partition(|c| !c.discogs());
        let mut collection = collection;
        collection.sort_by_key(|c| !c.wantlist);
        let discogs = collection.clone();
        for (i, group) in [mine, collection].into_iter().enumerate() {
            if i == 1 && !group.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new("Discogs").weak().small());
            }
            for c in group {
                let unreadable = self.crates.is_unreadable(c.id);
                let label =
                    crate_menu_label(&c.name, c.id == shown, live && c.id == playing, unreadable);
                if ui
                    .add_enabled(!unreadable, egui::Button::selectable(c.id == shown, label))
                    .clicked()
                {
                    actions.push(Action::ShowCrate(c.id));
                }
            }
        }
        // Refresh wantlist / collection, for the Discogs crates (with a token).
        #[cfg(not(target_arch = "wasm32"))]
        if self.dig.as_ref().is_some_and(|d| d.has_token()) && !discogs.is_empty() {
            ui.separator();
            for c in discogs {
                self.dig_refresh_item(ui, c, actions);
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = discogs;
        ui.separator();
        if ui.button("New crate…").clicked() {
            actions.push(Action::NewCrate);
        }
        if self.crates.is_label(shown) {
            ui.separator();
            self.label_menu(ui, shown, actions);
            return;
        }
        let editable = shown != PLAYLIST;
        if ui
            .add_enabled(editable, egui::Button::new("Rename crate…"))
            .clicked()
        {
            actions.push(Action::RenameCrate(shown));
        }
        if ui
            .add_enabled(editable, egui::Button::new("Delete crate…"))
            .clicked()
        {
            actions.push(Action::DeleteCrate(shown));
        }
    }

    /// Export to crate: every entry of label crate `from` copied into `to`, in order, without
    /// what `to` already holds. The label crate is unchanged.
    fn export_label(&mut self, from: CrateId, to: CrateId) {
        if !self.crates.load(from) || self.refuse_discogs_insert(to) {
            return;
        }
        let ids: Vec<EntryId> = self
            .crates
            .get(from)
            .map(|p| p.entries().iter().map(|e| e.id).collect())
            .unwrap_or_default();
        match self.crates.send(from, &ids, to) {
            Ok(n) => {
                self.mark_crate(to);
                self.notify(format!(
                    "{} › {}: {}",
                    self.crates.name(from),
                    self.crates.name(to),
                    match n {
                        0 => "nothing new".to_owned(),
                        n => entries_label(n),
                    }
                ));
            }
            Err(e) => self.notify(e),
        }
    }

    /// A label crate's menu, in the sidebar and the title-bar crate menu: Delete label…,
    /// Export to crate ▸ (the normal crates, then New crate…), Refresh label, and Download all
    /// tracks.
    fn label_menu(&self, ui: &mut Ui, c: CrateId, actions: &mut Vec<Action>) {
        if ui.button("Delete label…").clicked() {
            actions.push(Action::DeleteLabel(c));
            ui.close();
        }
        ui.menu_button("Export to crate", |ui| {
            for (id, name) in self.send_targets() {
                if id == c || self.crates.seller_of(id).is_some() {
                    continue;
                }
                if ui.button(name).clicked() {
                    actions.push(Action::ExportLabel(c, Some(id)));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("New crate…").clicked() {
                actions.push(Action::ExportLabel(c, None));
                ui.close();
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        {
            let busy = self.label_refreshing(c);
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(if busy {
                        "Refreshing…"
                    } else {
                        "Refresh label"
                    }),
                )
                .on_hover_text("Read the label's page again: only new records come in")
                .clicked()
            {
                actions.push(Action::Dig(DigAction::RefreshLabel(c)));
                ui.close();
            }
            let failed = self.retryable(c).0.len();
            if failed > 0
                && self.label_download().is_none_or(|(d, _, _)| d != c)
                && ui
                    .button(format!("Retry failed tracks ({failed})"))
                    .on_hover_text(
                        "Download the \"clip failed\" tracks again and search the \"not found\" \
                         ones again",
                    )
                    .clicked()
            {
                actions.push(Action::Dig(DigAction::RetryFailed(c)));
                ui.close();
            }
            match self.label_download() {
                Some((d, done, total)) if d == c => {
                    if ui
                        .button(format!("Downloading ({done} of {total})…"))
                        .on_hover_text("Show its progress, and Stop")
                        .clicked()
                    {
                        actions.push(Action::Dig(DigAction::ShowDownload));
                        ui.close();
                    }
                }
                other => {
                    let tip = match other {
                        Some(_) => {
                            "Downloads every preview of this label (stops the other label's)"
                        }
                        None => {
                            "Downloads every preview of this label to the cache, in the background"
                        }
                    };
                    if ui
                        .button("Download all tracks")
                        .on_hover_text(tip)
                        .clicked()
                    {
                        actions.push(Action::Dig(DigAction::DownloadLabel(c)));
                        ui.close();
                    }
                }
            }
        }
    }

    /// Shows a crate in the window; playback carries on with its own crate.
    fn show_crate(&mut self, id: CrateId) {
        if self.crates.show(id) {
            self.pl_scroll = 0;
            self.pl_drag_from = None;
        }
    }

    /// Deletes a crate; deleting the playing crate stops playback.
    fn delete_crate(&mut self, id: CrateId) {
        if id == self.crates.playing_id() {
            self.with_engine(|e| e.stop());
            self.queue_dirty = true;
        }
        if self.armed.is_some_and(|(c, _)| c == id) {
            self.armed = None;
        }
        match self.crates.delete(id) {
            Ok(()) => {
                #[cfg(not(target_arch = "wasm32"))]
                self.dig_crate_deleted(id);
            }
            Err(e) => self.notify(e),
        }
    }

    /// Copies entries of the shown crate to another one.
    fn send_to(&mut self, ids: &[EntryId], to: CrateId) {
        if self.refuse_discogs_insert(to) {
            return;
        }
        match self.crates.send(self.crates.shown_id(), ids, to) {
            Ok(sent) => {
                if sent > 0 && to == self.crates.playing_id() {
                    self.queue_dirty = true;
                }
                let skipped = ids.len() - sent;
                let mut text = format!("Sent {} to {}", entries_label(sent), self.crates.name(to));
                if skipped > 0 {
                    text += &format!(" ({skipped} already there)");
                }
                self.notify(text);
            }
            Err(e) => self.notify(e),
        }
    }

    fn save_presets(&mut self) {
        if let Some(s) = &self.store
            && let Err(e) = s.save(PRESETS_FILE, &self.presets)
        {
            self.notify(format!("Could not save presets: {e}"));
        }
    }

    fn apply(&mut self, a: Action, ctx: &egui::Context) {
        match a {
            #[cfg(not(target_arch = "wasm32"))]
            Action::Dig(a) => self.dig_act(self.crates.shown_id(), a),
            Action::Prev => {
                self.armed = None;
                self.with_engine(|e| e.previous());
            }
            Action::Play => self.play(),
            Action::Pause => self.with_engine(|e| e.toggle_pause()),
            Action::Stop => self.with_engine(|e| e.stop()),
            Action::Next => {
                self.armed = None;
                self.with_engine(|e| e.next());
            }
            Action::Eject => self.open_files_dialog(Open::Replace),
            Action::Seek(s) => self.with_engine(|e| e.seek(s)),
            Action::Volume(v) => self.set_volume(v),
            Action::Shuffle => {
                self.settings.shuffle = !self.settings.shuffle;
                self.queue_dirty = true;
                self.mark_settings();
            }
            Action::Repeat => {
                self.settings.repeat = self.settings.repeat.cycle();
                let r = self.settings.repeat.to_engine();
                self.with_engine(|e| e.set_repeat(r));
                self.mark_settings();
            }
            Action::ToggleWaveform => {
                self.settings.show_waveform = !self.settings.show_waveform;
                self.mark_settings();
            }
            Action::ToggleEq => {
                self.settings.show_eq = !self.settings.show_eq;
                self.mark_settings();
            }
            Action::TogglePlaylist => {
                if self.settings.playlist_maximized {
                    // Hiding the playlist leaves nothing to maximize.
                    self.set_maximized(ctx, false);
                }
                self.settings.show_playlist = !self.settings.show_playlist;
                self.mark_settings();
            }
            Action::ToggleMaximized => {
                self.set_maximized(ctx, !self.settings.playlist_maximized);
            }
            Action::ToggleRemaining => {
                self.settings.time_remaining = !self.settings.time_remaining;
                self.mark_settings();
            }
            Action::CycleVis => {
                self.settings.vis = self.settings.vis.cycle();
                self.mark_settings();
            }
            Action::ToggleSpectrogram => self.spectro_open = !self.spectro_open,
            Action::ToggleScale => {
                self.settings.scale = if self.settings.scale >= 2 { 1 } else { 2 };
                self.mark_settings();
            }
            Action::ResizePlaylist { rows, width } => {
                // From the rows shown, which may be more than chosen (never shorter than the
                // player column).
                let shown = crate::layout::playlist_rows(&self.settings, &self.skin.def);
                self.settings.playlist_rows = (shown as i32 + rows).clamp(4, 60) as u16;
                self.settings.playlist_width = (self.settings.playlist_width as i32 + width).clamp(
                    crate::settings::MIN_PLAYLIST_WIDTH as i32,
                    crate::settings::MAX_PLAYLIST_WIDTH as i32,
                ) as u16;
                self.mark_settings();
            }
            Action::SavePreset => self.name_dialog = Some(NameDialog::new(NameFor::Preset, "")),
            Action::DeletePreset(name) => {
                self.presets.remove(&name);
                self.save_presets();
            }
            Action::PlayEntry(id) => self.play_entry(self.crates.shown_id(), id),
            Action::ToggleGrouped => {
                let c = self.crates.shown_id();
                let on = !self.crates.is_grouped(c);
                self.crates.set_grouped(c, on);
                self.mark_crate(c);
                if let Some(i) = self.crates.shown().cursor_index() {
                    self.scroll_into_view(i);
                }
            }
            Action::ToggleRecord(key) => {
                let open = self.pl_open.entry(self.crates.shown_id()).or_default();
                if !open.remove(&key) {
                    open.insert(key);
                }
                self.pl_open_rev += 1;
            }
            Action::SelectRecord(ids, mods) => {
                self.side_crate = None;
                let p = self.crates.shown_mut();
                if mods.command {
                    p.toggle_ids(&ids);
                } else if mods.shift {
                    let last = ids.iter().filter_map(|&id| p.index_of(id)).max();
                    if let Some(last) = last {
                        p.extend_to(last, &ids, ids[0]);
                    }
                } else {
                    p.select_only(&ids, ids[0]);
                }
            }
            Action::PlayRecord(ids) => {
                let p = self.crates.shown();
                let pick = ids
                    .iter()
                    .copied()
                    .find(|&id| p.get(id).is_some_and(|e| e.status.is_playable()))
                    .or_else(|| {
                        ids.iter()
                            .copied()
                            .find(|&id| p.get(id).is_some_and(|e| e.status.in_play_order()))
                    });
                if let Some(id) = pick {
                    self.play_entry(self.crates.shown_id(), id);
                }
            }
            Action::MoveBlock(ids, before) => {
                self.crates.shown_mut().move_block(&ids, before);
                self.mark_shown();
            }
            Action::Select(i, m) => {
                self.side_crate = None;
                self.crates.shown_mut().click(i, m);
            }
            Action::Move(from, to) => {
                self.crates.shown_mut().move_entry(from, to);
                self.mark_shown();
            }
            Action::ToggleColumn(f) => {
                self.settings.columns.toggle(f);
                self.mark_settings();
            }
            Action::ResizeColumn(f, share) => {
                self.settings.columns.resize(f, share);
                self.mark_settings();
            }
            Action::SetBpmFilter(range) => {
                if self.crates.shown_mut().set_bpm_filter(range) {
                    self.mark_shown();
                    let rows = self.pl_list().len();
                    self.pl_scroll = self.pl_scroll.min(rows.saturating_sub(1));
                }
            }
            Action::TogglePick(f, value) => {
                let p = self.crates.shown_mut();
                let on = !p.picked(f, &value);
                p.set_pick(f, &value, on);
                self.filter_changed();
            }
            Action::ClearPicks(f) => {
                if self.crates.shown_mut().clear_picks(f) {
                    self.filter_changed();
                }
            }
            Action::SetSearch(text) => {
                if self.crates.shown_mut().set_search(&text) {
                    self.filter_changed();
                }
            }
            Action::PlayFirstShown | Action::SearchToList => {
                let p = self.crates.shown();
                let first = p
                    .shown_rows()
                    .into_iter()
                    .map(|i| &p.entries()[i])
                    .find(|e| e.status.in_play_order())
                    .map(|e| e.id);
                if matches!(a, Action::SearchToList) {
                    self.focus = Focus::Playlist;
                    self.crates.shown_mut().set_cursor(first);
                } else if let Some(id) = first {
                    self.apply(Action::PlayEntry(id), ctx);
                }
            }
            Action::ToggleCartOnly => {
                let p = self.crates.shown_mut();
                let on = !p.cart_only();
                p.set_cart_only(on);
                self.filter_changed();
            }
            Action::OnlyPick(f, v) => {
                let p = self.crates.shown_mut();
                p.clear_picks(Some(f));
                p.set_pick(f, &v, true);
                self.filter_changed();
            }
            Action::SearchFor(text) => {
                self.pl_search = text;
                self.search_focus = true;
                if self.crates.shown_mut().set_search(&self.pl_search) {
                    self.filter_changed();
                }
            }
            Action::ClearFilters => {
                if self.crates.shown_mut().clear_filters() {
                    self.filter_changed();
                }
            }
            Action::Sort(field) => {
                // The same field again sorts the other way.
                let p = self.crates.shown_mut();
                let dir = match p.sorted() {
                    Some((f, Dir::Asc)) if f == field => Dir::Desc,
                    _ => Dir::Asc,
                };
                p.sort_by(field, dir);
                if let Some(i) = p.cursor_index() {
                    self.scroll_into_view(i);
                }
                self.mark_shown();
            }
            Action::AddFiles => self.open_files_dialog(Open::Add),
            Action::AddFolder => self.open_folder_dialog(),
            Action::RemoveSelected => self.remove_selected(),
            Action::Clear => self.apply(Action::ClearCrate(self.crates.shown_id()), ctx),
            Action::ClearCrate(id) => {
                if !self.refuse_discogs_edit(id)
                    && self.crates.load(id)
                    && let Some(p) = self.crates.get_mut(id)
                {
                    p.clear();
                    if id == self.crates.playing_id() {
                        self.with_engine(|e| e.stop());
                    }
                    self.mark_crate(id);
                }
            }
            Action::SelectAll => self.crates.shown_mut().select_all(),
            Action::SelectNone => self.crates.shown_mut().select_none(),
            Action::InvertSelection => self.crates.shown_mut().invert_selection(),
            Action::ExportM3u => self.export_m3u(),
            Action::ShowCrate(id) => self.show_crate(id),
            Action::DeleteLabel(id) => self.confirm_delete = Some(id),
            Action::ExportLabel(from, Some(to)) => self.export_label(from, to),
            Action::ExportLabel(from, None) => {
                let name = self.crates.name(from);
                let name = name.strip_prefix("Label: ").unwrap_or(name).to_owned();
                self.name_dialog = Some(NameDialog::new(NameFor::ExportLabel(from), name));
            }
            Action::ToggleLabelsFold => {
                self.settings.labels_folded = !self.settings.labels_folded;
                self.mark_settings();
            }
            Action::MoveToLabels(id, label) => {
                if self.crates.find_label(label).is_none() {
                    self.crates.set_label(id, label);
                    self.notify(format!(
                        "{} is under LABELS: it fills from its label now",
                        self.crates.name(id)
                    ));
                }
            }
            Action::NewCrate => self.name_dialog = Some(NameDialog::new(NameFor::NewCrate, "")),
            Action::RenameCrate(id) => {
                let name = self.crates.name(id).to_owned();
                self.name_dialog = Some(NameDialog::new(NameFor::RenameCrate(id), name));
            }
            Action::DeleteCrate(id) => {
                if self.crates.entry_count(id) > 0 {
                    self.confirm_delete = Some(id);
                } else {
                    self.delete_crate(id);
                }
            }
            Action::RemoveEntry(entry) => {
                if self.crates.shown().is_selected(entry) {
                    self.remove_selected();
                } else {
                    self.remove_entries(&[entry]);
                }
            }
            Action::RemoveAlbum(entry) => {
                let album = self.crates.shown().album_of(entry);
                self.remove_entries(&album);
            }
            Action::SelectAlbum(entry) => {
                let p = self.crates.shown_mut();
                let album = p.album_of(entry);
                if !album.is_empty() {
                    p.select_only(&album, entry);
                }
            }
            Action::DropOnCrate(entry, to) => {
                if to != self.crates.shown_id() {
                    let shown = self.crates.shown();
                    let ids = if shown.is_selected(entry) {
                        shown.selected_ids()
                    } else {
                        vec![entry]
                    };
                    self.send_to(&ids, to);
                }
            }
            Action::SendTo(entry, to) => {
                let shown = self.crates.shown();
                let ids = if shown.is_selected(entry) {
                    shown.selected_ids()
                } else {
                    vec![entry]
                };
                match to {
                    Some(to) => self.send_to(&ids, to),
                    None => self.name_dialog = Some(NameDialog::new(NameFor::SendToNew(ids), "")),
                }
            }
        }
        let _ = ctx;
    }

    fn save_now(&mut self, force: bool) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let due = |t: Option<Instant>| t.is_some_and(|t| force || t.elapsed() > SAVE_DELAY);
        if due(self.settings_dirty) {
            self.settings_dirty = None;
            if let Err(e) = store.save(SETTINGS_FILE, &self.settings) {
                self.notify(format!("Could not save settings: {e}"));
            }
        }
        for e in self.crates.save_due(force, SAVE_DELAY) {
            self.notify(e);
        }
    }
}

/// A crate placed in the sidebar: its row, and for a seller crate whether it was dug and
/// is being refreshed.
type Placed = (crate::crates::CrateInfo, usize, Option<(bool, bool)>);

/// A row of the sidebar's Discogs group.
enum SideRow {
    Discogs,
    /// LABELS: how many, and whether folded.
    Labels(usize, bool),
    /// TOP SELLERS: how many, and whether folded.
    Sellers(usize, bool),
    /// A crate; for a seller crate, whether it was dug and is being refreshed.
    Crate(crate::crates::CrateInfo, Option<(bool, bool)>),
}

enum Action {
    /// Group the shown crate by record, or show it flat.
    ToggleGrouped,
    /// Open or close a record row of the shown crate.
    ToggleRecord(AlbumKey),
    /// A click on a record row: these entries, with these modifiers.
    SelectRecord(Vec<EntryId>, ClickMods),
    /// Plays a record: its first playable track, or arms its first one.
    PlayRecord(Vec<EntryId>),
    /// Moves a record (these entries) before an entry, or to the end.
    MoveBlock(Vec<EntryId>, Option<EntryId>),
    Prev,
    Play,
    Pause,
    Stop,
    Next,
    Eject,
    Seek(f64),
    Volume(f32),
    ToggleWaveform,
    /// Maximize the playlist beside a thin player strip, or restore the normal layout.
    ToggleMaximized,
    /// Sort the shown crate by a column (again: the other way).
    Sort(Field),
    /// Show (and play) only the shown crate's entries in this BPM range; `None` shows all.
    SetBpmFilter(Option<(u16, u16)>),
    /// Picks or unpicks a value of the shown crate's style, artist or label filter.
    TogglePick(Facet, String),
    /// Turns a style, artist or label filter off, or all three with `None`.
    ClearPicks(Option<Facet>),
    /// The CART switch of the shown (seller) crate.
    ToggleCartOnly,
    /// The filter bar's search text changed.
    SetSearch(String),
    /// Enter in the search field: play the first shown entry.
    PlayFirstShown,
    /// ↓ in the search field: the list takes the keyboard, its cursor on the first shown entry.
    SearchToList,
    /// The filter bar's ×: the search and every filter off.
    ClearFilters,
    /// Only this value in its filter (the entry menu).
    OnlyPick(Facet, String),
    /// Search this text, the field taking the keyboard (the entry menu).
    SearchFor(String),
    ToggleColumn(Field),
    /// Widen a column by a share of the list's width.
    ResizeColumn(Field, f32),
    /// Remove an entry, or the whole selection when it is part of it (the entry menu).
    RemoveEntry(EntryId),
    /// Remove every entry of this entry's album (the entry menu).
    RemoveAlbum(EntryId),
    /// Select exactly this entry's album, with the cursor on it (the entry menu).
    SelectAlbum(EntryId),
    Shuffle,
    Repeat,
    ToggleEq,
    TogglePlaylist,
    ToggleRemaining,
    CycleVis,
    ToggleScale,
    ToggleSpectrogram,
    /// Rows and skin pixels to add to the playlist.
    ResizePlaylist {
        rows: i32,
        width: i32,
    },
    SavePreset,
    DeletePreset(String),
    PlayEntry(EntryId),
    Select(usize, ClickMods),
    Move(usize, usize),
    AddFiles,
    AddFolder,
    RemoveSelected,
    Clear,
    SelectAll,
    SelectNone,
    InvertSelection,
    ExportM3u,
    ShowCrate(CrateId),
    NewCrate,
    /// Rename or delete a crate (from the crate menu or the sidebar).
    RenameCrate(CrateId),
    DeleteCrate(CrateId),
    /// Fold or unfold LABELS in the sidebar.
    ToggleLabelsFold,
    /// Delete label…: asks, then stops following the label and deletes its crate.
    DeleteLabel(CrateId),
    /// Export to crate: copy a label crate's entries into a crate, or (`None`) a new one.
    ExportLabel(CrateId, Option<CrateId>),
    /// Make a crate filled from a label's page that label's crate (LABELS).
    MoveToLabels(CrateId, u64),
    /// Send the dragged entries (the selection when the dragged one is in it) to a crate.
    DropOnCrate(EntryId, CrateId),
    /// Empty a crate (Clear crate in ≡ acts on the shown one).
    ClearCrate(CrateId),
    /// Send an entry (with the rest of the selection, when it is selected) to a crate, or to
    /// a new one.
    SendTo(EntryId, Option<CrateId>),
    #[cfg(not(target_arch = "wasm32"))]
    Dig(DigAction),
}

/// How opened files are used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Open {
    /// Add to the shown crate.
    Add,
    /// Add to the shown crate and play the first one.
    AddAndPlay,
    /// Replace the Playlist crate and play it (Eject).
    Replace,
}

struct NameDialog {
    purpose: NameFor,
    text: String,
    error: Option<String>,
}

impl NameDialog {
    fn new(purpose: NameFor, text: impl Into<String>) -> Self {
        Self {
            purpose,
            text: text.into(),
            error: None,
        }
    }
}

enum NameFor {
    Preset,
    NewCrate,
    RenameCrate(CrateId),
    /// A new crate for these entries of the shown crate.
    SendToNew(Vec<EntryId>),
    /// A new crate for a copy of this label crate.
    ExportLabel(CrateId),
}

impl NameFor {
    /// Window title and confirm button.
    fn labels(&self) -> (&'static str, &'static str) {
        match self {
            NameFor::Preset => ("Save EQ preset", "Save"),
            NameFor::NewCrate | NameFor::SendToNew(_) | NameFor::ExportLabel(_) => {
                ("New crate", "Create")
            }
            NameFor::RenameCrate(_) => ("Rename crate", "Rename"),
        }
    }
}

/// "1 record", "3 records".
fn records_label(n: usize) -> String {
    if n == 1 {
        "1 record".into()
    } else {
        format!("{n} records")
    }
}

fn entries_label(n: usize) -> String {
    if n == 1 {
        "1 entry".into()
    } else {
        format!("{n} entries")
    }
}

/// A crate in the crate menu: • marks the shown crate, ⏵ the playing one (marks egui's default
/// fonts can draw).
fn crate_menu_label(name: &str, shown: bool, playing: bool, unreadable: bool) -> String {
    let mut label = format!("{} {name}", if shown { "•" } else { "  " });
    if playing {
        label += "  ⏵";
    }
    if unreadable {
        label += " (unreadable)";
    }
    label
}

/// Colour and right-hand text of a playlist row: the duration, or a waiting or unavailable
/// entry's note, dimmed. Only files that couldn't be opened are drawn in the error colour.
/// The first visible row after scrolling by `dy` points (positive: towards the top), with rows
/// `row` points high. Partial rows add up in `acc`; they are dropped at either end of the list.
fn scroll_rows(acc: &mut f32, first: usize, max: usize, dy: f32, row: f32) -> usize {
    *acc += dy;
    let steps = (*acc / row).trunc();
    *acc -= steps * row;
    let to = first as isize - steps as isize;
    // Pushing past an end leaves nothing to carry over.
    if (to <= 0 && *acc > 0.0) || (to >= max as isize && *acc < 0.0) {
        *acc = 0.0;
    }
    to.clamp(0, max as isize) as usize
}

/// Seconds since the Unix epoch.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// What the playlist's rows were built from: the crate, its revision, whether it's grouped,
/// and the open records' revision.
type RowsKey = (CrateId, u64, bool, u64);

/// How record rows are drawn this frame.
struct RecordLook<'a> {
    colors: &'a crate::skin::Colors,
    font: &'a egui::FontId,
    mods: ClickMods,
}

/// What an entry's tooltip shows where its record's cover goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoverSlot {
    /// The cover, at its own size.
    Loaded(egui::TextureId, egui::Vec2),
    /// The cover's space, empty until it arrives.
    Waiting,
}

/// The cover's side in an entry tooltip, in points.
const TOOLTIP_COVER: f32 = 96.0;

/// An entry's tooltip: its full name, then its cover (when it has one) beside one labelled
/// line per known detail.
fn entry_tooltip(ui: &mut Ui, details: &[(&str, String)], cover: Option<CoverSlot>) {
    let mut lines = details.iter();
    if let Some((_, name)) = lines.next() {
        ui.label(egui::RichText::new(name).strong());
    }
    let grid = |ui: &mut Ui| {
        egui::Grid::new("entry_details")
            .num_columns(2)
            .spacing(vec2(10.0, 2.0))
            .show(ui, |ui| {
                for (label, value) in lines {
                    ui.label(egui::RichText::new(*label).weak());
                    ui.label(value);
                    ui.end_row();
                }
            });
    };
    let Some(cover) = cover else {
        return grid(ui);
    };
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(TOOLTIP_COVER), Sense::hover());
        match cover {
            CoverSlot::Loaded(tex, size) => {
                // Fitted into the square, centred, its shape kept.
                let fit = size * (TOOLTIP_COVER / size.x.max(size.y).max(1.0));
                let at = Rect::from_center_size(rect.center(), fit);
                ui.painter().image(
                    tex,
                    at,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            CoverSlot::Waiting => {
                let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
                ui.painter()
                    .rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
            }
        }
        ui.vertical(grid);
    });
}

/// What a playlist row shows where its duration goes.
#[derive(Debug, Clone, PartialEq)]
enum RowEnd {
    /// The duration (or nothing while it isn't known).
    Text(String),
    /// A status icon (a skin sprite, tinted with the row's colour).
    Icon(&'static str),
    /// A download's progress, in percent.
    Bar(u8),
}

/// Width of the download bar, in skin pixels.
const ROW_BAR_W: f32 = 30.0;

fn row_look(
    e: &crate::playlist::Entry,
    current: bool,
    colors: &crate::skin::Colors,
) -> (Color32, RowEnd) {
    use crate::playlist::{UnavailableKind as U, WaitKind as W};
    let base = if current {
        colors.pl_current
    } else {
        colors.pl_text
    };
    let dim = lerp_color(base, colors.pl_bg, 0.55);
    match &e.status {
        EntryStatus::Failed => (
            Color32::from_rgb(170, 60, 60),
            RowEnd::Text(duration_text(e)),
        ),
        EntryStatus::Waiting(w) => (
            dim,
            match w {
                W::Listed => RowEnd::Icon("st_listed"),
                W::Queued | W::Search => RowEnd::Icon("st_queued"),
                W::Downloading(p) => RowEnd::Bar(*p),
                W::NeedsYtDlp => RowEnd::Icon("st_needs_tool"),
                W::Other(_) => RowEnd::Icon("st_other"),
            },
        ),
        EntryStatus::Unavailable(u) => (
            dim,
            RowEnd::Icon(match u {
                U::NoClip | U::ClipFailed | U::NotFound | U::AlreadyInCrate => "st_unavailable",
                U::Other(_) => "st_other",
            }),
        ),
        _ => (color(base), RowEnd::Text(duration_text(e))),
    }
}

/// Draws a row's end (duration, icon or download bar) against the right of `row`, in `col`;
/// returns its width in points.
fn draw_row_end(
    sk: &Skinned,
    painter: &egui::Painter,
    row: Rect,
    end: &RowEnd,
    font: &egui::FontId,
    col: Color32,
) -> f32 {
    let s = sk.scale;
    let right = row.right() - 3.0 * s;
    match end {
        RowEnd::Text(t) => painter
            .text(
                pos2(right, row.center().y),
                egui::Align2::RIGHT_CENTER,
                t,
                font.clone(),
                col,
            )
            .width(),
        RowEnd::Icon(name) => {
            let size = 9.0 * s;
            let r = Rect::from_min_size(
                pos2(right - size, row.center().y - size / 2.0),
                vec2(size, size),
            );
            sk.sprite_tinted(name, r, col);
            size
        }
        RowEnd::Bar(pct) => {
            let (w, h) = (ROW_BAR_W * s, 3.0 * s);
            let track = Rect::from_min_size(pos2(right - w, row.center().y - h / 2.0), vec2(w, h));
            painter.rect_filled(track, 0.0, col.gamma_multiply(0.35));
            let done = w * (*pct).min(100) as f32 / 100.0;
            painter.rect_filled(Rect::from_min_size(track.min, vec2(done, h)), 0.0, col);
            w
        }
    }
}

fn duration_text(e: &crate::playlist::Entry) -> String {
    e.duration.map(format::clock).unwrap_or_default()
}

/// The playlist title bar's text: the crate name folded to the skin font's upper case, without
/// characters the font lacks, cut to `max_w` skin pixels.
/// The crate's title with, while a BPM filter is on, how many entries show of how many
/// ("LOWTIDE TAPES · 42/301"). The name is shortened first; the count always shows.
pub fn crate_title_with_count(
    def: &crate::skin::SkinDef,
    name: &str,
    count: Option<(usize, usize)>,
    max_w: f32,
) -> String {
    let Some((shown, all)) = count else {
        return crate_title(def, name, max_w);
    };
    let suffix = format!(" · {shown}/{all}");
    let suffix_w = suffix.chars().count() as f32 * def.font.advance as f32;
    let name = crate_title(def, name, max_w - suffix_w);
    format!("{name}{suffix}").trim().to_owned()
}

pub fn crate_title(def: &crate::skin::SkinDef, name: &str, max_w: f32) -> String {
    let advance = def.font.advance as f32;
    let fit = ((max_w + 1.0) / advance).floor().max(0.0) as usize;
    let text: String = name
        .chars()
        .map(crate::skin::fold)
        .filter(|&c| def.glyph(c).is_some())
        .take(fit)
        .collect();
    text.trim().to_owned()
}

/// Draws the shown crate's name centred on the playlist title bar, over a plain strip that
/// hides the bar's decorative lines behind it.
/// The badges before an entry's name: OWNED (dark letters on the skin's amber), then the
/// dim format mark of a record not on vinyl ("FILE", "CD"…), left edge centred on `at`.
/// Returns their width with the gap after them (0 for none).
/// The marks drawn before a row's name.
#[derive(Debug, Clone, Copy, Default)]
struct Badges {
    owned: bool,
    /// A copy is in the user's Discogs cart.
    cart: bool,
    /// No longer for sale (a copy, or a record whose copies all sold).
    sold: bool,
    /// The cart pill of a seller crate's copy, drawn where CART would be (instead of it).
    pill: Option<Pill>,
}

/// A copy's cart pill: + CART, or IN CART that reads REMOVE while hovered.
#[derive(Debug, Clone, Copy)]
struct Pill {
    in_cart: bool,
    /// The pointer, when it is over the pill's row (and nothing covers it): the pill takes
    /// the row's full height, so only its x matters.
    pointer: Option<Pos2>,
}

impl Badges {
    fn any(self) -> bool {
        self.owned || self.cart || self.sold || self.pill.is_some()
    }
}

/// CART's fill: apart from OWNED's amber, whatever the skin.
const CART_FILL: [u8; 3] = [64, 196, 232];
const SOLD_FILL: [u8; 3] = [196, 84, 84];

/// The pill's labels; it is as wide as the widest, so hovering never moves the row's text.
const PILL_LABELS: [&str; 3] = ["+ CART", "IN CART", "REMOVE"];

/// The badges' font, from the row's.
fn badge_font(font: &egui::FontId) -> egui::FontId {
    egui::FontId::proportional(font.size * 0.78)
}

/// The width of a cart pill's label area: its widest label.
fn pill_text_width(painter: &egui::Painter, font: &egui::FontId) -> f32 {
    PILL_LABELS
        .iter()
        .map(|l| {
            painter
                .layout_no_wrap((*l).into(), badge_font(font), Color32::WHITE)
                .size()
                .x
        })
        .fold(0.0, f32::max)
}

/// Draws the badges; returns their width with the gap after them (0 for none), and where the
/// cart pill went, for its click.
fn entry_badges(
    painter: &egui::Painter,
    at: Pos2,
    marks: Badges,
    format: Option<&str>,
    font: &egui::FontId,
    colors: &crate::skin::Colors,
    scale: f32,
) -> (f32, Option<Rect>) {
    let small = badge_font(font);
    let pad = vec2(2.5 * scale, 0.5 * scale);
    let bg = color(colors.pl_bg);
    let mut x = at.x;
    let mut pill_rect = None;
    if marks.owned {
        x += badge(
            painter,
            x,
            at.y,
            "OWNED",
            color(colors.pl_owned),
            &small,
            bg,
            scale,
        );
    }
    match marks.pill {
        Some(p) => {
            let galley_h = painter
                .layout_no_wrap("C".into(), small.clone(), bg)
                .size()
                .y;
            let rect = Rect::from_min_size(
                pos2(x, at.y - galley_h / 2.0 - pad.y),
                vec2(pill_text_width(painter, font), galley_h) + pad * 2.0,
            );
            let cart = color(CART_FILL);
            let r = 2.0 * scale;
            let hovered = p.pointer.is_some_and(|q| rect.x_range().contains(q.x));
            let (label, text) = match (p.in_cart, hovered) {
                (true, true) => {
                    painter.rect_filled(rect, r, color(SOLD_FILL));
                    ("REMOVE", bg)
                }
                (true, false) => {
                    painter.rect_filled(rect, r, cart);
                    ("IN CART", bg)
                }
                (false, hovered) => {
                    if hovered {
                        painter.rect_filled(rect, r, cart.gamma_multiply(0.3));
                    }
                    let stroke = egui::Stroke::new(scale, cart);
                    painter.rect_stroke(rect, r, stroke, egui::StrokeKind::Inside);
                    ("+ CART", cart)
                }
            };
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                small.clone(),
                text,
            );
            pill_rect = Some(rect);
            x += rect.width() + 3.0 * scale;
        }
        None if marks.cart => {
            x += badge(
                painter,
                x,
                at.y,
                "CART",
                color(CART_FILL),
                &small,
                bg,
                scale,
            );
        }
        None => {}
    }
    if marks.sold {
        x += badge(
            painter,
            x,
            at.y,
            "SOLD",
            color(SOLD_FILL),
            &small,
            bg,
            scale,
        );
    }
    if let Some(f) = format {
        let fill = lerp_color(colors.pl_text, colors.pl_bg, 0.55);
        x += badge(painter, x, at.y, f, fill, &small, bg, scale);
    }
    (x - at.x, pill_rect)
}

/// An entry's source at the end of its row ("YT", "BC"), outlined, its right edge at `right`;
/// returns the width it takes.
fn source_badge(
    painter: &egui::Painter,
    right: f32,
    y: f32,
    label: &str,
    font: &egui::FontId,
    col: Color32,
    scale: f32,
) -> f32 {
    let small = badge_font(font);
    let dim = col.gamma_multiply(0.7);
    let pad = vec2(2.0 * scale, 0.5 * scale);
    let galley = painter.layout_no_wrap(label.into(), small, dim);
    let size = galley.size() + pad * 2.0;
    let rect = Rect::from_min_size(pos2(right - size.x, y - size.y / 2.0), size);
    let stroke = egui::Stroke::new(scale * 0.75, dim);
    painter.rect_stroke(rect, 2.0 * scale, stroke, egui::StrokeKind::Inside);
    painter.galley(rect.min + pad, galley, dim);
    size.x + 4.0 * scale
}

/// One filled badge at `x`, centred on `y`; returns its width with the gap after it.
#[allow(clippy::too_many_arguments)]
fn badge(
    painter: &egui::Painter,
    x: f32,
    y: f32,
    label: &str,
    fill: Color32,
    font: &egui::FontId,
    text: Color32,
    scale: f32,
) -> f32 {
    let pad = vec2(2.5 * scale, 0.5 * scale);
    let galley = painter.layout_no_wrap(label.into(), font.clone(), text);
    let rect = Rect::from_min_size(
        pos2(x, y - galley.size().y / 2.0 - pad.y),
        galley.size() + pad * 2.0,
    );
    painter.rect_filled(rect, 2.0 * scale, fill);
    painter.galley(rect.min + pad, galley, text);
    rect.width() + 3.0 * scale
}

/// A momentary skin button at any rectangle (the mini player's buttons have no layout entry).
fn sprite_button(ui: &mut Ui, sk: &Skinned, id: &str, rect: Rect, sprite: &str) -> egui::Response {
    let resp = ui.interact(rect, Id::new(id), Sense::click());
    let name = if resp.is_pointer_button_down_on() {
        format!("{sprite}_p")
    } else {
        sprite.to_owned()
    };
    sk.sprite_in(&name, rect);
    resp
}

/// The side that has the keyboard: the player (main, waveform, EQ) or the playlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Player,
    Playlist,
}

impl Focus {
    fn other(self) -> Self {
        match self {
            Focus::Player => Focus::Playlist,
            Focus::Playlist => Focus::Player,
        }
    }
}

/// Title bars of the side without the keyboard are drawn dimmed, as classic players did with their
/// inactive windows. Drawn over the bar, before its buttons.
fn dim_title(sk: &Skinned, layout: &str, lit: bool) {
    if !lit {
        sk.painter
            .rect_filled(sk.at(layout), 0.0, Color32::from_black_alpha(140));
    }
}

/// A playlist bar (`{base}_l`, `{base}_fill`, `{base}_r`) stretched to `width`: the end pieces
/// as drawn, the middle column tiled between them.
fn draw_stretched_bar(sk: &Skinned, base: &str, y: f32, width: f32, h: f32) {
    let (l, r) = (
        sk.def.sprite(&format!("{base}_l")).w as f32,
        sk.def.sprite(&format!("{base}_r")).w as f32,
    );
    sk.sprite(&format!("{base}_l"), 0.0, y);
    sk.sprite_in(
        &format!("{base}_fill"),
        sk.rect(l, y, (width - l - r).max(0.0), h),
    );
    sk.sprite(&format!("{base}_r"), width - r, y);
}

fn draw_crate_name(sk: &Skinned, name: &str, count: Option<(usize, usize)>) {
    const PAD: f32 = 5.0; // plain title bar either side of the name
    const SIDE: f32 = 40.0; // decoration left visible at each end (the close button's side)
    let bar = sk.def.at("pl_titlebar");
    let text = crate_title_with_count(sk.def, name, count, bar.w as f32 - 2.0 * (SIDE + PAD));
    if text.is_empty() {
        return;
    }
    let w = sk.text_width(&text);
    let x = ((bar.w as f32 - w) / 2.0).round();
    sk.sprite_in(
        "pl_title_fill",
        sk.rect(
            bar.x as f32 + x - PAD,
            bar.y as f32,
            w + 2.0 * PAD,
            bar.h as f32,
        ),
    );
    let y = bar.y as f32 + ((bar.h - sk.def.font.glyph_h) / 2) as f32;
    sk.text(bar.x as f32 + x, y, &text, color(sk.def.colors.pl_title));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostAction {
    ToggleStrip,
    ToggleAnnotating,
    Tap,
    Boundary(analysis::SectionKind),
}

/// Fullscreen keys the host keeps instead of passing them to the scene: `T` (analysis strip),
/// `A` (annotation mode), and while annotating `Space` (beat tap) and `1`–`6` (boundary of kind
/// intro, build, drop, breakdown, groove, outro).
pub fn host_action(key: Key, annotating: bool) -> Option<HostAction> {
    use analysis::SectionKind as K;
    Some(match key {
        Key::T => HostAction::ToggleStrip,
        Key::A => HostAction::ToggleAnnotating,
        _ if !annotating => return None,
        Key::Space => HostAction::Tap,
        Key::Num1 => HostAction::Boundary(K::Intro),
        Key::Num2 => HostAction::Boundary(K::Build),
        Key::Num3 => HostAction::Boundary(K::Drop),
        Key::Num4 => HostAction::Boundary(K::Breakdown),
        Key::Num5 => HostAction::Boundary(K::Groove),
        Key::Num6 => HostAction::Boundary(K::Outro),
        _ => return None,
    })
}

/// What the window is doing, for the repaint policy.
#[derive(Debug, Clone, Copy, Default)]
pub struct Activity {
    pub playing: bool,
    /// The waveform section is visible (it scrolls with the playhead).
    pub waveform: bool,
    pub bars_moving: bool,
    pub engine_starting: bool,
    pub hidden: bool,
    pub fullscreen: bool,
}

/// When to repaint without input. `None` means only on input, so an idle window costs nothing.
/// Fullscreen schedules its own frames (one per vsync), so it is not handled here.
pub fn repaint_after(a: Activity) -> Option<Duration> {
    if a.hidden || a.fullscreen {
        None
    } else if a.playing && a.waveform {
        // The waveform scrolls with the playhead: smooth at the display rate.
        Some(Duration::from_millis(16))
    } else if a.playing || a.bars_moving {
        Some(Duration::from_millis(33))
    } else if a.engine_starting {
        Some(Duration::from_millis(50))
    } else {
        None
    }
}

fn engine_status(slot: &EngineSlot) -> String {
    match slot {
        EngineSlot::Starting(_) => "OPENING AUDIO DEVICE...".into(),
        EngineSlot::Ready(_) => "DIGGR - DROP FILES HERE".into(),
        EngineSlot::Failed(e) => format!("AUDIO ERROR: {e}"),
    }
}

/// What a label crate says when the user tries to add to it or remove from it.
const LABEL_CRATE_HINT: &str =
    "Label crates fill from their label: Refresh label brings new records, N passes a track";

/// How long a verdict stays on the title line.
const FLASH_SECS: Duration = Duration::from_millis(1500);

/// A verdict the title line flashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Wanted,
    Unwanted,
    Pass,
    Owned,
}

/// "WANTED", or "WANTED 3" when it took several records.
fn flash_text(verdict: Verdict, count: usize) -> String {
    let word = match verdict {
        Verdict::Wanted => "WANTED",
        Verdict::Unwanted => "UNWANTED",
        Verdict::Pass => "PASS",
        Verdict::Owned => "OWNED",
    };
    if count > 1 {
        format!("{word} {count}")
    } else {
        word.into()
    }
}

/// The paste key's modifier on this platform, as the hints spell it.
const PASTE_MODIFIER: &str = if cfg!(target_os = "macos") {
    "CMD"
} else {
    "CTRL"
};

/// What an empty crate shows in its list: how to fill it, and where the help is. An empty
/// line is a gap.
fn empty_crate_hint(modifier: &str) -> [String; 4] {
    [
        format!("PASTE A DISCOGS LINK · {modifier}+V"),
        "OR DROP FILES".into(),
        String::new(),
        "PRESS H FOR HELP".into(),
    ]
}

/// The BPM slider is this wide, whatever the playlist's width (skin pixels).
const BPM_SLIDER_W: f32 = 40.0;

/// The BPM control's "BPM" label (with its gap) and the rest (slider, gap, widest range).
fn bpm_widths(sk: &Skinned) -> (f32, f32) {
    (
        sk.text_width("BPM") + 4.0,
        BPM_SLIDER_W + 3.0 + sk.text_width("000-000"),
    )
}

/// The narrowest the filter bar's search field gets (skin pixels).
const SEARCH_MIN_W: f32 = 60.0;
/// The widest it gets, as a share of the bar: a maximized playlist doesn't need more.
const SEARCH_MAX_SHARE: f32 = 0.5;
/// A crate's values for a filter, each with its number of records, the most first.
type FacetCounts = Rc<Vec<(String, usize)>>;

/// How the footer shows the style, artist and label filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FooterFilters {
    /// Style chips, then ARTISTS and LABELS.
    Chips,
    /// STYLES, ARTISTS and LABELS.
    Buttons,
    /// One FILTERS button, opening the panel.
    Folded,
    /// Not even that fits.
    None,
}

/// The filter panel, open from a filter's button in the bar.
fn facet_popup_id(f: Facet) -> Id {
    Id::new(("facet_list", f.name()))
}

/// A filter's button in the bar: "ARTISTS", or "ARTISTS 2" while two are picked.
fn facet_button_label(shown: &crate::playlist::Playlist, f: Facet) -> String {
    let name = match f {
        Facet::Style => "STYLES",
        Facet::Artist => "ARTISTS",
        Facet::Label => "LABELS",
        Facet::Format => "FORMATS",
    };
    match shown.filter(f).map(|p| p.len()) {
        Some(n) => format!("{name} {n}"),
        None => name.to_owned(),
    }
}

/// The filter panel, open from the FILTERS button.
fn filters_popup_id() -> Id {
    Id::new("filters_panel")
}
/// Past this many styles, the footer shows the STYLES button and its list instead of chips.
const STYLE_CHIPS_MAX: usize = 20;
/// Skin pixels between two style chips (and before the first).
const STYLE_GAP: f32 = 5.0;

/// The crate sidebar's width, in skin pixels.
const SIDEBAR_W: u16 = 110;
/// The playlist width (skin pixels) from which the crate sidebar fits, unless maximized.
const SIDEBAR_FROM_WIDTH: u16 = 600;

/// A playlist row's widget id, by its index in the crate.
fn row_id(idx: usize) -> Id {
    Id::new(("pl_row", idx))
}

/// The id of a playlist row's context menu (egui's default for a response's popup).
fn row_menu_id(idx: usize) -> Id {
    row_id(idx).with("popup")
}

/// Text drawn left-centred at `at` as `Painter::text` draws it, with the chars a search word
/// covers (see [`crate::playlist::search_marks`]) in `hl`. Returns where it went.
fn text_marked(
    p: &egui::Painter,
    at: Pos2,
    text: String,
    font: &egui::FontId,
    col: Color32,
    hl: Color32,
    words: &[Vec<char>],
) -> Rect {
    let marks = crate::playlist::search_marks(&text, words);
    if !marks.contains(&true) {
        return p.text(at, egui::Align2::LEFT_CENTER, text, font.clone(), col);
    }
    let mut job = egui::text::LayoutJob::default();
    let mut run = String::new();
    let mut lit = marks[0];
    let mut push = |run: &str, lit: bool| {
        let c = if lit { hl } else { col };
        job.append(run, 0.0, egui::TextFormat::simple(font.clone(), c));
    };
    for (c, &m) in text.chars().zip(&marks) {
        if m != lit {
            push(&run, lit);
            run.clear();
            lit = m;
        }
        run.push(c);
    }
    push(&run, lit);
    let galley = p.layout_job(job);
    let rect = egui::Align2::LEFT_CENTER.anchor_size(at, galley.size());
    p.galley(rect.min, galley, col);
    rect
}

fn lerp_color(a: [u8; 3], b: [u8; 3], t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgb(l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2]))
}

impl eframe::App for DiggrApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.logic_inner(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ui_start = Instant::now();
        self.ui_inner(ui);
        if let Some(p) = &mut self.profile {
            p.ui_done(ui_start, self.fullscreen.is_some());
        }
    }

    fn on_exit(&mut self) {
        self.exit();
    }
}

impl DiggrApp {
    fn logic_inner(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if let Some(p) = &mut self.profile {
            p.logic_start(now);
        }
        if self.auto_fullscreen
            && self.fullscreen.is_none()
            && self.position.state == PlayState::Playing
        {
            self.auto_fullscreen = false;
            self.toggle_fullscreen(ctx);
        }
        if self.auto_quit.is_some_and(|t| now >= t) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        let dt = (now - self.last_frame).as_secs_f32().min(0.25);
        self.last_frame = now;
        self.update_audio(dt);
        self.show_playing_bpm();
        self.handle_keys(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        self.dig_update();
        // A grouped crate places what arrived (sends, digs, tags) with its record.
        for c in [self.crates.shown_id(), self.crates.playing_id()] {
            if self.crates.get_mut(c).is_some_and(|p| p.settle()) {
                self.mark_crate(c);
            }
        }

        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.add_paths(dropped, Open::Add);
        }

        // Scroll the title while playing (classic players scroll ~5 characters per second).
        if self.position.state == PlayState::Playing {
            self.title_tick += dt as f64;
            while self.title_tick > 0.2 {
                self.title_tick -= 0.2;
                self.title_offset = self.title_offset.wrapping_add(1);
            }
        }

        // A playlist width saved on a bigger screen is narrowed to fit this one, once.
        if !self.width_fitted
            && let Some(m) = ctx.input(|i| i.viewport().monitor_size)
        {
            self.width_fitted = true;
            let fit = crate::layout::fit_playlist_width(
                self.settings.playlist_width,
                &self.skin.def,
                self.settings.scale as f32,
                m.x,
            );
            if fit != self.settings.playlist_width {
                self.settings.playlist_width = fit;
                self.mark_settings();
            }
        }
        // While the playlist is maximized the OS sizes the window and the layout follows it.
        // Restored from outside (a window manager): back to the normal layout.
        if self.settings.playlist_maximized && self.fullscreen.is_none() {
            match ctx.input(|i| i.viewport().maximized) {
                Some(true) => self.max_seen = true,
                Some(false) if self.max_seen => {
                    self.settings.playlist_maximized = false;
                    self.max_seen = false;
                    self.last_size = None;
                    self.mark_settings();
                }
                _ => {}
            }
        }
        // Keep the window sized to the visible sections (not in fullscreen or maximized).
        if self.fullscreen.is_none() && !self.settings.playlist_maximized {
            let size = Self::window_size(&self.settings, &self.skin);
            if self.last_size != Some(size) {
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
                self.last_size = Some(size);
            }
            if let Some(r) = ctx.input(|i| i.viewport().outer_rect) {
                let pos = Some((r.min.x, r.min.y));
                if self.settings.window_pos != pos && ctx.input(|i| !i.pointer.any_down()) {
                    self.settings.window_pos = pos;
                    self.mark_settings();
                }
            }
        }

        let hidden = ctx.input(|i| {
            i.viewport().minimized.unwrap_or(false) || i.viewport().occluded.unwrap_or(false)
        });
        let activity = Activity {
            playing: self.position.state == PlayState::Playing,
            waveform: self.settings.show_waveform,
            bars_moving: self.analyzer.is_active(),
            engine_starting: matches!(self.engine, EngineSlot::Starting(_)),
            hidden,
            fullscreen: self.fullscreen.is_some(),
        };
        if let Some(after) = repaint_after(activity) {
            ctx.request_repaint_after(after);
        }
        if self.settings_dirty.is_some() || self.crates.is_dirty() {
            ctx.request_repaint_after(SAVE_DELAY);
        }
        self.save_now(false);
        if let Some(p) = &mut self.profile {
            p.logic_end();
        }
    }

    fn ui_inner(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.tex.is_none() {
            let img = egui::ColorImage::from_rgba_unmultiplied(self.skin.size, &self.skin.rgba);
            self.tex = Some(ctx.load_texture("skin-atlas", img, egui::TextureOptions::NEAREST));
        }
        if self.fullscreen.is_some() {
            self.fullscreen_ui(ui);
        } else {
            let origin = ui.max_rect().min;
            let scale = self.settings.scale as f32;
            let d = self.skin.def.clone();
            self.win_px = ui.max_rect().size() / scale;
            let maximized = self.settings.playlist_maximized;
            // A click gives its side the keyboard (lit in this same frame).
            if let Some(p) = ctx.input(|i| {
                i.pointer
                    .primary_pressed()
                    .then(|| i.pointer.interact_pos())
                    .flatten()
            }) {
                // Maximized, the player is the band on top; otherwise the column on the left.
                let on_playlist = if maximized {
                    p.y >= origin.y + crate::layout::BAND_H as f32 * scale
                } else {
                    p.x >= origin.x + d.main_size.0 as f32 * scale
                };
                self.focus = if self.settings.show_playlist && on_playlist {
                    Focus::Playlist
                } else {
                    Focus::Player
                };
            }
            if !self.settings.show_playlist {
                self.focus = Focus::Player;
            }
            if maximized {
                self.maximized_layout(ui, origin);
            } else {
                self.normal_layout(ui, origin);
            }
            if self
                .message
                .as_ref()
                .is_some_and(|(_, at)| at.elapsed() >= Duration::from_secs(4))
            {
                self.message = None;
            }
            // An armed entry's "Waiting for …" stays up until it plays.
            let line = self
                .armed_line()
                .or_else(|| self.message.as_ref().map(|(text, _)| text.clone()))
                .or_else(|| self.dig_line());
            if let Some(text) = line {
                egui::Tooltip::always_open(
                    ctx.clone(),
                    ui.layer_id(),
                    Id::new("msg"),
                    egui::PopupAnchor::Position(origin),
                )
                .show(|ui| ui.label(text));
            }
            self.name_dialog(&ctx);
            self.delete_dialog(&ctx);
            #[cfg(not(target_arch = "wasm32"))]
            self.dig_dialog_ui(&ctx);
            #[cfg(not(target_arch = "wasm32"))]
            self.dig_bridge_dialog_ui(&ctx);
        }
        if self.help {
            crate::help::show(&ctx, &mut self.help);
        }
        self.spectrogram_window(&ctx);
        if !self.first_frame_done {
            self.first_frame_done = true;
            let ms = self.startup.process_start.elapsed().as_secs_f64() * 1e3;
            if self.startup.report {
                println!("startup: first frame after {ms:.1} ms");
            }
            if self.startup.exit_after_first_frame {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    /// The player column (main, waveform, EQ) with the playlist to its right.
    fn normal_layout(&mut self, ui: &mut Ui, origin: Pos2) {
        let scale = self.settings.scale as f32;
        let d = self.skin.def.clone();
        self.main_section(ui, origin);
        let mut y = d.main_size.1 as f32;
        if self.settings.show_waveform {
            let title_h = crate::layout::wave_title_h(&d) as f32;
            if title_h > 0.0 {
                self.wave_title(ui, origin + vec2(0.0, y * scale));
                y += title_h;
            }
            let rect = Rect::from_min_size(
                origin + vec2(0.0, y * scale),
                vec2(d.main_size.0 as f32, crate::waveform::HEIGHT as f32) * scale,
            );
            self.waveform_section(ui, rect);
            y += crate::waveform::HEIGHT as f32;
        }
        if self.settings.show_eq {
            self.eq_section(ui, origin + vec2(0.0, y * scale));
            y += d.eq_size.1 as f32;
        }
        if self.settings.show_playlist {
            // The playlist sits right of the player column; below a shorter player column,
            // plain panel.
            let (_, h) = crate::layout::window_size(&self.settings, &d);
            if (h as f32) > y {
                let def = self.def.clone();
                let sk = self.skinned(&def, ui, origin);
                sk.sprite_in(
                    "pl_bottom_fill",
                    sk.rect(0.0, y, d.main_size.0 as f32, h as f32 - y),
                );
            }
            self.playlist_section(ui, origin + vec2(d.main_size.0 as f32 * scale, 0.0));
        }
    }

    /// The maximized playlist: a band on top with the mini player on the left and the
    /// waveform (when on) beside it, and the playlist under it, filling the window.
    fn maximized_layout(&mut self, ui: &mut Ui, origin: Pos2) {
        let scale = self.settings.scale as f32;
        let (mini_w, band_h) = (self.def.main_size.0 as f32, crate::layout::BAND_H as f32);
        let rest = Rect::from_min_size(
            origin + vec2(mini_w * scale, 0.0),
            vec2(self.win_px.x - mini_w, band_h) * scale,
        );
        if self.settings.show_waveform {
            self.waveform_section(ui, rest);
        } else {
            let def = self.def.clone();
            let sk = self.skinned(&def, ui, rest.min);
            sk.sprite_in(
                "pl_bottom_fill",
                sk.rect(0.0, 0.0, rest.width() / scale, band_h),
            );
        }
        self.mini_player(ui, origin);
        self.playlist_section(ui, origin + vec2(0.0, band_h * scale));
    }

    /// The player folded into the band: play state, time and title on an LCD line; previous,
    /// play or pause, stop, next, volume and ⇔ to restore under it. A right-click off the
    /// controls opens the Options menu.
    fn mini_player(&mut self, ui: &mut Ui, origin: Pos2) {
        let mut def = (*self.def).clone();
        let (w, h) = (def.main_size.0 as f32, crate::layout::BAND_H as f32);
        // The volume slider's place here: `hslider` reads its rectangle from the layout.
        def.layout.insert(
            "mini_volume".into(),
            crate::skin::R {
                x: 104,
                y: 36,
                ..def.at("volume")
            },
        );
        let sk = self.skinned(&def, ui, origin);
        sk.sprite_in("pl_bottom_fill", sk.rect(0.0, 0.0, w, h));
        let mut actions = Vec::new();
        self.options_menu(ui, sk.rect(0.0, 0.0, w, h), "mini_options", &mut actions);
        let colors = &def.colors;
        sk.fill(sk.rect(4.0, 4.0, w - 8.0, 22.0), color(colors.pl_bg));
        let state = self.position.state;
        sk.sprite(
            match state {
                PlayState::Playing => "status_play",
                PlayState::Paused => "status_pause",
                PlayState::Stopped => "status_stop",
            },
            8.0,
            11.0,
        );
        let (mm, ss) = format::lcd(self.position.seconds());
        let digit = |c: char| {
            if state == PlayState::Stopped {
                "digit_blank".to_owned()
            } else {
                format!("digit_{c}")
            }
        };
        let mins: Vec<char> = mm.chars().collect();
        for (i, c) in mins.iter().rev().take(2).enumerate() {
            sk.sprite(&digit(*c), 30.0 - 10.0 * i as f32, 9.0);
        }
        if state != PlayState::Stopped {
            sk.sprite("digit_colon", 40.0, 9.0);
        }
        for (i, c) in ss.chars().enumerate() {
            sk.sprite(&digit(c), 44.0 + 10.0 * i as f32, 9.0);
        }
        let title = self
            .now_playing_line()
            .unwrap_or_else(|| engine_status(&self.engine));
        let (tx, tw) = (70.0, w - 4.0 - 70.0 - 4.0);
        let fits = (tw / def.font.advance as f32) as usize;
        let ty = 4.0 + ((22.0 - def.font.glyph_h as f32) / 2.0).round();
        let shown = format::scroll(&title, fits, self.title_offset);
        sk.text(tx, ty, &shown, color(colors.lcd));
        let (play_sprite, play_action) = if state == PlayState::Playing {
            ("pause", Action::Pause)
        } else {
            ("play", Action::Play)
        };
        for (i, (id, sprite, action)) in [
            ("mini_prev", "prev", Action::Prev),
            ("mini_play", play_sprite, play_action),
            ("mini_stop", "stop", Action::Stop),
            ("mini_next", "next", Action::Next),
        ]
        .into_iter()
        .enumerate()
        {
            let r = sk.rect(4.0 + 23.0 * i as f32, 34.0, 23.0, 18.0);
            if sprite_button(ui, &sk, id, r, sprite).clicked() {
                actions.push(action);
            }
        }
        let vol = SliderSprites {
            track: "volume_track",
            fill: Some("volume_fill"),
            thumb: "volume_thumb",
        };
        if let (_, Some(v)) = widgets::hslider(
            ui,
            &sk,
            "mini_vol",
            "mini_volume",
            vol,
            self.settings.volume,
        ) {
            actions.push(Action::Volume(v));
        }
        let r = sk.rect(w - 13.0, 38.0, 9.0, 9.0);
        if sprite_button(ui, &sk, "mini_max", r, "btn_max").clicked() {
            actions.push(Action::ToggleMaximized);
        }
        for a in actions {
            self.apply(a, ui.ctx());
        }
    }

    /// Feeds and declares the spectrogram window while it is open, and applies its actions.
    fn spectrogram_window(&mut self, ctx: &egui::Context) {
        for action in self.spectro.take_actions() {
            match action {
                crate::spectrogram::Action::Seek(t) => self.with_engine(|e| e.seek(t)),
                crate::spectrogram::Action::Close => self.spectro_open = false,
                crate::spectrogram::Action::Settings(s) => {
                    self.settings.spectrogram = s;
                    self.mark_settings();
                }
            }
        }
        if !self.spectro_open {
            return;
        }
        let playing = self.position.state != PlayState::Stopped;
        let info = self.now_playing.as_ref().filter(|_| playing);
        let title = info.map_or(String::new(), |i| {
            if i.artist.is_empty() {
                i.title.clone()
            } else {
                format!("{} – {}", i.artist, i.title)
            }
        });
        let overview = self.overview.clone();
        let feed = crate::spectrogram::Feed {
            title,
            track: self
                .track_refs
                .get(&self.position.track)
                .cloned()
                .filter(|_| playing),
            duration: info.and_then(|i| i.duration_secs).or(overview
                .as_ref()
                .filter(|o| o.complete)
                .map(|o| o.seconds())),
            overview,
            now: self.position.seconds(),
            playing: self.position.state == PlayState::Playing,
            fullscreen: self.fullscreen.is_some(),
            lossless: info.is_some_and(|i| i.lossless),
        };
        if self.spectro.feed(feed) {
            ctx.request_repaint_of(crate::spectrogram::viewport_id());
        }
        self.spectro.show(ctx);
    }

    fn exit(&mut self) {
        if let Some(r) = self.fullscreen.as_ref().and_then(|f| f.restore) {
            self.settings.window_pos = Some((r.min.x, r.min.y));
        }
        self.settings_dirty.get_or_insert_with(Instant::now);
        self.save_now(true);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(e) = self.dig.as_mut().and_then(|d| d.save_memory()) {
            eprintln!("{e}");
        }
    }
}

/// Presentation settings for the app's window: vsync with up to 3 frames queued. With eframe's
/// default of 1 (measured on an M2 MacBook, even for a blank fullscreen window) frames regularly
/// miss their vsync slot and the rate swings between 30 and 60 fps; with 3 it holds 60. The
/// windowed player only draws on input or at 30 fps, so its queue stays short and clicks stay
/// snappy. (Set at startup: eframe 0.36's `Frame::set_wgpu_surface_config` changes a clone of
/// the render state and has no effect.) The visuals read the audio clock ahead to make up for
/// the display delay.
pub fn surface_config() -> eframe::egui_wgpu::SurfaceConfig {
    eframe::egui_wgpu::SurfaceConfig {
        present_mode: eframe::egui_wgpu::wgpu::PresentMode::AutoVsync,
        desired_maximum_frame_latency: Some(3),
    }
}

#[derive(Default)]
struct PhaseStats {
    sum: f64,
    max: f64,
    n: u32,
}

impl PhaseStats {
    fn add(&mut self, ms: f64) {
        self.sum += ms;
        self.max = self.max.max(ms);
        self.n += 1;
    }

    fn show(&self) -> String {
        format!("{:5.2}/{:5.2}", self.sum / self.n.max(1) as f64, self.max)
    }
}

/// Where each frame's time goes: our `logic`, our `ui` (which includes the visuals' `paint` and
/// egui layer), and everything eframe does outside them (egui tessellation, GPU submit, waiting
/// for a drawable, present, event loop).
struct FrameProfile {
    window: Instant,
    frames: u32,
    frame_dt: PhaseStats,
    logic: PhaseStats,
    ui: PhaseStats,
    outside: PhaseStats,
    last_logic_start: Option<Instant>,
    logic_started: Instant,
    ui_end: Option<Instant>,
    log: Option<std::fs::File>,
}

impl FrameProfile {
    fn new() -> Self {
        let path = std::env::temp_dir().join("diggr_frame_stats.log");
        eprintln!("frame stats → {}", path.display());
        Self {
            window: Instant::now(),
            frames: 0,
            frame_dt: PhaseStats::default(),
            logic: PhaseStats::default(),
            ui: PhaseStats::default(),
            outside: PhaseStats::default(),
            last_logic_start: None,
            logic_started: Instant::now(),
            ui_end: None,
            log: std::fs::File::create(path).ok(),
        }
    }

    fn logic_start(&mut self, now: Instant) {
        if let Some(end) = self.ui_end.take() {
            self.outside.add((now - end).as_secs_f64() * 1e3);
        }
        if let Some(prev) = self.last_logic_start {
            self.frame_dt.add((now - prev).as_secs_f64() * 1e3);
        }
        self.last_logic_start = Some(now);
        self.logic_started = now;
    }

    fn logic_end(&mut self) {
        self.logic
            .add(self.logic_started.elapsed().as_secs_f64() * 1e3);
    }

    fn ui_done(&mut self, start: Instant, fullscreen: bool) {
        let now = Instant::now();
        self.ui.add((now - start).as_secs_f64() * 1e3);
        self.ui_end = Some(now);
        self.frames += 1;
        if self.window.elapsed() >= Duration::from_secs(1) {
            let line = format!(
                "{} fps {:3} | frame ms avg/max {} | logic {} | ui+visuals {} | eframe paint+present {}",
                if fullscreen { "full" } else { "win " },
                self.frames,
                self.frame_dt.show(),
                self.logic.show(),
                self.ui.show(),
                self.outside.show(),
            );
            eprintln!("{line}");
            if let Some(f) = &mut self.log {
                use std::io::Write;
                let _ = writeln!(f, "{line}");
            }
            let log = self.log.take();
            *self = Self {
                log,
                last_logic_start: self.last_logic_start,
                ui_end: self.ui_end,
                ..Self::quiet()
            };
        }
    }

    fn quiet() -> Self {
        Self {
            window: Instant::now(),
            frames: 0,
            frame_dt: PhaseStats::default(),
            logic: PhaseStats::default(),
            ui: PhaseStats::default(),
            outside: PhaseStats::default(),
            last_logic_start: None,
            logic_started: Instant::now(),
            ui_end: None,
            log: None,
        }
    }
}

/// Whether a click opens a context menu: the secondary button, or Control-click where Control
/// isn't the command key (macOS; elsewhere Ctrl-click stays multi-select).
pub fn opens_context_menu(secondary: bool, clicked: bool, mods: Modifiers) -> bool {
    secondary || (clicked && mods.ctrl && !mods.command)
}

/// Structure-navigation state mirrored from engine events.
#[derive(Debug, Default)]
struct Nav {
    /// A quantized jump will land at this time of the current track.
    pending_jump: Option<f64>,
    /// Where the pending jump goes (to jump at once if it misses).
    jump_target: Option<f64>,
    loop_region: Option<(f64, f64)>,
    /// Length of the `Shift+L` loop, when that is what's looping.
    loop_bars: Option<usize>,
}

// Keep `Rect`/`Pos2` imports used in all cfgs.
#[allow(dead_code)]
fn _unused(_: Rect, _: Pos2) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_keys_are_not_forwarded_to_the_scene() {
        use analysis::SectionKind as K;
        assert_eq!(host_action(Key::T, false), Some(HostAction::ToggleStrip));
        assert_eq!(
            host_action(Key::A, false),
            Some(HostAction::ToggleAnnotating)
        );
        // Digits and Space belong to the scene unless annotating.
        assert_eq!(host_action(Key::Num1, false), None);
        assert_eq!(host_action(Key::Space, false), None);
        assert_eq!(
            host_action(Key::Num3, true),
            Some(HostAction::Boundary(K::Drop))
        );
        assert_eq!(
            host_action(Key::Num6, true),
            Some(HostAction::Boundary(K::Outro))
        );
        assert_eq!(host_action(Key::Space, true), Some(HostAction::Tap));
        // Anything else still reaches the scene (e.g. the visual engine's M / K / D).
        for k in [Key::M, Key::K, Key::D, Key::Num7] {
            assert_eq!(host_action(k, true), None);
        }
    }

    #[test]
    fn idle_window_never_repaints_on_its_own() {
        assert_eq!(
            repaint_after(Activity::default()),
            None,
            "stopped and quiet"
        );
        let playing = Activity {
            playing: true,
            ..Default::default()
        };
        assert_eq!(
            repaint_after(playing),
            Some(Duration::from_millis(33)),
            "30 Hz while playing"
        );
        let scrolling = Activity {
            playing: true,
            waveform: true,
            ..Default::default()
        };
        assert_eq!(
            repaint_after(scrolling),
            Some(Duration::from_millis(16)),
            "waveform scrolls smoothly"
        );
        let paused_waveform = Activity {
            waveform: true,
            ..Default::default()
        };
        assert_eq!(
            repaint_after(paused_waveform),
            None,
            "no repaint while paused"
        );
        let falling = Activity {
            bars_moving: true,
            ..Default::default()
        };
        assert!(repaint_after(falling).is_some(), "bars settle after stop");
        let minimized = Activity {
            playing: true,
            hidden: true,
            ..Default::default()
        };
        assert_eq!(
            repaint_after(minimized),
            None,
            "no repaints while minimized/occluded"
        );
    }

    #[test]
    fn a_verdict_flashes_its_word_and_count() {
        assert_eq!(flash_text(Verdict::Wanted, 1), "WANTED");
        assert_eq!(flash_text(Verdict::Wanted, 3), "WANTED 3");
        assert_eq!(flash_text(Verdict::Unwanted, 1), "UNWANTED");
        assert_eq!(flash_text(Verdict::Pass, 1), "PASS");
        assert_eq!(flash_text(Verdict::Owned, 2), "OWNED 2");
        // Every flash fits the title line.
        let def = LoadedSkin::default_skin().def;
        let w = flash_text(Verdict::Unwanted, 999).chars().count() as u16 * def.font.advance;
        assert!(w <= def.at("title_text").w);
    }

    #[test]
    fn an_empty_crate_says_how_to_fill_it() {
        assert_eq!(
            empty_crate_hint("CMD"),
            [
                "PASTE A DISCOGS LINK · CMD+V",
                "OR DROP FILES",
                "",
                "PRESS H FOR HELP"
            ]
        );
        assert_eq!(empty_crate_hint("CTRL")[0], "PASTE A DISCOGS LINK · CTRL+V");
        // Every character is in the skin font, and the longer line fits the narrowest list.
        let def = LoadedSkin::default_skin().def;
        for line in empty_crate_hint("CTRL")
            .into_iter()
            .filter(|l| !l.is_empty())
        {
            assert!(line.chars().all(|c| def.glyph(c).is_some()), "{line}");
            let w = line.chars().count() as u16 * def.font.advance - 1;
            assert!(w <= def.at("pl_list").w, "{line}: {w}");
        }
    }

    #[test]
    fn slow_scrolling_adds_up_to_whole_rows() {
        let mut acc = 0.0;
        let mut first = 10;
        // 3 points a frame for 10 frames, downwards, with 13-point rows: 2 rows.
        for _ in 0..10 {
            first = scroll_rows(&mut acc, first, 100, -3.0, 13.0);
        }
        assert_eq!(first, 12);
        // Leftovers pushing past an end are dropped; scrolling away from it keeps them.
        let mut acc = 12.0;
        assert_eq!(scroll_rows(&mut acc, 0, 100, 0.5, 13.0), 0);
        assert_eq!(acc, 0.0);
        let mut acc = 0.0;
        assert_eq!(scroll_rows(&mut acc, 0, 100, -5.0, 13.0), 0);
        assert_eq!(acc, -5.0);
        let mut acc = 0.0;
        assert_eq!(
            scroll_rows(&mut acc, 5, 100, 40.0, 13.0),
            2,
            "a wheel step moves at once"
        );
    }

    #[test]
    fn control_click_opens_the_context_menu_on_macos() {
        let mac_ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let mac_cmd = Modifiers {
            mac_cmd: true,
            command: true,
            ..Default::default()
        };
        let pc_ctrl = Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        assert!(
            opens_context_menu(true, false, Modifiers::NONE),
            "right button / two-finger click"
        );
        assert!(
            opens_context_menu(false, true, mac_ctrl),
            "Control-click on a Mac"
        );
        assert!(
            !opens_context_menu(false, true, mac_cmd),
            "Cmd-click selects"
        );
        assert!(
            !opens_context_menu(false, true, pc_ctrl),
            "Ctrl-click selects on Windows/Linux"
        );
        assert!(!opens_context_menu(false, true, Modifiers::NONE));
    }

    #[test]
    fn window_size_is_in_points() {
        let skin = LoadedSkin::default_skin();
        let s = Settings {
            scale: 2,
            show_eq: true,
            show_waveform: false,
            show_playlist: true,
            playlist_rows: 20,
            playlist_width: 300,
            ..Default::default()
        };
        assert_eq!(
            DiggrApp::window_size(&s, &skin),
            vec2(575.0, 20.0 + 16.0 + 260.0 + 38.0) * 2.0
        );
    }
}

/// The player driven headlessly: a real engine on a `ManualSink`, crates in a temp config
/// folder, and egui frames fed with synthetic pointer and key events.
#[cfg(test)]
mod headless_tests {
    use super::*;
    use crate::crates::PLAYLIST;
    use crate::playlist::Origin;
    use egui::{Event, PointerButton};
    use platform::CallbackInfo;
    use platform::native::{NativeFileSource, NativeSpawner};
    use platform::testing::ManualSink;
    use std::path::Path;

    const BUF: usize = 512;
    /// Classic size, main window and playlist only.
    /// The playlist's top-left at 1×: right of the player column.
    const PL_LEFT: f32 = 275.0;
    const PL_TOP: f32 = 0.0;
    /// The list's top below the playlist's: the title bar, then the filter bar.
    const LIST_TOP: f32 = 36.0;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(format!(
            "{}/../audio/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
    }

    fn temp(name: &str) -> platform::testing::TestDir {
        platform::testing::TestDir::new(&format!("ui-app-{name}"))
    }

    struct Rig {
        app: DiggrApp,
        ctx: egui::Context,
        sink: ManualSink,
        now_ns: u64,
        dir: platform::testing::TestDir,
        /// Modifier keys held during the next frames.
        mods: Modifiers,
    }

    impl Rig {
        /// `prepare` may fill the config folder before the app starts.
        fn new(name: &str, open: Vec<PathBuf>, prepare: impl FnOnce(&Store)) -> Self {
            Self::with_dig(name, open, prepare, |_| None)
        }

        /// `dig` gets the rig's folder and gives the digging setup (fakes), if any.
        fn with_dig(
            name: &str,
            open: Vec<PathBuf>,
            prepare: impl FnOnce(&Store),
            dig: impl FnOnce(&Path) -> Option<DigSetup>,
        ) -> Self {
            let mut rig = Self::build(name, open, prepare, dig);
            rig.until(|r| r.app.engine().is_some(), "the engine starts");
            // Digging starts on the frame after the first.
            rig.frame(Vec::new());
            rig
        }

        /// The app before its first frame.
        fn build(
            name: &str,
            open: Vec<PathBuf>,
            prepare: impl FnOnce(&Store),
            dig: impl FnOnce(&Path) -> Option<DigSetup>,
        ) -> Self {
            let dir = temp(name);
            let store = Store::new(dir.join("config"));
            store
                .save(
                    SETTINGS_FILE,
                    &Settings {
                        scale: 1,
                        show_eq: false,
                        show_waveform: false,
                        repeat: Repeat::One, // the 2 s fixtures keep playing
                        ..Default::default()
                    },
                )
                .unwrap();
            prepare(&store);
            let sink = ManualSink::new(48_000, 2);
            let engine_sink = sink.clone();
            let ctx = egui::Context::default();
            let app = DiggrApp::build(
                ctx.clone(),
                None,
                AppContext {
                    engine: Box::new(move || {
                        Engine::new(
                            Arc::new(engine_sink),
                            &NativeSpawner,
                            Arc::new(NativeFileSource),
                            audio::EngineConfig::default(),
                        )
                        .map_err(|e| e.to_string())
                    }),
                    spawner: Arc::new(NativeSpawner),
                    files: Arc::new(NativeFileSource),
                    store: Some(store),
                    open,
                    scene: Box::new(crate::fullscreen::BeatFlash::default()),
                    startup: Startup {
                        process_start: Instant::now(),
                        report: false,
                        exit_after_first_frame: false,
                    },
                    analysis: None,
                    annotations_dir: None,
                    overviews: None,
                    dig: dig(&dir),
                },
            );
            Rig {
                app,
                ctx,
                sink,
                now_ns: 0,
                dir,
                mods: Modifiers::NONE,
            }
        }

        fn frame(&mut self, mut events: Vec<Event>) -> egui::FullOutput {
            events.insert(0, Event::ModifiersChanged(self.mods));
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 400.0))),
                events,
                ..Default::default()
            };
            let app = &mut self.app;
            let mut out = self.ctx.run_ui(input, |ui| {
                app.logic_inner(ui.ctx());
                app.ui_inner(ui);
            });
            out.textures_delta.clear();
            out
        }

        /// One device callback (advancing fake time by a buffer), then a UI frame.
        fn pump(&mut self) -> egui::FullOutput {
            std::thread::sleep(Duration::from_micros(BUF as u64 * 1_000_000 / 48_000 / 2));
            self.now_ns += BUF as u64 * 1_000_000_000 / 48_000;
            self.sink.set_now_ns(self.now_ns);
            self.sink.pull(
                BUF,
                CallbackInfo {
                    host_ns: self.now_ns,
                    output_latency_ns: 0,
                },
            );
            self.frame(Vec::new())
        }

        fn until(&mut self, mut done: impl FnMut(&mut Self) -> bool, what: &str) {
            // Generous: the whole workspace's tests share the machine, and a passing wait
            // ends as soon as `done` holds.
            let deadline = Instant::now() + Duration::from_secs(15);
            while !done(self) {
                assert!(Instant::now() < deadline, "timed out: {what}");
                self.pump();
            }
        }

        fn engine(&mut self) -> &mut Engine {
            self.app.engine().expect("engine ready")
        }

        fn engine_queue(&mut self) -> Vec<TrackRef> {
            self.engine().queue().to_vec()
        }

        fn press(&mut self, pos: Pos2, button: PointerButton, pressed: bool) -> egui::FullOutput {
            self.frame(vec![Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: self.mods,
            }])
        }

        fn click_with(&mut self, pos: Pos2, button: PointerButton) -> egui::FullOutput {
            self.frame(vec![Event::PointerMoved(pos)]);
            self.press(pos, button, true);
            self.press(pos, button, false);
            self.frame(Vec::new())
        }

        fn click(&mut self, pos: Pos2) -> egui::FullOutput {
            self.click_with(pos, PointerButton::Primary)
        }

        /// A click with modifiers held (Cmd, Shift).
        fn click_with_mods(&mut self, pos: Pos2, modifiers: Modifiers) -> egui::FullOutput {
            self.mods = modifiers;
            let out = self.click_with(pos, PointerButton::Primary);
            self.mods = Modifiers::NONE;
            out
        }

        fn double_click(&mut self, pos: Pos2) -> egui::FullOutput {
            self.frame(vec![Event::PointerMoved(pos)]);
            for _ in 0..2 {
                self.press(pos, PointerButton::Primary, true);
                self.press(pos, PointerButton::Primary, false);
            }
            self.frame(Vec::new())
        }

        /// Clicks the text `label` wherever it is drawn (menu items, buttons).
        fn click_text(&mut self, label: &str) -> egui::FullOutput {
            let out = self.frame(Vec::new());
            let rect = texts(&out)
                .into_iter()
                .find(|t| t.text == label)
                .unwrap_or_else(|| panic!("{label:?} is not on screen: {:?}", text_list(&out)))
                .rect;
            self.click(rect.center())
        }

        fn type_text(&mut self, text: &str) {
            self.frame(vec![Event::Text(text.into())]);
            self.frame(vec![Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]);
            self.frame(Vec::new());
        }

        /// Cmd+A in the focused text field.
        fn select_all_text(&mut self) {
            self.mods = Modifiers::COMMAND;
            self.frame(vec![Event::Key {
                key: Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::COMMAND,
            }]);
            self.mods = Modifiers::NONE;
        }

        /// A key press, with `modifiers` held for that frame.
        fn key(&mut self, key: Key, modifiers: Modifiers) -> egui::FullOutput {
            self.mods = modifiers;
            let out = self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }]);
            self.mods = Modifiers::NONE;
            out
        }

        /// A crate of `n` entries (the same fixture) shown in the playlist.
        fn fill_playlist(&mut self, n: usize) -> Vec<EntryId> {
            let t = TrackRef::new(fixture("tone.flac").to_string_lossy());
            let p = self.app.crates.shown_mut();
            p.add(std::iter::repeat_n(t, n));
            p.entries().iter().map(|e| e.id).collect()
        }

        fn title_bar(&self) -> Pos2 {
            pos2(PL_LEFT + 60.0, PL_TOP + 10.0)
        }

        fn row(&self, index: usize) -> Pos2 {
            pos2(
                PL_LEFT + 60.0,
                PL_TOP + LIST_TOP + index as f32 * 13.0 + 6.5,
            )
        }

        fn ids(&self, crate_id: CrateId) -> Vec<EntryId> {
            let p = self.app.crates.get(crate_id).expect("loaded");
            p.entries().iter().map(|e| e.id).collect()
        }

        /// A new crate holding these fixtures (not shown).
        fn crate_with(&mut self, name: &str, files: &[&str]) -> CrateId {
            let id = self.app.crates.create(name).unwrap();
            let tracks = files
                .iter()
                .map(|f| TrackRef::new(fixture(f).to_string_lossy()));
            self.app.crates.get_mut(id).unwrap().add(tracks);
            id
        }

        fn menu_open(&self) -> bool {
            egui::Popup::is_id_open(&self.ctx, Id::new("crate_menu"))
        }
    }

    struct Text {
        text: String,
        rect: Rect,
        color: Option<Color32>,
    }

    // Bandcamp pages, with fake pages and previews.
    mod bandcamp_tests;
    // Digging, with a fake Discogs, fake previews and a fake browser.
    mod dig_tests;
    mod record_tests;
    // Top Sellers and the cart, with a fake Discogs.
    mod seller_tests;

    fn texts(out: &egui::FullOutput) -> Vec<Text> {
        fn walk(shape: &egui::Shape, acc: &mut Vec<Text>) {
            match shape {
                egui::Shape::Text(t) => acc.push(Text {
                    text: t.galley.text().to_owned(),
                    rect: t.visual_bounding_rect(),
                    color: t.galley.job.sections.first().map(|s| s.format.color),
                }),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, acc)),
                _ => {}
            }
        }
        let mut acc = Vec::new();
        for s in &out.shapes {
            walk(&s.shape, &mut acc);
        }
        acc
    }

    /// Filled rectangles drawn, with their colour.
    fn rects(out: &egui::FullOutput) -> Vec<(Rect, Color32)> {
        fn walk(shape: &egui::Shape, acc: &mut Vec<(Rect, Color32)>) {
            match shape {
                egui::Shape::Rect(r) => acc.push((r.rect, r.fill)),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, acc)),
                _ => {}
            }
        }
        let mut acc = Vec::new();
        for s in &out.shapes {
            walk(&s.shape, &mut acc);
        }
        acc
    }

    fn text_list(out: &egui::FullOutput) -> Vec<String> {
        texts(out).into_iter().map(|t| t.text).collect()
    }

    fn shows(out: &egui::FullOutput, text: &str) -> bool {
        texts(out).iter().any(|t| t.text == text)
    }

    fn start_drag(out: &egui::FullOutput) -> bool {
        out.viewport_output.values().any(|v| {
            v.commands
                .iter()
                .any(|c| matches!(c, ViewportCommand::StartDrag))
        })
    }

    // ---- playback follows its crate -------------------------------------------------------

    #[test]
    fn switching_crates_leaves_the_engine_queue_alone() {
        let mut rig = Rig::new("switch", Vec::new(), |_| {});
        rig.app.add_paths(
            vec![
                fixture("tone.flac"),
                fixture("tone.wav"),
                fixture("tone.ogg"),
            ],
            Open::Add,
        );
        rig.until(|r| r.engine().state() == PlayState::Playing, "playing");
        let queue = rig.engine_queue();
        assert_eq!(queue.len(), 3);

        let b = rig.crate_with("B", &["tone.mp3", "tone.m4a"]);
        rig.app.apply(Action::ShowCrate(b), &rig.ctx.clone());
        // Edits to a crate that isn't playing don't touch the queue either.
        rig.app.add_paths(vec![fixture("mono48k.wav")], Open::Add);
        for _ in 0..5 {
            rig.pump();
        }
        assert_eq!(rig.app.crates.shown_id(), b);
        assert_eq!(rig.app.crates.playing_id(), PLAYLIST);
        assert_eq!(
            rig.engine_queue(),
            queue,
            "the queue still is the Playlist crate"
        );
        assert_eq!(rig.engine().state(), PlayState::Playing);
        assert_eq!(rig.engine().current_index(), Some(0));
    }

    #[test]
    fn starting_a_track_in_another_crate_retargets_next_and_previous() {
        let mut rig = Rig::new("retarget", Vec::new(), |_| {});
        rig.app
            .add_paths(vec![fixture("tone.flac"), fixture("tone.wav")], Open::Add);
        rig.until(|r| r.engine().state() == PlayState::Playing, "playing");
        let b = rig.crate_with("B", &["tone.mp3", "tone.ogg", "tone.m4a"]);
        rig.app.apply(Action::ShowCrate(b), &rig.ctx.clone());
        rig.frame(Vec::new());

        rig.double_click(rig.row(0));
        assert_eq!(rig.app.crates.playing_id(), b);
        let b_tracks: Vec<TrackRef> = rig
            .app
            .crates
            .get(b)
            .unwrap()
            .entries()
            .iter()
            .map(|e| e.track.clone())
            .collect();
        assert_eq!(rig.engine_queue(), b_tracks, "the queue is crate B");

        rig.app.apply(Action::Next, &rig.ctx.clone());
        let second = rig.ids(b)[1];
        rig.until(
            |r| r.app.crates.get(b).unwrap().current() == Some(second),
            "next plays B's second entry",
        );
        rig.app.apply(Action::Prev, &rig.ctx.clone());
        let first = rig.ids(b)[0];
        rig.until(
            |r| r.app.crates.get(b).unwrap().current() == Some(first),
            "previous goes back within B",
        );

        // Deleting the playing crate stops playback.
        rig.app.delete_crate(b);
        rig.frame(Vec::new());
        assert_eq!(rig.engine().state(), PlayState::Stopped);
        assert_eq!(rig.app.crates.playing_id(), PLAYLIST);
        assert_eq!(rig.app.crates.shown_id(), PLAYLIST);
    }

    // ---- entries waiting for their audio ------------------------------------------------

    #[test]
    fn an_armed_entry_starts_within_100_ms_of_its_audio_arriving() {
        let mut rig = Rig::new("armed", Vec::new(), |_| {});
        rig.app.add_paths(vec![fixture("tone.flac")], Open::Add);
        rig.until(|r| r.engine().state() == PlayState::Playing, "playing");
        let waiting = rig.app.crates.shown_mut().add_waiting(
            "Nightcraft",
            "Glasshouse",
            Some("https://www.youtube.com/watch?v=abcdefghijk".into()),
            None,
            "downloading 40%",
        );
        rig.app.mark_shown();
        rig.frame(Vec::new());

        let out = rig.double_click(rig.row(1));
        assert_eq!(rig.app.armed, Some((PLAYLIST, waiting)));
        assert!(
            shows(&out, "Waiting for Nightcraft: Glasshouse (downloading 40%)"),
            "{:?}",
            text_list(&out)
        );
        for _ in 0..10 {
            rig.pump();
        }
        assert_eq!(
            rig.engine().state(),
            PlayState::Playing,
            "the current track plays on"
        );
        assert_eq!(rig.engine().current_index(), Some(0));
        assert_eq!(
            rig.engine_queue().len(),
            1,
            "the waiting entry isn't queued"
        );

        // The download finishes: the file lands in the cache and the producer reports it.
        let cached = rig.dir.join("abcdefghijk.wav");
        std::fs::copy(fixture("tone.wav"), &cached).unwrap();
        let before = rig.app.position.track;
        let arrived_ns = rig.now_ns;
        rig.app
            .set_audio(PLAYLIST, waiting, TrackRef::new(cached.to_string_lossy()));
        assert_eq!(rig.app.armed, None);
        // Audible: the clock runs on a new track instance, past its first frames.
        rig.until(
            |r| {
                let p = r.app.position;
                p.track != before && p.state == PlayState::Playing && p.frame > 0
            },
            "the armed entry plays",
        );
        assert_eq!(rig.app.crates.playing().current(), Some(waiting));
        let ms = (rig.now_ns - arrived_ns) as f64 / 1e6;
        assert!(ms < 100.0, "audible {ms:.1} ms after its audio arrived");
        assert_eq!(rig.engine().stats().underruns, 0);
    }

    #[test]
    fn starting_another_track_cancels_the_arming() {
        let mut rig = Rig::new("disarm", Vec::new(), |_| {});
        rig.app
            .add_paths(vec![fixture("tone.flac"), fixture("tone.wav")], Open::Add);
        rig.until(|r| r.engine().state() == PlayState::Playing, "playing");
        let waiting = rig
            .app
            .crates
            .shown_mut()
            .add_waiting("", "Later", None, None, "listed");
        rig.app.apply(Action::PlayEntry(waiting), &rig.ctx.clone());
        assert!(rig.app.armed.is_some());
        rig.double_click(rig.row(1));
        assert_eq!(rig.app.armed, None);
        rig.frame(Vec::new());
        assert!(rig.app.armed_line().is_none());
    }

    #[test]
    fn opening_files_replaces_only_the_playlist_crate() {
        let lowtide = std::cell::Cell::new(0);
        let mut rig = Rig::new(
            "open",
            vec![fixture("tone.flac"), fixture("tone.wav")],
            |store| {
                let mut c = Crates::open(store);
                c.shown_mut()
                    .add([TrackRef::new(fixture("tone.ogg").to_string_lossy())]);
                c.touch(PLAYLIST);
                let lt = c.create("Lowtide Tapes").unwrap();
                c.get_mut(lt)
                    .unwrap()
                    .add([TrackRef::new(fixture("tone.mp3").to_string_lossy())]);
                c.touch(lt);
                c.show(lt);
                c.save_due(true, Duration::ZERO);
                lowtide.set(lt);
            },
        );
        let lt = lowtide.get();
        let stem = |r: &Rig, c: CrateId| -> Vec<String> {
            r.app
                .crates
                .get(c)
                .unwrap()
                .entries()
                .iter()
                .map(|e| e.track.stem().to_owned())
                .collect()
        };
        assert_eq!(rig.app.crates.shown_id(), PLAYLIST);
        assert_eq!(rig.app.crates.playing_id(), PLAYLIST);
        rig.until(
            |r| r.engine().state() == PlayState::Playing,
            "plays the first file",
        );
        let first = rig.ids(PLAYLIST)[0];
        assert_eq!(rig.app.crates.playing().current(), Some(first));
        assert_eq!(rig.app.crates.shown().len(), 2, "exactly the opened files");
        rig.app.crates.load(lt);
        assert_eq!(stem(&rig, lt), ["tone"], "Lowtide Tapes is unchanged");
        assert!(
            rig.app.crates.get(lt).unwrap().entries()[0]
                .track
                .0
                .ends_with("tone.mp3")
        );

        // Eject does the same while another crate is shown.
        rig.app.apply(Action::ShowCrate(lt), &rig.ctx.clone());
        rig.app.add_paths(vec![fixture("tone.m4a")], Open::Replace);
        assert_eq!(
            (rig.app.crates.shown_id(), rig.app.crates.playing_id()),
            (PLAYLIST, PLAYLIST)
        );
        assert_eq!(rig.app.crates.shown().len(), 1);
        assert!(
            rig.app.crates.get(lt).unwrap().entries()[0]
                .track
                .0
                .ends_with("tone.mp3")
        );
    }

    #[test]
    fn waiting_and_unavailable_rows_are_dimmed_and_failed_rows_red() {
        let mut rig = Rig::new("rows", Vec::new(), |_| {});
        rig.app.add_paths(
            vec![fixture("tone.flac"), fixture("garbage.mp3")],
            Open::Add,
        );
        let p = rig.app.crates.shown_mut();
        p.add_waiting("Nightcraft", "Glasshouse", None, None, "downloading 40%");
        let gone = p.add_waiting("Nightcraft", "B-side", None, None, "listed");
        p.set_unavailable(gone, "no clip");
        rig.until(
            |r| r.app.crates.shown().entries()[1].status == EntryStatus::Failed,
            "the broken file is found",
        );
        let out = rig.frame(Vec::new());
        let colors = rig.app.skin.def.colors.clone();
        let dim = lerp_color(colors.pl_text, colors.pl_bg, 0.55);
        let color_of = |text: &str| {
            texts(&out)
                .into_iter()
                .find(|t| t.text == text)
                .unwrap_or_else(|| panic!("{text:?} not drawn: {:?}", text_list(&out)))
                .color
        };
        assert_eq!(color_of("3. Nightcraft: Glasshouse"), Some(dim));
        assert_eq!(color_of("4. Nightcraft: B-side"), Some(dim));
        // Their states are icons now, with the words in the tooltip.
        assert!(!shows(&out, "downloading 40%") && !shows(&out, "no clip"));
        // The download is a bar: 40% of its 30 pixels filled, in the dimmed colour.
        let row = Rect::from_min_size(rig.row(2) - vec2(60.0, 6.5), vec2(243.0, 13.0));
        let filled: Vec<Rect> = rects(&out)
            .into_iter()
            .filter(|(r, c)| *c == dim && row.contains_rect(*r) && (r.height() - 3.0).abs() < 0.01)
            .map(|(r, _)| r)
            .collect();
        assert_eq!(filled.len(), 1, "{filled:?}");
        assert!((filled[0].width() - 12.0).abs() < 0.01, "{filled:?}");
        assert_eq!(color_of("2. garbage"), Some(Color32::from_rgb(170, 60, 60)));
        let normal = color_of("1. M83: Midnight_City · Hurry_Up");
        assert!(normal == Some(color(colors.pl_text)) || normal == Some(color(colors.pl_current)));
    }

    #[test]
    fn hovering_an_entry_shows_everything_known_about_it() {
        let mut rig = Rig::new("tooltip", Vec::new(), |_| {});
        rig.ctx.global_style_mut(|s| {
            s.interaction.tooltip_delay = 0.0;
            s.interaction.tooltip_grace_time = 0.0;
        });
        rig.app.crates.shown_mut().add_waiting(
            "Nightcraft",
            "Glasshouse",
            None,
            Some(Origin {
                label: "Lowtide Tapes".into(),
                catno: "LT-012".into(),
                position: "A1".into(),
                ..Default::default()
            }),
            "downloading 40%",
        );
        let at = rig.row(0);
        let mut out = rig.frame(vec![Event::PointerMoved(at)]);
        for _ in 0..10 {
            if shows(&out, "Lowtide Tapes") {
                break;
            }
            out = rig.frame(vec![Event::PointerMoved(at)]);
        }
        for t in [
            "(LT-012) Nightcraft: Glasshouse",
            "Lowtide Tapes",
            "A1",
            "downloading 40%",
        ] {
            assert!(shows(&out, t), "{t}: {:?}", text_list(&out));
        }
    }

    #[test]
    fn the_title_line_keeps_an_origin_entrys_own_names() {
        let mut rig = Rig::new("origin-title", Vec::new(), |_| {});
        let id = rig.app.crates.shown_mut().add_waiting(
            "Nightcraft",
            "Glasshouse",
            None,
            Some(Origin {
                release: Some(123456),
                ..Default::default()
            }),
            "listed",
        );
        rig.app.apply(Action::PlayEntry(id), &rig.ctx.clone());
        // The fixture's tags say "M83 - Midnight_City".
        rig.app.set_audio(
            PLAYLIST,
            id,
            TrackRef::new(fixture("tone.flac").to_string_lossy()),
        );
        rig.until(
            |r| r.app.now_playing.is_some() && r.app.crates.shown().entries()[0].duration.is_some(),
            "playing, with its duration read",
        );
        assert_eq!(rig.app.now_playing.as_ref().unwrap().artist, "M83");
        let (artist, title, duration) = rig.app.now_playing_names().unwrap();
        assert_eq!(
            (artist.as_str(), title.as_str()),
            ("Nightcraft", "Glasshouse")
        );
        assert!(duration.is_some_and(|d| (d - 2.0).abs() < 0.05));
        assert_eq!(
            rig.app.now_playing_line().as_deref(),
            Some("1. Nightcraft: Glasshouse (0:02)")
        );
        let e = &rig.app.crates.shown().entries()[0];
        assert_eq!(e.display_name(), "Nightcraft: Glasshouse");
        assert!(
            e.duration.is_some_and(|d| (d - 2.0).abs() < 0.05),
            "takes the duration"
        );
    }

    #[test]
    fn a_click_or_tab_gives_a_side_the_arrow_keys() {
        let mut rig = Rig::new("focus", Vec::new(), |_| {});
        let ids = rig.fill_playlist(12);
        rig.frame(Vec::new());
        assert_eq!(rig.app.focus, Focus::Player, "the player at launch");
        let v = rig.app.settings.volume;
        rig.key(Key::ArrowUp, Modifiers::NONE);
        assert!(rig.app.settings.volume > v, "↑ is volume");
        assert_eq!(rig.app.crates.shown().cursor(), None);

        rig.click(rig.row(2));
        assert_eq!(rig.app.focus, Focus::Playlist);
        let v = rig.app.settings.volume;
        rig.key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(rig.app.settings.volume, v, "↓ no longer touches the volume");
        assert_eq!(rig.app.crates.shown().cursor(), Some(ids[3]));
        rig.key(Key::ArrowDown, Modifiers::SHIFT);
        assert_eq!(rig.app.crates.shown().selected_ids(), &ids[3..=4]);

        // Tab hands the keyboard back, and egui's own widget focus doesn't swallow it.
        rig.key(Key::Tab, Modifiers::NONE);
        assert_eq!(rig.app.focus, Focus::Player);
        assert!(!rig.ctx.egui_wants_keyboard_input());
        rig.key(Key::ArrowDown, Modifiers::NONE);
        assert!(rig.app.settings.volume < v);
        assert_eq!(rig.app.crates.shown().cursor(), Some(ids[4]));
        rig.key(Key::Tab, Modifiers::NONE);
        assert_eq!(rig.app.focus, Focus::Playlist);

        // A click on the player (on a bare spot of its panel) takes it back.
        rig.click(pos2(190.0, 45.0));
        assert_eq!(rig.app.focus, Focus::Player);
    }

    #[test]
    fn the_cursor_scrolls_the_list_and_enter_plays_it() {
        let mut rig = Rig::new("cursor", Vec::new(), |_| {});
        let ids = rig.fill_playlist(40);
        rig.click(rig.row(8));
        rig.key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(rig.app.pl_scroll, 0, "entry 9 is still on screen");
        rig.key(Key::ArrowDown, Modifiers::NONE);
        rig.key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(rig.app.crates.shown().cursor(), Some(ids[11]));
        assert_eq!(
            rig.app.pl_scroll, 2,
            "scrolled just enough to show entry 12"
        );
        rig.key(Key::End, Modifiers::NONE);
        assert_eq!(rig.app.pl_scroll, 30);
        rig.key(Key::PageUp, Modifiers::NONE);
        assert_eq!(rig.app.crates.shown().cursor(), Some(ids[29]));
        rig.key(Key::Home, Modifiers::NONE);
        assert_eq!(rig.app.pl_scroll, 0);
        rig.key(Key::Enter, Modifiers::NONE);
        rig.until(
            |r| r.app.crates.playing().current() == Some(ids[0]),
            "the cursor's entry plays",
        );
    }

    #[test]
    fn p_shows_the_playing_entry_and_the_list_follows_it_only_from_view() {
        let mut rig = Rig::new("follow", Vec::new(), |_| {});
        let ids = rig.fill_playlist(40);
        rig.app.crates.shown_mut().set_current(Some(ids[25]));
        rig.frame(Vec::new());
        rig.key(Key::P, Modifiers::NONE);
        assert_eq!(rig.app.crates.shown().cursor(), Some(ids[25]));
        assert_eq!(rig.app.pl_scroll, 16, "entry 26 in the last row");

        // The next track, one row below the view: the list follows.
        rig.app.crates.shown_mut().set_current(Some(ids[26]));
        rig.frame(Vec::new());
        assert_eq!(rig.app.pl_scroll, 17);
        // Scrolled away by the user: the list stays.
        rig.app.pl_scroll = 0;
        rig.frame(Vec::new());
        rig.app.crates.shown_mut().set_current(Some(ids[27]));
        rig.frame(Vec::new());
        assert_eq!(rig.app.pl_scroll, 0);
    }

    #[test]
    fn a_sort_reorders_the_crate_and_next_follows_it() {
        let mut rig = Rig::new(
            "sort",
            vec![
                fixture("tone.flac"),
                fixture("tone.wav"),
                fixture("tone.ogg"),
            ],
            |_| {},
        );
        rig.until(
            |r| r.app.position.state == PlayState::Playing,
            "the first file plays",
        );
        let ids = rig.ids(PLAYLIST);
        let set = |r: &mut Rig, i: usize, title: &str| {
            let e = r
                .app
                .crates
                .shown_mut()
                .entries_mut()
                .find(|e| e.id == ids[i])
                .unwrap();
            e.title = title.into();
        };
        set(&mut rig, 0, "b");
        set(&mut rig, 1, "c");
        set(&mut rig, 2, "a");
        let ctx = rig.ctx.clone();
        rig.app.apply(Action::Sort(Field::Title), &ctx);
        assert_eq!(rig.ids(PLAYLIST), [ids[2], ids[0], ids[1]]);
        assert_eq!(
            rig.app.crates.shown().sorted(),
            Some((Field::Title, Dir::Asc))
        );
        rig.app.apply(Action::Sort(Field::Title), &ctx);
        assert_eq!(
            rig.ids(PLAYLIST),
            [ids[1], ids[0], ids[2]],
            "again: the other way"
        );
        // "b" keeps playing, and "a" (now after it) is next.
        rig.frame(Vec::new());
        assert_eq!(rig.app.crates.playing().current(), Some(ids[0]));
        assert_eq!(rig.app.position.state, PlayState::Playing);
        rig.app.apply(Action::Next, &ctx);
        rig.until(
            |r| r.app.crates.playing().current() == Some(ids[2]),
            "next follows the new order",
        );
    }

    #[test]
    fn a_wide_playlist_shows_columns_that_sort_and_hide() {
        let mut rig = Rig::new("columns", vec![fixture("tone.wav")], |_| {});
        let p = rig.app.crates.shown_mut();
        let dig = p.add_waiting(
            "Nightcraft",
            "Glasshouse",
            None,
            Some(Origin {
                catno: "LT-012".into(),
                position: "A1".into(),
                year: Some(1994),
                ..Default::default()
            }),
            "listed",
        );
        p.entries_mut().find(|e| e.id == dig).unwrap().bpm = Some(124);
        rig.app.settings.playlist_width = 700;
        let out = rig.frame(Vec::new());
        for t in [
            "#", "Cat#", "Artist", "Title", "BPM", "Side", "Year", "For sale", "Time",
        ] {
            assert!(shows(&out, t), "header {t}: {:?}", text_list(&out));
        }
        for t in [
            "LT-012",
            "Nightcraft",
            "Glasshouse",
            "124",
            "A1",
            "1994",
            "2.",
        ] {
            assert!(shows(&out, t), "cell {t}: {:?}", text_list(&out));
        }
        assert!(!shows(&out, "2. (LT-012) Nightcraft: Glasshouse (124 BPM)"));

        // A click on BPM sorts (the tone has no tempo: it goes last); again, the other way.
        rig.click_text("BPM");
        assert_eq!(
            rig.app.crates.shown().sorted(),
            Some((Field::Bpm, Dir::Asc))
        );
        assert_eq!(rig.app.crates.shown().entries()[0].id, dig);
        rig.click_text("BPM");
        assert_eq!(
            rig.app.crates.shown().sorted(),
            Some((Field::Bpm, Dir::Desc))
        );
        // The direction is a drawn triangle, not a character the font may lack.
        let out = rig.frame(Vec::new());
        assert!(shows(&out, "BPM"));
        assert!(
            !text_list(&out)
                .iter()
                .any(|t| t.contains('▼') || t.contains('▲'))
        );

        // Right-click the header to hide a column.
        let year = texts(&rig.frame(Vec::new()))
            .into_iter()
            .find(|t| t.text == "Year")
            .unwrap()
            .rect
            .center();
        rig.click_with(year, PointerButton::Secondary);
        let out = rig.frame(Vec::new());
        assert!(
            shows(&out, "Cat#") && shows(&out, "Side"),
            "{:?}",
            text_list(&out)
        );
        // The menu's checkbox is drawn after the header's own "Year".
        let check = texts(&out)
            .into_iter()
            .rfind(|t| t.text == "Year")
            .unwrap()
            .rect
            .center();
        assert_ne!(check, year, "the menu shows");
        rig.click(check);
        assert!(!rig.app.settings.columns.shows(Field::Year));
        rig.key(Key::Escape, Modifiers::NONE);
        assert!(!shows(&rig.frame(Vec::new()), "1994"));

        // Narrow again: the single line, no header.
        rig.app.settings.playlist_width = 400;
        let out = rig.frame(Vec::new());
        assert!(!shows(&out, "Cat#"));
        assert!(
            shows(&out, "1. (LT-012) Nightcraft: Glasshouse (124 BPM)"),
            "{:?}",
            text_list(&out)
        );
    }

    #[test]
    fn trackpad_scrolling_moves_the_list_and_forgets_leftovers_off_it() {
        let mut rig = Rig::new("scroll", Vec::new(), |_| {});
        rig.fill_playlist(40);
        let over = rig.row(3);
        // Slow two-finger scrolling: many small steps, each far less than a row.
        for _ in 0..30 {
            rig.frame(vec![
                Event::PointerMoved(over),
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(0.0, -2.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Modifiers::NONE,
                },
            ]);
        }
        for _ in 0..30 {
            rig.frame(vec![Event::PointerMoved(over)]);
        }
        assert!(rig.app.pl_scroll >= 3, "moved {} rows", rig.app.pl_scroll);
        rig.app.pl_scroll_acc = 10.0;
        rig.frame(vec![Event::PointerMoved(pos2(150.0, 45.0))]);
        assert_eq!(rig.app.pl_scroll_acc, 0.0, "dropped off the list");
    }

    fn maximize_commands(out: &egui::FullOutput) -> Vec<bool> {
        out.viewport_output
            .values()
            .flat_map(|v| v.commands.iter())
            .filter_map(|c| match c {
                ViewportCommand::Maximized(on) => Some(*on),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn shift_p_maximizes_the_playlist_and_restores_it() {
        let mut rig = Rig::new("maximize", Vec::new(), |_| {});
        rig.fill_playlist(5);
        rig.app.settings.show_playlist = false;
        let out = rig.key(Key::P, Modifiers::SHIFT);
        assert!(rig.app.settings.playlist_maximized);
        assert!(rig.app.settings.show_playlist, "shown first");
        assert_eq!(rig.app.focus, Focus::Playlist, "with the keyboard");
        assert_eq!(maximize_commands(&out), [true]);
        let saved = (
            rig.app.settings.playlist_width,
            rig.app.settings.playlist_rows,
        );
        // The rig's window is 1100 × 400: the playlist takes its full width, in columns.
        let out = rig.frame(Vec::new());
        assert!(shows(&out, "Cat#"), "{:?}", text_list(&out));
        assert_eq!(rig.app.pl_geometry().0, 1100);
        let out = rig.key(Key::P, Modifiers::SHIFT);
        assert!(!rig.app.settings.playlist_maximized);
        assert_eq!(maximize_commands(&out), [false]);
        assert_eq!(
            (
                rig.app.settings.playlist_width,
                rig.app.settings.playlist_rows
            ),
            saved,
            "the normal size is untouched"
        );
        assert!(!shows(&rig.frame(Vec::new()), "Cat#"));
    }

    #[test]
    fn the_mini_player_steers_playback_and_restores_the_layout() {
        let mut rig = Rig::new(
            "mini-player",
            vec![fixture("tone.flac"), fixture("tone.wav")],
            |_| {},
        );
        rig.until(|r| r.app.position.state == PlayState::Playing, "playing");
        let ids = rig.ids(PLAYLIST);
        rig.key(Key::P, Modifiers::SHIFT);
        let rows = rig.app.pl_geometry().1;
        let out = rig.frame(Vec::new());
        assert!(!shows(&out, "DIGGR EQUALIZER"));
        // W off still keeps the band: the rows don't move.
        rig.key(Key::W, Modifiers::NONE);
        assert_eq!(rig.app.pl_geometry().1, rows);
        // Next is the fourth button of the second line.
        rig.click(pos2(4.0 + 23.0 * 3.0 + 11.0, 34.0 + 9.0));
        rig.until(
            |r| r.app.crates.playing().current() == Some(ids[1]),
            "the mini player's next starts the next track",
        );
        // A right-click on its LCD line opens the options.
        rig.click_with(pos2(150.0, 15.0), PointerButton::Secondary);
        assert!(shows(&rig.frame(Vec::new()), "Spectrogram (S)"));
        rig.key(Key::Escape, Modifiers::NONE);
        // ⇔ at its right end restores.
        rig.click(pos2(275.0 - 13.0 + 4.0, 38.0 + 4.0));
        assert!(!rig.app.settings.playlist_maximized);
    }

    #[test]
    fn leaving_fullscreen_visuals_maximizes_the_playlist_again() {
        let mut rig = Rig::new("max-fullscreen", Vec::new(), |_| {});
        rig.key(Key::P, Modifiers::SHIFT);
        rig.key(Key::F, Modifiers::NONE);
        assert!(rig.app.fullscreen.is_some());
        let out = rig.key(Key::F, Modifiers::NONE);
        assert!(rig.app.fullscreen.is_none());
        assert_eq!(maximize_commands(&out), [true]);
        let sized = out.viewport_output.values().any(|v| {
            v.commands
                .iter()
                .any(|c| matches!(c, ViewportCommand::InnerSize(_)))
        });
        assert!(!sized, "the OS sizes a maximized window");
    }

    #[test]
    fn the_waveform_sits_beside_the_mini_player_and_seeks() {
        let mut rig = Rig::new("band", vec![fixture("tone.flac")], |_| {});
        rig.app.settings.show_waveform = true;
        rig.until(|r| r.app.position.state == PlayState::Playing, "playing");
        rig.key(Key::P, Modifiers::SHIFT);
        rig.frame(Vec::new());
        // Three quarters across the overview row, right of the mini player: 1.5 s of 2 s.
        let mini = rig.app.skin.def.main_size.0 as f32;
        let x = mini + (1100.0 - mini) * 0.75;
        rig.click(pos2(x, 6.0));
        rig.until(
            |r| (r.app.position.seconds() - 1.5).abs() < 0.2,
            "seeked to 1.5 s",
        );
    }

    #[test]
    fn the_waveform_button_shows_and_hides_the_waveform_like_w() {
        let mut rig = Rig::new("wave-button", Vec::new(), |_| {});
        let b = rig.app.skin.def.at("wave_toggle");
        let at = pos2(b.x as f32 + 5.0, b.y as f32 + 5.0);
        assert!(!rig.app.settings.show_waveform);
        rig.click(at);
        assert!(rig.app.settings.show_waveform);
        // Main + waveform (174) still fits beside the playlist's 10 rows (204).
        let size = DiggrApp::window_size(&rig.app.settings, &rig.app.skin);
        assert_eq!(size, vec2(550.0, 204.0));
        rig.click(at);
        assert!(!rig.app.settings.show_waveform);
    }

    #[test]
    fn a_known_tempo_shows_on_every_entry_with_that_audio_and_is_saved() {
        let mut rig = Rig::new("bpm", Vec::new(), |_| {});
        let origin = Origin {
            catno: "LT-012".into(),
            clip: Some("aaaaaaaaaaa".into()),
            ..Default::default()
        };
        let p = rig.app.crates.shown_mut();
        let a = p.add_waiting(
            "Nightcraft",
            "Glasshouse",
            None,
            Some(origin.clone()),
            "listed",
        );
        p.add_waiting("Nightcraft", "Glasshouse", None, Some(origin), "listed");
        let track = TrackRef::new(fixture("tone.flac").to_string_lossy());
        rig.app.set_audio(PLAYLIST, a, track.clone());
        // A preview prepared at half time.
        rig.app.set_track_bpm(&track, 62.1);
        let out = rig.frame(Vec::new());
        for n in 1..=2 {
            let row = format!("{n}. (LT-012) Nightcraft: Glasshouse (124 BPM)");
            assert!(shows(&out, &row), "{row}: {:?}", text_list(&out));
        }
        rig.app.crates.save_due(true, Duration::ZERO);
        let reopened = Crates::open(&Store::new(rig.dir.join("config")));
        let bpms: Vec<_> = reopened.shown().entries().iter().map(|e| e.bpm).collect();
        assert_eq!(bpms, [Some(124), Some(124)]);
    }

    #[test]
    fn the_playing_tracks_whole_score_gives_its_entry_a_tempo() {
        let mut rig = Rig::new("playing-bpm", vec![fixture("tone.flac")], |_| {});
        rig.until(
            |r| {
                r.app.position.state == PlayState::Playing
                    && r.app.track_refs.contains_key(&r.app.position.track)
            },
            "the clock reports the playing track",
        );
        let mut score = analysis::SongScore {
            tempo_segments: vec![analysis::score::TempoSegment {
                start: 0.0,
                end: 2.0,
                t0: 0.0,
                period: 60.0 / 128.0,
                confidence: 1.0,
            }],
            ..Default::default()
        };
        rig.app.score = Some(Arc::new(score.clone()));
        rig.app.show_playing_bpm();
        let bpm = |r: &Rig| r.app.crates.playing().entries()[0].bpm;
        assert_eq!(bpm(&rig), None, "not while the score is partial");
        score.complete = true;
        rig.app.score = Some(Arc::new(score));
        rig.app.show_playing_bpm();
        assert_eq!(bpm(&rig), Some(128));
    }

    // ---- title bar and crate menu --------------------------------------------------------

    #[test]
    fn the_title_bar_names_the_crate_in_the_skin_font() {
        let def = LoadedSkin::default_skin().def;
        assert_eq!(crate_title(&def, "Lowtide Tapes", 200.0), "LOWTIDE TAPES");
        assert_eq!(
            crate_title(&def, "Canción ₩5", 200.0),
            "CANCION 5",
            "folded, ₩ skipped"
        );
        // 6 px per character, the last one without its gap: 29 px fit five, 28 px four.
        assert_eq!(crate_title(&def, "Keepers", 29.0), "KEEPE");
        assert_eq!(crate_title(&def, "Keepers", 28.0), "KEEP");
        let long = crate_title(&def, &"x".repeat(40), 275.0 - 90.0);
        assert!(
            long.len() < 40 && long.len() * 6 - 1 <= 185,
            "cut to fit: {long}"
        );
    }

    #[test]
    fn a_click_on_the_title_bar_opens_the_crate_menu_and_a_drag_moves_the_window() {
        let mut rig = Rig::new("titlebar", Vec::new(), |_| {});
        let at = rig.title_bar();
        rig.frame(vec![Event::PointerMoved(at)]);
        rig.press(at, PointerButton::Primary, true);
        let mut moved = false;
        for i in 1..=4 {
            let out = rig.frame(vec![Event::PointerMoved(at + vec2(8.0 * i as f32, 3.0))]);
            moved |= start_drag(&out);
        }
        rig.press(at + vec2(32.0, 3.0), PointerButton::Primary, false);
        let out = rig.frame(Vec::new());
        assert!(moved, "dragging the title bar moves the window");
        assert!(
            !rig.menu_open() && !shows(&out, "New crate…"),
            "and opens no menu"
        );

        let out = rig.click(at);
        assert!(!start_drag(&out));
        assert!(rig.menu_open(), "a click opens the crate menu");
        let out = rig.frame(Vec::new());
        for item in ["New crate…", "Rename crate…", "Delete crate…"] {
            assert!(shows(&out, item), "{item} in {:?}", text_list(&out));
        }
        assert!(
            shows(&out, &crate_menu_label("Playlist", true, false, false)),
            "nothing plays: no ⏵"
        );
        let font = egui::FontId::proportional(14.0);
        assert!(
            rig.ctx.fonts_mut(|f| f.has_glyphs(&font, "•⏵")),
            "the menu's marks have glyphs"
        );
    }

    #[test]
    fn the_crate_menu_switches_creates_and_refuses_duplicate_names() {
        let mut rig = Rig::new("menu", Vec::new(), |_| {});
        let keepers = rig.crate_with("Keepers", &["tone.flac"]);
        rig.click(rig.title_bar());
        rig.click_text(&crate_menu_label("Keepers", false, false, false));
        assert_eq!(rig.app.crates.shown_id(), keepers);
        assert!(!rig.menu_open(), "choosing a crate closes the menu");

        // Marks: • on the shown crate; ⏵ on the playing one only while it plays.
        rig.click(rig.title_bar());
        let out = rig.frame(Vec::new());
        assert!(shows(
            &out,
            &crate_menu_label("Keepers", true, false, false)
        ));
        assert!(shows(
            &out,
            &crate_menu_label("Playlist", false, false, false)
        ));

        rig.click_text("New crate…");
        rig.type_text("keepers");
        let out = rig.frame(Vec::new());
        assert!(
            shows(&out, "A crate named \"Keepers\" already exists"),
            "{:?}",
            text_list(&out)
        );
        assert_eq!(rig.app.crates.list().len(), 2, "no crate is created");
        rig.select_all_text();
        rig.type_text("Gig 12 Oct");
        let names: Vec<&str> = rig
            .app
            .crates
            .list()
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, ["Playlist", "Keepers", "Gig 12 Oct"]);
        let gig = rig.app.crates.list()[2].id;
        assert_eq!(rig.app.crates.shown_id(), gig, "the new crate is shown");
        assert!(rig.app.crates.shown().is_empty());
        assert!(rig.app.name_dialog.is_none());

        // Rename the shown crate.
        rig.click(rig.title_bar());
        rig.click_text("Rename crate…");
        rig.select_all_text();
        rig.type_text("Gig 13 Oct");
        assert_eq!(rig.app.crates.name(gig), "Gig 13 Oct");
    }

    #[test]
    fn deleting_a_crate_with_entries_asks_first_and_playlist_cannot_go() {
        let mut rig = Rig::new("delete", Vec::new(), |_| {});
        // Playlist: Rename and Delete are unavailable.
        rig.click(rig.title_bar());
        rig.click_text("Delete crate…");
        assert!(rig.app.confirm_delete.is_none());
        rig.click(rig.title_bar());
        rig.click_text("Rename crate…");
        assert!(rig.app.name_dialog.is_none());
        assert_eq!(rig.app.crates.list().len(), 1);

        let keepers = rig.crate_with("Keepers", &["tone.flac", "tone.wav"]);
        rig.app.apply(Action::ShowCrate(keepers), &rig.ctx.clone());
        rig.click(rig.title_bar());
        rig.click_text("Delete crate…");
        let out = rig.frame(Vec::new());
        assert!(
            shows(&out, "Delete crate \"Keepers\" (2 entries)?"),
            "{:?}",
            text_list(&out)
        );
        assert_eq!(rig.app.crates.list().len(), 2, "nothing deleted yet");
        rig.click_text("Cancel");
        assert_eq!(rig.app.crates.list().len(), 2, "cancelled");

        rig.click(rig.title_bar());
        rig.click_text("Delete crate…");
        rig.click_text("Delete");
        assert_eq!(rig.app.crates.list().len(), 1, "deleted after confirming");
        assert_eq!(rig.app.crates.shown_id(), PLAYLIST);

        // An empty crate goes without asking.
        let empty = rig.app.crates.create("Empty").unwrap();
        rig.app.apply(Action::ShowCrate(empty), &rig.ctx.clone());
        rig.click(rig.title_bar());
        rig.click_text("Delete crate…");
        assert_eq!(rig.app.crates.list().len(), 1);
    }

    #[test]
    fn the_lp_knob_sweeps_the_filter_and_double_click_turns_it_off() {
        let with_eq = |store: &Store| {
            store
                .save(
                    SETTINGS_FILE,
                    &Settings {
                        scale: 1,
                        show_eq: true,
                        show_waveform: false,
                        repeat: Repeat::One,
                        ..Default::default()
                    },
                )
                .unwrap();
        };
        let mut rig = Rig::new("lp-knob", Vec::new(), with_eq);
        // The EQ sits under the main section (116 px); the knob at (46, 17) in it.
        let knob = pos2(46.0 + 7.0, 116.0 + 17.0 + 7.0);
        assert_eq!(rig.engine().filter(), audio::filter::OFF, "off at launch");
        rig.frame(vec![Event::PointerMoved(knob)]);
        rig.press(knob, PointerButton::Primary, true);
        for dy in [10.0, 30.0, 50.0] {
            rig.frame(vec![Event::PointerMoved(knob + vec2(0.0, dy))]);
        }
        rig.press(knob + vec2(0.0, 50.0), PointerButton::Primary, false);
        rig.frame(Vec::new());
        let lp = rig.app.lp_knob;
        assert!((lp - 0.5).abs() < 0.11, "dragged down by half a turn: {lp}");
        assert_eq!(rig.engine().filter(), lp, "the engine follows");

        rig.double_click(knob);
        assert_eq!(rig.app.lp_knob, audio::filter::OFF);
        assert_eq!(rig.engine().filter(), audio::filter::OFF);
    }

    /// Sets the tempo of the shown crate's entries, in order.
    fn set_tempos(rig: &mut Rig, bpms: &[u16]) {
        let p = rig.app.crates.shown_mut();
        for (e, &b) in p.entries_mut().zip(bpms) {
            e.bpm = Some(b);
        }
    }

    #[test]
    fn under_a_bpm_filter_rows_selection_and_the_cursor_skip_hidden_entries() {
        let mut rig = Rig::new("bpm-rows", Vec::new(), |_| {});
        let ids = rig.fill_playlist(6);
        set_tempos(&mut rig, &[124, 134, 124, 138, 124, 136]);
        let ctx = rig.ctx.clone();
        rig.app.apply(Action::SetBpmFilter(Some((130, 140))), &ctx);
        let out = rig.frame(Vec::new());
        let drawn: Vec<String> = texts(&out).into_iter().map(|t| t.text).collect();
        let numbered = |n: usize| drawn.iter().any(|t| t.starts_with(&format!("{n}. ")));
        assert!(
            numbered(2) && numbered(4) && numbered(6),
            "crate numbers kept: {drawn:?}"
        );
        assert!(
            !numbered(1) && !numbered(3) && !numbered(5),
            "hidden entries aren't drawn"
        );

        // Row 1 is entry 2; Shift-click on row 3 (entry 6) selects only shown entries.
        rig.click(rig.row(0));
        rig.mods = Modifiers::SHIFT;
        rig.click(rig.row(2));
        rig.mods = Modifiers::NONE;
        assert_eq!(
            rig.app.crates.shown().selected_ids(),
            [ids[1], ids[3], ids[5]]
        );

        // ↓ from entry 2 goes to entry 4.
        rig.click(rig.row(0));
        rig.key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(rig.app.crates.shown().cursor(), Some(ids[3]));
        rig.app.apply(Action::SelectAll, &ctx);
        assert_eq!(
            rig.app.crates.shown().selected_ids(),
            [ids[1], ids[3], ids[5]]
        );

        // Export takes the whole crate.
        assert_eq!(files::m3u_entries(rig.app.crates.shown()).len(), 6);

        // P on an entry the filter hides turns the filter off.
        rig.app.crates.shown_mut().set_current(Some(ids[2]));
        rig.frame(Vec::new());
        rig.key(Key::P, Modifiers::NONE);
        assert_eq!(rig.app.crates.shown().bpm_filter(), None);
        assert_eq!(rig.app.crates.shown().cursor(), Some(ids[2]));
    }

    /// Row `i` of the crate sidebar (a playlist at least 600 pixels wide).
    fn side_row(i: usize) -> Pos2 {
        pos2(
            PL_LEFT + 12.0 + 40.0,
            PL_TOP + LIST_TOP + i as f32 * 13.0 + 6.5,
        )
    }

    /// Row `i` of the list beside the sidebar, under the column header.
    fn side_list_row(i: usize) -> Pos2 {
        pos2(
            PL_LEFT + 12.0 + 110.0 + 100.0,
            PL_TOP + LIST_TOP + (i + 1) as f32 * 13.0 + 6.5,
        )
    }

    /// A wide playlist of three files and two more crates, Keepers and Friday.
    fn sidebar_rig(name: &str) -> (Rig, Vec<EntryId>, CrateId, CrateId) {
        let mut rig = Rig::new(name, Vec::new(), |_| {});
        let tracks = ["tone.flac", "tone.wav", "tone.ogg"]
            .map(|f| TrackRef::new(fixture(f).to_string_lossy()));
        rig.app.crates.shown_mut().add(tracks);
        let ids = rig.ids(PLAYLIST);
        let keepers = rig.app.crates.create("Keepers").unwrap();
        let friday = rig.app.crates.create("Friday").unwrap();
        rig.app.settings.playlist_width = 700;
        rig.frame(Vec::new());
        (rig, ids, keepers, friday)
    }

    #[test]
    fn the_crate_sidebar_shows_crates_when_it_fits() {
        let (mut rig, _, keepers, _) = sidebar_rig("sidebar");
        assert!(rig.app.pl_sidebar(), "on by default at 700 pixels");
        let out = rig.frame(Vec::new());
        assert!(shows(&out, "Playlist"), "{:?}", text_list(&out));
        assert!(shows(&out, "Keepers") && shows(&out, "+ New crate"));
        assert!(!shows(&out, "DISCOGS"), "no collection: no Discogs group");

        // A click shows the crate.
        rig.click(side_row(1));
        assert_eq!(rig.app.crates.shown_id(), keepers);
        assert!(shows(&rig.frame(Vec::new()), "•"), "the shown crate's dot");

        // Right-click: Rename… opens the name dialog for that crate.
        rig.click_with(side_row(2), PointerButton::Secondary);
        rig.click_text("Rename crate…");
        assert!(rig.app.name_dialog.is_some());
        rig.app.name_dialog = None;
        rig.click(side_row(3));
        assert!(rig.app.name_dialog.is_some(), "+ New crate asks for a name");
        rig.app.name_dialog = None;

        // With the sidebar there, the title bar opens no crate menu; right-click deletes.
        rig.click(rig.title_bar());
        assert!(!rig.menu_open(), "the sidebar does the menu's job");
        let before = rig.app.crates.list().len();
        rig.click_with(side_row(1), PointerButton::Secondary);
        rig.click_text("Delete crate…");
        assert_eq!(
            rig.app.crates.list().len(),
            before - 1,
            "the empty crate is deleted"
        );

        // Narrower than 600 px it hides, and the title bar opens the crate menu again; it
        // comes back when widened: no switch.
        rig.app.settings.playlist_width = 400;
        rig.frame(Vec::new());
        assert!(!rig.app.pl_sidebar());
        rig.click(rig.title_bar());
        assert!(rig.menu_open(), "the menu is the way to crates when narrow");
        rig.click(rig.title_bar());
        rig.app.settings.playlist_width = 700;
        rig.frame(Vec::new());
        assert!(rig.app.pl_sidebar());
    }

    #[test]
    fn an_old_sidebar_setting_is_ignored() {
        let old = |store: &Store| {
            std::fs::write(
                store.dir().join(SETTINGS_FILE),
                "(scale: 1, show_eq: false, show_waveform: false, crate_sidebar: false, \
                 playlist_maximized: true)",
            )
            .unwrap();
        };
        let rig = Rig::new("sidebar-old-setting", Vec::new(), old);
        assert!(rig.app.settings.playlist_maximized, "the file loaded");
        assert!(rig.app.pl_sidebar(), "shown when it fits");
    }

    #[test]
    fn control_click_and_the_delete_key_delete_a_sidebar_crate() {
        let (mut rig, ids, keepers, friday) = sidebar_rig("sidebar-delete");
        rig.app.send_to(&ids[..2], friday);

        // Control-click (a Mac's right-click) opens the crate's menu, without showing it.
        rig.mods = Modifiers {
            ctrl: true,
            mac_cmd: false,
            command: false,
            ..Modifiers::NONE
        };
        let mac = cfg!(target_os = "macos");
        rig.click(side_row(1));
        rig.mods = Modifiers::NONE;
        if mac {
            let out = rig.frame(Vec::new());
            assert!(shows(&out, "Delete crate…"), "{:?}", text_list(&out));
            assert_eq!(rig.app.crates.shown_id(), PLAYLIST, "not shown");
            rig.click_text("Delete crate…");
            assert!(
                rig.app.crates.find("Keepers").is_none(),
                "an empty crate goes at once"
            );
        } else {
            rig.app
                .apply(Action::DeleteCrate(keepers), &rig.ctx.clone());
        }

        // A click on a crate, then Delete: it is deleted after confirming (it has entries).
        rig.click(side_row(1));
        assert_eq!(rig.app.crates.shown_id(), friday);
        rig.key(Key::Delete, Modifiers::NONE);
        assert_eq!(rig.app.confirm_delete, Some(friday), "asks first");
        rig.click_text("Delete");
        assert!(rig.app.crates.find("Friday").is_none());

        // After a click in the list, Delete removes entries again.
        rig.click(side_row(0));
        rig.click(side_list_row(0));
        rig.key(Key::Delete, Modifiers::NONE);
        assert_eq!(rig.ids(PLAYLIST).len(), 2, "an entry removed, no crate");

        // The Playlist crate is never deleted from the keyboard.
        rig.click(side_row(0));
        rig.key(Key::Delete, Modifiers::NONE);
        assert!(rig.app.crates.find("Playlist").is_some());
    }

    #[test]
    fn the_sidebar_lists_the_wantlist_above_the_collection_under_discogs() {
        let (mut rig, _, _, _) = sidebar_rig("sidebar-wantlist");
        let coll = rig.app.crates.create("Collection: digger").unwrap();
        rig.app.crates.set_collection(coll);
        let wl = rig.app.crates.create("Wantlist: digger").unwrap();
        let out = rig.frame(Vec::new());
        let y = |out: &egui::FullOutput, name: &str| {
            texts(out)
                .into_iter()
                .find(|t| t.text == name)
                .map(|t| t.rect.center().y)
                .unwrap_or_else(|| panic!("{name} in {:?}", text_list(out)))
        };
        assert!(
            y(&out, "Wantlist: digger") < y(&out, "DISCOGS"),
            "unconnected, one of the user's crates"
        );
        rig.app.crates.set_wantlist(wl, true);
        let out = rig.frame(Vec::new());
        let (heading, w, c) = (
            y(&out, "DISCOGS"),
            y(&out, "Wantlist: digger"),
            y(&out, "Collection: digger"),
        );
        assert!(
            heading < w && w < c,
            "DISCOGS, the wantlist, then the collection"
        );
    }

    #[test]
    fn the_sidebar_pins_the_collection_and_the_playlist_crate_clears() {
        let (mut rig, _, _, _) = sidebar_rig("sidebar-collection");
        let coll = rig.app.crates.create("Collection: digger").unwrap();
        rig.app.crates.set_collection(coll);
        let out = rig.frame(Vec::new());
        assert!(shows(&out, "DISCOGS"), "{:?}", text_list(&out));
        // Pinned to the bottom row of the sidebar, under DISCOGS.
        let last = rig.app.pl_rows() - 1;
        let bottom = pos2(
            PL_LEFT + 12.0 + 40.0,
            PL_TOP + LIST_TOP + last as f32 * 13.0 + 6.5,
        );
        rig.click(bottom);
        assert_eq!(rig.app.crates.shown_id(), coll);
        // And still there after the others: + New crate follows the user's crates.
        rig.click(side_row(3));
        assert!(
            rig.app.name_dialog.is_some(),
            "+ New crate right after Friday"
        );
        rig.app.name_dialog = None;

        // Playlist can't be renamed or deleted: its menu clears it instead.
        rig.click_with(side_row(0), PointerButton::Secondary);
        let out = rig.frame(Vec::new());
        assert!(!shows(&out, "Delete crate…"));
        rig.click_text("Clear crate");
        assert!(rig.app.crates.get(PLAYLIST).unwrap().is_empty());
        assert!(rig.app.crates.find("Playlist").is_some());
    }

    #[test]
    fn dragging_entries_onto_a_sidebar_crate_sends_them() {
        let (mut rig, ids, _, friday) = sidebar_rig("sidebar-drop");
        rig.app.send_to(&[ids[1]], friday);
        // Entries 1 to 3 selected; drag entry 1 onto Friday, which holds entry 2 already.
        rig.click(side_list_row(0));
        rig.mods = Modifiers::SHIFT;
        rig.click(side_list_row(2));
        rig.mods = Modifiers::NONE;
        let from = side_list_row(0);
        rig.frame(vec![Event::PointerMoved(from)]);
        rig.press(from, PointerButton::Primary, true);
        for t in [0.3, 0.6, 1.0] {
            let p = from + (side_row(2) - from) * t;
            rig.frame(vec![Event::PointerMoved(p)]);
        }
        rig.press(side_row(2), PointerButton::Primary, false);
        rig.frame(Vec::new());
        assert_eq!(rig.ids(friday).len(), 3, "two added, one already there");
        let msg = rig
            .app
            .message
            .as_ref()
            .map(|(m, _)| m.clone())
            .unwrap_or_default();
        assert_eq!(msg, "Sent 2 entries to Friday (1 already there)");
        assert_eq!(rig.ids(PLAYLIST), ids, "the list wasn't reordered");

        // Dropped inside the list, an entry is reordered and no crate receives it.
        let from = side_list_row(0);
        rig.frame(vec![Event::PointerMoved(from)]);
        rig.press(from, PointerButton::Primary, true);
        for t in [0.3, 0.6, 1.0] {
            let p = from + (side_list_row(2) - from) * t;
            rig.frame(vec![Event::PointerMoved(p)]);
        }
        rig.press(side_list_row(2), PointerButton::Primary, false);
        rig.frame(Vec::new());
        assert_eq!(rig.ids(PLAYLIST), [ids[1], ids[2], ids[0]]);
        assert_eq!(rig.ids(friday).len(), 3);
    }

    /// The centre of footer button `name` (`pl_plus`, `pl_menu`, `pl_opts`).
    /// The middle of the filter bar, top to bottom.
    const BAR_Y: f32 = PL_TOP + 20.0 + 8.0;

    /// The filter bar's ×, at its right end.
    fn bar_clear(rig: &Rig) -> Pos2 {
        let w = rig.app.pl_geometry().0 as f32;
        let clear_w = rig.app.def.sprite("bpm_clear").w as f32;
        pos2(PL_LEFT + w - 4.0 - clear_w / 2.0, BAR_Y)
    }

    fn footer_button(rig: &Rig, name: &str) -> Pos2 {
        let r = rig.app.def.at(name);
        let bottom = PL_TOP + LIST_TOP + rig.app.pl_rows() as f32 * 13.0;
        pos2(
            PL_LEFT + r.x as f32 + r.w as f32 / 2.0,
            bottom + r.y as f32 + r.h as f32 / 2.0,
        )
    }

    #[test]
    fn the_footer_has_an_add_and_a_crate_menu() {
        let mut rig = Rig::new("footer", Vec::new(), |_| {});
        let ids = rig.fill_playlist(3);
        set_tempos(&mut rig, &[128, 122, 140]);
        rig.frame(Vec::new());
        assert!(
            rig.app.def.sprite("pl_add").w == 0,
            "no ADD/REM/SEL/MISC/OPT any more"
        );

        rig.click(footer_button(&rig, "pl_plus"));
        let out = rig.frame(Vec::new());
        for t in ["Add files…", "Add folder…", "Import M3U…"] {
            assert!(shows(&out, t), "{t}: {:?}", text_list(&out));
        }
        assert!(!shows(&out, "Discogs…"), "no app setting in it");
        rig.click(footer_button(&rig, "pl_plus"));

        rig.click(footer_button(&rig, "pl_menu"));
        let out = rig.frame(Vec::new());
        for t in [
            "Select all",
            "Invert selection",
            "Remove selected",
            "Clear crate",
            "Sort",
            "Export M3U…",
        ] {
            assert!(shows(&out, t), "{t}: {:?}", text_list(&out));
        }
        assert!(!shows(&out, "Show all tempos"), "the filter bar clears it");
        rig.click_text("Sort");
        rig.click_text("BPM");
        assert_eq!(rig.ids(PLAYLIST), [ids[1], ids[0], ids[2]], "sorted by BPM");

        rig.click(footer_button(&rig, "pl_opts"));
        let out = rig.frame(Vec::new());
        for t in ["Double size (2×)", "Spectrogram (S)"] {
            assert!(shows(&out, t), "{t}: {:?}", text_list(&out));
        }
        rig.click_text("Spectrogram (S)");
        assert!(rig.app.spectro_open, "the gear's items act");
    }

    #[test]
    fn a_right_click_on_the_main_window_opens_the_options() {
        let mut rig = Rig::new("options", Vec::new(), |_| {});
        // The track info area: no control there.
        rig.click_with(pos2(150.0, 28.0), PointerButton::Secondary);
        let out = rig.frame(Vec::new());
        assert!(shows(&out, "Double size (2×)"), "{:?}", text_list(&out));
        rig.click_text("Spectrogram (S)");
        assert!(rig.app.spectro_open, "Spectrogram works from Options");
        rig.app.spectro_open = false;

        // The volume slider keeps its own right-click.
        rig.click_with(pos2(141.0, 63.0), PointerButton::Secondary);
        let out = rig.frame(Vec::new());
        assert!(!shows(&out, "Double size (2×)"), "{:?}", text_list(&out));

        // While the playlist is maximized, the mini player has it too.
        rig.app.settings.playlist_maximized = true;
        rig.frame(Vec::new());
        rig.click_with(pos2(150.0, 15.0), PointerButton::Secondary);
        let out = rig.frame(Vec::new());
        assert!(shows(&out, "Spectrogram (S)"), "{:?}", text_list(&out));
    }

    #[test]
    fn the_title_bar_counts_what_a_bpm_filter_shows() {
        let def = LoadedSkin::default_skin().def;
        assert_eq!(
            crate_title_with_count(&def, "Lowtide Tapes", Some((2, 4)), 200.0),
            "LOWTIDE TAPES · 2/4"
        );
        assert_eq!(
            crate_title_with_count(&def, "Lowtide Tapes", None, 200.0),
            "LOWTIDE TAPES"
        );
        // Short of room, the name gives way and the count stays.
        let t = crate_title_with_count(&def, "Lowtide Tapes", Some((42, 301)), 100.0);
        assert!(t.ends_with(" · 42/301") && t.starts_with("LOW"), "{t}");
    }

    #[test]
    fn the_bar_bpm_control_sets_the_range_and_the_bar_x_clears_it() {
        let mut rig = Rig::new("bpm-bar", Vec::new(), |_| {});
        let ids = rig.fill_playlist(7);
        rig.frame(Vec::new());
        assert_eq!(rig.app.bpm_slider, None, "no tempo: no control");
        set_tempos(&mut rig, &[124, 128, 137, 139]);
        rig.app.bpm_slider = None;
        rig.frame(Vec::new());
        let (x0, x1, text) = rig.app.bpm_slider.expect("the control is drawn");
        assert!(x1 - x0 == BPM_SLIDER_W && text, "{x0}..{x1}");
        assert_eq!(rig.app.pl_visible_rows(), rig.app.pl_rows(), "no row taken");

        // Drag the right handle to the middle of the track.
        let at = |x: f32| pos2(PL_LEFT + x, BAR_Y);
        rig.frame(vec![Event::PointerMoved(at(x1))]);
        rig.press(at(x1), PointerButton::Primary, true);
        let mid = (x0 + x1) / 2.0;
        for x in [x1 - 5.0, x1 - 15.0, mid] {
            rig.frame(vec![Event::PointerMoved(at(x))]);
        }
        rig.press(at(mid), PointerButton::Primary, false);
        rig.frame(Vec::new());
        let shown = rig.app.crates.shown();
        let (lo, hi) = shown.bpm_filter().expect("a range");
        assert_eq!(lo, 124, "the low handle stays");
        assert!((130..=133).contains(&hi), "the high handle moved to {hi}");
        assert_eq!(shown.shown_rows(), [0, 1]);
        assert_eq!(rig.app.crates.shown().cursor(), None);

        // The bar's × clears it; a double-click on the slider does too.
        rig.click(bar_clear(&rig));
        assert_eq!(rig.app.crates.shown().bpm_filter(), None, "× clears");
        let ctx = rig.ctx.clone();
        rig.app.apply(Action::SetBpmFilter(Some((130, 135))), &ctx);
        rig.frame(Vec::new());
        assert!(rig.app.crates.shown().shown_rows().is_empty());
        // Clicks count by time alone: let the × click above age out first.
        for _ in 0..30 {
            rig.frame(Vec::new());
        }
        rig.double_click(at(mid));
        assert_eq!(
            rig.app.crates.shown().bpm_filter(),
            None,
            "double-click clears"
        );
        assert_eq!(rig.ids(PLAYLIST), ids);
    }

    #[test]
    fn the_bpm_slider_is_short_and_fixed_and_the_search_keeps_its_room() {
        let mut rig = Rig::new("bpm-bar-narrow", Vec::new(), |_| {});
        rig.fill_playlist(3);
        set_tempos(&mut rig, &[124, 128, 139]);
        let p = rig.app.crates.shown_mut();
        for e in p.entries_mut() {
            e.duration = Some(23.0 * 3600.0 + 25.0 * 60.0); // 70 h in all: "0:00/70:15:00"
        }
        rig.frame(Vec::new());
        let (x0, x1, text) = rig.app.bpm_slider.expect("the control is drawn");
        assert_eq!(x1 - x0, BPM_SLIDER_W);
        assert!(text, "the range text fits at the classic width");
        assert!(
            x0 >= 4.0 + SEARCH_MIN_W + 4.0,
            "the search field keeps its room: {x0}"
        );

        // Wider, the slider keeps its width.
        rig.app.settings.playlist_width = 700;
        rig.frame(Vec::new());
        let (x0, x1, _) = rig.app.bpm_slider.unwrap();
        assert_eq!(x1 - x0, BPM_SLIDER_W);
    }

    /// Gives the shown crate's entries a release each (1, 2, …) with these styles.
    fn set_styles(rig: &mut Rig, styles: &[&str]) {
        let p = rig.app.crates.shown_mut();
        for (i, (e, st)) in p.entries_mut().zip(styles).enumerate() {
            e.origin = Some(crate::playlist::Origin {
                release: Some(i as u64 + 1),
                clip: Some(format!("c{i}")),
                styles: (*st).into(),
                ..Default::default()
            });
        }
    }

    #[test]
    fn the_style_filter_shows_chips_in_the_discogs_crates_only() {
        let mut rig = Rig::new("style-chips", Vec::new(), |_| {});
        rig.fill_playlist(4);
        set_styles(
            &mut rig,
            &["Deep House", "Minimal, Techno", "Electro", "Deep House"],
        );
        rig.app.settings.playlist_width = 700;
        rig.frame(Vec::new());
        assert_eq!(
            rig.app.filters_drawn, None,
            "not a Discogs crate: no control"
        );

        rig.app.crates.set_collection(PLAYLIST);
        rig.frame(Vec::new());
        let (x, tier) = rig.app.filters_drawn.expect("the control is drawn");
        assert_eq!(
            tier,
            FooterFilters::Chips,
            "4 short styles fit at 700 pixels"
        );
        assert_eq!(rig.app.pl_visible_rows(), rig.app.pl_rows(), "no row taken");
        // The first chip is the style of most records: DEEP HOUSE.
        let chip = pos2(PL_LEFT + x + 4.0, BAR_Y);
        rig.click(chip);
        let shown = rig.app.crates.shown();
        assert!(shown.picked(Facet::Style, "Deep House"));
        assert_eq!(shown.shown_rows().len(), 2, "the two Deep House records");
        for _ in 0..30 {
            rig.frame(Vec::new());
        }
        rig.double_click(chip);
        assert_eq!(
            rig.app.crates.shown().filter(Facet::Style),
            None,
            "double-click clears"
        );
        assert_eq!(rig.app.crates.shown().shown_rows().len(), 4);
    }

    #[test]
    fn many_styles_or_a_narrow_playlist_show_the_styles_button_and_its_list() {
        let mut rig = Rig::new("style-list", Vec::new(), |_| {});
        rig.fill_playlist(4);
        set_styles(
            &mut rig,
            &["Deep House", "Minimal, Techno", "Electro", "Tech House"],
        );
        rig.app.crates.set_collection(PLAYLIST);
        rig.frame(Vec::new());
        let (x, tier) = rig.app.filters_drawn.expect("the control is drawn");
        assert_eq!(
            tier,
            FooterFilters::Buttons,
            "the chips don't fit at the classic width"
        );
        rig.click(pos2(PL_LEFT + x + 4.0, BAR_Y));
        let out = rig.frame(Vec::new());
        assert!(
            shows(&out, "Minimal") && shows(&out, "Electro"),
            "{:?}",
            text_list(&out)
        );
        // The list's own "Electro" (record rows name their styles too): the last one drawn.
        let item = texts(&out)
            .into_iter()
            .rev()
            .find(|t| t.text == "Electro")
            .unwrap()
            .rect;
        rig.click(item.center());
        assert!(rig.app.crates.shown().picked(Facet::Style, "Electro"));
        assert_eq!(rig.app.crates.shown().shown_rows().len(), 1);

        // The search narrows the list.
        rig.click_text("Filter styles…");
        rig.frame(vec![Event::Text("house".into())]);
        let out = rig.frame(Vec::new());
        let listed = |t: &str| texts(&out).iter().filter(|x| x.text == t).count();
        // Only Electro's record row shows, so the House styles are drawn by the list alone.
        assert_eq!((listed("Deep House"), listed("Tech House")), (1, 1));
        assert_eq!(
            listed("Electro"),
            1,
            "only its record row: {:?}",
            text_list(&out)
        );
        rig.click_text("Clear");
        assert_eq!(rig.app.crates.shown().filter(Facet::Style), None);
    }

    /// Gives the shown crate's entries a release each (1, 2, …) with these styles, record
    /// artists and labels.
    fn set_records(rig: &mut Rig, records: &[(&str, &str, &str)]) {
        let p = rig.app.crates.shown_mut();
        for (i, (e, (st, artist, label))) in p.entries_mut().zip(records).enumerate() {
            e.origin = Some(crate::playlist::Origin {
                release: Some(i as u64 + 1),
                clip: Some(format!("c{i}")),
                styles: (*st).into(),
                artist: (*artist).into(),
                label: (*label).into(),
                ..Default::default()
            });
        }
    }

    /// A collection crate of six records with 3 styles, 4 artists and 3 labels.
    fn credited_rig(name: &str) -> Rig {
        let mut rig = Rig::new(name, Vec::new(), |_| {});
        rig.fill_playlist(6);
        set_records(
            &mut rig,
            &[
                ("Deep House", "Theo Parrish", "Sound Signature"),
                (
                    "Deep House",
                    "Theo Parrish & Marcellus Pittman",
                    "Unirhythm",
                ),
                ("Minimal", "Nightcraft", "Lowtide Tapes"),
                ("Minimal", "Nightcraft", "Lowtide Tapes"),
                ("Techno", "Various", "Lowtide Tapes"),
                ("Techno", "Theo Parrish", "Sound Signature"),
            ],
        );
        rig.app.crates.set_collection(PLAYLIST);
        rig
    }

    #[test]
    fn the_bar_shows_chips_then_buttons_then_filters_as_it_narrows() {
        let mut rig = credited_rig("filter-tiers");
        rig.app.settings.playlist_width = 700;
        rig.frame(Vec::new());
        let (x, tier) = rig.app.filters_drawn.unwrap();
        assert_eq!(tier, FooterFilters::Chips);
        // The search field takes at most half the bar; the filters follow it.
        assert!(x <= 4.0 + 350.0 + 4.0 + STYLE_GAP, "{x}");
        rig.app.settings.playlist_width = 300;
        rig.frame(Vec::new());
        let (x, tier) = rig.app.filters_drawn.unwrap();
        assert_eq!(tier, FooterFilters::Buttons);
        assert!(x >= 4.0 + SEARCH_MIN_W, "after the search field: {x}");
        // Classic width with tempos: one FILTERS button after the BPM control.
        rig.app.settings.playlist_width = 275;
        set_tempos(&mut rig, &[124, 128, 132, 136, 138, 140]);
        rig.frame(Vec::new());
        assert_eq!(rig.app.filters_drawn.unwrap().1, FooterFilters::Folded);
        assert!(rig.app.bpm_slider.is_some());
    }

    #[test]
    fn the_filters_button_opens_the_panel_at_classic_width() {
        let mut rig = credited_rig("filter-panel");
        set_tempos(&mut rig, &[124, 128, 132, 136, 138, 140]);
        rig.frame(Vec::new());
        let (x, tier) = rig.app.filters_drawn.unwrap();
        assert_eq!(tier, FooterFilters::Folded);
        rig.click(pos2(PL_LEFT + x + 4.0, BAR_Y));
        let out = rig.frame(Vec::new());
        for t in ["STYLE", "ARTIST", "LABEL"] {
            assert!(shows(&out, t), "{t}: {:?}", text_list(&out));
        }
        // The ARTIST tab: search, then pick.
        rig.click_text("ARTIST");
        let out = rig.frame(Vec::new());
        assert!(shows(&out, "Nightcraft"), "{:?}", text_list(&out));
        rig.click_text("Filter artists…");
        rig.frame(vec![Event::Text("parrish".into())]);
        let out = rig.frame(Vec::new());
        assert!(shows(&out, "Theo Parrish") && shows(&out, "Theo Parrish & Marcellus Pittman"));
        assert!(
            !text_list(&out).iter().any(|t| t == "Nightcraft"),
            "{:?}",
            text_list(&out)
        );
        rig.click_text("Theo Parrish");
        assert_eq!(
            rig.app.crates.shown().shown_rows().len(),
            2,
            "not the joint record"
        );
        // The LABEL tab keeps the artist picked.
        rig.click_text("LABEL");
        rig.click_text("Sound Signature");
        assert!(rig.app.crates.shown().picked(Facet::Artist, "Theo Parrish"));
        assert!(
            rig.app
                .crates
                .shown()
                .picked(Facet::Label, "Sound Signature")
        );
        // FILTERS counts the set filters; ☰ has no filter items.
        egui::Popup::close_all(&rig.ctx.clone());
        rig.frame(Vec::new());
        rig.click(footer_button(&rig, "pl_menu"));
        let out = rig.frame(Vec::new());
        assert!(!text_list(&out).iter().any(|t| t.starts_with("Filter by")));
        assert!(!shows(&out, "Show all records"));
        // The bar's × turns them all off, the BPM range too.
        let ctx = rig.ctx.clone();
        rig.app.apply(Action::SetBpmFilter(Some((130, 140))), &ctx);
        egui::Popup::close_all(&ctx);
        rig.frame(Vec::new());
        rig.click(bar_clear(&rig));
        let p = rig.app.crates.shown();
        assert!(!p.is_filtered(), "everything off");
    }

    #[test]
    fn a_lit_button_counts_its_picks_and_a_double_click_clears_them() {
        let mut rig = credited_rig("filter-button");
        rig.app.settings.playlist_width = 700;
        let ctx = rig.ctx.clone();
        rig.app.apply(
            Action::TogglePick(Facet::Label, "Lowtide Tapes".into()),
            &ctx,
        );
        rig.app
            .apply(Action::TogglePick(Facet::Label, "Unirhythm".into()), &ctx);
        rig.frame(Vec::new());
        assert_eq!(rig.app.crates.shown().shown_rows().len(), 4);
        // Chips (DEEP HOUSE MINIMAL TECHNO), then ARTISTS, then "LABELS 2".
        let (x, _) = rig.app.filters_drawn.unwrap();
        let advance = rig.app.def.font.advance as f32;
        let before = "DEEP HOUSE MINIMAL TECHNO ARTISTS ".len() as f32 * advance - 4.0 * advance
            + 4.0 * STYLE_GAP;
        let labels = pos2(PL_LEFT + x + before + 6.0, BAR_Y);
        rig.double_click(labels);
        assert_eq!(rig.app.crates.shown().filter(Facet::Label), None);
        assert_eq!(rig.app.crates.shown().shown_rows().len(), 6);
    }

    #[test]
    fn a_dig_crate_offers_only_the_format_filter() {
        let mut rig = Rig::new("format-filter", Vec::new(), |_| {});
        rig.fill_playlist(4);
        set_records(
            &mut rig,
            &[
                ("Electro", "A", "Analogical Force"),
                ("Electro", "B", "Analogical Force"),
                ("Techno", "C", "Other"),
                ("Techno", "D", "Other"),
            ],
        );
        for (e, f) in
            rig.app
                .crates
                .shown_mut()
                .entries_mut()
                .zip(["Vinyl", "File", "Vinyl, CD", "File"])
        {
            e.origin.as_mut().unwrap().formats = f.into();
        }
        rig.app.settings.playlist_width = 400;
        rig.frame(Vec::new());
        assert_eq!(rig.app.offered_facets(), [Facet::Format]);
        // No styles here: just the FORMATS button, opening the panel's only tab.
        let (x, tier) = rig.app.filters_drawn.unwrap();
        assert_eq!(tier, FooterFilters::Buttons);
        rig.click(pos2(PL_LEFT + x + 4.0, BAR_Y));
        let out = rig.frame(Vec::new());
        assert!(
            shows(&out, "FORMAT") && !shows(&out, "STYLE"),
            "{:?}",
            text_list(&out)
        );
        assert!(
            shows(&out, "Vinyl") && shows(&out, "File"),
            "{:?}",
            text_list(&out)
        );
        rig.click_text("Vinyl");
        assert_eq!(
            rig.app.crates.shown().shown_rows(),
            [0, 2],
            "vinyl records only"
        );
        let ctx = rig.ctx.clone();
        rig.app.apply(Action::ClearPicks(None), &ctx);
        assert!(!rig.app.crates.shown().is_filtered());
    }

    #[test]
    fn a_row_menu_keeps_only_its_label_or_searches_it() {
        // The collection crate: Only this label picks it, and LABELS counts it.
        let mut rig = credited_rig("row-only");
        rig.frame(Vec::new());
        rig.click_with(rig.row(0), PointerButton::Secondary);
        let out = rig.frame(Vec::new());
        assert!(shows(&out, "Only this artist"), "{:?}", text_list(&out));
        rig.click_text("Only this label");
        let p = rig.app.crates.shown();
        let picked: Vec<&String> = p.filter(Facet::Label).unwrap().iter().collect();
        assert_eq!(picked, ["Sound Signature"]);
        assert_eq!(p.shown_rows().len(), 2);

        // Another crate: Search ‹label› puts it in the search field, which takes the keyboard.
        let mut rig = Rig::new("row-search", Vec::new(), |_| {});
        rig.fill_playlist(3);
        set_records(
            &mut rig,
            &[
                ("Techno", "A", "Lowtide Tapes"),
                ("Techno", "B", "Other"),
                ("Techno", "C", "Lowtide Tapes"),
            ],
        );
        rig.frame(Vec::new());
        rig.click_with(rig.row(0), PointerButton::Secondary);
        let out = rig.frame(Vec::new());
        assert!(!shows(&out, "Only this label"), "not offered here");
        rig.click_text("Search Lowtide Tapes");
        rig.frame(Vec::new());
        assert_eq!(rig.app.pl_search, "Lowtide Tapes");
        assert_eq!(rig.app.crates.shown().shown_rows(), [0, 2]);
        assert!(rig.ctx.memory(|m| m.has_focus(Id::new("pl_search"))));
    }

    /// The middle of the filter bar's search field.
    fn search_at() -> Pos2 {
        pos2(PL_LEFT + 40.0, PL_TOP + 20.0 + 8.0)
    }

    /// Typing `text` as a keyboard does: each letter's key press and its text.
    fn type_keys(rig: &mut Rig, text: &str) {
        for c in text.chars() {
            let key = Key::from_name(&c.to_uppercase().to_string()).expect("a letter key");
            rig.frame(vec![
                Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                },
                Event::Text(c.to_string()),
            ]);
        }
    }

    /// Three files playing, credited to A, B and C, the first and last on Lowtide Tapes.
    fn search_rig(name: &str) -> (Rig, Vec<EntryId>) {
        let mut rig = Rig::new(
            name,
            vec![
                fixture("tone.flac"),
                fixture("tone.wav"),
                fixture("tone.ogg"),
            ],
            |_| {},
        );
        rig.until(
            |r| r.app.position.state == PlayState::Playing,
            "the first file plays",
        );
        let ids = rig.ids(PLAYLIST);
        set_records(
            &mut rig,
            &[
                ("Techno", "A", "Lowtide Tapes"),
                ("Techno", "B", "Other"),
                ("Techno", "C", "Lowtide Tapes"),
            ],
        );
        (rig, ids)
    }

    #[test]
    fn typing_in_the_search_bar_never_triggers_the_letter_shortcuts() {
        let (mut rig, _) = search_rig("search-keys");
        rig.click(search_at());
        type_keys(&mut rig, "xcvfnih");
        assert_eq!(rig.app.pl_search, "xcvfnih");
        assert_eq!(
            rig.app.position.state,
            PlayState::Playing,
            "not paused or stopped"
        );
        assert!(rig.app.fullscreen.is_none(), "no fullscreen");
        assert!(!rig.app.help, "no help");
        assert!(
            rig.app.crates.shown().shown_rows().is_empty(),
            "nothing matches"
        );
    }

    #[test]
    fn search_label_clears_a_bpm_range_set_before_it() {
        let (mut rig, _) = search_rig("search-clears");
        for (e, b) in rig.app.crates.shown_mut().entries_mut().zip([80, 120, 150]) {
            e.bpm = Some(b);
        }
        rig.app.crates.shown_mut().set_bpm_filter(Some((92, 171)));
        assert!(rig.app.crates.shown().bpm_filter().is_some());
        rig.app
            .crates
            .shown_mut()
            .set_pick(Facet::Artist, "C", true);
        let ctx = rig.ctx.clone();
        rig.app.apply(Action::SearchFor("lowtide".into()), &ctx);
        let p = rig.app.crates.shown();
        assert_eq!(p.bpm_filter(), None);
        assert!(!p.picked(Facet::Artist, "C"));
        assert_eq!(p.shown_rows(), [0, 2], "the whole crate is searched");
    }

    #[test]
    fn a_selected_search_is_drawn_once_in_the_skin() {
        let (mut rig, _) = search_rig("search-select");
        rig.click(search_at());
        type_keys(&mut rig, "vigne");
        let out = rig.key(Key::A, Modifiers::COMMAND);
        // egui's own glyphs stay invisible, selected or not.
        fn visible_glyphs(shape: &egui::Shape) -> bool {
            match shape {
                egui::Shape::Text(t) => t
                    .galley
                    .rows
                    .iter()
                    .any(|r| r.row.visuals.mesh.vertices.iter().any(|v| v.color.a() > 0)),
                egui::Shape::Vec(v) => v.iter().any(visible_glyphs),
                _ => false,
            }
        }
        assert!(!out.shapes.iter().any(|s| visible_glyphs(&s.shape)));
        let lcd = color(rig.app.def.colors.lcd);
        let field = Rect::from_center_size(search_at(), vec2(60.0, 16.0));
        let blocks = |out: &egui::FullOutput| {
            rects(out)
                .into_iter()
                .filter(|(r, c)| *c == lcd && r.width() > 2.0 && field.intersects(*r))
                .count()
        };
        assert_eq!(blocks(&out), 1, "the selection is an LCD block");
        type_keys(&mut rig, "a");
        let out = rig.frame(Vec::new());
        assert_eq!(rig.app.pl_search, "a");
        assert_eq!(blocks(&out), 0, "nothing selected after typing over it");
    }

    #[test]
    fn the_search_filters_as_typed_and_its_keys_work() {
        let (mut rig, ids) = search_rig("search-live");
        // Cmd+F gives the field the keyboard.
        rig.key(Key::F, Modifiers::COMMAND);
        rig.frame(Vec::new());
        assert!(rig.ctx.memory(|m| m.has_focus(Id::new("pl_search"))));
        assert!(rig.app.fullscreen.is_none());
        type_keys(&mut rig, "lowtide");
        rig.frame(Vec::new());
        assert_eq!(rig.app.crates.shown().shown_rows(), [0, 2]);
        assert_eq!(rig.app.queue, [ids[0], ids[2]], "next follows the search");
        // Esc clears only the search; the label filter stays.
        let ctx = rig.ctx.clone();
        rig.app
            .apply(Action::TogglePick(Facet::Artist, "C".into()), &ctx);
        rig.key(Key::Escape, Modifiers::NONE);
        rig.frame(Vec::new());
        assert!(rig.app.pl_search.is_empty());
        assert!(rig.app.crates.shown().search().is_empty());
        assert_eq!(
            rig.app.crates.shown().shown_rows(),
            [2],
            "the artist filter stays"
        );
        rig.app.apply(Action::ClearPicks(None), &ctx);
        // Enter plays the first shown entry.
        rig.click(search_at());
        type_keys(&mut rig, "other");
        rig.key(Key::Enter, Modifiers::NONE);
        rig.until(
            |r| r.app.crates.shown().current() == Some(ids[1]),
            "the first shown entry plays",
        );
        // P on the hidden playing entry clears the search and the filters.
        rig.click(search_at());
        type_keys(&mut rig, " zzz");
        rig.key(Key::Escape, Modifiers::NONE);
        rig.click(search_at());
        type_keys(&mut rig, "lowtide");
        rig.frame(Vec::new());
        assert_eq!(rig.app.crates.shown().shown_rows(), [0, 2]);
        rig.app.show_playing_entry();
        rig.frame(Vec::new());
        assert!(!rig.app.crates.shown().is_filtered());
        assert!(rig.app.pl_search.is_empty(), "the field follows");
    }

    #[test]
    fn the_rows_light_what_the_search_matches() {
        let (mut rig, _) = search_rig("search-light");
        rig.app
            .crates
            .shown_mut()
            .entries_mut()
            .nth(1)
            .unwrap()
            .title = "Night Moves".into();
        rig.click(search_at());
        type_keys(&mut rig, "moves");
        let out = rig.frame(Vec::new());
        let hl = color(rig.app.def.colors.pl_current);
        // The row's text in runs: the matched one in the current-entry colour.
        let mut lit = Vec::new();
        fn walk(shape: &egui::Shape, hl: Color32, lit: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) if t.galley.job.sections.len() > 1 => {
                    let job = &t.galley.job;
                    for sec in job.sections.iter().filter(|s| s.format.color == hl) {
                        lit.push(job.text[sec.byte_range.start.0..sec.byte_range.end.0].to_owned());
                    }
                }
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, hl, lit)),
                _ => {}
            }
        }
        for s in &out.shapes {
            walk(&s.shape, hl, &mut lit);
        }
        assert_eq!(lit, ["Moves"], "{:?}", text_list(&out));
    }

    #[test]
    fn showing_another_crate_clears_the_search() {
        let (mut rig, _) = search_rig("search-switch");
        rig.click(search_at());
        type_keys(&mut rig, "lowtide");
        rig.frame(Vec::new());
        assert!(rig.app.crates.shown().is_filtered());
        let other = rig.app.crates.create("Keepers").unwrap();
        let ctx = rig.ctx.clone();
        rig.app.apply(Action::ShowCrate(other), &ctx);
        rig.frame(Vec::new());
        assert!(rig.app.pl_search.is_empty());
        rig.app.apply(Action::ShowCrate(PLAYLIST), &ctx);
        rig.frame(Vec::new());
        assert!(
            !rig.app.crates.shown().is_filtered(),
            "not narrowed when shown again"
        );
    }

    #[test]
    fn next_and_p_follow_the_label_filter() {
        let mut rig = Rig::new(
            "label-next",
            vec![
                fixture("tone.flac"),
                fixture("tone.wav"),
                fixture("tone.ogg"),
            ],
            |_| {},
        );
        rig.until(
            |r| r.app.position.state == PlayState::Playing,
            "the first file plays",
        );
        let ids = rig.ids(PLAYLIST);
        set_records(
            &mut rig,
            &[
                ("Techno", "A", "Lowtide Tapes"),
                ("Techno", "B", "Other"),
                ("Techno", "C", "Lowtide Tapes"),
            ],
        );
        let ctx = rig.ctx.clone();
        rig.app.apply(
            Action::TogglePick(Facet::Label, "Lowtide Tapes".into()),
            &ctx,
        );
        rig.frame(Vec::new());
        assert_eq!(
            rig.app.queue,
            [ids[0], ids[2]],
            "the other label is left out"
        );
        // Only "Other": the playing entry is hidden, and P turns every filter off.
        rig.app.apply(Action::ClearPicks(None), &ctx);
        rig.app
            .apply(Action::TogglePick(Facet::Label, "Other".into()), &ctx);
        rig.app
            .apply(Action::TogglePick(Facet::Artist, "B".into()), &ctx);
        rig.frame(Vec::new());
        assert_eq!(rig.app.crates.shown().shown_rows(), [1]);
        rig.app.show_playing_entry();
        assert!(!rig.app.crates.shown().is_filtered());
    }

    #[test]
    fn next_follows_the_style_filter_and_p_clears_it() {
        let mut rig = Rig::new(
            "style-next",
            vec![
                fixture("tone.flac"),
                fixture("tone.wav"),
                fixture("tone.ogg"),
            ],
            |_| {},
        );
        rig.until(
            |r| r.app.position.state == PlayState::Playing,
            "the first file plays",
        );
        let ids = rig.ids(PLAYLIST);
        set_styles(&mut rig, &["Deep House", "Electro", "Deep House"]);
        let ctx = rig.ctx.clone();
        rig.app
            .apply(Action::TogglePick(Facet::Style, "Deep House".into()), &ctx);
        rig.frame(Vec::new());
        assert_eq!(rig.app.queue, [ids[0], ids[2]], "Electro is left out");
        // Electro only: the playing entry is hidden, and P shows it again.
        rig.app.apply(Action::ClearPicks(None), &ctx);
        rig.app
            .apply(Action::TogglePick(Facet::Style, "Electro".into()), &ctx);
        rig.frame(Vec::new());
        assert_eq!(rig.app.crates.shown().shown_rows(), [1]);
        rig.app.show_playing_entry();
        rig.frame(Vec::new());
        assert_eq!(rig.app.crates.shown().filter(Facet::Style), None);
    }

    #[test]
    fn next_and_a_hidden_playing_track_follow_the_bpm_filter() {
        let mut rig = Rig::new(
            "bpm-next",
            vec![
                fixture("tone.flac"),
                fixture("tone.wav"),
                fixture("tone.ogg"),
            ],
            |_| {},
        );
        rig.until(
            |r| r.app.position.state == PlayState::Playing,
            "the first file plays",
        );
        let ids = rig.ids(PLAYLIST);
        set_tempos(&mut rig, &[134, 124, 138]);
        let ctx = rig.ctx.clone();
        rig.app.apply(Action::SetBpmFilter(Some((130, 140))), &ctx);
        rig.frame(Vec::new());
        assert_eq!(rig.app.queue, [ids[0], ids[2]], "124 BPM is left out");
        rig.app.apply(Action::Next, &ctx);
        rig.until(
            |r| r.app.crates.playing().current() == Some(ids[2]),
            "next skips the hidden entry",
        );

        // A range that hides the playing entry: it plays on, and nothing else is left.
        rig.app.apply(Action::SetBpmFilter(Some((130, 135))), &ctx);
        rig.frame(Vec::new());
        assert_eq!(rig.app.position.state, PlayState::Playing);
        assert_eq!(rig.app.crates.playing().current(), Some(ids[2]));
        assert_eq!(
            rig.app.queue,
            [ids[0], ids[2]],
            "the playing entry stays queued"
        );
        // ALL: everything plays again.
        rig.app.apply(Action::SetBpmFilter(None), &ctx);
        rig.frame(Vec::new());
        assert_eq!(rig.app.queue, ids);
    }

    /// Entries 1 to 6 from releases 1, 1, 2, 1, (none) and 3; returns their ids.
    fn album_crate(rig: &mut Rig) -> Vec<EntryId> {
        let p = rig.app.crates.shown_mut();
        for (i, release) in [Some(1), Some(1), Some(2), Some(1), None, Some(3)]
            .into_iter()
            .enumerate()
        {
            let origin = Origin {
                release,
                album: release.map(|r| format!("Album {r}")).unwrap_or_default(),
                clip: Some(format!("clip{i:07}")),
                ..Default::default()
            };
            p.add_waiting(
                "Nightcraft",
                format!("Track {i}"),
                None,
                Some(origin),
                "queued",
            );
        }
        let ids = p.entries().iter().map(|e| e.id).collect();
        rig.frame(Vec::new());
        ids
    }

    #[test]
    fn right_clicking_an_entry_tints_the_rest_of_its_album() {
        let mut rig = Rig::new("menu-album-tint", Vec::new(), |_| {});
        let ids = album_crate(&mut rig);
        // Entries 3 to 5 selected; right-click entry 2 (outside the selection).
        rig.click(rig.row(2));
        rig.mods = Modifiers::SHIFT;
        rig.click(rig.row(4));
        rig.mods = Modifiers::NONE;
        rig.click_with(rig.row(1), PointerButton::Secondary);
        rig.frame(Vec::new());
        assert_eq!(rig.app.pl_tint, [ids[0], ids[3]]);
        assert_eq!(
            rig.app.crates.shown().selected_ids(),
            [ids[1]],
            "the selection is the clicked entry, as without albums"
        );
        // Closing the menu clears the tint.
        rig.frame(vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        rig.frame(Vec::new());
        assert!(rig.app.pl_tint.is_empty());
        // An entry without an album tints nothing.
        rig.click_with(rig.row(4), PointerButton::Secondary);
        rig.frame(Vec::new());
        assert!(rig.app.pl_tint.is_empty());
    }

    #[test]
    fn no_entry_menu_offers_render_show() {
        let mut rig = Rig::new("menu-no-render", Vec::new(), |_| {});
        album_crate(&mut rig);
        let out = rig.click_with(rig.row(3), PointerButton::Secondary);
        assert!(shows(&out, "Send to crate"), "the menu is open");
        assert!(
            !text_list(&out).iter().any(|t| t.contains("Render show")),
            "{:?}",
            text_list(&out)
        );
    }

    #[test]
    fn the_entry_menu_removes_or_selects_a_whole_album() {
        let mut rig = Rig::new("menu-album", Vec::new(), |_| {});
        let ids = album_crate(&mut rig);
        // Select album: exactly the album, the cursor on the clicked entry.
        rig.click_with(rig.row(3), PointerButton::Secondary);
        rig.click_text("Select album");
        let shown = rig.app.crates.shown();
        assert_eq!(shown.selected_ids(), [ids[0], ids[1], ids[3]]);
        assert_eq!(shown.cursor(), Some(ids[3]));

        // A single's items are there but disabled; an entry without an album has none.
        rig.click_with(rig.row(2), PointerButton::Secondary);
        let out = rig.frame(Vec::new());
        let on_screen: Vec<String> = texts(&out).into_iter().map(|t| t.text).collect();
        assert!(on_screen.iter().any(|t| t == "Remove album (1 track)"));
        rig.click_text("Remove album (1 track)");
        assert_eq!(rig.ids(PLAYLIST).len(), 6, "disabled: nothing removed");
        rig.frame(vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        rig.click_with(rig.row(4), PointerButton::Secondary);
        let out = rig.frame(Vec::new());
        assert!(
            !texts(&out).iter().any(|t| t.text == "Select album"),
            "no album, no album items"
        );
        rig.frame(vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);

        // Remove album: all of it, wherever it is; the rest keep their order.
        rig.click_with(rig.row(1), PointerButton::Secondary);
        rig.click_text("Remove album (3 tracks)");
        assert_eq!(rig.ids(PLAYLIST), [ids[2], ids[4], ids[5]]);
    }

    #[test]
    fn removing_the_playing_album_lets_its_track_finish() {
        let mut rig = Rig::new(
            "album-playing",
            vec![
                fixture("tone.flac"),
                fixture("tone.wav"),
                fixture("tone.ogg"),
            ],
            |_| {},
        );
        rig.until(
            |r| r.app.position.state == PlayState::Playing,
            "the first file plays",
        );
        let ids = rig.ids(PLAYLIST);
        for &id in &ids[..2] {
            let e = rig
                .app
                .crates
                .shown_mut()
                .entries_mut()
                .find(|e| e.id == id)
                .unwrap();
            (e.artist, e.album) = ("Tone".into(), "Tests".into());
        }
        let ctx = rig.ctx.clone();
        rig.app.apply(Action::RemoveAlbum(ids[1]), &ctx);
        assert_eq!(rig.ids(PLAYLIST), [ids[2]]);
        rig.frame(Vec::new());
        assert_eq!(
            rig.app.position.state,
            PlayState::Playing,
            "the removed track plays on"
        );
        rig.app.apply(Action::Next, &ctx);
        rig.until(
            |r| r.app.crates.playing().current() == Some(ids[2]),
            "next is the next remaining entry",
        );
    }

    #[test]
    fn remove_in_the_entry_menu_takes_the_selection_or_just_the_clicked_entry() {
        let mut rig = Rig::new("menu-remove", Vec::new(), |_| {});
        let ids = rig.fill_playlist(12);
        rig.frame(Vec::new());
        // Entries 3 to 6 selected; right-click entry 4: all four go.
        rig.click(rig.row(2));
        rig.mods = Modifiers::SHIFT;
        rig.click(rig.row(5));
        rig.mods = Modifiers::NONE;
        rig.click_with(rig.row(3), PointerButton::Secondary);
        rig.click_text("Remove");
        let left = |r: &Rig| -> Vec<EntryId> {
            r.app
                .crates
                .shown()
                .entries()
                .iter()
                .map(|e| e.id)
                .collect()
        };
        let mut expected: Vec<EntryId> = ids.clone();
        expected.drain(2..6);
        assert_eq!(left(&rig), expected);

        // Entries 1 and 2 selected; right-click entry 6 (outside): only it goes, and it was
        // selected first.
        rig.click(rig.row(0));
        rig.mods = Modifiers::SHIFT;
        rig.click(rig.row(1));
        rig.mods = Modifiers::NONE;
        rig.click_with(rig.row(5), PointerButton::Secondary);
        assert_eq!(rig.app.crates.shown().selected_ids(), [expected[5]]);
        rig.click_text("Remove");
        expected.remove(5);
        assert_eq!(left(&rig), expected);
    }

    #[test]
    fn arm_in_the_entry_menu_arms_a_waiting_entry() {
        let mut rig = Rig::new("menu-arm", Vec::new(), |_| {});
        let waiting = rig.app.crates.shown_mut().add_waiting(
            "Nightcraft",
            "Glasshouse",
            None,
            None,
            "downloading 40%",
        );
        rig.frame(Vec::new());
        rig.click_with(rig.row(0), PointerButton::Secondary);
        rig.click_text("Arm");
        assert_eq!(rig.app.armed, Some((PLAYLIST, waiting)));
    }

    #[test]
    fn send_to_crate_copies_the_selection_from_the_entry_menu() {
        let mut rig = Rig::new("send", Vec::new(), |_| {});
        rig.app.crates.shown_mut().add(
            ["tone.flac", "tone.wav", "tone.ogg"]
                .iter()
                .map(|f| TrackRef::new(fixture(f).to_string_lossy())),
        );
        let keepers = rig.crate_with("Keepers", &["tone.wav"]);
        rig.frame(Vec::new());
        // Select all three, then right-click one of them.
        rig.click(rig.row(0));
        rig.mods = Modifiers::SHIFT;
        rig.click(rig.row(2));
        rig.mods = Modifiers::NONE;
        assert_eq!(rig.app.crates.shown().selected_ids().len(), 3);
        let before = rig.app.crates.shown().to_saved();

        rig.click_with(rig.row(1), PointerButton::Secondary);
        rig.click_text("Send to crate");
        rig.click_text("Keepers");
        let files: Vec<String> = rig
            .app
            .crates
            .get(keepers)
            .unwrap()
            .entries()
            .iter()
            .map(|e| {
                Path::new(&e.track.0)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(
            files,
            ["tone.wav", "tone.flac", "tone.ogg"],
            "two appended, in order"
        );
        assert_eq!(
            rig.app.crates.shown().to_saved(),
            before,
            "the source is unchanged"
        );
        assert_eq!(
            rig.app.message.as_ref().map(|(m, _)| m.as_str()),
            Some("Sent 2 entries to Keepers (1 already there)")
        );

        // New crate… names a new crate for them.
        rig.click_with(rig.row(1), PointerButton::Secondary);
        rig.click_text("Send to crate");
        rig.click_text("New crate…");
        rig.type_text("Gig 12 Oct");
        let gig = rig.app.crates.list().last().unwrap().id;
        assert_eq!(rig.app.crates.name(gig), "Gig 12 Oct");
        assert_eq!(rig.app.crates.get(gig).unwrap().len(), 3);
        assert_eq!(
            rig.app.crates.shown_id(),
            PLAYLIST,
            "the source stays shown"
        );
    }
}
