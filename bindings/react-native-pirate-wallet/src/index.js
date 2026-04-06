function getNativeModule() {
  let reactNative
  try {
    reactNative = require('react-native')
  } catch (error) {
    throw new Error(
      'react-native is not available. Pass a native module explicitly when testing outside React Native.'
    )
  }

  const nativeModule =
    reactNative &&
    reactNative.NativeModules &&
    reactNative.NativeModules.PirateWalletReactNative

  if (
    nativeModule == null ||
    typeof nativeModule.invoke !== 'function'
  ) {
    throw new Error(
      'PirateWalletReactNative native module is not linked. Rebuild the app and check native installation.'
    )
  }
  return nativeModule
}

function buildRequest(method, params = {}) {
  const request = { method }
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null) {
      request[key] = value
    }
  }
  return JSON.stringify(request)
}

function unwrapEnvelope(responseJson, method) {
  let envelope
  try {
    envelope = JSON.parse(responseJson)
  } catch (error) {
    throw new Error(`Invalid JSON response from native bridge for ${method}: ${String(error)}`)
  }

  if (!envelope || envelope.ok !== true) {
    const message =
      envelope && typeof envelope.error === 'string'
        ? envelope.error
        : `Native request failed for ${method}`
    throw new Error(message)
  }

  return Object.prototype.hasOwnProperty.call(envelope, 'result')
    ? camelize(envelope.result)
    : null
}

function camelize(value) {
  if (Array.isArray(value)) {
    return value.map(camelize)
  }

  if (value && typeof value === 'object') {
    const result = {}
    for (const [key, entry] of Object.entries(value)) {
      const camelKey = key.replace(/_([a-z])/g, (_, chr) => chr.toUpperCase())
      result[camelKey] = camelize(entry)
    }
    return result
  }

  return value
}

function isSyncComplete(syncStatus) {
  return (
    syncStatus != null &&
    typeof syncStatus.localHeight === 'number' &&
    typeof syncStatus.targetHeight === 'number' &&
    syncStatus.targetHeight > 0 &&
    syncStatus.localHeight >= syncStatus.targetHeight
  )
}

function isSyncing(syncStatus) {
  return (
    syncStatus != null &&
    typeof syncStatus.localHeight === 'number' &&
    typeof syncStatus.targetHeight === 'number' &&
    syncStatus.targetHeight > 0 &&
    syncStatus.localHeight < syncStatus.targetHeight
  )
}

function cloneCallbacks(callbacks) {
  return {
    onStatusChanged:
      callbacks && typeof callbacks.onStatusChanged === 'function'
        ? callbacks.onStatusChanged
        : null,
    onUpdate:
      callbacks && typeof callbacks.onUpdate === 'function'
        ? callbacks.onUpdate
        : null,
    onError:
      callbacks && typeof callbacks.onError === 'function'
        ? callbacks.onError
        : null
  }
}

function sanitizeAddressInfo(entry) {
  if (!entry || typeof entry !== 'object') {
    return entry
  }
  const { label, colorTag, ...rest } = entry
  return rest
}

function sanitizeAddressBalanceInfo(entry) {
  if (!entry || typeof entry !== 'object') {
    return entry
  }
  const { label, colorTag, ...rest } = entry
  return rest
}

function sanitizeKeyGroupInfo(entry) {
  if (!entry || typeof entry !== 'object') {
    return entry
  }
  const { label, ...rest } = entry
  return rest
}

class PirateWalletAdvancedKeyManagement {
  constructor(sdk) {
    this.sdk = sdk
  }

  async listKeyGroups(walletId) {
    const result = await this.sdk._call('list_key_groups', { wallet_id: walletId })
    return Array.isArray(result) ? result.map(sanitizeKeyGroupInfo) : result
  }

  async exportKeyGroupKeys(walletId, keyId) {
    return this.sdk._call('export_key_group_keys', {
      wallet_id: walletId,
      key_id: keyId
    })
  }

  async importSpendingKey(requestOrWalletId, birthdayHeight, saplingSpendingKey, orchardSpendingKey) {
    const request =
      typeof requestOrWalletId === 'object' && requestOrWalletId !== null
        ? requestOrWalletId
        : {
            walletId: requestOrWalletId,
            birthdayHeight,
            saplingSpendingKey,
            orchardSpendingKey
          }

    return this.sdk._call('import_spending_key', {
      wallet_id: request.walletId,
      sapling_key: request.saplingSpendingKey,
      orchard_key: request.orchardSpendingKey,
      birthday_height: request.birthdayHeight
    })
  }

  async exportSeed(walletId) {
    return this.sdk._call('export_seed_raw', {
      wallet_id: walletId
    })
  }
}

