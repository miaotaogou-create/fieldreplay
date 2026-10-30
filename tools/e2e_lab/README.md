# E2E lab fixtures

See `docs/requirements/20261030-e2e-lab.md`. One step at a time.

Batch files are **ASCII-only** (cmd breaks on UTF-8 Chinese comments).

## E2E-1: RTSP fake source

1. Run `01_start_mediamtx.bat` (keep open)
2. Run `01_push_testpattern.bat` (keep open)
3. Run `01_ffplay_preview.bat` — expect moving color bars
4. Close preview only; keep MediaMTX + push for later recording

Fail checks: port `8554` free; `C:\01_Install\bin\ffplay.exe` exists.

## E2E-2: UDP fake status

1. Run `02_start_udp_send.bat` (keep open) — sends `STAT,...` to `127.0.0.1:56001`
2. Run `02_run_udp_recv.bat` — should print several `recv #n ... STAT,...` and `PASS`
3. Tell the guide "UDP OK" when PASS

RTSP windows from E2E-1 can stay open; not required for this step.

## E2E-3: TCP fake laser

**Order differs from UDP:** listen first (TCP needs accept).

1. Run `03_run_tcp_recv.bat` FIRST — listens `127.0.0.1:57002`
2. Within a few seconds, run `03_start_tcp_send.bat` (keep open) — sends `LASER,...` lines
3. Recv window should print several `recv #n : LASER,...` and `PASS`
4. Tell the guide "TCP OK" when PASS

Port `57002` (not `56xxx`): Hyper-V often reserves `56xxx` for TCP listen.
UDP E2E-2 can stay open (`56001`).
