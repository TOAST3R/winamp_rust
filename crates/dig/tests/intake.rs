//! The intake worker, step by step against recorded Discogs JSON: listings before details,
//! focus first, resuming after a restart, offline and back, and wantlist changes.

use std::sync::Arc;

use dig::clock::FakeClock;
use dig::discogs::cache::DiskCache;
use dig::discogs::client::{ApiError, Client};
use dig::discogs::matching::{Planned, TrackEntry};
use dig::discogs::model::RecordKey::{self, Master, Release};
use dig::discogs::transport::{FakeTransport, Fault, Method};
use dig::discogs::url::parse;
use dig::intake::{Command, Event, Intake, Outcome};
use dig::jobs::Filters;

const CRATE: u64 = 7;

fn fixtures() -> String {
    format!("{}/tests/fixtures/discogs", env!("CARGO_MANIFEST_DIR"))
}

fn temp(name: &str) -> platform::testing::TestDir {
    platform::testing::TestDir::new(&format!("dig-intake-{name}"))
}

fn intake(t: &Arc<FakeTransport>, token: bool, config: Option<std::path::PathBuf>) -> Intake {
    let client = Client::new(
        t.clone(),
        Arc::new(FakeClock::default()),
        token.then(|| "tok".to_owned()),
        DiskCache::default(),
    );
    Intake::new(client, config).with_now(|| 1_790_000_000)
}

fn send(i: &mut Intake, url: &str) {
    i.handle(Command::Send {
        page: parse(url).unwrap(),
        target: CRATE,
        filters: Filters::default(),
    });
}

/// Steps until idle (or `max` steps), returning every event.
fn run(i: &mut Intake, max: usize) -> Vec<Event> {
    let mut all = Vec::new();
    for _ in 0..max {
        let worked = i.step();
        all.extend(i.take_events());
        if !worked {
            break;
        }
    }
    all
}

fn records(events: &[Event]) -> Vec<(RecordKey, Outcome)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Record(_, info, o) => Some((info.key, o.clone())),
            _ => None,
        })
        .collect()
}

fn listed(events: &[Event]) -> Vec<RecordKey> {
    events
        .iter()
        .flat_map(|e| match e {
            Event::Listed(_, items) => items.iter().map(|l| l.key).collect(),
            _ => Vec::new(),
        })
        .collect()
}

