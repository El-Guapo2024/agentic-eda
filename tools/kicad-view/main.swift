// KiCad View: our own KiCad, on its own screen, shown in a browser.
//
// The way a simulator panel works: this app makes a screen that exists only
// in software (a virtual display), launches its own KiCad PCB Editor there
// with a private copy of the user's KiCad settings, streams that screen to
// a web page, and plays the page's mouse and keys back into that KiCad.
// Nothing on the user's real screens is captured, and it only listens on
// 127.0.0.1.
//
//   open build/KiCadView.app --args <board.kicad_pcb> [--port 8770] [--size 1600x1000] [--input pid|hid]
//
// The page is at http://127.0.0.1:<port>/; frames and input go over a
// WebSocket on the next port. Quitting the app quits that KiCad and takes
// its screen away. Needs Screen Recording (to see the screen) and
// Accessibility (to send it input), which macOS asks for on first run.

import AppKit
import CoreImage
import Network
import ScreenCaptureKit

// MARK: - Options and log

struct Options {
    var board: String?
    var port: UInt16 = 8770
    var width = 1600
    var height = 1000
    var fps: Int32 = 20
    /// "pid" sends input to KiCad's process and never touches the user's
    /// pointer. "hid" posts it as the hardware would, which moves the real
    /// pointer onto our screen: off unless asked for explicitly.
    var input = "pid"

    static func parse() -> Options {
        var o = Options()
        var args = CommandLine.arguments.dropFirst().makeIterator()
        while let a = args.next() {
            switch a {
            case "--port": if let v = args.next(), let p = UInt16(v) { o.port = p }
            case "--size":
                if let v = args.next() {
                    let wh = v.split(separator: "x").compactMap { Int($0) }
                    if wh.count == 2 { o.width = wh[0]; o.height = wh[1] }
                }
            case "--fps": if let v = args.next(), let f = Int32(v) { o.fps = max(1, min(60, f)) }
            case "--input": if let v = args.next() { o.input = v }
            default: if !a.hasPrefix("-") { o.board = a }
            }
        }
        return o
    }
}

let logURL = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Logs/KiCadView.log")

func log(_ s: String) {
    let line = "\(Date()) \(s)\n"
    if let h = try? FileHandle(forWritingTo: logURL) {
        h.seekToEndOfFile(); h.write(Data(line.utf8)); try? h.close()
    } else {
        try? Data(line.utf8).write(to: logURL)
    }
}

// MARK: - The screen

/// A monitor that exists only in software.
final class Screen {
    let display: CGVirtualDisplay
    let id: CGDirectDisplayID

    init(width: Int, height: Int) {
        let d = CGVirtualDisplayDescriptor()
        d.queue = DispatchQueue.main
        d.name = "KiCad View"
        d.maxPixelsWide = UInt32(width)
        d.maxPixelsHigh = UInt32(height)
        d.sizeInMillimeters = CGSize(width: Double(width) / 4, height: Double(height) / 4)
        d.productID = 0x4b43
        d.vendorID = 0x4b43
        d.serialNum = 1
        display = CGVirtualDisplay(descriptor: d)
        let s = CGVirtualDisplaySettings()
        s.hiDPI = 0
        s.modes = [CGVirtualDisplayMode(width: UInt32(width), height: UInt32(height), refreshRate: 60) as Any]
        display.apply(s)
        id = display.displayID
    }

    var bounds: CGRect { CGDisplayBounds(id) }

    /// Its place in the system's display list, which is how KiCad names
    /// the display it saved a window on.
    var index: Int {
        var ids = [CGDirectDisplayID](repeating: 0, count: 16)
        var n: UInt32 = 0
        CGGetActiveDisplayList(16, &ids, &n)
        return ids.prefix(Int(n)).firstIndex(of: id) ?? 1
    }
}

// MARK: - Our KiCad

