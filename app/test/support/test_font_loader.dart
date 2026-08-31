import 'dart:io';

import 'package:flutter/services.dart';

/// Loads a repository font without depending on a widget test asset bundle.
Future<void> loadTestFont(String family, String assetPath) async {
  final loader = FontLoader(family)
    ..addFont(File(assetPath).readAsBytes().then(ByteData.sublistView));
  await loader.load();
}
