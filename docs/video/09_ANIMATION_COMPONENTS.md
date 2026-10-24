# Animation Component Library

This document defines every reusable animation component in the W0rkTree launch video. Components are defined once here and referenced by the scene specs. Treat this as the "component library" — if a motion graphics artist needs to build something, it's specced here.

> **Design System Colors Reference**
>
> | Token | Hex | Usage |
> |---|---|---|
> | Deep Navy | `#0A0F1A` | Primary background |
> | Code Background | `#111827` | Terminal / card backgrounds |
> | White | `#F0F0F0` | Primary text |
> | Accent Cyan | `#00D4FF` | Accents, highlights, interactive elements |
> | Warning Red | `#FF3B3B` | Errors, problems, danger |
> | Confident Green | `#00C48F` | Success, confirmation |
> | Muted Gray | `#6B7280` | Labels, secondary text |
> | Dim Gray | `#374151` | Borders, dividers |

---

## Table of Contents

1. [Terminal Typing Animation](#1-terminal-typing-animation)
2. [Number Card Slam](#2-number-card-slam)
3. [Feature Card Entry](#3-feature-card-entry)
4. [Diagram Node Build](#4-diagram-node-build)
5. [Text Entry Animation](#5-text-entry-animation)
6. [Break-Apart / Dissolution](#6-break-apart--dissolution)
7. [Logo Glow Animation](#7-logo-glow-animation)
8. [Stack Overflow Card Scroll](#8-stack-overflow-card-scroll)
9. [Split-Screen Wipe](#9-split-screen-wipe)
10. [Strikethrough Animation](#10-strikethrough-animation)

---

## 1. TERMINAL TYPING ANIMATION

The most-used animation in the video. Used in **Cold Open** and **Act IV**.

### Parameters

| Parameter | Default | Description |
|---|---|---|
| `char_delay` | 40ms | Time between each character appearing |
| `char_delay_slow` | 80ms | Slower typing for emphasis |
| `space_pause` | 0ms | Additional pause after space characters (set to 0 for normal flow, 100ms for thoughtful typing) |
| `command_pause` | 300ms | Pause after command fully typed, before output begins |
| `output_line_delay` | 80ms | Time between each output line appearing |
| `cursor_blink_interval` | 500ms | Block cursor blink rate |
| `cursor_color` | `#00D4FF` | Cursor fill color |

### Behavior Spec

1. Terminal starts with a blinking block cursor at the prompt position.
2. When typing begins, cursor **stops blinking** and becomes solid.
3. Characters appear one at a time at `char_delay` intervals, left to right.
4. The cursor moves rightward with each character (it is **ALWAYS** at the insertion point).
5. When the command is fully typed, cursor blinks twice (at `cursor_blink_interval`).
6. After `command_pause`, the cursor moves to the next line.
7. Output appears line by line at `output_line_delay` intervals.
8. Output lines appear **FULLY** (not character-by-character) — only commands are typed.
9. After all output is shown, cursor blinks at the next prompt position.
10. Between shots: **hard cut** to a fresh terminal (no animation to clear).

### Prompt Rendering

```
$ ← prompt symbol in #6B7280
  ← one space
command text ← in #F0F0F0
```

The prompt symbol and space are **NEVER** typed — they are pre-rendered when the shot begins. Only the command text after the `$ ` types character-by-character.

### Color Rules for Terminal Text

| Element | Color |
|---|---|
| Prompt `$` | `#6B7280` (Muted Gray) |
| `wt` command prefix | `#00D4FF` (Accent Cyan) |
| `git` command prefix | `#FF3B3B` (Warning Red) |
| Command arguments | `#F0F0F0` (White) |
| Flags (`--message`, `--team`) | `#6B7280` (Muted Gray) |
| String arguments (`"message text"`) | `#F0F0F0` (White) |
| Output text (normal) | `#F0F0F0` at 80% opacity |
| Success checkmark `✓` | `#00C48F` (Confident Green) |
| Failure cross `✗` | `#FF3B3B` (Warning Red) |
| Error text / `CONFLICT` / `DENIED` | `#FF3B3B` (Warning Red) |
| File paths in output | `#00D4FF` (Accent Cyan) |
| Branch names in output | `#00D4FF` (Accent Cyan) |
| Snapshot IDs | `#00D4FF` (Accent Cyan) |
| Labels (`Branch:`, `Worker:`, `Policy:`) | `#6B7280` (Muted Gray) |
| Email addresses | `#F0F0F0` (White) |
| Metadata (`Scope:`, `Source:`) | `#6B7280` (Muted Gray) |
| Numeric values | `#00D4FF` (Accent Cyan) |

### Implementation Notes

- The typing animation **MUST** look real. If using After Effects: animate the text with a typewriter effect at the specified rate. Do **NOT** use a "reveal" mask — the characters must appear one at a time from left to right.
- For screen recordings: use a scripted replay tool (like `asciinema` or a custom script) to type at controlled speed, then composite into the terminal component.
- The cursor is a **filled rectangle**, exactly the width of one character in JetBrains Mono and the height of the line-height. It sits directly on the character position.
- When the cursor blinks, the cycle is: **visible** for `cursor_blink_interval`, **invisible** for `cursor_blink_interval`. One "blink" = one full visible/invisible cycle.
- The terminal background is always `#111827` (Code Background). There is no window chrome, no title bar, no close/minimize buttons. The terminal is a raw rendering surface.

---

## 2. NUMBER CARD SLAM

Used in **Act II** for each of the 5 problem numbers.

### Parameters

| Parameter | Value |
|---|---|
| `number_font` | Inter Tight, 900 weight |
| `number_size` | `display-xl` (192px at 4K) |
| `number_color` | `#FF3B3B` (Warning Red) |
| `glow_color` | `#FF3B3B` at 15% opacity |
| `glow_radius` | 400px at 4K |
| `subtitle_font` | Inter, 700 weight |
| `subtitle_size` | `heading-1` (96px at 4K) |
| `subtitle_color` | `#F0F0F0` (White) |
| `slam_duration` | 400ms |
| `slam_easing` | ease-bounce: `cubic-bezier(0.34, 1.56, 0.64, 1.0)` |
| `subtitle_delay` | 400ms after number lands |
| `subtitle_duration` | 600ms |
| `subtitle_easing` | ease-enter: `cubic-bezier(0.0, 0.0, 0.2, 1.0)` |

### Animation Sequence

1. **Frame 0**: Pure Deep Navy background (`#0A0F1A`). Subtle Red Alert radial gradient centered (`#FF3B3B` at 15% opacity, 400px radius at 4K, Gaussian blur falloff).
2. **0–400ms**: Number enters from `scale(0.8)` + `opacity(0)` to `scale(1.0)` + `opacity(1.0)`, ease-bounce. The bounce overshoots to approximately `scale(1.05)` before settling at `1.0`.
3. **400ms**: Number is centered on screen. SFX triggers: percussive hit.
4. **400–1000ms**: Number holds centered. No movement.
5. **800ms** (400ms after land): Subtitle text enters below number. `translate-y(+16px)` + `opacity(0)` → `translate-y(0)` + `opacity(1)`, ease-enter, 600ms duration.
6. **1400ms–2000ms**: Both number and subtitle hold. No movement.
7. **2000ms**: Transition begins (fade or hard cut to supporting visual).

### Layout

- Number is positioned at **40% from top** (slightly above true center — visually centered accounting for subtitle below).
- Subtitle is positioned **48px below** the number baseline.
- Both elements are **horizontally centered**.
- The red glow gradient is centered on the number, not the frame. It is a purely radial gradient — no directional bias.

### SFX

A percussive hit synchronized to the number reaching `scale(1.0)`. Sound: low tom hit (80–120Hz fundamental) layered with a kick drum transient. Sharp attack (<10ms), short decay (~200ms). -18dB LUFS. **NO** reverb — the hit should be dry and precise.

### Per-Problem Content

| Problem # | Number Text | Subtitle Text |
|---|---|---|
| 1 | `1` | The jargon wall |
| 2 | `2` | Merge roulette |
| 3 | `3` | History rewrites |
| 4 | `4` | Secret sprawl |
| 5 | `5` | Tribal knowledge |

---

## 3. FEATURE CARD ENTRY

Used in **Act III Segment 5D** for the four feature pillar cards.

### Parameters

| Parameter | Value |
|---|---|
| `card_width` | 800px at 4K |
| `card_bg` | `#111827` (Code Background) |
| `card_border` | 1px solid `#374151` (Dim Gray) |
| `card_accent` | 4px solid `#00D4FF` (Accent Cyan) on left edge |
| `card_radius` | 24px |
| `card_padding` | 48px all sides |
| `title_style` | `heading-2`, `#F0F0F0` (White) |
| `body_style` | `body-lg`, `#F0F0F0` at 80% opacity |
| `entry_translate_x` | 80px |
| `entry_duration` | 400ms |
| `entry_easing` | ease-enter: `cubic-bezier(0.0, 0.0, 0.2, 1.0)` |
| `stagger_delay` | 200ms between cards |

### Animation Sequence (Per Card)

1. Card starts at `translate-x(+80px)` + `opacity(0)`.
2. Animates to `translate-x(0)` + `opacity(1)` over 400ms, ease-enter.
3. Internal content (title, code snippet) fades in 100ms after card reaches position, 200ms duration.

### Stagger

Multiple cards enter sequentially. Card N+1 begins its entry **200ms after Card N begins** (NOT after Card N finishes). This creates an overlapping cascade.

**Stagger timeline for 4 cards:**

| Card | Entry Start | Entry End | Content Fade Start | Content Fade End |
|---|---|---|---|---|
| Card 1 | 0ms | 400ms | 500ms | 700ms |
| Card 2 | 200ms | 600ms | 700ms | 900ms |
| Card 3 | 400ms | 800ms | 900ms | 1100ms |
| Card 4 | 600ms | 1000ms | 1100ms | 1300ms |

Total time from first card entry to all content visible: **1300ms**.

### Card Layout

- Cards are arranged in a **2×2 grid** with 32px gutters.
- Grid is horizontally and vertically centered in the frame.
- Each card's left edge has the 4px cyan accent strip — this is inset inside the border-radius (the strip follows the curve of the top-left and bottom-left corners).

### Card Content Structure

```
┌─────────────────────────────────────┐
│ ▌ Feature Title (heading-2)         │
│ ▌                                   │
│ ▌ One-line description (body-lg)    │
│ ▌                                   │
│ ▌ ┌─ Code Snippet ──────────────┐   │
│ ▌ │ $ wt example-command        │   │
│ ▌ └─────────────────────────────┘   │
└─────────────────────────────────────┘
```

The cyan accent strip (`▌`) runs the full height of the card's left inner edge.

---

## 4. DIAGRAM NODE BUILD

Used in **Act III Segment 5B** for the architecture diagram.

### Container Animation

- Border draws **clockwise** starting from top-left corner.
- Stroke: 2px, `#374151` (Dim Gray).
- Duration: 1200ms.
- Easing: ease-standard: `cubic-bezier(0.2, 0.0, 0.0, 1.0)`.
- Fill (`#111827`) appears at **50%** of the draw completion (fills from top to bottom as a wipe reveal).
- Border-radius: 16px (the draw animation must follow the rounded corners).

### Node Animation

- Each node inside a container:
  - Starts at `opacity(0)` + `translate-y(+8px)`.
  - Animates to `opacity(1)` + `translate-y(0)`.
  - Duration: 400ms per node.
  - Easing: ease-enter: `cubic-bezier(0.0, 0.0, 0.2, 1.0)`.
  - Stagger: 200ms between sibling nodes.
- Node styling:
  - Background: `#0A0F1A` (Deep Navy) — slightly darker than container fill to create depth.
  - Border: 1px solid `#374151` (Dim Gray).
  - Border-radius: 8px.
  - Padding: 16px horizontal, 12px vertical.
  - Text: `body-md`, `#F0F0F0`.
  - Icon (if present): 20px, `#00D4FF`, positioned left of text.

### Connection Line Animation

- Line draws from source node to destination node.
- Style: dashed (12px dash, 8px gap), 3px stroke, `#00D4FF` (Accent Cyan).
- Draw duration: 800ms, ease-standard.
- After drawn: continuous `dash-offset` animation (dashes move in the direction of data flow, 2000ms per full cycle, linear easing). This creates a persistent "flowing" appearance.
- Label (if present) fades in 200ms after line draw completes. Label style: `caption`, `#6B7280`, positioned at the midpoint of the line, offset 8px perpendicular to the line direction.

### Data Flow Pulse

- A glow blob (`#00D4FF` at 30% opacity, 20px Gaussian blur radius) travels along the connection line path.
- Travel duration: 600ms, ease-standard.
- Triggered on specific narrative cues (see scene specs for exact timing).
- The glow is a radial gradient attached to a point that moves along the SVG/spline path of the connection line.
- The glow should be **additive** (Screen blend mode) so it brightens the dashed line as it passes over.
- Only one pulse per line at a time — never stack pulses.

### Build Order

The diagram builds in a specific order to match the narration. The general pattern is:

1. Central container draws first.
2. Nodes populate inside.
3. Outer containers draw one at a time, left to right or top to bottom.
4. Connection lines draw from central to outer.
5. Data flow pulses trigger on key narrative beats.

Exact build order is defined in the scene spec for Act III Segment 5B.

---

## 5. TEXT ENTRY ANIMATION

Generic text entry used throughout for titles, subtitles, and body copy.

### Variants

#### Standard Entry (most text)

- `translate-y(+16px)` + `opacity(0)` → `translate-y(0)` + `opacity(1)`
- Duration: 600ms
- Easing: ease-enter: `cubic-bezier(0.0, 0.0, 0.2, 1.0)`
- Use for: scene titles, subtitle text, body paragraphs, labels

#### Impact Entry (key terms)

- `scale(0.9)` + `opacity(0)` → `scale(1.0)` + `opacity(1)`
- Duration: 500ms
- Easing: ease-bounce: `cubic-bezier(0.34, 1.56, 0.64, 1.0)`
- Use for: emphasized phrases like "STAGED SNAPSHOT VISIBILITY", product name reveals, key concept introductions

#### Fade Only (subtle elements)

- `opacity(0)` → `opacity(1)`
- Duration: 400ms
- Easing: ease-standard: `cubic-bezier(0.2, 0.0, 0.0, 1.0)`
- Use for: timestamps, metadata text, attribution text, secondary labels

### Exit Variants

When text needs to leave the screen (not just cut away), use the reverse of the entry:

#### Standard Exit

- `translate-y(0)` + `opacity(1)` → `translate-y(-16px)` + `opacity(0)`
- Duration: 400ms (exits are faster than entries)
- Easing: ease-exit: `cubic-bezier(0.4, 0.0, 1.0, 1.0)`

#### Fade Exit

- `opacity(1)` → `opacity(0)`
- Duration: 300ms
- Easing: ease-standard

### Stagger Rules for Multi-Line Text

When multiple lines of text enter together (e.g., a title + subtitle):

- Line 1 enters first.
- Line 2 begins 150ms after Line 1 begins.
- Line 3 (if present) begins 150ms after Line 2 begins.
- All lines use the same variant (don't mix Standard Entry with Fade Only in the same text group).

---

## 6. BREAK-APART / DISSOLUTION

Used for **BitKeeper text crumbling** (Act I), **2005 text cracking** (Act I), and **DAG node dissolution** (Act II Problem 3).

### Text Crumble Variant

1. Text is pre-rendered as a bitmap, then split into fragments (approximately **20–40 pieces per character**). Fragment shapes should be irregular — jagged triangles and quadrilaterals, not uniform grid squares.
2. Each fragment gets:
   - Randomized velocity: `x: random(-200, 200)px/s`, `y: random(100, 400)px/s` (downward bias)
   - Randomized rotation: `random(-180°, 180°)` over the total duration
   - Opacity fade: `1.0 → 0.0` over the **last 40%** of the animation duration
3. Total duration: **800ms** (fast crumble) or **1200ms** (dramatic crumble)
4. Physics: simple ballistic — gravity acceleration at **980px/s²** at 4K scale. Fragments arc downward naturally.
5. Color: fragments maintain their **original color** as they fall. No color shift.
6. No collision — fragments pass through each other and through any other elements on screen.

#### Usage Table

| Instance | Duration | Fragment Count | Notes |
|---|---|---|---|
| BitKeeper text (Act I) | 800ms | ~30 per char | Fast, decisive. The text was struck down. |
| 2005 text (Act I) | 1200ms | ~25 per char | Dramatic. Cracks appear first (200ms), then fragments separate. |

### Crack Pre-Animation (2005 variant only)

Before the crumble begins, cracks appear across the text:

1. 3–5 crack lines draw across the text surface over 200ms.
2. Crack lines: 2px, `#FF3B3B` at 60% opacity.
3. Cracks originate from random points on the text surface and extend outward.
4. After cracks are drawn, the text splits along the crack lines + additional random fractures.
5. The crumble then proceeds as defined above.

### Node Dissolution Variant (DAG)

1. Node color transitions from current (`#00C48F` Confident Green) to `#FF3B3B` (Warning Red) over **200ms**, ease-standard.
2. Node then fragments into particles (smaller than text crumble — **40–80 pieces** per node).
3. Particles drift downward and outward, **slower** than text crumble:
   - Velocity: `x: random(-80, 80)px/s`, `y: random(40, 160)px/s`
   - No gravity — particles drift linearly (they float, not fall)
4. Duration: **800ms**.
5. Particles have a "dust" quality — smaller, more numerous, slower. Individual particles should be 2–6px in diameter.
6. Particle opacity: starts at 80%, fades to 0% over the full 800ms duration.
7. Connection lines attached to the dissolving node should also fade (`opacity 1.0 → 0.0`, 400ms, starting 200ms into the dissolution).

---

## 7. LOGO GLOW ANIMATION

Used in **Title Card** and **Close**. Applies to the "0" in "W0rkTree."

### Static Glow (Base Layer — Always Present When Logo Is On Screen)

- A **duplicate** of the "0" character, positioned **directly behind** the original, perfectly aligned.
- Gaussian blur: **20px** radius.
- Color: `#00D4FF` (Accent Cyan).
- Opacity: **30%**.
- Blend mode: **Screen** (or Additive in compositing software that supports it).
- The original "0" renders on top at full opacity — the glow sits behind and bleeds outward.

### Breathing Animation (Close Only)

- The glow **opacity** oscillates: `20% → 40% → 20%`.
- Cycle duration: **3000ms** (one full breath cycle).
- Easing: ease-standard on both the rise and fall.
- This runs **continuously** for the duration of the Close scene (approximately 15 seconds).
- The glow **blur radius** also oscillates slightly, synced with opacity: `18px → 22px → 18px`.
- The breathing should feel organic and calm — like a slow, confident pulse. Not a strobe. Not a flicker.

### Reveal Pulse (Title Card Only)

- During the logo fade-in, the glow does a **single pulse**: `0% → 30% → 45% → 30%`.
- Timing relative to the logo fade-in:
  - **0%** of fade-in: glow at 0% opacity (invisible)
  - **50%** of fade-in: glow begins rising
  - **80%** of fade-in: glow peaks at 45% opacity (brighter than its resting state)
  - **100%** of fade-in: glow settles to 30% opacity (resting state)
- This single pulse creates a "flash" effect that draws the eye to the zero, emphasizing the brand stylization.
- The blur radius during the pulse: peaks at **28px** (at the 80% mark), then settles to **20px**.

---

## 8. STACK OVERFLOW CARD SCROLL

Used in **Act I Segment 3D**.

### Card Template

| Parameter | Value |
|---|---|
| Width | `random(600px, 900px)` at 4K (randomized per card) |
| Height | Auto (based on content, typically 120–200px) |
| Background | `#111827` (Code Background) |
| Border | 1px solid `#374151` (Dim Gray) |
| Border-radius | 16px |
| Padding | 32px |
| Left accent | 4px solid `#FF8A3B` (Stack Overflow orange nod — **not** a design system color; used only here) |

### Card Content

- **Question text**: `body-lg`, `#F0F0F0` (White). 1–3 lines of plausible-sounding Git confusion questions.
- **Vote count**: `heading-3`, `#6B7280` (Muted Gray), positioned at the left side of the card (inside padding, before the question text).
- **Tag pills** (optional): small rounded rectangles (`border-radius: 4px`, `bg: #374151`, `text: #6B7280`, `caption` size) at the bottom of the card. Tags like `git`, `merge`, `rebase`, `git-workflow`.

### Scroll Behavior

1. Cards enter from **below the frame bottom**.
2. Each card scrolls **upward** at a constant speed.
3. Entry interval: starts at **400ms** between cards, **accelerates** to **100ms** between cards over the duration of the scene.
4. Scroll speed: starts at **100px/s**, accelerates to **300px/s** over the duration of the scene.
5. Cards that reach **80% of frame height** from the bottom begin fading (`opacity 1.0 → 0.3` over 200px of vertical travel).
6. Cards that reach the **top edge** of the frame are removed from rendering.
7. Cards have slight **x-offset randomization** (`±40px` from center) to avoid a rigid column feel.
8. Cards can **partially overlap** (they stack in z-order, newest on top). A card's top edge may cover the bottom 10–20px of the card above it.

### Performance Note

At peak speed (100ms interval, 300px/s scroll), there may be 8–12 cards on screen simultaneously. Ensure the composition can handle this without frame drops. Pre-render the card textures if necessary.

### Example Question Text (For Reference — Final Copy TBD)

- "How do I undo a git rebase that went wrong?"
- "Why does git merge create conflicts even when files weren't changed?"
- "Can someone explain the difference between rebase and merge?"
- "HELP: accidentally ran git push --force on main"
- "How to recover deleted branch after git gc?"

---

## 9. SPLIT-SCREEN WIPE

Used in **Act II Problem 2** (Merge Roulette).

### Setup Phase

1. Frame divides **vertically** at 50% (left half and right half).
2. Left panel slides in from the left edge: `translate-x(-100%)` → `translate-x(0)`.
3. Right panel slides in from the right edge: `translate-x(+100%)` → `translate-x(0)`.
4. Both panels animate simultaneously.
5. Duration: **400ms**, ease-standard: `cubic-bezier(0.2, 0.0, 0.0, 1.0)`.
6. A **4px divider line** appears between panels at animation completion: `#374151` (Dim Gray), full frame height.

### Panel Content

| Panel | Content | Background |
|---|---|---|
| Left | Developer A's terminal / code view | `#111827` (Code Background) |
| Right | Developer B's terminal / code view | `#111827` (Code Background) |

Both panels show simultaneous, conflicting work. The terminals inside the panels use the standard Terminal Typing Animation component but at **70% scale** to fit within half-frame.

### Collapse Phase

1. **Divider line shatters**: break-apart animation (use Text Crumble variant with the 4px-wide line as the source — ~50 small fragments). Duration: **300ms**.
2. **Both panels slam together** to center: `translate-x(±50%)` → `translate-x(0)`, **200ms**, ease-sharp: `cubic-bezier(0.4, 0.0, 0.6, 1.0)`.
3. The overlap of the two panels creates visual chaos — elements from both sides render on top of each other with no blending (hard overlap).
4. **"CONFLICT" text** slams on top of the chaos: ease-bounce (`cubic-bezier(0.34, 1.56, 0.64, 1.0)`), **300ms**. Text: `display-lg`, `#FF3B3B` (Warning Red), centered.
5. Red flash: frame-wide overlay of `#FF3B3B` at 10% opacity, 100ms, then fade to 0% over 200ms.

### Timing Summary

| Event | Start | Duration |
|---|---|---|
| Panels slide in | 0ms | 400ms |
| Divider visible | 400ms | holds |
| Content plays in panels | 400ms | ~6000ms (varies per scene spec) |
| Divider shatters | ~6400ms | 300ms |
| Panels slam together | ~6500ms | 200ms |
| CONFLICT text slam | ~6700ms | 300ms |
| Red flash | ~6700ms | 300ms |

(Exact timings depend on the scene spec. The above is the relative sequence.)

---

## 10. STRIKETHROUGH ANIMATION

Used in **Act III Segment 5A** for the "Not a Git wrapper" denials.

### Behavior

1. Text appears using **Standard Entry** animation (see [Text Entry Animation](#5-text-entry-animation)). Style: `heading-2`, `#6B7280` (Muted Gray). The text is intentionally muted — these are the wrong answers being dismissed.
2. **400ms** after text reaches full opacity, a horizontal line draws across the text from **left to right**.
3. Line specifications:
   - Stroke: **3px**, `#FF3B3B` (Warning Red)
   - Position: vertically centered on the text (aligned to the x-height center, not the bounding box center)
   - Length: **110%** of text width (extends 5% beyond each side for visual emphasis)
4. Draw duration: **300ms**, ease-sharp: `cubic-bezier(0.4, 0.0, 0.6, 1.0)`.
5. After strikethrough line completes, text opacity drops to **40%** over **200ms**, ease-standard.
6. The strikethrough line **remains at full opacity** — it does not fade with the text.

### SFX

A subtle "strike" or "slash" sound, timed to the start of the line draw. -28dB LUFS. Short (100–150ms). High-frequency transient (2–4kHz) with no low end. Think: a quick swipe, not a hit.

### Sequence for Multiple Denials

When multiple wrong answers are struck through in sequence:

| Step | Timing | Text |
|---|---|---|
| Denial 1 appears | 0ms | (see scene spec for text) |
| Denial 1 strikethrough | 400ms | |
| Denial 1 fades to 40% | 700ms | |
| Denial 2 appears | 800ms | (appears below Denial 1) |
| Denial 2 strikethrough | 1200ms | |
| Denial 2 fades to 40% | 1500ms | |
| Denial 3 appears | 1600ms | (appears below Denial 2) |
| Denial 3 strikethrough | 2000ms | |
| Denial 3 fades to 40% | 2300ms | |

Each denial stacks vertically with **24px spacing** between lines. The stack is vertically centered in the frame, adjusting as new items are added.

---

## Appendix A: Easing Function Reference

All easing functions used in this document, collected for convenience:

| Name | Value | Usage |
|---|---|---|
| `ease-standard` | `cubic-bezier(0.2, 0.0, 0.0, 1.0)` | General-purpose smooth motion |
| `ease-enter` | `cubic-bezier(0.0, 0.0, 0.2, 1.0)` | Elements entering the frame |
| `ease-exit` | `cubic-bezier(0.4, 0.0, 1.0, 1.0)` | Elements leaving the frame |
| `ease-bounce` | `cubic-bezier(0.34, 1.56, 0.64, 1.0)` | Impactful, attention-grabbing entries |
| `ease-sharp` | `cubic-bezier(0.4, 0.0, 0.6, 1.0)` | Quick, decisive movements |
| `linear` | `cubic-bezier(0.0, 0.0, 1.0, 1.0)` | Constant-speed motion (dash offsets, scrolls) |

---

## Appendix B: Component-to-Scene Cross-Reference

Quick reference showing where each component is used:

| Component | Scenes |
|---|---|
| Terminal Typing Animation | Cold Open (0:00–0:14), Act IV (4:15–4:40) |
| Number Card Slam | Act II — Problems 1–5 (1:30–2:36) |
| Feature Card Entry | Act III Segment 5D (3:45–4:05) |
| Diagram Node Build | Act III Segment 5B (2:55–3:15) |
| Text Entry Animation | Throughout — all scenes |
| Break-Apart / Dissolution | Act I (0:36, 1:00), Act II Problem 3 (1:57–2:06) |
| Logo Glow Animation | Title Card (0:25–0:30), Close (4:45–5:00) |
| Stack Overflow Card Scroll | Act I Segment 3D (1:10–1:30) |
| Split-Screen Wipe | Act II Problem 2 (1:42–1:54) |
| Strikethrough Animation | Act III Segment 5A (2:45–2:55) |

---

*This document is the single source of truth for all animation components. Scene specs reference components by name (e.g., "Use **Number Card Slam** component"). Any changes to animation behavior must be made here, not in the scene specs.*