<#
.SYNOPSIS
    End-to-end test of CasaPic's release updater, entirely on this machine.

.DESCRIPTION
    Builds two copies of a SEPARATE test product ("CasaPicE2E": its own exe name, identifier,
    throwaway signing key, a local http endpoint and NO photo file types), so the real
    installed CasaPic, its library and its photo-type registration are never involved.
    The test copy keeps its library in tmp\e2e\data (CASAPIC_DATA), never in the real one.

      1. Installs test version A silently (default per-user folder %LOCALAPPDATA%\CasaPicE2E)
         and seeds a marker file in the test library.
      2. REFUSED cases: serves a latest.json announcing version B whose installer is
         (a) tampered (one byte appended) and (b) signed with another key. Each time the
         update log must say REFUSED and the installed exe must be unchanged.
      3. A version that failed to install before is downloaded but NOT auto-installed.
      4. GOOD case: serves the genuine B. The exe must be replaced, the relaunched app must
         log "Up to date (version B)" in the TEST library folder (so the relaunch kept the
         test's data folder), the marker must be unchanged, and the running app must report
         version B with updates possible.
      5. Rollback: the genuine OLD installer announced as a newer version is refused, and an
         unfinished install is detected and remembered.
      6. Uninstalls the test product and stops the local server.

    Also checks that the real CasaPic's photo-type command in the registry is unchanged.
    Takes two release builds. Shows the test app's window while it runs.

.PARAMETER KeepArtifacts
    Leave tmp\e2e in place for inspection.
#>
[CmdletBinding()]
param(
    [int]$Port = 8766,
    [int]$DebugPort = 9334,
    [string]$CargoTargetDir = '',
    [switch]$KeepArtifacts,
    # Reuse tmp\e2e's keys and builds from an earlier -KeepArtifacts run.
    [switch]$SkipBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot   = Split-Path -Parent $PSScriptRoot
# set here, not in param(): Windows PowerShell 5.1 leaves $PSScriptRoot empty in param defaults
if (-not $CargoTargetDir) { $CargoTargetDir = Join-Path $RepoRoot 'target' }
$Publisher  = Join-Path $PSScriptRoot 'Publish-CasaPicRelease.ps1'
$Work       = Join-Path $RepoRoot 'tmp\e2e'
$Product    = 'CasaPicE2E'
$Identifier = 'org.casapic.e2etest'
# Installed WITHOUT /D, so the installer's own default per-user folder is what is exercised.
$InstallDir = Join-Path $env:LOCALAPPDATA $Product
$Serve      = Join-Path $Work 'serve'
$VersionA   = '0.1.0'
$VersionB   = '0.1.1'
$Exe        = Join-Path $InstallDir "$Product.exe"
# the test library; the update log and attempt file live in its parent, as for the real app
$Library    = Join-Path $Work 'data\library'
$State      = Join-Path $Work 'data'
$Log        = Join-Path $State 'update.log'
$Attempt    = Join-Path $State 'update-attempt.json'
$Marker     = Join-Path $Library 'e2e-marker.bin'
$RealLog    = Join-Path $env:LOCALAPPDATA 'org.casapic.app\update.log'
$AssocKey   = 'HKCU:\Software\Classes\CasaPic.Photo\shell\open\command'
$RememberKey = "HKCU:\Software\casapic\$Product"

$results = New-Object System.Collections.Generic.List[string]
$failed  = $false
function Check([string]$name, [bool]$ok, [string]$detail = '') {
    $line = '{0} {1}{2}' -f ($(if ($ok) { 'PASS' } else { 'FAIL' })), $name, $(if ($detail) { " - $detail" } else { '' })
    $script:results.Add($line)
    Write-Host $line -ForegroundColor $(if ($ok) { 'Green' } else { 'Red' })
    if (-not $ok) { $script:failed = $true }
}

function Invoke-Native([scriptblock]$Command) {
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $Command 2>&1 | ForEach-Object { "$_" } } finally { $ErrorActionPreference = $prev }
}

function Write-Utf8NoBom([string]$Path, [string]$Text) {
    [System.IO.File]::WriteAllText($Path, $Text, (New-Object System.Text.UTF8Encoding($false)))
}

function Get-ExeVersion { if (Test-Path -LiteralPath $Exe) { (Get-Item -LiteralPath $Exe).VersionInfo.ProductVersion } else { '' } }
# The exe's embedded version comes from Cargo.toml, which -VersionOverride does not change,
# so 'was the exe replaced' is judged by its SHA-256.
function Get-ExeHash { try { (Get-FileHash -LiteralPath $Exe -Algorithm SHA256 -ErrorAction Stop).Hash } catch { '' } }
function Get-LogText { if (Test-Path -LiteralPath $Log) { Get-Content -Raw -LiteralPath $Log } else { '' } }
function Get-NewLog([int]$From) { $t = Get-LogText; $t.Substring([Math]::Min($From, $t.Length)) }
function Get-RealLogLength { if (Test-Path -LiteralPath $RealLog) { (Get-Item -LiteralPath $RealLog).Length } else { -1 } }
function Get-AssocCommand { try { (Get-ItemProperty -LiteralPath $AssocKey -ErrorAction Stop).'(default)' } catch { '<none>' } }
# The test product has its own exe name: the installer closes every running process with
# its exe name, so with the real name it would close the user's CasaPic. Matching by path
# as well keeps the test from touching any other copy.
function Get-TestApp {
    Get-Process -Name $Product -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith($InstallDir, [StringComparison]::OrdinalIgnoreCase) }
}
function Stop-TestApp {
    Get-TestApp | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 800
}
function Wait-Until([scriptblock]$Condition, [int]$Seconds) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        if (& $Condition) { return $true }
        Start-Sleep -Milliseconds 500
    }
    return [bool](& $Condition)
}
function Set-Served([string]$InstallerName, [string]$Signature, [string]$Version = $VersionB) {
    $manifest = [ordered]@{
        version   = $Version
        notes     = 'e2e'
        pub_date  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
        platforms = [ordered]@{
            'windows-x86_64' = [ordered]@{ signature = $Signature; url = "http://127.0.0.1:$Port/$InstallerName" }
        }
    }
    Write-Utf8NoBom (Join-Path $Serve 'latest.json') ($manifest | ConvertTo-Json -Depth 5)
}
function Start-TestApp { Start-Process -FilePath $Exe -WindowStyle Minimized | Out-Null }

