#version 330
out vec4 color;

in vec2 v_tex_coords;

uniform sampler2D texture_0;
uniform vec2 viewport_dims;

void main() {
    color = texture(texture_0, v_tex_coords);
}
