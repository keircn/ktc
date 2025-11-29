#version 450

layout(push_constant) uniform PushConstants {
    vec2 offset;
    vec2 size;
    vec2 screen_size;
    vec4 color;
} pc;

layout(location = 0) out vec2 v_texcoord;

void main() {
    // Generate quad vertices
    vec2 positions[6] = vec2[](
        vec2(0.0, 0.0),
        vec2(1.0, 0.0),
        vec2(0.0, 1.0),
        vec2(1.0, 0.0),
        vec2(1.0, 1.0),
        vec2(0.0, 1.0)
    );
    
    vec2 pos = positions[gl_VertexIndex];
    v_texcoord = pos;
    
    // Transform to screen space
    vec2 screen_pos = (pos * pc.size + pc.offset) / pc.screen_size * 2.0 - 1.0;
    gl_Position = vec4(screen_pos, 0.0, 1.0);
}
