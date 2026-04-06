package com.pirate.wallet.sdk

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.ArrayDeque

class PirateWalletSdkTest {
    @Test
    fun `buildInfo parses the typed facade response`() {
        val invoker = ScriptedInvoker(
            expect("get_build_info") {
                ok(
                    JSONObject()
                        .put("version", "1.2.3")
                        .put("git_commit", "abc1234")
                        .put("build_date", "2026-03-20")
                        .put("rust_version", "1.86.0")
                        .put("target_triple", "aarch64-linux-android"),
                )
            },
        )

        val sdk = PirateWalletSdk(invoker)
        val info = sdk.buildInfo()

        assertEquals("1.2.3", info.version)
        assertEquals("abc1234", info.gitCommit)
        assertEquals("2026-03-20", info.buildDate)
        assertEquals("1.86.0", info.rustVersion)
        assertEquals("aarch64-linux-android", info.targetTriple)
        invoker.assertFinished()
    }

    @Test
    fun `wallet listing and active wallet resolve through the typed facade`() {
        val walletsJson = JSONArray()
            .put(walletJson(id = "wallet-1", name = "Primary", networkType = "mainnet"))
            .put(
                walletJson(
                    id = "wallet-2",
                    name = "Watch",
                    watchOnly = true,
                    birthdayHeight = 345_678,
                    networkType = "testnet",
                ),
            )

        val invoker = ScriptedInvoker(
            expect("list_wallets") { ok(walletsJson) },
            expect("get_active_wallet") { ok("wallet-2") },
            expect("list_wallets") { ok(walletsJson) },
        )

        val sdk = PirateWalletSdk(invoker)
        val wallets = sdk.listWallets()
        val activeWallet = sdk.getActiveWallet()

        assertEquals(2, wallets.size)
        assertEquals("wallet-1", wallets[0].id)
        assertEquals("Primary", wallets[0].name)
        assertFalse(wallets[0].watchOnly)
        assertEquals(NetworkType.Mainnet, wallets[0].networkType)
        assertEquals("wallet-2", activeWallet?.id)
        assertEquals("Watch", activeWallet?.name)
        assertTrue(activeWallet?.watchOnly == true)
        assertEquals(345_678, activeWallet?.birthdayHeight)
        assertEquals(NetworkType.Testnet, activeWallet?.networkType)
        invoker.assertFinished()
    }

    @Test
    fun `latest birthday height resolves from wallet metadata`() {
        val walletsJson = JSONArray()
            .put(
                walletJson(
                    id = "wallet-2",
                    name = "Watch",
                    watchOnly = true,
                    birthdayHeight = 345_678,
                    networkType = "testnet",
                ),
            )

        val invoker = ScriptedInvoker(
            expect("list_wallets") { ok(walletsJson) },
        )

        val sdk = PirateWalletSdk(invoker)
        val latestBirthdayHeight = sdk.getLatestBirthdayHeight("wallet-2")

        assertEquals(345_678, latestBirthdayHeight)
        invoker.assertFinished()
    }

    @Test
    fun `sync status parsing exposes typed progress helpers`() {
        val invoker = ScriptedInvoker(
            expect("sync_status") { request ->
                assertEquals("wallet-sync", request.getString("wallet_id"))
                ok(
                    JSONObject()
                        .put("local_height", 120L)
                        .put("target_height", 240L)
                        .put("percent", 50.0)
                        .put("eta", 125L)
                        .put("stage", "Notes")
                        .put("last_checkpoint", 96L)
                        .put("blocks_per_second", 4.5)
                        .put("notes_decrypted", 42L)
                        .put("last_batch_ms", 900L),
                )
            },
        )

        val sdk = PirateWalletSdk(invoker)
        val status = sdk.getSyncStatus("wallet-sync")

        assertEquals(120L, status.localHeight)
        assertEquals(240L, status.targetHeight)
        assertEquals(50.0, status.percent, 0.0)
        assertEquals(125L, status.eta)
        assertEquals(SyncStage.Notes, status.stage)
        assertEquals(96L, status.lastCheckpoint)
        assertEquals(4.5, status.blocksPerSecond, 0.0)
        assertEquals(42L, status.notesDecrypted)
        assertEquals(900L, status.lastBatchMs)
        assertTrue(status.isSyncing())
        assertFalse(status.isComplete())
        assertEquals("2m 5s", status.etaFormatted())
        assertEquals("Scanning Notes", status.stageName())
        invoker.assertFinished()
    }

