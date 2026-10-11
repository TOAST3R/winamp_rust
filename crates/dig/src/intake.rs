//! The intake worker: every Discogs request, one at a time, on one low-priority thread.
//!
//! A send becomes a [`Job`]: first the page's name, then its listing, 100 records a request,
//! each record reported at once as "listed"; then each record's details, nearest the playhead
//! first, reported as its clip entries or "no clip". Jobs are
//! saved as they go, so a send interrupted by quitting resumes when its crate is next shown.
//!
//! [`Intake`] is the synchronous core (tests drive it step by step); [`IntakeHandle`] runs it
//! on a thread and talks to the UI over channels.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Duration;

use platform::{Priority, Spawner};

use crate::collection::{self, Collection, Pressing};
use crate::discogs::cart::{self, AddResult, CartSnapshot};
use crate::discogs::client::{ApiError, Client, Identity, path_segment};
use crate::discogs::expand::{self, PER_PAGE};
use crate::discogs::matching::{self, RecordPlan};
use crate::discogs::model::{ForSale, Format, Listed, Record, RecordKey, Role};
use crate::discogs::seller::{self, Copy, Criteria, Ranked};
use crate::discogs::transport::Method;
use crate::discogs::url::{Page, PageKind};
use crate::friends;
use crate::jobs::{Filters, Job, JobId, Jobs};

/// While Discogs doesn't answer, the same request is tried again this often.
pub const OFFLINE_RETRY: Duration = Duration::from_secs(15);
/// Progress is written to `jobs.ron` at most this often while details are fetched.
const SAVE_EVERY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Expand a page into crate `target`.
    Send {
        page: Page,
        target: u64,
        filters: Filters,
    },
    /// A crate was shown or played: its unfinished jobs go on.
    CrateActive(u64),
    /// A crate was deleted: its jobs stop and are forgotten.
    CrateDeleted(u64),
    /// Where the user is in crate `target`: the listed records from the focus on, wrapping
    /// around. Their details are fetched in that order.
    Focus {
        target: u64,
        order: Vec<RecordKey>,
    },
    /// Fresh marketplace numbers for a release (its track started and they are a day old).
    Refresh(u64),
    /// A saved token (or none) to use from now on.
    SetToken(Option<String>),
    /// Check a token before it is saved; the current one stays in use.
    CheckToken(String),
    /// Add a release to the user's wantlist (unless it is already there).
    Want(u64),
    /// Take a release off the user's wantlist, whoever added it.
    Unwant(u64),
    /// Add one copy of a release to the user's collection (folder 1, Uncategorized). With
    /// `check`, first ask whether it is there already (a retry after an add that may have
    /// gone through), and add nothing if it is: a collection add is never repeated blindly.
    Collect {
        release: u64,
        check: bool,
    },
    /// Take one copy of a release out of the user's collection: the one added last. Asks
    /// Discogs for the copies first (which folder it is in), so a release with no copy left
    /// counts as done and a retry never removes a second copy.
    Discard(u64),
    /// The releases on the user's wantlist (read once a session; `fresh` reads it again).
    ReadWants {
        fresh: bool,
    },
    /// Learn whose token it is (answered with an identity event), when nothing else has.
    Identify,
    /// Bring the user's collection up to date from this cached one (or from nothing). Runs
    /// only when no send is waiting.
    SyncCollection(Option<Box<Collection>>),
    /// Stop the collection sync waiting or running (Stop in the refresh window), and a
    /// friend's collection read; nothing is sent about it, and the previous collection stays.
    CancelCollectionSync,
    /// Read the user's friends list (at a moment with no send), checking whether each
    /// friend's collection is private. Answered with [`Event::Friends`].
    ReadFriends,
    /// Read `user`'s whole collection, a page per step, for crate `target` (Refresh friend).
    /// Answered with [`Event::FriendProgress`] and then [`Event::FriendCollection`].
    ReadFriendCollection {
        target: u64,
        user: String,
    },
    /// End every send into crate `target` now (each answered with `Finished`); what arrived
    /// stays.
    StopJobs(u64),
    /// Learn which release a marketplace item sells (for the browser's owned check). Runs
    /// only when no send is waiting; cached for good.
    ResolveShopItem(u64),
    /// Details of crate `target`'s records saved before entries carried them (the album and
    /// cover), from the disk cache only: never a request.
    Backfill {
        target: u64,
        keys: Vec<RecordKey>,
    },
    /// Fill Top Sellers the first time: the sellers of the user's recent purchases, at a
    /// moment with no send.
    FirstSellers,
    /// How many copies a seller has for sale (matching the search text).
    CountSeller {
        seller: String,
        query: String,
    },
    /// Whether a seller exists (Add seller…), by name: their username and copies for sale.
    LookupSeller(String),
    /// Read a seller's listings page by page for Narrow down (ahead of any send); a new scan
    /// replaces the running one.
    ScanInventory {
        seller: String,
        query: String,
        newest: Option<u32>,
    },
    CancelScan,
    /// Read the user's cart; `soon` puts it ahead of sends (after a cart action or a dig),
    /// otherwise it waits for a moment with no send (at launch).
    ReadCart {
        soon: bool,
    },
    /// Add copies to the cart in one request, then read the cart.
    CartAdd(Vec<u64>),
    /// Take one copy out of the cart, then read the cart.
    CartRemove(u64),
    /// A record's cover address stopped working: fetch its data again for a newer one.
    RefreshCover(RecordKey),
}

/// What became of a listed record.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// One entry per track (its clip, or to be searched for), then the clips matching no
    /// track; possibly none left after a remix-credit filter.
    Entries(RecordPlan),
    /// Unavailable, with the reason: "no clip", "not found".
    Unavailable(String),
}

