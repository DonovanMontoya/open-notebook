import QuartzCore
import UIKit

/// A view backed by a `CAMetalLayer`, which Rune draws the app into.
final class MetalView: UIView {
    override class var layerClass: AnyClass { CAMetalLayer.self }
    var metalLayer: CAMetalLayer { layer as! CAMetalLayer }
}

/// Hosts the Rust app: owns the Metal view, the display link and the keyboard, and passes
/// touches and typed text to Rune through the C interface in `Bridge.h`.
final class RuneViewController: UIViewController, UIKeyInput {
    private let canvas = MetalView()
    private var host: OpaquePointer?
    private var link: CADisplayLink?
    private var wakeTimer: Timer?
    private var wantsKeyboard = false
    /// Fingers on the screen. The keyboard waits for them to lift before it goes, so the layout
    /// doesn't move under a tap that is still landing.
    private var fingers = 0

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = Self.pageColor
        canvas.isMultipleTouchEnabled = false
        canvas.backgroundColor = Self.pageColor
        canvas.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(canvas)
        // The app draws inside the safe area at the top and sides, and down to the bottom edge or
        // the keyboard's top. Under the home indicator it pads its last row instead (`on_inset_bottom`).
        view.keyboardLayoutGuide.usesBottomSafeArea = false
        let safe = view.safeAreaLayoutGuide
        NSLayoutConstraint.activate([
            canvas.topAnchor.constraint(equalTo: safe.topAnchor),
            canvas.leadingAnchor.constraint(equalTo: safe.leadingAnchor),
            canvas.trailingAnchor.constraint(equalTo: safe.trailingAnchor),
            canvas.bottomAnchor.constraint(equalTo: view.keyboardLayoutGuide.topAnchor),
        ])
        let center = NotificationCenter.default
        center.addObserver(self, selector: #selector(background), name: UIApplication.didEnterBackgroundNotification, object: nil)
        center.addObserver(self, selector: #selector(foreground), name: UIApplication.willEnterForegroundNotification, object: nil)
    }

    /// The surface the app draws on a phone and the Duo, so the edges outside the safe area
    /// (beside the camera, under the home indicator) match it.
    private static let pageColor = UIColor { t in
        t.userInterfaceStyle == .dark ? UIColor(red: 0x19 / 255, green: 0x19 / 255, blue: 0x1b / 255, alpha: 1) : .white
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        let size = canvas.bounds.size
        guard size.width > 0, size.height > 0 else { return }
        let scale = Float(traitCollection.displayScale)
        canvas.metalLayer.contentsScale = CGFloat(scale)
        canvas.metalLayer.drawableSize = CGSize(width: size.width * CGFloat(scale), height: size.height * CGFloat(scale))
        if let host {
            on_resize(host, Float(size.width), Float(size.height), scale)
        } else {
            start(size: size, scale: scale)
        }
        sendFold()
        // The home indicator's strip, unless the keyboard is up and covers it.
        if let host {
            let below = view.bounds.maxY - canvas.frame.maxY
            on_inset_bottom(host, Float(max(0, view.safeAreaInsets.bottom - below)))
        }
        wake()
    }

    /// Tells the app where the fold divides the view when the iPhone Duo is partly folded, so
    /// the notebook's two pages meet either side of it. Layout runs again whenever the fold moves.
    private func sendFold() {
        guard let host else { return }
        if #available(iOS 27.1, *) {
            let fold = canvas.reservedRegions(kind: .division).first(where: { $0.isActive })
            if let f = fold?.frame {
                on_fold(host, Float(f.minX), Float(f.minY), Float(f.width), Float(f.height), true)
                return
            }
        }
        on_fold(host, 0, 0, 0, 0, false)
    }

    private func start(size: CGSize, scale: Float) {
        let dir = Self.dataDirectory()
        let dark = traitCollection.userInterfaceStyle == .dark
        let me = Unmanaged.passUnretained(self).toOpaque()
        host = dir.path.withCString { path in
            on_create(Unmanaged.passUnretained(canvas.metalLayer).toOpaque(), Float(size.width), Float(size.height), scale, dark, path, { ctx in
                guard let ctx else { return }
                let vc = Unmanaged<RuneViewController>.fromOpaque(ctx).takeUnretainedValue()
                DispatchQueue.main.async { vc.wake() }
            }, me)
        }
        let link = CADisplayLink(target: self, selector: #selector(tick))
        link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    /// Notebooks, keys and settings live in Application Support inside the app's sandbox.
    private static func dataDirectory() -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let dir = base.appendingPathComponent("OpenNotebook", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    /// Asks for frames again: after input, a finished background job or a timer.
    func wake() {
        link?.isPaused = false
    }

    @objc private func tick() {
        guard let host else { return }
        let f = on_frame(host)
        if f.wants_keyboard != wantsKeyboard && (f.wants_keyboard || fingers == 0) {
            wantsKeyboard = f.wants_keyboard
            if wantsKeyboard { becomeFirstResponder() } else { resignFirstResponder() }
        }
        if !f.animating {
            link?.isPaused = true
            wakeTimer?.invalidate()
            wakeTimer = nil
            if f.wake_after >= 0 {
                wakeTimer = Timer.scheduledTimer(withTimeInterval: max(f.wake_after, 0.001), repeats: false) { [weak self] _ in self?.wake() }
            }
        }
    }

    @objc private func background() {
        // Metal may not draw in the background.
        link?.isPaused = true
        if let host { on_resize(host, 0, 0, Float(canvas.metalLayer.contentsScale)) }
    }

    @objc private func foreground() {
        view.setNeedsLayout()
        wake()
    }

    // Touches

    private func send(_ touches: Set<UITouch>, _ phase: Int32) {
        guard let host else { return }
        for t in touches {
            let p = t.location(in: canvas)
            let id = UInt64(UInt(bitPattern: ObjectIdentifier(t).hashValue))
            on_touch(host, id, phase, Float(p.x), Float(p.y), t.timestamp)
        }
        wake()
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        fingers += touches.count
        send(touches, 0)
    }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) { send(touches, 1) }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        fingers = max(0, fingers - touches.count)
        send(touches, 2)
    }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        fingers = max(0, fingers - touches.count)
        send(touches, 3)
    }

    // Keyboard: the view controller is the text input while a Rune text field has focus.

    override var canBecomeFirstResponder: Bool { wantsKeyboard }
    var hasText: Bool { true }
    var autocorrectionType: UITextAutocorrectionType = .no
    var autocapitalizationType: UITextAutocapitalizationType = .sentences

    func insertText(_ text: String) {
        guard let host else { return }
        text.withCString { on_text(host, $0) }
        wake()
    }

    func deleteBackward() {
        guard let host else { return }
        on_backspace(host)
        wake()
    }

    deinit {
        link?.invalidate()
        if let host { on_destroy(host) }
    }
}
