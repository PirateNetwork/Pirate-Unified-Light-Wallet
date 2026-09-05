import 'package:flutter/material.dart';

import '../i18n/arb_text_localizer.dart';

enum DesktopUpdateAction { update, later, skip, changelog }

class DesktopUpdateDialog extends StatelessWidget {
  const DesktopUpdateDialog({
    required this.currentVersion,
    required this.newVersion,
    super.key,
  });
  final String currentVersion;
  final String newVersion;

  @override
  Widget build(BuildContext context) => AlertDialog(
    icon: const Icon(Icons.system_update_alt_rounded, size: 36),
    title: Text('A new Stashi Wallet is ready'.tr),
    content: SizedBox(
      width: 460,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Text(
                currentVersion,
                style: Theme.of(context).textTheme.titleLarge,
              ),
              const Padding(
                padding: EdgeInsets.symmetric(horizontal: 12),
                child: Icon(Icons.arrow_forward_rounded, size: 22),
              ),
              Text(newVersion, style: Theme.of(context).textTheme.titleLarge),
            ],
          ),
          const SizedBox(height: 16),
          Text(
            'Download the official release. Its signed checksum will be verified before the installer opens.'
                .tr,
          ),
          const SizedBox(height: 12),
          Text(
            'Your wallet stays open while the download is prepared.'.tr,
            style: Theme.of(context).textTheme.bodySmall,
          ),
          const SizedBox(height: 12),
          TextButton.icon(
            onPressed: () =>
                Navigator.pop(context, DesktopUpdateAction.changelog),
            icon: const Icon(Icons.open_in_new, size: 18),
            label: Text('Release notes'.tr),
          ),
        ],
      ),
    ),
    actions: [
      TextButton(
        onPressed: () => Navigator.pop(context, DesktopUpdateAction.skip),
        child: Text('Skip this version'.tr),
      ),
      TextButton(
        onPressed: () => Navigator.pop(context, DesktopUpdateAction.later),
        child: Text('Later'.tr),
      ),
      FilledButton.icon(
        onPressed: () => Navigator.pop(context, DesktopUpdateAction.update),
        icon: const Icon(Icons.download_rounded),
        label: Text('Download update'.tr),
      ),
    ],
  );
}