fn clips(o: &Outcome) -> Vec<String> {
    match o {
        Outcome::Entries(p) => p
            .items
            .iter()
            .filter_map(|p| match p {
                Planned::Clip(c) => Some(c.clip.clone()),
                Planned::Search(_) => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn searches(o: &Outcome) -> Vec<TrackEntry> {
    match o {
        Outcome::Entries(p) => p
            .items
            .iter()
            .filter_map(|p| match p {
                Planned::Search(t) => Some(t.clone()),
                Planned::Clip(_) => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[test]
fn a_label_lists_every_record_before_any_details_then_expands_them() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    let mut i = intake(&t, true, None);
    send(&mut i, "https://www.discogs.com/label/12345-Lowtide-Tapes");
    let ev = run(&mut i, 100);
    assert!(matches!(&ev[0], Event::Started(j) if j.name == "Label: Lowtide Tapes"));
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::Named(j) if j.name == "Label: Lowtide Tapes"))
    );
    // Every format: the CD-only releases are listed too.
    assert_eq!(
        listed(&ev),
        [
            Release(1001),
            Release(1002),
            Release(1003),
            Release(1004),
            Release(1005),
            Release(1006)
        ]
    );
    let first_record = ev
        .iter()
        .position(|e| matches!(e, Event::Record(..)))
        .unwrap();
    let last_listed = ev
        .iter()
        .rposition(|e| matches!(e, Event::Listed(..)))
        .unwrap();
    assert!(last_listed < first_record, "all listings come first");

    let r = records(&ev);
    let keys: Vec<RecordKey> = r.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        keys,
        [
            Release(1001),
            Release(1002),
            Release(1003),
            Release(1004),
            Release(1005),
            Release(1006)
        ]
    );
    assert_eq!(
        clips(&r[0].1),
        ["GLASShouse1", "LUMENremix1", "LASTlight01"]
    );
    // No clip, but a tracklist: its tracks, to be searched for.
    let t = searches(&r[2].1);
    assert!(clips(&r[2].1).is_empty());
    assert_eq!(t.len(), 1);
    assert_eq!(
        (
            t[0].artist.as_str(),
            t[0].title.as_str(),
            t[0].position.as_str()
        ),
        ("Nightcraft", "Silent Tide", "A1")
    );
    assert_eq!(t[0].search_key, "track/nightcraft/silent tide");
    // The CD releases bring their clips too.
    assert_eq!(clips(&r[1].1), ["CDglasshou1"]);
    assert_eq!(clips(&r[4].1), ["UNKNOWNfmt1"]);
    assert!(matches!(ev.last(), Some(Event::Finished(_))));
    let progress = ev.iter().rev().find_map(|e| match e {
        Event::Progress(_, d, t) => Some((*d, *t)),
        _ => None,
    });
    assert_eq!(progress, Some((6, 6)));
    assert!(i.jobs().is_empty());
    // For-sale numbers in the account's currency.
    let info = ev.iter().find_map(|e| match e {
        Event::Record(_, info, _) if info.key == Release(1001) => Some(info.clone()),
        _ => None,
    });
    let info = info.unwrap();
    let fs = info.for_sale.unwrap();
    assert_eq!(
        (fs.count, fs.lowest, fs.currency.as_str()),
        (6, Some(9.0), "EUR")
    );
    // The album's name and its primary image's thumbnail, from the release; the listing's
    // own thumbnail comes earlier, with the listed record.
    assert_eq!(info.title, "Glasshouse EP");
    assert_eq!(
        info.cover,
        "https://i.discogs.com/fake/R-1001-front-150.jpeg"
    );
    let listed_cover = ev.iter().find_map(|e| match e {
        Event::Listed(_, items) => items.iter().find(|l| l.key == Release(1001)).cloned(),
        _ => None,
    });
    assert_eq!(
        listed_cover.unwrap().cover,
        "https://i.discogs.com/fake/R-1001-thumb.jpeg"
    );
}

#[test]
fn a_shop_item_expands_exactly_like_its_release() {
    let run_one = |url: &str| {
        let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
        let mut i = intake(&t, true, None);
        send(&mut i, url);
        let ev = run(&mut i, 100);
        let named: Vec<String> = ev
            .iter()
            .filter_map(|e| match e {
                Event::Named(j) => Some(j.name.clone()),
                _ => None,
            })
            .collect();
        (named, listed(&ev), records(&ev).len())
    };
    let item = run_one("https://www.discogs.com/shop/item/3923678974");
    let release = run_one("https://www.discogs.com/release/1001");
    assert_eq!(item, release);
    assert_eq!(item.0, ["Release: Nightcraft – Glasshouse EP"]);
}

#[test]
fn the_collection_sync_waits_for_the_digs_to_finish() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    t.route(
        "/users/digger/collection/folders/0/releases?sort=added&sort_order=desc&page=1&per_page=100",
        200,
        r#"{"pagination": {"page": 1, "pages": 1, "items": 1},
            "releases": [{"id": 1003, "instance_id": 7,
                          "basic_information": {"id": 1003, "master_id": 0, "year": 2016,
                                                "labels": [{"catno": "LT-003"}]}}]}"#,
    );
    let mut i = intake(&t, true, None);
    send(&mut i, "https://www.discogs.com/label/12345");
    i.handle(Command::SyncCollection(None));
    i.handle(Command::ResolveShopItem(3923678974));
    let ev = run(&mut i, 200);
    let paths = t.paths();
    let last_dig = paths
        .iter()
        .rposition(|p| p.starts_with("/labels/") || p.starts_with("/releases/"))
        .unwrap();
    let item = paths
        .iter()
        .position(|p| p.starts_with("/marketplace/"))
        .unwrap();
    let sync = paths
        .iter()
        .position(|p| p.contains("/collection/"))
        .unwrap();
    assert!(
        last_dig < item && item < sync,
        "digs, then the item, then the collection: {paths:?}"
    );
    assert!(ev.contains(&Event::ShopItem(3923678974, Some(1001))));
    let c = ev
        .iter()
        .find_map(|e| match e {
            Event::Collection(Ok(c)) => Some(c.clone()),
            _ => None,
        })
        .expect("synced");
    assert_eq!(c.len(), 1);
    assert_eq!(c.username, "digger");
}

#[test]
fn without_a_token_the_collection_is_never_asked_for() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    let mut i = intake(&t, false, None);
    i.handle(Command::SyncCollection(None));
    let ev = run(&mut i, 10);
    assert!(ev.contains(&Event::Collection(Err(ApiError::TokenNeeded))));
    assert!(!t.paths().iter().any(|p| p.contains("/collection/")));
}

#[test]
fn details_follow_the_focus() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    let mut i = intake(&t, true, None);
    send(&mut i, "https://www.discogs.com/label/12345");
    // Name and three listing pages.
    let mut ev = run(&mut i, 4);
    assert_eq!(listed(&ev).len(), 6);
    assert!(records(&ev).is_empty());
    // The user is on the fourth listed record: from there on, wrapping around.
    i.handle(Command::Focus {
        target: CRATE,
        order: vec![
            Release(1005),
            Release(1006),
            Release(1001),
            Release(1003),
            Release(1004),
        ],
    });
    ev.extend(run(&mut i, 2));
    // Then moves back to the first.
    i.handle(Command::Focus {
        target: CRATE,
        order: vec![Release(1001), Release(1003), Release(1004)],
    });
    ev.extend(run(&mut i, 100));
    let keys: Vec<RecordKey> = records(&ev).iter().map(|(k, _)| *k).collect();
    assert_eq!(
        keys,
        [
            Release(1005),
            Release(1006),
            Release(1001),
            Release(1003),
            Release(1004),
            Release(1002)
        ]
    );
}

