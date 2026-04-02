#version 330
vec3 linear_to_srgb(vec3 x);
float luminance(vec3 col);
float ease(float x);
vec3 tm_lottes(vec3 x);

in vec2 tex_coords;
out vec4 color;

uniform sampler2D blurred;
uniform sampler2D mapped;
uniform vec2 cursor_pos;
float offset();
void main() {
    float bloom_strength = 0.5 / (1 + 2.0 * distance(tex_coords, cursor_pos));
    vec3 orig = tm_lottes(texture(mapped, tex_coords).rgb);
    vec4 blurred = texture(blurred, tex_coords);
    vec3 out_color = mix(orig, blurred.rgb, bloom_strength);
    color = vec4(linear_to_srgb(out_color), 1.0);
}
