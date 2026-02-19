# Translation Workflow

This project uses Flutter ARB localization files in `app/lib/l10n/`.

## Files translators need

- Source English file: `app/lib/l10n/app_en.arb`
- Optional tabular handoff: `docs/localization/translation_handoff_en.csv`

## Add a new language

1. Copy `app/lib/l10n/app_en.arb` to `app/lib/l10n/app_<locale>.arb`.
2. Translate only the values. Do not change keys.
3. Keep placeholders exactly as-is (for example `{version}` and `{published}`).
4. Keep escaped characters (for example `\n`) intact.
5. Commit the new ARB file.

Examples:

- French: `app/lib/l10n/app_fr.arb`
- Spanish (Mexico): `app/lib/l10n/app_es_MX.arb`

## Generate localization code

From `app/`:

```bash
flutter gen-l10n
```

`lib/l10n/app_localizations.dart` is generated from the ARB files.

## Enable language in Settings

When a new ARB is added, expose it in:

- `app/lib/features/settings/providers/preferences_providers.dart`
  - `AppLocalePreference`
- `app/lib/features/settings/screens/language_screen.dart`
  - the language list UI

## Validation checklist

1. `flutter gen-l10n`
2. `flutter analyze`
3. Run desktop and mobile layouts, especially:
   - Settings screens
   - Update dialog
   - Buttons with long translated text
4. Confirm no text clipping/overflow on small screens.
