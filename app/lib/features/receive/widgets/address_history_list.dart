import 'package:flutter/material.dart';
import 'package:timeago/timeago.dart' as timeago;
import '../../../core/ffi/ffi_bridge.dart' show AddressBookColorTag;
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/molecules/p_card.dart';
import '../receive_viewmodel.dart';
import '../../../core/i18n/arb_text_localizer.dart';

/// Widget displaying list of previous addresses
class AddressHistoryList extends StatelessWidget {
  final List<AddressInfo> addresses;
  final bool isFiltered;
  final ValueChanged<AddressInfo> onCopy;
  final ValueChanged<AddressInfo> onLabel;
  final ValueChanged<AddressInfo> onColorTag;
  final ValueChanged<AddressInfo> onOpen;

  const AddressHistoryList({
    super.key,
    required this.addresses,
    this.isFiltered = false,
    required this.onCopy,
    required this.onLabel,
    required this.onColorTag,
    required this.onOpen,
  });

  @override
  Widget build(BuildContext context) {
    if (addresses.isEmpty) {
      final emptyTitle = isFiltered
          ? 'No matches found.'
          : 'No previous addresses.';
      final emptySubtitle = isFiltered
          ? 'Try a different label or address.'
          : 'Generate a new address to see it here.';
      return PCard(
        child: Padding(
          padding: EdgeInsets.all(PSpacing.lg),
          child: Column(
            children: [
              Icon(Icons.history, size: 48, color: AppColors.textTertiary),
              SizedBox(height: PSpacing.sm),
              Text(
                emptyTitle,
                style: PTypography.bodyMedium(color: AppColors.textPrimary),
              ),
              SizedBox(height: PSpacing.xs),
              Text(
                emptySubtitle,
                style: PTypography.bodySmall(color: AppColors.textSecondary),
                textAlign: TextAlign.center,
              ),
            ],
          ),
        ),
      );
    }

    return ListView.separated(
      shrinkWrap: true,
      physics: const NeverScrollableScrollPhysics(),
      itemCount: addresses.length,
      separatorBuilder: (context, index) => SizedBox(height: PSpacing.sm),
      itemBuilder: (context, index) {
        final address = addresses[index];
        return _AddressHistoryItem(
          address: address,
          onOpen: () => onOpen(address),
          onCopy: () => onCopy(address),
          onLabel: () => onLabel(address),
          onColorTag: () => onColorTag(address),
        );
      },
    );
  }
}

/// Sliver version of address history list for lazy rendering
class AddressHistorySliver extends StatelessWidget {
  final List<AddressInfo> addresses;
  final bool isFiltered;
  final ValueChanged<AddressInfo> onCopy;
  final ValueChanged<AddressInfo> onLabel;
  final ValueChanged<AddressInfo> onColorTag;
  final ValueChanged<AddressInfo> onOpen;

  const AddressHistorySliver({
    super.key,
    required this.addresses,
    this.isFiltered = false,
    required this.onCopy,
    required this.onLabel,
    required this.onColorTag,
    required this.onOpen,
  });

  @override
  Widget build(BuildContext context) {
    if (addresses.isEmpty) {
      final emptyTitle = isFiltered
          ? 'No matches found.'
          : 'No previous addresses.';
      final emptySubtitle = isFiltered
          ? 'Try a different label or address.'
          : 'Generate a new address to see it here.';
      return SliverToBoxAdapter(
        child: PCard(
          child: Padding(
            padding: EdgeInsets.all(PSpacing.lg),
            child: Column(
              children: [
                Icon(Icons.history, size: 48, color: AppColors.textTertiary),
                SizedBox(height: PSpacing.sm),
                Text(
                  emptyTitle,
                  style: PTypography.bodyMedium(color: AppColors.textPrimary),
                ),
                SizedBox(height: PSpacing.xs),
                Text(
                  emptySubtitle,
                  style: PTypography.bodySmall(color: AppColors.textSecondary),
                  textAlign: TextAlign.center,
                ),
              ],
            ),
          ),
        ),
      );
    }

    return SliverList(
      delegate: SliverChildBuilderDelegate((context, index) {
        final address = addresses[index];
        return Padding(
          padding: EdgeInsets.only(bottom: PSpacing.sm),
          child: _AddressHistoryItem(
            address: address,
            onOpen: () => onOpen(address),
            onCopy: () => onCopy(address),
            onLabel: () => onLabel(address),
            onColorTag: () => onColorTag(address),
          ),
        );
      }, childCount: addresses.length),
    );
  }
}

