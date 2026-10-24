# Asset Manifest

> **Project**: W0rkTree Launch Video
> **Document**: `11_ASSET_MANIFEST.md`
> **Purpose**: Catalogs every asset required for production of the W0rkTree launch video.

This document catalogs every asset required for production of the W0rkTree launch video. Each asset has a unique ID, type classification, the scene(s) where it's used, creation method, and delivery specifications.

---

## 1. MOTION GRAPHICS ASSETS

### 1.1 Logo & Branding

| ID | Asset Name | Description | Format | Resolution | Scenes Used | Notes |
|---|---|---|---|---|---|---|
| MG-001 | W0rkTree Logo (White) | Primary logo. "W0rkTree" in Inter Tight 800. White (#F0F0F0) on transparent. | SVG + PNG | 4K (vector / 8192px wide PNG) | Title Card, Close | Must be vector for crisp rendering at any scale. PNG fallback for compositing. |
| MG-002 | W0rkTree Logo Glow Layer | Duplicate of the "0" character only, Gaussian blur 20px, #00D4FF, for glow compositing. | PNG (transparent) | 4K | Title Card, Close | Render at 2× size for clean blur. Opacity controlled in comp. |
| MG-003 | Tagline Text | "Version control, rebuilt from zero." Pre-rendered in Inter 400, #F0F0F0 at 70%. | PNG (transparent) | 4K | Title Card, Close | Pre-render for consistent kerning. |

---

### 1.2 Terminal Components

| ID | Asset Name | Description | Format | Resolution | Scenes Used | Notes |
|---|---|---|---|---|---|---|
| MG-010 | Terminal Background | Rounded rectangle, #111827 fill, 16px border-radius. | Vector (SVG) | 4K | Cold Open, Act IV | 80% canvas width. No window chrome. |
| MG-011 | Terminal Cursor | Block cursor, #00D4FF fill, sized to one character cell in JetBrains Mono. | Vector / Shape | 4K | Cold Open, Act IV | Animated: 500ms blink cycle. |
| MG-012 | Cold Open Terminal — Shot 1.2 | Full terminal frame: `git push origin main` + error output. Pre-typeset with correct colors per the terminal component spec. | AE Comp / Pre-render | 4K@60fps | Cold Open | Typing animation: 40ms/char. Output: 80ms/line. Colors: `$` #6B7280, `git` #FF3B3B, error #FF3B3B, hint #6B7280. |
| MG-013 | Cold Open Terminal — Shot 1.3 | `git pull --rebase` + CONFLICT output. | AE Comp | 4K@60fps | Cold Open | `CONFLICT` in bold #FF3B3B. `src/auth/oauth.rs` in #00D4FF. |
| MG-014 | Cold Open Terminal — Shot 1.4 | `git rebase --abort` + `git reset --hard HEAD~3` + output. | AE Comp | 4K@60fps | Cold Open | `git reset --hard HEAD~3` entire command in #FF3B3B. Output at 60% opacity. |
| MG-015 | Act IV Terminal — Shot 6.1 | `wt init my-project` + output. | AE Comp | 4K@60fps | Act IV | `wt` in #00D4FF. Checkmarks in #00C48F. |
| MG-016 | Act IV Terminal — Shot 6.2 | `wt status` + full output. | AE Comp | 4K@60fps | Act IV | Labels #6B7280, values in brand colors. |
| MG-017 | Act IV Terminal — Shot 6.3 | `wt status --team` + team output. | AE Comp | 4K@60fps | Act IV | **HERO SHOT.** File paths #00D4FF. Emails #F0F0F0. |
| MG-018 | Act IV Terminal — Shot 6.4 | `wt snapshot --message "..."` + output. | AE Comp | 4K@60fps | Act IV | Snapshot ID in #00D4FF. |
| MG-019 | Act IV Terminal — Shot 6.5 | `wt push` + output. | AE Comp | 4K@60fps | Act IV | Arrow → in #6B7280. |
| MG-020 | Act IV Terminal — Shot 6.6 | `wt access test` + DENIED output. | AE Comp | 4K@60fps | Act IV | `DENIED` and `✗` in #FF3B3B bold. Policy details in #6B7280 / #F0F0F0 mix. |

---

### 1.3 Act I — History Visuals

| ID | Asset Name | Description | Format | Resolution | Scenes Used | Notes |
|---|---|---|---|---|---|---|
| MG-030 | Timeline Base | Horizontal line, 2px, #374151, with year nodes. | AE Comp | 4K@60fps | Act I | Draws left-to-right, 600ms. |
| MG-031 | 2005 Node | Circle (24px), fill #C4956A, label "2005" in heading-1, "April" in body #6B7280. | Vector + AE | 4K | Act I Seg A | Bounce-in: ease-bounce 400ms. |
| MG-032 | Illustrated Figure | Stylized line illustration of a person at a computer. Sepia palette (#C4956A, #8B6E4E, #E8D5B7). Editorial style, NOT photorealistic. | Illustrated (AI / Vector) | 4K | Act I Seg A | Line-draw reveal animation: 1500ms. |
| MG-033 | Email Icons | Small envelope shapes radiating outward from the figure. | Vector + AE | 4K | Act I Seg A | 8–12 envelopes, staggered animation. |
| MG-034 | BitKeeper Text | "BitKeeper" in heading-1, #C4956A. | AE Comp | 4K | Act I Seg A | Break-apart / crumble animation: 800ms. |
| MG-035 | REVOKED Stamp | "REVOKED" in display-lg, #FF3B3B. | AE Comp | 4K | Act I Seg A | Scale 1.2→1.0, 200ms, ease-sharp. SFX: stamp hit. |
| MG-036 | Day Counter | Counter "Day 1" through "Day 10" in display-lg. | AE Comp | 4K | Act I Seg B | 100ms per number, ease-sharp. |
| MG-037 | Kernel File Tree | Exponentially growing tree of file nodes. Starts with 1, grows to thousands. Sepia palette. | AE Comp (Procedural) | 4K@60fps | Act I Seg B | Growth duration: 3000ms, ease-enter curve applied to spawn rate. |
| MG-038 | Contributor Avatars | Small colored dots (8px) flowing toward the tree from frame edges. | AE Comp (Particle) | 4K@60fps | Act I Seg B | 50–100 particles, various colors, subtle trail lines. |
| MG-039 | Branch/Merge Diagram | Dynamic branching diagram. Lines split and recombine. Sepia palette with #7FB069 merge glows. | AE Comp | 4K@60fps | Act I Seg B | Morphs from file tree. Organic, alive. |
| MG-040 | 2005 Crack & Crumble | "2005" text aging, cracking, and breaking apart. | AE Comp | 4K@60fps | Act I Seg C | Slow crumble: 1200ms. Behind: "2025" in display-lg #F0F0F0. |
| MG-041 | Modern Tool Icons | Stylized line-art icons: VS Code, Slack, K8s, CI/CD, Docker. #6B7280 on #0A0F1A. | Illustrated (Vector) | 4K | Act I Seg C | Grid layout, staggered fade, 150ms apart. |
| MG-042 | Stack Overflow Cards (×8) | Q&A cards with question text + vote count. | AE Comp (Template) | 4K@60fps | Act I Seg D | Scrolling upward, accelerating interval. See animation component spec. |
| MG-043 | "Every Single Day" Text | heading-1, #F0F0F0, centered. Over dimmed SO wall. | AE Comp | 4K | Act I Seg D | ease-enter 600ms, translate-y +16px. |

---

### 1.4 Act II — Problem Visuals

| ID | Asset Name | Description | Format | Resolution | Scenes Used | Notes |
|---|---|---|---|---|---|---|
| MG-050 | Number Card 1 | "1" + "THE JARGON WALL" | AE Comp | 4K@60fps | Act II P1 | Number Card Slam component. |
| MG-051 | Jargon Word Cloud | 17+ Git terms floating, overlapping, growing chaotic. | AE Comp | 4K@60fps | Act II P1 | Terms float from edges to center, 120ms stagger, synonym flash effect. |
| MG-052 | "Bad Design" Text | Two-line punchline in heading-2. | AE Comp | 4K | Act II P1 | Over dimmed jargon cloud. |
| MG-053 | Number Card 2 | "2" + "THE INVISIBLE TEAM" | AE Comp | 4K@60fps | Act II P2 | Number Card Slam component. |
| MG-054 | Split-Screen Panels | Two terminal panels: ALICE (left) + BOB (right). Both editing same file. | AE Comp | 4K@60fps | Act II P2 | Split-screen component. 4px divider #374151. |
| MG-055 | Countdown Timer | "3:00:00" → "0:00:00" in heading-2, #FF3B3B. | AE Comp | 4K@60fps | Act II P2 | 5-second countdown with accelerating tick SFX. |
| MG-056 | CONFLICT Explosion | "CONFLICT" in display-lg, #FF3B3B. Divider shatter. Red flash. | AE Comp | 4K@60fps | Act II P2 | ease-bounce scale 1.5→1.0, 300ms. Red flash: 100ms at 20% opacity. |
| MG-057 | Number Card 3 | "3" + "DESTRUCTION IS ONE COMMAND AWAY" | AE Comp | 4K@60fps | Act II P3 | Number Card Slam component. |
| MG-058 | Healthy DAG | Directed acyclic graph with green (#00C48F) nodes, branch labels, pulsing connections. | AE Comp | 4K@60fps | Act II P3 | Alive: nodes pulse, lines glow. |
| MG-059 | Git Danger Commands | Three floating text labels: `git reset --hard`, `git push --force`, `git rebase --drop`. #FF3B3B, code-lg. | AE Comp | 4K@60fps | Act II P3 | Enter staggered 400ms apart, ease-sharp 300ms. |
| MG-060 | DAG Dissolution (×3) | Three dissolution events: nodes turn red then fragment into particles. | AE Comp | 4K@60fps | Act II P3 | Node dissolution component. 800ms per event. |
| MG-061 | "Rumor" Text | Two-line text over dimmed damaged DAG. | AE Comp | 4K | Act II P3 | Line 2 in #FF3B3B at 80%. |
| MG-062 | Number Card 4 | "4" + "THE SECURITY VACUUM" | AE Comp | 4K@60fps | Act II P4 | Number Card Slam component. |
| MG-063 | Open Vault | Line-art vault door (open), with files streaming outward. Sensitive files flash red. | AE Comp | 4K@60fps | Act II P4 | Vault: 2px #374151 line art. File stream accelerates. |
| MG-064 | Platform Badges | Three cards: "GitHub Permissions", "GitLab Roles", "Bitbucket Restrictions". With "Bolted on." header. | AE Comp | 4K | Act II P4 | Jagged / duct-tape border treatment. |
| MG-065 | git:// Protocol Text | `git://` in code-lg #FF3B3B + "No encryption. No auth." in body-lg #6B7280. | AE Comp | 4K | Act II P4 | Below platform badges. |
| MG-066 | Number Card 5 | "5" + "THE MONOREPO MELTDOWN" | AE Comp | 4K@60fps | Act II P5 | Number Card Slam component. |
| MG-067 | Workaround Tower | 10-block Jenga-style tower of labeled workaround blocks. Leaning, wobbling. | AE Comp | 4K@60fps | Act II P5 | Blocks stack from bottom. Progressive offset (2–4px per level). Top 3 wobble. |
| MG-068 | Tower Block Fall | Top block slides off and falls with rotation. | AE Comp | 4K@60fps | Act II P5 | Gravity physics, 15° rotation. SFX: wooden clack. |
| MG-069 | Bridge Question Text | Two-line centered text. Line 2 in #00D4FF. | AE Comp | 4K | Act II Bridge | First use of brand cyan in Act II. |

---

### 1.5 Act III — Product Visuals

| ID | Asset Name | Description | Format | Resolution | Scenes Used | Notes |
|---|---|---|---|---|---|---|
| MG-080 | Denial Strikethroughs | "Git wrapper", "Git extension", "hosting platform" with red strikethrough. | AE Comp | 4K@60fps | Act III 5A | Strikethrough component ×3. |
| MG-081 | "A ground-up replacement." Text | heading-1, #00D4FF. | AE Comp | 4K | Act III 5A | ease-enter 600ms. |
| MG-082 | Architecture Diagram — LOCAL | Full local container: worker node, sub-items, local worktree sub-node, mini file tree. | AE Comp | 4K@60fps | Act III 5B | Diagram node build component. Total build: ~8s. |
| MG-083 | Architecture Diagram — Connection | Dashed animated line + "QUIC" label + "(sync)" sub-label. | AE Comp | 4K@60fps | Act III 5B | Continuous dash animation. Data flow pulse on cue. |
| MG-084 | Architecture Diagram — REMOTE | Full server container: server node, sub-items, admin + CLI sub-nodes. | AE Comp | 4K@60fps | Act III 5B | Mirror of LOCAL structure. |
| MG-085 | "STAGED SNAPSHOT VISIBILITY" Text | display-lg, #00D4FF. Impact entry. | AE Comp | 4K@60fps | Act III 5C | ease-bounce scale 0.9→1.0, 500ms. |
| MG-086 | Three-Step Flow Diagram | Developer → Worker snapshots → Team sees staged work → (explicit push) → Branch updated. | AE Comp | 4K@60fps | Act III 5C | Step-by-step build, 400ms per step. "YOU DECIDE WHEN" label. |
| MG-087 | Team Dashboard Mockup | Web UI: header + 3 team member rows (Alice, Bob, Carol). | AE Comp | 4K@60fps | Act III 5C | **MONEY SHOT.** Rows stagger in 200ms apart. Hold 4s. |
| MG-088 | Push Button Animation | Alice's row highlights, push button appears, snapshots flow to branch column, checkmark. | AE Comp | 4K@60fps | Act III 5C | Button depress: 100ms. Flow animation. |
| MG-089 | Feature Card 1 — Access Control | Card with TOML snippet showing policies.toml. | AE Comp | 4K@60fps | Act III 5D | Feature card entry component. TOML syntax highlighting. |
| MG-090 | Feature Card 2 — License Compliance | Card with TOML snippet showing license.path + "proprietary" in red. | AE Comp | 4K@60fps | Act III 5D | 200ms stagger from card 1. |
| MG-091 | Feature Card 3 — Immutable History | Card with mini DAG (#00C48F) + strikethrough "--force" + "wt revert" checkmark. | AE Comp | 4K@60fps | Act III 5D | 200ms stagger from card 2. |
| MG-092 | Feature Card 4 — Declarative Config | Card with .wt/ file tree in code-sm. | AE Comp | 4K@60fps | Act III 5D | 200ms stagger from card 3. |
| MG-093 | Full Architecture + Protocol Labels | Complete diagram at full scale + "TLS / QUIC" + "Encrypted • Authenticated • Multiplexed" labels. | AE Comp | 4K@60fps | Act III 5E | Labels stagger 200ms. Final glow pulse on all cyan elements. |

---

### 1.6 Close Visuals

| ID | Asset Name | Description | Format | Resolution | Scenes Used | Notes |
|---|---|---|---|---|---|---|
| MG-100 | Close Logo + Breathing Glow | W0rkTree logo with continuous glow breathing (20%–40%, 3000ms cycle). | AE Comp | 4K@60fps | Close | Logo glow component (breathing variant). |
| MG-101 | Close Tagline | Same as MG-003 but re-used in close positioning. | Pre-rendered text | 4K | Close | Appears 500ms after logo. |
| MG-102 | CTA — URL | "w0rktree.dev" in heading-2, #00D4FF. Optional underline draw. | AE Comp | 4K@60fps | Close | URL pops visually as clickable. |
| MG-103 | CTA — Actions Text | "Star us on GitHub. Join the Discord. Build with us." in body-lg, #F0F0F0 at 60%. | Pre-rendered text | 4K | Close | Appears 300ms after URL. |

---

## 2. LIVE ACTION ASSETS

| ID | Asset Name | Description | Camera | Resolution | Scene Used | Notes |
|---|---|---|---|---|---|---|
| LA-001 | Developer at Desk | Medium close-up, 3/4 angle. Developer lit by monitor glow. Closes laptop. | Cinema camera (RED / Sony FX), 50mm, f/2.8, 24fps, LOG/RAW | 4K minimum | Cold Open Shot 1.5 | See Cold Open spec for full lighting, set, and casting notes. |
| LA-002 | Room Tone | 60 seconds of silence in the shooting environment. | On-set mic | 48kHz / 24bit | Cold Open (audio) | Record with all equipment powered on, no human activity. |
| LA-003 | Laptop Close Sound | Close-mic recording of laptop lid closing. 10 takes minimum. | Condenser mic, close-mic position | 48kHz / 24bit | Cold Open (audio) | Choose the cleanest mechanical click in post. |

---

## 3. FONT ASSETS

| ID | Font | Weights Needed | License | Source |
|---|---|---|---|---|
| FONT-001 | Inter | 400, 500, 600, 700, 800 | OFL 1.1 (Free) | rsms.me/inter or Google Fonts |
| FONT-002 | Inter Tight | 800, 900 | OFL 1.1 (Free) | Google Fonts |
| FONT-003 | JetBrains Mono (No Ligatures) | 400, 700 | OFL 1.1 (Free) | jetbrains.com/mono |

> **⚠️ CRITICAL**: JetBrains Mono **MUST** have ligatures **DISABLED**. In After Effects and all motion graphics tools, disable OpenType ligatures under the Character panel → OpenType features. We want `!=` not `≠`, `->` not `→`, `=>` not `⇒`. Verify in every terminal comp before final render.

---

## 4. COLOR SWATCHES

All colors below are the canonical brand palette. Deliver as `.ase` (Adobe Swatch Exchange) and `.aco` (Photoshop) swatch files alongside the project.

| ID | Color Name | Hex | RGB | HSL |
|---|---|---|---|---|
| CLR-001 | Deep Navy | #0A0F1A | 10, 15, 26 | 221°, 44%, 7% |
| CLR-002 | Code Background | #111827 | 17, 24, 39 | 221°, 39%, 11% |
| CLR-003 | Pure White | #F0F0F0 | 240, 240, 240 | 0°, 0%, 94% |
| CLR-004 | Accent Cyan | #00D4FF | 0, 212, 255 | 190°, 100%, 50% |
| CLR-005 | Warning Red | #FF3B3B | 255, 59, 59 | 0°, 100%, 62% |
| CLR-006 | Confident Green | #00C48F | 0, 196, 143 | 164°, 100%, 38% |
| CLR-007 | Muted Gray | #6B7280 | 107, 114, 128 | 220°, 9%, 46% |
| CLR-008 | Dim Gray | #374151 | 55, 65, 81 | 217°, 19%, 27% |
| CLR-009 | Sepia Warm | #C4956A | 196, 149, 106 | 29°, 42%, 59% |
| CLR-010 | Dark Sepia | #8B6E4E | 139, 110, 78 | 31°, 28%, 43% |
| CLR-011 | Light Sepia | #E8D5B7 | 232, 213, 183 | 37°, 53%, 81% |
| CLR-012 | Branch Green (Muted) | #7FB069 | 127, 176, 105 | 101°, 30%, 55% |
| CLR-013 | SO Orange (Accent) | #FF8A3B | 255, 138, 59 | 24°, 100%, 62% |
| CLR-014 | Third Person Orange | #FF8A3B | 255, 138, 59 | 24°, 100%, 62% |

> **Note**: CLR-013 and CLR-014 share the same hex value. They are listed separately because they serve different semantic roles — CLR-013 is tied to Stack Overflow branding in Act I, while CLR-014 is reserved for third-person developer callouts. If the brand ever diverges these roles, the swatch IDs are already separated.

---

## 5. ASSET ID INDEX

Quick-reference index of all asset ID ranges:

| Range | Category |
|---|---|
| MG-001 – MG-003 | Logo & Branding |
| MG-010 – MG-020 | Terminal Components |
| MG-030 – MG-043 | Act I — History Visuals |
| MG-050 – MG-069 | Act II — Problem Visuals |
| MG-080 – MG-093 | Act III — Product Visuals |
| MG-100 – MG-103 | Close Visuals |
| LA-001 – LA-003 | Live Action Assets |
| FONT-001 – FONT-003 | Font Assets |
| CLR-001 – CLR-014 | Color Swatches |

**Total unique assets**: 72

---

## 6. DELIVERY CHECKLIST

Final pre-delivery checklist — every item must be signed off before the project is considered delivery-ready:

- [ ] All motion graphics rendered at 3840×2160, 60fps, ProRes 422 HQ
- [ ] Live action footage color-graded and conformed to Rec. 709
- [ ] All fonts embedded or outlined in final renders
- [ ] JetBrains Mono ligatures confirmed **DISABLED** in all terminal comps
- [ ] Logo SVG delivered alongside rasterized versions
- [ ] All audio mixed to −14 LUFS integrated, −1 dBTP true peak
- [ ] SRT + VTT subtitle files generated from narrator script
- [ ] YouTube thumbnail (1280×720 PNG) designed and delivered
- [ ] All platform cuts (Twitter 60s, LinkedIn 90s, TikTok 9:16 60s, Square 1:1 60s) rendered
- [ ] Color accuracy verified on sRGB calibrated display
- [ ] Adobe Swatch Exchange (.ase) and Photoshop Swatch (.aco) files included in delivery package
- [ ] All AE project files organized by scene (Cold Open, Act I, Act II, Act III, Act IV, Close)
- [ ] Pre-rendered elements collected into `/assets/pre-renders/` with matching asset IDs as filenames
- [ ] Final delivery folder structure matches the naming convention: `{ASSET_ID}_{ASSET_NAME_SLUG}.{ext}`
