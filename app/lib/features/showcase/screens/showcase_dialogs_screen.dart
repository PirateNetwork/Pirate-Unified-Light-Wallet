import 'package:flutter/material.dart';
import '../../../design/tokens/spacing.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../ui/organisms/p_hero_header.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/molecules/p_dialog.dart';
import '../../../ui/molecules/p_bottom_sheet.dart';
import '../../../ui/molecules/p_snack.dart';
import '../../../ui/molecules/p_form_section.dart';
import '../../../core/i18n/arb_text_localizer.dart';

/// Showcase Dialogs Screen
class ShowcaseDialogsScreen extends StatelessWidget {
  const ShowcaseDialogsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Dialogs Showcase'.tr,
      body: SingleChildScrollView(
        padding: PSpacing.screenPadding(MediaQuery.of(context).size.width),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            PHeroHeader(
              title: 'Dialogs & Overlays'.tr,
              subtitle: 'Modals, bottom sheets, and snackbars'.tr,
            ),
            SizedBox(height: PSpacing.xl),

            // Dialogs
            PFormSection(
              title: 'Dialogs'.tr,
              children: [
                Wrap(
                  spacing: PSpacing.md,
                  runSpacing: PSpacing.md,
                  children: [
                    PButton(
                      onPressed: () => _showSimpleDialog(context),
                      child: Text('Simple Dialog'.tr),
                    ),
                    PButton(
                      onPressed: () => _showConfirmDialog(context),
                      variant: PButtonVariant.secondary,
                      child: Text('Confirm Dialog'.tr),
                    ),
                  ],
                ),
              ],
            ),

            SizedBox(height: PSpacing.sectionGap),

            // Bottom Sheets
            PFormSection(
              title: 'Bottom Sheets'.tr,
              children: [
                PButton(
                  onPressed: () => _showBottomSheet(context),
                  child: Text('Show Bottom Sheet'.tr),
                ),
              ],
            ),

            SizedBox(height: PSpacing.sectionGap),

            // Snackbars
            PFormSection(
              title: 'Snackbars'.tr,
              children: [
                Wrap(
                  spacing: PSpacing.md,
                  runSpacing: PSpacing.md,
                  children: [
                    PButton(
                      onPressed: () => PSnack.show(
                        context: context,
                        message: 'This is a neutral snackbar',
                      ),
                      variant: PButtonVariant.outline,
                      child: Text('Neutral'.tr),
                    ),
                    PButton(
                      onPressed: () => PSnack.show(
                        context: context,
                        message: 'Success! Operation completed',
                        variant: PSnackVariant.success,
                      ),
                      variant: PButtonVariant.outline,
                      child: Text('Success'.tr),
                    ),
                    PButton(
                      onPressed: () => PSnack.show(
                        context: context,
                        message: 'Warning: Check your input',
                        variant: PSnackVariant.warning,
                      ),
                      variant: PButtonVariant.outline,
                      child: Text('Warning'.tr),
                    ),
                    PButton(
                      onPressed: () => PSnack.show(
                        context: context,
                        message: 'Error: Something went wrong',
                        variant: PSnackVariant.error,
                      ),
                      variant: PButtonVariant.outline,
                      child: Text('Error'.tr),
                    ),
                    PButton(
                      onPressed: () => PSnack.show(
                        context: context,
                        message: 'Info: Did you know?',
                        variant: PSnackVariant.info,
                        actionLabel: 'Undo',
                        onAction: () {},
                      ),
                      variant: PButtonVariant.outline,
                      child: Text('Info with Action'.tr),
                    ),
                  ],
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  void _showSimpleDialog(BuildContext context) {
    PDialog.show<void>(
      context: context,
      title: 'Simple Dialog'.tr,
      content: Text('This is a simple dialog with a message.'.tr),
      actions: [PDialogAction(label: 'OK'.tr, onPressed: () {})],
    );
  }

  void _showConfirmDialog(BuildContext context) {
    PDialog.show<void>(
      context: context,
      title: 'Confirm Action'.tr,
      content: Text(
        'Are you sure you want to proceed? This action cannot be undone.'.tr,
      ),
      actions: [
        PDialogAction(
          label: 'Cancel'.tr,
          variant: PButtonVariant.ghost,
          onPressed: () {},
        ),
        PDialogAction(
          label: 'Confirm'.tr,
          variant: PButtonVariant.danger,
          onPressed: () {},
        ),
      ],
    );
  }

  void _showBottomSheet(BuildContext context) {
    PBottomSheet.show<void>(
      context: context,
      title: 'Bottom Sheet Example'.tr,
      content: Column(
        children: [
          ListTile(
            leading: const Icon(Icons.share),
            title: Text('Share'.tr),
            onTap: () => Navigator.pop(context),
          ),
          ListTile(
            leading: const Icon(Icons.link),
            title: Text('Copy Link'.tr),
            onTap: () => Navigator.pop(context),
          ),
          ListTile(
            leading: const Icon(Icons.edit),
            title: Text('Edit'.tr),
            onTap: () => Navigator.pop(context),
          ),
        ],
      ),
    );
  }
}
