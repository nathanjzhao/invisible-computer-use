/** Adapt an initialized MCP client's callTool(name, arguments) to the audio API. */
export function createAudioClient(callTool) {
  if (typeof callTool !== 'function') throw new TypeError('An initialized MCP callTool function is required');
  async function call(name, args) {
    const result = await callTool(name, args);
    if (result?.isError) throw new Error((result.content ?? []).filter(x => x.type === 'text').map(x => x.text).join('\n') || 'Audio operation failed');
    return result?.structuredContent ?? result;
  }
  return Object.freeze({
    async start_audio_recording(options = {}) {
      if (options === null || typeof options !== 'object' || Array.isArray(options)) throw new TypeError('Audio arguments must be an object');
      const milliseconds = options.max_duration_ms ?? 60000;
      // Validate before JSON encoding, which would turn NaN and Infinity into null.
      if (!Number.isInteger(milliseconds) || milliseconds < 100 || milliseconds > 300000) {
        throw new Error('audio recording duration must be an integer from 100 through 300000');
      }
      await call('start_audio_recording', { max_duration_ms: milliseconds });
    },
    async stop_audio_recording() {
      const result = await call('stop_audio_recording', {});
      const prefix = 'data:audio/wav;base64,';
      if (typeof result?.filepath !== 'string' || !result.filepath || typeof result.data_url !== 'string' || !result.data_url.startsWith(prefix)) {
        throw new Error('Audio service did not return a local WAV result');
      }
      const encoded = result.data_url.slice(prefix.length);
      if (encoded.length > 44 * 1024 * 1024 || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(encoded)) {
        throw new Error('Audio service returned invalid WAV bytes');
      }
      const binary = atob(encoded);
      const bytes = Uint8Array.from(binary, char => char.charCodeAt(0));
      if (bytes.length < 44 || binary.slice(0, 4) !== 'RIFF' || binary.slice(8, 12) !== 'WAVE') {
        throw new Error('Audio service returned a non-WAV file');
      }
      return { filepath: result.filepath, bytes, data_url: result.data_url };
    },
  });
}
