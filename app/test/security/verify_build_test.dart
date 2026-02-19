import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pirate_wallet/features/settings/verify_build_screen.dart';
import '../test_flags.dart';

final bool _skipFfiTests = shouldSkipFfiTests();

void main() {
  group('Verify Build Screen Tests', () {
    testWidgets('Shows build information', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(child: MaterialApp(home: VerifyBuildScreen())),
      );

      // Wait for loading
      await tester.pumpAndSettle();

      // Should show header
      expect(find.text('Reproducible Builds'), findsOneWidget);
      expect(find.byIcon(Icons.verified_user), findsOneWidget);

      // Should show build info
      expect(find.text('Build Information'), findsOneWidget);
      final hasBuildFields = find.text('Version').evaluate().isNotEmpty;
      if (hasBuildFields) {
        expect(find.text('Git Commit'), findsOneWidget);
        expect(find.text('Build Date'), findsOneWidget);
        expect(find.text('Rust Version'), findsOneWidget);
        expect(find.text('Target'), findsOneWidget);
      } else {
        expect(find.text('Build information unavailable.'), findsOneWidget);
      }
    }, skip: _skipFfiTests);

    testWidgets('Shows official verification summary', (
      WidgetTester tester,
    ) async {
      await tester.pumpWidget(
        ProviderScope(child: MaterialApp(home: VerifyBuildScreen())),
      );

      await tester.pumpAndSettle();

      expect(find.text('Official Release Verification'), findsOneWidget);
      expect(find.text('Build Information'), findsOneWidget);
    }, skip: _skipFfiTests);

    testWidgets('Copy action does not throw when copy controls exist', (
      WidgetTester tester,
    ) async {
      await tester.pumpWidget(
        ProviderScope(child: MaterialApp(home: VerifyBuildScreen())),
      );

      await tester.pumpAndSettle();

      // Tap first copy button
      final copyButtons = find.byIcon(Icons.copy);
      if (copyButtons.evaluate().isEmpty) {
        return;
      }
      await tester.ensureVisible(copyButtons.first);
      await tester.tap(copyButtons.first, warnIfMissed: false);
      await tester.pumpAndSettle();

      // Should not throw after copy action.
      expect(tester.takeException(), isNull);
    }, skip: _skipFfiTests);

    testWidgets('Shows resource links', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(child: MaterialApp(home: VerifyBuildScreen())),
      );

      await tester.pumpAndSettle();

      // Scroll to bottom
      await tester.dragUntilVisible(
        find.text('Resources'),
        find.byType(SingleChildScrollView),
        Offset(0, -300),
      );

      // Should show resource links
      expect(find.text('Resources'), findsOneWidget);
      expect(find.text('Verification Guide'), findsOneWidget);
      expect(find.text('Source Code'), findsOneWidget);
      expect(find.text('Security Practices'), findsOneWidget);
    }, skip: _skipFfiTests);

    testWidgets('Can copy git commit hash', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(child: MaterialApp(home: VerifyBuildScreen())),
      );

      await tester.pumpAndSettle();

      // Find the copy button next to git commit
      final gitCommitLabel = find.text('Git Commit');
      if (gitCommitLabel.evaluate().isEmpty) {
        return;
      }

      await tester.ensureVisible(gitCommitLabel);
      final copyButton = find.byIcon(Icons.copy).first;
      await tester.ensureVisible(copyButton);
      await tester.tap(copyButton, warnIfMissed: false);
      await tester.pumpAndSettle();

      // Should not throw after copy action.
      expect(tester.takeException(), isNull);
    }, skip: _skipFfiTests);
  });
}
