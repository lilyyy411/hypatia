#version 330
uniform sampler2D up_1;
void do_upsample(sampler2D tex, float scale);

void main() {
    do_upsample(up_1, 0.25);
}
