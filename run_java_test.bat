@echo off
cd /d C:\Users\skathera\Downloads\asistent\kore
echo === COMPILING KoreTest.java ===
javac --enable-preview --release 21 KoreTest.java 2>&1
echo.
echo === RUNNING KoreTest ===
java --enable-native-access=ALL-UNNAMED --enable-preview -cp . KoreTest
echo.
echo === JAVA TEST COMPLETE ===
