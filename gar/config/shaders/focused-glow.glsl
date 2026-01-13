// Focused window glow shader for gar + picom
// Applies a subtle brightness boost to focused windows
#version 330

uniform sampler2D tex;
uniform float opacity;
in vec2 texcoord;
out vec4 fragColor;

void main() {
    vec4 color = texture(tex, texcoord);
    // Subtle brightness boost for focused windows
    color.rgb *= 1.05;
    fragColor = color * opacity;
}
