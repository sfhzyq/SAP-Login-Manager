# SAP Login Manager GPUI 版构建脚本
# 用法：在项目根目录执行 .\build-gpui.ps1 [check|build|run] [debug|release]
#
# 原理：亚信安全间歇性拦截 E:\rust-dev 下的 cargo/rustc 写入（os error 5），
# 临时工具链在 %TEMP%\rust 下不受拦截，但需要正确设置 RUSTUP_HOME / CARGO_HOME。
#
# 参数：
#   check   — 仅类型检查（最快，不产出二进制）
#   build   — 编译产出二进制
#   run     — 编译并运行
#   （默认 check）
#
#   debug   — Debug 配置（默认）
#   release — Release 配置

param(
    [Parameter(Position=0)]
    [ValidateSet('check','build','run')]
    [string]$Action = 'check',

    [Parameter(Position=1)]
    [ValidateSet('debug','release')]
    [string]$Mode = 'debug'
)

# === 临时工具链路径（不受亚信安全拦截）===
$rustDir = Join-Path $env:LOCALAPPDATA 'Temp\rust'
$env:RUSTUP_HOME = Join-Path $rustDir 'rustup'
$env:CARGO_HOME  = Join-Path $rustDir 'cargo'
$cargo = Join-Path $env:CARGO_HOME 'bin\cargo.exe'

if (-not (Test-Path $cargo)) {
    Write-Host "错误：找不到临时工具链 $cargo" -ForegroundColor Red
    Write-Host "请确认 Rust 临时工具链已安装到 $rustDir" -ForegroundColor Yellow
    exit 1
}

Write-Host "Rust 工具链：$(& $cargo --version)" -ForegroundColor Cyan
Write-Host "RUSTUP_HOME  = $env:RUSTUP_HOME"
Write-Host "CARGO_HOME   = $env:CARGO_HOME"
Write-Host "工作目录     = $PSScriptRoot\gpui-version"
Write-Host ""

# 切换到 gpui-version workspace
Set-Location "$PSScriptRoot\gpui-version"

# 构建参数
$args = @($Action, '-p', 'sap-login-manager-gpui')
if ($Mode -eq 'release') {
    $args += @('--release')
}

Write-Host "执行：cargo $($args -join ' ')" -ForegroundColor Green
Write-Host "---"

& $cargo @args

$exitCode = $LASTEXITCODE
if ($exitCode -eq 0) {
    Write-Host "`n✅ 编译成功" -ForegroundColor Green
} else {
    Write-Host "`n❌ 编译失败 (exit $exitCode)" -ForegroundColor Red
    # 临时工具链 registry 缓存偶尔损坏，清理后重试
    $regSrc = Join-Path $env:CARGO_HOME 'registry\src'
    if (Test-Path $regSrc) {
        Write-Host "`n如遇 registry 缓存损坏，可尝试清理：" -ForegroundColor Yellow
        Write-Host "  Remove-Item -Recurse -Force '$regSrc'" -ForegroundColor Yellow
        Write-Host "  然后重新执行本脚本" -ForegroundColor Yellow
    }
}
exit $exitCode
