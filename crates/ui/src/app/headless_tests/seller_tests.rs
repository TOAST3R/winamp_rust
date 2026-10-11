//! Top Sellers and the cart, headless: the first list from purchases, double-click to dig,
//! Narrow down, refreshes, copies, SOLD, CART and the cart filter, against a fake Discogs.

use super::*;
use ::dig::browser::FakeBrowser;
use ::dig::clock::RealClock;
use ::dig::cover::{FakeImages, test_jpeg};
use ::dig::discogs::seller::inventory_path;
use ::dig::discogs::transport::{FakeTransport, Method};
use ::dig::preview::fetcher::{FakeFetcher, Fetcher};
use ::dig::preview::scheduler::Finder;
use ::dig::sellers::SellerList;

#[derive(Clone)]
struct Fakes {
    transport: Arc<FakeTransport>,
    fetcher: Arc<FakeFetcher>,
    browser: Arc<FakeBrowser>,
}

impl Fakes {
    fn new() -> Self {
        Self {
            transport: Arc::new(FakeTransport::with_fixtures(format!(
                "{}/../dig/tests/fixtures/discogs",
                env!("CARGO_MANIFEST_DIR")
            ))),
            fetcher: Arc::new(FakeFetcher::new(fixture("tone.m4a"))),
            browser: Arc::new(FakeBrowser::default()),
        }
    }

    fn setup(&self, dir: &Path) -> DigSetup {
        let f = self.fetcher.clone();
        let finder: Finder = Arc::new(move |_| {
            let v = f.version()?;
            Some((f.clone() as Arc<dyn Fetcher>, v))
        });
        DigSetup {
            transport: self.transport.clone(),
            images: FakeImages::new(Ok(test_jpeg(300, 300))),
            clock: Arc::new(RealClock::default()),
            finder,
            browser: self.browser.clone(),
            cache_root: Some(dir.join("cache")),
            bridge: BridgeSetup::Off,
        }
    }

    fn requests(&self, part: &str) -> usize {
        self.transport
            .paths()
            .iter()
            .filter(|p| p.contains(part))
            .count()
    }

    /// The user bought from `sellers` (best first), each with copies for sale.
    fn purchases(&self, sellers: &[(&str, usize, usize)]) {
        let mut items = Vec::new();
        for (name, orders, _) in sellers {
            for n in 0..*orders {
                items.push(serde_json::json!({
                    "created": format!("2026-01-{:02}", n + 1),
                    "seller": {"id": 1, "username": name}
                }));
            }
        }
        self.transport.route(
            "/purchases?sort=created&sort_order=desc&page=1&per_page=100",
            200,
            serde_json::json!({ "items": items }).to_string(),
        );
        for (name, _, for_sale) in sellers {
            self.count(name, *for_sale);
        }
    }

    fn count(&self, seller: &str, n: usize) {
        self.transport.route(
            inventory_path(seller, "", 1, 1),
            200,
            serde_json::json!({"pagination": {"items": n, "pages": n}, "listings": []}).to_string(),
        );
    }

    /// `seller`'s stock for the search text `query`: copies (listing, release, price,
    /// format), newest first, in pages of 100, and its count.
    fn inventory(&self, seller: &str, query: &str, copies: &[(u64, u64, f64, &str)]) {
        let pages = copies.len().div_ceil(100).max(1);
        for p in 0..pages {
            let listings: Vec<_> = copies
                .iter()
                .skip(p * 100)
                .take(100)
                .map(|(id, release, price, format)| listing(*id, *release, *price, format))
                .collect();
            self.transport.route(
                inventory_path(seller, query, p as u32 + 1, 100),
                200,
                serde_json::json!({
                    "pagination": {"page": p + 1, "pages": pages, "per_page": 100, "items": copies.len()},
                    "listings": listings,
                })
                .to_string(),
            );
        }
        self.transport.route(
            inventory_path(seller, query, 1, 1),
            200,
            serde_json::json!({"pagination": {"items": copies.len(), "pages": copies.len()}, "listings": []})
                .to_string(),
        );
    }

    fn empty_cart(&self) {
        self.transport.route("/cart", 200, "[]");
    }
}

fn listing(id: u64, release: u64, price: f64, format: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id, "status": "For Sale",
        "price": {"value": price, "currency": "EUR"},
        "condition": "Very Good Plus (VG+)", "sleeve_condition": "Very Good (VG)",
        "ships_from": "Germany", "posted": "2026-02-06T08:12:00-08:00",
        "release": {"id": release, "artist": "Nightcraft", "title": "Glasshouse EP",
                    "format": format, "label": "Lowtide Tapes", "catalog_number": "LT-012",
                    "year": 1994, "thumbnail": ""}
    })
}

fn rig(name: &str, fakes: &Fakes, prepare: impl FnOnce(&Store)) -> Rig {
    let f = fakes.clone();
    Rig::with_dig(name, Vec::new(), prepare, move |dir| Some(f.setup(dir)))
}

fn with_token(store: &Store) {
    ::dig::config::save_token(store.dir(), Some("tok")).unwrap();
}

fn sellers(rig: &Rig) -> &SellerList {
    &rig.app.dig.as_ref().unwrap().sellers
}

fn names(rig: &Rig) -> Vec<String> {
    sellers(rig)
        .sellers
        .iter()
        .map(|s| s.username.clone())
        .collect()
}

#[test]
fn the_first_list_comes_from_purchases_once() {
    let fakes = Fakes::new();
    fakes.empty_cart();
    fakes.purchases(&[
        ("logon", 3, 2675),
        ("closed", 2, 0),
        ("decks.de", 1, 40_728),
    ]);
    let mut rig = rig("sellers-first", &fakes, with_token);
    rig.until(|r| sellers(r).seeded, "the first list arrives");
    assert_eq!(
        names(&rig),
        ["logon", "decks.de"],
        "closed shops are skipped"
    );
    let s = &sellers(&rig).sellers[0];
    assert_eq!((s.total, s.last_dug), (2675, None), "counted, never dug");
    assert_eq!(rig.app.crates.seller_of(s.crate_id), Some("logon"));
    assert_eq!(rig.app.crates.name(s.crate_id), "Seller: logon");
    assert_eq!(rig.app.crates.entry_count(s.crate_id), 0, "empty until dug");
    assert_eq!(
        SellerList::load(&rig.dir.join("config")),
        *sellers(&rig),
        "saved"
    );
    assert_eq!(fakes.requests("/purchases"), 1);
}

#[test]
fn a_seeded_list_is_never_filled_again() {
    let fakes = Fakes::new();
    fakes.empty_cart();
    fakes.purchases(&[("logon", 3, 2675)]);
    let mut rig = rig("sellers-seeded", &fakes, |store| {
        with_token(store);
        SellerList {
            seeded: true,
            ..SellerList::default()
        }
        .save(store.dir())
        .unwrap();
    });
    rig.until(
        |r| r.app.dig.as_ref().unwrap().cart.read_at > 0,
        "the cart is read",
    );
    assert!(names(&rig).is_empty(), "the user removed every seller");
    assert_eq!(fakes.requests("/purchases"), 0);
}

