package io.github.xxkuroikenshixx.oval.engine

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager

/**
 * Keeps downloads at full speed while they run. Without it Android treats Oval
 * as idle once the screen goes off or the app leaves the foreground: the CPU
 * naps, Wi-Fi drops into power saving and downloads crawl or stop. A foreground
 * service (with its "Descargando" notification), a CPU wake lock and a
 * high-performance Wi-Fi lock are held while at least one download runs.
 */
class DownloadService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL, "Descargas", NotificationManager.IMPORTANCE_LOW),
            )
        }
        val launch = packageManager.getLaunchIntentForPackage(packageName)
        val open = launch?.let {
            android.app.PendingIntent.getActivity(
                this, 0, it,
                android.app.PendingIntent.FLAG_IMMUTABLE or android.app.PendingIntent.FLAG_UPDATE_CURRENT,
            )
        }
        @Suppress("DEPRECATION")
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, CHANNEL)
        } else {
            Notification.Builder(this)
        }
        val notification = builder
            .setSmallIcon(android.R.drawable.stat_sys_download)
            .setContentTitle("Oval")
            .setContentText("Descargando…")
            .setOngoing(true)
            .setContentIntent(open)
            .build()
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
            } else {
                startForeground(ID, notification)
            }
        } catch (e: Exception) {
            // Not allowed right now (started from the background): the locks
            // below still help, and the next start will try again.
            stopSelf()
        }
        return START_NOT_STICKY
    }

    companion object {
        private const val CHANNEL = "downloads"
        private const val ID = 4202
        private val main = Handler(Looper.getMainLooper())
        private var running = 0
        private var wake: PowerManager.WakeLock? = null
        private var wifi: WifiManager.WifiLock? = null
        private var pendingStop: Runnable? = null

        /** A download started. */
        fun acquire(ctx: Context) {
            val app = ctx.applicationContext
            main.post {
                pendingStop?.let { main.removeCallbacks(it) }
                pendingStop = null
                running++
                if (running > 1) return@post
                holdLocks(app)
                try {
                    val intent = Intent(app, DownloadService::class.java)
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                        app.startForegroundService(intent)
                    } else {
                        app.startService(intent)
                    }
                } catch (e: Exception) {
                    // Android 12+ refuses to start it from the background.
                }
            }
        }

        /** A download ended. The service lingers a little in case the queue starts the next one. */
        fun release(ctx: Context) {
            val app = ctx.applicationContext
            main.post {
                running = (running - 1).coerceAtLeast(0)
                if (running > 0) return@post
                val later = Runnable {
                    if (running == 0) {
                        dropLocks()
                        app.stopService(Intent(app, DownloadService::class.java))
                    }
                }
                pendingStop = later
                main.postDelayed(later, 8000)
            }
        }

        private fun holdLocks(app: Context) {
            try {
                if (wake == null) {
                    val pm = app.getSystemService(Context.POWER_SERVICE) as PowerManager
                    wake = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "oval:download").apply {
                        setReferenceCounted(false)
                        acquire(6 * 60 * 60 * 1000L)
                    }
                }
                if (wifi == null) {
                    val wm = app.getSystemService(Context.WIFI_SERVICE) as WifiManager
                    @Suppress("DEPRECATION")
                    // High-performance mode keeps Wi-Fi out of power saving even with
                    // the screen off; Android 14 dropped it, low latency remains.
                    val mode = if (Build.VERSION.SDK_INT >= 34) {
                        WifiManager.WIFI_MODE_FULL_LOW_LATENCY
                    } else {
                        WifiManager.WIFI_MODE_FULL_HIGH_PERF
                    }
                    wifi = wm.createWifiLock(mode, "oval:download").apply {
                        setReferenceCounted(false)
                        acquire()
                    }
                }
            } catch (e: Exception) {
                // Locks are an optimization; downloads work without them.
            }
        }

        private fun dropLocks() {
            try { wake?.takeIf { it.isHeld }?.release() } catch (e: Exception) {}
            try { wifi?.takeIf { it.isHeld }?.release() } catch (e: Exception) {}
            wake = null
            wifi = null
        }
    }
}
