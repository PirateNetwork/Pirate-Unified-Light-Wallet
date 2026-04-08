import '../ffi/generated/models.dart';

extension MnemonicLanguageX on MnemonicLanguage {
  String get nativeLabel {
    switch (this) {
      case MnemonicLanguage.english:
        return 'English';
      case MnemonicLanguage.chineseSimplified:
        return '中文(简体)';
      case MnemonicLanguage.chineseTraditional:
        return '中文(繁體)';
      case MnemonicLanguage.french:
        return 'Français';
      case MnemonicLanguage.italian:
        return 'Italiano';
      case MnemonicLanguage.japanese:
        return '日本語';
      case MnemonicLanguage.korean:
        return '한국어';
      case MnemonicLanguage.spanish:
        return 'Español';
    }
  }

  String get assetPath {
    switch (this) {
      case MnemonicLanguage.english:
        return 'assets/bip39/english.txt';
      case MnemonicLanguage.chineseSimplified:
        return 'assets/bip39/chinese_simplified.txt';
      case MnemonicLanguage.chineseTraditional:
        return 'assets/bip39/chinese_traditional.txt';
      case MnemonicLanguage.french:
        return 'assets/bip39/french.txt';
      case MnemonicLanguage.italian:
        return 'assets/bip39/italian.txt';
      case MnemonicLanguage.japanese:
        return 'assets/bip39/japanese.txt';
      case MnemonicLanguage.korean:
        return 'assets/bip39/korean.txt';
      case MnemonicLanguage.spanish:
        return 'assets/bip39/spanish.txt';
    }
  }
}

const List<MnemonicLanguage> supportedMnemonicLanguages =
    MnemonicLanguage.values;
