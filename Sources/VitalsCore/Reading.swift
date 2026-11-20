import Foundation

/// One number ready for display: the full text for a menu row and the
/// terse text for a menu bar slot.
public struct ReadingValue: Equatable {
    public let value: Double
    public let text: String
    public let compact: String

    public init(value: Double, text: String, compact: String) {
        self.value = value
        self.text = text
        self.compact = compact
    }

    /// Marker for a category that found nothing usable on this hardware.
    public static let noData = ReadingValue(value: .nan, text: "No data", compact: "—")

    public var hasData: Bool { !value.isNaN }
}

/// A single sensor a category discovered: either an SMC key or one of the
/// system counters.
public struct SensorReading: Identifiable {
    public let id: String
    public let key: String
    public let label: String
    public let reading: ReadingValue

    public init(id: String, key: String, label: String, reading: ReadingValue) {
        self.id = id
        self.key = key
        self.label = label
        self.reading = reading
    }
}

public struct CategoryReading {
    public let category: CategoryID
    public let sensors: [SensorReading]
    public let average: ReadingValue

    public init(category: CategoryID, sensors: [SensorReading], average: ReadingValue) {
        self.category = category
        self.sensors = sensors
        self.average = average
    }

    public func reading(for selection: String?) -> ReadingValue {
        guard let selection, let match = sensors.first(where: { $0.id == selection }) else {
            return average
        }
        return match.reading
    }

    public static func empty(_ category: CategoryID) -> CategoryReading {
        CategoryReading(category: category, sensors: [], average: .noData)
    }
}
