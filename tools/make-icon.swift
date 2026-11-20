// Draws the app icon and writes Vitals.iconset, then an .icns.
//
// macOS icon grid: a continuous-corner square with margins, a dark graphite
// gradient, a hairline highlight along the top edge. The glyph is the pulse in
// tools/pulse.png, drawn white.

import AppKit

let outputDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "packaging"
let glyphFile = CommandLine.arguments.count > 2 ? CommandLine.arguments[2] : "tools/pulse.png"

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
                  control2: CGPoint(x: minX, y: minY - r + k * r))
    path.addLine(to: CGPoint(x: minX, y: maxY - r))
    path.addCurve(to: CGPoint(x: minX + r, y: maxY),
                  control1: CGPoint(x: minX, y: maxY - r + k * r),
                  control2: CGPoint(x: minX + r - k * r, y: maxY))
    path.closeSubpath()
    return path
}

// MARK: - Glyph

/// The pulse drawn white: the source art is a black glyph on transparency, so
/// fill a canvas white and keep it only where the source has ink.
func whiteGlyph(_ path: String) -> (image: CGImage, ink: CGRect)? {
    guard let source = NSImage(contentsOfFile: path),
          let bitmap = source.representations.first as? NSBitmapImageRep,
          let cgSource = bitmap.cgImage else { return nil }
    let w = cgSource.width, h = cgSource.height
    let space = CGColorSpaceCreateDeviceRGB()
    guard let context = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8,
                                  bytesPerRow: 0, space: space,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
    else { return nil }
    context.setFillColor(NSColor.white.cgColor)
    context.fill(CGRect(x: 0, y: 0, width: w, height: h))
    context.setBlendMode(.destinationIn)
    context.draw(cgSource, in: CGRect(x: 0, y: 0, width: w, height: h))

    var minX = w, maxX = -1, minY = h, maxY = -1
    for y in 0..<h { for x in 0..<w {
        guard let a = bitmap.colorAt(x: x, y: y)?.alphaComponent, a > 0.35 else { continue }
        minX = min(minX, x); maxX = max(maxX, x)
        minY = min(minY, y); maxY = max(maxY, y)
    }}
    guard maxX >= 0, let image = context.makeImage() else { return nil }
    return (image, CGRect(x: minX, y: minY, width: maxX - minX + 1, height: maxY - minY + 1))
}

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
                              NSColor(srgbRed: 0.20, green: 0.24, blue: 0.28, alpha: 1).cgColor,
                              NSColor(srgbRed: 0.07, green: 0.08, blue: 0.10, alpha: 1).cgColor,
                          ] as CFArray, locations: [0, 1])!
    cg.drawLinearGradient(body,
                          start: CGPoint(x: rect.midX, y: rect.maxY),
                          end: CGPoint(x: rect.midX, y: rect.minY),
                          options: [])
    let highlight = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(),
                               colors: [
                                   NSColor(white: 1, alpha: 0.16).cgColor,
                                   NSColor(white: 1, alpha: 0).cgColor,
                               ] as CFArray, locations: [0, 1])!
    cg.drawLinearGradient(highlight,
                          start: CGPoint(x: rect.midX, y: rect.maxY),
                          end: CGPoint(x: rect.midX, y: rect.midY),
                          options: [])
    cg.restoreGState()

    // Hairline edge, light at the top and fading out by the bottom.
    let edge = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(),
                          colors: [
                              NSColor(white: 1, alpha: 0.22).cgColor,
                              NSColor(white: 1, alpha: 0.05).cgColor,
                          ] as CFArray, locations: [0, 1])!
    cg.saveGState()
    cg.addPath(shape)
    cg.clip()
    cg.addPath(shape)
    cg.setLineWidth(max(1, size * 0.005))
    cg.replacePathWithStrokedPath()
    cg.clip()
    cg.drawLinearGradient(edge,
                          start: CGPoint(x: rect.midX, y: rect.maxY),
                          end: CGPoint(x: rect.midX, y: rect.minY),
                          options: [])
    cg.restoreGState()

    // The pulse glyph: the art's ink box, centred at 54 % of the body width.
    if let glyph = whiteGlyph(glyphFile) {
        let ink = glyph.ink
        let scale = rect.width * 0.54 / ink.width
        let dest = CGRect(x: rect.midX - (ink.midX) * scale,
                          y: rect.midY - (CGFloat(glyph.image.height) - ink.midY) * scale,
                          width: CGFloat(glyph.image.width) * scale,
                          height: CGFloat(glyph.image.height) * scale)
        cg.saveGState()
        cg.interpolationQuality = .high
        cg.setShadow(offset: CGSize(width: 0, height: -size * 0.008),
                     blur: size * 0.025,
                     color: NSColor(white: 0, alpha: 0.5).cgColor)
        cg.draw(glyph.image, in: dest)
        cg.restoreGState()
    }

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