/// A record's details, as the entries' origin carries them.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordInfo {
    pub key: RecordKey,
    pub release: Option<u64>,
    pub master: Option<u64>,
    pub artist: String,
    pub title: String,
    pub label: String,
    pub catno: String,
    pub year: Option<u16>,
    pub for_sale: Option<ForSale>,
    /// The record's thumbnail address (empty when unknown).
    pub cover: String,
    /// Its styles, comma-separated (empty when unknown).
    pub styles: String,
    /// Its formats, vinyl first (empty when unknown).
    pub formats: Vec<Format>,
}

impl RecordInfo {
    fn from_record(r: &Record) -> Self {
        Self {
            key: r.key,
            release: r.release,
            master: r.master,
            artist: r.artist.clone(),
            title: r.title.clone(),
            label: r.label.clone(),
            catno: r.catno.clone(),
            year: r.year,
            for_sale: r.for_sale.clone(),
            cover: r.cover.clone(),
            styles: r.styles.join(", "),
            formats: r.formats.clone(),
        }
    }

    pub fn from_listed(l: &Listed) -> Self {
        let (release, master) = match l.key {
            RecordKey::Release(id) => (Some(id), None),
            RecordKey::Master(id) => (None, Some(id)),
        };
        Self {
            key: l.key,
            release,
            master,
            artist: l.artist.clone(),
            title: l.title.clone(),
            label: l.label.clone(),
            catno: l.catno.clone(),
            year: l.year,
            for_sale: None,
            cover: l.cover.clone(),
            styles: String::new(),
            formats: l.formats(),
        }
    }
}

/// Which send an event belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRef {
    pub id: JobId,
    pub target: u64,
    /// The page's address (entries' origin).
    pub page: String,
    pub name: String,
    pub filters: Filters,
}

// Events are few (one per record at most), so their size doesn't matter.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A send was accepted (with a provisional name).
    Started(JobRef),
    /// The page's name arrived from Discogs.
    Named(JobRef),
    /// Records from a listing page, in listing order; each becomes a "listed" entry.
    Listed(JobRef, Vec<Listed>),
    /// A seller's copies from a listing page, in order (sent before the page's `Listed`).
    Copies(JobRef, Vec<crate::discogs::seller::Copy>),
    /// A listed record's details.
    Record(JobRef, RecordInfo, Outcome),
    /// Records finished of the listing's total.
    Progress(JobRef, usize, usize),
    Finished(JobRef),
    /// The send stopped (page not found, private, token needed); nothing more comes.
    Failed(JobRef, ApiError),
    /// Discogs stopped (true) or started (false) answering.
    Offline(bool),
    ForSale(u64, ForSale),
    /// The saved token's account, or why it can't be used.
    Identity(Result<Identity, ApiError>),
    TokenChecked(String, Result<Identity, ApiError>),
    /// A collection sync finished: the up-to-date collection, or why not (the previous one
    /// stays in use).
    Collection(Result<Box<Collection>, ApiError>),
    /// A collection sync read `read` of `pages` pages (`pages` 0 until the first answer).
    CollectionProgress {
        read: u32,
        pages: u32,
    },
    /// The friends list, each with whether their collection is private; or why it couldn't
    /// be read (the last list stays).
    Friends(Result<Vec<(String, bool)>, ApiError>),
    /// A friend's collection read for crate `target` got through `read` of `pages` pages.
    FriendProgress {
        target: u64,
        read: u32,
        pages: u32,
    },
    /// A friend's collection, read whole for crate `target`, or why not.
    FriendCollection(u64, Result<Box<Collection>, ApiError>),
    /// A marketplace item's release (`None`: Discogs doesn't know the item).
    ShopItem(u64, Option<u64>),
    /// Cached details for a crate's records (see [`Command::Backfill`]); uncached ones are
    /// left out.
    Backfill(u64, Vec<RecordInfo>),
    /// A record's cover address from fresh data (empty when it has no image any more, or the
    /// data couldn't be fetched).
    Cover(RecordKey, String),
    /// A wantlist change: `Ok(true)` if the app changed the wantlist, `Ok(false)` if the
    /// release was already there.
    Wantlist {
        release: u64,
        add: bool,
        result: Result<bool, ApiError>,
    },
    /// A collection add: the new copy's instance id and its pressing (from the cached release
    /// data), or why not.
    Collected {
        release: u64,
        result: Result<(u64, Pressing), ApiError>,
    },
    /// A collection removal: the copy removed and how many are left, or why not.
    Discarded {
        release: u64,
        result: Result<Discarded, ApiError>,
    },
    /// The releases on the user's wantlist, or why they can't be read.
    Wants(Result<Vec<u64>, ApiError>),
    /// After a collection sync: wanted releases the collection owns (this pressing or another
    /// of the same master), to take off the wantlist.
    OwnedWants(Vec<u64>),
    /// The first Top Sellers list, best first, with each seller's copies for sale.
    FirstSellers(Result<Vec<(Ranked, usize)>, ApiError>),
    SellerCount {
        seller: String,
        query: String,
        result: Result<usize, ApiError>,
    },
    /// Add seller…: the name looked up, and the seller's username and copies for sale.
    SellerLookup {
        name: String,
        result: Result<(String, usize), ApiError>,
    },
    /// A page of a Narrow down scan: every copy on it (the criteria applied here are the
    /// UI's), and how far the scan is.
    Scanned {
        seller: String,
        copies: Vec<Copy>,
        read: u32,
        pages: u32,
        /// Copies a dig could reach at most.
        total: usize,
        done: bool,
    },
    ScanFailed {
        seller: String,
        error: ApiError,
    },
    Cart(Result<CartSnapshot, ApiError>),
    CartAdded {
        listings: Vec<u64>,
        result: Result<Vec<(u64, AddResult)>, ApiError>,
    },
    CartRemoved {
        listing: u64,
        result: Result<(), ApiError>,
    },
}

