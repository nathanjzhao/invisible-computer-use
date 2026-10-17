# Golden: refusals the model sees (Codex/Sky `ServerErrorCode`)

Sources:

- The no-login probe of the real service,
  `study/audit/golden-trace-prep-2026-09-25/validation/probe/`. `ipc:N` is line N of
  `sky-ipc.jsonl`, and `res:KEY` is `results.json[KEY]`.
- The decompiled enum, spec 19. `ServerErrorCode` is one flat enum, -10000..-10020.

## Observed in the capture

| Number | Message the model sees (verbatim) | When | Evidence |
| --- | --- | --- | --- |
| -10010 | `Invalid app: dev.icu.golden.does-not-exist` | an app id matching no app. It fails at the policy lookup (`ComputerUseIPCAppPolicyRequest`), before any approval prompt. | ipc:49-50, res:error_unknown_app |
| -10005 | `Computer Use server error -10005: 99999 is an invalid element ID` | an element index the latest tree does not show | ipc:53-54, res:error_bad_index |
| -10005 | `Computer Use server error -10005: NoSuchAction is not a valid secondary action for 1` | an action the element does not list | ipc:57-58, res:error_bad_secondary_action |

The JS rejects with an `Error` whose `message` is the IPC error message, and the model
reads that message. For -10005 the service itself prefixes
`Computer Use server error -10005: `. For -10010 it does not. The member names are not
in the capture. Spec 19 lists `invalidApp` among the infra members, and the message
matches it. The name of -10005 is not recovered.

## From the decompile (spec 19), not yet observed

The model-relevant members are appNotAllowed -10006, runningApplicationNotFound -10007,
accessibilityError -10008, permissionsNotGranted -10009, userStoppedSession -10012,
incompatibleClientVersion -10013, permissionsPending -10014, blockedURL -10015,
userIntervened -10016, ambiguousApp -10018 and screenLocked -10020. The transport error is
SkyComputerUseTransportError. The message text for these is not captured.

## Our mapping (`impl/icu/src/lib.rs`)

Our typed `code` always stays in `structuredContent`. Where the meaning matches Sky's, the
model-facing message is Sky's verbatim and `number` carries Sky's number.

| Situation | Our `code` | `number` | Message | Match |
| --- | --- | --- | --- | --- |
| `element_index` absent from the latest tree | `staleElementIndex` | -10005 | `Computer Use server error -10005: <id> is an invalid element ID` | verbatim |
| `element_index` shown, but a container or text our driver cannot act on | `staleElementIndex` | none | ours | no Sky counterpart: Sky acts on any shown id |
| `perform_secondary_action` with a label the row does not list (case-insensitive) | `invalidSecondaryAction` | -10005 | `Computer Use server error -10005: <action> is not a valid secondary action for <id>` | verbatim, refused before dispatch |
| `launch_app` for an app the driver finds no install of (`APP_NOT_INSTALLED`) | `invalidApp` | -10010 | `Invalid app: <app>` | verbatim. The driver's code and message ride along as `driver_code` / `driver_message`. |
| a window id that is not open | `runningApplicationNotFound` | -10007 | ours | window2 addresses windows, so Sky's app-level `Invalid app` has no exact counterpart here |
| the others above | the enum name | its number | ours | numbers from spec 19, messages ours |

Structured refusals return before dispatch. A successful driver response is never
evidence that input was delivered.
