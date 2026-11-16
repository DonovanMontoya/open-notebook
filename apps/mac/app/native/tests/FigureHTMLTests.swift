import Foundation
@main struct EnvelopeTest {
    static func main() {
        let attack = "<html><head></head><body></script><img src=\"https://example.invalid/leak\"><script>parent.document.body.textContent='escape';</script></body></html>"
        let html = FigureHTML.envelope(attack, dark: false)
        precondition(html.contains("sandbox=\"allow-scripts\""))
        precondition(!html.contains("allow-same-origin"))
        precondition(html.contains("connect-src 'none'"))
        precondition(html.contains("event.source!==frame.contentWindow"))
        precondition(!html.contains("</script><img"))
        precondition(html.components(separatedBy: "</script>").count == 2)
        let marker = "frame.srcdoc="
        let a = html.range(of: marker)!.upperBound
        let b = html.range(of: ";\n", range: a..<html.endIndex)!.lowerBound
        let encoded = String(html[a..<b]).data(using: .utf8)!
        let decoded = try! JSONSerialization.jsonObject(with: encoded, options: [.fragmentsAllowed]) as! String
        precondition(decoded.contains("https://example.invalid/leak"))
        precondition(decoded.contains("<head><script>"))
        print("Native HTML envelope isolation checks passed")
    }
}
