import 'package:flutter/material.dart';
import 'package:timeago/timeago.dart' as timeago;
import '../../../core/ffi/ffi_bridge.dart' show AddressBookColorTag;
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/molecules/p_card.dart';
import '../receive_viewmodel.dart';
import '../../../core/i18n/arb_text_localizer.dart';

/// Sliver version of address history list for lazy rendering
class AddressHistorySliver extends StatelessWidget {
  final List<AddressInfo> addresses;
  final bool isFiltered;
  final bool showArchived;
  final ValueChanged<AddressInfo> onCopy;
  final ValueChanged<AddressInfo> onLabel;
  final ValueChanged<AddressInfo> onColorTag;
  final ValueChanged<AddressInfo> onTogglePin;
  final ValueChanged<AddressInfo> onArchive;
  final ValueChanged<AddressInfo> onOpen;

  const AddressHistorySliver({
    super.key,
    required this.addresses,
    this.isFiltered = false,
    this.showArchived = false,
    required this.onCopy,
    required this.onLabel,
    required this.onColorTag,
    required this.onTogglePin,
    required this.onArchive,
    required this.onOpen,
  });

  @override
  Widget build(BuildContext context) {
    if (addresses.isEmpty) {
      final emptyTitle = isFiltered
          ? 'No matches found.'.tr
          : showArchived
          ? 'No archived addresses.'.tr
          : 'No addresses yet.'.tr;
      final emptySubtitle = isFiltered
          ? 'Try a different label or address.'.tr
          : showArchived
          ? 'Addresses you archive will appear here.'.tr
          : 'Generate a new address to see it here.'.tr;
      return SliverToBoxAdapter(
        child: PCard(
          child: Padding(
            padding: EdgeInsets.all(PSpacing.lg),
            child: Column(
              children: [
                Icon(
                  showArchived ? Icons.archive_outlined : Icons.history,
                  size: 48,
                  color: AppColors.textTertiary,
                ),
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
            onTogglePin: () => onTogglePin(address),
            onArchive: () => onArchive(address),
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
  final VoidCallback onTogglePin;
  final VoidCallback onArchive;

  const _AddressHistoryItem({
    required this.address,
    required this.onOpen,
    required this.onCopy,
    required this.onLabel,
    required this.onColorTag,
    required this.onTogglePin,
    required this.onArchive,
  });

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final isCompact = constraints.maxWidth < 380;
        final actionButtons = <Widget>[
          if (!address.isArchived)
            IconButton(
              onPressed: address.addressId == null ? null : onTogglePin,
              icon: Icon(
                address.isPinned ? Icons.push_pin : Icons.push_pin_outlined,
                size: isCompact ? 18 : 20,
              ),
              tooltip: address.isPinned ? 'Unpin address'.tr : 'Pin address'.tr,
              visualDensity: VisualDensity.standard,
              style: IconButton.styleFrom(
                foregroundColor: address.isPinned
                    ? AppColors.focusRing
                    : AppColors.textSecondary,
                padding: EdgeInsets.zero,
                minimumSize: const Size.square(44),
              ),
            ),
          IconButton(
            onPressed: onCopy,
            icon: Icon(Icons.copy, size: isCompact ? 18 : 20),
            tooltip: 'Copy address'.tr,
            visualDensity: VisualDensity.standard,
            style: IconButton.styleFrom(
              foregroundColor: AppColors.textSecondary,
              padding: EdgeInsets.zero,
              minimumSize: const Size.square(44),
            ),
          ),
          PopupMenuButton<_AddressHistoryAction>(
            tooltip: 'More actions'.tr,
            icon: Icon(Icons.more_vert, size: isCompact ? 18 : 20),
            constraints: const BoxConstraints(minWidth: 44, minHeight: 44),
            padding: EdgeInsets.zero,
            color: AppColors.backgroundElevated,
            onSelected: (action) {
              switch (action) {
                case _AddressHistoryAction.label:
                  onLabel();
                  break;
                case _AddressHistoryAction.color:
                  onColorTag();
                  break;
                case _AddressHistoryAction.archive:
                  onArchive();
                  break;
              }
            },
            itemBuilder: (context) => [
              PopupMenuItem(
                value: _AddressHistoryAction.label,
                child: ListTile(
                  dense: true,
                  contentPadding: EdgeInsets.zero,
                  leading: Icon(
                    address.label != null ? Icons.edit : Icons.label_outline,
                  ),
                  title: Text('Label address'.tr),
                ),
              ),
              PopupMenuItem(
                value: _AddressHistoryAction.color,
                child: ListTile(
                  dense: true,
                  contentPadding: EdgeInsets.zero,
                  leading: const Icon(Icons.palette_outlined),
                  title: Text('Color tag'.tr),
                ),
              ),
              const PopupMenuDivider(),
              if (!address.isActive)
                PopupMenuItem(
                  value: _AddressHistoryAction.archive,
                  enabled: address.addressId != null,
                  child: ListTile(
                    dense: true,
                    contentPadding: EdgeInsets.zero,
                    leading: Icon(
                      address.isArchived
                          ? Icons.unarchive_outlined
                          : Icons.archive_outlined,
                    ),
                    title: Text(
                      address.isArchived
                          ? 'Restore address'.tr
                          : 'Archive address'.tr,
                    ),
                  ),
                ),
            ],
          ),
        ];

        final statusBadges = <Widget>[];
        if (address.isActive) {
          statusBadges.add(
            _AddressStatusBadge(
              label: 'Active'.tr,
              foreground: AppColors.success,
              background: AppColors.successBackground,
              border: AppColors.successBorder,
            ),
          );
        }
        if (address.isPinned) {
          statusBadges.add(
            _AddressStatusBadge(
              label: 'Pinned'.tr,
              icon: Icons.push_pin,
              foreground: AppColors.focusRing,
              background: AppColors.selectedBackground,
              border: AppColors.selectedBorder,
            ),
          );
        }
        if (address.isArchived) {
          statusBadges.add(
            _AddressStatusBadge(
              label: 'Archived'.tr,
              icon: Icons.archive_outlined,
              foreground: AppColors.textSecondary,
              background: AppColors.backgroundPanel,
              border: AppColors.borderSubtle,
            ),
          );
        }
        if (address.wasShared && !address.isActive) {
          statusBadges.add(
            _AddressStatusBadge(
              label: 'Shared'.tr,
              foreground: AppColors.textSecondary,
              background: AppColors.backgroundPanel,
              border: AppColors.borderSubtle,
            ),
          );
        }

        return PCard(
          key: ValueKey(
            'address-history-${address.addressId ?? address.address}',
          ),
          onTap: onOpen,
          backgroundColor: address.isActive
              ? AppColors.selectedBackground
              : address.isArchived
              ? AppColors.backgroundPanel
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
                            : address.isArchived
                            ? Icons.archive_outlined
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
                                    'Balance {balance}'.trArgs({
                                      'balance': _formatArrr(address.balance),
                                    }),
                                    style: PTypography.bodySmall().copyWith(
                                      color: AppColors.textSecondary,
                                    ),
                                  ),
                                ],
                              ),
                              if (address.pending > BigInt.zero)
                                Text(
                                  'Pending {amount}'.trArgs({
                                    'amount': _formatArrr(address.pending),
                                  }),
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

enum _AddressHistoryAction { label, color, archive }

class _AddressStatusBadge extends StatelessWidget {
  const _AddressStatusBadge({
    required this.label,
    required this.foreground,
    required this.background,
    required this.border,
    this.icon,
  });

  final String label;
  final Color foreground;
  final Color background;
  final Color border;
  final IconData? icon;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: PSpacing.xs, vertical: 2),
      decoration: BoxDecoration(
        color: background,
        borderRadius: BorderRadius.circular(PSpacing.radiusSM),
        border: Border.all(color: border),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (icon != null) ...[
            Icon(icon, size: 11, color: foreground),
            const SizedBox(width: 3),
          ],
          Text(label, style: PTypography.labelSmall(color: foreground)),
        ],
      ),
    );
  }
}
