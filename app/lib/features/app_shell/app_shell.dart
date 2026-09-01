import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../design/tokens/colors.dart';
import '../../ui/molecules/p_bottom_sheet.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_nav.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/swaps/swap_providers.dart';
import '../pay/pay_screen.dart';
import '../../core/providers/wallet_providers.dart';
import '../../core/services/address_rotation_service.dart';
import '../../core/i18n/arb_text_localizer.dart';
import '../../core/platform/platform_utils.dart';
import '../../core/swaps/swap_availability.dart';
import '../settings/providers/preferences_providers.dart';
import 'desktop_status_bar.dart';

/// App shell with persistent navigation.
class AppShell extends ConsumerWidget {
  const AppShell({required this.location, required this.child, super.key});

  final String location;
  final Widget child;

  List<PNavDestination> _destinations() => [
    PNavDestination(
      icon: Icons.home_outlined,
      selectedIcon: Icons.home,
      label: 'Home'.tr,
    ),
    PNavDestination(
      icon: Icons.grid_view_outlined,
      selectedIcon: Icons.grid_view_rounded,
      label: 'Wallets'.tr,
      isPay: true,
    ),
    PNavDestination(
      icon: Icons.receipt_long_outlined,
      selectedIcon: Icons.receipt_long,
      label: 'Activity'.tr,
    ),
    PNavDestination(
      icon: Icons.settings_outlined,
      selectedIcon: Icons.settings,
      label: 'Settings'.tr,
    ),
  ];

  int _locationToIndex(String path) {
    if (path.startsWith('/pay')) return 1;
    if (path.startsWith('/activity')) return 2;
    if (path.startsWith('/settings')) return 3;
    return 0;
  }

  void _onDestinationSelected(BuildContext context, int index) {
    switch (index) {
      case 0:
        context.go('/home');
        return;
      case 1:
        context.go('/pay');
        return;
      case 2:
        context.go('/activity');
        return;
      case 3:
        context.go('/settings');
        return;
      default:
        context.go('/home');
    }
  }

  void _openPaySheet(BuildContext context) {
    PBottomSheet.showAdaptive<void>(
      context: context,
      backgroundColor: Colors.transparent,
      useSafeArea: true,
      isScrollControlled: true,
      builder: (sheetContext) {
        return PaySheet(
          onSend: () {
            Navigator.of(sheetContext).pop();
            context.push('/send');
          },
          onReceive: () {
            Navigator.of(sheetContext).pop();
            context.push('/receive');
          },
          onVerify: () {
            Navigator.of(sheetContext).pop();
            context.push('/payment-disclosure');
          },
          onSwap: () {
            Navigator.of(sheetContext).pop();
            context.push('/swap');
          },
        );
      },
    );
  }

  PAppBar? _desktopAppBarFor(String path) {
    if (!isDesktopPlatform) {
      return null;
    }
    if (path.startsWith('/pay')) {
      return PAppBar(
        title: 'Wallets'.tr,
        subtitle: 'Send, receive, swap, or verify in a few steps.'.tr,
        actions: [WalletSwitcherButton(compact: true)],
        showThemeToggle: false,
      );
    }
    if (path.startsWith('/activity')) {
      return PAppBar(
        title: 'Activity'.tr,
        actions: [WalletSwitcherButton(compact: true)],
        showThemeToggle: false,
      );
    }
    if (path.startsWith('/settings')) {
      return PAppBar(
        title: 'Settings'.tr,
        subtitle: 'Security and privacy controls.'.tr,
        actions: [WalletSwitcherButton(compact: true)],
        showThemeToggle: false,
      );
    }
    return null;
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    ref
      ..watch(transactionWatcherProvider)
      ..watch(syncCompletionWatcherProvider)
      ..watch(autoRotationWatcherProvider)
      ..watch(syncCompletionRotationWatcherProvider)
      ..watch(walletInitRotationWatcherProvider)
      ..watch(localePreferenceProvider);
    if (kAtomicSwapsEnabled) {
      ref.watch(kdfSwapWarmupProvider);
    }
    final currentIndex = _locationToIndex(location);
    final nav = PNav(
      currentIndex: currentIndex,
      onDestinationSelected: (index) => _onDestinationSelected(context, index),
      destinations: isDesktopPlatform
          ? _destinations().take(3).toList(growable: false)
          : _destinations(),
      onPayTap: isDesktopPlatform ? null : () => _openPaySheet(context),
      payIndex: 1,
    );

    final content = SafeArea(top: false, child: child);
    final desktopAppBar = _desktopAppBarFor(location);
    final body = isDesktopPlatform
        ? Column(
            children: [
              Expanded(
                child: Row(
                  children: [
                    DecoratedBox(
                      decoration: BoxDecoration(
                        color: AppColors.backgroundSurface,
                        border: Border(
                          right: BorderSide(color: AppColors.borderSubtle),
                        ),
                      ),
                      child: SafeArea(right: false, child: nav),
                    ),
                    Expanded(
                      child: DesktopAppPane(
                        appBar: desktopAppBar,
                        child: content,
                      ),
                    ),
                  ],
                ),
              ),
              DesktopStatusBar(
                settingsSelected: location.startsWith('/settings'),
                onSettingsTap: () => context.go('/settings'),
                onConnectionTap: () => context.push('/settings/privacy-shield'),
              ),
            ],
          )
        : content;
    return PScaffold(
      title: 'Stashi Wallet',
      useSafeArea: false,
      body: body,
      bottomNavigationBar: isDesktopPlatform ? null : nav,
    );
  }
}

/// Keeps desktop header effects inside the content side of the shell.
class DesktopAppPane extends StatelessWidget {
  const DesktopAppPane({required this.child, this.appBar, super.key});

  final PreferredSizeWidget? appBar;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final candidate = appBar;
    final appBarHeight = candidate is PAppBar
        ? candidate.preferredHeightFor(context)
        : candidate?.preferredSize.height;
    return ClipRect(
      clipBehavior: Clip.hardEdge,
      child: Column(
        children: [
          if (appBar != null) SizedBox(height: appBarHeight, child: appBar),
          Expanded(child: child),
        ],
      ),
    );
  }
}
