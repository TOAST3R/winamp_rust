//! Friends, headless: the list from Discogs, a crate per friend, double-click to dig their
//! collection, private collections, Refresh friend and friends who leave, against a fake
//! Discogs.

use super::*;
use ::dig::browser::FakeBrowser;
use ::dig::clock::RealClock;
use ::dig::cover::{FakeImages, test_jpeg};
use ::dig::discogs::client::ApiError;
use ::dig::discogs::transport::FakeTransport;
use ::dig::friends::FriendList;
use ::dig::preview::fetcher::{FakeFetcher, Fetcher};
use ::dig::preview::scheduler::Finder;

#[derive(Clone)]
struct Fakes {
    transport: Arc<FakeTransport>,
    fetcher: Arc<FakeFetcher>,
}

impl Fakes {
    fn new() -> Self {
        let f = Self {
            transport: Arc::new(FakeTransport::with_fixtures(format!(
                "{}/../dig/tests/fixtures/discogs",
                env!("CARGO_MANIFEST_DIR")
            ))),
            fetcher: Arc::new(FakeFetcher::new(fixture("tone.m4a"))),
        };
        f.transport.route("/cart", 200, "[]");
        f.transport.route(
            "/purchases?sort=created&sort_order=desc&page=1&per_page=100",
            200,
            r#"{"items": []}"#,
        );
        f
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
            browser: Arc::new(FakeBrowser::default()),
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

    /// The user's friends, in Discogs' order.
    fn friends(&self, names: &[&str]) {
        let list: Vec<_> = names
            .iter()
            .map(|u| serde_json::json!({"user": {"username": u}}))
            .collect();
        self.transport.route(
            "/users/digger/friends?page=1&per_page=100",
            200,
            serde_json::json!({"pagination": {"page": 1, "pages": 1}, "friends": list}).to_string(),
        );
    }

    /// `user`'s collection: these releases (fixtures 1001 to 1006), newest first.
    fn collection(&self, user: &str, releases: &[u64]) {
        let items: Vec<_> = releases
            .iter()
            .enumerate()
            .map(|(i, r)| {
                serde_json::json!({"id": r, "instance_id": i + 1,
                                   "basic_information": {"id": r, "master_id": 0}})
            })
            .collect();
        self.transport.route(
            format!(
                "/users/{user}/collection/folders/0/releases?sort=added&sort_order=desc&page=1&per_page=100"
            ),
            200,
            serde_json::json!({"pagination": {"page": 1, "pages": 1, "items": items.len()},
                               "releases": items})
            .to_string(),
        );
    }
}

fn rig(name: &str, fakes: &Fakes, tracks: Vec<PathBuf>, prepare: impl FnOnce(&Store)) -> Rig {
    let f = fakes.clone();
    let mut rig = Rig::with_dig(name, tracks, prepare, move |dir| Some(f.setup(dir)));
    // Wide enough for the sidebar.
    rig.app.settings.playlist_width = 700;
    rig
}

fn with_token(store: &Store) {
    ::dig::config::save_token(store.dir(), Some("tok")).unwrap();
}

fn friends(rig: &Rig) -> &FriendList {
    &rig.app.dig.as_ref().unwrap().friends
}

fn crate_of(rig: &Rig, user: &str) -> CrateId {
    friends(rig).find(user).expect("a friend").crate_id
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

fn message(rig: &Rig) -> String {
    rig.app
        .message
        .as_ref()
        .map(|(t, _)| t.clone())
        .unwrap_or_default()
}

fn releases_in(rig: &Rig, c: CrateId) -> Vec<u64> {
    let mut r: Vec<u64> = rig
        .app
        .crates
        .get(c)
        .map(|p| {
            p.entries()
                .iter()
                .filter_map(|e| e.origin.as_ref()?.release)
                .collect()
        })
        .unwrap_or_default();
    r.sort_unstable();
    r.dedup();
    r
}

/// Half a second of the rig's time, so the next clicks aren't counted with the last ones.
fn apart(rig: &mut Rig) {
    for _ in 0..60 {
        rig.pump();
    }
}

fn two_friends(name: &str, fakes: &Fakes) -> Rig {
    fakes.friends(&["javimaxilo", "Waxport"]);
    let mut rig = rig(name, fakes, Vec::new(), with_token);
    rig.until(
        |r| friends(r).friends.len() == 2,
        "the friends list arrives",
    );
    rig
}

#[test]
fn friends_come_from_discogs_once_a_week_and_leavers_keep_their_crate() {
    let fakes = Fakes::new();
    fakes.transport.route(
        "/users/Waxport/collection/folders/0/releases?page=1&per_page=1",
        403,
        "{}",
    );
    let mut rig = two_friends("friends-list", &fakes);
    assert_eq!(fakes.requests("/friends"), 1);
    // Each collection is checked: a private one is dimmed before anyone digs it.
    assert!(friends(&rig).find("Waxport").unwrap().private);
    assert!(!friends(&rig).find("javimaxilo").unwrap().private);
    let javi = crate_of(&rig, "javimaxilo");
    assert_eq!(rig.app.crates.name(javi), "Friend: javimaxilo");
    assert!(rig.app.crates.is_grouped(javi));
    assert_eq!(
        FriendList::load(&rig.dir.join("config")),
        *friends(&rig),
        "saved"
    );
    // Under FRIENDS in the sidebar, after the Discogs heading.
    assert!(at(&mut rig, "⏷ FRIENDS (2)").y > at(&mut rig, "DISCOGS").y);
    at(&mut rig, "Friend: javimaxilo");

    // A week later someone left and someone came: the leaver's crate stays, as an
    // ordinary crate.
    rig.app.dig_friends(Ok(vec![
        ("newfriend".into(), false),
        ("Waxport".into(), false),
    ]));
    let names: Vec<_> = friends(&rig)
        .friends
        .iter()
        .map(|f| f.username.clone())
        .collect();
    assert_eq!(names, ["newfriend", "Waxport"]);
    assert!(rig.app.crates.info(javi).is_some(), "the crate stays");
    assert_eq!(rig.app.crates.name(javi), "Friend: javimaxilo");

    // A failed read keeps the list, and the heading says why.
    rig.app.dig_friends(Err(ApiError::NotFound));
    assert_eq!(friends(&rig).friends.len(), 2);
    assert!(rig.app.friends_tip().contains("Couldn't read it again"));
}

#[test]
fn a_list_read_this_week_is_not_read_again() {
    let fakes = Fakes::new();
    fakes.friends(&["javimaxilo"]);
    let mut rig = rig("friends-fresh", &fakes, Vec::new(), |store| {
        with_token(store);
        FriendList {
            read_at: Some(::dig::now_secs() - 3 * 24 * 3600),
            ..FriendList::default()
        }
        .save(store.dir())
        .unwrap();
    });
    rig.until(
        |r| r.app.dig.as_ref().unwrap().cart.read_at > 0,
        "the account's other lists are read",
    );
    for _ in 0..20 {
        rig.pump();
    }
    assert_eq!(fakes.requests("/friends"), 0);
}

#[test]
fn a_double_click_digs_a_friend_and_a_private_one_says_so() {
    let fakes = Fakes::new();
    fakes.collection("javimaxilo", &[1001]);
    fakes
        .transport
        .route(
            "/users/Waxport/collection/folders/0/releases?sort=added&sort_order=desc&page=1&per_page=100",
            403,
            "{}",
        );
    let mut rig = two_friends("friends-dig", &fakes);
    let javi = crate_of(&rig, "javimaxilo");
    // A click shows it and asks nothing.
    let pos = at(&mut rig, "Friend: javimaxilo");
    rig.click(pos);
    assert_eq!(rig.app.crates.shown_id(), javi);
    assert_eq!(
        rig.app.friend_hint(javi).unwrap()[0],
        "DOUBLE-CLICK TO DIG",
        "the empty crate says how to fill it"
    );
    assert_eq!(
        fakes.requests("/users/javimaxilo/collection/folders/0/releases?sort"),
        0
    );
    rig.double_click(pos);
    rig.until(
        |r| releases_in(r, javi) == [1001],
        "the collection comes in",
    );
    assert!(friends(&rig).find("javimaxilo").unwrap().dug_at.is_some());
    // Dug: another double-click only shows it.
    let asked = fakes.requests("/users/javimaxilo/collection/folders/0/releases?sort");
    apart(&mut rig);
    rig.double_click(pos);
    rig.frame(Vec::new());
    assert_eq!(
        fakes.requests("/users/javimaxilo/collection/folders/0/releases?sort"),
        asked
    );

    // Private: dimmed from then on, and never asked again.
    // Past egui's double-click time, so the next clicks start a new pair.
    apart(&mut rig);
    let pos = at(&mut rig, "Friend: Waxport");
    rig.double_click(pos);
    rig.until(
        |r| friends(r).find("Waxport").unwrap().private,
        "the 403 marks it private",
    );
    assert_eq!(message(&rig), "Waxport's collection is private");
    let asked = fakes.requests("/users/Waxport/collection/folders/0/releases?sort");
    apart(&mut rig);
    rig.double_click(pos);
    rig.frame(Vec::new());
    assert_eq!(
        fakes.requests("/users/Waxport/collection/folders/0/releases?sort"),
        asked
    );
    // The next weekly read finds it public again.
    rig.app.dig_friends(Ok(vec![
        ("javimaxilo".into(), false),
        ("Waxport".into(), false),
    ]));
    assert!(!friends(&rig).find("Waxport").unwrap().private);
}

#[test]
fn refresh_friend_follows_their_collection_and_stop_keeps_what_came() {
    let fakes = Fakes::new();
    fakes.collection("javimaxilo", &[1001, 1002]);
    let mut rig = two_friends("friends-refresh", &fakes);
    let javi = crate_of(&rig, "javimaxilo");
    let ctx = rig.ctx.clone();
    rig.app.apply(
        Action::Dig(DigAction::Friend(super::friends::FriendAction::Dig(javi))),
        &ctx,
    );
    rig.until(
        |r| releases_in(r, javi) == [1001, 1002] && !r.app.dig.as_ref().unwrap().has_job_for(javi),
        "dug",
    );
    // They sold 1001 and bought 1003.
    fakes.collection("javimaxilo", &[1003, 1002]);
    rig.app.apply(
        Action::Dig(DigAction::Friend(super::friends::FriendAction::Refresh(
            javi,
        ))),
        &ctx,
    );
    rig.until(
        |r| message(r) == "javimaxilo: 1 new, 1 gone",
        "the refresh says so",
    );
    rig.until(
        |r| releases_in(r, javi) == [1002, 1003] && !r.app.dig.as_ref().unwrap().has_job_for(javi),
        "the crate follows",
    );

    // A refresh stopped while reading changes nothing.
    fakes.collection("javimaxilo", &[1004]);
    for _ in 0..10 {
        fakes
            .transport
            .fault(::dig::discogs::transport::Fault::Delay(
                Duration::from_millis(250),
            ));
    }
    rig.app.apply(
        Action::Dig(DigAction::Friend(super::friends::FriendAction::Refresh(
            javi,
        ))),
        &ctx,
    );
    assert!(shows(&rig.frame(Vec::new()), "Stop"));
    rig.app.dig_stop_refresh();
    assert_eq!(message(&rig), "Refresh Friend: javimaxilo stopped");
    std::thread::sleep(Duration::from_millis(800));
    rig.frame(Vec::new());
    assert_eq!(releases_in(&rig, javi), [1002, 1003]);
}

#[test]
fn digging_a_friend_never_interrupts_playback() {
    let fakes = Fakes::new();
    fakes.collection("javimaxilo", &[1001, 1002]);
    fakes.friends(&["javimaxilo"]);
    let mut rig = rig(
        "friends-play",
        &fakes,
        vec![fixture("tone.flac")],
        with_token,
    );
    rig.until(|r| r.app.position.state == PlayState::Playing, "playing");
    rig.until(
        |r| !friends(r).friends.is_empty(),
        "the friends list arrives",
    );
    let javi = crate_of(&rig, "javimaxilo");
    let ctx = rig.ctx.clone();
    rig.app.apply(
        Action::Dig(DigAction::Friend(super::friends::FriendAction::Dig(javi))),
        &ctx,
    );
    rig.until(|r| releases_in(r, javi) == [1001, 1002], "dug");
    assert_eq!(rig.app.position.state, PlayState::Playing);
    assert_eq!(rig.engine().stats().underruns, 0);
}
