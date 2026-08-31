import 'package:flutter/material.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Stashi Wallet Checkbox
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
  late final FocusNode _focusNode;

  @override
  void initState() {
    super.initState();
    _focusNode = FocusNode()..addListener(_handleFocusChange);
  }

  @override
  void dispose() {
    _focusNode
      ..removeListener(_handleFocusChange)
      ..dispose();
    super.dispose();
  }

  void _handleFocusChange() {
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final checkbox = MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 150),
        constraints: const BoxConstraints(minHeight: 48),
        padding: EdgeInsets.all(PSpacing.xs),
        decoration: BoxDecoration(
          color: _isHovered && widget.onChanged != null
              ? AppColors.hoverOverlay
              : Colors.transparent,
          borderRadius: BorderRadius.circular(PSpacing.radiusSM),
          border: Border.all(
            color: _focusNode.hasFocus
                ? AppColors.focusRing
                : Colors.transparent,
            width: 2,
          ),
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
                focusNode: _focusNode,
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
      return MergeSemantics(
        child: InkWell(
          canRequestFocus: false,
          excludeFromSemantics: true,
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
          child: checkbox,
        ),
      );
    }

    return checkbox;
  }
}
