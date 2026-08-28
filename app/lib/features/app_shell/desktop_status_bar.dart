import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/ffi/ffi_bridge.dart';
import '../../core/i18n/arb_text_localizer.dart';
import '../../core/providers/connection_status_provider.dart';
import '../../core/providers/wallet_providers.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/atoms/p_icon_button.dart';
import '../../ui/atoms/theme_toggle_button.dart';
import '../settings/providers/transport_providers.dart';

class DesktopStatusBar extends ConsumerWidget {
  const DesktopStatusBar({
    required this.settingsSelected,
    required this.onSettingsTap,
    required this.onConnectionTap,
    super.key,
  });

  static const barKey = Key('desktop-status-bar');
  static const settingsKey = Key('desktop-status-settings');
  static const connectionKey = Key('desktop-status-connection');
  static const transportKey = Key('desktop-status-transport');
  static const progressKey = Key('desktop-status-progress');

  final bool settingsSelected;
  final VoidCallback onSettingsTap;
  final VoidCallback onConnectionTap;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final connectionLevel = ref.watch(connectionStatusLevelProvider);
    final transportMode = ref.watch(transportConfigProvider).mode;
    final syncStatus = ref.watch(syncProgressStreamProvider).asData?.value;
    final isDecoy = ref.watch(decoyModeProvider);
    final decoyHeight = isDecoy
        ? ref
              .watch(decoySyncHeightProvider)
              .maybeWhen(data: (height) => height, orElse: () => 0)
        : 0;

    final complete = isDecoy
        ? decoyHeight > 0
        : (syncStatus?.isComplete ?? false);
    final percent = complete
        ? 100.0
        : (syncStatus?.percent ?? 0.0).clamp(0.0, 99.9);
    final syncLabel = complete
        ? 'Synced'.tr
        : syncStatus?.stageName ?? 'Not synced'.tr;

    return Container(
      key: barKey,
      height: PSpacing.desktopStatusBarHeight,
      decoration: BoxDecoration(
        color: AppColors.backgroundSurface,
        border: Border(top: BorderSide(color: AppColors.borderSubtle)),
      ),
      child: Row(
        children: [
          _UtilityArea(
            settingsSelected: settingsSelected,
            onSettingsTap: onSettingsTap,
          ),
          Expanded(
            child: _RuntimeStatus(
              connectionLevel: connectionLevel,
              transportLabel: _transportLabel(transportMode),
              syncLabel: syncLabel,
              percent: percent,
              onConnectionTap: onConnectionTap,
            ),
          ),
        ],
      ),
    );
  }

  String _transportLabel(String mode) {
    return switch (mode.trim().toLowerCase()) {
      'tor' => 'Tor',
      'i2p' => 'I2P',
      'socks5' => 'SOCKS5',
      _ => 'Direct',
    };
  }
}

class _UtilityArea extends StatelessWidget {
  const _UtilityArea({
    required this.settingsSelected,
    required this.onSettingsTap,
  });

  final bool settingsSelected;
  final VoidCallback onSettingsTap;

  @override
  Widget build(BuildContext context) {
    final reduceMotion =
        MediaQuery.maybeOf(context)?.disableAnimations ?? false;
    return Container(
      width: PSpacing.desktopNavRailWidth,
      decoration: BoxDecoration(
        border: Border(right: BorderSide(color: AppColors.borderSubtle)),
      ),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Semantics(
            button: true,
            selected: settingsSelected,
            child: AnimatedContainer(
              key: DesktopStatusBar.settingsKey,
              duration: reduceMotion
                  ? Duration.zero
                  : const Duration(milliseconds: 150),
              width: PIconButtonSize.compact.size,
              height: PIconButtonSize.compact.size,
              clipBehavior: Clip.antiAlias,
              decoration: BoxDecoration(
                color: settingsSelected
                    ? AppColors.selectedBackground
                    : Colors.transparent,
                borderRadius: BorderRadius.circular(PSpacing.radiusSM),
              ),
              child: PIconButton(
                icon: Icon(
                  settingsSelected ? Icons.settings : Icons.settings_outlined,
                  color: settingsSelected
                      ? AppColors.focusRing
                      : AppColors.textSecondary,
                ),
                onPressed: onSettingsTap,
                tooltip: 'Settings'.tr,
                size: PIconButtonSize.compact,
              ),
            ),
          ),
          const SizedBox(width: PSpacing.xxs),
          const ThemeToggleButton(size: PIconButtonSize.compact),
        ],
      ),
    );
  }
}