/// Individual address history item
class _AddressHistoryItem extends StatelessWidget {
  final AddressInfo address;
  final VoidCallback onOpen;
  final VoidCallback onCopy;
  final VoidCallback onLabel;
  final VoidCallback onColorTag;

  const _AddressHistoryItem({
    required this.address,
    required this.onOpen,
    required this.onCopy,
    required this.onLabel,
    required this.onColorTag,
  });

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final isCompact = constraints.maxWidth < 380;
        final actionButtons = [
          IconButton(
            onPressed: onColorTag,
            icon: Icon(Icons.palette_outlined, size: isCompact ? 18 : 20),
            tooltip: 'Color tag'.tr,
            visualDensity: VisualDensity.compact,
            style: IconButton.styleFrom(
              foregroundColor: AppColors.textSecondary,
              padding: EdgeInsets.zero,
              minimumSize: const Size(32, 32),
            ),
          ),
          IconButton(
            onPressed: onLabel,
            icon: Icon(
              address.label != null ? Icons.edit : Icons.label_outline,
              size: isCompact ? 18 : 20,
            ),
            tooltip: 'Label address'.tr,
            visualDensity: VisualDensity.compact,
            style: IconButton.styleFrom(
              foregroundColor: AppColors.textSecondary,
              padding: EdgeInsets.zero,
              minimumSize: const Size(32, 32),
            ),
          ),
          IconButton(
            onPressed: onCopy,
            icon: Icon(Icons.copy, size: isCompact ? 18 : 20),
            tooltip: 'Copy address'.tr,
            visualDensity: VisualDensity.compact,
            style: IconButton.styleFrom(
              foregroundColor: AppColors.textSecondary,
              padding: EdgeInsets.zero,
              minimumSize: const Size(32, 32),
            ),
          ),
        ];

        final statusBadges = <Widget>[];
        if (address.isActive) {
          statusBadges.add(
            Container(
              padding: const EdgeInsets.symmetric(
                horizontal: PSpacing.xs,
                vertical: 2,
              ),
              decoration: BoxDecoration(
                color: AppColors.successBackground,
                borderRadius: BorderRadius.circular(PSpacing.radiusSM),
                border: Border.all(color: AppColors.successBorder),
              ),
              child: Text(
                'Active'.tr,
                style: PTypography.labelSmall(color: AppColors.success),
              ),
            ),
          );
        }
        if (address.wasShared && !address.isActive) {
          statusBadges.add(
            Container(
              padding: const EdgeInsets.symmetric(
                horizontal: PSpacing.xs,
                vertical: 2,
              ),
              decoration: BoxDecoration(
                color: AppColors.backgroundPanel,
                borderRadius: BorderRadius.circular(PSpacing.radiusSM),
                border: Border.all(color: AppColors.borderSubtle),
              ),
              child: Text(
                'Shared'.tr,
                style: PTypography.labelSmall(color: AppColors.textSecondary),
              ),
            ),
          );
        }

