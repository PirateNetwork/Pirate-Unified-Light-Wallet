import 'package:flutter/material.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Stashi Wallet Radio Button
class PRadio<T> extends StatefulWidget {
  const PRadio({
    required this.value,
    required this.groupValue,
    required this.onChanged,
    this.label,
    super.key,
  });

  final T value;
  final T? groupValue;
  final ValueChanged<T?>? onChanged;
  final String? label;

  @override
  State<PRadio<T>> createState() => _PRadioState<T>();
}

class _PRadioState<T> extends State<PRadio<T>> {
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
    final radio = MouseRegion(
      onEnter: (_) => setState(() => _isHovered = true),
      onExit: (_) => setState(() => _isHovered = false),
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 150),
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
              child: RadioGroup<T>(
                groupValue: widget.groupValue,
                onChanged: widget.onChanged ?? (_) {},
                child: Radio<T>(value: widget.value, focusNode: _focusNode),
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
              : () => widget.onChanged?.call(widget.value),
          borderRadius: BorderRadius.circular(PSpacing.radiusSM),
          child: Padding(padding: EdgeInsets.all(PSpacing.xs), child: radio),
        ),
      );
    }

    return radio;
  }
}
