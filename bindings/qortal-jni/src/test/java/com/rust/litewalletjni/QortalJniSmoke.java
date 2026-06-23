package com.rust.litewalletjni;

import java.util.Base64;

/** Minimal host/JNI linkage test with no Qortal Core dependencies. */
public final class QortalJniSmoke {
    private QortalJniSmoke() {}

    private static String jsonStringField(String json, String field) {
        String prefix = "\"" + field + "\":\"";
        int start = json.indexOf(prefix);
        int end = json.indexOf('"', start + prefix.length());
        if (start < 0 || end < 0) {
            throw new AssertionError("Could not parse " + field + " from: " + json);
        }
        return json.substring(start + prefix.length(), end);
    }

    public static void main(String[] args) {
        String library = System.getProperty("qortal.jni.library");
        if (library == null || library.isBlank()) {
            throw new IllegalArgumentException("qortal.jni.library is required");
        }
        System.load(library);

        if (!"OK".equals(LiteWalletJni.initlogging())) {
            throw new AssertionError("Legacy logging initialization response changed");
        }
        String generatedSeed = LiteWalletJni.getseedphrase();
        if (!generatedSeed.contains("\"seedPhrase\"")) {
            throw new AssertionError("Fresh seed generation failed: " + generatedSeed);
        }
        String rawEntropySeed = LiteWalletJni.getseedphrasefromentropy(
                "0123456789abcdef0123456789abcdef");
        if (!rawEntropySeed.contains("\"seedPhrase\"")) {
            throw new AssertionError("Raw entropy conversion failed: " + rawEntropySeed);
        }

        byte[] entropy = new byte[32];
        for (int i = 0; i < entropy.length; i++) {
            entropy[i] = 7;
        }
        String response = LiteWalletJni.getseedphrasefromentropyb64(
                Base64.getEncoder().encodeToString(entropy));
        if (!response.contains("\"seedPhrase\"")) {
            throw new AssertionError("Unexpected JNI response: " + response);
        }
        String seedPrefix = "\"seedPhrase\":\"";
        int seedStart = response.indexOf(seedPrefix);
        int seedEnd = response.indexOf('"', seedStart + seedPrefix.length());
        if (seedStart < 0 || seedEnd < 0) {
            throw new AssertionError("Could not parse deterministic seed: " + response);
        }
        String seed = response.substring(seedStart + seedPrefix.length(), seedEnd);
        String validSeed = LiteWalletJni.checkseedphrase(seed);
        if (!validSeed.contains("\"checkSeedPhrase\":\"Ok\"")) {
            throw new AssertionError("Legacy seed validation response changed: " + validSeed);
        }
        String invalidSeed = LiteWalletJni.checkseedphrase("not a mnemonic");
        if (!invalidSeed.contains("\"checkSeedPhrase\":\"Error\"")) {
            throw new AssertionError("Invalid seed validation response changed: " + invalidSeed);
        }

        String storage = System.getProperty("qortal.jni.storage");
        if (storage == null || storage.isBlank()) {
            throw new IllegalArgumentException("qortal.jni.storage is required");
        }
        String configured = LiteWalletJni.configurestorage(storage, "qortal-smoke-passphrase");
        if (!configured.contains("\"initialized\":true")) {
            throw new AssertionError("Storage configuration failed: " + configured);
        }
        String initialized = LiteWalletJni.initfromseed(
                "https://127.0.0.1:1/", "", seed, "100000", "", "");
        if (!initialized.contains("\"seed\"")
                || !initialized.contains("\"birthday\":100000")) {
            throw new AssertionError("Seed restore failed: " + initialized);
        }
        String walletId = jsonStringField(initialized, "wallet_id");
        String height = LiteWalletJni.execute("height", "");
        if (!height.contains("\"height\":100000")) {
            throw new AssertionError("Initial birthday height was not preserved: " + height);
        }
        String legacySaplingAddress =
                "zs1ra3g8uphtg8ad7p8ye76pg06nr9rg5y8m5ycq40vpw4nvae6amehenaafv02g3dny9myxz7f60s";
        String exported = LiteWalletJni.execute("export", "");
        if (!exported.contains(legacySaplingAddress)
                || !exported.contains("\"private_key\"")) {
            throw new AssertionError("Legacy address/key export mismatch: " + exported);
        }
        String balance = LiteWalletJni.execute("balance", "");
        int saplingPosition = balance.indexOf(legacySaplingAddress);
        int orchardPosition = balance.indexOf("pirate1");
        if (saplingPosition < 0 || (orchardPosition >= 0 && orchardPosition < saplingPosition)) {
            throw new AssertionError("Qortal balance is not Sapling-first: " + balance);
        }
        String encryption = LiteWalletJni.execute("encryptionstatus", "");
        if (!encryption.contains("\"encrypted\":true")
                || !encryption.contains("\"locked\":false")) {
            throw new AssertionError("Encryption status shape changed: " + encryption);
        }
        String syncStatus = LiteWalletJni.execute("syncStatus", "");
        if (!syncStatus.contains("\"in_progress\":false")
                || !syncStatus.contains("\"scanned_height\":100000")) {
            throw new AssertionError("Idle sync status shape changed: " + syncStatus);
        }
        String transactions = LiteWalletJni.execute("list", "");
        if (!"[]".equals(transactions)) {
            throw new AssertionError("Fresh wallet transaction list changed: " + transactions);
        }

        String reopened = LiteWalletJni.initfromseed(
                "https://127.0.0.1:1/", "", seed, "100000", "", "");
        if (!reopened.contains("\"seed\"")) {
            throw new AssertionError("Existing wallet reopen failed: " + reopened);
        }
        String wallets = LiteWalletJni.invokeJson("{\"method\":\"list_wallets\"}", false);
        String walletMarker = "\"name\":\"Qortal ";
        int firstWallet = wallets.indexOf(walletMarker);
        int secondWallet = wallets.indexOf(walletMarker, firstWallet + walletMarker.length());
        if (firstWallet < 0 || secondWallet >= 0) {
            throw new AssertionError("Seed restore was not idempotent: " + wallets);
        }
        String migratedReopen = LiteWalletJni.initfromb64(
                "https://127.0.0.1:1/", "", "ignored-after-migration", "", "");
        if (!migratedReopen.contains("\"initalized\":true")
                || !migratedReopen.contains("\"error\":\"none\"")) {
            throw new AssertionError("Migrated database reopen failed: " + migratedReopen);
        }
        String saveMarker = new String(Base64.getDecoder().decode(LiteWalletJni.save()));
        if (!saveMarker.contains("pirate-unified-wallet-sqlite")) {
            throw new AssertionError("Unexpected persistence marker: " + saveMarker);
        }

        byte[] secondEntropy = new byte[32];
        for (int i = 0; i < secondEntropy.length; i++) {
            secondEntropy[i] = 8;
        }
        String secondSeedResponse = LiteWalletJni.getseedphrasefromentropyb64(
                Base64.getEncoder().encodeToString(secondEntropy));
        int secondSeedStart = secondSeedResponse.indexOf(seedPrefix);
        int secondSeedEnd = secondSeedResponse.indexOf(
                '"', secondSeedStart + seedPrefix.length());
        if (secondSeedStart < 0 || secondSeedEnd < 0) {
            throw new AssertionError("Could not parse second deterministic seed");
        }
        String secondSeed = secondSeedResponse.substring(
                secondSeedStart + seedPrefix.length(), secondSeedEnd);
        String secondStorage = storage + "-second";
        String secondConfigured = LiteWalletJni.configurestorage(
                secondStorage, "qortal-smoke-second-passphrase");
        if (!secondConfigured.contains("\"initialized\":true")) {
            throw new AssertionError("Second storage configuration failed: " + secondConfigured);
        }
        String secondInitialized = LiteWalletJni.initfromseed(
                "https://127.0.0.1:1/", "", secondSeed, "110000", "", "");
        if (!secondInitialized.contains("\"birthday\":110000")) {
            throw new AssertionError("Second wallet restore failed: " + secondInitialized);
        }
        String secondExport = LiteWalletJni.execute("export", "");
        if (secondExport.contains(legacySaplingAddress)) {
            throw new AssertionError("Wallet state leaked into the second namespace");
        }

        LiteWalletJni.configurestorage(storage, "qortal-smoke-passphrase");
        String originalReopened = LiteWalletJni.initfromseed(
                "https://127.0.0.1:1/", "", seed, "100000", "", "");
        walletId = jsonStringField(originalReopened, "wallet_id");
        String originalExport = LiteWalletJni.execute("export", "");
        if (!originalExport.contains(legacySaplingAddress)) {
            throw new AssertionError("Original namespace did not reopen after wallet switch");
        }

        String request = "{\"method\":\"get_build_info\"}";
        String serviceResponse = LiteWalletJni.invokeJson(request, false);
        if (!serviceResponse.contains("\"ok\":true")) {
            throw new AssertionError("JSON service invocation failed: " + serviceResponse);
        }

        String syncStarted = LiteWalletJni.execute("sync", "");
        if (!syncStarted.contains("\"result\":\"success\"")) {
            throw new AssertionError("Namespaced wallet could not initialize sync: " + syncStarted);
        }
        String cancelSync = LiteWalletJni.invokeJson(
                "{\"method\":\"cancel_sync\",\"wallet_id\":\"" + walletId + "\"}", false);
        if (!cancelSync.contains("\"ok\":true")) {
            throw new AssertionError("Could not cancel smoke sync: " + cancelSync);
        }
    }
}
