// Bounded human-speed input fixture for an explicitly disposable desktop.
import Cocoa
if ProcessInfo.processInfo.environment["ICU_DISPOSABLE_HOST"] != "1" { exit(2) }
guard CommandLine.arguments.count == 3, let x=Double(CommandLine.arguments[1]), let y=Double(CommandLine.arguments[2]), x.isFinite,y.isFinite else {exit(2)}
let point=CGPoint(x:x,y:y)
let source=CGEventSource(stateID:.hidSystemState)
CGEvent(mouseEventSource:source,mouseType:.mouseMoved,mouseCursorPosition:point,mouseButton:.left)!.post(tap:.cghidEventTap)
Thread.sleep(forTimeInterval:0.03)
CGEvent(mouseEventSource:source,mouseType:.leftMouseDown,mouseCursorPosition:point,mouseButton:.left)!.post(tap:.cghidEventTap)
Thread.sleep(forTimeInterval:0.20)
CGEvent(mouseEventSource:source,mouseType:.leftMouseUp,mouseCursorPosition:point,mouseButton:.left)!.post(tap:.cghidEventTap)
