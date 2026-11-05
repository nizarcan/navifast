# Run only on a disposable Windows CI runner. The installer creates a Start
# menu shortcut and an uninstall entry for the current user.
param(
    [Parameter(Mandatory)][string]$Installer,
    [Parameter(Mandatory)][string]$Version
)

$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or -not $env:RUNNER_TEMP) {
    throw 'Run the installer check on a disposable GitHub Actions runner.'
}

$destination = Join-Path $env:RUNNER_TEMP 'navifast-install-check'
if (Test-Path $destination) { throw "Test destination already exists: $destination" }
$installerPath = (Resolve-Path $Installer).Path
$install = Start-Process -FilePath $installerPath -Wait -PassThru -ArgumentList @(
    '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=`"$destination`""
)
if ($install.ExitCode -ne 0) { throw "Installer failed: $($install.ExitCode)" }

try {
    foreach ($file in @('navifast.exe', 'LICENSE', 'README.md', 'navifast-installer.txt', 'unins000.exe')) {
        if (-not (Test-Path (Join-Path $destination $file))) { throw "Missing installed file: $file" }
    }
    $marker = Get-Content (Join-Path $destination 'navifast-installer.txt') -Raw
    if ($marker.Trim() -ne 'navifast-installer-v1') { throw 'Wrong installer identity' }
    $output = Join-Path $env:RUNNER_TEMP 'navifast-installed-version.txt'
    $app = Start-Process -FilePath (Join-Path $destination 'navifast.exe') `
        -ArgumentList '--version' -RedirectStandardOutput $output -Wait -PassThru
    if ($app.ExitCode -ne 0) { throw "Installed app failed: $($app.ExitCode)" }
    $reported = (Get-Content $output -Raw).Trim()
    if ($reported -ne "navifast $Version") { throw "Wrong installed version: $reported" }
} finally {
    $uninstaller = Join-Path $destination 'unins000.exe'
    if (Test-Path $uninstaller) {
        $remove = Start-Process -FilePath $uninstaller -Wait -PassThru `
            -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART'
        if ($remove.ExitCode -ne 0) { throw "Uninstaller failed: $($remove.ExitCode)" }
    }
}
if (Test-Path (Join-Path $destination 'navifast.exe')) { throw 'Uninstall left the application behind' }
Write-Output "Installed, launched and removed Navifast $Version"