    @Test
    fun `advanced shielded pool balances parse correctly`() {
        val invoker = ScriptedInvoker(
            expect("get_shielded_pool_balances") { request ->
                assertEquals("wallet-1", request.getString("wallet_id"))
                ok(
                    JSONObject()
                        .put(
                            "sapling",
                            JSONObject()
                                .put("total", 10_000L)
                                .put("spendable", 8_000L)
                                .put("pending", 2_000L),
                        )
                        .put(
                            "orchard",
                            JSONObject()
                                .put("total", 25_000L)
                                .put("spendable", 20_000L)
                                .put("pending", 5_000L),
                        ),
                )
            },
        )

        val sdk = PirateWalletSdk(invoker)
        val balances = sdk.getShieldedPoolBalances("wallet-1")

        assertEquals(10_000L, balances.sapling.total)
        assertEquals(8_000L, balances.sapling.spendable)
        assertEquals(2_000L, balances.sapling.pending)
        assertEquals(25_000L, balances.orchard.total)
        assertEquals(20_000L, balances.orchard.spendable)
        assertEquals(5_000L, balances.orchard.pending)
        invoker.assertFinished()
    }

    @Test
    fun `shielded address validation and consensus branch parsing are typed`() {
        val invoker = ScriptedInvoker(
            expect("is_valid_shielded_address") { request ->
                assertEquals("pirate1testaddress", request.getString("address"))
                ok(true)
            },
            expect("validate_address") { request ->
                assertEquals("zs1validaddress", request.getString("address"))
                ok(
                    JSONObject()
                        .put("is_valid", true)
                        .put("address_type", "Sapling"),
                )
            },
            expect("validate_consensus_branch") { request ->
                assertEquals("wallet-1", request.getString("wallet_id"))
                ok(
                    JSONObject()
                        .put("sdk_branch_id", "26a7270a")
                        .put("server_branch_id", "26a7270a")
                        .put("is_valid", true)
                        .put("has_server_branch", true)
                        .put("has_sdk_branch", true)
                        .put("is_server_newer", false)
                        .put("is_sdk_newer", false)
                        .put("error_message", JSONObject.NULL),
                )
            },
        )

        val sdk = PirateWalletSdk(invoker)

        assertTrue(sdk.isValidShieldedAddr("pirate1testaddress"))
        val addressValidation = sdk.validateAddress("zs1validaddress")
        assertTrue(addressValidation.isValid)
        assertEquals(ShieldedAddressType.Sapling, addressValidation.addressType)
        assertEquals(null, addressValidation.reason)

        val branchValidation = sdk.validateConsensusBranch("wallet-1")
        assertTrue(branchValidation.isValid)
        assertEquals("26a7270a", branchValidation.sdkBranchId)
        assertEquals("26a7270a", branchValidation.serverBranchId)
        assertTrue(branchValidation.hasServerBranch)
        assertTrue(branchValidation.hasSdkBranch)
        invoker.assertFinished()
    }

    @Test
    fun `transaction details exposes memo and recipients through a typed API`() {
        val invoker = ScriptedInvoker(
            expect("get_transaction_details") { request ->
                assertEquals("wallet-1", request.getString("wallet_id"))
                assertEquals("deadbeef", request.getString("txid"))
                ok(
                    JSONObject()
                        .put("txid", "deadbeef")
                        .put("height", 321)
                        .put("timestamp", 1_710_000_123L)
                        .put("amount", -55_000L)
                        .put("fee", 1_000L)
                        .put("confirmed", true)
                        .put("memo", "hello")
                        .put(
                            "recipients",
                            JSONArray().put(
                                JSONObject()
                                    .put("address", "zs1recipient")
                                    .put("pool", "sapling")
                                    .put("amount", 54_000L)
                                    .put("output_index", 0)
                                    .put("memo", "hello"),
                            ),
                        ),
                )
            },
        )

        val sdk = PirateWalletSdk(invoker)
        val details = sdk.getTransactionDetails("wallet-1", "deadbeef")

        requireNotNull(details)
        assertEquals("deadbeef", details.txId)
        assertEquals(321, details.height)
        assertEquals(1_710_000_123L, details.timestamp)
        assertEquals(-55_000L, details.amount)
        assertEquals(1_000L, details.fee)
        assertTrue(details.confirmed)
        assertEquals("hello", details.memo)
        assertEquals(1, details.recipients.size)
        assertEquals("zs1recipient", details.recipients.first().address)
        assertEquals("sapling", details.recipients.first().pool)
        invoker.assertFinished()
    }

