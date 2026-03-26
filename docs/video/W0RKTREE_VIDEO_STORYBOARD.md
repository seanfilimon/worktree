# W0rkTree — YouTube Launch Video Storyboard

> **Title**: *"Git Is Broken. We Built the Fix."*
> **Runtime**: 5:00
> **Tone**: Confident, technical, honest. Not salesy — engineering-first. Think Fireship meets a well-produced product launch.
> **Audience**: Software engineers, tech leads, DevOps engineers, engineering managers — anyone who has suffered through Git.

---

## VIDEO OVERVIEW

| Segment | Time | Duration | Purpose |
|---|---|---|---|
| Cold Open — The Git Horror | 0:00–0:25 | 25s | Emotional hook. Everyone has a Git horror story. |
| Title Card | 0:25–0:30 | 5s | Brand moment. |
| Act I — How We Got Here | 0:30–1:30 | 60s | Git's origin story. Why it worked. Why it stopped working. |
| Act II — The Five Broken Things | 1:30–2:45 | 75s | The real problems. Not nitpicks — structural failures. |
| Act III — Introducing W0rkTree | 2:45–4:15 | 90s | The product. Architecture. Key innovations. |
| Act IV — See It Work | 4:15–4:45 | 30s | Quick CLI demo montage. |
| Close — The Invitation | 4:45–5:00 | 15s | CTA. Join the movement. |

---

## SCENE-BY-SCENE BREAKDOWN

---

### COLD OPEN — THE GIT HORROR

**[0:00–0:25] — 25 seconds**

**VISUAL**: Black screen. The sound of a single keyboard clacking in a quiet room. Then — a sharp inhale. A developer staring at a terminal. The camera is tight on their face, lit only by monitor glow. Their expression shifts from confusion to dread.

**ON SCREEN** (terminal text, typed one line at a time with realistic speed):

```
$ git push origin main
! [rejected] main -> main (non-fast-forward)
error: failed to push some refs to 'origin/main'
hint: Updates were rejected because the tip of your current branch is behind
```

Beat.

```
$ git pull --rebase origin main
CONFLICT (content): Merge conflict in src/auth/oauth.rs
error: could not apply fa39187... Add token rotation
```

Beat.

```
$ git rebase --abort
$ git reset --hard HEAD~3
HEAD is now at 7a2c1e0 initial commit
```

Silence. The developer closes their laptop. Stares into the void.

**NARRATOR (V.O.)**:

> "You know this feeling. That moment where you're not building software anymore — you're fighting your own tools. Three hours of work. Gone. Not because of a bug in your code. Because of a bug in your workflow."

**VISUAL**: The developer opens Slack. Types: *"hey... did anyone else push to main in the last hour?"*

**NARRATOR (V.O.)**:

> "And the worst part? Everyone just accepts it. Like this is normal. Like this is fine."

**VISUAL**: Cut to black.

---

### TITLE CARD

**[0:25–0:30] — 5 seconds**

**VISUAL**: Clean black screen. The W0rkTree logo fades in — minimal, sharp, the zero in W0rk subtly glowing. A single line appears beneath it:

**ON SCREEN**:

> **W0rkTree**
> *Version control, rebuilt from zero.*

A low, confident synth tone underscores the moment.

---

### ACT I — HOW WE GOT HERE

**[0:30–1:30] — 60 seconds**

**VISUAL**: Archival-style footage or stylized illustrations. Linus Torvalds at a conference. The Linux kernel mailing list. Early 2000s open-source culture. Think documentary B-roll with a modern color grade.

**NARRATOR (V.O.)**:

> "April 2005. Linus Torvalds is mass-emailing the Linux kernel mailing list. BitKeeper — the version control system the kernel team had been using for free — just revoked their license. The entire Linux development workflow is about to collapse."

**VISUAL**: A timeline graphic appears. 2005 highlighted. A clock ticking.

> "So Linus does what Linus does. He builds his own. In ten days, he writes the first version of Git. A distributed version control system designed for one very specific job: managing the Linux kernel's source code across thousands of contributors, none of whom trust each other."

**VISUAL**: The Linux kernel file tree — tens of thousands of files. Thousands of contributor avatars flowing in from around the world. A web of branches splitting and merging.

