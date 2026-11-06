[![Discord](https://img.shields.io/discord/1562178705512603710?logo=discord)](https://discord.gg/RRP5SRUmyj)

## About
This tool prepares Nintendo Switch games for direct execution on x86 Windows and ARM64 MacOS/iOS (more to come at some point). It's much more performant than emulation, however it's a newer tool, so do not expect it to be perfect.

## How do I get set up?
1. Dump your game and extract it. You should have a folder that contains folders titled ExeFS, RomFS, and Logo. (It would be legally dubious to make this project support that extraction, so sorry[^1].)
2. Download nxport-builder from the releases tab (okay there's no release yet, for now clone and build). Run it, and give it the path to your extracted game. Choose a build target and build!
3. After a long-ish wait you should have your processed game in the output location. Building for Windows, this is a folder with an executable that you can run to start the game. For MacOS, this is a .app package that you can run. For iOS, this is an .ipa file that you can sideload with something like [Impactor](https://github.com/claration/Impactor).
4. Have fun! Report bugs!

## How does it work?
Many switch games dynamically link against an SDK provided by Nintendo. Reimplementing parts of this SDK turns out to be enough to replace dependence on a Switch system with dependence on some non-Switch system. Of course, we also need to recompile everything for non-ARM64 systems, but that's not too hard.

This whole process is pretty generalizable, which is nice. There however are some cases where we want to be a little bit more specific in our approach, so this tool also contains a library of game-specific patches. None of these game-specific patches will ever be necessary for compatibility, but they can provide niceties like dynamic aspect ratio and NVN skipping.

Traditional mods are also supported well (I hope! I'll do more comprehensive testing in the coming days). This includes ExeFS patches and exlaunch[^2] mods as well as RomFS mods.

## AI Disclosure
People like to know if you're using AI to write something, which is understandable. Like many projects in this space, we are. However, since longevity is intended for this project, code quality is very important and will continually be iterated on.

## Screenshots
<table>
  <tr>
    <td align="center" width="50%">
      <img alt="image" src="https://github.com/user-attachments/assets/9b2aa313-67e1-4b47-ae5f-6edb2d910fdc" width="100%" />
    </td>
    <td align="center" width="50%">
      <img alt="gif3" src="https://github.com/user-attachments/assets/47e421e7-0981-4c9b-afc8-f28d41b041ad" width="100%" />
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <img alt="image" src="https://github.com/user-attachments/assets/ebba11ae-d5bc-4bd3-91c2-a2abd9065ec9" width="100%" />
    </td>
    <td align="center" width="50%">
      <img alt="image" src="https://github.com/user-attachments/assets/8536adf0-fa40-4723-b570-cf982cf0d0f0" width="100%" />
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <img alt="Screenshot 2026-10-20 at 12 28 50 PM" src="https://github.com/user-attachments/assets/7ec66b0c-be40-43ad-9aa8-eb93d4e878f8" width="100%" />
    </td>
    <td align="center" width="50%">
      <img width="2784" height="1720" alt="image" src="https://github.com/user-attachments/assets/ece0badf-867f-4c1e-a239-aa20f571fb03" width="100%" />
    </td>
  </tr>
</table>


## Credits
Greenlord/S41L0R - Started the project.  
Retro - Contributor. 
Peiikai - Offered LASRecomp code for referencing. Also seems nice.  
Nintendo - For being awesome. I hope. Please don't take this down.  

## This project has been tested on:
* Tears of the Kingdom, Windows, MacOS, iOS
  * Runs well. A few visual bugs here and there.
* Kirby and the Forgotten Land, MacOS, iOS
  * Runs very well so far. A few small visual bugs. 
* Super Mario Wonder, MacOS
  * Runs very well so far.
* Mario Kart 8 Deluxe v3.0.4, MacOS
  * Runs very well. A few small visual bugs.
* Super Mario Odyssey, MacOS
  * Runs very well so far.
* Breath of the Wild, MacOS
  * Still in the weeds of making this playable.

[^1]: I know this is a very common thing to provide, but anti-circumvention laws have been weaponized in the past to take down a certain emulator that we all know and love. Recomps are really making a stir right now, so I won't include or direct users to any code that provides this functionality.
[^2]: Exlaunch will probably never be supported (or at least performant) on non-jailbroken iOS devices due to W^X limitations.
