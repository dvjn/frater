# frater design system

## Direction

The interface is dark and plain. Each page carries one task and the controls that task needs.

- Prefer hierarchy, spacing, and surface contrast over decoration.
- Use one chromatic accent for primary actions and focus states.
- Keep pages sparse and task-focused.
- Do not use gradients, animations, or CSS transitions.
- Do not use decorative logos, atmospheric effects, or oversized shadows.
- Use `frater` for the product name, always lowercase, also at the start of a sentence.

The implementation source of truth is `src/web/views/assets/styles.css`.

## Assets

- Inter Variable is vendored locally and is the primary UI typeface.
- Do not introduce externally hosted fonts, stylesheets, scripts, or images.

## Color

The interface is dark-only.

| Token | Value | Use |
| --- | --- | --- |
| `--canvas` | `#010102` | Page background |
| `--surface-1` | `#121315` | Cards |
| `--surface-2` | `#181a1d` | Inputs and secondary controls |
| `--surface-3` | `#1e2024` | Hovered secondary controls |
| `--hairline` | `#292b30` | Card borders |
| `--hairline-strong` | `#383b42` | Input and control borders |
| `--hairline-hover` | `#454950` | Hovered borders |
| `--ink` | `#f7f8f8` | Primary text |
| `--ink-secondary` | `#d0d6e0` | Labels and secondary text |
| `--muted` | `#8a8f98` | Supporting text and metadata |
| `--primary` | `#5e6ad2` | Primary controls and focus |
| `--primary-hover` | `#5661c5` | Primary hover state |
| `--primary-pressed` | `#4f59b8` | Primary pressed state |
| `--danger` | `#e06c75` | Errors only |

Do not add another brand accent. A new color needs a semantic purpose and WCAG AA contrast against its background.

## Typography

- Family: `Inter Variable`, followed by system sans-serif fallbacks.
- Body: `16px`, weight `400`, line height `1.5`.
- Page heading: `28px`, weight `600`, line height `1.2`.
- Navigation title: `28px`, weight `650`, line height `1.2`.
- Labels and standard buttons: `14px`, weight `500`.
- Compact navigation actions: `13px`, weight `500`.
- Metadata and code: `12px` to `14px`.

Use sentence case. Prefer short sentences.

## Spacing and shape

Spacing follows a 4px base unit:

| Token | Value |
| --- | --- |
| `--space-1` | `4px` |
| `--space-2` | `8px` |
| `--space-3` | `12px` |
| `--space-4` | `16px` |
| `--space-6` | `24px` |
| `--space-8` | `32px` |
| `--space-12` | `48px` |

- Standard controls use an `8px` radius.
- Cards use a `12px` radius.
- Avoid pill-shaped primary actions.
- Cards use a surface and 1px hairline border rather than shadows.

## Navigation

- The product title is left-aligned and prominent.
- Page-level actions are right-aligned when present.
- Both sides are vertically centered in the same flex row.
- Avoid duplicating navigation actions in the page body.
- Keep navigation compact and limited to high-value destinations and actions.

## Cards and focused forms

- Focused cards use a maximum width of `30rem`.
- Desktop padding: `24px`; compact-screen padding: `16px`.
- Use a clear heading, one short supporting sentence, and only the controls required for the task.
- Inputs and primary form buttons share a `44px` minimum height.
- Keep `24px` between the final field and the primary action.
- Errors use a bounded semantic panel with `role="alert"`.

## Forms

- Labels appear above inputs.
- Inputs use `8px 12px` padding.
- Focus uses a visible 2px accent outline in addition to border color.
- Use the correct input type, autocomplete metadata, capitalization, and spellcheck behavior for the expected value.
- Do not rely on placeholder text as a label.

## Buttons

### Primary

- Accent background with white text.
- `44px` minimum height in forms.
- `8px 14px` padding and `8px` radius.
- Hover and pressed states become darker, so the white text keeps WCAG AA contrast.

### Secondary

- Raised dark surface with secondary text and a strong hairline border.
- Hover increases surface and border contrast.
- Use for cancellation and other subordinate actions.

## Detail and confirmation views

- Present important labels and values in a bordered secondary surface.
- Display a short code the user reads aloud or types in `--mono`, centered, weight `650`, `clamp(22px, 7vw, 30px)`, with `.08em` letter spacing.
- Keep a grouped code on one line with `white-space: nowrap`. Let a long identifier such as a client id or a redirect URI break with `overflow-wrap: anywhere`.
- Place secondary and primary actions in two columns on larger screens and one column on narrow screens.
- Make destructive, permission-granting, and irreversible actions explicit.
- Never trade required context or warnings for visual simplicity.

## Accessibility and security

- Interactive form controls must be at least `44px` high; compact navigation controls may be `34px` high.
- All interactive elements require a visible `:focus-visible` outline.
- Text and control states must meet WCAG AA contrast requirements.
- Pages must work at a minimum width of `320px`.
- Sensitive pages remain protected against framing and use a restrictive Content Security Policy.
- Do not weaken CSP to accommodate browser extensions or injected content.
- Do not add inline styles.
- Do not add scripts. The pages run no script at all.

## Responsive behavior

At widths up to `30rem`:

- Reduce card padding to `16px`.
- Stack detail labels and values.
- Stack paired actions vertically.
- Preserve control heights and focus visibility.
