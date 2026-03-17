import Foundation
import BackgroundTasks
import UserNotifications
import UIKit
import Flutter

/**
 * Background sync manager for iOS
 * 
 * Handles background blockchain synchronization using:
 * - BGAppRefreshTask for quick updates
 * - BGProcessingTask for deep sync (daily, when idle + charging)
 * - Privacy-respecting network tunnel (Tor/I2P/SOCKS5)
 * - Local notifications for received funds
 * - All RPC calls routed through configured NetTunnel
 * 
 * Acceptance criteria:
 * - Balances update without foregrounding the app
 * - Disabling Tor causes clean failure notification
 */
@available(iOS 13.0, *)
class BackgroundSyncManager: NSObject {
    
    static let shared = BackgroundSyncManager()
    
    // MARK: - Task Identifiers (must match Info.plist BGTaskSchedulerPermittedIdentifiers)
    
    static let compactTaskIdentifier = "com.pirate.wallet.sync.compact"
    static let deepTaskIdentifier = "com.pirate.wallet.sync.deep"
    
    // MARK: - Method Channel for FFI
    
    private let channelName = "com.pirate.wallet/background_sync"
    private var methodChannel: FlutterMethodChannel?
    private var flutterEngine: FlutterEngine?
    
    // MARK: - Configuration
    
    private let defaults = UserDefaults.standard
    private let keychainService = "com.pirate.wallet.backgroundsync"
    
    private let lastCompactSyncKey = "lastCompactSync"
    private let lastDeepSyncKey = "lastDeepSync"
    private let tunnelModeKey = "tunnelMode"
    private let socks5UrlKey = "socks5Url"
    private let notificationsEnabledKey = "notificationsEnabled"
    private let activeWalletIdKey = "activeWalletId"
    private let syncPausedKey = "syncPaused"
    private let compactIntervalSecondsKey = "compactIntervalSeconds"
    private let deepIntervalSecondsKey = "deepIntervalSeconds"
    private let compactMaxDurationSecsKey = "compactMaxDurationSecs"
    private let deepMaxDurationSecsKey = "deepMaxDurationSecs"
    private let compactMaxBlocksKey = "compactMaxBlocks"
    private let deepMaxBlocksKey = "deepMaxBlocks"
    private let deepRequiresExternalPowerKey = "deepRequiresExternalPower"
    
    private let defaultCompactIntervalSeconds: TimeInterval = 24 * 60 * 60
    private let defaultDeepIntervalSeconds: TimeInterval = 24 * 60 * 60
    private let defaultCompactMaxDurationSecs = 25
    private let defaultDeepMaxDurationSecs = 300
    private let defaultCompactMaxBlocks = 250_000
    private let defaultDeepMaxBlocks = 5_000_000
    private let defaultDeepRequiresExternalPower = true
    
    // Tunnel modes (must match Rust TunnelMode enum)
    enum TunnelMode: String {
        case tor = "tor"
        case i2p = "i2p"
        case socks5 = "socks5"
        case direct = "direct"
    }
    
    private override init() {
        super.init()
    }

    private var compactIntervalSeconds: TimeInterval {
        let stored = defaults.double(forKey: compactIntervalSecondsKey)
        return stored > 0 ? stored : defaultCompactIntervalSeconds
    }

    private var deepIntervalSeconds: TimeInterval {
        let stored = defaults.double(forKey: deepIntervalSecondsKey)
        return stored > 0 ? stored : defaultDeepIntervalSeconds
    }

    private var compactMaxDurationSecs: Int {
        let stored = defaults.integer(forKey: compactMaxDurationSecsKey)
        return stored > 0 ? stored : defaultCompactMaxDurationSecs
    }

    private var deepMaxDurationSecs: Int {
        let stored = defaults.integer(forKey: deepMaxDurationSecsKey)
        return stored > 0 ? stored : defaultDeepMaxDurationSecs
    }

    private var compactMaxBlocks: Int {
        let stored = defaults.integer(forKey: compactMaxBlocksKey)
        return stored > 0 ? stored : defaultCompactMaxBlocks
    }

    private var deepMaxBlocks: Int {
        let stored = defaults.integer(forKey: deepMaxBlocksKey)
        return stored > 0 ? stored : defaultDeepMaxBlocks
    }

    private var deepRequiresExternalPower: Bool {
        if defaults.object(forKey: deepRequiresExternalPowerKey) == nil {
            return defaultDeepRequiresExternalPower
        }
        return defaults.bool(forKey: deepRequiresExternalPowerKey)
    }
    
