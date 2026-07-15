# Codex Daily Token Heatmap and Morphing Card Design

## Context

Codex Quota Ball currently shows a compact 88×88 quota circle and expands into a separate quota card containing short- and weekly-window percentages. The requested revision makes the circle itself morph into a single rounded card, removes the short-window presentation, and adds a GitHub-style calendar heatmap based on real daily token usage.

The installed Codex CLI 0.144.4 app-server schema and a runtime capability check confirm that `account/usage/read` returns `dailyUsageBuckets` entries with `startDate` and `tokens`. The feature will use this account-level source directly rather than estimating token usage from rate-limit percentage changes.

## Goals

- Show only the longest available rate-limit window as `Weekly limits`.
- Display 26 weeks of exact daily token totals in a GitHub-style calendar.
- Morph the compact quota circle smoothly into one unified rounded card and back.
- Preserve always-on-top behavior, edge-aware placement, dragging, manual refresh, retry, stale-data handling, and saved position.
- Keep quota and token-history failures independent.

## Non-goals

- Reconstructing prompts, source code, thread contents, or per-project usage.
- Estimating missing token history locally.
- Maintaining a database or a second authoritative usage history.
- Displaying lifetime-token and streak summaries returned by the usage endpoint.
- Supporting Wayland or non-GNOME desktops in this revision.

## Data Sources

### Weekly limit

The app continues to call `account/rateLimits/read`. It examines all available primary and secondary windows and selects the window with the greatest `windowDurationMins`. If durations are equal or absent, it prefers the secondary window when present, otherwise the primary window. This permits a response containing only one current weekly window while remaining compatible with older two-window responses.

The UI labels the selected value `Weekly limits`; it does not render a 5-hour or short-cycle row.

### Daily token usage

The same initialized app-server process calls `account/usage/read` with no parameters. The relevant result shape is:

```json
{
  "dailyUsageBuckets": [
    { "startDate": "2026-07-15", "tokens": 2386420 }
  ]
}
```

`startDate` is treated as the calendar date supplied by Codex without timezone conversion. Tokens must be non-negative 64-bit integers. Invalid dates and negative values are ignored. If duplicate dates appear, the greatest value for that date wins so malformed input cannot inflate usage by summation.

No daily history is written to a local database. The application retains the last successful response in memory for stale-data display during the current process lifetime.

## Calendar Model and Color Scale

The view covers 26 complete week columns ending in the week containing the local current date, for 182 calendar positions. Columns progress chronologically from left to right. Rows represent Sunday through Saturday. The left edge labels Monday, Wednesday, and Friday; the top labels months at their first visible week boundary. Positions later than the local current date are transparent and non-interactive rather than being counted as zero-use days.

Past and current dates absent from a non-null bucket array have zero tokens and use the gray empty-cell color. A null bucket array means the history source is unavailable and must not be presented as 182 zero-use days. Future positions are excluded from the peak calculation.

For a non-empty range, let `peak` be the greatest visible daily token count. A non-zero day receives one of four green levels using:

```text
score = ln(1 + tokens) / ln(1 + peak)
level = clamp(ceil(score × 4), 1, 4)
```

This adaptive logarithmic scale keeps ordinary days distinguishable when one day is much larger. The legend is relative and reads `少` through `多`. Hovering a cell shows the exact date and a thousands-separated value such as `2,386,420 tokens`. The current date receives a thin light outline.

## Refresh and State Flow

A refresh cycle attempts both app-server methods on the same client connection. The outcomes are represented independently:

- A successful rate-limit response updates Weekly limits even when usage history fails.
- A successful usage response updates the heatmap even when the rate-limit response fails.
- A failed section retains its last in-memory value and becomes stale.
- A section with no previous successful value shows its own concise unavailable message.
- Manual refresh and Retry trigger both reads and retain the existing non-blocking coalescing behavior.
- A terminal protocol or I/O error still causes the worker to recreate the client on the next refresh.

The client continues to use bounded process startup, writes, reads, response waits, and shutdown behavior. It logs neither raw account responses nor credentials.

## Visual Layout

### Compact state