final class KiCad {
    var process: Process?
    let configHome = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/KiCadView/kicad-config")
    /// The PCB Editor inside KiCad.app: KiCad finds its editor library by
    /// walking up from where the binary sits, and only from inside the
    /// bundle does that land on KiCad.app/Contents/PlugIns. The "PCB
    /// Editor.app" copy beside it fails with "Error loading editor".
    let pcbnew = "/Applications/KiCad/KiCad.app/Contents/Applications/pcbnew.app/Contents/MacOS/pcbnew"

    /// "10.99" for 10.99.0-857-g8adf6cd23d: KiCad's settings folder name.
    var version: String {
        let plist = NSDictionary(contentsOfFile: "/Applications/KiCad/KiCad.app/Contents/Info.plist")
        let v = plist?["CFBundleShortVersionString"] as? String ?? "10.99"
        return v.split(separator: ".").prefix(2).joined(separator: ".")
    }

    /// A private copy of the user's KiCad settings (libraries, colours,
    /// hotkeys), with the PCB Editor's window put on our screen. The user's
    /// own settings are only read.
    func prepare(screen: Screen) throws {
        let fm = FileManager.default
        let mine = configHome.appendingPathComponent(version)
        if !fm.fileExists(atPath: mine.path) {
            let theirs = fm.homeDirectoryForCurrentUser.appendingPathComponent("Library/Preferences/kicad/\(version)")
            try fm.createDirectory(at: configHome, withIntermediateDirectories: true)
            if fm.fileExists(atPath: theirs.path) {
                try fm.copyItem(at: theirs, to: mine)
            } else {
                try fm.createDirectory(at: mine, withIntermediateDirectories: true)
            }
        }
        let url = mine.appendingPathComponent("pcbnew.json")
        var json = (try? JSONSerialization.jsonObject(with: Data(contentsOf: url))) as? [String: Any] ?? [:]
        var w = json["window"] as? [String: Any] ?? [:]
        let b = screen.bounds
        w["display"] = screen.index
        w["maximized"] = true
        w["pos_x"] = Int(b.origin.x) + 20
        w["pos_y"] = Int(b.origin.y) + 40
        w["size_x"] = Int(b.width) - 40
        w["size_y"] = Int(b.height) - 80
        json["window"] = w
        try JSONSerialization.data(withJSONObject: json, options: [.prettyPrinted]).write(to: url)

        // Software drawing (Cairo): KiCad's OpenGL canvas fails on a
        // virtual display ("switching framebuffer: invalid framebuffer
        // operation").
        let common = mine.appendingPathComponent("kicad_common.json")
        var c = (try? JSONSerialization.jsonObject(with: Data(contentsOf: common))) as? [String: Any] ?? [:]
        var g = c["graphics"] as? [String: Any] ?? [:]
        g["canvas_type"] = 2
        c["graphics"] = g
        try JSONSerialization.data(withJSONObject: c, options: [.prettyPrinted]).write(to: common)
    }

