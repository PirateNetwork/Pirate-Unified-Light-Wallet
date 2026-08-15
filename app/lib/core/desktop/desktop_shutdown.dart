typedef DesktopShutdownStep = Future<void> Function();

/// Orders desktop cleanup before the native window and Flutter engine exit.
class DesktopShutdownCoordinator {
  DesktopShutdownCoordinator({
    required this._hideWindow,
    required this._cleanUp,
    required this._releaseInstanceLock,
    required this._allowWindowClose,
    required this._closeWindow,
    required this._forceDestroyWindow,
  });

  final DesktopShutdownStep _hideWindow;
  final DesktopShutdownStep _cleanUp;
  final DesktopShutdownStep _releaseInstanceLock;
  final DesktopShutdownStep _allowWindowClose;
  final DesktopShutdownStep _closeWindow;
  final DesktopShutdownStep _forceDestroyWindow;

  Future<void>? _shutdown;

  Future<void> close() => _shutdown ??= _close();

  Future<void> _close() async {
    await _bestEffort(_hideWindow);
    await _bestEffort(_cleanUp);
    await _bestEffort(_releaseInstanceLock);

    try {
      await _allowWindowClose();
      await _closeWindow();
    } catch (_) {
      await _bestEffort(_forceDestroyWindow);
    }
  }

  Future<void> _bestEffort(DesktopShutdownStep step) async {
    try {
      await step();
    } catch (_) {
      // Exit must continue even when an optional cleanup step fails.
    }
  }
}
