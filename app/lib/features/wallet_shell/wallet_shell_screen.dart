// Wallet Shell Screen - Main app shell with FFI integration
//
// Demonstrates the complete FFI wiring with Riverpod providers.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/ffi/generated/models.dart'
    show
        TunnelMode,
        TunnelMode_Tor,
        TunnelMode_I2p,
        TunnelMode_Socks5,
        TunnelMode_Direct,
        SyncMode;
import '../../core/providers/wallet_providers.dart';
import '../../design/tokens/spacing.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/i18n/arb_text_localizer.dart';

class WalletShellScreen extends ConsumerStatefulWidget {
  const WalletShellScreen({super.key});

  @override
  ConsumerState<WalletShellScreen> createState() => _WalletShellScreenState();
}

class _WalletShellScreenState extends ConsumerState<WalletShellScreen> {
  @override
  Widget build(BuildContext context) {
    final padding = PSpacing.screenPadding(MediaQuery.of(context).size.width);
    final activeWallet = ref.watch(activeWalletProvider);
    final balanceAsync = ref.watch(balanceStreamProvider);
    final syncStatusAsync = ref.watch(syncStatusProvider);
    final tunnelMode = ref.watch(tunnelModeProvider);

    return PScaffold(
      title: 'Pirate Wallet'.tr,
      body: ListView(
        padding: padding,
        children: [
          // Session Info
          PCard(
            child: Padding(
              padding: const EdgeInsets.all(PSpacing.md),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Session Info'.tr,
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                  const SizedBox(height: PSpacing.sm),
                  _buildInfoRow(
                    'Active Wallet',
                    activeWallet?.toString() ?? 'None',
                  ),
                  _buildInfoRow(
                    'Tunnel Mode',
                    _getTunnelModeDisplayName(tunnelMode),
                  ),
                ],
              ),
            ),
          ),

          const SizedBox(height: PSpacing.md),

          // Balance
          if (activeWallet != null)
            PCard(
              child: Padding(
                padding: const EdgeInsets.all(PSpacing.md),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Balance'.tr,
                      style: Theme.of(context).textTheme.titleLarge,
                    ),
                    const SizedBox(height: PSpacing.sm),
                    balanceAsync.when(
                      data: (balance) {
                        if (balance == null) {
                          return Text('No balance data'.tr);
                        }
                        return Column(
                          children: [
                            _buildInfoRow('Total', _formatArrr(balance.total)),
                            _buildInfoRow(
                              'Spendable',
                              _formatArrr(balance.spendable),
                            ),
                            _buildInfoRow(
                              'Pending',
                              _formatArrr(balance.pending),
                            ),
                          ],
                        );
                      },
                      loading: () => const CircularProgressIndicator(),
                      error: (err, stack) => Text('Error: $err'),
                    ),
                  ],
                ),
              ),
            ),

          const SizedBox(height: PSpacing.md),

          // Sync Status
          if (activeWallet != null)
            PCard(
              child: Padding(
                padding: const EdgeInsets.all(PSpacing.md),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Sync Status'.tr,
                      style: Theme.of(context).textTheme.titleLarge,
                    ),
                    const SizedBox(height: PSpacing.sm),
                    syncStatusAsync.when(
                      data: (status) {
                        if (status == null) {
                          return Text('Not syncing'.tr);
                        }
                        return Column(
                          children: [
                            _buildInfoRow(
                              'Progress',
                              '${status.percent.toStringAsFixed(1)}%',
                            ),
                            _buildInfoRow(
                              'Height',
                              '${status.localHeight} / ${status.targetHeight}',
                            ),
                            _buildInfoRow(
                              'Stage',
                              status.stage.name.toUpperCase(),
                            ),
                            if (status.eta != null)
                              _buildInfoRow(
                                'ETA',
                                '${status.eta!.toInt() ~/ 60}m ${status.eta!.toInt() % 60}s',
                              ),
                          ],
                        );
                      },
                      loading: () => const CircularProgressIndicator(),
                      error: (err, stack) => Text('Error: $err'),
                    ),
                  ],
                ),
              ),
            ),

          const SizedBox(height: PSpacing.md),

          // Actions
          PCard(
            child: Padding(
              padding: const EdgeInsets.all(PSpacing.md),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(
                    'Actions'.tr,
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                  const SizedBox(height: PSpacing.md),
                  if (activeWallet == null) ...[
                    PButton(
                      onPressed: _createWallet,
                      variant: PButtonVariant.primary,
                      child: Text('Create Wallet'.tr),
                    ),
                    const SizedBox(height: PSpacing.sm),
                    PButton(
                      onPressed: _restoreWallet,
                      variant: PButtonVariant.secondary,
                      child: Text('Restore Wallet'.tr),
                    ),
                  ] else ...[
                    PButton(
                      onPressed: () => _startSync(SyncMode.compact),
                      variant: PButtonVariant.primary,
                      child: Text('Start Sync (Compact)'.tr),
                    ),
                    const SizedBox(height: PSpacing.sm),
                    PButton(
                      onPressed: () => _startSync(SyncMode.deep),
                      variant: PButtonVariant.secondary,
                      child: Text('Start Sync (Deep)'.tr),
                    ),
                    const SizedBox(height: PSpacing.sm),
                    PButton(
                      onPressed: _generateAddress,
                      variant: PButtonVariant.outline,
                      child: Text('Generate New Address'.tr),
                    ),
                    const SizedBox(height: PSpacing.sm),
                    PButton(
                      onPressed: _switchTunnelMode,
                      variant: PButtonVariant.ghost,
                      child: Text('Switch Tunnel Mode'.tr),
                    ),
                  ],
                ],
              ),
            ),
          ),

          const SizedBox(height: PSpacing.md),

          // Network Info
          Consumer(
            builder: (context, ref, _) {
              final networkInfoAsync = ref.watch(networkInfoProvider);
              return PCard(
                child: Padding(
                  padding: const EdgeInsets.all(PSpacing.md),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        'Network Info'.tr,
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                      const SizedBox(height: PSpacing.sm),
                      networkInfoAsync.when(
                        data: (info) => Column(
                          children: [
                            _buildInfoRow('Network', info.name),
                            _buildInfoRow(
                              'Coin Type',
                              info.coinType.toString(),
                            ),
                            _buildInfoRow('RPC Port', info.rpcPort.toString()),
                            _buildInfoRow(
                              'Default Birthday',
                              info.defaultBirthday.toString(),
                            ),
                          ],
                        ),
                        loading: () => const CircularProgressIndicator(),
                        error: (err, stack) => Text('Error: $err'),
                      ),
                    ],
                  ),
                ),
              );
            },
          ),
        ],
      ),
    );
  }

  String _getTunnelModeDisplayName(TunnelMode mode) {
    return switch (mode) {
      TunnelMode_Tor() => 'Tor',
      TunnelMode_I2p() => 'I2P',
      TunnelMode_Socks5(:final url) => 'SOCKS5 ($url)',
      TunnelMode_Direct() => 'Direct',
    };
  }

  Widget _buildInfoRow(String label, String value) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: PSpacing.xs),
      child: Row(
        children: [
          Expanded(
            child: Text(
              label,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                color: Theme.of(
                  context,
                ).colorScheme.onSurface.withValues(alpha: 0.7),
              ),
            ),
          ),
          const SizedBox(width: PSpacing.sm),
          Expanded(
            child: Text(
              value,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              textAlign: TextAlign.right,
              style: Theme.of(
                context,
              ).textTheme.bodyMedium?.copyWith(fontWeight: FontWeight.w600),
            ),
          ),
        ],
      ),
    );
  }

  String _formatArrr(BigInt arrrtoshis) {
    return '${(arrrtoshis.toDouble() / 100000000).toStringAsFixed(8)} ARRR';
  }

  Future<void> _createWallet() async {
    try {
      final createWallet = ref.read(createWalletProvider);
      final walletId = await createWallet(name: 'My Wallet', entropyLen: 256);

      if (!mounted) return;

      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Wallet created: $walletId'),
          backgroundColor: Colors.green,
        ),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Error: $e'), backgroundColor: Colors.red),
      );
    }
  }

  Future<void> _restoreWallet() async {
    try {
      final generateMnemonic = ref.read(generateMnemonicProvider);
      final mnemonic = await generateMnemonic(wordCount: 24);

      final restoreWallet = ref.read(restoreWalletProvider);
      final walletId = await restoreWallet(
        name: 'Restored Wallet',
        mnemonic: mnemonic,
      );

      if (!mounted) return;

      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Wallet restored: $walletId'),
          backgroundColor: Colors.green,
        ),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Error: $e'), backgroundColor: Colors.red),
      );
    }
  }

  Future<void> _startSync(SyncMode mode) async {
    try {
      final startSync = ref.read(startSyncProvider);
      await startSync(mode);

      if (!mounted) return;

      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Sync started: ${mode.name}'),
          backgroundColor: Colors.green,
        ),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Error: $e'), backgroundColor: Colors.red),
      );
    }
  }

  Future<void> _generateAddress() async {
    try {
      final generateAddress = ref.read(generateAddressProvider);
      final address = await generateAddress();

      if (!mounted) return;

      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('New address: ${address.substring(0, 20)}...'),
          backgroundColor: Colors.green,
        ),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Error: $e'), backgroundColor: Colors.red),
      );
    }
  }

  void _switchTunnelMode() {
    final current = ref.read(tunnelModeProvider);
    final next = switch (current) {
      TunnelMode_Tor() => const TunnelMode.i2P(),
      TunnelMode_I2p() => const TunnelMode.direct(),
      TunnelMode_Direct() => const TunnelMode.socks5(
        url: 'socks5://127.0.0.1:1080',
      ),
      TunnelMode_Socks5() => const TunnelMode.tor(),
    };

    final socksUrl = switch (next) {
      TunnelMode_Socks5(:final url) => url,
      _ => null,
    };

    ref
        .read(tunnelModeProvider.notifier)
        .setTunnelMode(next, socksUrl: socksUrl);

    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text('Tunnel mode: ${_getTunnelModeDisplayName(next)}'),
      ),
    );
  }
}
