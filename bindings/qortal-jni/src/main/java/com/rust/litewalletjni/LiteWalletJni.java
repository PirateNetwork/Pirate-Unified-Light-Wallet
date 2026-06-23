package com.rust.litewalletjni;

/** Native entry points provided by the Pirate Unified Wallet Qortal library. */
public final class LiteWalletJni {
    private LiteWalletJni() {}

    /** Select and unlock a wallet-specific encrypted SQLite namespace. */
    public static native String configurestorage(String baseDir, String passphrase);

    public static native String initlogging();
    public static native String initnew(
            String serverUri,
            String params,
            String saplingOutputBase64,
            String saplingSpendBase64);
    public static native String initfromseed(
            String serverUri,
            String params,
            String seed,
            String birthday,
            String saplingOutputBase64,
            String saplingSpendBase64);
    public static native String initfromb64(
            String serverUri,
            String params,
            String dataBase64,
            String saplingOutputBase64,
            String saplingSpendBase64);
    public static native String save();
    public static native String execute(String command, String args);
    public static native String getseedphrase();
    public static native String getseedphrasefromentropy(String entropy);
    public static native String getseedphrasefromentropyb64(String entropyBase64);
    public static native String checkseedphrase(String input);

    /** Direct access to the versioned wallet-service JSON contract. */
    public static native String invokeJson(String requestJson, boolean pretty);
}
