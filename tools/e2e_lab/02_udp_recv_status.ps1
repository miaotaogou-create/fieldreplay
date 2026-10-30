# E2E-2: local sniff — bind UDP and print lines (stand-in before FieldReplay)
param(
  [string]$BindHost = "127.0.0.1",
  [int]$Port = 56001,
  [int]$Seconds = 8
)
$ErrorActionPreference = "Stop"
$ep = New-Object System.Net.IPEndPoint ([System.Net.IPAddress]::Parse($BindHost), $Port)
$udp = New-Object System.Net.Sockets.UdpClient $ep
$udp.Client.ReceiveTimeout = 1000
Write-Host "[E2E-2] UDP recv on ${BindHost}:${Port} for ${Seconds}s"
$deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
$got = 0
$remote = New-Object System.Net.IPEndPoint ([System.Net.IPAddress]::Any, 0)
while ([DateTime]::UtcNow -lt $deadline) {
  try {
    $bytes = $udp.Receive([ref]$remote)
    $text = [Text.Encoding]::UTF8.GetString($bytes)
    $got++
    Write-Host "recv #$got from $($remote) : $text"
  } catch [System.Net.Sockets.SocketException] {
    # timeout — keep waiting until deadline
  }
}
$udp.Close()
Write-Host "[E2E-2] done, packets=$got"
if ($got -lt 3) {
  Write-Host "FAIL: expected at least 3 STAT lines"
  exit 1
}
Write-Host "PASS"
exit 0
