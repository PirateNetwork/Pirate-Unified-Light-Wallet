import 'package:flutter/material.dart';
import 'package:timeago/timeago.dart' as timeago;
import '../../../core/ffi/ffi_bridge.dart' show AddressBookColorTag;
import '../../../design/tokens/colors.dart';
import '../../../design/tokens/spacing.dart';
import '../../../design/tokens/typography.dart';
import '../../../ui/molecules/p_card.dart';
import '../receive_viewmodel.dart';

/// Widget displaying list of previous addresses
class AddressHistoryList extends StatelessWidget {
  final List<AddressInfo> addresses;
  final Function(AddressInfo) onCopy;
  final Function(AddressInfo) onLabel;
  final Function(AddressInfo) onColorTag;
  final Function(AddressInfo) onOpen;

  const AddressHistoryList({
    super.key,
    required this.addresses,
    required this.onCopy,
    required this.onLabel,
    required this.onColorTag,
    required this.onOpen,
  });

  @override
  Widget build(BuildContext context) {
    if (addresses.isEmpty) {
      return PCard(
        child: Padding(
          padding: EdgeInsets.all(PSpacing.lg),
          child: Column(
            children: [
              Icon(
                Icons.history,
                size: 48,
                color: AppColors.textTertiary,
              ),
              SizedBox(height: PSpacing.sm),
              Text(
                'No previous addresses.',
                style: PTypography.bodyMedium(
                  color: AppColors.textPrimary,
                ),
              ),
              SizedBox(height: PSpacing.xs),
              Text(
                'Generate a new address to see it here.',
                style: PTypography.bodySmall(
                  color: AppColors.textSecondary,
                ),
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
    return PCard(
      onTap: onOpen,
      backgroundColor: address.isActive
          ? AppColors.selectedBackground
          : AppColors.backgroundSurface,
      child: Padding(
        padding: EdgeInsets.all(PSpacing.md),
        child: Row(
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
                address.isActive ? Icons.check_circle : Icons.shield_outlined,
                color: AppColors.textOnAccent,
                size: 20,
              ),
            ),

            SizedBox(width: PSpacing.md),

            // Address Info
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  // Label or truncated address
                  Text(
                    address.label ?? _truncateAddress(address.address),
                    style: PTypography.bodyMedium().copyWith(
                      fontWeight: FontWeight.w600,
                      color:
                          address.isActive ? AppColors.focusRing : AppColors.textPrimary,
                    ),
                  ),

                  SizedBox(height: PSpacing.xs),

                  // Timestamp and status badges
                  Row(
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
                      if (address.isActive) ...[
                        SizedBox(width: PSpacing.sm),
                        Container(
                          padding: EdgeInsets.symmetric(
                            horizontal: PSpacing.xs,
                            vertical: 2,
                          ),
                          decoration: BoxDecoration(
                            color: AppColors.successBackground,
                            borderRadius: BorderRadius.circular(PSpacing.radiusSM),
                            border: Border.all(color: AppColors.successBorder),
                          ),
                          child: Text(
                            'Active',
                            style: PTypography.labelSmall(
                              color: AppColors.success,
                            ),
                          ),
                        ),
                      ],
                      if (address.wasShared && !address.isActive) ...[
                        SizedBox(width: PSpacing.sm),
                        Container(
                          padding: EdgeInsets.symmetric(
                            horizontal: PSpacing.xs,
                            vertical: 2,
                          ),
                          decoration: BoxDecoration(
                            color: AppColors.backgroundPanel,
                            borderRadius: BorderRadius.circular(PSpacing.radiusSM),
                            border: Border.all(color: AppColors.borderSubtle),
                          ),
                          child: Text(
                            'Shared',
                            style: PTypography.labelSmall(
                              color: AppColors.textSecondary,
                            ),
                          ),
                        ),
                      ],
                    ],
                  ),
                  
                  // Show label if present (below truncated address)
                  if (address.label != null && address.label!.isNotEmpty)
                    Padding(
                      padding: EdgeInsets.only(top: PSpacing.xs),
                      child: Text(
                        address.truncatedAddress,
                        style: PTypography.codeSmall(
                          color: AppColors.textTertiary,
                        ),
                      ),
                    ),
                ],
              ),
            ),

            // Actions
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                IconButton(
                  onPressed: onColorTag,
                  icon: const Icon(Icons.palette_outlined, size: 20),
                  tooltip: 'Color tag',
                  style: IconButton.styleFrom(
                    foregroundColor: AppColors.textSecondary,
                  ),
                ),
                IconButton(
                  onPressed: onLabel,
                  icon: Icon(
                    address.label != null ? Icons.edit : Icons.label_outline,
                    size: 20,
                  ),
                  tooltip: 'Label address',
                  style: IconButton.styleFrom(
                    foregroundColor: AppColors.textSecondary,
                  ),
                ),
                IconButton(
                  onPressed: onCopy,
                  icon: const Icon(Icons.copy, size: 20),
                  tooltip: 'Copy address',
                  style: IconButton.styleFrom(
                    foregroundColor: AppColors.textSecondary,
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
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
}
