export type SyncMode = 'Compact' | 'Deep'
export type SynchronizerStatus = 'STOPPED' | 'SYNCING' | 'SYNCED'

export interface WalletMeta {
  id: string
  name: string
  createdAt: number
  watchOnly: boolean
  birthdayHeight: number
  networkType?: 'mainnet' | 'testnet' | 'regtest' | null
}

export interface SynchronizerConfig {
  syncMode?: SyncMode
  syncingPollIntervalMs?: number
  syncedPollIntervalMs?: number
  errorPollIntervalMs?: number
  transactionLimit?: number | null
}

export interface SynchronizerSnapshot {
  walletId: string
  alias: string
  status: SynchronizerStatus
  progressPercent: number
  syncStatus: any
  latestBirthdayHeight: number | null
  balance: any
  transactions: any[]
  updatedAtMillis: number | null
  lastError: Error | null
}

export interface SynchronizerCallbacks {
  onStatusChanged?(event: { walletId: string; alias: string; name: SynchronizerStatus }): void
  onUpdate?(snapshot: SynchronizerSnapshot): void
  onError?(error: Error): void
}

export class PirateWalletAdvancedKeyManagement {
  listKeyGroups(walletId: string): Promise<any[]>
  exportKeyGroupKeys(walletId: string, keyId: number): Promise<any>
  importSpendingKey(
    requestOrWalletId: any,
    birthdayHeight?: number | null,
    saplingSpendingKey?: string | null,
    orchardSpendingKey?: string | null,
    label?: string | null
  ): Promise<number>
  exportSeed(walletId: string): Promise<string>
}

export class PirateWalletSynchronizer {
  constructor(sdk: PirateWalletSdk, walletId: string, config?: SynchronizerConfig)
  walletId: string
  config: SynchronizerConfig
  status: SynchronizerStatus
  progress: number
  syncStatus: any
  latestBirthdayHeight: number | null
  balance: any
  transactions: any[]
  lastError: Error | null
  currentSnapshot(): SynchronizerSnapshot
  isRunning(): boolean
  isSyncing(): boolean
  isComplete(): boolean
  start(): Promise<void>
  stop(): Promise<void>
  refresh(): Promise<SynchronizerSnapshot>
  close(): Promise<void>
  subscribe(callbacks?: SynchronizerCallbacks): () => void
}

export class PirateWalletSdk {
  advancedKeyManagement: PirateWalletAdvancedKeyManagement
  invoke(requestJson: string, pretty?: boolean): Promise<string>
  createSynchronizer(walletId: string, config?: SynchronizerConfig): PirateWalletSynchronizer
  buildInfoJson(pretty?: boolean): Promise<string>
  buildInfo(): Promise<any>
  walletRegistryExists(): Promise<boolean>
  listWallets(): Promise<WalletMeta[]>
  getActiveWalletId(): Promise<string | null>
  getActiveWallet(): Promise<WalletMeta | null>
  getWallet(walletId: string): Promise<WalletMeta | null>
  createWallet(requestOrName: any, birthdayHeight?: number | null): Promise<string>
  restoreWallet(requestOrName: any, mnemonic?: string, passphrase?: string | null, birthdayHeight?: number | null): Promise<string>
  importViewingWallet(requestOrName: any, saplingViewingKey?: string | null, orchardViewingKey?: string | null, birthdayHeight?: number): Promise<string>
  switchWallet(walletId: string): Promise<any>
  renameWallet(walletId: string, newName: string): Promise<any>
  deleteWallet(walletId: string): Promise<any>
  setWalletBirthdayHeight(walletId: string, birthdayHeight: number): Promise<any>
  getLatestBirthdayHeight(walletId: string): Promise<number | null>
  generateMnemonic(wordCount?: number | null): Promise<string>
  validateMnemonic(mnemonic: string): Promise<boolean>
  getNetworkInfo(): Promise<any>
  isValidShieldedAddr(address: string): Promise<boolean>
  validateAddress(address: string): Promise<any>
  validateConsensusBranch(walletId: string): Promise<any>
  formatAmount(arrrtoshis: number): Promise<string>
  parseAmount(arrr: string): Promise<number>
  getCurrentReceiveAddress(walletId: string): Promise<string>
  getCurrentAddress(walletId: string): Promise<string>
  getNextReceiveAddress(walletId: string): Promise<string>
  getNextAddress(walletId: string): Promise<string>
  listAddresses(walletId: string): Promise<any[]>
  listAddressBalances(walletId: string, keyId?: number | null): Promise<any[]>
  getBalance(walletId: string): Promise<any>
  getShieldedPoolBalances(walletId: string): Promise<any>
  getSpendabilityStatus(walletId: string): Promise<any>
  listTransactions(walletId: string, limit?: number | null): Promise<any[]>
  fetchTransactionMemo(walletId: string, txId: string, outputIndex?: number | null): Promise<string | null>
  getTransactionDetails(walletId: string, txId: string): Promise<any | null>
  getFeeInfo(): Promise<any>
  startSync(walletIdOrRequest: any, mode?: SyncMode): Promise<any>
  getSyncStatus(walletId: string): Promise<any>
  cancelSync(walletId: string): Promise<any>
  rescan(walletIdOrRequest: any, fromHeight?: number | null): Promise<any>
  buildTransaction(walletIdOrRequest: any, outputs?: any, fee?: number | null): Promise<any>
  signTransaction(walletId: string, pending: any): Promise<any>
  broadcastTransaction(signed: any): Promise<string>
  send(walletId: string, outputsOrOutput: any, fee?: number | null): Promise<string>
  exportSaplingViewingKey(walletId: string): Promise<string>
  exportOrchardViewingKey(walletId: string): Promise<string>
  importSaplingViewingKeyAsWatchOnly(requestOrName: any, saplingViewingKey?: string | null, birthdayHeight?: number | null): Promise<string>
  getWatchOnlyCapabilities(walletId: string): Promise<any>
}

export function createPirateWalletSdk(): PirateWalletSdk
