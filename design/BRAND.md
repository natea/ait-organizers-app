# AI Tinkerers — Brand Guide

*The global community for hands‑on AI builders.* Curated meetups, hackathons, dinners, and demos for people shipping with foundation models across 245 cities. No slides, no pitches.

All values below were measured from https://aitinkerers.org/ (captured HTML + CSS, July 2026), not guessed.

## The one correction that matters

AI Tinkerers is a **light** design system. The body is `background-color: #f8fafc` (Slate 50) with white cards; dark slate is the *ink*, not the canvas. An earlier automated pass inverted this (black background) because `#000000` literals dominate shadows and masks in the CSS — do not repeat that mistake. Dark surfaces appear only in the footer band (`bg-slate-950`) and terminal-styled motifs.

## Color

| Role | Hex | OKLch | Measured usage |
| --- | --- | --- | --- |
| Background | `#f8fafc` | oklch(98.4% 0.003 247.9) | body canvas |
| Surface | `#ffffff` | oklch(100% 0 0) | cards (`bg-white` ×65) |
| Foreground | `#0f172a` | oklch(20.8% 0.040 265.8) | headlines + body ink; footer band |
| Muted | `#64748b` | oklch(55.4% 0.041 257.4) | secondary text (`#475569` for stronger support copy) |
| Border | `#e2e8f0` | oklch(92.9% 0.013 255.5) | 1px hairlines everywhere |
| Accent | `#31439b` | oklch(42.2% 0.145 270.3) | primary buttons (`.tw-button-brand`); hover `#253274` |
| Accent secondary | `#3d53c2` | oklch(49.1% 0.175 270.2) | accent borders, tinted chips (`/10`–`/20` opacity washes) |

Rules: accent is a button-and-border color, never a large wash. Tints of `#3d53c2` at 10–20% opacity are the only permitted colored backgrounds besides slate neutrals.

## Typography

- **Display & body: Source Sans Pro** (Google Fonts now serves it as *Source Sans 3*), fallbacks Open Sans → system sans. Hero h1: `text-5xl/6xl`, extrabold, `tracking-tight`, slate-900. The workhorse emphasis weight is **semibold 600** (×235 on the homepage); 400 for body, 700/900 for display.
- **Mono:** system stack (`ui-monospace, SFMono-Regular, Menlo…`) for terminal-styled components and telemetry readouts.
- **Accent handwritten face: Pangolin** (self-hosted woff2 on the site) — used only for "sticky-tape" labels on a yellow `#FFF9C4→#FFF59D` gradient, taped over photo collages. This is the brand's single playful flourish; use it at most once per artifact.

## Layout posture

- Centered max-width container, generous section padding, 8px baseline grid.
- Cards: white, `rounded-2xl` (16px — ×138 occurrences), 1px `#e2e8f0` border, `shadow-sm`. Hero photo cards get the deeper `0 12px 28px rgba(15,23,42,.12)` shadow and `hover:scale-105` on the image.
- Chips, tags, avatars and secondary buttons are **full pills** (`rounded-full` ×337).
- Primary CTA: solid `#31439b`, white text, pill or rounded; hover `#253274`.

## Voice

Direct, practitioner-to-practitioner, quietly confident. Short declarative sentences; evidence over hype.

Pillars (verbatim from the site):
- "Bring working code, share what broke, and leave with patterns you can ship."
- "We focus on demos, code, and technical insight. No sales pitches, no generic AI panels. Working systems set the agenda."
- "Demos, traces, systems, and scars carry more weight than predictions."
- "Like the Homebrew Computer Club — small rooms where practitioners compare notes before the path is obvious."

**Use:** builders, demos, working code, shipping, traces, failure modes, curated rooms, practitioners.
**Avoid:** thought leadership, AI panels, sales pitches, "revolutionary/game-changing" hype, predictions without demos.

## Imagery

Real documentary photos from actual meetups — crowded rooms, laptops open, someone demoing at a projected screen. Warm indoor light, candid framing. Seven city-labeled event photos are saved under `imagery/` (San Francisco, Paris, São Paulo, Nürnberg, Seattle, Boston, Valencia) plus the social og:image. Never use stock business imagery or abstract AI brain/robot art.

## Logo

Primary: `logos/logo-stacked-760.png` — the rounded indigo "AI TINKERERS" wordmark on transparent PNG. The mark's own indigo is in the `#31439b`/`#3d53c2` family; place it on white or Slate 50, never on the accent color itself. Square app icon: `logos/favicon-0.png`.
