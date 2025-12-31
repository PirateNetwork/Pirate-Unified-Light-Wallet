import Foundation
import BackgroundTasks
import UserNotifications
import UIKit
import Flutter

/**
 * Background sync manager for iOS
 * 
 * Handles background blockchain synchronization using:
 * - BGAppRefreshTask for quick updates (15-30 min intervals)
 * - BGProcessingTask for deep sync (daily, when idle + charging)
 * - Privacy-respecting network tunnel (Tor/SOCKS5)
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
    
    private let lastCompactSyncKey = "lastCompactSync"
    private let lastDeepSyncKey = "lastDeepSync"
    private let tunnelModeKey = "tunnelMode"
    private let socks5UrlKey = "socks5Url"
    private let notificationsEnabledKey = "notificationsEnabled"
    private let activeWalletIdKey = "activeWalletId"
    
    // Sync intervals
    let compactIntervalSeconds: TimeInterval = 15 * 60  // 15 minutes
    let deepIntervalSeconds: TimeInterval = 24 * 60 * 60  // 24 hours
    
    // Tunnel modes (must match Rust TunnelMode enum)
    enum TunnelMode: String {
        case tor = "tor"
        case socks5 = "socks5"
        case direct = "direct"
    }
    
    private override init() {
        super.init()
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
        engine.run(withEntrypoint: nil)
        
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
    func scheduleCompactSync() {
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
    func scheduleDeepSync() {
        let request = BGProcessingTaskRequest(identifier: BackgroundSyncManager.deepTaskIdentifier)
        request.earliestBeginDate = Date(timeIntervalSinceNow: deepIntervalSeconds)
        request.requiresNetworkConnectivity = true
        request.requiresExternalPower = true  // Only when charging
        
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
                    maxDurationSecs: 25,  // Leave buffer for cleanup
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
                    maxDurationSecs: 180,  // 3 minutes max
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
                "maxBlocks": mode == .deep ? 50000 : 5000,
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
        let socks5Url = defaults.string(forKey: socks5UrlKey)
        
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
            defaults.set(url, forKey: socks5UrlKey)
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
        // Check if notifications are enabled
        guard defaults.bool(forKey: notificationsEnabledKey) else {
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
        
        // Request notification permissions
        Task {
            _ = await BackgroundSyncManager.shared.requestNotificationPermissions()
        }
        
        // Schedule initial tasks
        BackgroundSyncManager.shared.scheduleAllSyncs()
        
        print("[AppDelegate] Background sync initialized")
    }
    
    func applicationDidEnterBackground(_ application: UIApplication) {
        // Ensure syncs are scheduled when app goes to background
        BackgroundSyncManager.shared.scheduleAllSyncs()
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
