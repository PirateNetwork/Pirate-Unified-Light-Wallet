// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get appTitle => 'Pirate Wallet';

  @override
  String get settingsTitle => 'Settings';

  @override
  String get settingsSubtitle => 'Security and privacy controls.';

  @override
  String get outboundApiTitle => 'Outbound API Calls';

  @override
  String get outboundApiSubtitle => 'Control non-lightserver internet requests';

  @override
  String get allowNonLightserverApiCalls => 'Allow non-lightserver API calls';

  @override
  String get allowNonLightserverApiCallsHelp =>
      'Master switch for all outbound connections except your configured lightwalletd server.';

  @override
  String get livePriceFeedsTitle => 'Live Price Feeds';

  @override
  String get livePriceFeedsSubtitle =>
      'CoinGecko primary + CoinMarketCap fallback for ARRR prices and fiat conversion.';

  @override
  String get verifyBuildGithubChecksTitle => 'Verify Build GitHub Checks';

  @override
  String get verifyBuildGithubChecksSubtitle =>
      'Fetches releases and checksum files from GitHub when using Verify Build.';

  @override
  String get desktopUpdateChecksTitle => 'Desktop Update Checks';

  @override
  String get desktopUpdateChecksSubtitle =>
      'Allows periodic GitHub release polling for in-app update notifications.';

  @override
  String get disabledByMasterSwitch => 'Disabled by master switch';

  @override
  String get outboundApiFeaturesSummary =>
      'Current non-lightserver network features: live price feeds, Verify Build GitHub checks, and desktop release checks. Other wallet traffic stays on your selected lightwalletd transport.';

  @override
  String get updateAvailableTitle => 'Update Available';

  @override
  String get unknownPublishTime => 'Unknown publish time';

  @override
  String updateAvailableMessage(String version, String published) {
    return 'A new version ($version) is available.\nPublished: $published\n\nChoose Update to download and install now.';
  }

  @override
  String get updateButton => 'Update';

  @override
  String get changelogButton => 'Changelog';

  @override
  String get cancelButton => 'Cancel';

  @override
  String get preparingUpdateInstaller => 'Preparing update installer...';

  @override
  String get installerLaunchedClosing =>
      'Installer launched. Closing app to finish update...';

  @override
  String automaticUpdateFailedMessage(String error) {
    return 'Automatic update failed: $error. Use Changelog to download manually.';
  }

  @override
  String get languageTitle => 'Language';

  @override
  String get languageSubtitle => 'Select application language';

  @override
  String get languageSettingTitle => 'Language';

  @override
  String get languageSettingSubtitle => 'English';

  @override
  String get englishLanguage => 'English';

  @override
  String get moreLanguagesSoon =>
      'More languages can be added by dropping translated ARB files into lib/l10n and rebuilding.';
}
