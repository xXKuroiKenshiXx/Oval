package io.github.xxkuroikenshixx.oval.engine

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.yausername.ffmpeg.FFmpeg
import com.yausername.youtubedl_android.YoutubeDL
import com.yausername.youtubedl_android.YoutubeDLRequest
import java.util.concurrent.Executors

@InvokeArg
class RunArgs {
    var id: Long = 0
    var args: Array<String> = arrayOf()
    lateinit var channel: Channel
}

@InvokeArg
class IdArgs {
    var id: Long = 0
}

/** Runs yt-dlp (through youtubedl-android) for Oval's Rust code. */
@TauriPlugin
class EnginePlugin(private val activity: Activity) : Plugin(activity) {
    private val pool = Executors.newCachedThreadPool()
    private val lock = Any()
    @Volatile private var ready = false

    /** Unpacks Python, yt-dlp and FFmpeg the first time; cheap afterwards. */
    private fun ensureReady() {
        if (ready) return
        synchronized(lock) {
            if (ready) return
            val ctx = activity.applicationContext
            discardBrokenUnpack(java.io.File(ctx.noBackupFilesDir, "youtubedl-android/packages"))
            YoutubeDL.getInstance().init(ctx)
            FFmpeg.getInstance().init(ctx)
            ready = true
        }
    }

    /**
     * If the app was killed while Python/FFmpeg were being unpacked, empty
     * libraries are left behind and every run fails to link. Deleting the
     * folder makes the library unpack everything again.
     */
    private fun discardBrokenUnpack(packages: java.io.File) {
        val broken = packages.walkTopDown().any { it.isFile && it.name.endsWith(".so") && it.length() == 0L }
        if (broken) packages.deleteRecursively()
    }

    private fun Channel.emit(key: String, value: Any) {
        val o = JSObject()
        o.put(key, value)
        send(o)
    }

    @Command
    fun init(invoke: Invoke) {
        askForStorage()
        pool.execute {
            try {
                ensureReady()
                val ret = JSObject()
                ret.put("version", YoutubeDL.getInstance().version(activity.applicationContext) ?: "")
                invoke.resolve(ret)
            } catch (e: Throwable) {
                invoke.reject(e.message ?: "No se pudo preparar el motor")
            }
        }
    }

    @Command
    fun update(invoke: Invoke) {
        pool.execute {
            try {
                ensureReady()
                val status = YoutubeDL.getInstance()
                    .updateYoutubeDL(activity.applicationContext, YoutubeDL.UpdateChannel._STABLE)
                val ret = JSObject()
                ret.put("status", status.toString())
                invoke.resolve(ret)
            } catch (e: Throwable) {
                invoke.reject(e.message ?: "No se pudo actualizar el motor")
            }
        }
    }

    /** Starts yt-dlp and streams every output line back over the channel. */
    @Command
    fun run(invoke: Invoke) {
        val a = invoke.parseArgs(RunArgs::class.java)
        invoke.resolve()
        pool.execute {
            var code = 1
            try {
                ensureReady()
                // The library ships QuickJS; yt-dlp needs a JavaScript runtime to
                // solve YouTube's player challenges, but won't find it on its own.
                val qjs = java.io.File(activity.applicationInfo.nativeLibraryDir, "libqjs.so")
                val runtime = if (qjs.exists()) listOf("--js-runtimes", "quickjs:${qjs.absolutePath}") else emptyList()
                val request = YoutubeDLRequest(emptyList<String>()).addCommands(runtime + a.args.toList())
                val response = YoutubeDL.getInstance().execute(request, a.id.toString(), false) { _, _, line ->
                    a.channel.emit("out", line)
                }
                response.err.lineSequence().filter { it.isNotBlank() }.forEach { a.channel.emit("err", it) }
                code = response.exitCode
            } catch (e: YoutubeDL.CanceledException) {
                code = 130
            } catch (e: Throwable) {
                (e.message ?: "ERROR: yt-dlp falló").lineSequence()
                    .filter { it.isNotBlank() }
                    .forEach { a.channel.emit("err", it) }
            }
            a.channel.emit("exit", code)
        }
    }

    @Command
    fun kill(invoke: Invoke) {
        val a = invoke.parseArgs(IdArgs::class.java)
        runCatching { YoutubeDL.getInstance().destroyProcessById(a.id.toString()) }
        invoke.resolve()
    }

    /** Android 10 and older need the storage permission to write to Download/. */
    private fun askForStorage() {
        if (Build.VERSION.SDK_INT <= Build.VERSION_CODES.Q &&
            ContextCompat.checkSelfPermission(activity, Manifest.permission.WRITE_EXTERNAL_STORAGE)
            != PackageManager.PERMISSION_GRANTED
        ) {
            activity.runOnUiThread {
                ActivityCompat.requestPermissions(
                    activity,
                    arrayOf(Manifest.permission.WRITE_EXTERNAL_STORAGE),
                    4201,
                )
            }
        }
    }
}