/// A Narrow down scan in progress.
#[derive(Debug, Clone, PartialEq)]
struct Scan {
    seller: String,
    criteria: Criteria,
    next: u32,
    pages: Option<u32>,
}

/// What a collection removal did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Discarded {
    /// The copy removed; `None` when the collection had no copy of the release.
    pub instance: Option<u64>,
    /// Copies of the release still in the collection.
    pub remaining: usize,
}

pub struct Intake {
    client: Client,
    /// Where `jobs.ron` lives; `None` keeps jobs in memory.
    config: Option<PathBuf>,
    jobs: Jobs,
    /// Jobs whose crate was shown or played this session.
    active: HashSet<JobId>,
    focus: Option<u64>,
    identity_checked: bool,
    /// The user's wantlist this session: release → its master release, when known.
    wants: Option<HashMap<u64, Option<u64>>>,
    offline: bool,
    last_save: Option<Duration>,
    unsaved: bool,
    events: Vec<Event>,
    now: fn() -> u64,
    /// Waiting for a moment with no send: a collection sync, and items to resolve.
    collection_sync: Option<Option<Box<Collection>>>,
    /// The sync running, one page per step.
    syncer: Option<collection::Syncer>,
    /// The friends list is to be read.
    friends_read: bool,
    /// A friend's collection being read: their crate and the read, a page per step.
    friend_read: Option<(u64, collection::Syncer)>,
    shop_items: Vec<u64>,
    first_sellers: bool,
    /// The cart is to be read at a moment with no send (`false`), or ahead of sends (`true`).
    cart_read: Option<bool>,
    scan: Option<Scan>,
}

impl Intake {
    /// Loads the unfinished jobs from `config` (none run until their crate is active).
    pub fn new(client: Client, config: Option<PathBuf>) -> Self {
        let jobs = config.as_deref().map(Jobs::load).unwrap_or_default();
        Self {
            client,
            config,
            jobs,
            active: HashSet::new(),
            focus: None,
            identity_checked: false,
            wants: None,
            offline: false,
            last_save: None,
            unsaved: false,
            events: Vec::new(),
            now: crate::now_secs,
            collection_sync: None,
            syncer: None,
            friends_read: false,
            friend_read: None,
            shop_items: Vec::new(),
            first_sellers: false,
            cart_read: None,
            scan: None,
        }
    }

    /// Epoch seconds for cache freshness, for tests.
    pub fn with_now(mut self, now: fn() -> u64) -> Self {
        self.now = now;
        self
    }

    pub fn jobs(&self) -> &[Job] {
        &self.jobs.jobs
    }

