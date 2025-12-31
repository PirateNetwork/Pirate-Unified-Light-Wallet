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
      title: 'Forms Showcase',
      body: SingleChildScrollView(
        padding: EdgeInsets.all(PSpacing.lg),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            PHeroHeader(
              title: 'Form Components',
              subtitle: 'Inputs, checkboxes, radios, and more',
            ),
            SizedBox(height: PSpacing.xl),
            
            // Inputs
            PFormSection(
              title: 'Text Inputs',
              children: [
                const PInput(
                  label: 'Email',
                  hint: 'Enter your email',
                  helperText: 'We\'ll never share your email',
                ),
                const PInput(
                  label: 'Password',
                  hint: 'Enter your password',
                  obscureText: true,
                ),
                const PInput(
                  label: 'Wallet Address',
                  hint: 'zs1...',
                  monospace: true,
                  prefixIcon: Icon(Icons.account_balance_wallet),
                ),
                const PInput(
                  label: 'Disabled',
                  hint: 'This is disabled',
                  enabled: false,
                ),
              ],
            ),
            
            SizedBox(height: PSpacing.sectionGap),
            
            // Checkboxes
            PFormSection(
              title: 'Checkboxes',
              children: [
                PCheckbox(
                  value: _checkboxValue,
                  onChanged: (value) => setState(() => _checkboxValue = value!),
                  label: 'Accept terms and conditions',
                ),
                PCheckbox(
                  value: true,
                  onChanged: (value) {},
                  label: 'Checked checkbox',
                ),
                const PCheckbox(
                  value: false,
                  onChanged: null,
                  label: 'Disabled checkbox',
                ),
              ],
            ),
            
            SizedBox(height: PSpacing.sectionGap),
            
            // Radio buttons
            PFormSection(
              title: 'Radio Buttons',
              children: [
                PRadio<String>(
                  value: 'option1',
                  groupValue: _radioValue,
                  onChanged: (value) => setState(() => _radioValue = value!),
                  label: 'Option 1',
                ),
                PRadio<String>(
                  value: 'option2',
                  groupValue: _radioValue,
                  onChanged: (value) => setState(() => _radioValue = value!),
                  label: 'Option 2',
                ),
                PRadio<String>(
                  value: 'option3',
                  groupValue: _radioValue,
                  onChanged: null,
                  label: 'Disabled option',
                ),
              ],
            ),
            
            SizedBox(height: PSpacing.sectionGap),
            
            // Toggle switches
            PFormSection(
              title: 'Toggle Switches',
              children: [
                PToggle(
                  value: _toggleValue,
                  onChanged: (value) => setState(() => _toggleValue = value),
                  label: 'Enable notifications',
                ),
                const PToggle(
                  value: false,
                  onChanged: null,
                  label: 'Disabled toggle',
                ),
              ],
            ),
            
            SizedBox(height: PSpacing.sectionGap),
            
            // Badges
            PFormSection(
              title: 'Badges',
              children: [
                Wrap(
                  spacing: PSpacing.md,
                  runSpacing: PSpacing.md,
                  children: const [
                    PBadge(label: 'Neutral', variant: PBadgeVariant.neutral),
                    PBadge(label: 'Success', variant: PBadgeVariant.success),
                    PBadge(label: 'Warning', variant: PBadgeVariant.warning),
                    PBadge(label: 'Error', variant: PBadgeVariant.error),
                    PBadge(label: 'Info', variant: PBadgeVariant.info),
                  ],
                ),
              ],
            ),
            
            SizedBox(height: PSpacing.sectionGap),
            
            // Tags
            PFormSection(
              title: 'Tags',
              children: [
                Wrap(
                  spacing: PSpacing.chipGap,
                  runSpacing: PSpacing.chipGap,
                  children: [
                    PTag(label: 'Flutter', onDelete: () {}),
                    const PTag(label: 'Dart', selected: true),
                    PTag(label: 'Rust', onDelete: () {}),
                    const PTag(label: 'Bitcoin'),
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

