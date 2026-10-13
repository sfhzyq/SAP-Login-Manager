# SAP Login Manager GPUI 版构建脚本
# 用法：在项目根目录执行 .\build-gpui.ps1 [check|build|run] [debug|release]
#
# 原理：亚信安全曾间歇性拦截各盘工具链目录下的 cargo/rustc（os error 5）。
# 工具链优先级：E:\rust-dev（主）→ C:\Users\<user>\rust-dev（备）→ %TEMP%\rust（应急）。
# 若 E:\rust-dev 再次被拦截，建议在亚信安全控制台将 E:\rust-dev 加入信任/排除区。
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

# === 工具链选择：E:\rust-dev → %USERPROFILE%\rust-dev → %TEMP%\rust ===
function Get-Cargo([string]$root) {
    $env:RUSTUP_HOME = Join-Path $root 'rustup'
    $env:CARGO_HOME  = Join-Path $root 'cargo'
    return (Join-Path $root 'cargo\bin\cargo.exe')
}

$candidates = @(
    'E:\rust-dev',
    (Join-Path $env:USERPROFILE 'rust-dev'),
    (Join-Path $env:LOCALAPPDATA 'Temp\rust')
)

$cargo = $null
foreach ($root in $candidates) {
    if (Test-Path (Join-Path $root 'cargo\bin\cargo.exe')) { $cargo = Get-Cargo $root; break }
}

if (-not $cargo) {
    Write-Host "错误：找不到 cargo（已尝试 E:\rust-dev / C:\rust-dev / Temp）" -ForegroundColor Red
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
    # 若怀疑当前工具链被亚信拦截（os error 5），可手动改用其它工具链重试
    $regSrc = Join-Path $env:CARGO_HOME 'registry\src'
    if (Test-Path $regSrc) {
        Write-Host "`n如遇 registry 缓存损坏，可尝试清理：" -ForegroundColor Yellow
        Write-Host "  Remove-Item -Recurse -Force '$regSrc'" -ForegroundColor Yellow
        Write-Host "  然后重新执行本脚本" -ForegroundColor Yellow
    }
}
exit $exitCode
