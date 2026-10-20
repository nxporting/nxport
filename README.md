## About
This is a general-purpose Nintendo Switch recomp tool. It provides a generic layer for NVN translation and the like, as well as game-specific optimization and feature code. Every optimization and feature is toggleable either in the builder, via a runtime overlay, or both.

Unfortunately, right now this only supports Apple Silicon. This will change in the near future. In the meantime, enjoy *The Legend of Zelda: Tears of the Kingdom* or *Kirby and the Forgotten Land* on your iPhone or MacBook, if you have one! Or try another game--those are just the main two I've been testing.

## AI Disclosure
People like to know if you're using AI to write something, which is understandable. Like many projects in this space, we are. However, since longevity is intended for this project, code quality is very important and will continually be iterated on.

## Getting Started
1. Dump your game and extract it in Ryujinx or something. You should have a folder that contains folder titled ExeFS, RomFS, and Logo.
2. Download nxport-builder from the releases tab. Run it, and give it the path to your extracted game. Choose a profile and build target and build!
3. If you're building for MacOS, you should now have an app you can play! If you're building for iOS, you'll have a .ipa that you can sideload with something like [Impactor](https://github.com/claration/Impactor).

## Screenshots
<img width="2970" height="1498" alt="image" src="https://github.com/user-attachments/assets/9b2aa313-67e1-4b47-ae5f-6edb2d910fdc" />

<img width="2024" height="1800" alt="image" src="https://github.com/user-attachments/assets/ebba11ae-d5bc-4bd3-91c2-a2abd9065ec9" />



## Credits
Greenlord/S41L0R - Started the project.  
Retro - Is boutta help I hear!  
Peiikai - LASRecomp code I'm going to reference. Also seems nice.  
Nintendo - For being awesome. I hope. Please don't take this down.  

## This project has been tested on:
* Tears of the Kingdom, iOS, MacOS
  * ~60-90 fps on MacBook Pro M4, almost 30 on iPhone 16e
  * Two known visual bugs, both pretty rare tbh
  * Rauru is silent
* Kirby and the Forgotten Land, iOS, MacOS
  * Two known visual bugs 
* Super Mario Wonder
  * One known visual bug
