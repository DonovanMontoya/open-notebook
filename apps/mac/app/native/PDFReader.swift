import AppKit
import PDFKit

struct PaperFigure {
    let label: String
    let caption: String
    let page: Int
    let selection: PDFSelection
    let references: [[String: Any]]
}

final class Reader: NSObject, NSSearchFieldDelegate {
    let document: PDFDocument
    let root = NSView()
    let bar = NSStackView()
    let research = NSStackView()
    let findbar = NSStackView()
    var figureHTML: FigureHTML?
    var visualizationPage: Int32 = 0
    let pdf = PDFView()
    let figurePicker = NSPopUpButton()
    var figures: [PaperFigure] = []
    var event: Data?
    let thumbnails = PDFThumbnailView()
    let split = NSSplitView()
    let position = NSTextField(labelWithString: "")
    let search = NSSearchField()
    let result = NSTextField(labelWithString: "")
    var matches: [PDFSelection] = []
    var matchIndex = 0
    var fitting = true
    var mounted = false
    weak var hostWindow: NSWindow?
    var hostAccessibilityChildren: [Any]?
    var mouseMonitor: Any?

    init(document: PDFDocument) {
        self.document = document
        super.init()
        root.wantsLayer = true
        root.setAccessibilityElement(true)
        root.setAccessibilityRole(.group)
        root.setAccessibilityIdentifier("library-native-pdf")
        root.setAccessibilityLabel("Original PDF")
        bar.orientation = .vertical
        bar.alignment = .leading
        bar.spacing = 6
        bar.edgeInsets = NSEdgeInsets(top: 8, left: 12, bottom: 8, right: 12)
        func button(_ title: String, _ action: Selector, _ label: String) -> NSButton {
            let b = NSButton(title: title, target: self, action: action)
            b.bezelStyle = .rounded
            b.setAccessibilityLabel(label)
            return b
        }
        let navigation = NSStackView(); navigation.orientation = .horizontal; navigation.spacing = 6
        research.orientation = .horizontal; research.spacing = 6
        findbar.orientation = .horizontal; findbar.spacing = 6
        bar.addArrangedSubview(navigation); bar.addArrangedSubview(research)
        navigation.addArrangedSubview(button("Pages", #selector(toggleSidebar), "Toggle page thumbnails"))
        navigation.addArrangedSubview(button("‹", #selector(previousPage), "Previous page"))
        position.setContentHuggingPriority(.required, for: .horizontal)
        navigation.addArrangedSubview(position)
        navigation.addArrangedSubview(button("›", #selector(nextPage), "Next page"))
        navigation.addArrangedSubview(button("−", #selector(zoomOut), "Zoom out"))
        navigation.addArrangedSubview(button("+", #selector(zoomIn), "Zoom in"))
        navigation.addArrangedSubview(button("Fit", #selector(fit), "Fit page to window"))
        search.placeholderString = "Find in document"
        search.setAccessibilityLabel("Find in document")
        search.delegate = self
        search.target = self
        search.action = #selector(findNext)
        search.sendsWholeSearchString = true
        search.sendsSearchStringImmediately = false
        search.widthAnchor.constraint(greaterThanOrEqualToConstant: 140).isActive = true
        research.addArrangedSubview(button("Highlight & ask", #selector(highlightSelection), "Highlight selection and add it to chat"))
        figures = detectFigures()
        figurePicker.addItem(withTitle: "Figures (\(figures.count))")
        for figure in figures { figurePicker.addItem(withTitle: "\(figure.label) · p. \(figure.page + 1)") }
        figurePicker.target = self
        figurePicker.action = #selector(chooseFigure)
        figurePicker.setAccessibilityLabel("Figures in this paper")
        research.addArrangedSubview(figurePicker)
        mouseMonitor = NSEvent.addLocalMonitorForEvents(matching: .leftMouseUp) { [weak self] event in
            if let self, event.window === self.root.window, self.root.superview != nil, !self.root.isHidden, !self.split.isHidden,
               self.pdf.bounds.contains(self.pdf.convert(event.locationInWindow, from: nil)) {
                DispatchQueue.main.async { [weak self] in self?.captureSelection() }
            }
            return event
        }
        findbar.addArrangedSubview(search)
        findbar.addArrangedSubview(result)
        findbar.addArrangedSubview(button("‹", #selector(findPrevious), "Previous match"))
        findbar.addArrangedSubview(button("›", #selector(findNext), "Next match"))
        research.addArrangedSubview(findbar)
        pdf.document = document
        pdf.displayMode = .singlePageContinuous
        pdf.displayDirection = .vertical
        pdf.displaysPageBreaks = true
        pdf.autoScales = true
        pdf.minScaleFactor = 0.15
        pdf.maxScaleFactor = 6
        pdf.backgroundColor = .windowBackgroundColor
        thumbnails.pdfView = pdf
        thumbnails.thumbnailSize = NSSize(width: 110, height: 150)

        split.isVertical = true
        split.dividerStyle = .thin
        split.addArrangedSubview(thumbnails)
        split.addArrangedSubview(pdf)
        for view in [bar, split] {
            view.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(view)
        }
        NSLayoutConstraint.activate([
            bar.topAnchor.constraint(equalTo: root.topAnchor), bar.leadingAnchor.constraint(equalTo: root.leadingAnchor), bar.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            split.topAnchor.constraint(equalTo: bar.bottomAnchor), split.bottomAnchor.constraint(equalTo: root.bottomAnchor), split.leadingAnchor.constraint(equalTo: root.leadingAnchor), split.trailingAnchor.constraint(equalTo: root.trailingAnchor)
        ])
        NotificationCenter.default.addObserver(self, selector: #selector(pageChanged), name: .PDFViewPageChanged, object: pdf)
    }

    deinit {
        NotificationCenter.default.removeObserver(self)
        if let mouseMonitor { NSEvent.removeMonitor(mouseMonitor) }
        figureHTML?.close()
    }

    func mount(in parent: NSView, rect: NSRect) {
        let changed = root.frame.size != rect.size
        root.frame = rect
        if root.superview !== parent {
            hostWindow = parent.window
            hostAccessibilityChildren = parent.window?.accessibilityChildren()
            parent.addSubview(root)
            parent.window?.setAccessibilityChildren((hostAccessibilityChildren ?? []) + [root])
        }
        root.isHidden = false
        if rect.width < 580, !bar.arrangedSubviews.contains(where: { $0 === findbar }) {
            research.removeArrangedSubview(findbar); findbar.removeFromSuperview(); bar.addArrangedSubview(findbar)
        } else if rect.width >= 580, !research.arrangedSubviews.contains(where: { $0 === findbar }) {
            bar.removeArrangedSubview(findbar); findbar.removeFromSuperview(); research.addArrangedSubview(findbar)
        }
        root.layoutSubtreeIfNeeded()
        if !mounted {
            mounted = true
            split.setPosition(rect.width < 700 ? 0 : 140, ofDividerAt: 0)
            thumbnails.isHidden = rect.width < 700
            DispatchQueue.main.async { [weak self] in
                guard let self, self.root.superview != nil else { return }
                self.pdf.layoutDocumentView()
                self.fit()
                if let first = self.document.page(at: 0) {
                    let box = first.bounds(for: .cropBox)
                    self.pdf.go(to: PDFDestination(page: first, at: NSPoint(x: box.minX, y: box.maxY)))
                }
                self.pageChanged()
            }
        } else if changed && fitting { fit() }
    }

    func showVisualization(_ html: String?, dark: Bool, revision: UInt64) {
        guard let html else {
            bar.isHidden = false; split.isHidden = false; figureHTML?.hide()
            root.setAccessibilityLabel("Original PDF")
            return
        }
        if figureHTML == nil {
            let surface = FigureHTML()
            surface.pageRequested = { [weak self] page in
                guard let self, page <= self.document.pageCount else { return }
                self.visualizationPage = Int32(page)
            }
            surface.root.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(surface.root)
            NSLayoutConstraint.activate([
                surface.root.topAnchor.constraint(equalTo: root.topAnchor), surface.root.bottomAnchor.constraint(equalTo: root.bottomAnchor),
                surface.root.leadingAnchor.constraint(equalTo: root.leadingAnchor), surface.root.trailingAnchor.constraint(equalTo: root.trailingAnchor)
            ])
            figureHTML = surface
        }
        bar.isHidden = true; split.isHidden = true
        root.setAccessibilityLabel("Interactive figure")
        figureHTML?.display(html, dark: dark, revision: revision)
    }

    func detectFigures() -> [PaperFigure] {
        var found: [PaperFigure] = []
        let pattern = try! NSRegularExpression(pattern: "^\\s*(?:Figure|Fig\\.)\\s*([0-9]+[a-z]?)[.:]", options: [.caseInsensitive])
        for index in 0..<document.pageCount {
            guard let page = document.page(at: index), let all = page.selection(for: page.bounds(for: .cropBox)) else { continue }
            let lines = all.selectionsByLine()
            for (i, line) in lines.enumerated() {
                let text = line.string ?? ""
                guard let match = pattern.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)), let range = Range(match.range(at: 1), in: text) else { continue }
                let number = String(text[range])
                if found.contains(where: { $0.label == "Figure " + number }) { continue }
                let bounds = line.bounds(for: page)
                var caption = text
                for next in lines.dropFirst(i + 1).prefix(5) {
                    let rect = next.bounds(for: page)
                    if abs(rect.minX - bounds.minX) > 20 || rect.maxY > bounds.minY + 2 { break }
                    caption += " " + (next.string ?? "")
                    if caption.hasSuffix(".") { break }
                }
                let referencePattern = try! NSRegularExpression(pattern: "(?:Fig\\.?|Figure)\\s*" + NSRegularExpression.escapedPattern(for: number) + "(?![0-9])", options: [.caseInsensitive])
                var references: [[String: Any]] = []
                for otherIndex in 0..<document.pageCount {
                    guard let other = document.page(at: otherIndex)?.string else { continue }
                    let normalized = other.components(separatedBy: .whitespacesAndNewlines).filter { !$0.isEmpty }.joined(separator: " ") as NSString
                    for hit in referencePattern.matches(in: normalized as String, range: NSRange(location: 0, length: normalized.length)).prefix(4) {
                        let captionPrefix = String(caption.components(separatedBy: .whitespacesAndNewlines).filter { !$0.isEmpty }.joined(separator: " ").prefix(70))
                        if normalized.substring(from: hit.range.location).hasPrefix(captionPrefix) { continue }
                        let start = max(0, hit.range.location - 220)
                        let end = min(normalized.length, NSMaxRange(hit.range) + 400)
                        let snippet = normalized.substring(with: NSRange(location: start, length: end - start))
                        if !snippet.hasPrefix(caption) { references.append(["page":otherIndex + 1, "text":snippet]) }
                    }
                }
                found.append(PaperFigure(label: "Figure " + number, caption: caption, page: index, selection: line, references: Array(references.prefix(8))))
            }
        }
        return found
    }

    func captureSelection() {
        guard let selection = pdf.currentSelection, let text = selection.string?.trimmingCharacters(in: .whitespacesAndNewlines), !text.isEmpty, let page = selection.pages.first else { return }
        event = try? JSONSerialization.data(withJSONObject: ["kind":"passage", "page":document.index(for: page) + 1, "text":text, "label":"Selected passage", "references":[]])
    }

    @objc func highlightSelection() {
        guard let selection = pdf.currentSelection else { return }
        for line in selection.selectionsByLine() {
            for page in line.pages {
                let annotation = PDFAnnotation(bounds: line.bounds(for: page), forType: .highlight, withProperties: nil)
                annotation.color = NSColor.systemYellow.withAlphaComponent(0.35)
                page.addAnnotation(annotation)
            }
        }
        captureSelection()
    }

    @objc func chooseFigure() {
        let index = figurePicker.indexOfSelectedItem - 1
        guard figures.indices.contains(index) else { return }
        let figure = figures[index]
        guard let page = document.page(at: figure.page) else { return }
        pdf.setCurrentSelection(figure.selection, animate: false)
        fitting = true
        fit()
        let box = page.bounds(for: .cropBox)
        pdf.go(to: PDFDestination(page: page, at: NSPoint(x: box.minX, y: box.maxY)))
        let image = page.thumbnail(of: NSSize(width: 1600, height: 2100), for: .cropBox)
        guard let tiff = image.tiffRepresentation, let bitmap = NSBitmapImageRep(data: tiff), let png = bitmap.representation(using: .png, properties: [:]) else { return }
        event = try? JSONSerialization.data(withJSONObject: ["kind":"figure", "page":figure.page + 1, "text":figure.caption, "label":figure.label, "png":png.base64EncodedString(), "references":figure.references])
    }

    func unmount() {
        hostWindow?.setAccessibilityChildren(hostAccessibilityChildren)
        root.removeFromSuperview()
    }

    @objc func pageChanged() {
        let page = pdf.currentPage.map { document.index(for: $0) + 1 } ?? 1
        position.stringValue = "\(page) / \(document.pageCount)"
    }
    @objc func previousPage() { pdf.goToPreviousPage(nil) }
    @objc func nextPage() { pdf.goToNextPage(nil) }
    @objc func zoomIn() { fitting = false; pdf.zoomIn(nil) }
    @objc func zoomOut() { fitting = false; pdf.zoomOut(nil) }
    @objc func fit() {
        fitting = true
        guard let page = pdf.currentPage ?? document.page(at: 0) else { return }
        let bounds = page.bounds(for: .cropBox)
        guard bounds.width > 0, bounds.height > 0 else { return }
        pdf.autoScales = false
        pdf.scaleFactor = min(max(0.15, (pdf.bounds.width - 24) / bounds.width), max(0.15, (pdf.bounds.height - 24) / bounds.height))
    }
    @objc func toggleSidebar() { thumbnails.isHidden.toggle(); root.layoutSubtreeIfNeeded(); if fitting { fit() } }
    func controlTextDidChange(_ notification: Notification) { updateSearch() }
    func updateSearch() {
        matches = search.stringValue.isEmpty ? [] : document.findString(search.stringValue, withOptions: [.caseInsensitive])
        matchIndex = 0
        pdf.highlightedSelections = matches
        revealMatch()
    }
    @objc func findNext() {
        guard !matches.isEmpty else { updateSearch(); return }
        matchIndex = (matchIndex + 1) % matches.count
        revealMatch()
    }
    @objc func findPrevious() {
        guard !matches.isEmpty else { return }
        matchIndex = (matchIndex + matches.count - 1) % matches.count
        revealMatch()
    }
    func revealMatch() {
        result.stringValue = search.stringValue.isEmpty ? "" : matches.isEmpty ? "No matches" : "\(matchIndex + 1) / \(matches.count)"
        if !matches.isEmpty { pdf.setCurrentSelection(matches[matchIndex], animate: false); pdf.go(to: matches[matchIndex]) }
    }
}

private var reader: Reader?
private var documentID: UInt64?

@_cdecl("notebook_pdf_pages")
public func pageCount(_ bytes: UnsafePointer<UInt8>, _ length: Int) -> Int32 {
    guard let document = PDFDocument(data: Data(bytes: bytes, count: length)), !document.isLocked else { return 0 }
    return Int32(document.pageCount)
}

@_cdecl("notebook_pdf_update")
public func update(_ bytes: UnsafePointer<UInt8>, _ length: Int, _ id: UInt64, _ x: Double, _ y: Double, _ width: Double, _ height: Double) -> Int32 {
    guard Thread.isMainThread,
          let window = NSApp?.mainWindow ?? NSApp?.windows.first(where: { $0.title == "Open Notebook" }),
          let parent = window.contentView else { return 0 }
    if documentID != id || reader == nil {
        guard let document = PDFDocument(data: Data(bytes: bytes, count: length)), document.pageCount > 0, !document.isLocked else { return 0 }
        reader?.unmount()
        reader = Reader(document: document)
        documentID = id
    }
    let frame = NSRect(x: x, y: parent.isFlipped ? y : parent.bounds.height - y - height, width: width, height: height)
    reader?.mount(in: parent, rect: frame)
    return 1
}

@_cdecl("notebook_pdf_hide")
public func hide() {
    guard Thread.isMainThread else { return }
    if let root = reader?.root, root.window?.firstResponder is NSView,
       let focused = root.window?.firstResponder as? NSView, focused.isDescendant(of: root) {
        root.window?.makeFirstResponder(root.superview)
    }
    reader?.unmount()
    reader = nil
    documentID = nil
}

@_cdecl("notebook_pdf_event")
public func nextEvent(_ buffer: UnsafeMutablePointer<UInt8>?, _ capacity: Int) -> Int {
    guard Thread.isMainThread, let data = reader?.event else { return 0 }
    if let buffer, capacity >= data.count { data.copyBytes(to: buffer, count: data.count); reader?.event = nil }
    return data.count
}

@_cdecl("notebook_pdf_navigate")
public func navigate(_ page: Int32, _ bytes: UnsafePointer<UInt8>, _ length: Int) {
    guard Thread.isMainThread, let reader, let target = reader.document.page(at: Int(page) - 1) else { return }
    let box = target.bounds(for: .cropBox)
    reader.pdf.go(to: PDFDestination(page: target, at: NSPoint(x: box.minX, y: box.maxY)))
    if length > 0, let text = String(data: Data(bytes: bytes, count: length), encoding: .utf8), let original = target.string {
        let pattern = text.components(separatedBy: .whitespacesAndNewlines).filter { !$0.isEmpty }.map(NSRegularExpression.escapedPattern(for:)).joined(separator: "\\s+")
        if let regex = try? NSRegularExpression(pattern: pattern, options: [.caseInsensitive]),
           let match = regex.firstMatch(in: original, range: NSRange(original.startIndex..., in: original)), let selection = target.selection(for: match.range) {
            reader.pdf.setCurrentSelection(selection, animate: false)
            reader.pdf.go(to: selection)
        }
    }
}

@_cdecl("notebook_pdf_visualization")
public func visualization(_ bytes: UnsafePointer<UInt8>, _ length: Int, _ visible: Bool, _ dark: Bool, _ revision: UInt64) {
    guard Thread.isMainThread else { return }
    let html = visible ? String(data: Data(bytes: bytes, count: length), encoding: .utf8) : nil
    reader?.showVisualization(html, dark: dark, revision: revision)
}

@_cdecl("notebook_pdf_visualization_page")
public func visualizationPage() -> Int32 {
    guard Thread.isMainThread, let reader else { return 0 }
    let page = reader.visualizationPage; reader.visualizationPage = 0; return page
}
