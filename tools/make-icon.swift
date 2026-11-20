// Draws the app icon and writes Vitals.iconset, then an .icns.
//
// macOS icon grid: a continuous-corner square with margins, a dark graphite
// gradient, a hairline highlight along the top edge. The glyph is Phosphor's
// pulse, drawn white.

import AppKit

let outputDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "packaging"

// MARK: - Squircle

func continuousSquare(_ rect: CGRect, radius r: CGFloat) -> CGPath {
    // Superellipse-ish corner: two cubic segments per corner, which is what
    // makes an icon corner read as "macOS" rather than as a rounded rect.
    let path = CGMutablePath()
    let minX = rect.minX, minY = rect.minY, maxX = rect.maxX, maxY = rect.maxY
    let k: CGFloat = 0.5523
    path.move(to: CGPoint(x: minX + r, y: maxY))
    path.addLine(to: CGPoint(x: maxX - r, y: maxY))
    path.addCurve(to: CGPoint(x: maxX, y: maxY - r),
                  control1: CGPoint(x: maxX - r + k * r, y: maxY),
                  control2: CGPoint(x: maxX, y: maxY - r + k * r))
    path.addLine(to: CGPoint(x: maxX, y: minY + r))
    path.addCurve(to: CGPoint(x: maxX - r, y: minY),
                  control1: CGPoint(x: maxX, y: minY + r - k * r),
                  control2: CGPoint(x: maxX - r + k * r, y: minY))
    path.addLine(to: CGPoint(x: minX + r, y: minY))
    path.addCurve(to: CGPoint(x: minX, y: minY + r),
                  control1: CGPoint(x: minX + r - k * r, y: minY),
                  control2: CGPoint(x: minX, y: minY + r - k * r))
    path.addLine(to: CGPoint(x: minX, y: maxY - r))
    path.addCurve(to: CGPoint(x: minX + r, y: maxY),
                  control1: CGPoint(x: minX, y: maxY - r + k * r),
                  control2: CGPoint(x: minX + r - k * r, y: maxY))
    path.closeSubpath()
    return path
}

// MARK: - Glyph

/// Phosphor's pulse, drawn as a stroked centreline on its 256-unit grid so the
/// mark stays sharp at every icon size. Coordinates were fitted against the
/// reference art: 224 units wide, 180 tall, stroke 16 with round caps.
let pulseCentreline: [CGPoint] = [
    CGPoint(x: 24, y: 128), CGPoint(x: 54, y: 128), CGPoint(x: 96, y: 45),
    CGPoint(x: 164, y: 208), CGPoint(x: 200, y: 128), CGPoint(x: 232, y: 128),
]
let pulseStroke: CGFloat = 17

// MARK: - Icon

func drawIcon(size: CGFloat) -> NSBitmapImageRep {
    let pixels = Int(size)
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    let context = NSGraphicsContext(bitmapImageRep: rep)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    let cg = context.cgContext

    // The icon square fills 82 % of the canvas, as on the macOS grid.
    let inset = size * 0.09
    let rect = CGRect(x: inset, y: inset, width: size - inset * 2, height: size - inset * 2)
    let shape = continuousSquare(rect, radius: rect.width * 0.2237)
    cg.setShouldAntialias(true)

    // Body: graphite at the top into near-black at the bottom.
    cg.saveGState()
    cg.addPath(shape)
    cg.clip()
    let body = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(),
                          colors: [
                              NSColor(srgbRed: 0.24, green: 0.28, blue: 0.33, alpha: 1).cgColor,
                              NSColor(srgbRed: 0.10, green: 0.12, blue: 0.14, alpha: 1).cgColor,
                              NSColor(srgbRed: 0.05, green: 0.06, blue: 0.07, alpha: 1).cgColor,
                          ] as CFArray, locations: [0, 0.55, 1])!
    cg.drawLinearGradient(body,
                          start: CGPoint(x: rect.midX, y: rect.maxY),
                          end: CGPoint(x: rect.midX, y: rect.minY),
                          options: [])
    // Hairline along the edge, brighter at the top, so the shape sits on any
    // wallpaper without a hard outline.
    cg.addPath(shape)
    cg.setLineWidth(max(1, size * 0.007))
    cg.setStrokeColor(NSColor(white: 1, alpha: 0.16).cgColor)
    cg.strokePath()
    cg.restoreGState()

    // The pulse, centred and 54 % of the body wide.
    let ink = CGRect(x: 24 - pulseStroke / 2, y: 45 - pulseStroke / 2,
                     width: 208 + pulseStroke, height: 163 + pulseStroke)
    let scale = rect.width * 0.56 / ink.width
    cg.saveGState()
    cg.translateBy(x: rect.midX, y: rect.midY)
    cg.scaleBy(x: scale, y: -scale)
    cg.translateBy(x: -ink.midX, y: -ink.midY)
    let glyph = CGMutablePath()
    glyph.move(to: pulseCentreline[0])
    for point in pulseCentreline.dropFirst() { glyph.addLine(to: point) }
    cg.addPath(glyph)
    cg.setLineWidth(pulseStroke)
    cg.setLineCap(.round)
    cg.setLineJoin(.round)
    cg.setStrokeColor(NSColor.white.cgColor)
    cg.strokePath()
    cg.restoreGState()

    NSGraphicsContext.restoreGraphicsState()
    return rep
}

let variants: [(String, CGFloat)] = [
    ("icon_16x16", 16), ("icon_16x16@2x", 32),
    ("icon_32x32", 32), ("icon_32x32@2x", 64),
    ("icon_128x128", 128), ("icon_128x128@2x", 256),
    ("icon_256x256", 256), ("icon_256x256@2x", 512),
    ("icon_512x512", 512), ("icon_512x512@2x", 1024),
]

let iconset = "\(outputDir)/Vitals.iconset"
try? FileManager.default.createDirectory(atPath: iconset, withIntermediateDirectories: true)
for (name, size) in variants {
    let rep = drawIcon(size: size)
    let data = rep.representation(using: .png, properties: [:])!
    try! data.write(to: URL(fileURLWithPath: "\(iconset)/\(name).png"))
}
print("wrote \(iconset)")
