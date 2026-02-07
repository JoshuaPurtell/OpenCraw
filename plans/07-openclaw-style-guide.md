# OpenClaw Visual Style Guide (Reference Audit)

Date: 2026-02-07  
Source audited: `https://openclaws.io`  
Routes sampled: `/`, `/install`, `/showcase`, `/shoutouts`, `/faq`, `/blog/`  
Data source: live HTML + shipped CSS bundle (`/_astro/_slug_.BY5wMtfu.css`)

## 1. Design Character

OpenClaw uses a **dark-first developer aesthetic** with a single warm accent color and high contrast typography:

- Visual mood: terminal/tooling UI + product marketing.
- Primary accent: lobster/coral red.
- Surfaces: near-black dark panels with low-opacity borders.
- Shape language: medium-large rounded corners (`0.5rem`, `0.75rem`, `1rem`, `2rem`, pills).
- Motion: subtle transitions and hover emphasis, not heavy animation.

## 2. Color System (Observed)

Core brand/accent:

- `primary`: `rgb(255 90 80)` (`#ff5a50`)
- Primary transparencies used heavily:
  - `#ff5a501a` (10%)
  - `#ff5a5033` (20%)
  - `#ff5a504d` (30%)
  - `#ff5a5066` (40%)
  - `#ff5a5080` (50%)

Background/surface:

- Light bg token: `.bg-background-light` -> `rgb(248 250 252)` (`#f8fafc`)
- Dark bg token: `.dark:bg-background-dark` -> `rgb(5 7 10)` (`#05070a`)
- Card dark token: `.bg-card-dark` -> `rgb(13 17 23)` (`#0d1117`)
- Card dark overlays: `/40`, `/50`, `/60` variants

Text:

- Primary text in dark mode: `.dark:text-slate-100` -> `rgb(241 245 249)`
- Secondary text mostly `slate-300`, `slate-400`, `slate-500`.

Other accents used contextually:

- Cyan/blue (`#00a4ef`, Tailwind cyan/blue utility shades)
- Success green utilities
- White with low alpha for borders and frosted surfaces.

## 3. Typography

Font families found:

- `"Space Grotesk"` (display/body)
- `"JetBrains Mono"` (code/terminal/labels)
- `"Material Symbols Outlined"` (icon font)

Type scale pattern:

- Hero H1: `text-5xl` -> `md:text-8xl` with tighter tracking.
- Section H1/H2: `text-3xl` to `text-7xl` depending on page context.
- Card titles: `text-xl` + `font-bold`.
- Body copy: `text-sm` to `text-xl`, with `leading-relaxed`.
- Label/meta text: `text-xs`, `text-[10px]`, uppercase mono for commands/badges.

Signature treatments:

- Gradient-clipped hero title (`bg-clip-text text-transparent`).
- Mono uppercase tagline with wide tracking.

## 4. Spacing and Layout

Common spacing rhythm from class usage:

- Horizontal padding: `px-4`, `px-5`, `px-6`, `px-8`.
- Vertical padding: `py-2`, `py-3`, `py-4`, `py-5`, section `py-12` / `py-24`.
- Gaps: `gap-1`, `gap-2`, `gap-3`, `gap-4`, `gap-5`, `gap-8`.
- Width constraints: frequent `max-w-2xl`, `max-w-4xl`, `max-w-6xl`.

Layout style:

- Centered container, then stacked sections.
- Grid-heavy for cards/testimonials/showcase entries.
- Mobile-first responsive jumps (`md:` breakpoints heavily used).

## 5. Components and Patterns

### 5.1 Header/Nav

- Slim top nav, many text links.
- Active/hover state mostly color-shift to `primary`.
- Language switcher dropdown with subtle surface hover states.

### 5.2 Hero

- Large headline + concise subcopy.
- Primary CTA emphasis via accent color.
- Decorative glow/blur circles in background.

### 5.3 “Quick Start” / Command Blocks

- Terminal-like cards with mono text.
- Copy buttons, bordered dark cards, low-opacity overlays.
- Command rows are compact, utilitarian, high-contrast.

### 5.4 Capability / Feature Cards

- Rounded cards (`rounded-2xl`) with thin accent-tinted borders.
- Icon + title + short paragraph.
- Hover adds border and glow/brightness.

### 5.5 Social Proof Sections (Showcase/Shoutouts)

- Dense cards/quotes with clamped text.
- Masonry-like card rhythm (`break-inside-avoid` usage).
- Prominent quote text sizes (`text-xl md:text-2xl`) for testimonials.

### 5.6 FAQ

- Accordion rows (`faq-item`, `faq-toggle`, `faq-answer` hooks).
- Border + translucent surface change on hover/expand.
- Icons rotate/transition for open state.

### 5.7 Newsletter/Footer

- Reusable “Stay in the Loop” CTA block near bottom.
- Compact footer with muted/legal text and social/community links.

## 6. Motion and Interaction

Frequent interaction classes:

- `transition-colors`, `transition-all`, `transition-transform`
- `duration-200`, `duration-300`
- Hover: border accent, text accent, brightness boost
- Micro-motion: `hover:scale-110` for floating action/buttons

Motion profile is restrained and readable:

- Fast transitions, minimal travel distance.
- No heavy page-level animation dependency.

## 7. Border, Radius, and Surface Rules

Observed radii:

- `.25rem`, `.5rem`, `.75rem`, `1rem`, `2rem`, `9999px`

Borders:

- Default 1px thin lines, often with alpha.
- Accent borders are common at low opacity (`primary/20`, `primary/40`).
- Frosted/glass treatment: `backdrop-filter: blur(8px)` + very low white border.

Shadows:

- Standard shadow utilities are used sparsely.
- More common are glow-style hover shadows in accent color.

## 8. Reusable Token Recommendations (for OpenCraw)

If we mirror this style direction, implement these foundation tokens:

- `--color-bg-light: #f8fafc`
- `--color-bg-dark: #05070a`
- `--color-card-dark: #0d1117`
- `--color-primary: #ff5a50`
- `--radius-sm: 0.5rem`
- `--radius-md: 0.75rem`
- `--radius-lg: 1rem`
- `--radius-xl: 2rem`
- `--font-display: "Space Grotesk", sans-serif`
- `--font-mono: "JetBrains Mono", monospace`

Then map utilities/components around:

- dark shell + glass cards
- accent-tinted borders
- mono labels for technical metadata
- compact hover transitions.

## 9. Implementation Notes

- OpenClaw style is utility-driven and likely Tailwind-centered.
- The site uses strong consistency across all primary routes.
- This is a good reference for:
  - dark product marketing pages
  - developer-facing dashboard shells
  - command/terminal presentation UI.
