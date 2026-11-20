import Foundation
import IOKit.ps

/// Categories that come from kernel counters rather than the SMC.
/// Holds the previous sample where a rate has to be differenced.
public final class SystemSensors {
    private var previousCPUStates: [[UInt32]] = []
    private var previousNetwork: (received: UInt64, sent: UInt64, at: Date)?

    public init() {}

    // MARK: Memory

    public func memory() -> CategoryReading {
        guard let total = SystemSensors.sysctlUInt64("hw.memsize"), total > 0 else {
            return .empty(.memory)
        }
        var stats = vm_statistics64()
        var count = mach_msg_type_number_t(MemoryLayout<vm_statistics64>.size / MemoryLayout<integer_t>.size)
        let result = withUnsafeMutablePointer(to: &stats) { pointer -> kern_return_t in
            pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                host_statistics64(mach_host_self(), HOST_VM_INFO64, $0, &count)
            }
        }
        guard result == KERN_SUCCESS else { return .empty(.memory) }

        let page = UInt64(vm_kernel_page_size)
        let active = UInt64(stats.active_count) * page
        let wired = UInt64(stats.wire_count) * page
        let compressed = UInt64(stats.compressor_page_count) * page
        let free = UInt64(stats.free_count) * page
        let inactive = UInt64(stats.inactive_count) * page
        let used = active + wired + compressed
        let usedPercent = Double(used) / Double(total) * 100

