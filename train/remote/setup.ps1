# Настройка компьютера с RTX 5060 Ti для удалённого запуска обучения BookMind.
# Запуск в PowerShell от имени администратора:
#     powershell -ExecutionPolicy Bypass -File remote-setup.ps1
#
# Что делает: включает SSH-сервер Windows, пускает по нему только ключ владельца BookMind
# (без пароля), отключает сон при питании от сети. Ничего не удаляет.

$ErrorActionPreference = 'Stop'
$key = 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMmGEopm2jOi0jVZYKik0DG0PBQE8MIWD5KxCiKSGxzu bookmind'

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) { Write-Host 'Нужно запустить от имени администратора.' -ForegroundColor Red; pause; exit 1 }

Write-Host 'Ставлю SSH-сервер (может занять несколько минут)...'
if ((Get-WindowsCapability -Online -Name 'OpenSSH.Server~~~~0.0.1.0').State -ne 'Installed') {
    Add-WindowsCapability -Online -Name 'OpenSSH.Server~~~~0.0.1.0' | Out-Null
}
Set-Service sshd -StartupType Automatic
Start-Service sshd
if (-not (Get-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -ErrorAction SilentlyContinue)) {
    New-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -DisplayName 'OpenSSH Server (sshd)' -Enabled True `
        -Direction Inbound -Protocol TCP -Action Allow -LocalPort 22 | Out-Null
}

# Для администраторов Windows берёт ключи из ProgramData, для остальных — из профиля.
$adminKeys = 'C:\ProgramData\ssh\administrators_authorized_keys'
if (-not (Select-String -Path $adminKeys -SimpleMatch $key -Quiet -ErrorAction SilentlyContinue)) { Add-Content $adminKeys $key }
icacls $adminKeys /inheritance:r /grant '*S-1-5-32-544:F' /grant '*S-1-5-18:F' | Out-Null
$userKeys = "$env:USERPROFILE\.ssh\authorized_keys"
New-Item -ItemType Directory -Force (Split-Path $userKeys) | Out-Null
if (-not (Select-String -Path $userKeys -SimpleMatch $key -Quiet -ErrorAction SilentlyContinue)) { Add-Content $userKeys $key }

Write-Host 'Отключаю сон при питании от сети...'
powercfg /change standby-timeout-ac 0
powercfg /change hibernate-timeout-ac 0

$ts = 'C:\Program Files\Tailscale\tailscale.exe'
$ip = if (Test-Path $ts) { (& $ts ip -4) 2>$null } else { $null }
Write-Host ''
$gpu = try { (nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader) -join '; ' } catch { 'nvidia-smi не найден — нужен драйвер NVIDIA' }
$free = '{0:N0} ГБ' -f ((Get-PSDrive C).Free / 1GB)
Write-Host 'Готово. Отправьте владельцу BookMind эти строки:' -ForegroundColor Green
Write-Host "  пользователь: $env:USERNAME"
if ($ip) { Write-Host "  адрес Tailscale: $ip" } else { Write-Host '  адрес Tailscale: (Tailscale ещё не установлен или не подключён)' -ForegroundColor Yellow }
Write-Host "  видеокарта: $gpu"
Write-Host "  свободно на диске C: $free"
pause
