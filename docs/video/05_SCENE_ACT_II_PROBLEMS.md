# Scene 4 — Act II: The Five Broken Things

| Field | Detail |
|---|---|
| **Document** | `05_SCENE_ACT_II_PROBLEMS.md` |
| **Parent** | `00_PRODUCTION_BIBLE.md` |
| **Version** | 1.0 |
| **Status** | PRE-PRODUCTION |
| **Timecode** | 1:30–2:45 |
| **Duration** | 75 seconds |
| **Resolution** | 3840×2160 (4K master) · 1920×1080 (HD delivery) |
| **Frame Rate** | 60 fps |

---

## Table of Contents

1. [Scene Overview](#scene-overview)
2. [Segment Breakdown](#segment-breakdown)
   - [Problem 1 — The Jargon Wall (1:30–1:42)](#problem-1--the-jargon-wall-130142)
   - [Problem 2 — The Invisible Team (1:42–1:55)](#problem-2--the-invisible-team-142155)
   - [Problem 3 — Destruction Is One Command Away (1:55–2:10)](#problem-3--destruction-is-one-command-away-155210)
   - [Problem 4 — The Security Vacuum (2:10–2:24)](#problem-4--the-security-vacuum-210224)
   - [Problem 5 — The Monorepo Meltdown (2:24–2:38)](#problem-5--the-monorepo-meltdown-224238)
   - [Bridge — The Question (2:38–2:45)](#bridge--the-question-238245)
3. [Dialogue — Complete Narrator Script](#dialogue--complete-narrator-script)
4. [Audio Design](#audio-design)
5. [Color Palette for This Scene](#color-palette-for-this-scene)
6. [Reusable Component: Number Card](#reusable-component-number-card)
7. [Editorial Notes](#editorial-notes)
8. [Technical Notes](#technical-notes)

---

## Scene Overview

**Purpose**: Articulate Git's structural failures. Not nitpicks — fundamental problems that every developer has experienced. Each problem is visceral, immediately recognizable, and impossible to dismiss.

**Emotional Arc**: Frustration → Frustration → Frustration → Frustration → Frustration → Hope (the bridge question). The audience should feel the cumulative weight of five consecutive failures stacking on top of each other. By the time the bridge arrives at 2:38, the viewer is desperate for a way out — and we give them one.

**Structure**: Five numbered problems, each composed of a title card slam + supporting visual + punchline card, followed by a seventh-second bridge that pivots the emotional energy of the entire video from critique to creation.

**Visual Palette**: This scene is dominated by Warning Red (`#FF3B3B`), Muted Gray (`#6B7280`), and Deep Navy (`#0A0F1A`). The brand Accent Cyan (`#00D4FF`) is withheld for the entire 68 seconds of problems — it appears ONLY in the bridge's final line at 2:42, marking the first visual signal that W0rkTree is coming. This deliberate absence makes the cyan arrival feel like relief.

**Pacing**: Each problem gets 12–15 seconds. This is TIGHT. The editor must never linger on a visual that has already communicated its point. Cut as soon as the audience gets it.

---

## Design System Reference

### Colors

| Token | Hex | Usage in This Scene |
|---|---|---|
| Deep Navy | `#0A0F1A` | Background for all shots, clean slate between problems |
| Code Background | `#111827` | Card fills, block fills, terminal chrome |
| White | `#F0F0F0` | Primary text, term labels, narrator-synced text |
| Accent Cyan | `#00D4FF` | **Bridge only** — Line 2 of the closing question (2:42) |
| Warning Red | `#FF3B3B` | Number cards, danger indicators, CONFLICT text, punchlines |
| Confident Green | `#00C48F` | Healthy DAG nodes (before destruction), Alice's label color |
| Muted Gray | `#6B7280` | Secondary text, damaged nodes, protocol critique |
| Dim Gray | `#374151` | Borders, connecting lines, structural elements |

### Typography

| Token | 4K Size | HD Size | Family | Weight | Usage |
|---|---|---|---|---|---|
| `display-xl` | 192px | 96px | Inter Tight | 900 (Black) | Problem numbers (1–5) |
| `display-lg` | 128px | 64px | Inter Tight | 800 | CONFLICT text, major impacts |
| `heading-1` | 96px | 48px | Inter | 700 | Problem subtitles, bridge question |
| `heading-2` | 64px | 32px | Inter | 600 | Punchline text, jargon terms, supporting headings |
| `heading-3` | 48px | 24px | Inter | 600 | Panel labels (Alice, Bob), block labels |
| `body-lg` | 40px | 20px | Inter | 400 | Badge labels, secondary descriptions |
| `body` | 32px | 16px | Inter | 400 | Tertiary labels, small annotations |
| `code-lg` | 48px | 24px | JetBrains Mono | 400 | File paths, Git commands, protocol text |
| `code` | 32px | 16px | JetBrains Mono | 400 | Terminal content, code snippets |

### Easing Curves

| Token | Value | Usage |
|---|---|---|
| `ease-standard` | `cubic-bezier(0.4, 0.0, 0.2, 1.0)` | General motion, element positioning, block stacking |
| `ease-enter` | `cubic-bezier(0.0, 0.0, 0.2, 1.0)` | Text fade-ins, element entrances, opacity transitions |
| `ease-bounce` | `cubic-bezier(0.34, 1.56, 0.64, 1.0)` | Number card slams, CONFLICT impact, emphasis moments |
| `ease-sharp` | `cubic-bezier(0.4, 0.0, 0.6, 1.0)` | Destructive command entrances, aggressive motion |

---

## Segment Breakdown

---

### PROBLEM 1 — THE JARGON WALL (1:30–1:42)

**Duration**: 12 seconds
**Emotional Beat**: Confusion, overwhelm, exasperation
**Narrator**: "Number one. Git has a jargon problem. Ref. Refspec. HEAD. Detached HEAD. Origin. Upstream. Index. Staging area. Working tree. Stash. The thing is — half of these are synonyms for each other, and the other half mean completely different things depending on context. Git checkout — one command — does five completely different things depending on what flags you pass it. That's not power. That's bad design wearing a trenchcoat."

---

#### SHOT 4A.1 — Number Card Slam

| Property | Value |
|---|---|
| **Timecode** | 1:30.000–1:32.000 |
| **Duration** | 2.000s |
| **Shot Type** | Full-frame centered typography |
| **Purpose** | Structural anchor — signals the first item in a five-part list |

**Layout**:
- Number `1` centered both horizontally and vertically in frame
- Typography: `display-xl` (192px at 4K / 96px at HD)
- Color: Warning Red `#FF3B3B`
- Background: Deep Navy `#0A0F1A` with a subtle radial glow behind the number
  - Glow: Warning Red at 8% opacity, 600px radius (4K), centered on the number's midpoint
  - The glow is atmospheric, not distracting — it tints the background, not the number itself

**Animation Sequence**:

| Time Offset | Element | Property | From | To | Duration | Easing |
|---|---|---|---|---|---|---|
| 0ms | Number `1` | scale | 0.8 | 1.0 | 400ms | `ease-bounce` |
| 0ms | Number `1` | opacity | 0.0 | 1.0 | 400ms | `ease-bounce` |
| 0ms | Red glow | opacity | 0.0 | 0.08 | 400ms | `ease-enter` |
| 400ms | Subtitle | opacity | 0.0 | 1.0 | 600ms | `ease-enter` |
| 400ms | Subtitle | translate-y | +16px | 0px | 600ms | `ease-enter` |

**Subtitle**:
- Text: `THE JARGON WALL`
- Typography: `heading-1` (96px at 4K / 48px at HD)
- Color: `#F0F0F0`
- Position: centered horizontally, 64px below the number's baseline (4K spacing)
- Appears 400ms after the number lands

**Hold**: Both number and subtitle remain fully visible for 800ms (from t+1000ms to t+1800ms) before transitioning to the next shot.

**SFX**: Percussive hit at 1:30.000 — a layered low tom + kick drum, sharp attack (<10ms), short decay (200ms), mixed at −18dB. This same percussive hit is reused for all five number cards (see [Reusable Component: Number Card](#reusable-component-number-card)).

**Transition In**: Hard cut from Act I's final frame (Deep Navy black). The number card slam IS the transition.

---

#### SHOT 4A.2 — Jargon Cascade

| Property | Value |
|---|---|
| **Timecode** | 1:32.000–1:38.000 |
| **Duration** | 6.000s |
| **Shot Type** | Full-frame particle typography |
| **Purpose** | Visualize the overwhelming volume and redundancy of Git's vocabulary |

**Transition In**: The number card (number + subtitle + glow) fades out over 300ms (opacity 1.0→0.0, `ease-standard`). The jargon visualization begins building immediately.

**Term List** (17 terms total):

| # | Term | Typography | Starting Opacity | Entry Origin | Synonym Group |
|---|---|---|---|---|---|
| 1 | `ref` | `heading-2` | 80% | Top-left | — |
| 2 | `refspec` | `heading-2` | 70% | Top-right | — |
| 3 | `HEAD` | `heading-2` | 100% | Bottom-center | — |
| 4 | `detached HEAD` | `heading-2` | 90% | Left | — |
| 5 | `origin` | `heading-2` | 60% | Right | — |
| 6 | `upstream` | `heading-2` | 75% | Top-center | — |
| 7 | `index` | `heading-2` | 85% | Bottom-left | **Group A** |
| 8 | `staging area` | `heading-2` | 80% | Bottom-right | **Group A** |
| 9 | `working tree` | `heading-2` | 70% | Top-left | — |
| 10 | `stash` | `heading-2` | 65% | Right | — |
| 11 | `cherry-pick` | `heading-2` | 75% | Bottom-center | — |
| 12 | `rebase` | `heading-2` | 90% | Left | — |
| 13 | `fetch` | `heading-2` | 80% | Top-right | — |
| 14 | `pull` | `heading-2` | 85% | Bottom-left | — |
| 15 | `checkout` | `heading-2` | 100% | Top-center | **Group B** |
| 16 | `switch` | `heading-2` | 80% | Right | **Group B** |
| 17 | `restore` | `heading-2` | 75% | Bottom-right | **Group B** |

**Term Color**: All terms render in `#F0F0F0` at their specified starting opacity.

**Animation — Individual Term Entrance**:
- Each term floats in from its Entry Origin toward the center of the frame
- Motion: `ease-standard`, 800ms duration per term
- Terms are staggered 120ms apart (Term 1 starts at t+0ms, Term 2 at t+120ms, Term 3 at t+240ms, etc.)
- Total cascade duration: 120ms × 16 intervals + 800ms final animation = 2,720ms
- Terms do NOT stop at center — they drift slowly past center and settle at semi-random positions within the central 60% of frame, creating a dense cluster

**Animation — Synonym Flash**:
- When both members of a synonym group have arrived on screen, they simultaneously flash to Warning Red at 50% opacity (`#FF3B3B` at 50%) for 400ms, then return to `#F0F0F0`
- **Group A** (`index` ↔ `staging area`): flash occurs at approximately t+1,760ms (when Term 8 arrives)
- **Group B** (`checkout` ↔ `switch` ↔ `restore`): flash occurs at approximately t+2,720ms (when Term 17 arrives)
- The flash is a brief visual cue — the viewer should subconsciously register "wait, those are the same thing?" without needing to read every label
- Flash easing: instant on (0ms), `ease-standard` fade back to white (400ms)

**Collision and Overlap Behavior**:
- As more terms arrive, they begin overlapping. No collision avoidance — intentional visual chaos
- Terms settle at varying z-depths (simulate depth with slight scale variation: 0.9–1.1×)
- By t+3,000ms, the screen should feel FULL — a wall of overlapping Git terminology that is visually overwhelming and borderline illegible
- The illegibility IS the point — it mirrors the experience of trying to learn Git

**SFX**: A subtle layered "whoosh" for each term arriving. Each whoosh is mixed at −30dB. As terms overlap, the individual whooshes blend into a continuous wall of soft air movement. The composite sound should feel like a rising tide of noise, not 17 discrete events.

**Narrator Sync**: The narrator rapidly lists terms ("Ref. Refspec. HEAD. Detached HEAD...") during this shot. The visual term entrances should loosely align with the narrator's cadence — each spoken term triggers its corresponding visual term's entrance, with the remaining terms filling in the gaps to maintain the 120ms stagger rhythm.

---

#### SHOT 4A.3 — "Bad Design" Punchline

| Property | Value |
|---|---|
| **Timecode** | 1:38.000–1:42.000 |
| **Duration** | 4.000s |
| **Shot Type** | Text overlay on dimmed background |
| **Purpose** | Land the joke. Crystallize the problem into a single memorable line. |

**Background Treatment**:
- The jargon wall from Shot 4A.2 freezes in place (all drift and motion stops instantly at 1:38.000)
- Over 300ms (`ease-standard`), all jargon terms dim to 15% opacity — they become a ghostly texture behind the punchline text
- The glow and any remaining color flashes also dim to 15%

**Punchline Text**:

| Line | Text | Typography | Color | Position |
|---|---|---|---|---|
| 1 | "That's not power." | `heading-2` | `#F0F0F0` | Centered, 32px above vertical midpoint (4K) |
| 2 | "That's bad design wearing a trenchcoat." | `heading-2` | `#F0F0F0` | Centered, 32px below vertical midpoint (4K) |

**Animation**:
- Both lines enter together as a single unit
- Entrance: `ease-enter`, 400ms, `translate-y` from +16px to 0px, opacity 0→1
- Hold: both lines visible and stationary for 2,000ms minimum (2,600ms actual, accounting for the 400ms entrance and 1:42.000 cut)

**Narrator Sync**: The narrator delivers "That's not power. That's bad design wearing a trenchcoat." timed to the text entrance. The word "trenchcoat" should land while both lines are visible and stationary.

**Transition Out**: Hard cut to Problem 2 at 1:42.000. No fade, no dissolve. The cut is the punctuation on the joke.

---

### PROBLEM 2 — THE INVISIBLE TEAM (1:42–1:55)

**Duration**: 13 seconds
**Emotional Beat**: Blindness, frustration at preventable failure, recognition
**Narrator**: "Number two. In Git, all work is invisible until someone pushes. You have no idea what your teammates are working on. You don't know Alice has been editing the same file as you for three hours until you both push and the merge conflict explodes. So teams hold standups. They post in Slack. They update tickets. All to compensate for the fact that their version control system has zero awareness of what anyone is actually doing."

---

#### SHOT 4B.1 — Number Card Slam

| Property | Value |
|---|---|
| **Timecode** | 1:42.000–1:44.000 |
| **Duration** | 2.000s |
| **Shot Type** | Full-frame centered typography |

Identical structure to [SHOT 4A.1](#shot-4a1--number-card-slam) with the following substitutions:

| Property | Value |
|---|---|
| Number | `2` |
| Subtitle | `THE INVISIBLE TEAM` |

All animation timings, easing curves, SFX, and layout rules are inherited from the [Reusable Component: Number Card](#reusable-component-number-card) specification.

---

#### SHOT 4B.2 — Split Screen Animation

| Property | Value |
|---|---|
| **Timecode** | 1:44.000–1:51.000 |
| **Duration** | 7.000s |
| **Shot Type** | Split-screen with shared UI elements |
| **Purpose** | Dramatize the blindness problem — two developers editing the same file simultaneously, with zero mutual awareness |

**Transition In**: Number card fades out (300ms, `ease-standard`), then the frame splits.

**Layout — Split Panels**:

The frame divides vertically into two equal panels (50/50 split at the horizontal midpoint). A 2px vertical divider line (`#374151`) separates the panels.

| Panel | Position | Developer | Label Color | Terminal Content |
|---|---|---|---|---|
| Left | 0%–49.9% width | Alice | Accent Cyan `#00D4FF` | Editing `src/auth/oauth.rs` |
| Right | 50.1%–100% width | Bob | Confident Green `#00C48F` | Editing `src/auth/oauth.rs` |

**Panel Internal Layout** (each panel, mirrored):

```
┌─────────────────────────────┐
│  ALICE                      │  ← Label: `heading-3`, label color, top-left, 32px margin (4K)
│  ┌───────────────────────┐  │
│  │ src/auth/oauth.rs     │  │  ← File tab: `code`, #6B7280, inside terminal chrome
│  │                       │  │
│  │  fn verify_token(     │  │  ← Terminal content: `code`, #F0F0F0, scrolling
│  │    token: &str,       │  │
│  │    config: &OAuthConf │  │
│  │  ) -> Result<Claims>  │  │
│  │  {                    │  │
│  │    let decoded = deco │  │  ← Active cursor: blinking block, #F0F0F0, 530ms cycle
│  │                       │  │
│  └───────────────────────┘  │
└─────────────────────────────┘
```

- Terminal chrome: `#111827` fill, 1px `#374151` border, 8px border-radius (4K)
- Terminal interior: code scrolls upward at a steady pace (~2 lines per second), simulating active editing
- Cursor: blinking block cursor at the active editing position, standard 530ms blink interval
- Both terminals show Rust code (`.rs` file) — different functions in the same file, emphasizing that they're working in separate regions of the same file

**Shared File Path**:
- Positioned at the top center of the frame, spanning the divider line
- Text: `src/auth/oauth.rs` in `code-lg`, Warning Red `#FF3B3B`
- Background: a pill-shaped container, `#111827` fill, 1px `#FF3B3B` border at 40% opacity, 24px horizontal padding, 12px vertical padding (4K)
- The red color signals danger — they're editing the same file and neither knows it

**Countdown Timer**:
- Positioned vertically centered on the divider line, between the two panels
- Starting value: `3:00:00` (three hours)
- Ending value: `0:00:00`
- Typography: `heading-2`, Warning Red `#FF3B3B`
- Background: circular container, 200px diameter (4K), `#111827` fill, 2px `#FF3B3B` border
- The timer counts down from `3:00:00` to `0:00:00` over 5 seconds of real time
  - Update interval: every 100ms of real time, the displayed time decrements by 1,800 simulated seconds (3 hours / 50 ticks × 100ms = 5,000ms)
  - This means the timer visually ticks through hours in seconds — the acceleration creates urgency
  - The countdown should feel like a bomb timer

**Animation Sequence**:

| Time Offset | Element | Animation | Duration | Easing |
|---|---|---|---|---|
| 0ms | Left panel | Slide in from left (translate-x −50%→0%) | 400ms | `ease-standard` |
| 0ms | Right panel | Slide in from right (translate-x +50%→0%) | 400ms | `ease-standard` |
| 0ms | Divider line | Scale-y 0→1 (draws from center) | 400ms | `ease-standard` |
| 200ms | Labels (Alice, Bob) | Fade in, opacity 0→1 | 300ms | `ease-enter` |
| 300ms | Terminals | Fade in, opacity 0→1 | 300ms | `ease-enter` |
| 400ms | Shared file path | Fade in, opacity 0→1 | 300ms | `ease-enter` |
| 500ms | Timer | Fade in, opacity 0→1 | 300ms | `ease-enter` |
| 600ms | Timer | Countdown begins (3:00:00→0:00:00) | 5,000ms | Linear (clock tick) |
| 600ms | Code scrolling | Both terminals begin scrolling | 5,000ms | Linear |

**Timer Color Escalation**:

| Timer Range | Text Color | Border Color | Additional Effect |
|---|---|---|---|
| 3:00:00–1:00:00 | `#FF3B3B` at 70% | `#FF3B3B` at 30% | None |
| 1:00:00–0:10:00 | `#FF3B3B` at 85% | `#FF3B3B` at 50% | Gentle pulse (opacity oscillation ±10%, 800ms cycle) |
| 0:10:00–0:00:00 | `#FF3B3B` at 100% | `#FF3B3B` at 80% | Rapid pulse (opacity oscillation ±15%, 400ms cycle) |

**SFX**: A ticking clock sound begins at 1:44.600 (when the timer starts counting). The tick is a clean, dry metronome click at −28dB. The tick interval starts at 200ms and accelerates to 80ms as the timer approaches zero, creating a sense of mounting inevitability.

---

#### SHOT 4B.3 — CONFLICT Explosion

| Property | Value |
|---|---|
| **Timecode** | 1:51.000–1:55.000 |
| **Duration** | 4.000s |
| **Shot Type** | Full-frame impact typography with destruction animation |
| **Purpose** | The payoff — the collision the viewer has been dreading since the timer started |

**The Trigger** (at exactly 1:51.000):
- The timer hits `0:00:00`
- All ticking SFX stops — a 50ms beat of pure silence

**The Shatter** (1:51.050–1:51.350):
- The 2px vertical divider line between the two panels SHATTERS
- Shatter animation: the line fragments into 12–16 irregular shards
- Shards fly outward from the center in random directions (velocity: 800–1200px/s at 4K)
- Each shard rotates (random rotation speed: ±180°–720° per second) as it flies
- Shards fade to 0% opacity over 600ms as they fly outward
- Shard color: `#FF3B3B` at 60%

**Panel Merge** (1:51.050–1:51.400):
- Both panels slide toward center, overlapping chaotically
  - Left panel: translate-x 0→+15% over 350ms, `ease-sharp`
  - Right panel: translate-x 0→−15% over 350ms, `ease-sharp`
- The overlapping terminals create visual interference — code from both panels is visible, layered, unreadable
- Both panels simultaneously shift to 60% opacity to enhance the overlay confusion

**CONFLICT Text Impact** (1:51.300):
- The word `CONFLICT` slams onto the center of the screen
- Typography: `display-lg` (128px at 4K / 64px at HD), Inter Tight 800
- Color: Warning Red `#FF3B3B`
- Animation: `ease-bounce`, 300ms
  - Scale: 1.5→1.0
  - Opacity: 0→1
  - The overshoot from `ease-bounce` (scale briefly dips below 1.0 then settles) gives the text a satisfying physical weight, like it SLAMMED down

**Red Flash** (1:51.300–1:51.400):
- Full-screen overlay: Warning Red `#FF3B3B` at 20% opacity
- Duration: 100ms appearance, then fades to 0% over 200ms (`ease-standard`)
- This is a single alarming pulse — NOT a strobe, NOT repeated. One flash.
- Simultaneously, a sub-bass hit plays: a 40Hz sine wave, 150ms duration, fast attack, medium decay, mixed at −22dB. The viewer should feel this more than hear it.

**Hold** (1:51.600–1:53.500):
- `CONFLICT` text holds center-frame, fully opaque, for 1,900ms
- The merged, chaotic panel content remains visible at 30% opacity behind the text (dimmed further from the 60% during merge)
- The shared file path pill at the top pulses once: border opacity 40%→100%→40% over 800ms

**SFX**: At 1:51.050 — crash/impact sound. Deep, resonant, short (~400ms). Think cinematic trailer hit: layered sub-bass (40–60Hz), mid-range impact (200–400Hz), and a short metallic transient (2–4kHz). Mixed at −16dB. This is the loudest SFX in all of Act II.

**Transition Out**: Hard cut to Problem 3 at 1:55.000. The CONFLICT text and all panel content vanish instantaneously. Clean break.

---

### PROBLEM 3 — DESTRUCTION IS ONE COMMAND AWAY (1:55–2:10)

**Duration**: 15 seconds
**Emotional Beat**: Loss, violation, dark humor
**Narrator**: "Number three. Git lets you destroy things. Easily. git reset --hard. git push --force. git rebase and drop. These commands delete history. And Git doesn't stop you. It doesn't warn you. Your colleague's weekend of work — gone. One command. And sure, the reflog exists — if you know about the reflog, and you find it in time, and you haven't garbage collected yet. That's not a safety net. That's a rumor of a safety net."

---

#### SHOT 4C.1 — Number Card Slam

| Property | Value |
|---|---|
| **Timecode** | 1:55.000–1:57.000 |
| **Duration** | 2.000s |

Identical structure to [Reusable Component: Number Card](#reusable-component-number-card) with:

| Property | Value |
|---|---|
| Number | `3` |
| Subtitle | `DESTRUCTION IS ONE COMMAND AWAY` |

Note: the subtitle is longer than other problems. If it exceeds comfortable single-line width at `heading-1`, it may break to two lines: "DESTRUCTION IS ONE" / "COMMAND AWAY", centered, with 16px line gap (4K). Test at both 4K and HD to confirm readability within the 800ms hold time.

---

#### SHOT 4C.2 — The Dissolution

| Property | Value |
|---|---|
| **Timecode** | 1:57.000–2:06.000 |
| **Duration** | 9.000s |
| **Shot Type** | Diagram animation with progressive destruction |
| **Purpose** | Show healthy version history being irreversibly destroyed, node by node, by careless Git commands |

**Transition In**: Number card fades out (300ms), then the DAG builds in.

**Phase 1 — Healthy DAG (1:57.300–1:59.300, 2 seconds)**:

A directed acyclic graph (DAG) of commit nodes appears, representing a healthy repository history.

**DAG Layout**:

```
       ○───○───○───○  feature/billing
      /             \
 ○───○───○───○───○───●  main
      \         /
       ○───○───○  feature/auth
```

- **Nodes**: circles, 20px diameter (4K), fill Confident Green `#00C48F`
- **Connecting lines**: 2px stroke, Dim Gray `#374151`
- **Branch labels**: positioned at the rightmost node of each branch
  - `main` — `code` typography, `#F0F0F0`
  - `feature/auth` — `code` typography, `#F0F0F0`
  - `feature/billing` — `code` typography, `#F0F0F0`
- **Merge node** (●): same style as other nodes but 24px diameter, indicating merge commits
- The DAG occupies the left 60% of the frame, centered vertically

**DAG "Alive" Animations**:
- Nodes pulse gently: scale 1.0→1.05→1.0, 2,000ms cycle, staggered across nodes so the pulse "ripples" through the graph
- Connecting lines have a subtle glow: `#00C48F` at 10% opacity overlaid on the `#374151` stroke, pulsing in sync with the nodes
- This vitality communicates: this is HEALTHY history. This is valuable.

**DAG Entrance Animation**:
- The entire DAG fades in: opacity 0→1, 600ms, `ease-enter`
- Simultaneously, nodes scale from 0.5→1.0, 600ms, `ease-bounce`, staggered 30ms per node from left to right
- Total entrance: ~800ms (600ms base + stagger)

**Phase 2 — The Three Commands (1:59.300–2:04.700)**:

Three destructive Git commands appear sequentially on the right side of the frame (right 35%, centered vertically). Each command triggers a cluster of DAG nodes to dissolve.

**Command 1: `git reset --hard`**

| Property | Value |
|---|---|
| Appears at | 1:59.300 |
| Typography | `code-lg` (48px at 4K) |
| Color | Warning Red `#FF3B3B` |
| Entrance | `ease-sharp`, 300ms, translate-x from +32px to 0px, opacity 0→1 |
| Target nodes | The 3 nodes on `feature/auth` branch |

**Dissolution Animation** (triggered 200ms after command appears):
1. Target nodes transition color: `#00C48F` → `#FF3B3B` over 200ms, `ease-standard`
2. The red nodes hold for 100ms (the audience registers the color change)
3. Each red node fragments into 20–30 particles (small circles, 2–4px, `#FF3B3B` at varying opacities 30–80%)
4. Particles drift downward with simulated gravity (acceleration: 200px/s², initial velocity: random 20–80px/s in random directions)
5. Particles fade to 0% opacity over 800ms
6. The connecting lines to dissolved nodes snap: the line endpoints retract toward the surviving node at the branch point, traveling 40px then stopping with a small elastic bounce (±4px, 200ms). The retracted line stubs fade to 20% opacity.
7. The `feature/auth` branch label fades to 0% over 300ms

**Command 2: `git push --force`**

| Property | Value |
|---|---|
| Appears at | 2:00.700 (400ms after Command 1) |
| Typography | `code-lg` |
| Color | Warning Red `#FF3B3B` |
| Entrance | Same as Command 1 |
| Position | Below Command 1, 48px gap (4K) |
| Target nodes | The 4 nodes on `feature/billing` branch |

- Dissolution animation identical to Command 1, targeting the `feature/billing` nodes
- Triggered 200ms after command appears

**Command 3: `git rebase --drop`**

| Property | Value |
|---|---|
| Appears at | 2:02.100 (400ms after Command 2) |
| Typography | `code-lg` |
| Color | Warning Red `#FF3B3B` |
| Entrance | Same as Command 1 |
| Position | Below Command 2, 48px gap (4K) |
| Target nodes | 3 of the 5 remaining nodes on `main` (the middle ones) |

- Dissolution animation identical to Commands 1 and 2
- After this dissolution, only 2 nodes on `main` remain (the first and the merge node at the end), plus 2 stubs of connecting lines that lead to nothing
- The surviving nodes dim from `#00C48F` to Muted Gray `#6B7280` over 400ms — even the survivors look damaged, incomplete

**Phase 3 — Damaged DAG Hold (2:04.700–2:06.000)**:

- The damaged DAG holds for 1.3 seconds
- Only 2–3 gray nodes remain, connected by faded line stubs
- The three red commands remain visible on the right side, fully opaque — a forensic record of what destroyed the history
- Large gaps of empty Deep Navy space where healthy nodes used to be — the absence is the visual story
- All pulsing/glow animations have stopped. The DAG is dead.
- SFX: silence. The absence of the earlier pulse-glow ambient is itself an audio cue.

**SFX for Phase 2**: Each dissolution event has a quiet "crumble" sound — a granular texture (imagine sand or ash falling). Mixed at −26dB. Duration: 600ms per crumble. There should be a beat of silence between each crumble (the 400ms gap between commands provides this naturally). The contrast between sound and silence makes each destruction discrete and countable.

---

#### SHOT 4C.3 — "Rumor of a Safety Net"

| Property | Value |
|---|---|
| **Timecode** | 2:06.000–2:10.000 |
| **Duration** | 4.000s |
| **Shot Type** | Layered text over dimmed background |
| **Purpose** | The darkest joke in the video. Acknowledge the reflog as the inadequate failsafe it is. |

**Background Treatment**:
- The damaged DAG (surviving nodes + commands) dims to 10% opacity over 400ms, `ease-standard`
- The dimmed DAG remains as faint texture — the ghost of destroyed history

**Text — Line 1**:

| Property | Value |
|---|---|
| Text | "That's not a safety net." |
| Typography | `heading-2` (64px at 4K / 32px at HD) |
| Color | `#F0F0F0` |
| Position | Centered horizontally, 40px above vertical midpoint (4K) |
| Entrance | `ease-enter`, 400ms, translate-y +16px→0, opacity 0→1 |
| Appears at | 2:06.400 (after BG dim completes) |

**Text — Line 2**:

| Property | Value |
|---|---|
| Text | "That's a rumor of a safety net." |
| Typography | `heading-2` (64px at 4K / 32px at HD) |
| Color | Warning Red `#FF3B3B` at 80% opacity |
| Position | Centered horizontally, 40px below vertical midpoint (4K) |
| Entrance | `ease-enter`, 400ms, translate-y +16px→0, opacity 0→1 |
| Appears at | 2:06.800 (400ms after Line 1 begins entering) |

**Hold**: Both lines visible and stationary from ~2:07.200 to 2:10.000 (2.8 seconds). Exceeds the 2-second minimum readability rule.

**Transition Out**: Hard cut at 2:10.000 to Problem 4.

---

### PROBLEM 4 — THE SECURITY VACUUM (2:10–2:24)

**Duration**: 14 seconds
**Emotional Beat**: Vulnerability, disbelief, indictment of complacency
**Narrator**: "Number four. Git has no built-in access control. None. No authentication. No authorization. No concept of 'this person can read this folder but not that one.' Every solution — GitHub permissions, GitLab roles, Bitbucket restrictions — is bolted on by a third party. The protocol itself? The native git-colon-slash-slash protocol has no encryption and no auth. In 2025. We just... accepted that."

---

#### SHOT 4D.1 — Number Card Slam

| Property | Value |
|---|---|
| **Timecode** | 2:10.000–2:12.000 |
| **Duration** | 2.000s |

Per [Reusable Component: Number Card](#reusable-component-number-card):

| Property | Value |
|---|---|
| Number | `4` |
| Subtitle | `THE SECURITY VACUUM` |

---

#### SHOT 4D.2 — The Open Vault

| Property | Value |
|---|---|
| **Timecode** | 2:12.000–2:20.000 |
| **Duration** | 8.000s |
| **Shot Type** | Illustrated animation with particle stream |
| **Purpose** | Visualize the total absence of access control — everything is accessible, nothing is protected |

**Transition In**: Number card fades out (300ms), vault illustration begins building.

**Vault Door Illustration**:
- Style: line art, 2px stroke, Dim Gray `#374151`
- A large vault door drawn in a front-facing perspective, positioned center-left of frame
- The door is rendered with mechanical detail: a circular locking wheel, hinge bolts, a heavy frame
- The door is OPEN — swung wide to the left (~70° open angle). The opening reveals the vault's interior.
- The vault frame is approximately 800px wide × 1000px tall (4K). The door width is approximately 600px.
- Fill: none (line art only). Interior of vault: Deep Navy `#0A0F1A` (same as background — the vault's interior is the void)

**Vault Door Entrance Animation**:

| Time Offset | Element | Animation | Duration | Easing |
|---|---|---|---|---|
| 0ms | Vault frame | Line-draw reveal (stroke-dashoffset animation, clockwise from top-left) | 800ms | `ease-standard` |
| 400ms | Door (open position) | Line-draw reveal | 600ms | `ease-standard` |
| 600ms | Locking wheel | Line-draw reveal (radial, from center outward) | 400ms | `ease-standard` |

**Files Inside the Vault**:
- Arranged in a 3×4 grid inside the vault opening
- Each file is a document icon: a rectangle (60px×80px at 4K) with a folded corner, 2px stroke, `#F0F0F0`
- Each file has a small label beneath it:

| Row | File 1 | File 2 | File 3 |
|---|---|---|---|
| 1 | `.env` | `secrets.toml` | `api-keys.json` |
| 2 | `production.config` | `main.rs` | `schema.sql` |
| 3 | `deploy.yml` | `auth.rs` | `billing.rs` |
| 4 | `README.md` | `.ssh/id_rsa` | `terraform.tfstate` |

- Label typography: `code` (32px at 4K / 16px at HD), `#F0F0F0` at 60% opacity
- Files appear in a staggered grid reveal: each file fades in (opacity 0→1, 200ms, `ease-enter`), staggered 80ms per file, top-left to bottom-right

**File Stream Animation** (begins at approximately 2:13.500):
- Files begin detaching from the grid and streaming OUT of the vault toward the edges of the frame
- Exit directions: files radiate outward in a spreading fan pattern, with most files heading right (toward the open door side) but some scattering in all directions
- Each file's exit animation:
  - Duration: 1,500–2,500ms (randomized per file)
  - Path: a slight curve (quadratic bezier) from vault to frame edge
  - Scale: 1.0→0.7 (files shrink slightly as they "fly away")
  - Rotation: ±10°–30° (randomized, slight tumble)
  - Opacity: holds at 100% for 70% of the path, then fades to 0% over the remaining 30%
- Stream rate: starts at 2 files per second, accelerates to 8 files per second by the end of the shot
- As the original 12 files exit, new file icons are generated from the vault interior (an infinite supply — the vault contains everything)
- Total files visible at peak: 20–30 simultaneously on screen

**Sensitive File Highlighting**:
- Files with sensitive labels (`.env`, `secrets.toml`, `api-keys.json`, `production.config`, `.ssh/id_rsa`, `terraform.tfstate`) flash Warning Red `#FF3B3B` as they exit
  - Flash: stroke color transitions `#F0F0F0` → `#FF3B3B` over 200ms as they pass the vault door threshold
  - The red files are visually distinct from the white ones — they POP in the stream
- **Hero File** — one `.env` file, larger than the others (100px×130px at 4K), passes through the center of the frame at approximately 2:16.000
  - This file has visible content inside it (not just an icon):
    - Line 1: `API_KEY=sk_live_...` in `code` (24px at 4K), `#FF3B3B`
    - Line 2: `DB_PASSWORD=pr0d_...` in `code` (24px at 4K), `#FF3B3B`
  - The content is readable for approximately 1.5 seconds as the file drifts across the central third of the frame
  - This is the visual representation of leaked secrets — unmistakable, alarming

**SFX**: A whooshing air current begins softly (−30dB) at stream start and builds in intensity to −22dB by the end of the shot. The sound is a sustained, breathy whoosh (not sharp — more like wind through an open door). As the file stream accelerates, the whoosh pitch rises slightly (subtle Doppler-like effect). Individual file exits do NOT have discrete SFX — the composite whoosh covers all of them.

---

#### SHOT 4D.3 — Platform Badges

| Property | Value |
|---|---|
| **Timecode** | 2:20.000–2:24.000 |
| **Duration** | 4.000s |
| **Shot Type** | Card layout with critical annotation |
| **Purpose** | Name the workarounds, then expose the protocol's fundamental nakedness |

**Transition**: The vault and file stream fade to 0% over 400ms (`ease-standard`). The badge layout builds in.

**"Bolted on." Header**:
- Text: `Bolted on.` in `heading-2`, Warning Red `#FF3B3B` at 80% opacity
- Position: centered horizontally, top third of frame (approximately 30% from top)
- Entrance: `ease-enter`, 400ms, translate-y +16px→0, opacity 0→1
- Appears at 2:20.400

**Three Platform Badges** (appear at 2:20.800, staggered 150ms apart):

| # | Label Text | Position |
|---|---|---|
| 1 | "GitHub Permissions" | Left third, centered vertically at 50% |
| 2 | "GitLab Roles" | Center third, centered vertically at 50% |
| 3 | "Bitbucket Restrictions" | Right third, centered vertically at 50% |

**Badge Style** (per badge):
- Background: Code Background `#111827`
- Border: 1px `#374151` — but intentionally IMPERFECT:
  - The border uses a jagged/hand-drawn stroke effect: each edge segment has a ±1–2px random offset from the true rectangle, creating a "duct tape" visual treatment
  - This jaggedness should look rough, improvised, afterthought — NOT clean, NOT intentional
  - Implementation: SVG path with randomized control points along each edge, or a pre-rendered texture overlay
- Border-radius: 8px (4K)
- Padding: 32px horizontal, 24px vertical (4K)
- Label: `body-lg` (40px at 4K / 20px at HD), `#F0F0F0`
- Entrance: `ease-enter`, 300ms, translate-y +12px→0, opacity 0→1

**Badge Entrance Stagger**:

| Time | Badge |
|---|---|
| 2:20.800 | GitHub Permissions |
| 2:20.950 | GitLab Roles |
| 2:21.100 | Bitbucket Restrictions |

**Protocol Indictment** (below the badges):
- Line 1: `git://` in `code-lg` (48px at 4K), Warning Red `#FF3B3B`
- Line 2: `No encryption. No auth.` in `body-lg` (40px at 4K), Muted Gray `#6B7280`
- Position: centered horizontally, below the badges with 48px gap (4K)
- Entrance: `ease-enter`, 400ms, translate-y +16px→0, opacity 0→1
- Appears at 2:21.400

**Hold**: Full layout (header + 3 badges + protocol text) visible from ~2:21.800 to 2:24.000 (2.2 seconds). Meets the 2-second readability minimum.

**Transition Out**: Hard cut at 2:24.000.

---

### PROBLEM 5 — THE MONOREPO MELTDOWN (2:24–2:38)

**Duration**: 14 seconds
**Emotional Beat**: Exhaustion, dark comedy, precariousness
**Narrator**: "And number five. Git was not built for how modern teams actually work. Large binary files? You need a separate system called LFS. Multiple projects in one repo? Good luck with sparse checkout and submodule hell. Partial history? Shallow clones break half your tooling. Git scales down to one person beautifully. Scaling it up to a real organization? That's where you start building workarounds on top of workarounds on top of workarounds."

---

#### SHOT 4E.1 — Number Card Slam

| Property | Value |
|---|---|
| **Timecode** | 2:24.000–2:26.000 |
| **Duration** | 2.000s |

Per [Reusable Component: Number Card](#reusable-component-number-card):

| Property | Value |
|---|---|
| Number | `5` |
| Subtitle | `THE MONOREPO MELTDOWN` |

---

#### SHOT 4E.2 — The Workaround Tower

| Property | Value |
|---|---|
| **Timecode** | 2:26.000–2:36.000 |
| **Duration** | 10.000s |
| **Shot Type** | Animated Jenga-style block tower |
| **Purpose** | Visualize the absurd tower of third-party workarounds stacked on top of Git's inadequate foundation |

**Transition In**: Number card fades out (300ms), then the tower begins building from the bottom.

**Tower Block Specifications**:

Each block is a rounded rectangle with the following base style:
- Fill: Code Background `#111827`
- Border: 1px, Dim Gray `#374151`
- Border-radius: 12px (4K)
- Height: 80px (4K) / 40px (HD)
- Standard width: 400px (4K) / 200px (HD) — exceptions noted per block

**Block Stack** (bottom to top):

| Layer | Label | Typography | Text Color | Width (4K) | Offset from Center | Notes |
|---|---|---|---|---|---|---|
| 1 (foundation) | `Git` | `heading-3` | Muted Gray `#6B7280` | 500px | 0px | Wider than others — it's the base |
| 2 | `LFS` | `body-lg` | `#F0F0F0` | 400px | +2px right | Slight offset begins |
| 3 | `Submodules` | `body-lg` | `#F0F0F0` | 400px | −3px left | |
| 4 | `Sparse Checkout` | `body-lg` | `#F0F0F0` | 400px | +4px right | |
| 5 | `.gitattributes` | `body-lg` | `#F0F0F0` | 400px | −2px left | |
| 6 | `Git Hooks` | `body-lg` | `#F0F0F0` | 400px | +3px right | |
| 7 | `Husky` | `body-lg` | `#F0F0F0` | 400px | −4px left | |
| 8 | `GitHub Actions` | `body-lg` | `#F0F0F0` | 400px | +4px right | Wobble begins here |
| 9 | `Custom Scripts` | `body-lg` | `#F0F0F0` | 400px | −3px left | |
| 10 (top) | `Please Don't Touch This` | `body` | Warning Red `#FF3B3B` | 400px | +4px right | The most crooked block |

**Tower Position**: Centered horizontally in the frame. The bottom block's baseline sits at approximately 80% of frame height (4K), building upward.

**Block Stacking Animation**:

Each block enters from the right side of the frame, slides horizontally to its position, then settles into place:

| Per-Block Animation | Property | From | To | Duration | Easing |
|---|---|---|---|---|---|
| Slide in | translate-x | +600px (4K) | final offset position | 300ms | `ease-standard` |
| Slide in | opacity | 0.0 | 1.0 | 200ms | `ease-enter` |
| Settle | translate-y | −4px (slight overshoot) | 0px | 150ms | `ease-bounce` |

**Stagger timing**: 200ms apart. Block 1 enters at t+0ms, Block 2 at t+200ms, etc.
- Total build duration: 200ms × 9 intervals + 300ms final animation = 2,100ms
- Build begins at 2:26.300 (after number card fade), completes at approximately 2:28.400

**Cumulative Lean**:
- Each block's offset from center is specified in the table above
- The cumulative effect: the tower leans perceptibly to one side, then the other, with the center of mass shifting with each block
- By the top block, the tower is visibly unstable — the viewer should feel that it could fall at any moment

**Wobble Animations** (begin after full tower is built):

| Layer | Wobble Amplitude | Cycle Duration | Phase Offset |
|---|---|---|---|
| 1–7 (lower blocks) | ±0px (no independent wobble) | — | — |
| 8 (GitHub Actions) | ±2px horizontal | 1,500ms | 0° |
| 9 (Custom Scripts) | ±2px horizontal | 1,500ms | 120° |
| 10 (Please Don't Touch This) | ±2px horizontal | 1,500ms | 240° |

**Global Tower Sway**:
- The ENTIRE tower (all 10 blocks as a group) has a constant, very slight sway
- Amplitude: ±1px horizontal
- Cycle: 3,000ms, sinusoidal
- This is subtle — the viewer should feel the instability subconsciously
- The sway begins at the same time the wobble begins (tower build complete)

**Narrator Sync**: As the narrator says "workarounds on top of workarounds on top of workarounds," the tower should be fully built and wobbling. The repetition in the script matches the repetition in the tower — layer after layer of the same pattern.

---

#### SHOT 4E.3 — The Wobble

| Property | Value |
|---|---|
| **Timecode** | 2:36.000–2:38.000 |
| **Duration** | 2.000s |
| **Shot Type** | Animation payoff — the top block falls |
| **Purpose** | Physical metaphor: Git works, but barely. One wrong move and something falls. |

**Wobble Intensification** (2:36.000–2:36.800):
- The top 3 blocks' wobble amplitude increases:

| Layer | New Amplitude | New Cycle |
|---|---|---|
| 8 | ±4px | 800ms |
| 9 | ±4px | 800ms |
| 10 | ±6px | 600ms |

- Global tower sway increases: ±2px, 2,000ms cycle
- The intensification is abrupt — the transition from gentle wobble to alarming wobble happens over 200ms

**Top Block Falls** (2:36.800–2:37.400):
- Block 10 ("Please Don't Touch This") slides off the tower to the right
- Animation:
  - translate-x: current position → +300px (4K) over 600ms
  - translate-y: current position → +800px (4K) over 600ms (gravity: simulated with `ease-in` — starts slow, accelerates)
  - rotation: 0° → +15° over 600ms, `ease-standard`
  - opacity: 1.0 for the first 400ms, then fades to 0% over the final 200ms
- The block disappears off the bottom-right of the frame

**Tower Survives** (2:37.400–2:38.000):
- The remaining 9 blocks stabilize slightly — wobble reduces back to the original gentle values
- The tower holds. It's damaged (the top layer is gone, the wobble is still present) but it hasn't fully collapsed
- **This is the metaphor**: Git works. But it's precarious. It's one incident away from losing a piece. And everyone just... keeps building on it anyway.

**SFX**: At 2:36.800, a wooden "clack" — the sound of a Jenga block hitting a table surface. Clean, dry, medium-pitch wood impact. Duration: ~200ms. Mixed at −22dB. No reverb (the dryness makes it feel real, immediate, close). A brief silence follows — let the absence of the block register.

**Transition Out**: Hard cut to the Bridge at 2:38.000.

---

### BRIDGE — THE QUESTION (2:38–2:45)

**Duration**: 7 seconds
**Emotional Beat**: Relief, clarity, purpose, the first glimpse of hope
**Narrator**: "So we asked a simple question. What if we stopped building workarounds — and built the thing that should have existed all along?"

This is the emotional pivot of the entire video. It must feel like a weight being lifted.

---

#### SHOT 4F.1 — Clean Slate

| Property | Value |
|---|---|
| **Timecode** | 2:38.000–2:40.000 |
| **Duration** | 2.000s |
| **Shot Type** | Empty frame — pure negative space |
| **Purpose** | After the visual chaos of five problems, the emptiness is a relief. Let the audience exhale. |

**Visual**: Pure Deep Navy `#0A0F1A`. Nothing else. No text. No shapes. No glow. No particles. No ambient animation. Just the void.

**Audio**: The tension music DROPS at exactly 2:38.000 (see [Audio Design](#audio-design) for full spec). The sudden absence of the beat, arpeggio, and bass creates a vacuum that the narrator fills with calm authority.

**Narrator**: "So we asked a simple question." — delivered with calm confidence. The vocal performance shifts here: the frustration from the five problems is gone. In its place, quiet certainty. The narrator has stopped enumerating problems and started presenting a solution path.

**The Emptiness Is Intentional**: Do NOT add visual elements to this shot. The 2 seconds of pure Deep Navy is a compositional reset. It gives the viewer's visual cortex a break after 68 seconds of dense, chaotic, red-heavy imagery. The rest is the message.

---

#### SHOT 4F.2 — The Question

| Property | Value |
|---|---|
| **Timecode** | 2:40.000–2:45.000 |
| **Duration** | 5.000s |
| **Shot Type** | Two-line centered typography |
| **Purpose** | Pose the founding question of W0rkTree. Introduce brand cyan for the first time in Act II. |

**Text — Line 1**:

| Property | Value |
|---|---|
| Text | "What if we stopped building workarounds" |
| Typography | `heading-1` (96px at 4K / 48px at HD), Inter 700 |
| Color | `#F0F0F0` |
| Position | Centered horizontally, 48px above vertical midpoint (4K) |
| Entrance | `ease-enter`, 600ms, translate-y +24px→0px, opacity 0→1 |
| Appears at | 2:40.000 |

**Text — Line 2**:

| Property | Value |
|---|---|
| Text | "— and built the thing that should have existed all along?" |
| Typography | `heading-1` (96px at 4K / 48px at HD), Inter 700 |
| Color | **Accent Cyan `#00D4FF`** |
| Position | Centered horizontally, 48px below vertical midpoint (4K) |
| Entrance | `ease-enter`, 600ms, translate-y +24px→0px, opacity 0→1 |
| Appears at | 2:40.300 (300ms after Line 1 begins) |

**The Color Shift — Critical Note**:
- Line 2 is rendered in Accent Cyan `#00D4FF`. This is the **FIRST** appearance of the brand cyan color anywhere in Act II.
- Throughout the entire 68 seconds of problems (1:30–2:38), cyan has been deliberately absent. The palette has been exclusively Warning Red, White, Muted Gray, Dim Gray, and Deep Navy.
- The sudden introduction of cyan signals to the viewer — consciously or subconsciously — that something has changed. We have crossed from problem space into solution space.
- The cyan must be VIVID and CLEAN. No transparency reduction. No blur. Full `#00D4FF` at 100% opacity. It should feel like a light turning on.

**Hold**: Both lines visible and stationary from approximately 2:40.900 (when Line 2's entrance completes) to 2:45.000 — a hold of 4.1 seconds. This significantly exceeds the 2-second readability minimum and is intentional: the question must breathe. It must feel considered, not rushed.

**Audio at 2:42.000–2:45.000**:
- The sustained synth pad (which replaced the dropped beat at 2:38) resolves from a dissonant chord to a consonant one at approximately 2:42.000
- At approximately 2:43.500, the first notes of Act III's theme begin: a clean, open synth chord in a major key. Hopeful. Forward-looking. The audience hears the answer to the question before they see it.
- The musical transition is the audio equivalent of the cyan color shift — it signals "the frustration is over, something better is coming"

**Transition to Act III**: Hard cut at 2:45.000. The two lines of text vanish instantly. The next frame is the opening of Act III (Scene 5 — `06_SCENE_ACT_III_PRODUCT.md`). The palette shift is INSTANT on the cut — Act II's red/gray/navy color world is replaced by Act III's cyan/navy/white color world in a single frame. No cross-fade. No dissolve. The hard cut IS the resolution.

---

## Dialogue — Complete Narrator Script

The following is the authoritative narrator script for Scene 4. All timecodes are approximate sync points — the editor should prioritize natural vocal rhythm over frame-perfect alignment. If the narrator's delivery runs slightly ahead or behind, adjust visual timing to match the voice, not the reverse.

> **[1:30–1:42 — PROBLEM 1]**
>
> Number one. Git has a jargon problem. Ref. Refspec. HEAD. Detached HEAD. Origin. Upstream. Index. Staging area. Working tree. Stash. The thing is — half of these are synonyms for each other, and the other half mean completely different things depending on context. Git checkout — one command — does five completely different things depending on what flags you pass it. That's not power. That's bad design wearing a trenchcoat.

> **[1:42–1:55 — PROBLEM 2]**
>
> Number two. In Git, all work is invisible until someone pushes. You have no idea what your teammates are working on. You don't know Alice has been editing the same file as you for three hours until you both push and the merge conflict explodes. So teams hold standups. They post in Slack. They update tickets. All to compensate for the fact that their version control system has zero awareness of what anyone is actually doing.

> **[1:55–2:10 — PROBLEM 3]**
>
> Number three. Git lets you destroy things. Easily. git reset --hard. git push --force. git rebase and drop. These commands delete history. And Git doesn't stop you. It doesn't warn you. Your colleague's weekend of work — gone. One command. And sure, the reflog exists — if you know about the reflog, and you find it in time, and you haven't garbage collected yet. That's not a safety net. That's a rumor of a safety net.

> **[2:10–2:24 — PROBLEM 4]**
>
> Number four. Git has no built-in access control. None. No authentication. No authorization. No concept of "this person can read this folder but not that one." Every solution — GitHub permissions, GitLab roles, Bitbucket restrictions — is bolted on by a third party. The protocol itself? The native git-colon-slash-slash protocol has no encryption and no auth. In 2025. We just... accepted that.

> **[2:24–2:38 — PROBLEM 5]**
>
> And number five. Git was not built for how modern teams actually work. Large binary files? You need a separate system called LFS. Multiple projects in one repo? Good luck with sparse checkout and submodule hell. Partial history? Shallow clones break half your tooling. Git scales down to one person beautifully. Scaling it up to a real organization? That's where you start building workarounds on top of workarounds on top of workarounds.

> **[2:38–2:45 — BRIDGE]**
>
> So we asked a simple question. What if we stopped building workarounds — and built the thing that should have existed all along?

---

## Audio Design

### Music — Continuous Underscore

The music for Act II is a continuous, building electronic underscore that provides rhythmic tension without competing with the narrator. It begins minimal and builds across the five problems, then drops completely for the bridge. The music exists to create a sense of forward momentum and mounting frustration — the viewer should feel that the problems are accumulating, that the weight is growing.

| Timecode | Musical Content | BPM | Level | Notes |
|---|---|---|---|---|
| 1:30.000–1:42.000 | Minimal electronic beat. Sparse: kick drum on the 1, hi-hat on the off-beat. Under it, a low synth pad sustains, creating mild dissonance (minor 2nd interval, e.g., C2 + Db2). | 80 | −24dB (bed) | The beat establishes pulse without energy. The dissonance creates unease. Nothing resolves. |
| 1:42.000–2:10.000 | The beat adds a subtle bass line: a simple octave pulse (root note on 1 and 3, octave on 2 and 4). Still minimal. The synth pad continues. | 80 | −24dB (bed) | Energy comes from narrator and visuals, not music. The bass adds body without drama. |
| 2:10.000–2:24.000 | A filtered synth arpeggio begins: a 16th-note pattern on a single minor chord, heavily low-pass filtered (cutoff ~800Hz). Very quiet. Adds forward momentum. | 80 | Arpeggio: −28dB; Beat: −24dB | The arpeggio is felt more than heard. It creates a subliminal sense of acceleration. The filter opens slightly (~100Hz per problem) as we progress. |
| 2:24.000–2:38.000 | Beat, bass, and arpeggio build to their peak. The arpeggio filter opens further (cutoff ~1200Hz). A subtle ride cymbal joins the beat. The synth pad adds a second dissonant voice (tritone). | 80 | Arpeggio: −24dB; Beat: −22dB | This is the musical peak of Act II. The tower wobble (2:36–2:38) is scored with this maximum tension. |
| 2:38.000–2:42.000 | **EVERYTHING DROPS.** Beat stops. Arpeggio stops. Bass stops. Ride stops. Only the synth pad remains — and it resolves: the dissonant intervals (minor 2nd, tritone) move to a consonant chord (open 5th: C2 + G2). | — | Pad: −28dB | This is the emotional reset. The drop must be INSTANT at 2:38.000 — not a fade, not a filter sweep. One frame: full beat. Next frame: silence + pad. The contrast is the message. |
| 2:42.000–2:45.000 | The first notes of Act III's theme emerge from the sustained pad: a clean, open synth chord in a major key (e.g., C major: C3 + E3 + G3, voiced wide). Hopeful. Forward-looking. | — | Theme: −26dB (entering softly) | The Act III theme must feel like dawn. It begins quietly here and will build through the next scene. The transition is musical: the resolved pad chord is the harmonic bridge into the new key. |

### Music Production Notes

- **Key**: The underscore should stay in a minor key throughout Act II (suggested: C minor or A minor). The bridge at 2:42 modulates to the relative major (Eb major or C major) for Act III.
- **Instrumentation**: Purely electronic. No acoustic instruments. No guitar. No piano. The sound palette is: analog-style synth pads, digital kick and hi-hat, subtractive synth bass, digital arpeggiator. Everything should sound precise and cold — matching the clinical dissection of Git's problems.
- **Sidechain compression**: The kick drum should trigger a subtle sidechain on the pad and bass (3–4dB of gain reduction, fast release). This creates the pumping effect that gives electronic music its forward drive without increasing volume.
- **No melody**: Act II has NO melodic content. Melody implies resolution and narrative arc — neither of which belong in this section. The first melody the viewer hears should be the Act III theme at 2:43.

### SFX — Spotting Sheet

| Timecode | Sound Description | Duration | Level | Trigger |
|---|---|---|---|---|
| 1:30.000 | Percussive hit — layered low tom + kick drum, sharp attack (<10ms), short decay (200ms) | 200ms | −18dB | Number card "1" slam |
| 1:32.000–1:38.000 | Jargon term whooshes — soft air movement per term, 17 instances, blending into continuous texture | ~200ms each | −30dB per term | Each jargon term entrance |
| 1:42.000 | Percussive hit (same sample as 1:30) | 200ms | −18dB | Number card "2" slam |
| 1:44.600–1:51.000 | Ticking clock — dry metronome click, accelerating from 200ms interval to 80ms interval | Continuous | −28dB | Timer countdown begins |
| 1:51.000 | 50ms silence (tick stops, beat continues, then…) | 50ms | Silence | Timer hits 0:00:00 |
| 1:51.050 | Crash/impact — layered sub-bass (40–60Hz) + mid impact (200–400Hz) + metallic transient (2–4kHz). Sharp attack, 400ms decay. | 400ms | −16dB | Divider shatters, panels merge |
| 1:51.300 | Sub-bass hit — 40Hz sine wave, 150ms, fast attack, medium decay | 150ms | −22dB | Red flash + CONFLICT text impact |
| 1:55.000 | Percussive hit (same sample as 1:30) | 200ms | −18dB | Number card "3" slam |
| 1:59.500 | DAG node crumble #1 — granular texture (sand/ash), soft | 600ms | −26dB | `git reset --hard` dissolves nodes |
| 2:00.900 | DAG node crumble #2 — same texture | 600ms | −26dB | `git push --force` dissolves nodes |
| 2:02.300 | DAG node crumble #3 — same texture | 600ms | −26dB | `git rebase --drop` dissolves nodes |
| 2:10.000 | Percussive hit (same sample as 1:30) | 200ms | −18dB | Number card "4" slam |
| 2:13.500–2:20.000 | Vault files whooshing — sustained breathy whoosh, building in intensity and pitch | Continuous | −30dB → −22dB | File stream from vault |
| 2:24.000 | Percussive hit (same sample as 1:30) | 200ms | −18dB | Number card "5" slam |
| 2:36.800 | Wooden clack — dry Jenga-block-on-table impact, no reverb | 200ms | −22dB | Top block falls off tower |
| 2:38.000 | Music drop — ALL elements stop except synth pad | Instant | Silence + pad at −28dB | Bridge begins |

### SFX Production Notes

- The five percussive hits (number card slams) MUST be the same sample. This creates a Pavlovian expectation: the viewer hears the hit and knows "new problem incoming." Consistency is the point.
- All SFX must duck when the narrator is speaking. The narrator's voice is mixed at −12 LUFS and has absolute priority. SFX peaks (e.g., the CONFLICT crash at −16dB) are acceptable only in narrator pauses.
- The crumble sounds (Problem 3) should use a granular synthesis source, not a foley recording. The goal is an abstracted, slightly unnatural crumble — matching the abstracted visual of DAG nodes dissolving into particles. It should not sound like a real building collapsing.

---

## Color Palette for This Scene

Act II uses a deliberately restricted and aggressive palette. Warm colors are entirely absent (no sepia from Act I). Cool colors dominate. Warning Red carries the emotional weight.

| Color | Hex | Role in Act II |
|---|---|---|
| Deep Navy | `#0A0F1A` | Background — every shot, every frame. The void. |
| Code Background | `#111827` | Card fills (platform badges, Jenga blocks), terminal chrome, vault interior |
| White | `#F0F0F0` | Primary text (narration-synced), file icons, jargon terms |
| Warning Red | `#FF3B3B` | Number cards, CONFLICT text, danger indicators, punchline emphasis, sensitive files, destructive commands, "Please Don't Touch This" |
| Confident Green | `#00C48F` | Healthy DAG nodes (Problem 3 Phase 1 ONLY), Alice's label color (Problem 2) |
| Accent Cyan | `#00D4FF` | **BRIDGE LINE 2 ONLY (2:40–2:45).** Withheld from the entire problem sequence. Its first appearance is the signal that the solution is coming. |
| Muted Gray | `#6B7280` | Damaged/dimmed nodes, secondary text, Git foundation block label, protocol critique text |
| Dim Gray | `#374151` | Borders, connecting lines, divider lines, vault illustration strokes, block borders |

### Color Usage Rules

1. **No cyan before 2:40.** This is the most important color rule in Act II. Cyan is the W0rkTree brand color. Its absence during the problems section creates a subconscious longing; its sudden appearance in the bridge line creates relief and hope. If any element before 2:40 accidentally uses cyan, it undermines the entire emotional arc of the scene.

2. **Warning Red is for DANGER, not decoration.** Every use of `#FF3B3B` must correspond to something broken, dangerous, or wrong. Number cards (problems are bad), CONFLICT text (merges failed), destructive commands (history is being destroyed), sensitive files (secrets are leaking), the "Please Don't Touch This" block (the system is fragile). Do not use red for decorative accents or unrelated UI elements.

3. **Green is ONLY for "before" states.** Confident Green `#00C48F` appears only in contexts where something is healthy BEFORE it gets destroyed or imperiled — the DAG nodes before dissolution, Alice's label before the CONFLICT. Green never appears in a resolved or permanent state in Act II. This is deliberate: in Act II, nothing is okay.

4. **The 15% / 10% dim rule.** When a complex visual (jargon wall, damaged DAG) needs to serve as background texture for a punchline card, it dims to 10–15% opacity. This ensures the punchline text has clean contrast while the visual history remains faintly present — a ghost of the problem that was just demonstrated.

---

## Reusable Component: Number Card

The five number cards (Problems 1–5) share identical structure, animation, and SFX. This section defines the component once; each problem's Shot X.1 inherits from this spec.

### Layout

```
┌─────────────────────────────────────────────┐
│                                             │
│                                             │
│               ╔═══════════╗                 │
│               ║     N     ║                 │  ← Number: `display-xl`, #FF3B3B, centered
│               ╚═══════════╝                 │
│              THE SUBTITLE                   │  ← Subtitle: `heading-1`, #F0F0F0, centered, 64px below number
│                                             │
│                                             │
│                    ◉                        │  ← Red Alert glow: radial, #FF3B3B at 8%, 600px radius (4K)
│                                             │
└─────────────────────────────────────────────┘
```

### Properties

| Property | Value |
|---|---|
| Number typography | `display-xl` — 192px at 4K / 96px at HD, Inter Tight 900 |
| Number color | Warning Red `#FF3B3B` |
| Subtitle typography | `heading-1` — 96px at 4K / 48px at HD, Inter 700 |
| Subtitle color | `#F0F0F0` |
| Background | Deep Navy `#0A0F1A` |
| Glow | Radial gradient, centered on number, Warning Red `#FF3B3B` at 8% opacity, 600px radius (4K) / 300px radius (HD) |

### Animation Timeline

| Offset | Element | Property | From | To | Duration | Easing |
|---|---|---|---|---|---|---|
| 0ms | Number | scale | 0.8 | 1.0 | 400ms | `ease-bounce` |
| 0ms | Number | opacity | 0.0 | 1.0 | 400ms | `ease-bounce` |
| 0ms | Red glow | opacity | 0.0 | 0.08 | 400ms | `ease-enter` |
| 400ms | Subtitle | opacity | 0.0 | 1.0 | 600ms | `ease-enter` |
| 400ms | Subtitle | translate-y | +16px | 0px | 600ms | `ease-enter` |
| 1000ms | — | Hold (both visible) | — | — | 800ms | — |
| 1800ms | All elements | opacity | 1.0 | 0.0 | 300ms | `ease-standard` |

Total duration: 2,100ms (2.0s of the 2.0s timecode allocation, with the 300ms fade overlapping the beginning of the next shot).

### SFX

Percussive hit at offset 0ms. Layered:
- Low tom: 80Hz fundamental, sharp attack (<10ms), 200ms decay
- Kick drum: 50Hz sub-bass thud, sharp attack (<10ms), 150ms decay
- Combined mix level: −18dB
- The same sample is used for all five number cards without variation.

### Per-Problem Substitutions

| Problem | Number | Subtitle |
|---|---|---|
| 1 | `1` | `THE JARGON WALL` |
| 2 | `2` | `THE INVISIBLE TEAM` |
| 3 | `3` | `DESTRUCTION IS ONE COMMAND AWAY` |
| 4 | `4` | `THE SECURITY VACUUM` |
| 5 | `5` | `THE MONOREPO MELTDOWN` |

---

## Editorial Notes

### Pacing

- Each problem gets 12–15 seconds. The editor must be ruthless. If a visual has communicated its point, cut. Never linger on a completed animation waiting for the next narrator line — either tighten the visual's build time or start the next visual element's entrance earlier.
- The five number cards are the structural backbone of Act II. They tell the viewer "there is a list, you are progressing through it, you are N of 5." This makes the dense, fast-moving content navigable and prevents information overload. The viewer always knows where they are.
- If Act II runs long in edit, the **FIRST** cuts should be from the supporting visuals (jargon cascade, file stream, tower build), **NOT** the number cards or the bridge. The number cards and the bridge are sacred — they are the skeleton of the scene. Supporting visuals are the flesh; they can be trimmed.

### The Bridge Is the Pivot

The Bridge (2:38–2:45) is the emotional pivot of the entire video. Everything before it is critique. Everything after it is creation. The bridge must feel like a weight being lifted — like the first breath after holding your breath underwater.

Three things must synchronize perfectly at the bridge:
1. **Color**: Warning Red disappears. Accent Cyan appears. (Visual cue.)
2. **Music**: The beat drops to silence, then resolves to a major chord. (Audio cue.)
3. **Voice**: The narrator's tone shifts from frustrated enumeration to calm, purposeful confidence. (Vocal cue.)

All three cues must land within 500ms of each other. If any one of them is early or late, the pivot feels muddy. Test this transition obsessively.

### Narrator Performance Direction

- **Problems 1–5**: The narrator's energy should be controlled exasperation — not angry, not sarcastic, not mocking. Think: a very smart engineer explaining to a friend why they're frustrated. The tone is "I can't believe we've all just accepted this." Each problem should feel like the narrator is pulling back a curtain on something the audience already suspected but never articulated.
- **Bridge**: Vocal register drops. Pace slows by approximately 15%. The narrator is no longer listing problems — they are asking a genuine question. The words "should have existed all along" must land with quiet conviction, not with a sales pitch.

### Punchline Timing

Three of the five problems end with text-based punchlines (4A.3, 4C.3, 4D.3). These punchlines function as visual mic-drops — they crystallize each problem into a single memorable phrase. The hold times on these punchlines (2.0–2.8 seconds of stationary text) are intentionally long relative to Act II's otherwise aggressive pacing. This breathing room prevents the audience from feeling bulldozed and gives each problem its own closure before the next number card hits.

---

## Technical Notes

### Frame Rate and Motion

- All animations are rendered at 60fps for smooth motion
- The Jenga tower wobble and DAG node pulse animations are particularly sensitive to frame rate — at 30fps, these subtle oscillations look jerky. 60fps is mandatory for these elements.
- The CONFLICT text's `ease-bounce` animation (scale 1.5→1.0 with overshoot) contains approximately 3 sub-frames of overshoot at 60fps. At lower frame rates, the bounce feels like a glitch rather than a physical impact. Do not render below 60fps.

### Particle Systems

Three shots in this scene use particle effects:
1. **Shot 4B.3** — Divider shatters into shards (12–16 rigid body particles)
2. **Shot 4C.2** — DAG nodes dissolve into dust (20–30 particles per node, 3 dissolution events)
3. **Shot 4D.2** — File stream from vault (20–30 file icons simultaneously)

All particle systems must:
- Use deterministic random seeds (reproducible between renders)
- Respect the bounding box of the 16:9 frame (no particles visible outside the safe area)
- Fade to 0% opacity before exiting the frame (no hard clip at edges)
- Complete all motion within their shot's timecode (no particles lingering into the next shot after a hard cut)

### Typography Rendering

- All text in this scene is rendered in real-time (motion graphics), not pre-rendered as image assets
- Anti-aliasing: subpixel rendering enabled for all typography at 4K. At HD (1080p), switch to grayscale anti-aliasing to avoid color fringing on smaller text sizes.
- The `display-xl` number cards and `display-lg` CONFLICT text should be tested for readability at 720p (the lowest delivery resolution). If any text becomes illegible at 720p, increase the weight or size for that deliverable per `12_SOCIAL_CUTS.md`.

### Safe Areas

- **Title safe**: 5% inset from all edges (192px at 4K, 96px at HD). All readable text (numbers, subtitles, punchlines, labels) must fall within title safe.
- **Action safe**: 3.5% inset from all edges (134px at 4K, 67px at HD). All significant animation (DAG nodes, tower blocks, vault door) must fall within action safe.
- **Jargon cascade exception**: Shot 4A.2's jargon terms are intentionally allowed to enter from OUTSIDE the action safe area (they originate from the frame edges). However, terms must reach the action safe area before becoming legible. No readable text should exist outside action safe.
- **File stream exception**: Shot 4D.2's file icons may exit through the action safe boundary as they fly out of the vault. This is intentional — the files are escaping, and the frame edge represents the limit of control.

### Accessibility

- All punchline text meets WCAG AAA contrast ratio against the dimmed backgrounds:
  - `#F0F0F0` on `#0A0F1A` at 15% dim = contrast ratio >15:1 ✓
  - `#FF3B3B` at 80% on `#0A0F1A` at 10% dim = contrast ratio >7:1 ✓
  - `#00D4FF` on `#0A0F1A` = contrast ratio >9:1 ✓
- The red flash in Shot 4B.3 is a single 100ms pulse at 20% opacity. It does NOT meet the threshold for photosensitive seizure risk (which requires 3+ flashes per second at >25% of screen area at high contrast). However, it should be noted in the accessibility review and tested with sensitivity screening tools.
- Subtitles (SRT/VTT) must be generated for all narrator dialogue per `00_PRODUCTION_BIBLE.md` delivery specs. Git command names (`git reset --hard`, `git push --force`, etc.) must be spelled exactly as rendered on screen.

---

*This document is subordinate to `00_PRODUCTION_BIBLE.md`. In any conflict between this document and the production bible, the production bible wins. Shot-level timing may be adjusted ±500ms during editorial, provided segment-level timecodes (problem boundaries) remain fixed and total scene duration does not exceed 75 seconds.*