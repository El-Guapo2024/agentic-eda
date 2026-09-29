# KiCad View (parked experiment)

Our own KiCad PCB Editor on its own invisible screen, streamed into the
side panel -- the way a simulator panel works. Built 2026-09-29 as a one-day
proof; parked in favour of our own React editor.

What works:
- A virtual display (CoreGraphics' private `CGVirtualDisplay`) that exists
  only in software.
- Our own `pcbnew`, launched from inside KiCad.app with private settings and
  documents folders (`KICAD_CONFIG_HOME`, `KICAD_DOCUMENTS_HOME`) and its
  window saved onto that display.
- ScreenCaptureKit streaming of that display to `viewer.html` at
  http://127.0.0.1:8770 (JPEG frames over a WebSocket on 8771).

Why it is parked:
- KiCad on macOS always draws with OpenGL (`EDA_DRAW_FRAME::
  loadCanvasTypeSetting` returns OpenGL under `__WXMAC__`, whatever the
  settings), and OpenGL fails on the virtual display (framebuffer error
  0x8219), so the board canvas never draws.
- A background Mac app ignores clicks posted to its process (`--input pid`);
  hardware-style events (`--input hid`) work but drag the user's real
  pointer to the invisible screen. The default is `pid`: never move the
  user's pointer.
- The app that has focus decides which screen new windows open on, so our
  KiCad taking focus put System Settings and permission prompts on the
  invisible screen. `guardFocus` hands focus back.

Build and run: `./build.sh`, then
`open build/KiCadView.app --args <board.kicad_pcb>`. It needs Screen Recording
and Accessibility, granted to the app itself (not a hand-added path entry).
Logs: `~/Library/Logs/KiCadView.log`.
