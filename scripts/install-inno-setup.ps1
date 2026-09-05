param(
    [string]$InstallDir = 'C:\Program Files (x86)\Inno Setup 6',
    [switch]$CurrentUser
)

$ErrorActionPreference = 'Stop'
$version = '6.7.3'
$url = 'https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe'
# SHA-256 from the official GitHub release asset; update together with the URL.
$expectedHash = '9c73c3bae7ed48d44112a0f48e66742c00090bdb5bef71d9d3c056c66e97b732'
$installer = Join-Path ([IO.Path]::GetTempPath()) ("innosetup-$version-" + [guid]::NewGuid() + '.exe')
try {
    Invoke-WebRequest -Uri $url -OutFile $installer
    $actualHash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash
    if ($actualHash -ne $expectedHash) {
        throw "Inno Setup download checksum mismatch: expected $expectedHash, got $actualHash"
    }
    $scope = if ($CurrentUser) { '/CURRENTUSER' } else { '/ALLUSERS' }
    $process = Start-Process -FilePath $installer -WindowStyle Hidden -Wait -PassThru -ArgumentList @(
        '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', $scope,
        ('/DIR="' + $InstallDir + '"')
    )
    if ($process.ExitCode -ne 0) { throw "Inno Setup installation failed: $($process.ExitCode)" }
    $compiler = Join-Path $InstallDir 'ISCC.exe'
    if (-not (Test-Path -LiteralPath $compiler)) { throw "Inno Setup compiler missing: $compiler" }
    # Official binaries can report 0.0.0.0 in Windows version resources.
    # Ask the compiler itself and exercise its preprocessor without emitting files.
    $versionParts = $version.Replace('.', ', ')
    @"
#if Ver != EncodeVer($versionParts)
  #error Unexpected Inno Setup compiler version
#endif
[Setup]
AppName=Compiler version check
AppVersion=1
CreateAppDir=no
Uninstallable=no
"@ | & $compiler /Q /O- -
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup $version compiler validation failed: $LASTEXITCODE" }
    if ($env:GITHUB_PATH) {
        $InstallDir | Out-File -FilePath $env:GITHUB_PATH -Encoding utf8 -Append
    }
    Write-Output "Installed verified Inno Setup $version at $InstallDir"
} finally {
    if (Test-Path -LiteralPath $installer) { Remove-Item -LiteralPath $installer -Force }
}
