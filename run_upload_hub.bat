@echo off
setlocal
cd /d "%~dp0"
where node >nul 2>nul
if errorlevel 1 (
  echo Node.js is required. Please install Node.js and retry.
  pause
  exit /b 1
)
where cargo >nul 2>nul
if errorlevel 1 (
  echo Rust MSVC toolchain is required. Please install it and retry.
  pause
  exit /b 1
)
if not exist node_modules (
  call npm ci
  if errorlevel 1 (
    pause
    exit /b 1
  )
)
if /I "%~1"=="--dev" (
  call npm run desktop:dev
  if errorlevel 1 (
    pause
    exit /b 1
  )
  exit /b 0
)
call npm run desktop:build:debug
if errorlevel 1 (
  pause
  exit /b 1
)
start "" "%~dp0target\debug\upload-hub.exe"
