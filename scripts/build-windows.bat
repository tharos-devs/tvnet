@echo off
setlocal

set "ROOT=%~dp0.."
set "OUTPUT=%ROOT%\dist"

where cargo >nul 2>nul
if errorlevel 1 (
    echo cargo est requis. Installez Rust stable via https://rustup.rs/, puis relancez ce script.
    exit /b 1
)

pushd "%ROOT%"
cargo build --release
if errorlevel 1 (
    echo.
    echo La compilation a echoue.
    popd
    exit /b 1
)

if not exist "%OUTPUT%" mkdir "%OUTPUT%"
copy /Y "target\release\tvnet.exe" "%OUTPUT%\TVnet.exe" >nul
if errorlevel 1 (
    echo Impossible de copier l'executable dans dist.
    popd
    exit /b 1
)
copy /Y "packaging\appimage\tvnet.ico" "%OUTPUT%\TVnet.ico" >nul

echo Executable cree : %OUTPUT%\TVnet.exe
popd
