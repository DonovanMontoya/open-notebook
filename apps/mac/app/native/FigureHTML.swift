import AppKit
import WebKit

final class FigureHTML: NSObject, WKNavigationDelegate, WKScriptMessageHandler {
    let root = NSView()
    let web: WKWebView
    let status = NSTextField(labelWithString: "Loading visualization…")
    var pageRequested: ((Int) -> Void)?
    var content = ""
    var dark = false
    var revision: UInt64 = 0
    var shown = false

    override init() {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        configuration.mediaTypesRequiringUserActionForPlayback = .all
        web = WKWebView(frame: .zero, configuration: configuration)
        super.init()
        root.wantsLayer = true
        web.navigationDelegate = self
        web.configuration.userContentController.add(self, name: "paperPage")
        for view in [web, status] { view.translatesAutoresizingMaskIntoConstraints = false; root.addSubview(view) }
        status.textColor = .secondaryLabelColor
        status.maximumNumberOfLines = 3
        NSLayoutConstraint.activate([
            web.leadingAnchor.constraint(equalTo: root.leadingAnchor), web.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            web.topAnchor.constraint(equalTo: root.topAnchor), web.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            status.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 16), status.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -16),
            status.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -12)
        ])
        root.isHidden = true
    }

    func close() {
        web.configuration.userContentController.removeScriptMessageHandler(forName: "paperPage")
        web.navigationDelegate = nil
        web.loadHTMLString("", baseURL: nil)
    }

    func display(_ html: String, dark: Bool, revision: UInt64) {
        root.isHidden = false
        guard !shown || html != content || self.dark != dark || self.revision != revision else { return }
        shown = true; content = html; self.dark = dark; self.revision = revision
        web.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        status.stringValue = "Loading visualization…"; status.isHidden = false
        web.loadHTMLString(Self.envelope(html, dark: dark), baseURL: nil)
    }

    func hide() {
        guard shown else { return }
        shown = false; root.isHidden = true
        web.loadHTMLString("", baseURL: nil)
    }

    static func envelope(_ html: String, dark: Bool) -> String {
        let bridge = """
        <script>
        window.addEventListener('error', e => parent.postMessage({type:'figure-error',message:e.message},'*'));
        window.addEventListener('unhandledrejection', () => parent.postMessage({type:'figure-error',message:'An interaction could not complete.'},'*'));
        </script>
        """
        var document = html
        if let head = document.range(of: "<head>", options: .caseInsensitive) { document.insert(contentsOf: bridge, at: head.upperBound) }
        else { document = bridge + document }
        let encoded = try! JSONSerialization.data(withJSONObject: document, options: [.fragmentsAllowed])
        let literal = String(data: encoded, encoding: .utf8)!.replacingOccurrences(of: "<", with: "\\u003c").replacingOccurrences(of: "\u{2028}", with: "\\u2028").replacingOccurrences(of: "\u{2029}", with: "\\u2029")
        return """
        <!doctype html><html><head><meta charset="utf-8">
        <meta name="viewport" content="width=device-width,initial-scale=1">
        <meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; frame-src about:; media-src data:; object-src 'none'; base-uri 'none'; form-action 'none'">
        <style>html,body{height:100%;margin:0;background:\(dark ? "#19191b" : "#fff");color-scheme:\(dark ? "dark" : "light")}iframe{display:block;width:100%;height:100%;border:0}</style>
        </head><body><iframe sandbox="allow-scripts" title="Interactive figure"></iframe><script>
        const frame=document.querySelector('iframe');
        frame.srcdoc=\(literal);
        window.addEventListener('message',event=>{
          if(event.source!==frame.contentWindow||!event.data||typeof event.data!=='object')return;
          if(event.data.type==='pdf-page'&&Number.isInteger(event.data.page)&&event.data.page>0&&event.data.page<100000)
            window.webkit.messageHandlers.paperPage.postMessage({page:event.data.page});
          if(event.data.type==='figure-error'&&typeof event.data.message==='string')
            window.webkit.messageHandlers.paperPage.postMessage({error:event.data.message.slice(0,300)});
        });
        </script></body></html>
        """
    }

    func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
        guard shown, message.frameInfo.isMainFrame, let body = message.body as? [String: Any] else { return }
        if let page = body["page"] as? Int, page > 0 { pageRequested?(page) }
        if let error = body["error"] as? String {
            status.stringValue = "Visualization error: " + String(error.prefix(300)); status.isHidden = false
        }
    }

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        decisionHandler(navigationAction.request.url?.scheme == "about" ? .allow : .cancel)
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { status.isHidden = true }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { failed(error) }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { failed(error) }
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { status.stringValue = "Visualization stopped. Choose Restart to load it again."; status.isHidden = false }
    func failed(_ error: Error) { if shown { status.stringValue = "Could not load visualization: " + error.localizedDescription; status.isHidden = false } }
}
