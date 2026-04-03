# Basic Usage

Hypatia works differently compared to projects like [`mpvpaper`](<https://github.com/GhostNaN/mpvpaper/tree/master>) in that 
it requires a **pipeline** to render a wallpaper and it cannot just play a video file directly without additional work. 
The primary entrypoint for a Hypatia wallpaper is a KDL pipeline config file, typically named `pipeline.kdl`,
that configures the shader pipeline and locates assets used by the wallpaper. 

In this chapter, I will cover Hypatia's wallpaper player's commandline interface 
and general features the perspective of a casual user that only cares about using wallpapers. Developing 
custom wallpapers will come in a later chapter, as that requires a bit more advanced knowledge.
