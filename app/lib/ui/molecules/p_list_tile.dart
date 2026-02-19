import 'package:flutter/material.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Pirate Wallet List Tile
class PListTile extends StatefulWidget {
  const PListTile({
    required this.title,
    this.subtitle,
    this.leading,
    this.trailing,
    this.onTap,
    this.enabled = true,
    super.key,
  });

  final String title;
  final String? subtitle;
  final Widget? leading;
  final Widget? trailing;
  final VoidCallback? onTap;
  final bool enabled;

  @override
  State<PListTile> createState() => _PListTileState();
}

class _PListTileState extends State<PListTile> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      cursor: widget.onTap != null && widget.enabled
          ? SystemMouseCursors.click
          : SystemMouseCursors.basic,
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 150),
        decoration: BoxDecoration(
          color: _isHovered && widget.onTap != null && widget.enabled
              ? AppColors.hoverOverlay
              : Colors.transparent,
          borderRadius: BorderRadius.circular(PSpacing.radiusMD),
        ),
        child: Material(
          color: Colors.transparent,
          child: InkWell(
            onTap: widget.enabled ? widget.onTap : null,
            borderRadius: BorderRadius.circular(PSpacing.radiusMD),
            child: Padding(
              padding: EdgeInsets.symmetric(
                horizontal: PSpacing.listItemPaddingHorizontal,
                vertical: PSpacing.listItemPaddingVertical,
              ),
              child: Row(
                children: [
                  if (widget.leading != null) ...[
                    IconTheme(
                      data: IconThemeData(
                        color: widget.enabled
                            ? AppColors.textSecondary
                            : AppColors.textDisabled,
                        size: PSpacing.iconLG,
                      ),
                      child: widget.leading!,
                    ),
                    SizedBox(width: PSpacing.md),
                  ],
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(
                          widget.title,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: PTypography.titleSmall(
                            color: widget.enabled
                                ? AppColors.textPrimary
                                : AppColors.textDisabled,
                          ),
                        ),
                        if (widget.subtitle != null) ...[
                          SizedBox(height: PSpacing.xxs),
                          Text(
                            widget.subtitle!,
                            maxLines: 2,
                            overflow: TextOverflow.ellipsis,
                            style: PTypography.bodySmall(
                              color: widget.enabled
                                  ? AppColors.textSecondary
                                  : AppColors.textDisabled,
                            ),
                          ),
                        ],
                      ],
                    ),
                  ),
                  if (widget.trailing != null) ...[
                    SizedBox(width: PSpacing.md),
                    IconTheme(
                      data: IconThemeData(
                        color: widget.enabled
                            ? AppColors.textSecondary
                            : AppColors.textDisabled,
                        size: PSpacing.iconMD,
                      ),
                      child: widget.trailing!,
                    ),
                  ],
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
