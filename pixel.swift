import Cocoa
// pixel <png> center                                  -> "r,g,b WxH" at image center
// pixel <png> orange <ox> <oy> <ow> <x> <y> <w> <h>   -> count of orange pixels in screen-point rect (x,y,w,h)
//                                                       given the screenshot covers screen rect (ox,oy,ow,*)
let a = CommandLine.arguments
let img = NSImage(contentsOfFile: a[1])!
let rep = NSBitmapImageRep(data: img.tiffRepresentation!)!
if a[2] == "center" {
    let c = rep.colorAt(x: rep.pixelsWide / 2, y: rep.pixelsHigh / 2)!.usingColorSpace(.sRGB)!
    print("\(Int(c.redComponent*255)),\(Int(c.greenComponent*255)),\(Int(c.blueComponent*255)) \(rep.pixelsWide)x\(rep.pixelsHigh)")
} else {
    let ox = Double(a[3])!, oy = Double(a[4])!, ow = Double(a[5])!
    let scale = Double(rep.pixelsWide) / ow
    let x0 = Int((Double(a[6])! - ox) * scale), y0 = Int((Double(a[7])! - oy) * scale)
    let w = Int(Double(a[8])! * scale), h = Int(Double(a[9])! * scale)
    var n = 0
    for y in max(0,y0)..<min(rep.pixelsHigh, y0+h) { for x in max(0,x0)..<min(rep.pixelsWide, x0+w) {
        let c = rep.colorAt(x: x, y: y)!.usingColorSpace(.sRGB)!
        let r = c.redComponent*255, g = c.greenComponent*255, b = c.blueComponent*255
        if r >= 200 && g >= 100 && g <= 190 && b <= 110 { n += 1 }
    } }
    print(n)
}
