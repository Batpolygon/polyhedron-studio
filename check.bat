@echo off
cd /d "%~dp0"
del _check.done 2>nul
cargo check --color never > _check.log 2>&1
echo %ERRORLEVEL% > _check.done
exit
