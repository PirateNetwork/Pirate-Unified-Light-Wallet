import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/features/app_shell/app_shell.dart';

void main() {
  testWidgets('clips desktop app bar paint to the content pane', (
    tester,
  ) async {
    await tester.pumpWidget(
      const MaterialApp(
        home: SizedBox(
          width: 320,
          height: 240,
          child: Row(
            children: [
              SizedBox(width: 96, child: ColoredBox(color: Colors.white)),
              Expanded(
                child: DesktopAppPane(
                  appBar: _ShadowedTestAppBar(),
                  child: ColoredBox(color: Colors.black),
                ),
              ),
            ],
          ),
        ),
      ),
    );

    final paneFinder = find.byType(DesktopAppPane);
    final clipFinder = find.descendant(
      of: paneFinder,
      matching: find.byType(ClipRect),
    );
    final clip = tester.widget<ClipRect>(clipFinder);

    expect(clip.clipBehavior, Clip.hardEdge);
    expect(tester.getRect(clipFinder), tester.getRect(paneFinder));
    expect(tester.getRect(find.byType(_ShadowedTestAppBar)).left, 96);
  });
}

class _ShadowedTestAppBar extends StatelessWidget
    implements PreferredSizeWidget {
  const _ShadowedTestAppBar();

  @override
  Size get preferredSize => const Size.fromHeight(64);

  @override
  Widget build(BuildContext context) {
    return Container(
      decoration: const BoxDecoration(
        color: Colors.white,
        boxShadow: [BoxShadow(blurRadius: 12, offset: Offset(0, 6))],
      ),
    );
  }
}
