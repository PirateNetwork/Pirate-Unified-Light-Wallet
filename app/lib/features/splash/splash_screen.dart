import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../../design/deep_space_theme.dart';
import '../../core/providers/rust_init_provider.dart';
import '../../core/providers/wallet_providers.dart';
import '../../core/i18n/arb_text_localizer.dart';

/// Splash/Loading screen shown while determining initial route
class SplashScreen extends ConsumerStatefulWidget {
  const SplashScreen({super.key});

  @override
  ConsumerState<SplashScreen> createState() => _SplashScreenState();
}

class _SplashScreenState extends ConsumerState<SplashScreen> {
  bool _navigated = false;

  void _maybeNavigate({
    required AsyncValue<void> rustInit,
    required AsyncValue<bool> walletsExist,
    required bool appUnlocked,
  }) {
    if (_navigated) return;
    if (rustInit.hasValue && walletsExist.hasValue) {
      _navigated = true;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        if (walletsExist.value ?? false) {
          if (appUnlocked) {
            context.go('/home');
          } else {
            context.go('/unlock');
          }
        } else {
          context.go('/onboarding/welcome');
        }
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final rustInit = ref.watch(rustInitProvider);

    // Watch walletsExistProvider to determine when to navigate
    final walletsExist = ref.watch(walletsExistProvider);
    final appUnlocked = ref.watch(appUnlockedProvider);

    _maybeNavigate(
      rustInit: rustInit,
      walletsExist: walletsExist,
      appUnlocked: appUnlocked,
    );

    final initError = rustInit.hasError ? rustInit.error.toString() : null;

    return Scaffold(
      backgroundColor: AppColors.backgroundBase,
      body: Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            // App logo or icon
            Icon(
              Icons.account_balance_wallet,
              size: 64,
              color: AppColors.accentPrimary,
            ),
            SizedBox(height: AppSpacing.xl),
            Text(
              'Pirate Wallet'.tr,
              style: AppTypography.h1.copyWith(color: AppColors.textPrimary),
            ),
            SizedBox(height: AppSpacing.md),
            if (initError != null) ...[
              Padding(
                padding: EdgeInsets.symmetric(horizontal: AppSpacing.lg),
                child: Text(
                  'Core failed to initialize.\n$initError',
                  style: AppTypography.body.copyWith(color: Colors.redAccent),
                  textAlign: TextAlign.center,
                ),
              ),
              SizedBox(height: AppSpacing.lg),
              TextButton(
                onPressed: () => ref.invalidate(rustInitProvider),
                child: Text('Retry'.tr),
              ),
            ] else ...[
              const CircularProgressIndicator(),
              SizedBox(height: AppSpacing.md),
              Text(
                rustInit.isLoading
                    ? 'Initializing core...'
                    : 'Loading wallets...',
                style: AppTypography.body.copyWith(
                  color: AppColors.textSecondary,
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