    @Test
    fun `advanced key management exposes typed sapling and orchard key operations`() {
        val invoker = ScriptedInvoker(
            expect("list_key_groups") { request ->
                assertEquals("wallet-1", request.getString("wallet_id"))
                ok(
                    JSONArray().put(
                        JSONObject()
                            .put("id", 7L)
                            .put("label", "Imported bundle")
                            .put("key_type", "ImportedSpending")
                            .put("spendable", true)
                            .put("has_sapling", true)
                            .put("has_orchard", true)
                            .put("birthday_height", 2_345_678L)
                            .put("created_at", 1_710_000_999L),
                    ),
                )
            },
            expect("export_key_group_keys") { request ->
                assertEquals("wallet-1", request.getString("wallet_id"))
                assertEquals(7L, request.getLong("key_id"))
                ok(
                    JSONObject()
                        .put("key_id", 7L)
                        .put("sapling_viewing_key", "zxviewsapling")
                        .put("orchard_viewing_key", "uvieworchard")
                        .put("sapling_spending_key", "secret-sapling")
                        .put("orchard_spending_key", "secret-orchard"),
                )
            },
            expect("import_spending_key") { request ->
                assertEquals("wallet-1", request.getString("wallet_id"))
                assertEquals("secret-sapling", request.getString("sapling_key"))
                assertEquals("secret-orchard", request.getString("orchard_key"))
                assertEquals("Imported bundle", request.getString("label"))
                assertEquals(2_345_678, request.getInt("birthday_height"))
                ok(11L)
            },
            expect("export_seed_raw") { request ->
                assertEquals("wallet-1", request.getString("wallet_id"))
                ok("alpha beta gamma")
            },
        )

        val sdk = PirateWalletSdk(invoker)
        val groups = sdk.advancedKeyManagement.listKeyGroups("wallet-1")
        val exportInfo = sdk.advancedKeyManagement.exportKeyGroupKeys("wallet-1", 7L)
        val importedKeyId = sdk.advancedKeyManagement.importSpendingKey(
            walletId = "wallet-1",
            birthdayHeight = 2_345_678,
            saplingSpendingKey = "secret-sapling",
            orchardSpendingKey = "secret-orchard",
            label = "Imported bundle",
        )
        val seedWords = sdk.advancedKeyManagement.exportSeed("wallet-1")

        assertEquals(1, groups.size)
        assertEquals(7L, groups.first().id)
        assertEquals(KeyTypeInfo.ImportedSpending, groups.first().keyType)
        assertTrue(groups.first().hasSapling)
        assertTrue(groups.first().hasOrchard)
        assertEquals("zxviewsapling", exportInfo.saplingViewingKey)
        assertEquals("uvieworchard", exportInfo.orchardViewingKey)
        assertEquals("secret-sapling", exportInfo.saplingSpendingKey)
        assertEquals("secret-orchard", exportInfo.orchardSpendingKey)
        assertEquals(11L, importedKeyId)
        assertEquals("alpha beta gamma", seedWords)
        invoker.assertFinished()
    }
}

private data class ExpectedCall(
    val method: String,
    val responder: (JSONObject) -> String,
)

private fun expect(method: String, responder: (JSONObject) -> String): ExpectedCall =
    ExpectedCall(method, responder)

private class ScriptedInvoker(
    vararg expectedCalls: ExpectedCall,
) : PirateWalletNativeInvoker {
    private val remainingCalls = ArrayDeque(expectedCalls.toList())
    private val prettyFlags = mutableListOf<Boolean>()

    override fun invoke(requestJson: String, pretty: Boolean): String {
        val request = JSONObject(requestJson)
        prettyFlags += pretty

        val expected = if (remainingCalls.isEmpty()) {
            throw AssertionError("Unexpected native call: $requestJson")
        } else {
            remainingCalls.removeFirst()
        }
        assertEquals(expected.method, request.getString("method"))
        return expected.responder(request)
    }

    fun assertFinished() {
        assertTrue("Unconsumed expected calls: $remainingCalls", remainingCalls.isEmpty())
        assertTrue("Typed facade methods should use pretty=false", prettyFlags.all { !it })
    }
}

private fun ok(result: Any? = NoResult): String {
    val envelope = JSONObject().put("ok", true)
    if (result !== NoResult) {
        envelope.put("result", result ?: JSONObject.NULL)
    }
    return envelope.toString()
}

private object NoResult

private fun walletJson(
    id: String,
    name: String,
    createdAt: Long = 1_710_000_000L,
    watchOnly: Boolean = false,
    birthdayHeight: Int = 123_456,
    networkType: String? = null,
): JSONObject = JSONObject()
    .put("id", id)
    .put("name", name)
    .put("created_at", createdAt)
    .put("watch_only", watchOnly)
    .put("birthday_height", birthdayHeight)
    .apply {
        networkType?.let { put("network_type", it) }
    }
