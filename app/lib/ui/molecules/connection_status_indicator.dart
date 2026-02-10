import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/providers/connection_status_provider.dart';
import 'privacy_status_chip.dart';

class ConnectionStatusIndicator extends ConsumerWidget {
  const ConnectionStatusIndicator({
    this.full = true,
    this.compact = true,
    this.onTap,
    super.key,
  });

  final bool full;
  final bool compact;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final statusLevel = ref.watch(connectionStatusLevelProvider);
    final status = switch (statusLevel) {
      ConnectionStatusLevel.secure => PrivacyStatus.private,
      ConnectionStatusLevel.limited => PrivacyStatus.limited,
      ConnectionStatusLevel.connecting => PrivacyStatus.connecting,
      ConnectionStatusLevel.offline => PrivacyStatus.offline,
    };

    return PrivacyStatusChip(
      status: status,
      compact: compact,
      dotOnly: !full,
      onTap: onTap,
    );
  }
}