    // MARK: - Flutter Engine Setup
    
    /**
     * Initialize Flutter engine for background FFI calls
     */
    private func initializeFlutterEngine() -> FlutterEngine {
        if let engine = flutterEngine {
            return engine
        }
        
        let engine = FlutterEngine(name: "BackgroundSyncEngine")
        engine.run(withEntrypoint: "backgroundSyncMain")
        GeneratedPluginRegistrant.register(with: engine)
        
        methodChannel = FlutterMethodChannel(
            name: channelName,
            binaryMessenger: engine.binaryMessenger
        )
        
        flutterEngine = engine
        return engine
    }
    
    /**
     * Clean up Flutter engine
     */
    private func cleanupFlutterEngine() {
        methodChannel = nil
        flutterEngine?.destroyContext()
        flutterEngine = nil
    }
    
    // MARK: - Registration
    
    /**
     * Register background tasks
     * Call this in AppDelegate.didFinishLaunchingWithOptions BEFORE app finishes launching
     */
    func registerBackgroundTasks() {
        // Register compact sync (BGAppRefreshTask - quick, frequent)
        let compactRegistered = BGTaskScheduler.shared.register(
            forTaskWithIdentifier: BackgroundSyncManager.compactTaskIdentifier,
            using: nil
        ) { [weak self] task in
            self?.handleCompactSync(task: task as! BGAppRefreshTask)
        }
        
        // Register deep sync (BGProcessingTask - thorough, requires power)
        let deepRegistered = BGTaskScheduler.shared.register(
            forTaskWithIdentifier: BackgroundSyncManager.deepTaskIdentifier,
            using: nil
        ) { [weak self] task in
            self?.handleDeepSync(task: task as! BGProcessingTask)
        }
        
        print("[BackgroundSync] Registered tasks - compact: \(compactRegistered), deep: \(deepRegistered)")
    }
    
    // MARK: - Scheduling
    
    /**
     * Schedule compact sync (BGAppRefreshTask)
     * iOS will execute when convenient (typically 15-30 minutes)
     * Limited to ~30 seconds execution time
     */
    func scheduleCompactSync(
        intervalMinutes: Int? = nil,
        maxDurationSecs: Int? = nil,
        maxBlocks: Int? = nil
    ) {
        if let intervalMinutes = intervalMinutes, intervalMinutes > 0 {
            defaults.set(TimeInterval(intervalMinutes * 60), forKey: compactIntervalSecondsKey)
        }
        if let maxDurationSecs = maxDurationSecs, maxDurationSecs > 0 {
            defaults.set(maxDurationSecs, forKey: compactMaxDurationSecsKey)
        }
        if let maxBlocks = maxBlocks, maxBlocks > 0 {
            defaults.set(maxBlocks, forKey: compactMaxBlocksKey)
        }

        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: BackgroundSyncManager.compactTaskIdentifier)
        let request = BGAppRefreshTaskRequest(identifier: BackgroundSyncManager.compactTaskIdentifier)
        request.earliestBeginDate = Date(timeIntervalSinceNow: compactIntervalSeconds)
        
