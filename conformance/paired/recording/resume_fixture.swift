import AppKit
import CryptoKit
let output=CommandLine.arguments[1], total=81
let names=["Cedar","Birch","Maple","Pine","Elm"]
var stage=0,errors=0,events:[[String:Any]]=[]
func expected()->Int{Int(Array(SHA256.hash(data:Data("icu-resume-round3:\(stage):native-only".utf8)))[0]) % names.count}
let app=NSApplication.shared;app.setActivationPolicy(.regular)
let window=NSWindow(contentRect:NSRect(x:90,y:200,width:690,height:360),styleMask:[.titled,.closable],backing:.buffered,defer:false);window.title="Resume Lab"
let label=NSTextField(labelWithString:"");label.font=NSFont.systemFont(ofSize:22);label.frame=NSRect(x:24,y:240,width:640,height:80);window.contentView!.addSubview(label)
var buttons:[NSButton]=[]
func save(){let state:[String:Any]=["stage":stage,"total":total,"errors":errors,"completed":stage==total,"events":events];try! JSONSerialization.data(withJSONObject:state,options:.sortedKeys).write(to:URL(fileURLWithPath:output),options:.atomic)}
func refresh(){label.stringValue=stage==total ? "Completed all \(total) stages." : "Stage \(stage+1) of \(total). Choose \(names[expected()]).";for b in buttons{b.isEnabled=stage<total};save()}
class Handler:NSObject{@objc func select(_ b:NSButton){guard stage<total else{return};let choice=expected();events.append(["stage":stage,"selected":names[b.tag],"expected":names[choice],"accepted":b.tag==choice]);if b.tag==choice{stage+=1}else{errors+=1};refresh()}}
let handler=Handler()
for (i,name) in names.enumerated(){let b=NSButton(title:name,target:handler,action:#selector(Handler.select(_:)));b.tag=i;b.bezelStyle = .rounded;b.frame=NSRect(x:24+i*132,y:160,width:116,height:44);window.contentView!.addSubview(b);buttons.append(b)}
window.makeKeyAndOrderFront(nil);app.activate(ignoringOtherApps:true);refresh();app.run()
