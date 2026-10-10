## MODIFIED Requirements

### Requirement: Maximized playlist
A toggle SHALL maximize the playlist: the ⇔ button in the playlist's title bar, or Shift+P. The title bar SHALL hold, from the right: the close button, ⇔, and the ▤ button that groups the shown crate by record (see `record-view`), all at the classic 275 skin pixel width. While maximized:
- the window SHALL fill the screen's usable area (below the menu bar), as the operating system maximizes it;
- a band as tall as the waveform section (58 skin pixels) SHALL run across the top of the window. On its left, at the main window's width (275 skin pixels), it SHALL hold a mini player showing the play state, the elapsed time, the playing entry's title, previous, play or pause, stop, next, a volume slider and the ⇔ button. The waveform SHALL fill the rest of the band when it is on (see `waveform-view`); when it is off, the rest SHALL be skin panel. The band SHALL be shown whether the waveform is on or off;
- the player SHALL NOT be drawn as a strip on the left;
- the playlist SHALL take the full window width under the band, its width and rows following the window, with columns when it is wide enough;
- the EQ SHALL be hidden, and the keyboard SHALL go to the playlist.

Toggling again SHALL restore the previous window frame, layout, playlist width and rows exactly. When the playlist is hidden, the toggle SHALL show it first. The mode SHALL be remembered across launches, and a launch in this mode SHALL stay within the 300 ms launch target. Entering or leaving the mode SHALL NOT affect playback.

#### Scenario: Maximize
- **WHEN** the window is 1100 × 580 points at 2× on a 1440 × 900 display, and the user presses Shift+P
- **THEN** the window fills the area below the menu bar, the mini player is at the top left, the waveform at the top right, and the playlist fills the full width under them, with columns

#### Scenario: Restore
- **WHEN** the user clicks ⇔ while maximized
- **THEN** the window returns to 1100 × 580 points at its previous position, with the player column and the playlist at their previous width and rows

#### Scenario: Mini player controls
- **WHEN** the playlist is maximized and the user clicks next in the mini player
- **THEN** the next track starts, exactly as with B

#### Scenario: Waveform off
- **WHEN** the playlist is maximized and the waveform is hidden with W
- **THEN** the mini player is still shown at the top left, the rest of the band is skin panel, and the playlist keeps the same rows

#### Scenario: Playlist width
- **WHEN** the playlist is maximized in a window 1100 skin pixels wide
- **THEN** the playlist is 1100 skin pixels wide, and its rows fill the height under the 58-pixel band

#### Scenario: Remembered
- **WHEN** the app is quit while maximized and launched again
- **THEN** its first frame is already maximized, within 300 ms of launch

#### Scenario: Title bar buttons
- **WHEN** the playlist is shown at 275 skin pixels wide
- **THEN** its title bar shows ▤, ⇔ and the close button at its right end, and the crate name is shortened to fit before them

### Requirement: Playlist footer
The playlist footer SHALL hold, from left to right: a `+` button, a `≡` button, a gear button, and the "selected/total" time readout, right-aligned in its LCD box beside the resize grip (only the total when both don't fit in the box). The footer SHALL hold no filter control (see `filter-bar`). Everything SHALL fit at the classic 275 skin pixel width. The buttons SHALL open menus:
- `+`: Add files…, Add folder…, Import M3U…;
- `≡`: Select all, Select none, Invert selection, Remove selected, Clear crate, Sort ▸ (every column's field), Group by record (a checkbox, see `record-view`), Export M3U…;
- the gear: the Options menu (see the player window's Options menu), with the same items as the right-click.

Each item SHALL act as the same item did in the footer's earlier menus. No app setting SHALL be in the `+` and `≡` menus.

#### Scenario: Three buttons
- **WHEN** the playlist is shown at 275 skin pixels wide
- **THEN** the footer shows `+`, `≡`, the gear and the time readout, and no ADD, REM, SEL, MISC or OPT button

#### Scenario: No filters in the footer
- **WHEN** the collection crate, with tempos and styles, is shown 700 pixels wide
- **THEN** the footer shows `+`, `≡`, the gear and the time readout only, and the BPM control and style chips are in the filter bar

#### Scenario: Options from the footer
- **WHEN** the user clicks the gear and chooses Discogs…
- **THEN** the Discogs dialog opens, as from the right-click Options menu

#### Scenario: Add
- **WHEN** the user clicks `+` and chooses Add folder…
- **THEN** the folder dialog opens, as ADD ▸ Add folder… did

#### Scenario: Sort from the footer
- **WHEN** the user clicks `≡` and chooses Sort ▸ BPM
- **THEN** the crate is sorted by BPM, as by clicking the BPM column header

#### Scenario: Group from the menu
- **WHEN** the user clicks `≡` and ticks Group by record
- **THEN** the shown crate is grouped, as with ▤
