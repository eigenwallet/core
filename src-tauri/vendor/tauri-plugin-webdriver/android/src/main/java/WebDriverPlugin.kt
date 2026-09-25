package com.plugin.webdriver

import android.app.Activity
import android.graphics.Bitmap
import android.graphics.pdf.PdfDocument
import android.os.Handler
import android.os.Looper
import android.view.MotionEvent
import android.webkit.JavascriptInterface
import android.webkit.CookieManager
import android.webkit.JsPromptResult
import android.webkit.JsResult
import android.webkit.WebChromeClient
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.ByteArrayOutputStream
import java.util.concurrent.ConcurrentHashMap
import kotlin.math.roundToInt

@InvokeArg
class EvaluateJsArgs {
    lateinit var script: String
    var timeoutMs: Long = 30000
}

@InvokeArg
class ScreenshotArgs {
    var timeoutMs: Long = 30000
}

@InvokeArg
class PrintArgs {
    var orientation: String? = null
    var scale: Double? = null
    var background: Boolean? = null
    var pageWidth: Double? = null
    var pageHeight: Double? = null
    var marginTop: Double? = null
    var marginBottom: Double? = null
    var marginLeft: Double? = null
    var marginRight: Double? = null
    var shrinkToFit: Boolean? = null
    var pageRanges: List<String>? = null
}

@InvokeArg
class TouchArgs {
    lateinit var type: String  // "down", "up", "move"
    var x: Int = 0
    var y: Int = 0
}

@InvokeArg
class AsyncScriptArgs {
    lateinit var asyncId: String
    lateinit var script: String
    var timeoutMs: Long = 30000
}

@InvokeArg
class AlertResponseArgs {
    var accepted: Boolean = true
    var promptText: String? = null
}

@InvokeArg
class SetCookieArgs {
    lateinit var url: String
    lateinit var name: String
    lateinit var value: String
    var path: String? = null
    var domain: String? = null
    var secure: Boolean = false
    var httpOnly: Boolean = false
    var expiry: Long? = null
    var sameSite: String? = null
}

@InvokeArg
class DeleteCookieArgs {
    lateinit var url: String
    lateinit var name: String
}

@InvokeArg
class GetCookiesArgs {
    lateinit var url: String
}

data class PendingAlert(
    val message: String,
    val defaultText: String?,
    val type: String, // "alert", "confirm", "prompt"
    var promptInput: String? = null,
    var responseCallback: ((Boolean, String?) -> Unit)? = null
)

/**
 * Cookie metadata that CookieManager doesn't expose
 */
data class CookieMetadata(
    val name: String,
    val value: String,
    val path: String?,
    val domain: String?,
    val secure: Boolean,
    val httpOnly: Boolean,
    val expiry: Long?,
    val sameSite: String?
)

@TauriPlugin
class WebDriverPlugin(private val activity: Activity) : Plugin(activity) {
    private var webView: WebView? = null
    private val mainHandler = Handler(Looper.getMainLooper())
    private val pendingAsyncScripts = ConcurrentHashMap<String, (Any?) -> Unit>()
    private var pendingAlert: PendingAlert? = null
    private val alertLock = Object()
    // Cache cookie metadata since CookieManager doesn't expose attributes
    // Key format: "domain:path:name"
    private val cookieMetadataCache = ConcurrentHashMap<String, CookieMetadata>()

    override fun load(webView: WebView) {
        this.webView = webView

        mainHandler.post {
            // Add JavaScript interface for async script callbacks
            webView.addJavascriptInterface(AsyncScriptBridge(), "__webdriver_bridge")

            // Wrap existing WebChromeClient to intercept alerts
            val existingClient = webView.webChromeClient
            webView.webChromeClient = WebDriverChromeClient(existingClient)
        }
    }

