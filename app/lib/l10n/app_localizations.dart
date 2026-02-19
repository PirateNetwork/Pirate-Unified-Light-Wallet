import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_en.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of AppLocalizations
/// returned by `AppLocalizations.of(context)`.
///
/// Applications need to include `AppLocalizations.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'l10n/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: AppLocalizations.localizationsDelegates,
///   supportedLocales: AppLocalizations.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the AppLocalizations.supportedLocales
/// property.
abstract class AppLocalizations {
  AppLocalizations(String locale)
    : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static AppLocalizations of(BuildContext context) {
    return Localizations.of<AppLocalizations>(context, AppLocalizations)!;
  }

  static const LocalizationsDelegate<AppLocalizations> delegate =
      _AppLocalizationsDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
        delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
      ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[Locale('en')];

  /// No description provided for @appTitle.
  ///
  /// In en, this message translates to:
  /// **'Pirate Wallet'**
  String get appTitle;

  /// No description provided for @settingsTitle.
  ///
  /// In en, this message translates to:
  /// **'Settings'**
  String get settingsTitle;

  /// No description provided for @settingsSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Security and privacy controls.'**
  String get settingsSubtitle;

  /// No description provided for @outboundApiTitle.
  ///
  /// In en, this message translates to:
  /// **'Outbound API Calls'**
  String get outboundApiTitle;

  /// No description provided for @outboundApiSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Control non-lightserver internet requests'**
  String get outboundApiSubtitle;

  /// No description provided for @allowNonLightserverApiCalls.
  ///
  /// In en, this message translates to:
  /// **'Allow non-lightserver API calls'**
  String get allowNonLightserverApiCalls;

  /// No description provided for @allowNonLightserverApiCallsHelp.
  ///
  /// In en, this message translates to:
  /// **'Master switch for all outbound connections except your configured lightwalletd server.'**
  String get allowNonLightserverApiCallsHelp;

  /// No description provided for @livePriceFeedsTitle.
  ///
  /// In en, this message translates to:
  /// **'Live Price Feeds'**
  String get livePriceFeedsTitle;

  /// No description provided for @livePriceFeedsSubtitle.
  ///
  /// In en, this message translates to:
  /// **'CoinGecko primary + CoinMarketCap fallback for ARRR prices and fiat conversion.'**
  String get livePriceFeedsSubtitle;

  /// No description provided for @verifyBuildGithubChecksTitle.
  ///
  /// In en, this message translates to:
  /// **'Verify Build GitHub Checks'**
  String get verifyBuildGithubChecksTitle;

  /// No description provided for @verifyBuildGithubChecksSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Fetches releases and checksum files from GitHub when using Verify Build.'**
  String get verifyBuildGithubChecksSubtitle;

  /// No description provided for @desktopUpdateChecksTitle.
  ///
  /// In en, this message translates to:
  /// **'Desktop Update Checks'**
  String get desktopUpdateChecksTitle;

  /// No description provided for @desktopUpdateChecksSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Allows periodic GitHub release polling for in-app update notifications.'**
  String get desktopUpdateChecksSubtitle;

  /// No description provided for @disabledByMasterSwitch.
  ///
  /// In en, this message translates to:
  /// **'Disabled by master switch'**
  String get disabledByMasterSwitch;

  /// No description provided for @outboundApiFeaturesSummary.
  ///
  /// In en, this message translates to:
  /// **'Current non-lightserver network features: live price feeds, Verify Build GitHub checks, and desktop release checks. Other wallet traffic stays on your selected lightwalletd transport.'**
  String get outboundApiFeaturesSummary;

  /// No description provided for @updateAvailableTitle.
  ///
  /// In en, this message translates to:
  /// **'Update Available'**
  String get updateAvailableTitle;

  /// No description provided for @unknownPublishTime.
  ///
  /// In en, this message translates to:
  /// **'Unknown publish time'**
  String get unknownPublishTime;

  /// No description provided for @updateAvailableMessage.
  ///
  /// In en, this message translates to:
  /// **'A new version ({version}) is available.\nPublished: {published}\n\nChoose Update to download and install now.'**
  String updateAvailableMessage(String version, String published);

  /// No description provided for @updateButton.
  ///
  /// In en, this message translates to:
  /// **'Update'**
  String get updateButton;

  /// No description provided for @changelogButton.
  ///
  /// In en, this message translates to:
  /// **'Changelog'**
  String get changelogButton;

  /// No description provided for @cancelButton.
  ///
  /// In en, this message translates to:
  /// **'Cancel'**
  String get cancelButton;

  /// No description provided for @preparingUpdateInstaller.
  ///
  /// In en, this message translates to:
  /// **'Preparing update installer...'**
  String get preparingUpdateInstaller;

  /// No description provided for @installerLaunchedClosing.
  ///
  /// In en, this message translates to:
  /// **'Installer launched. Closing app to finish update...'**
  String get installerLaunchedClosing;

  /// No description provided for @automaticUpdateFailedMessage.
  ///
  /// In en, this message translates to:
  /// **'Automatic update failed: {error}. Use Changelog to download manually.'**
  String automaticUpdateFailedMessage(String error);

  /// No description provided for @languageTitle.
  ///
  /// In en, this message translates to:
  /// **'Language'**
  String get languageTitle;

  /// No description provided for @languageSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Select application language'**
  String get languageSubtitle;

  /// No description provided for @languageSettingTitle.
  ///
  /// In en, this message translates to:
  /// **'Language'**
  String get languageSettingTitle;

  /// No description provided for @languageSettingSubtitle.
  ///
  /// In en, this message translates to:
  /// **'English'**
  String get languageSettingSubtitle;

  /// No description provided for @englishLanguage.
  ///
  /// In en, this message translates to:
  /// **'English'**
  String get englishLanguage;

  /// No description provided for @moreLanguagesSoon.
  ///
  /// In en, this message translates to:
  /// **'More languages can be added by dropping translated ARB files into lib/l10n and rebuilding.'**
  String get moreLanguagesSoon;
}

class _AppLocalizationsDelegate
    extends LocalizationsDelegate<AppLocalizations> {
  const _AppLocalizationsDelegate();

  @override
  Future<AppLocalizations> load(Locale locale) {
    return SynchronousFuture<AppLocalizations>(lookupAppLocalizations(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['en'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppLocalizationsDelegate old) => false;
}

AppLocalizations lookupAppLocalizations(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return AppLocalizationsEn();
  }

  throw FlutterError(
    'AppLocalizations.delegate failed to load unsupported locale "$locale". This is likely '
    'an issue with the localizations generation tool. Please file an issue '
    'on GitHub with a reproducible sample app and the gen-l10n configuration '
    'that was used.',
  );
}