    func launch(board: String?) throws -> pid_t {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: pcbnew)
        if let board { p.arguments = [board] }
        // Its own documents folder too: KiCad looks for plugins in
        // ~/Documents/KiCad, and macOS holds that open() until the user
        // answers a folder-access prompt -- which stalled KiCad before it
        // drew a window.
        let documents = configHome.deletingLastPathComponent().appendingPathComponent("documents")
        try? FileManager.default.createDirectory(at: documents, withIntermediateDirectories: true)
        var env = ProcessInfo.processInfo.environment
        env["KICAD_CONFIG_HOME"] = configHome.path
        env["KICAD_DOCUMENTS_HOME"] = documents.path
        p.environment = env
        try p.run()
        process = p
        return p.processIdentifier
    }

    func quit() {
        guard let p = process, p.isRunning else { return }
        p.terminate()
    }

    /// The board last opened, for a relaunch that names none: System
    /// Settings' "Quit & Reopen" after a permission is granted passes no
    /// arguments.
    var lastBoard: String? {
        get { try? String(contentsOf: configHome.deletingLastPathComponent().appendingPathComponent("last-board.txt"), encoding: .utf8) }
        set {
            let url = configHome.deletingLastPathComponent().appendingPathComponent("last-board.txt")
            try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try? (newValue ?? "").write(to: url, atomically: true, encoding: .utf8)
        }
    }

    /// Put every window of our KiCad that is not on `screen` onto it,
    /// centred: dialogs a KiCad shows before its main window exists open on
    /// the main display, which is the user's. Needs Accessibility.
    func sweep(onto screen: CGRect) {
        guard let p = process, p.isRunning, AXIsProcessTrusted() else { return }
        let app = AXUIElementCreateApplication(p.processIdentifier)
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &value) == .success,
              let windows = value as? [AXUIElement] else { return }
        for w in windows {
            var posRef: CFTypeRef?, sizeRef: CFTypeRef?
            guard AXUIElementCopyAttributeValue(w, kAXPositionAttribute as CFString, &posRef) == .success,
                  AXUIElementCopyAttributeValue(w, kAXSizeAttribute as CFString, &sizeRef) == .success
            else { continue }
            var pos = CGPoint.zero, size = CGSize.zero
            AXValueGetValue(posRef as! AXValue, .cgPoint, &pos)
            AXValueGetValue(sizeRef as! AXValue, .cgSize, &size)
            let centre = CGPoint(x: pos.x + size.width / 2, y: pos.y + size.height / 2)
            if screen.contains(centre) { continue }
            var to = CGPoint(x: max(screen.minX, screen.midX - size.width / 2), y: max(screen.minY, screen.midY - size.height / 2))
            if let v = AXValueCreate(.cgPoint, &to) {
                AXUIElementSetAttributeValue(w, kAXPositionAttribute as CFString, v)
                log("moved a KiCad window from \(pos) onto our screen")
            }
        }
    }
}

// MARK: - Capture

/// Frames of our screen, as JPEG, only when something on it changed.
final class Capture: NSObject, SCStreamOutput, SCStreamDelegate {
    var stream: SCStream?
    let context = CIContext()
    var onFrame: ((Data) -> Void)?
    var onError: ((String) -> Void)?

    func start(display id: CGDirectDisplayID, width: Int, height: Int, fps: Int32) async throws {
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
        guard let display = content.displays.first(where: { $0.displayID == id }) else {
            throw NSError(domain: "KiCadView", code: 1, userInfo: [NSLocalizedDescriptionKey: "our screen is not among the shareable displays"])
        }
        let filter = SCContentFilter(display: display, excludingWindows: [])
        let cfg = SCStreamConfiguration()
        cfg.width = width
        cfg.height = height
        cfg.minimumFrameInterval = CMTime(value: 1, timescale: fps)
        cfg.pixelFormat = kCVPixelFormatType_32BGRA
        cfg.showsCursor = false
        cfg.queueDepth = 4
        let s = SCStream(filter: filter, configuration: cfg, delegate: self)
        try s.addStreamOutput(self, type: .screen, sampleHandlerQueue: DispatchQueue(label: "capture"))
        try await s.startCapture()
        stream = s
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sb: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, sb.isValid,
              let attachments = CMSampleBufferGetSampleAttachmentsArray(sb, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
              let raw = attachments.first?[.status] as? Int, SCFrameStatus(rawValue: raw) == .complete,
              let pixels = sb.imageBuffer
        else { return }
        let image = CIImage(cvPixelBuffer: pixels)
        let options = [CIImageRepresentationOption(rawValue: kCGImageDestinationLossyCompressionQuality as String): 0.8]
        guard let jpeg = context.jpegRepresentation(of: image, colorSpace: CGColorSpace(name: CGColorSpace.sRGB)!, options: options) else { return }
        onFrame?(jpeg)
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        onError?("capture stopped: \(error.localizedDescription)")
    }
}

// MARK: - Server

/// The viewer page on one port, a WebSocket for frames and input on the
/// next, both on 127.0.0.1 only.
final class Server {
    final class Client {
        let conn: NWConnection
        var sending = false
        var pending: Data?
        init(_ c: NWConnection) { conn = c }
    }