class PirateWalletSynchronizer {
  constructor(sdk, walletId, config = {}) {
    this.sdk = sdk
    this.walletId = walletId
    this.config = {
      syncMode: config.syncMode || 'Compact',
      syncingPollIntervalMs:
        config.syncingPollIntervalMs == null ? 1000 : config.syncingPollIntervalMs,
      syncedPollIntervalMs:
        config.syncedPollIntervalMs == null ? 5000 : config.syncedPollIntervalMs,
      errorPollIntervalMs:
        config.errorPollIntervalMs == null ? 5000 : config.errorPollIntervalMs,
      transactionLimit:
        config.transactionLimit == null ? null : config.transactionLimit
    }

    this.status = 'STOPPED'
    this.progress = 0
    this.syncStatus = null
    this.latestBirthdayHeight = null
    this.balance = null
    this.transactions = []
    this.lastError = null
    this.updatedAtMillis = null

    this._timer = null
    this._subscribers = new Set()
  }

  currentSnapshot() {
    return {
      walletId: this.walletId,
      alias: this.walletId,
      status: this.status,
      progressPercent: this.progress,
      syncStatus: this.syncStatus,
      latestBirthdayHeight: this.latestBirthdayHeight,
      balance: this.balance,
      transactions: this.transactions,
      updatedAtMillis: this.updatedAtMillis,
      lastError: this.lastError
    }
  }

  isRunning() {
    return this._timer !== null
  }

  isSyncing() {
    return this.status === 'SYNCING'
  }

  isComplete() {
    return isSyncComplete(this.syncStatus)
  }

  async start() {
    if (this._timer !== null) {
      return
    }

    const previousStatus = this.status
    this.status = 'SYNCING'
    this.lastError = null
    this.updatedAtMillis = Date.now()
    this._publish(previousStatus)

    try {
      await this.sdk.startSync(this.walletId, this.config.syncMode)
      this._schedule(0)
    } catch (error) {
      this.status = 'STOPPED'
      this.lastError = error
      this.updatedAtMillis = Date.now()
      this._publish(previousStatus)
      this._notifyError(error)
      throw error
    }
  }

  async stop() {
    const previousStatus = this.status
    const shouldCancelBackend = this._timer !== null || this.status !== 'STOPPED'
    if (this._timer !== null) {
      clearTimeout(this._timer)
      this._timer = null
    }

    this.status = 'STOPPED'
    this.updatedAtMillis = Date.now()
    this._publish(previousStatus)

    if (!shouldCancelBackend) {
      return
    }

    try {
      await this.sdk.cancelSync(this.walletId)
    } catch (error) {
      this.lastError = error
      this.updatedAtMillis = Date.now()
      this._notifyError(error)
      throw error
    }
  }

  async refresh() {
    return this._refreshOnce()
  }

  close() {
    return this.stop()
  }

  subscribe(callbacks = {}) {
    const subscriber = cloneCallbacks(callbacks)
    this._subscribers.add(subscriber)
    return () => {
      this._subscribers.delete(subscriber)
    }
  }

  _schedule(delayMs) {
    if (this._timer !== null) {
      clearTimeout(this._timer)
    }

    this._timer = setTimeout(() => {
      this._refreshOnce().catch(error => {
        this.lastError = error
        this.updatedAtMillis = Date.now()
        this._notifyError(error)
        if (this._timer !== null) {
          this._schedule(this.config.errorPollIntervalMs)
        }
      })
    }, delayMs)
  }

  async _refreshOnce() {
    const observedAtMillis = Date.now()
    const previousStatus = this.status

    const syncStatus = await this.sdk.getSyncStatus(this.walletId)
    const [balance, latestBirthdayHeight, transactions] = await Promise.all([
      this.sdk
        .getBalance(this.walletId)
        .catch(() => this.balance),
      this.sdk
        .getLatestBirthdayHeight(this.walletId)
        .catch(() => this.latestBirthdayHeight),
      this.sdk
        .listTransactions(this.walletId, this.config.transactionLimit)
        .catch(() => this.transactions)
    ])

    this.syncStatus = syncStatus
    this.latestBirthdayHeight = latestBirthdayHeight
    this.balance = balance
    this.transactions = transactions
    this.status = isSyncComplete(syncStatus) ? 'SYNCED' : 'SYNCING'
    this.progress =
      syncStatus && typeof syncStatus.percent === 'number'
        ? syncStatus.percent
        : this.status === 'SYNCED'
        ? 100
        : this.progress
    this.updatedAtMillis = observedAtMillis
    this.lastError = null
    this._publish(previousStatus)

    if (this._timer !== null) {
      this._schedule(
        isSyncing(syncStatus)
          ? this.config.syncingPollIntervalMs
          : this.config.syncedPollIntervalMs
      )
    }

    return this.currentSnapshot()
  }

