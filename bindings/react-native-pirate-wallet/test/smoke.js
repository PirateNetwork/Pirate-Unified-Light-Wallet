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
    async configureSecureAccountStorage(accountId, storagePath) {
      assert.strictEqual(accountId, 'edge-account-secure')
      assert.strictEqual(storagePath, null)
      calls.push('configure_secure_wallet_storage')
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
        case 'get_lightd_endpoint':
          assert.strictEqual(request.wallet_id, 'wallet-1')
          return ok('https://lightd1.pirate.black:443')
        case 'get_lightd_endpoint_config':
          assert.strictEqual(request.wallet_id, 'wallet-1')
          return ok({
            host: 'lightd1.pirate.black',
            port: 443,
            use_tls: true,
            tls_pin: null,
            label: 'Primary',
            automatic_failover: true,
            failover_endpoints: ['https://lightwalletd1.cryptoforge.cc:443'],
            is_configured: true
          })
        case 'get_lightd_endpoint_pool_diagnostics':
          return ok({
            wallet_id: 'wallet-1',
            configured_endpoint: 'https://lightd1.pirate.black:443',
            active_endpoint: 'https://lightwalletd1.cryptoforge.cc:443',
            automatic_failover: true,
            endpoints: [{
              endpoint: 'https://lightwalletd1.cryptoforge.cc:443',
              healthy: true,
              active: true,
              tip_height: 4200000,
              latency_ms: 95,
              reason: null
            }]
          })
        case 'set_lightd_endpoint':
          assert.strictEqual(request.wallet_id, 'wallet-1')
          assert.strictEqual(request.url, 'https://lightd1.pirate.black:443')
          assert.strictEqual(request.tls_pin_opt, 'base64-spki-pin')
          return ok({ acknowledged: true })
        case 'set_lightd_endpoint_pool':
          assert.strictEqual(request.wallet_id, 'wallet-1')
          assert.strictEqual(request.url, 'https://lightd1.pirate.black:443')
          assert.strictEqual(request.tls_pin_opt, undefined)
          assert.deepStrictEqual(request.failover_endpoints, [
            'https://lightwalletd1.cryptoforge.cc:443',
            'https://pirate.mathnodes.com:443'
          ])
          return ok({ acknowledged: true })
        case 'test_node':
          assert.strictEqual(request.url, 'https://lightwalletd1.cryptoforge.cc:443')
          assert.strictEqual(request.tls_pin, undefined)
          return ok({
            success: true,
            latest_block_height: 4200000,
            transport_mode: 'Direct',
            tls_enabled: true,
            tls_pin_matched: null,
            expected_pin: null,
            actual_pin: 'observed-pin',
            error_message: null,
            response_time_ms: 95,
            server_version: 'lightwalletd',
            chain_name: 'main'
          })
        case 'current_receive_address':
          assert.strictEqual(request.wallet_id, 'wallet-1')
          return ok('pirate1current')
        case 'next_receive_address':
          assert.strictEqual(request.wallet_id, 'wallet-1')
          return ok('pirate1next')
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
          assert.strictEqual(request.wallet_id, 'wallet-1')
          assert.strictEqual(request.signed.txid, 'tx-1')
          return ok('tx-1')
        case 'get_spendability_status':
          return ok({
            spendable: false,
            rescan_required: false,
            target_height: 4200001,
            anchor_height: 4199990,
            validated_anchor_height: 4199980,
            repair_queued: true,
            reason_code: 'ERR_WITNESS_REPAIR_QUEUED'
          })
        case 'enable_wallet_signing_protection':
        case 'unlock_wallet_signing':
          assert.strictEqual(request.wallet_id, 'wallet-1')
          assert.strictEqual(request.session_credential, 'edge-account-session-secret')
          return ok({ protection_enabled: true, unlocked: true })
        case 'lock_wallet_signing':
          return ok({ protection_enabled: true, unlocked: false })
        case 'lock_all_wallet_signing':
          return ok({ acknowledged: true })
        case 'get_wallet_signing_status':
          return ok({ protection_enabled: true, unlocked: false })
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
              has_ironwood: true,
              birthday_height: 2345678,
              created_at: 1710000999
            }
          ])
        case 'export_key_group_keys':
          return ok({
            key_id: 7,
            sapling_viewing_key: 'zxviewsapling',
            ironwood_viewing_key: 'uviewironwood',
            sapling_spending_key: 'secret-sapling',
            ironwood_spending_key: 'secret-ironwood'
          })
        case 'import_spending_key':
          return ok(11)
        case 'export_seed_raw':
          return ok('alpha beta gamma')
        case 'export_ironwood_payment_disclosure':
          assert.strictEqual(request.wallet_id, 'wallet-1')
          assert.strictEqual(request.txid, 'ironwood-tx')
          assert.strictEqual(request.action_index, 2)
          return ok('idisctest1ironwoodproof')
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
  await sdk.configureSecureAccountStorage({ accountId: 'edge-account-secure' })

  const buildInfo = await sdk.buildInfo()
  assert.strictEqual(buildInfo.version, '1.2.3')

  const wallets = await sdk.listWallets()
  assert.strictEqual(wallets.length, 1)
  assert.strictEqual(wallets[0].id, 'wallet-1')

  const activeWallet = await sdk.getActiveWallet()
  assert.strictEqual(activeWallet.id, 'wallet-1')

  const latestBirthdayHeight = await sdk.getLatestBirthdayHeight('wallet-1')
  assert.strictEqual(latestBirthdayHeight, 345678)

  const endpoint = await sdk.getLightdEndpoint('wallet-1')
  assert.strictEqual(endpoint, 'https://lightd1.pirate.black:443')

  const endpointConfig = await sdk.getLightdEndpointConfig('wallet-1')
  assert.strictEqual(endpointConfig.automaticFailover, true)
  assert.deepStrictEqual(endpointConfig.failoverEndpoints, [
    'https://lightwalletd1.cryptoforge.cc:443'
  ])
  const poolDiagnostics = await sdk.getLightdEndpointPoolDiagnostics('wallet-1')
  assert.strictEqual(poolDiagnostics.activeEndpoint, 'https://lightwalletd1.cryptoforge.cc:443')
  assert.strictEqual(poolDiagnostics.endpoints[0].latencyMs, 95)

  const endpointTest = await sdk.testLightdEndpoint(
    'https://lightwalletd1.cryptoforge.cc:443'
  )
  assert.strictEqual(endpointTest.latestBlockHeight, 4200000)
  assert.strictEqual(endpointTest.responseTimeMs, 95)

  const endpointAck = await sdk.setLightdEndpoint({
    walletId: 'wallet-1',
    url: 'https://lightd1.pirate.black:443',
    tlsPin: 'base64-spki-pin'
  })
  assert.strictEqual(endpointAck.acknowledged, true)

  const poolAck = await sdk.setLightdEndpointPool({
    walletId: 'wallet-1',
    url: 'https://lightd1.pirate.black:443',
    failoverEndpoints: [
      'https://lightwalletd1.cryptoforge.cc:443',
      'https://pirate.mathnodes.com:443'
    ]
  })
  assert.strictEqual(poolAck.acknowledged, true)

  assert.throws(
    () =>
      sdk.setLightdEndpointPool({
        walletId: 'wallet-1',
        url: 'https://lightd1.pirate.black:443',
        failoverEndpoints: 'not-an-array'
      }),
    /failoverEndpoints must be an array/
  )
  assert.throws(
    () => sdk.testLightdEndpoint('  '),
    /url must be a non-empty string/
  )

  assert.strictEqual(await sdk.getCurrentAddress('wallet-1'), 'pirate1current')
  assert.strictEqual(await sdk.getNextAddress('wallet-1'), 'pirate1next')

  const groups = await sdk.advancedKeyManagement.listKeyGroups('wallet-1')
  assert.strictEqual(groups.length, 1)

  const keyExport = await sdk.advancedKeyManagement.exportKeyGroupKeys('wallet-1', 7)
  assert.strictEqual(keyExport.saplingSpendingKey, 'secret-sapling')

  const importedKeyId = await sdk.advancedKeyManagement.importSpendingKey(
    'wallet-1',
    2345678,
    'secret-sapling',
    'secret-ironwood'
  )
  assert.strictEqual(importedKeyId, 11)

  const seedWords = await sdk.advancedKeyManagement.exportSeed('wallet-1')
  assert.strictEqual(seedWords, 'alpha beta gamma')

  const disclosure = await sdk.exportIronwoodPaymentDisclosure(
    'wallet-1',
    'ironwood-tx',
    2
  )
  assert.strictEqual(disclosure, 'idisctest1ironwoodproof')

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
  assert.throws(
    () => sdk.broadcastTransaction(signed),
    /broadcastTransaction.*walletId|walletId must be a non-empty string/
  )
  const txid = await sdk.broadcastTransaction('wallet-1', signed)
  assert.strictEqual(txid, 'tx-1')

  const spendability = await sdk.getSpendabilityStatus('wallet-1')
  assert.strictEqual(spendability.repairQueued, true)
  assert.strictEqual(spendability.reasonCode, 'ERR_WITNESS_REPAIR_QUEUED')
  const signing = await sdk.enableWalletSigningProtection(
    'wallet-1',
    'edge-account-session-secret'
  )
  assert.strictEqual(signing.unlocked, true)
  await sdk.unlockWalletSigning('wallet-1', 'edge-account-session-secret')
  assert.strictEqual((await sdk.lockWalletSigning('wallet-1')).unlocked, false)
  assert.strictEqual((await sdk.getWalletSigningStatus('wallet-1')).protectionEnabled, true)
  assert.strictEqual((await sdk.lockAllWalletSigning()).acknowledged, true)

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