        do {
            try BGTaskScheduler.shared.submit(request)
            print("[BackgroundSync] Scheduled compact sync for ~\(Int(compactIntervalSeconds/60))m from now")
        } catch BGTaskScheduler.Error.notPermitted {
            print("[BackgroundSync] Background refresh not permitted - check Info.plist and capabilities")
        } catch BGTaskScheduler.Error.tooManyPendingTaskRequests {
            print("[BackgroundSync] Too many pending requests - will retry later")
        } catch BGTaskScheduler.Error.unavailable {
            print("[BackgroundSync] Background tasks unavailable on this device")
        } catch {
            print("[BackgroundSync] Failed to schedule compact sync: \(error)")
        }
    }
    
    /**
     * Schedule deep sync (BGProcessingTask)
     * iOS will execute when device is idle, charging, and has WiFi
     * Can run for several minutes
     */
    func scheduleDeepSync(
        intervalHours: Int? = nil,
        maxDurationSecs: Int? = nil,
        maxBlocks: Int? = nil,
        requiresCharging: Bool? = nil
    ) {
        if let intervalHours = intervalHours, intervalHours > 0 {
            defaults.set(TimeInterval(intervalHours * 3600), forKey: deepIntervalSecondsKey)
        }
        if let maxDurationSecs = maxDurationSecs, maxDurationSecs > 0 {
            defaults.set(maxDurationSecs, forKey: deepMaxDurationSecsKey)
        }
        if let maxBlocks = maxBlocks, maxBlocks > 0 {
            defaults.set(maxBlocks, forKey: deepMaxBlocksKey)
        }
        if let requiresCharging = requiresCharging {
            defaults.set(requiresCharging, forKey: deepRequiresExternalPowerKey)
        }

        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: BackgroundSyncManager.deepTaskIdentifier)
        let request = BGProcessingTaskRequest(identifier: BackgroundSyncManager.deepTaskIdentifier)
        request.earliestBeginDate = Date(timeIntervalSinceNow: deepIntervalSeconds)
        request.requiresNetworkConnectivity = true
        request.requiresExternalPower = deepRequiresExternalPower
        
        do {
            try BGTaskScheduler.shared.submit(request)
            print("[BackgroundSync] Scheduled deep sync for ~\(Int(deepIntervalSeconds/3600))h from now (requires charging + network)")
        } catch BGTaskScheduler.Error.notPermitted {
            print("[BackgroundSync] Background processing not permitted - check capabilities")
        } catch BGTaskScheduler.Error.tooManyPendingTaskRequests {
            print("[BackgroundSync] Too many pending requests")
        } catch {
            print("[BackgroundSync] Failed to schedule deep sync: \(error)")
        }
    }
    
    /**
     * Schedule both sync tasks
     */
    func scheduleAllSyncs() {
        scheduleCompactSync()
        scheduleDeepSync()
    }
    
    /**
     * Cancel all scheduled background tasks
     */
    func cancelAllTasks() {
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: BackgroundSyncManager.compactTaskIdentifier)
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: BackgroundSyncManager.deepTaskIdentifier)
        print("[BackgroundSync] Cancelled all background tasks")
    }
    
    // MARK: - Task Handlers
    
    /**
     * Handle compact sync (BGAppRefreshTask)
     * Quick sync with ~30 second time limit
     */
    private func handleCompactSync(task: BGAppRefreshTask) {
        print("[BackgroundSync] Starting compact sync...")

        if defaults.bool(forKey: syncPausedKey) {
            print("[BackgroundSync] Background sync is paused")
            task.setTaskCompleted(success: true)
            return
        }
        
        // Schedule next compact sync immediately
        scheduleCompactSync()
        
        let walletId = defaults.string(forKey: activeWalletIdKey)
        let useRoundRobin = walletId == nil || walletId?.isEmpty == true
        if useRoundRobin {
            print("[BackgroundSync] No active wallet ID, using round-robin selection")
        }
        
        // Get tunnel configuration
        let tunnelConfig = getTunnelConfig()
        print("[BackgroundSync] Using tunnel: \(tunnelConfig.mode.rawValue)")
        
        // Create cancellation handler
        var syncTask: Task<Void, Never>?
        
        task.expirationHandler = {
            print("[BackgroundSync] Compact sync time expired, cancelling...")
            syncTask?.cancel()
        }
        
        // Execute sync
        syncTask = Task {
            do {
                let result = try await executeSync(
                    walletId: useRoundRobin ? nil : walletId,
                    mode: .compact,
                    maxDurationSecs: compactMaxDurationSecs,
                    maxBlocks: compactMaxBlocks,
                    tunnelConfig: tunnelConfig,
                    useRoundRobin: useRoundRobin
                )
                
                print("[BackgroundSync] Compact sync completed: \(result.blocksSynced) blocks in \(result.durationSecs)s")
                
                // Update last sync time
                updateLastSyncTime(mode: .compact)
                
                // Check for new transactions and notify
                if result.newTransactions > 0 {
                    await showReceivedFundsNotification(
                        count: result.newTransactions,
                        balance: result.newBalance ?? 0
                    )
                }
                
                task.setTaskCompleted(success: true)
            } catch let error as TorConnectionError {
                print("[BackgroundSync] Tor connection failed: \(error.message)")
                await showTorFailureNotification(message: error.message)
                task.setTaskCompleted(success: false)
            } catch let error as Socks5ConnectionError {
                print("[BackgroundSync] SOCKS5 connection failed: \(error.message)")
                await showNetworkErrorNotification(message: "SOCKS5 proxy failed: \(error.message)")
                task.setTaskCompleted(success: false)
            } catch is CancellationError {
                print("[BackgroundSync] Compact sync cancelled")
                task.setTaskCompleted(success: false)
            } catch {
                print("[BackgroundSync] Compact sync failed: \(error)")
                task.setTaskCompleted(success: false)
            }
        }
    }
    
    /**
     * Handle deep sync (BGProcessingTask)
     * Thorough sync with several minutes allowed
     */
    private func handleDeepSync(task: BGProcessingTask) {
        print("[BackgroundSync] Starting deep sync...")

        if defaults.bool(forKey: syncPausedKey) {
            print("[BackgroundSync] Background sync is paused")
            task.setTaskCompleted(success: true)
            return
        }
        
        // Schedule next deep sync
        scheduleDeepSync()
        
        let walletId = defaults.string(forKey: activeWalletIdKey)
        let useRoundRobin = walletId == nil || walletId?.isEmpty == true
        if useRoundRobin {
            print("[BackgroundSync] No active wallet ID, using round-robin selection")
        }
        
        // Get tunnel configuration
        let tunnelConfig = getTunnelConfig()
        print("[BackgroundSync] Using tunnel: \(tunnelConfig.mode.rawValue)")
        
        // Create cancellation handler
        var syncTask: Task<Void, Never>?
        
        task.expirationHandler = {
            print("[BackgroundSync] Deep sync time expired, cancelling...")
            syncTask?.cancel()
        }
        
        // Execute sync
        syncTask = Task {
            do {
                let result = try await executeSync(
                    walletId: useRoundRobin ? nil : walletId,
                    mode: .deep,
                    maxDurationSecs: deepMaxDurationSecs,
                    maxBlocks: deepMaxBlocks,
                    tunnelConfig: tunnelConfig,
                    useRoundRobin: useRoundRobin
                )
                
                print("[BackgroundSync] Deep sync completed: \(result.blocksSynced) blocks in \(result.durationSecs)s")
                
                // Update last sync time
                updateLastSyncTime(mode: .deep)
                
                // Check for new transactions and notify
                if result.newTransactions > 0 {
                    await showReceivedFundsNotification(
                        count: result.newTransactions,
                        balance: result.newBalance ?? 0
                    )
                }
                
                task.setTaskCompleted(success: true)
            } catch let error as TorConnectionError {
                print("[BackgroundSync] Tor connection failed: \(error.message)")
                await showTorFailureNotification(message: error.message)
                task.setTaskCompleted(success: false)
            } catch let error as Socks5ConnectionError {
                print("[BackgroundSync] SOCKS5 connection failed: \(error.message)")
                await showNetworkErrorNotification(message: "SOCKS5 proxy failed: \(error.message)")
                task.setTaskCompleted(success: false)
            } catch is CancellationError {
                print("[BackgroundSync] Deep sync cancelled")
                task.setTaskCompleted(success: false)
            } catch {
                print("[BackgroundSync] Deep sync failed: \(error)")
                task.setTaskCompleted(success: false)
            }
        }
    }
    
    // MARK: - Sync Execution
    
    /**
     * Execute sync via FFI bridge with tunnel configuration
     * All RPC calls are routed through the configured NetTunnel (Tor/SOCKS5/Direct)
     */
    private func executeSync(
        walletId: String?,
        mode: SyncMode,
        maxDurationSecs: Int,
        maxBlocks: Int,
        tunnelConfig: TunnelConfig,
        useRoundRobin: Bool
    ) async throws -> SyncResult {
        let startTime = Date()
        
        // Initialize Flutter engine for FFI calls
        let engine = initializeFlutterEngine()
        
        defer {
            // Clean up engine after sync
            cleanupFlutterEngine()
        }
        
        guard let channel = methodChannel else {
            throw NetworkTunnelError(message: "Failed to initialize FFI channel")
        }
        
        return try await withCheckedThrowingContinuation { continuation in
            let arguments: [String: Any?] = [
                "walletId": walletId,
                "mode": mode == .deep ? "deep" : "compact",
                "maxDurationSecs": maxDurationSecs,
                "maxBlocks": maxBlocks,
                "useRoundRobin": useRoundRobin,
                "tunnelMode": tunnelConfig.mode.rawValue,
                "socks5Url": tunnelConfig.socks5Url as Any
            ]
            
            channel.invokeMethod("executeBackgroundSync", arguments: arguments) { result in
                if let error = result as? FlutterError {
                    // Map error codes to specific exceptions
                    switch error.code {
                    case "TOR_CONNECTION_FAILED":
                        continuation.resume(throwing: TorConnectionError(
                            message: error.message ?? "Tor connection failed"
                        ))
                    case "SOCKS5_CONNECTION_FAILED":
                        continuation.resume(throwing: Socks5ConnectionError(
                            message: error.message ?? "SOCKS5 connection failed"
                        ))
                    case "NETWORK_ERROR":
                        continuation.resume(throwing: NetworkTunnelError(
                            message: error.message ?? "Network error"
                        ))
                    default:
                        continuation.resume(throwing: NetworkTunnelError(
                            message: "\(error.code): \(error.message ?? "Unknown error")"
                        ))
                    }
                    return
                }
                
                guard let resultMap = result as? [String: Any] else {
                    continuation.resume(throwing: NetworkTunnelError(
                        message: "Invalid sync result format"
                    ))
                    return
                }
                
                let duration = Int(Date().timeIntervalSince(startTime))
                
                // Check for tunnel errors in result
                if let errors = resultMap["errors"] as? [String] {
                    for error in errors {
                        if error.lowercased().contains("tor") {
                            continuation.resume(throwing: TorConnectionError(message: error))
                            return
                        }
                        if error.lowercased().contains("socks5") {
                            continuation.resume(throwing: Socks5ConnectionError(message: error))
                            return
                        }
                    }
                }
                
                let syncResult = SyncResult(
                    mode: mode,
                    blocksSynced: (resultMap["blocks_synced"] as? Int) ?? 0,
                    durationSecs: duration,
                    newTransactions: (resultMap["new_transactions"] as? Int) ?? 0,
                    newBalance: resultMap["new_balance"] as? UInt64,
                    tunnelUsed: (resultMap["tunnel_used"] as? String) ?? tunnelConfig.mode.rawValue
                )
                
                continuation.resume(returning: syncResult)
            }
        }
    }
    
    // MARK: - Tunnel Configuration
    
    /**
     * Get current tunnel configuration
     */
    func getTunnelConfig() -> TunnelConfig {
        let modeString = defaults.string(forKey: tunnelModeKey) ?? TunnelMode.tor.rawValue
        let mode = TunnelMode(rawValue: modeString) ?? .tor
        let legacySocks5Url = defaults.string(forKey: socks5UrlKey)
        let keychainSocks5Url = try? loadKeychainString(account: socks5UrlKey)
        let socks5Url = keychainSocks5Url ?? legacySocks5Url
        if keychainSocks5Url == nil, let legacy = legacySocks5Url {
            try? storeKeychainString(legacy, account: socks5UrlKey)
            defaults.removeObject(forKey: socks5UrlKey)
        }
        
        return TunnelConfig(
            mode: mode,
            socks5Url: mode == .socks5 ? socks5Url : nil
        )
    }
    
    /**
     * Set tunnel mode
     */
    func setTunnelMode(_ mode: TunnelMode, socks5Url: String? = nil) {
        defaults.set(mode.rawValue, forKey: tunnelModeKey)
        if mode == .socks5, let url = socks5Url {
            try? storeKeychainString(url, account: socks5UrlKey)
            defaults.removeObject(forKey: socks5UrlKey)
        } else {
            try? deleteKeychainString(account: socks5UrlKey)
            defaults.removeObject(forKey: socks5UrlKey)
        }
        print("[BackgroundSync] Set tunnel mode: \(mode.rawValue)")
    }
    
    /**
     * Set active wallet ID for background sync
     */
    func setActiveWalletId(_ walletId: String) {
        defaults.set(walletId, forKey: activeWalletIdKey)
        print("[BackgroundSync] Set active wallet ID: \(walletId)")
    }

    func clearActiveWalletId() {
        defaults.removeObject(forKey: activeWalletIdKey)
        print("[BackgroundSync] Cleared active wallet ID")
    }

    func setPaused(_ paused: Bool) {
        defaults.set(paused, forKey: syncPausedKey)
        print("[BackgroundSync] Set paused=\(paused)")
    }

    func runImmediateSync(
        mode: String,
        maxDurationSecs: Int? = nil,
        maxBlocks: Int? = nil
    ) async {
        guard !defaults.bool(forKey: syncPausedKey) else {
            print("[BackgroundSync] Immediate sync skipped because background sync is paused")
            return
        }
        let syncMode: SyncMode = mode.lowercased() == "deep" ? .deep : .compact
        let walletId = defaults.string(forKey: activeWalletIdKey)
        let useRoundRobin = walletId == nil || walletId?.isEmpty == true
        let tunnelConfig = getTunnelConfig()
        _ = try? await executeSync(
            walletId: useRoundRobin ? nil : walletId,
            mode: syncMode,
            maxDurationSecs: maxDurationSecs ?? (syncMode == .deep ? deepMaxDurationSecs : compactMaxDurationSecs),
            maxBlocks: maxBlocks ?? (syncMode == .deep ? deepMaxBlocks : compactMaxBlocks),
            tunnelConfig: tunnelConfig,
            useRoundRobin: useRoundRobin
        )
    }

    private func storeKeychainString(_ value: String, account: String) throws {
        let data = Data(value.utf8)
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: keychainService,
            kSecAttrAccount as String: account,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
            kSecValueData as String: data
        ]

        let status = SecItemAdd(query as CFDictionary, nil)
        if status == errSecDuplicateItem {
            let update: [String: Any] = [
                kSecValueData as String: data
            ]
            let queryUpdate: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: keychainService,
                kSecAttrAccount as String: account
            ]
            let updateStatus = SecItemUpdate(queryUpdate as CFDictionary, update as CFDictionary)
            if updateStatus != errSecSuccess {
                throw NSError(domain: "BackgroundSyncKeychainError", code: Int(updateStatus), userInfo: nil)
            }
        } else if status != errSecSuccess {
            throw NSError(domain: "BackgroundSyncKeychainError", code: Int(status), userInfo: nil)
        }
    }

    private func loadKeychainString(account: String) throws -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: keychainService,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        if status == errSecItemNotFound {
            return nil
        }
        if status != errSecSuccess {
            throw NSError(domain: "BackgroundSyncKeychainError", code: Int(status), userInfo: nil)
        }
        guard let data = item as? Data else {
            return nil
        }
        return String(data: data, encoding: .utf8)
    }

    private func deleteKeychainString(account: String) throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: keychainService,
            kSecAttrAccount as String: account
        ]
        let status = SecItemDelete(query as CFDictionary)
        if status != errSecSuccess && status != errSecItemNotFound {
            throw NSError(domain: "BackgroundSyncKeychainError", code: Int(status), userInfo: nil)
        }
    }
    
    // MARK: - Last Sync Tracking
    
    /**
     * Update last sync timestamp
     */
    private func updateLastSyncTime(mode: SyncMode) {
        let key = mode == .deep ? lastDeepSyncKey : lastCompactSyncKey
        defaults.set(Date(), forKey: key)
    }
    
    /**
     * Get last compact sync date
     */
    var lastCompactSync: Date? {
        defaults.object(forKey: lastCompactSyncKey) as? Date
    }
    
    /**
     * Get last deep sync date
     */
    var lastDeepSync: Date? {
        defaults.object(forKey: lastDeepSyncKey) as? Date
    }
    
    /**
     * Minutes since last compact sync (-1 if never)
     */
    var minutesSinceCompactSync: Int {
        guard let last = lastCompactSync else { return -1 }
        return Int(Date().timeIntervalSince(last) / 60)
    }
    
    /**
     * Hours since last deep sync (-1 if never)
     */
    var hoursSinceDeepSync: Int {
        guard let last = lastDeepSync else { return -1 }
        return Int(Date().timeIntervalSince(last) / 3600)
    }
    
    // MARK: - Notifications
    
    /**
     * Show local notification for received funds
     */
    private func showReceivedFundsNotification(count: Int, balance: UInt64) async {
        guard await refreshNotificationAuthorizationStatus() else {
            print("[BackgroundSync] Notifications disabled by user preference")
            return
        }
        
        let content = UNMutableNotificationContent()
        content.title = "Received \(count) Payment\(count > 1 ? "s" : "")"
        content.body = "New balance: \(formatBalance(zatoshis: balance)) ARRR"
        content.sound = .default
        content.badge = NSNumber(value: count)
        content.categoryIdentifier = "PIRATE_TRANSACTION_RECEIVED"
        content.threadIdentifier = "pirate_transactions"
        
        // Add custom data
        content.userInfo = [
            "type": "transaction_received",
            "count": count,
            "balance": balance
        ]
        
        let request = UNNotificationRequest(
            identifier: "pirate_tx_\(UUID().uuidString)",
            content: content,
            trigger: nil  // Deliver immediately
        )
        
        do {
            try await UNUserNotificationCenter.current().add(request)
            print("[BackgroundSync] Showed notification for \(count) new transaction(s)")
        } catch {
            print("[BackgroundSync] Failed to show notification: \(error)")
        }
    }
    
    /**
     * Show notification for Tor connection failure
     * This provides a clean user-facing message when Tor is disabled or fails
     */
    private func showTorFailureNotification(message: String) async {
        guard await refreshNotificationAuthorizationStatus() else {
            print("[BackgroundSync] Notifications unavailable, skipping Tor failure alert")
            return
        }
        let content = UNMutableNotificationContent()
        content.title = "Privacy Connection Failed"
        content.body = "Unable to connect via Tor. Tap to configure network settings."
        content.sound = .default
        content.categoryIdentifier = "PIRATE_NETWORK_ERROR"
        content.threadIdentifier = "pirate_network"
        
        content.userInfo = [
            "type": "tor_connection_failed",
            "message": message
        ]
        
        let request = UNNotificationRequest(
            identifier: "pirate_tor_error_\(UUID().uuidString)",
            content: content,
            trigger: nil
        )
        
        do {
            try await UNUserNotificationCenter.current().add(request)
            print("[BackgroundSync] Showed Tor failure notification")
        } catch {
            print("[BackgroundSync] Failed to show Tor failure notification: \(error)")
        }
    }
    
    /**
     * Show notification for general network errors
     */
    private func showNetworkErrorNotification(message: String) async {
        guard await refreshNotificationAuthorizationStatus() else {
            print("[BackgroundSync] Notifications unavailable, skipping network alert")
            return
        }
        let content = UNMutableNotificationContent()
        content.title = "Sync Connection Issue"
        content.body = message
        content.sound = nil  // Silent for non-critical errors
        content.categoryIdentifier = "PIRATE_NETWORK_ERROR"
        content.threadIdentifier = "pirate_network"
        
        let request = UNNotificationRequest(
            identifier: "pirate_network_error_\(UUID().uuidString)",
            content: content,
            trigger: nil
        )
        
        do {
            try await UNUserNotificationCenter.current().add(request)
            print("[BackgroundSync] Showed network error notification")
        } catch {
            print("[BackgroundSync] Failed to show network error notification: \(error)")
        }
    }
    
    /**
     * Request notification permissions
     */
    func requestNotificationPermissions() async -> Bool {
        do {
            let options: UNAuthorizationOptions = [.alert, .sound, .badge]
            let granted = try await UNUserNotificationCenter.current()
                .requestAuthorization(options: options)
            
            defaults.set(granted, forKey: notificationsEnabledKey)
            print("[BackgroundSync] Notification permission: \(granted)")
            
            // Register notification categories
            if granted {
                await registerNotificationCategories()
            }
            
            return granted
        } catch {
            print("[BackgroundSync] Failed to request notification permission: \(error)")
            return false
        }
    }
    
    /**
     * Register notification categories for actions
     */
    private func registerNotificationCategories() async {
        let viewAction = UNNotificationAction(
            identifier: "VIEW_TRANSACTION",
            title: "View",
            options: [.foreground]
        )
        
        let dismissAction = UNNotificationAction(
            identifier: "DISMISS",
            title: "Dismiss",
            options: []
        )
        
        let settingsAction = UNNotificationAction(
            identifier: "OPEN_SETTINGS",
            title: "Settings",
            options: [.foreground]
        )
        
        let transactionCategory = UNNotificationCategory(
            identifier: "PIRATE_TRANSACTION_RECEIVED",
            actions: [viewAction, dismissAction],
            intentIdentifiers: [],
            options: [.customDismissAction]
        )
        
        let networkErrorCategory = UNNotificationCategory(
            identifier: "PIRATE_NETWORK_ERROR",
            actions: [settingsAction, dismissAction],
            intentIdentifiers: [],
            options: [.customDismissAction]
        )
        
        UNUserNotificationCenter.current().setNotificationCategories([
            transactionCategory,
            networkErrorCategory
        ])
    }
    
    /**
     * Check notification authorization status
     */
    func checkNotificationStatus() async -> UNAuthorizationStatus {
        let settings = await UNUserNotificationCenter.current().notificationSettings()
        return settings.authorizationStatus
    }

    @discardableResult
    func refreshNotificationAuthorizationStatus() async -> Bool {
        let status = await checkNotificationStatus()
        let granted: Bool
        if status == .authorized || status == .provisional {
            granted = true
        } else if #available(iOS 14.0, *), status == .ephemeral {
            granted = true
        } else {
            granted = false
        }
        defaults.set(granted, forKey: notificationsEnabledKey)
        return granted
    }
    
    // MARK: - Status
    
    /**
     * Get sync status for UI
     */
    func getSyncStatus() -> SyncStatusInfo {
        return SyncStatusInfo(
            lastCompactSync: lastCompactSync,
            lastDeepSync: lastDeepSync,
            tunnelMode: getTunnelConfig().mode,
            minutesSinceCompact: minutesSinceCompactSync,
            hoursSinceDeep: hoursSinceDeepSync
        )
    }
    
    // MARK: - Helpers
    
    /**
     * Format balance for display
     */
    private func formatBalance(zatoshis: UInt64) -> String {
        let arrr = Double(zatoshis) / 100_000_000.0
        return String(format: "%.4f", arrr)
    }
}

