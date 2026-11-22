static uint4 intersect(ulong node, float4 origin, float4 direction,
                       float4 inverse) {
  uint4 descriptor = (uint4)(0u, 0x80000000u, 0xffffffffu, 0x810003ffu);
  return __builtin_amdgcn_image_bvh_intersect_ray_l(
      node, 100.f, origin, direction, inverse, descriptor);
}
__kernel void probe(__global unsigned *out) {
  float4 origin = (float4)(0.f, 0.f, -4.f, 0.f),
         inverse = (float4)(10.f, 10.f, 1.f, 0.f);
  float4 dir = (float4)(0.1f, 0.1f, 1.f, 0.f);
  *(__global uint4 *)(out + 0) =
      intersect(((ulong)(out + 64) >> 3) | 5ul, origin, dir, inverse);
  *(__global uint4 *)(out + 4) =
      intersect(((ulong)(out + 128) >> 3) | 4ul, origin, dir, inverse);
  dir = (float4)(0.f, 0.f, 1.f, 0.f);
  *(__global uint4 *)(out + 8) =
      intersect(((ulong)(out + 192) >> 3) | 0ul,
                (float4)(0.25f, 0.25f, -1.f, 0.f), dir, inverse);
  *(__global uint4 *)(out + 12) =
      intersect(((ulong)(out + 192) >> 3) | 1ul,
                (float4)(0.75f, 0.75f, -1.f, 0.f), dir, inverse);
  *(__global uint4 *)(out + 16) =
      intersect(((ulong)(out + 192) >> 3) | 0ul, (float4)(0.5f, 0.5f, 0.f, 0.f),
                dir, inverse);
  *(__global uint4 *)(out + 20) =
      intersect(((ulong)(out + 192) >> 3) | 0ul, (float4)(2.f, 2.f, -1.f, 0.f),
                dir, inverse);
  dir = (float4)(0.f, 0.f, -1.f, 0.f);
  *(__global uint4 *)(out + 24) =
      intersect(((ulong)(out + 192) >> 3) | 0ul, (float4)(0.5f, 0.5f, 0.f, 0.f),
                dir, inverse);
  *(__global uint4 *)(out + 28) =
      intersect(((ulong)(out + 192) >> 3) | 0ul, (float4)(2.f, 2.f, 1.f, 0.f),
                dir, inverse);
  *(__global uint4 *)(out + 32) =
      intersect(((ulong)(out + 192) >> 3) | 0ul,
                (float4)(0.5f, 0.5f, -1.f, 0.f), -dir, inverse);
  *(__global uint4 *)(out + 36) =
      intersect(((ulong)(out + 192) >> 3) | 0ul, (float4)(0.f, 0.5f, -1.f, 0.f),
                -dir, inverse);
  *(__global uint4 *)(out + 40) =
      intersect(((ulong)(out + 192) >> 3) | 0ul, (float4)(0.5f, 0.f, -1.f, 0.f),
                -dir, inverse);
  *(__global uint4 *)(out + 44) =
      intersect(((ulong)(out + 192) >> 3) | 0ul,
                (float4)(0.25f, 0.25f, 0.f, 0.f), -dir, inverse);
  *(__global uint4 *)(out + 48) =
      intersect(((ulong)(out + 256) >> 3) | 0ul, (float4)(2.f, 2.f, -1.f, 0.f),
                -dir, inverse);
  *(__global uint4 *)(out + 52) =
      intersect(((ulong)(out + 256) >> 3) | 0ul, (float4)(2.f, 2.f, 1.f, 0.f),
                dir, inverse);
}
