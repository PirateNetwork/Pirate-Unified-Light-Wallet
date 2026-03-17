# UI Structure and Design Tokens

This document describes the current UI structure in the Flutter application. It is a reference for contributors who need to work with the shared theme, tokens, and reusable components.

Design system layout
--------------------

The current design system code is organized as follows:

```text
lib/
|-- design/
|   |-- compat.dart
|   |-- deep_space_theme.dart
|   |-- theme.dart
|   `-- tokens/
|       |-- colors.dart
|       |-- spacing.dart
|       `-- typography.dart
|-- ui/
|   |-- atoms/
|   |-- molecules/
|   |-- organisms/
|   `-- motion/
`-- features/showcase/
    `-- screens/
```

Tokens
------

Shared tokens live under `lib/design/tokens/`:

- `colors.dart`
  - shared color constants and gradients
- `spacing.dart`
  - spacing, insets, and radius values
- `typography.dart`
  - text styles and shared font configuration

Theme entry points
------------------

The main theme files are:

- `lib/design/theme.dart`
- `lib/design/deep_space_theme.dart`
- `lib/design/compat.dart`

When adding or changing application-wide styling, start in those files before editing individual widgets.

Reusable UI layers
------------------

Atoms in `lib/ui/atoms/` provide low-level controls such as:

- buttons
- icon buttons
- text buttons
- inputs
- toggles
- badges and tags

Molecules in `lib/ui/molecules/` combine atoms into reusable application widgets such as:

- cards
- dialogs
- list tiles
- form sections
- connection and privacy indicators
- transaction rows
- wallet switcher components

Organisms in `lib/ui/organisms/` provide larger layout and application shell pieces such as:

- app bar
- scaffold
- desktop window wrapper
- navigation
- hero header
- sync indicator
- skeleton loaders

Motion helpers
--------------

Motion helpers live under `lib/ui/motion/`:

- `curves.dart`
- `durations.dart`
- `micro_interactions.dart`
- `transitions.dart`

These files should be used for shared timing and transition behavior instead of hard-coding animation constants in feature code.

Showcase screens
----------------

The showcase under `lib/features/showcase/screens/` is the fastest way to inspect reusable UI elements while developing:

- `showcase_home_screen.dart`
- `showcase_buttons_screen.dart`
- `showcase_forms_screen.dart`
- `showcase_cards_screen.dart`
- `showcase_dialogs_screen.dart`
- `showcase_animations_screen.dart`

Contributor guidelines
----------------------

- Add shared styling to the token and theme layers before adding one-off values to feature widgets.
- Prefer existing atoms, molecules, and organisms over new bespoke components when the behavior already exists.
- Keep generated files and platform runner files out of design-system changes unless the change truly depends on them.
- When adding a new reusable widget, place it in the lowest layer that matches its responsibility.
- If a change affects multiple features, add or update a showcase example.
