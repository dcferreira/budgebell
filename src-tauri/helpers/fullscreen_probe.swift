// fullscreen_probe — a tiny, fully-local "is a full-screen app in front?" probe.
//
// It prints, as one JSON object, the raw facts the app needs to decide whether
// the frontmost application is full screen; the decision itself lives in Rust
// (`quiet_os::fullscreen::parse_macos_fullscreen`) where it is unit-tested:
//
//   {"frontmost_pid": 123 | null,
//    "displays": [{"x", "y", "width", "height", "top_inset"}],
//    "windows":  [{"pid", "layer", "x", "y", "width", "height"}]}
//
// Display and window bounds are in the window server's global, top-left-origin
// coordinates (CGDisplayBounds / kCGWindowBounds). `top_inset` is the display's
// camera-housing (notch) inset, where macOS places native full-screen windows
// by default. Only on-screen windows are listed, so on a full-screen Space the
// full-screen window is there and the hidden desktop's windows are not.
//
// Window owners, layers and bounds need NO Screen Recording permission (only
// window titles do), so this triggers no TCC prompt. It performs NO network
// I/O. On failure it writes to stderr and exits non-zero rather than printing
// a misleading answer. The binary is ad-hoc-signed by the build.

import AppKit
import CoreGraphics
import Foundation

func failLoudly(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(2)
}

func number(_ value: CGFloat) -> NSNumber {
    NSNumber(value: Double(value))
}

let frontmostPid: Any = NSWorkspace.shared.frontmostApplication.map {
    NSNumber(value: $0.processIdentifier)
} ?? NSNull()

// Notch insets, keyed by display id.
var topInsets: [CGDirectDisplayID: CGFloat] = [:]
for screen in NSScreen.screens {
    guard
        let screenNumber = screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")]
            as? NSNumber
    else { continue }
    var inset: CGFloat = 0
    if #available(macOS 12.0, *) {
        inset = screen.safeAreaInsets.top
    }
    topInsets[CGDirectDisplayID(screenNumber.uint32Value)] = inset
}

var displayCount: UInt32 = 0
var displayStatus = CGGetActiveDisplayList(0, nil, &displayCount)
if displayStatus != .success {
    failLoudly("could not count the active displays: CGError \(displayStatus.rawValue)")
}
var displayIDs = [CGDirectDisplayID](repeating: 0, count: Int(displayCount))
displayStatus = CGGetActiveDisplayList(displayCount, &displayIDs, &displayCount)
if displayStatus != .success {
    failLoudly("could not list the active displays: CGError \(displayStatus.rawValue)")
}

let displays: [[String: Any]] = displayIDs.prefix(Int(displayCount)).map { id in
    let bounds = CGDisplayBounds(id)
    return [
        "x": number(bounds.origin.x),
        "y": number(bounds.origin.y),
        "width": number(bounds.width),
        "height": number(bounds.height),
        "top_inset": number(topInsets[id] ?? 0),
    ]
}

guard
    let windowInfo = CGWindowListCopyWindowInfo(
        [.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID
    ) as? [[String: Any]]
else {
    failLoudly("could not list the on-screen windows")
}

var windows: [[String: Any]] = []
for info in windowInfo {
    guard
        let pid = info[kCGWindowOwnerPID as String] as? NSNumber,
        let layer = info[kCGWindowLayer as String] as? NSNumber,
        let boundsDict = info[kCGWindowBounds as String] as? NSDictionary,
        let bounds = CGRect(dictionaryRepresentation: boundsDict as CFDictionary)
    else { continue }
    windows.append([
        "pid": pid,
        "layer": layer,
        "x": number(bounds.origin.x),
        "y": number(bounds.origin.y),
        "width": number(bounds.width),
        "height": number(bounds.height),
    ])
}

let payload: [String: Any] = [
    "frontmost_pid": frontmostPid,
    "displays": displays,
    "windows": windows,
]
guard let json = try? JSONSerialization.data(withJSONObject: payload) else {
    failLoudly("could not encode the probe output as JSON")
}
FileHandle.standardOutput.write(json)
