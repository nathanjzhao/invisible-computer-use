import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';

const source=fs.readFileSync(new URL('../src-tauri/linux-helpers/gnome/winrects@cua/history-pointer.js',import.meta.url),'utf8')
  .replace(/^import .*;\n/gm,'').replace('export class HistoryPointerSessions','globalThis.HistoryPointerSessions=class HistoryPointerSessions');
function fixture(){
  let now=1_000_000,serial=0,event=null;const callbacks=new Map(),watches=new Map(),timers=new Map();
  const rect={x:100,y:100,width:600,height:400};
  const win={minimized:false,get_pid:()=>42,get_stable_sequence:()=>7,get_client_type:()=>1,
    get_frame_rect:()=>rect,get_buffer_rect:()=>rect,get_user_time:()=>1000,
    connect:(name,callback)=>{const id=++serial;callbacks.set(id,{name,callback});return id;},disconnect:id=>callbacks.delete(id)};
  const Main={sessionMode:{isLocked:false,hasWindows:true},overview:{visible:false},modalCount:0};
  const global={display:{focus_window:win},stage:{get_key_focus:()=>null},get_window_actors:()=>[{meta_window:win,visible:true}]};
  const GLib={get_monotonic_time:()=>now,get_real_time:()=>1_700_000_000_000_000+now,
    uuid_string_random:()=>`00000000-0000-4000-8000-${String(++serial).padStart(12,'0')}`,
    timeout_add:(_priority,_ms,callback)=>{const id=++serial;timers.set(id,callback);return id;},source_remove:id=>timers.delete(id),SOURCE_CONTINUE:true};
  const Gio={DBus:{session:{}},BusNameWatcherFlags:{NONE:0},bus_watch_name_on_connection:(_bus,_caller,_flags,_present,gone)=>{const id=++serial;watches.set(id,gone);return id;},bus_unwatch_name:id=>watches.delete(id)};
  const Clutter={get_current_event:()=>event,EventType:{BUTTON_PRESS:6,KEY_PRESS:1,KEY_RELEASE:2},
    KEY_Return:0xff0d,KEY_KP_Enter:0xff8d,KEY_Tab:0xff09,KEY_ISO_Left_Tab:0xfe20,KEY_Escape:0xff1b,
    ModifierType:{CONTROL_MASK:4,MOD1_MASK:8,SHIFT_MASK:1,SUPER_MASK:64}};
  const context=vm.createContext({Gio,GLib,Clutter,Main,global,Meta:{WindowClientType:{WAYLAND:1}}});vm.runInContext(source,context);
  const sessions=new context.HistoryPointerSessions();
  const emit=(override={})=>{
    event={type:()=>6,get_source:()=>null,get_time:()=>1000,get_button:()=>1,get_coords:()=>[300.5,250.5],...override};
    for(const {name,callback} of callbacks.values())if(name==='notify::user-time')callback();
  };
  return {sessions,win,Main,global,rect,emit,watches,timers,advance:us=>{now+=us;}};
}
test('only the same live caller can read or stop its pointer scope',()=>{
  const f=fixture();const started=f.sessions.start(':1.4',7,42);f.emit();
  assert.throws(()=>f.sessions.read(':1.5',started.token),/unavailable/);
  assert.equal(f.sessions.stop(':1.5',started.token).stopped,false);
  const result=f.sessions.read(':1.4',started.token);assert.equal(result.events.length,1);assert.equal(result.events[0].button,1);
  assert.equal(f.sessions.read(':1.4',started.token).events.length,0);f.sessions.close();assert.equal(f.watches.size,0);assert.equal(f.timers.size,0);
});
test('Shell input, stale timestamps, locked sessions and foreign windows do not contribute',()=>{
  const f=fixture();const {token}=f.sessions.start(':1.4',7,42);
  f.emit({get_source:()=>({shell:true})});f.emit({get_time:()=>999});f.emit({get_coords:()=>[99,250]});f.emit({get_button:()=>8});
  assert.equal(f.sessions.read(':1.4',token).events.length,0);
  f.Main.sessionMode.isLocked=true;f.emit();assert.throws(()=>f.sessions.read(':1.4',token),/changed/);
  f.Main.sessionMode.isLocked=false;assert.equal(f.sessions.read(':1.4',token).events.length,0);
  f.global.display.focus_window={};f.emit();assert.throws(()=>f.sessions.check(':1.4',token),/changed/);f.sessions.close();
});
test('expiry, client exit and scope replacement invalidate queued events',()=>{
  const f=fixture();const a=f.sessions.start(':1.4',7,42);f.emit();const b=f.sessions.start(':1.4',7,42);
  assert.throws(()=>f.sessions.read(':1.4',a.token),/unavailable/);assert.equal(f.sessions.read(':1.4',b.token).events.length,0);
  f.emit();f.advance(2_000_001);assert.throws(()=>f.sessions.read(':1.4',b.token),/expired/);assert.equal(f.watches.size,0);
  const c=f.sessions.start(':1.4',7,42);f.emit();[...f.watches.values()][0]();assert.throws(()=>f.sessions.read(':1.4',c.token),/unavailable/);f.sessions.close();
});
test('native surface focus must resolve to the exact admitted window',()=>{
  const f=fixture();f.global.stage.get_key_focus=()=>({get_parent:()=>({get_meta_window:()=>f.win})});
  const {token}=f.sessions.start(':1.4',7,42);f.emit();assert.equal(f.sessions.read(':1.4',token).events.length,1);
  f.global.stage.get_key_focus=()=>({get_parent:()=>({get_meta_window:()=>({...f.win})})});
  assert.throws(()=>f.sessions.read(':1.4',token),/changed/);f.sessions.close();
});
test('the stage focus sentinel is distinct from an unrelated Shell actor',()=>{
  const f=fixture();f.global.stage.get_key_focus=()=>f.global.stage;
  const {token}=f.sessions.start(':1.4',7,42);f.emit();assert.equal(f.sessions.read(':1.4',token).events.length,1);
  f.global.stage.get_key_focus=()=>({get_parent:()=>null});
  assert.throws(()=>f.sessions.read(':1.4',token),/changed/);f.sessions.close();
});
test('overflow reports lost input without returning a misleading partial queue',()=>{
  const f=fixture();const {token}=f.sessions.start(':1.4',7,42);for(let i=0;i<70;i++)f.emit();
  const result=f.sessions.read(':1.4',token);assert.equal(result.lost,true);assert.equal(result.events.length,0);
  f.emit();const next=f.sessions.read(':1.4',token);assert.equal(next.lost,false);assert.equal(next.events.length,1);assert.equal(next.events[0].sequence,result.sequence+1);f.sessions.close();
});
test('only named non-text keys and modifiers enter the leased queue',()=>{
  const f=fixture();const {token}=f.sessions.start(':1.4',7,42);
  const key=symbol=>({type:()=>1,get_key_symbol:()=>symbol,get_state:()=>69});
  for(const symbol of [0x61,0x31,0x20,0x4e00])f.emit(key(symbol));
  f.emit({...key(0xff0d),type:()=>2});
  f.emit({...key(0xff0d),get_source:()=>({shell:true})});
  assert.equal(f.sessions.read(':1.4',token).events.length,0);
  for(const symbol of [0xff0d,0xff8d,0xff09,0xfe20,0xff1b])f.emit(key(symbol));
  const events=f.sessions.read(':1.4',token).events;
  assert.equal(events.map(e=>e.key).join(','),'return,return,tab,tab,escape');
  for(const event of events){
    assert.equal(event.button,0);assert.equal(event.x,0);assert.equal(event.y,0);
    assert.equal(event.ctrl,true);assert.equal(event.shift,true);assert.equal(event.win,true);assert.equal(event.alt,false);
    assert.equal('symbol' in event,false);assert.equal('keycode' in event,false);
  }
  f.sessions.close();
});
