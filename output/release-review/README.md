# Stashi 1.2.1 UI review

These are captures of the actual Flutter widgets with controlled, non-sensitive
test data, not a claim that an unreleased 1.2.1 binary already matches a published
release. The verification screen's successful and unavailable states are fixtures.

- `verify-build-desktop.png`: verification layout and explicit checked-file scope.
- `verify-build-phone.png`: narrow layout.
- `verify-build-unavailable-desktop.png`: network failure, not a tampering verdict.
- `02-update.png`: root-navigator update prompt; version comparison uses an icon
  so it does not depend on a font containing an arrow character.
- `03-desktop-icon.png`: previous/current icon comparison at the same pixel size.

Capture with `PIRATE_UI_CAPTURE_DIR` pointing here and
`PIRATE_MATERIAL_ICONS_FONT` pointing to Flutter's materialicons-regular.otf:

```text
flutter test --no-pub --update-goldens test/desktop_update_review_test.dart test/features/settings/verify_build_screen_test.dart
```

Validation: 38 focused Flutter tests passed, changed Dart files analyzed cleanly,
15 release policy/version tests passed, and the component scan/report test passed.
Inno Setup 6.7.3 successfully compiled a fixture installer and its log showed only
the wallet fixture embedded; all three privacy helper entries were external hash-
checked downloads. No fixture installer was installed. Windows UI capture could
not run because the computer-use helper failed to launch its app-server.

Native Android/iOS builds and the final CI-signed Windows release have not been
run here. The original other-device PGP failure was not reproduced with public
v1.1.9/v1.2.0 bundles; both validated locally. Final vendor detection changes and
native installation checks require the release artifacts.
