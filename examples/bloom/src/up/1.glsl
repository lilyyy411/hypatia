#version 330
uniform sampler2D down_final;
void do_upsample(sampler2D tex, float scale);

void main() {
    do_upsample(down_final, 0.125);
}
