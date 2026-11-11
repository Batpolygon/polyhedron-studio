@echo off
cd /d "%~dp0"
del _test.done 2>nul
cargo test --color never > _test.log 2>&1
echo %ERRORLEVEL% > _test.done
exit
