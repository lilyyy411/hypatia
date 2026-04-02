#version 330
uniform sampler2D down_2;
void do_downsample(sampler2D tex, float scale);

void main() {
    do_downsample(down_2, 0.25);
}
