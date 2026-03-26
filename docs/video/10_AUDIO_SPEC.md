# Audio Specification

Complete audio design for the W0rkTree launch video. This document covers every audible element: narrator voice, music score, sound effects, foley, and the deliberate use of silence. All specifications are production-ready — an audio engineer should be able to build the complete mix from this document alone.

---

## Design System Color Reference (for cross-referencing visual sync points)

| Token            | Hex       |
| ---------------- | --------- |
| Deep Navy        | `#0A0F1A` |
| Code Background  | `#111827` |
| White            | `#F0F0F0` |
| Accent Cyan      | `#00D4FF` |
| Warning Red      | `#FF3B3B` |
| Confident Green  | `#00C48F` |
| Muted Gray       | `#6B7280` |
| Dim Gray         | `#374151` |

---

## 1. AUDIO TECHNICAL SPECS

| Parameter          | Value                              |
| ------------------ | ---------------------------------- |
| Sample rate        | 48,000 Hz                          |
| Bit depth          | 24-bit                             |
| Channels           | Stereo (2.0)                       |
| Loudness target    | −14 LUFS integrated (YouTube std)  |
| Loudness range     | 8 LU maximum                       |
| True peak          | −1 dBTP maximum                    |
| Codec (master)     | WAV (uncompressed)                 |
| Codec (delivery)   | AAC 320 kbps (YouTube upload)      |

### Delivery Files

| File                | Format      | Contents                                        |
| ------------------- | ----------- | ----------------------------------------------- |
| `master_stereo.wav` | 48 kHz / 24-bit WAV | Final stereo mix, loudness-normalized    |
| `stems_VO.wav`      | 48 kHz / 24-bit WAV | Narrator voice-over, isolated            |
| `stems_music.wav`   | 48 kHz / 24-bit WAV | Full music score, pre-duck               |
| `stems_SFX.wav`     | 48 kHz / 24-bit WAV | All designed sound effects               |
| `stems_foley.wav`   | 48 kHz / 24-bit WAV | All foley recordings                     |
| `delivery.aac`      | AAC 320 kbps        | YouTube-ready encode from master         |

---

## 2. MIX HIERARCHY

The mix follows a strict hierarchy. If two elements conflict, the higher-priority element wins:

```
Priority 1 (highest): Narrator V.O.         — target −12 LUFS
Priority 2:           Sound Effects (SFX)    — target −18 to −24 LUFS
Priority 3:           Foley                  — target −28 to −34 LUFS
Priority 4 (lowest):  Music                  — target −22 to −28 LUFS
```

### Sidechain Ducking Rules

| Trigger Track | Ducked Track | Attack | Release | Ratio | Duck Target    |
| ------------- | ------------ | ------ | ------- | ----- | -------------- |
| Narrator V.O. | Music       | 50 ms  | 300 ms  | 3:1   | −28 LUFS or lower |
| Narrator V.O. | Foley       | 30 ms  | 200 ms  | 2:1   | −34 LUFS or lower |
| SFX (hits)    | Music       | 10 ms  | 150 ms  | 4:1   | −30 LUFS (momentary) |

When the narrator is speaking, music must duck to −28 LUFS or lower. When the narrator is silent, music can rise to −22 LUFS. The sidechain compressor should be smooth — no pumping artifacts. Use a look-ahead of 5 ms if available.

### Stereo Field Guidelines

| Element          | Pan Position        | Width    |
| ---------------- | ------------------- | -------- |
| Narrator V.O.    | Dead center (C)     | Mono     |
| Music — bass     | Center (C)          | Mono     |
| Music — lead     | Center (C)          | Narrow   |
| Music — arpeggio | L30–R30             | Wide     |
| Music — pads     | L50–R50             | Full     |
| SFX — hits       | Center (C)          | Mono     |
| SFX — whooshes   | Follows visual pan  | Variable |
| Foley — keyboard | Center (C)          | Narrow   |
| Foley — room tone | L50–R50            | Full     |

---

## 3. NARRATOR SPECIFICATIONS

### Voice Profile

- **Tone**: Calm authority. A senior engineer explaining something they built — proud but not arrogant. Knowledgeable but not lecturing. Think of a staff engineer giving a tech talk at a small, respected conference.
- **Pace**: 165–180 words per minute. Slightly faster than conversational (150 WPM) but NOT Fireship-fast (200+ WPM). The ideas need room to land.
- **Register**: Mid-range. Not deep radio voice, not high/nasal. Natural.
- **Accent**: Neutral English (American, British, or any clear English — not an issue as long as diction is clear and every technical term is precisely pronounced).
- **Emotional range**: Subtle. The narrator is not a hype man. Emphasis comes from slight changes in pace and tone, not from shouting or dramatic whispers.

### Recording Requirements

