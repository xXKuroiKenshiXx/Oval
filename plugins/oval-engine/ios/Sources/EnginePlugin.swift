import AVFoundation
import Foundation
import JavaScriptCore
import Tauri
import UIKit

// Runs yt-dlp for Oval's Rust code on iOS. iOS doesn't let apps start other
// programs, so yt-dlp runs inside the app on an embedded Python (the
// Python.framework built into the app, see packaging/ios). The Python side
// lives in engine_plugins/ios/oval_ios.py; it calls back into the three
// native helpers below for output lines, merging and JavaScript.

class PrepareArgs: Decodable {
  let driver: String?
  let home: String?
}

class RunArgs: Decodable {
  let id: Int64
  let args: [String]
  let channel: Channel
}

class IdArgs: Decodable {
  let id: Int64
}

struct Event: Encodable {
  var out: String?
  var err: String?
  var exit: Int32?
}

// MARK: - Python

/// The handful of CPython functions Oval needs, looked up at run time so the
/// plugin builds without Python's headers.
enum Python {
  private typealias Initialize = @convention(c) () -> Void
  private typealias SaveThread = @convention(c) () -> OpaquePointer?
  private typealias Ensure = @convention(c) () -> Int32
  private typealias Release = @convention(c) (Int32) -> Void
  private typealias RunString = @convention(c) (UnsafePointer<CChar>) -> Int32

  private static var handle: UnsafeMutableRawPointer?

  private static func symbol<T>(_ name: String, as type: T.Type) -> T? {
    guard let handle = handle, let sym = dlsym(handle, name) else { return nil }
    return unsafeBitCast(sym, to: type)
  }

  /// Starts the interpreter once, with the standard library and yt-dlp that
  /// are inside the app bundle.
  static func start() throws {
    if handle != nil { return }
    let bundle = Bundle.main
    let framework = bundle.privateFrameworksPath.map { "\($0)/Python.framework/Python" } ?? ""
    guard let h = dlopen(framework, RTLD_NOW | RTLD_GLOBAL) else {
      throw EngineError("No se encontró Python dentro de la app")
    }
    handle = h
    let resources = bundle.resourcePath ?? ""
    setenv("PYTHONHOME", "\(resources)/python", 1)
    setenv("PYTHONPATH", "\(resources)/app_packages", 1)
    setenv("PYTHONUTF8", "1", 1)
    setenv("PYTHONDONTWRITEBYTECODE", "1", 1)
    setenv("PYTHONNOUSERSITE", "1", 1)
    // Python's OpenSSL has no system certificates on iOS.
    setenv("SSL_CERT_FILE", "\(resources)/app_packages/certifi/cacert.pem", 1)
    guard let initialize = symbol("Py_Initialize", as: Initialize.self),
      let save = symbol("PyEval_SaveThread", as: SaveThread.self)
    else { throw EngineError("Python incompleto") }
    initialize()
    // Release the GIL; every call takes it again with `run(_:)`.
    _ = save()
  }

  /// Runs Python source on the current thread; true if it raised nothing.
  @discardableResult
  static func run(_ code: String) -> Bool {
    guard let ensure = symbol("PyGILState_Ensure", as: Ensure.self),
      let release = symbol("PyGILState_Release", as: Release.self),
      let exec = symbol("PyRun_SimpleString", as: RunString.self)
    else { return false }
    let state = ensure()
    let ok = code.withCString { exec($0) } == 0
    release(state)
    return ok
  }

  /// A Python string literal that survives any content.
  static func literal(_ text: String) -> String {
    "__import__('base64').b64decode('\(Data(text.utf8).base64EncodedString())').decode('utf-8')"
  }
}

struct EngineError: Error, CustomStringConvertible {
  let description: String
  init(_ description: String) { self.description = description }
}

// MARK: - Native helpers called from Python

/// Running jobs and their output channels.
enum Jobs {
  static let lock = NSLock()
  static var channels: [Int64: Channel] = [:]
  static var setupReply: String?
  static var background: UIBackgroundTaskIdentifier = .invalid

