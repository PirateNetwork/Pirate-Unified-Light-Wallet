import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:bitsdojo_window/bitsdojo_window.dart';
import 'package:window_manager/window_manager.dart';

import 'core/ffi/ffi_bridge.dart';
import 'core/ffi/generated/frb_generated.dart';
import 'design/theme.dart';
import 'design/tokens/colors.dart';
import 'features/settings/providers/preferences_providers.dart';
import 'features/settings/providers/transport_providers.dart';
import 'routes/app_router.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // Initialize Flutter Rust Bridge with timeout
  try {
    debugPrint('Initializing Rust library...');
    await RustLib.init().timeout(
      const Duration(seconds: 10),
      onTimeout: () {
        throw TimeoutException('Rust library initialization timed out after 10 seconds');
      },
    );
    debugPrint('✅ Rust library initialized successfully');
  } catch (e, stackTrace) {
    debugPrint('❌ Failed to initialize Rust library: $e');
    debugPrint('Stack trace: $stackTrace');
    // For now, continue - the app might work with mock mode or fail gracefully
    // In production, you might want to show an error screen
  }

  // Desktop window setup
  if (Platform.isWindows || Platform.isLinux || Platform.isMacOS) {
    await windowManager.ensureInitialized();

    const windowOptions = WindowOptions(
      size: Size(1200, 800),
      minimumSize: Size(960, 600),
      center: true,
      title: 'Pirate Wallet',
      backgroundColor: Color(0xFF0B0F14),
      titleBarStyle: TitleBarStyle.hidden,
    );

    await windowManager.waitUntilReadyToShow(windowOptions, () async {
      await windowManager.show();
      await windowManager.focus();
    });

    // Bitsdojo window sizing as an extra safety to surface the window
    doWhenWindowReady(() {
      appWindow.minSize = const Size(960, 600);
      appWindow.size = const Size(1200, 800);
      appWindow.alignment = Alignment.center;
      appWindow.title = 'Pirate Wallet';
      appWindow.show();
    });
  }

  runApp(
    const ProviderScope(
      child: PirateWalletApp(),
    ),
  );
}

class PirateWalletApp extends ConsumerStatefulWidget {
  const PirateWalletApp({super.key});

  @override
  ConsumerState<PirateWalletApp> createState() => _PirateWalletAppState();
}

class _PirateWalletAppState extends ConsumerState<PirateWalletApp>
    with WindowListener {
  bool _closing = false;

  bool get _isDesktop =>
      Platform.isWindows || Platform.isLinux || Platform.isMacOS;

  @override
  void initState() {
    super.initState();
    if (_isDesktop) {
      windowManager.addListener(this);
      windowManager.setPreventClose(true);
    }
  }

  @override
  void dispose() {
    if (_isDesktop) {
      windowManager.removeListener(this);
    }
    super.dispose();
  }

  Future<void> _shutdownTransports() async {
    try {
      await FfiBridge.shutdownTransport()
          .timeout(const Duration(seconds: 2));
    } catch (_) {}
  }

  @override
  void onWindowClose() async {
    if (_closing) return;
    _closing = true;
    await _shutdownTransports();
    await windowManager.destroy();
  }

  @override
  Widget build(BuildContext context) {
    final router = ref.watch(appRouterProvider);
    final themeModeSetting = ref.watch(appThemeModeProvider);
    // Trigger transport config load early so tunnel bootstrap can start pre-unlock.
    ref.watch(transportConfigProvider);
    
    // Determine brightness based on theme mode
    // For system mode, we'll sync in the builder after MaterialApp is built
    final brightness = themeModeSetting.themeMode == ThemeMode.dark
        ? Brightness.dark
        : themeModeSetting.themeMode == ThemeMode.light
            ? Brightness.light
            : Brightness.dark; // Default to dark, will be updated in builder for system mode
    AppColors.syncWithTheme(brightness);

    return MaterialApp.router(
      key: ValueKey(themeModeSetting.themeMode),
      title: 'Pirate Wallet',
      debugShowCheckedModeBanner: false,
      
      // Theme
      theme: PTheme.light(),
      darkTheme: PTheme.dark(),
      themeMode: themeModeSetting.themeMode,

      builder: (context, child) {
        // Sync colors with current theme brightness on every build
        // This ensures AppColors stays in sync when theme changes
        // For system mode, this will use the actual resolved brightness
        final currentBrightness = Theme.of(context).brightness;
        AppColors.syncWithTheme(currentBrightness);
        
        // Return a widget that forces rebuild when theme changes
        // This ensures all child widgets rebuild when AppColors changes
        return Theme(
          data: Theme.of(context),
          child: child ?? const SizedBox.shrink(),
        );
      },
      
      // Routing
      routerConfig: router,
      
      // Locale
      supportedLocales: const [
        Locale('en', 'US'),
      ],
    );
  }
}
