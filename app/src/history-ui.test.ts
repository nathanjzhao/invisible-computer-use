// @vitest-environment jsdom
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {readFileSync} from 'node:fs';
import {historySettings,historyText,wireHistory,type HistoryStatus} from './history-ui';
import {wireTrajectoryImport,type RecordingPreview} from './trajectory-ui';
let dispose:()=>void=()=>{};
beforeEach(()=>{vi.useFakeTimers();document.documentElement.innerHTML=readFileSync(`${process.cwd()}/index.html`,'utf8');});
afterEach(()=>{dispose();vi.clearAllTimers();vi.useRealTimers();});
const get=<T extends HTMLElement=HTMLInputElement>(id:string)=>document.getElementById(id) as T;
const base=():HistoryStatus=>({enabled:false,paused:false,capturing:false,capture_mode:'application_metadata',revision:3,generation:'fixture-generation',retention_days:7,collector:null,recent_activity_files:[],settings:{observation:{defaultApplicationBehavior:'observe',defaultURLBehavior:'observe',allowlist:[],blocklist:[]}}});
const flush=async()=>{for(let i=0;i<8;i++)await Promise.resolve();};
it('desktop setup is explicit and keeps app identity scope',async()=>{
 const calls:unknown[]=[];let state:HistoryStatus={...base(),helper_setup_supported:true};
 dispose=wireHistory(document,async<T>(command:string,args?:Record<string,unknown>)=>{
  calls.push([command,args]);
  if(command==='computer_history_configure')state={...state,enabled:true,revision:4};
  return structuredClone(state) as T;
 });await flush();
 const setup=get<HTMLButtonElement>('history-helper-setup');expect(setup.hidden).toBe(false);expect(setup.disabled).toBe(true);
 get<HTMLInputElement>('history-consent').checked=true;get('history-consent').dispatchEvent(new Event('change'));get('history-toggle').click();await flush();
 expect(setup.disabled).toBe(false);expect(calls.some(v=>(v as unknown[])[0]==='computer_history_setup_desktop')).toBe(false);
 setup.click();await flush();
 expect(calls).toContainEqual(['computer_history_setup_desktop',{approved:true,generation:'fixture-generation',revision:4}]);
 expect(get<HTMLSelectElement>('history-scope').value).toBe('application_metadata');
 expect(calls.some(v=>(v as unknown[])[0]==='computer_history_capture_mode')).toBe(false);
});
it('desktop setup stays hidden where unnecessary and cannot apply unsaved rules',async()=>{
 let state:HistoryStatus={...base(),enabled:true};const calls:string[]=[];
 dispose=wireHistory(document,async<T>(command:string)=>{calls.push(command);return structuredClone(state) as T;});await flush();
 expect(get<HTMLButtonElement>('history-helper-setup').hidden).toBe(true);
 state={...state,helper_setup_supported:true};await vi.advanceTimersByTimeAsync(2000);
 get<HTMLTextAreaElement>('history-block-apps').value='private.app';get('history-block-apps').dispatchEvent(new Event('input'));
 expect(get<HTMLButtonElement>('history-helper-setup').disabled).toBe(true);
 expect(calls).not.toContain('computer_history_setup_desktop');
});
it('enabling needs fresh permission and unsaved rules block starting capture',async()=>{
 const calls:unknown[]=[];let state=base();
 const invoke=async<T>(command:string,args?:Record<string,unknown>):Promise<T>=>{calls.push([command,args]);if(command==='computer_history_configure'){state={...state,enabled:Boolean(args?.enabled),revision:4};}return structuredClone(state) as T;};
 dispose=wireHistory(document,invoke);await flush();expect(get<HTMLButtonElement>('history-toggle').disabled).toBe(true);
 get<HTMLInputElement>('history-consent').checked=true;get('history-consent').dispatchEvent(new Event('change'));expect(get<HTMLButtonElement>('history-toggle').disabled).toBe(false);
 get<HTMLTextAreaElement>('history-block-apps').value='private.app';get('history-block-apps').dispatchEvent(new Event('input'));expect(get<HTMLInputElement>('history-consent').checked).toBe(false);expect(get<HTMLButtonElement>('history-toggle').disabled).toBe(true);
 get('history-reload').click();await flush();get<HTMLInputElement>('history-consent').checked=true;get('history-consent').dispatchEvent(new Event('change'));get('history-toggle').click();await flush();
 expect(calls).toContainEqual(['computer_history_configure',{enabled:true,retentionDays:7,approved:true}]);expect(get<HTMLInputElement>('history-consent').checked).toBe(false);expect(get('history-toggle').textContent).toBe('Disable history');
});
it('polling preserves the original revision while a user edits rules',async()=>{
 let state=base();const calls:unknown[]=[];
 const invoke=async<T>(command:string,args?:Record<string,unknown>):Promise<T>=>{calls.push([command,args]);if(command==='computer_history_settings_set')throw new Error('History changed; reload');return structuredClone(state) as T;};
 dispose=wireHistory(document,invoke);await flush();get<HTMLTextAreaElement>('history-block-apps').value='private.app';get('history-block-apps').dispatchEvent(new Event('input'));
 state={...state,revision:9,settings:{observation:{...state.settings.observation,blocklist:[{scope:'app',bundleID:'other.app'}]}}};await vi.advanceTimersByTimeAsync(2000);
 expect(get<HTMLTextAreaElement>('history-block-apps').value).toBe('private.app');get('history-save').click();await flush();
 const call=calls.find(v=>(v as unknown[])[0]==='computer_history_settings_set') as [string,Record<string,unknown>];expect(call[1].revision).toBe(3);expect(get('history-message').textContent).toContain('reload');
 get('history-reload').click();await flush();expect(get<HTMLTextAreaElement>('history-block-apps').value).toBe('other.app');
});
it('retention edits use a separate operation and never re-enable capture',async()=>{
 const state={...base(),enabled:true};const calls:unknown[]=[];
 const invoke=async<T>(command:string,args?:Record<string,unknown>):Promise<T>=>{calls.push([command,args]);return {...state,revision:command==='computer_history_settings_set'?4:3} as T;};
 dispose=wireHistory(document,invoke);await flush();get<HTMLInputElement>('history-retention').value='5';get('history-retention').dispatchEvent(new Event('input'));get('history-save').click();await flush();
 expect(calls).toContainEqual(['computer_history_retention_set',{retentionDays:5,generation:'fixture-generation',revision:4}]);expect(calls.some(v=>(v as unknown[])[0]==='computer_history_configure')).toBe(false);
});
it('disable works with an invalid draft retention, and filenames are rendered as text',async()=>{
 const state={...base(),enabled:true,capturing:true,recent_activity_files:['<img src=x onerror=alert(1)>']};const calls:unknown[]=[];
 const invoke=async<T>(command:string,args?:Record<string,unknown>):Promise<T>=>{calls.push([command,args]);return structuredClone(state) as T;};
 dispose=wireHistory(document,invoke);await flush();expect(document.querySelector('#history-files img')).toBeNull();get<HTMLInputElement>('history-retention').value='';get('history-retention').dispatchEvent(new Event('input'));get('history-toggle').click();await flush();
 expect(calls).toContainEqual(['computer_history_configure',{enabled:false,retentionDays:7,approved:false}]);
});
it('delete requires its dedicated confirmation and lists scope accurately',async()=>{
 const calls:string[]=[];const invoke=async<T>(command:string):Promise<T>=>{calls.push(command);return (command==='computer_history_clear'?2:base()) as T;};
 dispose=wireHistory(document,invoke);await flush();expect(get<HTMLButtonElement>('history-delete').disabled).toBe(true);get<HTMLInputElement>('history-delete-confirm').checked=true;get('history-delete-confirm').dispatchEvent(new Event('change'));get('history-delete').click();await flush();expect(calls).toContain('computer_history_clear');expect(get<HTMLInputElement>('history-delete-confirm').checked).toBe(false);
 expect(historyText({...base(),enabled:true,paused:true})).toContain('paused');
 expect(historySettings({app:'observe',url:'do_not_observe',allowApps:'app.one\napp.one',blockApps:'private.app',allowUrls:'example.com',blockUrls:''}).observation.allowlist).toEqual([{scope:'app',bundleID:'app.one'},{scope:'url',urlDomain:'example.com'}]);
});
it('content scope needs fresh approval and keeps the displayed revision during polling',async()=>{
 let state={...base(),semantic_supported:true};const calls:unknown[]=[];
 const invoke=async<T>(command:string,args?:Record<string,unknown>):Promise<T>=>{calls.push([command,args]);if(command==='computer_history_capture_mode')throw new Error('History changed; reload');return structuredClone(state) as T;};
 dispose=wireHistory(document,invoke);await flush();const scope=get<HTMLSelectElement>('history-scope');scope.value='values';scope.dispatchEvent(new Event('change'));expect(get<HTMLButtonElement>('history-scope-save').disabled).toBe(true);
 get<HTMLInputElement>('history-scope-consent').checked=true;get('history-scope-consent').dispatchEvent(new Event('change'));expect(get<HTMLButtonElement>('history-scope-save').disabled).toBe(false);
 state={...state,revision:9};await vi.advanceTimersByTimeAsync(2000);expect(scope.value).toBe('values');get('history-scope-save').click();await flush();expect(calls).toContainEqual(['computer_history_capture_mode',{mode:'values',approved:true,generation:'fixture-generation',revision:3}]);expect(get('history-message').textContent).toContain('reload');
 scope.value='semantic';scope.dispatchEvent(new Event('change'));expect(get<HTMLInputElement>('history-scope-consent').checked).toBe(false);get('history-reload').click();await flush();expect(scope.value).toBe('application_metadata');
});
it('unsupported platforms cannot select rich capture',async()=>{
 dispose=wireHistory(document,async<T>()=>({...base(),semantic_supported:false}) as T);await flush();expect(get<HTMLSelectElement>('history-scope').disabled).toBe(true);expect(get<HTMLButtonElement>('history-scope-save').disabled).toBe(true);
});
it('native interaction capture is a broader scope requiring fresh consent',async()=>{
 const state={...base(),semantic_supported:true,capture_mode:'values'};const calls:unknown[]=[];
 dispose=wireHistory(document,async<T>(command:string,args?:Record<string,unknown>)=>{calls.push([command,args]);return state as T;});await flush();
 get<HTMLSelectElement>('history-scope').value='interaction_events';get('history-scope').dispatchEvent(new Event('change'));
 expect(get<HTMLButtonElement>('history-scope-save').disabled).toBe(true);
 get<HTMLInputElement>('history-scope-consent').checked=true;get('history-scope-consent').dispatchEvent(new Event('change'));get('history-scope-save').click();await flush();
 expect(calls).toContainEqual(['computer_history_capture_mode',{mode:'interaction_events',approved:true,generation:'fixture-generation',revision:3}]);
 expect(historyText({...state,enabled:true,capturing:true,capture_mode:'interaction_events'})).toContain('Browser content is excluded');
});
it('Linux supports values while unavailable interaction scope stays disabled',async()=>{
 dispose=wireHistory(document,async<T>()=>({...base(),semantic_supported:true,interaction_supported:false}) as T);await flush();
 const scope=get<HTMLSelectElement>('history-scope');expect(scope.disabled).toBe(false);
 expect(Array.from(scope.options).find(option=>option.value==='interaction_events')?.disabled).toBe(true);
 expect(Array.from(scope.options).find(option=>option.value==='values')?.disabled).toBe(false);
});
it('browser capture has independent fresh consent and a separate OS permission action',async()=>{
 let state={...base(),browser_capture:false,browser_capture_supported:true};const calls:unknown[]=[];
 const invoke=async<T>(command:string,args?:Record<string,unknown>):Promise<T>=>{calls.push([command,args]);if(command==='computer_history_browser_capture'){state={...state,browser_capture:Boolean(args?.enabled),revision:4};}return structuredClone(state) as T;};
 dispose=wireHistory(document,invoke);await flush();expect(get<HTMLButtonElement>('history-browser-permission').disabled).toBe(true);
 get<HTMLInputElement>('history-browser-enabled').checked=true;get('history-browser-enabled').dispatchEvent(new Event('change'));expect(get<HTMLButtonElement>('history-browser-save').disabled).toBe(true);
 get<HTMLInputElement>('history-browser-consent').checked=true;get('history-browser-consent').dispatchEvent(new Event('change'));get('history-browser-save').click();await flush();
 expect(calls).toContainEqual(['computer_history_browser_capture',{enabled:true,approved:true,generation:'fixture-generation',revision:3}]);expect(get<HTMLInputElement>('history-browser-consent').checked).toBe(false);
 get('history-browser-permission').click();await flush();expect(calls).toContainEqual(['computer_history_browser_permission',{application:'com.google.Chrome'}]);
 get<HTMLInputElement>('history-browser-enabled').checked=false;get('history-browser-enabled').dispatchEvent(new Event('change'));expect(get<HTMLButtonElement>('history-browser-save').disabled).toBe(false);get('history-browser-save').click();await flush();expect(state.browser_capture).toBe(false);
});
it('browser consent preserves its displayed revision across external updates',async()=>{
 let state={...base(),browser_capture:false,browser_capture_supported:true};let seen:Record<string,unknown>|undefined;
 const invoke=async<T>(command:string,args?:Record<string,unknown>):Promise<T>=>{if(command==='computer_history_browser_capture'){seen=args;throw new Error('History changed; reload');}return structuredClone(state) as T;};
 dispose=wireHistory(document,invoke);await flush();get<HTMLInputElement>('history-browser-enabled').checked=true;get('history-browser-enabled').dispatchEvent(new Event('change'));state={...state,revision:12};await vi.advanceTimersByTimeAsync(2000);
 get<HTMLInputElement>('history-browser-consent').checked=true;get('history-browser-consent').dispatchEvent(new Event('change'));get('history-browser-save').click();await flush();expect(seen?.revision).toBe(3);expect(get('history-message').textContent).toContain('reload');
});
it('Linux browser consent omits the macOS Automation action and shows its supported scope',async()=>{
 const state={...base(),browser_capture:true,browser_capture_supported:true,browser_permission_required:false,browser_capture_notice:'English Chrome and Chromium deb/rpm; Firefox excluded.'};
 const invoke=async<T>():Promise<T>=>structuredClone(state) as T;
 dispose=wireHistory(document,invoke);await flush();
 expect(get('history-browser-automation').hidden).toBe(true);
 expect(get<HTMLButtonElement>('history-browser-permission').disabled).toBe(true);
 expect(get('history-browser-notice').textContent).toContain('Firefox excluded');
 expect(get('history-browser-status').textContent).toContain('accessibility and website rules');
});
it('unchanged status polling preserves the focused review button',async()=>{
 const state={...base(),enabled:true,paused:true,recent_activity_files:['/fixture/events.jsonl']};
 dispose=wireHistory(document,async<T>()=>structuredClone(state) as T);await flush();
 get('history-button').click();const review=document.querySelector<HTMLButtonElement>('#history-files button')!;review.focus();
 await vi.advanceTimersByTimeAsync(6000);
 expect(document.querySelector('#history-files button')).toBe(review);expect(document.activeElement).toBe(review);
 review.click();expect(get('history-panel').hidden).toBe(true);expect(get('memory-pop').hidden).toBe(false);
 get('history-button').click();expect(get('history-panel').hidden).toBe(false);expect(get('memory-pop').hidden).toBe(true);expect(document.activeElement).toBe(get('history-panel'));
});
it('deleting history removes its cached value review when Memory is revisited',async()=>{
 let state={...base(),enabled:true,paused:true,recent_activity_files:['/history/activity.jsonl']};
 const preview:RecordingPreview={source_format:'computer-history-v1',includes_values:true,recording_sha256:'a'.repeat(64),manifest_sha256:'b'.repeat(64),apps:{'test.app':1},steps:{'test.app':[{source_line:1,action:'Observed value PRIVATE_DELETED_VALUE'}]},semantic_events:1,secure_events_omitted:0,has_values:true,memory_policy:'Reviewed values'};
 const invoke=async<T>(command:string):Promise<T>=>{
  if(command==='trajectory_preview')return preview as T;
  if(command==='computer_history_clear'){state={...state,recent_activity_files:[]};return 1 as T;}
  return structuredClone(state) as T;
 };
 wireTrajectoryImport(get('trajectory-tools'),invoke,async()=>{});dispose=wireHistory(document,invoke);await flush();
 get('history-button').click();document.querySelector<HTMLButtonElement>('#history-files button')!.click();await flush();
 expect(get('trajectory-preview-steps').textContent).toContain('PRIVATE_DELETED_VALUE');
 get('trajectory-back').click();get<HTMLInputElement>('history-delete-confirm').checked=true;get('history-delete-confirm').dispatchEvent(new Event('change'));get('history-delete').click();await flush();
 expect(get('trajectory-preview-steps').textContent).not.toContain('PRIVATE_DELETED_VALUE');
 expect(get('trajectory-options').hidden).toBe(true);expect(get<HTMLButtonElement>('trajectory-import').disabled).toBe(true);
});