    let port: UInt16
    let page: Data
    let queue = DispatchQueue(label: "server")
    var clients: [ObjectIdentifier: Client] = [:]
    var lastFrame: Data?
    var status: [String: Any] = [:]
    var onInput: (([String: Any]) -> Void)?
    var listeners: [NWListener] = []

    init(port: UInt16, page: String) {
        self.port = port
        self.page = Data(page.replacingOccurrences(of: "__WS_PORT__", with: String(port + 1)).utf8)
    }

    func start() throws {
        let plain = NWParameters.tcp
        plain.requiredLocalEndpoint = .hostPort(host: .ipv4(.loopback), port: NWEndpoint.Port(rawValue: port)!)
        let pageListener = try NWListener(using: plain)
        pageListener.newConnectionHandler = { [weak self] c in self?.servePage(c) }
        pageListener.start(queue: queue)

        let ws = NWParameters.tcp
        let options = NWProtocolWebSocket.Options()
        options.autoReplyPing = true
        ws.defaultProtocolStack.applicationProtocols.insert(options, at: 0)
        ws.requiredLocalEndpoint = .hostPort(host: .ipv4(.loopback), port: NWEndpoint.Port(rawValue: port + 1)!)
        let wsListener = try NWListener(using: ws)
        wsListener.newConnectionHandler = { [weak self] c in self?.accept(c) }
        wsListener.start(queue: queue)
        listeners = [pageListener, wsListener]
    }

    private func servePage(_ c: NWConnection) {
        c.start(queue: queue)
        c.receive(minimumIncompleteLength: 1, maximumLength: 65536) { [weak self] _, _, _, _ in
            guard let self else { return }
            var out = Data("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: \(self.page.count)\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n".utf8)
            out.append(self.page)
            c.send(content: out, completion: .contentProcessed { _ in c.cancel() })
        }
    }

    private func accept(_ c: NWConnection) {
        let client = Client(c)
        let key = ObjectIdentifier(c)
        c.stateUpdateHandler = { [weak self] state in
            guard let self else { return }
            switch state {
            case .ready:
                self.sendText(self.status, to: client)
                if let f = self.lastFrame { self.send(f, to: client) }
            case .failed, .cancelled:
                self.clients[key] = nil
            default:
                break
            }
        }
        clients[key] = client
        c.start(queue: queue)
        receive(client)
    }

    private func receive(_ client: Client) {
        client.conn.receiveMessage { [weak self] data, _, _, error in
            if let data, !data.isEmpty, let m = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] {
                DispatchQueue.main.async { self?.onInput?(m) }
            }
            if error == nil { self?.receive(client) } else { client.conn.cancel() }
        }
    }

    func broadcast(_ frame: Data) {
        queue.async {
            self.lastFrame = frame
            for c in self.clients.values { self.send(frame, to: c) }
        }
    }

    /// Tell every page how things stand: the screen's size, or what is
    /// wrong.
    func announce(_ s: [String: Any]) {
        queue.async {
            self.status.merge(s) { _, new in new }
            for c in self.clients.values { self.sendText(self.status, to: c) }
        }
    }

    /// A slow page gets the newest frame when it is ready, not a queue of
    /// stale ones.
    private func send(_ frame: Data, to c: Client) {
        if c.sending { c.pending = frame; return }
        c.sending = true
        let ctx = NWConnection.ContentContext(identifier: "frame", metadata: [NWProtocolWebSocket.Metadata(opcode: .binary)])
        c.conn.send(content: frame, contentContext: ctx, isComplete: true, completion: .contentProcessed { [weak self] _ in
            guard let self else { return }
            c.sending = false
            if let p = c.pending { c.pending = nil; self.send(p, to: c) }
        })
    }

    private func sendText(_ obj: [String: Any], to c: Client) {
        guard let data = try? JSONSerialization.data(withJSONObject: obj) else { return }
        let ctx = NWConnection.ContentContext(identifier: "status", metadata: [NWProtocolWebSocket.Metadata(opcode: .text)])
        c.conn.send(content: data, contentContext: ctx, isComplete: true, completion: .idempotent)
    }
}

