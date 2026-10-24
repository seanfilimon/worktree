# Scene 7 — Close: The Invitation

| Field | Detail |
|---|---|
| **Document** | `08_SCENE_CLOSE.md` |
| **Parent** | `00_PRODUCTION_BIBLE.md` |
| **Version** | 1.0 |
| **Status** | PRE-PRODUCTION |
| **Scene** | 7 of 7 |
| **Timecode** | 4:45.000–5:00.000 |
| **Duration** | 15.000 seconds |
| **FPS** | 60 fps |
| **Canvas** | 3840×2160 |

---

## Table of Contents

1. [Scene Overview](#scene-overview)
2. [Philosophy](#philosophy)
3. [Shot List](#shot-list)
   - [SHOT 7.1 — Logo Return](#shot-71--logo-return)
   - [SHOT 7.2 — Tagline](#shot-72--tagline)
   - [SHOT 7.3 — CTA](#shot-73--cta)
   - [SHOT 7.4 — Final Hold + Fade](#shot-74--final-hold--fade)
4. [Full Dialogue for the Close](#full-dialogue-for-the-close)
5. [Audio Design](#audio-design)
   - [Music](#music)
   - [SFX](#sfx)
6. [Typography & Color Reference](#typography--color-reference)
   - [Type Tokens Used in This Scene](#type-tokens-used-in-this-scene)
   - [Complete Color Map](#complete-color-map)
7. [Layout Specification](#layout-specification)
8. [Editorial Notes](#editorial-notes)
9. [Frame-Accurate Timing Reference](#frame-accurate-timing-reference)
10. [Continuity & Handoff](#continuity--handoff)

---

## Scene Overview

The Close is the final 15 seconds of the video. Its job is deceptively simple: send the viewer off with the brand, the tagline, and a clear call to action. But "simple" is not "easy." These 15 seconds must carry the emotional weight of everything that came before — the Git horror of the Cold Open, the five structural failures of Act II, the architectural promise of Act III, the terminal proof of Act IV — and resolve it into a single feeling: **invitation**.

The viewer should leave this scene knowing three things:
1. **The brand**: W0rkTree. The name. The logo. The "0" glow.
2. **The promise**: "Version control, rebuilt from zero."
3. **The next step**: `w0rktree.dev`. Star on GitHub. Join the Discord.

Everything in this scene exists to serve those three outcomes. No new information is introduced. No new arguments are made. The Close is not persuasion — it is resolution.

**Purpose**: Brand recall. Tagline imprint. Clear CTA. Emotional resolution.

**Narrative function**: The Close completes the six-beat emotional arc defined in the Production Bible. The viewer has been taken from resignation (Cold Open) through frustration (Act II) to hope (Act III) to proof (Act IV). The Close transforms proof into **invitation** — the viewer is not just convinced, they are welcomed.

**Emotional Arc**: Resolution → Invitation → Action

---

## Philosophy

After the rapid-fire terminal demo of Act IV, the viewer needs a moment to breathe. The Close provides that breath. Where Act IV was fast cuts, real keystrokes, and dense terminal output, the Close is the opposite: slow fades, sustained chords, measured narration, and generous whitespace.

This is the "luxury" of confidence. A product that needs to shout doesn't hold for 3 seconds on a logo. A product that needs to rush doesn't let a chord sustain and decay. W0rkTree has already proven itself — in the architecture diagram, in the terminal demo. The Close says: "We showed you. Now come see for yourself."

The visual callback to the Title Card (Scene 2) is intentional. The video opened with the W0rkTree logo appearing out of darkness after 25 seconds of Git pain. The video closes with the same logo, in the same position, but the context has changed completely. The first time the viewer saw the logo, it was a question: "What is this?" The second time, it is an answer: "This is what I need."

---

## Shot List

### SHOT 7.1 — Logo Return

| Field | Detail |
|---|---|
| **Timecode** | 4:45.000–4:50.000 |
| **Duration** | 5.000 seconds |
| **Purpose** | Re-establish the brand after the terminal demo. Visual callback to the Title Card. |

#### Frame Description

Pure Deep Navy (`#0A0F1A`) background. No terminal remnants. No UI chrome. No texture. Just the canvas color and the W0rkTree logo, center-frame. The logo uses the same position, size, and style as the Title Card (Scene 2, `03_SCENE_TITLE_CARD.md`). This visual callback connects the two brand moments — one at the beginning of the video, one at the end.

The frame should feel spacious. The logo occupies roughly 20% of the horizontal frame width, centered both horizontally and vertically (with slight upward offset to leave room for the tagline and CTA elements that will appear below in subsequent shots).

#### Camera

Static. No movement. No drift. No parallax. The stillness is the point.

#### Animation Sequence

| Timecode | Event | Duration | Easing | Notes |
|---|---|---|---|---|
| 4:45.000–4:45.500 | Deep Navy holds | 500ms | — | Let the eye reset from the terminal. The viewer's visual system needs a beat of emptiness after the dense terminal output of Act IV. This 500ms is not dead time — it is a palate cleanser. |
| 4:45.500–4:46.500 | Logo fades in | 1000ms | `ease-decelerate` (`cubic-bezier(0, 0, 0.2, 1)`) | Opacity: 0% → 100%. Simultaneous `translate-y`: +8px → 0. Same animation as Title Card (Scene 2). The slight upward drift creates a sense of gentle arrival, not abrupt appearance. |
| 4:46.500–4:50.000 | Logo holds with breathing glow | 3500ms | — | Logo is static. The "0" glow begins its breathing animation (see below). |

#### Logo "0" Breathing Glow

The "0" in the W0rkTree wordmark has a radial glow effect (defined in `01_DESIGN_SYSTEM.md`, §5.2 Logo Breathing Glow). In this scene, the breathing animation activates and continues for the remainder of the video.

| Property | Value |
|---|---|
| Glow color | Accent Cyan `#00D4FF` |
| Glow type | Radial gradient, centered on the "0" character |
| Glow radius | 1.5× the character width |
| Opacity range | 20% ↔ 40% (oscillates between these values) |
| Cycle duration | 3000ms (full cycle: 20% → 40% → 20%) |
| Easing | `ease-standard` (`cubic-bezier(0.4, 0, 0.2, 1)`) for both expand and contract phases |
| Start time | 4:46.500 (immediately when logo reaches full opacity) |
| End time | 4:59.000 (glow fades out with everything else in SHOT 7.4) |

The breathing glow is the last moving element the viewer sees before the fade to black. It should feel like a heartbeat — W0rkTree is alive.

#### Audio

The Act III theme returns in its resolved form. This is not a recap or a replay — it is a resolution. The same harmonic material, but stripped to its essence: a single clear synth chord.

| Property | Value |
|---|---|
| Instrument | Synth pad + bell layer (same patch as Act III main theme) |
| Chord | C major (C3–E3–G3, spread voicing) |
| Character | Spacious. Final. Warm but not sentimental. |
| Reverb | Long hall reverb (3.5s decay, 30% wet). The chord should feel like it exists in a large, open space. |
| Volume | −20 dB LUFS, rising gently from the ambient pad of Act IV (which was at −34 dB LUFS). The transition is a smooth 2-second swell beginning at ~4:43. |

The narrator begins speaking at 4:46.000, 500ms after the logo starts fading in.

#### Narrator V.O.

> "Git was the right tool for 2005. W0rkTree is the right tool for what comes next."

#### Delivery Notes

This is the video's thesis statement. The single most important line of narration in the entire five minutes. Deliver it with quiet authority. Not a declaration — a fact. The narrator is not arguing. They are not selling. They are stating something that the viewer, after watching four and a half minutes of evidence, already believes.

| Word/Phrase | Emphasis | Notes |
|---|---|---|
| "Git" | Neutral | Not dismissive. Respectful acknowledgment. |
| "the right tool" | Slight emphasis | The word "right" carries the weight. Git was right — then. |
| "2005" | Emphasized | The year is the pivot. It anchors Git to its era. |
| "W0rkTree" | Confident, not loud | The brand name should feel inevitable, not promotional. |
| "what comes next" | Emphasized, slight upward inflection | Forward-looking. Open. The future, not a specific feature. The sentence ends on possibility, not certainty. |

Pacing: The sentence has a natural comma break after "2005." The narrator should honor that break — approximately 400ms of silence between the two halves. The first half (Git/2005) is a closing statement. The second half (W0rkTree/future) is an opening statement. The pause between them is the pivot.

---

### SHOT 7.2 — Tagline

| Field | Detail |
|---|---|
| **Timecode** | 4:50.000–4:53.000 |
| **Duration** | 3.000 seconds |
| **Purpose** | Imprint the tagline. Audio and visual sync. |

#### Frame Description

The logo remains in its position from SHOT 7.1. Below it, with a 32px gap (at 4K), the tagline appears:

**Text**: "Version control, rebuilt from zero."

| Property | Value |
|---|---|
| Type token | `tagline` |
| Font | Inter 400 (Regular) |
| Size | 56px at 4K / 28px at HD |
| Color | `#F0F0F0` at 70% opacity (`#F0F0F0B3`) |
| Alignment | Centered horizontally, positioned below the logo |
| Letter spacing | +0.02 em (per design system `tagline` token) |

The tagline text is intentionally set at 70% opacity rather than 80% — it sits one step quieter than secondary text, giving it a refined, almost whispered quality. The tagline is not competing with the logo. It is supporting it.

#### Animation

| Timecode | Event | Duration | Easing | Notes |
|---|---|---|---|---|
| 4:50.000–4:50.600 | Tagline fades in | 600ms | `ease-enter` (`cubic-bezier(0.0, 0.0, 0.2, 1)`) | Opacity: 0% → 70%. `translate-y`: +12px → 0. The slightly larger translate offset (12px vs. the logo's 8px) creates a sense of the tagline "rising into place" beneath the logo. |
| 4:50.600–4:53.000 | Tagline holds | 2400ms | — | Static. The viewer reads it while the narrator continues. |

**Timing alignment**: The tagline begins its fade-in approximately 500ms after the narrator says "what comes next" (the final words of the SHOT 7.1 narration). This 500ms gap ensures the viewer's attention has shifted from the audio back to the visual before new text appears.

#### Narrator V.O. (continues from SHOT 7.1)

> "Your repos import cleanly. Your workflow gets simpler. Your team gets visibility they've never had. And you never lose work again."

Then, after a beat:

> "W0rkTree. Version control, rebuilt from zero."

#### Delivery Notes

The narration in this shot consists of two distinct sections:

**Section 1 — The Four Promises** (4:50.000–4:53.000):

Four short sentences. Each one lands as its own statement. Not rushed. The rhythm is:

| Sentence | Approx. Timecode | Notes |
|---|---|---|
| "Your repos import cleanly." | 4:50.000–4:50.800 | Matter-of-fact. Addressing the #1 objection (migration) immediately. |
| "Your workflow gets simpler." | 4:50.800–4:51.500 | Slightly warmer. This is a benefit, not just a feature. |
| "Your team gets visibility they've never had." | 4:51.500–4:52.400 | The emphasis lands on "never." This is the Act III callback — staged snapshot visibility. |
| "And you never lose work again." | 4:52.400–4:53.200 | The emotional close. The pause before "And" is slightly longer than the gaps between the previous three sentences (~300ms vs. ~150ms). This sentence is the Cold Open's answer — the Git horror resolved. "Again" is the final word, and it should land with finality. |

**Section 2 — The Tagline Echo** (~4:53.500–4:55.000):

After a beat of approximately 300ms of silence, the narrator says the tagline:

> "W0rkTree. Version control, rebuilt from zero."

This echoes the tagline on screen. The narrator SAYS the tagline as the viewer READS it. Audio and visual sync perfectly. The effect is reinforcement without redundancy — the viewer absorbs the tagline through two channels simultaneously.

The delivery of the tagline is slower and more deliberate than the four promises. "W0rkTree" is spoken as a standalone word, followed by a comma-length pause (~200ms). "Version control" is neutral. "Rebuilt from zero" carries the emphasis, with "zero" as the absolute final word of narration in the entire video.

---

### SHOT 7.3 — CTA

| Field | Detail |
|---|---|
| **Timecode** | 4:53.000–4:57.000 |
| **Duration** | 4.000 seconds |
| **Purpose** | Give the viewer a clear, low-barrier next step. |

#### Frame Description

Below the tagline, two lines of CTA text fade in sequentially:

**Line 1 — URL**:

| Property | Value |
|---|---|
| Text | `w0rktree.dev` |
| Type token | `heading-2` |
| Font | Inter 600 (SemiBold) |
| Size | 64px at 4K / 32px at HD |
| Color | Accent Cyan `#00D4FF` at 100% opacity |
| Alignment | Centered horizontally |
| Position | 48px below the tagline (at 4K) |

The URL is the most visually prominent CTA element on screen. It is rendered in Accent Cyan — the only cyan text in the Close — specifically so it registers as the most "clickable" element. This is a web URL, and it must visually read as one. The viewer's eye should land here first when scanning the lower portion of the frame.

**Line 2 — Actions**:

| Property | Value |
|---|---|
| Text | "Star us on GitHub. Join the Discord. Build with us." |
| Type token | `body-lg` |
| Font | Inter 400 (Regular) |
| Size | 40px at 4K / 20px at HD |
| Color | `#F0F0F0` at 60% opacity (`#F0F0F099`) |
| Alignment | Centered horizontally |
| Position | 16px below the URL (at 4K) |

Three short action phrases. Low barrier. Each one is a distinct, achievable action. The 60% opacity positions this line as clearly subordinate to the URL — the URL is the primary CTA, and these are secondary invitations.

#### Animation Sequence

| Timecode | Event | Duration | Easing | Notes |
|---|---|---|---|---|
| 4:53.000–4:53.600 | URL fades in | 600ms | `ease-enter` (`cubic-bezier(0.0, 0.0, 0.2, 1)`) | Opacity: 0% → 100%. `translate-y`: +12px → 0. |
| 4:53.300–4:53.900 | Action text fades in | 600ms | `ease-enter` | Opacity: 0% → 60%. `translate-y`: +12px → 0. Begins 300ms after the URL animation starts — a staggered entry that draws the eye downward. |
| 4:54.400–4:55.200 | URL underline draws | 800ms | `ease-standard` (`cubic-bezier(0.4, 0, 0.2, 1)`) | Optional. A thin line (2px, `#00D4FF`) draws left-to-right beneath the URL. The line starts at the left edge of the "w" and ends at the right edge of the "v". This subtle animation reinforces that the URL is interactive — it reads as a hyperlink underline being "drawn." |
| 4:55.200–4:57.000 | All CTA elements hold | 1800ms | — | Static. The viewer reads. |

#### Audio

The narrator has finished speaking by ~4:55.000. The synth chord is fading. A brief, clean silence settles in. The music resolves to its final note and holds — this is the space where the viewer decides to visit the site. The audio should not compete with that decision.

---

### SHOT 7.4 — Final Hold + Fade

| Field | Detail |
|---|---|
| **Timecode** | 4:57.000–5:00.000 |
| **Duration** | 3.000 seconds |
| **Purpose** | Let everything breathe. Then end. |

#### Frame Description

Everything is visible: the logo (with breathing glow on the "0"), the tagline, the URL, and the action text. All elements are perfectly stacked vertically, centered on the canvas.

#### Layout (Vertical Stack, Centered)

```
                      [W0rkTree logo — breathing glow on "0"]

                                   32px gap

                  "Version control, rebuilt from zero."

                                   48px gap

                            w0rktree.dev

                                   16px gap

          "Star us on GitHub. Join the Discord. Build with us."
```

All gaps are measured at 4K (3840×2160). Divide by 2 for HD (1920×1080).

The vertical stack should be centered on the canvas as a group — meaning the midpoint of the entire stack (from the top of the logo to the bottom of the action text) aligns with the vertical center of the frame, with a slight upward bias (~5% of frame height) to account for the optical center being higher than the geometric center.

#### Animation Sequence

| Timecode | Event | Duration | Easing | Notes |
|---|---|---|---|---|
| 4:57.000–4:59.000 | Everything holds | 2000ms | — | All elements are static except the "0" glow breathing. This 2-second hold is the most important pause in the Close. The viewer is reading the CTA. Do not interrupt them. |
| 4:59.000–5:00.000 | Everything fades to black | 1000ms | `ease-exit` (`cubic-bezier(0.4, 0, 1, 1)`) | All elements fade simultaneously: opacity → 0%. The breathing glow on the "0" fades with the logo — it does not persist independently. The background remains Deep Navy (`#0A0F1A`) transitioning to pure black (`#000000`) over the same 1000ms. Music fades simultaneously (see Audio Design). |
| 5:00.000 | Pure black. Silence. End. | — | — | The final frame of the video is `#000000`. There is no audio content. The file ends here. |

---

## Full Dialogue for the Close

The following is the complete, authoritative narrator script for Scene 7. Line breaks indicate distinct delivery beats. The `[beat]` marker indicates a deliberate pause of approximately 300ms.

> "Git was the right tool for 2005. W0rkTree is the right tool for what comes next."

> "Your repos import cleanly. Your workflow gets simpler. Your team gets visibility they've never had. And you never lose work again."

> [beat]

> "W0rkTree. Version control, rebuilt from zero."

### Word Count & Pacing Validation

| Metric | Value |
|---|---|
| Total words | 47 |
| Speaking time | ~10 seconds (4:46.000–4:56.000) |
| Effective pace | ~282 WPM |
| Target pace | 260–300 WPM (conversational-to-measured) |

The word count is intentionally low for a 15-second scene. Approximately 5 seconds of the Close have no narration at all (the initial 1-second hold, the final 4 seconds). This is by design — the Close should feel spacious, not packed.

### Narrator Casting Brief (Scene-Specific)

The narrator's performance in the Close must be distinctly different from the rest of the video:

| Scene | Narrator Tone |
|---|---|
| Cold Open (Scene 1) | Intimate, quiet, naming the viewer's feeling |
| Act I (Scene 3) | Storytelling, documentary warmth |
| Act II (Scene 4) | Building intensity, controlled frustration |
| Act III (Scene 5) | Confident, revelatory, building excitement |
| Act IV (Scene 6) | Absent (terminal speaks for itself), then brief return |
| **Close (Scene 7)** | **Quiet authority. Satisfied. Not smug. The narrator has delivered a promise, and the product has kept it. This is the tone of someone who knows they are right and doesn't need to prove it anymore.** |

The Close is the narrator's goodbye. The last thing the viewer hears is "rebuilt from zero." That word — "zero" — should land with the weight of a period at the end of a well-written essay. Final. Complete. Definitive.

---

## Audio Design

### Music

The music in the Close serves a single function: resolution. The entire video's musical arc — from silence (Cold Open) through tension (Act II) through building confidence (Act III) through near-silence (Act IV) — resolves here into a single, sustained chord. This is the musical equivalent of exhaling.

| Timecode | Musical Event | Volume | Notes |
|---|---|---|---|
| 4:45.000–4:50.000 | Act III main theme returns, resolved. Single major chord (C major, synth pad + bell layer). | −20 dB LUFS | Rising gently from the ambient pad of Act IV (which was at −34 dB LUFS). The transition is a smooth swell that begins around 4:43 (during Act IV). By 4:45, the chord is fully present. Spacious reverb (3.5s decay). The chord should feel like arriving home — warm, open, final. |
| 4:50.000–4:53.000 | Chord sustains. A very subtle arpeggio (root + 5th + octave, quiet) plays beneath. | Chord: −20 dB LUFS. Arpeggio: −28 dB LUFS | The arpeggio adds gentle movement beneath the sustained chord. It should be felt more than heard — a sense of gentle forward motion during the tagline reveal. Notes: C3 → G3 → C4, slow arpeggiation (~1.5s per cycle), triangle wave or soft bell timbre. |
| 4:53.000–4:57.000 | Arpeggio fades. Just the sustained chord remains. Slowly decaying. | −20 dB LUFS → −26 dB LUFS | The chord is no longer actively played — it is ringing out. Natural decay. The reverb tail extends the sound beyond what the synth sustain alone would produce. This slow fade creates space for the CTA text to be read without competition. |
| 4:57.000–5:00.000 | Everything fades to silence. | −26 dB LUFS → −∞ | The final note's reverb tail extends past the visual fade — for the last 200–300ms before 5:00.000, the viewer hears just the faintest echo of the chord as the screen goes black. This reverb tail IS the goodbye. It is the last thing the viewer hears. Then: silence. True digital silence. End. |

### SFX

**There are no sound effects in the Close.**

No clicks. No tones. No chimes. No whooshes. No transition sounds. The Close is music + voice only. After the dense SFX landscape of Act IV (keyboard clicks, completion chimes, the denied buzzer), the absence of sound effects in the Close is itself a statement: the product has finished speaking. Now the brand speaks.

This parallels the Cold Open, which also had no music. The Cold Open was silence + SFX. The Close is music + silence. The two bookends of the video use absence as a design tool.

---

## Typography & Color Reference

This section consolidates every text style and color used in the Close for quick reference during production.

### Type Tokens Used in This Scene

| Token | 4K Size | HD Size | Weight | Font | Use in This Scene |
|---|---|---|---|---|---|
| `tagline` | 56px | 28px | 400 (Regular) | Inter | "Version control, rebuilt from zero." |
| `heading-2` | 64px | 32px | 600 (SemiBold) | Inter | `w0rktree.dev` URL |
| `body-lg` | 40px | 20px | 400 (Regular) | Inter | "Star us on GitHub. Join the Discord. Build with us." |

> **Note**: No monospace (`code-lg`, `code`, `code-sm`) tokens appear in the Close. This is intentional — the Close is a brand scene, not a technical scene. The font is entirely Inter.

### Complete Color Map

| Element | Hex | Opacity | Notes |
|---|---|---|---|
| Background (start) | `#0A0F1A` | 100% | Deep Navy. Holds for the first 14 seconds. |
| Background (end) | `#000000` | 100% | Pure black. Transition begins at 4:59.000. |
| Logo wordmark | `#F0F0F0` | 100% | Same as Title Card. |
| Logo "0" glow | `#00D4FF` | 20%–40% | Breathing animation. Radial gradient. |
| Tagline text | `#F0F0F0` | 70% | Slightly quieter than secondary text (80%). Refined, understated. |
| URL (`w0rktree.dev`) | `#00D4FF` | 100% | Accent Cyan. The most visually prominent text element in the Close. Must POP as the most clickable thing on screen. |
| URL underline | `#00D4FF` | 100% | 2px line, drawn left-to-right. Optional but recommended. |
| Action text | `#F0F0F0` | 60% | Tertiary. Clearly subordinate to the URL. |

---

## Layout Specification

The Close uses a single, vertically-stacked layout centered on the canvas. All measurements are at 4K (3840×2160).

### Vertical Stack

| Element | Width | Height (approx.) | Position | Notes |
|---|---|---|---|---|
| Logo | ~768px (20% of 3840) | ~120px | Center of stack | Same size and position as Title Card (Scene 2). Slight upward bias from true center (~5% of frame height above geometric center). |
| Gap 1 | — | 32px | — | Between logo baseline and tagline cap height. |
| Tagline | Auto (text width) | ~56px (single line) | Centered below logo | "Version control, rebuilt from zero." — should fit on one line at 56px. |
| Gap 2 | — | 48px | — | Larger gap separates the brand elements (logo + tagline) from the CTA elements (URL + actions). This visual separation creates two distinct "zones." |
| URL | Auto (text width) | ~64px (single line) | Centered below tagline | `w0rktree.dev` — the primary CTA. |
| Gap 3 | — | 16px | — | Tight gap keeps the URL and action text as a visual pair. |
| Action text | Auto (text width) | ~40px (single line) | Centered below URL | "Star us on GitHub. Join the Discord. Build with us." — should fit on one line at 40px. |

### Horizontal Alignment

Every element is centered horizontally on the canvas (`x = 1920px` at 4K). No element is offset left or right. The Close is perfectly symmetrical.

### Safe Zones

| Zone | Specification |
|---|---|
| Title safe | 5% inset from all edges (192px horizontal, 108px vertical at 4K) |
| Action safe | 3.5% inset from all edges (134px horizontal, 76px vertical at 4K) |

All text elements must fall within the title safe zone. The logo should fall within the action safe zone. These zones ensure readability across all display types, including TVs with overscan.

---

## Editorial Notes

These are high-level observations and constraints for the editor, director, and post-production team.

### 1. The Close Is Luxurious

The Close is 15 seconds. It should FEEL luxurious. After the rapid-fire Act IV demo — six hard-cut terminal shots in 25 seconds — the viewer needs a moment to decompress. The slow logo fade, the sustained chord, the measured narrator — all of this communicates confidence: "We don't need to rush. We've already shown you."

If the Close feels rushed in any edit pass, the solution is to remove content, not speed up delivery. The pacing is non-negotiable. Every millisecond of hold time was specified with intent.

### 2. CTA Readability Is Non-Negotiable

The CTA text (`w0rktree.dev` and the three action phrases) must be readable for the FULL 4 seconds it is on screen (4:53.000–4:57.000). This is where the viewer decides to visit the site. Do not:

- Overlay the CTA with competing visual elements
- Start the fade-to-black before 4:59.000
- Add motion or animation to the CTA text after it has appeared
- Reduce the size of the CTA text to "fit" additional elements

If the CTA is not readable on a phone screen at 1080p, increase the type size. Readability > design purity.

### 3. The 5:00.000 Boundary Is Sacred

The final fade to black must complete EXACTLY at 5:00.000. The video runtime is 5 minutes. This is a promise made implicitly by the Production Bible and explicitly by any video player's duration display. The viewer expects the video to end at 5:00. Deliver on that expectation.

- No post-credits content
- No extended black hold beyond 5:00.000
- No "bonus" content or easter eggs after the fade
- The 5:00.000 frame is the last frame. The file ends here.

### 4. The Breathing Glow Is the Final Moving Element

The "0" glow breathing animation is the last motion the viewer sees before the fade to black. As the static elements (text, layout) hold for the final 2 seconds, the gentle 20%→40%→20% opacity pulse on the "0" is the only thing moving in the frame. This should feel like a heartbeat — W0rkTree is alive, quietly, confidently.

When the fade-to-black begins at 4:59.000, the breathing glow fades with everything else. It does not persist independently or continue pulsing as other elements fade. All elements depart together.

### 5. Audio-Visual Sync on the Tagline

The narrator says "W0rkTree. Version control, rebuilt from zero." while the tagline text is on screen. The audio and visual must align:

- The tagline text is already visible by the time the narrator reaches it (~4:53.500)
- The narrator's delivery of the tagline should sync with the viewer reading it — approximately 2 seconds of delivery for a 6-word tagline
- The final word ("zero") should land with both audio and visual weight — it is the last word the narrator speaks, and it visually echoes the "0" in the logo above

This three-way resonance (the word "zero," the text "zero" in the tagline, the "0" in the logo) is the Close's final rhetorical device. Everything connects to zero — the starting point. W0rkTree was built from zero. The tagline says it. The logo shows it. The narrator speaks it.

### 6. Sound-Off Legibility

The Close must work with sound off. A viewer watching on mute should be able to extract:

1. The brand name (logo)
2. The tagline ("Version control, rebuilt from zero.")
3. The URL (`w0rktree.dev`)
4. The actions ("Star us on GitHub. Join the Discord. Build with us.")

All four pieces of information are on-screen text. No narration is required for comprehension. The narrator enhances the experience — the viewer understands the thesis statement and the emotional resolution through the voice — but the raw information is carried by the visual layer.

### 7. No New Information

The Close introduces zero new concepts, features, or arguments. Everything the narrator says in the Close has been established earlier in the video:

| Close Statement | Where It Was Established |
|---|---|
| "Git was the right tool for 2005" | Act I — History (Scene 3) |
| "W0rkTree is the right tool for what comes next" | Act III — Product Reveal (Scene 5) |
| "Your repos import cleanly" | Implied by Act III architecture discussion |
| "Your workflow gets simpler" | Act IV — Demo (Scene 6) |
| "Your team gets visibility they've never had" | Act III — Staged Snapshot Visibility; Act IV — `wt status --team` |
| "You never lose work again" | Act II — Problem 3 (data loss); Act III — immutable history |

The Close is a summary, not a pitch. The pitch already happened. The Close is the bow on the package.

---

## Frame-Accurate Timing Reference

For the editor's reference, here is the complete timing grid with frame numbers at 60fps.

| Event | Timecode | Frame # (60fps) |
|---|---|---|
| Scene 7 begins (Deep Navy hold) | 4:45.000 | 16200 |
| Logo fade-in begins | 4:45.500 | 16230 |
| Narrator begins: "Git was the right tool…" | 4:46.000 | 16260 |
| Logo reaches full opacity; breathing glow starts | 4:46.500 | 16290 |
| Logo holds | 4:46.500–4:50.000 | 16290–16800 |
| Tagline fade-in begins | 4:50.000 | 16800 |
| Tagline reaches full opacity | 4:50.600 | 16836 |
| Narrator: "Your repos import cleanly…" | 4:50.000 | 16800 |
| URL fade-in begins | 4:53.000 | 16980 |
| Action text fade-in begins | 4:53.300 | 16998 |
| URL reaches full opacity | 4:53.600 | 17016 |
| Narrator: "W0rkTree. Version control, rebuilt from zero." | ~4:53.500 | ~17010 |
| URL underline draw begins | 4:54.400 | 17064 |
| URL underline draw completes | 4:55.200 | 17112 |
| Narrator finishes (last word: "zero") | ~4:55.000 | ~17100 |
| All CTA elements holding | 4:55.200–4:57.000 | 17112–17220 |
| Final hold begins (all elements static, glow breathing) | 4:57.000 | 17220 |
| Fade-to-black begins | 4:59.000 | 17340 |
| Fade-to-black completes. Pure black. Silence. | 5:00.000 | 17400 |

---

## Continuity & Handoff

### Into Scene 7

Scene 7 receives handoff from Act IV (Scene 6, `07_SCENE_ACT_IV_DEMO.md`). The transition:

- **Visual**: The terminal dims to 30% in SHOT 6.7, narrator text appears over it, then the text and terminal fade to Deep Navy (`#0A0F1A`) over 500ms. The Close begins with a held Deep Navy canvas — the same color the terminal faded into. The seam should be invisible.
- **Audio**: The Act IV ambient pad (~−34 dB LUFS) swells over 2 seconds into the Act III resolved chord (~−20 dB LUFS). The swell begins during the final moments of Act IV (~4:43) so that by 4:45, the chord is fully present.
- **Emotional**: Satisfaction (Act IV) → 500ms breath (Deep Navy hold) → Invitation (Close). The viewer has just seen the product work. The Close transforms "it works" into "come try it."

### Out of Scene 7

There is no subsequent scene. Scene 7 is the final scene of the video. The last frame (5:00.000) is pure black (`#000000`). The file ends. There is no post-credits content, no end card, and no YouTube-style "next video" overlay baked into the render.

> **Note for platform uploads**: YouTube end screens (subscribe button, suggested videos) are added via YouTube Studio, not baked into the render. The rendered file ends cleanly at 5:00.000. If platform-specific end cards are needed, they are specified in `12_SOCIAL_CUTS.md`.

### Cross-References

| Document | Relevance to Scene 7 |
|---|---|
| `00_PRODUCTION_BIBLE.md` | Master authority. Narrative arc beat 6 (Invitation). Master Timeline rows 4:45–5:00. Editorial Principles (all apply). Render & Delivery Specs (final frame = 5:00.000). |
| `01_DESIGN_SYSTEM.md` | Logo component spec (§5.2). Breathing glow animation (§5.2). Color palette. Typography tokens (`tagline`, `heading-2`, `body-lg`). Easing curves. |
| `03_SCENE_TITLE_CARD.md` | Visual callback. The Close logo entrance mirrors the Title Card logo entrance — same position, same size, same animation parameters. The two scenes are deliberate bookends. |
| `07_SCENE_ACT_IV_DEMO.md` | Sends handoff to this scene. SHOT 6.7 terminal fade defines the transition seam. |
| `09_ANIMATION_COMPONENTS.md` | Logo entrance animation component. Breathing glow component. Fade-to-black component. Text ease-enter component. |
| `10_AUDIO_SPEC.md` | Narrator casting brief. Act III theme resolution spec. Music fade-out parameters. Loudness standards (−14 dB LUFS integrated). |
| `11_ASSET_MANIFEST.md` | Lists all assets needed for Scene 7: W0rkTree logo (vector), Inter font files, narrator V.O. recording for the Close, music stems (Act III theme resolved chord). |
| `12_SOCIAL_CUTS.md` | Platform-specific re-edits may truncate or modify the Close. This document is authoritative for the full-length version only. |

---

*This document is the authoritative specification for Scene 7 (Close) of the W0rkTree launch video. In any conflict between this document and the Production Bible (`00_PRODUCTION_BIBLE.md`), the Production Bible wins. In any conflict between this document and a scene-level peer document, this document is authoritative for its own timecode range (4:45.000–5:00.000) only. Questions, clarifications, and change requests are routed through the production lead.*