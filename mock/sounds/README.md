# The sound library's files

The shell serves this folder at `/sounds/<file>` (`apps/shell/src/server/app.ts`),
which is where the soundbox's `LiveSoundbox` plays a sound from. One file for each
sound of `mock/fixtures/sounds.json`, named as its `file`, at least as long as its
`duration`: Ogg Opus, silent — placeholders for the rules runtime, not recordings.

A file is made, for a sound of `D` seconds, with:

    ffmpeg -f lavfi -i anullsrc=r=48000:cl=mono -t <D rounded up to 0.1 + 0.2> \
      -c:a libopus -b:a 6k -map_metadata -1 mock/sounds/<file>
