import '../../core/i18n/arb_text_localizer.dart';

String localizedPrivacyPolicyText() =>
    '''
Stashi Wallet is designed to minimize data collection.
The app does not require account registration and does not send seed phrases, private keys, or plaintext passphrases to our servers.

The wallet communicates with your configured lightwalletd endpoint to sync chain data and submit transactions.
If optional external features are enabled (for example market-price lookups), those requests are sent to the selected third-party providers and may include standard network metadata such as IP address and request timing.

You can disable optional outbound API calls in Settings.
'''
        .tr;
