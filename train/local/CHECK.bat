@echo off
rem Докачка файлов (один раз, может занять несколько часов) и быстрая проверка обучения (5-10 минут).
chcp 65001 >nul
cd /d "%~dp0"
set PYTHONUTF8=1
python\python.exe fetch.py || goto fail
set HF_HUB_OFFLINE=1
set TRANSFORMERS_OFFLINE=1
python\python.exe train_5060.py --limit 16 --grad-accum 4 --out out-check || goto fail
echo.
echo ПРОВЕРКА ПРОШЛА. Теперь запустите START.bat
pause
exit /b 0
:fail
echo.
echo ОШИБКА. Сфотографируйте экран и пришлите файл out-check\log.txt
pause
exit /b 1