    pub fn is_offline(&self) -> bool {
        self.offline
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    fn job_ref(job: &Job) -> JobRef {
        JobRef {
            id: job.id,
            target: job.target,
            page: job.page.url(),
            name: job.name.clone(),
            filters: job.filters,
        }
    }

    pub fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Send {
                page,
                target,
                filters,
            } => {
                let id = self.jobs.alloc_id();
                let job = Job::new(id, page, target, filters);
                self.events.push(Event::Started(Self::job_ref(&job)));
                self.jobs.jobs.push(job);
                self.active.insert(id);
                self.save();
            }
            Command::CrateActive(target) => {
                for j in &self.jobs.jobs {
                    if j.target == target && self.active.insert(j.id) {
                        self.events.push(Event::Started(Self::job_ref(j)));
                    }
                }
            }
            Command::CrateDeleted(target) => {
                self.jobs.jobs.retain(|j| j.target != target);
                self.save();
            }
            Command::Focus { target, order } => {
                self.focus = Some(target);
                for j in self.jobs.jobs.iter_mut().filter(|j| j.target == target) {
                    reorder(&mut j.pending, &order);
                    vinyl_first(&mut j.pending);
                }
            }
            Command::Refresh(release) => {
                let now = (self.now)();
                self.ensure_identity();
                match expand::record(&mut self.client, RecordKey::Release(release), true, now) {
                    Ok(r) => {
                        self.online();
                        if let Some(fs) = r.for_sale {
                            self.events.push(Event::ForSale(release, fs));
                        }
                    }
                    Err(ApiError::Offline) => self.went_offline(),
                    Err(_) => {}
                }
            }
            Command::SetToken(token) => {
                self.client.set_token(token);
                self.identity_checked = false;
                self.wants = None;
            }
            Command::CheckToken(token) => {
                let saved = self.client.token().map(str::to_owned);
                self.client.set_token(Some(token.clone()));
                let result = self.client.check_identity();
                if result.is_ok() {
                    self.identity_checked = true;
                    self.wants = None;
                } else {
                    self.client.set_token(saved);
                    self.identity_checked = false;
                }
                self.events.push(Event::TokenChecked(token, result));
            }
            Command::Want(release) => self.wantlist(release, true),
            Command::Unwant(release) => self.wantlist(release, false),
            Command::Collect { release, check } => {
                let result = self.collect(release, check);
                match &result {
                    Err(ApiError::Offline) => self.went_offline(),
                    Ok(_) => self.online(),
                    _ => {}
                }
                self.events.push(Event::Collected { release, result });
            }
            Command::Discard(release) => {
                let result = self.discard(release);
                match &result {
                    Err(ApiError::Offline) => self.went_offline(),
                    Ok(_) => self.online(),
                    _ => {}
                }
                self.events.push(Event::Discarded { release, result });
            }
            Command::Identify => self.ensure_identity(),
            Command::ReadWants { fresh } => {
                if fresh {
                    self.wants = None;
                }
                let result = self.ensure_wants().map(|w| {
                    let mut ids: Vec<u64> = w.keys().copied().collect();
                    ids.sort_unstable();
                    ids
                });
                match &result {
                    Err(ApiError::Offline) => self.went_offline(),
                    Ok(_) => self.online(),
                    _ => {}
                }
                self.events.push(Event::Wants(result));
            }
            Command::SyncCollection(cached) => self.collection_sync = Some(cached),
            Command::CancelCollectionSync => {
                self.collection_sync = None;
                self.syncer = None;
                self.friend_read = None;
            }
            Command::ReadFriends => self.friends_read = true,
            Command::ReadFriendCollection { target, user } => {
                let now = (self.now)();
                self.friend_read = Some((target, collection::Syncer::new(&user, None, now)));
            }
            Command::StopJobs(target) => {
                let (stopped, kept) = std::mem::take(&mut self.jobs.jobs)
                    .into_iter()
                    .partition(|j| j.target == target);
                self.jobs.jobs = kept;
                for j in stopped {
                    self.active.remove(&j.id);
                    self.events.push(Event::Finished(Self::job_ref(&j)));
                }
                self.save();
            }
            Command::ResolveShopItem(id) => {
                if !self.shop_items.contains(&id) {
                    self.shop_items.push(id);
                }
            }
            Command::RefreshCover(key) => {
                let now = (self.now)();
                self.ensure_identity();
                let cover = match expand::fresh_cover(&mut self.client, key, now) {
                    Ok(r) => {
                        self.online();
                        if let (Some(release), Some(fs)) = (r.release, r.for_sale.clone()) {
                            self.events.push(Event::ForSale(release, fs));
                        }
                        r.cover
                    }
                    Err(ApiError::Offline) => {
                        self.went_offline();
                        String::new()
                    }
                    Err(_) => String::new(),
                };
                self.events.push(Event::Cover(key, cover));
            }
            Command::FirstSellers => self.first_sellers = true,
            Command::CountSeller { seller, query } => {
                let result = seller::count(&mut self.client, &seller, &query);
                self.track(&result);
                self.events.push(Event::SellerCount {
                    seller,
                    query,
                    result,
                });
            }
            Command::LookupSeller(name) => {
                let result = self.lookup_seller(&name);
                self.track(&result);
                self.events.push(Event::SellerLookup { name, result });
            }
            Command::ScanInventory {
                seller,
                query,
                newest,
            } => {
                self.scan = Some(Scan {
                    seller,
                    criteria: Criteria {
                        query,
                        newest,
                        ..Criteria::default()
                    },
                    next: 1,
                    pages: None,
                });
            }
            Command::CancelScan => self.scan = None,
            Command::ReadCart { soon } => {
                self.cart_read = Some(soon || self.cart_read == Some(true));
            }
            Command::CartAdd(listings) => {
                let result = cart::add(&mut self.client, &listings);
                self.track(&result);
                self.events.push(Event::CartAdded { listings, result });
                self.cart_read = Some(true);
            }
            Command::CartRemove(listing) => {
                let result = cart::remove(&mut self.client, listing);
                self.track(&result);
                self.events.push(Event::CartRemoved { listing, result });
                self.cart_read = Some(true);
            }
            Command::Backfill { target, keys } => {
                let infos: Vec<RecordInfo> = keys
                    .into_iter()
                    .filter_map(|k| expand::cached_record(&self.client.cache, k))
                    .map(|r| RecordInfo::from_record(&r))
                    .collect();
                if !infos.is_empty() {
                    self.events.push(Event::Backfill(target, infos));
                }
            }
        }
    }

    /// Online or offline after a request's result.
    fn track<T>(&mut self, result: &Result<T, ApiError>) {
        match result {
            Ok(_) => self.online(),
            Err(ApiError::Offline) => self.went_offline(),
            Err(_) => {}
        }
    }

    /// A seller's username as Discogs writes it, and their copies for sale.
    fn lookup_seller(&mut self, name: &str) -> Result<(String, usize), ApiError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApiError::NotFound);
        }
        let v = self
            .client
            .get_json(&format!("/users/{}", path_segment(name)))?;
        let username = v["username"].as_str().unwrap_or(name).to_owned();
        let n = seller::count(&mut self.client, &username, "")?;
        Ok((username, n))
    }

    /// One page of the running Narrow down scan.
    fn scan_step(&mut self) {
        let Some(scan) = self.scan.clone() else {
            return;
        };
        let now = (self.now)();
        let result = expand::inventory_listing(
            &mut self.client,
            &scan.seller,
            &scan.criteria,
            true,
            scan.next,
            now,
        );
        match result {
            Ok(lp) => {
                self.online();
                let done = scan.next >= lp.pages;
                self.events.push(Event::Scanned {
                    seller: scan.seller.clone(),
                    copies: lp.copies,
                    read: scan.next,
                    pages: lp.pages,
                    total: lp.total,
                    done,
                });
                self.scan = (!done).then(|| Scan {
                    next: scan.next + 1,
                    pages: Some(lp.pages),
                    ..scan
                });
            }
            Err(ApiError::Offline) => {
                self.went_offline();
                self.client.sleep(OFFLINE_RETRY);
            }
            Err(error) => {
                self.scan = None;
                self.events.push(Event::ScanFailed {
                    seller: scan.seller,
                    error,
                });
            }
        }
    }

    fn read_cart(&mut self) {
        self.cart_read = None;
        let result = cart::read(&mut self.client, (self.now)());
        if matches!(result, Err(ApiError::Offline)) {
            // Tried again at the next moment with no send.
            self.cart_read = Some(false);
        }
        self.track(&result);
        self.events.push(Event::Cart(result));
    }

    /// Work that waits for a moment with no send: an item someone is looking at first, then
    /// the collection, the cart and the first Top Sellers list. False when there was none.
    fn background_step(&mut self) -> bool {
        let now = (self.now)();
        if self.cart_read.is_some() && !self.offline {
            self.read_cart();
            return true;
        }
        if self.first_sellers {
            let result = seller::first_list(&mut self.client);
            self.track(&result);
            if !matches!(result, Err(ApiError::Offline)) {
                self.first_sellers = false;
            } else {
                self.client.sleep(OFFLINE_RETRY);
            }
            self.events.push(Event::FirstSellers(result));
            return true;
        }
        if std::mem::take(&mut self.friends_read) {
            self.read_friends();
            return true;
        }
        if let Some((target, syncer)) = &mut self.friend_read {
            let target = *target;
            let event = match syncer.step(&mut self.client) {
                Ok(None) => {
                    let (read, pages) = syncer.progress();
                    Event::FriendProgress {
                        target,
                        read,
                        pages,
                    }
                }
                done => {
                    self.friend_read = None;
                    Event::FriendCollection(target, done.map(|c| Box::new(c.expect("read"))))
                }
            };
            self.events.push(event);
            return true;
        }
        if let Some(id) = self.shop_items.first().copied() {
            match expand::shop_item_release(&mut self.client, id, now) {
                Err(ApiError::Offline) => {
                    self.went_offline();
                    self.client.sleep(OFFLINE_RETRY);
                    return true;
                }
                r => {
                    self.online();
                    self.shop_items.remove(0);
                    self.events.push(Event::ShopItem(id, r.ok()));
                }
            }
            return true;
        }
        if self.syncer.is_none() {
            let Some(cached) = self.collection_sync.take() else {
                return false;
            };
            self.ensure_identity();
            let Some(user) = self.client.identity().map(|i| i.username.clone()) else {
                self.events
                    .push(Event::Collection(Err(ApiError::TokenNeeded)));
                return true;
            };
            self.syncer = Some(collection::Syncer::new(&user, cached.as_deref(), now));
        }
        let syncer = self.syncer.as_mut().expect("a sync");
        let result = match syncer.step(&mut self.client) {
            Ok(None) => {
                let (read, pages) = syncer.progress();
                self.online();
                self.events.push(Event::CollectionProgress { read, pages });
                return true;
            }
            Ok(Some(c)) => Ok(c),
            Err(e) => Err(e),
        };
        self.syncer = None;
        match &result {
            Ok(_) => self.online(),
            Err(ApiError::Offline) => self.went_offline(),
            Err(_) => {}
        }
        // Nothing owned stays wanted: the wantlist is read (once a session) to find them.
        let owned = match &result {
            Ok(c) => self.ensure_wants().ok().map(|w| {
                let mut ids: Vec<u64> = w
                    .iter()
                    .filter(|&(&r, &m)| c.owned(Some(r), m).is_some())
                    .map(|(&r, _)| r)
                    .collect();
                ids.sort_unstable();
                ids
            }),
            Err(_) => None,
        };
        self.events.push(Event::Collection(result.map(Box::new)));
        if let Some(ids) = owned.filter(|ids| !ids.is_empty()) {
            self.events.push(Event::OwnedWants(ids));
        }
        true
    }

    /// The friends list, and whether each friend's collection is private (one small request
    /// each, once a week).
    fn read_friends(&mut self) {
        self.ensure_identity();
        let Some(user) = self.client.identity().map(|i| i.username.clone()) else {
            self.events.push(Event::Friends(Err(ApiError::TokenNeeded)));
            return;
        };
        let result = friends::read(&mut self.client, &user).and_then(|names| {
            names
                .into_iter()
                .map(|n| Ok((n.clone(), friends::is_private(&mut self.client, &n)?)))
                .collect()
        });
        self.track(&result);
        self.events.push(Event::Friends(result));
    }

    /// One request's worth of work. False when there is nothing to do.
    pub fn step(&mut self) -> bool {
        // What the user waits for in a dialog, and the cart after an action, go first.
        if self.scan.is_some() {
            self.scan_step();
            return true;
        }
        if self.cart_read == Some(true) {
            self.read_cart();
            return true;
        }
        let Some(i) = self.next_job() else {
            self.save_if_due(true);
            return self.background_step();
        };
        let now = (self.now)();
        let job = &self.jobs.jobs[i];
        let result = if job.subject.is_none() {
            self.name_job(i, now)
        } else if !job.listing_done() {
            self.list_page(i, now)
        } else {
            self.expand_next(i, now)
        };
        match result {
            Ok(()) => self.online(),
            Err(ApiError::Offline) => {
                self.went_offline();
                self.save_if_due(true);
                self.client.sleep(OFFLINE_RETRY);
            }
            Err(e) => {
                let job = self.jobs.jobs.remove(i);
                self.events.push(Event::Failed(Self::job_ref(&job), e));
                self.save();
            }
        }
        if let Some(i) = self.jobs.jobs.iter().position(Job::is_finished) {
            let job = self.jobs.jobs.remove(i);
            self.events.push(Event::Finished(Self::job_ref(&job)));
            self.save();
        }
        self.save_if_due(false);
        true
    }

    /// Unfinished active jobs: the focused crate's first, then the oldest.
    fn next_job(&self) -> Option<usize> {
        let runnable = |j: &&Job| self.active.contains(&j.id) && !j.is_finished();
        // Every listing before any details, so a big page shows all its records early.
        let listing = self
            .jobs
            .jobs
            .iter()
            .position(|j| runnable(&j) && (j.subject.is_none() || !j.listing_done()));
        listing.or_else(|| {
            self.jobs
                .jobs
                .iter()
                .position(|j| runnable(&j) && Some(j.target) == self.focus)
                .or_else(|| self.jobs.jobs.iter().position(|j| runnable(&j)))
        })
    }

    fn name_job(&mut self, i: usize, now: u64) -> Result<(), ApiError> {
        let page = self.jobs.jobs[i].page.clone();
        if matches!(
            page.kind,
            PageKind::Release(_) | PageKind::Master(_) | PageKind::ShopItem(_)
        ) {
            self.ensure_identity();
        }
        let name = expand::page_name(&mut self.client, &page, now)?;
        let prefix = format!("{}: ", page.kind_name());
        let job = &mut self.jobs.jobs[i];
        job.subject = Some(name.strip_prefix(&prefix).unwrap_or(&name).to_owned());
        job.name = name;
        self.events.push(Event::Named(Self::job_ref(job)));
        Ok(())
    }

    fn list_page(&mut self, i: usize, now: u64) -> Result<(), ApiError> {
        let (page, n) = {
            let j = &self.jobs.jobs[i];
            (j.page.clone(), j.next_page)
        };
        let lp = match expand::listing(&mut self.client, &page, n, now) {
            Ok(lp) => lp,
            // A later page gone missing (the listing shrank): the listing is done.
            Err(ApiError::NotFound) if n > 1 => expand::ListingPage {
                items: Vec::new(),
                copies: Vec::new(),
                pages: n - 1,
                total: self.jobs.jobs[i].total,
            },
            Err(e) => return Err(e),
        };
        let job = &mut self.jobs.jobs[i];
        job.pages = Some(lp.pages);
        job.next_page = n + 1;
        // A listing can give a record more than once (a label credited twice on it): it
        // comes in once.
        let mut seen: HashSet<RecordKey> = job.pending.iter().map(|l| l.key).collect();
        let (keep, repeats): (Vec<Listed>, Vec<Listed>) =
            lp.items.into_iter().partition(|l| seen.insert(l.key));
        job.repeats += repeats.len();
        job.total = lp.total.saturating_sub(job.repeats);
        job.pending.extend(keep.iter().cloned());
        vinyl_first(&mut job.pending);
        let r = Self::job_ref(job);
        let (done, total) = (job.done, job.total);
        if !lp.copies.is_empty() {
            self.events.push(Event::Copies(r.clone(), lp.copies));
        }
        if !keep.is_empty() {
            self.events.push(Event::Listed(r.clone(), keep));
        }
        self.events.push(Event::Progress(r, done, total));
        self.save();
        Ok(())
    }

    fn expand_next(&mut self, i: usize, now: u64) -> Result<(), ApiError> {
        self.ensure_identity();
        let (listed, subject) = {
            let j = &self.jobs.jobs[i];
            (j.pending[0].clone(), j.subject.clone().unwrap_or_default())
        };
        let (info, outcome) = match expand::record(&mut self.client, listed.key, false, now) {
            Ok(rec) => {
                let info = RecordInfo::from_record(&rec);
                let artist = if listed.role == Role::Remix {
                    subject.as_str()
                } else {
                    ""
                };
                let plan = matching::plan(&rec, listed.role, artist);
                let outcome =
                    if plan.is_empty() && (rec.clips.is_empty() || listed.role == Role::Main) {
                        Outcome::Unavailable("no clip".into())
                    } else {
                        Outcome::Entries(plan)
                    };
                (info, outcome)
            }
            Err(ApiError::NotFound) => (
                RecordInfo::from_listed(&listed),
                Outcome::Unavailable("not found".into()),
            ),
            Err(ApiError::Other(e)) => (
                RecordInfo::from_listed(&listed),
                Outcome::Unavailable(format!("failed: {e}")),
            ),
            Err(e) => return Err(e),
        };
        let job = &mut self.jobs.jobs[i];
        job.pending.remove(0);
        job.done += 1;
        self.unsaved = true;
        let r = Self::job_ref(job);
        let (done, total) = (job.done, job.total);
        self.events.push(Event::Record(r.clone(), info, outcome));
        self.events.push(Event::Progress(r, done, total));
        Ok(())
    }

    /// With a token, the account's currency is needed before any price is asked for.
    fn ensure_identity(&mut self) {
        if self.identity_checked || self.client.token().is_none() {
            return;
        }
        match self.client.check_identity() {
            Ok(id) => {
                self.identity_checked = true;
                self.events.push(Event::Identity(Ok(id)));
            }
            Err(ApiError::Offline) => {}
            Err(e) => {
                // A bad token: carry on as without one, at the lower limit, and say so.
                self.identity_checked = true;
                self.client.set_token(None);
                self.events.push(Event::Identity(Err(e)));
            }
        }
    }

    fn wantlist(&mut self, release: u64, add: bool) {
        let result = self.change_wantlist(release, add);
        match &result {
            Err(ApiError::Offline) => self.went_offline(),
            Ok(_) => self.online(),
            _ => {}
        }
        self.events.push(Event::Wantlist {
            release,
            add,
            result,
        });
    }

    /// The account's username, for a change to it (a token is needed).
    fn user(&mut self) -> Result<String, ApiError> {
        if self.client.token().is_none() {
            return Err(ApiError::TokenNeeded);
        }
        let user = self.client.ensure_identity()?.username;
        self.identity_checked = true;
        Ok(user)
    }

    /// The user's wantlist, read once a session.
    fn ensure_wants(&mut self) -> Result<&HashMap<u64, Option<u64>>, ApiError> {
        if self.wants.is_none() {
            let user = self.user()?;
            self.wants = Some(self.read_wants(&user)?);
        }
        Ok(self.wants.get_or_insert_default())
    }

    fn change_wantlist(&mut self, release: u64, add: bool) -> Result<bool, ApiError> {
        let user = self.user()?;
        self.ensure_wants()?;
        let path = format!("/users/{}/wants/{release}", path_segment(&user));
        if add {
            if self
                .wants
                .as_ref()
                .is_some_and(|w| w.contains_key(&release))
            {
                return Ok(false);
            }
            self.client.call(Method::Put, &path)?;
            let master = expand::cached_record(&self.client.cache, RecordKey::Release(release))
                .and_then(|r| r.master);
            self.wants.get_or_insert_default().insert(release, master);
        } else {
            match self.client.call(Method::Delete, &path) {
                Ok(_) | Err(ApiError::NotFound) => {}
                Err(e) => return Err(e),
            }
            self.wants.get_or_insert_default().remove(&release);
        }
        Ok(true)
    }

    /// One copy of `release` in the collection: its instance id and pressing.
    fn collect(&mut self, release: u64, check: bool) -> Result<(u64, Pressing), ApiError> {
        let user = self.user()?;
        let pressing = expand::cached_record(&self.client.cache, RecordKey::Release(release))
            .map(|r| Pressing {
                master: r.master,
                catno: r.catno,
                year: r.year,
            })
            .unwrap_or(Pressing {
                master: None,
                catno: String::new(),
                year: None,
            });
        if check {
            let v = self.client.get_json(&format!(
                "/users/{}/collection/releases/{release}",
                path_segment(&user)
            ))?;
            let newest = v["releases"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|i| i["instance_id"].as_u64())
                .max();
            if let Some(instance) = newest {
                return Ok((instance, pressing));
            }
        }
        let path = format!(
            "/users/{}/collection/folders/1/releases/{release}",
            path_segment(&user)
        );
        let r = self.client.call(Method::Post, &path)?;
        let v: serde_json::Value =
            serde_json::from_str(&r.body).map_err(|e| ApiError::Other(format!("{path}: {e}")))?;
        let instance = v["instance_id"]
            .as_u64()
            .ok_or_else(|| ApiError::Other(format!("{path}: no instance_id")))?;
        Ok((instance, pressing))
    }

    /// One copy of `release` out of the collection, the one added last (by date added, then
    /// instance id): one request for its copies, one to remove it.
    fn discard(&mut self, release: u64) -> Result<Discarded, ApiError> {
        let user = self.user()?;
        let v = match self.client.get_json(&format!(
            "/users/{}/collection/releases/{release}",
            path_segment(&user)
        )) {
            Ok(v) => v,
            Err(ApiError::NotFound) => serde_json::Value::Null,
            Err(e) => return Err(e),
        };
        let copies: Vec<(String, u64, u64)> = v["releases"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                Some((
                    c["date_added"].as_str().unwrap_or_default().to_owned(),
                    c["instance_id"].as_u64()?,
                    c["folder_id"].as_u64()?,
                ))
            })
            .collect();
        let Some((_, instance, folder)) = copies.iter().max().cloned() else {
            return Ok(Discarded {
                instance: None,
                remaining: 0,
            });
        };
        let path = format!(
            "/users/{}/collection/folders/{folder}/releases/{release}/instances/{instance}",
            path_segment(&user)
        );
        match self.client.call(Method::Delete, &path) {
            Ok(_) | Err(ApiError::NotFound) => {}
            Err(e) => return Err(e),
        }
        Ok(Discarded {
            instance: Some(instance),
            remaining: copies.len() - 1,
        })
    }

    /// The user's wantlist, with each release's master, once per session (100 a request).
    fn read_wants(&mut self, user: &str) -> Result<HashMap<u64, Option<u64>>, ApiError> {
        let mut ids = HashMap::new();
        let mut n = 1;
        loop {
            let v = self.client.get_json(&format!(
                "/users/{}/wants?page={n}&per_page={PER_PAGE}",
                path_segment(user)
            ))?;
            ids.extend(v["wants"].as_array().into_iter().flatten().filter_map(|w| {
                let master = w["basic_information"]["master_id"]
                    .as_u64()
                    .filter(|&m| m > 0);
                Some((w["id"].as_u64()?, master))
            }));
            if n >= v["pagination"]["pages"].as_u64().unwrap_or(1) {
                return Ok(ids);
            }
            n += 1;
        }
    }

    fn went_offline(&mut self) {
        if !self.offline {
            self.offline = true;
            self.events.push(Event::Offline(true));
        }
    }

    fn online(&mut self) {
        if self.offline {
            self.offline = false;
            self.events.push(Event::Offline(false));
        }
    }

    fn save(&mut self) {
        self.unsaved = false;
        self.last_save = Some(self.client.clock().now());
        if let Some(c) = &self.config {
            let _ = self.jobs.save(c);
        }
    }

    fn save_if_due(&mut self, force: bool) {
        let due = self
            .last_save
            .is_none_or(|t| self.client.clock().now() >= t + SAVE_EVERY);
        if self.unsaved && (force || due) {
            self.save();
        }
    }

    /// Writes whatever is unsaved (the thread does this before it ends).
    pub fn flush(&mut self) {
        self.save_if_due(true);
    }
}

