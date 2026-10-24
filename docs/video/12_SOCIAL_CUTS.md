# Social Platform Cut Specifications

This document specifies how the full 5:00 YouTube video is re-edited into shorter platform-specific cuts. Each cut is purpose-built for its platform's audience behavior, aspect ratio, and algorithmic preferences — not a lazy crop of the master.

---

## 1. PLATFORM OVERVIEW

| Platform | Format | Aspect | Resolution | Duration | FPS | Tone | Primary Goal |
|---|---|---|---|---|---|---|---|
| Twitter/X | Horizontal | 16:9 | 1920×1080 | 60s | 60 | Hook + reveal. Fast, punchy. | Drive clicks to full video |
| LinkedIn | Horizontal | 16:9 | 1920×1080 | 90s | 60 | Professional. Team-focused. | Establish credibility, drive signups |
| TikTok / IG Reels | Vertical | 9:16 | 1080×1920 | 60s | 60 | Bold, visual-first. | Virality, brand awareness |
| Instagram Feed | Square | 1:1 | 1080×1080 | 60s | 60 | Clean, designed. | Engagement, saves, shares |
| YouTube Shorts | Vertical | 9:16 | 1080×1920 | 55s | 60 | Same as TikTok but < 60s | Shorts shelf discovery |

### Encoding Targets (All Platforms)

| Parameter | Value |
|---|---|
| Codec | H.264 High Profile (H.265 for TikTok if supported) |
| Bitrate (1080p) | 12–16 Mbps VBR |
| Bitrate (4K, YouTube only) | 35–45 Mbps VBR |
| Audio Codec | AAC-LC |
| Audio Bitrate | 320 kbps stereo |
| Audio Loudness | −14 LUFS integrated, −1 dBTP true peak |
| Color Space | Rec. 709, sRGB |
| Container | MP4 |

---

## 2. TWITTER/X CUT (60 seconds, 16:9)

### 2.1 Metadata

| Field | Value |
|---|---|
| **Title** | 5 reasons Git is broken (and what replaces it) |
| **Duration** | 60 seconds exactly |
| **Aspect Ratio** | 16:9 (1920×1080) |
| **Captions** | Burned-in (mandatory) + SRT upload |
| **CTA** | "Full video link in bio" or "Full breakdown ↗ [link]" |
| **Hashtags** | #git #devtools #opensource #worktree #versioncontrol |

### 2.2 Edit Structure

```
TIMECODE    SOURCE              CONTENT
─────────────────────────────────────────────────────────────────────
0:00–0:05   Cold Open excerpt   git reset --hard → "initial commit"
                                (HOOK — no narration, text only, SFX only)

0:05–0:08   Title Card          W0rkTree logo + tagline
                                (compressed: 3s instead of 5s)

0:08–0:14   Act II P1           Number Card "1" + jargon cloud (single shot)
0:14–0:20   Act II P2           Number Card "2" + split-screen CONFLICT
0:20–0:26   Act II P3           Number Card "3" + DAG dissolution (single event)
0:26–0:32   Act II P4           Number Card "4" + vault door + files
0:32–0:38   Act II P5           Number Card "5" + tower fall

0:38–0:42   Bridge              "What if we stopped building workarounds?"

0:42–0:47   Act III excerpt     Architecture diagram build (compressed to 5s)
0:47–0:52   Act III excerpt     "STAGED SNAPSHOT VISIBILITY" + dashboard shot

0:52–0:57   Act IV excerpt      wt status --team terminal shot ONLY (hero shot)

0:57–1:00   Close               Logo + URL (w0rktree.dev)
─────────────────────────────────────────────────────────────────────
```

### 2.3 Adaptation Notes