> "And for that job? Git was brilliant. Truly. Content-addressable storage. A Merkle tree of commits. Cheap branching. Cryptographic integrity. For coordinating a massive, decentralized, open-source project in 2005 — it was exactly the right tool."

**VISUAL**: Transition — the warm archival tone cools. The footage modernizes. We see modern developer setups. VS Code. Slack. CI/CD dashboards. Kubernetes clusters.

> "But here's the thing nobody wants to say out loud."

Beat.

> "It's not 2005 anymore."

**VISUAL**: The 2005 on the timeline ages, cracks, fades. Modern year appears.

> "Git was designed for the Linux kernel. Not for your microservices monorepo. Not for your fifty-person team on four continents. Not for your designer who just wants to save their work without accidentally deleting the production branch. Git solved Linus's problem. And then the entire industry adopted it — and inherited all of its assumptions."

**VISUAL**: A montage of confused faces. Stack Overflow questions: *"What does 'detached HEAD' mean?"* ... *"How do I undo a git rebase?"* ... *"I ran git reset --hard and lost everything"* — the questions scroll, hundreds of them.

> "Twenty years later, we're still paying that tax. Every single day."

---

### ACT II — THE FIVE BROKEN THINGS

**[1:30–2:45] — 75 seconds**

**VISUAL**: Clean, bold typography on dark backgrounds. Each problem gets a numbered card that slams onto screen with impact. Think chapter titles in a documentary.

**ON SCREEN**: **1 — THE JARGON WALL**

**NARRATOR (V.O.)**:

> "Number one. Git has a jargon problem. Ref. Refspec. HEAD. Detached HEAD. Origin. Upstream. Index. Staging area. Working tree. Stash. The thing is — half of these are synonyms for each other, and the other half mean completely different things depending on context."

**VISUAL**: A rapid-fire scroll of Git terms, overlapping, growing chaotic.

> "Git checkout — one command — does five completely different things depending on what flags you pass it. That's not power. That's bad design wearing a trenchcoat."

---

**ON SCREEN**: **2 — THE INVISIBLE TEAM**

> "Number two. In Git, all work is invisible until someone pushes. You have no idea what your teammates are working on. You don't know Alice has been editing the same file as you for three hours until you both push and the merge conflict explodes. So teams hold standups. They post in Slack. They update tickets. All to compensate for the fact that their version control system has zero awareness of what anyone is actually doing."

**VISUAL**: Split screen — two developers typing away, editing the same file, completely unaware of each other. A countdown timer ticks. When it hits zero — CONFLICT. Red screen.

---

**ON SCREEN**: **3 — DESTRUCTION IS ONE COMMAND AWAY**

> "Number three. Git lets you destroy things. Easily. git reset --hard. git push --force. git rebase and drop. These commands delete history. And Git doesn't stop you. It doesn't warn you. Your colleague's weekend of work — gone. One command. And sure, the reflog exists — if you know about the reflog, and you find it in time, and you haven't garbage collected yet. That's not a safety net. That's a rumor of a safety net."

**VISUAL**: A file tree. Branches. Snapshots of work. One by one, they dissolve — erased by force-push, hard reset, rebase. Like a Thanos snap.

---

**ON SCREEN**: **4 — THE SECURITY VACUUM**

> "Number four. Git has no built-in access control. None. No authentication. No authorization. No concept of 'this person can read this folder but not that one.' Every solution — GitHub permissions, GitLab roles, Bitbucket restrictions — is bolted on by a third party. The protocol itself? The native git-colon-slash-slash protocol has no encryption and no auth. In 2025. We just... accepted that."

**VISUAL**: An unlocked vault door swinging open. Files flowing out unprotected. A developer accidentally pushes an API key. A contractor clones the entire repo including files they should never see.

---

**ON SCREEN**: **5 — THE MONOREPO MELTDOWN**

> "And number five. Git was not built for how modern teams actually work. Large binary files? You need a separate system called LFS. Multiple projects in one repo? Good luck with sparse checkout and submodule hell. Partial history? Shallow clones break half your tooling. Git scales down to one person beautifully. Scaling it up to a real organization? That's where you start building workarounds on top of workarounds on top of workarounds."

