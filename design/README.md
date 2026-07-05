# AI Tinkerers design package

Source of truth for the Mission Control app's visual language, extracted and measured from https://aitinkerers.org/ (July 2026). Sits alongside `openspec/changes/add-event-mission-control-v1`.

## Contents

- `mission-control.html` — interactive design proposal covering all four v1 surfaces: onboarding (valid + invalid key states), events overview (capacity gauge, RSVP funnel, truncation notice, sync status), event detail (RSVP summary, awaiting-payment, performance chart, `forbidden_api_group` degraded state, gallery), and the menubar tray item + popover. Open it directly in a browser; asset paths are relative to this folder. All UI renders from a single in-file `EVENTS` object — the stand-in for the SQLite cache (design D2), so the component-to-cache mapping is already explicit.
- `DESIGN.md` / `BRAND.md` — visual principles, voice, and measured usage notes.
- `brand.json` — machine-readable brand definition (colors with OKLch, typography, logo manifest, imagery samples, layout posture).
- `tokens/` — CSS custom properties (`variables.css`, `variables.dark.css`) and JSON token sets (default/dark/compact) plus `theme.json` (Ant-style token map: `colorPrimary #31439b`, `colorBgBase #f8fafc`, `borderRadius 16`).
- `fonts/` — Source Sans 3 woff2 (400/600/700/900). Note: these files are Vietnamese-range subsets; the prototype loads the full family from Google Fonts. For the shipped app, self-host full-range subsets or keep the declared fallback stack.
- `logos/` — primary transparent wordmark (`logo-stacked-760.png`), sticker variant, square app icon (`favicon-0.png`, use for tray/dock).
- `imagery/` — real city event photos used in gallery mocks.

## Core tokens (quick reference)

| Role | Hex |
| --- | --- |
| background | `#f8fafc` |
| surface | `#ffffff` |
| foreground | `#0f172a` |
| muted | `#64748b` |
| border | `#e2e8f0` |
| accent (CTA) | `#31439b` (hover `#253274`) |
| accent-secondary | `#3d53c2` (borders / 10–20% tints) |

Posture: light canvas, white 16px-radius cards with 1px `#e2e8f0` borders and small shadows; pills for chips/buttons; indigo reserved for CTAs and data accents, never large washes; tabular mono (`ui-monospace` stack) for all counts.
