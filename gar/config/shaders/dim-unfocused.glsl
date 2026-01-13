// Dimmed unfocused window shader for gar + picom
// Applies a subtle dimming effect to unfocused windows
#version 330

uniform sampler2D tex;
uniform float opacity;
in vec2 texcoord;
out vec4 fragColor;

void main() {
    vec4 color = texture(tex, texcoord);
    // Dim by 15%
    color.rgb *= 0.85;
    fragColor = color * opacity;
}
