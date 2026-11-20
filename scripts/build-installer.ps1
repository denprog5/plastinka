# Собирает установщик: release-сборка + Inno Setup → dist\Plastinka-Setup-<версия>.exe.
# Этот же файл ставится автообновлением (тихо, поверх установленной программы).
# Запуск:  powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $root

$iscc = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw "Не найден Inno Setup 6 (ISCC.exe). Установите: winget install JRSoftware.InnoSetup --scope user" }

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$rcVersion = (Select-String -Path assets\app.rc -Pattern 'VALUE "FileVersion", "(.+)"').Matches[0].Groups[1].Value
if ($rcVersion -ne $version) { throw "Версия в assets\app.rc ($rcVersion) не совпадает с Cargo.toml ($version)" }

Write-Host "Сборка release $version ..."
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build завершился с ошибкой" }

Write-Host "Inno Setup ..."
& $iscc /Q "/DAppVersion=$version" "installer\Plastinka.iss"
if ($LASTEXITCODE -ne 0) { throw "ISCC завершился с ошибкой" }
$out = Get-Item "dist\Plastinka-Setup-$version.exe"
Write-Host ("Готово: {0} ({1:N1} МБ)" -f $out.FullName, ($out.Length / 1MB))
