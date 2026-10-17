// Same owned AppKit fixture for both native engines, on disposable desktops only.
import Cocoa
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let work = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let main = NSWindow(contentRect: NSRect(x: 80,y: 180,width: 560,height: 440),styleMask:[.titled,.closable],backing:.buffered,defer:false)
main.title = "Paired task fixture"
let sibling = NSWindow(contentRect: NSRect(x: 760,y: 180,width: 360,height: 260),styleMask:[.titled],backing:.buffered,defer:false)
sibling.title = "Alpha document"
final class Fixture:NSObject {
 let result=NSTextField(string:"0")
 let draft=NSTextView(frame:NSRect(x:20,y:40,width:490,height:80))
 let alpha=NSTextField(string:"alpha")
 var buttons:[String]=[]
 var left=0;var number=0;var operation="";var entering=false
 func save() {
  let value:[String:Any] = ["display":result.stringValue,"draft":draft.string,"Alpha":alpha.stringValue,"buttons":buttons,"pid":ProcessInfo.processInfo.processIdentifier,"window":main.windowNumber,"alpha_window":sibling.windowNumber,"screen":[NSScreen.main!.frame.width,NSScreen.main!.frame.height,NSScreen.main!.backingScaleFactor]]
  try! JSONSerialization.data(withJSONObject:value,options:[.sortedKeys]).write(to:work.appendingPathComponent("state.json"),options:.atomic)
 }
 @objc func press(_ b:NSButton) {
  buttons.append(b.title)
  if let n=Int(b.title) {number=entering ? number*10+n : n;entering=true;result.stringValue=String(number)}
  else if b.title=="×" {left=number;operation="×";entering=false;result.stringValue="\(left) ×"}
  else if b.title=="=" {number=operation=="×" ? left*number : number;result.stringValue=String(number);entering=false}
  else if b.title=="Clear" {number=0;left=0;operation="";entering=false;result.stringValue="0"}
  save()
 }
}
let fixture=Fixture()
fixture.result.frame=NSRect(x:20,y:350,width:490,height:40)
fixture.result.setAccessibilityLabel("Result");fixture.result.setAccessibilityIdentifier("result")
fixture.draft.string="seed";fixture.draft.setAccessibilityLabel("Draft")
fixture.alpha.frame=NSRect(x:20,y:150,width:300,height:30);fixture.alpha.setAccessibilityLabel("Alpha")
main.contentView!.addSubview(fixture.result);main.contentView!.addSubview(fixture.draft);sibling.contentView!.addSubview(fixture.alpha)
for (index,label) in ["6","7","×","=","Clear"].enumerated() {
 let b=NSButton(title:label,target:fixture,action:#selector(Fixture.press(_:)))
 b.frame=NSRect(x:20+index*100,y:260,width:90,height:40);b.setAccessibilityIdentifier("button-\(label)");main.contentView!.addSubview(b)
}
sibling.orderFrontRegardless();main.orderFrontRegardless()
let timer=Timer.scheduledTimer(withTimeInterval:0.05,repeats:true){_ in fixture.save()}
fixture.save()
app.run()
