import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pirate_wallet/features/settings/verify_build_screen.dart';

void main() {
  group('Verify Build Screen Tests', () {
    testWidgets('Shows build information', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: VerifyBuildScreen(),
          ),
        ),
      );

      // Wait for loading
      await tester.pumpAndSettle();

      // Should show header
      expect(find.text('Reproducible Builds'), findsOneWidget);
      expect(find.byIcon(Icons.verified_user), findsOneWidget);
      
      // Should show build info
      expect(find.text('Build Information'), findsOneWidget);
      expect(find.text('Version'), findsOneWidget);
      expect(find.text('Git Commit'), findsOneWidget);
      expect(find.text('Build Date'), findsOneWidget);
      expect(find.text('Rust Version'), findsOneWidget);
      expect(find.text('Target'), findsOneWidget);
    });

    testWidgets('Shows verification steps', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: VerifyBuildScreen(),
          ),
        ),
      );

      await tester.pumpAndSettle();

      // Should show all 5 steps
      expect(find.text('Verification Steps'), findsOneWidget);
      expect(find.text('Download Release + Checksums'), findsOneWidget);
      expect(find.text('Verify Checksum'), findsOneWidget);
      expect(find.text('Reproduce with Nix Flake'), findsOneWidget);
      expect(find.text('Generate SBOM (Optional)'), findsOneWidget);
      expect(find.text('Generate Provenance (Optional)'), findsOneWidget);
    });

    testWidgets('Shows code snippets with copy buttons', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: VerifyBuildScreen(),
          ),
        ),
      );

      await tester.pumpAndSettle();

      // Should show code snippets
      expect(find.textContaining('curl -L -O'), findsWidgets);
      expect(find.textContaining('nix build'), findsWidgets);
      
      // Should have copy buttons
      expect(find.byIcon(Icons.copy), findsWidgets);
    });

    testWidgets('Can copy code snippets to clipboard', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: VerifyBuildScreen(),
          ),
        ),
      );

      await tester.pumpAndSettle();

      // Tap first copy button
      final copyButtons = find.byIcon(Icons.copy);
      await tester.tap(copyButtons.first);
      await tester.pumpAndSettle();

      // Should show success message
      expect(find.text('Copied to clipboard'), findsOneWidget);
    });

    testWidgets('Shows resource links', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: VerifyBuildScreen(),
          ),
        ),
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
    });

    testWidgets('Step numbers are displayed correctly', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: VerifyBuildScreen(),
          ),
        ),
      );

      await tester.pumpAndSettle();

      // Should show numbered steps
      expect(find.text('1'), findsOneWidget);
      expect(find.text('2'), findsOneWidget);
      expect(find.text('3'), findsOneWidget);
      expect(find.text('4'), findsOneWidget);
      expect(find.text('5'), findsOneWidget);
    });

    testWidgets('Shows platform-specific build commands', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: VerifyBuildScreen(),
          ),
        ),
      );

      await tester.pumpAndSettle();

      // Should show build command for current platform
      // The exact command depends on the platform
      expect(find.textContaining('nix build'), findsWidgets);
    });

    testWidgets('Can copy git commit hash', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: VerifyBuildScreen(),
          ),
        ),
      );

      await tester.pumpAndSettle();

      // Find the copy button next to git commit
      // (It should be the first icon button in the build info card)
      final copyButton = find.byIcon(Icons.copy).first;
      await tester.tap(copyButton);
      await tester.pumpAndSettle();

      // Should show success message
      expect(find.text('Copied to clipboard'), findsOneWidget);
    });
  });
}