#[test]
fn without_a_token_nothing_is_asked() {
    let fakes = Fakes::new();
    fakes.purchases(&[("logon", 3, 2675)]);
    let mut rig = rig("sellers-no-token", &fakes, |_| {});
    for _ in 0..20 {
        rig.pump();
    }
    assert!(!sellers(&rig).seeded);
    assert_eq!(fakes.requests("/purchases") + fakes.requests("/cart"), 0);
    assert!(
        fakes
            .transport
            .log()
            .iter()
            .all(|(r, _)| r.method == Method::Get)
    );
}

/// Where `label` is drawn.
fn at(rig: &mut Rig, label: &str) -> Pos2 {
    let out = rig.frame(Vec::new());
    texts(&out)
        .into_iter()
        .find(|t| t.text == label)
        .unwrap_or_else(|| panic!("{label:?} is not on screen: {:?}", text_list(&out)))
        .rect
        .center()
}

/// A wide playlist with two sellers in the list: decks.de (dug) and logon (never dug).
fn two_sellers(name: &str, fakes: &Fakes) -> (Rig, CrateId, CrateId) {
    fakes.empty_cart();
    let mut rig = rig(name, fakes, |store| {
        with_token(store);
        SellerList {
            seeded: true,
            ..SellerList::default()
        }
        .save(store.dir())
        .unwrap();
    });
    let decks = rig.app.seller_crate("decks.de").unwrap();
    let logon = rig.app.seller_crate("logon").unwrap();
    let d = rig.app.dig.as_mut().unwrap();
    let mut s = ::dig::sellers::Seller::new("decks.de", decks, ::dig::sellers::Source::Purchases);
    s.last_dug = Some(::dig::now_secs());
    d.sellers.sellers.push(s);
    d.sellers.sellers.push(::dig::sellers::Seller::new(
        "logon",
        logon,
        ::dig::sellers::Source::Purchases,
    ));
    rig.app.settings.playlist_width = 700;
    rig.frame(Vec::new());
    (rig, decks, logon)
}

#[test]
fn labels_sit_between_the_collection_and_top_sellers_and_fold() {
    let fakes = Fakes::new();
    let (mut rig, _, _) = two_sellers("labels-sidebar", &fakes);
    let siesta = rig.app.crates.create("Label: Siesta Records").unwrap();
    rig.app.crates.set_label(siesta, 77);
    let out = rig.frame(Vec::new());
    let y = |t: &str| {
        texts(&out)
            .into_iter()
            .find(|x| x.text == t)
            .unwrap_or_else(|| panic!("{t}: {:?}", text_list(&out)))
            .rect
            .center()
            .y
    };
    assert!(y("DISCOGS") < y("⏷ LABELS (1)"));
    assert!(y("⏷ LABELS (1)") < y("Label: Siesta Records"));
    assert!(y("Label: Siesta Records") < y("⏷ TOP SELLERS (2)"));
    assert_eq!(
        texts(&out)
            .iter()
            .filter(|t| t.text == "Label: Siesta Records")
            .count(),
        1,
        "not also in the top list"
    );

    rig.click_text("⏷ LABELS (1)");
    let out = rig.frame(Vec::new());
    assert!(shows(&out, "⏵ LABELS (1)"));
    assert!(!shows(&out, "Label: Siesta Records"), "folded");
    assert!(rig.app.settings.labels_folded, "the fold is a setting");
}

#[test]
fn top_sellers_sit_under_discogs_and_fold() {
    let fakes = Fakes::new();
    let (mut rig, _, _) = two_sellers("sellers-sidebar", &fakes);
    let out = rig.frame(Vec::new());
    for t in [
        "DISCOGS",
        "⏷ TOP SELLERS (2)",
        "Seller: decks.de",
        "Seller: logon",
    ] {
        assert!(shows(&out, t), "{t}: {:?}", text_list(&out));
    }
    let y = |t: &str| {
        texts(&out)
            .into_iter()
            .find(|x| x.text == t)
            .unwrap()
            .rect
            .center()
            .y
    };
    assert!(y("DISCOGS") < y("⏷ TOP SELLERS (2)"));
    assert!(y("⏷ TOP SELLERS (2)") < y("Seller: decks.de"));
    assert!(
        y("Seller: decks.de") < y("Seller: logon"),
        "in the list's order"
    );
    assert!(y("+ New crate") < y("DISCOGS"));

    rig.click_text("⏷ TOP SELLERS (2)");
    let out = rig.frame(Vec::new());
    assert!(shows(&out, "⏵ TOP SELLERS (2)"));
    assert!(!shows(&out, "Seller: decks.de"), "folded");
    assert!(
        SellerList::load(&rig.dir.join("config")).folded,
        "the fold is remembered"
    );
}

#[test]
fn a_seller_menu_offers_refresh_narrow_and_remove() {
    let fakes = Fakes::new();
    let (mut rig, _, _) = two_sellers("sellers-menu", &fakes);
    let pos = at(&mut rig, "Seller: decks.de");
    rig.click_with(pos, PointerButton::Secondary);
    let out = rig.frame(Vec::new());
    for t in [
        "Refresh seller",
        "Narrow down…",
        "Rename crate…",
        "Remove seller…",
    ] {
        assert!(shows(&out, t), "{t}: {:?}", text_list(&out));
    }
    assert!(!shows(&out, "Delete crate…"));
    rig.key(Key::Escape, Modifiers::NONE);
    rig.frame(Vec::new());

    // Never dug: nothing to refresh or narrow yet.
    let pos = at(&mut rig, "Seller: logon");
    rig.click_with(pos, PointerButton::Secondary);
    let out = rig.frame(Vec::new());
    assert!(shows(&out, "Remove seller…"));
    assert!(!shows(&out, "Refresh seller") && !shows(&out, "Narrow down…"));
}

#[test]
fn a_click_shows_a_seller_crate_without_asking_discogs() {
    let fakes = Fakes::new();
    let (mut rig, _, logon) = two_sellers("sellers-click", &fakes);
    // What launch reads in the background (cart, lists, details) settles first.
    let mut quiet = 0;
    while quiet < 30 {
        let n = fakes.transport.count();
        rig.pump();
        quiet = if fakes.transport.count() == n {
            quiet + 1
        } else {
            0
        };
    }
    let before = fakes.transport.count();
    let pos = at(&mut rig, "Seller: logon");
    rig.click(pos);
    for _ in 0..10 {
        rig.pump();
    }
    assert_eq!(rig.app.crates.shown_id(), logon);
    assert_eq!(
        fakes.transport.count(),
        before,
        "a click never reaches Discogs"
    );
}

use super::sellers::{DigDialog, SellerDialog};

