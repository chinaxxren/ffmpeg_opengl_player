#version 140

in vec2 v_tex_coords;
out vec4 FragColor;

uniform sampler2D y_tex;
uniform sampler2D u_tex;
uniform sampler2D v_tex;

void main() {
    // 从YUV纹理中采样
    float y = texture(y_tex, v_tex_coords).r;
    float u = texture(u_tex, v_tex_coords).r - 0.5;
    float v = texture(v_tex, v_tex_coords).r - 0.5;

    // BT.709 YUV到RGB的转换矩阵
    const vec3 Rcoeff = vec3(1.0, 0.0, 1.5748);
    const vec3 Gcoeff = vec3(1.0, -0.1873, -0.4681);
    const vec3 Bcoeff = vec3(1.0, 1.8556, 0.0);

    // 转换到RGB
    vec3 rgb = vec3(
        dot(vec3(y, u, v), Rcoeff),
        dot(vec3(y, u, v), Gcoeff),
        dot(vec3(y, u, v), Bcoeff)
    );
    
    // 确保颜色在有效范围内
    rgb = clamp(rgb, 0.0, 1.0);
    
    FragColor = vec4(rgb, 1.0);
}