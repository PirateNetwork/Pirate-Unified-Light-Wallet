import 'package:flutter/material.dart';
import '../../../design/tokens/spacing.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../ui/organisms/p_hero_header.dart';
import '../../../ui/atoms/p_input.dart';
import '../../../ui/atoms/p_checkbox.dart';
import '../../../ui/atoms/p_radio.dart';
import '../../../ui/atoms/p_toggle.dart';
import '../../../ui/atoms/p_badge.dart';
import '../../../ui/atoms/p_tag.dart';
import '../../../ui/molecules/p_form_section.dart';
import '../../../core/i18n/arb_text_localizer.dart';

/// Showcase Forms Screen
class ShowcaseFormsScreen extends StatefulWidget {
  const ShowcaseFormsScreen({super.key});

  @override
  State<ShowcaseFormsScreen> createState() => _ShowcaseFormsScreenState();
}

class _ShowcaseFormsScreenState extends State<ShowcaseFormsScreen> {
  bool _checkboxValue = false;
  String _radioValue = 'option1';
  bool _toggleValue = false;

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Forms Showcase'.tr,
      body: SingleChildScrollView(
        padding: PSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            PHeroHeader(
              title: 'Form Components'.tr,
              subtitle: 'Inputs, checkboxes, radios, and more'.tr,
            ),
            SizedBox(height: PSpacing.xl),

            // Inputs
            PFormSection(
              title: 'Text Inputs'.tr,
              children: [
                PInput(
                  label: 'Email'.tr,
                  hint: 'Enter your email',
                  helperText: "We'll never share your email",
                ),
                PInput(
                  label: 'Password'.tr,
                  hint: 'Enter your password',
                  obscureText: true,
                ),
                PInput(
                  label: 'Wallet Address'.tr,
                  hint: 'zs1...',
                  monospace: true,
                  prefixIcon: Icon(Icons.account_balance_wallet),
                ),
                PInput(
                  label: 'Disabled'.tr,
                  hint: 'This is disabled',
                  enabled: false,
                ),
              ],
            ),

            SizedBox(height: PSpacing.sectionGap),

            // Checkboxes
            PFormSection(
              title: 'Checkboxes'.tr,
              children: [
                PCheckbox(
                  value: _checkboxValue,
                  onChanged: (value) => setState(() => _checkboxValue = value!),
                  label: 'Accept terms and conditions'.tr,
                ),
                PCheckbox(
                  value: true,
                  onChanged: (value) {},
                  label: 'Checked checkbox'.tr,
                ),
                PCheckbox(
                  value: false,
                  onChanged: null,
                  label: 'Disabled checkbox'.tr,
                ),
              ],
            ),

            SizedBox(height: PSpacing.sectionGap),

            // Radio buttons
            PFormSection(
              title: 'Radio Buttons'.tr,
              children: [
                PRadio<String>(
                  value: 'option1',
                  groupValue: _radioValue,
                  onChanged: (value) => setState(() => _radioValue = value!),
                  label: 'Option 1'.tr,
                ),
                PRadio<String>(
                  value: 'option2',
                  groupValue: _radioValue,
                  onChanged: (value) => setState(() => _radioValue = value!),
                  label: 'Option 2'.tr,
                ),
                PRadio<String>(
                  value: 'option3',
                  groupValue: _radioValue,
                  onChanged: null,
                  label: 'Disabled option'.tr,
                ),
              ],
            ),

            SizedBox(height: PSpacing.sectionGap),

            // Toggle switches
            PFormSection(
              title: 'Toggle Switches'.tr,
              children: [
                PToggle(
                  value: _toggleValue,
                  onChanged: (value) => setState(() => _toggleValue = value),
                  label: 'Enable notifications'.tr,
                ),
                PToggle(
                  value: false,
                  onChanged: null,
                  label: 'Disabled toggle'.tr,
                ),
              ],
            ),

            SizedBox(height: PSpacing.sectionGap),

            // Badges
            PFormSection(
              title: 'Badges'.tr,
              children: [
                Wrap(
                  spacing: PSpacing.md,
                  runSpacing: PSpacing.md,
                  children: [
                    PBadge(label: 'Neutral'.tr, variant: PBadgeVariant.neutral),
                    PBadge(label: 'Success'.tr, variant: PBadgeVariant.success),
                    PBadge(label: 'Warning'.tr, variant: PBadgeVariant.warning),
                    PBadge(label: 'Error'.tr, variant: PBadgeVariant.error),
                    PBadge(label: 'Info'.tr, variant: PBadgeVariant.info),
                  ],
                ),
              ],
            ),

            SizedBox(height: PSpacing.sectionGap),

            // Tags
            PFormSection(
              title: 'Tags'.tr,
              children: [
                Wrap(
                  spacing: PSpacing.chipGap,
                  runSpacing: PSpacing.chipGap,
                  children: [
                    PTag(label: 'Flutter'.tr, onDelete: () {}),
                    PTag(label: 'Dart'.tr, selected: true),
                    PTag(label: 'Rust'.tr, onDelete: () {}),
                    PTag(label: 'Bitcoin'.tr),
                  ],
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