    /**
     * WebChromeClient wrapper that intercepts JS dialogs
     */
    inner class WebDriverChromeClient(
        private val delegate: WebChromeClient?
    ) : WebChromeClient() {

        override fun onJsAlert(
            view: WebView,
            url: String,
            message: String,
            result: JsResult
        ): Boolean {
            setPendingAlert(message, null, "alert") { accepted, _ ->
                if (accepted) result.confirm() else result.cancel()
            }
            return true
        }

        override fun onJsConfirm(
            view: WebView,
            url: String,
            message: String,
            result: JsResult
        ): Boolean {
            setPendingAlert(message, null, "confirm") { accepted, _ ->
                if (accepted) result.confirm() else result.cancel()
            }
            return true
        }

        override fun onJsPrompt(
            view: WebView,
            url: String,
            message: String,
            defaultValue: String?,
            result: JsPromptResult
        ): Boolean {
            setPendingAlert(message, defaultValue, "prompt") { accepted, promptText ->
                if (accepted) {
                    result.confirm(promptText ?: defaultValue ?: "")
                } else {
                    result.cancel()
                }
            }
            return true
        }

        // Delegate other methods to existing client
        override fun onProgressChanged(view: WebView, newProgress: Int) {
            delegate?.onProgressChanged(view, newProgress)
        }

        override fun onReceivedTitle(view: WebView, title: String?) {
            delegate?.onReceivedTitle(view, title)
        }

        override fun onReceivedIcon(view: WebView, icon: android.graphics.Bitmap?) {
            delegate?.onReceivedIcon(view, icon)
        }
    }

    /**
     * JavaScript interface for async script callbacks
     */
    inner class AsyncScriptBridge {
        @JavascriptInterface
        fun postResult(asyncId: String, result: String?, error: String?) {
            val callback = pendingAsyncScripts.remove(asyncId)
            if (callback != null) {
                if (error != null) {
                    callback(mapOf("error" to error))
                } else {
                    callback(mapOf("result" to result))
                }
            }
        }
    }

    /**
     * Evaluate JavaScript synchronously and return result
     */
    @Command
    fun evaluateJs(invoke: Invoke) {
        val args = invoke.parseArgs(EvaluateJsArgs::class.java)
        val wv = webView

        if (wv == null) {
            invoke.reject("WebView not available")
            return
        }

        mainHandler.post {
            wv.evaluateJavascript(args.script) { result ->
                val ret = JSObject()
                ret.put("success", true)
                ret.put("value", result)
                invoke.resolve(ret)
            }
        }
    }

    /**
     * Execute async script with callback support
     */
    @Command
    fun executeAsyncScript(invoke: Invoke) {
        val args = invoke.parseArgs(AsyncScriptArgs::class.java)
        val wv = webView

        if (wv == null) {
            invoke.reject("WebView not available")
            return
        }

        // Register callback for this async operation
        pendingAsyncScripts[args.asyncId] = { response ->
            val ret = JSObject()
            if (response is Map<*, *>) {
                val error = response["error"] as? String
                val result = response["result"] as? String
                if (error != null) {
                    ret.put("success", false)
                    ret.put("error", error)
                } else {
                    ret.put("success", true)
                    ret.put("value", result)
                }
            } else {
                ret.put("success", true)
                ret.put("value", response)
            }
            invoke.resolve(ret)
        }

        // Inject script with callback bridge
        val wrappedScript = """
            (function() {
                var __done = function(result) {
                    __webdriver_bridge.postResult('${args.asyncId}', JSON.stringify(result), null);
                };
                try {
                    ${args.script}
                } catch (e) {
                    __webdriver_bridge.postResult('${args.asyncId}', null, e.message || String(e));
                }
            })();
        """.trimIndent()

        mainHandler.post {
            wv.evaluateJavascript(wrappedScript, null)
        }

        // Set timeout for cleanup
        mainHandler.postDelayed({
            val callback = pendingAsyncScripts.remove(args.asyncId)
            if (callback != null) {
                val ret = JSObject()
                ret.put("success", false)
                ret.put("error", "Script timeout")
                invoke.resolve(ret)
            }
        }, args.timeoutMs)
    }

