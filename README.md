# Hypatia - Live interactive desktop wallpapers for Wayland

Disclaimer: This project is not even close to feature-complete yet and is likely buggy. I still need to write documentation.

Hypatia is a modern feature-rich interactive live wallpaper application for Wayland compositors that support the layer shell protocol.
It allows users to not only play videos on their desktop wallpaper, but also have custom responsive post-processing effects. 

## Feature Overview
- Play videos/static images on the desktop wallpaper
- Configurable fading in/out of audio + playing/pausing when hovering/stop hovering over the desktop
- Custom multi-stage GLSL shader pipeline configured through KDL
- Change wallpapers with a transition effect (not implemented yet)
- Does not sell your data to Fraser (if you know you know)

Because of the extensive customization that Hypatia allows (see the `documentation` folder), it can also be used as a tool similar to ShaderToy (but local) 
to be able to "just quickly test an effect." 

## Installing
There are no prebuilt packages currently distributed. The only runtime dependency is the `mpv` video player and its library `libmpv`. 


If you're on a Debian-based distribution, you can use `cargo-deb` to build and install a deb package:
```sh
cargo deb --install
```

Alternatively, you can build `hypatia` through the standard `cargo` build process:
```sh
git clone https://github.com/lilyyy411/hypatia.git
cd ./hypatia
cargo install --path=.
```


## TODO
- Implement IPC control
- Implement transitions, custom transition shaders
- Support screenshots
- Give access to time-based uniforms in shaders
- Allow playing audio files independent of video 
- Allow setting custom mpv options
- Add audio visualization support?
- Allow custom vertex buffers? 
- Allow changing intermediate texture storage format? (currently RGBF16)

## FAQ
- Q: Why? You're basically reinventing wallpaper engine and there are so many Wallpaper Engine alternatives for Linux out there.
  - A: Not many support the features I actually want. There are projects like `swaybg`, `swww`, `hidamari`, and `mpvpaper` that can play videos on the desktop wallpaper, but they simply play videos and don't have interaction.
    There's also several direct ports of Wallpaper Engine that "work" (in a very loose sense of the word) on a bunch of different X11 desktops, but they mostly don't work on Wayland and are also extremely buggy as hell.
    There's also the KDE Plasma Wallpaper Engine plugin, but the last time I tried it, it constantly crashed and most scene wallpapers simply did not work and turned into a buggy mess covered in glitching black triangles.
    Besides, that only works on KDE Plasma and I don't use KDE Plasma anymore.
  
    I could go on for hours about the quirks of the alternatives and how they simply just suck in their own ways.
  
    TLDR; essentially, most alternatives have at least one of the following flaws:
      - X11 only
      - Does not allow user interaction
      - Does not allow custom effects
      - Extremely buggy

- Q: When will you add X11 support?
  - A: Never because of limitations with X11 itself and quirkiness of implementation details. There is no universal way to truly have a background layer window on X11 despite the fact there's a hint for displaying below other windows.
    X11 doesn't have anything nearly similar to the wlroots layer shell protocol and the way to actually get windows to display properly on the desktop "layer" is extremely dependent
    on the WM. Some WMs (like Cinnamon) simply refuse to make the window appear on the desktop "layer" unless you destroy all of the input regions for the window,
    making it impossible to properly support interaction, which is one of the major selling points of Hypatia. 
  
    Trust me. I've tried to make this project for X11 in the past and after 2000 lines of code, I realized that it was simply infeasible for a mere mortal to achieve.
- Q: What's with the name?
  - A: Hypatia is named after a character from the game Path to Nowhere. Hypatia (the character) is a devout academic researcher and inventor that has 
     the ability to manipulate matter upon physical contact, changing its properties and composition. Hypatia (the application) encourages users to "invent" their
     own wallpapers / effects and fully manipulate their desktop experience. It's also kind of ironic to name an interactive wallpaper program after someone who does not like to interact 
     with others and has a terrible sense of humor.
