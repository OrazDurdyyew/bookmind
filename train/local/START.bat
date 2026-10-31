@echo off
rem Докачка файлов, обучение и сборка модели. Если компьютер выключится — запустите снова, продолжит с места.
chcp 65001 >nul
cd /d "%~dp0"
set PYTHONUTF8=1
python\python.exe fetch.py || goto fail
set HF_HUB_OFFLINE=1
set TRANSFORMERS_OFFLINE=1
if exist out\lora\adapter_config.json goto gguf
python\python.exe train_5060.py || goto fail
:gguf
python\python.exe make_gguf.py || goto fail
echo.
echo ГОТОВО: файл out\bookmind-tm-Q4_K_M.gguf
pause
exit /b 0
:fail
echo.
echo ОШИБКА. Сфотографируйте экран и пришлите файл out\log.txt
pause
exit /b 1
