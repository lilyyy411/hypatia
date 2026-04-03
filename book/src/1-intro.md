# Introduction

[Hypatia](<https://github.com/lilyyy411/hypatia>) is a feature-rich modern interactive live wallpaper application for Wayland.
It allows users to not only play videos on their desktop wallpaper, but also have custom post-processing effects
that respond to input. 

## Feature Overview 
- Play videos/static images on the desktop wallpaper
- Multiple monitor support
- Custom multi-stage GLSL shader pipeline configured through KDL
- Fading out/in wallpapers when (un)focused
- Custom fade effects
- Interaction based mouse cursor position
- Changing wallpapers with a transition effect (WIP)
- Does not sell your data to Fraser (if you know you know)

## Platform Support
Hypatia supports basically any Linux distribution running a Wayland compositor 
that has support for the [layer shell protocol](<https://wayland.app/protocols/wlr-layer-shell-unstable-v1>).
It is currently tested on my personal laptop running Linux Mint with the [Niri](<https://github.com/niri-wm/niri>) compositor 
to ensure that it can run even with ancient packages, but many users run it on various other distros without any issues.
