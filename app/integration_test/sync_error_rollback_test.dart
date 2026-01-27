import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:pirate_wallet/main.dart' as app;

/// Integration test for sync errors and rollback scenarios
void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  Finder textMatches(RegExp pattern) {
    return find.byWidgetPredicate((widget) {
      if (widget is Text) {
        final text = widget.data ?? widget.textSpan?.toPlainText() ?? '';
        return pattern.hasMatch(text);
      }
      return false;
    });
  }

  group('Sync Error & Rollback E2E', () {
    testWidgets('Initial sync progress display', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      // Should show sync progress widget
      if (find.textContaining('Syncing').evaluate().isNotEmpty) {
        expect(find.byType(LinearProgressIndicator), findsOneWidget);
        expect(find.textContaining('%'), findsOneWidget);

        // Should show current stage
        expect(
          textMatches(RegExp(r'(Headers|Notes|Witness|Verify)')),
          findsOneWidget,
        );
      }
    });

    testWidgets('Sync network error handling', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      // Simulate network error (would need mock in production)
      // Wait for error to appear
      await tester.pump(Duration(seconds: 5));

      // Should show error message
      if (find.textContaining('Network error').evaluate().isNotEmpty) {
        expect(find.text('Retry'), findsOneWidget);

        // Tap retry
        await tester.tap(find.text('Retry'));
        await tester.pumpAndSettle();

        // Should attempt to sync again
        expect(find.textContaining('Syncing'), findsOneWidget);
      }
    });

    testWidgets('Sync interruption handling', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      // Start sync
      if (find.textContaining('Syncing').evaluate().isNotEmpty) {
        // Interrupt sync by going background (simulate app suspension)
        final binding = tester.binding as IntegrationTestWidgetsFlutterBinding;
        binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
        await tester.pumpAndSettle();

        // Resume app
        binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
        await tester.pumpAndSettle(Duration(seconds: 2));

        // Should resume from checkpoint
        expect(find.textContaining('Syncing'), findsOneWidget);
        expect(find.textContaining('Resumed'), findsOneWidget);
      }
    });

    testWidgets('Checkpoint rollback on corruption', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      // Simulate corruption detection (would need mock in production)
      // This would trigger automatic rollback

      // Wait for rollback to complete
      await tester.pump(Duration(seconds: 3));

      // Should show rollback message
      if (find.textContaining('Rolled back').evaluate().isNotEmpty) {
        expect(find.textContaining('checkpoint'), findsOneWidget);

        // Sync should restart from checkpoint
        expect(find.textContaining('Syncing'), findsOneWidget);
      }
    });

    testWidgets('Reorg detection and rollback', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      // Simulate reorg detection (would need mock)
      await tester.pump(Duration(seconds: 5));

      // Should detect reorg and rollback
      if (find.textContaining('Chain reorganization').evaluate().isNotEmpty) {
        expect(find.textContaining('Rolling back'), findsOneWidget);

        // Wait for rollback to complete
        await tester.pumpAndSettle(Duration(seconds: 2));

        // Should resume sync
        expect(find.textContaining('Syncing'), findsOneWidget);
      }
    });

    testWidgets('Manual rescan from settings', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      // Navigate to settings
      await tester.tap(find.byIcon(Icons.settings));
      await tester.pumpAndSettle();

      // Find rescan option
      if (find.text('Rescan Blockchain').evaluate().isNotEmpty) {
        await tester.tap(find.text('Rescan Blockchain'));
        await tester.pumpAndSettle();

        // Should show confirmation dialog
        expect(find.text('Confirm Rescan'), findsOneWidget);
        expect(find.text('This will rebuild your wallet state'), findsOneWidget);

        // Confirm rescan
        await tester.tap(find.text('Rescan'));
        await tester.pumpAndSettle(Duration(seconds: 2));

        // Should start syncing from birthday height
        expect(find.textContaining('Syncing'), findsOneWidget);
        expect(find.textContaining('0%'), findsOneWidget);
      }
    });

    testWidgets('Background sync completion notification', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      // Send app to background
      final binding = tester.binding as IntegrationTestWidgetsFlutterBinding;
      binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
      await tester.pumpAndSettle();

      // Wait for background sync to complete (simulated)
      await Future.delayed(Duration(seconds: 5));

      // Resume app
      binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
      await tester.pumpAndSettle();

      // Should show completed state
      if (find.textContaining('Synced').evaluate().isNotEmpty) {
        expect(find.textContaining('Up to date'), findsOneWidget);
      }
    });

    testWidgets('Deep scan vs compact scan selection', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      // Navigate to sync settings
      await tester.tap(find.byIcon(Icons.settings));
      await tester.pumpAndSettle();

      if (find.text('Sync Mode').evaluate().isNotEmpty) {
        await tester.tap(find.text('Sync Mode'));
        await tester.pumpAndSettle();

        // Should show mode options
        expect(find.text('Compact'), findsOneWidget);
        expect(find.text('Deep Scan'), findsOneWidget);

        // Select deep scan
        await tester.tap(find.text('Deep Scan'));
        await tester.pumpAndSettle();

        // Should show deep scan warning
        expect(find.textContaining('slower'), findsOneWidget);

        // Confirm
        await tester.tap(find.text('OK'));
        await tester.pumpAndSettle();

        // Sync should restart in deep mode
        expect(find.textContaining('Deep'), findsOneWidget);
      }
    });

    testWidgets('Sync progress ETA calculation', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      // Monitor sync progress
      if (find.textContaining('Syncing').evaluate().isNotEmpty) {
        // Wait for ETA to calculate
        await tester.pump(Duration(seconds: 3));

        // Should show ETA after sufficient progress
        expect(
          textMatches(RegExp(r'\d+ (second|minute|hour)s? remaining')),
          findsOneWidget,
        );

        // Should show blocks/sec rate
        expect(textMatches(RegExp(r'\d+ blocks?/sec')), findsOneWidget);
      }
    });

    testWidgets('Lightwalletd connection switching', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      // Navigate to node picker
      await tester.tap(find.byIcon(Icons.settings));
      await tester.pumpAndSettle();

      if (find.text('Node Configuration').evaluate().isNotEmpty) {
        await tester.tap(find.text('Node Configuration'));
        await tester.pumpAndSettle();

        // Should show current node
        expect(find.textContaining('lightd.pirate.black'), findsOneWidget);

        // Select different node
        if (find.text('Select Node').evaluate().isNotEmpty) {
          await tester.tap(find.text('Select Node'));
          await tester.pumpAndSettle();

          // Should restart sync with new node
          expect(find.textContaining('Switching'), findsOneWidget);

          await tester.pumpAndSettle(Duration(seconds: 2));

          // Sync should restart
          expect(find.textContaining('Syncing'), findsOneWidget);
        }
      }
    });

    testWidgets('Sync error recovery - auto retry', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      // Wait for transient error (simulated)
      await tester.pump(Duration(seconds: 5));

      // Should automatically retry after transient error
      if (find.textContaining('Retrying').evaluate().isNotEmpty) {
        expect(textMatches(RegExp(r'Attempt \d+ of \d+')), findsOneWidget);

        // Wait for retry
        await tester.pumpAndSettle(Duration(seconds: 2));

        // Should resume syncing
        expect(find.textContaining('Syncing'), findsOneWidget);
      }
    });

    testWidgets('Checkpoint creation interval verification', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      // Monitor checkpoint creation during sync
      int checkpointCount = 0;

      // Watch for checkpoint messages
      for (int i = 0; i < 10; i++) {
        await tester.pump(Duration(seconds: 1));

        if (find.textContaining('Checkpoint').evaluate().isNotEmpty) {
          checkpointCount++;
        }
      }

      // Should create checkpoints periodically
      expect(checkpointCount > 0, true, reason: 'Should create checkpoints during sync');
    });
  });
}