  static func send(_ id: Int64, _ event: Event) {
    lock.lock()
    let channel = channels[id]
    if event.exit != nil { channels[id] = nil }
    let idle = channels.isEmpty
    lock.unlock()
    try? channel?.send(event)
    if idle { endBackground() }
  }

  /// iOS suspends apps soon after they leave the screen; ask for the extra
  /// minutes it allows so running downloads can finish.
  static func beginBackground() {
    DispatchQueue.main.async {
      if background != .invalid { return }
      background = UIApplication.shared.beginBackgroundTask(withName: "Oval") { endBackground() }
    }
  }

  static func endBackground() {
    DispatchQueue.main.async {
      if background == .invalid { return }
      UIApplication.shared.endBackgroundTask(background)
      background = .invalid
    }
  }
}

/// emit(job, stream, line): stream 1 = stdout, 2 = stderr, 0 = exit code
/// (or, for job 0, the reply to setup/update).
private let emitLine: @convention(c) (Int64, Int32, UnsafePointer<CChar>?) -> Void = { id, stream, line in
  let text = line.map { String(cString: $0) } ?? ""
  switch stream {
  case 0 where id == 0:
    Jobs.lock.lock()
    Jobs.setupReply = text
    Jobs.lock.unlock()
  case 0:
    Jobs.send(id, Event(exit: Int32(text) ?? 1))
  case 1:
    Jobs.send(id, Event(out: text))
  default:
    if id == 0 { NSLog("Oval engine: %@", text) } else { Jobs.send(id, Event(err: text)) }
  }
}

/// merge(video, audio, out): joins an MP4 video and an M4A audio without
/// re-encoding. 0 on success.
private let mergeFiles: @convention(c) (UnsafePointer<CChar>?, UnsafePointer<CChar>?, UnsafePointer<CChar>?) -> Int32 = { v, a, o in
  guard let v = v, let a = a, let o = o else { return 1 }
  return Media.merge(
    video: URL(fileURLWithPath: String(cString: v)),
    audio: URL(fileURLWithPath: String(cString: a)),
    out: URL(fileURLWithPath: String(cString: o)))
}

/// run_js(script): evaluates JavaScript (YouTube's player challenges) and
/// returns a malloc'd JSON envelope that Python hands back to `releaseText`.
private let runJS: @convention(c) (UnsafePointer<CChar>?) -> UnsafeMutableRawPointer? = { script in
  let answer = Script.run(script.map { String(cString: $0) } ?? "")
  return UnsafeMutableRawPointer(strdup(answer))
}

private let releaseText: @convention(c) (UnsafeMutableRawPointer?) -> Void = { ptr in
  free(ptr)
}

private func address<T>(_ fn: T) -> Int {
  unsafeBitCast(fn, to: Int.self)
}

enum Media {
  static func merge(video: URL, audio: URL, out: URL) -> Int32 {
    let v = AVURLAsset(url: video)
    let a = AVURLAsset(url: audio)
    let composition = AVMutableComposition()
    guard let vt = v.tracks(withMediaType: .video).first,
      let at = a.tracks(withMediaType: .audio).first,
      let cv = composition.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid),
      let ca = composition.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid)
    else { return 1 }
    do {
      try cv.insertTimeRange(CMTimeRange(start: .zero, duration: v.duration), of: vt, at: .zero)
      let length = CMTimeMinimum(a.duration, v.duration)
      try ca.insertTimeRange(CMTimeRange(start: .zero, duration: length), of: at, at: .zero)
    } catch {
      return 1
    }
    cv.preferredTransform = vt.preferredTransform
    guard let export = AVAssetExportSession(asset: composition, presetName: AVAssetExportPresetPassthrough) else {
      return 1
    }
    try? FileManager.default.removeItem(at: out)
    export.outputURL = out
    export.outputFileType = .mp4
    export.shouldOptimizeForNetworkUse = true
    let done = DispatchSemaphore(value: 0)
    export.exportAsynchronously { done.signal() }
    done.wait()
    if export.status != .completed {
      NSLog("Oval merge failed: %@", export.error?.localizedDescription ?? "?")
      return 1
    }
    return 0
  }
}