// MARK: - Input

/// The page's mouse and keys, played into our KiCad at the same place on
/// our screen.
final class Input {
    var pid: pid_t = 0
    var origin = CGPoint.zero
    var mode = "pid"
    /// When the page last sent anything: while the user works in it, our
    /// KiCad may keep focus.
    var lastEvent = Date.distantPast
    private var held = Set<Int>()
    private let source = CGEventSource(stateID: .hidSystemState)

    func handle(_ m: [String: Any]) {
        lastEvent = Date()
        let t = m["t"] as? String ?? ""
        let p = CGPoint(x: origin.x + (m["x"] as? Double ?? 0), y: origin.y + (m["y"] as? Double ?? 0))
        switch t {
        case "down", "up":
            let b = m["b"] as? Int ?? 0
            let down = t == "down"
            if down { held.insert(b) } else { held.remove(b) }
            let (type, button): (CGEventType, CGMouseButton)
            switch b {
            case 2: (type, button) = (down ? .rightMouseDown : .rightMouseUp, .right)
            case 1: (type, button) = (down ? .otherMouseDown : .otherMouseUp, .center)
            default: (type, button) = (down ? .leftMouseDown : .leftMouseUp, .left)
            }
            mouse(type, at: p, button: button, clicks: max(1, m["n"] as? Int ?? 1), mods: m)
        case "move":
            let (type, button): (CGEventType, CGMouseButton) =
                held.contains(0) ? (.leftMouseDragged, .left)
                : held.contains(2) ? (.rightMouseDragged, .right)
                : held.contains(1) ? (.otherMouseDragged, .center)
                : (.mouseMoved, .left)
            mouse(type, at: p, button: button, clicks: 0, mods: m)
        case "wheel":
            // Whole lines, as a mouse wheel sends: KiCad zooms on those and
            // pans on the smooth deltas a trackpad sends.
            let dy = m["dy"] as? Double ?? 0, dx = m["dx"] as? Double ?? 0
            let lines = { (d: Double) -> Int32 in
                if d == 0 { return 0 }
                let n = Int32(max(1, min(5, (abs(d) / 40).rounded())))
                return d < 0 ? n : -n
            }
            if let e = CGEvent(scrollWheelEvent2Source: source, units: .line, wheelCount: 2, wheel1: lines(dy), wheel2: lines(dx), wheel3: 0) {
                e.location = p
                e.flags = flags(m)
                deliver(e)
            }
        case "key":
            guard let code = keyCodes[m["code"] as? String ?? ""],
                  let e = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: m["down"] as? Bool ?? true)
            else { return }
            e.flags = flags(m)
            deliver(e)
        default:
            break
        }
    }

    private func mouse(_ type: CGEventType, at p: CGPoint, button: CGMouseButton, clicks: Int, mods: [String: Any]) {
        guard let e = CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: p, mouseButton: button) else { return }
        if clicks > 0 { e.setIntegerValueField(.mouseEventClickState, value: Int64(clicks)) }
        if let w = window(at: p) {
            e.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: Int64(w))
            e.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent, value: Int64(w))
        }
        e.flags = flags(mods)
        deliver(e)
    }

    private func deliver(_ e: CGEvent) {
        if mode == "hid" { e.post(tap: .cghidEventTap) } else if pid != 0 { e.postToPid(pid) }
    }

    /// Our KiCad's frontmost window under `p`, so an event reaches the
    /// right one even though the system cursor is elsewhere.
    private func window(at p: CGPoint) -> Int? {
        guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] else { return nil }
        for w in list where (w[kCGWindowOwnerPID as String] as? pid_t) == pid {
            guard let b = w[kCGWindowBounds as String] as? [String: Double],
                  CGRect(x: b["X"] ?? 0, y: b["Y"] ?? 0, width: b["Width"] ?? 0, height: b["Height"] ?? 0).contains(p)
            else { continue }
            return w[kCGWindowNumber as String] as? Int
        }
        return nil
    }

    private func flags(_ m: [String: Any]) -> CGEventFlags {
        var f = CGEventFlags()
        if m["shift"] as? Bool == true { f.insert(.maskShift) }
        if m["ctrl"] as? Bool == true { f.insert(.maskControl) }
        if m["alt"] as? Bool == true { f.insert(.maskAlternate) }
        if m["meta"] as? Bool == true { f.insert(.maskCommand) }
        return f
    }

    /// Browser `KeyboardEvent.code` to macOS virtual key codes.
    private let keyCodes: [String: CGKeyCode] = [
        "KeyA": 0x00, "KeyS": 0x01, "KeyD": 0x02, "KeyF": 0x03, "KeyH": 0x04, "KeyG": 0x05, "KeyZ": 0x06, "KeyX": 0x07,
        "KeyC": 0x08, "KeyV": 0x09, "KeyB": 0x0B, "KeyQ": 0x0C, "KeyW": 0x0D, "KeyE": 0x0E, "KeyR": 0x0F, "KeyY": 0x10,
        "KeyT": 0x11, "Digit1": 0x12, "Digit2": 0x13, "Digit3": 0x14, "Digit4": 0x15, "Digit6": 0x16, "Digit5": 0x17,
        "Equal": 0x18, "Digit9": 0x19, "Digit7": 0x1A, "Minus": 0x1B, "Digit8": 0x1C, "Digit0": 0x1D, "BracketRight": 0x1E,
        "KeyO": 0x1F, "KeyU": 0x20, "BracketLeft": 0x21, "KeyI": 0x22, "KeyP": 0x23, "Enter": 0x24, "KeyL": 0x25,
        "KeyJ": 0x26, "Quote": 0x27, "KeyK": 0x28, "Semicolon": 0x29, "Backslash": 0x2A, "Comma": 0x2B, "Slash": 0x2C,
        "KeyN": 0x2D, "KeyM": 0x2E, "Period": 0x2F, "Tab": 0x30, "Space": 0x31, "Backquote": 0x32, "Backspace": 0x33,
        "Escape": 0x35, "MetaRight": 0x36, "MetaLeft": 0x37, "ShiftLeft": 0x38, "CapsLock": 0x39, "AltLeft": 0x3A,
        "ControlLeft": 0x3B, "ShiftRight": 0x3C, "AltRight": 0x3D, "ControlRight": 0x3E, "NumpadEnter": 0x4C,
        "F5": 0x60, "F6": 0x61, "F7": 0x62, "F3": 0x63, "F8": 0x64, "F9": 0x65, "F11": 0x67, "F10": 0x6D, "F12": 0x6F,
        "Home": 0x73, "PageUp": 0x74, "Delete": 0x75, "F4": 0x76, "End": 0x77, "F2": 0x78, "PageDown": 0x79, "F1": 0x7A,
        "ArrowLeft": 0x7B, "ArrowRight": 0x7C, "ArrowDown": 0x7D, "ArrowUp": 0x7E,
    ]
}

