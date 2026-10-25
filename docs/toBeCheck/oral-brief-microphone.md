# OralBrief — a real-microphone take through Phonon

## Context

Task #552. OralBrief sends the brief as `dictation` (base64 audio) when the DM
told it at the mic and typed nothing; `ai.analyzeBrief` has the
`speech-to-text` port transcribe it, and only the text reaches the llm port.
The shell (`apps/shell/src/client/dictation.ts`) records with `MediaRecorder`.

What was run: the shell's recorder over stubs, the Micro-UI over a fake shell,
and the journey on the server's fake (`PORTS=fake`), where a browser with no
microphone hands the *no-device take* (the bytes `OralBrief`) and the server's
fake hears the OralBrief fixture's brief in it.

## Not exercised

A real microphone take, recorded by a browser and transcribed by the Phonon
adapter. Nothing here shows that the container `MediaRecorder` produces (webm /
opus in Chromium, mp4 in Safari) is one the adapter decodes.

## Checklist

- [ ] In a browser with a microphone, click the mic, tell a short brief, then
      « Terminer et analyser »: the browser asks for the microphone once, and
      the brief is analysed into scenes.
- [ ] The command's payload holds `dictation` and no `text`; the llm port's
      journal holds the transcript only, never the audio.
- [ ] Refusing the microphone, or a browser without one: the no-device take is
      sent and the DM is told that no words were heard (`NoBrief`), not a
      silent failure.
- [ ] Pause then resume before analysing: only the last take is sent
      (the Micro-UI keeps one take). Decide whether the earlier one must be kept.
