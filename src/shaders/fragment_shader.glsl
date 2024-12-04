#version 140

in vec2 v_tex_coords;
out vec4 color;

uniform sampler2D y_tex;
uniform sampler2D u_tex;
uniform sampler2D v_tex;

// BT.601 标准的 YUV 到 RGB 转换矩阵
const vec3 Rcoeff = vec3(1.164, 0.000, 1.596);
const vec3 Gcoeff = vec3(1.164, -0.392, -0.813);
const vec3 Bcoeff = vec3(1.164, 2.017, 0.000);

void main() {
    // 从纹理中采样 YUV 值
    float y = texture(y_tex, v_tex_coords).r;
    float u = texture(u_tex, v_tex_coords).r;
    float v = texture(v_tex, v_tex_coords).r;
    
    // 调整 YUV 值的范围
    y = (y - 16.0/255.0) * (255.0/219.0);
    u = (u - 128.0/255.0) * (255.0/224.0);
    v = (v - 128.0/255.0) * (255.0/224.0);
    
    // 转换到 RGB
    float r = dot(vec3(y, u, v), Rcoeff);
    float g = dot(vec3(y, u, v), Gcoeff);
    float b = dot(vec3(y, u, v), Bcoeff);
    
    // 输出颜色
    color = vec4(clamp(vec3(r, g, b), 0.0, 1.0), 1.0);
}