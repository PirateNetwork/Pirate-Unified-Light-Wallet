import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../../design/deep_space_theme.dart';
import '../../core/providers/wallet_providers.dart';

/// Splash/Loading screen shown while determining initial route
class SplashScreen extends ConsumerWidget {
  const SplashScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    // Watch walletsExistProvider to determine when to navigate
    final walletsExist = ref.watch(walletsExistProvider);
    final appUnlocked = ref.watch(appUnlockedProvider);

    // Navigate once we know the state
    if (walletsExist.hasValue) {
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
            const CircularProgressIndicator(),
          ],
        ),
      ),
    );
  }
}

