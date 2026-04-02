#version 330
uniform sampler2D mapped;
void do_downsample(sampler2D tex, float scale);

void main() {
    do_downsample(mapped, 1.0);
}
