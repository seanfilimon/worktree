# Scene 5 — Act III: Introducing W0rkTree

**Timecode**: 2:45–4:15
**Duration**: 90 seconds
**Purpose**: The reveal. Show what W0rkTree IS — architecture, key innovations, and why it's different. This is the longest scene and the emotional peak of the entire video.
**Emotional Arc**: Clarity → Understanding → Innovation → Confidence → Wholeness

---

## Table of Contents

- [Overview](#overview)
- [Segment 5A — The Declaration (2:45–2:55)](#segment-5a--the-declaration-245255)
- [Segment 5B — Two-Runtime Architecture (2:55–3:20)](#segment-5b--two-runtime-architecture-255320)
- [Segment 5C — Staged Snapshot Visibility (3:20–3:45)](#segment-5c--staged-snapshot-visibility-320345)
- [Segment 5D — Feature Pillars (3:45–4:05)](#segment-5d--feature-pillars-345405)
- [Segment 5E — Architecture Complete + Protocol (4:05–4:15)](#segment-5e--architecture-complete--protocol-405415)
- [Dialogue — Complete for Act III](#dialogue--complete-for-act-iii)
- [Audio Design](#audio-design)
- [Color Palette for This Scene](#color-palette-for-this-scene)
- [Editorial Notes](#editorial-notes)
- [Technical Notes](#technical-notes)

---

## Overview

Act III is the emotional and informational centerpiece of the video. Everything before it — the cold open, the history, the five problems — builds to this moment. Everything after it — the demo, the close — extends from it.

The color palette **permanently shifts** on the hard cut from Act II at 2:45. No more Warning Red (`#FF3B3B`) as a dominant accent. The frame is now Deep Navy (`#0A0F1A`) with Accent Cyan (`#00D4FF`) as the primary accent and Confident Green (`#00C48F`) as the secondary. This shift happened instantaneously — no crossfade, no transition gradient. A hard cut. The viewer should immediately feel the difference: cleaner, more confident, more spacious, more open. The visual oppression of Act II is over.

Act III is divided into five segments:

| Segment | Timecode    | Duration | Content                              |
| ------- | ----------- | -------- | ------------------------------------ |
| 5A      | 2:45–2:55   | 10 s     | The Declaration — logo + definition  |
| 5B      | 2:55–3:20   | 25 s     | Two-Runtime Architecture diagram     |
| 5C      | 3:20–3:45   | 25 s     | Staged Snapshot Visibility           |
| 5D      | 3:45–4:05   | 20 s     | Feature Pillars (4 cards)            |
| 5E      | 4:05–4:15   | 10 s     | Architecture Complete + Protocol     |

---

## Segment 5A — The Declaration (2:45–2:55, 10 seconds)

### Narrator

> "This is W0rkTree. Not a Git wrapper. Not a Git extension. Not a hosting platform for Git repos. A ground-up replacement for Git — with a migration bridge so you can bring your existing repos with you."

### Visual Concept

A clean, open frame. After 75 seconds of Act II's claustrophobic, red-tinged problem space, this scene breathes. The logo dominates the frame. The denials (what W0rkTree is NOT) appear and are crossed out. Then the affirmation (what it IS) lands with weight.

### SHOT 5A.1 — Logo Entrance (2:45.000–2:47.000)

| Property        | Value                                                                                                  |
| --------------- | ------------------------------------------------------------------------------------------------------ |
| Background      | Pure `#0A0F1A` (Deep Navy) — no gradient, no texture, no noise overlay for these 2 seconds             |
| Logo text       | "W0rkTree" in Inter Tight, weight 800 (`display-lg` = 128 px at 4K / 64 px at HD)                     |
| Logo color      | `#F0F0F0` (Pure White)                                                                                 |
| Logo "0" glow   | Radial gradient `#00D4FF` at 30 % opacity, 20 px blur, positioned behind the "0" character             |
| Logo position   | Dead center of frame (grid intersection of columns 6–7, rows 4–5)                                     |
| Entry animation | `opacity(0)` + `scale(0.98)` → `opacity(1)` + `scale(1.0)`, 1000 ms, `ease-decelerate` (`cubic-bezier(0.0, 0.0, 0.0, 1.0)`) |
| Timing          | Logo begins fading in at 2:45.000, reaches full visibility at 2:46.000                                |
| Hold            | Logo holds at full size, centered, for 1000 ms (2:46.000–2:47.000) before denial text begins          |

> **Note:** This logo treatment is identical to the Title Card (Scene 2, SHOT 2.1) but scaled to `display-lg` instead of `display-lg` — same size. The consistency is intentional: the viewer's brain connects this moment to the earlier brand reveal, creating a callback.

### SHOT 5A.2 — Denial Sequence (2:47.000–2:51.500)

Three denial phrases appear below the logo, one at a time, each immediately struck through. The denials are what W0rkTree is NOT.

**Layout:**
- All denial text is centered horizontally below the logo
- Vertical gap between logo and first denial: 64 px
- Vertical gap between each denial line: 32 px

**Denial 1: "Git wrapper"**

| Property           | Value                                                                                           |
| ------------------ | ----------------------------------------------------------------------------------------------- |
| Timecode           | 2:47.000–2:48.500                                                                               |
| Text               | "Git wrapper" in `heading-2` (Inter SemiBold, 64 px at 4K / 32 px at HD)                       |
| Text color         | `#6B7280` (Muted Gray) — these are diminished claims, not primary text                          |
| Entry animation    | `opacity(0)` + `translateY(8px)` → `opacity(1)` + `translateY(0)`, 300 ms, `ease-enter`        |
| Entry start        | 2:47.000                                                                                        |
| Strikethrough line | Begins at 2:47.400 (400 ms after text entry starts)                                            |
| Strikethrough anim | A horizontal line draws left-to-right across the full width of the text                         |
| Line style         | 3 px solid `#FF3B3B` (Warning Red) — the only appearance of red in Act III, used for negation   |
| Line position      | Vertically centered on the text (50 % of cap-height)                                           |
| Line duration      | 300 ms, `ease-sharp` (`cubic-bezier(0.4, 0.0, 0.6, 1.0)`)                                     |
| Line complete      | 2:47.700                                                                                        |
| SFX                | Subtle "strike" sound at 2:47.400, −28 dB — a quick, clean swipe (high-pass filtered white noise burst, 80 ms decay) |

**Denial 2: "Git extension"**

| Property           | Value                                                                                           |
| ------------------ | ----------------------------------------------------------------------------------------------- |
| Timecode           | 2:48.500–2:50.000                                                                               |
| Text               | "Git extension" — same style as Denial 1                                                        |
| Entry start        | 2:48.500                                                                                        |
| Strikethrough      | Begins at 2:48.900, same animation spec as Denial 1                                            |
| Strikethrough done | 2:49.200                                                                                        |
| SFX                | Same strike sound, −28 dB, at 2:48.900                                                         |

**Denial 3: "hosting platform"**

| Property           | Value                                                                                           |
| ------------------ | ----------------------------------------------------------------------------------------------- |
| Timecode           | 2:50.000–2:51.500                                                                               |
| Text               | "hosting platform" — same style as Denials 1–2                                                  |
| Entry start        | 2:50.000                                                                                        |
| Strikethrough      | Begins at 2:50.400, same animation spec                                                        |
| Strikethrough done | 2:50.700                                                                                        |
| SFX                | Same strike sound, −28 dB, at 2:50.400                                                         |

> **Timing rationale:** The three denials are spaced 1.5 s apart (2:47.0, 2:48.5, 2:50.0). This gives the narrator time to say each phrase — "Not a Git wrapper" takes ~1.2 s, leaving 0.3 s of breathing room before the next denial. The strikethrough animation occurs 400 ms after each text entry, synchronized with the word "Not" in the narration.

### SHOT 5A.3 — Affirmation (2:51.500–2:55.000)

| Property           | Value                                                                                           |
| ------------------ | ----------------------------------------------------------------------------------------------- |
| Timecode           | 2:51.500–2:55.000                                                                               |
| Text               | "A ground-up replacement." in `heading-1` (Inter Bold, 96 px at 4K / 48 px at HD)              |
| Text color         | `#00D4FF` (Accent Cyan) — this is a primary affirmation, it gets brand color                    |
| Position           | Centered, below the three struck-through denials, with 48 px gap above                         |
| Entry animation    | `opacity(0)` + `translateY(16px)` → `opacity(1)` + `translateY(0)`, 600 ms, `ease-enter` (`cubic-bezier(0.0, 0.0, 0.2, 1.0)`) |
| Entry start        | 2:51.500                                                                                        |
| Entry complete     | 2:52.100                                                                                        |
| Hold               | Holds visible from 2:52.100 through 2:55.000 (2.9 s)                                          |

At this point the frame contains:
- Logo (top center, `display-lg`)
- ~~Git wrapper~~ (struck through, muted)
- ~~Git extension~~ (struck through, muted)
- ~~hosting platform~~ (struck through, muted)
- "A ground-up replacement." (bright cyan, the only thing that isn't negated)

The visual hierarchy is unmistakable: the denials are gray and crossed out; the affirmation is cyan and bold. The viewer's eye goes directly to the affirmation.

**Transition to Segment 5B:** At 2:54.500, the entire composition (logo + denials + affirmation) begins fading out: `opacity(1)` → `opacity(0)`, 500 ms, `ease-exit`. Frame reaches pure Deep Navy at 2:55.000, and the architecture diagram begins building immediately.

---

## Segment 5B — Two-Runtime Architecture (2:55–3:20, 25 seconds)

### Narrator

> "W0rkTree runs two systems. On your machine — a background process we call the worker. It watches your files. It snapshots your work automatically. It handles branching, diffing, merging — everything you'd expect from version control — all locally, all instantly."

> "On the server — a multi-tenant platform that your whole team connects to. It stores canonical history, enforces access control, manages tenants and teams — and does something Git has never done."

### Visual Concept

The architecture diagram builds itself on screen, animated piece by piece. This is THE key diagram of the entire video. It must be clear, beautiful, and memorable. If a viewer pauses the video on this frame, they must be able to understand W0rkTree's architecture from the diagram alone.

The diagram uses a two-column layout as defined in Design System § 5.5:

| Property            | Value                                                   |
| ------------------- | ------------------------------------------------------- |
| Total width         | 2800 px at 4K, centered in content area                 |
| Total height        | ≤ 1400 px at 4K                                        |
| Layout              | Two-column: LOCAL (left half) and REMOTE (right half)   |
| Column gap          | 200 px (space for the connection line)                  |

### SHOT 5B.1 — LOCAL Container Build (2:55.000–3:03.000)

This shot builds the entire left side of the architecture diagram.

**Phase 1: Container label (2:55.000–2:55.400)**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Text            | "LOCAL MACHINE" in `heading-3` (Inter SemiBold, 48 px at 4K / 24 px at HD)                     |
| Text color      | `#6B7280` (Muted Gray)                                                                          |
| Position        | Left-aligned with the container's left edge, 16 px above the container's top border             |
| Letter spacing  | +0.08 em above baseline (all-caps adjustment per Design System § 2.3 rule 3)                   |
| Animation       | `opacity(0)` + `translateY(8px)` → `opacity(1)` + `translateY(0)`, 400 ms, `ease-enter`        |
| Start           | 2:55.000                                                                                        |
| Complete        | 2:55.400                                                                                        |

**Phase 2: Container rectangle draw (2:55.700–2:56.900)**

| Property          | Value                                                                                         |
| ----------------- | --------------------------------------------------------------------------------------------- |
| Start delay       | 300 ms after label begins (2:55.700)                                                          |
| Shape             | Rounded rectangle, 1300 px wide × auto height                                                |
| Border radius     | 16 px                                                                                         |
| Border            | 2 px solid `#374151` (Dim Gray)                                                               |
| Fill              | `#111827` (Code Background)                                                                   |
| Animation type    | Border-draw — the border traces itself clockwise starting from the top-left corner             |
| Draw sequence     | Top-left → top-right → bottom-right → bottom-left → close                                    |
| Duration          | 1200 ms, `ease-standard` (`cubic-bezier(0.4, 0.0, 0.2, 1.0)`)                                |
| Fill behavior     | The `#111827` fill fades in progressively as the border completes (fill reaches 100 % at the moment the border closes) |
| Start             | 2:55.700                                                                                      |
| Complete          | 2:56.900                                                                                      |

**Phase 3: Main node — "worktree-worker" (2:56.900–2:59.400)**

The main node builds inside the container after the container border completes.

| Property             | Value                                                                                      |
| -------------------- | ------------------------------------------------------------------------------------------ |
| Node shape           | Rounded rectangle, border-radius 12 px                                                    |
| Node border          | 1 px solid `#374151`                                                                       |
| Node fill            | `#0A0F1A` (Deep Navy — darker than container, per Design System § 5.5)                     |
| Node padding         | 24 px all sides                                                                            |
| Title text           | "worktree-worker" in `heading-2` (Inter SemiBold, 64 px at 4K / 32 px at HD)              |
| Title color          | `#00D4FF` (Accent Cyan) — the worker is a W0rkTree component, it gets brand color          |
| Title animation      | `opacity(0)` + `translateY(16px)` → `opacity(1)` + `translateY(0)`, 400 ms, `ease-enter`  |
| Title start          | 2:56.900                                                                                   |
| Title complete       | 2:57.300                                                                                   |

Sub-items appear staggered below the title, 200 ms apart:

| Sub-item                   | Start     | Complete  | Style                                                        |
| -------------------------- | --------- | --------- | ------------------------------------------------------------ |
| "• Filesystem watcher"     | 2:57.500  | 2:57.800  | `body` (32 px at 4K), `#F0F0F0` at 70 % (`#F0F0F0B3`)      |
| "• Auto-snapshots"         | 2:57.700  | 2:58.000  | Same style                                                   |
| "• Local change tracking"  | 2:57.900  | 2:58.200  | Same style                                                   |
| "• Background sync"        | 2:58.100  | 2:58.400  | Same style                                                   |
| "• Manages .wt/"           | 2:58.300  | 2:58.600  | Same style, ".wt/" in `code-sm` (28 px), `#00D4FF`          |

Each sub-item animation: `opacity(0)` + `translateY(8px)` → `opacity(1)` + `translateY(0)`, 300 ms, `ease-enter`

Bullet style: 6 px circle, `#374151`, per Design System § 5.5.

SFX: A subtle "snap" click at each sub-item appearance, −32 dB. Source: a clean, short digital click (5 ms attack, 50 ms decay, 6 kHz center frequency). Panned 15 % left (matching the LOCAL side's position in the stereo field).

**Phase 4: Local Worktree sub-node (2:58.900–2:59.900)**

A smaller secondary node appears below the main node, inside the container.

| Property             | Value                                                                                      |
| -------------------- | ------------------------------------------------------------------------------------------ |
| Delay                | 300 ms after last sub-item completes (2:58.900)                                            |
| Node shape           | Rounded rectangle, border-radius 12 px, same styling as main node but smaller              |
| Title text           | "Local Worktree" in `heading-3` (48 px at 4K), `#F0F0F0`                                  |
| Title animation      | `opacity(0)` + `translateY(8px)` → `opacity(1)` + `translateY(0)`, 300 ms, `ease-enter`   |
| Node-to-node gap     | 24 px between main node and this sub-node                                                  |

Inside the sub-node, a miniature file tree appears:

```
project/
├── .wt/
├── src/
├── tests/
└── config.toml
```

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Font            | `code-sm` (JetBrains Mono 400, 28 px at 4K / 14 px at HD)                                     |
| Color           | `#6B7280` (Muted Gray) — this is structural information, not primary content                    |
| ".wt/" color    | `#00D4FF` (Accent Cyan) — emphasize the W0rkTree directory                                     |
| Animation       | Lines appear top-to-bottom, 100 ms stagger per line, `ease-enter`, 200 ms each                 |
| Start           | 2:59.200 (300 ms after sub-node title)                                                         |
| Complete        | 2:59.900                                                                                        |

**Phase 4 SFX:** Single soft "snap" at 2:59.200, −32 dB.

**LOCAL container build complete at 2:59.900.** Total build time: ~5 seconds.

### SHOT 5B.2 — Connection Line (3:00.000–3:03.000)

The connection line bridges the LOCAL and REMOTE containers.

| Property              | Value                                                                                       |
| --------------------- | ------------------------------------------------------------------------------------------- |
| Start point           | Right edge of LOCAL container, vertically centered                                          |
| End point             | Left edge of REMOTE container position (the container hasn't appeared yet — the line draws to where it will be) |
| Line style            | Dashed: 12 px dash, 8 px gap                                                               |
| Line stroke           | 3 px, `#00D4FF` (Accent Cyan)                                                              |
| Arrow heads           | Bidirectional filled triangles, 12 px, `#00D4FF` — one at each end                         |
| Draw animation        | Left-to-right stroke reveal, 800 ms, `ease-standard`                                       |
| Draw start            | 3:00.000                                                                                    |
| Draw complete         | 3:00.800                                                                                    |

**Dash flow animation (continuous):**

| Property              | Value                                                                                       |
| --------------------- | ------------------------------------------------------------------------------------------- |
| Type                  | `stroke-dashoffset` animation — dashes continuously move left-to-right                      |
| Speed                 | 2000 ms per full cycle                                                                      |
| Easing                | `linear` (exception per Design System § 5.5 — continuous loops may use linear)              |
| Direction             | Left-to-right (LOCAL → REMOTE), suggesting data flowing to the server                       |
| Start                 | Begins immediately when line draw completes (3:00.800)                                      |
| Duration              | Runs continuously for the remainder of the scene (through 4:15)                             |

**Label above the line:**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Text            | "QUIC" in `code` (JetBrains Mono 400, 36 px at 4K / 18 px at HD)                              |
| Color           | `#00D4FF` (Accent Cyan)                                                                         |
| Position        | Centered on the connection line's horizontal midpoint, 24 px above the line                    |
| Animation       | `opacity(0)` → `opacity(1)`, 400 ms, `ease-enter`                                             |
| Start           | 3:00.400 (midpoint of line draw)                                                               |
| Complete        | 3:00.800                                                                                        |

**Label below the line:**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Text            | "(sync)" in `code-sm` (JetBrains Mono 400, 28 px at 4K / 14 px at HD)                         |
| Color           | `#6B7280` (Muted Gray)                                                                          |
| Position        | Centered on the connection line's horizontal midpoint, 24 px below the line                    |
| Animation       | `opacity(0)` → `opacity(1)`, 400 ms, `ease-enter`                                             |
| Start           | 3:00.600 (200 ms after "QUIC" label starts)                                                   |
| Complete        | 3:01.000                                                                                        |

**SFX:** A subtle digital "whoosh" at 3:00.000, −28 dB. Source: a filtered sweep (low-to-high, 200 Hz → 4 kHz, 600 ms, with a fast 200 ms tail-off). This is the sound of the connection establishing.

### SHOT 5B.3 — REMOTE Container Build (3:03.000–3:15.000)

Mirror structure of the LOCAL build, for the server side. The build is slightly faster because the viewer has already learned the visual language from the LOCAL side.

**Phase 1: Container label (3:03.000–3:03.400)**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Text            | "REMOTE / HOSTED" in `heading-3` (48 px at 4K), `#6B7280`                                     |
| Position        | Right side of frame, left-aligned with REMOTE container's left edge, 16 px above top border    |
| Letter spacing  | +0.08 em (all-caps adjustment)                                                                 |
| Animation       | Same as LOCAL label: `opacity(0)` + `translateY(8px)` → visible, 400 ms, `ease-enter`         |
| Start           | 3:03.000                                                                                        |
| Complete        | 3:03.400                                                                                        |

**Phase 2: Container rectangle draw (3:03.700–3:04.900)**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Start delay     | 300 ms after label (3:03.700)                                                                  |
| Shape           | Identical spec to LOCAL container: 1300 px wide, rounded-rect 16 px, 2 px `#374151`, `#111827` |
| Animation       | Border-draw, clockwise from top-left, 1200 ms, `ease-standard`                                |
| Start           | 3:03.700                                                                                        |
| Complete        | 3:04.900                                                                                        |

**Phase 3: Main node — "worktree-server" (3:04.900–3:08.100)**

| Property             | Value                                                                                      |
| -------------------- | ------------------------------------------------------------------------------------------ |
| Title text           | "worktree-server" in `heading-2` (64 px at 4K), `#00D4FF`                                 |
| Title animation      | Same as worker: 400 ms, `ease-enter`                                                      |
| Title start          | 3:04.900                                                                                   |
| Title complete       | 3:05.300                                                                                   |

Sub-items staggered 200 ms apart:

| Sub-item                      | Start     | Complete  | Style                                                   |
| ----------------------------- | --------- | --------- | ------------------------------------------------------- |
| "• Multi-tenant"              | 3:05.500  | 3:05.800  | `body` (32 px at 4K), `#F0F0F0` at 70 %                |
| "• IAM / Access Control"      | 3:05.700  | 3:06.000  | Same style                                              |
| "• Stores all history"        | 3:05.900  | 3:06.200  | Same style                                              |
| "• Manages worktrees"         | 3:06.100  | 3:06.400  | Same style                                              |
| "• Serves API"                | 3:06.300  | 3:06.600  | Same style                                              |
| "• Tenant isolation"          | 3:06.500  | 3:06.800  | Same style                                              |

Each sub-item: `opacity(0)` + `translateY(8px)` → visible, 300 ms, `ease-enter`.
SFX: Same "snap" clicks as LOCAL side, −32 dB per node, panned 15 % right.

**Phase 4: Secondary nodes (3:07.100–3:08.800)**

Two smaller nodes appear below the main server node, inside the container, side-by-side.

**Node A: worktree-admin**

| Property             | Value                                                                                      |
| -------------------- | ------------------------------------------------------------------------------------------ |
| Title text           | "worktree-admin" in `heading-3` (48 px at 4K), `#F0F0F0`                                  |
| Subtitle text        | "Web UI for server mgmt" in `body` (32 px at 4K), `#6B7280`                               |
| Position             | Left side of the REMOTE container's lower area                                             |
| Node gap             | 24 px below main node                                                                     |
| Animation            | `opacity(0)` + `translateY(8px)` → visible, 300 ms, `ease-enter`                          |
| Start                | 3:07.100                                                                                   |
| Complete             | 3:07.400                                                                                   |

**Node B: worktree-cli (wt)**

| Property             | Value                                                                                      |
| -------------------- | ------------------------------------------------------------------------------------------ |
| Title text           | "worktree-cli (wt)" in `heading-3` (48 px at 4K), `#F0F0F0`                               |
| Subtitle text        | "CLI interface for users" in `body` (32 px at 4K), `#6B7280`                               |
| Position             | Right side of the REMOTE container's lower area, same vertical level as Node A             |
| Node-to-node gap     | 24 px horizontal between A and B                                                          |
| Animation            | Same as Node A, 300 ms, `ease-enter`                                                      |
| Start                | 3:07.300 (200 ms stagger after Node A)                                                    |
| Complete             | 3:07.600                                                                                   |

**Phase 5: Additional connection — CLI to LOCAL (3:07.800–3:08.800)**

A thin secondary connection line from the "worktree-cli (wt)" node to the LOCAL container, indicating the CLI runs on the user's machine but communicates with the server.

| Property              | Value                                                                                       |
| --------------------- | ------------------------------------------------------------------------------------------- |
| Line style            | Dotted: 4 px dash, 4 px gap (visually lighter than the main QUIC connection)               |
| Line stroke           | 1 px, `#374151` (Dim Gray — this is a secondary relationship, not the primary data path)   |
| Draw animation        | Right-to-left stroke reveal, 600 ms, `ease-standard`                                      |
| Draw start            | 3:07.800                                                                                    |
| Draw complete         | 3:08.400                                                                                    |
| Label                 | None — the line's existence implies the relationship; labeling would clutter                |

**REMOTE container build complete at 3:08.800.** Total build time for REMOTE side: ~5.8 seconds.

> **Narrator sync:** The REMOTE container finishes building right as the narrator says "and does something Git has never done." This creates a deliberate pause — the diagram is complete, both systems are visible, but the KEY INNOVATION hasn't been named yet. The pause creates anticipation.

### SHOT 5B.4 — Diagram Complete + Hold (3:08.800–3:20.000)

| Property              | Value                                                                                       |
| --------------------- | ------------------------------------------------------------------------------------------- |
| Duration              | ~11 seconds (3:08.800–3:20.000) — the narrator continues over the completed diagram        |
| Diagram state         | Both containers fully built, all nodes visible, connection line animating (dash-offset)     |
| Connection pulse      | At 3:12.000 and 3:16.000: a "data pulse" travels along the connection line. The pulse is a glow effect (#00D4FF at 30 % opacity, 40 px wide, Gaussian blur 12 px) that moves from LOCAL to REMOTE over 600 ms, `ease-standard`. This creates a sense of living data flow. |
| Diagram balance       | The composition is symmetric: LOCAL left, REMOTE right, equal visual weight. The connection line is the horizontal axis of symmetry. |
| Ambient state         | All `#00D4FF` elements are at 80 % opacity in the ambient state, creating room for the pulse to "brighten" them to 100 % as it passes |

During this hold, the narrator delivers the second half of the Segment 5B narration. The diagram serves as a persistent visual anchor — the viewer studies it while listening.

**Frame composition at 3:20.000 (diagram fully built):**

```
┌─────────────────────────────────────────────────────────────────────────┐
│                                                                         │
│   LOCAL MACHINE                                    REMOTE / HOSTED      │
│  ┌──────────────────────┐                        ┌──────────────────────┐│
│  │                      │                        │                      ││
│  │  worktree-worker     │    QUIC                │  worktree-server     ││
│  │  ─────────────────   │  ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄▶   │  ─────────────────   ││
│  │  • Filesystem watcher│    (sync)              │  • Multi-tenant      ││
│  │  • Auto-snapshots    │                        │  • IAM / Access Ctrl ││
│  │  • Local change track│                        │  • Stores all history││
│  │  • Background sync   │                        │  • Manages worktrees ││
│  │  • Manages .wt/      │                        │  • Serves API        ││
│  │                      │                        │  • Tenant isolation   ││
│  │  ┌────────────────┐  │                        │                      ││
│  │  │ Local Worktree │  │                        │  ┌─────────┐┌───────┐││
│  │  │ project/       │  │                        │  │wt-admin ││wt-cli │││
│  │  │ ├── .wt/       │  │                        │  │ Web UI  ││ CLI   │││
│  │  │ ├── src/       │  │                        │  └─────────┘└───────┘││
│  │  │ └── ...        │  │                        │                      ││
│  │  └────────────────┘  │                        └──────────────────────┘│
│  └──────────────────────┘                                               │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

> **ASCII reference only.** The actual rendering uses the full component specs from Design System § 5.5 with proper rounded rectangles, color fills, and typographic styling.

---

## Segment 5C — Staged Snapshot Visibility (3:20–3:45, 25 seconds)

### Narrator

> "When the worker captures a snapshot of your work — whether automatic or manual — it syncs that snapshot to the server as a staged snapshot. Not pushed. Not merged. Staged. Your team can see what you're working on and which files you're touching — in real time — without you doing anything."

> "Think about what that means. No more 'I had no idea you were working on that file.' No more merge conflicts that could have been prevented with thirty seconds of awareness. The entire team sees the full picture of what's in flight — and when you're ready, you explicitly push your staged work to the branch. Visible doesn't mean merged. Staged doesn't mean pushed. You're always in control."

### Visual Concept

This is the innovation reveal. The architecture diagram compresses to make room for the single most important concept in the video: staged snapshot visibility. The dashboard mockup (Design System § 5.6) is the "emotional money shot" — the moment the viewer goes "oh, THAT'S what this solves."

### SHOT 5C.1 — "STAGED SNAPSHOT VISIBILITY" Impact (3:20.000–3:23.000)

**Architecture diagram transition:**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Animation       | The architecture diagram scales to 40 % of its full size and translates to the top-left corner of the frame |
| Scale target    | `scale(0.40)`, positioned at grid columns 1–4, rows 1–2                                       |
| Duration        | 600 ms, `ease-standard`                                                                        |
| Opacity         | Dims to 60 % — it remains visible as context but is no longer the focus                        |
| Start           | 3:20.000                                                                                        |
| Complete        | 3:20.600                                                                                        |
| Dash animation  | The connection line's dash animation continues even at reduced scale — the diagram is still alive |

**Impact text:**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Text            | "STAGED SNAPSHOT VISIBILITY" in `display-lg` (Inter Tight 800, 128 px at 4K / 64 px at HD)    |
| Color           | `#00D4FF` (Accent Cyan)                                                                         |
| Letter spacing  | −0.02 em base + 0.08 em all-caps adjustment = +0.06 em effective                              |
| Position        | Centered on canvas (grid columns 3–10, row 5)                                                  |
| Background      | Radial gradient `cyan-glow` behind the text: `radial-gradient(circle, #00D4FF33 0%, transparent 70%)` |
| Entry animation | `opacity(0)` + `scale(0.9)` → `opacity(1)` + `scale(1.0)`, 500 ms, `ease-bounce` (`cubic-bezier(0.34, 1.56, 0.64, 1.0)`) |
| Start           | 3:20.300 (300 ms after diagram begins shrinking — the title enters AS the diagram moves)       |
| Complete        | 3:20.800                                                                                        |
| Hold            | 3:20.800–3:23.000 (2.2 s)                                                                     |

> **Easing note:** This is one of the permitted uses of `ease-bounce` per Design System § 4.3.2: it is an impact moment, not body text. The slight overshoot gives the title physical weight.

**SFX:** At 3:20.300 — a clean, resonant bell-like tone. NOT a percussive hit like Act II's problem numbers. This is a revelation, not a problem.

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Sound source    | Synthesized bell tone: fundamental at C4 (261.6 Hz) with harmonics at G4 (392 Hz) and E5 (659.3 Hz) |
| Attack           | 10 ms (near-instant)                                                                           |
| Sustain          | 800 ms                                                                                         |
| Decay            | 600 ms exponential fade                                                                        |
| Total duration   | ~1.4 s                                                                                         |
| Level            | −20 dB LUFS                                                                                    |
| Character        | Clean, crystalline, bell-like. Think a singing bowl or a Rhodes piano with the tine emphasized. Not harsh, not dark. Pure. |

### SHOT 5C.2 — The Flow Diagram (3:23.000–3:33.000)

The "STAGED SNAPSHOT VISIBILITY" text fades up to the top of frame (becomes a persistent header) while a flow diagram builds below it.

**Header transition:**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Text            | "STAGED SNAPSHOT VISIBILITY" scales down to `heading-2` (64 px at 4K) and moves to top-center  |
| Animation       | `scale(1.0)` → `scale(0.5)` + translate to top-center, 400 ms, `ease-standard`                |
| Final opacity   | 60 % — it serves as a section header, not the focus                                            |
| Timing          | 3:23.000–3:23.400                                                                               |

**Flow diagram build:**

The flow diagram is a horizontal sequence of four steps connected by arrows. It builds left-to-right.

**Step 1: "Developer edits files" (3:23.400–3:24.200)**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Icon            | Stylized person silhouette (24 px line art) + file document icon, `#F0F0F0`, stacked vertically |
| Label           | "Developer edits files" in `body-lg` (40 px at 4K), `#F0F0F0`                                 |
| Position        | Left quarter of the flow area (grid columns 2–3)                                               |
| Animation       | `opacity(0)` + `translateY(16px)` → visible, 400 ms, `ease-enter`                             |
| Start           | 3:23.400                                                                                        |
| Complete        | 3:23.800                                                                                        |

**Arrow 1 → (3:24.200–3:24.600)**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Shape           | Horizontal arrow, 2 px stroke, `#374151`, with a filled triangle head (8 px)                   |
| Draw animation  | Left-to-right stroke reveal, 400 ms, `ease-standard`                                          |
| Start           | 3:24.200                                                                                        |
| Complete        | 3:24.600                                                                                        |

**Step 2: "Worker captures snapshot" (3:24.600–3:25.400)**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Icon            | Stylized camera/capture icon (concentric circles suggesting a lens), `#00D4FF`                 |
| Label           | "Worker captures snapshot" in `body-lg` (40 px at 4K), `#00D4FF`                              |
| Position        | Left-center of the flow area (grid columns 4–5)                                                |
| Animation       | Same as Step 1: 400 ms, `ease-enter`                                                          |
| Start           | 3:24.600                                                                                        |
| Complete        | 3:25.000                                                                                        |

**Arrow 2 → with label (3:25.400–3:26.200)**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Arrow           | Same style as Arrow 1, 400 ms draw                                                            |
| Label           | "auto-sync" in `code-sm` (28 px at 4K), `#6B7280`, centered above the arrow                   |
| Label animation | `opacity(0)` → `opacity(1)`, 300 ms, `ease-enter`, starting 200 ms after arrow draw begins    |
| Start           | 3:25.400                                                                                        |
| Complete        | 3:26.200                                                                                        |

**Step 3: "Team sees staged work" (3:26.200–3:27.000)**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Icon            | Three person silhouettes (team) + a dashboard/grid icon, `#00C48F` (Confident Green)           |
| Label           | "Team sees staged work on server" in `body-lg` (40 px at 4K), `#00C48F`                       |
| Position        | Right-center of the flow area (grid columns 7–9)                                               |
| Animation       | Same pattern: 400 ms, `ease-enter`                                                             |
| Start           | 3:26.200                                                                                        |
| Complete        | 3:26.600                                                                                        |

**Gap + "YOU DECIDE WHEN" label (3:27.000–3:28.000)**

Between Step 3 and Step 4, a prominent label emphasizes the user's control.

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Text            | "YOU DECIDE WHEN" in `heading-3` (Inter SemiBold, 48 px at 4K), `#F0F0F0`                     |
| Position        | Centered below the gap between Step 3 and Step 4                                               |
| Letter spacing  | +0.08 em (all-caps adjustment)                                                                 |
| Background      | A subtle horizontal line (1 px, `#374151`) running behind the text, with a 48 px gap cut out for the label |
| Animation       | `opacity(0)` + `translateY(8px)` → visible, 400 ms, `ease-enter`                              |
| Start           | 3:27.000                                                                                        |
| Complete        | 3:27.400                                                                                        |

**Arrow 3 → with label (3:28.000–3:28.800)**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Arrow           | Dashed line (8 px dash, 4 px gap) — dashed to indicate this is a conscious choice, not automatic |
| Arrow color     | `#00D4FF` (Accent Cyan) — this is the push action, it's a W0rkTree operation                   |
| Label           | "explicit push" in `code-sm` (28 px at 4K), `#00D4FF`, centered above the arrow               |
| Draw animation  | 400 ms, `ease-standard`                                                                        |
| Start           | 3:28.000                                                                                        |
| Complete        | 3:28.800                                                                                        |

**Step 4: "Branch history updated" (3:28.800–3:29.600)**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Icon            | Branch line + checkmark (✓), `#00C48F`                                                         |
| Label           | "Branch history updated" in `body-lg` (40 px at 4K), `#00C48F`                                |
| Position        | Right quarter of the flow area (grid columns 10–11)                                            |
| Animation       | Same pattern: 400 ms, `ease-enter`                                                             |
| Start           | 3:28.800                                                                                        |
| Complete        | 3:29.200                                                                                        |

**Flow diagram hold (3:29.200–3:33.000):** The complete flow diagram holds for ~4 seconds. The viewer should absorb the key insight: there is a GAP between "team sees" (Step 3) and "branch updated" (Step 4). That gap — the space between visibility and merge — is the innovation.

> **Total flow diagram build time:** ~6 seconds (3:23.4–3:29.2), matching narration pacing.

### SHOT 5C.3 — Team Dashboard (3:33.000–3:42.000)

This is the **emotional money shot** of the entire video. The team dashboard makes the abstract concept of staged snapshot visibility concrete and tangible.

**Flow diagram transition:**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Animation       | Flow diagram fades to 15 % opacity and translates upward 200 px, 400 ms, `ease-exit`          |
| Start           | 3:33.000                                                                                        |
| Complete        | 3:33.400                                                                                        |

**Dashboard component build:**

Uses the full specification from Design System § 5.6.

**Outer frame (3:33.200–3:33.600):**

| Property           | Value                                                                                        |
| ------------------ | -------------------------------------------------------------------------------------------- |
| Style              | Clean web UI mockup — flat, minimal, on-brand                                                |
| Background         | `#111827` (Code Background)                                                                  |
| Border             | 1 px solid `#374151`                                                                         |
| Border radius      | 16 px                                                                                        |
| Width              | 2400 px at 4K                                                                                |
| Height             | ~900 px (auto, content-driven)                                                               |
| Position           | Centered on canvas                                                                           |
| Internal padding   | 48 px                                                                                        |
| Animation          | `opacity(0)` → `opacity(1)`, 400 ms, `ease-enter`                                          |
| Start              | 3:33.200                                                                                     |
| Complete           | 3:33.600                                                                                     |

**Header bar (3:33.800–3:34.200):**

| Property           | Value                                                                                        |
| ------------------ | -------------------------------------------------------------------------------------------- |
| Text               | "W0rkTree — Team Activity" in `heading-3` (Inter SemiBold, 48 px at 4K)                     |
| Color              | `#F0F0F0` (Pure White)                                                                       |
| Position           | Top of dashboard, left-aligned within padding                                                |
| Bottom border      | 1 px solid `#374151`, spanning full dashboard width                                          |
| Header padding     | 24 px bottom (space between text and divider)                                                |
| Animation          | `opacity(0)` + `translateY(8px)` → visible, 300 ms, `ease-enter`, starting 200 ms after frame |
| Start              | 3:33.800                                                                                     |
| Complete           | 3:34.100                                                                                     |

**Row 1 — Alice (3:34.200–3:34.600):**

| Property              | Value                                                                                     |
| --------------------- | ----------------------------------------------------------------------------------------- |
| Row height            | ~120 px                                                                                   |
| Row padding           | 24 px vertical                                                                            |
| Row divider           | 1 px solid `#374151` below row                                                            |
| Avatar                | Circle, 48 px diameter, solid fill `#00D4FF`                                              |
| Avatar position       | Left edge of row, vertically centered                                                     |
| Name                  | "alice@company.com" in `body-lg` (40 px at 4K), `#F0F0F0`                                |
| Name position         | 24 px right of avatar, top-aligned                                                        |
| Activity              | "3 staged snapshots on feature/oauth" in `body` (32 px at 4K), `#6B7280`                 |
| Activity position     | Below name, 8 px gap                                                                      |
| File path             | "auth-service/src/oauth.rs" in `code-sm` (28 px at 4K), `#00D4FF`                        |
| File path position    | Below activity, 8 px gap                                                                   |
| Entry animation       | `translateX(40px)` + `opacity(0)` → `translateX(0)` + `opacity(1)`, 400 ms, `ease-enter` |
| Start                 | 3:34.200                                                                                   |
| Complete              | 3:34.600                                                                                   |

**Row 2 — Bob (3:34.400–3:34.800):**

| Property              | Value                                                                                     |
| --------------------- | ----------------------------------------------------------------------------------------- |
| Avatar fill           | `#00C48F` (Confident Green)                                                               |
| Name                  | "bob@company.com"                                                                         |
| Activity              | "1 snapshot on fix/token-expiry"                                                          |
| File path             | "auth-service/src/tokens.rs"                                                              |
| Stagger delay         | 200 ms after Alice's row start                                                            |
| Start                 | 3:34.400                                                                                   |
| Complete              | 3:34.800                                                                                   |
| All other properties  | Identical to Row 1                                                                        |

**Row 3 — Carol (3:34.600–3:35.000):**

| Property              | Value                                                                                     |
| --------------------- | ----------------------------------------------------------------------------------------- |
| Avatar fill           | `#FF8A3B` (warm orange — per Design System § 5.6 note, this is the ONLY appearance of this color in the entire video, used exclusively for Carol's avatar) |
| Name                  | "carol@company.com"                                                                       |
| Activity              | "3 snapshots on feature/billing"                                                          |
| File path             | "billing-engine/src/pricing.rs"                                                           |
| Stagger delay         | 200 ms after Bob's row start                                                              |
| Start                 | 3:34.600                                                                                   |
| Complete              | 3:35.000                                                                                   |
| All other properties  | Identical to Row 1                                                                        |

**Dashboard hold (3:35.000–3:42.000): 7 seconds.**

This hold is the longest single-visual hold in the entire video. It is intentional. The dashboard is the proof of concept — the viewer needs time to:
1. Recognize it as a team dashboard (1 s)
2. Read Alice's row and understand what it means (2 s)
3. Scan Bob's and Carol's rows (1 s)
4. Absorb the implication: the entire team's work is visible, in real time, without anyone doing anything special (3 s)

**SFX: None.** Let the music and narrator carry this moment. Adding SFX would cheapen it. The dashboard speaks for itself.

### SHOT 5C.4 — The Push (3:42.000–3:45.000)

This shot visualizes the critical distinction: staged ≠ pushed. Alice explicitly pushes when she's ready.

**Alice's row highlight (3:42.000–3:42.300):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Animation       | Alice's row gains a left border: 4 px solid `#00D4FF`, 300 ms, `ease-enter`                   |
| Row background  | Shifts from transparent to `#00D4FF` at 5 % (`#00D4FF0D`), creating a subtle highlight         |

**Push button (3:42.300–3:42.600):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Appearance      | A button slides in from the right side of Alice's row                                          |
| Button style    | Rounded rectangle (8 px radius), `#00D4FF` fill, 120 px × 48 px                               |
| Button text     | "Push" in `body` (32 px at 4K), `#0A0F1A` (Deep Navy — dark text on cyan button)              |
| Entry animation | `translateX(20px)` + `opacity(0)` → visible, 300 ms, `ease-enter`                             |
| Start           | 3:42.300                                                                                        |
| Complete        | 3:42.600                                                                                        |

**Button press (3:42.800–3:42.900):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Animation       | Button depresses: `scale(1.0)` → `scale(0.95)` → `scale(1.0)`, 100 ms, `ease-sharp`          |
| Visual          | Button brightness dims by 10 % on press, returns on release                                    |
| SFX             | Clean click at 3:42.800, −26 dB. Source: mechanical key press, high-pass filtered (>2 kHz), 40 ms |

**Snapshot flow animation (3:42.900–3:43.800):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Concept         | Alice's activity text ("3 staged snapshots on feature/oauth") transforms — the number "3" and "staged" text animate rightward, leaving the row and traveling to a new column |
| Visual          | Three small square icons (representing snapshots) slide from Alice's row to a new "Branch History" column that fades in on the right side of the dashboard |
| Icon style      | 16 px squares, `#00D4FF` fill, with rounded 4 px corners                                      |
| Motion path     | Horizontal slide from activity column to branch column, 600 ms per icon, staggered 100 ms     |
| Easing          | `ease-standard`                                                                                 |
| "Branch History" column header | "Branch History" in `code-sm`, `#6B7280`, appears at 3:42.900 (200 ms fade) |

**Checkmark (3:43.800–3:44.200):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Icon            | ✓ checkmark, 32 px, `#00C48F` (Confident Green)                                               |
| Position        | Right side of Alice's row, replacing the Push button                                           |
| Animation       | `scale(0)` → `scale(1.0)`, 200 ms, `ease-enter` — the checkmark "pops" into existence        |
| Start           | 3:43.800                                                                                        |
| SFX             | Soft chime at 3:43.800, −28 dB. Source: a single glass chime (G5, 784 Hz), 300 ms sustain, gentle decay |

**Activity text update (3:44.000–3:44.400):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Alice's activity| Changes from "3 staged snapshots on feature/oauth" to "Pushed to feature/oauth ✓"             |
| Animation       | Old text fades out (200 ms, `ease-exit`), new text fades in (200 ms, `ease-enter`)            |
| New text color  | `#00C48F` (Confident Green) — the push was successful                                          |

**Hold (3:44.400–3:45.000):** 600 ms hold. The dashboard shows Alice's work successfully pushed while Bob and Carol's work remains staged. This single frame tells the whole story: visibility without involuntary merge. You're always in control.

**Transition to Segment 5D:** Dashboard begins fading at 3:44.700 — `opacity(1)` → `opacity(0)`, 300 ms, `ease-exit`.

---

## Segment 5D — Feature Pillars (3:45–4:05, 20 seconds)

### Narrator

> "And because we built this from scratch, we built the things Git never had."

> "Native access control. Not bolted on. Built in. Define who can read, write, and merge — down to individual files — using simple config files that live right in your project."

> "File-level license compliance. Assign licenses to any path. MIT here. Proprietary there. The server enforces it."

> "Append-only history. No rebase. No force push. No rewriting the past. History is immutable."

> "And everything — your access config, your ignore patterns, your branch protection rules — it's all declarative. Files in your project. Version-controlled. Auditable."

### Visual Concept

Four feature cards appear one by one, stacked on the right side of the frame. On the left side, the architecture diagram from Segment 5B returns at 60 % scale as a persistent visual anchor — reminding the viewer that these features are BUILT INTO the architecture they just saw.

**Layout:**

| Element              | Grid Position                                                                    |
| -------------------- | -------------------------------------------------------------------------------- |
| Architecture diagram | Columns 1–6 (left half), scaled to 60 %, vertically centered                    |
| Feature cards        | Columns 7–12 (right half), right-aligned per Design System § 5.4                |
| Card-to-card gap     | 24 px vertical                                                                  |

**Architecture diagram return (3:45.000–3:45.600):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Animation       | Diagram scales from 40 % (its position in top-left from Segment 5C) to 60 %, repositions to left half of frame |
| Duration        | 600 ms, `ease-standard`                                                                        |
| Opacity         | Restores to 80 % (from the 60 % it was dimmed to in 5C)                                       |
| Connection line | Dash animation resumes / continues                                                             |
| Start           | 3:45.000                                                                                        |
| Complete        | 3:45.600                                                                                        |

### SHOT 5D.1 — Feature Card 1: Native Access Control (3:45.600–3:50.000)

**Card entry animation (per Design System § 5.4):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Transform       | `translateX(80px)` + `opacity(0)` → `translateX(0)` + `opacity(1)`                            |
| Duration        | 400 ms, `ease-enter`                                                                           |
| Start           | 3:45.600                                                                                        |
| Complete        | 3:46.000                                                                                        |

**Card style (per Design System § 5.4):**

| Property         | Value                                                                                          |
| ---------------- | ---------------------------------------------------------------------------------------------- |
| Width            | 800 px                                                                                         |
| Height           | Auto (content-driven), minimum 200 px                                                         |
| Background       | `#111827` (Code Background)                                                                   |
| Border           | 1 px solid `#374151` (Dim Gray)                                                               |
| Left accent      | 4 px solid `#00D4FF` (Accent Cyan), full card height                                          |
| Border radius    | 24 px                                                                                          |
| Padding          | 48 px all sides                                                                                |

**Card content:**

| Element          | Value                                                                                          |
| ---------------- | ---------------------------------------------------------------------------------------------- |
| Title            | "Native Access Control" in `heading-2` (Inter SemiBold, 64 px at 4K), `#F0F0F0`              |
| Title-to-body gap| 24 px                                                                                         |
| Code snippet     | A mini `.wt/access/policies.toml` code block:                                                 |

Code snippet content:

```
[[policy]]
name = "backend-team"
scope = { path = "src/crypto" }
permissions = ["tree:read", "tree:write"]
```

| Property                 | Value                                                                            |
| ------------------------ | -------------------------------------------------------------------------------- |
| Font                     | `code-sm` (JetBrains Mono 400, 28 px at 4K / 14 px at HD)                      |
| Snippet background       | `#0A0F1A` (Deep Navy), 12 px border-radius, 24 px padding                      |
| Syntax highlighting      | TOML keys (`name`, `scope`, `permissions`) in `#00D4FF`; string values (`"backend-team"`, `"src/crypto"`, `"tree:read"`, `"tree:write"`) in `#00C48F`; brackets (`[[`, `]]`, `{`, `}`, `[`, `]`) in `#6B7280`; operators (`=`) in `#F0F0F0` at 60 % |
| Code entry               | Lines appear top-to-bottom, 100 ms stagger per line, `ease-enter`, 200 ms each |
| Code start               | 3:46.200 (200 ms after card entry completes)                                    |
| Code complete            | 3:47.000                                                                         |

**Card hold:** 3:47.000–3:50.000 (3 s). The viewer reads the title and glances at the TOML structure. They don't need to parse every line — the presence of real configuration code proves depth.

### SHOT 5D.2 — Feature Card 2: File-Level License Compliance (3:50.000–3:55.000)

**Card entry (3:50.000–3:50.400):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Transform       | Same as Card 1: `translateX(80px)` + `opacity(0)` → visible, 400 ms, `ease-enter`             |
| Stagger         | 200 ms after Card 1's entry would have completed (cards stack, not replace)                    |
| Position        | Below Card 1, 24 px gap                                                                        |

**Card content:**

| Element          | Value                                                                                          |
| ---------------- | ---------------------------------------------------------------------------------------------- |
| Title            | "File-Level License Compliance" in `heading-2`, `#F0F0F0`                                     |
| Left accent      | 4 px solid `#00D4FF`                                                                          |

Code snippet:

```
[[license.path]]
path = "services/billing-engine"
license = "proprietary"
```

| Property                 | Value                                                                            |
| ------------------------ | -------------------------------------------------------------------------------- |
| Syntax highlighting      | Same scheme as Card 1, EXCEPT: the string `"proprietary"` is highlighted in `#FF3B3B` (Warning Red) — this is restricted code, and the red immediately signals danger/restriction |
| All other properties     | Identical to Card 1's code snippet                                               |

**Card hold:** 3:51.000–3:55.000 (4 s).

### SHOT 5D.3 — Feature Card 3: Append-Only History (3:55.000–4:00.000)

**Card entry (3:55.000–3:55.400):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Transform       | Same entry animation as Cards 1–2                                                              |
| Position        | Below Card 2, 24 px gap                                                                        |

> **Note:** With three cards stacked, if total height exceeds the available vertical space (rows 3–11 of the grid), Cards 1 and 2 should compress vertically by reducing their code snippet to 2 lines each, per Design System § 5.4 (max 3 cards visible at once).

**Card content:**

| Element          | Value                                                                                          |
| ---------------- | ---------------------------------------------------------------------------------------------- |
| Title            | "Append-Only History" in `heading-2`, `#F0F0F0`                                               |
| Left accent      | 4 px solid `#00D4FF`                                                                          |

Instead of a code snippet, this card uses a visual:

**Mini DAG (Directed Acyclic Graph):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Layout          | A horizontal chain of 5 commit nodes (circles, 16 px diameter), connected by 2 px lines       |
| Node color      | All nodes `#00C48F` (Confident Green) — healthy, intact history                                |
| Line color      | `#374151` (Dim Gray)                                                                            |
| Animation       | Nodes appear left-to-right, 150 ms stagger, 200 ms per node, `ease-enter`                     |
| DAG start       | 3:55.600                                                                                        |
| DAG complete    | 3:56.400                                                                                        |

**Crossed-out "force push":**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Text            | "~~--force~~" in `code` (36 px at 4K), `#FF3B3B`, with strikethrough line                      |
| Strikethrough   | 2 px line, `#FF3B3B`, draws left-to-right, 200 ms, `ease-sharp`                               |
| Position        | Below the DAG, left side                                                                        |
| Animation       | Fade in + strikethrough simultaneously, 300 ms total                                           |
| Start           | 3:56.600                                                                                        |

**Safe alternative:**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Text            | "wt revert ✓" in `code` (36 px at 4K), `#00C48F` (Confident Green)                            |
| Position        | Below the DAG, right side (horizontally opposite the crossed-out text)                         |
| Checkmark       | ✓ character in `#00C48F`, inline with text                                                      |
| Animation       | `opacity(0)` + `translateY(8px)` → visible, 300 ms, `ease-enter`                              |
| Start           | 3:56.900 (300 ms after the crossed-out text)                                                   |

The juxtaposition is clear: the destructive command is red and struck through; the safe command is green and affirmed.

**Card hold:** 3:57.200–4:00.000 (~2.8 s).

### SHOT 5D.4 — Feature Card 4: Declarative Everything (4:00.000–4:05.000)

**Card entry (4:00.000–4:00.400):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Transform       | Same entry animation                                                                           |
| Position        | Below Card 3, 24 px gap. If vertical space is exhausted, Card 1 scrolls upward and off-frame (200 ms, `ease-exit`) to make room. |

**Card content:**

| Element          | Value                                                                                          |
| ---------------- | ---------------------------------------------------------------------------------------------- |
| Title            | "Declarative Everything" in `heading-2`, `#F0F0F0`                                            |
| Left accent      | 4 px solid `#00D4FF`                                                                          |

File tree content:

```
.wt/
├── config.toml
├── ignore
└── access/
    ├── roles.toml
    └── policies.toml
```

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Font            | `code-sm` (JetBrains Mono 400, 28 px at 4K / 14 px at HD)                                     |
| Color           | `#F0F0F0` at 70 % (`#F0F0F0B3`)                                                               |
| ".wt/" color    | `#00D4FF` (Accent Cyan) — highlights the W0rkTree directory                                    |
| ".toml" files   | `#00C48F` for `.toml` extension text — these are the declarative config files                   |
| Tree characters | `├`, `└`, `─`, `│` in `#374151` (Dim Gray)                                                     |
| Animation       | Lines appear top-to-bottom, 100 ms stagger per line, `ease-enter`, 200 ms each                 |
| Tree start      | 4:00.600                                                                                        |
| Tree complete   | 4:01.400                                                                                        |

**Card hold:** 4:01.400–4:05.000 (3.6 s).

**All four feature cards visible state (4:01.400–4:05.000):**

The right side of the frame displays four stacked cards (or the top three + fourth if scrolled). The left side shows the architecture diagram at 60 %. The composition is balanced: technical architecture left, human-readable features right. The viewer understands that these features aren't bolted on — they're part of the architecture.

---

## Segment 5E — Architecture Complete + Protocol (4:05–4:15, 10 seconds)

### Narrator

> "One protocol. Encrypted. Authenticated. No separate SSH setup. No PAT tokens. No choosing between HTTPS and SSH and hoping the firewall cooperates."

### Visual Concept

The feature cards exit. The architecture diagram returns to full size for its final, definitive presentation. The connection line between LOCAL and REMOTE is emphasized — this is the protocol story.

### SHOT 5E.1 — Feature Cards Exit + Diagram Restore (4:05.000–4:06.200)

**Feature cards exit (per Design System § 5.4):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Transform       | `translateX(0)` + `opacity(1)` → `translateX(-40px)` + `opacity(0)`                           |
| Duration        | 400 ms per card, `ease-exit` (`cubic-bezier(0.4, 0.0, 1.0, 1.0)`)                            |
| Stagger         | 100 ms between cards (faster exit than entry — per Design System § 5.4)                       |
| Card 1 start    | 4:05.000                                                                                        |
| Card 2 start    | 4:05.100                                                                                        |
| Card 3 start    | 4:05.200                                                                                        |
| Card 4 start    | 4:05.300                                                                                        |
| All exit done   | 4:05.700                                                                                        |

**Architecture diagram scale-up (4:05.400–4:06.200):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Animation       | Diagram scales from 60 % to 100 %, repositions to centered on canvas                          |
| Duration        | 800 ms, `ease-standard`                                                                        |
| Opacity         | Restores to 100 % (all elements at full brightness)                                           |
| Start           | 4:05.400 (overlaps slightly with card exit — the diagram grows as cards leave)                 |
| Complete        | 4:06.200                                                                                        |

### SHOT 5E.2 — Protocol Emphasis (4:06.200–4:12.000)

**Connection line enhancement:**

| Property              | Value                                                                                       |
| --------------------- | ------------------------------------------------------------------------------------------- |
| Line stroke           | Thickens from 3 px to 4 px over 400 ms, `ease-standard` (subtle emphasis)                  |
| Dash animation        | Speed increases: cycle time reduces from 2000 ms to 1200 ms over 600 ms (data flowing faster — bidirectional now) |
| Bidirectional flow    | A second set of dashes animates right-to-left simultaneously (data now flows both ways)     |
| Glow effect           | The connection line gains a 6 px `#00D4FF` glow (Gaussian blur, 30 % opacity) — it becomes the brightest element on screen |

**"QUIC" label update (4:06.200–4:06.800):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Old label       | "QUIC" fades out (200 ms, `ease-exit`)                                                         |
| New label       | "TLS / QUIC" in `code` (36 px at 4K), `#00D4FF`, fades in (400 ms, `ease-enter`)             |
| Position        | Centered above the connection line                                                              |

**Protocol properties (staggered, 4:07.000–4:08.200):**

Three property words appear below the connection line, staggered 200 ms apart:

| Word              | Start     | Complete  | Style                                                          |
| ----------------- | --------- | --------- | -------------------------------------------------------------- |
| "Encrypted"       | 4:07.000  | 4:07.400  | `body` (32 px at 4K), `#00C48F` (Confident Green)             |
| "•"               | —         | —         | Inline separator between words, `#374151`, 8 px                |
| "Authenticated"   | 4:07.200  | 4:07.600  | Same style as "Encrypted"                                      |
| "•"               | —         | —         | Inline separator                                               |
| "Multiplexed"     | 4:07.400  | 4:07.800  | Same style                                                     |

Each word: `opacity(0)` + `translateY(8px)` → visible, 400 ms, `ease-enter`.

The three words form a single centered line below the connection: "Encrypted • Authenticated • Multiplexed"

**Heartbeat glow pulse (4:08.500–4:10.500):**

| Property              | Value                                                                                       |
| --------------------- | ------------------------------------------------------------------------------------------- |
| Effect                | ALL `#00D4FF` elements in the diagram simultaneously pulse: 80 % → 100 % → 80 % opacity   |
| Duration              | 2000 ms total (1000 ms brightening, 1000 ms dimming)                                       |
| Easing                | `ease-standard`                                                                             |
| Elements affected     | "worktree-worker" label, "worktree-server" label, connection line, "TLS / QUIC" label, all `#00D4FF` file paths |
| Meaning               | A "heartbeat" — the system is alive, connected, flowing                                     |

### SHOT 5E.3 — Final Hold (4:10.500–4:15.000)

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Duration        | 4.5 seconds                                                                                    |
| Diagram state   | Fully complete, fully visible, connection line animating bidirectionally                        |
| Ambient motion  | Connection line dashes flow continuously. One additional subtle data pulse (glow traveling left-to-right along the line, #00D4FF at 30 %, 600 ms travel, `ease-standard`) at 4:12.000 |
| Music           | Reaches its peak resolution here (see Audio Design)                                            |
| Viewer task     | Absorb. This is the last time the architecture diagram appears at full size. Let it breathe.   |

**Transition to Act IV (4:14.500–4:15.000):**

| Property        | Value                                                                                           |
| --------------- | ----------------------------------------------------------------------------------------------- |
| Transition type | **Hard cut** to a terminal at 4:15.000                                                         |
| Preparation     | At 4:14.500, the diagram's glow effects begin dimming (500 ms, `ease-exit`) — a subtle visual cue that the scene is ending, but the hard cut arrives before the dim completes, creating a snappy, confident transition |
| Audio           | Music begins its volume reduction at 4:12.000 (see Audio Design)                              |

---

## Dialogue — Complete for Act III

Full narrator script for this scene, continuous:

> This is W0rkTree. Not a Git wrapper. Not a Git extension. Not a hosting platform for Git repos. A ground-up replacement for Git — with a migration bridge so you can bring your existing repos with you.
>
> W0rkTree runs two systems. On your machine — a background process we call the worker. It watches your files. It snapshots your work automatically. It handles branching, diffing, merging — everything you'd expect from version control — all locally, all instantly.
>
> On the server — a multi-tenant platform that your whole team connects to. It stores canonical history, enforces access control, manages tenants and teams — and does something Git has never done.
>
> When the worker captures a snapshot of your work — whether automatic or manual — it syncs that snapshot to the server as a staged snapshot. Not pushed. Not merged. Staged. Your team can see what you're working on and which files you're touching — in real time — without you doing anything.
>
> Think about what that means. No more "I had no idea you were working on that file." No more merge conflicts that could have been prevented with thirty seconds of awareness. The entire team sees the full picture of what's in flight — and when you're ready, you explicitly push your staged work to the branch. Visible doesn't mean merged. Staged doesn't mean pushed. You're always in control.
>
> And because we built this from scratch, we built the things Git never had.
>
> Native access control. Not bolted on. Built in. Define who can read, write, and merge — down to individual files — using simple config files that live right in your project.
>
> File-level license compliance. Assign licenses to any path. MIT here. Proprietary there. The server enforces it.
>
> Append-only history. No rebase. No force push. No rewriting the past. History is immutable.
>
> And everything — your access config, your ignore patterns, your branch protection rules — it's all declarative. Files in your project. Version-controlled. Auditable.
>
> One protocol. Encrypted. Authenticated. No separate SSH setup. No PAT tokens. No choosing between HTTPS and SSH and hoping the firewall cooperates.

**Word count:** ~315 words across 90 seconds = ~3.5 words/second (comfortable, unhurried pace with room for dramatic pauses).

**Key narrator direction notes:**
- "This is W0rkTree." — confident, declarative. Not loud. Calm authority.
- "Not a Git wrapper." — each denial is delivered with slight emphasis on "Not", but not aggressive. Think dismissive-of-the-misconception, not combative.
- "A ground-up replacement." — this is the line. Slower delivery. Let it land.
- "and does something Git has never done." — pause for 1 beat after this line. The pause creates anticipation.
- "Staged." — isolated, emphasized. The single word carries the weight.
- "You're always in control." — the emotional anchor of the segment. Deliver with warmth, not sternness.
- Feature descriptions — slightly faster pace. These are a rapid-fire proof of depth. The energy is "look at all the things we built" without breathlessness.
- "One protocol." — return to the slower, confident pace. This is the closing statement.

---

## Audio Design

### Music

**Overview:** Act III's music is the emotional inverse of Act II. Where Act II was tense, discordant, and building pressure, Act III is open, confident, and resolving. The key shifts from minor to major. The texture shifts from electronic tension to clean, airy synthesis.

**Reference artists:** Tycho, Bonobo, Jon Hopkins (the quieter moments), Ólafur Arnalds. Clean, modern, electronic but warm. Not EDM. Not ambient. Melodic electronic with clear harmonic progression.

**BPM:** 90 (consistent throughout Act III — this is slightly faster than the 60 BPM heartbeat of Act II, creating a sense of forward momentum without urgency).

**Key:** D major (or relative mode). The major tonality signals resolution. If Act II ended in D minor, Act III opens in D major — a classic Picardy-third-style emotional shift.

| Timecode        | Music Direction                                                                                |
| --------------- | ---------------------------------------------------------------------------------------------- |
| 2:45.000–2:55.000 | **The Act III theme opens.** A clean, open synth chord (D major). Spacious. Confident. Not bombastic — controlled. A single sustained pad with gentle high-frequency shimmer. The pad swells gradually from −30 dB to −18 dB over 10 seconds. Think the opening chord of a Tycho track: warm, wide stereo, long sustain. No rhythm yet — just harmony and space. The contrast with Act II's dark tension should be immediate and visceral. |
| 2:55.000–3:20.000 | **The theme builds.** At 2:55, a subtle beat enters: a clean electronic kick on beat 1 of each bar (90 BPM = one kick every 2.67 s). At 3:03 (connection line draw), a hi-hat pattern joins at half the kick's volume. Each time a diagram node appears, the music adds a small textural element — a pluck, a blip, a shimmer — synchronized to the visual "snap" of nodes appearing. These elements are quiet (−32 dB to −28 dB) and serve as ear candy, not primary melody. By 3:15, the beat is fully established: kick, hat, a subtle bass line (arpeggiated, 8th notes, following the chord progression), and the sustained pad from the opening. |
| 3:20.000–3:23.000 | **Momentary lift.** When "STAGED SNAPSHOT VISIBILITY" appears, the music pulls back for exactly 1 beat (~667 ms at 90 BPM). The kick drops out. The hat drops out. Only the pad sustains. This creates a "gasp" — a moment of silence that gives the bell SFX room to ring. At the end of the beat, the kick returns with slightly more presence (compressed harder, +2 dB from before the drop). |
| 3:23.000–3:45.000 | **Emotional peak.** This is the heart of the entire video's soundtrack. The melody — a clear, singable synth lead (saw wave, detuned, with light chorus) — enters for the first time. The melody plays a simple 8-bar phrase that ascends through the chord progression. Underneath: the full beat (kick, hat, bass arp, pad). Above: occasional high-register arpeggiated chimes that cascade downward. This section should be **memorable**. If the viewer remembers any piece of music from this video, it should be the melody that plays during the team dashboard reveal (3:33–3:42). By 3:42, the melody has reached its highest note and sustains. |
| 3:45.000–4:05.000 | **Sustained energy.** The melody drops out (replaced by the pad in a higher register). The beat continues unchanged. Energy level: 80 % of the 3:23–3:45 peak. The information density is high here (feature cards); the music provides continuity without competing with the narration. If a listener were to hear only the music, this section should feel like the "verse" after a "chorus." |
| 4:05.000–4:12.000 | **Resolution.** The chord progression reaches its final resolution — typically the tonic (D major). The melody returns for a brief 4-bar reprise, ending on the root note. All musical elements are playing: pad, kick, hat, bass arp, melody, high chimes. This is the fullest the music has been in the entire video. The soundtrack is resolved, confident, complete — matching the visual of the complete architecture diagram. |
| 4:12.000–4:15.000 | **Fade begins.** The beat elements (kick, hat, bass) begin fading out over 3 seconds. By 4:15 (hard cut to Act IV), only the pad remains at −24 dB, creating a clean transition into the quieter terminal demo. The music does NOT fully stop — it sustains at very low volume into Act IV, providing continuity. |

### SFX Spotting Sheet

| Timecode        | Sound Description                            | Level     | Pan     | Duration  | Source / Character                                       |
| --------------- | -------------------------------------------- | --------- | ------- | --------- | -------------------------------------------------------- |
| 2:47.400        | Denial strikethrough #1                      | −28 dB    | Center  | 80 ms     | High-pass filtered white noise burst, fast decay         |
| 2:48.900        | Denial strikethrough #2                      | −28 dB    | Center  | 80 ms     | Same as #1                                               |
| 2:50.400        | Denial strikethrough #3                      | −28 dB    | Center  | 80 ms     | Same as #1                                               |
| 2:55.0–2:58.6   | LOCAL diagram node snaps (×8 nodes/items)   | −32 dB ea | 15 % L  | 50 ms ea  | Clean digital click, 5 ms attack, 50 ms decay, 6 kHz    |
| 2:59.2          | Local Worktree sub-node snap                 | −32 dB    | 15 % L  | 50 ms     | Same click                                               |
| 3:00.000        | Connection line whoosh                       | −28 dB    | Center  | 800 ms    | Filtered sweep, 200 Hz → 4 kHz, fast tail-off            |
| 3:03.0–3:07.6   | REMOTE diagram node snaps (×10 nodes/items) | −32 dB ea | 15 % R  | 50 ms ea  | Same click as LOCAL, panned right                        |
| 3:20.300        | "Staged Snapshot Visibility" bell tone       | −20 dB    | Center  | 1400 ms   | Synthesized bell: C4 + G4 + E5 harmonics, singing bowl   |
| 3:42.800        | Push button click                            | −26 dB    | Center  | 40 ms     | Mechanical key press, high-pass >2 kHz                   |
| 3:43.800        | Checkmark chime                              | −28 dB    | Center  | 300 ms    | Single glass chime, G5 (784 Hz), gentle decay            |

**SFX Mixing Rules for Act III:**
- All SFX in Act III are **quieter and cleaner** than Act II SFX. Act II's sounds were harsh and alarming (error beeps, crashes). Act III's sounds are precise and crystalline (clicks, chimes, tones).
- No SFX during the dashboard hold (3:35–3:42). Silence = emphasis.
- The bell tone at 3:20.3 is the loudest SFX in Act III (−20 dB). It marks the most important single moment.
- All SFX are mixed to avoid masking the narrator. If a SFX and a narrator syllable land on the same frame, the SFX ducks by 6 dB.

---

## Color Palette for This Scene

Act III uses the **standard W0rkTree palette** — no scene-specific colors (unlike Act I's sepia palette). The dominant shift is in HOW the colors are weighted:

| Color Role              | Hex       | Scene Weight | Notes                                                            |
| ----------------------- | --------- | ------------ | ---------------------------------------------------------------- |
| Deep Navy (Background)  | `#0A0F1A` | Dominant     | Every background surface                                         |
| Accent Cyan (Brand)     | `#00D4FF` | Primary      | **This is the first scene where Cyan is the dominant accent.** In Act II, Warning Red dominated. In Act III, Cyan replaces it entirely. |
| Pure White (Text)       | `#F0F0F0` | Heavy        | All primary text, card titles, dashboard names                   |
| Confident Green         | `#00C48F` | Secondary    | Success states, positive flow steps, DAG nodes, checkmarks       |
| Code Background         | `#111827` | Supporting   | Card fills, dashboard fill, diagram container fills              |
| Muted Gray              | `#6B7280` | Supporting   | Labels, metadata, secondary descriptions, struck-through denials |
| Dim Gray                | `#374151` | Tertiary     | Borders, separators, tree characters, secondary connection lines |
| Warning Red             | `#FF3B3B` | **Minimal**  | ONLY used for: (1) strikethrough lines in Segment 5A, (2) `"proprietary"` highlight in Card 2, (3) `--force` strikethrough text in Card 3. Three appearances total. Its near-absence is the point — this is the solution space, not the problem space. |
| Carol's Orange          | `#FF8A3B` | **Singular** | ONLY Carol's avatar circle in the dashboard. Nowhere else.       |

> **The color temperature shift from Act II to Act III is the single most important visual signal in the video.** On the hard cut at 2:45, the viewer's subconscious should register: "something changed." The warm-to-cool shift (Act I → II) already happened at 0:55. Now the dark-and-red to dark-and-cyan shift (Act II → III) completes the journey. The viewer has traveled from nostalgia (warm) through frustration (red) to resolution (cyan).

---

## Editorial Notes

### Information Density Management

Act III has the highest information density in the entire video — approximately 315 words of narration, 5 distinct visual sequences, 4 feature cards, and 1 full architecture diagram, all in 90 seconds. The editor must ensure:

1. **Every visual serves the narration.** If a visual and the narration are telling different stories at any moment, the visual loses. The narration is the primary information channel; the visuals reinforce and extend it.

2. **The architecture diagram (Segment 5B) is the MOST IMPORTANT VISUAL in the entire video.** It must be absolutely clear, with no unnecessary elements. Every label must be readable at 1080p. Every connection must be obvious. If a viewer pauses the video on this frame, they should be able to understand W0rkTree's architecture from the diagram alone.

3. **The team dashboard (SHOT 5C.3) is the emotional money shot.** This is the moment where the viewer goes "oh, THAT'S what this solves." Linger here. Give it a full 7-second hold. Do NOT rush past it. Do NOT overlay text on it. Let the dashboard speak.

4. **Feature cards (Segment 5D) are rapid but not rushed.** Each card gets ~5 seconds — long enough to read the title and glance at the content, not long enough to read every TOML line. That's intentional. The cards prove depth without requiring comprehension. The viewer should feel "they've thought of everything" without needing to understand every config option.

5. **The architecture diagram must be revisitable.** It appears three times: full-size at 5B, compressed at 5C/5D, full-size again at 5E. Each return should feel like coming home to a familiar structure, not like seeing it for the first time again. Maintain visual consistency across all three appearances — same positions, same colors, same proportions.

### Pacing

| Segment | Information Type    | Pacing Directive                                                   |
| ------- | ------------------- | ------------------------------------------------------------------ |
| 5A      | Conceptual (what)   | **Slow.** Let the declaration land. 10 seconds for one idea.       |
| 5B      | Structural (how)    | **Measured.** Build piece by piece. Never rush the diagram.        |
| 5C      | Innovative (why)    | **Deliberate then lingering.** Build the concept, then hold on the proof (dashboard). |
| 5D      | Supportive (also)   | **Brisk.** Four cards, 5 seconds each. Energy is "and also this, and this, and this." |
| 5E      | Culminating (all)   | **Slow again.** Return to the full picture. Let it breathe. End with confidence, not speed. |

### Critical Viewer Moments

These are the moments where the viewer either "gets it" or loses interest. The edit must protect them:

1. **2:51.5 — "A ground-up replacement."** The viewer decides if they believe us. The visual must be clean and authoritative. No clutter.

2. **3:15 — "and does something Git has never done."** The pause after this line is critical. The diagram is complete but the innovation hasn't been named. The viewer leans in. Do not fill this pause with visual noise.

3. **3:20 — "STAGED SNAPSHOT VISIBILITY"** The bell tone rings. The text lands. This is the thesis of the entire product. If the viewer only remembers one thing from this video, it should be this concept.

4. **3:35 — Dashboard fully visible.** Seven seconds of holding. The viewer scans the rows, reads the names, sees the file paths. They make the connection: "my team could have this." This is where desire is created.

5. **3:43.8 — Alice pushes.** The checkmark appears. The distinction between "visible" and "merged" becomes concrete. This is where trust is built: "I'm still in control."

---

## Technical Notes

### Rendering

- **Frame rate:** All animations rendered at 60 fps for smooth motion. Final delivery at the project's master frame rate (see Production Bible § Render & Delivery Specs).
- **Resolution:** All specifications in this document are in 4K (3840 × 2160) units. Divide by 2 for HD (1920 × 1080). All elements must be resolution-independent (vector / procedural) except for icons, which should be SVG.

### Architecture Diagram Persistence

The architecture diagram exists in three states across Act III:

| State       | Timecode    | Scale | Opacity | Position       |
| ----------- | ----------- | ----- | ------- | -------------- |
| Full build  | 2:55–3:20   | 100 % | 100 %   | Centered       |
| Compressed  | 3:20–3:45   | 40 %  | 60 %    | Top-left       |
| Side anchor | 3:45–4:05   | 60 %  | 80 %    | Left half      |
| Full return | 4:05–4:15   | 100 % | 100 %   | Centered       |

Each transition between states must be smooth (`ease-standard`, 600–800 ms). The diagram is NEVER rebuilt — it is always the same instance, just repositioned and rescaled. The connection line dash animation runs continuously across all states without interruption.

### Font Loading

Act III uses all three font stacks:
- **Inter Tight** — "STAGED SNAPSHOT VISIBILITY" impact text (Segment 5C), logo (Segment 5A)
- **Inter** — All headings, body text, card titles, dashboard text, flow diagram labels
- **JetBrains Mono** — TOML code snippets (Cards 1–2), file trees (Card 4, SHOT 5B.1), protocol labels, `--force` / `wt revert` text (Card 3)

All fonts must be loaded before frame 1 of Act III renders. No FOUT (flash of unstyled text) is acceptable.

### Dashboard Mockup

The dashboard (SHOT 5C.3) is a **custom motion graphic**, not a screenshot of a real application. This is deliberate:
- A real screenshot would require a real application in a real state, creating a maintenance burden
- A motion graphic can be styled to exactly match the video's design system
- Typography, spacing, and color can be controlled to the pixel

The dashboard must look **plausibly real** — like a web application that could exist — without being a literal representation of any specific UI that's been built. It should feel aspirational but grounded.

### TOML Syntax Highlighting

Feature cards 1 and 2 contain TOML code. The syntax highlighting scheme:

| TOML Element     | Color                                         |
| ---------------- | --------------------------------------------- |
| Table headers    | `#6B7280` (`[[policy]]`, `[[license.path]]`)  |
| Key names        | `#00D4FF` (`name`, `scope`, `permissions`)    |
| String values    | `#00C48F` (`"backend-team"`, `"src/crypto"`)  |
| Special strings  | `#FF3B3B` (`"proprietary"` only)              |
| Operators        | `#F0F0F0` at 60 % (`=`)                       |
| Brackets/braces  | `#6B7280` (`{`, `}`, `[`, `]`)               |
| Comments         | `#6B7280` (if any appear)                     |

This highlighting scheme is consistent across all TOML appearances in the video.

### Accessibility

- All text meets WCAG AA contrast requirements against its background:
  - `#F0F0F0` on `#0A0F1A`: contrast ratio 15.9:1 ✓
  - `#00D4FF` on `#0A0F1A`: contrast ratio 9.7:1 ✓
  - `#00C48F` on `#0A0F1A`: contrast ratio 8.4:1 ✓
  - `#6B7280` on `#0A0F1A`: contrast ratio 4.1:1 ✓ (AA for large text)
  - `#6B7280` on `#111827`: contrast ratio 3.4:1 — **below AA for body text**. Acceptable because these are secondary/tertiary labels, and the video format provides narrator audio as the primary information channel. In static screenshots (social cuts), consider boosting these labels to `#9CA3AF` for better contrast.
- The strikethrough red (`#FF3B3B`) on gray text (`#6B7280`) in Segment 5A is a decorative element, not informational — the denial is communicated by the narrator, not solely by the color of the line. Colorblind viewers will understand the denials from the strikethrough motion and the narrator's "Not a…" phrasing.