**VISUAL**: A tower of blocks labeled "LFS", "Submodules", "Sparse Checkout", "Git Hooks", "Husky", "GitHub Actions", wobbling, about to collapse. The label at the base: "Git."

> "So we asked a simple question."

Beat.

> "What if we stopped building workarounds — and built the thing that should have existed all along?"

---

### ACT III — INTRODUCING W0RKTREE

**[2:45–4:15] — 90 seconds**

**VISUAL**: The tone shifts. The dark, problem-focused palette gives way to something clean — deep navy, white type, sharp lines. The W0rkTree logo appears. Architecture diagrams begin to build themselves on screen, animated and precise.

**NARRATOR (V.O.)**:

> "This is W0rkTree. Not a Git wrapper. Not a Git extension. Not a hosting platform for Git repos. A ground-up replacement for Git — with a migration bridge so you can bring your existing repos with you."

**VISUAL**: The two-runtime architecture diagram builds on screen, animated piece by piece.

> "W0rkTree runs two systems. On your machine — a background process we call the worker. It watches your files. It snapshots your work automatically. It handles branching, diffing, merging — everything you'd expect from version control — all locally, all instantly."

**VISUAL**: The local side of the diagram lights up. File watcher pulses. Snapshots appear. All happening silently in the background.

> "On the server — a multi-tenant platform that your whole team connects to. It stores canonical history, enforces access control, manages tenants and teams — and does something Git has never done."

Beat.

**ON SCREEN**: The words **STAGED SNAPSHOT VISIBILITY** appear.

> "When the worker captures a snapshot of your work — whether automatic or manual — it syncs that snapshot to the server as a staged snapshot. Not pushed. Not merged. Staged. Your team can see what you're working on and which files you're touching — in real time — without you doing anything."

**VISUAL**: A team dashboard. Alice's avatar — *"3 staged snapshots on feature/oauth, touching auth-service/src/oauth.rs"*. Bob's avatar — *"1 staged snapshot on fix/token-expiry"*. The whole team's work, visible at a glance.

> "Think about what that means. No more 'I had no idea you were working on that file.' No more merge conflicts that could have been prevented with thirty seconds of awareness. The entire team sees the full picture of what's in flight — and when you're ready, you explicitly push your staged work to the branch. Visible doesn't mean merged. Staged doesn't mean pushed. You're always in control."

**VISUAL**: Alice clicks push. Her staged snapshots flow into the branch history. Clean. Intentional.

> "And because we built this from scratch, we built the things Git never had."

**VISUAL**: Feature cards animate in, one by one, clean and confident:

> "Native access control. Not bolted on. Built in. Define who can read, write, and merge — down to individual files — using simple config files that live right in your project. No GitHub admin panel. No hoping your hosting provider's permission model matches what you actually need."

**VISUAL**: A `.wt/access/policies.toml` file. Clean TOML. Readable. A developer edits it. The server enforces it instantly.

> "File-level license compliance. Assign licenses to any path. MIT here. Proprietary there. The server enforces it. If someone tries to export code they're not licensed for — the system blocks it. Not an honor system. Not a LICENSE file people ignore. Actual enforcement."

> "Append-only history. No rebase. No force push. No rewriting the past. History is immutable. If you need to undo something, you revert — which creates a new snapshot. Nothing is ever silently deleted."

> "And everything — your access config, your ignore patterns, your branch protection rules — it's all declarative. Files in your project. Version-controlled. Auditable. No clicking through a web UI hoping you set the right checkbox."

**VISUAL**: The full architecture diagram, now complete. Local worker on the left. Server on the right. QUIC connection between them. Clean, symmetric, intentional.

> "One protocol. Encrypted. Authenticated. No separate SSH setup. No PAT tokens. No choosing between HTTPS and SSH and hoping the firewall cooperates."

---

### ACT IV — SEE IT WORK

**[4:15–4:45] — 30 seconds**

**VISUAL**: A real terminal. Dark theme. Clean font. Commands typed at realistic speed with brief pauses between each. No voice-over tricks — just the product working.

**ON SCREEN** (typed live, results appearing naturally):

```
$ wt init my-project
✓ Worktree initialized at ./my-project
✓ Worker started (PID 4821)
✓ Connected to wt.company.com as alice@company.com
```

