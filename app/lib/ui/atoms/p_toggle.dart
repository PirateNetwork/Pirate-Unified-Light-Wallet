import 'package:flutter/material.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Pirate Wallet Toggle Switch
class PToggle extends StatefulWidget {
  const PToggle({
    required this.value,
    required this.onChanged,
    this.label,
    super.key,
  });

  final bool value;
  final ValueChanged<bool>? onChanged;
  final String? label;

  @override
  State<PToggle> createState() => _PToggleState();
}

class _PToggleState extends State<PToggle> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: InkWell(
        onTap: widget.onChanged == null ? null : () => widget.onChanged?.call(!widget.value),
        borderRadius: BorderRadius.circular(PSpacing.radiusSM),
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 150),
          decoration: BoxDecoration(
            color: _isHovered && widget.onChanged != null
                ? AppColors.hoverOverlay
                : Colors.transparent,
            borderRadius: BorderRadius.circular(PSpacing.radiusSM),
          ),
          padding: EdgeInsets.all(PSpacing.xs),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (widget.label != null) ...[
                Flexible(
                  child: Text(
                    widget.label!,
                    style: PTypography.bodyMedium(
                      color: widget.onChanged == null
                          ? AppColors.textDisabled
                          : AppColors.textPrimary,
                    ),
                  ),
                ),
                SizedBox(width: PSpacing.md),
              ],
              Switch(
                value: widget.value,
                onChanged: widget.onChanged,
              ),
            ],
          ),
        ),
      ),
    );
  }
}

