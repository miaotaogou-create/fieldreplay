# E2E-3: local sniff — listen TCP and print LASER lines (stand-in before FieldReplay)
param(
  [string]$BindHost = "127.0.0.1",
  [int]$Port = 57002,
  [int]$Seconds = 10
)
$ErrorActionPreference = "Stop"
$listener = New-Object System.Net.Sockets.TcpListener ([System.Net.IPAddress]::Parse($BindHost), $Port)
$listener.Start()
Write-Host "[E2E-3] TCP listen ${BindHost}:${Port} for ${Seconds}s (waiting accept)"
$listener.Server.ReceiveTimeout = 1000
$deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
$got = 0
$client = $null
$stream = $null
$buf = New-Object byte[] 4096
$pending = ""
try {
  while ([DateTime]::UtcNow -lt $deadline -and $null -eq $client) {
    if ($listener.Pending()) {
      $client = $listener.AcceptTcpClient()
      $stream = $client.GetStream()
      $stream.ReadTimeout = 1000
      Write-Host "[E2E-3] accepted from $($client.Client.RemoteEndPoint)"
    } else {
      Start-Sleep -Milliseconds 50
    }
  }
  if ($null -eq $client) {
    Write-Host "FAIL: no TCP connect within ${Seconds}s - start 03_start_tcp_send.bat"
    exit 1
  }
  while ([DateTime]::UtcNow -lt $deadline) {
    try {
      $n = $stream.Read($buf, 0, $buf.Length)
      if ($n -le 0) { Start-Sleep -Milliseconds 50; continue }
      $pending += [Text.Encoding]::UTF8.GetString($buf, 0, $n)
      while ($pending.Contains("`n")) {
        $idx = $pending.IndexOf("`n")
        $line = $pending.Substring(0, $idx).TrimEnd("`r")
        $pending = $pending.Substring($idx + 1)
        if ($line.Length -eq 0) { continue }
        $got++
        Write-Host "recv #$got : $line"
      }
    } catch [System.IO.IOException] {
      # read timeout — keep until deadline
    } catch [System.Net.Sockets.SocketException] {
      # read timeout — keep until deadline
    }
  }
} finally {
  if ($null -ne $stream) { $stream.Close() }
  if ($null -ne $client) { $client.Close() }
  $listener.Stop()
}
Write-Host "[E2E-3] done, lines=$got"
if ($got -lt 3) {
  Write-Host "FAIL: expected at least 3 LASER lines"
  exit 1
}
Write-Host "PASS"
exit 0
