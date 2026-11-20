import Foundation

/// Builds the SMC-backed categories out of raw key values.
public enum SMCSensors {
    public static func temperature(from values: [String: Double]) -> CategoryReading {
        let sensors: [SensorReading] = values
            .filter { key, value in
                key.hasPrefix("T") && value.isFinite && value > 1 && value < 130
            }
            .sorted { $0.key < $1.key }
            .map { key, value in
                SensorReading(id: key, key: key, label: label(for: key), reading: Format.temperature(value))
            }
        guard !sensors.isEmpty else { return .empty(.temperature) }
        let mean = sensors.reduce(0) { $0 + $1.reading.value } / Double(sensors.count)
        return CategoryReading(category: .temperature, sensors: sensors, average: Format.temperature(mean))
    }

    public static func voltage(from values: [String: Double]) -> CategoryReading {
        let sensors: [SensorReading] = values
            .filter { key, value in
                key.hasPrefix("V") && value.isFinite && value > 0.05 && value < 60
            }
            .sorted { $0.key < $1.key }
            .map { key, value in
                SensorReading(id: key, key: key, label: voltageLabel(for: key), reading: Format.voltage(value))
            }
        guard !sensors.isEmpty else { return .empty(.voltage) }
        let mean = sensors.reduce(0) { $0 + $1.reading.value } / Double(sensors.count)
        return CategoryReading(category: .voltage, sensors: sensors, average: Format.voltage(mean))
    }

    public static func fan(from values: [String: Double]) -> CategoryReading {
        let sensors: [SensorReading] = values
            .filter { key, value in isFanKey(key) && value.isFinite && value >= 0 && value < 12000 }
            .sorted { $0.key < $1.key }
            .map { key, value in
                let index = key.dropFirst().prefix(1)
                return SensorReading(id: key, key: key, label: "Fan \(index)", reading: Format.rpm(value))
            }
        guard !sensors.isEmpty else { return .empty(.fan) }
        let mean = sensors.reduce(0) { $0 + $1.reading.value } / Double(sensors.count)
        return CategoryReading(category: .fan, sensors: sensors, average: Format.rpm(mean))
    }

    private static func isFanKey(_ key: String) -> Bool {
        let chars = Array(key)
        return chars.count == 4 && chars[0] == "F" && chars[1].isNumber && chars[2] == "A" && chars[3] == "c"
    }

    /// Human labels for the thermal diodes. Keys we recognise get a name; the
    /// rest keep a generic label and are always shown with their raw key.
    static func label(for key: String) -> String {
        switch key {
        case "TB0T", "TB1T", "TB2T": return "Battery"
        case "TC0P", "TC0E", "TC0D": return "CPU Proximity"
        case "TCMb", "TCMz": return "SoC"
        case "TW0P": return "Wi-Fi"
        case "TPMP": return "Power Management"
        case "TS0P", "TS1P": return "SSD"
        default: break
        }
        let chars = Array(key)
        guard chars.count == 4, chars[0] == "T" else { return "Thermal sensor" }
        let cluster = chars[1]
        let member = String(chars[2...3])
        switch cluster {
        case "C":
            // TC1x = efficiency cluster, TC2x/TC3x = performance clusters.
            if chars[2] == "1" { return "CPU E-Core \(hexIndex(chars[3]).map { $0 + 1 } ?? 1)" }
            if chars[2] == "2" { return "CPU P-Core \(hexIndex(chars[3]).map { $0 + 1 } ?? 1)" }
            if chars[2] == "3" { return "CPU P-Core \(hexIndex(chars[3]).map { $0 + 5 } ?? 5)" }
            return "CPU Core"
        case "P":
            if chars[2] == "D" {
                guard let index = hexIndex(chars[3]) else { return "GPU Core" }
                return "GPU Core \(index)"
            }
            return "CPU Core"
        case "p": return "CPU P-Core"
        case "d": return "SoC Cluster"
        case "e", "g", "G": return "GPU Cluster"
        case "h", "m", "M": return "Memory"
        case "a": return "Ambient"
        case "s": return "Sensor"
        case "v", "V": return "Media Engine"
        case "z": return "Unused"
        default: return "Thermal sensor"
        }
    }

    private static func hexIndex(_ character: Character) -> Int? {
        guard let digit = character.hexDigitValue else { return nil }
        return digit
    }

    static func voltageLabel(for key: String) -> String {
        switch key {
        case "Vb0f", "Vb1f": return "DC-In"
        case "VDMA", "VDMM": return "DRAM"
        case "VMVC": return "Memory Controller"
        case "VSVR": return "SoC Rail"
        default: break
        }
        let chars = Array(key)
        guard chars.count == 4 else { return "Voltage rail" }
        switch chars[1] {
        case "P": return "Power Rail"
        case "R": return "SoC Rail"
        case "C": return "CPU Rail"
        case "D": return "Voltage Rail"
        case "S": return "Sensor Rail"
        default: return "Voltage rail"
        }
    }
}
