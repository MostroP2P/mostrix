# Proposal: Improved Keybindings for Mostrix TUI

## Motivation

Mostrix currently lacks several standard TUI keybindings that users expect from
tools like lazydocker, bluetui, and htop. There is no quick way to quit (`q`),
no visible keybinding hints, and `Tab` does not cycle between tabs as users
would expect. This makes the TUI harder to use without reading documentation
first.

This proposal introduces keybinding improvements in three phases, ordered by
impact and implementation complexity.

---

## TUI Conventions and Prior Art

Most popular terminal applications share a common set of keybinding conventions.
Users develop muscle memory around these patterns, and deviating from them
creates friction. Below is a comparison of how widely-used TUIs handle the same
interactions that Mostrix needs:

### `q` to quit

Virtually every TUI uses `q` to quit or initiate an exit flow:

| Application | Quit key |
|-------------|----------|
| htop | `q` |
| lazydocker | `q` |
| bluetui | `q` |
| k9s | `:q` |
| vim/less/man | `q` |
| tig (git TUI) | `q` |

Mostrix currently requires navigating to the Exit tab and pressing Enter, which
is a multi-step process that no other TUI uses. Even applications that want a
safety net (like vim with unsaved changes) show a confirmation prompt rather
than requiring navigation to an exit screen.

### `Tab` to switch tabs/panels

`Tab` is the standard key for cycling between panels, panes, or tabs:

| Application | Tab behavior |
|-------------|--------------|
| bluetui | `Tab` switches between Paired Devices and Adapter panels |
| lazydocker | `Tab` cycles between panels (Services, Containers, etc.) |
| htop | `Tab` switches between tree/list view |
| tmux | `Ctrl+B` then `n`/`p` for next/prev window |
| Firefox/Chrome | `Ctrl+Tab` for next tab |

Arrow keys (`Left`/`Right`) are typically reserved for **within-content
navigation** (e.g., moving a cursor in a text field, expanding/collapsing tree
nodes), not for top-level tab switching. Using arrows exclusively for tab
switching means they cannot be reused for content-level horizontal navigation in
the future without creating ambiguity.

### `?` for help

`?` is the de facto standard for opening help in TUI applications:

| Application | Help key |
|-------------|----------|
| vim | `:help` or `?` (reverse search, but `F1` for help) |
| less/man | `h` or `?` |
| htop | `F1` or `?` |
| tig | `?` |
| lazydocker | `?` |
| k9s | `?` |

`Ctrl+H` is uncommon as a help key in major TUIs. While it works, users
will not discover it without reading documentation. `?` is what users try first
instinctively.

### Visible keybinding hints

Modern TUIs show available shortcuts directly in the interface, typically in a
footer bar. This eliminates the need to memorize keybindings or consult a help
screen:

| Application | Hint style |
|-------------|------------|
| bluetui | Footer: `k,Up \| j,Down \| s Scan on/off \| u Unpair \| ...` |
| lazydocker | Footer: `scroll, b: view bulk commands, q: quit, x: menu` |
| htop | Function key bar: `F1Help F2Setup F3Search ...` |
| midnight commander | Function key bar at bottom |

Mostrix only shows footer hints in the Disputes In Progress tab. All other tabs
have no visible keybinding information, leaving users unaware of available
actions.

### `Shift+Key` for destructive or mode-switching actions

Using modifier keys for potentially disruptive actions prevents accidental
activation:

| Application | Example |
|-------------|---------|
| vim | `Shift+Z Shift+Z` to save and quit |
| lazydocker | `Shift+D` to remove container |
| bluetui | `Shift` combinations for dangerous operations |

Mostrix currently uses bare `m` (no modifier) to switch between User and Admin
mode, which changes the entire tab set. This should require `Shift+M` to
prevent accidental mode switches.

---

## Current State

### Tab switching
- `Left`/`Right` arrows: switch between tabs
- `Tab`/`BackTab`: **do not** switch tabs in general; only used for
  party-switching in Disputes In Progress and field navigation in Create New
  Order

### Help
- `Ctrl+H`: opens context-aware help popup (non-standard, hard to discover)

### Exiting
- Navigate to the Exit tab, press Enter, select Yes (3+ steps)
- `q` was previously available but was removed

### Mode switch (User/Admin)
- Both `m` and `M` trigger mode switch in the Settings tab; lowercase `m` is
  too easy to hit accidentally

### No visible keybinding hints
- Only the Disputes In Progress tab shows footer hints
- All other tabs show no keybinding information

---

## Documented but Not Implemented

The following keybindings are referenced in existing documentation but do not
match the current code:

- **`Q` to quit** (`docs/TUI_INTERFACE.md`, line 211): The doc states
  *"Pressing `Q` or selecting the Exit tab shows a confirmation popup before
  exiting the application."* However, in the code there is a comment
  `// 'q' key removed - use Exit tab instead.` (`src/ui/key_handler/mod.rs`,
  line 709). The keybinding was removed but the documentation was not updated.

## Already Documented Keybindings

The following keybindings are documented in `docs/` and implemented in code.
This proposal builds on top of them without breaking any existing behavior.

### Global (`docs/TUI_INTERFACE.md`)

| Key | Action |
|-----|--------|
| `Left`/`Right` | Switch tabs |
| `Up`/`Down` | Navigate lists/tables |
| `Enter` | Confirm / select / open |
| `Esc` | Cancel / close popup |
| `Ctrl+H` | Context-aware help popup |
| `C` | Copy invoice (in PayInvoice notification) |

### Disputes In Progress (`docs/ADMIN_DISPUTES.md`)

