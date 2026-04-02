#version 330
uniform sampler2D up_2;
void do_upsample(sampler2D tex, float scale);

void main() {
    do_upsample(up_2, 0.5);
}