- **Narrator pace**: Increases to ~180 WPM for the compressed Act II. Each problem gets exactly one sentence of narration plus the number card.
- **Problem supporting visuals**: Single shots only — no multi-shot sequences. Pick the most visually striking moment from each problem.
- **Act III compression**: Brutally compressed to the two most visually interesting moments: (1) architecture diagram building itself, (2) team dashboard with rows staggering in.
- **Terminal shot**: Only ONE terminal shot — `wt status --team`. This is the hero shot because it shows something impossible in Git. No other terminal shots.
- **Music**: Same track as master, but crossfade to the high-energy section earlier (align with Act II start). Fade out cleanly at 0:57.
- **Sound design**: Keep all impact SFX (number card slams, DAG dissolution, CONFLICT explosion). These sell the pace.
- **First frame**: Must work as auto-generated thumbnail — the Cold Open terminal with `git reset --hard` visible.

### 2.4 Caption Specifications

- **Style**: Burned-in (do NOT rely on Twitter's auto-captions — they are unreliable)
- **Font**: Inter 600
- **Size**: 48px at 1080p
- **Color**: #F0F0F0
- **Background**: #0A0F1A at 80% opacity, pill-shaped (8px padding, 6px border-radius)
- **Position**: Bottom 15% of frame
- **Max lines**: 2
- **Duration per block**: 1.5–3 seconds, aligned to natural phrase breaks

---

## 3. LINKEDIN CUT (90 seconds, 16:9)

### 3.1 Metadata

| Field | Value |
|---|---|
| **Title** | We're building a Git replacement. Here's why. |
| **Duration** | 90 seconds exactly |
| **Aspect Ratio** | 16:9 (1920×1080) |
| **Captions** | Burned-in + SRT upload |
| **CTA** | "Learn more: w0rktree.dev" |
| **Post Copy** | "What does Git look like rebuilt from zero? We've spent [X] months finding out." |

### 3.2 Edit Structure

```
TIMECODE    SOURCE              CONTENT
─────────────────────────────────────────────────────────────────────
0:00–0:05   Cold Open excerpt   Terminal only (git push error + conflict)
                                No live action — keep it clean and professional

0:05–0:10   Title Card          W0rkTree logo + tagline (full 5s)

0:10–0:15   Act I compressed    Timeline: "2005" node + illustrated figure
0:15–0:20   Act I compressed    "Git was built for the Linux kernel."
0:20–0:25   Act I compressed    Modern tool icons + "It's not 2005 anymore."

0:25–0:31   Act II P1           Number Card "1" + jargon cloud
0:31–0:37   Act II P2           Number Card "2" + split-screen CONFLICT
0:37–0:43   Act II P3           Number Card "3" + DAG dissolution
0:43–0:49   Act II P4           Number Card "4" + vault + git:// text
0:49–0:55   Act II P5           Number Card "5" + tower fall

0:55–1:00   Bridge              "What if we stopped building workarounds?"

1:00–1:08   Act III excerpt     Architecture diagram (LOCAL + connection + REMOTE)
1:08–1:14   Act III excerpt     "STAGED SNAPSHOT VISIBILITY" text
1:14–1:20   Act III excerpt     Team dashboard mockup (MONEY SHOT — hold 6s)

1:20–1:25   Act IV excerpt      wt status --team (hero terminal shot)

1:25–1:30   Close               Logo + URL + "Learn more: w0rktree.dev"
─────────────────────────────────────────────────────────────────────
```

### 3.3 Adaptation Notes

- **Slightly more room to breathe** than the Twitter cut. Act I gets a compressed 15-second version (instead of 60s in the master). Act III gets 20 seconds — enough for architecture diagram build + dashboard.
- **Tone**: Same narrator, slightly more measured pace (~160 WPM). LinkedIn audiences expect substance, not just flash.
- **Act I compression**: Three shots that tell the whole story: (1) 2005 node + figure, (2) "built for the Linux kernel" context, (3) modern tools + "not 2005 anymore." This replaces the full 60-second history lesson.
- **Act II**: Same structure as Twitter (6s per problem), but narrator can breathe slightly more with the extra overall time.
- **Act III**: 20 seconds allows the architecture diagram to build properly (8s), the visibility concept to land (6s), and the dashboard to be the money shot (6s hold). The dashboard is the most LinkedIn-relevant visual — team collaboration is the value prop for engineering managers.
- **Professional framing**: No memes, no TikTok energy. The number card slams should feel authoritative, not aggressive.
- **Post copy**: The LinkedIn post itself should frame this as thought leadership. Suggested copy: "What does Git look like rebuilt from zero? We've spent [X] months finding out. Here's what we learned about what version control should be in 2025."
- **Music**: Same track, but keep the energy level moderate throughout. No sudden spikes.

### 3.4 Caption Specifications

Same as Twitter/X cut (see §2.4). LinkedIn's native player supports SRT upload — use it as a fallback, but burned-in captions are still primary because many LinkedIn users browse in feed with sound off.

---

## 4. TIKTOK / IG REELS CUT (60 seconds, 9:16 vertical)

### 4.1 Metadata

| Field | Value |
|---|---|
| **Title** | Git is broken 💀 |
| **Alt Title** | 5 things wrong with Git that nobody talks about |
| **Duration** | 60 seconds exactly |
| **Aspect Ratio** | 9:16 (1080×1920) |
| **Captions** | Burned-in (MANDATORY — most viewers watch without sound) |
| **CTA** | "Link in bio" |
| **Hashtags** | #git #coding #developer #devtok #programming #tech #softwareengineering |

### 4.2 Edit Structure

```
TIMECODE    SOURCE              CONTENT
─────────────────────────────────────────────────────────────────────
0:00–0:03   HOOK                Terminal: git reset --hard → "initial commit"
                                Text fills vertical frame. SFX: glitch + impact.
                                THIS IS THE MOST IMPORTANT 3 SECONDS.

0:03–0:05   Text card           "Git has been broken for 20 years. Here's why."
                                Bold, vertical-centered, Inter Tight 900

0:05–0:11   Act II P1           Number Card "1" (VERTICAL) + jargon cloud
0:11–0:17   Act II P2           Number Card "2" (VERTICAL) + CONFLICT explosion
0:17–0:23   Act II P3           Number Card "3" (VERTICAL) + DAG dissolution
0:23–0:29   Act II P4           Number Card "4" (VERTICAL) + vault
0:29–0:35   Act II P5           Number Card "5" (VERTICAL) + tower fall

0:35–0:38   Bridge              "What if we stopped building workarounds?"

0:38–0:45   Act III excerpt     Vertical architecture diagram build
0:45–0:52   Act III excerpt     Vertical dashboard (card stack)

0:52–0:57   Act IV excerpt      Terminal shot (vertical crop, larger font)

0:57–1:00   Close               Logo + "Link in bio"
─────────────────────────────────────────────────────────────────────
```

### 4.3 9:16 Recomposition Notes

**CRITICAL: None of these visuals can be cropped from 16:9. They must be REDESIGNED for vertical.**

#### Architecture Diagram (Vertical)
- Stack LOCAL container on top, REMOTE container on bottom
- Connection line runs vertically between them (not horizontal)
- QUIC label centered on the vertical connection line
- Each container is full-width (1080px minus 48px padding each side = 984px usable)
- Node text scales up to remain readable at mobile resolution

#### Terminal Shots (Vertical)
- Increase font size to `code-lg` equivalent (~20px at 1080 width)
- Reduce terminal width to fit vertical frame with comfortable padding (48px each side)
- Stack output vertically if lines are too long — break at natural boundaries
- Terminal background fills the vertical frame edge-to-edge (no floating terminal window)

#### Number Cards (Vertical)
- The number fills the top 40% of the vertical frame (massive, ~400px tall)
- The subtitle text fills the bottom 40%
- Middle 20% is breathing room
- This creates a massive visual impact at scroll speed — the number must be readable in 0.2 seconds

#### Team Dashboard (Vertical)
- Redesign as a vertical card stack — one person per card, full width
- Each card: avatar, name, current file, snapshot status
- Cards stagger in from bottom, 200ms apart
- This is more natural for mobile viewing than the horizontal table layout

#### Feature Cards (Vertical)
- One card at a time, full width
- Swipe/scroll transition between cards (vertical slide)
- Each card gets 2 seconds minimum

#### Captions (Vertical)
- Position: centered horizontally, bottom 20% of frame
- Size: 56px at 1080 width (larger than 16:9 to compensate for mobile viewing distance)
- Background pill: #0A0F1A at 85% opacity (slightly more opaque for busy backgrounds)
- MANDATORY — this is non-negotiable for TikTok/Reels

### 4.4 Hook Optimization

The first 3 seconds determine whether a viewer swipes away. The hook must:

1. **Show code immediately** — developer audience recognizes terminal output instantly
2. **Create dread** — `git reset --hard` followed by "initial commit" is universally recognized as catastrophic
3. **Use motion** — the text must animate in (typing effect), not just appear
4. **Use sound** — glitch + bass impact SFX, even though most viewers have sound off (the ones who do have sound on will be hooked harder)
5. **Fill the frame** — no empty space. The terminal text should be large enough to read at a glance on a phone screen

### 4.5 Music & Sound

- Use a different music edit for vertical — TikTok/Reels audiences expect faster cutting and more bass
- Bass-heavy impacts on every number card slam
- Subtle whoosh transitions between problems
- Music should feel current (2024–2025 production style), not corporate
- If the main track doesn't work for vertical pacing, license a separate 60-second track for social cuts

---

## 5. INSTAGRAM FEED / SQUARE CUT (60 seconds, 1:1)

### 5.1 Metadata

| Field | Value |
|---|---|
| **Title** | Git is broken. We built the fix. |
| **Duration** | 60 seconds exactly |
| **Aspect Ratio** | 1:1 (1080×1080) |
| **Captions** | Burned-in |
| **CTA** | "Link in bio" |

### 5.2 Edit Structure

Same as Twitter/X cut (§2.2) but recomposed for 1:1 aspect ratio. The timing and shot selection are identical — only the visual layout changes.

```
TIMECODE    SOURCE              CONTENT
─────────────────────────────────────────────────────────────────────
0:00–0:05   Cold Open excerpt   git reset --hard → "initial commit" (1:1 framed)
0:05–0:08   Title Card          W0rkTree logo + tagline (1:1 framed)
0:08–0:38   Act II              Five Problems (6s each, 1:1 recomposed)
0:38–0:42   Bridge              "What if we stopped building workarounds?"
0:42–0:52   Act III excerpt     Architecture diagram + dashboard (1:1 recomposed)
0:52–0:57   Act IV excerpt      wt status --team (1:1 recomposed)
0:57–1:00   Close               Logo + URL
─────────────────────────────────────────────────────────────────────
```

### 5.3 1:1 Recomposition Notes

#### Terminal
- Reduce terminal width to 70% of canvas (756px of 1080px)
- Center horizontally and vertically
- Increase horizontal padding to 24px inside the terminal
- Font size: same as 16:9 (no increase needed — square is wider than vertical)

#### Architecture Diagram
- Compress horizontally — the full 16:9 diagram won't fit
- Simplify: remove sub-items from nodes, keep primary labels only
- LOCAL on left, REMOTE on right, connection centered (same as 16:9 but tighter)
- If it still doesn't fit, stack vertically (same as 9:16 layout)

#### Number Cards
- Number and subtitle stacked vertically, both large
- Number centered in top half, subtitle centered in bottom half
- Comfortable vertical padding (64px from edges)

#### Text Sizes
- All text sizes increase by ~20% relative to 16:9 compositions
- This compensates for the smaller effective viewport on a square video
- Body text: minimum 24px at 1080×1080
- Headings: minimum 48px at 1080×1080

#### Caption Positioning
- Positioned below center (lower 25% of frame)
- Avoid Instagram UI overlap at top (username, follow button) and bottom (like/comment/share icons)
- Safe area for captions: between 65%–85% from top of frame
- Test on iOS and Android before finalizing position

### 5.4 Visual Density

Square format has less horizontal space than 16:9 but more vertical space than a crop would suggest. Key principles:
- **Center everything** — square compositions look best with centered subjects
- **Increase padding** — elements need more breathing room in square
- **Simplify** — if a shot has more than 3 visual elements, reduce to 2
- **Bold colors** — the smaller canvas means colors need to pop harder; increase saturation by 5–10% on accent colors (#00D4FF, #FF3B3B)

---

## 6. YOUTUBE SHORTS CUT (55 seconds, 9:16)

### 6.1 Metadata

| Field | Value |
|---|---|
| **Title** | Git has been broken for 20 years #shorts #git #coding |
| **Duration** | 55 seconds (MUST be under 60s for Shorts eligibility) |
| **Aspect Ratio** | 9:16 (1080×1920) |
| **Captions** | Burned-in |

### 6.2 Edit Structure

Same as TikTok cut (§4.2) but trimmed by 5 seconds. The 5 seconds are removed from Act II by compressing Problem 4 ("The Security Vacuum") from 6 seconds to 1 second (just the number card slam, no supporting visual).

```
TIMECODE    SOURCE              CONTENT
─────────────────────────────────────────────────────────────────────
0:00–0:03   HOOK                Terminal: git reset --hard → "initial commit"
0:03–0:05   Text card           "Git has been broken for 20 years. Here's why."

0:05–0:11   Act II P1           Number Card "1" + jargon cloud (6s)
0:11–0:17   Act II P2           Number Card "2" + CONFLICT explosion (6s)
0:17–0:23   Act II P3           Number Card "3" + DAG dissolution (6s)
0:23–0:24   Act II P4           Number Card "4" ONLY (1s — slam, no visual)
0:24–0:30   Act II P5           Number Card "5" + tower fall (6s)

0:30–0:33   Bridge              "What if we stopped building workarounds?"

0:33–0:40   Act III excerpt     Vertical architecture diagram build
0:40–0:47   Act III excerpt     Vertical dashboard (card stack)

0:47–0:52   Act IV excerpt      Terminal shot (vertical crop, larger font)

0:52–0:55   Close               Logo + "Link in bio"
─────────────────────────────────────────────────────────────────────
```

### 6.3 Why Problem 4 Gets Compressed

Problem 4 ("The Security Vacuum") is the easiest to compress because:
1. Its supporting visuals (vault door, platform badges, git:// text) are less immediately visually striking than the other problems
2. The concept (security) is understood from the number card title alone — "THE SECURITY VACUUM" communicates without needing supporting imagery
3. Problems 1, 2, 3, and 5 have more dynamic visuals (jargon cloud, CONFLICT explosion, DAG dissolution, tower collapse) that benefit from the full 6 seconds
4. The 1-second slam of "4 — THE SECURITY VACUUM" actually creates a nice rhythmic break — it hits fast and moves on, which can feel intentional and punchy

### 6.4 All Other Specs

All other specifications (recomposition, captions, hook, music) are identical to the TikTok/IG Reels cut (§4). The only difference is total duration and the Problem 4 compression.

---

## 7. THUMBNAIL SPECIFICATIONS

### 7.1 YouTube Thumbnail (1280×720)

| Element | Specification |
|---|---|
| **Canvas** | 1280×720px, sRGB |
| **Background** | Deep Navy (#0A0F1A), solid fill |
| **File Format** | PNG, < 2MB |

#### Layout

```
┌─────────────────────────────────────────────────────┐
│  "GIT IS BROKEN"                                    │
│  Inter Tight 900, #FF3B3B                           │
│  Top-left area, large                               │
│                                                     │
│  "we built the fix"                                 │
│  Inter 600, #F0F0F0, smaller                        │
│  Below main text                                    │
│                                                     │
│  ┌──────────────┐    ╱╲    ┌──────────────────┐     │
│  │ Terminal      │   ╱  ╲   │  W0rkTree Logo  │     │
│  │ git reset     │  ╱ VS ╲  │  + cyan glow    │     │
│  │ --hard        │  ╲    ╱  │                  │     │
│  │ (#FF3B3B)     │   ╲  ╱   │  (#00D4FF glow) │     │
│  │ + cursor blink│    ╲╱    │                  │     │
│  └──────────────┘          └──────────────────┘     │
│                                          ┌────────┐ │
│                                          │  5:00  │ │
│                                          └────────┘ │
└─────────────────────────────────────────────────────┘
```

#### Element Details

| Element | Details |
|---|---|
| **Left side** | Terminal showing `git reset --hard` in #FF3B3B with block cursor (#00D4FF) — the "oh no" moment |
| **Right side** | W0rkTree logo (MG-001) with cyan glow layer (MG-002) |
| **Center divider** | Jagged "crack" or "VS" element — communicates conflict/comparison |
| **Main text** | "GIT IS BROKEN" — Inter Tight 900, #FF3B3B, positioned top-left |
| **Sub-text** | "we built the fix" — Inter 600, #F0F0F0, positioned below main text |
| **Runtime badge** | "5:00" — Inter 500, #F0F0F0, on #374151 pill, bottom-right corner |

#### Readability Test

**IMPORTANT**: The thumbnail must be readable at 160×90px — the smallest size YouTube renders it. At this size:
- "GIT IS BROKEN" must be legible
- The terminal and logo must be distinguishable shapes (not blurry blobs)
- The red/cyan color contrast must be visible

**Testing procedure**: Export at 1280×720, then scale down to 160×90 in an image editor. If the text is unreadable or the composition is unclear, simplify until it works at thumbnail size.

### 7.2 Social Thumbnails

| Platform | Size | Source | Notes |
|---|---|---|---|
| **Twitter/X** | Auto-generated | First frame of video | Ensure the Cold Open terminal frame works as a static thumbnail. The `git reset --hard` text must be visible and striking. |
| **LinkedIn** | 1200×627px | Custom card image | Deep Navy background. W0rkTree logo centered. Text: "We're building a Git replacement." in Inter 600, #F0F0F0. Subtitle: "Here's why." in #00D4FF. Clean, professional, no terminal imagery. |
| **TikTok** | 1080×1920px | Cover frame at 0:03 | The number card "1" frame — bold, massive number, eye-catching at small sizes. TikTok shows this as the cover in the creator's profile grid. |
| **Instagram Reels** | 1080×1920px | Cover frame at 0:03 | Same as TikTok. Can optionally add a custom cover with "Git is broken 💀" text overlay if the platform allows. |
| **YouTube Shorts** | 1080×1920px | Cover frame at 0:03 | Same as TikTok. YouTube Shorts auto-selects a frame but allows manual override — use the number card "1" frame. |
| **Instagram Feed** | 1080×1080px | First frame | Ensure the 1:1 Cold Open terminal frame works as a cover. Instagram Feed shows this as a still in the grid. |

---

## 8. CAPTION & SUBTITLE SPECS

### 8.1 Required Deliverables Per Cut

| Cut | SRT | VTT | Burned-In |
|---|---|---|---|
| YouTube (Full 5:00) | ✅ | ✅ | ❌ (optional) |
| Twitter/X (60s) | ❌ | ❌ | ✅ (mandatory) |
| LinkedIn (90s) | ✅ | ✅ | ✅ (mandatory) |
| TikTok (60s) | ❌ | ❌ | ✅ (mandatory) |
| IG Reels (60s) | ❌ | ❌ | ✅ (mandatory) |
| YouTube Shorts (55s) | ❌ | ❌ | ✅ (mandatory) |
| Instagram Feed (60s) | ❌ | ❌ | ✅ (mandatory) |
| Web Embed | ❌ | ✅ | ❌ |

### 8.2 SRT Format (SubRip)

For YouTube and LinkedIn uploads. Standard SRT format:

```
1
00:00:01,000 --> 00:00:03,500
Git push origin main.

2
00:00:04,000 --> 00:00:06,500
Error. Rejected. Non-fast-forward.

3
00:00:07,000 --> 00:00:09,500
Sound familiar?
```

**Requirements**:
- Sequential numbering starting at 1
- Timecodes in `HH:MM:SS,mmm` format (comma separator, not period)
- Each subtitle block: 1.5–3 seconds duration
- Maximum 2 lines per block
- Maximum 42 characters per line (standard broadcast limit)
- Aligned to natural phrase breaks — never split a word or break mid-thought
- UTF-8 encoding, no BOM

### 8.3 VTT Format (WebVTT)

For web embeds on w0rktree.dev and documentation sites. Standard WebVTT format:

```
WEBVTT

00:00:01.000 --> 00:00:03.500
Git push origin main.

00:00:04.000 --> 00:00:06.500
Error. Rejected. Non-fast-forward.

00:00:07.000 --> 00:00:09.500
Sound familiar?
```

**Requirements**:
- Must begin with `WEBVTT` header
- Timecodes in `HH:MM:SS.mmm` format (period separator, not comma)
- No sequential numbering required (but optional)
- Same timing, line count, and character limits as SRT
- UTF-8 encoding

### 8.4 Burned-In Caption Styling

For all social platform cuts where captions are composited directly into the video frames.

| Property | Value | Notes |
|---|---|---|
| **Font** | Inter 600 | Same font family as the video's design system |
| **Size (1080p, 16:9)** | 48px | Scale proportionally for other resolutions |
| **Size (1080p, 9:16)** | 56px | Larger for mobile viewing distance |
| **Size (1080p, 1:1)** | 48px | Same as 16:9 |
| **Color** | #F0F0F0 | Pure white from the design system |
| **Background** | #0A0F1A at 80% opacity | Deep Navy, pill-shaped |
| **Background Padding** | 8px horizontal, 6px vertical | Creates the pill shape |
| **Background Radius** | 6px | Subtle rounding |
| **Position (16:9)** | Bottom 15% of frame | Safe from platform UI overlays |
| **Position (9:16)** | Bottom 20% of frame | Higher to avoid TikTok/Reels bottom bar |
| **Position (1:1)** | 65%–85% from top | Between Instagram's top and bottom UI |
| **Max Lines** | 2 | Never more than 2 lines on screen |
| **Line Spacing** | 1.4× font size | Comfortable reading |
| **Duration Per Block** | 1.5–3 seconds | Aligned to natural phrase breaks |
| **Transition** | Cut (no fade) | Clean, immediate. Fading captions feel sluggish. |
| **Drop Shadow** | 2px offset, #000000 at 40% | Subtle, for readability over bright backgrounds |

### 8.5 Caption Timing Guidelines

- **Start time**: 100ms before the narrator begins the phrase (primes the reader)
- **End time**: 200ms after the narrator finishes the phrase (allows reading completion)
- **Minimum gap between blocks**: 80ms (prevents flashing)
- **Sync with visuals**: If a visual impact (number card slam, explosion, etc.) occurs during narration, break the caption block at the impact point — don't have a caption straddling a major visual transition
- **Terminal-only sections**: When there is no narration (e.g., Cold Open typing), display the terminal text as the caption — e.g., "git push origin main" as the typing animation plays. This reinforces the content for sound-off viewers.

### 8.6 Language & Localization

Initial release: English only. For future localization:
- SRT/VTT files are the translation source — burned-in captions are rendered from these
- Target languages (priority order): Spanish, Portuguese, Japanese, Korean, Mandarin Chinese, German, French
- Caption timing may need adjustment for languages with longer average word lengths (e.g., German)
- Right-to-left languages (Arabic, Hebrew) require caption position mirroring and RTL text rendering
- All localized caption files should be named with ISO 639-1 codes: `captions_en.srt`, `captions_es.srt`, etc.

---

## 9. CROSS-PLATFORM CONSISTENCY CHECKLIST

Before delivering any social cut, verify:

- [ ] **Logo**: W0rkTree logo appears identically in every cut (same size relative to frame, same glow treatment)
- [ ] **Colors**: All brand colors match the master — no color shift from re-encoding. Spot-check #00D4FF and #FF3B3B on a calibrated display.
- [ ] **Typography**: All text uses the correct fonts from the design system. No system font fallbacks.
- [ ] **Audio levels**: −14 LUFS integrated, −1 dBTP true peak on every cut. Re-master audio for each cut (don't just trim the master audio — the shorter duration changes the integrated loudness).
- [ ] **Captions**: Burned-in captions present and readable on every social cut. Test on a phone screen at arm's length.
- [ ] **First frame**: Works as a static thumbnail for every platform.
- [ ] **Last frame**: CTA is visible for at least 3 seconds. URL or "link in bio" is legible.
- [ ] **Safe areas**: No critical text or visuals in platform-specific unsafe zones (TikTok bottom bar, Instagram top icons, YouTube Shorts subscribe button area).
- [ ] **Aspect ratio**: No black bars, no unintended cropping. Each cut is natively composed for its aspect ratio.
- [ ] **Duration**: Twitter ≤ 60s, LinkedIn ≤ 90s, TikTok ≤ 60s, Shorts < 60s (55s target), Instagram Feed ≤ 60s. Verify to the frame.
- [ ] **File size**: Under platform upload limits (Twitter: 512MB, LinkedIn: 5GB, TikTok: 287MB for 60s, Instagram: 250MB for 60s). Target well under limits for faster upload and processing.

---

## 10. DELIVERY FILE NAMING CONVENTION

All social cuts follow this naming pattern:

```
W0rkTree_Launch_{Platform}_{Aspect}_{Duration}s_{Version}.{ext}
```

### Deliverable File List

| File Name | Description |
|---|---|
| `W0rkTree_Launch_YouTube_16x9_300s_v1.mp4` | Full master video |
| `W0rkTree_Launch_YouTube_16x9_300s_v1.srt` | YouTube SRT captions |
| `W0rkTree_Launch_YouTube_16x9_300s_v1.vtt` | YouTube VTT captions |
| `W0rkTree_Launch_Twitter_16x9_60s_v1.mp4` | Twitter/X cut |
| `W0rkTree_Launch_LinkedIn_16x9_90s_v1.mp4` | LinkedIn cut |
| `W0rkTree_Launch_LinkedIn_16x9_90s_v1.srt` | LinkedIn SRT captions |
| `W0rkTree_Launch_TikTok_9x16_60s_v1.mp4` | TikTok / IG Reels cut |
| `W0rkTree_Launch_Shorts_9x16_55s_v1.mp4` | YouTube Shorts cut |
| `W0rkTree_Launch_IGFeed_1x1_60s_v1.mp4` | Instagram Feed cut |
| `W0rkTree_Launch_Thumbnail_YT.png` | YouTube thumbnail (1280×720) |
| `W0rkTree_Launch_Thumbnail_LinkedIn.png` | LinkedIn card image (1200×627) |
| `W0rkTree_Launch_Thumbnail_TikTok.png` | TikTok cover frame (1080×1920) |

Version numbers increment with each revision: `v1`, `v2`, `v3`, etc. Final approved versions are additionally tagged `_FINAL` (e.g., `W0rkTree_Launch_Twitter_16x9_60s_v3_FINAL.mp4`).