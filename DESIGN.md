# Frater design system

## Direction

The interface is dark and plain. Each page carries one task and the controls that task needs.

- Prefer hierarchy, spacing, and surface contrast over decoration.
- Use one chromatic accent for primary actions and focus states.
- Keep pages sparse and task-focused.
- Do not use gradients, animations, or CSS transitions.
- Do not use decorative logos, atmospheric effects, or shadows of any size.
- Use `Frater` for the product name, always capitalized.

The implementation source of truth is `src/web/views/assets/styles.css`.

## Assets

- Inter Variable is vendored locally and is the primary UI typeface.
- Do not introduce externally hosted fonts, stylesheets, scripts, or images.

## Color

The interface is dark-only.

| Token | Value | Use |
| --- | --- | --- |
| `--canvas` | `#08090b` | Page background |
| `--surface-1` | `#0e1013` | Cards and the site header |
| `--surface-2` | `#14171b` | Inputs, lists, and secondary controls |
| `--surface-3` | `#1b1f24` | Hovered secondary controls |
| `--hairline` | `#23272d` | Card borders and row rules |
| `--hairline-strong` | `#30353d` | Input and control borders |
| `--hairline-hover` | `#414751` | Hovered borders |
| `--ink` | `#f4f6f8` | Primary text |
| `--ink-secondary` | `#c3cad4` | Secondary text |
| `--muted` | `#79828f` | Supporting text, labels, and metadata |
| `--primary` | `#5e6ad2` | Primary controls and focus |
| `--primary-hover` | `#6975dc` | Primary hover state |
| `--primary-pressed` | `#4f59b8` | Primary pressed state |
| `--primary-ink` | `#ffffff` | Text on primary surfaces |
| `--primary-faint` | `rgba(94, 106, 210, .12)` | Accent-tinted fills such as the device code |
| `--focus-ring` | `rgba(130, 143, 255, .6)` | Every `:focus-visible` outline |
| `--danger-ink` | `#ef6b73` | Error text and the error rule |
| `--danger-line` | `#4d2429` | Error panel border |
| `--danger-surface` | `#1a1012` | Error panel background |

Every color in the stylesheet comes from this table. Do not write a literal color in a rule. Do not add another brand accent. A new color needs a semantic purpose and WCAG AA contrast against its background.

## Typography

- Family: `Inter Variable`, followed by system sans-serif fallbacks.
- Home hero: `44px`, weight `600`, `-.02em` letter spacing, line height `1.1`.
- Page title: `22px`, weight `600`, `-.015em` letter spacing. Names a wide page such as the account dashboard.
- Card heading: `24px`, weight `600`, `-.015em` letter spacing, line height `1.2`.
- Navigation title: `17px`, weight `600`, `.02em` letter spacing.
- Section heading: `15px`, weight `600`, line height `1.3`.
- Body: `15px`, weight `400`, line height `1.5`.
- Buttons: `13px`, weight `550`. The variable font carries the intermediate weight.
- Inputs: `14px`, weight `400`.
- Compact controls: `12px`, weight `550`.
- Metadata and code: `12px`.
- Micro-label: `11px`, weight `600`, `.08em` letter spacing, uppercase, `--muted`. One shared rule styles every `label`, `legend`, caption, and detail label.

This scale is closed. A new size needs a new role in this list, not a one-off value in a rule.

Use sentence case. Prefer short sentences.

## Spacing and shape

Spacing follows a 4px base unit:

| Token | Value |
| --- | --- |
| `--space-1` | `4px` |
| `--space-2` | `8px` |
| `--space-3` | `12px` |
| `--space-4` | `16px` |
| `--space-5` | `20px` |
| `--space-6` | `24px` |
| `--space-8` | `32px` |
| `--space-12` | `48px` |

Sizes are tokens too:

| Token | Value | Use |
| --- | --- | --- |
| `--control-height` | `36px` | Inputs and form buttons |
| `--control-height-sm` | `30px` | Compact and navigation controls |
| `--card-width` | `32rem` | Focused cards |

- Nothing is rounded. Inputs and buttons declare `border-radius: 0` and no other rule reintroduces a radius.
- Nothing casts a shadow. Elevation is a surface color plus a 1px hairline border.
- Every focus ring is a square 2px `--focus-ring` outline at a fixed offset.

## Navigation

- The header is a `56px` minimum-height bar on `--surface-1` with a `--hairline` bottom border.
- The product title is left-aligned at `17px`, weight `600`.
- Page-level actions are right-aligned when present, at compact control height.
- Both sides are vertically centered in the same flex row.
- Avoid duplicating navigation actions in the page body.
- Keep navigation compact and limited to high-value destinations and actions.

## Cards and focused forms

- Focused cards use a maximum width of `32rem`.
- Cards sit on `--surface-1` with a 1px `--hairline` border and `16px` padding, held in a local `--card-pad` custom property.
- Use a clear heading, one short supporting sentence, and only the controls required for the task.
- Inputs and primary form buttons share a `36px` minimum height.
- Keep one `--space-2` step between the final field and the primary action.
- Errors use a bounded semantic panel with `role="alert"`.

## Forms