  _publish(previousStatus) {
    const snapshot = this.currentSnapshot()
    if (previousStatus !== this.status) {
      for (const subscriber of this._subscribers) {
        if (subscriber.onStatusChanged) {
          subscriber.onStatusChanged({
            walletId: this.walletId,
            alias: this.walletId,
            name: this.status
          })
        }
      }
    }

    for (const subscriber of this._subscribers) {
      if (subscriber.onUpdate) {
        subscriber.onUpdate(snapshot)
      }
    }
  }

  _notifyError(error) {
    for (const subscriber of this._subscribers) {
      if (subscriber.onError) {
        subscriber.onError(error)
      }
    }
  }
}

class PirateWalletSdk {
  constructor(nativeModule = getNativeModule()) {
    this._native = nativeModule
    this.advancedKeyManagement = new PirateWalletAdvancedKeyManagement(this)
  }

  async invoke(requestJson, pretty = false) {
    return this._native.invoke(requestJson, pretty)
  }

  async _call(method, params = {}, pretty = false) {
    const response = await this.invoke(buildRequest(method, params), pretty)
    return unwrapEnvelope(response, method)
  }

  createSynchronizer(walletId, config = {}) {
    return new PirateWalletSynchronizer(this, walletId, config)
  }

  buildInfoJson(pretty = false) {
    return this.invoke(buildRequest('get_build_info'), pretty)
  }

  buildInfo() {
    return this._call('get_build_info')
  }

  walletRegistryExists() {
    return this._call('wallet_registry_exists')
  }

  listWallets() {
    return this._call('list_wallets')
  }

  getActiveWalletId() {
    return this._call('get_active_wallet')
  }

  async getActiveWallet() {
    const activeWalletId = await this.getActiveWalletId()
    if (!activeWalletId) {
      return null
    }
    return this.getWallet(activeWalletId)
  }

  async getWallet(walletId) {
    const wallets = await this.listWallets()
    return wallets.find(wallet => wallet.id === walletId) || null
  }

  createWallet(requestOrName, birthdayHeight = null) {
    const request =
      typeof requestOrName === 'object' && requestOrName !== null
        ? requestOrName
        : { name: requestOrName, birthdayHeight }

    return this._call('create_wallet', {
      name: request.name,
      birthday_opt: request.birthdayHeight
    })
  }

  restoreWallet(requestOrName, mnemonic, birthdayHeight = null) {
    const request =
      typeof requestOrName === 'object' && requestOrName !== null
        ? requestOrName
        : { name: requestOrName, mnemonic, birthdayHeight }

    return this._call('restore_wallet', {
      name: request.name,
      mnemonic: request.mnemonic,
      birthday_opt: request.birthdayHeight
    })
  }

  importViewingWallet(requestOrName, saplingViewingKey = null, orchardViewingKey = null, birthdayHeight) {
    const request =
      typeof requestOrName === 'object' && requestOrName !== null
        ? requestOrName
        : { name: requestOrName, saplingViewingKey, orchardViewingKey, birthdayHeight }

    return this._call('import_viewing_wallet', {
      name: request.name,
      sapling_viewing_key: request.saplingViewingKey,
      orchard_viewing_key: request.orchardViewingKey,
      birthday: request.birthdayHeight
    })
  }

  switchWallet(walletId) {
    return this._call('switch_wallet', { wallet_id: walletId })
  }

  renameWallet(walletId, newName) {
    return this._call('rename_wallet', { wallet_id: walletId, new_name: newName })
  }

  deleteWallet(walletId) {
    return this._call('delete_wallet', { wallet_id: walletId })
  }

  setWalletBirthdayHeight(walletId, birthdayHeight) {
    return this._call('set_wallet_birthday_height', {
      wallet_id: walletId,
      birthday_height: birthdayHeight
    })
  }

  async getLatestBirthdayHeight(walletId) {
    const wallet = await this.getWallet(walletId)
    return wallet ? wallet.birthdayHeight : null
  }

  generateMnemonic(wordCount = null) {
    return this._call('generate_mnemonic', { word_count: wordCount })
  }

  validateMnemonic(mnemonic) {
    return this._call('validate_mnemonic', { mnemonic })
  }

  getNetworkInfo() {
    return this._call('get_network_info')
  }

  isValidShieldedAddr(address) {
    return this._call('is_valid_shielded_address', { address })
  }

  validateAddress(address) {
    return this._call('validate_address', { address })
  }

  validateConsensusBranch(walletId) {
    return this._call('validate_consensus_branch', { wallet_id: walletId })
  }

  formatAmount(arrrtoshis) {
    return this._call('format_amount', { arrrtoshis })
  }

  parseAmount(arrr) {
    return this._call('parse_amount', { arrr })
  }

  getCurrentReceiveAddress(walletId) {
    return this.getCurrentAddress(walletId)
  }

  getCurrentAddress(walletId) {
    return this._call('current_receive_address', { wallet_id: walletId })
  }

