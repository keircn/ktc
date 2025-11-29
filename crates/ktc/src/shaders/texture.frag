#version 450

layout(push_constant) uniform PushConstants {
    vec2 offset;
    vec2 size;
    vec2 screen_size;
    vec4 color;
} pc;

layout(set = 0, binding = 0) uniform sampler2D tex;

layout(location = 0) in vec2 v_texcoord;
layout(location = 0) out vec4 out_color;

void main() {
    vec4 texel = texture(tex, v_texcoord);
    out_color = vec4(texel.rgb, 1.0);
}