#[test]
fn a_half_done_job_resumes_after_a_restart_without_duplicates() {
    let config = temp("resume");
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    let mut first = intake(&t, true, Some(config.to_path_buf()));
    send(&mut first, "https://www.discogs.com/label/12345");
    // Name, 3 listing pages, 2 records; then quit.
    let before = run(&mut first, 6);
    assert_eq!(records(&before).len(), 2);
    first.flush();
    drop(first);
    let requests = t.count();

    let mut second = intake(&t, true, Some(config.to_path_buf()));
    assert_eq!(second.jobs().len(), 1);
    assert!(
        run(&mut second, 10).is_empty(),
        "nothing runs until its crate is shown"
    );
    assert_eq!(t.count(), requests, "and nothing is requested");
    second.handle(Command::CrateActive(CRATE));
    let after = run(&mut second, 100);
    assert!(listed(&after).is_empty(), "the listing isn't fetched again");
    let mut keys: Vec<RecordKey> = records(&before)
        .iter()
        .chain(records(&after).iter())
        .map(|(k, _)| *k)
        .collect();
    assert_eq!(keys.len(), 6);
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), 6, "each record once");
    assert!(matches!(after.last(), Some(Event::Finished(_))));
    assert!(
        dig::jobs::Jobs::load(&config).jobs.is_empty(),
        "the finished job is forgotten"
    );
}

#[test]
fn offline_pauses_and_resumes_where_it_stopped() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    let mut i = intake(&t, true, None);
    send(&mut i, "https://www.discogs.com/label/12345");
    let mut ev = run(&mut i, 5); // name, 3 pages, the first record
    assert_eq!(records(&ev).len(), 1);
    t.set_offline(true);
    ev.extend(run(&mut i, 3));
    assert!(i.is_offline());
    assert_eq!(
        ev.iter()
            .filter(|e| matches!(e, Event::Offline(true)))
            .count(),
        1,
        "said once"
    );
    assert_eq!(records(&ev).len(), 1, "nothing is lost or skipped");
    t.set_offline(false);
    ev.extend(run(&mut i, 100));
    assert!(ev.iter().any(|e| matches!(e, Event::Offline(false))));
    let keys: Vec<RecordKey> = records(&ev).iter().map(|(k, _)| *k).collect();
    assert_eq!(
        keys,
        [
            Release(1001),
            Release(1002),
            Release(1003),
            Release(1004),
            Release(1005),
            Release(1006)
        ]
    );
}

#[test]
fn a_missing_page_fails_the_send() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    let mut i = intake(&t, false, None);
    send(&mut i, "https://www.discogs.com/label/999");
    let ev = run(&mut i, 10);
    assert!(matches!(
        ev.last(),
        Some(Event::Failed(_, ApiError::NotFound))
    ));
    assert!(listed(&ev).is_empty());
    assert!(i.jobs().is_empty());
}

#[test]
fn artists_masters_and_remix_credits() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    let mut i = intake(&t, true, None);
    send(&mut i, "https://www.discogs.com/artist/4242-Nightcraft");
    let r = records(&run(&mut i, 100));
    let keys: Vec<RecordKey> = r.iter().map(|(k, _)| *k).collect();
    assert_eq!(keys, [Master(98765), Release(2001), Release(1004)]);
    assert_eq!(clips(&r[0].1), ["GLASShouse1", "TIDALpull01"]);
    assert_eq!(
        clips(&r[1].1),
        ["CURRENTSnc1"],
        "only the clip naming the artist"
    );
}

#[test]
fn a_rejected_token_carries_on_without_one() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    t.route("/oauth/identity", 401, "{}");
    let mut i = intake(&t, true, None);
    send(&mut i, "https://discogs.com/release/1001");
    let ev = run(&mut i, 10);
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::Identity(Err(ApiError::TokenRejected))))
    );
    assert_eq!(records(&ev).len(), 1);
    let last = t.log().last().unwrap().clone();
    assert!(last.0.token.is_none(), "later requests go without it");
}

