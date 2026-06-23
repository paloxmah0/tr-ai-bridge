@echo off
REM ============================================
REM  Trading App - Quick Start (no recompiling!)
REM  Just double-click this file or run: start.bat
REM ============================================

cd /d "C:\Users\san\trading-backend"

echo Starting Trading App...
echo.
echo Open your browser to: http://localhost:8080
echo.
echo Press Ctrl+C to stop the server.
echo.

target\debug\trading-backend.exe

pause
