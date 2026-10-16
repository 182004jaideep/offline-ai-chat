@echo off
title Portable AI - Auto-Detect Workstation
cd /d "%~dp0"

echo ================================================================
echo Running Zero-Trace Dynamic Hardware Discovery...
echo ================================================================

:: Execute PowerShell probe to inspect hardware and generate optimal flags
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$cores = (Get-CimInstance Win32_Processor | Measure-Object -Property NumberOfCores -Sum).Sum;" ^
  "$threads = [Math]::Max(2, [Math]::Min($cores, 8));" ^
  "$totalRamGB = [Math]::Round((Get-CimInstance Win32_OperatingSystem).TotalVisibleMemorySize / 1MB, 1);" ^
  "$freeRamGB = [Math]::Round((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB, 1);" ^
  "Write-Host '[HARDWARE DETECTED]' -ForegroundColor Cyan;" ^
  "Write-Host ('  CPU Physical Cores : ' + $cores + ' (Allocating ' + $threads + ' inference threads)');" ^
  "Write-Host ('  Total System RAM   : ' + $totalRamGB + ' GB');" ^
  "Write-Host ('  Available RAM      : ' + $freeRamGB + ' GB');" ^
  "if ($totalRamGB -lt 12) { $ctx = 4096 } else { $ctx = 8192 };" ^
  "$model = Get-ChildItem -Path '%~dp0models' -Filter '*.gguf' | Sort-Object Length -Descending | Select-Object -First 1;" ^
  "if (-not $model) { Write-Host '[ERROR] No .gguf model found in models\' -ForegroundColor Red; exit 1 };" ^
  "Write-Host ('  Selected Model     : ' + $model.Name);" ^
  "Write-Host ('  Dynamic Context    : ' + $ctx + ' tokens');" ^
  "$params = @('-m', ('models\' + $model.Name), '-c', $ctx, '-t', $threads, '-tb', $threads, '-fa', '--no-mmap', '--host', '127.0.0.1', '--port', '8585', '--slots');" ^
  "$params -join ' ' | Out-File -FilePath '%~dp0dyn_args.txt' -Encoding ascii -NoNewline"

if %errorlevel% neq 0 (
    echo Hardware probe failed or no model found. Aborting.
    pause
    exit /b
)

set /p DYN_ARGS=<"%~dp0dyn_args.txt"
del "%~dp0dyn_args.txt" >nul 2>&1

:: Clean up previous temporary cache sandbox on USB
if exist "%~dp0temp" rmdir /s /q "%~dp0temp" >nul 2>&1
mkdir "%~dp0temp"

:: Clear any orphaned process on port 8585
netstat -ano | findstr :8585 | findstr LISTENING >nul
if %errorlevel% equ 0 (
    taskkill /f /im llama-server.exe >nul 2>&1
    timeout /t 1 /nobreak >nul
)

:: Launch llama-server with auto-configured flags
start "AutoAI" /min llama-server.exe %DYN_ARGS%

echo Engine loading into RAM. Synchronizing readiness...

:WAIT_LOOP
powershell -Command "try { $r = Invoke-RestMethod -Uri 'http://127.0.0.1:8585/health' -TimeoutSec 1; if ($r.status -eq 'ok') { exit 0 } else { exit 1 } } catch { exit 1 }" >nul 2>&1
if %errorlevel% neq 0 (
    timeout /t 2 /nobreak >nul
    goto WAIT_LOOP
)

echo Engine active! Launching Incognito Workspace...

start /wait msedge --app="%~dp0index.html" ^
  --inprivate ^
  --user-data-dir="%~dp0temp" ^
  --no-first-run ^
  --disable-background-networking ^
  --disable-sync ^
  --disable-features=Translate,OptimizationHints ^
  --window-size=1200,900

echo Terminating local engine and purging session caches...
taskkill /f /im llama-server.exe >nul 2>&1

set RETRIES=0
:SHRED_LOOP
rmdir /s /q "%~dp0temp" >nul 2>&1
if exist "%~dp0temp" (
    set /a RETRIES+=1
    if %RETRIES% leq 5 (
        timeout /t 1 /nobreak >nul
        goto SHRED_LOOP
    )
)
echo Clean exit complete.
exit