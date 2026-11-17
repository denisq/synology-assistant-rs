// Renders the app icon (original artwork) to PNG files of the given sizes.
// usage: swift make_icon.swift <out-dir>
import AppKit

func render(_ size: CGFloat) -> Data {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(size), pixelsHigh: Int(size),
                               bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let s = size / 1024.0
    // Background: rounded square (macOS icon grid: 824 of 1024, 100 inset), teal gradient.
    let bg = NSBezierPath(roundedRect: NSRect(x: 100*s, y: 100*s, width: 824*s, height: 824*s), xRadius: 185*s, yRadius: 185*s)
    NSGradient(starting: NSColor(calibratedRed: 0.09, green: 0.36, blue: 0.48, alpha: 1),
               ending: NSColor(calibratedRed: 0.05, green: 0.17, blue: 0.27, alpha: 1))!.draw(in: bg, angle: -90)
    // NAS body: light rounded box.
    let body = NSRect(x: 300*s, y: 230*s, width: 424*s, height: 470*s)
    NSColor(calibratedWhite: 0.96, alpha: 1).setFill()
    NSBezierPath(roundedRect: body, xRadius: 48*s, yRadius: 48*s).fill()
    // Two drive bays.
    NSColor(calibratedRed: 0.10, green: 0.25, blue: 0.34, alpha: 1).setFill()
    for i in 0..<2 {
        let x = (352 + CGFloat(i) * 168) * s
        NSBezierPath(roundedRect: NSRect(x: x, y: 300*s, width: 152*s, height: 330*s), xRadius: 18*s, yRadius: 18*s).fill()
    }
    // Status LEDs.
    NSColor(calibratedRed: 0.35, green: 0.90, blue: 0.62, alpha: 1).setFill()
    for i in 0..<2 {
        let x = (414 + CGFloat(i) * 168) * s
        NSBezierPath(ovalIn: NSRect(x: x, y: 330*s, width: 28*s, height: 28*s)).fill()
    }
    // Discovery waves above the box.
    NSColor(calibratedRed: 0.35, green: 0.90, blue: 0.62, alpha: 1).setStroke()
    for (i, r) in [70.0, 130.0].enumerated() {
        let p = NSBezierPath()
        p.appendArc(withCenter: NSPoint(x: 512*s, y: 700*s), radius: CGFloat(r)*s, startAngle: 50, endAngle: 130)
        p.lineWidth = CGFloat(30 - i*4) * s
        p.lineCapStyle = .round
        p.stroke()
    }
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

let out = CommandLine.arguments[1]
for px in [16, 32, 64, 128, 256, 512, 1024] {
    try! render(CGFloat(px)).write(to: URL(fileURLWithPath: "\(out)/icon_\(px).png"))
}
