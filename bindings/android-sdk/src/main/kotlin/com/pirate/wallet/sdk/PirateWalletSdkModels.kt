package com.pirate.wallet.sdk

public enum class AddressBookColorTag(
    internal val jsonValue: String,
) {
    None("None"),
    Red("Red"),
    Orange("Orange"),
    Yellow("Yellow"),
    Green("Green"),
    Blue("Blue"),
    Purple("Purple"),
    Pink("Pink"),
    Gray("Gray");

    public companion object {
        public fun fromJson(value: Any?): AddressBookColorTag = when (value?.toString()?.lowercase()) {
            "none" -> None
            "red" -> Red
            "orange" -> Orange
            "yellow" -> Yellow
            "green" -> Green
            "blue" -> Blue
            "purple" -> Purple
            "pink" -> Pink
            "gray" -> Gray
            else -> throw PirateWalletSdkException("Unknown address book color tag: $value")
        }
    }
}

public data class AddressInfo(
    val address: String,
    val diversifierIndex: Int,
    val label: String?,
    val createdAt: Long,
    val colorTag: AddressBookColorTag,
)

public data class AddressBalanceInfo(
    val address: String,
    val balance: Long,
    val spendable: Long,
    val pending: Long,
    val keyId: Long?,
    val addressId: Long,
    val label: String?,
    val createdAt: Long,
    val colorTag: AddressBookColorTag,
    val diversifierIndex: Int,
)

public data class SpendabilityStatus(
    val spendable: Boolean,
    val rescanRequired: Boolean,
    val targetHeight: Long,
    val anchorHeight: Long,
    val validatedAnchorHeight: Long,
    val repairQueued: Boolean,
    val reasonCode: String,
) {
    public fun isReadyToSpend(): Boolean = spendable
}

public data class NetworkInfo(
    val name: String,
    val coinType: Int,
    val rpcPort: Int,
    val defaultBirthday: Int,
)

public data class WatchOnlyCapabilities(
    val canViewIncoming: Boolean,
    val canViewOutgoing: Boolean,
    val canSpend: Boolean,
    val canExportSeed: Boolean,
    val canGenerateAddresses: Boolean,
    val isWatchOnly: Boolean,
)

public enum class KeyTypeInfo {
    Seed,
    ImportedSpending,
    ImportedViewing,
}

public data class KeyGroupInfo(
    val id: Long,
    val label: String?,
    val keyType: KeyTypeInfo,
    val spendable: Boolean,
    val hasSapling: Boolean,
    val hasOrchard: Boolean,
    val birthdayHeight: Long,
    val createdAt: Long,
)

public data class KeyExportInfo(
    val keyId: Long,
    val saplingViewingKey: String?,
    val orchardViewingKey: String?,
    val saplingSpendingKey: String?,
    val orchardSpendingKey: String?,
)

public data class ImportSpendingKeyRequest(
    val walletId: String,
    val saplingSpendingKey: String? = null,
    val orchardSpendingKey: String? = null,
    val label: String? = null,
    val birthdayHeight: Int,
)

public data class ImportWatchOnlyWalletRequest(
    val name: String,
    val saplingViewingKey: String,
    val birthdayHeight: Int,
)

public enum class ShieldedAddressType {
    Sapling,
    Orchard,
}

public data class AddressValidation(
    val isValid: Boolean,
    val addressType: ShieldedAddressType?,
    val reason: String?,
) {
    public fun isInvalid(): Boolean = !isValid
}

public data class ConsensusBranchValidation(
    val sdkBranchId: String?,
    val serverBranchId: String?,
    val isValid: Boolean,
    val hasServerBranch: Boolean,
    val hasSdkBranch: Boolean,
    val isServerNewer: Boolean,
    val isSdkNewer: Boolean,
    val errorMessage: String?,
)

public data class TransactionRecipient(
    val address: String,
    val pool: String,
    val amount: Long,
    val outputIndex: Int,
    val memo: String?,
)

public data class TransactionDetails(
    val txId: String,
    val height: Int?,
    val timestamp: Long,
    val amount: Long,
    val fee: Long,
    val confirmed: Boolean,
    val memo: String?,
    val recipients: List<TransactionRecipient>,
)
