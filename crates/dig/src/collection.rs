//! The user's Discogs collection, so records they already own can be marked and never bought
//! twice. Kept in `<cache>/collection.ron` and synced with as few requests as possible:
//!
//! - the first sync reads every page (100 records a request), newest first;
//! - later syncs read newest first and stop at the first record already known, so a normal
//!   week costs one request; only when the collection's total shows records were removed
//!   (sold, deleted) is everything read again;
//! - other pressings are never looked up: the match uses master ids that the collection items
//!   and already-fetched release data carry.
//!
//! A sync that fails part-way returns an error and the previous collection stays in use.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::discogs::client::{ApiError, Client, path_segment};
use crate::discogs::transport::Method;

pub const FILE: &str = "collection.ron";
/// A collection older than this is synced again, when there is something to mark.
pub const MAX_AGE_SECS: u64 = 7 * 24 * 3600;
const PER_PAGE: u32 = 100;

/// One owned release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pressing {
    pub master: Option<u64>,
    pub catno: String,
    pub year: Option<u16>,
}

/// Whether a record is owned, and which pressing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owned {
    /// This very release.
    ThisPressing,
    /// Another release of the same master.
    Another { catno: String, year: Option<u16> },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Collection {
    pub username: String,
    /// Seconds since the Unix epoch.
    pub fetched_at: u64,
    /// Items (copies) Discogs counted at the last sync.
    pub count: usize,
    /// Every item's instance id, to know where the new ones end.
    pub instances: BTreeSet<u64>,
    /// Owned releases.
    pub releases: BTreeMap<u64, Pressing>,
    /// Master id → owned releases of it (built on load, not saved).
    #[serde(skip)]
    by_master: HashMap<u64, Vec<u64>>,
}

impl Collection {
    /// An empty collection for `username`, as of `now` (filled by syncs and adds).
    pub fn new(username: &str, now: u64) -> Self {
        Self {
            username: username.to_owned(),
            fetched_at: now,
            ..Default::default()
        }
    }

    pub fn path(cache: &Path) -> PathBuf {
        cache.join(FILE)
    }

    /// The cached collection, if there is a readable one.
    pub fn load(cache: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(Self::path(cache)).ok()?;
        let mut c: Self = ron::from_str(&text).ok()?;
        c.index();
        Some(c)
    }

    /// Written atomically: a crash never leaves half a collection.
    pub fn save(&self, cache: &Path) -> std::io::Result<()> {
        let text = ron::ser::to_string(self).map_err(std::io::Error::other)?;
        crate::config::write_atomic(&Self::path(cache), text.as_bytes(), false)
    }

    fn index(&mut self) {
        self.by_master.clear();
        for (&id, p) in &self.releases {
            if let Some(m) = p.master {
                self.by_master.entry(m).or_default().push(id);
            }
        }
    }

    /// Records owned (releases; several copies count once).
    pub fn len(&self) -> usize {
        self.releases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.releases.is_empty()
    }

    /// Old enough (or for another account) to sync again.
    pub fn is_stale(&self, username: &str, now: u64) -> bool {
        self.username != username || now.saturating_sub(self.fetched_at) >= MAX_AGE_SECS
    }

    /// Is the record owned: this release, or another release of its master?
    pub fn owned(&self, release: Option<u64>, master: Option<u64>) -> Option<Owned> {
        if release.is_some_and(|r| self.releases.contains_key(&r)) {
            return Some(Owned::ThisPressing);
        }
        let other = self.by_master.get(&master?)?.first()?;
        let p = &self.releases[other];
        Some(Owned::Another {
            catno: p.catno.clone(),
            year: p.year,
        })
    }

    fn add(&mut self, item: &Item) {
        self.instances.insert(item.instance);
        self.releases.insert(item.release, item.pressing.clone());
    }

