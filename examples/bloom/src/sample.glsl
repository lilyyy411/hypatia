#version 330

in vec2 tex_coords;
out vec4 color;

uniform sampler2D tex;
uniform vec2 viewport_dims;
uniform vec2 cursor_pos;
float luminance(vec3 pixel);
vec3 linear_to_srgb(vec3 col);

float karis(vec3 col)
{
    float luma = luminance(linear_to_srgb(col)) * 0.0625;
    return 1.0f / (1.0f + luma);
}
vec4 downsample(sampler2D tex, vec2 uv, vec2 offset, bool is_first) {
    vec2 o = offset;

    if (is_first) {
        vec4 sum = texture(tex, uv) * 4.0;
        sum += texture(tex, uv + vec2(-o.x, -o.y));
        sum += texture(tex, uv + vec2(o.x, -o.y));
        sum += texture(tex, uv + vec2(-o.x, o.y));
        sum += texture(tex, uv + vec2(o.x, o.y));
        sum /= sum.a;
        return vec4(sum.rgb * karis(sum.rgb), 1.0);
    } else {
        vec4 sum = texture(tex, uv) * 4.0;
        sum += texture(tex, uv + vec2(-o.x, -o.y));
        sum += texture(tex, uv + vec2(o.x, -o.y));
        sum += texture(tex, uv + vec2(-o.x, o.y));
        sum += texture(tex, uv + vec2(o.x, o.y));
        return sum / sum.a;
    }
}

vec4 upsample(sampler2D tex, vec2 uv, vec2 offset) {
    vec2 o = offset;
    vec4 sum = vec4(0.0);

    // Four edge centers
    sum += texture(tex, uv + vec2(-o.x * 2.0, 0.0));
    sum += texture(tex, uv + vec2(o.x * 2.0, 0.0));
    sum += texture(tex, uv + vec2(0.0, -o.y * 2.0));
    sum += texture(tex, uv + vec2(0.0, o.y * 2.0));

    // Four diagonal corners
    sum += texture(tex, uv + vec2(-o.x, o.y)) * 2.0;
    sum += texture(tex, uv + vec2(o.x, o.y)) * 2.0;
    sum += texture(tex, uv + vec2(-o.x, -o.y)) * 2.0;
    sum += texture(tex, uv + vec2(o.x, -o.y)) * 2.0;

    return sum / sum.a;
}

vec2 half_pixel(float scale) {
    return 0.5 / (viewport_dims * scale);
}

float offset() {
    // return distance(tex_coords, cursor_pos) * 2.5;
    return 1.0;
}

void do_downsample(sampler2D tex, float scale) {
    color = downsample(tex, tex_coords, offset() * half_pixel(scale), scale == 1.0);
}

void do_upsample(sampler2D tex, float scale) {
    color = upsample(tex, tex_coords, offset() * half_pixel(scale));
}
