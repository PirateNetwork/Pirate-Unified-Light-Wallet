import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../design/tokens/colors.dart';
import '../../ui/molecules/connection_status_indicator.dart';
import '../../ui/molecules/p_bottom_sheet.dart';
import '../../ui/molecules/p_snack.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_nav.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../pay/pay_screen.dart';
import '../../core/providers/wallet_providers.dart';
import '../../core/services/address_rotation_service.dart';

/// App shell with persistent navigation.
class AppShell extends ConsumerWidget {
  const AppShell({required this.location, required this.child, super.key});

  final String location;
  final Widget child;

  bool get _isDesktop =>
      Platform.isWindows || Platform.isMacOS || Platform.isLinux;

  static const List<PNavDestination> _destinations = [
    PNavDestination(
      icon: Icons.home_outlined,
      selectedIcon: Icons.home,
      label: 'Home',
    ),
    PNavDestination(
      icon: Icons.payments_outlined,
      selectedIcon: Icons.payments,
      label: 'Pay',
      isPay: true,
    ),
    PNavDestination(
      icon: Icons.receipt_long_outlined,
      selectedIcon: Icons.receipt_long,
      label: 'Activity',
    ),
    PNavDestination(
      icon: Icons.settings_outlined,
      selectedIcon: Icons.settings,
      label: 'Settings',
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

  void _showComingSoon(BuildContext context) {
    PSnack.show(
      context: context,
      message: 'Coming Soon',
      duration: const Duration(seconds: 1),
      variant: PSnackVariant.info,
    );
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
          onBuy: () {
            Navigator.of(sheetContext).pop();
            _showComingSoon(context);
          },
          onSpend: () {
            Navigator.of(sheetContext).pop();
            _showComingSoon(context);
          },
        );
      },
    );
  }

  PAppBar? _desktopAppBarFor(String path) {
    if (!_isDesktop) {
      return null;
    }
    if (path.startsWith('/pay')) {
      return const PAppBar(
        title: 'Pay',
        subtitle: 'Send, receive, buy, or spend in a few taps.',
        actions: [
          ConnectionStatusIndicator(full: true),
          WalletSwitcherButton(compact: true),
        ],
      );
    }
    if (path.startsWith('/activity')) {
      return const PAppBar(
        title: 'Activity',
        actions: [
          ConnectionStatusIndicator(full: true),
          WalletSwitcherButton(compact: true),
        ],
      );
    }
    if (path.startsWith('/settings')) {
      return const PAppBar(
        title: 'Settings',
        subtitle: 'Security and privacy controls.',
        actions: [
          ConnectionStatusIndicator(full: true),
          WalletSwitcherButton(compact: true),
        ],
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
      ..watch(walletInitRotationWatcherProvider);
    final currentIndex = _locationToIndex(location);
    final nav = PNav(
      currentIndex: currentIndex,
      onDestinationSelected: (index) => _onDestinationSelected(context, index),
      destinations: _destinations,
      onPayTap: _isDesktop ? null : () => _openPaySheet(context),
      payIndex: 1,
    );

    final content = SafeArea(top: false, child: child);
    final desktopAppBar = _desktopAppBarFor(location);
    final body = _isDesktop
        ? Row(
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
                child: Column(
                  children: [
                    if (desktopAppBar != null)
                      SizedBox(
                        height: desktopAppBar.preferredSize.height,
                        child: desktopAppBar,
                      ),
                    Expanded(child: content),
                  ],
                ),
              ),
            ],
          )
        : content;
    return PScaffold(
      title: 'Pirate Wallet',
      useSafeArea: false,
      body: body,
      bottomNavigationBar: _isDesktop ? null : nav,
    );
  }
}
