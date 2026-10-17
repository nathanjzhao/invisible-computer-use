// Synthetic click duration is part of the recorder acceptance input.
import Cocoa
if ProcessInfo.processInfo.environment["ICU_DISPOSABLE_HOST"] != "1" {exit(2)}
guard CommandLine.arguments.count==4,let x=Double(CommandLine.arguments[1]),let y=Double(CommandLine.arguments[2]),let ms=Double(CommandLine.arguments[3]),x.isFinite,y.isFinite,ms>=1,ms<=500 else {exit(2)}
let point=CGPoint(x:x,y:y);let source=CGEventSource(stateID:.hidSystemState)
CGEvent(mouseEventSource:source,mouseType:.mouseMoved,mouseCursorPosition:point,mouseButton:.left)!.post(tap:.cghidEventTap)
Thread.sleep(forTimeInterval:0.03)
CGEvent(mouseEventSource:source,mouseType:.leftMouseDown,mouseCursorPosition:point,mouseButton:.left)!.post(tap:.cghidEventTap)
Thread.sleep(forTimeInterval:ms/1000)
CGEvent(mouseEventSource:source,mouseType:.leftMouseUp,mouseCursorPosition:point,mouseButton:.left)!.post(tap:.cghidEventTap)
