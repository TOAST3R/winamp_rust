## MODIFIED Requirements

### Requirement: Search is live and combines with the filters
Every change to the search text SHALL update the list, the title bar's shown/all count, what plays next, shuffle, the pre-warmed track and the previews downloaded ahead within one frame (16 ms) for a crate of 5,000 entries, SHALL NOT interrupt playback, and SHALL cause zero underruns. Starting a search, meaning the search text going from empty to non-empty (typed, pasted, or set by "Search ‹label›" / "Search ‹catalogue number›"), SHALL clear the BPM range and every record filter (style, artist, label, format), so the search covers the whole crate. The CART switch SHALL be left as it is. Clearing the search SHALL NOT bring the cleared filters back. A filter set while the search is non-empty SHALL narrow the shown entries: an entry SHALL be shown when it matches the search and passes every filter that is set (see `record-filters` "Filters combine"). The playing entry SHALL keep playing if the search hides it.

#### Scenario: Typing
- **WHEN** a track plays in a grouped collection crate of 5,000 entries and the user types "deep" one letter at a time
- **THEN** after each letter the list shows the matching entries within 16 ms, and playback has zero underruns

#### Scenario: Next under a search
- **WHEN** the search is "parrish" and a Theo Parrish track ends, followed in crate order by a Nightcraft record and then a Theo Parrish record
- **THEN** the next Theo Parrish record's first track plays next

#### Scenario: Starting a search clears the filters
- **WHEN** the BPM range is 92–171, the label Lowtide Tapes is picked, the search is empty, and the user types "c"
- **THEN** the BPM range covers every tempo, no label is picked, and every entry matching "c" is shown

#### Scenario: Filters set during a search
- **WHEN** the search is "parrish" and the user then sets the BPM range to 120–125
- **THEN** only Theo Parrish entries at 120 to 125 BPM are shown, and the title bar shows the shown and total counts

#### Scenario: Editing a search keeps the filters
- **WHEN** the search is "parrish", a BPM range of 120–125 was set after it, and the user types " summer"
- **THEN** the BPM range is still 120–125

#### Scenario: Cart left alone
- **WHEN** the CART switch is on in a seller crate and the user starts a search
- **THEN** the CART switch is still on

## ADDED Requirements

### Requirement: Search field draws its text once
The search field's text, its caret and its selection SHALL be drawn only in the skin's font. Selected characters SHALL be drawn as a block in the skin's LCD colour with the characters in the field's background colour, and no other font's glyphs SHALL appear in the field, whether text is selected or not.

#### Scenario: Select all
- **WHEN** the search is "vigne" and the user presses Cmd+A in the field
- **THEN** the field shows "VIGNE" once, in the skin font, as dark characters on an LCD-coloured block, and no smaller "vigne" is drawn

#### Scenario: Replace the selection
- **WHEN** the whole search is selected and the user types "a"
- **THEN** the field reads "A" with the caret after it, and nothing is highlighted
