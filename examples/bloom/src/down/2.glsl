#version 330
uniform sampler2D down_1;
void do_downsample(sampler2D tex, float scale);

void main() {
    do_downsample(down_1, 0.5);
}
