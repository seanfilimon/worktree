# Scene 2 — Title Card: Brand Reveal

**Timecode**: 0:25–0:30
**Duration**: 5 seconds
**Purpose**: Brand moment. A breath between the pain (Cold Open) and the explanation (Act I). This is where the viewer first sees W0rkTree.

---

## SHOT LIST

### SHOT 2.1 — Logo Fade

- **Timecode**: 0:25.000–0:28.000
- **Duration**: 3.0s
- **Frame**: Pure Deep Navy background (#0A0F1A). The W0rkTree logo fades in, dead center. The logo is set in Inter Tight, weight 800, white (#F0F0F0). The "0" (zero) in "W0rk" has a subtle cyan glow — a radial gradient of #00D4FF at 30% opacity, 20px blur radius, pulsing once during the reveal.
- **Animation sequence**:
  1. **0:25.000–0:25.300**: Background holds pure Deep Navy (300ms of nothing — let the eye reset from the hard cut)
  2. **0:25.300–0:26.300**: Logo fades in over 1000ms using ease-decelerate (`cubic-bezier(0.0, 0.0, 0.0, 1.0)`). Simultaneously, a very subtle Y-translate from +8px to 0px (the logo "settles" into place). The "0" glow begins at 0% and reaches 30% by the end of the fade.
  3. **0:26.300–0:28.000**: Logo holds. The glow on the "0" does a single pulse: 30% → 45% → 30% over 1200ms, ease-standard. This single pulse draws the eye to the zero and makes it feel alive.
- **Audio**: At 0:25.300 (when logo begins fading), a single synth tone plays. Deep, clean, confident. Not a stinger — a tone. Think a sustained low C (C2, ~65 Hz) with harmonics at the 5th (G2) fading in. The tone sustains for 2 seconds, then fades. Total loudness: −28 dB LUFS. This should feel like a room breathing, not an announcement.
- **On-screen**: Just the logo. "W0rkTree" in Inter Tight 800. Size: `display-lg` (128px at 4K, 64px at HD).

---

### SHOT 2.2 — Tagline Reveal

- **Timecode**: 0:28.000–0:30.000
- **Duration**: 2.0s
- **Frame**: The tagline fades in below the logo.
- **Animation sequence**:
  1. **0:28.000–0:28.600**: Tagline fades in over 600ms, ease-enter. Y-translate from +12px to 0px. The tagline is: "Version control, rebuilt from zero."
  2. **0:28.600–0:30.000**: Hold. Logo + tagline both visible.
- **Text**: `tagline` size (56px at 4K / 28px at HD), Inter weight 400, #F0F0F0 at 70% opacity. Letter-spacing: +0.02em. Centered below logo with 32px gap.
- **Audio**: The synth tone is fading out during this shot. At 0:28.0, the narrator does NOT speak — let the text speak for itself. By 0:29.5, audio has faded to near-silence, creating a clean transition to Act I.
- **Transition out**: Hard cut to Act I at 0:30.000

---

## TECHNICAL NOTES

- The logo **MUST** be delivered as a vector (SVG) so it renders crisp at any resolution.
- The glow effect on the "0" is achieved with a separate layer: duplicate the "0" character, apply Gaussian blur (20px radius), set color to #00D4FF, set opacity to 30%, and position exactly behind the original "0".
- The pulse animation on the glow: animate the blur layer's opacity from 30% → 45% → 30% using ease-standard over 1200ms.
- The tagline uses a different weight (400 Regular) and opacity (70%) from the logo to create clear visual hierarchy — the logo is the star, the tagline is supporting.
- The tagline italics: "Version control, rebuilt from zero." should be in regular weight, not italic, despite the storyboard showing it in markdown italics. The italic in the storyboard was for emphasis in the script document, not a visual direction.