import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../features/showcase/screens/showcase_home_screen.dart';
import '../features/showcase/screens/showcase_buttons_screen.dart';
import '../features/showcase/screens/showcase_forms_screen.dart';
import '../features/showcase/screens/showcase_cards_screen.dart';
import '../features/showcase/screens/showcase_dialogs_screen.dart';
import '../features/showcase/screens/showcase_animations_screen.dart';
import '../features/app_shell/app_shell.dart';
import '../features/activity/activity_screen.dart';
import '../features/activity/transaction_detail_screen.dart';
import '../features/wallet_shell/wallet_shell_screen.dart';
import '../features/receive/receive_screen.dart';
import '../features/settings/export_seed_screen.dart';
import '../features/settings/panic_pin_screen.dart';
import '../features/settings/watch_only_screen.dart';
import '../features/settings/verify_build_screen.dart';
import '../features/settings/screens/node_settings_screen.dart';
import '../features/settings/settings_screen.dart';
import '../features/onboarding/screens/welcome_screen.dart';
import '../features/onboarding/screens/create_or_import_screen.dart';
import '../features/onboarding/screens/passphrase_setup_screen.dart';
import '../features/onboarding/screens/biometrics_screen.dart';
import '../features/onboarding/screens/backup_warning_screen.dart';
import '../features/onboarding/screens/seed_display_screen.dart';
import '../features/onboarding/screens/seed_confirm_screen.dart';
import '../features/onboarding/screens/seed_import_screen.dart';
import '../features/onboarding/screens/ivk_import_screen.dart';
import '../features/onboarding/screens/birthday_picker_screen.dart';
import '../features/home/home_screen.dart';
import '../features/send/send_screen.dart';
import '../features/pay/pay_screen.dart';
import '../features/settings/screens/biometrics_screen.dart';
import '../features/settings/screens/theme_screen.dart';
import '../features/settings/screens/currency_screen.dart';
import '../features/settings/screens/birthday_height_screen.dart';
import '../features/settings/screens/terms_screen.dart';
import '../features/settings/screens/licenses_screen.dart';
import '../features/settings/screens/privacy_shield_screen.dart';
import '../features/unlock/unlock_screen.dart';
import '../features/splash/splash_screen.dart';
import '../core/providers/wallet_providers.dart';
import '../ui/motion/curves.dart';
import '../ui/motion/durations.dart';