class _RuntimeStatus extends StatelessWidget {
  const _RuntimeStatus({
    required this.connectionLevel,
    required this.transportLabel,
    required this.syncLabel,
    required this.percent,
    required this.onConnectionTap,
  });

  final ConnectionStatusLevel connectionLevel;
  final String transportLabel;
  final String syncLabel;
  final double percent;
  final VoidCallback onConnectionTap;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final showTransport = constraints.maxWidth >= 560;
        return Padding(
          padding: const EdgeInsets.symmetric(horizontal: PSpacing.sm),
          child: Row(
            children: [
              SizedBox(
                width: showTransport ? 168 : 132,
                child: _ConnectionSummary(
                  level: connectionLevel,
                  onTap: onConnectionTap,
                ),
              ),
              if (showTransport) ...[
                const SizedBox(width: PSpacing.sm),
                SizedBox(
                  height: 16,
                  child: VerticalDivider(
                    width: 1,
                    thickness: 1,
                    color: AppColors.borderDefault,
                  ),
                ),
                const SizedBox(width: PSpacing.sm),
                Tooltip(
                  message: transportLabel,
                  child: Text(
                    transportLabel,
                    key: DesktopStatusBar.transportKey,
                    maxLines: 1,
                    style: PTypography.caption(
                      color: AppColors.textTertiary,
                    ).copyWith(fontSize: 11),
                  ),
                ),
                const SizedBox(width: PSpacing.lg),
              ] else
                const SizedBox(width: PSpacing.sm),
              Expanded(
                child: _SyncSummary(label: syncLabel, percent: percent),
              ),
            ],
          ),
        );
      },
    );
  }
}

class _ConnectionSummary extends StatelessWidget {
  const _ConnectionSummary({required this.level, required this.onTap});

  final ConnectionStatusLevel level;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final label = switch (level) {
      ConnectionStatusLevel.secure => 'Connected - Secure'.tr,
      ConnectionStatusLevel.limited => 'Connected - Limited'.tr,
      ConnectionStatusLevel.connecting => 'Connecting'.tr,
      ConnectionStatusLevel.offline => 'Offline'.tr,
    };
    final color = switch (level) {
      ConnectionStatusLevel.secure => AppColors.success,
      ConnectionStatusLevel.limited => AppColors.highlight,
      ConnectionStatusLevel.connecting => AppColors.info,
      ConnectionStatusLevel.offline => AppColors.error,
    };

    return Tooltip(
      message: 'Network Privacy'.tr,
      child: InkWell(
        key: DesktopStatusBar.connectionKey,
        onTap: onTap,
        borderRadius: BorderRadius.circular(PSpacing.radiusXS),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: PSpacing.xxs),
          child: Row(
            children: [
              Container(
                width: 7,
                height: 7,
                decoration: BoxDecoration(color: color, shape: BoxShape.circle),
              ),
              const SizedBox(width: PSpacing.xs),
              Expanded(
                child: Text(
                  label,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: PTypography.caption(
                    color: AppColors.textSecondary,
                  ).copyWith(fontSize: 11),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _SyncSummary extends StatelessWidget {
  const _SyncSummary({required this.label, required this.percent});

  final String label;
  final double percent;

  @override
  Widget build(BuildContext context) {
    final percentLabel = '${percent.round()}%';
    return Row(
      children: [
        SizedBox(
          width: 116,
          child: Text(
            label,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: PTypography.caption(
              color: AppColors.textTertiary,
            ).copyWith(fontSize: 11),
          ),
        ),
        const SizedBox(width: PSpacing.sm),
        Expanded(
          child: ClipRRect(
            borderRadius: BorderRadius.circular(PSpacing.radiusFull),
            child: LinearProgressIndicator(
              key: DesktopStatusBar.progressKey,
              value: percent / 100,
              minHeight: 3,
              color: AppColors.focusRing,
              backgroundColor: AppColors.backgroundElevated,
              semanticsLabel: label,
              semanticsValue: percentLabel,
            ),
          ),
        ),
        const SizedBox(width: PSpacing.sm),
        SizedBox(
          width: 40,
          child: Text(
            percentLabel,
            textAlign: TextAlign.end,
            maxLines: 1,
            style: PTypography.caption(
              color: AppColors.textSecondary,
            ).copyWith(fontSize: 11),
          ),
        ),
      ],
    );
  }
}
