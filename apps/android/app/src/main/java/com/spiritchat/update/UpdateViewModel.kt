package com.spiritchat.update

import android.app.Application
import android.app.DownloadManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Environment
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.FileProvider
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

enum class UpdateStatus { Idle, Downloading, Paused, Done, Error }

class UpdateViewModel(app: Application) : AndroidViewModel(app) {

    var info by mutableStateOf<UpdateInfo?>(null)
        private set
    var status by mutableStateOf(UpdateStatus.Idle)
        private set
    var bytesDownloaded by mutableStateOf(0L)
        private set
    var totalBytes by mutableStateOf(0L)
        private set

    fun checkForUpdate() {
        viewModelScope.launch {
            info = withContext(Dispatchers.IO) { UpdateChecker.check() }
        }
    }

    fun dismiss() {
        info = null
    }

    private fun apkFile(context: Context): File =
        File(context.getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS), "update.apk")

    fun download() {
        val update = info ?: return
        val context = getApplication<Application>()
        status = UpdateStatus.Downloading
        bytesDownloaded = 0
        totalBytes = 0

        apkFile(context).delete()
        val dm = context.getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager
        val request =
            DownloadManager.Request(Uri.parse(update.apkUrl))
                .setTitle("SpiritChat")
                .setDescription("Загрузка обновления")
                .setMimeType("application/vnd.android.package-archive")
                .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE)
                .setDestinationInExternalFilesDir(
                    context, Environment.DIRECTORY_DOWNLOADS, "update.apk")
                .setAllowedOverMetered(true)
                .setAllowedOverRoaming(true)
        val id = dm.enqueue(request)

        viewModelScope.launch {
            while (true) {
                val cursor = dm.query(DownloadManager.Query().setFilterById(id))
                var finished = false
                if (cursor.moveToFirst()) {
                    val st = cursor.getInt(cursor.getColumnIndexOrThrow(DownloadManager.COLUMN_STATUS))
                    bytesDownloaded = cursor.getLong(
                        cursor.getColumnIndexOrThrow(DownloadManager.COLUMN_BYTES_DOWNLOADED_SO_FAR))
                    totalBytes = cursor.getLong(
                        cursor.getColumnIndexOrThrow(DownloadManager.COLUMN_TOTAL_SIZE_BYTES))
                    when (st) {
                        DownloadManager.STATUS_SUCCESSFUL -> {
                            status = UpdateStatus.Done
                            finished = true
                        }
                        DownloadManager.STATUS_FAILED -> {
                            status = UpdateStatus.Error
                            finished = true
                        }
                        DownloadManager.STATUS_PAUSED -> status = UpdateStatus.Paused
                        else -> status = UpdateStatus.Downloading
                    }
                }
                cursor.close()
                if (finished) {
                    if (status == UpdateStatus.Done) install(context)
                    return@launch
                }
                delay(500)
            }
        }
    }

    private fun install(context: Context) {
        val file = apkFile(context)
        if (!file.exists()) return
        val uri = FileProvider.getUriForFile(context, context.packageName + ".fileprovider", file)
        val intent =
            Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, "application/vnd.android.package-archive")
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
        context.startActivity(intent)
    }
}
