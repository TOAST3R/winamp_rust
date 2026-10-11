## Why

Your Discogs friends' collections are a good place to dig: records a person you trust actually owns. Today the only way in is to paste each friend's `/user/‹name›/collection` page by hand, and nothing keeps a list of them. Discogs returns the user's friends to their token (`GET /users/‹you›/friends`, which the developer docs don't list but which answers today: 12 friends for the current user, 7 of them with a readable collection).

## What Changes

- A **FRIENDS** group in the crate sidebar, under TOP SELLERS, with a crate per friend named "Friend: ‹username›". The list comes from Discogs; there is no button to add or remove friends.
- The friends list is read with the token the first time it's needed and then **once a week**. If the request fails or the endpoint goes away, the last list stays and the group says why it can't refresh.
- A friend's crate holds **their collection**, one row per record (grouped like your own collection crate), with the OWNED badge on the records you already have. It isn't a seller crate: no prices, SOLD, Narrow down or copy rows.
- **Click shows, double-click digs**, as for sellers. A click shows what's saved without any request. A double-click on a never-dug crate sends the friend's collection page. A dug crate is dug again only from **Refresh friend**.
- **Refresh friend** on the crate's right-click reads the friend's collection again: new records come in, records they no longer have leave. It uses the same progress window with Stop as Refresh collection.
- A friend whose collection is **private** (Discogs answers 403) is shown dimmed, with the tooltip "Private collection", and can't be dug.
- When someone is no longer your friend on Discogs, their crate leaves FRIENDS and stays as an ordinary crate with its records.

## Capabilities

### New Capabilities
- `friend-crates`: the friends list from Discogs, a crate per friend filled from their collection, dig, refresh, private collections, and friends who leave.

### Modified Capabilities
- `crates`: the sidebar gets a FRIENDS group under TOP SELLERS (heading, fold, count, dimmed private crates).

## Impact

- `crates/dig`: a request for the friends list (paginated, token needed), a `friends.ron` in the config folder (list, last read, folded, private flags), worker commands and events for reading the list and refreshing a friend.
- `crates/ui`: `CrateInfo` gains a `friend` username (like `seller`), the sidebar group, click/double-click/Refresh friend handling in `app/digging.rs`, reusing the collection page send (`PageKind::Collection(user)`) and the refresh progress window.
- No change to playback, analysis or the visuals. Requests go through the existing rate limiter at Discogs' 60 a minute.
- `README.md`: a Friends section; the test count.