/// Moves the records named in `order` to the front, in that order; the rest keep theirs.
fn reorder(pending: &mut Vec<Listed>, order: &[RecordKey]) {
    let mut front = Vec::with_capacity(pending.len());
    for key in order {
        if let Some(i) = pending.iter().position(|l| l.key == *key) {
            front.push(pending.remove(i));
        }
    }
    front.append(pending);
    *pending = front;
}

/// A listed record's catalog number and title, to pair a release with its twin in another
/// format; `None` when either is missing.
fn twin_key(l: &Listed) -> Option<(String, String)> {
    let catno = l.catno.trim().to_lowercase();
    let title = l.title.trim().to_lowercase();
    (!catno.is_empty() && !title.is_empty()).then_some((catno, title))
}

/// Moves the vinyl release of a record up to just before its non-vinyl twin (same catalog
/// number and title) when the twin comes first, so the vinyl release is fetched first and
/// brings the tunes. The rest keep their order.
fn vinyl_first(pending: &mut Vec<Listed>) {
    let twin_of_vinyl: HashSet<(String, String)> = pending
        .iter()
        .filter(|l| l.is_vinyl())
        .filter_map(twin_key)
        .collect();
    if twin_of_vinyl.is_empty() {
        return;
    }
    let mut slots: Vec<Option<Listed>> = pending.drain(..).map(Some).collect();
    let mut out = Vec::with_capacity(slots.len());
    for i in 0..slots.len() {
        let Some(l) = slots[i].take() else { continue };
        if let Some(k) = twin_key(&l).filter(|k| !l.is_vinyl() && twin_of_vinyl.contains(k)) {
            for later in slots[i + 1..].iter_mut() {
                if later
                    .as_ref()
                    .is_some_and(|v| v.is_vinyl() && twin_key(v).as_ref() == Some(&k))
                {
                    out.extend(later.take());
                }
            }
        }
        out.push(l);
    }
    *pending = out;
}

