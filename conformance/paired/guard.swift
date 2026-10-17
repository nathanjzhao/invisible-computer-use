import Cocoa
// Owned foreground sentinel, never started on the user's desktop.
guard ProcessInfo.processInfo.environment["ICU_DISPOSABLE_HOST"] == "1" else {exit(2)}
let out=URL(fileURLWithPath:CommandLine.arguments[1],isDirectory:true)
let app=NSApplication.shared;app.setActivationPolicy(.regular)
let menu=NSMenu();let item=NSMenuItem(title:"Guard",action:nil,keyEquivalent:"");let submenu=NSMenu();submenu.addItem(withTitle:"Quit",action:#selector(NSApplication.terminate(_:)),keyEquivalent:"q");item.submenu=submenu;menu.addItem(item);app.mainMenu=menu
let window=NSWindow(contentRect:NSRect(x:1300,y:40,width:420,height:150),styleMask:[.titled,.closable],backing:.buffered,defer:false)
window.title="Paired Foreground Guard"
let field=NSTextField(string:"guard");field.frame=NSRect(x:20,y:70,width:370,height:35);field.setAccessibilityLabel("Guard input");window.contentView!.addSubview(field)
func publish(){let data=try! JSONSerialization.data(withJSONObject:["pid":ProcessInfo.processInfo.processIdentifier,"window":window.windowNumber,"text":field.stringValue],options:[.sortedKeys]);try! data.write(to:out.appendingPathComponent("guard.json"),options:.atomic)}
window.makeKeyAndOrderFront(nil);app.activate(ignoringOtherApps:true);window.makeFirstResponder(field)
let timer=Timer.scheduledTimer(withTimeInterval:0.03,repeats:true){_ in publish()};publish();app.run()