    /// A copy the app just added on Discogs (`instance` is its new id): owned from now on,
    /// and counted, so the next sync stays incremental. Known instances are ignored.
    pub fn insert(&mut self, instance: u64, release: u64, pressing: Pressing) {
        if self.instances.contains(&instance) {
            return;
        }
        self.add(&Item {
            instance,
            release,
            pressing,
        });
        self.count += 1;
        self.index();
    }

    /// A copy the app just removed on Discogs (`instance`, or `None` when Discogs had no copy
    /// left to remove): no longer counted, so the next sync stays incremental; with no copy
    /// `remaining`, the release is no longer owned.
    pub fn discard(&mut self, instance: Option<u64>, release: u64, remaining: usize) {
        if let Some(i) = instance
            && self.instances.remove(&i)
        {
            self.count = self.count.saturating_sub(1);
        }
        if remaining == 0 && self.releases.remove(&release).is_some() {
            self.index();
        }
    }
}

/// One collection item (a copy of a release).
#[derive(Debug, Clone, PartialEq)]
struct Item {
    instance: u64,
    release: u64,
    pressing: Pressing,
}

fn item(v: &Value) -> Option<Item> {
    let b = &v["basic_information"];
    Some(Item {
        instance: v["instance_id"].as_u64()?,
        release: v["id"].as_u64().or(b["id"].as_u64())?,
        pressing: Pressing {
            master: b["master_id"].as_u64().filter(|&m| m > 0),
            catno: b["labels"][0]["catno"]
                .as_str()
                .unwrap_or("")
                .trim()
                .to_owned(),
            year: b["year"]
                .as_u64()
                .filter(|&y| y > 0)
                .and_then(|y| u16::try_from(y).ok()),
        },
    })
}

/// One page, newest first: its items and the collection's total.
fn page(client: &mut Client, user: &str, n: u32) -> Result<(Vec<Item>, u32, usize), ApiError> {
    let path = format!(
        "/users/{}/collection/folders/0/releases?sort=added&sort_order=desc&page={n}&per_page={PER_PAGE}",
        path_segment(user)
    );
    let r = client.call(Method::Get, &path)?;
    let v: Value =
        serde_json::from_str(&r.body).map_err(|e| ApiError::Other(format!("{path}: {e}")))?;
    let items = v["releases"]
        .as_array()
        .map(|a| a.iter().filter_map(item).collect())
        .unwrap_or_default();
    let pages = v["pagination"]["pages"].as_u64().unwrap_or(1) as u32;
    let total = v["pagination"]["items"].as_u64().unwrap_or(0) as usize;
    Ok((items, pages, total))
}

/// Brings `cached` up to date for `user` (or reads everything when there is none, it belongs
/// to someone else, or records were removed). Only a complete result is returned.
pub fn sync(
    client: &mut Client,
    user: &str,
    cached: Option<&Collection>,
    now: u64,
) -> Result<Collection, ApiError> {
    let mut s = Syncer::new(user, cached, now);
    loop {
        if let Some(c) = s.step(client)? {
            return Ok(c);
        }
    }
}

/// A sync one page at a time, so it can show how far it got and be stopped between pages.
#[derive(Debug, Clone)]
pub struct Syncer {
    user: String,
    now: u64,
    /// Bringing a copy of the cache up to date: what it had, and what was added.
    newer: Option<(Collection, usize)>,
    /// What a full read has gathered.
    full: Collection,
    next: u32,
    pages: u32,
}

impl Syncer {
    pub fn new(user: &str, cached: Option<&Collection>, now: u64) -> Self {
        let newer = cached
            .filter(|c| c.username == user && !c.instances.is_empty())
            .map(|c| (c.clone(), 0));
        Self {
            user: user.to_owned(),
            now,
            newer,
            full: Collection::default(),
            next: 1,
            pages: 0,
        }
    }

    /// Pages read, and in all (0 until the first page says).
    pub fn progress(&self) -> (u32, u32) {
        (self.next - 1, self.pages)
    }