*Quick cut.*

```
$ wt status
Branch: main
Worker: running, auto-sync active

Modified:
  services/auth-service/src/oauth.rs
  libs/shared-models/src/user.rs

Staged snapshots (not yet pushed): 2
```

*Quick cut.*

```
$ wt status --team
Your staged work:
  2 snapshots on main (auth-service/src/oauth.rs, shared-models/src/user.rs)

Teammates:
  bob@company.com  — 1 snapshot on fix/token-expiry (auth-service/src/tokens.rs)
  carol@company.com — 3 snapshots on feature/billing (billing-engine/src/pricing.rs)
```

*Quick cut.*

```
$ wt snapshot --message "OAuth token rotation with configurable expiry"
✓ Snapshot snap_f1e2d3 created
✓ Synced to server (staged, not pushed)
```

*Quick cut.*

```
$ wt push
✓ Pushed 3 snapshots to main
✓ Branch main updated: snap_7a8b9c → snap_f1e2d3
```

*Quick cut.*

```
$ wt access test intern@company.com tree:write config/production.toml
✗ DENIED
  Policy: "lock-production-config" (deny tree:write on registered path config/production.toml)
  Scope: RegisteredPath
  Source: .wt/access/policies.toml line 34
```

**NARRATOR (V.O.)** (returning, quieter, letting the terminal speak):

> "One command per job. Plain language. Human-readable errors that tell you exactly what happened and why. No Googling cryptic messages. No Stack Overflow rabbit holes. It just works the way you always wished it would."

---

### CLOSE — THE INVITATION

**[4:45–5:00] — 15 seconds**

**VISUAL**: The terminal fades. The W0rkTree logo returns, centered, breathing gently with a subtle glow. The background is deep navy. Clean. Quiet.

**NARRATOR (V.O.)**:

> "Git was the right tool for 2005. W0rkTree is the right tool for what comes next. Your repos import cleanly. Your workflow gets simpler. Your team gets visibility they've never had. And you never lose work again."

Beat.

> "W0rkTree. Version control, rebuilt from zero."

**ON SCREEN** (fading in below the logo):

> **w0rktree.dev**
>
> Star us on GitHub. Join the Discord. Build with us.

**VISUAL**: Holds for 3 seconds. Fade to black.

---

## PRODUCTION NOTES

### Music & Sound Design

| Segment | Audio Direction |
|---|---|
| Cold Open (0:00–0:25) | Quiet. Ambient room tone. A single keyboard. The silence IS the tension. No music. |
| Title Card (0:25–0:30) | A single low synth tone. Confident, not dramatic. Think the Vercel or Linear reveal sound. |
| Act I — History (0:30–1:30) | Subtle documentary-style underscore. Warm but building. Piano and light strings. Nostalgic at first, then darkening as we approach "it's not 2005 anymore." |
| Act II — Problems (1:30–2:45) | Tension. Minimal electronic beats. Each numbered problem hits with a percussive impact. The music is rhythmic, driving, slightly uncomfortable — matching the frustration. |
| Act III — W0rkTree (2:45–4:15) | The shift. The music opens up. Clean, modern, confident. Think product launch energy — not hype, but clarity. Synths with space. Building toward the architecture reveal. |
| Act IV — Demo (4:15–4:45) | Music drops to near-silence. Just subtle ambient tone. Let the terminal sounds and typing carry the moment. The product speaks for itself. |
| Close (4:45–5:00) | The main theme returns, resolved. One clear melodic phrase. Fade out with the logo. |

### Visual Style Guide