    /**
     * Take screenshot by drawing WebView to Canvas
     */
    @Command
    fun takeScreenshot(invoke: Invoke) {
        val wv = webView

        if (wv == null) {
            invoke.reject("WebView not available")
            return
        }

        mainHandler.post {
            try {
                val bitmap = Bitmap.createBitmap(wv.width, wv.height, Bitmap.Config.ARGB_8888)
                val canvas = android.graphics.Canvas(bitmap)
                wv.draw(canvas)

                val outputStream = ByteArrayOutputStream()
                bitmap.compress(Bitmap.CompressFormat.PNG, 100, outputStream)
                val base64 = android.util.Base64.encodeToString(outputStream.toByteArray(), android.util.Base64.NO_WRAP)

                val ret = JSObject()
                ret.put("success", true)
                ret.put("value", base64)
                invoke.resolve(ret)

                bitmap.recycle()
            } catch (e: Exception) {
                invoke.reject("Screenshot failed: ${e.message}")
            }
        }
    }

    /**
     * Print page to PDF using PdfDocument
     */
    @Command
    fun printToPdf(invoke: Invoke) {
        val args = invoke.parseArgs(PrintArgs::class.java)
        val wv = webView

        if (wv == null) {
            invoke.reject("WebView not available")
            return
        }

        mainHandler.post {
            try {
                // Get content dimensions
                val contentWidth = wv.width
                val contentHeight = (wv.contentHeight * wv.scale).roundToInt()

                if (contentWidth <= 0 || contentHeight <= 0) {
                    invoke.reject("WebView has no content")
                    return@post
                }

                // Create PDF document
                val document = PdfDocument()

                // Calculate page size (A4 in points: 595 x 842)
                val pageWidth = args.pageWidth?.times(72)?.roundToInt() ?: 595
                val pageHeight = args.pageHeight?.times(72)?.roundToInt() ?: 842

                // Calculate scale to fit content width to page
                val scale = pageWidth.toFloat() / contentWidth.toFloat()
                val scaledContentHeight = (contentHeight * scale).roundToInt()

                // Calculate number of pages needed
                val numPages = ((scaledContentHeight + pageHeight - 1) / pageHeight).coerceAtLeast(1)

                // Create a bitmap of the full content
                val bitmap = Bitmap.createBitmap(contentWidth, contentHeight, Bitmap.Config.ARGB_8888)
                val canvas = android.graphics.Canvas(bitmap)
                wv.draw(canvas)

                // Create pages
                for (pageNum in 0 until numPages) {
                    val pageInfo = PdfDocument.PageInfo.Builder(pageWidth, pageHeight, pageNum).create()
                    val page = document.startPage(pageInfo)

                    val pageCanvas = page.canvas
                    pageCanvas.scale(scale, scale)
                    pageCanvas.translate(0f, -pageNum * (pageHeight / scale))
                    pageCanvas.drawBitmap(bitmap, 0f, 0f, null)

                    document.finishPage(page)
                }

                bitmap.recycle()

                // Write to byte array
                val outputStream = ByteArrayOutputStream()
                document.writeTo(outputStream)
                document.close()

                val base64 = android.util.Base64.encodeToString(
                    outputStream.toByteArray(),
                    android.util.Base64.NO_WRAP
                )

                val ret = JSObject()
                ret.put("success", true)
                ret.put("value", base64)
                invoke.resolve(ret)
            } catch (e: Exception) {
                invoke.reject("Print to PDF failed: ${e.message}")
            }
        }
    }

