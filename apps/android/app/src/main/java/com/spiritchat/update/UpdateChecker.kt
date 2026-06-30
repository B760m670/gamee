package com.spiritchat.update

import com.spiritchat.BuildConfig
import org.json.JSONObject
import java.net.URL

// Reads the "dev" release manifest and returns it only if it's newer than this
// build. Each push publishes a manifest with versionCode = CI run number.
object UpdateChecker {
    private const val UPDATE_JSON =
        "https://github.com/B760m670/gamee/releases/download/dev/update.json"

    fun check(): UpdateInfo? {
        return try {
            val text = URL("$UPDATE_JSON?t=${System.currentTimeMillis()}").readText()
            val o = JSONObject(text)
            val versionCode = o.getInt("versionCode")
            if (versionCode <= BuildConfig.VERSION_CODE) {
                null
            } else {
                UpdateInfo(
                    versionCode = versionCode,
                    versionName = o.optString("versionName"),
                    apkUrl = o.getString("apkUrl"),
                    notes = o.optString("notes"),
                )
            }
        } catch (e: Exception) {
            null
        }
    }
}
