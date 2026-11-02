# E2E-6: fake host — bind UDP and print text + short HEX (replay sink later)
param(
  [string]$BindHost = "127.0.0.1",
  [int]$Port = 56101,
  [int]$Seconds = 10
)
$ErrorActionPreference = "Stop"

function Format-HexPreview([byte[]]$bytes, [int]$max = 24) {
  $n = [Math]::Min($bytes.Length, $max)
  if ($n -le 0) { return "" }
  ($bytes[0..($n - 1)] | ForEach-Object { $_.ToString("X2") }) -join " "
}

$ep = New-Object System.Net.IPEndPoint ([System.Net.IPAddress]::Parse($BindHost), $Port)
$udp = New-Object System.Net.Sockets.UdpClient $ep
$udp.Client.ReceiveTimeout = 1000
Write-Host "[E2E-6] fake-host UDP listen ${BindHost}:${Port} for ${Seconds}s"
$deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
$got = 0
$remote = New-Object System.Net.IPEndPoint ([System.Net.IPAddress]::Any, 0)
while ([DateTime]::UtcNow -lt $deadline) {
  try {
    $bytes = $udp.Receive([ref]$remote)
    $text = [Text.Encoding]::UTF8.GetString($bytes)
    $hex = Format-HexPreview $bytes
    $got++
    Write-Host "recv #$got from $($remote) text=$text"
    Write-Host "         hex=$hex$(if ($bytes.Length -gt 24) { ' ...' })"
  } catch [System.Net.Sockets.SocketException] {
    # timeout
  }
}
$udp.Close()
Write-Host "[E2E-6] UDP host done, packets=$got"
if ($got -lt 3) {
  Write-Host "FAIL: expected at least 3 lines (start 06_start_udp_probe.bat)"
  exit 1
}
Write-Host "PASS"
exit 0