fn dig_dialog(rig: &Rig) -> Option<&DigDialog> {
    match &rig.app.dig.as_ref()?.seller_ui.dialog {
        Some(SellerDialog::Dig(d)) => Some(d),
        _ => None,
    }
}

fn dig_dialog_mut(rig: &mut Rig) -> &mut DigDialog {
    match &mut rig.app.dig.as_mut().unwrap().seller_ui.dialog {
        Some(SellerDialog::Dig(d)) => d,
        _ => panic!("no Dig dialog"),
    }
}

fn seller(rig: &Rig, name: &str) -> ::dig::sellers::Seller {
    sellers(rig).find(name).cloned().unwrap()
}

/// Three copies of release 1001 and one of 1002.
const SMALL: [(u64, u64, f64, &str); 4] = [
    (11, 1001, 9.0, "12\""),
    (12, 1002, 7.5, "LP"),
    (13, 1001, 18.0, "12\""),
    (14, 1001, 12.0, "12\""),
];

fn double_click_seller(rig: &mut Rig, name: &str) {
    let pos = at(rig, &format!("Seller: {name}"));
    rig.double_click(pos);
}

#[test]
fn a_first_double_click_counts_then_digs_every_copy() {
    let fakes = Fakes::new();
    let (mut rig, _, logon) = two_sellers("sellers-dig", &fakes);
    fakes.inventory("logon", "", &SMALL);
    double_click_seller(&mut rig, "logon");
    rig.until(
        |r| dig_dialog(r).is_some_and(|d| d.total() == Some(4)),
        "the copies are counted",
    );
    assert!(shows(&rig.frame(Vec::new()), "Dig: logon"));
    rig.click_text("Dig");
    rig.until(
        |r| {
            r.app
                .crates
                .get(logon)
                .is_some_and(|p| p.copies().len() == 4)
        },
        "every copy comes in",
    );
    rig.until(
        |r| {
            r.app.crates.get(logon).is_some_and(|p| {
                p.entries()
                    .iter()
                    .filter(|e| e.status.is_playable())
                    .count()
                    >= 3
            })
        },
        "the records' tracks come in",
    );
    let p = rig.app.crates.get(logon).unwrap();
    let prices: Vec<u64> = p.copies_of(1001).iter().map(|c| c.cents).collect();
    assert_eq!(prices, [900, 1200, 1800], "cheapest first");
    let releases: std::collections::BTreeSet<u64> = p
        .entries()
        .iter()
        .filter_map(|e| e.origin.as_ref()?.release)
        .collect();
    assert_eq!(
        releases.into_iter().collect::<Vec<_>>(),
        [1001, 1002],
        "each record once"
    );
    assert!(seller(&rig, "logon").last_dug.is_some());
    assert!(rig.app.crates.is_grouped(logon));
    assert_eq!(rig.app.crates.shown_id(), logon);
}

#[test]
fn a_double_click_refreshes_only_a_day_after() {
    let fakes = Fakes::new();
    let (mut rig, decks, _) = two_sellers("sellers-stale", &fakes);
    fakes.inventory("decks.de", "", &SMALL);
    let before = fakes.requests("/inventory");
    double_click_seller(&mut rig, "decks.de");
    for _ in 0..10 {
        rig.pump();
    }
    assert_eq!(
        fakes.requests("/inventory"),
        before,
        "dug 0 h ago: just shown"
    );
    assert!(dig_dialog(&rig).is_none());
    assert_eq!(rig.app.crates.shown_id(), decks);

    // Well after the first double-click, so this one isn't read as a triple click.
    for _ in 0..60 {
        rig.pump();
    }
    let d = rig.app.dig.as_mut().unwrap();
    d.sellers.find_mut("decks.de").unwrap().last_dug = Some(::dig::now_secs() - 30 * 3600);
    double_click_seller(&mut rig, "decks.de");
    rig.until(
        |r| {
            r.app
                .crates
                .get(decks)
                .is_some_and(|p| p.copies().len() == 4)
        },
        "a stale crate is refreshed",
    );
    assert!(dig_dialog(&rig).is_none(), "no dialog for a refresh");
    assert_eq!(message(&rig), "decks.de: 4 new");
}

fn message(rig: &Rig) -> String {
    rig.app
        .message
        .as_ref()
        .map(|(m, _)| m.clone())
        .unwrap_or_default()
}

#[test]
fn a_big_seller_asks_to_narrow_down() {
    let fakes = Fakes::new();
    let (mut rig, logon, _) = two_sellers("sellers-narrow", &fakes);
    let _ = logon;
    fakes.count("logon", 40_728);
    let techno: Vec<(u64, u64, f64, &str)> = (0..1200)
        .map(|n| (100 + n, 1001 + n % 2, 9.0, "12\""))
        .collect();
    fakes.inventory("logon", "techno", &techno);
    double_click_seller(&mut rig, "logon");
    rig.until(
        |r| dig_dialog(r).is_some_and(|d| d.total() == Some(40_728)),
        "counted",
    );
    let out = rig.frame(Vec::new());
    assert!(shows(&out, "Narrow down: logon"), "{:?}", text_list(&out));
    assert!(
        text_list(&out)
            .iter()
            .any(|t| t.starts_with("Discogs only shows the first 10,000")),
        "the 10,000 note"
    );
    assert!(!dig_dialog(&rig).unwrap().can_dig());

    // Search text, counted once typing pauses.
    let d = dig_dialog_mut(&mut rig);
    d.criteria.query = "techno".into();
    d.typed = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
    rig.until(
        |r| dig_dialog(r).is_some_and(|d| d.total() == Some(1200)),
        "searched",
    );
    assert!(
        !dig_dialog(&rig).unwrap().can_dig(),
        "1,200 is still too many"
    );

    // Newest 500: Discogs applies it, so Dig is offered at once.
    dig_dialog_mut(&mut rig).criteria.newest = Some(500);
    rig.frame(Vec::new());
    assert_eq!(dig_dialog(&rig).unwrap().matching(), Some((500, true)));
    assert!(dig_dialog(&rig).unwrap().can_dig());
}

#[test]
fn criteria_applied_here_read_the_listings_first() {
    let fakes = Fakes::new();
    let (mut rig, _, logon) = two_sellers("sellers-scan", &fakes);
    // 1,500 copies: 600 on vinyl, 900 on CD.
    let stock: Vec<(u64, u64, f64, &str)> = (0..1500)
        .map(|n| {
            (
                100 + n,
                1001 + n % 2,
                9.0,
                if n % 5 < 2 { "12\"" } else { "CD" },
            )
        })
        .collect();
    fakes.inventory("logon", "", &stock);
    double_click_seller(&mut rig, "logon");
    rig.until(
        |r| dig_dialog(r).is_some_and(|d| d.total() == Some(1500)),
        "counted",
    );
    dig_dialog_mut(&mut rig).criteria.formats = vec![::dig::discogs::model::Format::Vinyl];
    rig.until(
        |r| dig_dialog(r).is_some_and(|d| d.matching() == Some((600, true))),
        "the listings are read and counted",
    );
    assert!(dig_dialog(&rig).unwrap().can_dig());
    rig.click_text("Dig");
    rig.until(
        |r| {
            r.app
                .crates
                .get(logon)
                .is_some_and(|p| p.copies().len() == 600)
        },
        "only the vinyl copies come in",
    );
    assert_eq!(
        seller(&rig, "logon").criteria.formats,
        [::dig::discogs::model::Format::Vinyl],
        "the criteria are kept for refreshes"
    );
}

