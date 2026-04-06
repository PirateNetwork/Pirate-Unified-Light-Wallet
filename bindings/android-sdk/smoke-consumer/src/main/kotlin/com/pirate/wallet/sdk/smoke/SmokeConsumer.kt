package com.pirate.wallet.sdk.smoke

import com.pirate.wallet.sdk.BuildInfo
import com.pirate.wallet.sdk.ImportSpendingKeyRequest
import com.pirate.wallet.sdk.PirateWalletSdk
import com.pirate.wallet.sdk.PirateWalletSynchronizer
import com.pirate.wallet.sdk.SyncMode
import com.pirate.wallet.sdk.TransactionOutput

@Suppress("UNUSED_PARAMETER")
class SmokeConsumer(
    private val sdk: PirateWalletSdk,
) {
    fun compileSurface(walletId: String) {
        val synchronizer = sdk.createSynchronizer(
            walletId = walletId,
            config = PirateWalletSynchronizer.Config(syncMode = SyncMode.Compact),
        )
        synchronizer.currentSnapshot()
        synchronizer.isRunning()
        synchronizer.isSyncing()
        synchronizer.isComplete()

        val output = TransactionOutput(
            address = "zs1smoketestaddress",
            amount = 1_000L,
            memo = "sdk-smoke",
        )

        sdk.buildInfoJson()
        val buildInfo: BuildInfo = sdk.buildInfo()
        sdk.getLatestBirthdayHeight(walletId)
        sdk.getShieldedPoolBalances(walletId)
        sdk.getSpendabilityStatus(walletId)
        sdk.listTransactions(walletId, limit = 10)
        sdk.getTransactionDetails(walletId, txId = "deadbeef")
        sdk.fetchTransactionMemo(walletId, txId = "deadbeef", outputIndex = 0)
        sdk.buildTransaction(walletId, output)

        sdk.advancedKeyManagement.listKeyGroups(walletId)
        sdk.advancedKeyManagement.exportKeyGroupKeys(walletId, keyId = 1L)
        sdk.advancedKeyManagement.importSpendingKey(
            ImportSpendingKeyRequest(
                walletId = walletId,
                saplingSpendingKey = null,
                orchardSpendingKey = null,
                birthdayHeight = 1,
            ),
        )
        sdk.advancedKeyManagement.exportSeed(walletId)

        check(buildInfo.version.isNotEmpty())
    }
}