fn wantlist_result(ev: &[Event]) -> Result<bool, ApiError> {
    ev.iter()
        .find_map(|e| match e {
            Event::Wantlist { result, .. } => Some(result.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn want_adds_to_the_wantlist_unless_it_is_there_already() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    let mut i = intake(&t, true, None);
    i.handle(Command::Want(1004));
    assert_eq!(
        wantlist_result(&i.take_events()),
        Ok(false),
        "already on it"
    );
    i.handle(Command::Want(1001));
    assert_eq!(wantlist_result(&i.take_events()), Ok(true));
    i.handle(Command::Unwant(1001));
    assert_eq!(wantlist_result(&i.take_events()), Ok(true));
    let changes: Vec<(Method, String)> = t
        .log()
        .into_iter()
        .filter(|(r, _)| r.method != Method::Get)
        .map(|(r, _)| (r.method, r.path))
        .collect();
    assert_eq!(
        changes,
        [
            (Method::Put, "/users/digger/wants/1001".to_owned()),
            (Method::Delete, "/users/digger/wants/1001".to_owned())
        ]
    );
    let reads = t.paths().iter().filter(|p| p.contains("/wants?")).count();
    assert_eq!(reads, 1, "the wantlist is read once per session");

    t.fault(Fault::Network);
    i.handle(Command::Want(1003));
    assert_eq!(wantlist_result(&i.take_events()), Err(ApiError::Offline));

    let mut anon = intake(&t, false, None);
    anon.handle(Command::Want(1001));
    assert_eq!(
        wantlist_result(&anon.take_events()),
        Err(ApiError::TokenNeeded)
    );
}

fn collected(ev: &[Event]) -> Result<(u64, dig::collection::Pressing), ApiError> {
    ev.iter()
        .find_map(|e| match e {
            Event::Collected { result, .. } => Some(result.clone()),
            _ => None,
        })
        .unwrap()
}

fn changes(t: &FakeTransport) -> Vec<(Method, String)> {
    t.log()
        .into_iter()
        .filter(|(r, _)| r.method != Method::Get)
        .map(|(r, _)| (r.method, r.path))
        .collect()
}

#[test]
fn collect_adds_one_copy_to_uncategorized_with_its_pressing() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    let dir = temp("collect");
    let client = Client::new(
        t.clone(),
        Arc::new(FakeClock::default()),
        Some("tok".to_owned()),
        DiskCache::new(Some(dir.path())),
    );
    let mut i = Intake::new(client, None).with_now(|| 1_790_000_000);
    // The release is cached, as it is once its entries exist.
    send(&mut i, "https://www.discogs.com/release/1001");
    run(&mut i, 20);
    let before = t.count();
    i.handle(Command::Collect {
        release: 1001,
        check: false,
    });
    let (instance, pressing) = collected(&i.take_events()).unwrap();
    assert_eq!(instance, 900_001);
    assert_eq!(pressing.catno, "LT-012");
    assert_eq!(
        changes(&t),
        [(
            Method::Post,
            "/users/digger/collection/folders/1/releases/1001".to_owned()
        )]
    );
    assert_eq!(
        t.count(),
        before + 1,
        "one request, the release data from the cache"
    );

    let mut anon = intake(&t, false, None);
    anon.handle(Command::Collect {
        release: 1001,
        check: false,
    });
    assert_eq!(collected(&anon.take_events()), Err(ApiError::TokenNeeded));
    t.set_offline(true);
    i.handle(Command::Collect {
        release: 1002,
        check: false,
    });
    assert_eq!(collected(&i.take_events()), Err(ApiError::Offline));
}

#[test]
fn a_checked_retry_never_adds_a_second_copy() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    // The first add went through although its answer was lost.
    t.route(
        "/users/digger/collection/releases/1001",
        200,
        r#"{"releases": [{"id": 1001, "instance_id": 41, "folder_id": 1},
                         {"id": 1001, "instance_id": 77, "folder_id": 1}]}"#,
    );
    t.route(
        "/users/digger/collection/releases/1002",
        200,
        r#"{"releases": []}"#,
    );
    let mut i = intake(&t, true, None);
    i.handle(Command::Collect {
        release: 1001,
        check: true,
    });
    assert_eq!(
        collected(&i.take_events()).unwrap().0,
        77,
        "the newest copy"
    );
    assert!(changes(&t).is_empty(), "nothing added");
    i.handle(Command::Collect {
        release: 1002,
        check: true,
    });
    assert!(collected(&i.take_events()).is_ok());
    assert_eq!(
        changes(&t),
        [(
            Method::Post,
            "/users/digger/collection/folders/1/releases/1002".to_owned()
        )]
    );
}

fn discarded(ev: &[Event]) -> Result<dig::intake::Discarded, ApiError> {
    ev.iter()
        .find_map(|e| match e {
            Event::Discarded { result, .. } => Some(result.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn discard_removes_the_copy_added_last_from_its_folder() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    // Two copies: 41 added in 2021 (folder 1), 77 in 2024 (folder 3).
    t.route(
        "/users/digger/collection/releases/1001",
        200,
        r#"{"releases": [
            {"id": 1001, "instance_id": 77, "folder_id": 3, "date_added": "2024-03-01T10:00:00-08:00"},
            {"id": 1001, "instance_id": 41, "folder_id": 1, "date_added": "2021-06-01T10:00:00-07:00"}]}"#,
    );
    let mut i = intake(&t, true, None);
    i.handle(Command::Discard(1001));
    let d = discarded(&i.take_events()).unwrap();
    assert_eq!((d.instance, d.remaining), (Some(77), 1));
    assert_eq!(
        changes(&t),
        [(
            Method::Delete,
            "/users/digger/collection/folders/3/releases/1001/instances/77".to_owned()
        )]
    );
}

#[test]
fn discarding_a_copy_already_gone_sends_no_removal() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    t.route(
        "/users/digger/collection/releases/1001",
        200,
        r#"{"releases": []}"#,
    );
    let mut i = intake(&t, true, None);
    i.handle(Command::Discard(1001));
    let d = discarded(&i.take_events()).unwrap();
    assert_eq!((d.instance, d.remaining), (None, 0));
    // Not in the collection at all: Discogs answers 404, which is the same.
    i.handle(Command::Discard(1002));
    assert_eq!(discarded(&i.take_events()).unwrap().instance, None);
    assert!(changes(&t).is_empty(), "nothing removed");
    // The copy went between the look-up and the removal: done too.
    t.route(
        "/users/digger/collection/releases/1003",
        200,
        r#"{"releases": [{"id": 1003, "instance_id": 5, "folder_id": 1, "date_added": "2020"}]}"#,
    );
    t.route(
        "/users/digger/collection/folders/1/releases/1003/instances/5",
        404,
        "{}",
    );
    i.handle(Command::Discard(1003));
    let d = discarded(&i.take_events()).unwrap();
    assert_eq!((d.instance, d.remaining), (Some(5), 0));
}

