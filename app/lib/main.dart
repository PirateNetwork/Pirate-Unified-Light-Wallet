import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:bitsdojo_window/bitsdojo_window.dart';
import 'package:window_manager/window_manager.dart';

import 'core/ffi/ffi_bridge.dart';
import 'core/desktop/single_instance.dart';
import 'design/theme.dart';
import 'design/tokens/colors.dart';
import 'features/settings/providers/preferences_providers.dart';
import 'features/settings/providers/transport_providers.dart';
import 'routes/app_router.dart';
import 'core/providers/rust_init_provider.dart';

SingleInstanceLock? _singleInstanceLock;

bool _appInitialized = false;

void main() async {
  if (_appInitialized) {
    runApp(
      const ProviderScope(
        child: PirateWalletApp(),
      ),
    );
    return;
  }
  _appInitialized = true;

  WidgetsFlutterBinding.ensureInitialized();
  _installFlutterErrorLogging();

  final isTest = Platform.environment.containsKey('FLUTTER_TEST');

  if (!isTest && (Platform.isWindows || Platform.isLinux || Platform.isMacOS)) {
    _singleInstanceLock = await SingleInstanceLock.acquire();
    if (_singleInstanceLock == null) {
      stderr.writeln('Pirate Wallet is already running.');
      exit(0);
    }
  }

  // Desktop window setup
  if (!isTest && (Platform.isWindows || Platform.isLinux || Platform.isMacOS)) {
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

void _installFlutterErrorLogging() {
  Future<void> writeLog(String message, StackTrace? stack) async {
    try {
      final baseDir = Directory.current.path;
      final logPath = Platform.environment['PIRATE_DEBUG_LOG_PATH'] ??
          '$baseDir${Platform.pathSeparator}.cursor${Platform.pathSeparator}debug.log';
      final logFile = File(logPath);
      await logFile.parent.create(recursive: true);
      final payload = jsonEncode({
        'id': 'log_flutter_error',
        'timestamp': DateTime.now().millisecondsSinceEpoch,
        'message': message,
        'stack': stack?.toString(),
      });
      await logFile.writeAsString(
        '$payload\n',
        mode: FileMode.append,
        flush: true,
      );
    } catch (_) {
      // Ignore logging failures.
    }
  }

  FlutterError.onError = (details) {
    FlutterError.presentError(details);
    unawaited(writeLog(details.exceptionAsString(), details.stack));
  };

  PlatformDispatcher.instance.onError = (error, stack) {
    unawaited(writeLog(error.toString(), stack));
    return true;
  };
}
class PirateWalletApp extends ConsumerStatefulWidget {
  const PirateWalletApp({super.key});

  @override
  ConsumerState<PirateWalletApp> createState() => _PirateWalletAppState();
}

class _PirateWalletAppState extends ConsumerState<PirateWalletApp>
    with WindowListener, WidgetsBindingObserver {
  bool _closing = false;

  bool get _isDesktop =>
      Platform.isWindows || Platform.isLinux || Platform.isMacOS;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    FfiBridge.setAppActive(true);
    if (_isDesktop) {
      windowManager
        ..addListener(this)
        ..setPreventClose(true);
    }

    ref.listen<AsyncValue<void>>(rustInitProvider, (_, next) {
      if (next.hasValue) {
        unawaited(ref.read(transportConfigProvider.notifier).refresh());
      }
    });
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    if (_isDesktop) {
      windowManager.removeListener(this);
    }
    final release = _singleInstanceLock?.release();
    if (release != null) {
      unawaited(release);
    }
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    FfiBridge.setAppActive(state == AppLifecycleState.resumed);
  }

  @override
  void onWindowFocus() {
    FfiBridge.setAppActive(true);
  }

  @override
  void onWindowBlur() {
    FfiBridge.setAppActive(false);
  }

  @override
  void onWindowMinimize() {
    FfiBridge.setAppActive(false);
  }

  @override
  void onWindowRestore() {
    FfiBridge.setAppActive(true);
  }

  Future<void> _shutdownTransports() async {
    try {
      await FfiBridge.shutdownTransport()
          .timeout(const Duration(seconds: 2));
    } catch (_) {}
  }

  @override
  Future<void> onWindowClose() async {
    if (_closing) return;
    _closing = true;
    unawaited(windowManager.hide());
    unawaited(_shutdownTransports());
    final release = _singleInstanceLock?.release();
    if (release != null) {
      unawaited(release);
    }
    unawaited(windowManager.destroy());
  }

  @override
  Widget build(BuildContext context) {
    final router = ref.watch(appRouterProvider);
    final themeModeSetting = ref.watch(appThemeModeProvider);
    
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



