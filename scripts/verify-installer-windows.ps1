$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.Security
$package = [System.IO.Path]::GetFullPath($env:INSTALLER_PATH)
$expectedPublisher = $env:INSTALLER_PUBLISHER
$identities = @($env:INSTALLER_IDENTITIES | ConvertFrom-Json)
$expectedArch = if ($env:INSTALLER_ARCH -eq 'aarch64') { 'arm64' } elseif ($env:INSTALLER_ARCH -eq 'x86_64') { 'x64' } else { throw 'Unknown architecture' }
if (-not $expectedPublisher -or $identities.Count -eq 0) { throw 'No trusted package publisher configured' }

# SignTool uses Windows trust policy and validates the complete package signature.
# Never install/register or execute the package to inspect it.
$signTool = Get-ChildItem -LiteralPath "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Filter signtool.exe -Recurse |
    Where-Object { $_.Directory.Name -eq 'x64' } | Sort-Object FullName -Descending | Select-Object -First 1
if (-not $signTool) { throw 'Windows SDK SignTool is unavailable' }
& $signTool.FullName verify /pa /all $package | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Windows package signature verification failed' }

$archive = [System.IO.Compression.ZipFile]::OpenRead($package)
try {
    $entry = $archive.GetEntry('AppxManifest.xml')
    $signatureEntry = $archive.GetEntry('AppxSignature.p7x')
    if (-not $entry -or $entry.Length -gt 1048576 -or -not $signatureEntry -or $signatureEntry.Length -gt 1048576) { throw 'Package metadata missing or oversized' }
    $reader = [System.IO.StreamReader]::new($entry.Open())
    try { $manifestText = $reader.ReadToEnd() } finally { $reader.Dispose() }
    $settings = [System.Xml.XmlReaderSettings]::new()
    $settings.DtdProcessing = [System.Xml.DtdProcessing]::Prohibit
    $settings.XmlResolver = $null
    $xmlReader = [System.Xml.XmlReader]::Create([System.IO.StringReader]::new($manifestText), $settings)
    $manifest = [System.Xml.XmlDocument]::new()
    $manifest.XmlResolver = $null
    try { $manifest.Load($xmlReader) } finally { $xmlReader.Dispose() }
    $identity = $manifest.SelectSingleNode('/*[local-name()="Package"]/*[local-name()="Identity"]')
    if (-not $identity -or $identity.Publisher -cne $expectedPublisher -or $identities -cnotcontains $identity.Name -or $identity.ProcessorArchitecture -cne $expectedArch) { throw 'Unexpected package identity, publisher or architecture' }

    $stream = $signatureEntry.Open()
    $memory = [System.IO.MemoryStream]::new()
    try { $stream.CopyTo($memory); $signature = $memory.ToArray() } finally { $stream.Dispose(); $memory.Dispose() }
    if ($signature.Length -lt 5 -or [System.Text.Encoding]::ASCII.GetString($signature, 0, 4) -cne 'PKCX') { throw 'Invalid Appx signature' }
    $cms = [System.Security.Cryptography.Pkcs.SignedCms]::new()
    $cms.Decode($signature[4..($signature.Length - 1)])
    # Trust and Authenticode timestamp policy were checked by SignTool above.
    $cms.CheckSignature($true)
    if ($cms.SignerInfos.Count -ne 1 -or $cms.SignerInfos[0].Certificate.Subject -cne $expectedPublisher) { throw 'Unexpected signing certificate publisher' }

    # Package dependencies cannot be silently satisfied from an overseas store.
    $deps = @($manifest.SelectNodes('/*[local-name()="Package"]/*[local-name()="Dependencies"]/*[local-name()="PackageDependency"]'))
    if ($deps.Count -ne 0) { throw 'External package dependencies are not mirrored' }
    $families = @($manifest.SelectNodes('/*[local-name()="Package"]/*[local-name()="Dependencies"]/*[local-name()="TargetDeviceFamily"]'))
    $desktop = @($families | Where-Object { $_.Name -eq 'Windows.Desktop' -or $_.Name -eq 'Windows.Universal' })
    if ($desktop.Count -eq 0) { throw 'Windows desktop minimum version is missing' }
    $minimum = @($desktop | ForEach-Object { [version]$_.MinVersion } | Sort-Object -Descending)[0].ToString()
    [ordered]@{
        version = [string]$identity.Version
        build = [string]$identity.Version
        supportedArchs = @($env:INSTALLER_ARCH)
        minOsVersion = $minimum
        publisher = $expectedPublisher
        identity = [string]$identity.Name
        bundleId = [string]$identity.Name
        checks = @{ signature = $true; publisher = $true; identity = $true; architecture = $true; system = $true; dependencies = $true }
    } | ConvertTo-Json -Depth 4 -Compress
} finally { $archive.Dispose() }