// MARK: - App

final class App: NSObject, NSApplicationDelegate {
    let opts = Options.parse()
    var screen: Screen!
    let kicad = KiCad()
    let capture = Capture()
    var server: Server!
    let input = Input()

    func applicationDidFinishLaunching(_ n: Notification) {
        log("start \(CommandLine.arguments.dropFirst().joined(separator: " "))")
        let page = (try? String(contentsOf: Bundle.main.url(forResource: "viewer", withExtension: "html")!, encoding: .utf8)) ?? "no viewer page"
        server = Server(port: opts.port, page: page)
        server.onInput = { [weak self] m in self?.input.handle(m) }
        do {
            try server.start()
        } catch {
            log("cannot listen on \(opts.port): \(error)")
            NSApp.terminate(nil)
            return
        }
        var problems: [String] = []
        if !CGPreflightScreenCaptureAccess() {
            problems.append("KiCad View needs Screen Recording: allow it in System Settings > Privacy & Security, then restart it")
            CGRequestScreenCaptureAccess()
        }
        let prompt = kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String
        if !AXIsProcessTrustedWithOptions([prompt: true] as CFDictionary) {
            problems.append("KiCad View needs Accessibility to pass on clicks and keys: allow it in System Settings > Privacy & Security")
        }
        server.announce(["w": opts.width, "h": opts.height, "problems": problems])
        problems.forEach { log($0) }

        screen = Screen(width: opts.width, height: opts.height)
        waitForScreen(tries: 50)
    }

