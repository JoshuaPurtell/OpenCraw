# OpenCraw Creek Visual Redesign Plan

Date: 2026-02-07  
Scope: `/Users/synth/OpenCraw/web/src/App.tsx`, `/Users/synth/OpenCraw/web/src/index.css`

## 1. Why This Plan Exists

Current issues from the latest UI pass:

- The large light surfaces still read as sterile.
- Background and section gradients feel decorative instead of intentional.
- Color is present but not organized into a coherent hierarchy.
- Visual emphasis is inconsistent between status, actions, and content.

Goal: move to a **creek-bank utility UI** that feels warm, practical, and alive without gimmicky copy or mascot metaphors.

## 2. Constraints and Guardrails

- No anthropomorphic/novelty labels or themed phrases.
- Keep existing information architecture and interaction model.
- Keep the interface light mode for now.
- Use color and surface treatment, not extra ornament, to create character.
- Avoid “hero gradients” across large regions.

## 3. Design Direction: “Sediment + Waterline”

Visual language:

- Base environment: muted mineral background, not white paper.
- Foreground surfaces: slightly warm off-white cards with clear edge contrast.
- Accent family:
  - water teal (primary)
  - moss green (support/success)
  - clay/rust (warning/action contrast)
- Structure first: stronger section boundaries, less ambient glow.

In practice:

- Replace broad radial backdrop gradients with flatter layered backgrounds.
- Keep only very subtle tonal shifts (small delta) on surfaces.
- Shift color emphasis to components that carry meaning (status, CTA, sender/system/message roles).

## 4. Token Plan (Concrete)

Use one coherent token set; no ad-hoc per-component colors.

Base neutrals:

- `--canvas`: low-saturation mineral gray-green (non-white base)
- `--canvas-deep`: slightly darker companion for depth
- `--surface`: warm off-white (cards/forms)
- `--surface-strong`: mid-tone for secondary containers
- `--ink`: deep cool charcoal
- `--ink-muted`: medium contrast body/support text
- `--line`: clearly visible but soft border color

Accents:

- `--accent`: creek teal (primary actions + active state)
- `--accent-support`: moss (healthy/success/connected)
- `--accent-alt`: clay (warning/secondary emphasis)
- `--accent-soft`: desaturated teal for passive highlights

Semantic mapping:

- `connected` -> `accent-support`
- `connecting` -> `accent-alt`
- `disconnected` -> neutral line/surface-strong
- primary CTA -> `accent`
- secondary CTA -> neutral with accent border on hover

## 5. Surface and Gradient Policy

Hard rules:

- No full-container multi-color gradients.
- No blurred color blobs in the page background.
- Gradients allowed only in these places:
  - primary button background (2 stops, same hue family)
  - tiny decorative top rule (header accent bar), optional
- Any allowed gradient must stay within a narrow lightness range to avoid a “neon sweep” look.

Preferred replacements:

- Use two or three flat/tinted layers for depth:
  - page canvas
  - panel surface
  - inset log/input zones

## 6. Component-by-Component Plan

Header panel:

- Remove glow-heavy ornaments.
- Keep a restrained top accent rule or remove entirely if distracting.
- Increase contrast of title block against panel background.

Status chips:

- Distinct fill for each state; never border-only where clarity suffers.
- Keep uppercase micro-label style but increase legibility.

Info tiles:

- Use subtle semantic tint per tile purpose.
- Normalize border strength and shadow so tiles read as one system.
- Reserve strongest color for “Reconnect” and connection state only.

Chat log area:

- Give log container a gentle mid-tone background separate from page and cards.
- Role bubbles:
  - `you`: teal-tinted
  - `assistant`: clay-tinted but subdued
  - `system`: neutral mineral
- Ensure timestamp and role labels have sufficient contrast.

Composer row:

- Input gets solid high-contrast surface, no muddy gradient.
- Primary send button should feel prominent but not glossy.
- Disabled state should read inert via reduced contrast and shadow removal.

## 7. Typography and Rhythm

Typography:

- Keep current families unless changed globally later.
- Increase text contrast before increasing weight.
- Keep micro-label tracking; avoid over-condensed caps for body text.

Spacing/rhythm:

- Standardize vertical rhythm to a 4px or 8px scale.
- Keep panel radii consistent across header, tiles, and chat cards.
- Ensure mobile spacing remains comfortable at narrow widths.

## 8. Accessibility and Quality Gates

Minimum gates before merge:

- Body text contrast >= WCAG AA against all surfaces.
- Status chips distinguishable by more than color alone:
  - text label always visible (`connected`, `connecting`, `disconnected`).
- Keyboard focus ring visible on input and both action buttons.
- No motion dependency for conveying state.

## 9. Implementation Sequence

Phase 1: Token cleanup in CSS

- Consolidate and rename tokens for neutral/semantic clarity.
- Remove backdrop blob layers and broad page gradients.
- Set new surface stack and border/shadow baselines.

Phase 2: Component restyling in `App.tsx`

- Apply semantic token mapping consistently.
- Normalize tile/chip/button treatments.
- Simplify message bubble treatments with clearer role hierarchy.

Phase 3: QA and stabilization

- `bun run lint`
- `bun run typecheck`
- `bun run build`
- Manual visual pass at:
  - 1440px desktop
  - 768px tablet
  - 390px mobile

## 10. Definition of Done

The redesign is done when:

- The UI no longer reads as sterile white.
- Large decorative gradients/blobs are removed.
- Color hierarchy is obvious and consistent across states/components.
- The interface still feels operational and plain-spoken.
- Build, lint, and typecheck are all green.

## 11. Optional Follow-Up (Not in This Pass)

- Add a tiny texture/noise overlay at very low opacity to reduce flatness.
- Add dark mode with a separate token set based on the same creek palette.