enum Script {
  /// Same contract as a JavaScript runtime started by yt-dlp: whatever the
  /// script prints with console.log is the answer.
  static func run(_ source: String) -> String {
    guard let context = JSContext() else { return envelope(["error": "JavaScriptCore no disponible"]) }
    var failure: String?
    context.exceptionHandler = { _, exception in
      failure = exception?.toString() ?? "error"
    }
    context.evaluateScript("""
      var __out = [];
      var __log = function () { __out.push(Array.prototype.join.call(arguments, ' ')); };
      var console = { log: __log, info: function () {}, warn: function () {}, error: function () {}, debug: function () {} };
      """)
    context.evaluateScript(source)
    if let failure = failure { return envelope(["error": failure]) }
    let out = context.evaluateScript("__out.join('\\n')")?.toString() ?? ""
    return envelope(["out": out])
  }

  private static func envelope(_ value: [String: String]) -> String {
    let data = (try? JSONSerialization.data(withJSONObject: value)) ?? Data("{\"error\":\"json\"}".utf8)
    return String(decoding: data, as: UTF8.self)
  }
}

// MARK: - Plugin

class EnginePlugin: Plugin {
  private let queue = DispatchQueue(label: "oval.engine.setup")
  private var version: String?

  /// Python needs deep stacks; secondary threads get 512 KB by default.
  private func thread(_ body: @escaping () -> Void) {
    let t = Thread(block: body)
    t.stackSize = 16 << 20
    t.start()
  }

  @objc public func prepare(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(PrepareArgs.self)
    thread { [self] in
      queue.sync {
        if let version = version {
          invoke.resolve(["version": version])
          return
        }
        do {
          guard let driver = args.driver, let home = args.home else {
            throw EngineError("Falta el motor de Oval")
          }
          try Python.start()
          let ok = Python.run("""
            import sys
            sys.path.insert(0, \(Python.literal(driver)))
            import oval_ios
            oval_ios._native_reply(oval_ios.setup(\(address(emitLine)), \(address(mergeFiles)), \(address(runJS)), \(address(releaseText)), \(Python.literal(home))))
            """)
          Jobs.lock.lock()
          let reply = Jobs.setupReply
          Jobs.lock.unlock()
          guard ok, let v = reply else { throw EngineError("No se pudo iniciar yt-dlp") }
          version = v
          invoke.resolve(["version": v])
        } catch {
          invoke.reject(String(describing: error))
        }
      }
    }
  }

  @objc public func update(_ invoke: Invoke) throws {
    thread {
      let ok = Python.run("import oval_ios\noval_ios._native_reply(oval_ios.update())")
      if ok { invoke.resolve() } else { invoke.reject("No se pudo actualizar yt-dlp") }
    }
  }

  /// Starts yt-dlp and streams every output line back over the channel.
  @objc public func run(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(RunArgs.self)
    let json = (try? JSONSerialization.data(withJSONObject: args.args)).map { String(decoding: $0, as: UTF8.self) } ?? "[]"
    Jobs.lock.lock()
    Jobs.channels[args.id] = args.channel
    Jobs.lock.unlock()
    Jobs.beginBackground()
    invoke.resolve()
    thread {
      let ok = Python.run("import oval_ios\noval_ios.run_and_report(\(args.id), \(Python.literal(json)))")
      if !ok {
        Jobs.send(args.id, Event(err: "ERROR: el motor de Oval falló"))
        Jobs.send(args.id, Event(exit: 1))
      }
    }
  }

  @objc public func kill(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(IdArgs.self)
    thread { Python.run("import oval_ios\noval_ios.kill(\(args.id))") }
    invoke.resolve()
  }
}

@_cdecl("init_plugin_oval_engine")
func initPlugin() -> Plugin {
  return EnginePlugin()
}
