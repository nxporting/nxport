[![Discord](https://img.shields.io/discord/1562178705512603710?logo=discord)](https://discord.gg/RRP5SRUmyj)

## About
This is a general-purpose Nintendo Switch recomp tool. It provides a generic layer for NVN translation and the like, as well as game-specific optimization and feature code. Every optimization and feature is toggleable either in the builder, via a runtime overlay, or both.

Unfortunately, right now this only supports Apple Silicon. This will change in the near future. In the meantime, enjoy *The Legend of Zelda: Tears of the Kingdom* or *Kirby and the Forgotten Land* on your iPhone or MacBook, if you have one! Or try another game--those are just the main two I've been testing.

## AI Disclosure
People like to know if you're using AI to write something, which is understandable. Like many projects in this space, we are. However, since longevity is intended for this project, code quality is very important and will continually be iterated on.

## Getting Started
1. Dump your game and extract it. You should have a folder that contains folder titled ExeFS, RomFS, and Logo. (It would be legally dubious to make this project support that extraction, so sorry.)
2. Download nxport-builder from the releases tab. Run it, and give it the path to your extracted game. Choose a profile and build target and build!
3. If you're building for MacOS, you should now have an app you can play! If you're building for iOS, you'll have a .ipa that you can sideload with something like [Impactor](https://github.com/claration/Impactor).

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
Retro - Is boutta help I hear!  
Peiikai - LASRecomp code I'm going to reference. Also seems nice.  
Nintendo - For being awesome. I hope. Please don't take this down.  

## This project has been tested on:
* Tears of the Kingdom, iOS, MacOS
  * ~60-90 fps when unlocked on MacBook Pro M4, almost 30 on iPhone 16e
  * Occasional flickering
  * Terrain tile normal zeroing
  * Rauru is silent
* Kirby and the Forgotten Land, iOS, MacOS
  * Invisible exhale
  * Some sort of shadow rounding bug? Hard to describe
* Super Mario Wonder, MacOS
  * Button prompts are "?"
* Mario Kart 8 Deluxe v3.0.4, MacOS
  * Flickering behind press-LR-to-start prompt
  * Miis facial features cull too eagerly
  * Trophy flickering
  * Some things are overly shiny
  * Selecting time trials crashes game
  * Selecting multiplayer crashes game
  * Selecting online play crashes game
  * Selecting wireless play crashes game
* Super Mario Odyssey
  * Rendering looks good so far
  * Some saving-related bug, I think. For all I know I was out of storage space
* Breath of the Wild
  * Still in the weeds of making this playable