- Labels appear above inputs in the micro-label style.
- Inputs use `6px 10px` padding, `--surface-2` background, and a `--hairline-strong` border.
- Hover raises the border to `--hairline-hover`. Focus turns the border `--primary` and sets the outline flush with `outline-offset: 0`.
- Use the correct input type, autocomplete metadata, capitalization, and spellcheck behavior for the expected value.
- Do not rely on placeholder text as a label.

## Buttons

### Primary

- Accent background with white text.
- `36px` minimum height, `0 16px` padding, `13px` at weight `550`.
- Hover and pressed states shift along the accent ramp, so the white text keeps WCAG AA contrast.

### Secondary

- Raised dark surface with secondary text and a strong hairline border.
- Hover increases surface and border contrast.
- Use for cancellation and other subordinate actions.

### Compact

- `30px` minimum height, `0 12px` padding, `12px` font.
- Use beside a value or inside a list row.

## Messages

- A note is a `--surface-2` panel with a `--hairline-strong` border and a 2px `--primary` left rule.
- An error is the same panel in the danger triad: `--danger-ink` text, `--danger-line` border, `--danger-surface` background, and a 2px `--danger-ink` left rule.
- Both are `13px` and square-cornered.

## Wide pages

The account page and the dashboard share the wide shell.

- A wide page uses `width: min(100% - 32px, 72rem)` instead of `--card-width`, with a page title in a `page-head` row above the content.
- The account grid is `grid-template-columns: minmax(0, 3fr) minmax(0, 2fr)`: tables on the wide left, forms on the narrow right. Each column is its own flex stack with a `--space-4` gap, so a tall card in one column does not stretch the cards in the other.
- The grid collapses to one column at `60rem`, well before the `30rem` phone tweaks.

## Tables

- Tabular data uses a real `table` at `13px` inside a square `--hairline` panel with an `overflow-x: auto` wrapper, so a narrow screen scrolls the table instead of collapsing its columns.
- Headers use the `11px` micro-label style. Rows separate with a single `--hairline` rule on `tr + tr`, not per-cell borders.
- Body cells reserve `--control-height-sm` of height, so a row with compact action buttons is no taller than one without.
- A table that ends a card bleeds to the card edge with negative margins from the card's local `--card-pad`, dropping its side and bottom borders so the card border is the only hairline there.
- The action column takes `width: 1%` with `white-space: nowrap`, so the flexible columns give up width before the buttons do.
- Numeric and date columns are right-aligned with `tabular-nums`; timestamps carry their zone in the column header and never wrap.
- An empty state is one full sentence with a specific noun and a terminal period, in `--muted`, as a single cell spanning the table. Example: `No session is active.`
- Values are humanized server-side: group digits with commas, and shorten large counts past four digits.

## Data pages

The dashboard is the first data page. It uses the wide shell, a row of stat tiles, a time-range switch, and two even two-column grids of table cards.

- A stat tile is a micro-label, a value at `32px`, weight `600`, `tabular-nums`, and an optional one-line note in `--muted`. The note keeps a fixed `min-height`, so tiles with and without a note stay the same height. Tiles sit in a three-column grid that collapses with the account grid at `60rem`.
- A time-range switch is a row of links, not a form, so the range lives in the URL. The links join into one segmented control on `--surface-2` inside a `--hairline-strong` border, separated by `--hairline` rules, at compact control height. The current range uses `--primary` with `--primary-ink` text and `aria-current="page"`.
- Cards of equal weight use the even variant of the account grid, `repeat(2, minmax(0, 1fr))`, with the same `60rem` collapse.

## Detail and confirmation views

- Present important labels and values in a bordered secondary surface.
- Display a short code the user reads aloud or types in `--mono` at `26px`, weight `500`, `.22em` letter spacing, centered, on `--primary-faint` with a 1px `--primary` border. Below `34rem` it drops to `20px` with `.16em` spacing.
- Let the code panel scroll horizontally rather than wrap. Let a long identifier such as a client id or a redirect URI break with `overflow-wrap: anywhere`.
- The consent avatar is a `32px` square with a `--primary` border, `--primary-faint` fill, and `--primary-hover` text.
- Place secondary and primary actions in two columns on larger screens and one column on narrow screens.
- Make destructive, permission-granting, and irreversible actions explicit.
- Never trade required context or warnings for visual simplicity.

## Accessibility and security

- Interactive form controls must be at least `36px` high; compact and navigation controls may be `30px` high.
- All interactive elements require a visible `:focus-visible` outline.
- Text and control states must meet WCAG AA contrast requirements.
- Pages must work at a minimum width of `320px`.
- Sensitive pages remain protected against framing and use a restrictive Content Security Policy.
- Do not weaken CSP to accommodate browser extensions or injected content.
- Do not add inline styles.
- Do not write a raw color, size, or spacing value where a token exists.
- Do not add scripts. The pages run no script at all.

## Responsive behavior

At widths up to `34rem`, the device code drops to its narrow size.

At widths up to `30rem`:

- Reduce the top padding of the page shells.
- Stack detail labels and values.
- Stack paired actions vertically.
- Preserve control heights and focus visibility.
