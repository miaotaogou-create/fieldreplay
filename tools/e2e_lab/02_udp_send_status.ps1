# E2E-2: fake device — periodic UDP status lines to 127.0.0.1:56001
param(
  [string]$HostAddr = "127.0.0.1",
  [int]$Port = 56001,
  [int]$IntervalMs = 200,
  [int]$Count = 0
)
$ErrorActionPreference = "Stop"
$udp = New-Object System.Net.Sockets.UdpClient
$ep = New-Object System.Net.IPEndPoint ([System.Net.IPAddress]::Parse($HostAddr), $Port)
Write-Host "[E2E-2] UDP send -> ${HostAddr}:${Port} every ${IntervalMs}ms (Ctrl+C stop)"
$n = 0
try {
  while ($true) {
    $n++
    $ts = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $line = "STAT,seq=$n,ts_ms=$ts,az=$(($n % 360)),el=$(($n % 90)),ok=1"
    $bytes = [Text.Encoding]::UTF8.GetBytes($line)
    [void]$udp.Send($bytes, $bytes.Length, $ep)
    if (($n % 10) -eq 0) { Write-Host "sent $n : $line" }
    if ($Count -gt 0 -and $n -ge $Count) { break }
    Start-Sleep -Milliseconds $IntervalMs
  }
} finally {
  $udp.Close()
  Write-Host "[E2E-2] stopped after $n packets"
}
