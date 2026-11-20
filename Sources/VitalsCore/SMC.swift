import CSMC
import Foundation

/// Turns SMC payload bytes into numbers.
///
/// The fixed-point types are the traditional source of wrong readings:
/// `sp78` is a *signed* 7.8 fixed point value in two big-endian bytes, and
/// `fpe2` is an *unsigned* 14.2 value, so the sign handling has to differ.
public enum SMCDecoder {
    public static func decode(type rawType: String, bytes: [UInt8], size: Int) -> Double? {
        let type = rawType.trimmingCharacters(in: .whitespaces)
        switch type {
        case "flt":
            guard size >= 4 else { return nil }
            let bits = UInt32(bytes[0]) | UInt32(bytes[1]) << 8 | UInt32(bytes[2]) << 16 | UInt32(bytes[3]) << 24
            return Double(Float(bitPattern: bits))
        case "ioft":
            guard size >= 8 else { return nil }
            var bits: UInt64 = 0
            for index in 0..<8 { bits |= UInt64(bytes[index]) << (8 * UInt64(index)) }
            return Double(bitPattern: bits)
        case "sp78":
            return signedFixed(bytes, size: size, divisor: 256)
        case "sp87":
            return signedFixed(bytes, size: size, divisor: 128)
        case "sp5a":
            return signedFixed(bytes, size: size, divisor: 1024)
        case "sp96":
            return signedFixed(bytes, size: size, divisor: 64)
        case "sp4b":
            return signedFixed(bytes, size: size, divisor: 2048)
        case "fpe2":
            guard size >= 2 else { return nil }
            return Double(UInt16(bytes[0]) << 8 | UInt16(bytes[1])) / 4
        case "fp1f":
            guard size >= 2 else { return nil }
            return Double(UInt16(bytes[0]) << 8 | UInt16(bytes[1])) / 32768
        case "ui8", "flag":
            return size >= 1 ? Double(bytes[0]) : nil
        case "ui16":
            guard size >= 2 else { return nil }
            return Double(UInt16(bytes[0]) << 8 | UInt16(bytes[1]))
        case "ui32":
            guard size >= 4 else { return nil }
            return Double(UInt32(bytes[0]) << 24 | UInt32(bytes[1]) << 16 | UInt32(bytes[2]) << 8 | UInt32(bytes[3]))
        case "si8":
            return size >= 1 ? Double(Int8(bitPattern: bytes[0])) : nil
        case "si16":
            guard size >= 2 else { return nil }
            return Double(Int16(bitPattern: UInt16(bytes[0]) << 8 | UInt16(bytes[1])))
        case "si32":
            guard size >= 4 else { return nil }
            return Double(Int32(bitPattern: UInt32(bytes[0]) << 24 | UInt32(bytes[1]) << 16 | UInt32(bytes[2]) << 8 | UInt32(bytes[3])))
        default:
            return nil
        }
    }

    private static func signedFixed(_ bytes: [UInt8], size: Int, divisor: Double) -> Double? {
        guard size >= 2 else { return nil }
        let raw = Int16(bitPattern: UInt16(bytes[0]) << 8 | UInt16(bytes[1]))
        return Double(raw) / divisor
    }
}

/// An open AppleSMC connection with the key list cached.
///
/// Enumerating keys and reading every key's type costs one round trip each,
/// so it happens once at startup; a refresh only re-reads values.
public final class SMCService {
    public struct Spec {
        public let key: String
        public let type: String
        public let size: Int
    }

    public private(set) var isAvailable = false
    public private(set) var totalKeyCount = 0
    public private(set) var specs: [Spec] = []

    /// Keys whose values are worth polling every refresh.
    private static func isInteresting(_ key: String) -> Bool {
        let chars = Array(key)
        guard chars.count == 4 else { return false }
        if chars[0] == "T" || chars[0] == "V" { return true }
        if chars[0] == "F", chars[2] == "A", chars[3] == "c", chars[1].isNumber { return true }
        if key == "B0AV" || key == "B0AC" || key == "B0FC" || key == "B0RM" { return true }
        return false
    }

    public init() {
        isAvailable = smc_open() == 0
        guard isAvailable else { return }
        loadKeyList()
    }

    deinit {
        if isAvailable { smc_close() }
    }

    private func loadKeyList() {
        var count: UInt32 = 0
        guard smc_key_count(&count) == 0, count > 0 else { return }
        totalKeyCount = Int(count)
        var discovered: [Spec] = []
        for index in 0..<count {
            var buffer = [CChar](repeating: 0, count: 5)
            guard smc_key_at(index, &buffer) == 0 else { continue }
            let key = String(cString: buffer)
            guard SMCService.isInteresting(key) else { continue }
            var size: UInt32 = 0
            var type = [CChar](repeating: 0, count: 5)
            guard smc_read_keyinfo(key, &size, &type) == 0, size > 0, size <= 32 else { continue }
            discovered.append(Spec(key: key, type: String(cString: type), size: Int(size)))
        }
        specs = discovered
    }

    /// Re-reads every cached key. Returns key -> value for the keys that answered.
    public func readValues() -> [String: Double] {
        guard isAvailable else { return [:] }
        var values: [String: Double] = [:]
        values.reserveCapacity(specs.count)
        for spec in specs {
            var bytes = [UInt8](repeating: 0, count: 32)
            guard smc_read_bytes(spec.key, UInt32(spec.size), &bytes) == 0 else { continue }
            if let value = SMCDecoder.decode(type: spec.type, bytes: bytes, size: spec.size) {
                values[spec.key] = value
            }
        }
        return values
    }
}
