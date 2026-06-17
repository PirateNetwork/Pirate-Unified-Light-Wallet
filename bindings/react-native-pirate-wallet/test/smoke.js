const assert = require('assert')
const {
  PirateWalletSdk
} = require('../src/index.js')

function ok(result) {
  const envelope = { ok: true }
  if (result !== undefined) {
    envelope.result = result === null ? null : result
  }
  return JSON.stringify(envelope)
}

function createMockNativeModule() {
  const calls = []

  return {
    calls,
    async configureAccountStorage(accountId, passphrase, storagePath) {
      assert.strictEqual(accountId, 'edge-account-a')
      assert.strictEqual(passphrase, 'EdgeAccountSecretPassphrase123!')
      assert.strictEqual(storagePath, '/tmp/pirate-wallet/edge-account-a')
      calls.push('configure_wallet_storage')
      return ok(null)
    },
    async invoke(requestJson) {
      const request = JSON.parse(requestJson)
      calls.push(request.method)
      switch (request.method) {
        case 'get_build_info':
          return ok({
            version: '1.2.3',
            git_commit: 'abc1234',
            build_date: '2026-03-20',
            rust_version: '1.86.0',
            target_triple: 'react-native-smoke'
          })
        case 'list_wallets':
          return ok([
            {
              id: 'wallet-1',
              name: 'Primary',
              created_at: 1710000000,
              watch_only: false,
              birthday_height: 345678,
              network_type: 'mainnet'
            }
          ])
        case 'get_active_wallet':
          return ok('wallet-1')
        case 'get_balance':
          return ok({ total: '1000', spendable: '900', pending: '100' })
        case 'format_amount':
          assert.strictEqual(request.arrrtoshis, '9007199254740993')
          return ok('90071992.54740993')
        case 'parse_amount':
          return ok('9007199254740993')
        case 'build_tx':
          assert.strictEqual(request.outputs[0].amount, '9007199254740993')
          assert.strictEqual(request.fee_opt, '1000')
          return ok({
            id: 'pending-1',
            outputs: request.outputs,
            total_amount: '9007199254740993',
            fee: '1000',
            change: '0',
            input_total: '9007199254741993',
            num_inputs: 1,
            expiry_height: 123456,
            created_at: 1710000001
          })
        case 'sign_tx':
          assert.strictEqual(request.pending.total_amount, '9007199254740993')
          assert.strictEqual(request.pending.totalAmount, undefined)
          assert.strictEqual(request.pending.input_total, '9007199254741993')
          return ok({
            txid: 'tx-1',
            raw: [1, 2, 3],
            size: 3
          })
        case 'broadcast_tx':
          assert.strictEqual(request.signed.txid, 'tx-1')
          return ok('tx-1')
        case 'sync_status':
          return ok({
            local_height: 120,
            target_height: 240,
            percent: 50,
            eta: 120,
            stage: 'Notes',
            last_checkpoint: 96,
            blocks_per_second: 4.5,
            notes_decrypted: 42,
            last_batch_ms: 900
          })
        case 'list_transactions':
          return ok([])
        case 'start_sync':
        case 'cancel_sync':
          return ok(null)
        case 'list_key_groups':
          return ok([
            {
              id: 7,
              label: 'Imported bundle',
              key_type: 'ImportedSpending',
              spendable: true,
              has_sapling: true,
              has_orchard: true,
              birthday_height: 2345678,
              created_at: 1710000999
            }
          ])
        case 'export_key_group_keys':
          return ok({
            key_id: 7,
            sapling_viewing_key: 'zxviewsapling',
            orchard_viewing_key: 'uvieworchard',
            sapling_spending_key: 'secret-sapling',
            orchard_spending_key: 'secret-orchard'
          })
        case 'import_spending_key':
          return ok(11)
        case 'export_seed_raw':
          return ok('alpha beta gamma')
        default:
          throw new Error(`Unexpected method in smoke test: ${request.method}`)
      }
    }
  }
}

async function main() {
  const nativeModule = createMockNativeModule()
  const sdk = new PirateWalletSdk(nativeModule)

  await sdk.configureAccountStorage({
    accountId: 'edge-account-a',
    passphrase: 'EdgeAccountSecretPassphrase123!',
    storagePath: '/tmp/pirate-wallet/edge-account-a'
  })

  const buildInfo = await sdk.buildInfo()
  assert.strictEqual(buildInfo.version, '1.2.3')

  const wallets = await sdk.listWallets()
  assert.strictEqual(wallets.length, 1)
  assert.strictEqual(wallets[0].id, 'wallet-1')

  const activeWallet = await sdk.getActiveWallet()
  assert.strictEqual(activeWallet.id, 'wallet-1')

  const latestBirthdayHeight = await sdk.getLatestBirthdayHeight('wallet-1')
  assert.strictEqual(latestBirthdayHeight, 345678)

  const groups = await sdk.advancedKeyManagement.listKeyGroups('wallet-1')
  assert.strictEqual(groups.length, 1)

  const keyExport = await sdk.advancedKeyManagement.exportKeyGroupKeys('wallet-1', 7)
  assert.strictEqual(keyExport.saplingSpendingKey, 'secret-sapling')

  const importedKeyId = await sdk.advancedKeyManagement.importSpendingKey(
    'wallet-1',
    2345678,
    'secret-sapling',
    'secret-orchard'
  )
  assert.strictEqual(importedKeyId, 11)

  const seedWords = await sdk.advancedKeyManagement.exportSeed('wallet-1')
  assert.strictEqual(seedWords, 'alpha beta gamma')

  const formatted = await sdk.formatAmount(9007199254740993n)
  assert.strictEqual(formatted, '90071992.54740993')

  const parsed = await sdk.parseAmount('90071992.54740993')
  assert.strictEqual(parsed, '9007199254740993')

  const pending = await sdk.buildTransaction(
    'wallet-1',
    { addr: 'zs1receiver', amount: '9007199254740993' },
    1000
  )
  assert.strictEqual(pending.totalAmount, '9007199254740993')
  assert.strictEqual(pending.outputs[0].amount, '9007199254740993')

  const signed = await sdk.signTransaction('wallet-1', pending)
  const txid = await sdk.broadcastTransaction(signed)
  assert.strictEqual(txid, 'tx-1')

  const synchronizer = sdk.createSynchronizer('wallet-1')
  const snapshot = await synchronizer.refresh()
  assert.strictEqual(snapshot.walletId, 'wallet-1')
  assert.strictEqual(snapshot.status, 'SYNCING')
  assert.strictEqual(snapshot.progressPercent, 50)
  assert.strictEqual(snapshot.latestBirthdayHeight, 345678)
  assert.strictEqual(snapshot.balance.total, '1000')

  await synchronizer.start()
  await synchronizer.close()
  assert(nativeModule.calls.includes('start_sync'))
  assert(nativeModule.calls.includes('cancel_sync'))
}

main().catch(error => {
  console.error(error)
  process.exit(1)
})
