#version 450

layout(push_constant) uniform PushConstants {
    vec2 offset;
    vec2 size;
    vec2 screen_size;
    vec4 color;
} pc;

layout(location = 0) out vec4 out_color;

void main() {
    out_color = pc.color;
}
