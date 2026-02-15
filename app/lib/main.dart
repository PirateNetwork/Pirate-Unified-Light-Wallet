import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:window_manager/window_manager.dart';

import 'core/ffi/ffi_bridge.dart';
import 'core/ffi/generated/models.dart' show SyncMode;
import 'core/desktop/single_instance.dart';
import 'core/desktop/windows_version.dart';
import 'core/logging/debug_log_path.dart';
import 'design/theme.dart';
import 'design/tokens/colors.dart';
import 'features/settings/providers/preferences_providers.dart';
import 'features/settings/providers/transport_providers.dart';
import 'routes/app_router.dart';
import 'core/providers/rust_init_provider.dart';
import 'ui/molecules/p_overlay_toast.dart';

SingleInstanceLock? _singleInstanceLock;

bool _appInitialized = false;
const Size _desktopInitialSize = Size(1100, 640);
const Size _desktopMinimumSize = Size(960, 600);

void main() async {
  if (_appInitialized) {
    runApp(const ProviderScope(child: PirateWalletApp()));
    return;
  }
  _appInitialized = true;

  WidgetsFlutterBinding.ensureInitialized();
  final logPath = await resolveDebugLogPath();
  _installFlutterErrorLogging(logPath);

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
    final useCustomTitleBar = shouldUseCustomTitleBar();

    final windowOptions = WindowOptions(
      size: _desktopInitialSize,
      minimumSize: _desktopMinimumSize,
      center: true,
      title: 'Pirate Wallet',
      backgroundColor: Color(0xFF0B0F14),
      titleBarStyle: useCustomTitleBar
          ? TitleBarStyle.hidden
          : TitleBarStyle.normal,
    );

    await windowManager.waitUntilReadyToShow(windowOptions, () async {
      await windowManager.show();
      await windowManager.focus();
    });
  }

  runApp(const ProviderScope(child: PirateWalletApp()));
}

void _installFlutterErrorLogging(String logPath) {
  Future<void> writeLog(String message, StackTrace? stack) async {
    try {
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
  Color? _lastWindowBackground;

  bool get _isDesktop =>
      Platform.isWindows || Platform.isLinux || Platform.isMacOS;

  void _syncWindowBackground(Color color) {
    if (!_isDesktop) {
      return;
    }
    final lastColor = _lastWindowBackground;
    if (lastColor != null && lastColor.toARGB32() == color.toARGB32()) {
      return;
    }
    _lastWindowBackground = color;
    unawaited(windowManager.setBackgroundColor(color));
  }

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
    if (_isDesktop) {
      // Desktop stays effectively "active" while the window exists.
      // We only mark inactive when fully detached/closing.
      FfiBridge.setAppActive(state != AppLifecycleState.detached);
      return;
    }

    // Mobile: pause UI polling while backgrounded. The Rust sync engine has
    // its own timeout/reconnect logic and should self-heal on resume.
    switch (state) {
      case AppLifecycleState.resumed:
        FfiBridge.setAppActive(true);
        unawaited(_ensureMobileSyncRunning());
        break;
      case AppLifecycleState.inactive:
      case AppLifecycleState.paused:
      case AppLifecycleState.hidden:
      case AppLifecycleState.detached:
        FfiBridge.setAppActive(false);
        break;
    }
  }

  Future<void> _ensureMobileSyncRunning() async {
    try {
      final walletId = await FfiBridge.getActiveWallet();
      if (walletId == null) return;
      await FfiBridge.startSync(walletId, SyncMode.compact);
    } catch (_) {
      // Best-effort.
    }
  }

  @override
  void onWindowFocus() {
    FfiBridge.setAppActive(true);
  }

  @override
  void onWindowBlur() {
    // Keep polling active while the app is open.
  }

  @override
  void onWindowMinimize() {
    FfiBridge.setAppActive(true);
  }

  @override
  void onWindowRestore() {
    FfiBridge.setAppActive(true);
  }

  Future<void> _shutdownTransports() async {
    try {
      await FfiBridge.shutdownTransport().timeout(const Duration(seconds: 2));
    } catch (_) {}
  }

  @override
  Future<void> onWindowClose() async {
    if (_closing) return;
    _closing = true;
    FfiBridge.setAppActive(false);
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
        : Brightness
              .dark; // Default to dark, will be updated in builder for system mode
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

        if (Platform.isWindows) {
          _syncWindowBackground(AppColors.backgroundBase);
        }

        // Return a widget that forces rebuild when theme changes
        // This ensures all child widgets rebuild when AppColors changes
        return POverlayToastHost(
          key: rootOverlayToastHostKey,
          child: Theme(
            data: Theme.of(context),
            child: child ?? const SizedBox.shrink(),
          ),
        );
      },

      // Routing
      routerConfig: router,

      // Locale
      supportedLocales: const [Locale('en', 'US')],
    );
  }
}