#[test]
fn a_failed_discard_says_why() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    t.route(
        "/users/digger/collection/releases/1001",
        200,
        r#"{"releases": [{"id": 1001, "instance_id": 41, "folder_id": 1, "date_added": "2021"}]}"#,
    );
    let mut i = intake(&t, true, None);
    i.handle(Command::Identify);
    i.take_events();
    t.fault(Fault::Status(503));
    i.handle(Command::Discard(1001));
    assert!(matches!(
        discarded(&i.take_events()),
        Err(ApiError::Other(_))
    ));
    t.set_offline(true);
    i.handle(Command::Discard(1001));
    assert_eq!(discarded(&i.take_events()), Err(ApiError::Offline));
    let mut anon = intake(&t, false, None);
    anon.handle(Command::Discard(1001));
    assert_eq!(discarded(&anon.take_events()), Err(ApiError::TokenNeeded));
}

#[test]
fn the_wantlist_is_read_once_and_owned_wants_follow_a_sync() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    // The collection holds 1002, which is wanted (1004 and 1006 are too, and not owned).
    t.route(
        "/users/digger/collection/folders/0/releases?sort=added&sort_order=desc&page=1&per_page=100",
        200,
        r#"{"pagination": {"page": 1, "pages": 1, "items": 1},
            "releases": [{"id": 1002, "instance_id": 7,
                          "basic_information": {"id": 1002, "master_id": 0, "year": 2016,
                                                "labels": [{"catno": "LT-002"}]}}]}"#,
    );
    let mut i = intake(&t, true, None);
    i.handle(Command::ReadWants { fresh: false });
    let ev = i.take_events();
    assert!(
        ev.contains(&Event::Wants(Ok(vec![1002, 1004, 1006]))),
        "{ev:?}"
    );
    i.handle(Command::SyncCollection(None));
    let ev = run(&mut i, 10);
    assert!(ev.contains(&Event::OwnedWants(vec![1002])), "{ev:?}");
    let reads = t.paths().iter().filter(|p| p.contains("/wants?")).count();
    assert_eq!(reads, 1, "read once a session");
    // A refresh reads it again, and sees what changed on discogs.com.
    t.route(
        "/users/digger/wants?page=1&per_page=100",
        200,
        r#"{"pagination": {"page": 1, "pages": 1, "items": 1}, "wants": [{"id": 1004}]}"#,
    );
    i.handle(Command::ReadWants { fresh: false });
    assert!(
        i.take_events()
            .contains(&Event::Wants(Ok(vec![1002, 1004, 1006])))
    );
    i.handle(Command::ReadWants { fresh: true });
    assert!(i.take_events().contains(&Event::Wants(Ok(vec![1004]))));
    let reads = t.paths().iter().filter(|p| p.contains("/wants?")).count();
    assert_eq!(reads, 2);
}

#[test]
fn a_token_is_checked_before_it_is_used() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    let mut i = intake(&t, false, None);
    i.handle(Command::CheckToken("good".into()));
    let ev = i.take_events();
    assert!(
        matches!(&ev[0], Event::TokenChecked(tok, Ok(id)) if tok == "good" && id.username == "digger")
    );
    t.route("/oauth/identity", 401, "{}");
    i.handle(Command::CheckToken("bad".into()));
    let ev = i.take_events();
    assert!(matches!(
        &ev[0],
        Event::TokenChecked(_, Err(ApiError::TokenRejected))
    ));
}

#[test]
fn a_stale_release_is_refreshed() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    let mut i = intake(&t, true, None);
    i.handle(Command::Refresh(1006));
    let ev = i.take_events();
    assert!(
        ev.iter().any(
            |e| matches!(e, Event::ForSale(1006, fs) if fs.count == 1 && fs.lowest == Some(4.5))
        )
    );
}

/// Against the live API, by hand: `cargo test -p dig --test intake -- --ignored`, with
/// `DIGGR_DISCOGS_TOKEN` set to use a token.
#[test]
#[ignore]
fn a_real_release_expands() {
    let client = Client::new(
        Arc::new(dig::discogs::transport::UreqTransport::default()),
        Arc::new(dig::clock::RealClock::default()),
        std::env::var("DIGGR_DISCOGS_TOKEN").ok(),
        DiskCache::default(),
    );
    let mut i = Intake::new(client, None);
    i.handle(Command::Send {
        page: parse("https://www.discogs.com/release/1-The-Persuader-Stockholm").unwrap(),
        target: CRATE,
        filters: Filters { skip_passed: false },
    });
    let ev = run(&mut i, 10);
    let r = records(&ev);
    assert_eq!(r.len(), 1, "{ev:?}");
    assert_eq!(r[0].0, Release(1));
}