// MARK: - Models

@available(iOS 13.0, *)
enum SyncMode {
    case compact
    case deep
}

@available(iOS 13.0, *)
struct SyncResult {
    let mode: SyncMode
    let blocksSynced: Int
    let durationSecs: Int
    let newTransactions: Int
    let newBalance: UInt64?
    let tunnelUsed: String
}

@available(iOS 13.0, *)
struct TunnelConfig {
    let mode: BackgroundSyncManager.TunnelMode
    let socks5Url: String?
}

@available(iOS 13.0, *)
struct SyncStatusInfo {
    let lastCompactSync: Date?
    let lastDeepSync: Date?
    let tunnelMode: BackgroundSyncManager.TunnelMode
    let minutesSinceCompact: Int
    let hoursSinceDeep: Int
}

// MARK: - Errors

@available(iOS 13.0, *)
class NetworkTunnelError: Error {
    let message: String
    
    init(message: String) {
        self.message = message
    }
}

@available(iOS 13.0, *)
class TorConnectionError: NetworkTunnelError {
    override init(message: String) {
        super.init(message: message)
    }
}

@available(iOS 13.0, *)
class Socks5ConnectionError: NetworkTunnelError {
    override init(message: String) {
        super.init(message: message)
    }
}

// MARK: - AppDelegate Extension

