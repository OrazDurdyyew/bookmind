@echo off
rem Python 3.12.16 (сборка python-build-standalone, ~20 МБ) в папку python\, если его ещё нет.
rem curl и tar встроены в Windows 10/11 (curl там старый — без --retry-all-errors, поэтому свой цикл).
if exist "%~dp0python\python.exe" exit /b 0
pushd "%~dp0"
set PYFILE=cpython-3.12.16%%2B20261026-x86_64-pc-windows-msvc-install_only_stripped.tar.gz
echo качаю Python...
for /l %%i in (1,1,5) do (
  curl.exe -L --fail -C - -o python.tar.gz https://github.com/astral-sh/python-build-standalone/releases/download/20261026/%PYFILE% && goto unpack
  curl.exe -L --fail -C - -o python.tar.gz https://releases.astral.sh/github/python-build-standalone/releases/download/20261026/%PYFILE% && goto unpack
  echo     не скачалось, попытка %%i из 5
  timeout /t 10 /nobreak >nul
)
popd
exit /b 1
:unpack
tar -xzf python.tar.gz || (popd & exit /b 1)
del python.tar.gz
popd
if not exist "%~dp0python\python.exe" exit /b 1
exit /b 0
