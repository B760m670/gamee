package com.spiritchat.update

import android.app.DownloadManager
import android.content.Context
import android.content.Intent
import android.database.Cursor
import android.net.Uri
import android.os.Environment
import android.os.Handler
import android.os.Looper
import androidx.core.content.FileProvider
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.WritableMap
import com.facebook.react.modules.core.DeviceEventManagerModule
import java.io.File

/**
 * Downloads an APK update using the system DownloadManager, which runs in the
 * background, survives the app being minimized, and resumes automatically when
 * the network drops and comes back. Progress is polled and forwarded to JS as
 * "UpdateProgress" events; install is launched via FileProvider.
 */
class UpdateModule(private val reactContext: ReactApplicationContext) :
    ReactContextBaseJavaModule(reactContext) {

  private val handler = Handler(Looper.getMainLooper())
  private var downloadId: Long = -1
  private var poller: Runnable? = null

  override fun getName(): String = "UpdateModule"

  private fun apkFile(): File =
      File(reactContext.getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS), "update.apk")

  private fun downloadManager(): DownloadManager =
      reactContext.getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager

  private fun emitProgress(status: String, downloaded: Long, total: Long) {
    val map: WritableMap = Arguments.createMap()
    map.putString("status", status)
    map.putDouble("bytesDownloaded", downloaded.toDouble())
    map.putDouble("totalBytes", total.toDouble())
    reactContext
        .getJSModule(DeviceEventManagerModule.RCTDeviceEventEmitter::class.java)
        .emit("UpdateProgress", map)
  }

  @ReactMethod
  fun startDownload(url: String, promise: Promise) {
    try {
      // Remove any previous file so DownloadManager doesn't add a numeric suffix.
      apkFile().delete()

      val request =
          DownloadManager.Request(Uri.parse(url))
              .setTitle("SpiritChat")
              .setDescription("Загрузка обновления")
              .setMimeType("application/vnd.android.package-archive")
              .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE)
              .setDestinationInExternalFilesDir(
                  reactContext, Environment.DIRECTORY_DOWNLOADS, "update.apk")
              .setAllowedOverMetered(true)
              .setAllowedOverRoaming(true)

      downloadId = downloadManager().enqueue(request)
      startPolling()
      promise.resolve(downloadId.toString())
    } catch (e: Exception) {
      promise.reject("download_error", e.message, e)
    }
  }

  private fun startPolling() {
    stopPolling()
    val runnable =
        object : Runnable {
          override fun run() {
            val cursor: Cursor =
                downloadManager().query(DownloadManager.Query().setFilterById(downloadId))
            var finished = false
            if (cursor.moveToFirst()) {
              val status = cursor.getInt(cursor.getColumnIndexOrThrow(DownloadManager.COLUMN_STATUS))
              val downloaded =
                  cursor.getLong(
                      cursor.getColumnIndexOrThrow(DownloadManager.COLUMN_BYTES_DOWNLOADED_SO_FAR))
              val total =
                  cursor.getLong(
                      cursor.getColumnIndexOrThrow(DownloadManager.COLUMN_TOTAL_SIZE_BYTES))
              when (status) {
                DownloadManager.STATUS_SUCCESSFUL -> {
                  val size = if (total > 0) total else downloaded
                  emitProgress("done", size, size)
                  finished = true
                }
                DownloadManager.STATUS_FAILED -> {
                  emitProgress("error", downloaded, total)
                  finished = true
                }
                DownloadManager.STATUS_PAUSED -> emitProgress("paused", downloaded, total)
                else -> emitProgress("downloading", downloaded, total)
              }
            }
            cursor.close()
            if (!finished) handler.postDelayed(this, 600)
          }
        }
    poller = runnable
    handler.post(runnable)
  }

  private fun stopPolling() {
    poller?.let { handler.removeCallbacks(it) }
    poller = null
  }

  @ReactMethod
  fun cancel(promise: Promise) {
    try {
      stopPolling()
      if (downloadId != -1L) {
        downloadManager().remove(downloadId)
      }
      downloadId = -1
      promise.resolve(true)
    } catch (e: Exception) {
      promise.reject("cancel_error", e.message, e)
    }
  }

  @ReactMethod
  fun install(promise: Promise) {
    try {
      val file = apkFile()
      if (!file.exists()) {
        promise.reject("no_file", "APK not found")
        return
      }
      val uri =
          FileProvider.getUriForFile(
              reactContext, reactContext.packageName + ".fileprovider", file)
      val intent =
          Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "application/vnd.android.package-archive")
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
          }
      reactContext.startActivity(intent)
      promise.resolve(true)
    } catch (e: Exception) {
      promise.reject("install_error", e.message, e)
    }
  }

  // Required so the JS NativeEventEmitter does not warn.
  @ReactMethod fun addListener(eventName: String) {}

  @ReactMethod fun removeListeners(count: Int) {}
}
