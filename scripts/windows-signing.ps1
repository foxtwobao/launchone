param(
  [string]$Path,
  [switch]$Check,
  [switch]$VerifyOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$required = @(
  'DESKTOP_WIN_AZURE_CLIENT_ID', 'DESKTOP_WIN_AZURE_TENANT_ID',
  'DESKTOP_WIN_SIGNING_ENDPOINT', 'DESKTOP_WIN_SIGNING_ACCOUNT',
  'DESKTOP_WIN_SIGNING_PROFILE', 'DESKTOP_WIN_PUBLISHER'
)
foreach ($name in $required) {
  if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) {
    throw "Missing Windows signing setting: $name"
  }
}
$endpoint = [uri]$env:DESKTOP_WIN_SIGNING_ENDPOINT
if (-not $endpoint.IsAbsoluteUri -or $endpoint.Scheme -ne 'https' -or $endpoint.UserInfo -or $endpoint.Query -or $endpoint.Fragment) {
  throw 'DESKTOP_WIN_SIGNING_ENDPOINT must be an HTTPS endpoint without credentials or query parameters.'
}

# The workflow installs this pinned Microsoft NuGet package; no credentials are
# stored in its directory. Azure login has already populated the CLI OIDC cache.
$tempRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$dlib = Join-Path $tempRoot 'launchone-signing/Microsoft.ArtifactSigning.Client/bin/x64/Azure.CodeSigning.Dlib.dll'
if (-not (Test-Path -LiteralPath $dlib -PathType Leaf)) {
  throw 'Install Microsoft.ArtifactSigning.Client into RUNNER_TEMP/launchone-signing before packaging.'
}
$sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin'
$signTool = Get-ChildItem -Path "$sdkRoot/*/x64/signtool.exe" -File |
  Where-Object { [version]$_.Directory.Parent.Name -ge [version]'10.0.26100.0' } |
  Sort-Object { [version]$_.Directory.Parent.Name } -Descending |
  Select-Object -First 1
if (-not $signTool) { throw 'An x64 SignTool from Windows SDK 10.0.26100.0 or newer is required.' }
if ($Check) {
  Write-Host 'Windows signing configuration and tools are ready.'
  exit 0
}
if ([string]::IsNullOrWhiteSpace($Path)) { throw 'A file path is required.' }
$file = (Get-Item -LiteralPath $Path).FullName

if (-not $VerifyOnly) {
  $metadataPath = Join-Path $tempRoot "launchone-signing-$([guid]::NewGuid().ToString('N')).json"
  try {
    @{
      Endpoint = $endpoint.AbsoluteUri
      CodeSigningAccountName = $env:DESKTOP_WIN_SIGNING_ACCOUNT
      CertificateProfileName = $env:DESKTOP_WIN_SIGNING_PROFILE
      # Use azure/login's CLI credential exclusively, avoiding other accounts or
      # interactive authentication while Tauri signs each executable.
      ExcludeCredentials = @(
        'EnvironmentCredential', 'WorkloadIdentityCredential', 'ManagedIdentityCredential',
        'SharedTokenCacheCredential', 'VisualStudioCredential', 'VisualStudioCodeCredential',
        'AzurePowerShellCredential', 'AzureDeveloperCliCredential', 'InteractiveBrowserCredential'
      )
    } | ConvertTo-Json | Set-Content -LiteralPath $metadataPath -Encoding utf8NoBOM
    & $signTool.FullName sign /fd SHA256 /tr 'http://timestamp.acs.microsoft.com' /td SHA256 /d 'LaunchOne' /dlib $dlib /dmdf $metadataPath $file
    if ($LASTEXITCODE -ne 0) { throw 'Azure Artifact Signing failed.' }
  } finally {
    Remove-Item -LiteralPath $metadataPath -Force -ErrorAction SilentlyContinue
  }
}

& $signTool.FullName verify /pa /all /v $file
if ($LASTEXITCODE -ne 0) { throw 'Windows Authenticode verification failed.' }
$signature = Get-AuthenticodeSignature -LiteralPath $file
if ($signature.Status -ne 'Valid' -or $signature.SignatureType -ne 'Authenticode') {
  throw 'The executable does not have a valid embedded Authenticode signature.'
}
if (-not $signature.TimeStamperCertificate) { throw 'The executable is missing its signing timestamp.' }
$publisher = $signature.SignerCertificate.GetNameInfo([Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false)
if ($publisher -cne $env:DESKTOP_WIN_PUBLISHER) {
  throw 'The executable was not signed by DESKTOP_WIN_PUBLISHER.'
}
