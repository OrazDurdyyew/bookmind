# Отменяет setup.ps1: выключает SSH-сервер, убирает ключ BookMind, отключает компьютер от сети Tailscale.
# Запуск в PowerShell от имени администратора:
#     powershell -ExecutionPolicy Bypass -File undo.ps1

$ErrorActionPreference = 'Continue'
$mark = 'bookmind'

foreach ($f in 'C:\ProgramData\ssh\administrators_authorized_keys', "$env:USERPROFILE\.ssh\authorized_keys") {
    if (Test-Path $f) { Set-Content $f (Get-Content $f | Where-Object { $_ -notmatch "$mark`$" }) }
}
Stop-Service sshd -ErrorAction SilentlyContinue
Set-Service sshd -StartupType Disabled -ErrorAction SilentlyContinue

$ts = 'C:\Program Files\Tailscale\tailscale.exe'
if (Test-Path $ts) { & $ts logout }

Write-Host 'Доступ закрыт: SSH выключен, ключ удалён, Tailscale отключён.' -ForegroundColor Green
Write-Host 'Сам Tailscale можно удалить через "Установка и удаление программ", папку bookmind-5060 — вручную.'
pause
