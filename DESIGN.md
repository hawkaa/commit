# Design System — Commit

## Product Context
- **What this is:** A behavioral trust layer that surfaces ZK-verified commitment signals alongside search results and GitHub repos
- **Who it's for:** Open-source developers and crypto-native builders evaluating tools, libraries, and services
- **Space/industry:** Developer tools, trust/verification, blockchain infrastructure
- **Project type:** Hybrid (app UI for extension overlay + data cards, marketing-like for shareable trust card pages)

## Aesthetic Direction
- **Direction:** Industrial/Utilitarian with monospace accents
- **Decoration level:** Minimal. Typography and data do all the work. The only decorative element is the Commit Score circle.
- **Mood:** Rationalist, austere, trustworthy. The design mirrors the thesis: stripped of decoration, grounded in evidence. Trust through austerity.
- **Reference sites:** getcommit.dev (existing, the baseline), Snyk Advisor (category standard), shields.io (badge standard)

## Typography
- **Display/Hero:** Geist (800 weight) — clean, geometric, built for data. Vercel association signals "built by serious engineers"
- **Body:** Geist (400 weight) — unified stack, consistency across all surfaces
- **UI/Labels:** Geist (600 weight, 11px, uppercase, letter-spacing 0.5-1.5px)
- **Data/Tables:** Geist with `font-variant-numeric: tabular-nums` — numbers align in columns
- **Code:** JetBrains Mono — existing choice from getcommit.dev, excellent for terminal context
- **Loading:** Google Fonts CDN: `family=Geist:wght@400;500;600;700;800` + `family=JetBrains+Mono:wght@400;500;600`
- **Scale:** 11px labels, 13px body, 16px subheads, 20px page titles, 24px section heroes, 28px+ score display

## Color
- **Approach:** Restrained. The only color is semantic (score quality + ZK verification). Everything else is grayscale.
- **Score High:** `#16a34a` (green, >70) — gradient to `#15803d` for score circles
- **Score Mid:** `#ca8a04` (amber, 40-70) — gradient to `#a16207` for circles
- **Score None:** `#6b7280` (gray, no data)
- **ZK Accent:** `#7c3aed` (violet, used exclusively for ZK-verified badges/tags). Background: `rgba(124, 58, 237, 0.1)`
- **Ink (text primary):** `#1a1a2e` — near-black with warm undertone
- **Ink secondary:** `#666666`
- **Ink tertiary:** `#888888`
- **Paper (background):** `#f5f5f0` — warm paper tone. Distinguishes from GitHub's cool grays. Like a verified document, not a SaaS dashboard.
- **Surface:** `#ffffff` — cards, modals, inputs
- **Border:** `#e5e5e0` — warm gray border
- **Border light:** `#f0f0eb` — subtle dividers inside cards
- **Semantic:** success `#16a34a`, warning `#ca8a04`, error `#dc2626`, info `#2563eb`
- **Dark mode:** Reduce surface to `#0f0f14`, cards to `#1a1a20`, borders to `#2a2a30`. Score circle gradients remain unchanged. Reduce text saturation. Score colors are the anchors, everything else adapts.

## Spacing
- **Base unit:** 4px
- **Density:** Comfortable (not cramped, not spacious)
- **Scale:** 2xs(2px) xs(4px) sm(8px) md(12px) lg(16px) xl(24px) 2xl(32px) 3xl(48px) 4xl(64px)
- **Trust card padding:** 10-14px (compact, doesn't fight host page)
- **Trust card page padding:** 32px (breathable)
- **Badge dimensions:** 88x20px (shields.io standard)

## Layout
- **Approach:** Grid-disciplined
- **Trust card:** Fixed-width component that adapts to host context. Flex row: score circle + text block.
- **Trust card page:** Centered content column, max-width 680px
- **Signal grid:** 4 columns on desktop, 2x2 on mobile
- **Max content width:** 680px (trust card pages), fluid (extension injection)
- **Border radius:** sm(4px) for badges/tags, md(6px) for cards/inputs, lg(12px) for page containers, full(50%) for score circles

## Motion
- **Approach:** Minimal-functional
- **Score fill:** 400ms ease-out fill animation on first load (score circle fills from 0 to value)
- **Skeleton shimmer:** 1.5s ease-in-out pulse animation for loading state
- **Card entrance:** 150ms ease-out fade-in after data loads
- **Easing:** enter(ease-out) exit(ease-in) move(ease-in-out)
- **Duration:** micro(50-100ms) short(150ms) medium(250-400ms)
- **No scroll animations. No hover transforms. No decorative motion.**

## Brand Mark
The **Commit Score circle** is the brand mark. A filled circle with a gradient (green/amber/gray based on score). It appears in all 4 contexts:
- Extension trust card: 48px diameter
- Google SERP: 28px diameter
- Trust card page: 72px diameter
- Badge: text-only (no circle, just the number in a colored rectangle)

The score circle is the ONE decorative element in an otherwise austere system. It should be instantly recognizable.

## Component Inventory
- **Trust card (inline):** Score circle + label + signals + network badge. Flex row.
- **Trust card (page):** Score hero + signal grid + endorsement list + CTA
- **Badge:** shields.io-style flat rectangle. Label "commit" + value (score number or "—")
- **ZK tag:** 9px, `#7c3aed` on violet-10% background, rounded 3px. Used inline.
- **Button primary:** `#1a1a2e` background, white text, 6px radius
- **Button secondary:** transparent, border, 6px radius
- **Input:** white background, border, 6px radius. Focus: 2px green outline.

## Decisions Log
| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-04-10 | Initial design system created | Built by /design-consultation. Industrial/utilitarian aesthetic matching getcommit.dev's existing direction. Geist + JetBrains Mono. Warm paper background. Score circle as brand mark. |
| 2026-04-10 | Score-first trust card hierarchy | CEO review decided: Commit Score is the hero element. Individual signals are the expandable breakdown. People remember numbers. |
| 2026-04-10 | Click score → navigate to trust card page | Design review decided: extension card is a teaser. Full experience on commit.dev/trust/... Feeds growth loop. |
