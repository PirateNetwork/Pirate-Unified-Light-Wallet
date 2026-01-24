import 'dart:io';

import 'package:path_provider/path_provider.dart';

class SingleInstanceLock {
  SingleInstanceLock._(this._raf);

  final RandomAccessFile _raf;

  static Future<SingleInstanceLock?> acquire({
    String name = 'pirate_wallet.lock',
  }) async {
    Directory baseDir;
    try {
      baseDir = await getApplicationSupportDirectory();
    } catch (_) {
      baseDir = Directory.current;
    }

    final lockPath = '${baseDir.path}${Platform.pathSeparator}$name';
    final file = File(lockPath);
    await file.parent.create(recursive: true);
    final raf = await file.open(mode: FileMode.write);

    try {
      await raf.lock(FileLock.exclusive);
    } on FileSystemException {
      await raf.close();
      return null;
    }

    try {
      await raf.setPosition(0);
      await raf.truncate(0);
      await raf.writeString('pid=$pid\n');
      await raf.flush();
    } catch (_) {
      // Ignore lock file metadata errors.
    }

    return SingleInstanceLock._(raf);
  }

  Future<void> release() async {
    try {
      await _raf.unlock();
    } catch (_) {}
    try {
      await _raf.close();
    } catch (_) {}
  }
}
