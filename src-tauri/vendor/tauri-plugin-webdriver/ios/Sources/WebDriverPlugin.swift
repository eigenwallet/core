import SwiftRs
import Tauri
import UIKit
import WebKit

// MARK: - Argument Classes

class EvaluateJsArgs: Decodable {
    let script: String
    var timeoutMs: Int64?
}

class AsyncScriptArgs: Decodable {
    let script: String
    var timeoutMs: Int64?
}

class TouchArgs: Decodable {
    let type: String
    let x: Int
    let y: Int
}

class ScreenshotArgs: Decodable {
    var timeoutMs: Int64?
}

class PrintArgs: Decodable {
    var orientation: String?
    var scale: Double?
    var background: Bool?
    var pageWidth: Double?
    var pageHeight: Double?
    var marginTop: Double?
    var marginBottom: Double?
    var marginLeft: Double?
    var marginRight: Double?
    var shrinkToFit: Bool?
    var pageRanges: [String]?
}

class SendAlertTextArgs: Decodable {
    let promptText: String
}

// MARK: - Pending Alert

class PendingAlert {
    let message: String
    let type: String  // "alert", "confirm", "prompt"
    let defaultText: String?
    var promptInput: String?
    var completionHandler: ((Bool, String?) -> Void)?

    init(message: String, type: String, defaultText: String? = nil, completionHandler: ((Bool, String?) -> Void)? = nil) {
        self.message = message
        self.type = type
        self.defaultText = defaultText
        self.completionHandler = completionHandler
    }
}

// MARK: - WebDriver Plugin

class WebDriverPlugin: Plugin, WKUIDelegate {
    private var webView: WKWebView?
    private var pendingAlert: PendingAlert?
    private let alertLock = NSLock()
    private var originalUIDelegate: WKUIDelegate?

    @objc public override func load(webview: WKWebView) {
        self.webView = webview

        // Store original delegate to forward non-alert calls
        self.originalUIDelegate = webview.uiDelegate

        // Set ourselves as the UI delegate for alert handling
        webview.uiDelegate = self
    }

    // MARK: - WKUIDelegate (Alert Handling)

    func webView(_ webView: WKWebView, runJavaScriptAlertPanelWithMessage message: String, initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping () -> Void) {
        alertLock.lock()
        pendingAlert = PendingAlert(message: message, type: "alert") { accepted, _ in
            completionHandler()
        }
        alertLock.unlock()
    }

    func webView(_ webView: WKWebView, runJavaScriptConfirmPanelWithMessage message: String, initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping (Bool) -> Void) {
        alertLock.lock()
        pendingAlert = PendingAlert(message: message, type: "confirm") { accepted, _ in
            completionHandler(accepted)
        }
        alertLock.unlock()
    }

    func webView(_ webView: WKWebView, runJavaScriptTextInputPanelWithPrompt prompt: String, defaultText: String?, initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping (String?) -> Void) {
        alertLock.lock()
        pendingAlert = PendingAlert(message: prompt, type: "prompt", defaultText: defaultText) { accepted, text in
            if accepted {
                completionHandler(text ?? defaultText ?? "")
            } else {
                completionHandler(nil)
            }
        }
        alertLock.unlock()
    }

    // MARK: - Commands

    @objc public func evaluateJs(_ invoke: Invoke) {
        guard let args = try? invoke.parseArgs(EvaluateJsArgs.self) else {
            invoke.reject("Failed to parse arguments")
            return
        }

        guard let wv = webView else {
            invoke.reject("WebView not available")
            return
        }

        DispatchQueue.main.async {
            wv.evaluateJavaScript(args.script) { result, error in
                if let error = error {
                    invoke.resolve([
                        "success": false,
                        "error": error.localizedDescription
                    ])
                } else {
                    // Return the result directly - Tauri will handle JSON serialization
                    // We just need to convert to a JSON-compatible format
                    var jsonValue: Any = NSNull()
                    if let result = result {
                        if result is NSNull {
                            jsonValue = NSNull()
                        } else if let str = result as? String {
                            jsonValue = str
                        } else if let num = result as? NSNumber {
                            jsonValue = num
                        } else if let arr = result as? [Any] {
                            jsonValue = arr
                        } else if let dict = result as? [String: Any] {
                            jsonValue = dict
                        } else {
                            // Fallback: convert to string
                            jsonValue = String(describing: result)
                        }
                    }
                    invoke.resolve([
                        "success": true,
                        "value": jsonValue
                    ])
                }
            }
        }
    }

    @objc public func executeAsyncScript(_ invoke: Invoke) {
        guard let args = try? invoke.parseArgs(AsyncScriptArgs.self) else {
            invoke.reject("Failed to parse arguments")
            return
        }

        guard let wv = webView else {
            invoke.reject("WebView not available")
            return
        }

        // Wrap script in a Promise - __done resolves/rejects the promise
        // The script from Rust already sets up __args and pushes __done
        let promiseScript = """
        return new Promise((resolve, reject) => {
            var __done = function(result, error) {
                if (error) {
                    reject(new Error(typeof error === 'string' ? error : String(error)));
                } else {
                    resolve(result);
                }
            };
            try {
                \(args.script)
            } catch (e) {
                reject(e);
            }
        });
        """

        DispatchQueue.main.async {
            wv.callAsyncJavaScript(
                promiseScript,
                arguments: [:],
                in: nil,
                in: .page,
                completionHandler: { result in
                    switch result {
                    case .success(let value):
                        // Convert value to JSON-compatible format
                        var jsonValue: Any = NSNull()
                        if value is NSNull {
                            jsonValue = NSNull()
                        } else if let str = value as? String {
                            jsonValue = str
                        } else if let num = value as? NSNumber {
                            jsonValue = num
                        } else if let arr = value as? [Any] {
                            jsonValue = arr
                        } else if let dict = value as? [String: Any] {
                            jsonValue = dict
                        } else {
                            jsonValue = String(describing: value)
                        }
                        invoke.resolve([
                            "success": true,
                            "value": jsonValue
                        ])
                    case .failure(let error):
                        invoke.resolve([
                            "success": false,
                            "error": error.localizedDescription
                        ])
                    }
                }
            )
        }
    }

