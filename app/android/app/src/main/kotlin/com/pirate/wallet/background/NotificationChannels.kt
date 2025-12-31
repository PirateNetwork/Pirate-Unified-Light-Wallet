package com.pirate.wallet.background

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import android.os.Build

/**
 * Notification channel management for Android
 * 
 * Creates and manages notification channels for:
 * - Background sync status
 * - Received transaction alerts
 * - Security alerts
 */
object NotificationChannels {
    
    const val CHANNEL_SYNC = "pirate_sync"
    const val CHANNEL_TRANSACTIONS = "pirate_transactions"
    const val CHANNEL_SECURITY = "pirate_security"
    
    /**
     * Create all notification channels
     */
    fun createChannels(context: Context) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val notificationManager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            
            // Sync channel (low importance, no sound)
            val syncChannel = NotificationChannel(
                CHANNEL_SYNC,
                "Background Sync",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "Blockchain synchronization status"
                enableLights(false)
                enableVibration(false)
                setShowBadge(false)
            }
            
            // Transactions channel (default importance)
            val txChannel = NotificationChannel(
                CHANNEL_TRANSACTIONS,
                "Received Payments",
                NotificationManager.IMPORTANCE_DEFAULT
            ).apply {
                description = "Notifications for incoming ARRR payments"
                enableLights(true)
                enableVibration(true)
                setShowBadge(true)
            }
            
            // Security channel (high importance)
            val securityChannel = NotificationChannel(
                CHANNEL_SECURITY,
                "Security Alerts",
                NotificationManager.IMPORTANCE_HIGH
            ).apply {
                description = "Important security notifications"
                enableLights(true)
                enableVibration(true)
                setShowBadge(true)
            }
            
            notificationManager.createNotificationChannels(
                listOf(syncChannel, txChannel, securityChannel)
            )
            
            android.util.Log.i("NotificationChannels", "Created notification channels")
        }
    }
}

