import Cocoa
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
for w in list where (w[kCGWindowLayer as String] as? Int) == 0 {
    let b = CGRect(dictionaryRepresentation: w[kCGWindowBounds as String] as! CFDictionary)!
    print(w[kCGWindowOwnerName as String] ?? "", "|", w[kCGWindowName as String] ?? "", "|", Int(b.minX), Int(b.minY), Int(b.width), Int(b.height))
}
print("screens:", NSScreen.screens.map { "\($0.frame)" })