/// decks.de dug with SMALL; then its stock changes to `now`.
fn dug_then(name: &str, fakes: &Fakes, now: &[(u64, u64, f64, &str)]) -> (Rig, CrateId) {
    let (mut rig, decks, _) = two_sellers(name, fakes);
    rig.app
        .crates
        .get_mut(decks)
        .unwrap()
        .add_copies(SMALL.iter().map(|(l, r, p, _)| crate::playlist::SaleCopy {
            listing: *l,
            release: *r,
            cents: (*p * 100.0) as u64,
            currency: "EUR".into(),
            ..Default::default()
        }));
    fakes.inventory("decks.de", "", now);
    (rig, decks)
}

fn refresh(rig: &mut Rig) {
    let pos = at(rig, "Seller: decks.de");
    rig.click_with(pos, PointerButton::Secondary);
    rig.click_text("Refresh seller");
}

#[test]
fn a_refresh_says_what_changed_and_keeps_sold_copies() {
    let fakes = Fakes::new();
    // 12 sold, 13 cheaper, 15 new.
    let now = [
        (11, 1001, 9.0, "12\""),
        (13, 1001, 15.0, "12\""),
        (14, 1001, 12.0, "12\""),
        (15, 1002, 6.0, "LP"),
    ];
    let (mut rig, decks) = dug_then("sellers-refresh", &fakes, &now);
    refresh(&mut rig);
    rig.until(|r| message(r).starts_with("decks.de:"), "the refresh ends");
    assert_eq!(message(&rig), "decks.de: 1 new, 1 sold, 1 cheaper");
    let p = rig.app.crates.get(decks).unwrap();
    assert!(p.copy(12).unwrap().sold, "kept as sold");
    assert_eq!(p.copy(13).unwrap().was_cents, Some(1800));
    assert!(p.copy(15).is_some());
    assert_eq!(p.copies().len(), 5);
}

#[test]
fn a_refresh_past_the_limit_changes_nothing_and_asks() {
    let fakes = Fakes::new();
    let big: Vec<(u64, u64, f64, &str)> = (0..1120).map(|n| (100 + n, 1001, 9.0, "12\"")).collect();
    let (mut rig, decks) = dug_then("sellers-refresh-limit", &fakes, &big);
    refresh(&mut rig);
    rig.until(
        |r| dig_dialog(r).is_some_and(|d| d.from_refresh && d.total() == Some(1120)),
        "Narrow down opens",
    );
    assert_eq!(
        rig.app.crates.get(decks).unwrap().copies().len(),
        4,
        "unchanged"
    );
    assert!(shows(&rig.frame(Vec::new()), "Narrow down: decks.de"));
}

#[test]
fn a_failed_refresh_changes_nothing() {
    let fakes = Fakes::new();
    let (mut rig, decks) = dug_then("sellers-refresh-offline", &fakes, &SMALL[..1]);
    fakes
        .transport
        .route(inventory_path("decks.de", "", 1, 100), 500, "");
    refresh(&mut rig);
    rig.until(
        |r| message(r).contains("Discogs error"),
        "the refresh fails",
    );
    let p = rig.app.crates.get(decks).unwrap();
    assert_eq!(p.copies().len(), 4);
    assert!(p.copies().iter().all(|c| !c.sold), "nothing marked sold");
}

fn add_dialog(rig: &Rig) -> Option<&super::sellers::AddDialog> {
    match &rig.app.dig.as_ref()?.seller_ui.dialog {
        Some(SellerDialog::Add(d)) => Some(d),
        _ => None,
    }
}

#[test]
fn add_seller_looks_up_the_page_and_puts_it_first() {
    let fakes = Fakes::new();
    let (mut rig, _, _) = two_sellers("sellers-add", &fakes);
    fakes.transport.route(
        "/users/ADEPTA_STORE",
        200,
        r#"{"username": "ADEPTA_STORE"}"#,
    );
    fakes.inventory("ADEPTA_STORE", "", &SMALL);
    fakes.transport.route("/users/nobody-xyz", 404, "{}");
    let pos = at(&mut rig, "⏷ TOP SELLERS (2)");
    rig.click_with(pos, PointerButton::Secondary);
    rig.click_text("Add seller…");
    assert!(add_dialog(&rig).is_some());

    rig.frame(vec![egui::Event::Text("nobody-xyz".into())]);
    rig.until(
        |r| add_dialog(r).is_some_and(|d| matches!(d.found, Some((_, Err(_))))),
        "an unknown user is said so",
    );
    assert!(shows(
        &rig.frame(Vec::new()),
        "No Discogs user by that name"
    ));

    rig.app.dig.as_mut().unwrap().seller_ui.dialog =
        Some(SellerDialog::Add(super::sellers::AddDialog {
            text: "https://www.discogs.com/seller/ADEPTA_STORE/profile".into(),
            typed: Some(std::time::Instant::now() - std::time::Duration::from_secs(1)),
            found: None,
        }));
    rig.until(
        |r| add_dialog(r).is_some_and(|d| matches!(d.found, Some((_, Ok(_))))),
        "the seller is found",
    );
    assert!(shows(
        &rig.frame(Vec::new()),
        "Found: ADEPTA_STORE · 4 for sale"
    ));
    rig.click_text("Add");
    assert_eq!(names(&rig)[0], "ADEPTA_STORE", "first in the list");
    let id = seller(&rig, "ADEPTA_STORE").crate_id;
    assert_eq!(rig.app.crates.shown_id(), id);
    assert_eq!(
        seller(&rig, "ADEPTA_STORE").source,
        ::dig::sellers::Source::Added
    );
    assert!(
        dig_dialog(&rig).is_some_and(|d| d.total() == Some(4) && d.can_dig()),
        "the dig flow opens, counted"
    );
}