    /**
     * Dispatch touch event
     */
    @Command
    fun dispatchTouch(invoke: Invoke) {
        val args = invoke.parseArgs(TouchArgs::class.java)
        val wv = webView

        if (wv == null) {
            invoke.reject("WebView not available")
            return
        }

        mainHandler.post {
            try {
                val action = when (args.type) {
                    "down" -> MotionEvent.ACTION_DOWN
                    "up" -> MotionEvent.ACTION_UP
                    "move" -> MotionEvent.ACTION_MOVE
                    else -> {
                        invoke.reject("Unknown touch type: ${args.type}")
                        return@post
                    }
                }

                val downTime = System.currentTimeMillis()
                val eventTime = System.currentTimeMillis()

                val event = MotionEvent.obtain(
                    downTime,
                    eventTime,
                    action,
                    args.x.toFloat(),
                    args.y.toFloat(),
                    0
                )

                wv.dispatchTouchEvent(event)
                event.recycle()

                invoke.resolve()
            } catch (e: Exception) {
                invoke.reject("Touch dispatch failed: ${e.message}")
            }
        }
    }

    /**
     * Get current alert text (if any alert is pending)
     */
    @Command
    fun getAlertText(invoke: Invoke) {
        synchronized(alertLock) {
            val alert = pendingAlert
            if (alert != null) {
                val ret = JSObject()
                ret.put("message", alert.message)
                invoke.resolve(ret)
            } else {
                invoke.reject("no such alert")
            }
        }
    }

    /**
     * Accept current alert
     */
    @Command
    fun acceptAlert(invoke: Invoke) {
        synchronized(alertLock) {
            val alert = pendingAlert
            if (alert != null) {
                val promptText = alert.promptInput ?: alert.defaultText
                alert.responseCallback?.invoke(true, promptText)
                pendingAlert = null
                invoke.resolve()
            } else {
                invoke.reject("no such alert")
            }
        }
    }

    /**
     * Dismiss current alert
     */
    @Command
    fun dismissAlert(invoke: Invoke) {
        synchronized(alertLock) {
            val alert = pendingAlert
            if (alert != null) {
                alert.responseCallback?.invoke(false, null)
                pendingAlert = null
                invoke.resolve()
            } else {
                invoke.reject("no such alert")
            }
        }
    }

    /**
     * Send text to prompt dialog
     */
    @Command
    fun sendAlertText(invoke: Invoke) {
        val args = invoke.parseArgs(AlertResponseArgs::class.java)
        synchronized(alertLock) {
            val alert = pendingAlert
            if (alert != null) {
                if (alert.type == "prompt") {
                    alert.promptInput = args.promptText
                    invoke.resolve()
                } else {
                    invoke.reject("Alert is not a prompt")
                }
            } else {
                invoke.reject("no such alert")
            }
        }
    }

    /**
     * Internal method to set pending alert (called from WebChromeClient)
     */
    fun setPendingAlert(
        message: String,
        defaultText: String?,
        type: String,
        callback: (Boolean, String?) -> Unit
    ) {
        synchronized(alertLock) {
            pendingAlert = PendingAlert(
                message = message,
                defaultText = defaultText,
                type = type,
                responseCallback = callback
            )
        }
    }

    /**
     * Get all cookies for a URL - returns JSON array with full metadata
     */
    @Command
    fun getCookies(invoke: Invoke) {
        val args = invoke.parseArgs(GetCookiesArgs::class.java)

        mainHandler.post {
            try {
                val cookieManager = CookieManager.getInstance()
                val cookieString = cookieManager.getCookie(args.url)

                val cookiesArray = org.json.JSONArray()

                if (cookieString != null) {
                    val cookies = cookieString.split(";").map { it.trim() }
                    for (cookie in cookies) {
                        val parts = cookie.split("=", limit = 2)
                        if (parts.size >= 2) {
                            val name = parts[0].trim()
                            val value = parts[1].trim()

                            // Look up metadata from cache
                            val metadata = findCookieMetadata(name)

                            val cookieObj = org.json.JSONObject()
                            cookieObj.put("name", name)
                            cookieObj.put("value", value)
                            cookieObj.put("path", metadata?.path ?: "/")
                            if (metadata?.domain != null) {
                                cookieObj.put("domain", metadata.domain)
                            }
                            cookieObj.put("secure", metadata?.secure ?: false)
                            cookieObj.put("httpOnly", metadata?.httpOnly ?: false)
                            if (metadata?.expiry != null) {
                                cookieObj.put("expiry", metadata.expiry)
                            }
                            if (metadata?.sameSite != null) {
                                cookieObj.put("sameSite", metadata.sameSite)
                            }
                            cookiesArray.put(cookieObj)
                        }
                    }
                }

                val ret = JSObject()
                ret.put("success", true)
                ret.put("cookies", cookiesArray.toString())
                invoke.resolve(ret)
            } catch (e: Exception) {
                invoke.reject("Failed to get cookies: ${e.message}")
            }
        }
    }

