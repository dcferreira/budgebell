// meeting_probe — a tiny, fully-local EventKit reader (design spec §8).
//
// It reads the local Calendar store, finds events overlapping a window, and
// prints them as JSON so the Rust core can classify a "real meeting" by
// attendee count (the "meeting now" probe) or list a day's events for the
// Stats window. It performs NO network I/O — EventKit reads the on-device
// store only. Requires macOS calendar TCC permission; the binary is
// ad-hoc-signed by the build so the permission grant sticks.
//
// Window selection:
//   • no arguments      → a ±6h window around "now" (the meeting-now probe)
//   • <start> <end> args → the given local wall-clock range, each formatted
//     "yyyy-MM-dd'T'HH:mm:ss" (the Stats window's day listing)
//
// Output (stdout): a JSON array of
//   { "title": String, "start": "yyyy-MM-dd'T'HH:mm:ss",
//     "end": "yyyy-MM-dd'T'HH:mm:ss", "otherAttendeeCount": Int }
// with times in the machine's local timezone so the Rust side can compare
// against its own local "now". On denied/failed access it fails loudly:
// writes to stderr and exits non-zero rather than emitting an empty array.

import EventKit
import Foundation

struct OutEvent: Codable {
    let title: String
    let start: String
    let end: String
    let otherAttendeeCount: Int
}

func failLoudly(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(2)
}

let store = EKEventStore()
let semaphore = DispatchSemaphore(value: 0)
var accessGranted = false

let handler: EKEventStoreRequestAccessCompletionHandler = { granted, _ in
    accessGranted = granted
    semaphore.signal()
}

if #available(macOS 14.0, *) {
    store.requestFullAccessToEvents(completion: handler)
} else {
    store.requestAccess(to: .event, completion: handler)
}
semaphore.wait()

if !accessGranted {
    failLoudly("calendar access was not granted")
}

let formatter = DateFormatter()
formatter.locale = Locale(identifier: "en_US_POSIX")
formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ss"
// No timezone set — the machine's local zone is used, so a local wall-clock
// string maps to the correct absolute instant and event dates print back as
// local wall-clock times for the Rust side to read.

// A ±6h window around "now" by default (the meeting-now probe); an explicit
// local wall-clock range when the Stats window asks for a specific day.
let now = Date()
let arguments = CommandLine.arguments
let windowStart: Date
let windowEnd: Date
if arguments.count >= 3 {
    guard let start = formatter.date(from: arguments[1]),
          let end = formatter.date(from: arguments[2])
    else {
        failLoudly("could not parse the day range arguments: \(arguments[1]) \(arguments[2])")
    }
    windowStart = start
    windowEnd = end
} else {
    windowStart = now.addingTimeInterval(-6 * 3600)
    windowEnd = now.addingTimeInterval(6 * 3600)
}
let predicate = store.predicateForEvents(withStart: windowStart, end: windowEnd, calendars: nil)

let events = store.events(matching: predicate).map { event -> OutEvent in
    // "Other" attendees are everyone on the event who is not the current
    // user — this is what distinguishes a real meeting from a solo focus
    // block (design spec §8). An event with no attendee list is solo.
    let others = (event.attendees ?? []).filter { !$0.isCurrentUser }.count
    return OutEvent(
        title: event.title ?? "",
        start: formatter.string(from: event.startDate),
        end: formatter.string(from: event.endDate),
        otherAttendeeCount: others
    )
}

do {
    let encoded = try JSONEncoder().encode(events)
    FileHandle.standardOutput.write(encoded)
} catch {
    failLoudly("failed to encode calendar events: \(error)")
}