$server = $null
$assocBefore = Get-AssocCommand
$realLogBefore = Get-RealLogLength
try {
    # --- Setup: throwaway keys and test config --------------------------------
    Stop-TestApp
    $oldUninstaller = Join-Path $InstallDir 'uninstall.exe'
    if (Test-Path -LiteralPath $oldUninstaller) { Start-Process -FilePath $oldUninstaller -ArgumentList '/S' -Wait }
    if (Test-Path -LiteralPath $InstallDir) { Remove-Item -LiteralPath $InstallDir -Recurse -Force }
    # A remembered install folder from an earlier run would override the default.
    Remove-Item -LiteralPath $RememberKey -Recurse -Force -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $Work) {
        if ($SkipBuild) {
            foreach ($d in $Serve, (Join-Path $Work 'data')) { if (Test-Path -LiteralPath $d) { Remove-Item -LiteralPath $d -Recurse -Force } }
        } else {
            Remove-Item -LiteralPath $Work -Recurse -Force
        }
    }
    New-Item -ItemType Directory -Path $Work, $Serve, $Library -Force | Out-Null
    $keyGood  = Join-Path $Work 'e2e.key'
    $keyOther = Join-Path $Work 'other.key'
    # The "other" key carries a password: `tauri signer sign` prompts for one when the key
    # has none (and hangs unattended), and PowerShell 5.1 cannot pass an empty -p argument.
    $otherPassword = 'e2e-other-key'
    if (-not $SkipBuild) {
        Push-Location $RepoRoot
        try {
            Invoke-Native { npx.cmd tauri signer generate --ci -f -w $keyGood } | Out-Null
            if ($LASTEXITCODE -ne 0) { throw 'signer generate failed (build key)' }
        } finally { Pop-Location }
    }
    Push-Location $RepoRoot
    try {
        Invoke-Native { npx.cmd tauri signer generate --ci -f -p $otherPassword -w $keyOther } | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'signer generate failed (other key)' }
    } finally { Pop-Location }

    $conf = [ordered]@{
        productName    = $Product
        mainBinaryName = $Product
        identifier     = $Identifier
        # no photo file types: the test copy must never take over the real CasaPic's
        bundle         = [ordered]@{ fileAssociations = @() }
        plugins        = [ordered]@{
            updater = [ordered]@{
                pubkey    = (Get-Content -Raw -LiteralPath "$keyGood.pub").Trim()
                endpoints = @("http://127.0.0.1:$Port/latest.json")
                dangerousInsecureTransportProtocol = $true
            }
        }
    }
    $confPath = Join-Path $Work 'e2e.conf.json'
    Write-Utf8NoBom $confPath ($conf | ConvertTo-Json -Depth 10)

    # --- Build A and B ------------------------------------------------------
    foreach ($v in $VersionA, $VersionB) {
        if ($SkipBuild) { break }
        & powershell -NoProfile -ExecutionPolicy Bypass -File $Publisher -ConfigOverride $confPath `
            -VersionOverride $v -KeyPath $keyGood -OutDir (Join-Path $Work "build-$v") -CargoTargetDir $CargoTargetDir `
            -BaseUrl "http://127.0.0.1:$Port"
        if ($LASTEXITCODE -ne 0) { throw "Build of $v failed." }
    }
    $setupA = Join-Path $Work "build-$VersionA\${Product}_${VersionA}_x64-setup.exe"
    $setupB = Join-Path $Work "build-$VersionB\${Product}_${VersionB}_x64-setup.exe"
    $sigA   = (Get-Content -Raw -LiteralPath "$setupA.sig").Trim()
    $sigB   = (Get-Content -Raw -LiteralPath "$setupB.sig").Trim()

    # Served files: genuine B, tampered B (one extra byte), B signed by another key.
    $nameB = Split-Path -Leaf $setupB
    Copy-Item -LiteralPath $setupB -Destination (Join-Path $Serve $nameB)
    $nameA = Split-Path -Leaf $setupA
    Copy-Item -LiteralPath $setupA -Destination (Join-Path $Serve $nameA)
    $tampered = Join-Path $Serve "tampered-$nameB"
    Copy-Item -LiteralPath $setupB -Destination $tampered
    $fs = [System.IO.File]::Open($tampered, 'Append'); $fs.WriteByte(0); $fs.Close()
    $otherSigned = Join-Path $Serve "other-$nameB"
    Copy-Item -LiteralPath $setupB -Destination $otherSigned
    Push-Location $RepoRoot
    try {
        $env:CI = 'true'
        Invoke-Native { npx.cmd tauri signer sign -f $keyOther -p $otherPassword $otherSigned } | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'signer sign (other key) failed' }
    } finally { Remove-Item Env:\CI -ErrorAction SilentlyContinue; Pop-Location }
    $sigOther = (Get-Content -Raw -LiteralPath "$otherSigned.sig").Trim()

    # --- Install A ----------------------------------------------------------
    Start-Process -FilePath $setupA -ArgumentList '/S' -Wait
    Check 'install A into %LOCALAPPDATA%\<product>' ((Get-ExeVersion) -eq $VersionA) "exe version '$(Get-ExeVersion)' at $Exe"
    $hashA = Get-ExeHash
    Check 'installed by setup (uninstall.exe present)' (Test-Path -LiteralPath (Join-Path $InstallDir 'uninstall.exe'))
    Check 'test install leaves the real photo-type command alone' ((Get-AssocCommand) -eq $assocBefore) "$(Get-AssocCommand)"
    $bytes = New-Object byte[] 65536; (New-Object Random 7).NextBytes($bytes)
    [System.IO.File]::WriteAllBytes($Marker, $bytes)
    $markerHash = (Get-FileHash -LiteralPath $Marker).Hash

    # Every start of the test copy (and the installer's relaunch, which inherits this
    # process's environment) uses the test library, never the real one.
    $env:CASAPIC_DATA = $Library

    # --- Local server -------------------------------------------------------
    $python = (Get-Command python -ErrorAction Stop).Source
    $server = Start-Process -FilePath $python -ArgumentList '-m', 'http.server', $Port, '--bind', '127.0.0.1', '--directory', $Serve `
        -WindowStyle Hidden -PassThru
    Check 'local server up' (Wait-Until { try { (Invoke-WebRequest "http://127.0.0.1:$Port/" -UseBasicParsing -TimeoutSec 2).StatusCode -eq 200 } catch { $false } } 15)

    # --- REFUSED cases ------------------------------------------------------
    foreach ($case in @(
            @{ Name = 'tampered installer'; File = (Split-Path -Leaf $tampered); Sig = $sigB },
            @{ Name = 'signed with another key'; File = (Split-Path -Leaf $otherSigned); Sig = $sigOther })) {
        Set-Served $case.File $case.Sig
        $before = (Get-LogText).Length
        Start-TestApp
        # 'REFUSED - signature check failed' is written only for signature errors; a 404 or a
        # dead server logs 'Downloading ... failed' instead.
        $refused = Wait-Until { (Get-NewLog $before) -match 'REFUSED - signature check failed' } 90
        $newLog = (Get-NewLog $before).Trim()
        Check "refuse $($case.Name)" ($refused -and $newLog -notmatch 'signed for version') ($newLog -replace "`r?`n", ' | ')
        Start-Sleep -Seconds 2
        Check "installed copy untouched after $($case.Name)" ((Get-ExeHash) -eq $hashA) 'exe SHA-256 unchanged'
        Check "app still running after $($case.Name)" ([bool](Get-TestApp))
        Stop-TestApp
    }
    Check 'test copy used the test library (index made there)' (Test-Path -LiteralPath (Join-Path $Library 'index.db'))

    # --- A version that failed to install before is NOT auto-installed -------
    Set-Served $nameB $sigB
    Write-Utf8NoBom $Attempt ('{"version":"' + $VersionB + '","failed":true}')
    $before = (Get-LogText).Length
    Start-TestApp
    $staged = Wait-Until { (Get-NewLog $before) -match "Version $([regex]::Escape($VersionB)) is ready" } 90
    Start-Sleep -Seconds 3
    Check 'earlier failed version is staged, not auto-installed' ($staged -and (Get-ExeHash) -eq $hashA -and [bool](Get-TestApp))
    Stop-TestApp
    Remove-Item -LiteralPath $Attempt -Force

    # --- GOOD case ----------------------------------------------------------
    # debugging port for the probe; the installer passes this environment on to the relaunch
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$DebugPort"
    $before = (Get-LogText).Length
    Start-TestApp
    $installed = Wait-Until { $h = Get-ExeHash; $h -and $h -ne $hashA } 180
    Check 'genuine update replaced the exe' $installed 'exe SHA-256 changed'
    $relaunched = Wait-Until { (Get-LogText) -match "Up to date \(version $([regex]::Escape($VersionB))\)" } 120
    $newLog = (Get-NewLog $before).Trim()
    Check 'log (in the test library folder) shows verify -> install -> relaunched and up to date' (
        $relaunched -and $newLog -match 'signature verified' -and $newLog -match "Installing $([regex]::Escape($VersionB))" -and
        $newLog -match "Updated to version $([regex]::Escape($VersionB))"
    ) ($newLog -replace "`r?`n", ' | ')
    Check 'test library marker unchanged' ((Get-FileHash -LiteralPath $Marker).Hash -eq $markerHash)
    $probeOut = & $python (Join-Path $PSScriptRoot 'e2e_update_probe.py') $DebugPort 60 | Select-Object -Last 1
    $probe = try { $probeOut | ConvertFrom-Json } catch { $null }
    Check 'updated app runs and reports the new version with updates on' (
        $probe -and $probe.ok -and $probe.status.running_version -eq $VersionB -and $probe.status.can_update -and $probe.status.auto_update
    ) "$probeOut"
    Stop-TestApp
    Remove-Item Env:\WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
    $hashB = Get-ExeHash

    # --- Rollback: genuine OLD installer announced as a newer version --------
    # plus a pretend unfinished install of a version that never arrived.
    Set-Served $nameA $sigA '0.1.2'
    Write-Utf8NoBom $Attempt '{"version":"0.3.0","failed":false}'
    $before = (Get-LogText).Length
    Start-TestApp
    $rolled = Wait-Until { (Get-NewLog $before) -match 'REFUSED' } 90
    # The updater library refuses this itself (its message), before CasaPic's own check
    # (updates.rs, same rule, kept as a second layer) is reached; accept either wording.
    $newLog = (Get-NewLog $before).Trim()
    Start-Sleep -Seconds 2
    Check 'old genuine installer served as 0.1.2 is refused (version bound to signature)' (
        $rolled -and $newLog -match "REFUSED.*signed for version $([regex]::Escape($VersionA))(, not| but the update endpoint announced version) 0\.1\.2" -and (Get-ExeHash) -eq $hashB
    ) ($newLog -replace "`r?`n", ' | ')
    $att = try { Get-Content -Raw -LiteralPath $Attempt | ConvertFrom-Json } catch { $null }
    Check 'unfinished install is detected and remembered' (
        $newLog -match 'Installing 0\.3\.0 did not finish' -and $att -and $att.failed -eq $true
    )
    Stop-TestApp
} catch {
    Check 'test harness' $false $_.Exception.Message
} finally {
    Remove-Item Env:\WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
    Remove-Item Env:\CASAPIC_DATA -ErrorAction SilentlyContinue
    Stop-TestApp
    if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue }
    $uninstaller = Join-Path $InstallDir 'uninstall.exe'
    if (Test-Path -LiteralPath $uninstaller) {
        Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait
        Check 'test product uninstalled (registry entry gone)' (-not (Test-Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$Product"))
        Check 'uninstall leaves the test library in place' (Test-Path -LiteralPath $Marker)
    }
    # the test's own leftovers only: its remembered folder, its webview data, its program folder
    Remove-Item -LiteralPath $RememberKey -Recurse -Force -ErrorAction SilentlyContinue
    foreach ($d in (Join-Path $env:APPDATA $Identifier), (Join-Path $env:LOCALAPPDATA $Identifier)) {
        Remove-Item -LiteralPath $d -Recurse -Force -ErrorAction SilentlyContinue
    }
    if ((Split-Path -Leaf $InstallDir) -eq $Product -and (Test-Path -LiteralPath $InstallDir)) {
        Remove-Item -LiteralPath $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
    }
    Check 'real photo-type command unchanged at the end' ((Get-AssocCommand) -eq $assocBefore) "$(Get-AssocCommand)"
    Check 'real CasaPic update log not written by the test' ((Get-RealLogLength) -eq $realLogBefore)
    if (-not $KeepArtifacts -and (Test-Path -LiteralPath $Work)) {
        Remove-Item -LiteralPath $Work -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Host ''
Write-Host ('RESULT: {0} ({1} checks)' -f $(if ($failed) { 'FAIL' } else { 'PASS' }), $results.Count) -ForegroundColor $(if ($failed) { 'Red' } else { 'Green' })
exit $(if ($failed) { 1 } else { 0 })