        return PCard(
          onTap: onOpen,
          backgroundColor: address.isActive
              ? AppColors.selectedBackground
              : AppColors.backgroundSurface,
          child: Padding(
            padding: EdgeInsets.all(PSpacing.md),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    // Address Icon
                    Container(
                      width: 40,
                      height: 40,
                      decoration: BoxDecoration(
                        color: address.isActive
                            ? AppColors.focusRing
                            : _resolveColorTag(address),
                        borderRadius: BorderRadius.circular(PSpacing.radiusSM),
                      ),
                      child: Icon(
                        address.isActive
                            ? Icons.check_circle
                            : Icons.shield_outlined,
                        color: AppColors.textOnAccent,
                        size: 20,
                      ),
                    ),
                    SizedBox(width: PSpacing.md),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          // Label or truncated address
                          Text(
                            address.label ?? _truncateAddress(address.address),
                            maxLines: isCompact ? 2 : 1,
                            overflow: TextOverflow.ellipsis,
                            style: PTypography.bodyMedium().copyWith(
                              fontWeight: FontWeight.w600,
                              color: address.isActive
                                  ? AppColors.focusRing
                                  : AppColors.textPrimary,
                            ),
                          ),
                          SizedBox(height: PSpacing.xs),
                          Wrap(
                            spacing: PSpacing.sm,
                            runSpacing: PSpacing.xs,
                            crossAxisAlignment: WrapCrossAlignment.center,
                            children: [
                              Row(
                                mainAxisSize: MainAxisSize.min,
                                children: [
                                  Icon(
                                    Icons.access_time,
                                    size: 12,
                                    color: AppColors.textTertiary,
                                  ),
                                  SizedBox(width: PSpacing.xs),
                                  Text(
                                    _formatTimestamp(address.createdAt),
                                    style: PTypography.bodySmall().copyWith(
                                      color: AppColors.textSecondary,
                                    ),
                                  ),
                                ],
                              ),
                              ...statusBadges,
                            ],
                          ),
                          SizedBox(height: PSpacing.xs),
                          Wrap(
                            spacing: PSpacing.sm,
                            runSpacing: PSpacing.xs,
                            crossAxisAlignment: WrapCrossAlignment.center,
                            children: [
                              Row(
                                mainAxisSize: MainAxisSize.min,
                                children: [
                                  Icon(
                                    Icons.account_balance_wallet_outlined,
                                    size: 12,
                                    color: AppColors.textTertiary,
                                  ),
                                  SizedBox(width: PSpacing.xs),
                                  Text(
                                    'Balance ${_formatArrr(address.balance)}',
                                    style: PTypography.bodySmall().copyWith(
                                      color: AppColors.textSecondary,
                                    ),
                                  ),
                                ],
                              ),
                              if (address.pending > BigInt.zero)
                                Text(
                                  'Pending ${_formatArrr(address.pending)}',
                                  style: PTypography.bodySmall().copyWith(
                                    color: AppColors.warning,
                                  ),
                                ),
                            ],
                          ),
                          if (address.label != null &&
                              address.label!.isNotEmpty)
                            Padding(
                              padding: EdgeInsets.only(top: PSpacing.xs),
                              child: Text(
                                address.truncatedAddress,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: PTypography.codeSmall(
                                  color: AppColors.textTertiary,
                                ),
                              ),
                            ),
                        ],
                      ),
                    ),
                    if (!isCompact)
                      Row(
                        mainAxisSize: MainAxisSize.min,
                        children: actionButtons,
                      ),
                  ],
                ),
                if (isCompact) ...[
                  SizedBox(height: PSpacing.sm),
                  Align(
                    alignment: Alignment.centerRight,
                    child: Wrap(
                      spacing: PSpacing.xs,
                      runSpacing: PSpacing.xs,
                      children: actionButtons,
                    ),
                  ),
                ],
              ],
            ),
          ),
        );
      },
    );
  }

  String _truncateAddress(String address) {
    if (address.length <= 20) return address;
    return '${address.substring(0, 10)}...${address.substring(address.length - 10)}';
  }

  String _formatTimestamp(DateTime timestamp) {
    if (timestamp.millisecondsSinceEpoch <= 0) {
      return 'Unknown';
    }
    return timeago.format(timestamp);
  }

  Color _resolveColorTag(AddressInfo address) {
    if (address.colorTag == AddressBookColorTag.none) {
      return AppColors.backgroundPanel;
    }
    return Color(address.colorTag.colorValue);
  }

  String _formatArrr(BigInt value) {
    final amount = value.toDouble() / 100000000.0;
    return '${amount.toStringAsFixed(8)} ARRR';
  }
}
