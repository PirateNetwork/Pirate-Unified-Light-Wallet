import 'package:flutter/material.dart';

import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Stashi Wallet Toggle Switch
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
    return MergeSemantics(
      child: MouseRegion(
        onEnter: (_) => setState(() => _isHovered = true),
        onExit: (_) => setState(() => _isHovered = false),
        child: InkWell(
          canRequestFocus: false,
          excludeFromSemantics: true,
          onTap: widget.onChanged == null
              ? null
              : () => widget.onChanged?.call(!widget.value),
          borderRadius: BorderRadius.circular(PSpacing.radiusSM),
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
                  focusNode: _focusNode,
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
