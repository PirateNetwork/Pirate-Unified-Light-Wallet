import 'package:flutter/material.dart';

import '../../../design/tokens/spacing.dart';
import '../../../ui/molecules/connection_status_indicator.dart';
import '../../../ui/molecules/wallet_switcher.dart';

class HomeHeaderControls extends StatelessWidget {
  const HomeHeaderControls({
    required this.onConnectionTap,
    this.showConnectionStatus = true,
    super.key,
  });

  static const double stackedBreakpoint = 640.0;
  static const Key walletControlKey = Key('home-header-wallet-control');
  static const Key connectionControlKey = Key('home-header-connection-control');

  final VoidCallback onConnectionTap;
  final bool showConnectionStatus;

  static bool shouldStack(double availableWidth) {
    return availableWidth < stackedBreakpoint;
  }

  @override
  Widget build(BuildContext context) {
    if (!showConnectionStatus) {
      return const Align(
        alignment: AlignmentDirectional.centerEnd,
        child: WalletSwitcherButton(key: walletControlKey),
      );
    }

    return LayoutBuilder(
      builder: (context, constraints) {
        final connectionStatus = ConnectionStatusIndicator(
          full: true,
          compact: true,
          onTap: onConnectionTap,
        );

        if (shouldStack(constraints.maxWidth)) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              const WalletSwitcherButton(
                key: walletControlKey,
                fullWidth: true,
              ),
              const SizedBox(height: PSpacing.xs),
              Align(
                alignment: Alignment.centerLeft,
                child: KeyedSubtree(
                  key: connectionControlKey,
                  child: connectionStatus,
                ),
              ),
            ],
          );
        }

        return Row(
          children: [
            const WalletSwitcherButton(key: walletControlKey),
            const SizedBox(width: PSpacing.md),
            const Spacer(),
            KeyedSubtree(key: connectionControlKey, child: connectionStatus),
          ],
        );
      },
    );
  }
}
