import 'package:flutter/material.dart';
import '../../../design/tokens/spacing.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../ui/organisms/p_hero_header.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/molecules/p_dialog.dart';
import '../../../ui/molecules/p_bottom_sheet.dart';
import '../../../ui/molecules/p_snack.dart';
import '../../../ui/molecules/p_form_section.dart';

/// Showcase Dialogs Screen
class ShowcaseDialogsScreen extends StatelessWidget {
  const ShowcaseDialogsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Dialogs Showcase',
      body: SingleChildScrollView(
        padding: EdgeInsets.all(PSpacing.lg),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            PHeroHeader(
              title: 'Dialogs & Overlays',
              subtitle: 'Modals, bottom sheets, and snackbars',
            ),
            SizedBox(height: PSpacing.xl),
            
            // Dialogs
            PFormSection(
              title: 'Dialogs',
              children: [
                Wrap(
                  spacing: PSpacing.md,
                  runSpacing: PSpacing.md,
                  children: [
                    PButton(
                      onPressed: () => _showSimpleDialog(context),
                      child: const Text('Simple Dialog'),
                    ),
                    PButton(
                      onPressed: () => _showConfirmDialog(context),
                      variant: PButtonVariant.secondary,
                      child: const Text('Confirm Dialog'),
                    ),
                  ],
                ),
              ],
            ),
            
            SizedBox(height: PSpacing.sectionGap),
            
            // Bottom Sheets
            PFormSection(
              title: 'Bottom Sheets',
              children: [
                PButton(
                  onPressed: () => _showBottomSheet(context),
                  child: const Text('Show Bottom Sheet'),
                ),
              ],
            ),
            
            SizedBox(height: PSpacing.sectionGap),
            
            // Snackbars
            PFormSection(
              title: 'Snackbars',
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
                      child: const Text('Neutral'),
                    ),
                    PButton(
                      onPressed: () => PSnack.show(
                        context: context,
                        message: 'Success! Operation completed',
                        variant: PSnackVariant.success,
                      ),
                      variant: PButtonVariant.outline,
                      child: const Text('Success'),
                    ),
                    PButton(
                      onPressed: () => PSnack.show(
                        context: context,
                        message: 'Warning: Check your input',
                        variant: PSnackVariant.warning,
                      ),
                      variant: PButtonVariant.outline,
                      child: const Text('Warning'),
                    ),
                    PButton(
                      onPressed: () => PSnack.show(
                        context: context,
                        message: 'Error: Something went wrong',
                        variant: PSnackVariant.error,
                      ),
                      variant: PButtonVariant.outline,
                      child: const Text('Error'),
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
                      child: const Text('Info with Action'),
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
    PDialog.show(
      context: context,
      title: 'Simple Dialog',
      content: const Text('This is a simple dialog with a message.'),
      actions: [
        PDialogAction(
          label: 'OK',
          onPressed: () {},
        ),
      ],
    );
  }

  void _showConfirmDialog(BuildContext context) {
    PDialog.show(
      context: context,
      title: 'Confirm Action',
      content: const Text('Are you sure you want to proceed? This action cannot be undone.'),
      actions: [
        PDialogAction(
          label: 'Cancel',
          variant: PButtonVariant.ghost,
          onPressed: () {},
        ),
        PDialogAction(
          label: 'Confirm',
          variant: PButtonVariant.danger,
          onPressed: () {},
        ),
      ],
    );
  }

  void _showBottomSheet(BuildContext context) {
    PBottomSheet.show(
      context: context,
      title: 'Bottom Sheet Example',
      content: Column(
        children: [
          ListTile(
            leading: const Icon(Icons.share),
            title: const Text('Share'),
            onTap: () => Navigator.pop(context),
          ),
          ListTile(
            leading: const Icon(Icons.link),
            title: const Text('Copy Link'),
            onTap: () => Navigator.pop(context),
          ),
          ListTile(
            leading: const Icon(Icons.edit),
            title: const Text('Edit'),
            onTap: () => Navigator.pop(context),
          ),
        ],
      ),
    );
  }
}

