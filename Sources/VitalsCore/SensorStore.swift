import Foundation

/// Reads every category into an immutable snapshot.
///
/// `refresh()` touches the SMC user client, so callers must funnel it through
/// one serial queue; `current()` is safe from any thread.
public final class SensorStore {
    private let smc: SMCService
    private let system = SystemSensors()
    private let lock = NSLock()
    private var snapshot: [CategoryID: CategoryReading] = [:]

    public init() {
        smc = SMCService()
        // Seed the delta-based samplers so the first refresh already carries a
        // real CPU load and network rate instead of a zero baseline.
        _ = system.processor()
        _ = system.network()
    }

    public var smcAvailable: Bool { smc.isAvailable }
    public var smcKeyCount: Int { smc.totalKeyCount }
    public var smcSensorKeyCount: Int { smc.specs.count }

    public func refresh() {
        let smcValues = smc.isAvailable ? smc.readValues() : [:]
        var fresh: [CategoryID: CategoryReading] = [:]
        fresh[.temperature] = SMCSensors.temperature(from: smcValues)
        fresh[.voltage] = SMCSensors.voltage(from: smcValues)
        fresh[.fan] = SMCSensors.fan(from: smcValues)
        fresh[.memory] = system.memory()
        fresh[.processor] = system.processor()
        fresh[.system] = system.system()
        fresh[.network] = system.network()
        fresh[.storage] = system.storage()
        fresh[.battery] = system.battery(smcValues: smcValues)

        lock.lock()
        snapshot = fresh
        lock.unlock()
    }

    public func current() -> [CategoryID: CategoryReading] {
        lock.lock()
        defer { lock.unlock() }
        return snapshot
    }

    public func reading(_ category: CategoryID) -> CategoryReading {
        current()[category] ?? .empty(category)
    }
}
