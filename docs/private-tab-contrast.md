# Private tab titles: contrast on every style

Krunk 0.16.0 chooses the colour of a private tab's title (and of its mask icon) for contrast
with what is drawn behind it: the first of

`#6d28d9`, `#a78bfa`, `#c4b5fd`, `#ddd6fe`, `#ede9fe`, `#5b21b6`, `#4c1d95`, `#2e1065`, `#ffffff`, `#111827`, `#000000`

that reaches 4.5:1 (WCAG 2 contrast ratio, AA for normal text) on every colour behind that state of
the tab; when none does, the one with the best worst case (`core/src/color.rs`,
`PrivateTabColors`). 0.15.3 drew `#6d28d9` everywhere, and the theme's muted grey on inactive
tabs while the window was in the background.

This file is generated: `LB_UPDATE_FIXTURES=1 cargo test -p core --lib color::` writes it, and
the test `private_tab_contrast_table_is_current` fails when it is stale. The UI test
`ui/dev/test-0160-private.cjs` draws the shell with the same colours and measures the pixels.

- **Behind the title:** the colour the 0.16.0 title reads worst on, of all the colours that can
  lie behind it in that state (a gradient strip: every point of the gradient; a glass strip:
  the strip over every shade of its accent backdrop; under the mouse on those two: the same
  under a 15 % veil of black or white, whichever keeps the title readable). On a solid or
  ombre strip a private tab lies flat on the strip's colour and a hovered one is filled with
  the theme's hover colour.
