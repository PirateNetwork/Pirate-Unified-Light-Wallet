import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/ui/molecules/p_list_tile.dart';

void main() {
  testWidgets('wraps complete labels instead of truncating them', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(320, 700);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      const MaterialApp(
        home: Scaffold(
          body: PListTile(
            leading: Icon(Icons.settings_outlined),
            title: 'A complete settings title that needs another line',
            subtitle:
                'Supporting information remains readable at accessible text sizes.',
            trailing: Icon(Icons.chevron_right),
          ),
        ),
      ),
    );

    final title = tester.widget<Text>(
      find.text('A complete settings title that needs another line'),
    );
    final subtitle = tester.widget<Text>(
      find.text(
        'Supporting information remains readable at accessible text sizes.',
      ),
    );

    expect(title.maxLines, isNull);
    expect(title.overflow, isNull);
    expect(subtitle.maxLines, isNull);
    expect(subtitle.overflow, isNull);
    expect(tester.takeException(), isNull);
  });
}
