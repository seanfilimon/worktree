# W0rkTree Launch Video — Production Bible

| Field | Detail |
|---|---|
| **Document** | `00_PRODUCTION_BIBLE.md` |
| **Version** | 1.0 |
| **Status** | PRE-PRODUCTION |
| **Video Title** | *"Git Is Broken. We Built the Fix."* |
| **Runtime** | 5:00 |
| **Resolution** | 3840×2160 (4K master) · 1920×1080 (HD delivery) · 1280×720 (social cuts) |
| **Frame Rate** | 60 fps (motion graphics) · 24 fps (live-action Cold Open) |
| **Aspect Ratio** | 16:9 (primary) · 9:16 (social vertical) · 1:1 (social square) |
| **Color Space** | Rec. 709 / sRGB |
| **Audio** | 48 kHz / 24-bit stereo · −14 LUFS integrated loudness (YouTube standard) |

---

## Table of Contents

1. [Project Overview](#project-overview)
2. [Narrative Arc](#narrative-arc)
3. [Document Map](#document-map)
4. [Master Timeline](#master-timeline)
5. [Editorial Principles](#editorial-principles)
6. [Render & Delivery Specs](#render--delivery-specs)

---

## Project Overview

This document is the single source of truth for the W0rkTree launch video — a five-minute, narrative-driven product film that introduces W0rkTree as the ground-up replacement for Git. The video opens with a visceral, universally recognizable moment of Git failure to establish immediate empathy, then traces Git's origin as a quick-fix born inside the Linux kernel in 2005, surfaces the five structural failures that twenty years of workarounds have never resolved, and pivots into a confident reveal of W0rkTree's purpose-built architecture: the worker–server model, staged snapshot visibility, immutable audit history, native access control, and built-in license compliance. A live CLI demo proves the product is real and usable today, and a closing invitation drives viewers to the repository and early-access waitlist. Every creative decision — pacing, color, typography, music, motion — serves a single goal: make the audience feel the pain of Git, then feel the relief of something better, and give them an immediate next step.

---

## Narrative Arc

The video follows a six-beat emotional arc. Each beat maps to one or more timed segments in the Master Timeline below.

### 1. Empathy — Cold Open (0:00–0:25)

> *"We've all been there."*

The audience sees a terminal in the dark. Real Git commands fail in real time — a rejected push, a merge conflict, a destructive `reset --hard`. No narrator yet. The story is told entirely through keystrokes, silence, and the sinking feeling every developer already knows. By the time a developer slowly closes a laptop and a Slack notification pops up, the narrator's first words land on an audience that already agrees.

### 2. Context — Act I (0:30–1:30)

> *"Here's how we got here."*

Respect first, then reframing. We honor Git's origin: Linus needed something fast for the kernel, and he built it in two weeks in April 2005. Illustrated archival footage, a timeline, the kernel file tree growing. Then the tone shifts — the world changed, Git didn't. Modern monorepos, distributed teams across time zones, design files, ML model weights, compliance requirements. The tools moved on. Version control stayed behind.

### 3. Frustration — Act II (1:30–2:45)

> *"Here's what's actually broken."*

Five problems, five visual metaphors, rapid-fire. Each problem gets a numbered title card, a visual concept, and approximately fifteen seconds of screen time:

1. **Complexity** — 168 commands, jargon word cloud.
2. **Merge Blindness** — two developers, one branch, collision.
3. **Destructive Operations** — a file tree dissolving (the "Thanos snap").
4. **No Access Control** — a vault door swinging open, files flowing out.
5. **Workaround Culture** — a Jenga tower of third-party tools, wobbling.

The section ends with a beat of silence and a single question: *"What if we stopped building workarounds… and built the thing right?"*

### 4. Resolution — Act III (2:45–4:15)

> *"Here's what we built."*

The palette shifts. The music opens. W0rkTree is not a wrapper, not a plugin, not a shim — it is a new protocol. The architecture diagram builds piece by piece: the worker (local), the server (remote), staged snapshot visibility between them. Feature cards animate in: native access control, built-in license compliance, immutable audit history. The full system diagram completes and holds.

### 5. Proof — Act IV (4:15–4:45)

> *"Here's it working."*

A real terminal. Real commands. `wt init`, `wt status`, `wt status --team`, `wt snapshot`, `wt push`, `wt access`. No voiceover trickery — the commands run, the output appears, the system works. The narrator returns with three words: *"It just works."*

### 6. Invitation — Close (4:45–5:00)

> *"Join us."*

The logo returns with a breathing glow. The narrator delivers one final line. The URL appears. The music resolves. Silence. Fade to black.

---

## Document Map

The complete production package consists of the following specification documents. Each document is self-contained but references this bible as its parent authority.

| # | File | Description | Status |
|---|---|---|---|
| 00 | `00_PRODUCTION_BIBLE.md` | Master index, narrative arc, timeline, editorial rules, delivery specs — **this document** | ✅ Complete |
| 01 | `01_DESIGN_SYSTEM.md` | Visual identity: color palette (hex/RGB/HSL), typography (families, weights, sizes), spacing scale, grid system, component styles, dark-mode rules, accessibility contrast ratios | 🔲 Draft |
| 02 | `02_SCENE_COLD_OPEN.md` | Scene 1 — *The Git Horror* (0:00–0:25): shot list, terminal script, camera direction, foley notes, lighting brief | 🔲 Draft |
| 03 | `03_SCENE_TITLE_CARD.md` | Scene 2 — *Brand Reveal* (0:25–0:30): logo animation keyframes, synth tone spec, timing grid | 🔲 Draft |
| 04 | `04_SCENE_ACT_I_HISTORY.md` | Scene 3 — *Git's Origin Story* (0:30–1:30): illustrated storyboard, archival reference list, narrator script, music cue sheet | 🔲 Draft |
| 05 | `05_SCENE_ACT_II_PROBLEMS.md` | Scene 4 — *Five Structural Failures* (1:30–2:45): per-problem visual concept, motion design brief, narrator script, SFX spotting | 🔲 Draft |
| 06 | `06_SCENE_ACT_III_PRODUCT.md` | Scene 5 — *W0rkTree Reveal* (2:45–4:15): architecture diagram build sequence, feature card specs, narrator script, music cue sheet | 🔲 Draft |
| 07 | `07_SCENE_ACT_IV_DEMO.md` | Scene 6 — *CLI Demo Montage* (4:15–4:45): terminal recording script, exact commands and expected output, keystroke timing, overlay specs | 🔲 Draft |
| 08 | `08_SCENE_CLOSE.md` | Scene 7 — *The Invitation* (4:45–5:00): logo animation, CTA layout, URL display, narrator final line, music resolution, fade-to-black timing | 🔲 Draft |
| 09 | `09_ANIMATION_COMPONENTS.md` | Reusable animation component library: number cards, text impacts, feature cards, diagram nodes, terminal chrome, transition presets, easing curves | 🔲 Draft |
| 10 | `10_AUDIO_SPEC.md` | Full audio specification: music brief (genre, tempo, key, mood per segment), SFX library, foley notes, narrator casting brief, pronunciation guide, VO recording specs | 🔲 Draft |
| 11 | `11_ASSET_MANIFEST.md` | Complete asset list organized by type (illustration, icon, screenshot, font, audio, video) and by scene, with source/license info and file-naming conventions | 🔲 Draft |
| 12 | `12_SOCIAL_CUTS.md` | Platform-specific re-edit specs for Twitter/X (60 s), LinkedIn (90 s), TikTok/Reels (60 s vertical), Reddit (60 s), Instagram square (60 s): timing, crop, text safe zones, CTA variants | 🔲 Draft |

---

## Master Timeline

The table below is the authoritative timing grid for every segment of the video. Each row specifies the timecode range, segment name, primary visual content, audio content (narrator, music, SFX), the target emotional beat, and the transition into/out of the segment.

> **Reading notes:** "V.O." = voice-over narration. "SFX" = sound effect. "MX" = music. Timecodes are `M:SS` format relative to video start.

| Timecode | Segment | Visual Layer | Audio Layer | Emotional Beat | Transition |
|---|---|---|---|---|---|
| 0:00–0:03 | Cold Open | Black screen → faint glow of a monitor. Cursor blinks. | Room tone. Quiet hum of a fan. Single keypress. | Tension | Fade from black (1.5 s) |
| 0:03–0:12 | Cold Open | Terminal fills frame. `git push origin main` → REJECTED. `git pull --rebase` → CONFLICT in 3 files. Developer scrolls, pastes, re-runs. Conflict markers highlighted. | Keystrokes (mechanical keyboard). No music. Brief silence after REJECTED. | Dread | Hard cut between commands |
| 0:12–0:17 | Cold Open | Terminal: `git reset --hard HEAD~3`. Three files vanish from the working tree listing. Cursor blinks alone. | Single keystroke, then 2 seconds of dead silence. | Loss | Hard cut from previous terminal state |
| 0:17–0:22 | Cold Open | Live-action 24 fps: developer slowly closes laptop lid. Reflection disappears. Slack notification slides in on a phone screen beside the laptop: *"hey, did you push yet?"* | Laptop click (foley). Phone buzz. V.O. begins: *"You know this feeling."* | Empathy | Camera push-in, shallow DOF |
| 0:22–0:25 | Cold Open | Close-up of the closed laptop. Screen dims to black. | V.O.: *"Everyone just… accepts it."* Silence. | Resignation | Fade to black (1 s) |
| 0:25–0:30 | Title Card | Black holds 0.5 s. W0rkTree logo fades in center-screen (2 s ease). Tagline fades in below: *"Version control, rebuilt."* | Single sustained synth tone (C3, warm pad). No narration. | Anticipation | Fade in from black (0.5 s); hold logo for 3 s |
| 0:30–0:45 | Act I — Origin | Illustrated scene: 2005, a beige CRT monitor, kernel mailing list, Linus typing furiously. Hand-drawn style animation builds the scene left to right. Date stamp: *"April 2005"*. | V.O.: *"In 2005, Linus Torvalds needed a version control tool for the Linux kernel. So he built one. In two weeks."* MX: documentary underscore, warm strings, low tempo. | Nostalgia | Hard cut from title card |
| 0:45–1:00 | Act I — Growth | Animated timeline grows from 2005 → 2010 → 2015 → 2020 → 2024. Below it, a kernel file tree expands. Contributor count ticks up: 1 → 100 → 10,000 → millions. GitHub logo appears. | V.O.: *"It worked. Brilliantly. Git became the backbone of open source, then the industry, then the world."* MX: strings swell, tempo increases slightly. | Respect | Animated build from previous scene, continuous camera pan right |
| 1:00–1:10 | Act I — Shift | The warm illustration style bleeds into a cooler, modern flat-design aesthetic. Modern dev setups appear: multiple monitors, Slack, Figma, Jupyter notebooks, CI/CD dashboards, Kubernetes clusters. Color grade shifts from warm amber to cool blue-grey. | V.O.: *"But the world kept moving."* MX: tone shifts — strings fade, replaced by a low electronic pulse. | Tension | Color grade shift (1.5 s cross-grade) |
| 1:10–1:15 | Act I — Break | Large typography: **"2005"** in center frame. A crack forms through the year. It shatters. Behind it: **"2024"**. | V.O.: *"It's not 2005 anymore."* MX: single percussive beat on the crack. | Realization | Typography impact animation (0.3 s slam, 0.5 s crack, 0.2 s shatter) |
| 1:15–1:30 | Act I — Pain | Rapid montage: Stack Overflow questions scrolling ("How do I undo a git rebase?", "git merge vs rebase?", "force push deleted my work"). Each question card flies in and stacks. Counter in corner: *12.4 million questions tagged [git]*. | V.O.: *"Twelve million questions on Stack Overflow. Not because developers are bad at Git — because Git is bad at being learnable."* MX: darkening pulse, tempo increases. | Frustration | Rapid scroll animation, cards stacking with slight rotation |
| 1:30–1:45 | Act II — Problem 1 | Black screen. Large **"01"** slams into frame from above. Word cloud explodes outward: `rebase`, `cherry-pick`, `reflog`, `detached HEAD`, `octopus merge`, `worktree`, `bisect`, `stash pop`, `fsck`. Count text: *"168 commands."* | SFX: deep percussive slam on number impact. V.O.: *"One hundred and sixty-eight commands. A jargon vocabulary larger than most programming languages. Git doesn't have a learning curve — it has a cliff."* MX: staccato tension pulse. | Confusion | Impact cut (single-frame flash to white before number card) |
| 1:45–2:00 | Act II — Problem 2 | **"02"** slams in. Split-screen: two developers typing in separate terminals. A countdown timer appears between them: *3… 2… 1…* Both push. The word **"CONFLICT"** explodes across both screens in red. | SFX: slam on "02". Ticking clock during countdown. Alarm tone on CONFLICT. V.O.: *"Two developers. Same branch. No warning. Git doesn't know your teammate exists until it's too late."* MX: tension builds under narration. | Blindness | Split-screen wipe from center outward |
| 2:00–2:15 | Act II — Problem 3 | **"03"** slams in. A detailed file tree (project with dozens of files) is on screen. A cursor highlights files. Then — a ripple effect — files dissolve from the tree one by one, accelerating. The "Thanos snap" — half the tree vanishes in a particle effect. Blank space remains. | SFX: slam on "03". Soft dissolve sounds. Silence after the snap (1.5 s). V.O.: *"One wrong command. History rewritten. Files gone. No undo. Git gives you the power to destroy your own work — and calls it a feature."* | Loss | Dissolve particle effect (snap at 2:08, silence holds to 2:15) |
| 2:15–2:30 | Act II — Problem 4 | **"04"** slams in. A steel vault door rendered in 3D/isometric style. The door swings open. Inside: files, credentials, proprietary code. Files begin flowing out of the vault in a stream, scattering across the screen. A badge appears: *"Clone = Copy Everything. Forever."* | SFX: slam on "04". Heavy vault door creak. Paper scatter SFX. V.O.: *"Clone a repo and you get everything. Every file. Every branch. Every secret anyone ever committed. Git has no access control. None."* MX: low tension drone. | Vulnerability | Hard cut from previous scene; vault door opens with 0.5 s animation |
| 2:30–2:42 | Act II — Problem 5 | **"05"** slams in. Isometric Jenga-style tower. Each block is labeled: *"GitHub Actions"*, *"Husky"*, *"git-lfs"*, *"pre-commit"*, *".gitignore"*, *"git-crypt"*, *"GitGuardian"*, *"Lerna"*. The tower wobbles. Blocks shift. It collapses in slow motion. | SFX: slam on "05". Wooden block shifting sounds. Crash on collapse. V.O.: *"So we built workarounds on top of workarounds. An entire ecosystem of duct tape. But the foundation never changed."* MX: building tension under narration, drops to silence on collapse. | Exhaustion | Tower collapse animation (begins at 2:38, completes at 2:42) |
| 2:42–2:45 | Act II — Bridge | Black screen. White text fades in, centered: *"What if we stopped building workarounds…"* Beat. Second line: *"…and built the thing right?"* | V.O. reads the text. MX: drops out completely. 1 s of silence after the last word. | Hope | Cut to black (instant); text fades in (0.8 s ease) |
| 2:45–2:55 | Act III — Reveal | Palette shift: background transitions from dark grey to the W0rkTree brand deep navy. Logo appears center, smaller than title card. Below it, text: *"Not a wrapper. Not a plugin. A new protocol."* | V.O.: *"We didn't fork Git. We didn't wrap Git. We sat down with a blank screen and asked: what should version control actually be?"* MX: new theme — confident, open, synthesizers with warmth. | Clarity | Palette shift (1 s cross-fade to brand navy); logo scales in (0.5 s ease) |
| 2:55–3:15 | Act III — Architecture (Worker) | Architecture diagram begins building. First element: a rounded rectangle labeled **"WORKER"** (local machine). Inside it, sub-components animate in: *working directory*, *snapshot staging*, *local cache*. Arrows show data flow. Annotations appear beside each component. | V.O.: *"The Worker lives on your machine. It watches your files, stages snapshots — not diffs, full snapshots — and keeps a local cache so you're never blocked by the network."* MX: synth arpeggios, building. | Understanding | Animated build — components fly in from left and assemble |
| 3:15–3:30 | Act III — Architecture (Server + Staged Visibility) | Second rounded rectangle animates in to the right: **"SERVER"**. A connection line draws between Worker and Server. Between them, a glowing label appears: **"STAGED SNAPSHOT VISIBILITY"**. The label pulses once. Below it, a brief annotation: *"Your team sees changes before they merge."* | V.O.: *"The Server holds the truth. But between Worker and Server, something new: Staged Snapshot Visibility. Your teammates can see what you're working on — before you push, before you merge. Conflicts detected in real time. Not after the fact."* MX: synth chord opens on "Staged Snapshot Visibility". | Innovation | Text impact animation for "STAGED SNAPSHOT VISIBILITY" (0.3 s slam, pulse glow) |
| 3:30–3:45 | Act III — Dashboard | Architecture diagram slides left (30% frame). Right 70%: a team dashboard mockup animates in. It shows team members, their current working branches, file-level change indicators, and a real-time conflict warning badge. A user avatar has a green indicator; another has an amber warning. | V.O.: *"Imagine knowing — right now — that your teammate is editing the same file. No surprise conflicts. No lost work. Just awareness."* MX: music builds, hopeful. | Excitement | Animated UI build — dashboard elements appear sequentially (0.2 s stagger) |
| 3:45–4:00 | Act III — Features | Dashboard slides away. Three feature cards animate in from below, evenly spaced: **"Native Access Control"** (lock icon), **"Built-in License Compliance"** (scale icon), **"Immutable Audit History"** (chain icon). Each card has a one-line description beneath the title. | V.O.: *"Access control at the protocol level — not bolted on. License compliance built in — not an afterthought. And an immutable history that no one can rewrite. Not even admins."* MX: confident three-note motif, one note per card. | Confidence | Card animations — each card rises and settles with a slight bounce (cubic-bezier ease) |
| 4:00–4:15 | Act III — Complete | Feature cards dissolve. Full architecture diagram returns center-frame, now complete: Worker, Server, Staged Visibility, access control layer, audit log, license engine. All components connected. A subtle particle animation flows along the connection lines. The word **"W0rkTree Protocol"** appears beneath. | V.O.: *"This is the W0rkTree Protocol. Everything you need. Nothing you don't. Built from first principles for how teams actually work today."* MX: music reaches its peak — full, warm, resolved. | Wholeness | Diagram completion animation — final connections draw in, particles begin flowing |
| 4:15–4:25 | Act IV — Demo (Init) | Hard cut. Full-screen terminal, dark theme, W0rkTree prompt. Commands type in at realistic speed: `wt init my-project` → output shows initialization. `wt status` → clean status display with branch, file tree, team indicators. | SFX: mechanical keyboard (softer than Cold Open). Ambient low synth tone. V.O. is silent — the terminal speaks. | Proof | Hard cut from diagram to terminal |
| 4:25–4:35 | Act IV — Demo (Team) | Terminal continues. `wt status --team` → shows two team members, their active files, no conflicts. `wt snapshot "Add payment module"` → snapshot created, hash displayed. | SFX: keystrokes. Subtle confirmation chime on snapshot. V.O. still silent. | Proof | Quick cuts between commands (0.3 s black between each) |
| 4:35–4:42 | Act IV — Demo (Push + Access) | Terminal continues. `wt push` → pushed to server, team notified. `wt access grant @designer --path /assets --read` → access granted confirmation. | SFX: keystrokes. Network send sound on push. V.O. still silent. | Proof | Quick cuts between commands |
| 4:42–4:45 | Act IV — Narrator Return | Terminal dims slightly. Lower-third text appears: *"Real commands. Real output. No tricks."* | V.O. returns: *"It just works."* MX: single sustained chord. | Satisfaction | Terminal dims (0.5 s); text fades in (0.3 s) |
| 4:45–4:52 | Close — Logo | Terminal fades to black. W0rkTree logo fades in center-screen. The logo has a subtle breathing glow animation (gentle pulse, 2 s cycle). Tagline appears below: *"Version control, rebuilt."* | MX: theme melody returns, gentle, resolving. V.O. is silent for 3 s. | Invitation | Fade from terminal to black (0.5 s); logo fades in (1 s) |
| 4:52–4:57 | Close — CTA | Logo shifts up slightly. Below the tagline, the narrator's final line appears as text simultaneously with V.O. Below that: a URL and secondary CTA. | V.O.: *"Star the repo. Join the waitlist. Help us build what Git should have been."* MX: final chord, slow fade. | Resolution | Text elements fade in sequentially (0.3 s stagger) |
| 4:57–5:00 | Close — End | Everything holds for 1 s. Then all elements fade to pure black over 1.5 s. 0.5 s of black silence before the file ends. | MX: resolves to silence. Room tone (0.5 s). | Action | Fade to black (1.5 s); hold black (0.5 s) |

---

## Editorial Principles

These rules are non-negotiable. Every editor, animator, and compositor working on this project must follow them without exception.

1. **Hard cuts between segments.** Dissolves and cross-fades are prohibited except for the logo fade-in at 0:25 and 4:45. Segment boundaries are clean, decisive cuts — they mirror the confidence of the product messaging.

2. **No static frames longer than 4 seconds.** Something must always be building, moving, typing, or animating. If a frame must hold (e.g., the final CTA), it must contain a subtle ambient animation — a breathing glow, a flowing particle, a blinking cursor.

3. **Terminal text types at realistic human speed.** Character output rate: 8–12 characters per second for "typing" and instant-reveal for command output (as a real terminal would behave). Never instant-type a command. Never artificially slow the output. The audience must believe a human is at the keyboard.

4. **Narrator audio dominance is absolute.** Narrator V.O. is mixed at −12 LUFS. Music bed is mixed at −24 LUFS (minimum 12 dB below narrator at all times). SFX peaks may reach −18 LUFS but must duck immediately when narration begins. There is no scenario where music or SFX compete with the narrator's intelligibility.

5. **On-screen text readability minimum: 2 seconds.** Every text element — title card, annotation, label, URL — must be fully visible and stationary for at least 2 seconds after its entrance animation completes. If pacing doesn't allow 2 seconds, the text must be cut, not rushed.

6. **Strict color palette adherence.** No off-brand colors, no approximations, no "close enough." Every hex value must come from `01_DESIGN_SYSTEM.md`. If a color isn't in the system, it doesn't appear in the video. Violations are blocking issues in review.

7. **Sound-off legibility for the first 10 seconds.** The Cold Open (0:00–0:10) must tell its story entirely through terminal text. A viewer watching on mute in a social feed must understand that Git commands are failing. No reliance on narration, SFX, or music for comprehension in this window.

8. **Cubic-bezier easing on all motion.** Every animation — position, scale, opacity, color — uses `cubic-bezier(0.4, 0, 0.2, 1)` (Material Design standard ease) or a documented variant from the animation component library. Linear interpolation is strictly prohibited. Motion must feel organic and intentional.

9. **No stock footage, no stock illustrations, no stock music.** All visuals are one of three categories: (a) original live-action footage shot for this production (Cold Open only), (b) screen capture of actual W0rkTree CLI output, or (c) custom motion graphics built to the design system. Music and SFX are original compositions or commissioned works with full licensing.

10. **Final frame hold: exactly 3 seconds.** The URL and CTA text must be fully visible and motionless for exactly 3.0 seconds before the fade-to-black begins. This is measured from the moment the last text element's entrance animation completes to the first frame of the fade. No shorter. No longer.

---

## Render & Delivery Specs

All deliverables are rendered from the same master project file. Social cuts are re-edited per `12_SOCIAL_CUTS.md`, not simply cropped or resized.

| Deliverable | Resolution | FPS | Codec | Bitrate | Container | Use |
|---|---|---|---|---|---|---|
| Master (4K) | 3840×2160 | 60 | Apple ProRes 422 HQ | N/A (lossless) | `.mov` | Archive master — never uploaded directly |
| YouTube Upload | 3840×2160 | 60 | H.265 (HEVC) | 50 Mbps CBR | `.mp4` | YouTube primary upload |
| HD Fallback | 1920×1080 | 60 | H.264 (AVC) | 20 Mbps CBR | `.mp4` | Website embed, backup hosting, presentations |
| Twitter/X (60 s) | 1920×1080 | 30 | H.264 (AVC) | 12 Mbps CBR | `.mp4` | Twitter/X native upload |
| LinkedIn (90 s) | 1920×1080 | 30 | H.264 (AVC) | 12 Mbps CBR | `.mp4` | LinkedIn native video |
| TikTok / IG Reels (60 s) | 1080×1920 (9:16) | 30 | H.264 (AVC) | 12 Mbps CBR | `.mp4` | TikTok, Instagram Reels |
| Instagram Square (60 s) | 1080×1080 (1:1) | 30 | H.264 (AVC) | 12 Mbps CBR | `.mp4` | Instagram feed post |
| YouTube Thumbnail | 1280×720 | N/A | PNG | N/A | `.png` | YouTube thumbnail image |
| Subtitle File (SRT) | N/A | N/A | SubRip | N/A | `.srt` | YouTube, accessibility |
| Subtitle File (VTT) | N/A | N/A | WebVTT | N/A | `.vtt` | Website player, accessibility |

### Delivery Notes

- **Audio in all video deliverables:** AAC-LC, 48 kHz, stereo, 320 kbps.
- **Loudness normalization:** All deliverables are mastered to −14 LUFS integrated (YouTube standard). Social cuts may be re-mastered to −16 LUFS if platform-specific testing reveals clipping on mobile speakers.
- **Subtitle accuracy:** SRT and VTT files must be hand-reviewed — no auto-generated captions. Technical terms (`W0rkTree`, `wt init`, `git reset --hard`, `STAGED SNAPSHOT VISIBILITY`) must be spelled exactly as they appear on screen.
- **File naming convention:** `w0rktree_launch_[deliverable]_v[version].[ext]` — e.g., `w0rktree_launch_youtube4k_v1.0.mp4`.
- **Color space verification:** All deliverables must be spot-checked for Rec. 709 conformance. No wide-gamut leakage. Brand navy and brand accent colors must match hex values in `01_DESIGN_SYSTEM.md` when sampled from the encoded output.

---

*This document is the master authority for all production decisions. In any conflict between this document and a scene-level spec, this document wins. Questions, clarifications, and change requests are routed through the production lead and versioned in this file's header.*