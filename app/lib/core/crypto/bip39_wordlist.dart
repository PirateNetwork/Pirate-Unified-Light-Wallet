import 'package:flutter/services.dart';

import '../ffi/generated/models.dart';
import 'mnemonic_language.dart';

class Bip39WordlistService {
  Bip39WordlistService._();

  static final Bip39WordlistService instance = Bip39WordlistService._();

  final Map<MnemonicLanguage, List<String>> _cache =
      <MnemonicLanguage, List<String>>{};

  Future<List<String>> load(MnemonicLanguage language) async {
    final cached = _cache[language];
    if (cached != null) {
      return cached;
    }

    final raw = await rootBundle.loadString(language.assetPath);
    final words = raw
        .split('\n')
        .map((word) => word.trim())
        .where((word) => word.isNotEmpty)
        .toList(growable: false);
    _cache[language] = words;
    return words;
  }
}

Future<List<String>> loadBip39Wordlist(MnemonicLanguage language) {
  return Bip39WordlistService.instance.load(language);
}

List<String> bip39SuggestionsFromWordlist(
  String query,
  List<String> words, {
  int limit = 6,
}) {
  final trimmed = query.trim().toLowerCase();
  if (trimmed.isEmpty || words.isEmpty) {
    return const <String>[];
  }

  final matches = <String>[];
  for (final word in words) {
    if (word.startsWith(trimmed)) {
      matches.add(word);
      if (matches.length >= limit) {
        break;
      }
    }
  }
  return matches;
}
