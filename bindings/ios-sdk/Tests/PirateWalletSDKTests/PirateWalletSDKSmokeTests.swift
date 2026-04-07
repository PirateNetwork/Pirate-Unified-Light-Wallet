import Foundation
import XCTest
@testable import PirateWalletSDK

final class PirateWalletSDKSmokeTests: XCTestCase {
    func testTypedSurfaceBuildInfoAndWalletMetadata() throws {
        let invoker = ScriptedInvoker(expectedCalls: [
            expected("get_build_info") { _ in
                try ok([
                    "version": "1.2.3",
                    "git_commit": "abc1234",
                    "build_date": "2026-03-20",
                    "rust_version": "1.86.0",
                    "target_triple": "aarch64-apple-ios",
                ])
            },
            expected("list_wallets") { _ in
                try ok([
                    [
                        "id": "wallet-1",
                        "name": "Primary",
                        "created_at": 1_710_000_000,
                        "watch_only": false,
                        "birthday_height": 234_567,
                        "network_type": "mainnet",
                    ],
                ])
            },
        ])

        let sdk = PirateWalletSDK(invoker: invoker)
        let info = try sdk.buildInfo()
        let wallets = try sdk.listWallets()

        XCTAssertEqual(info.version, "1.2.3")
        XCTAssertEqual(info.gitCommit, "abc1234")
        XCTAssertEqual(info.targetTriple, "aarch64-apple-ios")
        XCTAssertEqual(wallets.count, 1)
        XCTAssertEqual(wallets.first?.id, "wallet-1")
        XCTAssertEqual(wallets.first?.birthdayHeight, 234_567)
        XCTAssertEqual(wallets.first?.networkType, .mainnet)
        invoker.assertFinished()
    }

    func testAdvancedKeyManagementSurface() throws {
        let invoker = ScriptedInvoker(expectedCalls: [
            expected("list_key_groups") { request in
                XCTAssertEqual(request["wallet_id"] as? String, "wallet-1")
                return try ok([
                    [
                        "id": 7,
                        "label": "Imported bundle",
                        "key_type": "ImportedSpending",
                        "spendable": true,
                        "has_sapling": true,
                        "has_orchard": true,
                        "birthday_height": 2_345_678,
                        "created_at": 1_710_000_999,
                    ],
                ])
            },
            expected("export_key_group_keys") { request in
                XCTAssertEqual(request["wallet_id"] as? String, "wallet-1")
                XCTAssertEqual(request["key_id"] as? Int, 7)
                return try ok([
                    "key_id": 7,
                    "sapling_viewing_key": "zxviewsapling",
                    "orchard_viewing_key": "uvieworchard",
                    "sapling_spending_key": "secret-sapling",
                    "orchard_spending_key": "secret-orchard",
                ])
            },
            expected("import_spending_key") { request in
                XCTAssertEqual(request["wallet_id"] as? String, "wallet-1")
                XCTAssertEqual(request["sapling_key"] as? String, "secret-sapling")
                XCTAssertEqual(request["orchard_key"] as? String, "secret-orchard")
                XCTAssertEqual(request["label"] as? String, "Imported bundle")
                XCTAssertEqual(request["birthday_height"] as? Int, 2_345_678)
                return try ok(11)
            },
            expected("export_seed_raw") { request in
                XCTAssertEqual(request["wallet_id"] as? String, "wallet-1")
                return try ok(["alpha", "beta", "gamma"])
            },
        ])

        let sdk = PirateWalletSDK(invoker: invoker)
        let groups = try sdk.advancedKeyManagement.listKeyGroups(walletId: "wallet-1")
        let exportInfo = try sdk.advancedKeyManagement.exportKeyGroupKeys(walletId: "wallet-1", keyId: 7)
        let importedKeyId = try sdk.advancedKeyManagement.importSpendingKey(
            walletId: "wallet-1",
            birthdayHeight: 2_345_678,
            saplingSpendingKey: "secret-sapling",
            orchardSpendingKey: "secret-orchard"
        )
        let seedWords = try sdk.advancedKeyManagement.exportSeed(walletId: "wallet-1")

        XCTAssertEqual(groups.count, 1)
        XCTAssertEqual(groups.first?.id, 7)
        XCTAssertEqual(groups.first?.keyType, .importedSpending)
        XCTAssertEqual(exportInfo.saplingViewingKey, "zxviewsapling")
        XCTAssertEqual(exportInfo.orchardViewingKey, "uvieworchard")
        XCTAssertEqual(exportInfo.saplingSpendingKey, "secret-sapling")
        XCTAssertEqual(exportInfo.orchardSpendingKey, "secret-orchard")
        XCTAssertEqual(importedKeyId, 11)
        XCTAssertEqual(seedWords, "alpha beta gamma")
        invoker.assertFinished()
    }

