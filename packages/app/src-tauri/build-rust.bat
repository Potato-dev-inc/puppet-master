@echo off
REM Wrapper that sets up MSVC + cargo PATH for Puppet Master builds.
REM Usage: build-rust.bat [cargo args...]
REM        build-rust.bat check
REM        build-rust.bat build --release

call "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul
set "PATH=C:\Users\Ren-pc\.cargo\bin;%PATH%"
cargo %*
