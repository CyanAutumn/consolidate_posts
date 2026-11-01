@echo off
setlocal EnableExtensions DisableDelayedExpansion
call "%~dp0run_upload_hub.bat" %*
exit /b %errorlevel%