#[test]
fn a_stale_cover_is_looked_up_again_once() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    let mut i = intake(&t, true, None);
    i.handle(Command::RefreshCover(Release(1001)));
    let ev = i.take_events();
    let cover = ev.iter().find_map(|e| match e {
        Event::Cover(k, url) => Some((*k, url.clone())),
        _ => None,
    });
    assert_eq!(
        cover,
        Some((
            Release(1001),
            "https://i.discogs.com/fake/R-1001-front-150.jpeg".to_owned()
        ))
    );
    let releases = t
        .paths()
        .iter()
        .filter(|p| p.starts_with("/releases/1001"))
        .count();
    assert_eq!(releases, 1, "one request for the release's data");
    // A record Discogs doesn't know: no address.
    i.handle(Command::RefreshCover(Release(424242)));
    assert!(
        i.take_events()
            .iter()
            .any(|e| matches!(e, Event::Cover(Release(424242), url) if url.is_empty()))
    );
}

#[test]
fn a_record_listed_more_than_once_comes_in_once() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    // AF069 is listed three times on the label's page (the label is credited thrice).
    t.route(
        "/labels/906282",
        200,
        r#"{"id": 906282, "name": "Analogical Force"}"#,
    );
    t.route(
        "/labels/906282/releases?page=1&per_page=100",
        200,
        r#"{"pagination": {"page": 1, "pages": 1, "items": 4},
            "releases": [
              {"id": 1001, "catno": "AF069", "title": "Hidden Soul EP", "format": "12\", EP"},
              {"id": 1001, "catno": "AF069", "title": "Hidden Soul EP", "format": "12\", EP"},
              {"id": 1003, "catno": "AF070", "title": "Next", "format": "12\", EP"},
              {"id": 1001, "catno": "AF069", "title": "Hidden Soul EP", "format": "12\", EP"}]}"#,
    );
    let mut i = intake(&t, true, None);
    send(&mut i, "https://www.discogs.com/label/906282");
    let ev = run(&mut i, 100);
    assert_eq!(listed(&ev), [Release(1001), Release(1003)]);
    let keys: Vec<RecordKey> = records(&ev).iter().map(|(k, _)| *k).collect();
    assert_eq!(keys, [Release(1001), Release(1003)], "each expanded once");
    let progress = ev.iter().rev().find_map(|e| match e {
        Event::Progress(_, d, t) => Some((*d, *t)),
        _ => None,
    });
    assert_eq!(progress, Some((2, 2)), "counted once too");
}

#[test]
fn a_record_with_neither_clips_nor_tracklist_is_no_clip() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    t.route(
        "/releases/4242?curr_abbr=EUR",
        200,
        r#"{"id": 4242, "title": "Untitled", "artists": [{"name": "Nobody"}],
            "formats": [{"name": "Vinyl"}], "tracklist": [], "videos": []}"#,
    );
    let mut i = intake(&t, true, None);
    send(&mut i, "https://www.discogs.com/release/4242");
    let r = records(&run(&mut i, 20));
    assert_eq!(r, [(Release(4242), Outcome::Unavailable("no clip".into()))]);
}

fn inventory_listing(id: u64, release: u64, price: f64) -> serde_json::Value {
    serde_json::json!({
        "id": id, "status": "For Sale",
        "price": {"value": price, "currency": "EUR"},
        "condition": "Very Good Plus (VG+)", "sleeve_condition": "Very Good (VG)",
        "ships_from": "Germany", "posted": "2026-02-06T08:12:00-08:00",
        "release": {"id": release, "artist": "Nightcraft", "title": "Glasshouse EP",
                    "format": "12\"", "label": "Lowtide Tapes", "catalog_number": "LT-012",
                    "year": 1994, "thumbnail": ""}
    })
}

fn send_inventory(i: &mut Intake, criteria: dig::discogs::seller::Criteria) {
    use dig::discogs::url::{Page, PageKind};
    i.handle(Command::Send {
        page: Page::new(PageKind::Inventory {
            seller: "decks.de".into(),
            criteria,
            fresh: false,
        }),
        target: CRATE,
        filters: Filters::default(),
    });
}