  getNextReceiveAddress(walletId) {
    return this.getNextAddress(walletId)
  }

  getNextAddress(walletId) {
    return this._call('next_receive_address', { wallet_id: walletId })
  }

  listAddresses(walletId) {
    return this._call('list_addresses', { wallet_id: walletId }).then(result =>
      Array.isArray(result) ? result.map(sanitizeAddressInfo) : result
    )
  }

  listAddressBalances(walletId, keyId = null) {
    return this._call('list_address_balances', {
      wallet_id: walletId,
      key_id: keyId
    }).then(result => (Array.isArray(result) ? result.map(sanitizeAddressBalanceInfo) : result))
  }

  getBalance(walletId) {
    return this._call('get_balance', { wallet_id: walletId })
  }

  getShieldedPoolBalances(walletId) {
    return this._call('get_shielded_pool_balances', { wallet_id: walletId })
  }

  getSpendabilityStatus(walletId) {
    return this._call('get_spendability_status', { wallet_id: walletId })
  }

  listTransactions(walletId, limit = null) {
    return this._call('list_transactions', { wallet_id: walletId, limit })
  }

  fetchTransactionMemo(walletId, txId, outputIndex = null) {
    return this._call('fetch_transaction_memo', {
      wallet_id: walletId,
      txid: txId,
      output_index: outputIndex
    })
  }

  getTransactionDetails(walletId, txId) {
    return this._call('get_transaction_details', {
      wallet_id: walletId,
      txid: txId
    })
  }

  getFeeInfo() {
    return this._call('get_fee_info')
  }

  startSync(walletIdOrRequest, mode = 'Compact') {
    const request =
      typeof walletIdOrRequest === 'object' && walletIdOrRequest !== null
        ? walletIdOrRequest
        : { walletId: walletIdOrRequest, mode }

    return this._call('start_sync', {
      wallet_id: request.walletId,
      mode: request.mode
    })
  }

  getSyncStatus(walletId) {
    return this._call('sync_status', { wallet_id: walletId })
  }

  cancelSync(walletId) {
    return this._call('cancel_sync', { wallet_id: walletId })
  }

  rescan(walletIdOrRequest, fromHeight = null) {
    const request =
      typeof walletIdOrRequest === 'object' && walletIdOrRequest !== null
        ? walletIdOrRequest
        : { walletId: walletIdOrRequest, fromHeight }

    return this._call('rescan', {
      wallet_id: request.walletId,
      from_height: request.fromHeight
    })
  }

  buildTransaction(walletIdOrRequest, outputs = null, fee = null) {
    let request
    if (typeof walletIdOrRequest === 'object' && walletIdOrRequest !== null && outputs == null) {
      request = walletIdOrRequest
    } else if (Array.isArray(outputs)) {
      request = { walletId: walletIdOrRequest, outputs, fee }
    } else {
      request = { walletId: walletIdOrRequest, outputs: [outputs], fee }
    }

    return this._call('build_tx', {
      wallet_id: request.walletId,
      outputs: request.outputs,
      fee_opt: request.fee
    })
  }

  signTransaction(walletId, pending) {
    return this._call('sign_tx', {
      wallet_id: walletId,
      pending
    })
  }

  broadcastTransaction(signed) {
    return this._call('broadcast_tx', { signed })
  }

  async send(walletId, outputsOrOutput, fee = null) {
    const outputs = Array.isArray(outputsOrOutput)
      ? outputsOrOutput
      : [outputsOrOutput]
    const pending = await this.buildTransaction(walletId, outputs, fee)
    const signed = await this.signTransaction(walletId, pending)
    return this.broadcastTransaction(signed)
  }

  exportSaplingViewingKey(walletId) {
    return this._call('export_sapling_viewing_key', { wallet_id: walletId })
  }

  exportOrchardViewingKey(walletId) {
    return this._call('export_orchard_viewing_key', { wallet_id: walletId })
  }

  importSaplingViewingKeyAsWatchOnly(requestOrName, saplingViewingKey = null, birthdayHeight = null) {
    const request =
      typeof requestOrName === 'object' && requestOrName !== null
        ? requestOrName
        : { name: requestOrName, saplingViewingKey, birthdayHeight }

    return this._call('import_sapling_viewing_key_as_watch_only', {
      name: request.name,
      sapling_viewing_key: request.saplingViewingKey,
      birthday_height: request.birthdayHeight
    })
  }

  getWatchOnlyCapabilities(walletId) {
    return this._call('get_watch_only_capabilities', { wallet_id: walletId })
  }
}

function createPirateWalletSdk() {
  return new PirateWalletSdk()
}

module.exports = {
  PirateWalletSdk,
  PirateWalletSynchronizer,
  PirateWalletAdvancedKeyManagement,
  createPirateWalletSdk
}
