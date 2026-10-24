# Scene 6 — Act IV: See It Work

| Field | Detail |
|---|---|
| **Document** | `07_SCENE_ACT_IV_DEMO.md` |
| **Parent** | `00_PRODUCTION_BIBLE.md` |
| **Version** | 1.0 |
| **Status** | PRE-PRODUCTION |
| **Scene** | 6 of 7 |
| **Timecode** | 4:15.000–4:45.000 |
| **Duration** | 30.000 seconds |
| **FPS** | 60 fps |
| **Canvas** | 3840×2160 |

---

## Table of Contents

1. [Scene Overview](#scene-overview)
2. [Philosophy](#philosophy)
3. [Design System Reference](#design-system-reference)
   - [Color Palette](#color-palette)
   - [Typography Scale](#typography-scale)
4. [Shot List](#shot-list)
   - [SHOT 6.1 — wt init](#shot-61--wt-init)
   - [SHOT 6.2 — wt status](#shot-62--wt-status)
   - [SHOT 6.3 — wt status --team](#shot-63--wt-status---team)
   - [SHOT 6.4 — wt snapshot](#shot-64--wt-snapshot)
   - [SHOT 6.5 — wt push](#shot-65--wt-push)
   - [SHOT 6.6 — wt access test](#shot-66--wt-access-test)
   - [SHOT 6.7 — Narrator Returns](#shot-67--narrator-returns)
5. [Audio Design](#audio-design)
   - [Music](#music)
   - [SFX](#sfx)
6. [Terminal Typography & Color Reference](#terminal-typography--color-reference)
   - [Font Stack](#font-stack)
   - [Size & Spacing](#size--spacing)
   - [Complete Color Map](#complete-color-map)
7. [Frame-Accurate Timing Reference](#frame-accurate-timing-reference)
8. [Editorial Notes](#editorial-notes)
9. [Continuity & Handoff](#continuity--handoff)

---

## Scene Overview

Act IV is the most important 30 seconds after the Cold Open. Its job is singular: **prove it**. After three acts of narrative — history, problems, architecture — the viewer has been told that W0rkTree is better. Now they need to *see* it. This scene is pure terminal. Real commands. Real output. The product speaks for itself.

The viewer arrives here convinced by the architecture diagrams and feature cards of Act III. They are intellectually engaged but not yet *believing*. Act IV converts understanding into belief through the oldest trick in software: a working demo.

**Purpose**: Proof. Show the product working. Pure terminal. No decoration.

**Narrative function**: After explaining the architecture, SHOW it working. The terminal is the evidence that everything promised in Acts I–III is real and functional.

**Emotional Arc**: The viewer is convinced by the architecture — now they see it's real.

**Constraint**: Every command and every output line must be legible at 1080p on a mobile device held at arm's length. If any text requires squinting, it has failed. Readability is the non-negotiable constraint of this scene.

---

## Philosophy

This scene is the antithesis of Act III. No diagrams, no motion graphics, no cards. Just a terminal. Real commands. Real output. The confidence comes from simplicity — the product works, and it doesn't need decoration.

The terminal styling matches the Cold Open, creating a **bookend**: the video opened with a terminal full of Git pain; it closes (before the CTA) with a terminal full of W0rkTree clarity. The visual symmetry is deliberate. Same terminal component, same font, same background, same prompt style — but the *content* is the opposite. Where the Cold Open showed rejection, conflict, and data loss, Act IV shows initialization, visibility, and access control. The terminal itself hasn't changed. The tool inside it has.

This bookend is the most important structural decision in the video. It tells the viewer, without a single word of narration: *"Remember that pain? It's gone now."*

---

## Design System Reference

All values below are drawn from `01_DESIGN_SYSTEM.md`. This section is a scene-local quick reference — in any conflict, the design system document wins.

### Color Palette

| Token | Name | Hex | Role in This Scene |
|---|---|---|---|
| `deep-navy` | Deep Navy | `#0A0F1A` | Primary canvas background. Every frame opens and closes on this color. |
| `code-bg` | Code Background | `#111827` | Terminal panel fill. The slightly-lighter-than-navy surface that all terminal content sits on. |
| `pure-white` | White | `#F0F0F0` | Primary terminal text. Command arguments, output body, labels. |
| `accent-cyan` | Accent Cyan | `#00D4FF` | W0rkTree command keyword (`wt`), file paths, branch names, snapshot IDs, URLs. The eye-draw color. |
| `warning-red` | Warning Red | `#FF3B3B` | DENIED output, permission test failures, the `✗` marker. Used only in SHOT 6.6. |
| `confident-green` | Confident Green | `#00C48F` | Success checkmarks (`✓`), positive status indicators (`running, auto-sync active`). |
| `muted-gray` | Muted Gray | `#6B7280` | Terminal prompt (`$`), secondary labels (`Branch:`, `Worker:`, `Policy:`), flag text (`--message`), separators (`—`). |
| `dim-gray` | Dim Gray | `#374151` | Not used in terminal output for this scene. Reserved for subtle borders if terminal chrome is visible. |

### Typography Scale

All sizes are defined at 4K (3840 × 2160). Divide by 2 for HD (1920 × 1080).

| Token | 4K Size | HD Size | Weight | Font | Use in This Scene |
|---|---|---|---|---|---|
| `display-lg` | 128 px | 64 px | 800 (ExtraBold) | Inter Tight | Not used in this scene. |
| `heading-1` | 96 px | 48 px | 700 (Bold) | Inter | Not used in this scene. |
| `heading-2` | 64 px | 32 px | 600 (SemiBold) | Inter | SHOT 6.7 narrator text overlay. |
| `heading-3` | 48 px | 24 px | 600 (SemiBold) | Inter | Not used in this scene. |
| `body-lg` | 40 px | 20 px | 400 (Regular) | Inter | Not used in this scene. |
| `tagline` | 56 px | 28 px | 400 (Regular) | Inter | Not used in this scene. |
| `code-lg` | 48 px | 24 px | 400 (Regular) | JetBrains Mono | Terminal commands (the typed input line). Primary terminal text size. |
| `code` | 36 px | 18 px | 400 (Regular) | JetBrains Mono | Terminal output lines. |
| `code-sm` | 28 px | 14 px | 400 (Regular) | JetBrains Mono | Not used in this scene — output must remain at `code` minimum for readability. |

> **Scene-specific override**: If `code` (36px at 4K / 18px at HD) proves too small for terminal output on mobile at arm's length, the editor may increase terminal output to `code-lg` (48px at 4K / 24px at HD) for this scene only. Readability > design purity. Log the override in the asset manifest.

---

## Shot List

Act IV is structured as **six quick-cut terminal shots** followed by a narrator overlay. Each shot is a separate terminal "moment" — the frame clears between each (hard cut to a fresh terminal state). No transitions, no fades between terminal shots. Cut. New terminal. Type. Output. Cut.

Every terminal shot uses the same visual setup:

- **Background**: Deep Navy (`#0A0F1A`) canvas with Code Background (`#111827`) terminal panel centered
- **Terminal panel**: No window chrome. No title bar. No scrollbar. No decorative borders. Just the code surface.
- **Cursor**: Thin vertical bar, `#F0F0F0`, blinking at 530ms on / 530ms off. Solid (not blinking) during typing. Resumes blinking 530ms after typing stops.
- **Typing speed**: 40ms per character. A competent developer at ~60 WPM. Not fast, not slow. Real.
- **Line-height**: 1.6 (per design system monospace specification)

---

### SHOT 6.1 — wt init

| Field | Detail |
|---|---|
| **Timecode** | 4:15.000–4:19.500 |
| **Duration** | 4.500 seconds |
| **Purpose** | First contact. The viewer sees W0rkTree initialize a project in one command. Clean, fast, done. |

#### Terminal State

Empty terminal. Deep Navy (`#0A0F1A`) background, Code Background (`#111827`) terminal panel, no window chrome. Cursor blinks once before typing begins (530ms on-state visible).

#### Typed Command

Typing begins at 4:15.000. Speed: 40ms per character.

```
$ wt init my-project
```

Total characters (excluding `$ `): 18. Typing duration: 720ms. Typing completes at approximately 4:15.720.

#### Pause

300ms after typing completes. Output begins at approximately 4:16.020.

#### Output

Appears line-by-line, 80ms between lines:

```
✓ Worktree initialized at ./my-project
✓ Worker started (PID 4821)
✓ Connected to wt.company.com as alice@company.com
```

Three lines. Total output duration: 160ms (80ms × 2 gaps). All lines visible by approximately 4:16.180.

#### Text Colors

| Element | Color | Hex | Opacity | Weight |
|---|---|---|---|---|
| `$` prompt | Muted Gray | `#6B7280` | 100% | 400 |
| `wt` command keyword | Accent Cyan | `#00D4FF` | 100% | 400 |
| `init my-project` arguments | White | `#F0F0F0` | 100% | 400 |
| `✓` checkmark (all three) | Confident Green | `#00C48F` | 100% | 400 |
| `./my-project` path | Accent Cyan | `#00D4FF` | 100% | 400 |
| `wt.company.com` server URL | Accent Cyan | `#00D4FF` | 100% | 400 |
| `alice@company.com` identity | White | `#F0F0F0` | 100% | 400 |
| Remaining output text | White | `#F0F0F0` | 80% | 400 |

#### Audio

- **Keyboard clicks**: Synced to each character being typed. Real mechanical keyboard sound (Cherry MX Blue or equivalent). Same keyboard sound as Cold Open — this creates the bookend. Level: −20 dB.
- **Complete chime**: After all output appears, a very subtle "complete" chime. Level: −32 dB. A soft tonal blip — not a notification sound, just a gentle tonal acknowledgment. Duration: ~200ms.
- **Background**: Ambient tone only, sustained pad at −34 dB LUFS. Almost no music. The terminal sounds ARE the music.

#### Hold & Transition

500ms hold after last output line. Cursor blinks once during the hold. Then **hard cut** to SHOT 6.2.

---

### SHOT 6.2 — wt status

| Field | Detail |
|---|---|
| **Timecode** | 4:19.500–4:23.500 |
| **Duration** | 4.000 seconds |
| **Purpose** | Show the developer their current state — branch, worker, modified files, staged snapshots. Clean status at a glance. |

#### Typed Command

```
$ wt status
```

Total characters (excluding `$ `): 9. Typing duration: 360ms.

#### Pause

300ms after typing completes.

#### Output

Appears line-by-line, 80ms between lines:

```
Branch: main
Worker: running, auto-sync active

Modified:
  services/auth-service/src/oauth.rs
  libs/shared-models/src/user.rs

Staged snapshots (not yet pushed): 2
```

Eight lines (including two blank separator lines). Total output duration: 560ms (80ms × 7 gaps).

#### Text Colors

| Element | Color | Hex | Opacity | Weight |
|---|---|---|---|---|
| `$` prompt | Muted Gray | `#6B7280` | 100% | 400 |
| `wt` command keyword | Accent Cyan | `#00D4FF` | 100% | 400 |
| `status` argument | White | `#F0F0F0` | 100% | 400 |
| `Branch:` label | Muted Gray | `#6B7280` | 100% | 400 |
| `main` branch value | Accent Cyan | `#00D4FF` | 100% | 400 |
| `Worker:` label | Muted Gray | `#6B7280` | 100% | 400 |
| `running, auto-sync active` status | Confident Green | `#00C48F` | 100% | 400 |
| `Modified:` header | White | `#F0F0F0` | 100% | 400 |
| `services/auth-service/src/oauth.rs` path | Accent Cyan | `#00D4FF` | 100% | 400 |
| `libs/shared-models/src/user.rs` path | Accent Cyan | `#00D4FF` | 100% | 400 |
| `Staged snapshots (not yet pushed):` text | White | `#F0F0F0` | 80% | 400 |
| `2` count | Accent Cyan | `#00D4FF` | 100% | 400 |

> **Design note**: Individual file paths are ALWAYS cyan. This draws the eye to the specific files and reinforces that W0rkTree thinks in terms of real paths, not abstract diffs.

#### Audio

- **Keyboard clicks**: Synced to typing. Level: −20 dB.
- **Complete chime**: After output appears. Level: −32 dB.
- **Background**: Ambient pad continues at −34 dB LUFS.

#### Hold & Transition

400ms hold after last output line. **Hard cut** to SHOT 6.3.

---

### SHOT 6.3 — wt status --team

| Field | Detail |
|---|---|
| **Timecode** | 4:23.500–4:28.500 |
| **Duration** | 5.000 seconds |
| **Purpose** | **HERO SHOT.** This is the proof of Staged Snapshot Visibility — the thing Git cannot do. The viewer sees not just their own work, but their teammates' in-progress changes. This is the single most important terminal output in the entire video. |

#### Typed Command

```
$ wt status --team
```

Total characters (excluding `$ `): 16. Typing duration: 640ms.

#### Pause

300ms after typing completes.

#### Output

Appears line-by-line, 80ms between lines:

```
Your staged work:
  2 snapshots on main (auth-service/src/oauth.rs, shared-models/src/user.rs)

Teammates:
  bob@company.com  — 1 snapshot on fix/token-expiry (auth-service/src/tokens.rs)
  carol@company.com — 3 snapshots on feature/billing (billing-engine/src/pricing.rs)
```

Six lines (including one blank separator line). Total output duration: 400ms (80ms × 5 gaps).

#### Text Colors

| Element | Color | Hex | Opacity | Weight |
|---|---|---|---|---|
| `$` prompt | Muted Gray | `#6B7280` | 100% | 400 |
| `wt` command keyword | Accent Cyan | `#00D4FF` | 100% | 400 |
| `status --team` arguments | White | `#F0F0F0` | 100% | 400 |
| `Your staged work:` header | White | `#F0F0F0` | 100% | **700** |
| `2` snapshot count | Accent Cyan | `#00D4FF` | 100% | 400 |
| `main` branch name | Accent Cyan | `#00D4FF` | 100% | 400 |
| `auth-service/src/oauth.rs` path | Accent Cyan | `#00D4FF` | 100% | 400 |
| `shared-models/src/user.rs` path | Accent Cyan | `#00D4FF` | 100% | 400 |
| `Teammates:` header | White | `#F0F0F0` | 100% | **700** |
| `bob@company.com` email | White | `#F0F0F0` | 100% | 400 |
| `carol@company.com` email | White | `#F0F0F0` | 100% | 400 |
| `—` separators | Muted Gray | `#6B7280` | 100% | 400 |
| `1` snapshot count | Accent Cyan | `#00D4FF` | 100% | 400 |
| `3` snapshot count | Accent Cyan | `#00D4FF` | 100% | 400 |
| `fix/token-expiry` branch name | Accent Cyan | `#00D4FF` | 100% | 400 |
| `feature/billing` branch name | Accent Cyan | `#00D4FF` | 100% | 400 |
| `auth-service/src/tokens.rs` path | Accent Cyan | `#00D4FF` | 100% | 400 |
| `billing-engine/src/pricing.rs` path | Accent Cyan | `#00D4FF` | 100% | 400 |
| Remaining text (`snapshots on`, `snapshot on`, parentheses) | White | `#F0F0F0` | 80% | 400 |

#### Special Treatment

**Hold this output for 2 full seconds** after it finishes appearing. Let the viewer READ it. This is the proof of staged snapshot visibility — the thing no other VCS does. The cursor blinks during the hold. No animation, no highlighting, no progressive reveal. Just the output, sitting there, being true.

The 2-second hold is non-negotiable. If the scene needs to be shortened for time, take the time from another shot — never from this hold.

#### Audio

- **Keyboard clicks**: Synced to typing. Level: −20 dB.
- **Complete chime**: After output appears. Level: −32 dB. Slightly warmer in tone than previous chimes — this is the hero moment.
- **Background**: Ambient pad continues at −34 dB LUFS.

#### Hold & Transition

2000ms hold (as specified above). **Hard cut** to SHOT 6.4.

---

### SHOT 6.4 — wt snapshot

| Field | Detail |
|---|---|
| **Timecode** | 4:28.500–4:32.000 |
| **Duration** | 3.500 seconds |
| **Purpose** | Show the snapshot workflow — one command to capture work with a human-readable message. Not a commit, not a stash. A snapshot. |

#### Typed Command

```
$ wt snapshot --message "OAuth token rotation with configurable expiry"
```

Total characters (excluding `$ `): 62. Typing duration: 2480ms. This is the longest command in the scene. The typing itself demonstrates that the message is descriptive and human-readable — the viewer reads along as the developer types.

#### Pause

300ms after typing completes.

#### Output

Appears line-by-line, 80ms between lines:

```
✓ Snapshot snap_f1e2d3 created
✓ Synced to server (staged, not pushed)
```

Two lines. Total output duration: 80ms.

#### Text Colors

| Element | Color | Hex | Opacity | Weight |
|---|---|---|---|---|
| `$` prompt | Muted Gray | `#6B7280` | 100% | 400 |
| `wt` command keyword | Accent Cyan | `#00D4FF` | 100% | 400 |
| `snapshot` argument | White | `#F0F0F0` | 100% | 400 |
| `--message` flag | Muted Gray | `#6B7280` | 100% | 400 |
| `"OAuth token rotation with configurable expiry"` message string | White | `#F0F0F0` | 100% | 400 |
| `✓` checkmarks (both) | Confident Green | `#00C48F` | 100% | 400 |
| `snap_f1e2d3` snapshot ID | Accent Cyan | `#00D4FF` | 100% | 400 |
| `created` | White | `#F0F0F0` | 80% | 400 |
| `Synced to server` | White | `#F0F0F0` | 80% | 400 |
| `(staged, not pushed)` parenthetical | Muted Gray | `#6B7280` | 100% | 400 |

> **Design note**: The parenthetical `(staged, not pushed)` is rendered in Muted Gray to visually de-emphasize it, but its *content* is critical — it reinforces the core concept of staged snapshots one more time. The viewer absorbs this reinforcement subconsciously through repetition.

#### Audio

- **Keyboard clicks**: Synced to typing. Level: −20 dB. The long typing sequence creates a satisfying rhythm.
- **Complete chime**: After output appears. Level: −32 dB.
- **Background**: Ambient pad continues at −34 dB LUFS.

#### Hold & Transition

400ms hold. **Hard cut** to SHOT 6.5.

---

### SHOT 6.5 — wt push

| Field | Detail |
|---|---|
| **Timecode** | 4:32.000–4:35.500 |
| **Duration** | 3.500 seconds |
| **Purpose** | Show the push — multiple snapshots pushed in one command. Clean, fast, no merge drama. The output shows a hash progression, proving that snapshots have a real, ordered history. |

#### Typed Command

```
$ wt push
```

Total characters (excluding `$ `): 7. Typing duration: 280ms.

#### Pause

300ms after typing completes.

#### Output

Appears line-by-line, 80ms between lines:

```
✓ Pushed 3 snapshots to main
✓ Branch main updated: snap_7a8b9c → snap_f1e2d3
```

Two lines. Total output duration: 80ms.

#### Text Colors

| Element | Color | Hex | Opacity | Weight |
|---|---|---|---|---|
| `$` prompt | Muted Gray | `#6B7280` | 100% | 400 |
| `wt` command keyword | Accent Cyan | `#00D4FF` | 100% | 400 |
| `push` argument | White | `#F0F0F0` | 100% | 400 |
| `✓` checkmarks (both) | Confident Green | `#00C48F` | 100% | 400 |
| `3` count | Accent Cyan | `#00D4FF` | 100% | 400 |
| `main` branch name (both occurrences) | Accent Cyan | `#00D4FF` | 100% | 400 |
| `→` arrow | Muted Gray | `#6B7280` | 100% | 400 |
| `snap_7a8b9c` snapshot ID (old) | Accent Cyan | `#00D4FF` | 100% | 400 |
| `snap_f1e2d3` snapshot ID (new) | Accent Cyan | `#00D4FF` | 100% | 400 |
| Remaining text (`Pushed`, `snapshots to`, `Branch`, `updated:`) | White | `#F0F0F0` | 80% | 400 |

> **Continuity note**: The snapshot ID `snap_f1e2d3` matches the one created in SHOT 6.4. This is deliberate — the viewer who reads closely sees that the snapshots have a consistent, traceable identity across commands. The old hash `snap_7a8b9c` implies there were previous snapshots already on the server, reinforcing that this is an ongoing workflow, not a first-time setup.

#### Audio

- **Keyboard clicks**: Synced to typing. Level: −20 dB.
- **Complete chime**: After output appears. Level: −32 dB.
- **Background**: Ambient pad continues at −34 dB LUFS.

#### Hold & Transition

500ms hold. **Hard cut** to SHOT 6.6.

---

### SHOT 6.6 — wt access test

| Field | Detail |
|---|---|
| **Timecode** | 4:35.500–4:40.500 |
| **Duration** | 5.000 seconds |
| **Purpose** | **"WOW" SHOT.** Proof that access control is built into the protocol and works. This is the payoff for Act II Problem 4 ("Git has no access control. None."). The output is a denial — but a *beautiful* denial. Clear, readable, self-explanatory. No cryptic error codes. The system tells you exactly what happened and why. |

#### Typed Command

```
$ wt access test intern@company.com tree:write config/production.toml
```

Total characters (excluding `$ `): 60. Typing duration: 2400ms.

#### Pause

300ms after typing completes.

#### Output

Appears line-by-line, 80ms between lines:

```
✗ DENIED
  Policy: "lock-production-config" (deny tree:write on registered path config/production.toml)
  Scope: RegisteredPath
  Source: .wt/access/policies.toml line 34
```

Four lines. Total output duration: 240ms (80ms × 3 gaps).

#### Text Colors

| Element | Color | Hex | Opacity | Weight |
|---|---|---|---|---|
| `$` prompt | Muted Gray | `#6B7280` | 100% | 400 |
| `wt` command keyword | Accent Cyan | `#00D4FF` | 100% | 400 |
| `access test` subcommand | Accent Cyan | `#00D4FF` | 100% | 400 |
| `intern@company.com` identity | White | `#F0F0F0` | 100% | 400 |
| `tree:write` permission | Warning Red | `#FF3B3B` | 100% | 400 |
| `config/production.toml` path (in command) | Accent Cyan | `#00D4FF` | 100% | 400 |
| `✗` marker | Warning Red | `#FF3B3B` | 100% | 400 |
| `DENIED` | Warning Red | `#FF3B3B` | 100% | **700** |
| `Policy:` label | Muted Gray | `#6B7280` | 100% | 400 |
| `"lock-production-config"` policy name | White | `#F0F0F0` | 100% | 400 |
| `deny` keyword (in parenthetical) | Warning Red | `#FF3B3B` | 100% | 400 |
| `tree:write` permission (in parenthetical) | Warning Red | `#FF3B3B` | 100% | 400 |
| `registered path` text (in parenthetical) | White | `#F0F0F0` | 80% | 400 |
| `config/production.toml` path (in parenthetical) | Accent Cyan | `#00D4FF` | 100% | 400 |
| `Scope:` label | Muted Gray | `#6B7280` | 100% | 400 |
| `RegisteredPath` scope value | White | `#F0F0F0` | 100% | 400 |
| `Source:` label | Muted Gray | `#6B7280` | 100% | 400 |
| `.wt/access/policies.toml` source path | Accent Cyan | `#00D4FF` | 100% | 400 |
| `line 34` line number | White | `#F0F0F0` | 100% | 400 |
| `on` conjunction (in parenthetical) | White | `#F0F0F0` | 80% | 400 |

> **Design note**: The `tree:write` permission is rendered in Warning Red both in the command and the output. In the command, it signals "this is the permission being tested — and it's about to be denied." In the output, it echoes the denial reason. Red is used here not as an error state (the system worked correctly) but as a *denial* state — the access control did exactly what it was supposed to do.

> **Emotional note**: This output proves three things simultaneously: (1) the access system works, (2) it's readable — no cryptic codes, no opaque messages, and (3) it explains itself — the viewer can trace the denial back to a specific policy, a specific scope, and a specific line in a specific file. This is the antithesis of Git's access model (which is: there is no access model).

#### Special Treatment

**Hold this output for 2 full seconds** after it finishes appearing. Same rationale as SHOT 6.3 — this is a hero moment. The viewer needs time to read and absorb the output. The cursor blinks. Nothing moves. The output just sits there, being clear.

#### Audio

- **Keyboard clicks**: Synced to typing. Level: −20 dB.
- **Low negative tone**: At the moment `✗ DENIED` appears, a brief, low-pitched "denied" tone plays. Level: −28 dB. This is a soft buzzer — not harsh, not alarming. Think a gentle low-frequency pulse (80Hz fundamental, 100ms duration, fast decay). It communicates "no" without communicating "error." The system is working correctly; it's just saying no.
- **No complete chime**: This shot does NOT get the success chime. The denied tone replaces it.
- **Background**: Ambient pad continues at −34 dB LUFS.

#### Hold & Transition

2000ms hold (as specified above). **Hard cut** to SHOT 6.7.

---

### SHOT 6.7 — Narrator Returns

| Field | Detail |
|---|---|
| **Timecode** | 4:40.500–4:45.000 |
| **Duration** | 4.500 seconds |
| **Purpose** | The narrator names what the viewer just saw. A single, calm statement that connects the demo to the product's values. Then transition to the Close. |

#### Visual

The terminal from SHOT 6.6 dims to **30% opacity** over 400ms (ease-standard). It does not disappear — it remains visible as a faint ghost behind the overlay text. This preserves the terminal context while shifting focus to the narrator's words.

Over the dimmed terminal, centered on screen:

> "One command per job. Plain language.
> Human-readable errors that tell you exactly
> what happened and why."

**Text specifications**:

| Property | Value |
|---|---|
| Font | Inter |
| Weight | 600 (SemiBold) |
| Size | `heading-2` — 64px at 4K / 32px at HD |
| Color | `#F0F0F0` (White) at 100% opacity |
| Alignment | Centered horizontally and vertically on canvas |
| Line height | 1.3 |
| Max width | 70% of canvas width |

**Text animation**: All text appears together as a single block. Ease-enter (cubic-bezier 0.0, 0.0, 0.2, 1.0), 600ms duration, translate-y +20px → 0 combined with opacity 0% → 100%.

#### Narrator V.O.

> "One command per job. Plain language. Human-readable errors that tell you exactly what happened and why."

**Delivery direction**: Calm. Confident. Not smug — *satisfied*. The product delivered on the promise. The narrator's tone should convey the quiet pride of someone showing you something they built and watching you understand it. No urgency. No sales pitch. Just observation.

Emphasized words: "One," "Plain," "exactly."

Delivery begins at 4:40.500. Estimated duration: 4.0 seconds. The narrator finishes by approximately 4:44.500, leaving a 500ms breath before the scene transition.

#### Transition to Close

At 4:43.000 (2 seconds after the text appears, while narrator is finishing or has just finished): all elements — dimmed terminal, overlay text — begin fading to Deep Navy (`#0A0F1A`). Duration: 500ms, ease-exit (cubic-bezier 0.4, 0.0, 1.0, 1.0). By 4:43.500 the overlay text is gone. The dimmed terminal continues fading.

At 4:44.500: screen reaches pure Deep Navy. Holds at Deep Navy through 4:45.000. This 500ms of pure Deep Navy is the breath between Act IV and the Close — the same structural beat as the held black between the Cold Open and the Title Card.

At 4:45.000: Scene 7 (Close) begins. Handoff to `08_SCENE_CLOSE.md`.

---

## Audio Design

### Music

| Timecode | Description | Level | Notes |
|---|---|---|---|
| 4:15.000–4:40.500 | Sustained ambient pad | −34 dB LUFS | Music from Act III has faded to near-silence. Just a single held tone — warm, analog-sounding synth pad. Barely audible. Creates space without emptiness. The terminal sounds ARE the music of this section. The pad provides the barest sense of continuity so the scene doesn't feel like dead air, but it must never compete with the keyboard clicks or output chimes. |
| 4:40.500–4:45.000 | Ambient pad swell | −34 → −28 dB LUFS | The pad swells by approximately +6 dB over 3 seconds (4:40.500–4:43.500), transitioning into the Close. This swell coincides with the narrator's overlay and signals the emotional shift from "proof" to "invitation." The swell should feel like a deep breath in — anticipation of resolution. |

> **Critical constraint**: No melodic content, no rhythmic elements, no arpeggios during 4:15–4:40. The ambient pad is a single sustained tone or a very slow chord (root + 5th, no 3rd — harmonically neutral). Any musical "interest" during the terminal shots would compete with the typing rhythm and undermine the scene's core principle: the product speaks for itself.

### SFX

| Timecode | Sound | Level | Duration | Notes |
|---|---|---|---|---|
| 4:15.000–4:40.500 | Keyboard typing | −20 dB | Synced to each typed character | Real mechanical keyboard. Cherry MX Blue or equivalent. **Same sound source as Cold Open** — this is the bookend. Same keyboard, same mic position, same recording. The only difference is what's being typed. Record or select enough keystroke variations (8–12 unique samples) to avoid audible repetition. Natural velocity variation between keystrokes. |
| ~4:16.180 | Subtle complete chime | −32 dB | ~200ms | After SHOT 6.1 output appears. Extremely quiet tonal blip — a soft sine or triangle wave, high register (~2kHz), fast decay. Not a notification sound. Not a bell. Just a gentle tonal acknowledgment that something completed. |
| ~4:20.560 | Subtle complete chime | −32 dB | ~200ms | After SHOT 6.2 output appears. Same sound as above. |
| ~4:25.340 | Subtle complete chime (warm) | −32 dB | ~200ms | After SHOT 6.3 output appears. Same fundamental but with a slightly warmer character (add subtle 2nd harmonic at −6 dB relative). This is the hero shot; the chime should feel fractionally more satisfying. |
| ~4:31.360 | Subtle complete chime | −32 dB | ~200ms | After SHOT 6.4 output appears. Standard chime. |
| ~4:33.160 | Subtle complete chime | −32 dB | ~200ms | After SHOT 6.5 output appears. Standard chime. |
| ~4:38.440 | Low negative tone | −28 dB | ~300ms | On `✗ DENIED` appearance in SHOT 6.6. A brief, low-pitched denial tone. 80Hz sine fundamental with gentle 2nd harmonic (160Hz at −6 dB). Attack: 5ms. Decay: 250ms exponential. Not harsh — a soft buzzer. Think "access denied" in a calm security system, not "ERROR" in a broken machine. This is 4 dB louder than the complete chimes because it needs to register as distinct. |

> **SFX mixing note**: The keyboard clicks, complete chimes, and denial tone are the ONLY sounds in this scene besides the ambient pad and the narrator V.O. in SHOT 6.7. No whooshes, no transitions, no risers, no stingers. The sparsity is the design.

---

## Terminal Typography & Color Reference

This section consolidates every text style used in Act IV for quick reference during production.

### Font Stack

| Priority | Font | Fallback Use |
|---|---|---|
| 1 | JetBrains Mono | Primary — all terminal text (commands and output) |
| 2 | Fira Code | Fallback if JetBrains Mono licensing is unavailable |
| 3 | Source Code Pro | System fallback |

> **Ligatures**: DISABLED. Set `font-feature-settings: "liga" 0, "calt" 0;` explicitly. We want literal character sequences: `--` not em-dash, `->` not arrow, `!=` not `≠`.

### Size & Spacing

| Property | Value | Notes |
|---|---|---|
| Command font size | `code-lg`: 48px at 4K / 24px at HD | Typed commands are the largest text in the terminal. |
| Output font size | `code`: 36px at 4K / 18px at HD | Output is one step smaller. May be increased to `code-lg` if readability testing fails on mobile. |
| Line height | 1.6 | Generous spacing. Terminal output should breathe. |
| Letter spacing | 0 (default monospace) | Do not adjust tracking. |
| Tab width | 2 spaces | Used for indented output lines (e.g., file paths under `Modified:`, policy details under `✗ DENIED`). |

### Complete Color Map

| Token | Hex | Opacity | Weight | Used For |
|---|---|---|---|---|
| `terminal-bg` | `#111827` | 100% | — | Terminal panel background |
| `canvas-bg` | `#0A0F1A` | 100% | — | Canvas behind terminal panel, SHOT 6.7 fade target |
| `prompt` | `#6B7280` | 100% | 400 | `$` prompt character |
| `wt-command` | `#00D4FF` | 100% | 400 | `wt` keyword, `access test` subcommand |
| `command-args` | `#F0F0F0` | 100% | 400 | Command arguments (`init my-project`, `status`, `push`) |
| `command-flag` | `#6B7280` | 100% | 400 | Flags (`--message`, `--team`) |
| `output-primary` | `#F0F0F0` | 80% | 400 | Default terminal output text |
| `output-header` | `#F0F0F0` | 100% | 700 | Bold headers in output (`Your staged work:`, `Teammates:`, `Modified:`) |
| `label` | `#6B7280` | 100% | 400 | Field labels (`Branch:`, `Worker:`, `Policy:`, `Scope:`, `Source:`) |
| `separator` | `#6B7280` | 100% | 400 | `—` separators, `→` arrows |
| `path` | `#00D4FF` | 100% | 400 | All file paths, URLs, server addresses |
| `branch` | `#00D4FF` | 100% | 400 | Branch names (`main`, `fix/token-expiry`, `feature/billing`) |
| `snapshot-id` | `#00D4FF` | 100% | 400 | Snapshot hashes (`snap_f1e2d3`, `snap_7a8b9c`) |
| `count` | `#00D4FF` | 100% | 400 | Numeric counts (`2`, `3`, `1`) |
| `success-check` | `#00C48F` | 100% | 400 | `✓` checkmarks |
| `success-status` | `#00C48F` | 100% | 400 | Positive statuses (`running, auto-sync active`) |
| `denied-marker` | `#FF3B3B` | 100% | 400 | `✗` marker |
| `denied-text` | `#FF3B3B` | 100% | 700 | `DENIED` keyword |
| `denied-keyword` | `#FF3B3B` | 100% | 400 | `deny`, `tree:write` (permission being denied) |
| `identity` | `#F0F0F0` | 100% | 400 | Email addresses (`alice@company.com`, `intern@company.com`) |
| `parenthetical` | `#6B7280` | 100% | 400 | De-emphasized parenthetical notes (`(staged, not pushed)`) |
| `overlay-text` | `#F0F0F0` | 100% | 600 | SHOT 6.7 narrator overlay text |
| `cursor` | `#F0F0F0` | 100% | — | Terminal cursor (thin vertical bar) |

---

## Frame-Accurate Timing Reference

For the editor's reference, here is the complete timing grid with frame numbers at 60fps:

| Event | Timecode | Frame # (60fps) | Notes |
|---|---|---|---|
| **SHOT 6.1 begins** | 4:15.000 | 15300 | Hard cut from Act III. Empty terminal. |
| `$ wt init my-project` typing begins | 4:15.000 | 15300 | 18 chars × 40ms = 720ms |
| Typing completes | 4:15.720 | 15343 | |
| Output begins | 4:16.020 | 15361 | 300ms pause after typing |
| All output visible | 4:16.180 | 15371 | 3 lines, 80ms gaps |
| Complete chime | 4:16.180 | 15371 | −32 dB |
| Hold ends | 4:16.680 | 15401 | 500ms hold |
| **SHOT 6.2 begins** | 4:19.500 | 15570 | Hard cut. Fresh terminal. |
| `$ wt status` typing begins | 4:19.500 | 15570 | 9 chars × 40ms = 360ms |
| Typing completes | 4:19.860 | 15592 | |
| Output begins | 4:20.160 | 15610 | 300ms pause |
| All output visible | 4:20.560 | 15634 | 8 lines (incl. blanks), 80ms gaps |
| Complete chime | 4:20.560 | 15634 | −32 dB |
| Hold ends | 4:20.960 | 15658 | 400ms hold |
| **SHOT 6.3 begins** | 4:23.500 | 15810 | Hard cut. Fresh terminal. HERO SHOT. |
| `$ wt status --team` typing begins | 4:23.500 | 15810 | 16 chars × 40ms = 640ms |
| Typing completes | 4:24.140 | 15848 | |
| Output begins | 4:24.440 | 15866 | 300ms pause |
| All output visible | 4:24.840 | 15890 | 6 lines (incl. blank), 80ms gaps |
| Complete chime (warm) | 4:24.840 | 15890 | −32 dB, warm variant |
| 2-second hold begins | 4:24.840 | 15890 | Non-negotiable hold |
| Hold ends | 4:26.840 | 16010 | |
| **SHOT 6.4 begins** | 4:28.500 | 16110 | Hard cut. Fresh terminal. |
| `$ wt snapshot --message "..."` typing begins | 4:28.500 | 16110 | 62 chars × 40ms = 2480ms |
| Typing completes | 4:30.980 | 16259 | |
| Output begins | 4:31.280 | 16277 | 300ms pause |
| All output visible | 4:31.360 | 16282 | 2 lines, 80ms gap |
| Complete chime | 4:31.360 | 16282 | −32 dB |
| Hold ends | 4:31.760 | 16306 | 400ms hold |
| **SHOT 6.5 begins** | 4:32.000 | 16320 | Hard cut. Fresh terminal. |
| `$ wt push` typing begins | 4:32.000 | 16320 | 7 chars × 40ms = 280ms |
| Typing completes | 4:32.280 | 16337 | |
| Output begins | 4:32.580 | 16355 | 300ms pause |
| All output visible | 4:32.660 | 16360 | 2 lines, 80ms gap |
| Complete chime | 4:32.660 | 16360 | −32 dB |
| Hold ends | 4:33.160 | 16390 | 500ms hold |
| **SHOT 6.6 begins** | 4:35.500 | 16530 | Hard cut. Fresh terminal. WOW SHOT. |
| `$ wt access test ...` typing begins | 4:35.500 | 16530 | 60 chars × 40ms = 2400ms |
| Typing completes | 4:37.900 | 16674 | |
| Output begins | 4:38.200 | 16692 | 300ms pause |
| `✗ DENIED` appears | 4:38.200 | 16692 | First output line |
| Low negative tone | 4:38.200 | 16692 | −28 dB, 300ms duration |
| All output visible | 4:38.440 | 16706 | 4 lines, 80ms gaps |
| 2-second hold begins | 4:38.440 | 16706 | Non-negotiable hold |
| Hold ends | 4:40.440 | 16826 | |
| **SHOT 6.7 begins** | 4:40.500 | 16830 | Terminal dims to 30% over 400ms. |
| Terminal dim complete | 4:40.900 | 16854 | |
| Narrator overlay text enters | 4:41.000 | 16860 | ease-enter, 600ms, translate-y +20→0 |
| Text animation complete | 4:41.600 | 16896 | |
| Narrator V.O. begins | 4:40.500 | 16830 | Coincides with SHOT 6.7 start |
| All elements begin fade to Deep Navy | 4:43.000 | 16980 | 500ms ease-exit |
| Overlay text gone | 4:43.500 | 17010 | |
| Pure Deep Navy reached | 4:44.500 | 17070 | |
| Deep Navy hold | 4:44.500–4:45.000 | 17070–17100 | 500ms breath before Close |
| **Scene ends / Close begins** | 4:45.000 | 17100 | Handoff to `08_SCENE_CLOSE.md` |

---

## Editorial Notes

These are high-level observations and constraints for the editor, director, and post-production team.

### 1. Terminal Shots Are Hard Cuts Only

Every transition between terminal shots (6.1 through 6.6) is a **hard cut**. No cross-dissolves, no wipes, no fades. Cut. New terminal. Type. Output. Cut. This pace creates energy through rhythm, not motion graphics. The regularity of the pattern — cut, type, output, cut — becomes almost musical. The viewer falls into the rhythm, and each new command feels like the next beat in a sequence.

### 2. Typing Speed Is Sacred

Typing speed is 40ms per character across all shots. Not faster (illegible, feels robotic). Not slower (feels sluggish, loses energy). This is a competent developer typing at ~60 WPM. The typing must feel *real* — like a person is actually using the tool, not like a script is running.

If the editor needs to adjust timing, they may modify the hold durations or the pause between typing and output — never the typing speed.

### 3. Readability Is Non-Negotiable

The terminal text MUST be readable on every target platform. Test criteria:

- **Desktop (1080p, 24" monitor, 2 feet away)**: All text legible without effort.
- **Mobile (1080p, 6" screen, arm's length)**: All text legible. If `code` (18px at HD) is too small, escalate to `code-lg` (24px at HD) for output.
- **Embedded player (Twitter/X, 480p effective resolution)**: Command keywords and checkmarks/X-marks still distinguishable by color even if individual characters blur.

If readability and design purity conflict, readability wins. Every time. No exceptions.

### 4. Hero Shot Priority

SHOT 6.3 (`wt status --team`) and SHOT 6.6 (`wt access test`) are the two hero moments. They carry the two biggest proof points of the entire product:

- **6.3**: Staged Snapshot Visibility — seeing your teammates' work before they push.
- **6.6**: Built-in access control — with human-readable, self-explaining denial output.

The editor should hold these shots longer than the others. The 2-second holds specified are minimums, not maximums. If the scene has room to breathe, add time to these two shots.

If the scene needs to be trimmed for time, trim in this order:

1. SHOT 6.4 (`wt snapshot`) — most expendable. The concept of snapshots has been demonstrated implicitly in other shots.
2. SHOT 6.5 (`wt push`) — can be shortened by reducing the hold.
3. SHOT 6.1 (`wt init`) — can be shortened by reducing the hold, but must still complete its full output.
4. **Never trim** SHOT 6.3 or SHOT 6.6 holds.

### 5. The Bookend Structure

Act IV's terminal is visually identical to the Cold Open's terminal. Same component, same background, same font, same cursor. The only difference is what's happening inside. This is the most important structural decision connecting the two ends of the video:

- **Cold Open terminal**: `git push` → REJECTED. `git pull --rebase` → CONFLICT. `git reset --hard` → everything gone. Pain.
- **Act IV terminal**: `wt init` → initialized. `wt status --team` → full visibility. `wt access test` → clear, human-readable policy enforcement. Clarity.

The editor must preserve this visual identity between the two scenes. If any terminal styling changes are made in the Cold Open, they must be reflected here, and vice versa.

### 6. No Narrator During Terminal Shots

Shots 6.1 through 6.6 have **no narrator voice-over**. This is deliberate and non-negotiable. The terminal speaks for itself. The absence of narration communicates confidence — the product doesn't need someone explaining what you're looking at. You can read it. You can understand it. That IS the point.

The narrator returns only in SHOT 6.7, and even then, the line is observational, not explanatory: "One command per job. Plain language. Human-readable errors that tell you exactly what happened and why." The narrator is describing what the viewer already saw and understood. It's validation, not instruction.

### 7. Sound Design Minimalism

The audio palette for this scene is intentionally sparse:

- Mechanical keyboard clicks (same as Cold Open)
- Subtle complete chimes (barely audible)
- One denial tone (SHOT 6.6)
- Ambient pad (barely audible)
- Narrator V.O. (SHOT 6.7 only)

That's it. No whooshes, no transitions, no rises, no stingers, no musical phrases. The sparsity communicates the same confidence as the visual simplicity: this product doesn't need decoration.

---

## Continuity & Handoff

### Into Scene 6

Scene 6 receives a hard cut from Act III's final frame (Scene 5, documented in `06_SCENE_ACT_III_PRODUCT.md`). The last frame of Act III shows the complete architecture diagram with "W0rkTree Protocol" text. The hard cut to Act IV's terminal is deliberate and jarring — it breaks the motion graphics language that has been building for 90 seconds and drops the viewer into a raw terminal. This is the "show, don't tell" moment.

- **Visual**: Architecture diagram (Act III final frame) → hard cut → empty terminal on Deep Navy (Act IV SHOT 6.1).
- **Audio**: Act III music fades during the last seconds of Act III, reaching −34 dB by 4:15.000. The ambient pad is already playing at this level. Keyboard clicks begin immediately at 4:15.000.
- **Emotional**: Understanding (Act III) → hard cut → Proof (Act IV). The hard cut is a gear shift. No transition needed.

### Out of Scene 6

Scene 6 fades to Deep Navy at 4:44.500, holds pure Deep Navy for 500ms, and hands off to Scene 7 (Close) at 4:45.000. This 500ms of Deep Navy is the structural breath — identical in function to the held black between the Cold Open and the Title Card.

- **Visual**: Narrator overlay on dimmed terminal → fade to Deep Navy (500ms) → Deep Navy holds (500ms) → Close begins.
- **Audio**: Ambient pad swells during 4:40.500–4:43.500, reaching −28 dB LUFS. The swell sustains through the Deep Navy hold and transitions into the Close's resolved synth chord.
- **Emotional**: Proof / Satisfaction (Act IV) → breath (Deep Navy hold) → Invitation (Close).

### Cross-References

| Document | Relevance to Scene 6 |
|---|---|
| `00_PRODUCTION_BIBLE.md` | Master authority. Narrative arc beat 5 (Proof). Master Timeline rows 4:15–4:45. Editorial Principles (all apply). |
| `01_DESIGN_SYSTEM.md` | Terminal component spec. Font definitions (JetBrains Mono, Inter). Color palette (all hex values). Typography scale (code-lg, code, heading-2). Animation easing curves (ease-enter, ease-exit, ease-standard). |
| `02_SCENE_COLD_OPEN.md` | Bookend partner. Terminal visual identity must match exactly. Same keyboard foley source. Same cursor behavior. Compare color maps to ensure consistency. |
| `06_SCENE_ACT_III_PRODUCT.md` | Provides handoff into Scene 6. Architecture diagram is the last frame before the hard cut. Music fade timing must be coordinated. |
| `08_SCENE_CLOSE.md` | Receives handoff from Scene 6. The 500ms Deep Navy hold is the transition seam. Ambient pad swell must align with Close's opening chord. |
| `09_ANIMATION_COMPONENTS.md` | Terminal typing animation component. Cursor blink component. Text reveal component. Ease-enter and ease-exit definitions. |
| `10_AUDIO_SPEC.md` | Keyboard foley source selection. Complete chime sound design spec. Denial tone design spec. Ambient pad specification. Narrator recording requirements. |
| `11_ASSET_MANIFEST.md` | Lists all assets needed for Scene 6: keyboard foley recordings (same source as Cold Open), complete chime sound files, denial tone sound file, ambient pad audio, narrator V.O. recording for SHOT 6.7. |

---

*This document is the authoritative specification for Scene 6 (Act IV: See It Work) of the W0rkTree launch video. In any conflict between this document and the Production Bible (`00_PRODUCTION_BIBLE.md`), the Production Bible wins. In any conflict between this document and the Design System (`01_DESIGN_SYSTEM.md`), the Design System wins for visual specifications. In any conflict between this document and a scene-level peer document, this document is authoritative for its own timecode range (4:15.000–4:45.000) only. Questions, clarifications, and change requests are routed through the production lead.*