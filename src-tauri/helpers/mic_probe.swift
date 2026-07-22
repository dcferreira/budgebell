// mic_probe — a tiny, fully-local CoreAudio input-device probe (design spec §4.5).
//
// It asks the CoreAudio HAL whether the *default input device* is currently
// running (capturing) anywhere on the system, via the device property
// `kAudioDevicePropertyDeviceIsRunningSomewhere`. This is a strong proxy for
// an ongoing call — including ad-hoc, off-calendar ones the calendar probe
// cannot see.
//
// Crucially this reads a device *property*; it never opens an audio stream or
// touches sample data, so it requires NO microphone TCC permission and
// triggers NO permission prompt. It performs NO network I/O.
//
// Output (stdout): "1" if the default input device is capturing, "0" if idle.
// On any CoreAudio failure it fails loudly: writes to stderr and exits
// non-zero rather than emitting a misleading "0". The binary is ad-hoc-signed
// by the build for consistency with the calendar helper.

import CoreAudio
import Foundation

func failLoudly(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(2)
}

// Resolve the system's default input device.
var deviceID = AudioDeviceID(kAudioObjectUnknown)
var deviceSize = UInt32(MemoryLayout<AudioDeviceID>.size)
var defaultInputAddress = AudioObjectPropertyAddress(
    mSelector: kAudioHardwarePropertyDefaultInputDevice,
    mScope: kAudioObjectPropertyScopeGlobal,
    mElement: kAudioObjectPropertyElementMain
)

let deviceStatus = AudioObjectGetPropertyData(
    AudioObjectID(kAudioObjectSystemObject),
    &defaultInputAddress,
    0,
    nil,
    &deviceSize,
    &deviceID
)
if deviceStatus != noErr {
    failLoudly("could not read the default input device: OSStatus \(deviceStatus)")
}
if deviceID == kAudioObjectUnknown {
    failLoudly("there is no default input device")
}

// Ask whether that device is running (capturing) anywhere on the system.
var isRunning = UInt32(0)
var runningSize = UInt32(MemoryLayout<UInt32>.size)
var runningAddress = AudioObjectPropertyAddress(
    mSelector: kAudioDevicePropertyDeviceIsRunningSomewhere,
    mScope: kAudioObjectPropertyScopeGlobal,
    mElement: kAudioObjectPropertyElementMain
)

let runningStatus = AudioObjectGetPropertyData(
    deviceID,
    &runningAddress,
    0,
    nil,
    &runningSize,
    &isRunning
)
if runningStatus != noErr {
    failLoudly("could not read the device-running state: OSStatus \(runningStatus)")
}

FileHandle.standardOutput.write(Data((isRunning != 0 ? "1" : "0").utf8))