    /// Reads one page: the collection once complete, `None` while pages are left.
    pub fn step(&mut self, client: &mut Client) -> Result<Option<Collection>, ApiError> {
        let n = self.next;
        let (items, pages, total) = page(client, &self.user, n)?;
        self.pages = pages.max(n);
        self.next += 1;
        let last = n >= pages || items.is_empty();
        if let Some((c, added)) = &mut self.newer {
            let mut known = false;
            for it in &items {
                if c.instances.contains(&it.instance) {
                    known = true;
                    break;
                }
                c.add(it);
                *added += 1;
            }
            if !(known || last) {
                return Ok(None);
            }
            let (mut c, added) = self.newer.take().expect("updating");
            if c.count + added == total {
                c.count = total;
                c.fetched_at = self.now;
                c.index();
                return Ok(Some(c));
            }
            // Something was removed: only a full read knows what.
            (self.next, self.pages) = (1, 0);
            return Ok(None);
        }
        for it in &items {
            self.full.add(it);
        }
        self.full.count = total;
        if !last {
            return Ok(None);
        }
        let mut c = std::mem::take(&mut self.full);
        c.username = self.user.clone();
        c.fetched_at = self.now;
        c.index();
        Ok(Some(c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use crate::discogs::cache::DiskCache;
    use crate::discogs::transport::FakeTransport;
    use serde_json::json;
    use std::sync::Arc;

    fn client(t: &Arc<FakeTransport>) -> Client {
        Client::new(
            t.clone(),
            Arc::new(FakeClock::default()),
            Some("tok".into()),
            DiskCache::new(None),
        )
    }

    /// Item `i`: release 1000 + i, master 500 + i (so two items can share a master).
    fn it(instance: u64, release: u64, master: u64) -> Value {
        json!({
            "id": release, "instance_id": instance,
            "basic_information": {
                "id": release, "master_id": master, "year": 2018,
                "labels": [{"name": "Analogical Force", "catno": format!("AF{release}")}]
            }
        })
    }

    fn route_pages(t: &FakeTransport, pages: &[Vec<Value>], total: usize) {
        for (i, items) in pages.iter().enumerate() {
            let n = i + 1;
            t.route(
                format!(
                    "/users/digger/collection/folders/0/releases?sort=added&sort_order=desc&page={n}&per_page=100"
                ),
                200,
                json!({
                    "pagination": {"page": n, "pages": pages.len(), "items": total},
                    "releases": items
                })
                .to_string(),
            );
        }
    }

    fn collection_calls(t: &FakeTransport) -> usize {
        t.paths()
            .iter()
            .filter(|p| p.contains("/collection/"))
            .count()
    }

    #[test]
    fn the_first_sync_reads_every_page() {
        let t = Arc::new(FakeTransport::new());
        let pages: Vec<Vec<Value>> = (0..3)
            .map(|p| {
                (0..100u64)
                    .map(|i| {
                        let k = p * 100 + i;
                        it(k, 10_000 + k, 50_000 + k)
                    })
                    .take(if p == 2 { 34 } else { 100 })
                    .collect()
            })
            .collect();
        route_pages(&t, &pages, 234);
        let c = sync(&mut client(&t), "digger", None, 7).unwrap();
        assert_eq!((c.len(), c.count, c.fetched_at), (234, 234, 7));
        assert_eq!(collection_calls(&t), 3);

        // One page a step, saying how far it got; dropping it between pages stops it.
        let mut s = Syncer::new("digger", None, 7);
        let mut cl = client(&t);
        assert_eq!(s.progress(), (0, 0));
        assert!(s.step(&mut cl).unwrap().is_none());
        assert_eq!(s.progress(), (1, 3));
        assert!(s.step(&mut cl).unwrap().is_none());
        assert_eq!(s.progress(), (2, 3));
        assert_eq!(s.step(&mut cl).unwrap().map(|c| c.len()), Some(234));
        assert_eq!(collection_calls(&t), 6);
    }

    #[test]
    fn a_week_later_one_request_brings_the_new_records() {
        let t = Arc::new(FakeTransport::new());
        route_pages(&t, &[vec![it(1, 101, 901), it(2, 102, 902)]], 2);
        let old = sync(&mut client(&t), "digger", None, 0).unwrap();
        // Three bought since, newest first, then the two known ones.
        let t = Arc::new(FakeTransport::new());
        route_pages(
            &t,
            &[vec![
                it(5, 105, 905),
                it(4, 104, 904),
                it(3, 103, 903),
                it(2, 102, 902),
                it(1, 101, 901),
            ]],
            5,
        );
        let c = sync(&mut client(&t), "digger", Some(&old), 100).unwrap();
        assert_eq!(collection_calls(&t), 1);
        assert_eq!((c.len(), c.count), (5, 5));
        assert_eq!(c.owned(Some(103), None), Some(Owned::ThisPressing));
    }

    #[test]
    fn a_removal_forces_a_full_read() {
        let t = Arc::new(FakeTransport::new());
        route_pages(&t, &[vec![it(2, 102, 902), it(1, 101, 901)]], 2);
        let old = sync(&mut client(&t), "digger", None, 0).unwrap();
        // 101 was sold, 103 bought: the total stays 2, so the new one alone doesn't add up.
        let t = Arc::new(FakeTransport::new());
        route_pages(&t, &[vec![it(3, 103, 903), it(2, 102, 902)]], 2);
        let c = sync(&mut client(&t), "digger", Some(&old), 100).unwrap();
        assert_eq!(
            collection_calls(&t),
            2,
            "the incremental read, then the full one"
        );
        assert_eq!(c.owned(Some(101), None), None, "the sold record is gone");
        assert_eq!(c.owned(Some(103), None), Some(Owned::ThisPressing));
    }

    #[test]
    fn a_failure_midway_returns_nothing() {
        let t = Arc::new(FakeTransport::new());
        // Page 2 of 2 is missing.
        t.route(
            "/users/digger/collection/folders/0/releases?sort=added&sort_order=desc&page=1&per_page=100",
            200,
            json!({"pagination": {"page": 1, "pages": 2, "items": 150},
                   "releases": [it(1, 101, 901)]})
            .to_string(),
        );
        assert!(sync(&mut client(&t), "digger", None, 0).is_err());
    }

    #[test]
    fn another_pressing_of_the_same_master_counts() {
        let t = Arc::new(FakeTransport::new());
        route_pages(&t, &[vec![it(1, 12_049_380, 1_371_366)]], 1);
        let c = sync(&mut client(&t), "digger", None, 0).unwrap();
        assert_eq!(
            c.owned(Some(12_049_380), Some(1_371_366)),
            Some(Owned::ThisPressing)
        );
        assert_eq!(
            c.owned(Some(99), Some(1_371_366)),
            Some(Owned::Another {
                catno: "AF12049380".into(),
                year: Some(2018)
            })
        );
        assert_eq!(c.owned(Some(99), Some(5)), None);
        assert_eq!(c.owned(None, None), None);
    }

    #[test]
    fn an_add_from_the_app_keeps_the_next_sync_incremental() {
        let t = Arc::new(FakeTransport::new());
        route_pages(&t, &[vec![it(2, 102, 902), it(1, 101, 901)]], 2);
        let mut c = sync(&mut client(&t), "digger", None, 0).unwrap();
        let pressing = Pressing {
            master: Some(903),
            catno: "AF103".into(),
            year: Some(2019),
        };
        c.insert(3, 103, pressing.clone());
        c.insert(3, 103, pressing);
        assert_eq!((c.len(), c.count), (3, 3), "a known instance counts once");
        assert!(c.owned(Some(7), Some(903)).is_some(), "indexed by master");
        // Discogs now lists it first: the sync stops there, with one request.
        let t = Arc::new(FakeTransport::new());
        route_pages(
            &t,
            &[vec![it(3, 103, 903), it(2, 102, 902), it(1, 101, 901)]],
            3,
        );
        let next = sync(&mut client(&t), "digger", Some(&c), 100).unwrap();
        assert_eq!(collection_calls(&t), 1);
        assert_eq!(next.len(), 3);
    }

    #[test]
    fn a_removal_from_the_app_keeps_the_next_sync_incremental() {
        let t = Arc::new(FakeTransport::new());
        // 102 and 101 are pressings of master 900; 103 has two copies (3 and 4).
        route_pages(
            &t,
            &[vec![
                it(4, 103, 903),
                it(3, 103, 903),
                it(2, 102, 900),
                it(1, 101, 900),
            ]],
            4,
        );
        let mut c = sync(&mut client(&t), "digger", None, 0).unwrap();
        c.discard(Some(4), 103, 1);
        assert_eq!((c.len(), c.count), (3, 3), "one copy of 103 is left");
        assert_eq!(c.owned(Some(103), None), Some(Owned::ThisPressing));
        c.discard(Some(2), 102, 0);
        assert_eq!((c.len(), c.count), (2, 2));
        assert_eq!(
            c.owned(Some(102), Some(900)),
            Some(Owned::Another {
                catno: "AF101".into(),
                year: Some(2018)
            }),
            "another pressing of its master is still owned"
        );
        c.discard(None, 101, 0);
        assert_eq!(c.owned(Some(101), Some(900)), None);
        assert_eq!(c.count, 2, "an unknown copy isn't counted off");

        // With the app's removals only, the next sync takes one request.
        let t = Arc::new(FakeTransport::new());
        route_pages(&t, &[vec![it(2, 102, 902), it(1, 101, 901)]], 2);
        let mut c = sync(&mut client(&t), "digger", None, 0).unwrap();
        c.discard(Some(2), 102, 0);
        let t = Arc::new(FakeTransport::new());
        route_pages(&t, &[vec![it(1, 101, 901)]], 1);
        let next = sync(&mut client(&t), "digger", Some(&c), 100).unwrap();
        assert_eq!(collection_calls(&t), 1);
        assert_eq!(next.len(), 1);
    }

    #[test]
    fn an_add_on_discogs_com_too_forces_a_full_read() {
        let t = Arc::new(FakeTransport::new());
        route_pages(&t, &[vec![it(1, 101, 901)]], 1);
        let mut c = sync(&mut client(&t), "digger", None, 0).unwrap();
        // 104 was added on discogs.com, then 103 from the app (newest).
        c.insert(
            3,
            103,
            Pressing {
                master: None,
                catno: String::new(),
                year: None,
            },
        );
        let t = Arc::new(FakeTransport::new());
        route_pages(
            &t,
            &[vec![it(3, 103, 903), it(4, 104, 904), it(1, 101, 901)]],
            3,
        );
        let next = sync(&mut client(&t), "digger", Some(&c), 100).unwrap();
        assert_eq!(
            collection_calls(&t),
            2,
            "the count doesn't add up: everything again"
        );
        assert!(next.owned(Some(104), None).is_some() && next.owned(Some(103), None).is_some());
    }

    #[test]
    fn the_cache_round_trips_and_ages() {
        let dir = crate::test_dir("collection-cache");
        let t = Arc::new(FakeTransport::new());
        route_pages(&t, &[vec![it(1, 101, 901)]], 1);
        let c = sync(&mut client(&t), "digger", None, 1_000).unwrap();
        c.save(dir.path()).unwrap();
        let back = Collection::load(dir.path()).unwrap();
        assert!(
            back.owned(Some(7), Some(901)).is_some(),
            "the index is rebuilt"
        );
        assert!(!back.is_stale("digger", 1_000 + MAX_AGE_SECS - 1));
        assert!(back.is_stale("digger", 1_000 + MAX_AGE_SECS));
        assert!(back.is_stale("someone-else", 1_000));
    }
}
