# W0rkTree Launch Video — Visual Design System

> **Document version**: 1.0.0
> **Canvas**: 3840 × 2160 (4K UHD), downscaled to 1920 × 1080 (HD) for delivery
> **Frame rate**: 60 fps (motion graphics), 30 fps (final export)
> **Duration target**: 5 minutes (300 seconds)
> **Color space**: sRGB (Rec. 709)

---

## Table of Contents

1. [Color System](#1-color-system)
   1. [Primary Palette](#11-primary-palette)
   2. [Semantic Color Assignments](#12-semantic-color-assignments)
   3. [Gradient Definitions](#13-gradient-definitions)
   4. [Opacity Scale](#14-opacity-scale)
2. [Typography](#2-typography)
   1. [Font Families](#21-font-families)
   2. [Type Scale](#22-type-scale)
   3. [Text Rendering Rules](#23-text-rendering-rules)
3. [Layout & Grid](#3-layout--grid)
   1. [Canvas Grid](#31-canvas-grid-4k-3840--2160)
   2. [Terminal Layout](#32-terminal-layout)
   3. [Diagram Layout](#33-diagram-layout)
   4. [Card Layout (Feature Cards — Act III)](#34-card-layout-feature-cards--act-iii)
   5. [Number Card Layout (Act II Problem Numbers)](#35-number-card-layout-act-ii-problem-numbers)
4. [Motion & Animation Principles](#4-motion--animation-principles)
   1. [Easing Curves](#41-easing-curves)
   2. [Duration Scale](#42-duration-scale)
   3. [Animation Rules](#43-animation-rules)
5. [Component Specifications](#5-component-specifications)
   1. [Terminal Component](#51-terminal-component)
   2. [Logo Component](#52-logo-component)
   3. [Problem Number Card Component](#53-problem-number-card-component)
   4. [Feature Card Component](#54-feature-card-component)
   5. [Architecture Diagram Component](#55-architecture-diagram-component)
   6. [Dashboard Mockup Component](#56-dashboard-mockup-component)
6. [Iconography](#6-iconography)
7. [Do-Not List](#7-do-not-list)

---

## 1. Color System

Every color in this video is pulled from a locked palette of **eight named values**.
Nothing else may appear on screen. If a color is not in this section it does not exist.

---

### 1.1 Primary Palette

| Token              | Name                    | Hex       | RGB                  | HSL                     | Role                                                                                              |
| ------------------- | ----------------------- | --------- | -------------------- | ------------------------ | ------------------------------------------------------------------------------------------------- |
| `deep-navy`        | Deep Navy (Background)  | `#0A0F1A` | `rgb(10, 15, 26)`   | `hsl(221, 44%, 7%)`     | Primary canvas. Used on ≥ 90 % of frames. Every scene opens and closes on this color.             |
| `pure-white`       | Pure White (Primary Text) | `#F0F0F0` | `rgb(240, 240, 240)` | `hsl(0, 0%, 94%)`       | NOT `#FFFFFF`. Slightly warm to reduce eye strain against dark backgrounds. All primary copy.      |
| `accent-cyan`      | Accent Cyan (Brand Highlight) | `#00D4FF` | `rgb(0, 212, 255)`  | `hsl(190, 100%, 50%)`   | W0rkTree brand accent. Logo glow, key terms, diagram highlights, success states, interactive cues. |
| `warning-red`      | Warning Red (Git Problems) | `#FF3B3B` | `rgb(255, 59, 59)`  | `hsl(0, 100%, 62%)`     | **ONLY** used in Act II for Git problem callouts, error states, conflict markers.                  |
| `confident-green`  | Confident Green (Solutions) | `#00C48F` | `rgb(0, 196, 143)`  | `hsl(164, 100%, 38%)`   | Used in Act III and Act IV for success checkmarks, positive terminal output, solution states.       |
| `muted-gray`       | Muted Gray (Secondary Text) | `#6B7280` | `rgb(107, 114, 128)` | `hsl(220, 9%, 46%)`     | Timestamps, metadata, secondary labels, terminal prompts, code comments.                           |
| `dim-gray`         | Dim Gray (Tertiary / Disabled) | `#374151` | `rgb(55, 65, 81)`   | `hsl(218, 19%, 27%)`    | Background accents, disabled states, subtle borders, diagram connectors (default).                 |
| `code-bg`          | Code Background         | `#111827` | `rgb(17, 24, 39)`   | `hsl(221, 39%, 11%)`    | Slightly lighter than Deep Navy. Terminal panel fill, inline code blocks, diagram node fill.        |

> **Why not pure white?** On a deep-navy background, `#FFFFFF` creates too much luminance contrast
> at the subpixel level, producing visible fringing on consumer displays. `#F0F0F0` softens this
> without any perceptible loss in readability.

---

### 1.2 Semantic Color Assignments

The table below is the **single source of truth** for every color decision. Designers and
animators must reference this table — not the palette above — when assigning colors to elements.

| Use Case                                  | Token / Name          | Hex (with alpha)     | Opacity | Where Used                                                          |
| ----------------------------------------- | --------------------- | -------------------- | ------- | ------------------------------------------------------------------- |
| Background (default)                      | Deep Navy             | `#0A0F1A`            | 100 %   | Every scene background                                              |
| Background (terminal panel)               | Code Background       | `#111827`            | 100 %   | Cold Open terminal, Act IV terminal, any terminal panel             |
| Background (code block inside diagram)    | Code Background       | `#111827`            | 100 %   | Act III config snippets, TOML examples                              |
| Background (Act II number overlay)        | Warning Red at 15 %   | `#FF3B3B26`          | 15 %    | Full-screen radial fill behind each problem number                  |
| Primary text                              | Pure White            | `#F0F0F0`            | 100 %   | All narrator text, titles, headings, primary labels                 |
| Secondary text                            | Pure White at 80 %    | `#F0F0F0CC`          | 80 %    | Card body text, terminal output, secondary descriptions             |
| Brand accent                              | Accent Cyan           | `#00D4FF`            | 100 %   | Logo wordmark glow source, key terms, diagram active nodes          |
| Brand accent (glow effect)                | Accent Cyan at 30 %   | `#00D4FF4D`          | 30 %    | Logo glow halo, node highlight rings, connection pulse              |
| Error / problem                           | Warning Red           | `#FF3B3B`            | 100 %   | Git error text in terminals, Act II problem numbers, ✗ markers      |
| Success / solution                        | Confident Green       | `#00C48F`            | 100 %   | ✓ checkmarks, `wt` terminal success output, Act III/IV positive     |
| Git command text (in terminal)            | Warning Red at 80 %   | `#FF3B3BCC`          | 80 %    | `git` commands typed in Cold Open and comparison scenes             |
| W0rkTree command text (in terminal)       | Accent Cyan at 80 %   | `#00D4FFCC`          | 80 %    | `wt` commands typed in Act IV and comparison scenes                 |
| Terminal prompt symbol (`$`)              | Muted Gray            | `#6B7280`            | 100 %   | The `$` character preceding every command                           |
| Code comment text                         | Muted Gray            | `#6B7280`            | 100 %   | `# …` comments in TOML snippets and terminal annotations            |
| Subtle borders / separators              | Dim Gray              | `#374151`            | 100 %   | Diagram node borders, card borders, horizontal rules                |
| Diagram connector lines (default)        | Dim Gray              | `#374151`            | 100 %   | Lines/arrows between diagram nodes at rest                          |
| Diagram connector lines (active)         | Accent Cyan           | `#00D4FF`            | 100 %   | Lines/arrows that highlight when narration references them          |
| Number cards (Act II)                     | Warning Red           | `#FF3B3B`            | 100 %   | The large numerals 1–5                                              |
| Number card subtitle text                 | Pure White            | `#F0F0F0`            | 100 %   | The line of text that appears below each problem number             |
| Feature card left-accent bar             | Accent Cyan           | `#00D4FF`            | 100 %   | 4 px vertical bar on the left edge of feature cards in Act III      |
| Dashboard avatar circles                 | Per-person accent      | `#00D4FF` / `#00C48F` / `#FF8A3B` | 100 % | One unique accent per team member row                     |
| Dashboard file paths                     | Accent Cyan           | `#00D4FF`            | 100 %   | Monospace file path strings inside dashboard rows                   |
| Tagline text ("Version control, …")      | Pure White at 80 %    | `#F0F0F0CC`          | 80 %    | Logo scene tagline, Close scene tagline                             |
| Timestamp / caption labels               | Muted Gray            | `#6B7280`            | 100 %   | Any `[mm:ss]` markers, attributions, small metadata                 |
| CTA text ("star the repo")               | Accent Cyan           | `#00D4FF`            | 100 %   | Final call-to-action strings                                        |
| CTA URL / handle text                    | Pure White            | `#F0F0F0`            | 100 %   | github.com/… , @w0rktree, etc.                                      |
| Noise texture overlay                    | Pure White at 5 %     | `#F0F0F00D`          | 5 %     | Subtle film grain overlay composited on every frame (optional)      |

---

### 1.3 Gradient Definitions

All gradients are defined in CSS-style notation for easy hand-off to After Effects / Remotion.

| Token                | Type            | Definition                                                            | Use                                                      |
| -------------------- | --------------- | --------------------------------------------------------------------- | -------------------------------------------------------- |
| `navy-fade`          | Linear (↓)     | `linear-gradient(180deg, #0A0F1A 0%, #111827 100%)`                   | Terminal panel backgrounds, top-to-bottom depth cue       |
| `cyan-glow`          | Radial (circle) | `radial-gradient(circle, #00D4FF33 0%, transparent 70%)`              | Behind the logo, behind active diagram nodes              |
| `red-alert`          | Radial (circle) | `radial-gradient(circle, #FF3B3B1A 0%, transparent 60%)`             | Behind Act II problem number cards                        |
| `transition-wash`    | Linear (→)     | `linear-gradient(90deg, #0A0F1A 0%, #0D1424 50%, #0A0F1A 100%)`      | Horizontal wipe during act transitions                    |
| `green-pulse`        | Radial (circle) | `radial-gradient(circle, #00C48F1A 0%, transparent 60%)`             | Behind success moments in Act IV (subtle background fill) |

> **Implementation note:** In After Effects these map to radial ramp or 4-color gradient effects.
> In Remotion / CSS they can be applied directly via `background-image`.

---

### 1.4 Opacity Scale

A fixed set of opacity levels. No arbitrary values are permitted.

| Level  | Alpha (float) | Alpha (hex byte) | Use                                                         |
| ------ | ------------- | ----------------- | ----------------------------------------------------------- |
| 100 %  | `1.00`        | `FF`              | Primary elements — text, icons, active diagram nodes        |
| 80 %   | `0.80`        | `CC`              | Secondary emphasis — terminal command text, card body text  |
| 60 %   | `0.60`        | `99`              | Tertiary elements — timestamps, metadata                    |
| 30 %   | `0.30`        | `4D`              | Background accents — glow effects, highlight rings          |
| 15 %   | `0.15`        | `26`              | Subtle fills — number card overlays, card backgrounds       |
| 5 %    | `0.05`        | `0D`              | Ultra-subtle — noise texture overlay, ambient texture       |

> **Rule:** If a design calls for an opacity not on this list, round to the nearest defined level.

---

## 2. Typography

---

### 2.1 Font Families

Three font stacks cover every typographic need.

#### Monospace (Code / Terminal)

| Priority | Font             | Notes                                            |
| -------- | ---------------- | ------------------------------------------------ |
| 1        | **JetBrains Mono** | Weights 400 (Regular) and 700 (Bold). **Ligatures OFF.** |
| 2        | Fira Code        | First fallback                                   |
| 3        | Cascadia Code    | Second fallback                                  |
| 4        | monospace        | System generic                                   |

**CSS declaration:**
```
font-family: "JetBrains Mono", "Fira Code", "Cascadia Code", monospace;
font-feature-settings: "liga" 0, "calt" 0;
```

#### Sans-Serif (Titles / UI Text)

| Priority | Font        | Notes                                                            |
| -------- | ----------- | ---------------------------------------------------------------- |
| 1        | **Inter**   | Weights 400, 500, 600, 700, 800. Variable font preferred.        |
| 2        | Geist       | First fallback (Vercel's system font)                            |
| 3        | SF Pro      | macOS system fallback                                            |
| 4        | system-ui   | System generic                                                   |

**CSS declaration:**
```
font-family: "Inter", "Geist", "SF Pro", system-ui;
```

#### Display (Impact Titles)

| Priority | Font               | Notes                                                                               |
| -------- | ------------------ | ----------------------------------------------------------------------------------- |
| 1        | **Inter Tight**    | Weights 800 (ExtraBold) and 900 (Black). Narrower than Inter for display sizes.      |
| 2        | Inter Display       | Alternate optical size of Inter for large settings.                                  |
| 3        | Inter               | Graceful degradation.                                                               |

**CSS declaration:**
```
font-family: "Inter Tight", "Inter Display", "Inter", system-ui;
```

> **Usage restriction:** The Display stack is used **only** for:
> - Act II problem numbers (1–5)
> - The "STAGED SNAPSHOT VISIBILITY" reveal title
> - The W0rkTree logo wordmark

---

### 2.2 Type Scale

All sizes are defined at 4K (3840 × 2160). Divide by 2 for HD (1920 × 1080).

| Token          | 4K Size | HD Size | Weight            | Font Stack    | Line Height | Letter Spacing | Use                                                  |
| -------------- | ------- | ------- | ----------------- | ------------- | ----------- | -------------- | ---------------------------------------------------- |
| `display-xl`   | 192 px  | 96 px   | 900 (Black)       | Inter Tight   | 1.0         | −0.03 em       | Act II problem numbers (1–5)                         |
| `display-lg`   | 128 px  | 64 px   | 800 (ExtraBold)   | Inter Tight   | 1.1         | −0.02 em       | "STAGED SNAPSHOT VISIBILITY" and similar impact text |
| `heading-1`    | 96 px   | 48 px   | 700 (Bold)        | Inter         | 1.2         | −0.02 em       | Section titles ("HOW WE GOT HERE", act headings)     |
| `heading-2`    | 64 px   | 32 px   | 600 (SemiBold)    | Inter         | 1.3         | −0.01 em       | Sub-headings, feature card titles                    |
| `heading-3`    | 48 px   | 24 px   | 600 (SemiBold)    | Inter         | 1.3         | 0              | Card labels, dashboard headers, container labels     |
| `tagline`      | 56 px   | 28 px   | 400 (Regular)     | Inter         | 1.4         | +0.02 em       | "Version control, rebuilt from zero."                |
| `body-lg`      | 40 px   | 20 px   | 400 (Regular)     | Inter         | 1.5         | 0              | Subtitle text, dashboard body, card descriptions     |
| `body`         | 32 px   | 16 px   | 400 (Regular)     | Inter         | 1.5         | 0              | Small labels, metadata, diagram sub-labels           |
| `caption`      | 24 px   | 12 px   | 500 (Medium)      | Inter         | 1.4         | +0.05 em       | Timestamp labels, attribution, fine print            |
| `code-lg`      | 48 px   | 24 px   | 400 (Regular)     | JetBrains Mono | 1.6        | 0              | Terminal commands (main typed commands)               |
| `code`         | 36 px   | 18 px   | 400 (Regular)     | JetBrains Mono | 1.6        | 0              | Terminal output, inline code blocks, config lines    |
| `code-sm`      | 28 px   | 14 px   | 400 (Regular)     | JetBrains Mono | 1.6        | 0              | Terminal secondary output, file paths in dashboard   |

---

### 2.3 Text Rendering Rules

1. **Antialiasing mode:** Grayscale antialiasing everywhere. Subpixel antialiasing is **OFF**.
   This ensures clean, consistent edges on dark backgrounds regardless of display subpixel layout.

2. **Ligatures in monospace:** **DISABLED**. We want literal character sequences:
   - `!=` not `≠`
   - `->` not `→`
   - `=>` not `⇒`
   - `<=` not `≤`
   - `::` not a single glyph
   - Set `font-feature-settings: "liga" 0, "calt" 0;` explicitly.

3. **All-caps text:** Whenever text is rendered in uppercase, add `+0.08 em` letter-spacing on top
   of the token's defined tracking. This improves legibility for capitalized strings.
   - Example: `heading-1` at `−0.02 em` base → uppercase renders at `+0.06 em` effective.

4. **Maximum line width:**
   - Terminal / code: **72 characters** maximum per line. Wrap or truncate beyond this.
   - UI text (titles, subtitles, body): **60 characters** maximum per line.
   - If text must be longer, break it onto a second line rather than reducing font size.

5. **Alignment:**
   - Code blocks and terminal output: **always left-aligned**, never centered.
   - Titles and headings: **centered** on canvas unless used as a card label (then left-aligned within card).
   - Body text in cards: **left-aligned** within card padding.
   - Tagline: **centered** beneath the logo.

6. **Kerning:** Optical kerning ON for Inter and Inter Tight. Metric kerning for JetBrains Mono
   (monospace should not kern).

7. **Paragraph spacing:** When multiple lines of body text appear, use `1.5 ×` the font size as
   line-height and `0.75 ×` the font size as paragraph spacing (space between blocks, not lines).

---

## 3. Layout & Grid

---

### 3.1 Canvas Grid (4K: 3840 × 2160)

```
┌──────────────────────────────────────────────────────────────┐
│ 120 px safe-area inset (all edges)                           │
│  ┌──────────────────────────────────────────────────────────┐│
│  │ Safe area: 3600 × 1920                                   ││
│  │                                                          ││
│  │  ┌────────────────────────────────────────────────────┐  ││
│  │  │ Content max-width: 3200 px (centered)              │  ││
│  │  │ 200 px gutter left  ·  200 px gutter right         │  ││
│  │  │                                                    │  ││
│  │  │ 12-column grid                                     │  ││
│  │  │ Column width: 234.67 px                            │  ││
│  │  │ Gutter: 32 px                                      │  ││
│  │  │ (12 × 234.67) + (11 × 32) = 2816 + 352 = 3168 px  │  ││
│  │  │ + 16 px padding each side = 3200 px                │  ││
│  │  │                                                    │  ││
│  │  └────────────────────────────────────────────────────┘  ││
│  │                                                          ││
│  └──────────────────────────────────────────────────────────┘│
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

| Property                | Value               | Notes                                          |
| ----------------------- | ------------------- | ---------------------------------------------- |
| Canvas size             | 3840 × 2160 px      | 16 : 9 UHD                                     |
| Safe area inset         | 120 px all edges     | Nothing meaningful outside this boundary        |
| Safe area               | 3600 × 1920 px      | Active area for all content                     |
| Content max-width       | 3200 px              | Centered horizontally within safe area          |
| Side gutters            | 200 px each side     | (3600 − 3200) / 2                              |
| Column count            | 12                   |                                                |
| Column width            | ≈ 234.67 px          | (3200 − 11 × 32) / 12                          |
| Column gutter           | 32 px                |                                                |
| Baseline grid           | **8 px** increments  | All vertical spacing snaps to multiples of 8 px |

> **Important:** The 12-column grid is a compositional guide, not a rigid constraint. Terminal
> panels and full-width diagrams may span all 12 columns. Cards and split layouts should align
> to column boundaries.

---

### 3.2 Terminal Layout

The terminal panel is the most-used component (Cold Open, Act IV, comparison scenes).

| Property                  | Value                          | Notes                                        |
| ------------------------- | ------------------------------ | -------------------------------------------- |
| Width                     | 80 % of canvas = **3072 px**   | Centered horizontally                        |
| Height                    | Auto (content-driven)          | Minimum 600 px, maximum 1600 px              |
| Horizontal position       | Centered                       | (3840 − 3072) / 2 = 384 px from each edge   |
| Vertical position         | Vertically centered in safe area | Adjusted per scene for subtitle clearance   |
| Background                | `#111827` (Code Background)    | Solid fill, no gradient unless specified      |
| Border radius             | 16 px                          | All four corners                             |
| Border                    | None                           | No visible border stroke                     |
| Internal padding          | 48 px all sides                | Content begins 48 px inside the panel edge   |
| Window chrome             | **None**                       | No title bar, no traffic-light dots, no tabs |
| Cursor style              | Block cursor                   | Solid rectangle, not line cursor             |
| Cursor color              | `#00D4FF` (Accent Cyan)        |                                              |
| Cursor blink interval     | 500 ms                         | 500 ms visible, 500 ms hidden, repeating     |
| Prompt string             | `$ `                           | Dollar sign + space                          |
| Prompt color              | `#6B7280` (Muted Gray)         |                                              |
| Command text color        | `#F0F0F0` (Pure White)         | Unless semantically colored (see § 1.2)      |
| Monospace font            | JetBrains Mono, `code-lg`      | 48 px at 4K for typed commands               |
| Output font               | JetBrains Mono, `code`         | 36 px at 4K for command output               |
| Max visible lines         | 20                             | Scroll up if more output is needed           |

---

### 3.3 Diagram Layout

Applies to the two-runtime architecture diagram (Act III) and any explanatory diagrams.

| Property                          | Value                                              |
| --------------------------------- | -------------------------------------------------- |
| Diagram total width               | ≤ 3200 px (content max-width), centered             |
| Diagram total height              | ≤ 1600 px (leaves room for heading + caption)        |
| Node shape                        | Rounded rectangle                                   |
| Node border radius                | 16 px                                               |
| Node border                       | 2 px solid `#374151` (Dim Gray)                     |
| Node fill                         | `#111827` (Code Background)                         |
| Node label font                   | `heading-3` (Inter SemiBold, 48 px at 4K)           |
| Node label color                  | `#F0F0F0` (Pure White)                              |
| Node label alignment              | Centered within node                                |
| Node internal padding             | 32 px                                               |
| Node minimum size                 | 240 × 80 px                                        |
| Connection line stroke            | 3 px                                                |
| Connection line color (default)   | `#374151` (Dim Gray)                                |
| Connection line color (active)    | `#00D4FF` (Accent Cyan)                             |
| Connection line style (default)   | Solid                                               |
| Connection line style (remote)    | Dashed (12 px dash, 8 px gap)                       |
| Arrow heads                       | Filled triangle, 12 px tall, color matches line     |
| Connection label font             | `code` (JetBrains Mono, 36 px at 4K)                |
| Connection label color            | `#00D4FF` (Accent Cyan)                             |
| Connection label position         | Centered on midpoint of connection line              |
| Diagram caption / annotation font | `body` (Inter Regular, 32 px at 4K)                 |
| Diagram caption color             | `#6B7280` (Muted Gray)                              |
| Node-to-node gap                  | 48 px minimum                                       |

---

### 3.4 Card Layout (Feature Cards — Act III)

Feature cards present W0rkTree capabilities one at a time alongside the architecture diagram.

| Property                 | Value                                                     |
| ------------------------ | --------------------------------------------------------- |
| Card width               | 800 px                                                    |
| Card height              | Auto (content-driven)                                     |
| Card background          | `#111827` (Code Background)                               |
| Card border              | 1 px solid `#374151` (Dim Gray)                           |
| Card left accent         | 4 px solid `#00D4FF` (Accent Cyan), full height           |
| Card border radius       | 24 px                                                     |
| Card padding             | 48 px all sides                                           |
| Card title font          | `heading-2` (Inter SemiBold, 64 px at 4K)                |
| Card title color         | `#F0F0F0` (Pure White)                                    |
| Card body font           | `body-lg` (Inter Regular, 40 px at 4K)                    |
| Card body color          | `#F0F0F0` at 80 % opacity (`#F0F0F0CC`)                  |
| Card vertical spacing    | 24 px gap between title and body text                     |
| Card stack direction     | Vertical, top-aligned                                     |
| Card stack gap           | 24 px between cards                                       |
| Card horizontal position | **Right-aligned** within content area, leaving ≈ 2000 px on left for architecture diagram |
| Card entry animation     | `translate-x(80px)` + `opacity(0)` → `translate-x(0)` + `opacity(1)` |
| Card entry duration      | 400 ms, `ease-enter`                                      |
| Card stagger delay       | 200 ms between sequential cards                           |

---

### 3.5 Number Card Layout (Act II Problem Numbers)

Each of the five Git problems is introduced by a full-screen number card.

| Property                     | Value                                                                  |
| ---------------------------- | ---------------------------------------------------------------------- |
| Layout                       | Full canvas (3840 × 2160)                                              |
| Background                   | `#0A0F1A` (Deep Navy) solid fill                                       |
| Background accent            | `red-alert` radial gradient centered on the number position            |
| Number font                  | `display-xl` (Inter Tight Black, 192 px at 4K)                        |
| Number color                 | `#FF3B3B` (Warning Red)                                                |
| Number position              | Centered horizontally and vertically in the safe area                  |
| Number entry animation       | `scale(0.8)` + `opacity(0)` → `scale(1.0)` + `opacity(1)`            |
| Number entry duration        | 400 ms, `ease-bounce`                                                  |
| Subtitle font                | `heading-1` (Inter Bold, 96 px at 4K)                                 |
| Subtitle color               | `#F0F0F0` (Pure White)                                                 |
| Subtitle position            | Centered, 64 px below the number's text baseline                       |
| Subtitle entry delay         | 400 ms after the number reaches full opacity                           |
| Subtitle entry animation     | `translateY(16px)` + `opacity(0)` → `translateY(0)` + `opacity(1)`    |
| Subtitle entry duration      | 600 ms, `ease-enter`                                                   |
| Total hold time              | ≥ 2000 ms (2 seconds) before transitioning to supporting visuals       |
| Accompanying SFX             | Percussive hit (kick + snap), synced to the frame the number lands     |

---

## 4. Motion & Animation Principles

---

### 4.1 Easing Curves

Every animation in the video must use one of these named easing curves.
**Linear motion (`cubic-bezier(0, 0, 1, 1)`) is banned.**

| Token              | Cubic-Bezier                           | Character          | Use                                                         |
| ------------------ | -------------------------------------- | ------------------ | ----------------------------------------------------------- |
| `ease-standard`    | `cubic-bezier(0.40, 0.00, 0.20, 1.00)` | Smooth, natural    | Default for all motion that doesn't fit another category     |
| `ease-enter`       | `cubic-bezier(0.00, 0.00, 0.20, 1.00)` | Accelerates in     | Elements entering the frame from off-screen or from zero opacity |
| `ease-exit`        | `cubic-bezier(0.40, 0.00, 1.00, 1.00)` | Decelerates out    | Elements leaving the frame or fading to zero                 |
| `ease-bounce`      | `cubic-bezier(0.34, 1.56, 0.64, 1.00)` | Overshoot + settle | **Impact moments only:** problem numbers landing, "CONFLICT" text, stat counters |
| `ease-decelerate`  | `cubic-bezier(0.00, 0.00, 0.00, 1.00)` | Slow, gentle stop  | Logo fade-in, final text reveals, closing card               |
| `ease-sharp`       | `cubic-bezier(0.40, 0.00, 0.60, 1.00)` | Quick, precise     | Quick toggles, cursor blink, diagram line draw completion    |

> **After Effects mapping:**
> - `ease-standard` ≈ Keyframe velocity 40 % influence in, 20 % influence out
> - `ease-bounce` ≈ Overshoot expression with `amp = 1.56`, `freq = 1`
> - For exact curves, use the `cubic-bezier` values with the "Flow" or "Ease and Wizz" plug-in.

---

### 4.2 Duration Scale

| Token              | Duration | Frames (60 fps) | Frames (30 fps) | Use                                                     |
| ------------------ | -------- | ---------------- | ---------------- | ------------------------------------------------------- |
| `instant`          | 100 ms   | 6                | 3                | Cursor blink toggle, highlight flash, micro-interactions |
| `fast`             | 200 ms   | 12               | 6                | Card stagger, small element transitions, button states   |
| `standard`         | 400 ms   | 24               | 12               | Default element entry/exit, diagram node build           |
| `slow`             | 600 ms   | 36               | 18               | Text fades, subtitle reveals, diagram line draw          |
| `dramatic`         | 1000 ms  | 60               | 30               | Title card fades, act transitions, logo reveal           |
| `type-char`        | 40 ms    | 2.4 (≈ 3)       | 1.2 (≈ 1)       | Per-character typing speed in terminal (standard)        |
| `type-char-slow`   | 80 ms    | 4.8 (≈ 5)       | 2.4 (≈ 2)       | Per-character typing for emphasis / dramatic moments      |

---

### 4.3 Animation Rules

These rules are **inviolable**. Every animator on the project must follow them.

#### 4.3.1 No Linear Motion

Every movement — position, scale, opacity, rotation — must use one of the easing curves from
§ 4.1. If you are unsure which curve to use, use `ease-standard`.

#### 4.3.2 No Bouncing Text

The `ease-bounce` curve is reserved for:
- Act II problem numbers landing
- The word "CONFLICT" appearing
- Stat counter roll-ups (optional)

It must **never** be used on body text, subtitles, card entries, or diagram labels.

#### 4.3.3 Consistent Entry Direction

Within a single scene, all elements enter from the **same direction**. The allowed directions are:

| Scene Type           | Entry Direction                                      |
| -------------------- | ---------------------------------------------------- |
| Terminal scenes      | Typing appears left-to-right (natural for terminals) |
| Card scenes          | Cards slide in from the right                        |
| Diagram scenes       | Nodes build left-to-right, then top-to-bottom        |
| Title / number cards | Center-origin (scale up from center)                 |
| Act transitions      | Cross-fade (no directional movement)                 |

Elements must **never** enter from random or inconsistent directions within the same scene.

#### 4.3.4 Opacity + Translate Pairing

Opacity animations must **always** pair with a subtle Y-translate:
- **Enter:** `translateY(16px)` + `opacity(0)` → `translateY(0)` + `opacity(1)`
- **Exit:** `translateY(0)` + `opacity(1)` → `translateY(-16px)` + `opacity(0)`

Never animate opacity alone — the slight positional shift creates perceived physicality.

> **Exception:** The logo breathing glow (§ 5.2) animates opacity without translate,
> because it is an ambient effect on an already-visible element.

#### 4.3.5 Stagger Timing

When multiple elements enter sequentially (e.g., a list of cards, diagram nodes):
- Stagger delay: **150–200 ms** between items
- Each item uses the same duration and easing
- The first item begins at `t = 0`; the second at `t = 150–200 ms`; etc.

#### 4.3.6 Line Drawing

Diagram connection lines animate by drawing themselves along their path:
- Direction: **left-to-right** or **top-to-bottom** (whichever follows the reading direction)
- Duration: `slow` (600 ms) per line segment
- Easing: `ease-standard`
- Lines must **never** appear instantly

#### 4.3.7 Text Fade Durations

| Text Type        | Fade Duration | Easing          |
| ---------------- | ------------- | --------------- |
| Title text       | `slow` (600 ms) | `ease-enter`  |
| Body / subtitle  | `slow` (600 ms) | `ease-enter`  |
| Caption / meta   | `standard` (400 ms) | `ease-enter` |
| Terminal typing  | Per-character (`type-char`) | N/A (instant per char) |
| Terminal output  | Line-by-line, 50 ms gap | `ease-enter` per line |

#### 4.3.8 Diagram Build Timing

- Each node: `standard` (400 ms) to animate in
- Stagger between nodes: 300 ms
- Connection lines begin drawing 200 ms after their source node completes
- Total build for a 6-node diagram: ≈ 4–5 seconds
- Total build for the full architecture diagram: ≈ 8 seconds (see § 5.5)

#### 4.3.9 Logo Motion

The W0rkTree logo always uses:
- Duration: `dramatic` (1000 ms)
- Easing: `ease-decelerate`
- Entry: `opacity(0)` → `opacity(1)` paired with a subtle `scale(0.98)` → `scale(1.0)`

---

## 5. Component Specifications

---

### 5.1 Terminal Component

The terminal is the most frequently recurring visual in the video. It appears in:
- **Cold Open** (0:00–0:30): Git commands and error output
- **Act IV** (3:00–3:45): W0rkTree commands and success output
- **Comparison beats** (if any): Side-by-side git vs. wt

#### Visual Properties

| Property            | Value                                                          |
| ------------------- | -------------------------------------------------------------- |
| Background          | `#111827` (Code Background), solid fill                        |
| Border radius       | 16 px (all corners)                                            |
| Border              | None (no stroke)                                               |
| Box shadow          | None (see Do-Not List § 7)                                     |
| Padding             | 48 px (all sides)                                              |
| Width               | 80 % of canvas width = 3072 px at 4K                          |
| Horizontal position | Centered on canvas                                             |
| Vertical position   | Centered in safe area (adjustable per scene)                   |

#### Cursor

| Property     | Value                                    |
| ------------ | ---------------------------------------- |
| Style        | Block (solid filled rectangle)           |
| Size         | 1 character wide × 1 line tall           |
| Color        | `#00D4FF` (Accent Cyan)                  |
| Blink rate   | 500 ms on, 500 ms off, repeating         |
| Blink easing | Instant toggle (step function, not fade)  |

#### Prompt

| Property        | Value                                          |
| --------------- | ---------------------------------------------- |
| String          | `$ ` (dollar sign + single space)              |
| Color           | `#6B7280` (Muted Gray)                         |
| Font            | JetBrains Mono, `code-lg` (48 px at 4K)       |

#### Typed Commands

| Property          | Value                                                           |
| ----------------- | --------------------------------------------------------------- |
| Font              | JetBrains Mono, `code-lg` (48 px at 4K)                        |
| Default color     | `#F0F0F0` (Pure White)                                          |
| Git command color | `#FF3B3BCC` (Warning Red at 80 %)                               |
| wt command color  | `#00D4FFCC` (Accent Cyan at 80 %)                               |
| Typing speed      | `type-char` = 40 ms per character                               |
| Space pause       | 200 ms pause after each space in commands longer than 20 chars  |
| Post-type pause   | 300 ms pause after the full command is typed, before output     |

#### Command Output

| Property            | Value                                                       |
| ------------------- | ----------------------------------------------------------- |
| Font                | JetBrains Mono, `code` (36 px at 4K)                       |
| Default color       | `#F0F0F0` at 80 % opacity (`#F0F0F0CC`)                    |
| Error text color    | `#FF3B3B` (Warning Red) — for lines starting with `error:` |
| Success marker (✓)  | `#00C48F` (Confident Green)                                 |
| Failure marker (✗)  | `#FF3B3B` (Warning Red)                                     |
| Line appearance     | Lines appear one at a time, 50 ms gap between lines         |
| Line animation      | Instant opacity (no translate) — mimicking real terminal     |

#### Terminal Typing Choreography

```
t = 0 ms        Cursor appears, blinking
t = 500 ms      Cursor stops blinking (held visible)
t = 600 ms      First character of command appears
t = 600 + (N × 40) ms   N-th character appears
                 200 ms extra pause after each space (for commands > 20 chars)
t = end + 300 ms Output begins
t = output + (L × 50) ms   L-th line of output appears
t = final line + 800 ms    Cursor resumes blinking (ready for next command)
```

---

### 5.2 Logo Component

The W0rkTree logo appears in the **Opening title** (≈ 0:05) and the **Close / CTA** (4:30–5:00).

#### Wordmark

| Property         | Value                                                       |
| ---------------- | ----------------------------------------------------------- |
| Text             | `W0rkTree`                                                  |
| Font             | Inter Tight, weight 800 (ExtraBold)                         |
| Size             | `display-lg` (128 px at 4K) — or larger if scene demands    |
| Color            | `#F0F0F0` (Pure White) on transparent background            |
| Zero glow        | The character "0" in "W0rk" has a glow effect:              |
|                  | — Color: `#00D4FF` at 30 % opacity                          |
|                  | — Blur radius: 20 px                                        |
|                  | — Spread: 0 px                                              |
|                  | — Applied as outer glow / box-shadow on the "0" glyph only |

#### Logo Entrance Animation

| Property    | Value                                                        |
| ----------- | ------------------------------------------------------------ |
| Duration    | `dramatic` = 1000 ms                                         |
| Easing      | `ease-decelerate` = `cubic-bezier(0.0, 0.0, 0.0, 1.0)`     |
| Transform   | `scale(0.98)` + `opacity(0)` → `scale(1.0)` + `opacity(1)` |
| Glow delay  | Glow begins fading in 400 ms after text reaches full opacity |
| Glow fade   | 600 ms, `ease-standard`                                      |

#### Logo Breathing Glow (Close Scene)

During the final CTA screen, the logo's "0" glow oscillates to create a living, breathing feel.

| Property       | Value                                                    |
| -------------- | -------------------------------------------------------- |
| Animation      | Glow opacity oscillates between 20 % and 40 %           |
| Cycle duration | 3000 ms (one full oscillation)                           |
| Easing         | `ease-standard` (smooth sinusoidal feel)                 |
| Loop           | Infinite (runs until scene ends)                         |

#### Tagline

| Property        | Value                                                    |
| --------------- | -------------------------------------------------------- |
| Text            | "Version control, rebuilt from zero."                    |
| Font            | Inter Regular, `tagline` (56 px at 4K)                   |
| Color           | `#F0F0F0` at 80 % (`#F0F0F0CC`)                         |
| Position        | Centered, 48 px below the logo baseline                  |
| Entry delay     | 500 ms after the logo reaches full opacity               |
| Entry animation | `translateY(16px)` + `opacity(0)` → `translateY(0)` + `opacity(1)` |
| Entry duration  | `slow` = 600 ms                                          |
| Entry easing    | `ease-enter`                                              |

---

### 5.3 Problem Number Card Component

Used in **Act II** (0:30–2:00) to introduce each of the five Git problems.

#### Full Rendering Spec

**Frame composition:**
1. Full-canvas background: `#0A0F1A` (Deep Navy)
2. Centered radial gradient: `red-alert` (`radial-gradient(circle, #FF3B3B1A 0%, transparent 60%)`)
   — circle origin at the number's center point
3. Number glyph: `display-xl`, `#FF3B3B`, centered in safe area
4. Subtitle text: `heading-1`, `#F0F0F0`, centered, 64 px below number baseline

**Animation timeline:**

```
t = 0 ms          Scene begins (background + gradient visible)
t = 200 ms         Number enters:
                     From: scale(0.8) + opacity(0)
                     To:   scale(1.0) + opacity(1)
                     Duration: 400 ms
                     Easing: ease-bounce
t = 200 ms         SFX: percussive hit fires (synced to animation start)
t = 600 ms         Number is fully visible, bounce settles
t = 1000 ms        Subtitle enters:
                     From: translateY(16px) + opacity(0)
                     To:   translateY(0) + opacity(1)
                     Duration: 600 ms
                     Easing: ease-enter
t = 1600 ms        Subtitle fully visible
t = 1600–3600 ms   Hold (≥ 2000 ms total visible time)
t = 3600 ms        Begin transition to supporting visual:
                     Number + subtitle fade out together
                     Duration: 400 ms
                     Easing: ease-exit
```

**The five cards (for reference — content defined in storyboard):**

| Number | Subtitle Topic                                            |
| ------ | --------------------------------------------------------- |
| 1      | Fragile history model / rebase disasters                  |
| 2      | Merge conflicts as a workflow bottleneck                   |
| 3      | No visibility into teammate work-in-progress              |
| 4      | Poor large-file and monorepo performance                  |
| 5      | Bolted-on security / unsigned-by-default commits          |

---

### 5.4 Feature Card Component

Used in **Act III** (2:00–3:00) to present W0rkTree's solutions.

#### Visual Spec

| Property                 | Value                                                  |
| ------------------------ | ------------------------------------------------------ |
| Width                    | 800 px                                                 |
| Height                   | Auto (content-driven), minimum 200 px                  |
| Background               | `#111827` (Code Background)                            |
| Border                   | 1 px solid `#374151` (Dim Gray)                        |
| Left accent              | 4 px solid `#00D4FF` (Accent Cyan), full card height   |
| Border radius            | 24 px                                                  |
| Padding                  | 48 px all sides                                        |
| Title                    | `heading-2`, `#F0F0F0`                                 |
| Body                     | `body-lg`, `#F0F0F0CC` (80 % opacity)                  |
| Title-to-body gap        | 24 px                                                  |
| Card-to-card gap         | 24 px (vertical stack)                                 |
| Max cards visible        | 3 at once (stack scrolls if more are needed)           |

#### Entry Animation

| Property   | Value                                                                         |
| ---------- | ----------------------------------------------------------------------------- |
| Transform  | `translateX(80px)` + `opacity(0)` → `translateX(0)` + `opacity(1)`           |
| Duration   | `standard` = 400 ms                                                           |
| Easing     | `ease-enter` = `cubic-bezier(0.0, 0.0, 0.2, 1.0)`                           |
| Stagger    | 200 ms between sequential cards                                               |

#### Exit Animation

| Property   | Value                                                                         |
| ---------- | ----------------------------------------------------------------------------- |
| Transform  | `translateX(0)` + `opacity(1)` → `translateX(-40px)` + `opacity(0)`          |
| Duration   | `standard` = 400 ms                                                           |
| Easing     | `ease-exit` = `cubic-bezier(0.4, 0.0, 1.0, 1.0)`                            |
| Stagger    | 100 ms between sequential cards (faster exit than entry)                      |

#### Positioning

Cards are **right-aligned** within the content area:
- Cards occupy columns 7–12 of the grid (right half)
- Columns 1–6 (left half) are reserved for the architecture diagram
- This creates a natural split-screen composition

---

### 5.5 Architecture Diagram Component

The two-runtime architecture diagram is the centerpiece of **Act III**.

#### Overall Dimensions

| Property            | Value                                                   |
| ------------------- | ------------------------------------------------------- |
| Total width         | 2800 px at 4K, centered in content area                 |
| Total height        | ≤ 1400 px at 4K                                        |
| Layout              | Two-column: LOCAL (left half) and REMOTE (right half)   |
| Column gap          | 200 px (space for the connection line)                  |

#### Container Boxes

Each runtime (LOCAL and REMOTE) is wrapped in a container.

| Property            | Value                                                   |
| ------------------- | ------------------------------------------------------- |
| Width               | 1300 px each                                            |
| Height              | Auto (content-driven)                                   |
| Shape               | Rounded rectangle                                       |
| Border radius       | 16 px                                                   |
| Border              | 2 px solid `#374151` (Dim Gray)                         |
| Fill                | `#111827` (Code Background)                             |
| Label               | `heading-3`, `#6B7280` (Muted Gray)                    |
| Label position      | 16 px above the container's top edge, left-aligned      |
| Internal padding    | 32 px                                                   |

#### Internal Nodes

| Property            | Value                                                   |
| ------------------- | ------------------------------------------------------- |
| Shape               | Rounded rectangle                                       |
| Border radius       | 12 px                                                   |
| Border              | 1 px solid `#374151` (Dim Gray)                         |
| Fill                | `#0A0F1A` (Deep Navy — darker than container)           |
| Label               | `body-lg`, `#F0F0F0` (Pure White)                       |
| Sub-items           | `body`, `#6B7280` (Muted Gray), bulleted list           |
| Bullet style        | 6 px circle, `#374151`                                  |
| Node padding        | 24 px                                                   |
| Node-to-node gap    | 24 px within a container                                |

#### Connection (LOCAL ↔ REMOTE)

| Property              | Value                                                 |
| --------------------- | ----------------------------------------------------- |
| Line style            | Dashed (12 px dash, 8 px gap)                         |
| Line stroke           | 3 px                                                  |
| Line color            | `#00D4FF` (Accent Cyan)                               |
| Arrow heads           | Bidirectional filled triangles, 12 px, `#00D4FF`      |
| Label text            | "QUIC" in `code` font, `#00D4FF`                      |
| Label position        | Centered on the connection line's midpoint             |
| Dash animation        | Continuous dash-offset animation, 2000 ms cycle, linear (this is the one exception to the no-linear rule — continuous loops may use linear) |

#### Build Animation Sequence

```
t = 0 ms          LOCAL container fades in:
                     opacity(0) → opacity(1), 400 ms, ease-enter

t = 400 ms         First LOCAL node builds:
                     translateY(16px) + opacity(0) → translateY(0) + opacity(1)
                     400 ms, ease-enter

t = 700 ms         Second LOCAL node builds (300 ms stagger)
                     Same animation as above

t = 1000 ms        Third LOCAL node builds (300 ms stagger)

t = 1300 ms        (Additional LOCAL nodes continue at 300 ms stagger)

t ≈ 2500 ms        All LOCAL nodes complete.
                   Connection line begins drawing:
                     Left-to-right stroke animation
                     Duration: 600 ms, ease-standard
                     "QUIC" label fades in at midpoint of line draw

t ≈ 3100 ms        Connection complete.
                   REMOTE container fades in:
                     opacity(0) → opacity(1), 400 ms, ease-enter

t ≈ 3500 ms        First REMOTE node builds (same style as LOCAL nodes)

t ≈ 3800 ms        Second REMOTE node (300 ms stagger)

t ≈ 4100 ms        Third REMOTE node (300 ms stagger)

t ≈ 4400 ms        (Additional REMOTE nodes continue)

t ≈ 5500 ms        All REMOTE nodes complete.
                   Dash-offset animation on connection line begins
                   (continuous, runs for remainder of scene)

t ≈ 8000 ms        Full diagram build complete.
```

**Total build time: approximately 8 seconds.**

---

### 5.6 Dashboard Mockup Component

A custom motion graphic (not a screenshot) representing the W0rkTree team visibility dashboard.
Appears in **Act III** during the "visibility" feature beat.

#### Overall Frame

| Property           | Value                                                    |
| ------------------ | -------------------------------------------------------- |
| Style              | Clean web UI mockup — flat, minimal, on-brand            |
| Background         | `#111827` (Code Background)                              |
| Border             | 1 px solid `#374151`                                     |
| Border radius      | 16 px                                                    |
| Width              | 2400 px at 4K                                            |
| Height             | Auto, approximately 900 px                               |
| Position           | Centered on canvas                                       |
| Internal padding   | 48 px                                                    |

#### Header Bar

| Property           | Value                                                    |
| ------------------ | -------------------------------------------------------- |
| Text               | "W0rkTree — Team Activity"                               |
| Font               | `heading-3` (Inter SemiBold, 48 px at 4K)               |
| Color              | `#F0F0F0` (Pure White)                                   |
| Position           | Top of dashboard, left-aligned within padding            |
| Bottom border      | 1 px solid `#374151`, spanning full dashboard width      |
| Header padding     | 24 px bottom (space between text and divider)            |

#### Team Member Rows

Three rows, each representing one team member. All rows share the same structure.

| Property              | Value                                                 |
| --------------------- | ----------------------------------------------------- |
| Row height            | Auto, approximately 120 px                            |
| Row padding           | 24 px vertical                                        |
| Row divider           | 1 px solid `#374151` between rows                     |
| Avatar shape          | Circle, 48 px diameter                                |
| Avatar fill (solid)   | Unique per person (see table below)                   |
| Avatar position       | Left edge of row, vertically centered                 |
| Name font             | `body-lg` (Inter Regular, 40 px at 4K)               |
| Name color            | `#F0F0F0`                                             |
| Name position         | 24 px right of avatar, top-aligned                    |
| Activity font         | `body` (Inter Regular, 32 px at 4K)                   |
| Activity color        | `#6B7280` (Muted Gray)                                |
| Activity position     | Below name, 8 px gap                                  |
| File path font        | `code-sm` (JetBrains Mono, 28 px at 4K)              |
| File path color       | `#00D4FF` (Accent Cyan)                               |
| File path position    | Below activity, 8 px gap                              |

#### Per-Person Data

| Person | Avatar Color | Name                 | Activity                                          | File Path                        |
| ------ | ------------ | -------------------- | ------------------------------------------------- | -------------------------------- |
| Alice  | `#00D4FF`    | alice@company.com    | 3 staged snapshots on `feature/oauth`             | `auth-service/src/oauth.rs`      |
| Bob    | `#00C48F`    | bob@company.com      | 1 snapshot on `fix/token-expiry`                  | `auth-service/src/token.rs`      |
| Carol  | `#FF8A3B`    | carol@company.com    | 2 staged snapshots on `feature/dashboard`         | `web-app/src/views/dashboard.ts` |

> **Note on `#FF8A3B`**: This warm orange is the **only** color in the video that is not in the
> primary palette. It is permitted exclusively for Carol's avatar to ensure three visually
> distinct person-colors. It must not be used anywhere else.

#### Row Entry Animation

| Property          | Value                                                            |
| ----------------- | ---------------------------------------------------------------- |
| Transform         | `translateX(40px)` + `opacity(0)` → `translateX(0)` + `opacity(1)` |
| Duration          | `standard` = 400 ms                                              |
| Easing            | `ease-enter`                                                      |
| Stagger           | 200 ms between rows (Alice → Bob → Carol)                        |
| Dashboard entry   | The outer dashboard frame fades in first (400 ms, ease-enter), then header appears (200 ms delay), then rows stagger in |

---

## 6. Iconography

All icons are **custom-drawn**, line-style, consistent with the design system.
No stock icon libraries (Feather, Lucide, Material, etc.) are used.

### 6.1 Icon Grid

| Property          | 4K Value    | HD Value   |
| ----------------- | ----------- | ---------- |
| Artboard size     | 48 × 48 px | 24 × 24 px |
| Safe area         | 4 px inset  | 2 px inset |
| Stroke width      | 2 px        | 1 px       |
| Stroke end caps   | Round       | Round      |
| Stroke line join  | Round       | Round      |
| Corner radius     | 4 px (where applicable) | 2 px |

### 6.2 Icon Inventory

| Icon            | Description                                              | Stroke Color (default)            | Context                                  |
| --------------- | -------------------------------------------------------- | --------------------------------- | ---------------------------------------- |
| **Checkmark** (✓) | Single polyline stroke: bottom-left → center-bottom → top-right | `#00C48F` (Confident Green) | Terminal success output, Act III feature confirmations |
| **Cross** (✗)     | Two diagonal lines forming an X                          | `#FF3B3B` (Warning Red)        | Terminal error output, Act II conflict markers |
| **Lock**          | Padlock: rectangular body + shackle arch                 | Context-dependent (see below)  | Security references                       |
|                   |                                                          | `#FF3B3B` — when showing Git's weak security model | Act II, Problem 5 |
|                   |                                                          | `#00D4FF` — when showing W0rkTree's built-in signing | Act III |
| **Branch**        | Vertical line with a fork splitting into two paths        | `#00D4FF` (Accent Cyan)        | Branching model discussions               |
| **Snapshot**      | Camera/aperture shape: circle with inner partial circle   | `#00D4FF` (Accent Cyan)        | Staged snapshot concept, Act III          |
| **Sync**          | Two curved arrows forming a circle (refresh shape)        | `#00D4FF` (Accent Cyan)        | Sync/push/pull operations                 |
| **Shield**        | Classic shield outline (pointed bottom)                   | `#00C48F` (Confident Green)    | Data integrity and protection references  |

### 6.3 Icon Usage Rules

- Icons are **always** used at their grid size (48 × 48 at 4K). Never scale an icon to an arbitrary size.
- Icons in terminal output are rendered as **text characters** (✓ and ✗), not SVG icons, using JetBrains Mono.
- Icons in diagrams and cards are rendered as **SVG paths** at the icon grid size.
- Icons must have a minimum **16 px clearance** from adjacent text or other icons.
- Icon color must come from the semantic color table (§ 1.2). No exceptions.
- Icons are never filled. They are always **stroke-only** (line style).

---

## 7. Do-Not List

The following visual patterns are **explicitly banned** from the entire video.
This list exists to prevent common mistakes and maintain visual coherence.

| #  | Rule                                                                                 | Reason                                                        |
| -- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------- |
| 1  | ❌ **No gradients on text**                                                          | Gradient text is illegible on dark backgrounds and looks dated |
| 2  | ❌ **No drop shadows**                                                               | Too skeuomorphic for this flat, code-forward aesthetic         |
| 3  | ❌ **No rounded bubble / pill shapes** (except buttons in the dashboard mockup)       | Rounded pills clash with the rectangular, grid-based system   |
| 4  | ❌ **No emojis in motion graphics** (terminal output may use ✓ and ✗ only)            | Emojis break the controlled, professional tone                |
| 5  | ❌ **No white backgrounds — EVER**                                                   | Deep Navy (`#0A0F1A`) or Code Background (`#111827`) only     |
| 6  | ❌ **No stock photography, stock illustrations, or clip art**                         | Everything is custom motion graphics or typography             |
| 7  | ❌ **No 3D effects, no perspective transforms, no parallax**                          | Flat 2D compositions only — depth via layering and opacity    |
| 8  | ❌ **No particle effects or confetti**                                                | Overused in tech videos; undercuts the serious tone           |
| 9  | ❌ **No text on top of busy visuals without a scrim / overlay**                       | All text must be on solid or near-solid backgrounds           |
| 10 | ❌ **No color outside the defined palette** (sole exception: Carol's `#FF8A3B` avatar) | Every pixel on screen must trace back to § 1.1                |
| 11 | ❌ **No fonts outside the defined type system**                                       | Inter, Inter Tight, JetBrains Mono — nothing else             |
| 12 | ❌ **No linear easing** (sole exception: continuous dash-offset loops)                 | Linear motion looks robotic and unfinished                    |
| 13 | ❌ **No opacity-only animations** (sole exception: logo breathing glow)                | Opacity changes must pair with translateY (see § 4.3.4)       |
| 14 | ❌ **No inconsistent entry directions within a scene**                                 | All elements in a scene enter from the same side (see § 4.3.3)|
| 15 | ❌ **No window chrome on terminals** (no title bars, no traffic-light dots)            | Clean, minimal terminal panel — content only                  |
| 16 | ❌ **No underlined text**                                                              | Underlines are reserved for hyperlinks in interactive UI only |
| 17 | ❌ **No italic text**                                                                  | Inter's italic is acceptable but not used in this video's style|
| 18 | ❌ **No text smaller than `caption` (24 px at 4K / 12 px at HD)**                     | Below this size, text is illegible on YouTube compression     |
| 19 | ❌ **No pure black (`#000000`) backgrounds**                                           | Deep Navy provides more visual depth than true black          |
| 20 | ❌ **No animation durations outside the defined scale** (§ 4.2)                        | Use the token system — no arbitrary millisecond values        |

---

## Appendix A: Quick-Reference Cheat Sheet

For animators and designers who need a one-page summary.

### Colors (copy-paste ready)

```
--deep-navy:        #0A0F1A;
--pure-white:       #F0F0F0;
--accent-cyan:      #00D4FF;
--warning-red:      #FF3B3B;
--confident-green:  #00C48F;
--muted-gray:       #6B7280;
--dim-gray:         #374151;
--code-bg:          #111827;
```

### Fonts (copy-paste ready)

```
--font-mono:    "JetBrains Mono", "Fira Code", "Cascadia Code", monospace;
--font-sans:    "Inter", "Geist", "SF Pro", system-ui;
--font-display: "Inter Tight", "Inter Display", "Inter", system-ui;
```

### Easing (copy-paste ready)

```
--ease-standard:   cubic-bezier(0.40, 0.00, 0.20, 1.00);
--ease-enter:      cubic-bezier(0.00, 0.00, 0.20, 1.00);
--ease-exit:       cubic-bezier(0.40, 0.00, 1.00, 1.00);
--ease-bounce:     cubic-bezier(0.34, 1.56, 0.64, 1.00);
--ease-decelerate: cubic-bezier(0.00, 0.00, 0.00, 1.00);
--ease-sharp:      cubic-bezier(0.40, 0.00, 0.60, 1.00);
```

### Durations (copy-paste ready)

```
--instant:        100ms;
--fast:           200ms;
--standard:       400ms;
--slow:           600ms;
--dramatic:       1000ms;
--type-char:      40ms;
--type-char-slow: 80ms;
```

---

## Appendix B: File & Asset Checklist

Ensure the following assets are prepared before production begins:

- [ ] **JetBrains Mono** font files (Regular 400, Bold 700) — `.woff2` or `.otf`
- [ ] **Inter** variable font file — `.woff2` or `.otf`
- [ ] **Inter Tight** variable font file — `.woff2` or `.otf`
- [ ] **W0rkTree logo** — SVG with the "0" glow layer separated
- [ ] **Icon set** — 7 SVG icons at 48 × 48 px (Checkmark, Cross, Lock, Branch, Snapshot, Sync, Shield)
- [ ] **SFX: percussive hit** — `.wav`, ≤ 500 ms, for Act II number reveals
- [ ] **Background music track** — licensed, mixed for voice-over (ducking on narration)
- [ ] **Noise texture** — 3840 × 2160 grayscale noise at 5 % opacity, tiling `.png`

---

*End of Design System — Document version 1.0.0*