    /// A new screen reports a 1x1 size until its mode takes; KiCad has to
    /// be told the real one, so wait for it (up to five seconds).
    private func waitForScreen(tries: Int) {
        if screen.bounds.width >= CGFloat(opts.width) || tries == 0 {
            go()
        } else {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { self.waitForScreen(tries: tries - 1) }
        }
    }

    private func go() {
        input.origin = screen.bounds.origin
        input.mode = opts.input
        log("screen \(screen.id) at \(screen.bounds), display index \(screen.index)")
        do {
            try kicad.prepare(screen: screen)
            let board = opts.board ?? kicad.lastBoard
            if let b = opts.board { kicad.lastBoard = b }
            previous = NSWorkspace.shared.frontmostApplication
            input.pid = try kicad.launch(board: board)
            Timer.scheduledTimer(withTimeInterval: 0.3, repeats: true) { [weak self] _ in self?.guardFocus() }
            let bounds = screen.bounds
            Timer.scheduledTimer(withTimeInterval: 0.3, repeats: true) { [weak self] _ in self?.kicad.sweep(onto: bounds) }
            kicad.process?.terminationHandler = { _ in DispatchQueue.main.async { NSApp.terminate(nil) } }
            log("kicad pid \(input.pid)")
        } catch {
            log("kicad did not start: \(error)")
            server.announce(["problems": ["KiCad did not start: \(error.localizedDescription)"]])
        }
        capture.onFrame = { [weak self] f in self?.server.broadcast(f) }
        capture.onError = { [weak self] e in log(e); self?.server.announce(["problems": [e]]) }
        let (id, w, h, fps) = (screen.id, opts.width, opts.height, opts.fps)
        Task { [self] in
            do {
                try await capture.start(display: id, width: w, height: h, fps: fps)
                log("capturing")
            } catch {
                log("capture failed: \(error)")
                server.announce(["problems": ["cannot capture our screen: \(error.localizedDescription)"]])
            }
        }
    }

    /// The app the user was last in, other than our KiCad.
    private var previous: NSRunningApplication?

    /// Our KiCad must not stay the active app. The active app's screen is
    /// where macOS opens new windows, so with our KiCad focused, System
    /// Settings and permission prompts opened on our screen, out of the
    /// user's sight. KiCad takes focus when it starts, when it finishes
    /// loading and when it shows a dialog; each time, focus goes back to
    /// the user's app, unless the user is working in the page (an input
    /// event in the last few seconds).
    private func guardFocus() {
        guard let front = NSWorkspace.shared.frontmostApplication else { return }
        if front.processIdentifier != input.pid {
            if front.processIdentifier != ProcessInfo.processInfo.processIdentifier { previous = front }
            return
        }
        guard Date().timeIntervalSince(input.lastEvent) > 3, let back = previous, !back.isTerminated else { return }
        back.activate()
        log("gave focus back to \(back.localizedName ?? "the previous app")")
    }

    func applicationWillTerminate(_ n: Notification) {
        kicad.quit()
        log("stop")
    }
}

var signalSources: [DispatchSourceSignal] = []
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let delegate = App()
app.delegate = delegate
for sig in [SIGTERM, SIGINT] {
    signal(sig, SIG_IGN)
    let s = DispatchSource.makeSignalSource(signal: sig, queue: .main)
    s.setEventHandler { NSApp.terminate(nil) }
    s.resume()
    signalSources.append(s)
}
app.run()