| Element | Direction |
|---|---|
| **Color palette** | Deep navy (#0a0f1a), white (#f0f0f0), accent cyan (#00d4ff), warning red (#ff3b3b) for Git problems, confident green (#00c48f) for W0rkTree solutions |
| **Typography** | Monospace for all terminal/code (JetBrains Mono or similar). Clean sans-serif for titles and callouts (Inter, Geist, or similar). |
| **Terminal styling** | Real terminal, not mockup. Dark theme. No window chrome — just the content. Commands typed at human speed with natural pauses. |
| **Diagrams** | Animated SVG-style. Clean lines. Components build on screen piece by piece. No clip art. No stock icons. Custom illustrations. |
| **Transitions** | Hard cuts between segments. No dissolves, no wipes, no swooshes. Confidence. Each scene starts clean. |
| **Face on camera** | Only the Cold Open uses a person (the frustrated developer). The rest is narration over visuals. The product is the star, not a presenter. |

### Narrator Casting Notes

- **Voice**: Male or female — doesn't matter. What matters: calm authority. Not excited. Not selling. Explaining. Like a senior engineer walking you through something they built and they're genuinely proud of, but they're not going to oversell it because the work speaks for itself.
- **Pace**: Measured. Not rushed. This is a 5-minute video, not a 60-second ad. Give the ideas room to land. Pauses are powerful.
- **Reference**: Think the narration style of a Fireship video but slower, or a well-produced documentary like "Explained" on Netflix.

### Key Messaging Principles

1. **Never mock Git.** Respect what it accomplished. Linus built something extraordinary in 2005. The point is that the world changed and Git didn't change with it. This is evolution, not contempt.
2. **Never promise magic.** W0rkTree is better by design, not by hype. Show the architecture. Show the terminal. Let engineers evaluate it on merit.
3. **Always ground in pain.** Every feature we show must connect back to a real problem the viewer has experienced. "Staged snapshot visibility" means nothing until you say "you'll never be surprised by a merge conflict again."
4. **Lead with the team, not the individual.** Git works fine for solo developers. W0rkTree's edge is what happens when teams use it — visibility, access control, license compliance, no-more-force-push safety. The pitch is: your team deserves better tools.

---

## DIALOGUE SCRIPT — CONTINUOUS

Below is the complete narrator dialogue as a single continuous script, for recording.

---

*[Cold Open]*

You know this feeling. That moment where you're not building software anymore — you're fighting your own tools. Three hours of work. Gone. Not because of a bug in your code. Because of a bug in your workflow.

And the worst part? Everyone just accepts it. Like this is normal. Like this is fine.

*[Act I]*

April 2005. Linus Torvalds is mass-emailing the Linux kernel mailing list. BitKeeper — the version control system the kernel team had been using for free — just revoked their license. The entire Linux development workflow is about to collapse.

So Linus does what Linus does. He builds his own. In ten days, he writes the first version of Git. A distributed version control system designed for one very specific job: managing the Linux kernel's source code across thousands of contributors, none of whom trust each other.

And for that job? Git was brilliant. Truly. Content-addressable storage. A Merkle tree of commits. Cheap branching. Cryptographic integrity. For coordinating a massive, decentralized, open-source project in 2005 — it was exactly the right tool.

But here's the thing nobody wants to say out loud.

It's not 2005 anymore.

Git was designed for the Linux kernel. Not for your microservices monorepo. Not for your fifty-person team on four continents. Not for your designer who just wants to save their work without accidentally deleting the production branch. Git solved Linus's problem. And then the entire industry adopted it — and inherited all of its assumptions.

Twenty years later, we're still paying that tax. Every single day.

*[Act II]*

Number one. Git has a jargon problem. Ref. Refspec. HEAD. Detached HEAD. Origin. Upstream. Index. Staging area. Working tree. Stash. The thing is — half of these are synonyms for each other, and the other half mean completely different things depending on context.

Git checkout — one command — does five completely different things depending on what flags you pass it. That's not power. That's bad design wearing a trenchcoat.

Number two. In Git, all work is invisible until someone pushes. You have no idea what your teammates are working on. You don't know Alice has been editing the same file as you for three hours until you both push and the merge conflict explodes. So teams hold standups. They post in Slack. They update tickets. All to compensate for the fact that their version control system has zero awareness of what anyone is actually doing.

Number three. Git lets you destroy things. Easily. git reset --hard. git push --force. git rebase and drop. These commands delete history. And Git doesn't stop you. It doesn't warn you. Your colleague's weekend of work — gone. One command. And sure, the reflog exists — if you know about the reflog, and you find it in time, and you haven't garbage collected yet. That's not a safety net. That's a rumor of a safety net.

Number four. Git has no built-in access control. None. No authentication. No authorization. No concept of "this person can read this folder but not that one." Every solution — GitHub permissions, GitLab roles, Bitbucket restrictions — is bolted on by a third party. The protocol itself? The native git-colon-slash-slash protocol has no encryption and no auth. In 2025. We just... accepted that.

And number five. Git was not built for how modern teams actually work. Large binary files? You need a separate system called LFS. Multiple projects in one repo? Good luck with sparse checkout and submodule hell. Partial history? Shallow clones break half your tooling. Git scales down to one person beautifully. Scaling it up to a real organization? That's where you start building workarounds on top of workarounds on top of workarounds.

So we asked a simple question.

What if we stopped building workarounds — and built the thing that should have existed all along?

*[Act III]*

This is W0rkTree. Not a Git wrapper. Not a Git extension. Not a hosting platform for Git repos. A ground-up replacement for Git — with a migration bridge so you can bring your existing repos with you.

W0rkTree runs two systems. On your machine — a background process we call the worker. It watches your files. It snapshots your work automatically. It handles branching, diffing, merging — everything you'd expect from version control — all locally, all instantly.

On the server — a multi-tenant platform that your whole team connects to. It stores canonical history, enforces access control, manages tenants and teams — and does something Git has never done.

When the worker captures a snapshot of your work — whether automatic or manual — it syncs that snapshot to the server as a staged snapshot. Not pushed. Not merged. Staged. Your team can see what you're working on and which files you're touching — in real time — without you doing anything.

Think about what that means. No more "I had no idea you were working on that file." No more merge conflicts that could have been prevented with thirty seconds of awareness. The entire team sees the full picture of what's in flight — and when you're ready, you explicitly push your staged work to the branch. Visible doesn't mean merged. Staged doesn't mean pushed. You're always in control.

And because we built this from scratch, we built the things Git never had.

Native access control. Not bolted on. Built in. Define who can read, write, and merge — down to individual files — using simple config files that live right in your project. No GitHub admin panel. No hoping your hosting provider's permission model matches what you actually need.

File-level license compliance. Assign licenses to any path. MIT here. Proprietary there. The server enforces it. If someone tries to export code they're not licensed for — the system blocks it. Not an honor system. Not a LICENSE file people ignore. Actual enforcement.

Append-only history. No rebase. No force push. No rewriting the past. History is immutable. If you need to undo something, you revert — which creates a new snapshot. Nothing is ever silently deleted.

And everything — your access config, your ignore patterns, your branch protection rules — it's all declarative. Files in your project. Version-controlled. Auditable. No clicking through a web UI hoping you set the right checkbox.

One protocol. Encrypted. Authenticated. No separate SSH setup. No PAT tokens. No choosing between HTTPS and SSH and hoping the firewall cooperates.

*[Act IV]*

One command per job. Plain language. Human-readable errors that tell you exactly what happened and why. No Googling cryptic messages. No Stack Overflow rabbit holes. It just works the way you always wished it would.

*[Close]*

Git was the right tool for 2005. W0rkTree is the right tool for what comes next. Your repos import cleanly. Your workflow gets simpler. Your team gets visibility they've never had. And you never lose work again.

W0rkTree. Version control, rebuilt from zero.

---

## TIMING BREAKDOWN — WORD COUNT VALIDATION

Professional narration pace: ~150 words per minute.

| Segment | Word Count | Target Duration | Actual at 150 WPM |
|---|---|---|---|
| Cold Open | ~65 words | 25s | ~26s ✓ |
| Act I — History | ~230 words | 60s | ~92s ⚠ (trim if needed) |
| Act II — Five Problems | ~340 words | 75s | ~136s ⚠ (trim if needed) |
| Act III — W0rkTree | ~330 words | 90s | ~132s ⚠ (trim if needed) |
| Act IV — Demo | ~35 words | 30s (mostly terminal) | ~14s narration + 16s terminal ✓ |
| Close | ~45 words | 15s | ~18s ✓ |
| **Total** | **~1,045 words** | **5:00** | **~7:00 at pure narration** |

> **Note on timing**: The raw word count exceeds 5 minutes at a standard narration pace. This is intentional — Acts I, II, and III are designed to play **over visuals and animations**, not in silence. The narrator speaks while diagrams build, while terminals type, while illustrations animate. In practice, the pacing will be faster than 150 WPM in dense sections (closer to 170–180 WPM, which is standard for tech explainer videos like Fireship or Vox). Additionally, the director should treat the script as the maximum — cut lines during editing to hit the 5:00 mark. Lines marked with *(optional cut)* below are safe to remove without losing narrative coherence.

### Suggested Cuts If Over Time

If the video runs long in edit, these lines can be trimmed first:

1. Act I — *"Content-addressable storage. A Merkle tree of commits. Cheap branching. Cryptographic integrity."* → Can be replaced with just *"And the engineering was brilliant."*
2. Act II, Problem 1 — *"Git checkout — one command — does five completely different things depending on what flags you pass it."* → Cut entirely; the jargon list already makes the point.
3. Act II, Problem 3 — *"And sure, the reflog exists — if you know about the reflog, and you find it in time, and you haven't garbage collected yet."* → Shorten to *"And Git's safety net — the reflog — is a secret most developers don't know about until it's too late."*
4. Act II, Problem 4 — *"The protocol itself? The native git-colon-slash-slash protocol has no encryption and no auth. In 2025."* → Cut; the point is made with the first two sentences.
5. Act III — *"Not a Git wrapper. Not a Git extension. Not a hosting platform for Git repos."* → Shorten to *"Not a wrapper. Not an extension."*

With these cuts, the script drops to approximately 900 words, comfortably fitting 5:00 at 175–180 WPM with visual breathing room.

---

## B-ROLL & ASSET LIST

| Asset | Type | Used In | Notes |
|---|---|---|---|
| Developer at desk (frustrated) | Live action or animation | Cold Open | Single actor, minimal set — just a desk, monitor, dark room |
| Terminal recordings | Screen capture | Cold Open, Act IV | Use a real terminal with realistic typing speed. No sped-up playback. |
| Linus Torvalds archival footage | Stock/archival or illustrated | Act I | If rights are an issue, use stylized illustrations of the era instead |
| Linux kernel file tree visualization | Motion graphics | Act I | Animated file tree expanding. Thousands of files. |
| Git jargon word cloud | Motion graphics | Act II, Problem 1 | Words flying in, overlapping, growing chaotic |
| Split-screen developers | Motion graphics or live action | Act II, Problem 2 | Two developers, same file, collision |
| "Thanos snap" file deletion | Motion graphics | Act II, Problem 3 | Branches and files dissolving |
| Unlocked vault | Motion graphics | Act II, Problem 4 | Files flowing out unprotected |
| Jenga tower of workarounds | Motion graphics | Act II, Problem 5 | Blocks labeled with Git tooling names, wobbling |
| W0rkTree architecture diagram | Motion graphics (animated SVG) | Act III | Builds piece by piece: local worker → server → QUIC connection |
| Staged snapshot dashboard | UI mockup or motion graphics | Act III | Team dashboard showing staged work per person |
| `.wt/access/policies.toml` file | Screen capture or motion graphics | Act III | Clean TOML file, readable |
| CLI demo recordings | Screen capture | Act IV | `wt init`, `wt status`, `wt status --team`, `wt snapshot`, `wt push`, `wt access test` |
| W0rkTree logo animation | Motion graphics | Title Card, Close | Clean reveal. Subtle glow on the zero. |

---

## DISTRIBUTION PLAN

| Platform | Format | Notes |
|---|---|---|
| YouTube | Full 5:00 video | Primary release. SEO title: *"Git Is Broken. We Built the Fix. Introducing W0rkTree."* |
| Twitter/X | 60s cut (Act II highlights + Act III intro) | Hook: the five problems, reveal: W0rkTree exists |
| LinkedIn | 90s cut (Act I + Act III) | Professional tone. Focus on team visibility and access control. |
| Hacker News | Link to YouTube + text post summarizing the architecture | HN loves technical depth. Link to the spec plan too. |
| Reddit (r/programming, r/git) | YouTube link + discussion prompt | *"We're building a ground-up Git replacement. Here's why."* |
| Discord / Community | Full video + behind-the-scenes | Show the architecture diagram, invite feedback |

---

*Last updated: 2025*
*Document version: 1.0*
*Status: READY FOR PRODUCTION REVIEW*