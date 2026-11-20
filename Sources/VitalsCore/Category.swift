import Foundation

/// A category is one row in the dropdown and one optional slot in the menu bar.
public enum CategoryID: String, CaseIterable {
    case temperature
    case voltage
    case fan
    case memory
    case processor
    case system
    case network
    case storage
    case battery

    public var title: String {
        switch self {
        case .temperature: return "Temperature"
        case .voltage: return "Voltage"
        case .fan: return "Fan"
        case .memory: return "Memory"
        case .processor: return "Processor"
        case .system: return "System"
        case .network: return "Network"
        case .storage: return "Storage"
        case .battery: return "Battery"
        }
    }

    /// SF Symbols names, most specific first; the first one the OS knows wins.
    public var symbolCandidates: [String] {
        switch self {
        case .temperature: return ["thermometer.medium", "thermometer"]
        case .voltage: return ["bolt.fill", "bolt"]
        case .fan: return ["fanblades.fill", "fanblades", "wind"]
        case .memory: return ["memorychip.fill", "memorychip"]
        case .processor: return ["cpu.fill", "cpu"]
        case .system: return ["gauge.with.dots.needle.50percent", "gauge.medium", "chart.line.uptrend.xyaxis"]
        case .network: return ["network", "globe"]
        case .storage: return ["internaldrive.fill", "internaldrive"]
        case .battery: return ["battery.100", "battery.75", "bolt.batteryblock"]
        }
    }
}