- **Microphone**: Large-diaphragm condenser (Neumann U87, TLM 103, or equivalent). NOT a dynamic mic (SM7B is too warm for this tone — we need clarity and presence).
- **Room**: Treated recording space. No room reflections. Dry recording — reverb will be added in post if needed (but it probably won't be — this video is intimate, not cinematic).
- **Distance**: 6–8 inches from mic. Consistent distance throughout.
- **Pop filter**: Yes. Double-mesh preferred.
- **Isolation**: No bleed from headphones. Closed-back monitoring only during recording.
- **Takes**: Record each paragraph/section as a separate take. Minimum 3 takes per section. Slate each take verbally.

### Processing Chain (Post-Production)

Apply in this order:

1. **High-pass filter** at 80 Hz, 12 dB/oct slope — remove room rumble and proximity effect
2. **De-esser** targeting 5–8 kHz sibilance range — gentle, 3 dB maximum reduction. Do NOT over-de-ess; clarity is more important than smoothness
3. **Compression**: 2:1 ratio, −18 dB threshold, 30 ms attack (slow — preserve transients), 100 ms release (medium). Gain reduction should rarely exceed 4 dB.
4. **EQ**:
   - Gentle +2 dB shelf at 3 kHz (presence / clarity)
   - Gentle −2 dB at 200 Hz (reduce muddiness from proximity)
   - Optional: subtle +1 dB at 10 kHz (air) — only if the recording sounds dull
5. **Normalize** to −12 LUFS
6. **NO reverb. NO delay.** Bone dry. The voice should feel like it's speaking directly to the viewer, not into a room.

### Technical Term Pronunciation Guide

| Term       | Pronunciation                                                                 |
| ---------- | ----------------------------------------------------------------------------- |
| W0rkTree   | "Work-tree" (the zero is silent — it is a visual element only)                |
| Git        | "Git" (hard G, rhymes with "bit")                                             |
| BitKeeper  | "Bit-keeper"                                                                  |
| QUIC       | "Quick"                                                                       |
| DAG        | "Dag" (rhymes with "tag")                                                     |
| CLI        | "C-L-I" (spell out each letter)                                               |
| TOML       | "Tom-L" (first syllable rhymes with "Tom")                                    |
| IAM        | "I-A-M" (spell out each letter)                                               |
| RBAC       | "R-back"                                                                      |
| SPDX       | "S-P-D-X" (spell out each letter)                                             |
| bgprocess  | "background process" (always say the full phrase, never the abbreviation)     |
| worktree   | "work-tree" (two syllables, emphasis on "work")                               |
| SHA        | "Shah" (one syllable, rhymes with "spa")                                      |
| API        | "A-P-I" (spell out each letter)                                               |
| SSH        | "S-S-H" (spell out each letter)                                               |
| HTTPS      | "H-T-T-P-S" (spell out each letter)                                           |
| UUID       | "U-U-I-D" (spell out each letter)                                             |
| JSON       | "Jay-son"                                                                     |
| mutex      | "mew-tex"                                                                     |

### Narration Pacing Notes

| Section       | Timecode       | Pacing Note                                                                 |
| ------------- | -------------- | --------------------------------------------------------------------------- |
| Cold Open     | 0:00–0:25      | NO narration. Terminal and foley only.                                       |
| Title Card    | 0:25–0:30      | NO narration. Music and tone only.                                          |
| Act I         | 0:30–1:30      | Measured, storytelling pace. ~165 WPM. Let the history breathe.             |
| Act II        | 1:30–2:38      | Slightly faster, ~175 WPM. Building frustration. Clipped delivery.          |
| Bridge        | 2:38–2:45      | Slow. Deliberate. Every word lands. ~140 WPM.                               |
| Act III       | 2:45–4:15      | Confident, ~170 WPM. Not rushed — the product is strong, no need to hurry. |
| Act IV        | 4:15–4:40      | Minimal narration. Let the terminal speak.                                  |
| Close         | 4:40–5:00      | Slow, final. ~150 WPM. The last words should linger.                        |

---

## 4. MUSIC — FULL SCORE DIRECTION

### Overview

The music is a continuous, evolving piece that transforms across the video's emotional arc. It is NOT multiple separate tracks — it is ONE composition with distinct movements that flow seamlessly into each other.

**Genre**: Modern electronic + ambient. Think: Tycho, Bonobo, Ólafur Arnalds, or the soundtrack to *Ex Machina*. Clean. Atmospheric. Not EDM. Not lo-fi. Not chiptune. Not corporate stock music.

**Key**: C minor for Acts I–II (tension, weight). Modulates to C major for Acts III–Close (resolution, confidence). The modulation happens at 2:42–2:45 (the Bridge between Act II and Act III).

**Tempo**: 80 BPM throughout. Steady. The tempo does NOT change — consistency creates confidence. No ritardando, no accelerando, no tempo automation. If the feel needs to shift, it happens through instrumentation and dynamics, not tempo.

**Time Signature**: 4/4 throughout.

### Instrument Palette

| Instrument        | Plugin / Source Suggestion          | Role                          |
| ----------------- | ----------------------------------- | ----------------------------- |
| Piano             | Nils Frahm Piano (Kontakt) / Felt  | Melodic lead (Acts I–II)     |
| String pad        | Spitfire Audio Albion / CSS         | Harmonic bed                  |
| Synth pad (warm)  | Serum / Vital — saw, low-pass      | Harmonic bed (electronic)     |
| Synth lead        | Serum / Vital — clean sine/tri     | Melodic lead (Acts III+)     |
| Arpeggio synth    | Serum — pluck patch                | Rhythmic texture              |
| Sub bass          | Serum — sine sub                   | Low-end foundation            |
| Kick              | 808 sample, tuned to C             | Rhythmic pulse                |
| Hi-hat            | Acoustic sample, crisp             | Rhythmic texture              |
| Snare / rimshot   | Acoustic sample, tight             | Backbeat (Act III only)       |

### Movement-by-Movement Score Direction

---

#### Movement 0 — Silence (0:00–0:25, Cold Open)

**NO music.** The Cold Open is scored with silence and foley only. The absence of music makes its arrival at the Title Card more impactful.

The only audible elements are:
- Mechanical keyboard typing (SFX-001)
- Room tone / HVAC hum (SFX-002)
- Sub-bass pulse at CONFLICT (SFX-003)
- Silence drop (SFX-004)
- Laptop close (SFX-005)

The deliberate withholding of music creates intimacy. The viewer is alone with the developer. When the Title Card music arrives, it will feel like a world expanding.

---

#### Movement 1 — The Tone (0:25–0:30, Title Card)

- A single synthesizer note: **C2** (65.4 Hz)
- Waveform: saw wave with heavy low-pass filter (cutoff ~200 Hz), creating a warm, deep tone
- Harmonic: **G2** (98 Hz) fades in at 50% volume after 500 ms, creating a power fifth
- Attack: 500 ms (slow fade in — not an abrupt start)
- Sustain: full duration of the Title Card
- Release: fades over the last 1000 ms of the Title Card
- Reverb: Large hall, 20% wet, 3 s decay. The reverb tail carries into Act I.
- Total loudness: −28 dB LUFS. This should be **felt** as much as heard — the sub frequencies are critical. Test on full-range monitors and subwoofer.

**Sync point**: The tone begins at the exact frame the logo starts its fade-in (0:25.0). The G2 harmonic enters as the "0" glow pulse peaks.

---

#### Movement 2 — Documentary Warmth (0:30–0:55, Act I Segments A + B)

- **Solo piano** enters at 0:30. Sparse. 2–3 notes every 2 bars. Right hand only. Mid-register (C4–G5).
- Key: **C minor**
- Feel: Philip Glass-lite. Repetitive, contemplative, not melancholic — *thoughtful*. Each phrase should feel like a quiet observation.
- Dynamics: pp–p. The piano is a whisper, not a statement.
- At **0:45**: Sustained **strings** enter beneath the piano. A string pad (cello + viola) holding the root (C) and fifth (G). Very quiet (−30 dB). The warmth matches the sepia visual palette of the history segment.
- The piano and strings together create a sense of *history*, *respect*, *looking back*.
- No percussion. No electronics. This is an acoustic, human sound.

**Transition note**: The warmth established here makes the coming chill (Movement 3) feel like a loss.

---

#### Movement 3 — The Chill (0:55–1:30, Act I Segments C + D)

- Piano shifts to emphasize **minor intervals**. The previously warm notes become cooler, more uncertain. Use minor 2nds and diminished intervals.
- At **1:00**: An electronic pulse enters. A low **kick drum** at 80 BPM, very quiet (−28 dB). Not a beat — a *heartbeat*. No hi-hat. No snare. Just the pulse.
- Strings become slightly **dissonant**. Not ugly — *unsettled*. A half-step up in the viola (Db against C) creates unresolved tension.
- At **1:10**: The piano drops out. Just strings + electronic pulse. The acoustic warmth is gone. We're in electronic territory now. Building.
- At **1:15**: A subtle filtered noise sweep begins (white noise through a band-pass, rising from 200 Hz to 2 kHz over 15 seconds). This creates subliminal anticipation.
- At **1:20**: A low synth pad (sawtooth, heavily filtered, cutoff ~300 Hz) joins. The sound is now fully electronic with string accents.
- By **1:30**: The underscore has fully transformed from warm piano to cool electronic tension. The transformation should feel inevitable, not jarring.

---

#### Movement 4 — Problems (1:30–2:38, Act II)

This movement accompanies the five problems. The music must support the escalating tension without competing with the narrator or the percussive Number Card Slam SFX.

- The electronic beat becomes more defined. **Kick on 1 and 3**, hi-hat on off-beats (eighth notes). Still minimal. Still 80 BPM.
- The energy comes from the narrator and SFX, not the music. The music is a *bed*, not a *feature*.
- At **1:42**: **Bass line** enters. A simple, low (C1, 32.7 Hz) synth bass, one sustained note per bar. Adds weight and gravitas.
- At **2:10**: A **filtered arpeggio** begins. Sixteenth notes on a pluck synth, very quiet (−28 dB), creating forward momentum. The low-pass filter starts at ~400 Hz and opens gradually (reaching ~4 kHz by 2:36), adding brightness as problems stack.
- At **2:24**: Peak of Act II. The arpeggio is now mid-range, the bass is present, the hi-hat is driving. This is the most energetic the music gets during the "problem" section.
- At **2:30**: A subtle **riser** (noise sweep + pitch-rising synth) begins building toward the tower collapse.
- At **2:36** (tower wobble): **Everything collapses.** A sustained, dissonant chord replaces the beat. The chord: C, Db, Gb — ugly, unresolved, tense. All rhythmic elements drop. Just the chord, held and trembling (gentle LFO on volume, 4 Hz, ±2 dB).

**Critical rule**: On each Number Card Slam hit (SFX-013, 015, 019, 021, 023), the music ducks momentarily (−6 dB, 10 ms attack, 150 ms release). The hit must cut through cleanly.

---

#### Movement 5 — The Drop (2:38–2:45, Bridge)

The emotional pivot of the entire video. Every musical choice here matters.

- At **2:38.0**: **EVERYTHING stops.** Total silence for 500 ms. No music, no SFX, no foley. The viewer should feel the absence physically.
- At **2:38.5**: A sustained synth pad enters. It starts on the **dissonant chord** from the end of Movement 4 (C, Db, Gb)...
- At **2:40.0**: The Db begins sliding down to D natural (pitch bend, 2 seconds)...
- At **2:42.0**: ...and **RESOLVES to C major** (C, E, G). The chord change is the emotional pivot of the entire video. The resolution should feel like a weight being lifted, like sunlight after clouds.
- At **2:43.0**: The first notes of the Act III theme begin: a clean, bright arpeggio in C major. Just 3–4 notes. A preview. A promise.
- Reverb opens up during this section (20% → 30% wet). The space expands with the harmony.

**Mix note**: This section should be slightly louder than the preceding silence makes you expect. The contrast between 500 ms of nothing and the resolving chord is the most powerful moment in the score.

---

#### Movement 6 — W0rkTree Theme (2:45–4:15, Act III)

The main theme. This is what the viewer remembers after the video ends.

**2:45–3:00 — Theme Introduction**
- Clean **synthesizer lead**. A simple, memorable 8-bar melody in C major. The melody should be hummable — think of it as a product's sonic logo.
- Below: the arpeggio from the Bridge, now full and open. No filter. Sixteenth notes, C major arpeggiation (C–E–G–C').
- Below that: the **bass**, now playing a more melodic line (root–fifth–octave pattern: C1–G1–C2, one note per beat).
- Kick drum at 80 BPM, consistent.
- Hi-hat: eighth notes with a subtle swing (6% shuffle — just enough to feel human, not enough to feel "jazzy").

**3:00–3:20 — Full Beat Established**
- **Snare** enters on beats 2 and 4. Subtle — a rimshot or clap at −24 dB. Not aggressive. The snare says "confidence," not "energy."
- All elements are now present: lead, arpeggio, bass, kick, hi-hat, snare, pad.
- Dynamics: mf. This is the comfortable cruising altitude of the score.

**3:20 — The Reveal Break ("STAGED SNAPSHOT VISIBILITY")**
- Brief **1-bar break**. Everything drops for beat 1 of the bar. On beat 2, everything comes back at +2 dB from before. The break creates a "mic drop" moment for the key feature reveal.
- The SFX-028 (reveal bell) rings during the silent beat. It must be the only sound.

**3:23–3:45 — Peak Intensity**
- The melody plays its **highest phrase**. The synth lead reaches G5–C6 range.
- The arpeggio is at its brightest (filter fully open, slight resonance at cutoff).
- The bass is at its deepest (sub-bass octave C0–C1 layered).
- The pad swells to its widest stereo spread.
- This is the **emotional climax** of the music. Dynamics: f. Not loud for loud's sake — full, rich, complete.

**3:45–4:05 — Sustain & Support**
- The beat maintains but the melody simplifies. Long sustained notes replace the melodic phrases.
- Feature cards are dense with information; the music holds steady, providing energy without competing for cognitive bandwidth.
- The arpeggio continues but drops in volume (−3 dB). Space for the narrator.

**4:05–4:12 — Resolution Phrase**
- The melody plays its **final descending phrase**: G5–E5–D5–C5, whole notes.
- The arpeggio resolves to the root and holds. No more arpeggiation — just a sustained C major chord.
- The bass holds C1. No movement. Stability.
- The beat thins: kick drops out first (4:08), then hi-hat (4:10), then snare (4:12).

**4:12–4:15 — Fade to Ambient**
- Everything begins to fade. Just the sustained C major pad remains.
- Volume drops from −22 LUFS to −34 LUFS over 3 seconds.
- The reverb tail lengthens (3 s → 5 s decay). The space becomes vast.

---

#### Movement 7 — Ambient (4:15–4:40, Act IV)

- Music at **−34 dB LUFS**. Barely audible. Just a sustained synth pad (C major triad, no movement, no modulation).
- The terminal typing sounds are the music here. The pad is atmospheric context, not content.
- The pad should feel like "air" — present but invisible. If the viewer notices it, it's too loud.
- At **4:40**: The pad **swells** to −28 dB over 2 seconds. The resolution chord becomes slightly brighter — add a **major 7th** (B natural) to the C major chord, creating Cmaj7. This adds a wistful, open quality. Transition into the Close.

---

#### Movement 8 — Resolution (4:45–5:00, Close)

- The main theme **returns**, but reduced. Just the arpeggio (half speed — eighth notes instead of sixteenths) + the sustained Cmaj7 chord. No beat. No bass. No lead melody.
- Spacious **reverb** (30% wet, 4 s decay). The sound should feel like it's in a cathedral.
- Dynamics: p. Quiet, reflective, final.
- At **4:53**: The arpeggio fades. Just the sustained chord.
- At **4:55**: The chord begins its final fade. Smooth, linear, inevitable.
- At **4:57**: Only the reverb tail remains.
- At **4:59.5**: Silence.
- At **5:00**: Hard end. No sound at all on the final frame.

---

### Music Reference Tracks

For the composer/producer, these tracks capture specific qualities to aim for:

| Reference Track                        | What to Take From It                                     |
| -------------------------------------- | -------------------------------------------------------- |
| Tycho — "Awake"                        | The clean, warm synth tones. The spacious mix.           |
| Ólafur Arnalds — "Near Light"          | The piano-to-electronic transformation (Movements 2→3). |
| Bonobo — "Kerala"                      | The arpeggio texture. The steady build.                  |
| Ex Machina OST — "Bunsen Burner"       | The unsettling tension (Movement 4).                     |
| Trent Reznor — "Hand Covers Bruise"    | The sparse, deliberate pacing. Less is more.             |
| Jon Hopkins — "Open Eye Signal"        | The rhythmic drive that doesn't overwhelm.               |

---

## 5. SOUND EFFECTS MASTER LIST

Every sound effect in the video, in chronological order. Each entry includes enough detail for a sound designer to source or create the effect.

### Cold Open (0:00–0:25)

| ID      | Timecode       | Name              | Description                                                                                              | Source Type     | Level  | Duration   |
| ------- | -------------- | ----------------- | -------------------------------------------------------------------------------------------------------- | --------------- | ------ | ---------- |
| SFX-001 | 0:00–0:14      | mech-keyboard     | Mechanical keyboard typing. Cherry MX Blue (clicky) character. Steady moderate pace matching terminal typing animation. Individual keystrokes must align with character appearance on screen. | Foley           | −18 dB | 14 s       |
| SFX-002 | 0:00–0:14      | room-tone         | Ambient HVAC hum. Low, constant, barely perceptible. Creates the "3 AM office" atmosphere. Frequency: ~120 Hz fundamental with harmonics. | Foley           | −40 dB | 14 s       |
| SFX-003 | 0:08.5         | conflict-sub-bass | Sub-bass pulse triggered when "CONFLICT" appears in terminal output. Sine wave at 40 Hz, fast attack (5 ms), 200 ms decay. Should be felt in the chest on a subwoofer, barely audible on laptop speakers. | Designed        | −30 dB | 200 ms     |
| SFX-004 | 0:13.5         | silence-drop      | Room tone (SFX-002) fades to near-zero over 200 ms. The sudden absence of ambient sound creates a visceral "vacuum" effect. Not a fade — a drop. | Mix automation  | Silence| 1.5 s      |
| SFX-005 | 0:14.5         | laptop-close      | Laptop lid closing. A clean, precise mechanical click. Not a slam — a deliberate, quiet close. The sound of "I'm done." | Foley           | −22 dB | 300 ms     |
| SFX-006 | 0:17.5–0:19.5  | laptop-keys       | Soft laptop keyboard typing for the chat message scene. Different texture from SFX-001 — quieter, mushier, laptop membrane feel. | Foley           | −24 dB | 2 s        |

### Title Card (0:25–0:30)

| ID      | Timecode  | Name        | Description                                                                                                  | Source Type | Level  | Duration |
| ------- | --------- | ----------- | ------------------------------------------------------------------------------------------------------------ | ----------- | ------ | -------- |
| SFX-007 | 0:25.3    | title-tone  | Synth tone for logo reveal. Synchronized with Movement 1 music entry. A bright transient layered over the musical tone — a soft metallic "ping" at ~2 kHz that gives the moment definition. | Designed    | −28 dB | 2 s      |

### Act I — Origin Story (0:30–1:30)

| ID      | Timecode    | Name         | Description                                                                                                                              | Source Type | Level  | Duration |
| ------- | ----------- | ------------ | ---------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ------ | -------- |
| SFX-008 | 0:36        | text-crumble | BitKeeper text crumbling apart. A dry, granular sound — like sand pouring mixed with tiny glass fragments. High-frequency content (4–12 kHz) with no low end. Matches the Break-Apart animation component. | Designed    | −24 dB | 800 ms   |
| SFX-009 | 0:36.5      | stamp-hit    | "REVOKED" stamp impact. A sharp, authoritative thud — like a rubber stamp hitting paper, but with added weight. Transient-heavy (< 5 ms attack). Low-mid body (~200 Hz). Very short decay. | Designed    | −20 dB | 200 ms   |
| SFX-010 | 0:42–0:45   | clock-ticks  | Day counter ticking through 10 days rapidly. Mechanical clock tick, accelerating from 2 ticks/s to 10 ticks/s. Each tick is identical — a short, crisp "tck" at ~4 kHz. | Designed    | −30 dB | 3 s      |
| SFX-011 | 1:00–1:02   | year-crack   | "2005" text cracking and splitting. Deep, resonant crack sound — like ice breaking on a lake. Starts with a sharp transient, followed by creaking/splitting. Low-frequency emphasis (60–200 Hz). | Designed    | −22 dB | 1.2 s    |
| SFX-012 | 1:10–1:30   | card-whoosh  | Stack Overflow card scroll whooshes. Repeated soft air movement as each card enters frame. Individual whooshes are ~200 ms, pitched slightly higher as scroll speed increases. Start interval: 400 ms, end interval: 100 ms. | Designed    | −32 dB | 20 s     |

### Act II — Five Problems (1:30–2:38)

| ID      | Timecode          | Name             | Description                                                                                                             | Source Type      | Level  | Duration     |
| ------- | ----------------- | ---------------- | ----------------------------------------------------------------------------------------------------------------------- | ---------------- | ------ | ------------ |
| SFX-013 | 1:30              | hit-01           | Problem 1 percussive hit. Low tom (80–120 Hz fundamental) layered with kick drum transient. Sharp attack (< 10 ms), short decay (~200 ms). DRY — no reverb. The hit must feel like a physical impact. | Designed         | −18 dB | 200 ms       |
| SFX-014 | 1:32–1:38         | jargon-whoosh    | Jargon terms arriving on screen. Individual soft whooshes for each term, staggered. Similar character to SFX-012 but shorter (100 ms each) and lower-pitched. | Designed         | −30 dB | 6 s          |
| SFX-015 | 1:42              | hit-02           | Problem 2 percussive hit. Same character as SFX-013 but pitched 2 semitones lower. Adds weight — problems are accumulating. | Designed         | −18 dB | 200 ms       |
| SFX-016 | 1:44–1:51         | clock-tick-accel | Timer countdown ticking, accelerating. Starts at 1 tick/s, accelerates to 8 ticks/s by end. Tick sound: sharper and more metallic than SFX-010. Increasing urgency. | Designed         | −24 dB | 7 s          |
| SFX-017 | 1:51              | crash-impact     | CONFLICT explosion. The biggest SFX in the video. Layered: (1) sub-bass boom at 30 Hz, (2) mid-range crash/shatter, (3) high-frequency debris scatter. Fast attack, 400 ms total. Should feel like something breaking. | Designed         | −16 dB | 400 ms       |
| SFX-018 | 1:51.1            | red-flash-bass   | Sub-bass pulse accompanying the red screen flash. Pure sine at 30 Hz, 100 ms. Layered under SFX-017 for extra low-end impact. | Designed         | −22 dB | 100 ms       |
| SFX-019 | 1:55              | hit-03           | Problem 3 percussive hit. Same family as SFX-013/015, pitched 4 semitones below original. The hits are getting deeper. | Designed         | −18 dB | 200 ms       |
| SFX-020 | 1:57–2:06         | node-crumble ×3  | DAG nodes dissolving. Three separate instances. A granular dissolution sound — particles breaking apart. Higher, more delicate than SFX-008 (text crumble). Each instance: 800 ms, with a slight pitch variation between them. | Designed         | −26 dB | 800 ms each  |
| SFX-021 | 2:10              | hit-04           | Problem 4 percussive hit. Same family, pitched 6 semitones below original. Noticeably heavier. | Designed         | −18 dB | 200 ms       |
| SFX-022 | 2:12–2:20         | vault-whoosh     | Files streaming from a vault. A continuous, airy sound with individual "flick" transients as each file passes. Wind-tunnel character with paper-flutter texture. | Designed         | −24 dB | 8 s          |
| SFX-023 | 2:24              | hit-05           | Problem 5 percussive hit. Same family, pitched 8 semitones below original. The lowest and heaviest of all five. This is the final blow. | Designed         | −18 dB | 200 ms       |
| SFX-024 | 2:36              | jenga-fall       | Block falling from tower. Wooden clack on impact + subtle scatter of smaller pieces. Recorded foley enhanced with designed low-end. | Foley / Designed | −22 dB | 400 ms       |

### Act III — Solution (2:45–4:15)

| ID      | Timecode          | Name           | Description                                                                                                                                      | Source Type | Level  | Duration     |
| ------- | ----------------- | -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ----------- | ------ | ------------ |
| SFX-025 | 2:47, 2:49, 2:51  | strike-deny    | Strikethrough denial sound. Three instances. A quick, sharp horizontal "swipe" — like a marker drawn fast across paper. Short, clipped, definitive. Pitched slightly differently each time (ascending by 1 semitone). | Designed    | −28 dB | 200 ms each  |
| SFX-026 | 2:55–3:15         | node-snap      | Diagram node appearance. A small, precise "snap" or "click" — like a UI element locking into place. Clean, digital. Each node gets one snap. ~15 instances total, staggered per the Diagram Node Build animation component timing. | Designed    | −32 dB | 100 ms each  |
| SFX-027 | 3:03–3:06         | line-whoosh    | Connection line drawing between nodes. A smooth directional whoosh that follows the line's path (pan follows animation). Slightly "electric" quality — like current flowing through a wire. | Designed    | −28 dB | 800 ms       |
| SFX-028 | 3:20              | reveal-bell    | "STAGED SNAPSHOT VISIBILITY" reveal. A clear, resonant bell tone. Not a church bell — a crystal singing bowl or a high-quality chime. Fundamental at ~1 kHz with clean harmonics. Long sustain (800 ms). This is a highlight moment — the sound should feel special. | Designed    | −20 dB | 800 ms       |
| SFX-029 | 3:42              | button-click   | Push button press. A tactile UI "click" — sharp, clean, satisfying. Like a well-made physical button. Short and definitive. | Designed    | −26 dB | 100 ms       |
| SFX-030 | 3:42.2            | success-chime  | Checkmark appearance chime. A two-note ascending chime (major third interval). Clean, bright, positive. The audible version of ✓. | Designed    | −28 dB | 300 ms       |

### Act IV — Live Demo (4:15–4:40)

| ID      | Timecode    | Name          | Description                                                                                                          | Source Type | Level  | Duration |
| ------- | ----------- | ------------- | -------------------------------------------------------------------------------------------------------------------- | ----------- | ------ | -------- |
| SFX-031 | 4:15–4:40   | demo-keyboard | Terminal typing for Act IV demo. Same mechanical keyboard as SFX-001 but at a slightly more deliberate pace. Keystrokes sync precisely with the Terminal Typing Animation character timing. | Foley       | −20 dB | 25 s     |
| SFX-032 | various     | output-chime  | Command completion blip. A tiny, subtle "blip" that plays when each command's output finishes rendering. Sine tone at 800 Hz, 100 ms, fast decay. Barely noticeable — subliminal confirmation. | Designed    | −32 dB | 100 ms   |
| SFX-033 | 4:36        | denied-tone   | Access denied low tone. A low, muted "bwonk" — two notes descending (minor second). Conveys rejection without drama. It's expected — the policy is working correctly. | Designed    | −28 dB | 300 ms   |

### Percussive Hit Pitch Progression (Act II)

The five Number Card Slam hits form a descending pitch sequence that creates a sense of accumulating weight:

| Hit    | SFX ID  | Timecode | Pitch Offset   | Fundamental (approx.) |
| ------ | ------- | -------- | -------------- | --------------------- |
| Hit 1  | SFX-013 | 1:30     | 0 semitones    | ~100 Hz               |
| Hit 2  | SFX-015 | 1:42     | −2 semitones   | ~89 Hz                |
| Hit 3  | SFX-019 | 1:55     | −4 semitones   | ~79 Hz                |
| Hit 4  | SFX-021 | 2:10     | −6 semitones   | ~71 Hz                |
| Hit 5  | SFX-023 | 2:24     | −8 semitones   | ~63 Hz                |

Each successive hit is lower and heavier. By hit 5, the viewer should feel the weight of all five problems stacking.

---

## 6. FOLEY RECORDING REQUIREMENTS

All sounds that must be physically recorded (not synthesized or sourced from libraries).

### Recording Session Setup

| Parameter          | Specification                                            |
| ------------------ | -------------------------------------------------------- |
| Microphone         | Small-diaphragm condenser (Neumann KM184 or equivalent) |
| Backup microphone  | Large-diaphragm condenser for alternate perspective      |
| Preamp             | Clean, transparent (Grace Design, RME, or equivalent)    |
| Sample rate        | 96 kHz (downsample to 48 kHz in post for extra headroom) |
| Bit depth          | 24-bit                                                   |
| Room               | Treated studio, dead room. No reflections.               |
| Monitoring         | Headphones only during recording (no speakers)           |

### Sound List

| Sound              | Equipment Needed                                                 | Recording Notes                                                                                                                                                            | Used In      |
| ------------------ | ---------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------ |
| Mechanical keyboard | Cherry MX Blue (or similar clicky switch) keyboard + close-mic condenser (4–6 inches above keys) | Record 2 minutes of continuous typing at moderate speed (~60 WPM). Also record 30 individual isolated keystrokes at varying velocities for precision sync in post. Record spacebar separately (different sound). Record Enter key separately. | SFX-001, SFX-031 |
| Laptop keyboard    | Modern laptop (MacBook Pro or ThinkPad) + close-mic condenser    | Record 1 minute of soft, deliberate typing. Different texture from mechanical — quieter, mushier, membrane feel. Also record individual keystrokes.                         | SFX-006      |
| Laptop lid close   | Same laptop as above + close-mic condenser at lid hinge level    | Record 10 takes at varying speeds (slow/deliberate to quick/firm). Choose the cleanest mechanical click with the right emotional weight — deliberate, not angry.             | SFX-005      |
| Room tone          | On-set (or recording room) + stereo pair (ORTF configuration)   | 60 seconds of complete silence in the recording environment. All equipment powered on but no human activity. Capture the natural ambient of the space. Record at the same gain level as other foley. | SFX-002      |
| Wooden block fall  | Small wooden block (~3 cm cube) on hard surface + overhead mic   | For Jenga tower SFX base layer. Record multiple takes at different heights (5 cm, 10 cm, 20 cm). Also record the block tumbling/rolling for scatter texture. Record on wood, tile, and fabric surfaces for options. | SFX-024      |

### Foley Sync Requirements

| SFX     | Sync Precision | Notes                                                                                       |
| ------- | -------------- | ------------------------------------------------------------------------------------------- |
| SFX-001 | Per-character  | Each keystroke must align within ±1 frame (±20 ms) of the corresponding character appearing on screen in the Terminal Typing Animation. |
| SFX-005 | Per-frame      | The "click" transient must land on the exact frame of the visual laptop close.               |
| SFX-006 | Loose          | General typing texture. Does not need per-character sync. Just needs to feel natural over the chat typing scene. |
| SFX-031 | Per-character  | Same precision as SFX-001. Every keystroke in the Act IV demo must sync to the terminal.     |

---

## 7. SILENCE AS A TOOL

Explicit moments where **silence** is a deliberate creative choice. These are not gaps or mistakes — they are scored silences that must be protected in the mix.

| Timecode       | Duration | Context                           | Why Silence                                                                                                          |
| -------------- | -------- | --------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| 0:00–0:25      | 25 s     | Entire Cold Open                  | **Intimacy.** The viewer is alone with the developer. Music would create emotional distance. The foley-only approach makes this feel like a real moment, not a produced video. The viewer leans in. |
| 0:13.5–0:15    | 1.5 s    | After `git push` / "initial commit" | **Weight.** The damage is done. The developer has silently committed code to avoid a broken process. Silence here is the emotional equivalent of a sigh. Adding any sound would diminish the moment. |
| 2:38–2:38.5    | 500 ms   | Music drops before Bridge         | **Relief and anticipation.** After 68 seconds of building tension (Act II), the sudden silence is a physical release. The viewer's brain gets a micro-rest before the C major resolution hits. This silence makes the chord change 10× more powerful. |
| 3:20 (beat 1)  | ~750 ms  | 1-bar break for reveal            | **Emphasis.** Everything drops for one beat so that the reveal bell (SFX-028) rings in clean space. The silence frames the most important feature name in the video. |
| 4:15–4:40      | 25 s     | Act IV terminal demo (near-silence) | **Confidence.** The product speaks for itself. The terminal sounds ARE the soundtrack. Adding music would imply the demo needs emotional support — it doesn't. The near-silence says "watch this." |

### Rules for Protecting Silence

1. During scored silences, the noise floor must be below −60 dBFS. No hiss, no hum, no artifacts.
2. Transitions INTO silence must be clean — use 50 ms fades to avoid click artifacts.
3. Transitions OUT of silence should respect the moment. The first sound after silence should never be jarring (except where intentional, like the Bridge chord).
4. During the Cold Open silence (0:00–0:25), do NOT add ambient music "just to fill space." The silence is the creative choice. Resist the urge to score it.

---

## 8. AUDIO SYNC POINTS

Critical moments where audio and visual must be frame-accurate (within ±1 frame at 30 fps = ±33 ms).

| Timecode | Audio Event                       | Visual Event                              | Tolerance |
| -------- | --------------------------------- | ----------------------------------------- | --------- |
| 0:00.0   | First keystroke (SFX-001)         | First character appears in terminal       | ±1 frame  |
| 0:08.5   | Sub-bass pulse (SFX-003)          | "CONFLICT" text appears                   | ±1 frame  |
| 0:14.5   | Laptop close click (SFX-005)      | Visual laptop lid close                   | ±1 frame  |
| 0:25.0   | Title tone begins (Movement 1)    | Logo fade-in begins                       | ±1 frame  |
| 0:36.0   | Text crumble (SFX-008)            | BitKeeper text break-apart animation      | ±1 frame  |
| 0:36.5   | Stamp hit (SFX-009)               | REVOKED stamp lands at scale(1.0)         | ±1 frame  |
| 1:30.0   | Hit-01 (SFX-013)                  | Number "1" reaches scale(1.0) in slam     | ±1 frame  |
| 1:42.0   | Hit-02 (SFX-015)                  | Number "2" reaches scale(1.0) in slam     | ±1 frame  |
| 1:51.0   | Crash impact (SFX-017)            | Split-screen collapse / CONFLICT slam     | ±1 frame  |
| 1:55.0   | Hit-03 (SFX-019)                  | Number "3" reaches scale(1.0) in slam     | ±1 frame  |
| 2:10.0   | Hit-04 (SFX-021)                  | Number "4" reaches scale(1.0) in slam     | ±1 frame  |
| 2:24.0   | Hit-05 (SFX-023)                  | Number "5" reaches scale(1.0) in slam     | ±1 frame  |
| 2:36.0   | Jenga fall (SFX-024)              | Tower block hits ground                   | ±1 frame  |
| 2:38.0   | Total silence begins              | Visual freeze / transition frame          | ±1 frame  |
| 2:42.0   | C major chord resolves            | W0rkTree text/logo appears                | ±1 frame  |
| 2:47.0   | First strike-deny (SFX-025)       | First strikethrough line begins drawing   | ±1 frame  |
| 3:20.0   | Reveal bell (SFX-028)             | "STAGED SNAPSHOT VISIBILITY" text entry   | ±1 frame  |
| 3:42.0   | Button click (SFX-029)            | Push button visual press                  | ±1 frame  |
| 3:42.2   | Success chime (SFX-030)           | Checkmark ✓ appears                       | ±1 frame  |
| 4:36.0   | Denied tone (SFX-033)             | Access denied / ✗ appears                 | ±1 frame  |

---

## 9. LOUDNESS AUTOMATION TIMELINE

A visual guide to the overall loudness contour of the video. This represents the *integrated* loudness of the full mix at each section:

```
Section         Timecode      Target LUFS   Notes
─────────────────────────────────────────────────────────────
Cold Open       0:00–0:25     −28 to −36    Foley only. Quiet, intimate.
Title Card      0:25–0:30     −28           Music tone + SFX. Subtle.
Act I (early)   0:30–0:55     −20 to −22    V.O. + piano. Calm.
Act I (late)    0:55–1:30     −18 to −20    V.O. + building electronic. Growing.
Act II          1:30–2:38     −16 to −18    V.O. + SFX hits + beat. Energetic.
Bridge          2:38–2:45     −40 → −18     Silence → chord. Maximum contrast.
Act III (early) 2:45–3:20     −16 to −18    V.O. + full theme. Peak energy.
Act III (peak)  3:20–3:45     −14 to −16    Loudest section. Full arrangement.
Act III (late)  3:45–4:15     −18 to −20    Settling. Feature detail.
Act IV          4:15–4:40     −28 to −34    Near-silence. Terminal foley.
Close           4:40–5:00     −24 → −60     Fade to silence.
```

### Loudness Contour Shape

The overall loudness should follow this emotional arc:

1. **Quiet start** (Cold Open) — draw the viewer in
2. **Gradual build** (Acts I–II) — escalating tension
3. **Dramatic dip** (Bridge) — the deepest valley
4. **Peak** (Act III mid-section) — the emotional and sonic climax
5. **Graceful descent** (Act IV–Close) — resolution and release

The loudness range between the quietest moment (Bridge silence) and the loudest moment (Act III peak) should not exceed **20 LU**. Use limiting on the master bus to catch any transients that would push past −1 dBTP.

---

## 10. MASTERING NOTES

### Final Processing Chain (Master Bus)

Apply in this order:

1. **EQ**: High-pass at 25 Hz (remove sub-sub-bass rumble), gentle low shelf +1 dB at 60 Hz (warmth), gentle high shelf +0.5 dB at 12 kHz (air)
2. **Multiband compression** (optional): Only if dynamic range between sections is too wide after automation. Target: tame the Act II hits from poking out too far without squashing Act IV's silence.
3. **Stereo imaging**: Check mono compatibility. All critical elements (V.O., bass, hits) must be fully mono-compatible. Width elements (pads, reverb tails) are acceptable at reduced level in mono.
4. **True peak limiter**: Ceiling at −1 dBTP. Attack: auto. Release: auto. This is a safety net, not a loudness tool. If the limiter is reducing more than 1 dB regularly, the mix is too hot.
5. **Loudness normalization**: Target −14 LUFS integrated. Measure the entire 5:00 video. If the integrated loudness doesn't hit −14 LUFS naturally, adjust the master fader — do NOT push the limiter harder.

### Quality Control Checklist

Before delivery, verify:

- [ ] Integrated loudness: −14 LUFS ±0.5
- [ ] Loudness range: ≤ 8 LU
- [ ] True peak: ≤ −1 dBTP at all points
- [ ] No clipping on any individual track
- [ ] Mono compatibility: all critical elements audible in mono fold-down
- [ ] Silence sections are truly silent (< −60 dBFS noise floor)
- [ ] All sync points verified frame-accurate (see Section 8)
- [ ] Narrator intelligibility: all technical terms clearly audible over music/SFX
- [ ] Sub-bass content present but controlled (check on full-range system AND laptop speakers)
- [ ] No sibilance artifacts in narrator track
- [ ] Reverb tails do not bleed into silence sections
- [ ] Fade-out at end reaches true silence by 5:00.0

### Playback Testing

The final mix must be tested on:

| System                        | What to Check                                                        |
| ----------------------------- | -------------------------------------------------------------------- |
| Studio monitors (full range)  | Overall balance, sub-bass, stereo image, detail                      |
| Headphones (open-back)        | Spatial accuracy, stereo field, quiet detail in Act IV               |
| Headphones (consumer, AirPods)| Narrator clarity, overall tonal balance, no harsh frequencies        |
| Laptop speakers (MacBook)     | Narrator must be clearly intelligible. Sub-bass will be absent — ensure the mix still works emotionally without it. |
| Phone speaker                 | Narrator must still be audible and intelligible. Music may be minimal — that's acceptable. |
| TV / soundbar                 | General balance check. This is a common YouTube viewing scenario.    |