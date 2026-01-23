import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../../design/deep_space_theme.dart';
import '../../core/providers/rust_init_provider.dart';
import '../../core/providers/wallet_providers.dart';

/// Splash/Loading screen shown while determining initial route
class SplashScreen extends ConsumerWidget {
  const SplashScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final rustInit = ref.watch(rustInitProvider);

    // Watch walletsExistProvider to determine when to navigate
    final walletsExist = ref.watch(walletsExistProvider);
    final appUnlocked = ref.watch(appUnlockedProvider);

    // Navigate once we know the state
    if (rustInit.hasValue && walletsExist.hasValue) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (walletsExist.value == true) {
          // Wallets exist
          if (appUnlocked) {
            context.go('/home');
          } else {
            context.go('/unlock');
          }
        } else {
          // No wallets - go to onboarding
          context.go('/onboarding/welcome');
        }
      });
    }

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
              'Pirate Wallet',
              style: AppTypography.h1.copyWith(
                color: AppColors.textPrimary,
              ),
            ),
            SizedBox(height: AppSpacing.md),
            if (initError != null) ...[
              Padding(
                padding: EdgeInsets.symmetric(horizontal: AppSpacing.lg),
                child: Text(
                  'Core failed to initialize.\n$initError',
                  style: AppTypography.body.copyWith(
                    color: Colors.redAccent,
                  ),
                  textAlign: TextAlign.center,
                ),
              ),
              SizedBox(height: AppSpacing.lg),
              TextButton(
                onPressed: () => ref.invalidate(rustInitProvider),
                child: const Text('Retry'),
              ),
            ] else ...[
              const CircularProgressIndicator(),
              SizedBox(height: AppSpacing.md),
              Text(
                rustInit.isLoading ? 'Initializing core...' : 'Loading wallets...',
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