/**
 * Extension for AppDelegate to initialize background sync
 * 
 * Usage in AppDelegate.swift:
 * ```swift
 * func application(_ application: UIApplication, 
 *                  didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
 *     if #available(iOS 13.0, *) {
 *         setupBackgroundSync()
 *     }
 *     // ... rest of setup
 *     return true
 * }
 * ```
 */
@available(iOS 13.0, *)
extension AppDelegate {
    
    func setupBackgroundSync() {
        // Register background tasks (MUST be done before app finishes launching)
        BackgroundSyncManager.shared.registerBackgroundTasks()

        Task {
            _ = await BackgroundSyncManager.shared.refreshNotificationAuthorizationStatus()
        }
        
        // Schedule initial tasks
        BackgroundSyncManager.shared.scheduleAllSyncs()
        
        print("[AppDelegate] Background sync initialized")
    }
}

// MARK: - Debug Helpers (Development only)

#if DEBUG
@available(iOS 13.0, *)
extension BackgroundSyncManager {
    
    /**
     * Trigger background task for testing
     * Run in Xcode debugger: `e -l objc -- (void)[[BGTaskScheduler sharedScheduler] _simulateLaunchForTaskWithIdentifier:@"com.pirate.wallet.sync.compact"]`
     */
    func debugTriggerCompactSync() {
        print("[BackgroundSync] DEBUG: Would trigger compact sync")
        // In simulator, use debugger command above
    }
    
    func debugTriggerDeepSync() {
        print("[BackgroundSync] DEBUG: Would trigger deep sync")
        // In simulator, use debugger command above
    }
    
    func debugPrintStatus() {
        let status = getSyncStatus()
        print("""
        [BackgroundSync] Status:
          - Tunnel Mode: \(status.tunnelMode.rawValue)
          - Last Compact: \(status.lastCompactSync?.description ?? "never") (\(status.minutesSinceCompact)m ago)
          - Last Deep: \(status.lastDeepSync?.description ?? "never") (\(status.hoursSinceDeep)h ago)
        """)
    }
}
#endif
