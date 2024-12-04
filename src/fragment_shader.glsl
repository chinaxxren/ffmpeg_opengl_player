#version 330 core
in vec2 v_texcoord;
out vec4 FragColor;

uniform sampler2D y_tex;
uniform sampler2D u_tex;
uniform sampler2D v_tex;

void main() {
    float y = texture(y_tex, v_texcoord).r;
    float u = texture(u_tex, v_texcoord).r - 0.5;
    float v = texture(v_tex, v_texcoord).r - 0.5;

    // YUV to RGB conversion
    vec3 rgb;
    rgb.r = y + 1.402 * v;
    rgb.g = y - 0.344 * u - 0.714 * v;
    rgb.b = y + 1.772 * u;

    FragColor = vec4(rgb, 1.0);
}