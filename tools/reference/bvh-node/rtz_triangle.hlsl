// Experimental scalar control for finite, nonzero-direction rays hitting
// uncompressed TRI0/TRI1 leaves. It is deliberately not a runtime path.
// Each single-precision operation is rounded toward zero using a binary64
// intermediate. This is expensive and does not establish full ISA fidelity.
float tb_rtz(double value) {
    float rounded = (float)value;
    if (abs((double)rounded) > abs(value))
        rounded = asfloat(asuint(rounded) - 1u);
    return rounded;
}
float tb_rtz_add(float a, float b) { return tb_rtz((double)a + (double)b); }
float tb_rtz_sub(float a, float b) { return tb_rtz((double)a - (double)b); }
float tb_rtz_mul(float a, float b) { return tb_rtz((double)a * (double)b); }
float tb_rtz_cross2(float2 a, float2 b) {
    return tb_rtz_sub(tb_rtz_mul(a.x, b.y), tb_rtz_mul(a.y, b.x));
}
float4 tb_bvh_triangle_rtz(uint node[32], uint side, float3 origin, float3 direction) {
    uint ia = side == 0 ? 0 : 1;
    uint ib = side == 0 ? 1 : 3;
    uint ic = 2;
    float3 a = asfloat(uint3(node[3 * ia], node[3 * ia + 1], node[3 * ia + 2]));
    float3 b = asfloat(uint3(node[3 * ib], node[3 * ib + 1], node[3 * ib + 2]));
    float3 c = asfloat(uint3(node[3 * ic], node[3 * ic + 1], node[3 * ic + 2]));
    float3 x = float3(tb_rtz_sub(a.x, origin.x), tb_rtz_sub(a.y, origin.y), tb_rtz_sub(a.z, origin.z));
    float3 y = float3(tb_rtz_sub(b.x, origin.x), tb_rtz_sub(b.y, origin.y), tb_rtz_sub(b.z, origin.z));
    float3 z = float3(tb_rtz_sub(c.x, origin.x), tb_rtz_sub(c.y, origin.y), tb_rtz_sub(c.z, origin.z));
    float3 magnitude = abs(direction);
    uint axis = magnitude.x >= magnitude.y && magnitude.x >= magnitude.z ? 0u :
                magnitude.y >= magnitude.z ? 1u : 2u;
    uint px = (axis + 1) % 3, py = (axis + 2) % 3;
    float2 projectedX = float2(tb_rtz_sub(tb_rtz_mul(x[px], direction[axis]), tb_rtz_mul(x[axis], direction[px])),
                               tb_rtz_sub(tb_rtz_mul(x[py], direction[axis]), tb_rtz_mul(x[axis], direction[py])));
    float2 projectedY = float2(tb_rtz_sub(tb_rtz_mul(y[px], direction[axis]), tb_rtz_mul(y[axis], direction[px])),
                               tb_rtz_sub(tb_rtz_mul(y[py], direction[axis]), tb_rtz_mul(y[axis], direction[py])));
    float2 projectedZ = float2(tb_rtz_sub(tb_rtz_mul(z[px], direction[axis]), tb_rtz_mul(z[axis], direction[px])),
                               tb_rtz_sub(tb_rtz_mul(z[py], direction[axis]), tb_rtz_mul(z[axis], direction[py])));
    float u = tb_rtz_cross2(projectedZ, projectedY);
    float v = tb_rtz_cross2(projectedX, projectedZ);
    float w = tb_rtz_cross2(projectedY, projectedX);
    float su = tb_rtz_mul(u, direction[axis]);
    float sv = tb_rtz_mul(v, direction[axis]);
    float sw = tb_rtz_mul(w, direction[axis]);
    float denominator = tb_rtz_add(tb_rtz_add(su, sv), sw);
    float numerator = tb_rtz_add(tb_rtz_add(tb_rtz_mul(u, x[axis]), tb_rtz_mul(v, y[axis])), tb_rtz_mul(w, z[axis]));
    uint selector = (node[15] >> (8 * side)) & 0xffu;
    float bary[3] = {su, sv, sw};
    return float4(numerator, denominator, bary[selector & 3u], bary[(selector >> 2) & 3u]);
}
