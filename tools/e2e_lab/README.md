# E2E lab fixtures

See `docs/requirements/20261030-e2e-lab.md`. One step at a time.

Batch files are **ASCII-only** (cmd breaks on UTF-8 Chinese comments).

## E2E-1: RTSP fake source

1. Run `01_start_mediamtx.bat` (keep open)
2. Run `01_push_testpattern.bat` (keep open)
3. Run `01_ffplay_preview.bat` — expect moving color bars
4. Close preview only; keep MediaMTX + push for later recording

Fail checks: port `8554` free; `C:\01_Install\bin\ffplay.exe` exists.
