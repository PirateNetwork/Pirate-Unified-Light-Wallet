# Translation Workflow

The Flutter app loads translations at runtime from `app/assets/i18n/`. The
English catalog is `app/assets/i18n/app_en.arb`.

## What to translate

Translate user-facing language, including:

- screen titles, labels, buttons, hints, tooltips, and empty states
- dialogs, errors, validation messages, and progress/status text
- accessibility labels
- text included in shared or exported user summaries
- terms, privacy, and security guidance

Do not translate:

- ARB keys or placeholder names such as `{ticker}`, `{error}`, or `{walletName}`
- product and protocol names such as `Pirate Wallet`, `Pirate Unified Wallet`, `ARRR`, `LTC`, `vARRR`, `KDF`, `Snowflake`, `obfs4`, `Face ID`, and `Bitcoin`
- addresses, hashes, transaction IDs, URLs, paths, filenames, extensions, and numeric examples
- internal error codes, JSON/RPC field names, log messages, and error-matching fragments
- the bundled MIT License text

When a fixed name appears inside a sentence, translate the sentence but leave the name unchanged.

## Add a language

1. Copy `app/assets/i18n/app_en.arb` to `app/assets/i18n/app_<locale>.arb`.
2. Translate values only. The English source sentence is also the key and must not change.
3. Preserve every placeholder exactly. Translators may move placeholders to fit the target language.
4. Preserve newlines and intended punctuation.
5. Add the locale to `AppLocalePreference` in `app/lib/features/settings/providers/preferences_providers.dart`.
6. Expose it in `app/lib/features/settings/screens/language_screen.dart`.

Examples are `app_fr.arb` for French and `app_es_MX.arb` for Mexican Spanish.

Language names in the selector should use their native form (for example, `Deutsch` rather than a translation of "German").

## Refresh English catalogs

After production UI text changes, run from `app/`:

```bash
dart run tool/audit_runtime_translations.dart --write
```

The audit command rebuilds the English runtime ARB from production source. It
removes stale entries and adds newly localized text deterministically.

Do not use `--write` to update a translated locale. Locale files require human translation.

## Validate

Run from `app/`:

```bash
dart run tool/audit_runtime_translations.dart
flutter analyze --fatal-infos
flutter test --no-test-assets
```

The translation audit fails for:

- production text missing from the English runtime ARB
- stale runtime ARB entries
- dynamic translation keys that cannot be audited
- direct or interpolated UI text without localization
- translated runtime locales with missing/stale keys or changed placeholders

Finally, inspect desktop and mobile layouts at large text scale, especially settings, onboarding, send, swap, update dialogs, and long error messages.