    /**
     * Find cookie metadata by name (searches all entries)
     */
    private fun findCookieMetadata(name: String): CookieMetadata? {
        return cookieMetadataCache.values.find { it.name == name }
    }

    /**
     * Set a cookie for a URL
     */
    @Command
    fun setCookie(invoke: Invoke) {
        val args = invoke.parseArgs(SetCookieArgs::class.java)

        mainHandler.post {
            try {
                val cookieManager = CookieManager.getInstance()

                // Build cookie string for CookieManager
                val cookieStr = StringBuilder("${args.name}=${args.value}")
                args.path?.let { cookieStr.append("; path=$it") }
                args.domain?.let { cookieStr.append("; domain=$it") }
                if (args.secure) cookieStr.append("; secure")
                if (args.httpOnly) cookieStr.append("; httponly")
                args.expiry?.let { cookieStr.append("; max-age=$it") }
                args.sameSite?.let { cookieStr.append("; samesite=$it") }

                cookieManager.setCookie(args.url, cookieStr.toString())

                // Store metadata in cache
                val cacheKey = "${args.domain ?: ""}:${args.path ?: "/"}:${args.name}"
                cookieMetadataCache[cacheKey] = CookieMetadata(
                    name = args.name,
                    value = args.value,
                    path = args.path ?: "/",
                    domain = args.domain,
                    secure = args.secure,
                    httpOnly = args.httpOnly,
                    expiry = args.expiry,
                    sameSite = args.sameSite
                )

                // Flush to persist
                cookieManager.flush()

                invoke.resolve()
            } catch (e: Exception) {
                invoke.reject("Failed to set cookie: ${e.message}")
            }
        }
    }

    /**
     * Delete a specific cookie by name
     */
    @Command
    fun deleteCookie(invoke: Invoke) {
        val args = invoke.parseArgs(DeleteCookieArgs::class.java)

        mainHandler.post {
            try {
                val cookieManager = CookieManager.getInstance()

                // Look up metadata to get the original path/domain
                val metadata = findCookieMetadata(args.name)
                val path = metadata?.path ?: "/"
                val domain = metadata?.domain

                // Delete by setting expired cookie with same path/domain
                val deleteCookie = StringBuilder("${args.name}=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=$path")
                domain?.let { deleteCookie.append("; domain=$it") }

                cookieManager.setCookie(args.url, deleteCookie.toString())
                cookieManager.flush()

                // Remove from metadata cache
                cookieMetadataCache.entries.removeIf { it.value.name == args.name }

                invoke.resolve()
            } catch (e: Exception) {
                invoke.reject("Failed to delete cookie: ${e.message}")
            }
        }
    }

    /**
     * Delete all cookies
     */
    @Command
    fun deleteAllCookies(invoke: Invoke) {
        mainHandler.post {
            try {
                val cookieManager = CookieManager.getInstance()
                cookieManager.removeAllCookies(null)
                cookieManager.flush()

                // Clear metadata cache
                cookieMetadataCache.clear()

                invoke.resolve()
            } catch (e: Exception) {
                invoke.reject("Failed to delete all cookies: ${e.message}")
            }
        }
    }

    /**
     * Get WebView dimensions
     */
    @Command
    fun getViewportSize(invoke: Invoke) {
        val wv = webView

        if (wv == null) {
            invoke.reject("WebView not available")
            return
        }

        mainHandler.post {
            val ret = JSObject()
            ret.put("width", wv.width)
            ret.put("height", wv.height)
            invoke.resolve(ret)
        }
    }
}
