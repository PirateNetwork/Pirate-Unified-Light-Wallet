import 'package:flutter/material.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Pirate Wallet Checkbox
class PCheckbox extends StatefulWidget {
  const PCheckbox({
    required this.value,
    required this.onChanged,
    this.label,
    this.tristate = false,
    super.key,
  });

  final bool? value;
  final ValueChanged<bool?>? onChanged;
  final String? label;
  final bool tristate;

  @override
  State<PCheckbox> createState() => _PCheckboxState();
}

class _PCheckboxState extends State<PCheckbox> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    final checkbox = MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 150),
        decoration: BoxDecoration(
          color: _isHovered && widget.onChanged != null
              ? AppColors.hoverOverlay
              : Colors.transparent,
          borderRadius: BorderRadius.circular(PSpacing.radiusSM),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            SizedBox(
              width: 24,
              height: 24,
              child: Checkbox(
                value: widget.value,
                onChanged: widget.onChanged,
                tristate: widget.tristate,
              ),
            ),
            if (widget.label != null) ...[
              SizedBox(width: PSpacing.sm),
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
            ],
          ],
        ),
      ),
    );

    if (widget.label != null) {
      return InkWell(
        onTap: widget.onChanged == null
            ? null
            : () {
                if (widget.tristate) {
                  if (widget.value == false) {
                    widget.onChanged?.call(null);
                  } else if (widget.value == null) {
                    widget.onChanged?.call(true);
                  } else {
                    widget.onChanged?.call(false);
                  }
                } else {
                  widget.onChanged?.call(!(widget.value ?? false));
                }
              },
        borderRadius: BorderRadius.circular(PSpacing.radiusSM),
        child: Padding(
          padding: EdgeInsets.all(PSpacing.xs),
          child: checkbox,
        ),
      );
    }

    return checkbox;
  }
}