/// The worker thread's end of the channels.
pub struct IntakeHandle {
    commands: Sender<Command>,
    events: Receiver<Event>,
}

impl IntakeHandle {
    /// Runs `intake` on a low-priority thread; `wake` is called when events are ready.
    pub fn start(
        spawner: &dyn Spawner,
        mut intake: Intake,
        wake: impl Fn() + Send + 'static,
    ) -> Result<Self, platform::PlatformError> {
        let (cmd_tx, cmd_rx) = channel::<Command>();
        let (ev_tx, ev_rx) = channel();
        spawner.spawn(
            "dig-intake",
            Priority::Low,
            Box::new(move || {
                loop {
                    loop {
                        match cmd_rx.try_recv() {
                            Ok(c) => intake.handle(c),
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => return intake.flush(),
                        }
                    }
                    let worked = intake.step();
                    let events = intake.take_events();
                    if !events.is_empty() {
                        for e in events {
                            if ev_tx.send(e).is_err() {
                                return intake.flush();
                            }
                        }
                        wake();
                    }
                    if !worked {
                        match cmd_rx.recv() {
                            Ok(c) => intake.handle(c),
                            Err(_) => return intake.flush(),
                        }
                    }
                }
            }),
        )?;
        Ok(Self {
            commands: cmd_tx,
            events: ev_rx,
        })
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.commands.send(cmd);
    }

    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discogs::model::Format;

