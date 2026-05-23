@echo off
REM KORE Node.js Binding Build Script (Windows)
REM Builds the npm package

echo.
echo 🔨 Building KORE Node.js bindings...
echo.

REM Install dependencies
call npm install
if errorlevel 1 exit /b 1

REM Build release binaries
call npm run build
if errorlevel 1 exit /b 1

REM Run tests
echo.
echo 🧪 Running tests...
echo.
call npm test
if errorlevel 1 exit /b 1

REM Check authentication
if not exist "%USERPROFILE%\.npmrc" (
    echo.
    echo ⚠️  No .npmrc found. Please run 'npm login' first.
    echo.
    exit /b 1
)

REM Publish to npm
echo.
echo 📦 Publishing to npm...
echo.
call npm publish

echo.
echo ✅ Successfully published kore-fileformat
echo 🎉 Check it out: https://www.npmjs.com/package/kore-fileformat
echo.
