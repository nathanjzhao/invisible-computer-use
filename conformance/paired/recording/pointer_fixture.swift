import AppKit
let output=CommandLine.arguments[1]
var rows:[[String:Any]]=[]
func record(_ kind:String,_ e:NSEvent){rows.append(["kind":kind,"button":e.buttonNumber,"dx":kind == "scroll" ? e.scrollingDeltaX : 0,"dy":kind == "scroll" ? e.scrollingDeltaY : 0]);try? JSONSerialization.data(withJSONObject:rows,options:.sortedKeys).write(to:URL(fileURLWithPath:output),options:.atomic)}
class Pad:NSView {
 override var acceptsFirstResponder:Bool{true}
 override func acceptsFirstMouse(for event:NSEvent?)->Bool{true}
 override func mouseDown(with e:NSEvent){record("down",e)}
 override func mouseUp(with e:NSEvent){record("up",e)}
 override func rightMouseDown(with e:NSEvent){record("down",e)}
 override func rightMouseUp(with e:NSEvent){record("up",e)}
 override func otherMouseDown(with e:NSEvent){record("down",e)}
 override func otherMouseUp(with e:NSEvent){record("up",e)}
 override func mouseDragged(with e:NSEvent){record("drag",e)}
 override func rightMouseDragged(with e:NSEvent){record("drag",e)}
 override func otherMouseDragged(with e:NSEvent){record("drag",e)}
 override func scrollWheel(with e:NSEvent){record("scroll",e)}
 override func draw(_ r:NSRect){NSColor(calibratedRed:0.12,green:0.25,blue:0.4,alpha:1).setFill();r.fill();("Pointer capture fixture" as NSString).draw(at:NSPoint(x:28,y:210),withAttributes:[.font:NSFont.systemFont(ofSize:25),.foregroundColor:NSColor.white]);("Synthetic input only" as NSString).draw(at:NSPoint(x:28,y:165),withAttributes:[.font:NSFont.systemFont(ofSize:17),.foregroundColor:NSColor.white])}
}
let app=NSApplication.shared;app.setActivationPolicy(.regular)
let window=NSWindow(contentRect:NSRect(x:100,y:160,width:560,height:300),styleMask:[.titled,.closable],backing:.buffered,defer:false)
window.title="Pointer capture fixture"
let pad=Pad(frame:NSRect(x:0,y:0,width:560,height:300));pad.setAccessibilityElement(true);pad.setAccessibilityRole(.group);pad.setAccessibilityLabel("Pointer pad");window.contentView=pad
window.makeKeyAndOrderFront(nil);window.makeFirstResponder(pad);app.activate(ignoringOtherApps:true)
try! Data("[]".utf8).write(to:URL(fileURLWithPath:output));app.run()
