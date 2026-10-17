// @vitest-environment jsdom
import {afterEach,expect,it,vi} from 'vitest';
import {wireAudio} from './audio-ui';
afterEach(()=>{vi.clearAllTimers();vi.useRealTimers();});
it('source changes invalidate consent and an active recording remains visible',async()=>{
 vi.useFakeTimers();
 document.body.innerHTML=markup;
 const calls:unknown[]=[];
 const invoke=async<T>(command:string,args?:Record<string,unknown>):Promise<T>=>{calls.push([command,args]);return (command==='audio_start'?{state:'recording',system:true}:{state:'idle'}) as T;};
 wireAudio(document,invoke);await Promise.resolve();
 const get=(id:string)=>document.getElementById(id) as HTMLInputElement;
 get('audio-system').checked=true;get('audio-system').dispatchEvent(new Event('input'));
 get('audio-consent').checked=true;get('audio-consent').dispatchEvent(new Event('change'));
 expect(get('audio-start').disabled).toBe(false);
 get('audio-microphone').checked=true;get('audio-microphone').dispatchEvent(new Event('input'));
 expect(get('audio-consent').checked).toBe(false);expect(get('audio-start').disabled).toBe(true);
 get('audio-microphone').checked=false;get('audio-microphone').dispatchEvent(new Event('input'));
 get('audio-consent').checked=true;get('audio-consent').dispatchEvent(new Event('change'));get('audio-start').click();await Promise.resolve();
 expect(calls).toContainEqual(['audio_start',{request:{microphone:false,system:true,seconds:60,consent:true}}]);
 expect(get('audio-button').textContent).toContain('Recording audio');expect(get('audio-consent').checked).toBe(false);
 expect(get('audio-start').disabled).toBe(true);expect(get('audio-stop').disabled).toBe(false);
});

const markup='<button id="audio-button"></button><div id="audio-panel" hidden></div><input id="audio-microphone" type="checkbox"><input id="audio-system" type="checkbox"><input id="audio-consent" type="checkbox"><input id="audio-seconds" type="number" min="0.1" max="3600" step="0.001" value="60"><button id="audio-start"></button><button id="audio-stop"></button><button id="audio-export"></button><p id="audio-status"></p>';
it('exports completed audio without starting capture or reusing recording consent',async()=>{
 vi.useFakeTimers();document.body.innerHTML=markup;
 const calls:string[]=[];let state='idle';
 const invoke=async<T>(command:string):Promise<T>=>{calls.push(command);return (command==='audio_export_wav'?{directory:'/recording/wav',files:[{source:'system'}]}:{state}) as T;};
 wireAudio(document,invoke);await Promise.resolve();
 const button=document.getElementById('audio-export') as HTMLButtonElement;
 expect(button.disabled).toBe(true);button.click();expect(calls).not.toContain('audio_export_wav');
 state='saved';await vi.advanceTimersByTimeAsync(1000);expect(button.disabled).toBe(false);
 button.click();await Promise.resolve();expect(calls.filter(c=>c==='audio_export_wav')).toHaveLength(1);
 expect(calls).not.toContain('audio_start');expect(document.getElementById('audio-status')?.textContent).toContain('Saved 1 WAV file');
 expect((document.getElementById('audio-consent') as HTMLInputElement).checked).toBe(false);
 state='recording';await vi.advanceTimersByTimeAsync(1000);expect(button.disabled).toBe(true);
});
it('admits a 100 ms recording only with fresh source consent',async()=>{
 vi.useFakeTimers();document.body.innerHTML=markup;const calls:unknown[]=[];
 const invoke=async<T>(command:string,args?:Record<string,unknown>):Promise<T>=>{calls.push([command,args]);return {state:'idle'} as T;};
 wireAudio(document,invoke);await Promise.resolve();
 const input=(id:string)=>document.getElementById(id) as HTMLInputElement;
 input('audio-system').checked=true;input('audio-system').dispatchEvent(new Event('input'));
 input('audio-seconds').value='0.1';input('audio-seconds').dispatchEvent(new Event('input'));
 expect(input('audio-start').disabled).toBe(true);
 input('audio-consent').checked=true;input('audio-consent').dispatchEvent(new Event('change'));
 expect(input('audio-start').disabled).toBe(false);input('audio-start').click();await Promise.resolve();
 expect(calls).toContainEqual(['audio_start',{request:{microphone:false,system:true,seconds:0.1,consent:true}}]);
 input('audio-seconds').value='0.099';input('audio-seconds').dispatchEvent(new Event('input'));
 input('audio-consent').checked=true;input('audio-consent').dispatchEvent(new Event('change'));expect(input('audio-start').disabled).toBe(true);
});
