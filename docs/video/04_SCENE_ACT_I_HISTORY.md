# Scene 3 — Act I: How We Got Here

**Timecode**: 0:30–1:30
**Duration**: 60 seconds
**Purpose**: Establish context. Tell Git's origin story with respect. Then pivot: the world changed, Git didn't.
**Emotional Arc**: Nostalgia → Respect → Realization → Frustration

---

## SEGMENT BREAKDOWN

Divide this 60-second act into 4 segments, each with full detail:

---

### SEGMENT 3A — The 2005 Crisis (0:30–0:42, 12 seconds)

**Narrator**: "April 2005. Linus Torvalds is mass-emailing the Linux kernel mailing list. BitKeeper — the version control system the kernel team had been using for free — just revoked their license. The entire Linux development workflow is about to collapse."

**Visual Concept**: Stylized, desaturated illustration style. NOT real archival footage (rights issues). Think editorial illustration with a limited color palette — sepia/warm tones (#C4956A, #8B6E4E) over the Deep Navy background. This warm palette will contrast sharply with the cool cyan of W0rkTree later, reinforcing "old world vs. new world."

**Shot-by-shot**:

#### SHOT 3A.1 — Timeline Anchor (0:30.000–0:32.000)

- A horizontal timeline materializes across the center of frame
- Line: 2px, #374151, draws left-to-right over 600ms
- A single node appears on the timeline with a bounce: labeled "2005" in `heading-1`, #F0F0F0
- The node is a circle (24px diameter), fill #C4956A (sepia/warm accent)
- Below the node, "April" fades in, `body`, #6B7280

#### SHOT 3A.2 — Illustrated Linus (0:32.000–0:36.000)

- Above the timeline, an illustrated portrait of a figure at a computer appears (stylized, NOT a photo of Linus — avoid likeness rights issues). The figure is drawn in a warm, limited palette (sepia tones + Deep Navy).
- The illustration is a line drawing with selective fill — minimal, editorial style
- Email icons (envelope shapes) radiate outward from the figure, suggesting mass communication
- Animation: illustration draws itself on (line-draw reveal, left to right, 1500ms, ease-standard)

#### SHOT 3A.3 — BitKeeper → Collapse Visual (0:36.000–0:42.000)

- A "BitKeeper" text label appears, then cracks and crumbles apart (a break-apart animation — the text fragments into pieces that fall with gravity physics, 800ms)
- Below the crumbling text, the word "REVOKED" stamps on in Warning Red (#FF3B3B) with a percussive hit
- The timeline now shows an arrow from "2005" pointing forward, suggesting what comes next
- The email icons from the illustrated figure accelerate, becoming frantic

---

### SEGMENT 3B — Git's Creation (0:42–0:55, 13 seconds)

**Narrator**: "So Linus does what Linus does. He builds his own. In ten days, he writes the first version of Git. A distributed version control system designed for one very specific job: managing the Linux kernel's source code across thousands of contributors, none of whom trust each other."

**Visual Concept**: The timeline advances. A new node appears: "Git" with a green (#00C48F) fill. The visuals shift to representing Git's architecture in an abstracted, respectful way.

**Shot-by-shot**:

#### SHOT 3B.1 — "10 Days" Counter (0:42.000–0:45.000)

- A counter rapidly counts from "Day 1" to "Day 10" in `display-lg` size, centered above the timeline
- Each day number snaps in with ease-sharp timing (100ms per number)
- On "Day 10", the Git logo (or a stylized "Git" wordmark in the sepia palette) appears below the counter
- SFX: A subtle "tick" sound on each day increment (clock tick, very quiet, -30dB)

#### SHOT 3B.2 — Kernel File Tree (0:45.000–0:50.000)

- The timeline shrinks and moves to the top of frame
- A file tree visualization grows from the center of the screen
- Starts as a single root node, then branches rapidly — dozens, then hundreds, then thousands of files
- The tree uses the sepia warm palette for nodes, with thin #374151 lines connecting them
- Animation: exponential growth — slow at first, then cascading (use ease-enter curve, total duration 3000ms)
- Small avatar circles (just colored dots, 8px) flow toward the tree from all edges of the frame — representing contributors from around the world. Each avatar leaves a faint trail line.

#### SHOT 3B.3 — Branch Web (0:50.000–0:55.000)

- The file tree transforms (morphs, 600ms) into a branch/merge diagram — lines splitting and recombining
- This represents Git's branching model — cheap branches splitting off and merging back
- Color: branches are #C4956A (warm), merge points glow briefly with #00C48F
- The diagram is dynamic — branches keep splitting and merging, alive, organic
- At its peak complexity, hold for 1 second so the viewer absorbs the intricacy

---

### SEGMENT 3C — "It's Not 2005 Anymore" (0:55–1:10, 15 seconds)

**Narrator**: "And for that job? Git was brilliant. Truly... But here's the thing nobody wants to say out loud. It's not 2005 anymore."

**Visual Concept**: The warm palette COOLS. The sepia tones drain out of the frame, replaced by modern cool tones. The branch diagram stiffens, stops moving, becomes static and brittle-looking. The "2005" on the timeline cracks and ages.

**Shot-by-shot**:

#### SHOT 3C.1 — Color Temperature Shift (0:55.000–1:00.000)

- Over 3 seconds, the warm sepia palette desaturates. Colors shift from warm (#C4956A) to cool (#6B7280)
- The branch diagram freezes — no more organic movement
- The "2005" label on the timeline begins to visually age: hairline cracks appear in the text, the warm fill color drains to gray
- This is a slow, almost subliminal transition — the viewer should feel the shift before they notice it
- Narrator says "But here's the thing nobody wants to say out loud." during this transition — the pause before the revelation

#### SHOT 3C.2 — The Year Crack (1:00.000–1:05.000)

- On "It's not 2005 anymore" — the "2005" text cracks and crumbles (same break-apart animation as BitKeeper, but slower, more dramatic — 1200ms)
- The pieces fall away
- Behind it, the current year fades in: "2025" in `display-lg`, #F0F0F0, clean and sharp — starkly modern against the now-gray, static branch diagram
- SFX: A clean, low-pitched "crack" sound when 2005 breaks. Then a very subtle synth pad sustains (the first hint of the documentary underscore building)

#### SHOT 3C.3 — Modern Montage (1:05.000–1:10.000)

- Quick montage of modern dev tool icons/illustrations: VS Code logo shape, Slack logo shape, a Kubernetes wheel, a CI/CD pipeline diagram, a Docker container
- Each icon appears in a grid layout, fading in staggered (150ms apart), in the modern cool palette (#6B7280 line art on #0A0F1A)
- The narrator says: "Git was designed for the Linux kernel. Not for your microservices monorepo..." during this montage
- The icons fill the frame, surrounding the frozen 2005-era branch diagram — visually showing how the world grew around Git while Git stayed the same

---

### SEGMENT 3D — The Tax We Pay (1:10–1:30, 20 seconds)

**Narrator**: "Not for your fifty-person team on four continents. Not for your designer who just wants to save their work without accidentally deleting the production branch. Git solved Linus's problem. And then the entire industry adopted it — and inherited all of its assumptions. Twenty years later, we're still paying that tax. Every single day."

**Visual Concept**: Stack Overflow montage. Real questions (paraphrased to avoid copyright, but recognizable). Scrolling, overlapping, building into a visual cacophony of confusion.

**Shot-by-shot**:

#### SHOT 3D.1 — Stack Overflow Wall (1:10.000–1:22.000)

- Cards styled like Q&A site questions begin appearing:
  - Card style: #111827 background, 1px #374151 border, 16px border-radius, 32px padding
  - Question text: `body-lg`, #F0F0F0
  - Vote count: a number in #6B7280, left side
  - Orange accent line on left (Stack Overflow nod): 4px, #FF8A3B
- Questions appear staggered, from different positions, building into a wall:
  - "What does 'detached HEAD' mean?" (vote: 2,847)
  - "How do I undo a git rebase?" (vote: 4,102)
  - "I ran git reset --hard and lost everything" (vote: 3,291)
  - "Why does git checkout do five different things?" (vote: 1,756)
  - "What's the difference between git pull and git fetch?" (vote: 5,892)
  - "How to undo last commit without losing changes?" (vote: 6,134)
  - "git push rejected non-fast-forward" (vote: 3,847)
  - "Accidentally committed to wrong branch" (vote: 2,103)
- Cards scroll upward, continuously, faster and faster — the volume of confusion building
- By the end, cards overlap, becoming semi-transparent, creating a dense, chaotic wall of text
- Animation: each card enters from the bottom, slides up with ease-standard (600ms), then continues scrolling. New cards enter at accelerating intervals (starting 400ms apart, ending 100ms apart).

#### SHOT 3D.2 — "Every Single Day" (1:22.000–1:30.000)

- The Stack Overflow wall fades to 15% opacity
- Over it, clean white text: "Every single day." in `heading-1`, #F0F0F0, centered
- The text appears with ease-enter, 600ms, translate-y from +16px to 0
- Holds for 3 seconds
- Then fades with the entire frame, transitioning to Act II
- The last 2 seconds before the transition: the documentary underscore music darkens, a minor key shift. This is the bridge to Act II's tension.
- **Transition out**: The frame dims to pure Deep Navy (#0A0F1A) over 500ms. Then hard cut to Act II Problem 1.

---

## DIALOGUE (COMPLETE FOR ACT I)

Full narrator script for this scene:

> April 2005. Linus Torvalds is mass-emailing the Linux kernel mailing list. BitKeeper — the version control system the kernel team had been using for free — just revoked their license. The entire Linux development workflow is about to collapse.

> So Linus does what Linus does. He builds his own. In ten days, he writes the first version of Git. A distributed version control system designed for one very specific job: managing the Linux kernel's source code across thousands of contributors, none of whom trust each other.

> And for that job? Git was brilliant. Truly. Content-addressable storage. A Merkle tree of commits. Cheap branching. Cryptographic integrity. For coordinating a massive, decentralized, open-source project in 2005 — it was exactly the right tool.

> But here's the thing nobody wants to say out loud.

> It's not 2005 anymore.

> Git was designed for the Linux kernel. Not for your microservices monorepo. Not for your fifty-person team on four continents. Not for your designer who just wants to save their work without accidentally deleting the production branch. Git solved Linus's problem. And then the entire industry adopted it — and inherited all of its assumptions.

> Twenty years later, we're still paying that tax. Every single day.

---

## AUDIO DESIGN

### Music

- **0:30–0:55**: Subtle documentary underscore begins. Warm. A solo piano playing sparse, contemplative notes in a major key (think Philip Glass-lite). Under it, very quiet strings sustained at -30dB. The warmth matches the sepia visual palette.
- **0:55–1:10**: The piano shifts to minor key. The strings become slightly discordant. A subtle electronic pulse begins underneath — a slow heartbeat rhythm at 60 BPM. This is the tonal shift matching "it's not 2005 anymore."
- **1:10–1:25**: The electronic pulse builds slightly. The piano drops out. Just strings + electronic pulse. Building tension for Act II.
- **1:25–1:30**: Music darkens further. A low synth pad joins, creating unease. This is the bridge into Act II. By 1:30, the music has transformed from warm documentary to cool tension.

### SFX

| Timecode  | Sound                              | Level                    |
| --------- | ---------------------------------- | ------------------------ |
| 0:36      | BitKeeper text crumbling           | -24dB, granular texture  |
| 0:36.5    | "REVOKED" stamp                    | -20dB, percussive hit    |
| 0:42–0:45 | Clock ticks (day counter)          | -30dB per tick           |
| 1:00–1:02 | 2005 text cracking                 | -22dB, clean crack       |
| 1:10–1:30 | Stack Overflow cards whooshing in  | -32dB, subtle air movement per card |

---

## COLOR PALETTE FOR THIS SCENE

This scene uses a UNIQUE warm palette that does NOT appear elsewhere in the video:

| Color                  | Hex       | Use                                                             |
| ---------------------- | --------- | --------------------------------------------------------------- |
| Sepia warm             | `#C4956A` | 2005-era illustrations, timeline nodes                          |
| Dark sepia             | `#8B6E4E` | Illustration shadows, secondary elements                        |
| Light sepia            | `#E8D5B7` | Illustration highlights                                         |
| Branch green (muted)   | `#7FB069` | Git's branch diagram merge points (muted, not the W0rkTree green) |

These warm colors are ONLY used in Segments 3A and 3B (0:30–0:55). By Segment 3C, they desaturate and are replaced by the standard cool palette. This color temperature shift is one of the most important visual storytelling devices in the entire video — it physically shows the viewer that the world moved from warm nostalgia to cold reality.

---

## TECHNICAL NOTES

- All illustrations for this scene must be original editorial artwork — no photographs, no real likenesses, no archival footage
- The timeline element introduced in SHOT 3A.1 persists (in some form) through the entire act — it is the visual through-line that ties the four segments together
- The branch/merge diagram in SHOT 3B.3 should use real Git-like topology (commits as nodes, branches as parallel lines, merges as convergence points) — but stylized and abstracted, not a literal Git log
- The Stack Overflow cards in SHOT 3D.1 must use paraphrased questions only — do not reproduce exact Stack Overflow titles verbatim due to CC BY-SA licensing requirements
- The color temperature shift in Segment 3C is the single most important visual transition in the entire video — it must be gradual, subtle, and unmistakable. Test this with the colorist before locking the edit.
- Frame rate: all animations rendered at 60fps for smooth motion, final delivery at 24fps with proper frame blending
- The "2005" crumble animation (SHOT 3C.2) should use the same particle system as the BitKeeper crumble (SHOT 3A.3) but at 1.5× duration to make it feel heavier, more significant — this is a callback the viewer will feel even if they don't consciously notice it