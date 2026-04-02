#version 330
out vec4 color;
in vec2 tex_coords;

uniform sampler2D texture_1;
uniform sampler2D processed;

vec4 grade(vec4 px, sampler2D lut);

void main() {
    vec4 orig = texture(processed, tex_coords);
    color = grade(clamp(orig, vec4(0), vec4(1)), texture_1);
}