| Key | Action |
|-----|--------|
| `Tab` | Switch Buyer/Seller chat |
| `Shift+F` | Finalize dispute |
| `Shift+I` | Toggle chat input |
| `Shift+C` | Toggle In Progress / Finalized filter |
| `PageUp`/`PageDown` | Scroll chat |
| `End` | Jump to bottom of chat |
| `Enter` | Send message (when input enabled) |
| `Ctrl+S` | Save attachment popup |
| `Backspace` | Delete characters |

### Observer (`docs/ADMIN_DISPUTES.md`)

| Key | Action |
|-----|--------|
| `Enter` | Fetch chat for shared key |
| `Ctrl+C` | Clear all (shared key, messages, errors) |
| `Ctrl+S` | Save attachment popup |
| `Ctrl+H` | Help popup |

### Create New Order (`docs/TUI_INTERFACE.md` help constants)

| Key | Action |
|-----|--------|
| `Up`/`Down` | Change field |
| `Tab` | Next field |
| `Enter` | Confirm order |

### Settings

| Key | Action |
|-----|--------|
| `m`/`M` | Switch User/Admin mode |
| `Up`/`Down` | Select option |
| `Enter` | Open selected option |

---

## Phase 1: High-Impact Quick Wins

### 1.1 `q` to quit (with confirmation)

Add `q` as a shortcut to open the exit confirmation popup (defaulting to "No"
for safety). This is the same popup shown when pressing Enter on the Exit tab,
so no new exit flow is introduced.

**Guard**: Only active in Normal mode. Disabled during form input, Observer tab
(text input), admin chat input, and any popup/overlay.

### 1.2 `?` as alternative help key

Add `?` as an alternative to `Ctrl+H` for opening the context-aware help popup.
`?` is the universal convention for help in TUI applications (vim, htop,
lazydocker, less, man).

**Guard**: Same mode check as `Ctrl+H`. On the Observer tab, only `Ctrl+H`
works since `?` is captured as text input (acceptable trade-off for a
specialized input tab).

### 1.3 `Tab`/`BackTab` cycle through tabs

Make `Tab` and `BackTab` (Shift+Tab) cycle forward and backward through tabs in
normal mode. The existing overrides are preserved:
- Disputes In Progress: `Tab`/`BackTab` still switches Buyer/Seller party
- Create New Order: `Tab`/`BackTab` still navigates form fields

In all other tabs, `Tab`/`BackTab` would switch to the next/previous tab
(same behavior as `Right`/`Left` arrows).

### 1.4 Change mode switch from `m` to `Shift+M`

Currently both `m` and `M` trigger User/Admin mode switch in the Settings tab.
Lowercase `m` is too easy to press accidentally. Change it so only `Shift+M`
(uppercase `M`) triggers the mode switch.

---

## Phase 2: Keybinding Hint Bar

### 2.1 Context-aware keybinding hints in the footer

Add a persistent, context-aware hint line at the bottom of the screen showing
relevant shortcuts for the current tab. This follows the convention used by
bluetui, lazydocker, and other modern TUIs.

Format: `key Action | key Action | key Action`

Examples per tab (User mode, Normal state):

| Tab | Hints |
|-----|-------|
| Orders | `q Quit \| ? Help \| Tab/Arrows Tabs \| Up/Down Select \| Enter Take` |
| My Trades | `q Quit \| ? Help \| Tab/Arrows Tabs` |
| Messages | `q Quit \| ? Help \| Tab/Arrows Tabs \| Up/Down Select \| Enter Open` |
| Mostro Info | `q Quit \| ? Help \| Tab/Arrows Tabs` |
| Create New Order | `? Help \| Up/Down/Tab Fields \| Enter Confirm` |
| Settings | `q Quit \| ? Help \| Tab/Arrows Tabs \| Up/Down Select \| Shift+M Mode` |
| Exit | `? Help \| Tab/Arrows Tabs \| Enter Confirm` |

The Disputes In Progress tab already has its own footer hints; the global hint
bar should yield to it or merge them to avoid duplication.

---

## Phase 3: Advanced Navigation (Optional)

These are more ambitious improvements that can be discussed and prioritized
independently.

### 3.1 Number keys for direct tab access

Press `1`-`7` (User mode) or `1`-`6` (Admin mode) to jump directly to a
specific tab. Disabled during text input modes.

### 3.2 Vim-style `j`/`k` for up/down navigation

Add `j` (down) and `k` (up) as alternatives to arrow keys for navigating
lists and tables. Disabled during text input modes.

### 3.3 `r` for manual refresh

Trigger a re-fetch/reconnection to relays. Requires async channel plumbing from
the key handler to the main event loop.

### 3.4 `/` for search/filter

Open a filter input to search orders by currency, amount, or payment method.
Requires a new `UiMode` variant and rendering logic.

---

## Implementation Notes

- New character keybindings (`q`, `?`, `j`, `k`, numbers) must be inserted
  **before** the Observer tab early-return block and the admin chat input block
  in the key handler, as those capture all character input.
- Mode-awareness (`app.mode`) is the correct guard pattern, not tab checking,
  to correctly handle popups and overlays.
- `Ctrl+M` was considered for mode switch but is equivalent to Enter in terminal
  emulators, making it unsuitable. `Shift+M` is the recommended alternative.

---

## Summary

| Phase | Change | Effort |
|-------|--------|--------|
| 1.1 | `q` to quit with confirmation | Small |
| 1.2 | `?` for help | Small |
| 1.3 | `Tab`/`BackTab` cycle tabs | Small |
| 1.4 | Mode switch only on `Shift+M` | Minimal |
| 2.1 | Keybinding hint bar | Medium |
| 3.1 | Number keys for tabs | Small |
| 3.2 | Vim `j`/`k` navigation | Small |
| 3.3 | Manual refresh (`r`) | Medium |
| 3.4 | Search/filter (`/`) | Large |
