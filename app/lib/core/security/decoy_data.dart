import 'dart:math';

import '../ffi/generated/models.dart';

class DecoyAddressEntry {
  final String address;
  final DateTime createdAt;
  final int index;

  const DecoyAddressEntry({
    required this.address,
    required this.createdAt,
    required this.index,
  });
}

class DecoyData {
  static final Random _random = _initRandom();
  static const String _bech32Chars = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l';
  static const List<String> _wordList = [
    'ability',
    'absorb',
    'access',
    'account',
    'across',
    'action',
    'active',
    'adapt',
    'address',
    'advance',
    'advice',
    'aerobic',
    'affair',
    'again',
    'agent',
    'ahead',
    'aim',
    'air',
    'album',
    'alert',
    'alive',
    'allow',
    'almost',
    'alpha',
    'always',
    'amount',
    'ancient',
    'angel',
    'answer',
    'any',
    'apart',
    'apple',
    'arena',
    'argue',
    'armor',
    'artist',
    'aspect',
    'asset',
    'assist',
    'attack',
    'audit',
    'author',
    'auto',
    'awake',
    'balance',
    'banner',
    'basic',
    'battery',
    'beach',
    'become',
    'benefit',
    'between',
    'beyond',
    'bitter',
    'blank',
    'blend',
    'bless',
    'blossom',
    'blue',
    'bonus',
    'borrow',
    'brief',
    'bright',
    'build',
    'buyer',
    'cable',
    'camera',
    'canal',
    'cancel',
    'carbon',
    'card',
    'castle',
    'casual',
    'catalog',
    'cattle',
    'cause',
    'center',
    'chain',
    'chance',
    'charge',
    'charm',
    'cheap',
    'chess',
    'choice',
    'citizen',
    'claim',
    'clarify',
    'classic',
    'clean',
    'client',
    'clinic',
    'cluster',
    'coach',
    'coast',
    'collect',
    'color',
    'comfort',
    'comic',
    'common',
    'company',
    'confirm',
    'connect',
    'control',
    'convince',
    'corner',
    'correct',
    'cost',
    'crack',
    'create',
    'credit',
    'crystal',
    'culture',
    'curve',
    'cycle',
    'danger',
    'deal',
    'debris',
    'decide',
    'defense',
    'delay',
    'deliver',
    'depend',
    'deposit',
    'design',
    'detail',
    'device',
    'differ',
    'dinner',
    'direct',
    'discover',
    'display',
    'doctor',
    'domain',
    'double',
    'dream',
    'driver',
    'during',
    'earth',
    'echo',
    'edit',
    'effect',
    'effort',
    'either',
    'elder',
    'elegant',
    'embark',
    'energy',
    'engine',
    'enhance',
    'enter',
    'equal',
    'escape',
    'estate',
    'ethics',
    'event',
    'exact',
    'exit',
    'expand',
    'expect',
    'explain',
    'extend',
    'fabric',
    'family',
    'fashion',
    'feature',
    'federal',
    'filter',
    'final',
    'finger',
    'finish',
    'flame',
    'focus',
    'forest',
    'fortune',
    'frame',
    'future',
    'garden',
    'gather',
    'globe',
    'gold',
    'good',
    'gravity',
    'green',
    'group',
    'habit',
    'handle',
    'harvest',
    'health',
    'hidden',
    'history',
    'honor',
    'hotel',
    'house',
    'image',
    'impact',
    'improve',
    'include',
    'index',
    'inform',
    'input',
    'inside',
    'island',
    'item',
    'jacket',
    'jungle',
    'keep',
    'king',
    'knock',
    'label',
    'laser',
    'leader',
    'legend',
    'level',
    'limit',
    'local',
    'logic',
    'lucky',
    'machine',
    'manage',
    'market',
    'matrix',
    'media',
    'memory',
    'middle',
    'model',
    'modern',
    'moment',
    'motion',
    'museum',
    'native',
    'nature',
    'network',
    'neutral',
    'noise',
    'normal',
    'object',
    'offer',
    'online',
    'open',
    'option',
    'orbit',
    'order',
    'origin',
    'output',
    'owner',
    'panel',
    'paper',
    'parent',
    'party',
    'pass',
    'pattern',
    'people',
    'perfect',
    'photo',
    'planet',
    'player',
    'point',
    'power',
    'prefer',
    'primary',
    'private',
    'process',
    'promise',
    'proof',
    'purpose',
    'quiet',
    'random',
    'rapid',
    'reason',
    'record',
    'refresh',
    'region',
    'return',
    'reward',
    'risk',
    'route',
    'safe',
    'sail',
    'scale',
    'scene',
    'screen',
    'search',
    'secret',
    'shadow',
    'signal',
    'simple',
    'smart',
    'solid',
    'source',
    'space',
    'spirit',
    'stable',
    'story',
    'stream',
    'sudden',
    'summer',
    'survey',
    'system',
    'target',
    'theme',
    'ticket',
    'token',
    'trade',
    'travel',
    'trust',
    'tunnel',
    'uniform',
    'unique',
    'update',
    'urban',
    'value',
    'vector',
    'victory',
    'video',
    'vital',
    'wallet',
    'window',
    'winter',
    'world',
    'yellow',
    'zone',
  ];

