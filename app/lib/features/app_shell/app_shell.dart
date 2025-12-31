import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/typography.dart';
import '../../ui/organisms/p_nav.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../pay/pay_screen.dart';
import '../../core/providers/wallet_providers.dart';

/// App shell with persistent navigation.
class AppShell extends ConsumerWidget {
  const AppShell({
    required this.location,
    required this.child,
    super.key,
  });

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
    final messenger = ScaffoldMessenger.of(context);
    messenger.hideCurrentSnackBar();
    messenger.showSnackBar(
      SnackBar(
        content: Text(
          'Coming Soon',
          style: PTypography.bodyMedium(color: AppColors.textPrimary),
        ),
        duration: const Duration(seconds: 1),
        behavior: SnackBarBehavior.floating,
        backgroundColor: AppColors.backgroundElevated,
      ),
    );
  }

  void _openPaySheet(BuildContext context) {
    showModalBottomSheet<void>(
      context: context,
      backgroundColor: Colors.transparent,
      useSafeArea: true,
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

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    ref.watch(transactionWatcherProvider);
    final currentIndex = _locationToIndex(location);
    final nav = PNav(
      currentIndex: currentIndex,
      onDestinationSelected: (index) => _onDestinationSelected(context, index),
      destinations: _destinations,
      onPayTap: _isDesktop ? null : () => _openPaySheet(context),
      payIndex: 1,
    );

    final content = SafeArea(top: false, child: child);
    final body = _isDesktop
        ? Row(
            children: [
              Container(
                decoration: BoxDecoration(
                  color: AppColors.backgroundSurface,
                  border: Border(
                    right: BorderSide(color: AppColors.borderSubtle),
                  ),
                ),
                child: SafeArea(
                  right: false,
                  child: nav,
                ),
              ),
              Expanded(child: content),
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
