@echo off
title Offline AI Chat - Setup
cd /d "%~dp0"

echo ================================================================
echo  Offline AI Chat - First-time setup
echo ================================================================
echo.

if exist "%~dp0llama-server.exe" (
    echo [OK] llama-server.exe already present. Skipping engine download.
    goto CHECK_MODEL
)

echo [1/2] Downloading the llama.cpp inference engine (Windows CPU build)...
echo        Source: https://github.com/ggml-org/llama.cpp/releases
echo        License: MIT ^(see https://github.com/ggml-org/llama.cpp/blob/master/LICENSE^)
echo.

powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$ErrorActionPreference = 'Stop';" ^
  "try {" ^
  "  $releases = Invoke-RestMethod -Uri 'https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=10' -Headers @{ 'User-Agent' = 'offline-ai-chat-setup' };" ^
  "  $build = $releases | Where-Object { $_.tag_name -match '^b[0-9]+$' } | Select-Object -First 1;" ^
  "  if (-not $build) { throw 'Could not find a numbered llama.cpp build release.' };" ^
  "  $asset = $build.assets | Where-Object { $_.name -like '*-bin-win-cpu-x64.zip' } | Select-Object -First 1;" ^
  "  if (-not $asset) { throw 'Could not find a Windows CPU build asset in release ' + $build.tag_name };" ^
  "  Write-Host ('  Found build ' + $build.tag_name + ': ' + $asset.name);" ^
  "  $zipPath = Join-Path $env:TEMP 'llama-cpu-x64.zip';" ^
  "  Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath -UseBasicParsing;" ^
  "  $extractDir = Join-Path $env:TEMP 'llama-cpu-x64-extracted';" ^
  "  if (Test-Path $extractDir) { Remove-Item $extractDir -Recurse -Force };" ^
  "  Expand-Archive -Path $zipPath -DestinationPath $extractDir -Force;" ^
  "  $files = Get-ChildItem -Path $extractDir -Recurse -Include 'llama-server.exe','ggml*.dll','llama.dll';" ^
  "  foreach ($f in $files) { Copy-Item $f.FullName -Destination '%~dp0' -Force; Write-Host ('  Installed ' + $f.Name) };" ^
  "  Remove-Item $zipPath -Force -ErrorAction SilentlyContinue;" ^
  "  Remove-Item $extractDir -Recurse -Force -ErrorAction SilentlyContinue;" ^
  "  Write-Host '[OK] Engine installed.' -ForegroundColor Green;" ^
  "} catch {" ^
  "  Write-Host ('[ERROR] ' + $_.Exception.Message) -ForegroundColor Red;" ^
  "  Write-Host 'Download it manually from https://github.com/ggml-org/llama.cpp/releases';" ^
  "  Write-Host '(grab the *-bin-win-cpu-x64.zip asset) and copy llama-server.exe + the DLLs next to this script.';" ^
  "  exit 1;" ^
  "}"

if %errorlevel% neq 0 (
    pause
    exit /b
)

:CHECK_MODEL
echo.
if exist "%~dp0models\*.gguf" (
    echo [OK] A .gguf model is already present in models\. Setup complete.
    goto DONE
)

echo [2/2] No model found in models\.
echo.
echo This project does not auto-download a model, since the right size/model
echo depends on your hardware and preference. Pick ONE of the following,
echo download the .gguf file, and place it in the "models" folder:
echo.
echo   Low-end / 8GB RAM   : Qwen2.5-Coder-1.5B-Instruct-GGUF  (~1 GB)
echo     https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF
echo.
echo   Balanced / 16GB RAM : Qwen2.5-Coder-7B-Instruct-GGUF    (~4.5 GB, Q4_K_M)
echo     https://huggingface.co/Qwen/Qwen2.5-Coder-7B-Instruct-GGUF
echo.
echo   General chat        : Meta-Llama-3.1-8B-Instruct-GGUF   (~4.9 GB, Q4_K_M)
echo     https://huggingface.co/bartowski/Meta-Llama-3.1-8B-Instruct-GGUF
echo.
echo After downloading, re-run this setup.bat (or just run Run-Office-AI.bat
echo directly - it auto-picks whichever .gguf file it finds in models\).
echo.

:DONE
pause
