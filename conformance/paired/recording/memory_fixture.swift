import AppKit
let args=CommandLine.arguments, directory=args[1], family=args[2]
let families=["invoice-routing","expense-review","inventory-transfer","appointment-booking","support-triage","document-export","contact-import","report-filtering"]
let choices=[["Review","Prepare","Validate"],["Assign","Route","Categorize"],["Finish","Archive","Approve"]]
let routes=[[0,1,2],[2,0,1],[1,2,0],[2,1,0],[0,2,1],[1,0,2],[2,2,0],[0,0,1]]
let familyIndex=families.firstIndex(of:family)!, route=routes[familyIndex]
let app=NSApplication.shared;app.setActivationPolicy(.regular)
let window=NSWindow(contentRect:NSRect(x:90,y:200,width:690,height:420),styleMask:[.titled,.closable],backing:.buffered,defer:false);window.title="Workflow Lab"
let content=NSView(frame:NSRect(x:0,y:0,width:690,height:420));window.contentView=content
let title=NSTextField(labelWithString:family.replacingOccurrences(of:"-",with:" "));title.font=NSFont.boldSystemFont(ofSize:24);title.frame=NSRect(x:24,y:362,width:640,height:40);content.addSubview(title)
let status=NSTextField(labelWithString:"Enter a reference, then complete the three workflow stages.");status.frame=NSRect(x:24,y:304,width:640,height:40);content.addSubview(status)
let label=NSTextField(labelWithString:"Reference");label.frame=NSRect(x:24,y:260,width:100,height:26);content.addSubview(label)
let field=NSTextField(frame:NSRect(x:130,y:256,width:400,height:28));field.setAccessibilityLabel("Reference");content.addSubview(field)
let guide=NSTextField(wrappingLabelWithString:"");guide.frame=NSRect(x:24,y:48,width:640,height:50);content.addSubview(guide)
var stage=0,errors=0,helpViews=0,selected:[String]=[],complete=false
func save(){let obj:[String:Any]=["family":family,"reference":field.stringValue,"stage":stage,"errors":errors,"help_views":helpViews,"path":selected,"completed":complete];try! JSONSerialization.data(withJSONObject:obj,options:.sortedKeys).write(to:URL(fileURLWithPath:directory+"/state.json"),options:.atomic)}
var buttons:[NSButton]=[]
func refresh(){status.stringValue=complete ? "Workflow completed for "+field.stringValue : "Stage \(stage+1) of 3";for (i,b) in buttons.enumerated(){b.title=choices[min(stage,2)][i];b.isEnabled = !complete};save()}
class Handler:NSObject{
 @objc func select(_ b:NSButton){
  if field.stringValue.isEmpty{status.stringValue="Enter a reference first.";save();return}
  if b.tag != route[stage]{errors+=1;status.stringValue="That is not the configured route. Read Workflow guide and try again.";save();return}
  selected.append(b.title);stage+=1;if stage==3{complete=true};refresh()
 }
 @objc func help(_ b:NSButton){helpViews+=1;guide.stringValue="For \(family): "+route.enumerated().map{choices[$0.offset][$0.element]}.joined(separator:" → ")+". Keep the current reference.";save()}
}
let handler=Handler()
for i in 0..<3{let b=NSButton(title:choices[0][i],target:handler,action:#selector(Handler.select(_:)));b.tag=i;b.bezelStyle = .rounded;b.frame=NSRect(x:24+i*210,y:170,width:190,height:42);content.addSubview(b);buttons.append(b)}
let help=NSButton(title:"Workflow guide",target:handler,action:#selector(Handler.help(_:)));help.bezelStyle = .rounded;help.frame=NSRect(x:24,y:105,width:190,height:36);content.addSubview(help)
window.makeKeyAndOrderFront(nil);app.activate(ignoringOtherApps:true);refresh();app.run()
