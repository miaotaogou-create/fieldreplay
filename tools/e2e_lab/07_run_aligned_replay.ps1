# E2E-7: open E2E-5 .fcap via control WS, multi-target replay into fake hosts.
# ASCII-only comments (cmd/ps1 lab habit). Does not change product code.
param(
  # Prefer ASCII path; empty = newest *.fcap under Downloads (avoids console encoding of CJK names).
  [string]$Fcap = "",
  [double]$Speed = 8,
  [int]$ServePort = 17907,
  [int]$UdpPort = 56101,
  [int]$TcpPort = 58102,
  [string]$MqttBroker = "127.0.0.1:1883",
  [int]$MqttSubSeconds = 20
)
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($Fcap)) {
  $hit = Get-ChildItem -LiteralPath "$env:USERPROFILE\Downloads" -Filter "*.fcap" -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
  if (-not $hit) { throw "FAIL: no .fcap under Downloads; pass -Fcap <path>" }
  $Fcap = $hit.FullName
}

function Resolve-Engine {
  $candidates = @(
    (Join-Path $PSScriptRoot "..\..\engine\target\release\fieldreplay-engine.exe"),
    (Join-Path $PSScriptRoot "..\..\engine\target\debug\fieldreplay-engine.exe"),
    (Join-Path $PSScriptRoot "..\..\pack\FieldReplay\fieldreplay-engine.exe")
  ) | ForEach-Object { [IO.Path]::GetFullPath($_) } | Where-Object { Test-Path $_ }
  if (-not $candidates) { throw "FAIL: fieldreplay-engine.exe not found" }
  ($candidates | ForEach-Object { Get-Item $_ } | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
}

function Stop-PidSafe([int]$ProcessId) {
  if ($ProcessId -le 0) { return }
  Stop-Process -Id $ProcessId -Force -ErrorAction SilentlyContinue
}

function Wait-TcpOpen([string]$HostName, [int]$Port, [int]$TimeoutMs = 8000) {
  $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
  while ([DateTime]::UtcNow -lt $deadline) {
    try {
      $c = New-Object System.Net.Sockets.TcpClient
      $iar = $c.BeginConnect($HostName, $Port, $null, $null)
      if ($iar.AsyncWaitHandle.WaitOne(200) -and $c.Connected) {
        $c.Close()
        return
      }
      $c.Close()
    } catch {}
    Start-Sleep -Milliseconds 100
  }
  throw "FAIL: TCP ${HostName}:${Port} not open within ${TimeoutMs}ms"
}

function New-WsClient([string]$HostName, [int]$Port) {
  $ws = [System.Net.WebSockets.ClientWebSocket]::new()
  $uri = [Uri]"ws://${HostName}:${Port}/"
  $cts = [Threading.CancellationTokenSource]::new()
  $cts.CancelAfter(8000)
  # Suppress void Task result so caller gets only ClientWebSocket.
  [void]$ws.ConnectAsync($uri, $cts.Token).GetAwaiter().GetResult()
  ,$ws
}

function Send-WsText([System.Net.WebSockets.ClientWebSocket]$Ws, [string]$Text) {
  $bytes = [Text.Encoding]::UTF8.GetBytes($Text)
  $seg = [ArraySegment[byte]]::new($bytes)
  [void]$Ws.SendAsync($seg, [System.Net.WebSockets.WebSocketMessageType]::Text, $true, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
}

function Recv-WsText([System.Net.WebSockets.ClientWebSocket]$Ws, [int]$TimeoutMs = 15000) {
  $buf = New-Object byte[] 65536
  $ms = [IO.MemoryStream]::new()
  $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
  while ($true) {
    $left = [int]($deadline - [DateTime]::UtcNow).TotalMilliseconds
    if ($left -le 0) { throw "FAIL: WS recv timeout" }
    $cts = [Threading.CancellationTokenSource]::new()
    $cts.CancelAfter($left)
    $seg = [ArraySegment[byte]]::new($buf)
    $r = $Ws.ReceiveAsync($seg, $cts.Token).GetAwaiter().GetResult()
    if ($r.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) {
      throw "FAIL: WS closed by peer"
    }
    $ms.Write($buf, 0, $r.Count)
    if ($r.EndOfMessage) {
      return [Text.Encoding]::UTF8.GetString($ms.ToArray())
    }
  }
}

function Wait-WsId([System.Net.WebSockets.ClientWebSocket]$Ws, [string]$Id, [int]$TimeoutMs = 20000) {
  $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
  while ([DateTime]::UtcNow -lt $deadline) {
    $left = [int]($deadline - [DateTime]::UtcNow).TotalMilliseconds
    $msg = Recv-WsText $Ws $left
    if ($msg -match ('"id"\s*:\s*"' + [regex]::Escape($Id) + '"')) {
      return $msg
    }
    # ignore pushes (replay.progress / done) until matching id, but keep last push around
    if ($msg -match '"cmd"\s*:\s*"replay\.done"' -or $msg -match '"event"\s*:\s*"replay\.done"') {
      $script:SawReplayDone = $true
    }
  }
  throw "FAIL: no WS response for id=$Id"
}

$engine = Resolve-Engine
Write-Host "[E2E-7] engine=$engine"
Write-Host "[E2E-7] fcap=$Fcap speed=$Speed"

if (-not (Test-Path -LiteralPath $Fcap)) {
  throw "FAIL: fcap missing: $Fcap"
}

# Copy to ASCII temp path to avoid JSON escape issues with non-ASCII names.
$workDir = Join-Path $env:TEMP "fieldreplay_e2e7"
New-Item -ItemType Directory -Force -Path $workDir | Out-Null
$fcapWork = Join-Path $workDir "e2e5_session.fcap"
$srcFull = [IO.Path]::GetFullPath($Fcap)
$dstFull = [IO.Path]::GetFullPath($fcapWork)
if ($srcFull -ne $dstFull) {
  Copy-Item -LiteralPath $srcFull -Destination $dstFull -Force
}

$udpLog = Join-Path $workDir "udp_host.log"
$udpErr = Join-Path $workDir "udp_host.err"
$tcpLog = Join-Path $workDir "tcp_host.log"
$tcpErr = Join-Path $workDir "tcp_host.err"
$mqttLog = Join-Path $workDir "mqtt_host.log"
$mqttErr = Join-Path $workDir "mqtt_host.err"
$brokerLog = Join-Path $workDir "broker.log"
$brokerErr = Join-Path $workDir "broker.err"
$serveLog = Join-Path $workDir "serve.log"
$serveErr = Join-Path $workDir "serve.err"
Remove-Item $udpLog, $udpErr, $tcpLog, $tcpErr, $mqttLog, $mqttErr, $brokerLog, $brokerErr, $serveLog, $serveErr -ErrorAction SilentlyContinue

$procs = @()
$script:SawReplayDone = $false

try {
  # Broker
  $broker = Start-Process -FilePath $engine -ArgumentList @("lab-mqtt-broker", "--bind", "127.0.0.1:1883") `
    -PassThru -WindowStyle Hidden -RedirectStandardOutput $brokerLog -RedirectStandardError $brokerErr
  $procs += $broker
  Wait-TcpOpen "127.0.0.1" 1883 10000
  Write-Host "[E2E-7] MQTT broker up"

  # Fake hosts (longer windows; replay ~ duration/speed)
  $hostPs = Join-Path $PSScriptRoot "06_udp_host_recv.ps1"
  $tcpPs = Join-Path $PSScriptRoot "06_tcp_host_recv.ps1"
  $udpHost = Start-Process -FilePath "powershell.exe" -PassThru -WindowStyle Hidden `
    -ArgumentList @(
      "-NoProfile", "-ExecutionPolicy", "Bypass",
      "-File", $hostPs,
      "-BindHost", "127.0.0.1", "-Port", "$UdpPort", "-Seconds", "25"
    ) -RedirectStandardOutput $udpLog -RedirectStandardError $udpErr
  $procs += $udpHost

  $tcpHost = Start-Process -FilePath "powershell.exe" -PassThru -WindowStyle Hidden `
    -ArgumentList @(
      "-NoProfile", "-ExecutionPolicy", "Bypass",
      "-File", $tcpPs,
      "-BindHost", "127.0.0.1", "-Port", "$TcpPort", "-Seconds", "25"
    ) -RedirectStandardOutput $tcpLog -RedirectStandardError $tcpErr
  $procs += $tcpHost
  Start-Sleep -Milliseconds 400

  $mqttHost = Start-Process -FilePath $engine -PassThru -WindowStyle Hidden `
    -ArgumentList @(
      "lab-mqtt-sub", "--mqtt", $MqttBroker, "--topic", "e2e/#", "--seconds", "$MqttSubSeconds"
    ) -RedirectStandardOutput $mqttLog -RedirectStandardError $mqttErr
  $procs += $mqttHost
  Write-Host "[E2E-7] fake hosts listening (UDP $UdpPort / TCP $TcpPort / MQTT e2e/#)"

  # Engine serve on dedicated port (avoid clobbering a running UI pack)
  $serve = Start-Process -FilePath $engine -PassThru -WindowStyle Hidden `
    -ArgumentList @("serve", "--bind", "127.0.0.1:$ServePort") `
    -RedirectStandardOutput $serveLog -RedirectStandardError $serveErr
  $procs += $serve
  Wait-TcpOpen "127.0.0.1" $ServePort 10000
  Write-Host "[E2E-7] serve 127.0.0.1:$ServePort"

  $ws = New-WsClient "127.0.0.1" $ServePort
  try {
    $pathJson = ($fcapWork -replace '\\', '/')
    $openMsg = @{ id = "e2e7-open"; cmd = "session.open"; path = $pathJson } | ConvertTo-Json -Compress
    Send-WsText $ws $openMsg
    $opened = Wait-WsId $ws "e2e7-open" 15000
    if ($opened -notmatch '"ok"\s*:\s*true') {
      throw "FAIL: session.open: $opened"
    }
    Write-Host "[E2E-7] session.open ok"

    $startObj = @{
      id = "e2e7-start"
      cmd = "replay.start"
      speed = $Speed
      targets = @(
        @{
          ch = 0
          type = "udp"
          config = @{ target_ip = "127.0.0.1"; target_port = $UdpPort }
        },
        @{
          ch = 1
          type = "mqtt"
          config = @{ broker = "127.0.0.1"; port = 1883; topic_prefix = "" }
        },
        @{
          ch = 3
          type = "tcp"
          config = @{ host = "127.0.0.1"; port = $TcpPort }
        }
      )
    }
    $startMsg = $startObj | ConvertTo-Json -Compress -Depth 6
    Send-WsText $ws $startMsg
    $started = Wait-WsId $ws "e2e7-start" 15000
    if ($started -notmatch '"ok"\s*:\s*true' -or $started -notmatch '"started"\s*:\s*true') {
      throw "FAIL: replay.start: $started"
    }
    Write-Host "[E2E-7] replay.start ok (targets UDP+MQTT+TCP, skip video)"

    # Drain until done / timeout (~ duration/speed + slack)
    $waitMs = [int]((46429430.0 / $Speed / 1000.0) + 8000)
    if ($waitMs -lt 12000) { $waitMs = 12000 }
    $deadline = [DateTime]::UtcNow.AddMilliseconds($waitMs)
    while ([DateTime]::UtcNow -lt $deadline -and -not $script:SawReplayDone) {
      $left = [int]($deadline - [DateTime]::UtcNow).TotalMilliseconds
      if ($left -le 0) { break }
      try {
        $msg = Recv-WsText $ws ([Math]::Min($left, 2000))
        if ($msg -match 'replay\.done' -or $msg -match '"done"\s*:\s*true') {
          $script:SawReplayDone = $true
          Write-Host "[E2E-7] replay.done"
        }
      } catch {
        # brief idle is fine
      }
    }
    if (-not $script:SawReplayDone) {
      Send-WsText $ws '{"id":"e2e7-stop","cmd":"replay.stop"}'
      try { Wait-WsId $ws "e2e7-stop" 5000 | Out-Null } catch {}
      Write-Host "[E2E-7] WARN: no replay.done; sent stop"
    }
  } finally {
    if ($null -ne $ws) {
      try {
        [void]$ws.CloseAsync(
          [System.Net.WebSockets.WebSocketCloseStatus]::NormalClosure,
          "bye",
          [Threading.CancellationToken]::None
        ).Wait(2000)
      } catch {}
      try { $ws.Dispose() } catch {}
    }
  }

  # Let hosts flush
  Start-Sleep -Seconds 2
  foreach ($p in @($udpHost, $tcpHost, $mqttHost)) {
    if (-not $p.HasExited) {
      Wait-Process -Id $p.Id -Timeout 20 -ErrorAction SilentlyContinue
    }
  }

  $udpText = if (Test-Path $udpLog) { Get-Content -LiteralPath $udpLog -Raw } else { "" }
  $tcpText = if (Test-Path $tcpLog) { Get-Content -LiteralPath $tcpLog -Raw } else { "" }
  $mqttText = if (Test-Path $mqttLog) { Get-Content -LiteralPath $mqttLog -Raw } else { "" }

  Write-Host "---- UDP host ----"
  Write-Host ($udpText.Substring(0, [Math]::Min(800, $udpText.Length)))
  Write-Host "---- TCP host ----"
  Write-Host ($tcpText.Substring(0, [Math]::Min(800, $tcpText.Length)))
  Write-Host "---- MQTT host ----"
  Write-Host ($mqttText.Substring(0, [Math]::Min(800, $mqttText.Length)))

  $udpOk = ($udpText -match 'STAT,') -and ($udpText -match 'PASS')
  $tcpOk = ($tcpText -match 'LASER,') -and ($tcpText -match 'PASS')
  $mqttOk = ($mqttText -match 'SENSOR,') -and (($mqttText -match 'PASS') -or ([regex]::Matches($mqttText, 'SENSOR,').Count -ge 3))

  Write-Host "[E2E-7] check UDP=$udpOk TCP=$tcpOk MQTT=$mqttOk"
  if (-not ($udpOk -and $tcpOk -and $mqttOk)) {
    Write-Host "FAIL: aligned replay did not satisfy all three hosts"
    Write-Host "logs: $workDir"
    exit 1
  }
  Write-Host "PASS"
  exit 0
}
finally {
  foreach ($p in $procs) {
    if ($null -ne $p -and -not $p.HasExited) { Stop-PidSafe $p.Id }
  }
  # orphan listeners on lab ports
  Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
    Where-Object {
      $_.CommandLine -and (
        $_.CommandLine -match 'lab-mqtt-broker' -or
        $_.CommandLine -match "serve --bind 127\.0\.0\.1:$ServePort" -or
        $_.CommandLine -match '06_udp_host_recv' -or
        $_.CommandLine -match '06_tcp_host_recv' -or
        $_.CommandLine -match 'lab-mqtt-sub'
      )
    } |
    ForEach-Object { Stop-PidSafe $_.ProcessId }
}
