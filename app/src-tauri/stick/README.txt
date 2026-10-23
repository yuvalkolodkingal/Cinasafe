CINASAFE
========

This stick holds an encrypted movie and TV library made with Cinasafe.
Everything in the hidden .cinasafe folder is encrypted: films, episodes,
subtitles, posters and the library itself. Without the password (or the
recovery key) it can't be read.

To open it, run the player for your computer, straight from the stick.
Nothing needs to be installed, and nothing is decrypted to the computer's disk.

  Windows (64-bit)       Cinasafe-Windows.exe
                         (Windows may warn that the app is unrecognised:
                         choose "More info", then "Run anyway".)
  Linux (x86-64, ARM64)  run-linux.sh, or the matching
                         Cinasafe-Linux-<arch>.AppImage

Pulling the stick out locks the vault. So does the Lock button, the idle
timer, and putting the computer to sleep.

Cinasafe is for media you own. It is free software under the Apache-2.0
licence: https://github.com/yuvalkolodkingal/cinasafe

Do not delete or rename anything inside .cinasafe.