#[test]
fn a_seller_brings_every_copy_and_each_record_once() {
    use dig::discogs::seller::inventory_path;
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    let body = serde_json::json!({
        "pagination": {"page": 1, "pages": 1, "per_page": 100, "items": 3},
        "listings": [
            inventory_listing(11, 1001, 9.0),
            inventory_listing(12, 1002, 7.5),
            inventory_listing(13, 1001, 18.0),
        ]
    });
    t.route(
        inventory_path("decks.de", "", 1, 100),
        200,
        body.to_string(),
    );
    let mut i = intake(&t, true, None);
    send_inventory(&mut i, Default::default());
    let ev = run(&mut i, 100);
    let copies: Vec<(u64, u64)> = ev
        .iter()
        .flat_map(|e| match e {
            Event::Copies(_, c) => c.iter().map(|c| (c.listing, c.release)).collect(),
            _ => Vec::new(),
        })
        .collect();
    assert_eq!(copies, [(11, 1001), (12, 1002), (13, 1001)], "every copy");
    assert_eq!(
        listed(&ev),
        [Release(1001), Release(1002)],
        "each record once"
    );
    assert_eq!(records(&ev).len(), 2);
    let named: Vec<String> = ev
        .iter()
        .filter_map(|e| match e {
            Event::Named(j) => Some(j.name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(named, ["Seller: decks.de"]);
}

#[test]
fn newest_n_stops_reading_at_the_nth_copy() {
    use dig::discogs::seller::{Criteria, inventory_path};
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    let listings: Vec<_> = (0..100)
        .map(|n| inventory_listing(100 + n, 1001, 9.0))
        .collect();
    let page1 = serde_json::json!({
        "pagination": {"page": 1, "pages": 408, "per_page": 100, "items": 40_728},
        "listings": listings,
    });
    let listings: Vec<_> = (0..100)
        .map(|n| inventory_listing(200 + n, 1002, 9.0))
        .collect();
    let page2 = serde_json::json!({
        "pagination": {"page": 2, "pages": 408, "per_page": 100, "items": 40_728},
        "listings": listings,
    });
    t.route(
        inventory_path("decks.de", "", 1, 100),
        200,
        page1.to_string(),
    );
    t.route(
        inventory_path("decks.de", "", 2, 100),
        200,
        page2.to_string(),
    );
    let mut i = intake(&t, true, None);
    send_inventory(
        &mut i,
        Criteria {
            newest: Some(150),
            ..Criteria::default()
        },
    );
    let ev = run(&mut i, 100);
    let n: usize = ev
        .iter()
        .map(|e| match e {
            Event::Copies(_, c) => c.len(),
            _ => 0,
        })
        .sum();
    assert_eq!(n, 150);
    let inventory_reads = t
        .paths()
        .iter()
        .filter(|p| p.contains("/inventory"))
        .count();
    assert_eq!(inventory_reads, 2, "two pages for the newest 150, not 100");
}

#[test]
fn a_scan_reads_page_by_page_ahead_of_sends_and_can_be_cancelled() {
    use dig::discogs::seller::inventory_path;
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    for n in 1..=3u64 {
        let listings: Vec<_> = (0..100)
            .map(|k| inventory_listing(n * 1000 + k, 1001, 9.0))
            .collect();
        let body = serde_json::json!({
            "pagination": {"page": n, "pages": 3, "per_page": 100, "items": 300},
            "listings": listings,
        });
        t.route(
            inventory_path("decks.de", "", n as u32, 100),
            200,
            body.to_string(),
        );
    }
    let mut i = intake(&t, true, None);
    send(&mut i, "https://www.discogs.com/label/12345");
    i.take_events();
    i.handle(Command::ScanInventory {
        seller: "decks.de".into(),
        query: String::new(),
        newest: None,
    });
    i.step();
    let ev = i.take_events();
    assert!(
        matches!(&ev[..], [Event::Scanned { read: 1, pages: 3, total: 300, done: false, copies, .. }] if copies.len() == 100),
        "the scan goes before the send: {} events",
        ev.len()
    );
    i.handle(Command::CancelScan);
    i.step();
    assert!(
        !i.take_events()
            .iter()
            .any(|e| matches!(e, Event::Scanned { .. })),
        "cancelled"
    );
}

#[test]
fn a_whole_scan_ends_done() {
    use dig::discogs::seller::inventory_path;
    let t = Arc::new(FakeTransport::new());
    for n in 1..=2u64 {
        let body = serde_json::json!({
            "pagination": {"page": n, "pages": 2, "per_page": 100, "items": 2},
            "listings": [inventory_listing(n, 1001, 9.0)],
        });
        t.route(
            inventory_path("logon", "acid", n as u32, 100),
            200,
            body.to_string(),
        );
    }
    let mut i = intake(&t, true, None);
    i.handle(Command::ScanInventory {
        seller: "logon".into(),
        query: "acid".into(),
        newest: None,
    });
    let ev = run(&mut i, 10);
    let pages: Vec<(u32, bool)> = ev
        .iter()
        .filter_map(|e| match e {
            Event::Scanned { read, done, .. } => Some((*read, *done)),
            _ => None,
        })
        .collect();
    assert_eq!(pages, [(1, false), (2, true)]);
}

#[test]
fn the_first_sellers_wait_for_no_send() {
    use dig::discogs::seller::inventory_path;
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    let order =
        |s: &str| serde_json::json!({"created": "2026-01-01", "seller": {"id": 1, "username": s}});
    t.route(
        "/purchases?sort=created&sort_order=desc&page=1&per_page=100",
        200,
        serde_json::json!({"items": [order("logon"), order("logon"), order("decks.de")]})
            .to_string(),
    );
    let count = |n: u64| {
        serde_json::json!({"pagination": {"items": n, "pages": 1}, "listings": []}).to_string()
    };
    t.route(inventory_path("logon", "", 1, 1), 200, count(2675));
    t.route(inventory_path("decks.de", "", 1, 1), 200, count(40_728));
    let mut i = intake(&t, true, None);
    i.handle(Command::FirstSellers);
    let ev = run(&mut i, 20);
    let list = ev
        .iter()
        .find_map(|e| match e {
            Event::FirstSellers(Ok(l)) => Some(l.clone()),
            _ => None,
        })
        .expect("a first list");
    let names: Vec<_> = list
        .iter()
        .map(|(r, n)| (r.username.as_str(), *n))
        .collect();
    assert_eq!(names, [("logon", 2675), ("decks.de", 40_728)]);
}

#[test]
fn a_seller_is_looked_up_by_name() {
    use dig::discogs::seller::inventory_path;
    let t = Arc::new(FakeTransport::new());
    t.route("/users/LOGON", 200, r#"{"username": "logon"}"#);
    t.route(
        inventory_path("logon", "", 1, 1),
        200,
        r#"{"pagination": {"items": 2675}, "listings": []}"#,
    );
    let mut i = intake(&t, true, None);
    i.handle(Command::LookupSeller(" LOGON ".into()));
    i.handle(Command::LookupSeller("nobody-xyz".into()));
    let ev = i.take_events();
    let results: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            Event::SellerLookup { result, .. } => Some(result.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        results,
        [Ok(("logon".to_owned(), 2675)), Err(ApiError::NotFound)]
    );
}

#[test]
fn a_cart_add_is_one_request_then_the_cart_is_read() {
    use dig::discogs::cart::AddResult;
    let t = Arc::new(FakeTransport::new());
    t.route_for(
        Method::Post,
        "/cart/items",
        201,
        r#"{"errors": [{"item_id": 12, "status": 404, "message": "This item is not for sale."}]}"#,
    );
    t.route(
        "/cart",
        200,
        r#"[{"seller": {"username": "decks.de"}, "subtotal": {"formatted": "€9.00"},
            "cart_items": [{"item_id": 11, "price": {"formatted": "€9.00"}, "release": {"id": 1001}}]}]"#,
    );
    let mut i = intake(&t, true, None);
    i.handle(Command::CartAdd(vec![11, 12]));
    let mut ev = i.take_events();
    ev.extend(run(&mut i, 5));
    assert!(ev.iter().any(|e| matches!(e,
        Event::CartAdded { result: Ok(r), .. } if r == &[(11, AddResult::Added), (12, AddResult::Sold)])));
    let snap = ev
        .iter()
        .find_map(|e| match e {
            Event::Cart(Ok(s)) => Some(s.clone()),
            _ => None,
        })
        .expect("the cart is read after the add");
    assert!(snap.has_listing(11));
    assert_eq!(t.paths(), ["/cart/items", "/cart"]);
}

#[test]
fn friends_are_read_with_private_ones_checked_again_and_a_friends_collection_page_by_page() {
    let t = Arc::new(FakeTransport::with_fixtures(fixtures()));
    t.route(
        "/users/digger/friends?page=1&per_page=100",
        200,
        r#"{"pagination": {"page": 1, "pages": 1},
            "friends": [{"user": {"username": "javimaxilo"}}, {"user": {"username": "Waxport"}}]}"#,
    );
    t.route(
        "/users/Waxport/collection/folders/0/releases?page=1&per_page=1",
        403,
        "{}",
    );
    let mut i = intake(&t, true, None);
    t.route(
        "/users/javimaxilo/collection/folders/0/releases?page=1&per_page=1",
        200,
        "{}",
    );
    i.handle(Command::ReadFriends);
    let ev = run(&mut i, 20);
    let list = ev
        .iter()
        .find_map(|e| match e {
            Event::Friends(Ok(l)) => Some(l.clone()),
            _ => None,
        })
        .expect("a friends list");
    assert_eq!(
        list,
        [
            ("javimaxilo".to_owned(), false),
            ("Waxport".to_owned(), true)
        ]
    );
    let checks = t
        .paths()
        .iter()
        .filter(|p| p.ends_with("per_page=1"))
        .count();
    assert_eq!(checks, 2, "each friend's collection is checked");

    // A friend's collection, two pages: progress, then the whole of it.
    for n in 1..=2u64 {
        t.route(
            format!(
                "/users/javimaxilo/collection/folders/0/releases?sort=added&sort_order=desc&page={n}&per_page=100"
            ),
            200,
            format!(
                r#"{{"pagination": {{"page": {n}, "pages": 2, "items": 2}},
                    "releases": [{{"id": {r}, "instance_id": {n}, "basic_information": {{"id": {r}}}}}]}}"#,
                r = 1000 + n
            ),
        );
    }
    i.handle(Command::ReadFriendCollection {
        target: 7,
        user: "javimaxilo".into(),
    });
    let ev = run(&mut i, 20);
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::FriendProgress {
            target: 7,
            read: 1,
            pages: 2
        }
    )));
    let c = ev
        .iter()
        .find_map(|e| match e {
            Event::FriendCollection(7, Ok(c)) => Some(c.releases.len()),
            _ => None,
        })
        .expect("the collection");
    assert_eq!(c, 2);
}
