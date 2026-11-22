// Experimental per-instance encoder. This is a standalone control and is not
// wired into the runtime. The caller supplies already-validated BLAS metadata.
// Float32 matrix inversion can diverge from the CPU reference for ill-conditioned
// transforms; conservative bounds and GPU-produced input are not covered here.
// Status: 0 = disabled, 1 = encoded, 2 = invalid/unsupported input.
uint tb_encode_instance(uint src[16], uint root, uint compression, uint index,
                        uint parent, out uint dst[32]) {
    for (uint n = 0; n < 32; ++n) dst[n] = 0;
    if ((src[12] >> 24) == 0 || (src[14] | src[15]) == 0) return 0;
    if ((src[14] & 255) != 0 || src[15] >= 256 || root >= (1u << 29) ||
        ((root & 7) != 0 && (root & 7) != 4 && (root & 7) != 5) ||
        compression != 0x007f7f7f) return 2;
    float m[12];
    for (uint n = 0; n < 12; ++n) {
        m[n] = asfloat(src[n]);
        if (!isfinite(m[n])) return 2;
    }
    float a=m[0], b=m[1], c=m[2], d=m[4], e=m[5], f=m[6], g=m[8], h=m[9], i=m[10];
    float det = a*(e*i-f*h) - b*(d*i-f*g) + c*(d*h-e*g);
    if (det == 0 || !isfinite(det)) return 2;
    float inv[9] = {(e*i-f*h)/det, (c*h-b*i)/det, (b*f-c*e)/det,
                    (f*g-d*i)/det, (a*i-c*g)/det, (c*d-a*f)/det,
                    (d*h-e*g)/det, (b*g-a*h)/det, (a*e-b*d)/det};
    for (uint n = 0; n < 9; ++n) if (!isfinite(inv[n])) return 2;
    float3 translation = float3(m[3], m[7], m[11]);
    float3 t = -float3(dot(float3(inv[0],inv[1],inv[2]),translation),
                       dot(float3(inv[3],inv[4],inv[5]),translation),
                       dot(float3(inv[6],inv[7],inv[8]),translation));
    if (!all(isfinite(t))) return 2;
    for (uint n = 0; n < 14; ++n) dst[n] = src[n];
    bool identity = a == 1 && e == 1 && i == 1 && b == 0 && c == 0 &&
                    d == 0 && f == 0 && g == 0 && h == 0;
    dst[14] = root | (identity ? 0x80000000 : 0);
    dst[15] = (src[14] >> 8) | (src[15] << 24);
    dst[16] = asuint(t.x); dst[17] = asuint(t.y); dst[18] = asuint(t.z);
    for (uint n = 0; n < 9; ++n) dst[19+n] = asuint(inv[n]);
    dst[28] = index; dst[29] = parent; dst[30] = compression;
    return 1;
}
