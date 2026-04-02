#version 330

vec3 srgb_to_linear(vec3 col) {
    return mix(pow((col + 0.055) / 1.055, vec3(2.4)), col / 12.92, lessThan(col, vec3(0.04045)));
}

vec3 linear_to_srgb(vec3 col) {
    return mix(1.055 * pow(col, vec3(1 / 2.4)) - 0.055, col * 12.92, lessThan(col, vec3(0.0031308)));
}

float luminance(vec3 col) {
    return dot(col, vec3(0.2126729, 0.7151522, 0.0721750));
}

#define TWEAK 0.75
vec3 tm_lottes(vec3 color) {
    return color / (1.0 + TWEAK * luminance(color));
}

vec3 inverse_lottes(vec3 color) {
    return color / (1.0 - TWEAK * luminance(color));
}

// ease inout quart
float ease(float x) {
    vec2 vs = vec2(x, x - 1.0);
    vs += vs;
    vs *= vs;
    vs *= vs;
    vs.y = 2.0 - vs.y;
    vs *= 0.5;
    return x < 0.5 ? vs.x : vs.y;
}

#define MAXCOLOR 15.0
#define COLORS 16.0
#define WIDTH 256.0
#define HEIGHT COLORS

vec4 grade(vec4 px, sampler2D lut) {
    float cell = px.b * MAXCOLOR;

    float cell_l = floor(cell) / COLORS;
    float cell_h = ceil(cell) / COLORS;

    float half_px_x = 0.5 / WIDTH;
    float half_px_y = 0.5 / HEIGHT;
    float r_offset = half_px_x + px.r * (MAXCOLOR / WIDTH);
    float g_offset = half_px_y + px.g * (MAXCOLOR / HEIGHT);
    vec2 base = vec2(r_offset, g_offset);
    vec2 lut_pos_l = vec2(cell_l + r_offset, g_offset);
    vec2 lut_pos_h = vec2(cell_h + r_offset, g_offset);

    vec3 graded_color_l = texture2D(lut, lut_pos_l).rgb;
    vec3 graded_color_h = texture2D(lut, lut_pos_h).rgb;

    return vec4(mix(graded_color_l, graded_color_h, fract(cell)), px.a);
}