final appRouterProvider = Provider<GoRouter>((ref) {
  // Always start with splash screen - it will navigate once walletsExist resolves
  // This prevents any flash of wrong screen
  return GoRouter(
    initialLocation: '/splash',
    debugLogDiagnostics: true,
    redirect: (context, state) {
      final walletsExistAsync = ref.read(walletsExistProvider);
      final appUnlockedValue = ref.read(appUnlockedProvider);
      final isOnboarding = state.uri.path.startsWith('/onboarding');
      final isUnlock = state.uri.path == '/unlock';
      
      // Get walletsExist value (if available)
      final walletsExistValue = walletsExistAsync.value;
      
      // If still loading, don't redirect yet (let initialLocation handle it)
      if (!walletsExistAsync.hasValue) {
        return null;
      }
      
      // If wallets exist and we're on onboarding, redirect to unlock
      if (walletsExistValue == true && isOnboarding && !appUnlockedValue) {
        return '/unlock';
      }
      
      // If wallets exist and app is not unlocked, redirect to unlock (unless already there)
      if (walletsExistValue == true && !appUnlockedValue && !isUnlock && !isOnboarding) {
        return '/unlock';
      }
      
      // If no wallets exist and we're on unlock, redirect to onboarding
      if (walletsExistValue == false && isUnlock) {
        return '/onboarding/welcome';
      }
      
      return null;
    },
    routes: [
      // ========================================================================
      // SPLASH/LOADING SCREEN
      // ========================================================================
      
      GoRoute(
        path: '/splash',
        name: 'splash',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const SplashScreen(),
        ),
      ),
      
      // ========================================================================
      // UNLOCK SCREEN
      // ========================================================================
      
      GoRoute(
        path: '/unlock',
        name: 'unlock',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const UnlockScreen(),
        ),
      ),
      
      // ========================================================================
      // ONBOARDING FLOW
      // ========================================================================
      
      // Welcome screen - entry point
      GoRoute(
        path: '/onboarding/welcome',
        name: 'onboarding-welcome',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const WelcomeScreen(),
        ),
      ),
      
      // Create or Import selection
      GoRoute(
        path: '/onboarding/create-or-import',
        name: 'onboarding-create-or-import',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const CreateOrImportScreen(),
        ),
      ),
      
      // Passphrase setup (create flow)
      GoRoute(
        path: '/onboarding/passphrase',
        name: 'onboarding-passphrase',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const PassphraseSetupScreen(),
        ),
      ),

      // Biometrics (optional)
      GoRoute(
        path: '/onboarding/biometrics',
        name: 'onboarding-biometrics',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const OnboardingBiometricsScreen(),
        ),
      ),
      
      // Backup warning (create flow)
      GoRoute(
        path: '/onboarding/backup-warning',
        name: 'onboarding-backup-warning',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const BackupWarningScreen(),
        ),
      ),
      
      // Seed display (create flow)
      GoRoute(
        path: '/onboarding/seed-display',
        name: 'onboarding-seed-display',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const SeedDisplayScreen(),
        ),
      ),
      
      // Seed confirm (create flow)
      GoRoute(
        path: '/onboarding/seed-confirm',
        name: 'onboarding-seed-confirm',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const SeedConfirmScreen(),
        ),
      ),
      
      // Seed import (restore flow)
      GoRoute(
        path: '/onboarding/import-seed',
        name: 'onboarding-import-seed',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const SeedImportScreen(),
        ),
      ),
      
      // IVK import (watch-only flow)
      GoRoute(
        path: '/onboarding/import-ivk',
        name: 'onboarding-import-ivk',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const IvkImportScreen(),
        ),
      ),
      
      // Birthday picker (restore/create finalization)
      GoRoute(
        path: '/onboarding/birthday',
        name: 'onboarding-birthday',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const BirthdayPickerScreen(),
        ),
      ),
      
      // ========================================================================
      // MAIN APP
      // ========================================================================
      ShellRoute(
        builder: (context, state, child) => AppShell(
          location: state.uri.path,
          child: child,
        ),
        routes: [
          // Home screen - main wallet dashboard
          GoRoute(
            path: '/home',
            name: 'home',
            pageBuilder: (context, state) => _buildPageWithTransition(
              context: context,
              state: state,
              child: const HomeScreen(useScaffold: false),
            ),
          ),

          // Pay - entry point for Send/Receive
          GoRoute(
            path: '/pay',
            name: 'pay',
            pageBuilder: (context, state) => _buildPageWithTransition(
              context: context,
              state: state,
              child: const PayScreen(useScaffold: false),
            ),
          ),

          // Activity - full transaction history
          GoRoute(
            path: '/activity',
            name: 'activity',
            pageBuilder: (context, state) => _buildPageWithTransition(
              context: context,
              state: state,
              child: const ActivityScreen(useScaffold: false),
            ),
          ),

          // Settings overview
          GoRoute(
            path: '/settings',
            name: 'settings',
            pageBuilder: (context, state) => _buildPageWithTransition(
              context: context,
              state: state,
              child: const SettingsScreen(useScaffold: false),
            ),
          ),
        ],
      ),

      // Wallet Shell (FFI Integration Demo)
      GoRoute(
        path: '/wallet-shell',
        name: 'wallet-shell',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const WalletShellScreen(),
        ),
      ),

      // Send Screen
      GoRoute(
        path: '/send',
        name: 'send',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const SendScreen(),
        ),
      ),

      // Receive Screen
      GoRoute(
        path: '/receive',
        name: 'receive',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const ReceiveScreen(),
        ),
      ),

      // Transaction details
      GoRoute(
        path: '/transaction/:txid',
        name: 'transaction-detail',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: TransactionDetailScreen(
            txid: state.pathParameters['txid'] ?? '',
          ),
        ),
      ),
      
      // Settings - Security Features
      GoRoute(
        path: '/settings/export-seed',
        name: 'export-seed',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: ExportSeedScreen(
            walletId: state.uri.queryParameters['walletId'] ?? 'default',
            walletName: state.uri.queryParameters['walletName'] ?? 'My Wallet',
          ),
        ),
      ),
      GoRoute(
        path: '/settings/panic-pin',
        name: 'panic-pin',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const PanicPinScreen(),
        ),
      ),
      GoRoute(
        path: '/settings/watch-only',
        name: 'watch-only',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const WatchOnlyScreen(),
        ),
      ),
      GoRoute(
        path: '/settings/biometrics',
        name: 'settings-biometrics',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const BiometricsScreen(),
        ),
      ),
      GoRoute(
        path: '/settings/theme',
        name: 'settings-theme',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const ThemeScreen(),
        ),
      ),
      GoRoute(
        path: '/settings/currency',
        name: 'settings-currency',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const CurrencyScreen(),
        ),
      ),
      GoRoute(
        path: '/settings/birthday-height',
        name: 'settings-birthday-height',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const BirthdayHeightScreen(),
        ),
      ),
      GoRoute(
        path: '/settings/verify-build',
        name: 'verify-build',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const VerifyBuildScreen(),
        ),
      ),
      GoRoute(
        path: '/settings/terms',
        name: 'settings-terms',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const TermsScreen(),
        ),
      ),
      GoRoute(
        path: '/settings/licenses',
        name: 'settings-licenses',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const LicensesScreen(),
        ),
      ),
      
      // Node Settings
      GoRoute(
        path: '/settings/node-picker',
        name: 'node-picker',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const NodeSettingsScreen(),
        ),
      ),
      GoRoute(
        path: '/settings/privacy-shield',
        name: 'privacy-shield',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const PrivacyShieldScreen(),
        ),
      ),
      
      // Design System Showcase
      GoRoute(
        path: '/showcase',
        name: 'showcase',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const ShowcaseHomeScreen(),
        ),
      ),
      GoRoute(
        path: '/buttons',
        name: 'buttons',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const ShowcaseButtonsScreen(),
        ),
      ),
      GoRoute(
        path: '/forms',
        name: 'forms',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const ShowcaseFormsScreen(),
        ),
      ),
      GoRoute(
        path: '/cards',
        name: 'cards',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const ShowcaseCardsScreen(),
        ),
      ),
      GoRoute(
        path: '/dialogs',
        name: 'dialogs',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const ShowcaseDialogsScreen(),
        ),
      ),
      GoRoute(
        path: '/animations',
        name: 'animations',
        pageBuilder: (context, state) => _buildPageWithTransition(
          context: context,
          state: state,
          child: const ShowcaseAnimationsScreen(),
        ),
      ),
    ],
  );
});

/// Build page with custom transition
CustomTransitionPage _buildPageWithTransition({
  required BuildContext context,
  required GoRouterState state,
  required Widget child,
}) {
  return CustomTransitionPage(
    key: state.pageKey,
    child: child,
    transitionsBuilder: (context, animation, secondaryAnimation, child) {
      return child;
    },
    transitionDuration: PDurations.instant,
    reverseTransitionDuration: PDurations.instant,
  );
}
