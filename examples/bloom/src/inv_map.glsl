#version 330
out vec4 color;

in vec2 tex_coords;
uniform sampler2D texture_0;
uniform float focus_fade;
uniform vec2 viewport_dims;
uniform vec2 cursor_pos;

float ease(float x);
vec3 inverse_lottes(vec3 color);
float luminance(vec3 col);
vec3 srgb_to_linear(vec3 col);

float vignette_multiplier(vec2 coord, float radius, float softness) {
    vec2 position = coord - vec2(0.5);
    float dist = length(position);
    return smoothstep(radius, radius - softness, dist);
}

void main() {
    float radius = mix(0.7, 1.0, ease(focus_fade));
    float softness = 0.2;
    float vignette = vignette_multiplier(tex_coords, radius, softness);
    color = vec4(inverse_lottes(srgb_to_linear(texture(texture_0, tex_coords).rgb)), 1.0);
    color = mix(color, color * vignette, 0.8);
}