- Size: 88×88 logical pixels.
- Shape: a dark circle with the existing colored quota ring.
- Value: Weekly remaining percentage, or `!` when unavailable.
- Behavior: left click expands; right click retains Refresh and Quit; drag persists the compact anchor.

### Expanded state

- Target size: 290×292 logical pixels.
- Shape: one dark rounded rectangle with an 18-pixel corner radius.
- The title area uses the same dark background as the rest of the card, with no white title strip.
- Content order:
  1. `Codex 额度`, refresh button, and freshness status.
  2. Weekly remaining percentage, progress bar, and reset time.
  3. `每日使用强度`, `近 26 周`, month labels, weekday labels, calendar cells, and legend.
  4. A concise section-level error when either source is unavailable.

The entire expanded rectangle stays within the selected GNOME work area. It grows toward the side with sufficient space and uses the existing `_GTK_WORKAREAS_Dn` → `_NET_WORKAREA` → RandR fallback chain.

## Morph Animation and Interaction

The UI has four explicit states: `Collapsed`, `Expanding`, `Expanded`, and `Collapsing`.

- Expansion lasts 220 ms with an ease-out curve.
- Collapse lasts 180 ms with an ease-in curve.
- Window size, painted bounds, and corner radius interpolate from 88×88 and a circular radius to the expanded dimensions and 18-pixel radius.
- The percentage face fades and scales down while card content fades in; collapse reverses the transition.
- Input that could begin a drag or trigger another transition is disabled during animation.

Before expansion, the app selects a work area and an anchored growth direction. When growing right or down, the compact top/left edge remains fixed. When growing left or up, the corresponding compact bottom/right edge remains fixed. Collapse uses the inverse geometry and returns to the exact compact anchor.

Clicking another window, losing application focus, or pressing Escape starts collapse. Clicking controls or heatmap cells inside the card does not collapse it.

To preserve current movement behavior, the expanded card can be dragged from non-interactive title or background space. The complete expanded rectangle is clamped to the current work area. Its moved anchored edge is converted back into a compact anchor and saved, so collapse returns to the correct new position. Interactive controls and heatmap hover targets do not initiate window dragging.

## Error Presentation

- `dailyUsageBuckets: null` or an unsupported `account/usage/read` method shows `Token 历史不可用` rather than an empty heatmap.
- An empty non-null array renders a valid all-gray calendar.
- A stale section continues showing its last successful data with `数据可能已过期`.
- Protocol incompatibility messages remain concise and recommend updating Codex when the usage method is unavailable.
- Weekly and token-history errors are displayed independently without covering usable content from the other section.

## Implementation Boundaries

- Add a focused usage domain module for parsing, visible-date construction, token formatting, and color levels.
- Extend the existing app-server client and worker state rather than adding another process or polling loop.
- Replace the separate ball-and-card drawing with a single animated surface in the existing UI module.
- Continue using `chrono`, `serde_json`, `egui`, and `x11rb`; add no chart, animation, database, or telemetry dependency.
- Keep the existing position configuration format compatible.

## Verification

Automated tests will cover:

- Daily usage parsing, null and empty buckets, invalid dates, negative values, and duplicates.
- The 182-day calendar across month, year, leap-year, and leading/trailing partial-week boundaries.
- Logarithmic color levels, zero and extreme token values, exact token formatting, and current-day marking.
- Longest-window Weekly selection for one- and two-window responses and missing durations.
- Independent success, failure, stale retention, refresh coalescing, and client recreation for the two reads.
- Animation endpoints, timing curves, corner-radius interpolation, anchored growth in every direction, and exact compact-anchor restoration.
- Work-area clamping, negative monitor origins, HiDPI coordinates, expanded dragging, focus loss, and Escape collapse.
- Existing installer, configuration, quota, client timeout, and X11 geometry behavior.

Final verification includes formatting, all locked tests, Clippy with warnings denied, a locked release build, isolated installation tests, and a short real GNOME X11 startup/animation smoke run. Visual smoothness and outside-click behavior receive manual acceptance because they cannot be proven by pure geometry tests alone.
