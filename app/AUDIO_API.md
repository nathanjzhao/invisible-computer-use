# Explicit audio API

The installed executable exposes a stdio MCP server with `--icu-audio-api`.
This mode runs without the application window or History workers. It uses the
same audio service, supervisor and packaged recorder as the Audio panel.

Initialize with the MCP elicitation capability. Each `start_audio_recording`
requests approval to record system audio and return the result to the task.
The client must present that request to its authorized operator. Evaluated code
cannot approve a recording. Missing, declined, cancelled and expired approvals
start no sensor. Disconnect stops and finalizes an active recording.

`start_audio_recording({max_duration_ms})` accepts integer limits from 100 through
300000 milliseconds and defaults to 60000. The API records system audio only.
The Audio panel separately supports microphone and combined capture, with
explicit source consent and limits up to an hour.

`stop_audio_recording()` finalizes the recording and returns `filepath` and an
`audio/wav` data URL. The bundled `audio-client.mjs` adapter exposes the same
method names and adds a `Uint8Array` named `bytes`. Wire the adapter to an
initialized MCP client's `callTool` function. Native exports are 24 kHz stereo
PCM16. No network or model call is made by the recorder or adapter.

Audio defaults to the platform's local application-data folder. A test harness
may set `ICU_AUDIO_ROOT` to an absolute directory before starting this mode.
The tools accept no filesystem paths. Completed output remains local after the
connection ends; closing the connection does not delete recordings.