#[test]
fn a_pasted_seller_page_adds_or_refreshes() {
    let fakes = Fakes::new();
    let (mut rig, decks, _) = two_sellers("sellers-paste", &fakes);
    fakes
        .transport
        .route("/users/logon2", 200, r#"{"username": "logon2"}"#);
    fakes.count("logon2", 12);
    rig.app
        .dig_paste("https://www.discogs.com/seller/logon2/profile");
    rig.until(
        |r| add_dialog(r).is_some_and(|d| matches!(d.found, Some((_, Ok(_))))),
        "a new seller is looked up",
    );
    rig.app.seller_dialog_closed();

    // decks.de is in the list: it's shown and refreshed (dug long ago).
    rig.app
        .dig
        .as_mut()
        .unwrap()
        .sellers
        .find_mut("decks.de")
        .unwrap()
        .last_dug = Some(1);
    fakes.inventory("decks.de", "", &SMALL);
    rig.app.dig_paste("https://www.discogs.com/user/decks.de");
    assert_eq!(rig.app.crates.shown_id(), decks);
    rig.until(|r| message(r).starts_with("decks.de:"), "refreshed");
}

#[test]
fn remove_seller_asks_then_deletes_its_crate() {
    let fakes = Fakes::new();
    let (mut rig, _, logon) = two_sellers("sellers-remove", &fakes);
    let pos = at(&mut rig, "Seller: logon");
    rig.click_with(pos, PointerButton::Secondary);
    rig.click_text("Remove seller…");
    assert!(shows(
        &rig.frame(Vec::new()),
        "Remove logon from Top Sellers?"
    ));
    rig.click_text("Remove");
    assert_eq!(names(&rig), ["decks.de"]);
    assert!(rig.app.crates.info(logon).is_none(), "its crate is deleted");
    assert_eq!(SellerList::load(&rig.dir.join("config")).sellers.len(), 1);
}

/// logon dug with SMALL (three copies of 1001, one of 1002), shown and playable.
fn dug(name: &str, fakes: &Fakes) -> (Rig, CrateId) {
    let (mut rig, _, logon) = two_sellers(name, fakes);
    fakes.inventory("logon", "", &SMALL);
    double_click_seller(&mut rig, "logon");
    rig.until(|r| dig_dialog(r).is_some_and(DigDialog::can_dig), "counted");
    rig.click_text("Dig");
    rig.until(
        |r| {
            r.app.crates.get(logon).is_some_and(|p| {
                p.copies().len() == 4 && p.entries().iter().all(|e| e.status.is_playable())
            })
        },
        "dug",
    );
    rig.app.settings.playlist_width = 700;
    // Past egui's double-click window, so the next click isn't counted with Dig's.
    for _ in 0..30 {
        rig.pump();
    }
    (rig, logon)
}

fn open_record(rig: &mut Rig, release: u64) {
    let ctx = rig.ctx.clone();
    rig.app
        .apply(Action::ToggleRecord(AlbumKey::Release(release)), &ctx);
    rig.frame(Vec::new());
}

#[test]
fn an_open_record_shows_its_copies_cheapest_first() {
    let fakes = Fakes::new();
    let (mut rig, _) = dug("sellers-copies", &fakes);
    let out = rig.frame(Vec::new());
    let line = text_list(&out)
        .into_iter()
        .find(|t| t.contains("copies €"))
        .unwrap_or_else(|| panic!("{:?}", text_list(&out)));
    assert!(line.ends_with("3 copies €9.00–€18.00"), "{line}");
    open_record(&mut rig, 1001);
    let out = rig.frame(Vec::new());
    let ys: Vec<f32> = [
        "€9.00 · VG+ / VG · Germany",
        "€12.00 · VG+ / VG · Germany",
        "€18.00 · VG+ / VG · Germany",
    ]
    .iter()
    .map(|t| {
        texts(&out)
            .into_iter()
            .find(|x| x.text == *t)
            .unwrap_or_else(|| panic!("{t}: {:?}", text_list(&out)))
            .rect
            .center()
            .y
    })
    .collect();
    assert!(ys[0] < ys[1] && ys[1] < ys[2], "cheapest first");

    // A double-click opens the listing; nothing plays.
    let before = rig.app.position.state;
    let pos = at(&mut rig, "€12.00 · VG+ / VG · Germany");
    rig.double_click(pos);
    assert_eq!(
        fakes
            .browser
            .opened
            .lock()
            .unwrap()
            .last()
            .map(String::as_str),
        Some("https://www.discogs.com/sell/item/14")
    );
    assert_eq!(rig.app.position.state, before, "a copy holds no audio");
}

#[test]
fn the_cursor_steps_over_copy_rows() {
    let fakes = Fakes::new();
    let (mut rig, logon) = dug("sellers-copies-keys", &fakes);
    open_record(&mut rig, 1001);
    let rows = rig.app.pl_list();
    let record_row = rows
        .iter()
        .position(|r| matches!(r, ListRow::Record(rec) if rec.key == AlbumKey::Release(1001)))
        .unwrap();
    assert!(matches!(rows[record_row + 1], ListRow::Copy { .. }));
    rig.app.focus = Focus::Playlist;
    let p = rig.app.crates.get(logon).unwrap();
    let first = p.entries()[rows[record_row].first()].id;
    rig.app.crates.shown_mut().select_only(&[first], first);
    rig.app.move_row_cursor(CursorMove::Down, false, None);
    let cursor = rig.app.crates.shown().cursor().unwrap();
    let at = records::row_of(
        &rig.app.pl_list(),
        rig.app.crates.shown().index_of(cursor).unwrap(),
    );
    assert!(
        matches!(rig.app.pl_list()[at], ListRow::Entry { track: true, .. }),
        "down from the record goes to its first track, past its copies"
    );
}

#[test]
fn sold_copies_and_records_are_marked_and_still_play() {
    let fakes = Fakes::new();
    let (mut rig, logon) = dug("sellers-sold", &fakes);
    let p = rig.app.crates.get_mut(logon).unwrap();
    assert!(p.mark_sold(12), "release 1002's only copy");
    assert!(p.mark_sold(13));
    open_record(&mut rig, 1001);
    let out = rig.frame(Vec::new());
    let sold = texts(&out).iter().filter(|t| t.text == "SOLD").count();
    assert_eq!(
        sold,
        2,
        "the sold copy of 1001, and record 1002: {:?}",
        text_list(&out)
    );
    let rows = rig.app.pl_list();
    let copies: Vec<u64> = rows
        .iter()
        .filter_map(|r| match r {
            ListRow::Copy { listing, .. } => Some(*listing),
            _ => None,
        })
        .collect();
    assert_eq!(copies, [11, 14, 13], "sold last");
    let p = rig.app.crates.get(logon).unwrap();
    assert!(
        p.entries()
            .iter()
            .filter(|e| e.origin.as_ref().and_then(|o| o.release) == Some(1002))
            .all(|e| e.status.is_playable()),
        "a sold record's tracks still play"
    );
    assert!(text_list(&out).iter().any(|t| t.ends_with("· sold")));
}

fn cart_with(items: &[(u64, u64, &str, &str)]) -> ::dig::discogs::cart::CartSnapshot {
    let mut snap = ::dig::discogs::cart::CartSnapshot {
        read_at: 1,
        ..Default::default()
    };
    for (listing, release, seller, price) in items {
        snap.apply(
            ::dig::discogs::cart::CartItem {
                listing: *listing,
                release: *release,
                seller: (*seller).into(),
                price: (*price).into(),
            },
            true,
        );
    }
    snap
}

#[test]
fn a_cached_cart_marks_entries_in_every_crate_at_once() {
    let fakes = Fakes::new();
    fakes.transport.route("/cart", 503, "");
    let mut rig = rig("cart-cached", &fakes, |store| {
        with_token(store);
        let cache = store.dir().parent().unwrap().join("cache");
        cart_with(&[(11, 1001, "Housevinyl.nl", "€14.00")])
            .save(&cache)
            .unwrap();
    });
    let origin = |r: u64| crate::playlist::Origin {
        release: Some(r),
        clip: Some(format!("clip{r}")),
        ..Default::default()
    };
    let p = rig.app.crates.shown_mut();
    p.add_waiting(
        "Nightcraft",
        "Glasshouse",
        None,
        Some(origin(1001)),
        "listed",
    );
    p.add_waiting("Nightcraft", "Other", None, Some(origin(1002)), "listed");
    let out = rig.frame(Vec::new());
    let carts = texts(&out).iter().filter(|t| t.text == "CART").count();
    assert_eq!(carts, 1, "only release 1001's entry: {:?}", text_list(&out));
    let e = rig.app.crates.shown().entries()[0].clone();
    assert_eq!(
        rig.app.dig_marks(&e).cart.as_deref(),
        Some("from Housevinyl.nl, €14.00")
    );
}

#[test]
fn a_copy_put_in_the_cart_on_discogs_shows_after_the_next_read() {
    let fakes = Fakes::new();
    let (mut rig, _) = dug("cart-elsewhere", &fakes);
    open_record(&mut rig, 1001);
    let out = rig.frame(Vec::new());
    assert!(!shows(&out, "CART") && !shows(&out, "IN CART"));
    fakes.transport.route(
        "/cart",
        200,
        r#"[{"seller": {"username": "logon"}, "subtotal": {"formatted": "€9.00"},
            "cart_items": [{"item_id": 11, "price": {"formatted": "€9.00"}, "release": {"id": 1001}}]}]"#,
    );
    rig.app
        .dig
        .as_ref()
        .unwrap()
        .send_cmd(::dig::intake::Command::ReadCart { soon: true });
    rig.until(
        |r| r.app.dig.as_ref().unwrap().cart.has_listing(11),
        "the cart is read",
    );
    let out = rig.frame(Vec::new());
    assert_eq!(pill_of(&out, "€9.00 · VG+ / VG · Germany"), "IN CART");
    let carts = texts(&out).iter().filter(|t| t.text == "CART").count();
    assert!(
        carts >= 1,
        "the record and its tracks: {:?}",
        text_list(&out)
    );
}

