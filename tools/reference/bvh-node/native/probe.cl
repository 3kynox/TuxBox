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
      intersect(((ulong)(out + 1024) >> 3) | 5ul, origin, dir, inverse);
  *(__global uint4 *)(out + 4) =
      intersect(((ulong)(out + 1088) >> 3) | 4ul, origin, dir, inverse);
  dir = (float4)(0.f, 0.f, 1.f, 0.f);
  *(__global uint4 *)(out + 8) =
      intersect(((ulong)(out + 1152) >> 3) | 0ul,
                (float4)(0.25f, 0.25f, -1.f, 0.f), dir, inverse);
  *(__global uint4 *)(out + 12) =
      intersect(((ulong)(out + 1152) >> 3) | 1ul,
                (float4)(0.75f, 0.75f, -1.f, 0.f), dir, inverse);
  *(__global uint4 *)(out + 16) =
      intersect(((ulong)(out + 1152) >> 3) | 0ul,
                (float4)(0.5f, 0.5f, 0.f, 0.f), dir, inverse);
  *(__global uint4 *)(out + 20) =
      intersect(((ulong)(out + 1152) >> 3) | 0ul, (float4)(2.f, 2.f, -1.f, 0.f),
                dir, inverse);
  dir = (float4)(0.f, 0.f, -1.f, 0.f);
  *(__global uint4 *)(out + 24) =
      intersect(((ulong)(out + 1152) >> 3) | 0ul,
                (float4)(0.5f, 0.5f, 0.f, 0.f), dir, inverse);
  *(__global uint4 *)(out + 28) =
      intersect(((ulong)(out + 1152) >> 3) | 0ul, (float4)(2.f, 2.f, 1.f, 0.f),
                dir, inverse);
  *(__global uint4 *)(out + 32) =
      intersect(((ulong)(out + 1152) >> 3) | 0ul,
                (float4)(0.5f, 0.5f, -1.f, 0.f), -dir, inverse);
  *(__global uint4 *)(out + 36) =
      intersect(((ulong)(out + 1152) >> 3) | 0ul,
                (float4)(0.f, 0.5f, -1.f, 0.f), -dir, inverse);
  *(__global uint4 *)(out + 40) =
      intersect(((ulong)(out + 1152) >> 3) | 0ul,
                (float4)(0.5f, 0.f, -1.f, 0.f), -dir, inverse);
  *(__global uint4 *)(out + 44) =
      intersect(((ulong)(out + 1152) >> 3) | 0ul,
                (float4)(0.25f, 0.25f, 0.f, 0.f), -dir, inverse);
  *(__global uint4 *)(out + 48) =
      intersect(((ulong)(out + 1216) >> 3) | 0ul, (float4)(2.f, 2.f, -1.f, 0.f),
                -dir, inverse);
  *(__global uint4 *)(out + 52) =
      intersect(((ulong)(out + 1216) >> 3) | 0ul, (float4)(2.f, 2.f, 1.f, 0.f),
                dir, inverse);
  const float2 points[3][6] = {
      {(float2)(0.f, .5f), (float2)(.5f, 0.f), (float2)(.5f, .5f),
       (float2)(0.f, 0.f), (float2)(1.f, 0.f), (float2)(0.f, 1.f)},
      {(float2)(0.f, -.5f), (float2)(.5f, 0.f), (float2)(.5f, -.5f),
       (float2)(0.f, 0.f), (float2)(1.f, 0.f), (float2)(0.f, -1.f)},
      {(float2)(-.5f, .5f), (float2)(.5f, .5f), (float2)(0.f, 1.f),
       (float2)(0.f, 0.f), (float2)(1.f, 1.f), (float2)(-1.f, 1.f)}};
  for (uint shape = 0; shape < 3; ++shape) {
    for (uint permutation = 0; permutation < 6; ++permutation) {
      for (uint facing = 0; facing < 2; ++facing) {
        for (uint p = 0; p < 6; ++p) {
          float z = facing ? 1.f : -1.f;
          float4 ray = (float4)(0.f, 0.f, -z, 0.f);
          uint node_base = shape == 0 ? 1280 : 1472 + (shape - 1) * 96;
          uint result_base = shape == 0 ? 56 : 368 + (shape - 1) * 288;
          ulong node = ((ulong)(out + node_base + permutation * 16)) >> 3;
          *(__global uint4 *)(out + result_base +
                              4 * (permutation * 12 + facing * 6 + p)) =
              intersect(
                  node,
                  (float4)(points[shape][p].x, points[shape][p].y, z, 0.f), ray,
                  inverse);
        }
      }
    }
  }
  for (uint side = 0; side < 2; ++side) {
    for (uint rotation = 0; rotation < 3; ++rotation) {
      uint index = side * 3 + rotation;
      float4 start = side ? (float4)(.75f, .625f, -1.f, 0.f)
                          : (float4)(.25f, .125f, -1.f, 0.f);
      ulong node = (((ulong)(out + 1376 + index * 16)) >> 3) | side;
      *(__global uint4 *)(out + 344 + index * 4) =
          intersect(node, start, (float4)(0.f, 0.f, 1.f, 0.f), inverse);
    }
  }
}