        let sensors = [
            SensorReading(id: "memory.used.percent", key: "used", label: "Used", reading: Format.percent(usedPercent)),
            SensorReading(id: "memory.used.gb", key: "used", label: "Used (GB)", reading: Format.gigabytes(Format.gigabytesValue(used))),
            SensorReading(id: "memory.app", key: "active", label: "App", reading: Format.gigabytes(Format.gigabytesValue(active))),
            SensorReading(id: "memory.wired", key: "wired", label: "Wired", reading: Format.gigabytes(Format.gigabytesValue(wired))),
            SensorReading(id: "memory.compressed", key: "compressed", label: "Compressed", reading: Format.gigabytes(Format.gigabytesValue(compressed))),
            SensorReading(id: "memory.free", key: "free", label: "Free", reading: Format.gigabytes(Format.gigabytesValue(free + inactive))),
            SensorReading(id: "memory.total", key: "total", label: "Total", reading: Format.gigabytes(Format.gigabytesValue(total))),
        ]
        return CategoryReading(category: .memory, sensors: sensors, average: Format.percent(usedPercent))
    }

    // MARK: Processor

    public func processor() -> CategoryReading {
        var info: processor_info_array_t?
        var cpuCount: mach_msg_type_number_t = 0
        var infoCount: mach_msg_type_number_t = 0
        let result = host_processor_info(mach_host_self(), PROCESSOR_CPU_LOAD_INFO, &cpuCount, &info, &infoCount)
        guard result == KERN_SUCCESS, let info else { return .empty(.processor) }
        defer {
            vm_deallocate(mach_task_self_, vm_address_t(UInt(bitPattern: info)),
                          vm_size_t(Int(infoCount) * MemoryLayout<integer_t>.stride))
        }

        let statesPerCPU = Int(CPU_STATE_MAX)
        var current: [[UInt32]] = []
        current.reserveCapacity(Int(cpuCount))
        for cpu in 0..<Int(cpuCount) {
            let base = cpu * statesPerCPU
            current.append([
                UInt32(bitPattern: info[base + Int(CPU_STATE_USER)]),
                UInt32(bitPattern: info[base + Int(CPU_STATE_SYSTEM)]),
                UInt32(bitPattern: info[base + Int(CPU_STATE_IDLE)]),
                UInt32(bitPattern: info[base + Int(CPU_STATE_NICE)]),
            ])
        }

        var perCore: [Double] = []
        if previousCPUStates.count == current.count {
            for (index, now) in current.enumerated() {
                let before = previousCPUStates[index]
                var busy: Double = 0
                var totalTicks: Double = 0
                for lane in 0..<statesPerCPU {
                    let delta = Double(now[lane] &- before[lane])
                    totalTicks += delta
                    if lane != Int(CPU_STATE_IDLE) { busy += delta }
                }
                perCore.append(totalTicks > 0 ? busy / totalTicks * 100 : 0)
            }
        } else {
            perCore = Array(repeating: 0, count: current.count)
        }
        previousCPUStates = current

        let totalLoad = perCore.isEmpty ? 0 : perCore.reduce(0, +) / Double(perCore.count)
        var sensors: [SensorReading] = [
            SensorReading(id: "cpu.total", key: "total", label: "Total", reading: Format.percent(totalLoad))
        ]
        for (index, value) in perCore.enumerated() {
            sensors.append(SensorReading(id: "cpu.core.\(index)", key: "cpu\(index)",
                                         label: "Core \(index)", reading: Format.percent(value)))
        }
        return CategoryReading(category: .processor, sensors: sensors, average: Format.percent(totalLoad))
    }

    // MARK: System load average

    public func system() -> CategoryReading {
        var loads = [Double](repeating: 0, count: 3)
        guard getloadavg(&loads, 3) > 0 else { return .empty(.system) }
        let sensors = [
            SensorReading(id: "load.1", key: "1m", label: "1 minute", reading: Format.load(loads[0])),
            SensorReading(id: "load.5", key: "5m", label: "5 minutes", reading: Format.load(loads[1])),
            SensorReading(id: "load.15", key: "15m", label: "15 minutes", reading: Format.load(loads[2])),
        ]
        return CategoryReading(category: .system, sensors: sensors, average: Format.load(loads[0]))
    }

    // MARK: Network

    public func network() -> CategoryReading {
        guard let counters = SystemSensors.interfaceCounters() else { return .empty(.network) }
        let now = Date()
        var receivedRate: Double = 0
        var sentRate: Double = 0
        if let previous = previousNetwork {
            let elapsed = now.timeIntervalSince(previous.at)
            if elapsed > 0.05 {
                receivedRate = Double(counters.received &- min(counters.received, previous.received)) / elapsed
                sentRate = Double(counters.sent &- min(counters.sent, previous.sent)) / elapsed
            }
        }
        previousNetwork = (counters.received, counters.sent, now)

        let sensors = [
            SensorReading(id: "net.received", key: "rx", label: "Received", reading: Format.bytesPerSecond(receivedRate)),
            SensorReading(id: "net.sent", key: "tx", label: "Sent", reading: Format.bytesPerSecond(sentRate)),
        ]
        let total = ReadingValue(
            value: receivedRate + sentRate,
            text: "↓\(Format.scaledBytes(receivedRate))/s  ↑\(Format.scaledBytes(sentRate))/s",
            compact: "↓\(Format.scaledBytes(receivedRate)) ↑\(Format.scaledBytes(sentRate))"
        )
        return CategoryReading(category: .network, sensors: sensors, average: total)
    }

    // MARK: Storage

    public func storage() -> CategoryReading {
        let url = URL(fileURLWithPath: "/")
        guard let values = try? url.resourceValues(forKeys: [
            .volumeAvailableCapacityForImportantUsageKey,
            .volumeTotalCapacityKey,
        ]), let totalBytes = values.volumeTotalCapacity, totalBytes > 0 else {
            return .empty(.storage)
        }
        let availableBytes = UInt64(max(0, values.volumeAvailableCapacityForImportantUsage ?? 0))
        let usedBytes = UInt64(totalBytes) > availableBytes ? UInt64(totalBytes) - availableBytes : 0
        let usedPercent = Double(usedBytes) / Double(totalBytes) * 100

        let sensors = [
            SensorReading(id: "storage.available", key: "available", label: "Available",
                          reading: Format.gigabytes(Format.gigabytesValue(availableBytes))),
            SensorReading(id: "storage.used", key: "used", label: "Used",
                          reading: Format.gigabytes(Format.gigabytesValue(usedBytes))),
            SensorReading(id: "storage.total", key: "total", label: "Total",
                          reading: Format.gigabytes(Format.gigabytesValue(UInt64(totalBytes)))),
        ]
        return CategoryReading(category: .storage, sensors: sensors, average: Format.percent(usedPercent))
    }

    // MARK: Battery

    public func battery(smcValues: [String: Double]) -> CategoryReading {
        guard let snapshot = IOPSCopyPowerSourcesInfo()?.takeRetainedValue(),
              let sources = IOPSCopyPowerSourcesList(snapshot)?.takeRetainedValue() as? [CFTypeRef],
              let source = sources.first,
              let description = IOPSGetPowerSourceDescription(snapshot, source)?.takeUnretainedValue() as? [String: Any]
        else {
            return .empty(.battery)
        }

        let currentCapacity = description[kIOPSCurrentCapacityKey] as? Int ?? 0
        let maxCapacity = description[kIOPSMaxCapacityKey] as? Int ?? 100
        let charging = description[kIOPSIsChargingKey] as? Bool ?? false
        let onAC = (description[kIOPSPowerSourceStateKey] as? String) == kIOPSACPowerValue
        let chargePercent = maxCapacity > 0 ? Double(currentCapacity) / Double(maxCapacity) * 100 : Double(currentCapacity)

        let voltageVolts: Double? = smcValues["B0AV"].map { $0 / 1000.0 }
        // B0AC is the pack amperage in mA and is signed: negative while discharging.
        // IOPowerSources reports amps, so only use it when the SMC key is absent.
        let amperageAmps: Double?
        if let smcMilliamps = smcValues["B0AC"] {
            amperageAmps = smcMilliamps / 1000.0
        } else if let amps = (description[kIOPSCurrentKey] as? NSNumber)?.doubleValue {
            amperageAmps = amps
        } else {
            amperageAmps = nil
        }

        var sensors: [SensorReading] = [
            SensorReading(id: "battery.charge", key: "charge", label: "Charge", reading: Format.percent(chargePercent))
        ]
        if let voltageVolts, let amperageAmps {
            let watts = voltageVolts * amperageAmps
            sensors.append(SensorReading(id: "battery.power", key: "B0AV×B0AC", label: "Power", reading: Format.watts(watts)))
            sensors.append(SensorReading(id: "battery.voltage", key: "B0AV", label: "Voltage", reading: Format.voltage(voltageVolts)))
            sensors.append(SensorReading(id: "battery.current", key: "B0AC", label: "Current", reading: Format.amperes(amperageAmps)))
        }

        let stateText: String
        if charging {
            stateText = "Charging"
        } else if onAC {
            stateText = "AC Power"
        } else {
            stateText = "On Battery"
        }
        sensors.append(SensorReading(id: "battery.state", key: "state", label: "State",
                                     reading: ReadingValue(value: onAC ? 1 : 0, text: stateText, compact: stateText)))

        if !onAC {
            let minutes = description[kIOPSTimeToEmptyKey] as? Int ?? 0
            if minutes > 0 {
                let text = String(format: "%d:%02d", minutes / 60, minutes % 60)
                sensors.append(SensorReading(id: "battery.remaining", key: "time", label: "Remaining",
                                             reading: ReadingValue(value: Double(minutes), text: text, compact: text)))
            }
        }

        return CategoryReading(category: .battery, sensors: sensors, average: Format.percent(chargePercent))
    }

    // MARK: Helpers

    static func sysctlUInt64(_ name: String) -> UInt64? {
        var value: UInt64 = 0
        var size = MemoryLayout<UInt64>.size
        guard sysctlbyname(name, &value, &size, nil, 0) == 0 else { return nil }
        return value
    }

    /// Sum of the byte counters across every non-loopback interface.
    static func interfaceCounters() -> (received: UInt64, sent: UInt64)? {
        var head: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&head) == 0, let first = head else { return nil }
        defer { freeifaddrs(head) }

        var received: UInt64 = 0
        var sent: UInt64 = 0
        var cursor: UnsafeMutablePointer<ifaddrs>? = first
        while let entry = cursor {
            let interface = entry.pointee
            if let address = interface.ifa_addr, address.pointee.sa_family == UInt8(AF_LINK),
               let data = interface.ifa_data {
                let name = String(cString: interface.ifa_name)
                if !name.hasPrefix("lo") {
                    let stats = data.load(as: if_data.self)
                    received &+= UInt64(stats.ifi_ibytes)
                    sent &+= UInt64(stats.ifi_obytes)
                }
            }
            cursor = interface.ifa_next
        }
        return (received, sent)
    }
}
