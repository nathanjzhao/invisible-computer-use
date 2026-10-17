import Foundation
import CoreGraphics
let a=CommandLine.arguments
let p=CGPoint(x:Double(a[2])!,y:Double(a[3])!)
let source=CGEventSource(stateID:.hidSystemState)!
func post(_ type:CGEventType,_ at:CGPoint,_ b:CGMouseButton){let e=CGEvent(mouseEventSource:source,mouseType:type,mouseCursorPosition:at,mouseButton:b)!;e.post(tap:.cghidEventTap)}
if a[1]=="scroll" {post(.mouseMoved,p,.left);usleep(20000);let e=CGEvent(scrollWheelEvent2Source:source,units:.line,wheelCount:2,wheel1:Int32(a[4])!,wheel2:Int32(a[5])!,wheel3:0)!;e.location=p;e.post(tap:.cghidEventTap)}
else {
 let b=CGMouseButton(rawValue:UInt32(a[4])!)!;let down:CGEventType=b == .left ? .leftMouseDown : b == .right ? .rightMouseDown : .otherMouseDown;let up:CGEventType=b == .left ? .leftMouseUp : b == .right ? .rightMouseUp : .otherMouseUp
 let count=["burst","long"].contains(a[1]) ? Int(a[5])! : 1
 for _ in 0..<count {
  post(.mouseMoved,p,b);post(down,p,b)
  if a[1]=="drag"{let end=CGPoint(x:p.x+80,y:p.y+45);usleep(20000);post(b == .left ? .leftMouseDragged : b == .right ? .rightMouseDragged : .otherMouseDragged,end,b);usleep(20000);post(up,end,b)}
  else{usleep(a[1]=="burst" ? 1000 : 12000);post(up,p,b);if a[1]=="long"{usleep(80000)}}
 }
}
