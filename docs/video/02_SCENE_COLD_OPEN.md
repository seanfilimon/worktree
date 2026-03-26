# Scene 1 — Cold Open: The Git Horror

| Field | Detail |
|---|---|
| **Document** | `02_SCENE_COLD_OPEN.md` |
| **Parent** | `00_PRODUCTION_BIBLE.md` |
| **Version** | 1.0 |
| **Status** | PRE-PRODUCTION |
| **Scene** | 1 of 7 |
| **Timecode** | 0:00.000–0:25.000 |
| **Duration** | 25.000 seconds |
| **FPS** | 60 fps (motion graphics) · 24 fps (live-action SHOT 1.5) |
| **Canvas** | 3840×2160 |

---

## Table of Contents

1. [Scene Overview](#scene-overview)
2. [Emotional Arc](#emotional-arc)
3. [Shot List](#shot-list)
   - [SHOT 1.1 — Black Void + Keyboard](#shot-11--black-void--keyboard)
   - [SHOT 1.2 — Terminal: git push rejected](#shot-12--terminal-git-push-rejected)
   - [SHOT 1.3 — Terminal: git pull --rebase conflict](#shot-13--terminal-git-pull---rebase-conflict)
   - [SHOT 1.4 — Terminal: The Catastrophe](#shot-14--terminal-the-catastrophe)
   - [SHOT 1.5 — The Human Moment (Live Action)](#shot-15--the-human-moment-live-action)
   - [SHOT 1.6 — Slack Message](#shot-16--slack-message)
   - [SHOT 1.7 — Narrator Over Black](#shot-17--narrator-over-black)
4. [Dialogue](#dialogue)
5. [Audio Design](#audio-design)
   - [Foley & SFX](#foley--sfx)
   - [Music](#music)
6. [Technical Requirements](#technical-requirements)
   - [Live Action (SHOT 1.5)](#live-action-shot-15)
   - [Motion Graphics (SHOTS 1.1–1.4, 1.6)](#motion-graphics-shots-1114-16)
7. [Terminal Typography & Color Reference](#terminal-typography--color-reference)
8. [Editorial Notes](#editorial-notes)
9. [Continuity & Handoff](#continuity--handoff)

---

## Scene Overview

The Cold Open is the most important 25 seconds of the entire video. Its job is singular and non-negotiable: **make the viewer stay**. Every developer has a Git horror story — the rejected push, the merge conflict that spirals, the `reset --hard` that erases hours of work. This scene puts the viewer *inside* that story before a single word of narration is spoken.

No music. No logo. No branding. Just a dark terminal, real keystrokes, and the slow-motion disaster that every developer has lived through at least once. The viewer recognizes themselves in the first three seconds. By ten seconds they are committed. By the time the narrator finally speaks at 0:17.5, the audience has already agreed with whatever comes next.

This document is the most detailed scene spec in the production package because the Cold Open is the scene where every frame, every sound, and every millisecond of timing matters. There is zero margin for "good enough." The Cold Open either hooks or it doesn't.

**Purpose**: Emotional hook. Universal developer empathy. Immediate pattern recognition.

**Narrative function**: Establish the problem (Git is hostile) through *experience*, not explanation. When the narrator later says "because of a bug in your workflow," the audience already knows exactly what that means — they just lived it.

**Constraint**: The first 10 seconds must work with **sound off**. A viewer scrolling YouTube or a social feed on mute must be able to read `git push`, `CONFLICT`, `git reset --hard`, and `initial commit` and understand the story purely through terminal text. No reliance on audio for comprehension in this window. (See Production Bible, Editorial Principle #7.)

---

## Emotional Arc

The Cold Open follows a precise six-beat emotional progression. Each beat maps to one or more shots.

| Beat | Emotion | Timecode | Shot(s) | Mechanism |
|---|---|---|---|---|
| 1 | **Silence** | 0:00.000–0:02.500 | 1.1 | Black screen. Keyboard sounds. The viewer leans in: *"What am I looking at?"* Curiosity is triggered by the absence of visual information. |
| 2 | **Tension** | 0:02.500–0:06.000 | 1.2 | Terminal appears. `git push` is rejected. The viewer recognizes the situation instantly. Tension is identification: *"I've been here."* |
| 3 | **Dread** | 0:06.000–0:10.000 | 1.3 | The developer tries to fix it. `git pull --rebase`. `CONFLICT`. The situation is getting worse, not better. The viewer knows what comes next. |
| 4 | **Loss** | 0:10.000–0:14.000 | 1.4 | `git reset --hard HEAD~3`. `HEAD is now at 7a2c1e0 initial commit`. Everything is gone. The emotional peak. Silence. |
| 5 | **Empathy** | 0:14.000–0:19.500 | 1.5, 1.6 | The human behind the terminal. A face. A closed laptop. A hesitant Slack message. The viewer transitions from *"I've been here"* to *"I am this person."* |
| 6 | **Resignation** | 0:19.500–0:25.000 | 1.7 | The narrator names the feeling. *"Everyone just accepts it."* The viewer's own resignation is reflected back at them. This is the emotional setup for the rest of the video: *"What if you didn't have to accept it?"* |

---

## Shot List

---

### SHOT 1.1 — Black Void + Keyboard

| Property | Value |
|---|---|
| **Shot ID** | `SHOT-1.1` |
| **Timecode** | 0:00.000–0:02.500 |
| **Duration** | 2.500s |
| **Type** | Black screen (no visual content) |

#### Frame Description

Pure black screen. Absolute black — `#000000`, not near-black, not dark gray. There is nothing to see. The entire sensory experience is auditory. We hear a single mechanical keyboard in a quiet room. The typing is not frantic — it is steady, purposeful, rhythmic. The kind of typing that says *"I'm concentrating."* Approximately 4–6 keystrokes per second with natural human variation (not metronomic). Occasional micro-pauses between words or thoughts (80–150ms gaps). This is a developer in the middle of work. They haven't hit the problem yet.

#### Camera

N/A — black screen. No camera, no virtual camera, no movement.

#### Lighting

N/A — pure black.

#### Audio

| Element | Description | Level | Technical Notes |
|---|---|---|---|
| Room tone | Subtle HVAC hum, the ambient sound of a quiet room with a computer running. Not silence — *inhabited* silence. | −40 dB | Record on location or in a treated room. The hum should be constant, flat, below conscious perception but present enough to establish physical space. Frequency content: broadband below 500Hz, gentle roll-off above. |
| Mechanical keyboard | Cherry MX Blue profile (or equivalent clicky switch). Close-mic'd. Individual keystrokes with natural velocity variation. | −18 dB | **Must be real** — recorded from an actual mechanical keyboard in a quiet room. Not a sound effect library. Not synthesized. The viewer should feel like they're sitting next to someone. Record with a condenser mic 15–20cm from the keyboard, slightly off-axis to avoid harsh transients. Capture at 48kHz/24-bit. |
| Music | None. | — | **No music in the Cold Open.** This is a hard rule. See [Music](#music) section. |

> **CRITICAL NOTE**: The audio design of these first 2.5 seconds is everything. The keyboard sounds must feel *physically real* — the click of the switch, the subtle bottom-out thud, the slight rattle of keycap return. Each keystroke should have micro-variations in timing and velocity. A perfectly even, sample-triggered keyboard loop will feel synthetic and break the illusion immediately. Record 60+ seconds of real typing and select the best 2.5-second segment that has the right rhythm and feel.

#### On-Screen Text

None.

#### Transitions

| Direction | Type | Duration | Notes |
|---|---|---|---|
| **In** | Fade from pure black | 0ms | Instant. The video *starts* on black. There is no fade-in because we are already at black. Frame 1 is `#000000`. |
| **Out** | Hard cut to SHOT 1.2 | 0ms | At exactly 0:02.500, the terminal appears with zero transition. The visual jump from black to terminal is sudden and attention-grabbing. |

---

### SHOT 1.2 — Terminal: git push rejected

| Property | Value |
|---|---|
| **Shot ID** | `SHOT-1.2` |
| **Timecode** | 0:02.500–0:06.000 |
| **Duration** | 3.500s |
| **Type** | Motion graphic (terminal simulation) |

#### Frame Description

A terminal window appears. Full-frame. Code background `#111827`. No window chrome — no title bar, no close/minimize/maximize buttons, no tabs, no scrollbar. Just the terminal content on the dark background. The terminal occupies approximately 80% of the frame width, centered horizontally, vertically centered or slightly above center (following the design system grid). There is a blinking cursor — a thin vertical bar, `#F0F0F0`, blinking at 530ms intervals (standard terminal blink rate). The cursor blinks once, then the command begins typing out character by character.

#### Camera

N/A — screen capture / motion graphic. No virtual camera movement. The frame is static.

#### Lighting

N/A — motion graphic. The terminal IS the light source (conceptually). The dark background creates the sense of a screen glowing in a dark room.

#### Audio

| Element | Description | Level | Technical Notes |
|---|---|---|---|
| Keystrokes | Mechanical keyboard continues, now synced to character typing on screen. Each visible character gets a keystroke sound. | −18 dB | Sync does not need to be sample-accurate but must be perceptually aligned — within ±30ms of the character appearing on screen. Use the same keyboard recording from SHOT 1.1 (continuity). |
| Room tone | Continues from SHOT 1.1. | −40 dB | Unbroken. The room tone must not have any edit point or discontinuity across the SHOT 1.1→1.2 cut. |
| Music | None. | — | |

#### On-Screen Text

**Command line** — typed character by character at **40ms per character** (25 characters/second):

```
$ git push origin main
```

- Total typing duration: 22 characters × 40ms = 880ms
- The `$` prompt appears instantly (it is already on screen when the terminal appears — it was "there" before we cut in)
- A single space after `$`, then typing begins
- After the last character (`n` in `main`) is typed, the cursor holds for **500ms** (the command is "executing")

**Command output** — appears line by line with **50ms between lines** (not typed — revealed instantly per line, as a real terminal renders output):

```
! [rejected] main -> main (non-fast-forward)
error: failed to push some refs to 'origin/main'
hint: Updates were rejected because the tip of your current branch is behind
```

- Line 1 appears at: 0:02.500 + 880ms (typing) + 500ms (execute pause) = **0:03.880**
- Line 2 appears at: 0:03.880 + 50ms = **0:03.930**
- Line 3 appears at: 0:03.930 + 50ms = **0:03.980**
- Output remains on screen for the remainder of the shot (~2.020s of read time)

#### Text Colors

All text renders in **JetBrains Mono** (or the terminal font specified in `01_DESIGN_SYSTEM.md`). Size: `code-lg` from the design system. Line height: 1.6.

| Text Segment | Color | Weight | Notes |
|---|---|---|---|
| `$` | `#6B7280` | 400 (Regular) | Muted prompt. Should not draw attention. |
| `git push origin main` | `#F0F0F0` | 400 (Regular) | Primary text color — bright, readable. |
| `! [rejected]` | `#FF3B3B` | 700 (Bold) | Warning red. This is the first red the viewer sees in the entire video. It must pop. |
| `main -> main (non-fast-forward)` | `#F0F0F0` at 80% opacity | 400 (Regular) | Slightly muted — supporting detail. |
| `error:` | `#FF3B3B` | 700 (Bold) | Same warning red as `[rejected]`. Consistent danger signaling. |
| `failed to push some refs to 'origin/main'` | `#F0F0F0` at 80% opacity | 400 (Regular) | |
| `hint:` | `#6B7280` | 400 (Regular) | Git's hint text is always muted — it's noise the developer ignores. |
| `Updates were rejected because the tip of your current branch is behind` | `#F0F0F0` at 60% opacity | 400 (Regular) | Even more muted. The viewer doesn't need to read this — they just need to see the wall of text that represents "something went wrong." |

#### Transitions

| Direction | Type | Duration | Notes |
|---|---|---|---|
| **In** | Hard cut from black | 0ms | The terminal appears instantly. No fade, no slide, no scale animation. Frame N is black; frame N+1 is the terminal. This is jarring by design — it mimics the feeling of snapping to attention when something goes wrong. |
| **Out** | Hard cut to SHOT 1.3 | 0ms | |

---

### SHOT 1.3 — Terminal: git pull --rebase conflict

| Property | Value |
|---|---|
| **Shot ID** | `SHOT-1.3` |
| **Timecode** | 0:06.000–0:10.000 |
| **Duration** | 4.000s |
| **Type** | Motion graphic (terminal simulation) |

#### Frame Description

Same terminal. The previous output has been cleared (or scrolled up off screen — the viewer should perceive a fresh prompt, as if the developer hit `clear` or the terminal scrolled). New prompt, new command. The developer is trying to fix the rejected push by pulling with rebase. This is the "reasonable next step" that every developer takes. And it makes things worse.

#### Camera

N/A — motion graphic. Static frame. Same framing as SHOT 1.2 (visual continuity).

#### Audio

| Element | Description | Level | Technical Notes |
|---|---|---|---|
| Keystrokes | Mechanical keyboard, synced to typing. Same source recordings as previous shots. | −18 dB | |
| Room tone | Continues unbroken. | −40 dB | |
| Sub-bass hit | A single sub-bass pulse when `CONFLICT` appears on screen. 40Hz fundamental, sine wave with gentle 2nd harmonic. Duration: 400ms with a 200ms exponential decay tail. | −30 dB | This sound should be **felt more than heard**. On laptop speakers it will be inaudible. On headphones or monitors with sub extension, it will register as a physical sensation in the chest — a gut-punch. It must not be consciously perceived as a "sound effect." It is subliminal tension reinforcement. |
| Music | None. | — | |

#### On-Screen Text

**Command line** — typed at **40ms per character**:

```
$ git pull --rebase origin main
```

- Total typing duration: 30 characters × 40ms = 1200ms
- `$` prompt is pre-rendered (instant)
- Typing begins immediately at 0:06.000

**Execution pause**: 500ms after last character.

**Command output** — line-by-line reveal, 50ms between lines:

```
CONFLICT (content): Merge conflict in src/auth/oauth.rs
error: could not apply fa39187... Add token rotation
```

- Line 1 (`CONFLICT...`) appears at: 0:06.000 + 1200ms + 500ms = **0:07.700**
- Line 2 (`error:...`) appears at: 0:07.700 + 50ms = **0:07.750**
- Output remains on screen for ~2.250s of read time

#### Text Colors

| Text Segment | Color | Weight | Notes |
|---|---|---|---|
| `$` | `#6B7280` | 400 | Muted prompt. |
| `git pull --rebase origin main` | `#F0F0F0` | 400 | |
| `CONFLICT` | `#FF3B3B` | **700 (Bold)** | JetBrains Mono 700. This word must visually *punch*. It is the largest, boldest red text on screen. The sub-bass hit fires on the exact frame this word appears. |
| `(content):` | `#F0F0F0` at 80% opacity | 400 | |
| `Merge conflict in` | `#F0F0F0` at 80% opacity | 400 | |
| `src/auth/oauth.rs` | `#00D4FF` | 400 | **Accent cyan.** This is the file path — it should visually pop against the red and white. The cyan draws the eye and makes the output scannable even at a glance. This color must match the brand accent defined in `01_DESIGN_SYSTEM.md`. |
| `error:` | `#FF3B3B` | 700 (Bold) | Consistent with SHOT 1.2 error styling. |
| `could not apply fa39187...` | `#F0F0F0` at 80% opacity | 400 | |
| `Add token rotation` | `#F0F0F0` at 80% opacity | 400 | The commit message — a detail that makes this feel like a real project, not a demo. |

#### Transitions

| Direction | Type | Duration | Notes |
|---|---|---|---|
| **In** | Hard cut from SHOT 1.2 | 0ms | The previous terminal output vanishes; the new prompt appears. This mirrors how a developer's terminal looks after clearing. |
| **Out** | Hard cut to SHOT 1.4 | 0ms | |

---

### SHOT 1.4 — Terminal: The Catastrophe

| Property | Value |
|---|---|
| **Shot ID** | `SHOT-1.4` |
| **Timecode** | 0:10.000–0:14.000 |
| **Duration** | 4.000s |
| **Type** | Motion graphic (terminal simulation) |

#### Frame Description

Same terminal. Cleared. Two commands are typed in sequence. This is the "oh no" moment — the emotional peak of the Cold Open. The developer, frustrated and out of options, aborts the rebase and then does the unthinkable: a hard reset. Not back one commit. Not back two. Back to `HEAD~3` — and the output reveals that `HEAD~3` is the **initial commit**. Everything is gone. Three hours of work, erased in one command.

The power of this shot is in the *output*. The words `initial commit` should land like a punch to the gut. The viewer who has done this before — and many have — will physically cringe.

#### Camera

N/A — motion graphic. Static frame. Same framing.

#### Audio

| Element | Description | Level | Technical Notes |
|---|---|---|---|
| Keystrokes | Mechanical keyboard for both commands. | −18 dB | The typing for `git reset --hard HEAD~3` should feel slightly faster than previous commands — the developer is frustrated, typing with force. Increase keystroke velocity (louder transients) by 2–3 dB for this command only. |
| Room tone | Present during typing. **Drops to near-zero after output appears.** | −40 dB → −∞ | The room tone fade begins on the exact frame that `HEAD is now at...` appears. Fade duration: 200ms exponential decay to silence. The silence that follows is the emotional peak. |
| Silence | Complete silence after `initial commit` output. No room tone, no keyboard, no hum. 1.5 seconds of absolute void. | Silence (−∞) | **This silence is the most important sound in the Cold Open.** It represents the moment where the developer's brain catches up to what they've done. The absence of *any* sound forces the viewer's brain to fill the gap with their own emotional memory. Do not underestimate this. Do not shorten it. |
| Music | None. | — | |

#### On-Screen Text

**Command line 1** — typed at **40ms per character**:

```
$ git rebase --abort
```

- 20 characters × 40ms = 800ms typing duration
- `$` prompt pre-rendered
- Typing begins at 0:10.000

**No output** for `git rebase --abort` (it completes silently, as it does in real Git).

**Pause**: 300ms (the developer's brief breath before the next command).

**New prompt + Command line 2** — typed at **40ms per character**:

```
$ git reset --hard HEAD~3
```

- 25 characters × 40ms = 1000ms typing duration
- Typing begins at: 0:10.000 + 800ms + 300ms = **0:11.100**

**Execution pause**: 500ms.

**Command output** — single line, instant reveal:

```
HEAD is now at 7a2c1e0 initial commit
```

- Appears at: 0:11.100 + 1000ms + 500ms = **0:12.600**
- Remains on screen for ~1.400s (into silence)

#### Text Colors

| Text Segment | Color | Weight | Notes |
|---|---|---|---|
| `$` (both prompts) | `#6B7280` | 400 | |
| `git rebase --abort` | `#F0F0F0` | 400 | Normal command — the developer is still trying to recover. |
| `git reset --hard HEAD~3` | `#FF3B3B` | **700 (Bold)** | **The entire command is danger-red.** Not just `--hard`, not just `reset` — the whole line. This is the destructive act. The red color is a warning the developer is ignoring. Rendering the command itself in red (rather than the output) puts the danger on the *action*, not the *consequence*. |
| `HEAD is now at` | `#FF3B3B` at 60% opacity | 400 | Muted red. The damage is done. This line should feel faded, final, like ash after a fire. |
| `7a2c1e0` | `#FF3B3B` at 60% opacity | 400 | The commit hash — detail that makes it real. |
| `initial commit` | `#FF3B3B` at 60% opacity | 400 | **These two words are the payload.** Despite the muted styling, their *meaning* is devastating. The muted treatment is deliberate — it mirrors how Git delivers catastrophic information in a flat, emotionless way. The tool doesn't care that it just destroyed your work. |

#### Emotional Staging Note

The sequence of colors across SHOTS 1.2–1.4 tells its own story:

- SHOT 1.2: Red appears in the *output* (the system is warning you)
- SHOT 1.3: Red appears in `CONFLICT` (the system is failing you)
- SHOT 1.4: Red appears in the *command itself* (you are now the danger)

This color progression — from system warning → system failure → self-destruction — mirrors the emotional descent without any narration needed.

#### Transitions

| Direction | Type | Duration | Notes |
|---|---|---|---|
| **In** | Hard cut from SHOT 1.3 | 0ms | |
| **Out** | Hard cut to SHOT 1.5 | 0ms | The cut from terminal to live action is deliberate and abrupt. The viewer has been watching abstract text for 14 seconds. Suddenly: a human face. The context switch is disorienting in the best way — it forces re-engagement. |

---

### SHOT 1.5 — The Human Moment (Live Action)

| Property | Value |
|---|---|
| **Shot ID** | `SHOT-1.5` |
| **Timecode** | 0:14.000–0:17.500 |
| **Duration** | 3.500s |
| **Type** | Live action (24 fps) |

#### Frame Description

Live action. A developer at a desk. We see them from a **3/4 angle** (camera is positioned roughly 30–40° off the subject's center line, favoring the side nearest the monitor). **Medium close-up** framing: shoulders and head fill the frame. The subject is slightly off-center (rule of thirds — their eyes at the upper-third intersection point, looking toward the larger negative space).

The developer is lit **only by monitor glow** — deep blue/cyan light on their face, the rest of the room is dark (practical darkness, not post-production darkness). Their expression is not anger. Not panic. Not frustration. It is **quiet defeat**. The kind of moment where you don't even curse. You don't slam the desk. You just... stop. You close the laptop. Slowly. Deliberately. As if acknowledging that the last three hours are gone and there's nothing to be done.

**Action**: The developer slowly closes the laptop lid. The gesture takes approximately 2.0–2.5 seconds (not a slam — a resignation). As the lid closes, the monitor light on their face dims and disappears (the lid physically blocks the light source). By the final frames of the shot, the developer's face is in near-darkness — just enough ambient light to see their silhouette and the outline of their expression.

#### Camera

| Property | Value |
|---|---|
| Angle | 3/4 front (30–40° off center line) |
| Shot size | Medium close-up (MCU) — head and shoulders |
| Lens equivalent | 50mm (on full-frame sensor) |
| Aperture | f/2.8 (shallow depth of field) |
| Focus | Rack focus is not needed — maintain focus on the developer's face and hands throughout. Background is soft bokeh. |
| Movement | **None.** Fixed tripod. No pan, no tilt, no dolly, no zoom. The stillness of the camera mirrors the stillness of the moment. Any camera movement would add energy that contradicts the emotional beat (defeat, resignation). |
| Frame rate | **24 fps** — cinematic cadence, visually distinct from the 60fps motion graphics that precede and follow this shot. The subtle difference in motion texture signals "this is real" to the viewer's subconscious. |

#### Lighting

| Property | Value |
|---|---|
| Key light | The monitor itself (or a panel mimicking monitor light — see Technical Requirements). Positioned directly in front of the subject, below eye level (as a laptop screen would be). |
| Color temperature | 6500K (cold, blue-ish daylight equivalent). This is the color of a code editor in dark mode — the color every developer sees at 11pm. |
| Color tone | Blue-cyan cast. Not white, not warm. The face should look cold. |
| Fill | None. No fill light. The non-monitor side of the face falls to near-black. This is deliberately high-contrast, single-source lighting. |
| Practical lights | None in frame. No desk lamp, no RGB strips, no window light. The only light in this world is the screen. |
| Light animation | As the laptop lid closes (over ~2.0s), the light on the developer's face **dims progressively**. This should be achieved practically — the lid physically blocks the light source. By the end of the shot, the face is at approximately 10–15% of its starting brightness. Do NOT achieve this in post (it looks fake). |
| Background | Dark. If any background elements are visible (wall, shelf, second monitor), they should be at ≤5% brightness — deep in shadow. The bokeh from the f/2.8 aperture will soften any background detail. |

#### Audio

| Element | Description | Level | Technical Notes |
|---|---|---|---|
| Laptop closing | The mechanical click/thud of a laptop lid closing. A real, physical sound — metal and plastic meeting with a satisfying finality. | −22 dB | Record on set. Multiple takes (minimum 8). Choose the take with the cleanest, most satisfying mechanical click. The sound should have a brief transient (the click of the latch) followed by a soft thud (the lid settling). No reverb — this is a close, intimate sound. |
| Silence | After the laptop closes, silence. The room tone from the terminal shots is gone (it belonged to the "terminal world"). This is a different space. | Silence | The transition from "inhabited silence" (room tone in SHOTS 1.1–1.4) to "dead silence" (SHOT 1.5) creates an almost subliminal sense of loss. The ambient world has been extinguished along with the monitor light. |
| Music | None. | — | |
| Narrator | Does NOT begin in this shot. | — | The narrator's first words land at 0:17.500 (SHOT 1.6). Keeping SHOT 1.5 narrator-free preserves the visual storytelling and lets the human moment breathe. |

#### On-Screen Text

None. No overlay, no subtitle, no lower-third. Just the live action. The image speaks.

#### Cast Direction

| Property | Direction |
|---|---|
| **Age range** | Mid-20s to mid-40s. |
| **Ethnicity** | Any. Cast for authenticity and expression, not demographics. |
| **Gender** | Any. |
| **Appearance** | Must look like a **real developer**, not a model or actor playing "developer." Authentic. Slightly tired. Not disheveled — just... someone at the end of a long coding session. |
| **Wardrobe** | Hoodie (dark, muted — navy, charcoal, or black) OR a plain crew-neck t-shirt. No logos, no graphics, no branding. Nothing that dates the shot or draws attention from the face. |
| **Expression** | Quiet defeat. Not anger (no furrowed brow, no clenched jaw). Not panic (no wide eyes). Not sadness (no tears, no quivering lip). Just... **nothing**. The blankness of someone processing a loss. A micro-expression of exhaustion. The eyes might close for a beat as the laptop closes. |
| **Performance note** | The laptop close is the only action. It should feel inevitable — like gravity. The developer isn't making a decision to close the laptop; they're just... done. Direct the actor to think about a real moment of quiet frustration, not to "act frustrated." |

#### Set Design

| Property | Direction |
|---|---|
| **Desk** | A real desk — not pristine, not messy. The kind of desk a developer actually works at. Some wear and personality. |
| **Equipment** | A laptop (the primary prop — the one being closed). Optionally: a second monitor (off or showing a dark screen), a mechanical keyboard (visible but not prominent — ties back to the audio from SHOTS 1.1–1.4), a coffee mug (half-full — not a prop mug, a real one). |
| **Room** | Dark. The set should feel like a real home office or late-night workspace. Not a studio. Not a co-working space. If shooting on a set, dress it to look lived-in. A bookshelf with real books, a plant, a pair of headphones. Nothing draws focus — everything is in deep shadow and bokeh. |
| **Anti-direction** | No RGB lighting. No gaming peripherals. No multiple-monitor "battlestation." No standing desk in standing position. No trendy tech aesthetic. This is not aspirational — it's relatable. |

#### Transitions

| Direction | Type | Duration | Notes |
|---|---|---|---|
| **In** | Hard cut from SHOT 1.4 | 0ms | Terminal → face. The most dramatic visual shift in the Cold Open. 14 seconds of abstract text, then suddenly: a human. |
| **Out** | Soft cut to SHOT 1.6 | 2 frames (~83ms at 24fps) | A **2-frame dissolve** — almost a hard cut, but not quite. The slight softness bridges the transition from live action back to motion graphic without the jarring quality of a pure hard cut. At 24fps, this is barely perceptible — it registers as a "gentle" cut rather than a "hard" cut. In the 60fps timeline, this is ~5 frames. |

---

### SHOT 1.6 — Slack Message

| Property | Value |
|---|---|
| **Shot ID** | `SHOT-1.6` |
| **Timecode** | 0:17.500–0:19.500 |
| **Duration** | 2.000s |
| **Type** | Motion graphic (messaging UI simulation) |

#### Frame Description

A close-up of a messaging interface. **This is NOT Slack.** Do not use Slack's name, logo, colors, or distinctive UI patterns (e.g., the sidebar channel list, Slack's specific message bubble styling). Create a **generic team messaging UI** styled consistently with the W0rkTree video design system. The viewer will recognize it as "a work chat app" without any brand association.

The view is tight — we see only the message input area and the channel header. No sidebar, no user list, no unread badges. The developer is typing a message. This is shown as a motion graphic, not a screen recording.

#### Messaging UI Design Specification

| Element | Style |
|---|---|
| **Background** | `#111827` (matches terminal background — visual continuity) |
| **Channel header** | Top of frame. Channel name: `#backend` in the design system's `body` size, color `#6B7280`. A thin `1px` horizontal divider below the header in `#1F2937`. |
| **Message input area** | Bottom 40% of frame. Background: `#0A0F1A`. Border: `1px solid #374151`. Rounded corners: `8px`. The input field is centered in this area with `16px` internal padding. |
| **Typed text** | `#F0F0F0`, design system `body-lg` size, JetBrains Mono or the UI font from `01_DESIGN_SYSTEM.md`. |
| **Cursor** | Thin vertical bar, `#F0F0F0`, blinking at 530ms. |
| **Message content** | `hey... did anyone else push to main in the last hour?` |
| **Typing animation** | Characters appear one at a time at 45ms/char. **Exception**: the ellipsis (`...`) types slowly — **200ms per dot** — conveying hesitation. The developer is choosing their words carefully. They don't want to accuse. They don't want to admit what happened. The three dots are the most emotionally loaded punctuation in the entire video. |
| **Typing timing breakdown** | `h-e-y` = 3 chars × 45ms = 135ms. Then `.` at 200ms, `.` at 200ms, `.` at 200ms = 600ms for the ellipsis. Then `space` = 45ms. Then `d-i-d...h-o-u-r-?` = remaining 43 chars × 45ms = 1935ms. Total: 135 + 600 + 45 + 1935 = **2715ms**. This exceeds the shot duration — so typing begins immediately and the message is still being typed as we transition to SHOT 1.7. The message does NOT need to complete on screen. The viewer reads the beginning and fills in the rest. |

#### Audio

| Element | Description | Level | Technical Notes |
|---|---|---|---|
| Narrator V.O. | **Begins here.** First line: *"You know this feeling."* The narrator's voice is the first human voice in the video. It arrives 17.5 seconds in — an eternity in YouTube terms. But the audience has been hooked by the visuals. The voice is a reward, not a crutch. | −12 dB LUFS | See [Dialogue](#dialogue) section for delivery notes. |
| Soft keyboard | Laptop-style keyboard sounds, synced to the message typing. A completely different texture from the mechanical keyboard in SHOTS 1.1–1.4 — softer, more muffled, shorter transient. Scissor-switch or butterfly-mechanism feel. | −24 dB | Quieter than the terminal keystrokes. The messaging keyboard is background texture, not foreground. It should be perceptible but not compete with the narrator. |
| Music | None. | — | |

#### On-Screen Text

Only the messaging UI content described above. No additional overlays.

#### Transitions

| Direction | Type | Duration | Notes |
|---|---|---|---|
| **In** | Soft cut from SHOT 1.5 | 2 frames | See SHOT 1.5 transition out. |
| **Out** | Dim to SHOT 1.7 | 500ms | The messaging UI dims (opacity fades from 100% to 0% over 500ms) as the narrator continues speaking. This is the only dissolve-type transition in the Cold Open besides the 2-frame soft cut, and it serves a specific purpose: it de-emphasizes the visual to shift attention entirely to the narrator's voice. |

---

### SHOT 1.7 — Narrator Over Black

| Property | Value |
|---|---|
| **Shot ID** | `SHOT-1.7` |
| **Timecode** | 0:19.500–0:25.000 |
| **Duration** | 5.500s |
| **Type** | Black screen (or near-black with residual messaging UI at ≤5% opacity) |

#### Frame Description

Cut to black. Or, optionally, the messaging UI from SHOT 1.6 remains at extremely low opacity (3–5%) as a ghost image — a memory of the screen that was just there, fading. The visual is deliberately minimal. This shot is **entirely about the voice**. The viewer's eyes have nothing to latch onto, which forces all attention to the narrator's words.

There are two acceptable visual treatments. Choose in edit based on what feels better:

1. **Pure black** (`#000000`) — clean, final, matches the opening of the video (bookend structure).
2. **Ghost image** — the messaging UI at 3–5% opacity, gradually fading to `#000000` over the duration of the shot. Creates a more cinematic "memory dissolve" feel.

In either case, there is no text on screen. No lower-third. No title. Just darkness and a voice.

#### Camera

N/A.

#### Lighting

N/A.

#### Audio

| Element | Description | Level | Technical Notes |
|---|---|---|---|
| Narrator V.O. | The narrator delivers the final lines of the Cold Open. See [Dialogue](#dialogue) for the exact text and delivery direction. | −12 dB LUFS | The narrator's voice is the only sound. There is nothing else. No room tone. No keyboard. No ambience. The voice exists in a void — intimate, close, as if speaking directly into the viewer's ear. |
| Room tone | None. | — | The absence of room tone in SHOTS 1.5–1.7 (versus its presence in 1.1–1.4) is a deliberate audio design choice. The terminal world had physical space (room tone = a room). The human moment and narration exist in a psychological space (no room tone = inside your head). |
| Music | None. | — | |

#### On-Screen Text

None.

#### Narrator Direction for This Shot

This is the most important vocal delivery in the entire video. The final line of the Cold Open — *"And the worst part? Everyone just accepts it. Like this is normal. Like this is fine."* — is what the viewer carries into the Title Card. It must be delivered with:

- **Volume**: Quiet. Not whispered — just low. Conversational. The narrator is not on a stage; they're sitting across a table from you.
- **Pace**: Slow. Each sentence gets space. There is air between *"Like this is normal"* and *"Like this is fine."* At least 400ms of silence between them.
- **Tone**: Resigned. Tired. The same emotional register as the developer who just closed their laptop. The narrator isn't angry. They aren't incredulous. They're stating a fact. And the fact is absurd. There might be the ghost of a dry smile behind *"Like this is fine"* — a callback to the "This Is Fine" meme, but it is **never** explicit. If the viewer catches it, great. If they don't, the line works on its own.
- **Anti-direction**: Not dramatic. Not solemn. Not "movie trailer voice." Not sarcastic. Not condescending. Think: a senior engineer at a bar after a long week, saying this to another engineer who just told their own Git horror story. Empathy, not performance.

#### Transitions

| Direction | Type | Duration | Notes |
|---|---|---|---|
| **In** | Dim from SHOT 1.6 | 500ms | See SHOT 1.6 transition out. |
| **Out** | Hard cut to black (held 300ms) → Title Card | 0ms + 300ms hold | At the end of the narrator's last word, hard cut to pure black (if not already there). Hold `#000000` for exactly **300ms**. This 300ms of held black is a breath — a palate cleanser between the emotional Cold Open and the confident Title Card. Then the Title Card sequence begins (see `03_SCENE_TITLE_CARD.md`). |

---

## Dialogue

### Full Narrator Script — Scene 1

The narrator speaks for **7.5 seconds** of the 25-second Cold Open (0:17.500–0:25.000). The first 17.5 seconds are narration-free by design.

#### Lines

**Block 1** (0:17.500–0:21.500, approximately 4.0s):

> "You know this feeling. That moment where you're not building software anymore — you're fighting your own tools. Three hours of work. Gone. Not because of a bug in your code. Because of a bug in your workflow."

**Block 2** (0:21.500–0:25.000, approximately 3.5s):

> "And the worst part? Everyone just accepts it. Like this is normal. Like this is fine."

#### Word Count & Pacing Validation

- Block 1: 38 words in ~4.0s = **9.5 words/second** (natural conversational pace; target range is 8–11 wps)
- Block 2: 19 words in ~3.5s = **5.4 words/second** (deliberately slow — each word lands with weight)
- Total: 57 words in 7.5s = **7.6 wps average** (well within comfortable range; leaves room for pauses)

#### Line-by-Line Delivery Notes

| Line | Delivery Direction |
|---|---|
| *"You know this feeling."* | Conversational. Direct. Like talking to a colleague over coffee. Not dramatic. Not a question — a statement. The viewer does know this feeling. The narrator is acknowledging a shared experience. A slight nod in the voice. |
| *"That moment where you're not building software anymore —"* | The em-dash is a breath, not a full stop. The sentence flows into the next thought. Pace: natural, unhurried. |
| *"you're fighting your own tools."* | A slight emphasis on **"own"**. The tools are supposed to help you. They're yours. And they're the enemy. The irony is understated. |
| *"Three hours of work."* | **Pause before.** Let the previous thought settle (300ms). Then this phrase, delivered simply. No vocal weight — just the fact. Let the number do the work. **Pause after** (400ms). |
| *"Gone."* | Flat. Final. One word. No rising inflection. No falling inflection. Dead-level. Like a period at the end of a sentence — not a exclamation point, not a question mark. A period. |
| *"Not because of a bug in your code."* | Normal pace resumes. This is setup for the payoff. |
| *"Because of a bug in your workflow."* | The word **"workflow"** should be slightly emphasized — not louder, but with a micro-pause before it (150ms) and a marginally slower delivery of the word itself. This is the *reframe*: Git isn't a tool that sometimes fails. Git *is* the failure. The word "workflow" replaces "Git" intentionally — it's bigger than one tool, it's the entire system. |
| *"And the worst part?"* | Conversational aside. The kind of thing you say when you're about to tell someone the punchline of a bad joke. Slightly lower pitch than the previous line. |
| *"Everyone just accepts it."* | **Slow.** Each word gets space. "Everyone" — beat — "just" — beat — "accepts it." The word "just" is the knife. It implies passivity, resignation, learned helplessness. |
| *"Like this is normal."* | Measured. A statement of the absurd framed as observation. |
| *"Like this is fine."* | The slightest hint of irony. A microsecond of warmth in the voice — not a smile, not a laugh, just... awareness that this phrase means something beyond its literal content. The "This Is Fine" meme lives in every developer's subconscious. The narrator is not referencing it. They're just saying words that happen to resonate. If the viewer catches it: a moment of recognition. If they don't: the line still works as resigned observation. |

#### Narrator Casting Brief (Scene-Specific)

This supplements the casting notes in `10_AUDIO_SPEC.md` with Cold-Open-specific requirements:

- **Voice type**: Mid-range. Not deep bass (too dramatic), not high tenor (too energetic). A voice that sounds like it belongs to someone who writes code for a living.
- **Accent**: Neutral. No strong regional accent. The voice should be accessible to a global English-speaking audience.
- **Quality**: Warm but not smooth. Some texture — not a polished radio voice. Think: a tech conference keynote speaker, not a podcast host.
- **Recording note**: Record the Cold Open lines separately from the rest of the script. The energy is completely different. The Cold Open narrator is tired and empathetic. By Act III, the narrator is energized and forward-looking. These are two different emotional registers, and recording them in the same session risks blending the energy. Record Cold Open lines first, at the beginning of the session, before the voice warms up into its confident register.

---

## Audio Design

### Foley & SFX

| Timecode | Sound | Source | Level | Duration | Technical Notes |
|---|---|---|---|---|---|
| 0:00.000–0:14.000 | Mechanical keyboard (Cherry MX Blue or equivalent) | Foley recording — real keyboard, real room | −18 dB | 14.0s | Record at 48kHz/24-bit with a condenser mic (e.g., AKG C414, Neumann KM184) 15–20cm from keyboard, slightly off-axis. Capture 90+ seconds of continuous typing. Select and edit segments for each shot. Keystroke velocity must have natural variation — no two keystrokes at identical levels. For SHOT 1.4, increase velocity by 2–3 dB to convey frustration. |
| 0:00.000–0:12.600 | Room tone (HVAC / ambient hum) | Foley recording — real room with computer running | −40 dB | 12.6s | Record 60 seconds of room tone in the same room as the keyboard recording. This tone is the "space" the terminal exists in. Broadband below 500Hz, gentle roll-off above. Must be a consistent, flat, loopable bed with no transients or variations. |
| 0:07.700 | Sub-bass hit (40Hz pulse) | Sound design — synthesized | −30 dB | 400ms | 40Hz sine wave with gentle 2nd harmonic (80Hz at −40 dB). Attack: 10ms. Decay: 200ms exponential. The hit fires on the exact frame that `CONFLICT` appears in SHOT 1.3. On systems without sub-bass reproduction (laptops, phone speakers), this will be inaudible — that is acceptable and intentional. It is a physical sensation for headphone/monitor listeners, not a conscious sound effect. |
| 0:12.600 | Room tone fadeout | Sound design — automated fade | −40 dB → −∞ | 200ms | Exponential decay. Begins on the exact frame that `HEAD is now at...` appears. By 0:12.800, the room is dead silent. |
| 0:12.800–0:14.000 | Absolute silence | — | −∞ | 1.200s | No audio content whatsoever. Not even dithering noise. True digital silence. This is the emotional peak. |
| 0:14.000–0:14.800 | Ambient settle (live-action room) | Production recording — on set | −45 dB | ~800ms | The barely perceptible ambience of the live-action set. Different from the "terminal room tone" — this is a physical room with a human in it. Very quiet. Serves as a subliminal transition between the terminal world and the human world. Fades in over the first 400ms of SHOT 1.5. |
| 0:15.500 (approx.) | Laptop lid closing | Foley recording — on set | −22 dB | ~300ms | Record on set with the actual laptop used as a prop. Record 8+ takes at varying close speeds. Select the take with the cleanest mechanical click. The sound should have: (1) a brief transient click from the magnetic latch engaging, and (2) a soft thud from the lid settling against the body. No post-processing reverb. Close-mic (lavalier on the laptop or boom at 30cm). |
| 0:16.000–0:17.500 | Silence (post-laptop) | — | −∞ | ~1.500s | Dead silence after the laptop closes. The ambient settle from the live-action room fades out with the monitor light. |
| 0:17.500–0:19.500 | Soft keyboard (laptop / scissor-switch) | Foley recording | −24 dB | 2.0s | Different keyboard texture from the mechanical switches. Softer, muffled, shorter transient. Record from a MacBook keyboard or similar thin-travel mechanism. Synced to the messaging UI typing. Quieter than the terminal keystrokes (−24 dB vs −18 dB) to sit beneath the narrator V.O. |
| 0:17.500–0:25.000 | Narrator V.O. | Voice recording — studio | −12 dB LUFS | 7.5s | See [Dialogue](#dialogue) for full script and delivery notes. Record in a treated vocal booth. Mic: large-diaphragm condenser (e.g., Neumann U87, TLM 103). Minimal processing: gentle high-pass at 80Hz, light compression (2:1 ratio, slow attack). No de-essing beyond what's necessary for sibilance control. The voice should sound natural, not processed. |

### Music

**There is no music in the Cold Open.**

This is not an oversight. It is the single most important audio design decision in the scene. The absence of music creates:

1. **Intimacy.** Music creates distance — it tells the viewer "you are watching a production." Silence tells the viewer "you are *in* this moment."
2. **Tension.** The human brain expects music in video content. When it doesn't arrive, the brain stays alert, waiting. This is free engagement.
3. **Contrast.** The first music cue in the entire video is the synth tone on the Title Card at 0:25. After 25 seconds of no music, even a single sustained note will feel enormous. The Cold Open's silence is the setup for that payoff.

If a music supervisor or editor suggests adding "just a subtle pad" or "a low drone" to the Cold Open: **no**. The silence is the music.

---

## Technical Requirements

### Live Action (SHOT 1.5)

| Requirement | Specification |
|---|---|
| **Camera body** | Any cinema camera or high-end mirrorless capable of 24fps in LOG/RAW. Recommended: RED Komodo, Sony FX6, Sony FX3, Blackmagic Pocket Cinema Camera 6K, Canon R5C. A DSLR/mirrorless shooting H.264/H.265 internally is acceptable only if shooting in LOG profile (S-Log3, C-Log3, BRAW, etc.). |
| **Frame rate** | **24.000 fps** (not 23.976 — use true 24p for simplicity in the 60fps timeline). The cinematic cadence is visually distinct from the 60fps motion graphics and signals "this is real" to the viewer. |
| **Recording format** | LOG or RAW. ProRes 422 HQ minimum for intermediate. RAW (BRAW, R3D, ProRes RAW) preferred for maximum color grading flexibility. The monitor-glow lighting setup has extreme dynamic range (bright face vs. black room) that will crush in a standard Rec. 709 recording. |
| **Lens** | 50mm equivalent (full-frame). A prime lens is preferred for maximum sharpness at f/2.8. Recommended: Canon EF 50mm f/1.4, Sony FE 55mm f/1.8, Sigma Art 50mm f/1.4. |
| **Aperture** | f/2.8. Shallow depth of field isolates the subject from the background. The background should be a wash of soft, dark bokeh — recognizable as "a room" but with no sharp detail. |
| **Monitor light source** | The developer's face is lit by monitor glow. **Preferred method**: Use an actual monitor positioned just off-camera, displaying a full-screen blue-white gradient (#111827 to #1E3A5F) at maximum brightness. Place it at the exact position a laptop screen would occupy relative to the subject. This produces authentic, directional, flickering-free monitor light. **Alternative method**: A small LED panel (e.g., Aputure MC, Nanlite PavoSlim) set to 6500K at low power (10–20%), positioned at laptop-screen distance and angle. This allows more control but may look less natural. The preferred method produces subtler light gradients on the face because the monitor's light distribution is non-uniform (brighter center, dimmer edges), which reads as more realistic. |
| **Color temperature** | 6500K. Cold, blue. Not warm. Not neutral. The face should have a distinct blue-cyan cast that reads as "screen light in a dark room." In color grading, push the shadows slightly toward blue-cyan (H: 200°, S: 15%). |
| **Room setup** | A real desk setup or a set dressed to look like one. See SHOT 1.5 Set Design section above. Black out all windows. Kill all ambient light sources. The only light in the room should be the monitor/panel simulating monitor glow. Test by turning off the monitor — the room should be effectively pitch black. |
| **Room tone recording** | Record **30 seconds of silence** in the room with all equipment powered on (camera, monitor, any computers, HVAC). This is the live-action room tone used in SHOT 1.5. Record at the same mic position and gain settings used for the laptop close foley. |
| **Laptop close foley** | Record the laptop close as a **separate foley pass** after the performance takes. Use the same mic setup. Open and close the laptop 8–12 times at varying speeds and forces. Slate each take. The editor will select the best take independently from the visual performance. |
| **Wardrobe** | See SHOT 1.5 Cast Direction. Pre-approve wardrobe in a camera test to ensure it reads correctly under the blue monitor light (some fabric colors shift unexpectedly under narrow-spectrum LED/monitor light). |
| **Makeup** | Minimal. Anti-shine only. The developer should look like themselves, not "made up." A light mattifying powder to control forehead/nose shine under the monitor light is acceptable. No foundation, no contouring, no color correction. |
| **Take count** | Shoot minimum **15 takes** of the laptop-close action. The performance is subtle — the difference between "acting resigned" and "being resigned" is one take away. Direct the actor to think about different personal frustrations on different takes to get variation. |

### Motion Graphics (SHOTS 1.1–1.4, 1.6)

| Requirement | Specification |
|---|---|
| **Canvas** | 3840×2160 (4K UHD) |
| **Frame rate** | 60 fps |
| **Background color** | `#111827` for terminal shots (SHOTS 1.2–1.4). `#000000` for SHOT 1.1. `#111827` for messaging UI (SHOT 1.6). |
| **Terminal component** | Use the terminal component specification from `01_DESIGN_SYSTEM.md`. If the design system terminal spec is not yet finalized, use: JetBrains Mono 400/700 at 24px (at 4K), line-height 1.6, `#111827` background, no window chrome, no scrollbar, `#F0F0F0` default text, blinking cursor (thin vertical bar, 530ms interval). |
| **Text rendering** | All terminal and UI text must be rendered as **vector text** (not rasterized images of text). This ensures pixel-perfect sharpness at any resolution. Anti-aliasing: subpixel (if the renderer supports it) or grayscale. |
| **Typing animation** | Characters appear one at a time at the specified rate (40ms/char for terminal, 45ms/char for messaging UI). Each character appears on a single frame (not faded in). The cursor advances with each character. This is a **text reveal**, not a fade — it mimics how a real terminal renders typed input. |
| **Output animation** | Command output appears **line by line** (not character by character). Each line appears fully formed on a single frame, with the specified inter-line delay (50ms). This mimics how a real terminal renders command output — the program writes a full line to stdout, and the terminal renders it at once. |
| **Cursor behavior** | Blinking thin vertical bar, `#F0F0F0`. Blink rate: 530ms on, 530ms off. During typing, cursor is solid (not blinking) and advances with each character. When typing stops, cursor resumes blinking after a 530ms delay. |
| **Easing** | Per Production Bible Editorial Principle #8: all motion (except text reveal, which is instant) uses `cubic-bezier(0.4, 0, 0.2, 1)`. In the Cold Open, this applies primarily to the messaging UI element transitions in SHOT 1.6 and the dim-to-black in SHOT 1.7. |
| **Color accuracy** | All hex values in this document are authoritative for this scene. If they conflict with `01_DESIGN_SYSTEM.md`, the design system wins (per Production Bible hierarchy). Verify by sampling rendered frames — encoded output must match specified hex values within ±2 on each RGB channel. |

---

## Terminal Typography & Color Reference

This section consolidates every text style used in the Cold Open terminal shots for quick reference during production.

### Font Stack

| Priority | Font | Fallback Use |
|---|---|---|
| 1 | JetBrains Mono | Primary — all terminal and code text |
| 2 | Fira Code | Fallback if JetBrains Mono licensing is unavailable |
| 3 | Source Code Pro | System fallback |

### Size & Spacing

| Property | Value | Notes |
|---|---|---|
| Font size | 24px at 4K (3840×2160) | Scales to 12px at 1080p. Equivalent to `code-lg` in the design system. |
| Line height | 1.6 (38.4px at 4K) | Generous line height for readability. Terminal output should breathe. |
| Letter spacing | 0 (default monospace) | Do not adjust tracking. Monospace fonts are designed for 0 letter-spacing. |
| Tab width | 4 spaces | Not used in the Cold Open, but defined for consistency. |

### Complete Color Map

| Token | Hex | Opacity | Weight | Used For |
|---|---|---|---|---|
| `terminal-bg` | `#111827` | 100% | — | Terminal background |
| `black` | `#000000` | 100% | — | SHOT 1.1 and 1.7 backgrounds |
| `prompt` | `#6B7280` | 100% | 400 | `$` prompt character, `hint:` prefix |
| `text-primary` | `#F0F0F0` | 100% | 400 | Command text, primary output |
| `text-secondary` | `#F0F0F0` | 80% | 400 | Supporting output lines |
| `text-tertiary` | `#F0F0F0` | 60% | 400 | Muted output (hint text, damage-done lines) |
| `danger` | `#FF3B3B` | 100% | 700 | `[rejected]`, `error:`, `CONFLICT`, destructive commands |
| `danger-muted` | `#FF3B3B` | 60% | 400 | Post-catastrophe output (`HEAD is now at...`) |
| `accent-cyan` | `#00D4FF` | 100% | 400 | File paths in output (`src/auth/oauth.rs`) |
| `cursor` | `#F0F0F0` | 100% | — | Terminal cursor (thin vertical bar) |
| `msg-bg` | `#0A0F1A` | 100% | — | Messaging UI input field background |
| `msg-border` | `#374151` | 100% | — | Messaging UI input field border |
| `msg-channel` | `#6B7280` | 100% | 400 | Channel name in messaging UI header |

---

## Editorial Notes

These are high-level observations and constraints for the editor, director, and post-production team. They supplement the per-shot specifications above.

### 1. Sound-Off Legibility Is Non-Negotiable

The Cold Open must work **with sound off** for the first 10 seconds (0:00–0:10). A viewer scrolling through YouTube or a social media feed with muted audio should be hooked entirely by the terminal text. They can read:

- `git push origin main` — familiar command
- `[rejected]` — something went wrong
- `CONFLICT` — it's getting worse
- `git reset --hard HEAD~3` — in red, clearly bad
- `initial commit` — everything is gone

This is a complete story told in text. No narration, SFX, or music is required for comprehension. The audio enriches the experience for viewers with sound on; it is not load-bearing for the first 10 seconds.

**Testing**: Play the Cold Open at 0:00–0:10 on mute for five developers who have not seen the project. Ask them to describe what happened. If any of them cannot articulate "someone lost their work because of a Git mistake," the terminal text is not clear enough.

### 2. Live Action Fallback

If SHOT 1.5 (live action) does not work in testing — whether due to budget constraints, casting issues, or the shot simply not landing emotionally — it can be replaced with a **continued terminal scene**. In this fallback:

- The developer does not appear on camera.
- After `initial commit`, the terminal clears.
- A new prompt appears. The developer types the Slack/messaging text directly into the terminal (as if it were a different application). Same hesitant typing, same slow ellipsis.
- The emotional beat (empathy/human connection) is carried entirely by the typing rhythm and the message content.

This fallback is functional but **significantly weaker** than the live-action version. The live action breaks the "motion graphics" visual pattern that has been established for 14 seconds, and it humanizes the Cold Open in a way that terminal text cannot. Live action is **strongly preferred** and should only be abandoned if it genuinely doesn't work.

### 3. Pacing Check

The total Cold Open is 25 seconds. In the final edit, it should **feel like 10**. If the pacing feels slow at any point, the editor may:

- Tighten SHOT 1.2 (reduce the 500ms execution pause to 300ms)
- Tighten SHOT 1.3 (reduce the 500ms execution pause to 300ms)
- Reduce inter-line output delay from 50ms to 30ms

The editor must **never**:

- Speed up the typing rate (40ms/char is the minimum for legibility and realism)
- Shorten the silence in SHOT 1.4 (the 1.2-second silence after `initial commit` is sacred)
- Shorten SHOT 1.5 (the human moment needs its full duration to land)
- Add music to "fill dead air" (the silence IS the design)

### 4. Subtitle Considerations

When generating subtitle files (SRT/VTT) per the Production Bible delivery specs:

- Terminal text is **not subtitled**. It is on-screen text, not dialogue. Subtitling it would be redundant and clutter the frame.
- The Slack/messaging text is **not subtitled** for the same reason.
- Narrator V.O. (0:17.5–0:25.0) **is subtitled**. Begin subtitle at the first word, end at the last word plus 1 second of hold.
- Sound descriptions for accessibility: `[keyboard typing]` at 0:00, `[silence]` at 0:13, `[laptop closing]` at 0:15. These appear in the VTT file only (not SRT) per standard accessibility practice.

### 5. The "Initial Commit" Detail

The output `HEAD is now at 7a2c1e0 initial commit` contains a specific commit hash (`7a2c1e0`) and the message `initial commit`. Both of these are deliberate choices:

- **The hash** (`7a2c1e0`): A plausible short-SHA. It should look real. Do not use `1234567` or `abcdefg` or any obviously fake hash.
- **The message** (`initial commit`): This is the standard first commit message in most repositories. Every developer recognizes it. Its presence in the `reset --hard` output means the developer has been reset to the very beginning of their project's history. Everything after the first commit — every feature, every fix, every hour of work — is gone. This is the detail that transforms "I lost some work" into "I lost *everything*."

If the commit message were something like `Fix typo in README`, the emotional impact would be 90% weaker. `initial commit` is the only correct choice.

### 6. Frame-Accurate Timing Reference

For the editor's reference, here is the complete timing grid with frame numbers at 60fps:

| Event | Timecode | Frame # (60fps) |
|---|---|---|
| Video start (black) | 0:00.000 | 0 |
| Terminal appears (SHOT 1.2) | 0:02.500 | 150 |
| `$ git push origin main` typing begins | 0:02.500 | 150 |
| Typing completes | 0:03.380 | 203 |
| Execution pause ends, output begins | 0:03.880 | 233 |
| All output visible | 0:03.980 | 239 |
| SHOT 1.3 begins | 0:06.000 | 360 |
| `$ git pull --rebase origin main` typing begins | 0:06.000 | 360 |
| Typing completes | 0:07.200 | 432 |
| `CONFLICT` output appears (+ sub-bass hit) | 0:07.700 | 462 |
| All output visible | 0:07.750 | 465 |
| SHOT 1.4 begins | 0:10.000 | 600 |
| `$ git rebase --abort` typing begins | 0:10.000 | 600 |
| Typing completes | 0:10.800 | 648 |
| `$ git reset --hard HEAD~3` typing begins | 0:11.100 | 666 |
| Typing completes | 0:12.100 | 726 |
| `HEAD is now at...` output appears | 0:12.600 | 756 |
| Room tone fadeout complete / silence begins | 0:12.800 | 768 |
| SHOT 1.5 begins (live action) | 0:14.000 | 840 |
| Laptop close (approx.) | 0:15.500 | 930 |
| SHOT 1.6 begins (messaging UI) | 0:17.500 | 1050 |
| Narrator V.O. begins | 0:17.500 | 1050 |
| SHOT 1.7 begins (black / near-black) | 0:19.500 | 1170 |
| Narrator final word | ~0:24.700 | ~1482 |
| Hard cut to held black | 0:24.700 | ~1482 |
| Held black ends / Title Card begins | 0:25.000 | 1500 |

---

## Continuity & Handoff

### Into Scene 1

There is no preceding scene. The Cold Open is frame 0 of the video. The first frame is `#000000` black.

### Out of Scene 1

The Cold Open ends at 0:25.000 with a **300ms held black**. The Title Card sequence (documented in `03_SCENE_TITLE_CARD.md`) begins immediately after. The handoff:

- **Visual**: Pure black (`#000000`) → Title Card black background holds for 500ms → W0rkTree logo fades in.
- **Audio**: Dead silence during the 300ms hold → single sustained synth tone (C3, warm pad) begins with the logo fade-in.
- **Emotional**: Resignation (Cold Open) → 300ms void → Anticipation (Title Card). The held black is a breath. The viewer's emotional state at the end of the Cold Open is resignation. The Title Card's job is to transform that resignation into curiosity. The 300ms of black is the pivot point.

### Cross-References

| Document | Relevance to Scene 1 |
|---|---|
| `00_PRODUCTION_BIBLE.md` | Master authority. Narrative arc beat 1 (Empathy). Master Timeline rows 0:00–0:25. Editorial Principles (all apply; #7 is Cold-Open-specific). |
| `01_DESIGN_SYSTEM.md` | Terminal component spec. Font definitions. Color palette (hex values). Spacing scale. |
| `03_SCENE_TITLE_CARD.md` | Receives handoff from Cold Open. The 300ms black hold is the transition seam. |
| `09_ANIMATION_COMPONENTS.md` | Terminal typing animation component. Cursor blink component. Text reveal component. |
| `10_AUDIO_SPEC.md` | Narrator casting brief. Foley recording requirements. Loudness standards (−14 LUFS integrated). |
| `11_ASSET_MANIFEST.md` | Lists all assets needed for Scene 1: keyboard foley recordings, room tone recordings, laptop close foley, sub-bass sound design file, live-action footage, narrator V.O. recording. |

---

*This document is the authoritative specification for Scene 1 (Cold Open) of the W0rkTree launch video. In any conflict between this document and the Production Bible (`00_PRODUCTION_BIBLE.md`), the Production Bible wins. In any conflict between this document and a scene-level peer document, this document is authoritative for its own timecode range (0:00.000–0:25.000) only. Questions, clarifications, and change requests are routed through the production lead.*