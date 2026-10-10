## MODIFIED Requirements

### Requirement: Preview cache
Previews SHALL be stored in `previews/` in the cache folder. They SHALL use at most the preview cache size, which is 2 GB by default and set in Options ▸ Discogs…. When a new preview doesn't fit, the previews played least recently SHALL be deleted first. A preview downloaded for Download all tracks SHALL never cause a deletion: downloading pauses instead (see `label-crates`). The previews of the playing entry, the armed entry and the next 3 entries SHALL never be deleted. A deleted preview SHALL be downloaded again when it is needed, whether or not its crate was open when it was deleted, and also when its file is missing for any other reason (such as a cache folder from an older version). Such an entry SHALL go back to waiting for its download, never to failed. When the downloaded file is identical, its cached analysis and waveform SHALL be reused.

#### Scenario: Over the limit
- **WHEN** a new preview would take the cache over its limit
- **THEN** the least recently played previews are deleted until it fits, and the playing, armed and next 3 previews are kept

#### Scenario: Downloaded again
- **WHEN** a preview that was deleted is downloaded again as an identical file
- **THEN** it starts with its whole waveform and sections, without being analyzed again

#### Scenario: Missing file downloads again
- **WHEN** a collection-crate track whose preview file no longer exists is played or its details are read
- **THEN** the track waits for its preview again (not red, not failed), its preview is downloaded, and it plays