  static String? _mnemonic;
  static String? _saplingViewingKey;
  static String? _orchardViewingKey;
  static String? _saplingSpendingKey;
  static String? _orchardSpendingKey;
  static int _diversifierIndex = 0;
  static DecoyAddressEntry? _currentAddress;
  static final List<DecoyAddressEntry> _addressHistory = [];

  static String mnemonic() {
    return _mnemonic ??= _generateMnemonic();
  }

  static String saplingViewingKey() {
    return _saplingViewingKey ??= _generateBech32Like('zxviews1', 96);
  }

  static String orchardViewingKey() {
    return _orchardViewingKey ??= _generateBech32Like('uview1', 96);
  }

  static String saplingSpendingKey() {
    return _saplingSpendingKey ??= _generateBech32Like('secret1', 96);
  }

  static String orchardSpendingKey() {
    return _orchardSpendingKey ??= _generateBech32Like('secret1', 96);
  }

  static DecoyAddressEntry currentAddress() {
    if (_currentAddress == null) {
      _currentAddress = _createAddressEntry();
    }
    return _currentAddress!;
  }

  static DecoyAddressEntry generateNextAddress() {
    _currentAddress = _createAddressEntry();
    return _currentAddress!;
  }

  static List<DecoyAddressEntry> addressHistory() {
    if (_addressHistory.isEmpty) {
      _currentAddress = _currentAddress ?? _createAddressEntry();
    }
    return List.unmodifiable(_addressHistory);
  }

  static List<KeyGroupInfo> keyGroups() {
    return [
      KeyGroupInfo(
        id: 1,
        label: 'Default wallet keys',
        keyType: KeyTypeInfo.seed,
        spendable: true,
        hasSapling: true,
        hasOrchard: true,
        birthdayHeight: 3_700_000,
        createdAt: DateTime.now().millisecondsSinceEpoch ~/ 1000,
      ),
    ];
  }

  static KeyExportInfo exportKeyGroup(int keyId) {
    return KeyExportInfo(
      keyId: keyId,
      saplingViewingKey: saplingViewingKey(),
      orchardViewingKey: orchardViewingKey(),
      saplingSpendingKey: saplingSpendingKey(),
      orchardSpendingKey: orchardSpendingKey(),
    );
  }

  static Random _initRandom() {
    try {
      return Random.secure();
    } catch (_) {
      return Random();
    }
  }

  static DecoyAddressEntry _createAddressEntry() {
    final address = 'zs1${_randomChars(75)}';
    final entry = DecoyAddressEntry(
      address: address,
      createdAt: DateTime.now(),
      index: _diversifierIndex,
    );
    _diversifierIndex += 1;
    _addressHistory.insert(0, entry);
    return entry;
  }

  static String _generateMnemonic() {
    final words = List<String>.generate(
      24,
      (_) => _wordList[_random.nextInt(_wordList.length)],
    );
    return words.join(' ');
  }

  static String _generateBech32Like(String prefix, int length) {
    return prefix + _randomChars(length);
  }

  static String _randomChars(int length) {
    final buffer = StringBuffer();
    for (var i = 0; i < length; i += 1) {
      buffer.write(_bech32Chars[_random.nextInt(_bech32Chars.length)]);
    }
    return buffer.toString();
  }
}