- **Ratios** are each title's worst case over those colours, rounded down. The 0.15.3 ratio
  is measured on what 0.15.3 drew (under the mouse on a gradient or glass strip: a 15 % tint
  of the title's own colour).
- A window in the background uses the colours of the focused one since 0.16.0.

## Worst case per style

The lowest ratio over both themes: on the strip (inactive and hovered, focused window and
background window), and over every state (the active private tab's fill and the ghost of a
dragged tab are the theme's own, the same on every style).

| Style | On the strip, 0.15.3 | On the strip, 0.16.0 | Every state, 0.15.3 | Every state, 0.16.0 |
|---|---:|---:|---:|---:|
| Classic | 1.55 | 5.73 | 1.55 | 4.61 |
| Sunset | 1.55 | 5.73 | 1.55 | 4.61 |
| Ocean | 1.00 | 5.49 | 1.00 | 4.61 |
| Glass | 1.29 | 4.97 | 1.29 | 4.61 |
| Mono | 1.55 | 5.73 | 1.55 | 4.61 |
| Aurora | 1.55 | 5.73 | 1.55 | 4.61 |
| Midnight | 1.41 | 5.61 | 1.41 | 4.61 |
| Forest | 1.00 | 4.60 | 1.00 | 4.60 |
| Ember | 1.00 | 5.17 | 1.00 | 4.61 |
| Candy | 1.55 | 5.73 | 1.55 | 4.61 |
| Frost | 1.75 | 5.35 | 1.75 | 4.61 |
| Neon | 2.38 | 6.89 | 1.76 | 4.61 |
| Synthwave | 1.55 | 5.73 | 1.55 | 4.61 |
| Matrix | 2.29 | 6.32 | 1.76 | 4.61 |
| Graphite | 1.43 | 5.58 | 1.43 | 4.61 |
| Citrus | 1.08 | 4.84 | 1.08 | 4.61 |
| Mint | 1.80 | 5.53 | 1.76 | 4.61 |
| Sakura | 1.68 | 5.13 | 1.68 | 4.61 |
| Coffee | 1.00 | 4.91 | 1.00 | 4.61 |
| Royal | 1.19 | 4.72 | 1.19 | 4.61 |

## Worst case of other looks

Custom colours are not styles; these show the rule on purple and dark strips, and its limit: no
single colour reaches 4.5:1 on a gradient that runs through mid-tones of very different
lightness, so the title gets the best there is.

| Look | On the strip, 0.15.3 | On the strip, 0.16.0 | Every state, 0.15.3 | Every state, 0.16.0 |
|---|---:|---:|---:|---:|
| Purple tab strip (#6d28d9) | 1.00 | 5.11 | 1.00 | 4.61 |
| Dark tab strip (#0b1220) | 1.55 | 5.73 | 1.55 | 4.61 |
| Lavender window (#ede9fe) | 1.55 | 5.73 | 1.55 | 4.61 |
| Glass, purple accent (#7c3aed) | 1.52 | 5.21 | 1.52 | 4.61 |
| Solid tabs, glass toolbar, purple accent (#7c3aed) | 1.24 | 4.79 | 1.24 | 4.61 |
| Midnight, purple accent (#7c3aed) | 1.41 | 5.61 | 1.41 | 4.61 |
| Gradient from a purple accent (#7c3aed) | 1.01 | 3.68 | 1.01 | 3.68 |

## Built-in styles

| Style | Theme | State | Behind the title | 0.15.3 title | Ratio | 0.16.0 title | Ratio |
|---|---|---|---|---|---:|---|---:|
| Classic | light | inactive | `#f3f4f6` | `#6d28d9` | 6.45 | `#6d28d9` | 6.45 |
| Classic | light | hovered | `#e5e7eb` | `#6d28d9` | 5.73 | `#6d28d9` | 5.73 |
| Classic | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Classic | light | inactive, window in the background | `#f3f4f6` | `#6b7280` | 4.39 | `#6d28d9` | 6.45 |
| Classic | light | hovered, window in the background | `#e5e7eb` | `#6b7280` | 3.90 | `#6d28d9` | 5.73 |
| Classic | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Classic | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Classic | dark | inactive | `#1f2023` | `#6d28d9` | 2.29 | `#a78bfa` | 5.98 |
| Classic | dark | hovered | `#3a3c42` | `#6d28d9` | 1.55 | `#c4b5fd` | 5.97 |
| Classic | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Classic | dark | inactive, window in the background | `#1f2023` | `#9ca3af` | 6.41 | `#a78bfa` | 5.98 |
| Classic | dark | hovered, window in the background | `#3a3c42` | `#9ca3af` | 4.34 | `#c4b5fd` | 5.97 |
| Classic | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Classic | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Sunset | light | inactive | `#f3f4f6` | `#6d28d9` | 6.45 | `#6d28d9` | 6.45 |
| Sunset | light | hovered | `#e5e7eb` | `#6d28d9` | 5.73 | `#6d28d9` | 5.73 |
| Sunset | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Sunset | light | inactive, window in the background | `#f3f4f6` | `#6b7280` | 4.39 | `#6d28d9` | 6.45 |
| Sunset | light | hovered, window in the background | `#e5e7eb` | `#6b7280` | 3.90 | `#6d28d9` | 5.73 |
| Sunset | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Sunset | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Sunset | dark | inactive | `#1f2023` | `#6d28d9` | 2.29 | `#a78bfa` | 5.98 |
| Sunset | dark | hovered | `#3a3c42` | `#6d28d9` | 1.55 | `#c4b5fd` | 5.97 |
| Sunset | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Sunset | dark | inactive, window in the background | `#1f2023` | `#9ca3af` | 6.41 | `#a78bfa` | 5.98 |
| Sunset | dark | hovered, window in the background | `#3a3c42` | `#9ca3af` | 4.34 | `#c4b5fd` | 5.97 |
| Sunset | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Sunset | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Ocean | light | inactive | `#0ea5e9` | `#6d28d9` | 2.56 | `#2e1065` | 5.49 |
| Ocean | light | hovered | `#32b3ec` | `#6d28d9` | 2.13 | `#2e1065` | 6.38 |
| Ocean | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Ocean | light | inactive, window in the background | `#0ea5e9` | `#6b7280` | 1.74 | `#2e1065` | 5.49 |
| Ocean | light | hovered, window in the background | `#32b3ec` | `#6b7280` | 1.58 | `#2e1065` | 6.38 |
| Ocean | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Ocean | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Ocean | dark | inactive | `#0ea5e9` | `#6d28d9` | 2.56 | `#2e1065` | 5.49 |
| Ocean | dark | hovered | `#32b3ec` | `#6d28d9` | 2.13 | `#2e1065` | 6.38 |
| Ocean | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Ocean | dark | inactive, window in the background | `#0ea5e9` | `#9ca3af` | 1.00 | `#2e1065` | 5.49 |
| Ocean | dark | hovered, window in the background | `#32b3ec` | `#9ca3af` | 1.00 | `#2e1065` | 6.38 |
| Ocean | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Ocean | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Glass | light | inactive | `#adc9fb` | `#6d28d9` | 4.23 | `#5b21b6` | 5.35 |
| Glass | light | hovered | `#b9d1fc` | `#6d28d9` | 3.44 | `#5b21b6` | 5.81 |
| Glass | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Glass | light | inactive, window in the background | `#adc9fb` | `#6b7280` | 2.88 | `#5b21b6` | 5.35 |
| Glass | light | hovered, window in the background | `#b9d1fc` | `#6b7280` | 2.51 | `#5b21b6` | 5.81 |
| Glass | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Glass | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Glass | dark | inactive | `#294971` | `#6d28d9` | 1.29 | `#c4b5fd` | 4.97 |
| Glass | dark | hovered | `#233e60` | `#6d28d9` | 1.29 | `#c4b5fd` | 5.89 |
| Glass | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Glass | dark | inactive, window in the background | `#294971` | `#9ca3af` | 3.61 | `#c4b5fd` | 4.97 |
| Glass | dark | hovered, window in the background | `#233e60` | `#9ca3af` | 2.92 | `#c4b5fd` | 5.89 |
| Glass | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Glass | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Mono | light | inactive | `#f3f4f6` | `#6d28d9` | 6.45 | `#6d28d9` | 6.45 |
| Mono | light | hovered | `#e5e7eb` | `#6d28d9` | 5.73 | `#6d28d9` | 5.73 |
| Mono | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Mono | light | inactive, window in the background | `#f3f4f6` | `#6b7280` | 4.39 | `#6d28d9` | 6.45 |
| Mono | light | hovered, window in the background | `#e5e7eb` | `#6b7280` | 3.90 | `#6d28d9` | 5.73 |
| Mono | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Mono | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Mono | dark | inactive | `#1f2023` | `#6d28d9` | 2.29 | `#a78bfa` | 5.98 |
| Mono | dark | hovered | `#3a3c42` | `#6d28d9` | 1.55 | `#c4b5fd` | 5.97 |
| Mono | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Mono | dark | inactive, window in the background | `#1f2023` | `#9ca3af` | 6.41 | `#a78bfa` | 5.98 |
| Mono | dark | hovered, window in the background | `#3a3c42` | `#9ca3af` | 4.34 | `#c4b5fd` | 5.97 |
| Mono | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Mono | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Aurora | light | inactive | `#f3f4f6` | `#6d28d9` | 6.45 | `#6d28d9` | 6.45 |
| Aurora | light | hovered | `#e5e7eb` | `#6d28d9` | 5.73 | `#6d28d9` | 5.73 |
| Aurora | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Aurora | light | inactive, window in the background | `#f3f4f6` | `#6b7280` | 4.39 | `#6d28d9` | 6.45 |
| Aurora | light | hovered, window in the background | `#e5e7eb` | `#6b7280` | 3.90 | `#6d28d9` | 5.73 |
| Aurora | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Aurora | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Aurora | dark | inactive | `#1f2023` | `#6d28d9` | 2.29 | `#a78bfa` | 5.98 |
| Aurora | dark | hovered | `#3a3c42` | `#6d28d9` | 1.55 | `#c4b5fd` | 5.97 |
| Aurora | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Aurora | dark | inactive, window in the background | `#1f2023` | `#9ca3af` | 6.41 | `#a78bfa` | 5.98 |
| Aurora | dark | hovered, window in the background | `#3a3c42` | `#9ca3af` | 4.34 | `#c4b5fd` | 5.97 |
| Aurora | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Aurora | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Midnight | light | inactive | `#1e3a8a` | `#6d28d9` | 1.45 | `#c4b5fd` | 5.61 |
| Midnight | light | hovered | `#1a3175` | `#6d28d9` | 1.41 | `#c4b5fd` | 6.53 |
| Midnight | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Midnight | light | inactive, window in the background | `#1e3a8a` | `#6b7280` | 2.14 | `#c4b5fd` | 5.61 |
| Midnight | light | hovered, window in the background | `#1a3175` | `#6b7280` | 1.94 | `#c4b5fd` | 6.53 |
| Midnight | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Midnight | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Midnight | dark | inactive | `#1e3a8a` | `#6d28d9` | 1.45 | `#c4b5fd` | 5.61 |
| Midnight | dark | hovered | `#1a3175` | `#6d28d9` | 1.41 | `#c4b5fd` | 6.53 |
| Midnight | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Midnight | dark | inactive, window in the background | `#1e3a8a` | `#9ca3af` | 4.07 | `#c4b5fd` | 5.61 |
| Midnight | dark | hovered, window in the background | `#1a3175` | `#9ca3af` | 3.29 | `#c4b5fd` | 6.53 |
| Midnight | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Midnight | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Forest | light | inactive | `#0f766e` | `#6d28d9` | 1.00 | `#ede9fe` | 4.60 |
| Forest | light | hovered | `#0d645e` | `#6d28d9` | 1.00 | `#ede9fe` | 5.89 |
| Forest | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Forest | light | inactive, window in the background | `#0f766e` | `#6b7280` | 1.13 | `#ede9fe` | 4.60 |
| Forest | light | hovered, window in the background | `#0d645e` | `#6b7280` | 1.13 | `#ede9fe` | 5.89 |
| Forest | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Forest | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Forest | dark | inactive | `#0f766e` | `#6d28d9` | 1.00 | `#ede9fe` | 4.60 |
| Forest | dark | hovered | `#0d645e` | `#6d28d9` | 1.00 | `#ede9fe` | 5.89 |
| Forest | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Forest | dark | inactive, window in the background | `#0f766e` | `#9ca3af` | 2.15 | `#ede9fe` | 4.60 |
| Forest | dark | hovered, window in the background | `#0d645e` | `#9ca3af` | 1.93 | `#ede9fe` | 5.89 |
| Forest | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Forest | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Ember | light | inactive | `#c2410c` | `#6d28d9` | 1.00 | `#ffffff` | 5.17 |
| Ember | light | hovered | `#a5370a` | `#6d28d9` | 1.01 | `#ffffff` | 6.66 |
| Ember | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Ember | light | inactive, window in the background | `#c2410c` | `#6b7280` | 1.07 | `#ffffff` | 5.17 |
| Ember | light | hovered, window in the background | `#a5370a` | `#6b7280` | 1.11 | `#ffffff` | 6.66 |
| Ember | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Ember | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Ember | dark | inactive | `#c2410c` | `#6d28d9` | 1.00 | `#ffffff` | 5.17 |
| Ember | dark | hovered | `#a5370a` | `#6d28d9` | 1.01 | `#ffffff` | 6.66 |
| Ember | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Ember | dark | inactive, window in the background | `#c2410c` | `#9ca3af` | 2.03 | `#ffffff` | 5.17 |
| Ember | dark | hovered, window in the background | `#a5370a` | `#9ca3af` | 1.91 | `#ffffff` | 6.66 |
| Ember | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Ember | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Candy | light | inactive | `#f3f4f6` | `#6d28d9` | 6.45 | `#6d28d9` | 6.45 |
| Candy | light | hovered | `#e5e7eb` | `#6d28d9` | 5.73 | `#6d28d9` | 5.73 |
| Candy | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Candy | light | inactive, window in the background | `#f3f4f6` | `#6b7280` | 4.39 | `#6d28d9` | 6.45 |
| Candy | light | hovered, window in the background | `#e5e7eb` | `#6b7280` | 3.90 | `#6d28d9` | 5.73 |
| Candy | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Candy | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Candy | dark | inactive | `#1f2023` | `#6d28d9` | 2.29 | `#a78bfa` | 5.98 |
| Candy | dark | hovered | `#3a3c42` | `#6d28d9` | 1.55 | `#c4b5fd` | 5.97 |
| Candy | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Candy | dark | inactive, window in the background | `#1f2023` | `#9ca3af` | 6.41 | `#a78bfa` | 5.98 |
| Candy | dark | hovered, window in the background | `#3a3c42` | `#9ca3af` | 4.34 | `#c4b5fd` | 5.97 |
| Candy | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Candy | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Frost | light | inactive | `#bae6fd` | `#6d28d9` | 5.35 | `#6d28d9` | 5.35 |
| Frost | light | hovered | `#c4eafd` | `#6d28d9` | 4.26 | `#6d28d9` | 5.59 |
| Frost | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Frost | light | inactive, window in the background | `#bae6fd` | `#6b7280` | 3.64 | `#6d28d9` | 5.35 |
| Frost | light | hovered, window in the background | `#c4eafd` | `#6b7280` | 3.10 | `#6d28d9` | 5.59 |
| Frost | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Frost | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Frost | dark | inactive | `#bae6fd` | `#6d28d9` | 5.35 | `#6d28d9` | 5.35 |
| Frost | dark | hovered | `#c4eafd` | `#6d28d9` | 4.26 | `#6d28d9` | 5.59 |
| Frost | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Frost | dark | inactive, window in the background | `#bae6fd` | `#9ca3af` | 1.91 | `#6d28d9` | 5.35 |
| Frost | dark | hovered, window in the background | `#c4eafd` | `#9ca3af` | 1.75 | `#6d28d9` | 5.59 |
| Frost | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Frost | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Neon | light | inactive | `#170a2e` | `#6d28d9` | 2.64 | `#a78bfa` | 6.89 |
| Neon | light | hovered | `#140927` | `#6d28d9` | 2.38 | `#a78bfa` | 7.02 |
| Neon | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Neon | light | inactive, window in the background | `#170a2e` | `#6b7280` | 3.88 | `#a78bfa` | 6.89 |
| Neon | light | hovered, window in the background | `#140927` | `#6b7280` | 3.38 | `#a78bfa` | 7.02 |
| Neon | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Neon | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Neon | dark | inactive | `#170a2e` | `#6d28d9` | 2.64 | `#a78bfa` | 6.89 |
| Neon | dark | hovered | `#140927` | `#6d28d9` | 2.38 | `#a78bfa` | 7.02 |
| Neon | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Neon | dark | inactive, window in the background | `#170a2e` | `#9ca3af` | 7.38 | `#a78bfa` | 6.89 |
| Neon | dark | hovered, window in the background | `#140927` | `#9ca3af` | 5.92 | `#a78bfa` | 7.02 |
| Neon | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Neon | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Synthwave | light | inactive | `#f3f4f6` | `#6d28d9` | 6.45 | `#6d28d9` | 6.45 |
| Synthwave | light | hovered | `#e5e7eb` | `#6d28d9` | 5.73 | `#6d28d9` | 5.73 |
| Synthwave | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Synthwave | light | inactive, window in the background | `#f3f4f6` | `#6b7280` | 4.39 | `#6d28d9` | 6.45 |
| Synthwave | light | hovered, window in the background | `#e5e7eb` | `#6b7280` | 3.90 | `#6d28d9` | 5.73 |
| Synthwave | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Synthwave | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Synthwave | dark | inactive | `#1f2023` | `#6d28d9` | 2.29 | `#a78bfa` | 5.98 |
| Synthwave | dark | hovered | `#3a3c42` | `#6d28d9` | 1.55 | `#c4b5fd` | 5.97 |
| Synthwave | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Synthwave | dark | inactive, window in the background | `#1f2023` | `#9ca3af` | 6.41 | `#a78bfa` | 5.98 |
| Synthwave | dark | hovered, window in the background | `#3a3c42` | `#9ca3af` | 4.34 | `#c4b5fd` | 5.97 |
| Synthwave | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Synthwave | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Matrix | light | inactive | `#06200e` | `#6d28d9` | 2.42 | `#a78bfa` | 6.32 |
| Matrix | light | hovered | `#051b0c` | `#6d28d9` | 2.29 | `#a78bfa` | 6.60 |
| Matrix | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Matrix | light | inactive, window in the background | `#06200e` | `#6b7280` | 3.55 | `#a78bfa` | 6.32 |
| Matrix | light | hovered, window in the background | `#051b0c` | `#6b7280` | 3.07 | `#a78bfa` | 6.60 |
| Matrix | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Matrix | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Matrix | dark | inactive | `#06200e` | `#6d28d9` | 2.42 | `#a78bfa` | 6.32 |
| Matrix | dark | hovered | `#051b0c` | `#6d28d9` | 2.29 | `#a78bfa` | 6.60 |
| Matrix | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Matrix | dark | inactive, window in the background | `#06200e` | `#9ca3af` | 6.77 | `#a78bfa` | 6.32 |
| Matrix | dark | hovered, window in the background | `#051b0c` | `#9ca3af` | 5.26 | `#a78bfa` | 6.60 |
| Matrix | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Matrix | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Graphite | light | inactive | `#374151` | `#6d28d9` | 1.45 | `#c4b5fd` | 5.58 |
| Graphite | light | hovered | `#2f3745` | `#6d28d9` | 1.43 | `#c4b5fd` | 6.48 |
| Graphite | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Graphite | light | inactive, window in the background | `#374151` | `#6b7280` | 2.13 | `#c4b5fd` | 5.58 |
| Graphite | light | hovered, window in the background | `#2f3745` | `#6b7280` | 1.90 | `#c4b5fd` | 6.48 |
| Graphite | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Graphite | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Graphite | dark | inactive | `#374151` | `#6d28d9` | 1.45 | `#c4b5fd` | 5.58 |
| Graphite | dark | hovered | `#2f3745` | `#6d28d9` | 1.43 | `#c4b5fd` | 6.48 |
| Graphite | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Graphite | dark | inactive, window in the background | `#374151` | `#9ca3af` | 4.05 | `#c4b5fd` | 5.58 |
| Graphite | dark | hovered, window in the background | `#2f3745` | `#9ca3af` | 3.21 | `#c4b5fd` | 6.48 |
| Graphite | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Graphite | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Citrus | light | inactive | `#fb923c` | `#6d28d9` | 3.13 | `#4c1d95` | 4.84 |
| Citrus | light | hovered | `#fca259` | `#6d28d9` | 2.60 | `#4c1d95` | 5.45 |
| Citrus | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Citrus | light | inactive, window in the background | `#fb923c` | `#6b7280` | 2.13 | `#4c1d95` | 4.84 |
| Citrus | light | hovered, window in the background | `#fca259` | `#6b7280` | 1.89 | `#4c1d95` | 5.45 |
| Citrus | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Citrus | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Citrus | dark | inactive | `#fb923c` | `#6d28d9` | 3.13 | `#4c1d95` | 4.84 |
| Citrus | dark | hovered | `#fca259` | `#6d28d9` | 2.60 | `#4c1d95` | 5.45 |
| Citrus | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Citrus | dark | inactive, window in the background | `#fb923c` | `#9ca3af` | 1.12 | `#4c1d95` | 4.84 |
| Citrus | dark | hovered, window in the background | `#fca259` | `#9ca3af` | 1.08 | `#4c1d95` | 5.45 |
| Citrus | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Citrus | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Mint | light | inactive | `#a5f3d3` | `#6d28d9` | 5.53 | `#6d28d9` | 5.53 |
| Mint | light | hovered | `#b4f5d7` | `#6d28d9` | 4.35 | `#6d28d9` | 5.74 |
| Mint | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Mint | light | inactive, window in the background | `#a5f3d3` | `#6b7280` | 3.76 | `#6d28d9` | 5.53 |
| Mint | light | hovered, window in the background | `#b4f5d7` | `#6b7280` | 3.20 | `#6d28d9` | 5.74 |
| Mint | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Mint | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Mint | dark | inactive | `#a5f3d3` | `#6d28d9` | 5.53 | `#6d28d9` | 5.53 |
| Mint | dark | hovered | `#b4f5d7` | `#6d28d9` | 4.35 | `#6d28d9` | 5.74 |
| Mint | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Mint | dark | inactive, window in the background | `#a5f3d3` | `#9ca3af` | 1.97 | `#6d28d9` | 5.53 |
| Mint | dark | hovered, window in the background | `#b4f5d7` | `#9ca3af` | 1.80 | `#6d28d9` | 5.74 |
| Mint | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Mint | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Sakura | light | inactive | `#fbcfe8` | `#6d28d9` | 5.13 | `#6d28d9` | 5.13 |
| Sakura | light | hovered | `#fcd6eb` | `#6d28d9` | 4.12 | `#6d28d9` | 5.39 |
| Sakura | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Sakura | light | inactive, window in the background | `#fbcfe8` | `#6b7280` | 3.49 | `#6d28d9` | 5.13 |
| Sakura | light | hovered, window in the background | `#fcd6eb` | `#6b7280` | 2.98 | `#6d28d9` | 5.39 |
| Sakura | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Sakura | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Sakura | dark | inactive | `#fbcfe8` | `#6d28d9` | 5.13 | `#6d28d9` | 5.13 |
| Sakura | dark | hovered | `#fcd6eb` | `#6d28d9` | 4.12 | `#6d28d9` | 5.39 |
| Sakura | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Sakura | dark | inactive, window in the background | `#fbcfe8` | `#9ca3af` | 1.83 | `#6d28d9` | 5.13 |
| Sakura | dark | hovered, window in the background | `#fcd6eb` | `#9ca3af` | 1.68 | `#6d28d9` | 5.39 |
| Sakura | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Sakura | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Coffee | light | inactive | `#8a5a36` | `#6d28d9` | 1.01 | `#ede9fe` | 4.91 |
| Coffee | light | hovered | `#754d2e` | `#6d28d9` | 1.00 | `#ede9fe` | 6.19 |
| Coffee | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Coffee | light | inactive, window in the background | `#8a5a36` | `#6b7280` | 1.20 | `#ede9fe` | 4.91 |
| Coffee | light | hovered, window in the background | `#754d2e` | `#6b7280` | 1.18 | `#ede9fe` | 6.19 |
| Coffee | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Coffee | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Coffee | dark | inactive | `#8a5a36` | `#6d28d9` | 1.01 | `#ede9fe` | 4.91 |
| Coffee | dark | hovered | `#754d2e` | `#6d28d9` | 1.00 | `#ede9fe` | 6.19 |
| Coffee | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Coffee | dark | inactive, window in the background | `#8a5a36` | `#9ca3af` | 2.29 | `#ede9fe` | 4.91 |
| Coffee | dark | hovered, window in the background | `#754d2e` | `#9ca3af` | 2.02 | `#ede9fe` | 6.19 |
| Coffee | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Coffee | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Royal | light | inactive | `#6b21a8` | `#6d28d9` | 1.22 | `#c4b5fd` | 4.72 |
| Royal | light | hovered | `#5b1c8f` | `#6d28d9` | 1.19 | `#c4b5fd` | 5.66 |
| Royal | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Royal | light | inactive, window in the background | `#6b21a8` | `#6b7280` | 1.80 | `#c4b5fd` | 4.72 |
| Royal | light | hovered, window in the background | `#5b1c8f` | `#6b7280` | 1.72 | `#c4b5fd` | 5.66 |
| Royal | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Royal | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Royal | dark | inactive | `#6b21a8` | `#6d28d9` | 1.22 | `#c4b5fd` | 4.72 |
| Royal | dark | hovered | `#5b1c8f` | `#6d28d9` | 1.19 | `#c4b5fd` | 5.66 |
| Royal | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Royal | dark | inactive, window in the background | `#6b21a8` | `#9ca3af` | 3.43 | `#c4b5fd` | 4.72 |
| Royal | dark | hovered, window in the background | `#5b1c8f` | `#9ca3af` | 2.95 | `#c4b5fd` | 5.66 |
| Royal | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Royal | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |

## Other looks

| Style | Theme | State | Behind the title | 0.15.3 title | Ratio | 0.16.0 title | Ratio |
|---|---|---|---|---|---:|---|---:|
| Purple tab strip (#6d28d9) | light | inactive | `#6d28d9` | `#6d28d9` | 1.00 | `#ddd6fe` | 5.11 |
| Purple tab strip (#6d28d9) | light | hovered | `#e5e7eb` | `#6d28d9` | 5.73 | `#6d28d9` | 5.73 |
| Purple tab strip (#6d28d9) | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Purple tab strip (#6d28d9) | light | inactive, window in the background | `#6d28d9` | `#6b7280` | 1.46 | `#ddd6fe` | 5.11 |
| Purple tab strip (#6d28d9) | light | hovered, window in the background | `#e5e7eb` | `#6b7280` | 3.90 | `#6d28d9` | 5.73 |
| Purple tab strip (#6d28d9) | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Purple tab strip (#6d28d9) | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Purple tab strip (#6d28d9) | dark | inactive | `#6d28d9` | `#6d28d9` | 1.00 | `#ddd6fe` | 5.11 |
| Purple tab strip (#6d28d9) | dark | hovered | `#3a3c42` | `#6d28d9` | 1.55 | `#ddd6fe` | 7.93 |
| Purple tab strip (#6d28d9) | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Purple tab strip (#6d28d9) | dark | inactive, window in the background | `#6d28d9` | `#9ca3af` | 2.79 | `#ddd6fe` | 5.11 |
| Purple tab strip (#6d28d9) | dark | hovered, window in the background | `#3a3c42` | `#9ca3af` | 4.34 | `#ddd6fe` | 7.93 |
| Purple tab strip (#6d28d9) | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Purple tab strip (#6d28d9) | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Dark tab strip (#0b1220) | light | inactive | `#0b1220` | `#6d28d9` | 2.63 | `#a78bfa` | 6.87 |
| Dark tab strip (#0b1220) | light | hovered | `#e5e7eb` | `#6d28d9` | 5.73 | `#6d28d9` | 5.73 |
| Dark tab strip (#0b1220) | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Dark tab strip (#0b1220) | light | inactive, window in the background | `#0b1220` | `#6b7280` | 3.87 | `#a78bfa` | 6.87 |
| Dark tab strip (#0b1220) | light | hovered, window in the background | `#e5e7eb` | `#6b7280` | 3.90 | `#6d28d9` | 5.73 |
| Dark tab strip (#0b1220) | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Dark tab strip (#0b1220) | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Dark tab strip (#0b1220) | dark | inactive | `#0b1220` | `#6d28d9` | 2.63 | `#a78bfa` | 6.87 |
| Dark tab strip (#0b1220) | dark | hovered | `#3a3c42` | `#6d28d9` | 1.55 | `#c4b5fd` | 5.97 |
| Dark tab strip (#0b1220) | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Dark tab strip (#0b1220) | dark | inactive, window in the background | `#0b1220` | `#9ca3af` | 7.37 | `#a78bfa` | 6.87 |
| Dark tab strip (#0b1220) | dark | hovered, window in the background | `#3a3c42` | `#9ca3af` | 4.34 | `#c4b5fd` | 5.97 |
| Dark tab strip (#0b1220) | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Dark tab strip (#0b1220) | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Lavender window (#ede9fe) | light | inactive | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Lavender window (#ede9fe) | light | hovered | `#e5e7eb` | `#6d28d9` | 5.73 | `#6d28d9` | 5.73 |
| Lavender window (#ede9fe) | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Lavender window (#ede9fe) | light | inactive, window in the background | `#ede9fe` | `#6b7280` | 4.07 | `#6d28d9` | 5.98 |
| Lavender window (#ede9fe) | light | hovered, window in the background | `#e5e7eb` | `#6b7280` | 3.90 | `#6d28d9` | 5.73 |
| Lavender window (#ede9fe) | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Lavender window (#ede9fe) | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Lavender window (#ede9fe) | dark | inactive | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Lavender window (#ede9fe) | dark | hovered | `#3a3c42` | `#6d28d9` | 1.55 | `#c4b5fd` | 5.97 |
| Lavender window (#ede9fe) | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Lavender window (#ede9fe) | dark | inactive, window in the background | `#ede9fe` | `#9ca3af` | 2.13 | `#6d28d9` | 5.98 |
| Lavender window (#ede9fe) | dark | hovered, window in the background | `#3a3c42` | `#9ca3af` | 4.34 | `#c4b5fd` | 5.97 |
| Lavender window (#ede9fe) | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Lavender window (#ede9fe) | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Glass, purple accent (#7c3aed) | light | inactive | `#d6b9fb` | `#6d28d9` | 4.12 | `#5b21b6` | 5.21 |
| Glass, purple accent (#7c3aed) | light | hovered | `#dcc4fc` | `#6d28d9` | 3.37 | `#5b21b6` | 5.70 |
| Glass, purple accent (#7c3aed) | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Glass, purple accent (#7c3aed) | light | inactive, window in the background | `#d6b9fb` | `#6b7280` | 2.80 | `#5b21b6` | 5.21 |
| Glass, purple accent (#7c3aed) | light | hovered, window in the background | `#dcc4fc` | `#6b7280` | 2.44 | `#5b21b6` | 5.70 |
| Glass, purple accent (#7c3aed) | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Glass, purple accent (#7c3aed) | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Glass, purple accent (#7c3aed) | dark | inactive | `#5d205d` | `#6d28d9` | 1.61 | `#c4b5fd` | 6.21 |
| Glass, purple accent (#7c3aed) | dark | hovered | `#4f1b4f` | `#6d28d9` | 1.52 | `#c4b5fd` | 7.09 |
| Glass, purple accent (#7c3aed) | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Glass, purple accent (#7c3aed) | dark | inactive, window in the background | `#5d205d` | `#9ca3af` | 4.51 | `#c4b5fd` | 6.21 |
| Glass, purple accent (#7c3aed) | dark | hovered, window in the background | `#4f1b4f` | `#9ca3af` | 3.65 | `#c4b5fd` | 7.09 |
| Glass, purple accent (#7c3aed) | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Glass, purple accent (#7c3aed) | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Solid tabs, glass toolbar, purple accent (#7c3aed) | light | inactive | `#b28ef1` | `#6d28d9` | 2.71 | `#2e1065` | 5.82 |
| Solid tabs, glass toolbar, purple accent (#7c3aed) | light | hovered | `#e5e7eb` | `#6d28d9` | 5.73 | `#2e1065` | 12.30 |
| Solid tabs, glass toolbar, purple accent (#7c3aed) | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Solid tabs, glass toolbar, purple accent (#7c3aed) | light | inactive, window in the background | `#b28ef1` | `#6b7280` | 1.84 | `#2e1065` | 5.82 |
| Solid tabs, glass toolbar, purple accent (#7c3aed) | light | hovered, window in the background | `#e5e7eb` | `#6b7280` | 3.90 | `#2e1065` | 12.30 |
| Solid tabs, glass toolbar, purple accent (#7c3aed) | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Solid tabs, glass toolbar, purple accent (#7c3aed) | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Solid tabs, glass toolbar, purple accent (#7c3aed) | dark | inactive | `#762b75` | `#6d28d9` | 1.24 | `#c4b5fd` | 4.79 |
| Solid tabs, glass toolbar, purple accent (#7c3aed) | dark | hovered | `#3a3c42` | `#6d28d9` | 1.55 | `#c4b5fd` | 5.97 |
| Solid tabs, glass toolbar, purple accent (#7c3aed) | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Solid tabs, glass toolbar, purple accent (#7c3aed) | dark | inactive, window in the background | `#762b75` | `#9ca3af` | 3.48 | `#c4b5fd` | 4.79 |
| Solid tabs, glass toolbar, purple accent (#7c3aed) | dark | hovered, window in the background | `#3a3c42` | `#9ca3af` | 4.34 | `#c4b5fd` | 5.97 |
| Solid tabs, glass toolbar, purple accent (#7c3aed) | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Solid tabs, glass toolbar, purple accent (#7c3aed) | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Midnight, purple accent (#7c3aed) | light | inactive | `#1e3a8a` | `#6d28d9` | 1.45 | `#c4b5fd` | 5.61 |
| Midnight, purple accent (#7c3aed) | light | hovered | `#1a3175` | `#6d28d9` | 1.41 | `#c4b5fd` | 6.53 |
| Midnight, purple accent (#7c3aed) | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Midnight, purple accent (#7c3aed) | light | inactive, window in the background | `#1e3a8a` | `#6b7280` | 2.14 | `#c4b5fd` | 5.61 |
| Midnight, purple accent (#7c3aed) | light | hovered, window in the background | `#1a3175` | `#6b7280` | 1.94 | `#c4b5fd` | 6.53 |
| Midnight, purple accent (#7c3aed) | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Midnight, purple accent (#7c3aed) | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Midnight, purple accent (#7c3aed) | dark | inactive | `#1e3a8a` | `#6d28d9` | 1.45 | `#c4b5fd` | 5.61 |
| Midnight, purple accent (#7c3aed) | dark | hovered | `#1a3175` | `#6d28d9` | 1.41 | `#c4b5fd` | 6.53 |
| Midnight, purple accent (#7c3aed) | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Midnight, purple accent (#7c3aed) | dark | inactive, window in the background | `#1e3a8a` | `#9ca3af` | 4.07 | `#c4b5fd` | 5.61 |
| Midnight, purple accent (#7c3aed) | dark | hovered, window in the background | `#1a3175` | `#9ca3af` | 3.29 | `#c4b5fd` | 6.53 |
| Midnight, purple accent (#7c3aed) | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Midnight, purple accent (#7c3aed) | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
| Gradient from a purple accent (#7c3aed) | light | inactive | `#7c3aed` | `#6d28d9` | 1.24 | `#000000` | 3.68 |
| Gradient from a purple accent (#7c3aed) | light | hovered | `#9058f0` | `#6d28d9` | 1.20 | `#000000` | 4.84 |
| Gradient from a purple accent (#7c3aed) | light | active | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Gradient from a purple accent (#7c3aed) | light | inactive, window in the background | `#7c3aed` | `#6b7280` | 1.01 | `#000000` | 3.68 |
| Gradient from a purple accent (#7c3aed) | light | hovered, window in the background | `#9058f0` | `#6b7280` | 1.02 | `#000000` | 4.84 |
| Gradient from a purple accent (#7c3aed) | light | active, window in the background | `#ede9fe` | `#6d28d9` | 5.98 | `#6d28d9` | 5.98 |
| Gradient from a purple accent (#7c3aed) | light | dragged in from another window | `#ffffff` | `#6d28d9` | 7.10 | `#6d28d9` | 7.10 |
| Gradient from a purple accent (#7c3aed) | dark | inactive | `#7c3aed` | `#6d28d9` | 1.24 | `#000000` | 3.68 |
| Gradient from a purple accent (#7c3aed) | dark | hovered | `#9058f0` | `#6d28d9` | 1.20 | `#000000` | 4.84 |
| Gradient from a purple accent (#7c3aed) | dark | active | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Gradient from a purple accent (#7c3aed) | dark | inactive, window in the background | `#7c3aed` | `#9ca3af` | 1.30 | `#000000` | 3.68 |
| Gradient from a purple accent (#7c3aed) | dark | hovered, window in the background | `#9058f0` | `#9ca3af` | 1.32 | `#000000` | 4.84 |
| Gradient from a purple accent (#7c3aed) | dark | active, window in the background | `#3b2a5c` | `#6d28d9` | 1.76 | `#a78bfa` | 4.61 |
| Gradient from a purple accent (#7c3aed) | dark | dragged in from another window | `#2b2d31` | `#6d28d9` | 1.94 | `#a78bfa` | 5.06 |