    fn listed(id: u64, catno: &str, title: &str, format: Option<Format>) -> Listed {
        Listed {
            catno: catno.into(),
            title: title.into(),
            formats: format.into_iter().collect(),
            ..Listed::new(RecordKey::Release(id))
        }
    }

    fn ids(pending: &[Listed]) -> Vec<RecordKey> {
        pending.iter().map(|l| l.key).collect()
    }

    #[test]
    fn a_digital_twin_waits_for_its_vinyl_release() {
        use Format::*;
        let mut pending = vec![
            listed(1, "AF060LP", "Sixty", Some(File)),
            listed(2, "AF059", "Other", Some(File)),
            listed(3, "AF060LP ", "SIXTY", Some(Vinyl)),
            listed(4, "AF058", "Third", Some(Vinyl)),
            listed(5, "AF057", "Fourth", Some(File)),
        ];
        vinyl_first(&mut pending);
        use RecordKey::Release as R;
        assert_eq!(
            ids(&pending),
            [R(3), R(1), R(2), R(4), R(5)],
            "the vinyl comes up to just before its FLAC; digital-only ones keep their place"
        );
        // Already in order: unchanged.
        let before = ids(&pending);
        vinyl_first(&mut pending);
        assert_eq!(ids(&pending), before);
    }

    #[test]
    fn the_focus_keeps_the_vinyl_ahead_of_its_twin() {
        use Format::*;
        let mut pending = vec![
            listed(3, "AF060LP", "Sixty", Some(Vinyl)),
            listed(1, "AF060LP", "Sixty", Some(File)),
            listed(9, "X1", "Elsewhere", Some(Vinyl)),
        ];
        // The user is on the FLAC: its vinyl twin is fetched first, right there.
        reorder(
            &mut pending,
            &[RecordKey::Release(1), RecordKey::Release(9)],
        );
        vinyl_first(&mut pending);
        use RecordKey::Release as R;
        assert_eq!(ids(&pending), [R(3), R(1), R(9)]);
    }
}
