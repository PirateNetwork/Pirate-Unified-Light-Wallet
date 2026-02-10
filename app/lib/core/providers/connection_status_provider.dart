import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../features/settings/providers/transport_providers.dart';
import '../ffi/generated/models.dart'
    show TunnelMode_I2p, TunnelMode_Socks5, TunnelMode_Tor;
import 'wallet_providers.dart';

enum ConnectionStatusLevel { secure, limited, connecting, offline }

/// Real-time connection status used by status indicators outside the home screen.
///
/// This mirrors the same runtime signals as Home:
/// - Active tunnel mode (Tor/I2P/SOCKS5/Direct)
/// - Tor readiness
/// - Endpoint availability
/// - Sync stream availability
/// - Decoy mode behavior
final connectionStatusLevelProvider = Provider<ConnectionStatusLevel>((ref) {
  final tunnelMode = ref.watch(tunnelModeProvider);
  final torStatus = ref.watch(torStatusProvider);
  final transportConfig = ref.watch(transportConfigProvider);
  final endpointConfigAsync = ref.watch(lightdEndpointConfigProvider);
  final syncStatusAsync = ref.watch(syncProgressStreamProvider);
  final isDecoy = ref.watch(decoyModeProvider);

  final syncStatus = syncStatusAsync.maybeWhen(
    data: (status) => status,
    orElse: () => null,
  );

  final i2pEndpoint = transportConfig.i2pEndpoint.trim();
  final i2pEndpointReady =
      tunnelMode is! TunnelMode_I2p || i2pEndpoint.isNotEmpty;
  final usesPrivacyTunnel =
      tunnelMode is TunnelMode_Tor ||
      tunnelMode is TunnelMode_I2p ||
      tunnelMode is TunnelMode_Socks5;
  final tunnelReady =
      (tunnelMode is! TunnelMode_Tor || torStatus.isReady) && i2pEndpointReady;
  final tunnelError =
      !isDecoy &&
      ((tunnelMode is TunnelMode_Tor && torStatus.status == 'error') ||
          (tunnelMode is TunnelMode_I2p && !i2pEndpointReady));

  // Provider always resolves to a config value when loaded.
  final hasEndpoint = endpointConfigAsync.hasValue;
  final effectiveHasEndpoint =
      isDecoy ||
      (tunnelMode is TunnelMode_I2p ? i2pEndpointReady : hasEndpoint);
  final tunnelBlocked = !isDecoy && usesPrivacyTunnel && !tunnelReady;
  final hasStatus =
      syncStatus != null &&
      (syncStatus.targetHeight > BigInt.zero ||
          syncStatus.localHeight > BigInt.zero);

  if (isDecoy) {
    return usesPrivacyTunnel
        ? ConnectionStatusLevel.secure
        : ConnectionStatusLevel.limited;
  }
  if (!effectiveHasEndpoint) {
    return ConnectionStatusLevel.offline;
  }
  if (tunnelBlocked) {
    return tunnelError
        ? ConnectionStatusLevel.offline
        : ConnectionStatusLevel.connecting;
  }
  if (!hasStatus) {
    return ConnectionStatusLevel.connecting;
  }
  return usesPrivacyTunnel
      ? ConnectionStatusLevel.secure
      : ConnectionStatusLevel.limited;
});