    @objc public func takeScreenshot(_ invoke: Invoke) {
        guard let wv = webView else {
            invoke.reject("WebView not available")
            return
        }

        DispatchQueue.main.async {
            let config = WKSnapshotConfiguration()

            wv.takeSnapshot(with: config) { image, error in
                if let error = error {
                    invoke.resolve([
                        "success": false,
                        "error": error.localizedDescription
                    ])
                    return
                }

                guard let image = image,
                      let pngData = image.pngData() else {
                    invoke.resolve([
                        "success": false,
                        "error": "Failed to capture screenshot"
                    ])
                    return
                }

                let base64 = pngData.base64EncodedString()
                invoke.resolve([
                    "success": true,
                    "value": base64
                ])
            }
        }
    }

    @objc public func printToPdf(_ invoke: Invoke) {
        guard let wv = webView else {
            invoke.reject("WebView not available")
            return
        }

        DispatchQueue.main.async {
            let config = WKPDFConfiguration()

            // Parse optional print arguments
            if let args = try? invoke.parseArgs(PrintArgs.self) {
                // Configure page size if provided (in inches, convert to points)
                if let width = args.pageWidth, let height = args.pageHeight {
                    config.rect = CGRect(x: 0, y: 0, width: width * 72, height: height * 72)
                }
            }

            wv.createPDF(configuration: config) { result in
                switch result {
                case .success(let data):
                    let base64 = data.base64EncodedString()
                    invoke.resolve([
                        "success": true,
                        "value": base64
                    ])
                case .failure(let error):
                    invoke.resolve([
                        "success": false,
                        "error": error.localizedDescription
                    ])
                }
            }
        }
    }

    @objc public func dispatchTouch(_ invoke: Invoke) {
        guard let args = try? invoke.parseArgs(TouchArgs.self) else {
            invoke.reject("Failed to parse arguments")
            return
        }

        guard let wv = webView else {
            invoke.reject("WebView not available")
            return
        }

        // Use JavaScript to dispatch touch/pointer events
        // Native UITouch injection is complex and requires private APIs
        let eventType: String
        switch args.type {
        case "down":
            eventType = "pointerdown"
        case "up":
            eventType = "pointerup"
        case "move":
            eventType = "pointermove"
        default:
            invoke.reject("Unknown touch type: \(args.type)")
            return
        }

        let script = """
        (function() {
            var el = document.elementFromPoint(\(args.x), \(args.y));
            if (el) {
                var event = new PointerEvent('\(eventType)', {
                    bubbles: true,
                    cancelable: true,
                    clientX: \(args.x),
                    clientY: \(args.y),
                    pointerId: 1,
                    pointerType: 'touch',
                    isPrimary: true
                });
                el.dispatchEvent(event);
            }
        })();
        """

        DispatchQueue.main.async {
            wv.evaluateJavaScript(script) { _, error in
                if let error = error {
                    invoke.reject("Touch dispatch failed: \(error.localizedDescription)")
                } else {
                    invoke.resolve()
                }
            }
        }
    }

    @objc public func getAlertText(_ invoke: Invoke) {
        alertLock.lock()
        let alert = pendingAlert
        alertLock.unlock()

        if let alert = alert {
            invoke.resolve([
                "message": alert.message
            ])
        } else {
            invoke.reject("no such alert")
        }
    }

    @objc public func acceptAlert(_ invoke: Invoke) {
        alertLock.lock()
        let alert = pendingAlert
        pendingAlert = nil
        alertLock.unlock()

        if let alert = alert {
            let promptText = alert.promptInput ?? alert.defaultText
            alert.completionHandler?(true, promptText)
            invoke.resolve()
        } else {
            invoke.reject("no such alert")
        }
    }

    @objc public func dismissAlert(_ invoke: Invoke) {
        alertLock.lock()
        let alert = pendingAlert
        pendingAlert = nil
        alertLock.unlock()

        if let alert = alert {
            alert.completionHandler?(false, nil)
            invoke.resolve()
        } else {
            invoke.reject("no such alert")
        }
    }

    @objc public func sendAlertText(_ invoke: Invoke) {
        guard let args = try? invoke.parseArgs(SendAlertTextArgs.self) else {
            invoke.reject("Failed to parse arguments")
            return
        }

        alertLock.lock()
        let alert = pendingAlert
        alertLock.unlock()

        if let alert = alert {
            if alert.type == "prompt" {
                alert.promptInput = args.promptText
                invoke.resolve()
            } else {
                invoke.reject("Alert is not a prompt")
            }
        } else {
            invoke.reject("no such alert")
        }
    }

    @objc public func getViewportSize(_ invoke: Invoke) {
        guard let wv = webView else {
            invoke.reject("WebView not available")
            return
        }

        DispatchQueue.main.async {
            invoke.resolve([
                "width": Int(wv.bounds.width),
                "height": Int(wv.bounds.height)
            ])
        }
    }
}

@_cdecl("init_plugin_webdriver")
func initPlugin() -> Plugin {
    return WebDriverPlugin()
}