fn cart_answer(fakes: &Fakes, errors: &str) {
    fakes.transport.route_for(
        Method::Post,
        "/cart/items",
        201,
        format!(r#"{{"message": "Cart merge complete", "errors": [{errors}]}}"#),
    );
}

fn posted(fakes: &Fakes) -> Vec<String> {
    fakes
        .transport
        .log()
        .into_iter()
        .filter(|(r, _)| r.method == Method::Post)
        .filter_map(|(r, _)| r.body)
        .collect()
}

#[test]
fn add_to_cart_marks_the_copy_at_once_and_says_so() {
    let fakes = Fakes::new();
    let (mut rig, _) = dug("cart-add", &fakes);
    cart_answer(&fakes, "");
    // After the add, Discogs' cart holds the copy.
    fakes.transport.route(
        "/cart",
        200,
        r#"[{"seller": {"username": "logon"}, "subtotal": {"formatted": "€12.00"},
            "cart_items": [{"item_id": 14, "price": {"formatted": "€12.00"}, "release": {"id": 1001}}]}]"#,
    );
    open_record(&mut rig, 1001);
    let pill = pill_at(&mut rig, "€12.00 · VG+ / VG · Germany");
    // Discogs takes half a second to answer the add.
    fakes
        .transport
        .fault(::dig::discogs::transport::Fault::Delay(
            std::time::Duration::from_millis(500),
        ));
    let selected = rig.app.crates.shown().selected_ids();
    let out = rig.click(pill);
    assert!(
        rig.app.dig.as_ref().unwrap().cart.has_listing(14),
        "in the cart within the frame, before Discogs answers"
    );
    assert_eq!(pill_of(&out, "€12.00 · VG+ / VG · Germany"), "REMOVE");
    assert_eq!(
        rig.app.crates.shown().selected_ids(),
        selected,
        "the pill's click doesn't select the row"
    );
    rig.until(|r| message(r) == "Added to your cart", "Discogs answers");
    assert_eq!(posted(&fakes), [r#"{"item_ids":[14]}"#]);
    rig.until(
        |r| r.app.dig.as_ref().unwrap().cart.read_at > 1,
        "the cart is read after the add",
    );
    assert!(
        rig.app.dig.as_ref().unwrap().cart.has_listing(14),
        "and keeps it"
    );
}

/// The pill drawn on the row whose text is `row` (its label).
fn pill_of(out: &egui::FullOutput, row: &str) -> String {
    let all = texts(out);
    let y = all
        .iter()
        .find(|t| t.text == row)
        .unwrap_or_else(|| panic!("{row:?}: {:?}", text_list(out)))
        .rect
        .center()
        .y;
    all.into_iter()
        .filter(|t| ["+ CART", "IN CART", "REMOVE"].contains(&t.text.as_str()))
        .find(|t| (t.rect.center().y - y).abs() < 3.0)
        .map(|t| t.text)
        .unwrap_or_default()
}

/// Where the pill of the row whose text is `row` is.
fn pill_at(rig: &mut Rig, row: &str) -> Pos2 {
    let out = rig.frame(Vec::new());
    let all = texts(&out);
    let y = all
        .iter()
        .find(|t| t.text == row)
        .unwrap_or_else(|| panic!("{row:?}: {:?}", text_list(&out)))
        .rect
        .center()
        .y;
    all.into_iter()
        .filter(|t| ["+ CART", "IN CART", "REMOVE"].contains(&t.text.as_str()))
        .find(|t| (t.rect.center().y - y).abs() < 3.0)
        .unwrap_or_else(|| panic!("no pill on {row:?}: {:?}", text_list(&out)))
        .rect
        .center()
}

fn pills(out: &egui::FullOutput) -> usize {
    texts(out)
        .iter()
        .filter(|t| ["+ CART", "IN CART", "REMOVE"].contains(&t.text.as_str()))
        .count()
}

#[test]
fn no_menu_offers_the_cart_or_render_show() {
    let fakes = Fakes::new();
    let (mut rig, logon) = dug("cart-menu", &fakes);
    rig.app.dig.as_mut().unwrap().cart = cart_with(&[(11, 1001, "logon", "€9.00")]);
    open_record(&mut rig, 1001);
    let title = rig
        .app
        .crates
        .get(logon)
        .unwrap()
        .entries()
        .iter()
        .find(|e| e.origin.as_ref().and_then(|o| o.release) == Some(1001))
        .unwrap()
        .title
        .clone();
    let record = text_list(&rig.frame(Vec::new()))
        .into_iter()
        .find(|t| t.contains("Nightcraft") || t.contains(" – "))
        .unwrap();
    for row in [
        "€9.00 · VG+ / VG · Germany",
        "€12.00 · VG+ / VG · Germany",
        &title,
        &record,
    ] {
        let pos = at(&mut rig, row);
        let out = rig.click_with(pos, PointerButton::Secondary);
        let items = text_list(&out);
        assert!(
            !items.iter().any(|t| t.contains("Add to cart")
                || t.contains("Remove from cart")
                || t.contains("Render show")),
            "{row}: {items:?}"
        );
        if row.starts_with('€') {
            assert!(shows(&out, "Open on discogs.com"), "{row}: {items:?}");
        }
        rig.frame(vec![egui::Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        rig.frame(Vec::new());
    }
}

#[test]
fn a_record_with_one_copy_left_carries_its_pill() {
    let fakes = Fakes::new();
    let (mut rig, _) = dug("cart-one-copy", &fakes);
    cart_answer(&fakes, "");
    // Release 1002 has one copy (12); release 1001 three, the €9.00 one in the cart.
    rig.app.dig.as_mut().unwrap().cart = cart_with(&[(11, 1001, "logon", "€9.00")]);
    let out = rig.frame(Vec::new());
    assert_eq!(
        pills(&out),
        1,
        "only 1002's record row: {:?}",
        text_list(&out)
    );
    assert!(shows(&out, "+ CART"));
    assert!(shows(&out, "CART"), "1001's record row keeps the badge");
    let pos = texts(&out)
        .into_iter()
        .find(|t| t.text == "+ CART")
        .unwrap()
        .rect
        .center();
    let opened = rig.app.pl_open_rev;
    rig.click(pos);
    assert_eq!(rig.app.pl_open_rev, opened, "the record didn't open");
    rig.until(|r| message(r) == "Added to your cart", "Discogs answers");
    assert_eq!(posted(&fakes), [r#"{"item_ids":[12]}"#]);
}

#[test]
fn copy_pills_line_up_and_a_sold_copy_has_none() {
    let fakes = Fakes::new();
    let (mut rig, logon) = dug("cart-pills", &fakes);
    rig.app.dig.as_mut().unwrap().cart = cart_with(&[(11, 1001, "logon", "€9.00")]);
    assert!(rig.app.crates.get_mut(logon).unwrap().mark_sold(13));
    open_record(&mut rig, 1001);
    let out = rig.frame(Vec::new());
    assert_eq!(pill_of(&out, "€9.00 · VG+ / VG · Germany"), "IN CART");
    assert_eq!(pill_of(&out, "€12.00 · VG+ / VG · Germany"), "+ CART");
    assert_eq!(
        pill_of(&out, "€18.00 · VG+ / VG · Germany"),
        "",
        "sold: SOLD only"
    );
    let left = |t: &str| {
        texts(&out)
            .into_iter()
            .find(|x| x.text == t)
            .unwrap()
            .rect
            .left()
    };
    assert!(
        (left("€9.00 · VG+ / VG · Germany") - left("€12.00 · VG+ / VG · Germany")).abs() < 0.5,
        "IN CART and + CART are as wide"
    );
}

#[test]
fn a_double_click_on_the_pill_never_opens_the_listing() {
    let fakes = Fakes::new();
    let (mut rig, _) = dug("cart-double", &fakes);
    cart_answer(&fakes, "");
    fakes.empty_cart();
    open_record(&mut rig, 1001);
    let pill = pill_at(&mut rig, "€12.00 · VG+ / VG · Germany");
    rig.double_click(pill);
    assert!(
        fakes.browser.opened.lock().unwrap().is_empty(),
        "no listing opened"
    );
    assert!(
        !rig.app.dig.as_ref().unwrap().cart.has_listing(14),
        "added and taken out again"
    );
}

#[test]
fn without_a_token_the_pill_asks_to_connect() {
    let fakes = Fakes::new();
    let (mut rig, _) = dug("cart-pill-no-token", &fakes);
    rig.app.dig.as_mut().unwrap().token = None;
    open_record(&mut rig, 1001);
    let pill = pill_at(&mut rig, "€12.00 · VG+ / VG · Germany");
    rig.click(pill);
    assert!(rig.app.dig.as_ref().unwrap().connect.is_some());
    assert!(posted(&fakes).is_empty());
}

#[test]
fn a_copy_sold_meanwhile_is_marked_sold() {
    let fakes = Fakes::new();
    let (mut rig, logon) = dug("cart-sold", &fakes);
    cart_answer(
        &fakes,
        r#"{"item_id": 13, "status": 404, "message": "This item is not for sale."}"#,
    );
    fakes.empty_cart();
    let ctx = rig.ctx.clone();
    rig.app.apply(
        Action::Dig(DigAction::Seller(super::sellers::SellerAction::CartAdd(
            vec![13],
        ))),
        &ctx,
    );
    rig.until(|r| message(r) == "no longer for sale", "Discogs answers");
    assert!(rig.app.crates.get(logon).unwrap().copy(13).unwrap().sold);
    assert!(!rig.app.dig.as_ref().unwrap().cart.has_listing(13));
}

#[test]
fn when_the_cart_fails_the_listing_opens_instead() {
    let fakes = Fakes::new();
    let (mut rig, _) = dug("cart-fails", &fakes);
    fakes
        .transport
        .route_for(Method::Post, "/cart/items", 500, "");
    let ctx = rig.ctx.clone();
    rig.app.apply(
        Action::Dig(DigAction::Seller(super::sellers::SellerAction::CartAdd(
            vec![11],
        ))),
        &ctx,
    );
    rig.until(
        |r| message(r).contains("opened on discogs.com instead"),
        "it fails",
    );
    assert_eq!(
        fakes
            .browser
            .opened
            .lock()
            .unwrap()
            .last()
            .map(String::as_str),
        Some("https://www.discogs.com/sell/item/11")
    );
    assert!(
        !rig.app.dig.as_ref().unwrap().cart.has_listing(11),
        "no CART badge"
    );
}

#[test]
fn remove_from_cart_takes_one_copy_out() {
    let fakes = Fakes::new();
    let (mut rig, _) = dug("cart-remove", &fakes);
    rig.app.dig.as_mut().unwrap().cart = cart_with(&[(11, 1001, "logon", "€9.00")]);
    fakes.empty_cart();
    open_record(&mut rig, 1001);
    let pill = pill_at(&mut rig, "€9.00 · VG+ / VG · Germany");
    assert_eq!(
        pill_of(&rig.frame(Vec::new()), "€9.00 · VG+ / VG · Germany"),
        "IN CART"
    );
    let out = rig.frame(vec![egui::Event::PointerMoved(pill)]);
    assert_eq!(pill_of(&out, "€9.00 · VG+ / VG · Germany"), "REMOVE");
    let out = rig.click(pill);
    assert!(!rig.app.dig.as_ref().unwrap().cart.has_listing(11));
    assert_eq!(pill_of(&out, "€9.00 · VG+ / VG · Germany"), "+ CART");
    rig.until(
        |_| {
            fakes
                .transport
                .log()
                .iter()
                .any(|(q, _)| q.method == Method::Delete)
        },
        "the removal is sent",
    );
    let deletes: Vec<String> = fakes
        .transport
        .log()
        .into_iter()
        .filter(|(r, _)| r.method == Method::Delete)
        .map(|(r, _)| r.path)
        .collect();
    assert_eq!(deletes, ["/cart/item/11"]);
}

#[test]
fn without_a_token_the_cart_asks_to_connect() {
    let fakes = Fakes::new();
    let mut rig = rig("cart-no-token", &fakes, |_| {});
    let ctx = rig.ctx.clone();
    rig.app.apply(
        Action::Dig(DigAction::Seller(super::sellers::SellerAction::CartAdd(
            vec![11],
        ))),
        &ctx,
    );
    assert!(rig.app.dig.as_ref().unwrap().connect.is_some());
    assert!(
        fakes
            .transport
            .log()
            .iter()
            .all(|(r, _)| r.method == Method::Get)
    );
}

#[test]
fn the_cart_switch_shows_and_plays_only_whats_in_the_cart() {
    let fakes = Fakes::new();
    let (mut rig, logon) = dug("cart-switch", &fakes);
    // Wide enough for the switch in the filter bar.
    rig.app.settings.playlist_width = 500;
    rig.frame(Vec::new());
    assert_eq!(rig.app.cart_drawn, None, "nothing in the cart yet");
    let mut snap = cart_with(&[(11, 1001, "logon", "€9.00")]);
    snap.sellers.get_mut("logon").unwrap().subtotal = "€9.00".into();
    rig.app.dig.as_mut().unwrap().cart = snap;
    rig.frame(Vec::new());
    let (label, pos) = rig.app.cart_drawn.clone().expect("the switch is drawn");
    assert_eq!(label, "CART 1 · €9.00");
    rig.click(pos);
    let p = rig.app.crates.get(logon).unwrap();
    assert!(p.cart_only());
    let shown: std::collections::BTreeSet<u64> = p
        .shown_rows()
        .into_iter()
        .filter_map(|i| p.entries()[i].origin.as_ref()?.release)
        .collect();
    assert_eq!(
        shown.into_iter().collect::<Vec<_>>(),
        [1001],
        "only the record in the cart"
    );

    // ☰ offers the cart's page.
    rig.click(footer_button(&rig, "pl_menu"));
    rig.click_text("Open cart on discogs.com");
    assert_eq!(
        fakes
            .browser
            .opened
            .lock()
            .unwrap()
            .last()
            .map(String::as_str),
        Some("https://www.discogs.com/sell/cart")
    );
    // The bar's × turns the switch off.
    rig.click(bar_clear(&rig));
    assert!(!rig.app.crates.get(logon).unwrap().cart_only());

    // At the classic width, beside a BPM control, the switch is in the panel FILTERS opens.
    for (i, e) in rig.app.crates.shown_mut().entries_mut().enumerate() {
        e.bpm = Some(120 + i as u16);
    }
    rig.app.settings.playlist_width = 275;
    rig.frame(Vec::new());
    assert_eq!(rig.app.cart_drawn, None, "no room in the bar");
    let (x, _) = rig.app.filters_drawn.expect("FILTERS is drawn");
    rig.click(pos2(PL_LEFT + x + 4.0, BAR_Y));
    rig.click_text("CART 1 · €9.00");
    assert!(rig.app.crates.get(logon).unwrap().cart_only());
}

#[test]
fn a_seller_sent_from_the_browser_is_added_and_the_app_comes_forward() {
    let fakes = Fakes::new();
    let (mut rig, _, _) = two_sellers("sellers-bridge", &fakes);
    fakes
        .transport
        .route("/users/logon2", 200, r#"{"username": "logon2"}"#);
    fakes.count("logon2", 12);
    rig.app
        .dig_bridge_command(::dig::bridge::BridgeCommand::Send {
            page: ::dig::discogs::url::parse("https://www.discogs.com/seller/logon2/profile")
                .unwrap(),
            mode: ::dig::bridge::Mode::Enqueue,
            filters: ::dig::jobs::Filters::default(),
        });
    let out = rig.frame(Vec::new());
    let commands: Vec<&ViewportCommand> = out
        .viewport_output
        .values()
        .flat_map(|v| &v.commands)
        .collect();
    assert!(
        commands.iter().any(|c| matches!(c, ViewportCommand::Focus)),
        "{commands:?}"
    );
    rig.until(
        |r| add_dialog(r).is_some_and(|d| matches!(d.found, Some((_, Ok(_))))),
        "the seller is looked up for Add",
    );
    assert_eq!(
        rig.app
            .crates
            .get(rig.app.crates.shown_id())
            .map(|p| p.len()),
        Some(0)
    );
}

#[test]
fn a_pill_is_as_wide_whatever_it_reads() {
    let rig = rig("pill-width", &Fakes::new(), |_| {});
    let colors = rig.app.def.colors.clone();
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        let painter = ui.painter();
        let font = egui::FontId::proportional(11.0);
        let width = |in_cart: bool, pointer: Option<Pos2>| {
            let marks = Badges {
                pill: Some(Pill { in_cart, pointer }),
                ..Default::default()
            };
            let (w, rect) =
                entry_badges(painter, pos2(10.0, 10.0), marks, None, &font, &colors, 1.0);
            assert!(rect.is_some());
            w
        };
        let over = Some(pos2(14.0, 10.0));
        let plus = width(false, None);
        assert_eq!(width(true, None), plus, "IN CART");
        assert_eq!(width(true, over), plus, "REMOVE");
        assert_eq!(width(false, over), plus, "+ CART hovered");
    });
    out.textures_delta.clear();
}

#[test]
fn the_flat_view_keeps_the_cart_badge() {
    let fakes = Fakes::new();
    let (mut rig, _) = dug("cart-flat", &fakes);
    rig.app.dig.as_mut().unwrap().cart = cart_with(&[(11, 1001, "logon", "€9.00")]);
    let ctx = rig.ctx.clone();
    rig.app.apply(Action::ToggleGrouped, &ctx);
    assert!(!rig.app.crates.shown().is_grouped());
    let out = rig.frame(Vec::new());
    assert_eq!(pills(&out), 0, "{:?}", text_list(&out));
    assert!(shows(&out, "CART"), "{:?}", text_list(&out));
}
