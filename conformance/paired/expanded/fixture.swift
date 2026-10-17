// Owned AppKit workflow fixture. State is independent of either computer-use runtime.
import Cocoa
let app=NSApplication.shared
app.setActivationPolicy(.accessory)
let work=URL(fileURLWithPath:CommandLine.arguments[1],isDirectory:true)
let main=NSWindow(contentRect:NSRect(x:40,y:100,width:640,height:720),styleMask:[.titled,.closable],backing:.buffered,defer:false)
main.title="Paired task fixture"
let sibling=NSWindow(contentRect:NSRect(x:750,y:150,width:330,height:240),styleMask:[.titled],backing:.buffered,defer:false)
sibling.title="Alpha document"
final class Flipped:NSView {override var isFlipped:Bool {true}}
final class Fixture:NSObject {
 let result=NSTextField(string:"0"),alpha=NSTextField(string:"alpha")
 let draft=NSTextView(frame:NSRect(x:20,y:490,width:580,height:85))
 let enabled=NSButton(checkboxWithTitle:"Enable option",target:nil,action:nil)
 let basic=NSButton(radioButtonWithTitle:"Basic mode",target:nil,action:nil)
 let detailed=NSButton(radioButtonWithTitle:"Detailed mode",target:nil,action:nil)
 let format=NSPopUpButton(frame:NSRect(x:350,y:430,width:180,height:28)),scroll=NSScrollView(frame:NSRect(x:20,y:80,width:580,height:320))
 var buttons:[String]=[],actions:[String]=[];var left=0,number=0,toggles=0;var operation="",mode="basic",applied="";var entering=false,lastClicked=false
 func save(){let r:[String:Any]=["display":result.stringValue,"draft":draft.string,"Alpha":alpha.stringValue,"enabled":enabled.state == .on,"toggle_count":toggles,"mode":mode,"format":format.titleOfSelectedItem ?? "Plain","applied":applied,"scrolled":scroll.contentView.bounds.origin.y>100,"scroll_y":scroll.contentView.bounds.origin.y,"last_record_clicked":lastClicked,"buttons":buttons,"actions":actions,"pid":ProcessInfo.processInfo.processIdentifier,"window":main.windowNumber,"alpha_window":sibling.windowNumber,"screen":[NSScreen.main!.frame.width,NSScreen.main!.frame.height,NSScreen.main!.backingScaleFactor]];try! JSONSerialization.data(withJSONObject:r,options:[.sortedKeys]).write(to:work.appendingPathComponent("state.json"),options:.atomic)}
 @objc func calc(_ b:NSButton){let key=b.title;buttons.append(key);if let n=Int(key){number=entering ? number*10+n:n;entering=true;result.stringValue=String(number)}else if key=="×"{left=number;operation=key;entering=false;result.stringValue="\(left) ×"}else if key=="="{number=operation=="×" ? left*number:number;entering=false;result.stringValue=String(number)}else{number=0;left=0;operation="";entering=false;result.stringValue="0"};save()}
 @objc func toggle(_ b:NSButton){toggles+=1;actions.append("toggle");save()}
 @objc func radio(_ b:NSButton){mode=b === detailed ? "detailed":"basic";basic.state=b === basic ? .on:.off;detailed.state=b === detailed ? .on:.off;actions.append("mode:"+mode);save()}
 @objc func choose(_ p:NSPopUpButton){actions.append("format:"+(p.titleOfSelectedItem ?? ""));save()}
 @objc func apply(_ b:NSButton){applied=draft.string;actions.append("apply");save()}
 @objc func last(_ b:NSButton){lastClicked=true;actions.append("last_record");save()}
}
let f=Fixture()
f.result.frame=NSRect(x:20,y:650,width:580,height:35);f.result.setAccessibilityLabel("Result");main.contentView!.addSubview(f.result)
for (i,label) in ["6","7","×","=","Clear"].enumerated(){let b=NSButton(title:label,target:f,action:#selector(Fixture.calc(_:)));b.frame=NSRect(x:20+i*110,y:600,width:100,height:35);b.setAccessibilityIdentifier("button-"+label);main.contentView!.addSubview(b)}
f.draft.string="seed";f.draft.setAccessibilityLabel("Draft");main.contentView!.addSubview(f.draft)
f.alpha.frame=NSRect(x:20,y:140,width:280,height:35);f.alpha.setAccessibilityLabel("Alpha");sibling.contentView!.addSubview(f.alpha)
f.enabled.frame=NSRect(x:20,y:440,width:150,height:25);f.enabled.target=f;f.enabled.action=#selector(Fixture.toggle(_:));main.contentView!.addSubview(f.enabled)
f.basic.frame=NSRect(x:175,y:440,width:140,height:25);f.basic.state = .on;f.basic.target=f;f.basic.action=#selector(Fixture.radio(_:));main.contentView!.addSubview(f.basic)
f.detailed.frame=NSRect(x:175,y:410,width:140,height:25);f.detailed.target=f;f.detailed.action=#selector(Fixture.radio(_:));main.contentView!.addSubview(f.detailed)
f.format.addItems(withTitles:["Plain","Markdown","CSV"]);f.format.setAccessibilityLabel("Format");f.format.target=f;f.format.action=#selector(Fixture.choose(_:));main.contentView!.addSubview(f.format)
let doc=Flipped(frame:NSRect(x:0,y:0,width:555,height:900));for i in 0..<16 {let b=NSButton(title:i==15 ? "Last record":"Record \(i+1)",target:i==15 ? f:nil,action:i==15 ? #selector(Fixture.last(_:)):nil);b.frame=NSRect(x:15,y:i*55+8,width:300,height:35);doc.addSubview(b)}
f.scroll.documentView=doc;f.scroll.hasVerticalScroller=true;f.scroll.setAccessibilityLabel("Records");main.contentView!.addSubview(f.scroll)
let apply=NSButton(title:"Apply draft",target:f,action:#selector(Fixture.apply(_:)));apply.frame=NSRect(x:20,y:20,width:150,height:35);main.contentView!.addSubview(apply)
sibling.orderFrontRegardless();main.orderFrontRegardless();f.scroll.contentView.scroll(to:.zero)
let timer=Timer.scheduledTimer(withTimeInterval:0.05,repeats:true){_ in f.save()};f.save();app.run()
