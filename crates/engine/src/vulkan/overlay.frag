#version 450
// The performance overlay (overlay.rs).
layout(set = 0, binding = 0) uniform texture2D atlas;
layout(set = 0, binding = 1) uniform sampler atlas_sampler;
layout(location = 0) in vec2 v_uv;
layout(location = 1) in vec4 v_color;
layout(location = 0) out vec4 out_color;

void main() {
    out_color = v_color * texture(sampler2D(atlas, atlas_sampler), v_uv);
}
