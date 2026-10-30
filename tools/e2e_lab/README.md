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

## E2E-4: MQTT Broker + publish

Needs built engine (`engine/target/release` or `debug` `fieldreplay-engine.exe`).

**Order:** broker first, then subscribe window, then publisher (or pub before sub — both ok after broker).

1. Run `04_start_mqtt_broker.bat` (keep open) — listens `127.0.0.1:1883`
2. Run `04_start_mqtt_pub.bat` (keep open) — publishes `SENSOR,...` on `e2e/sensor`
3. Run `04_run_mqtt_sub.bat` — should print several `recv #n ... SENSOR,...` and `PASS`
4. Tell the guide "MQTT OK" when PASS

Uses engine mini-broker (no Mosquitto install). Plain MQTT, no TLS/auth (E15 later).

## E2E-5: FieldReplay multi-channel record + export .fcap

Goal: with fake sources running, FieldReplay records UDP+TCP+MQTT(+optional video)
into one session and downloads a real `.fcap`.

**Do NOT run** `02_run_udp_recv.bat` / `03_run_tcp_recv.bat` / `04_run_mqtt_sub.bat`
during this step — FieldReplay replaces those receivers.

### A. Fake sources (keep windows open)

1. `01_start_mediamtx.bat` then `01_push_testpattern.bat` (optional video)
2. `04_start_mqtt_broker.bat` then `04_start_mqtt_pub.bat`
3. `02_start_udp_send.bat` (STAT -> `127.0.0.1:56001`)
4. **TCP sender later** — start only AFTER FieldReplay is already recording
   (FR listens `:57002`; then `03_start_tcp_send.bat`)

### B. Launch app

- Prefer fresh pack: `tools\pack_green.bat` then
  `pack\FieldReplay\fieldreplay.exe`
- Or print this checklist: `05_print_record_steps.bat`

### C. Channel setup in UI (录制页)

Default demo channels use wrong ports / COM3 — **turn them all OFF**.
Then **添加自定义端口** for lab channels only:

| Name | Type | Param |
|------|------|-------|
| e2e-udp | UDP | `0.0.0.0:56001` |
| e2e-tcp | TCP | `0.0.0.0:57002` (engine role defaults to server) |
| e2e-mqtt | MQTT | `127.0.0.1:1883, e2e/sensor` (TLS off) |
| e2e-video | 视频 RTP | `rtsp://127.0.0.1:8554/live/cam1` (optional) |

Leave serial disabled (no COM needed).

### D. Record + export

1. Confirm UI shows engine connected
2. Start multi-channel record (all enabled channels)
3. Within a few seconds: `03_start_tcp_send.bat`
4. Watch packet list ~10–20s — expect `STAT,...` / `LASER,...` / `SENSOR,...`
   (and video frames if RTSP enabled)
5. Stop and export `.fcap` (engine path, not JSON demo)
6. Tell the guide "E2E-5 OK" with: channel count, rough packet count, file size

Fail checks: serial still enabled; TCP sender started before FR listen;
old `*_recv` / mqtt-sub still bound on the same ports; engine disconnected
(export falls back to JSON demo — not PASS).
