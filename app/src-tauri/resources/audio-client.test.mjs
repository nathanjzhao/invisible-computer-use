import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createAudioClient } from './audio-client.mjs';

test('rehydrates byte-for-byte WAV data without a second tool call', async () => {
  const buffer=Buffer.alloc(48);buffer.write('RIFF',0);buffer.writeUInt32LE(40,4);buffer.write('WAVE',8);
  const data_url='data:audio/wav;base64,'+buffer.toString('base64');const calls=[];
  const client=createAudioClient(async(name,args)=>{calls.push([name,args]);return {structuredContent:name==='stop_audio_recording'?{filepath:'/fixture/audio.wav',data_url}:{state:'recording'}};});
  assert.equal(await client.start_audio_recording({max_duration_ms:100}),undefined);
  const result=await client.stop_audio_recording();assert(result.bytes instanceof Uint8Array);assert.deepEqual(Buffer.from(result.bytes),buffer);
  assert.equal(result.data_url,data_url);assert.deepEqual(calls,[['start_audio_recording',{max_duration_ms:100}],['stop_audio_recording',{}]]);
});
test('propagates denied approval and never supplies its own acceptance',async()=>{
  const client=createAudioClient(async()=>({isError:true,content:[{type:'text',text:'Recording was not approved'}]}));
  await assert.rejects(client.start_audio_recording(),/not approved/);
});
test('rejects malformed or non-WAV transport data',async()=>{
  for(const data_url of ['data:audio/wav;base64,!!!','data:audio/png;base64,AAAA','data:audio/wav;base64,AAAA']) {
    const client=createAudioClient(async()=>({structuredContent:{filepath:'/fixture/audio.wav',data_url}}));
    await assert.rejects(client.stop_audio_recording());
  }
});
test('rejects non-finite and invalid duration values before JSON encoding',async()=>{
  const calls=[];const client=createAudioClient(async(...args)=>{calls.push(args);return {structuredContent:{state:'recording'}};});
  for(const max_duration_ms of [NaN,Infinity,-Infinity,99,300001,100.5,'100',true]) {
    await assert.rejects(client.start_audio_recording({max_duration_ms}),/integer from 100 through 300000/);
  }
  assert.equal(calls.length,0);
  await client.start_audio_recording({max_duration_ms:null});await client.start_audio_recording({max_duration_ms:100});
  assert.equal(calls.length,2);
});
test('sends the validated duration snapshot instead of rereading caller state',async()=>{
  let reads=0;const calls=[];const options={get max_duration_ms(){return reads++===0?100:Infinity;}};
  const client=createAudioClient(async(name,args)=>{calls.push([name,args]);JSON.stringify(args);return {structuredContent:{state:'recording'}};});
  await client.start_audio_recording(options);assert.equal(reads,1);
  assert.deepEqual(calls,[['start_audio_recording',{max_duration_ms:100}]]);
});
