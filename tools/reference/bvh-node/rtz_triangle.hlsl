// Experimental scalar control for finite, nonzero-direction rays testing
// uncompressed TRI0/TRI1 leaves. It is deliberately not a runtime path.
// Each single-precision operation is rounded toward zero. The default uses a
// binary64 intermediate. The optional float-only candidate is experimental;
// neither path establishes full ISA fidelity.
#ifdef TUXBOX_RTZ_FLOAT32
float tb_rtz_adjust(float rounded, float residual) {
    if ((rounded > 0.0 && residual < 0.0) || (rounded < 0.0 && residual > 0.0))
        return asfloat(asuint(rounded) - 1u);
    return rounded;
}
float tb_rtz_add(float a, float b) {
    precise float sum = a + b;
    precise float recoveredB = sum - a;
    precise float residual = (a - (sum - recoveredB)) + (b - recoveredB);
    return tb_rtz_adjust(sum, residual);
}
float tb_rtz_sub(float a, float b) { return tb_rtz_add(a, -b); }
float tb_rtz_mul(float a, float b) {
    precise float product = a * b;
    precise float splitA = 4097.0 * a;
    precise float highA = splitA - (splitA - a);
    precise float lowA = a - highA;
    precise float splitB = 4097.0 * b;
    precise float highB = splitB - (splitB - b);
    precise float lowB = b - highB;
    precise float error1 = product - highA * highB;
    precise float error2 = error1 - lowA * highB;
    precise float error3 = error2 - highA * lowB;
    precise float residual = lowA * lowB - error3;
    return tb_rtz_adjust(product, residual);
}
#else
float tb_rtz(double value) {
    float rounded = (float)value;
    if (abs((double)rounded) > abs(value))
        rounded = asfloat(asuint(rounded) - 1u);
    return rounded;
}
float tb_rtz_add(float a, float b) { return tb_rtz((double)a + (double)b); }
float tb_rtz_sub(float a, float b) { return tb_rtz((double)a - (double)b); }
float tb_rtz_mul(float a, float b) { return tb_rtz((double)a * (double)b); }
#endif
float tb_rtz_cross2(float2 a, float2 b) {
    return tb_rtz_sub(tb_rtz_mul(a.x, b.y), tb_rtz_mul(a.y, b.x));
}
bool tb_rtz_edge_owned(float2 a, float2 b, float2 interior, float axisDirection) {
    float2 edge = float2(tb_rtz_sub(b.x, a.x), tb_rtz_sub(b.y, a.y));
    float2 inward = float2(-edge.y, edge.x);
    float2 offset = float2(tb_rtz_sub(interior.x, a.x), tb_rtz_sub(interior.y, a.y));
    float orientation = tb_rtz_add(tb_rtz_mul(inward.x, offset.x), tb_rtz_mul(inward.y, offset.y));
    if (orientation < 0.0)
        inward = -inward;
    return inward.x > 0.0 || (inward.x == 0.0 && tb_rtz_mul(inward.y, axisDirection) < 0.0);
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
    [unroll] for (uint i = 0; i < 3; ++i)
        if (bary[i] == 0.0) bary[i] = 0.0;
    if (denominator == 0.0)
        return float4(asfloat(0x7f800000u), 0.0, bary[selector & 3u], bary[(selector >> 2) & 3u]);
    bool miss = (numerator < 0.0 && denominator > 0.0) ||
                (numerator > 0.0 && denominator < 0.0);
    [unroll] for (uint i = 0; i < 3; ++i)
        miss = miss || (bary[i] < 0.0 && denominator > 0.0) ||
                       (bary[i] > 0.0 && denominator < 0.0);
    float projectionSign = direction[axis] < 0.0 ? -1.0 : 1.0;
    float2 pa = projectedX * projectionSign;
    float2 pb = projectedY * projectionSign;
    float2 pc = projectedZ * projectionSign;
    if (u == 0.0 && !tb_rtz_edge_owned(pb, pc, pa, direction[axis])) miss = true;
    if (v == 0.0 && !tb_rtz_edge_owned(pa, pc, pb, direction[axis])) miss = true;
    if (w == 0.0 && !tb_rtz_edge_owned(pa, pb, pc, direction[axis])) miss = true;
    if (miss)
        numerator = asfloat(0x7f800000u | (asuint(denominator) & 0x80000000u));
    return float4(numerator, denominator, bary[selector & 3u], bary[(selector >> 2) & 3u]);
}
