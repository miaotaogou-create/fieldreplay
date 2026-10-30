# E2E-3: fake device — periodic TCP LASER lines to 127.0.0.1:56002
param(
  [string]$HostAddr = "127.0.0.1",
  [int]$Port = 57002,
  [int]$IntervalMs = 200,
  [int]$Count = 0,
  [int]$ConnectWaitSec = 30
)
$ErrorActionPreference = "Stop"
Write-Host "[E2E-3] TCP send -> ${HostAddr}:${Port} every ${IntervalMs}ms (Ctrl+C stop)"
$client = $null
$deadline = [DateTime]::UtcNow.AddSeconds($ConnectWaitSec)
while ($null -eq $client) {
  try {
    $client = New-Object System.Net.Sockets.TcpClient
    $client.Connect($HostAddr, $Port)
  } catch {
    if ($null -ne $client) { $client.Close(); $client = $null }
    if ([DateTime]::UtcNow -ge $deadline) {
      Write-Host "FAIL: connect timeout ${ConnectWaitSec}s - start 03_run_tcp_recv.bat first"
      exit 1
    }
    Start-Sleep -Milliseconds 300
  }
}
Write-Host "[E2E-3] connected"
$stream = $client.GetStream()
$n = 0
try {
  while ($true) {
    $n++
    $ts = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $line = "LASER,seq=$n,ts_ms=$ts,pwr=$((50 + ($n % 50))),ready=1`n"
    $bytes = [Text.Encoding]::UTF8.GetBytes($line)
    $stream.Write($bytes, 0, $bytes.Length)
    $stream.Flush()
    if (($n % 10) -eq 0) { Write-Host "sent $n : $($line.TrimEnd())" }
    if ($Count -gt 0 -and $n -ge $Count) { break }
    Start-Sleep -Milliseconds $IntervalMs
  }
} finally {
  $stream.Close()
  $client.Close()
  Write-Host "[E2E-3] stopped after $n packets"
}