    func testAsyncTypedSurfaceBuildInfoAndWalletMetadata() async throws {
        let invoker = ScriptedInvoker(expectedCalls: [
            expected("get_build_info") { _ in
                try ok([
                    "version": "1.2.3",
                    "git_commit": "def5678",
                    "build_date": "2026-04-04",
                    "rust_version": "1.86.0",
                    "target_triple": "aarch64-apple-ios",
                ])
            },
            expected("list_wallets") { _ in
                try ok([
                    [
                        "id": "wallet-async",
                        "name": "Async Wallet",
                        "created_at": 1_710_000_100,
                        "watch_only": false,
                        "birthday_height": 345_678,
                        "network_type": "mainnet",
                    ],
                ])
            },
        ])

        let sdk = PirateWalletSDK(invoker: invoker)
        let info = try await sdk.buildInfoAsync()
        let wallets = try await sdk.listWalletsAsync()

        XCTAssertEqual(info.gitCommit, "def5678")
        XCTAssertEqual(wallets.first?.id, "wallet-async")
        XCTAssertEqual(wallets.first?.birthdayHeight, 345_678)
        invoker.assertFinished()
    }

    func testAdvancedKeyManagementAsyncSurface() async throws {
        let invoker = ScriptedInvoker(expectedCalls: [
            expected("list_key_groups") { request in
                XCTAssertEqual(request["wallet_id"] as? String, "wallet-async")
                return try ok([
                    [
                        "id": 9,
                        "label": "Async imported bundle",
                        "key_type": "ImportedSpending",
                        "spendable": true,
                        "has_sapling": true,
                        "has_orchard": true,
                        "birthday_height": 4_567_890,
                        "created_at": 1_710_000_222,
                    ],
                ])
            },
            expected("export_seed_raw") { request in
                XCTAssertEqual(request["wallet_id"] as? String, "wallet-async")
                return try ok(["delta", "echo", "foxtrot"])
            },
        ])

        let sdk = PirateWalletSDK(invoker: invoker)
        let groups = try await sdk.advancedKeyManagement.listKeyGroupsAsync(walletId: "wallet-async")
        let seedWords = try await sdk.advancedKeyManagement.exportSeedAsync(walletId: "wallet-async")

        XCTAssertEqual(groups.first?.id, 9)
        XCTAssertEqual(seedWords, "delta echo foxtrot")
        invoker.assertFinished()
    }

    @MainActor
    func testSynchronizerSmokeSurface() throws {
        let sdk = PirateWalletSDK(invoker: ScriptedInvoker(expectedCalls: []))
        let synchronizer = sdk.createSynchronizer(walletId: "wallet-smoke")

        XCTAssertEqual(synchronizer.walletId, "wallet-smoke")
        XCTAssertFalse(synchronizer.isRunning())
        XCTAssertFalse(synchronizer.isSyncing())
        XCTAssertFalse(synchronizer.isComplete())
        XCTAssertEqual(synchronizer.currentSnapshot().walletId, "wallet-smoke")
    }
}

private func expected(
    _ method: String,
    _ responder: @escaping ([String: Any]) throws -> String
) -> ExpectedCall {
    ExpectedCall(method: method, responder: responder)
}

private struct ExpectedCall {
    let method: String
    let responder: ([String: Any]) throws -> String
}

private final class ScriptedInvoker: PirateWalletNativeInvoker, @unchecked Sendable {
    private var remainingCalls: [ExpectedCall]

    init(expectedCalls: [ExpectedCall]) {
        self.remainingCalls = expectedCalls
    }

    func invoke(requestJson: String, pretty: Bool) throws -> String {
        let data = try XCTUnwrap(requestJson.data(using: .utf8))
        let requestObject = try JSONSerialization.jsonObject(with: data, options: [])
        let request = try XCTUnwrap(requestObject as? [String: Any])

        guard !remainingCalls.isEmpty else {
            XCTFail("Unexpected native call: \(requestJson)")
            throw PirateWalletSdkError.invalidJsonResponse
        }

        let expectedCall = remainingCalls.removeFirst()
        XCTAssertEqual(expectedCall.method, request["method"] as? String)
        XCTAssertFalse(pretty)
        return try expectedCall.responder(request)
    }

    func assertFinished(file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(remainingCalls.isEmpty, "Unconsumed expected calls: \(remainingCalls.map { $0.method })", file: file, line: line)
    }
}

private func ok(_ result: Any? = NoResult.shared) throws -> String {
    var envelope: [String: Any] = ["ok": true]
    if !(result is NoResult) {
        envelope["result"] = result ?? NSNull()
    }
    let data = try JSONSerialization.data(withJSONObject: envelope, options: [])
    return try XCTUnwrap(String(data: data, encoding: .utf8))
}

private final class NoResult {
    static let shared = NoResult()
    private init() {}
}
