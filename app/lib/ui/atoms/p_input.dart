import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Pirate Wallet Input Field
class PInput extends StatefulWidget {
  const PInput({
    this.controller,
    this.value,
    this.label,
    this.hint,
    this.helperText,
    this.errorText,
    this.prefixIcon,
    this.suffixIcon,
    this.obscureText = false,
    this.enabled = true,
    this.readOnly = false,
    this.maxLines = 1,
    this.maxLength,
    this.autocorrect = true,
    this.enableSuggestions = true,
    this.keyboardType,
    this.textInputAction,
    this.onChanged,
    this.onSubmitted,
    this.validator,
    this.inputFormatters,
    this.focusNode,
    this.autofocus = false,
    this.monospace = false,
    super.key,
  }) : assert(controller == null || value == null, 'Cannot provide both controller and value');

  final TextEditingController? controller;
  final String? value;
  final String? label;
  final String? hint;
  final String? helperText;
  final String? errorText;
  final Widget? prefixIcon;
  final Widget? suffixIcon;
  final bool obscureText;
  final bool enabled;
  final bool readOnly;
  final int maxLines;
  final int? maxLength;
  final bool autocorrect;
  final bool enableSuggestions;
  final TextInputType? keyboardType;
  final TextInputAction? textInputAction;
  final ValueChanged<String>? onChanged;
  final ValueChanged<String>? onSubmitted;
  final FormFieldValidator<String>? validator;
  final List<TextInputFormatter>? inputFormatters;
  final FocusNode? focusNode;
  final bool autofocus;
  final bool monospace;

  @override
  State<PInput> createState() => _PInputState();
}

class _PInputState extends State<PInput> {
  late FocusNode _focusNode;
  bool _ownsFocusNode = false;
  bool _isFocused = false;
  late TextEditingController _internalController;
  bool _isInternalController = false;

  @override
  void initState() {
    super.initState();
    if (widget.focusNode != null) {
      _focusNode = widget.focusNode!;
    } else {
      _focusNode = FocusNode();
      _ownsFocusNode = true;
    }
    _focusNode.addListener(_onFocusChange);
    if (widget.controller == null && widget.value != null) {
      _internalController = TextEditingController(text: widget.value);
      _isInternalController = true;
    } else if (widget.controller == null) {
      _internalController = TextEditingController();
      _isInternalController = true;
    }
  }

  @override
  void didUpdateWidget(PInput oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_isInternalController && widget.value != oldWidget.value) {
      _internalController.text = widget.value ?? '';
    }
    if (oldWidget.focusNode != widget.focusNode) {
      _focusNode.removeListener(_onFocusChange);
      if (_ownsFocusNode) {
        _focusNode.dispose();
      }
      if (widget.focusNode != null) {
        _focusNode = widget.focusNode!;
        _ownsFocusNode = false;
      } else {
        _focusNode = FocusNode();
        _ownsFocusNode = true;
      }
      _focusNode.addListener(_onFocusChange);
    }
  }

  @override
  void dispose() {
    if (_isInternalController) {
      _internalController.dispose();
    }
    _focusNode.removeListener(_onFocusChange);
    if (_ownsFocusNode) {
      _focusNode.dispose();
    }
    super.dispose();
  }

  void _onFocusChange() {
    setState(() {
      _isFocused = _focusNode.hasFocus;
    });
  }

  @override
  Widget build(BuildContext context) {
    final reduceMotion = MediaQuery.of(context).disableAnimations;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        if (widget.label != null) ...[
          Text(
            widget.label!,
            style: PTypography.labelMedium(color: AppColors.textSecondary),
          ),
          SizedBox(height: PSpacing.xs),
        ],
        AnimatedContainer(
          duration:
              reduceMotion ? Duration.zero : const Duration(milliseconds: 200),
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(PSpacing.radiusInput),
            boxShadow: _isFocused
                ? [
                    BoxShadow(
                      color: AppColors.focusRingSubtle,
                      blurRadius: 8.0,
                      offset: Offset.zero,
                    ),
                  ]
                : null,
          ),
          child: TextField(
            controller: widget.controller ?? (_isInternalController ? _internalController : null),
            focusNode: _focusNode,
            obscureText: widget.obscureText,
            enabled: widget.enabled,
            readOnly: widget.readOnly,
            maxLines: widget.maxLines,
            maxLength: widget.maxLength,
            autocorrect: widget.autocorrect,
            enableSuggestions: widget.enableSuggestions,
            keyboardType: widget.keyboardType,
            textInputAction: widget.textInputAction,
            onChanged: widget.onChanged,
            onSubmitted: widget.onSubmitted,
            inputFormatters: widget.inputFormatters,
            autofocus: widget.autofocus,
            style: widget.monospace
                ? PTypography.codeMedium(color: AppColors.textPrimary)
                : PTypography.bodyMedium(color: AppColors.textPrimary),
            cursorColor: AppColors.focusRing,
            decoration: InputDecoration(
              hintText: widget.hint,
              errorText: widget.errorText,
              prefixIcon: widget.prefixIcon,
              suffixIcon: widget.suffixIcon,
              filled: true,
              fillColor: widget.enabled
                  ? AppColors.backgroundSurface
                  : AppColors.backgroundBase,
            ),
          ),
        ),
        if (widget.helperText != null) ...[
          SizedBox(height: PSpacing.xxs),
          Text(
            widget.helperText!,
            style: PTypography.caption(color: AppColors.textTertiary),
          ),
        ],
      ],
    );
  }
}
