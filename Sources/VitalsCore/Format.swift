import Foundation

public enum Format {
    public static func temperature(_ celsius: Double) -> ReadingValue {
        ReadingValue(value: celsius,
                     text: String(format: "%.1f °C", celsius),
                     compact: String(format: "%.0f°", celsius))
    }

    public static func voltage(_ volts: Double) -> ReadingValue {
        let text = volts < 10 ? String(format: "%.3f V", volts) : String(format: "%.2f V", volts)
        return ReadingValue(value: volts, text: text, compact: String(format: "%.2f V", volts))
    }

    public static func rpm(_ value: Double) -> ReadingValue {
        ReadingValue(value: value,
                     text: String(format: "%.0f RPM", value),
                     compact: String(format: "%.0f", value))
    }

    public static func percent(_ value: Double) -> ReadingValue {
        ReadingValue(value: value,
                     text: String(format: "%.1f%%", value),
                     compact: String(format: "%.0f%%", value))
    }

    public static func gigabytes(_ value: Double) -> ReadingValue {
        ReadingValue(value: value,
                     text: String(format: "%.1f GB", value),
                     compact: String(format: "%.1fG", value))
    }

    public static func load(_ value: Double) -> ReadingValue {
        ReadingValue(value: value,
                     text: String(format: "%.2f", value),
                     compact: String(format: "%.2f", value))
    }

    public static func watts(_ value: Double) -> ReadingValue {
        ReadingValue(value: value,
                     text: String(format: "%.1f W", value),
                     compact: String(format: "%.1fW", value))
    }

    public static func amperes(_ value: Double) -> ReadingValue {
        ReadingValue(value: value,
                     text: String(format: "%.2f A", value),
                     compact: String(format: "%.2fA", value))
    }

    public static func bytesPerSecond(_ value: Double) -> ReadingValue {
        ReadingValue(value: value,
                     text: "\(scaledBytes(value))/s",
                     compact: "\(scaledBytes(value))/s")
    }

    /// Compact byte count, e.g. 0 B, 812 B, 12.4 KB, 3.1 MB.
    public static func scaledBytes(_ value: Double) -> String {
        let units = ["B", "KB", "MB", "GB", "TB"]
        var amount = max(0, value)
        var index = 0
        while amount >= 1024 && index < units.count - 1 {
            amount /= 1024
            index += 1
        }
        if index == 0 {
            return String(format: "%.0f B", amount)
        }
        return String(format: amount < 10 ? "%.2f %@" : "%.1f %@", amount, units[index])
    }

    public static func gigabytesValue(_ bytes: UInt64) -> Double {
        Double(bytes) / 1_073_741_824.0
    }
}
