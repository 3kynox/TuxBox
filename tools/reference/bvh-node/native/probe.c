#include <hsa/hsa.h>
#include <hsa/hsa_ext_amd.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define CHK(x)                                                                 \
  do {                                                                         \
    hsa_status_t s = (x);                                                      \
    if (s != HSA_STATUS_SUCCESS) {                                             \
      const char *msg = 0;                                                     \
      hsa_status_string(s, &msg);                                              \
      fprintf(stderr, "%s: %s (%d)\n", #x, msg, s);                            \
      exit(1);                                                                 \
    }                                                                          \
  } while (0)
static hsa_agent_t gpu, cpu;
static hsa_region_t region;
static hsa_status_t each_agent(hsa_agent_t a, void *p) {
  hsa_device_type_t type;
  hsa_agent_get_info(a, HSA_AGENT_INFO_DEVICE, &type);
  if (type == HSA_DEVICE_TYPE_GPU)
    gpu = a;
  if (type == HSA_DEVICE_TYPE_CPU)
    cpu = a;
  return HSA_STATUS_SUCCESS;
}
static hsa_status_t each_region(hsa_region_t r, void *p) {
  hsa_region_segment_t seg;
  uint32_t flags;
  hsa_region_get_info(r, HSA_REGION_INFO_SEGMENT, &seg);
  if (seg == HSA_REGION_SEGMENT_GLOBAL) {
    hsa_region_get_info(r, HSA_REGION_INFO_GLOBAL_FLAGS, &flags);
    if (flags & HSA_REGION_GLOBAL_FLAG_KERNARG)
      region = r;
  }
  return HSA_STATUS_SUCCESS;
}
static uint32_t next_random(uint32_t *state) {
  *state = *state * 1664525u + 1013904223u;
  return *state;
}
static float smaller(float a, float b) { return a < b ? a : b; }
static float larger(float a, float b) { return a > b ? a : b; }
static uint32_t bits(float value) {
  uint32_t result;
  memcpy(&result, &value, 4);
  return result;
}
int main(int argc, char **argv) {
  if (argc != 2) {
    fprintf(stderr, "usage: %s probe.hsaco\n", argv[0]);
    return 2;
  }
  CHK(hsa_init());
  CHK(hsa_iterate_agents(each_agent, 0));
  if (!gpu.handle || !cpu.handle) {
    fprintf(stderr, "agent/region absent\n");
    return 1;
  }
  CHK(hsa_agent_iterate_regions(cpu, each_region, 0));
  if (!region.handle) {
    fprintf(stderr, "kernarg region absent\n");
    return 1;
  }
  char agent_name[64] = {0};
  CHK(hsa_agent_get_info(gpu, HSA_AGENT_INFO_NAME, agent_name));
  if (strcmp(agent_name, "gfx1036") != 0) {
    fprintf(stderr, "this probe was validated for gfx1036, got %s\n",
            agent_name);
    return 1;
  }
  hsa_queue_t *queue;
  CHK(hsa_queue_create(gpu, 64, HSA_QUEUE_TYPE_SINGLE, NULL, NULL, UINT32_MAX,
                       UINT32_MAX, &queue));
  FILE *f = fopen(argv[1], "rb");
  if (!f)
    return 2;
  if (fseek(f, 0, SEEK_END) != 0)
    return 2;
  long file_size = ftell(f);
  if (file_size <= 0)
    return 2;
  size_t len = (size_t)file_size;
  rewind(f);
  void *data = malloc(len);
  if (!data || fread(data, 1, len, f) != len)
    return 2;
  fclose(f);
  hsa_code_object_reader_t reader;
  CHK(hsa_code_object_reader_create_from_memory(data, len, &reader));
  hsa_executable_t exe;
  CHK(hsa_executable_create_alt(
      HSA_PROFILE_FULL, HSA_DEFAULT_FLOAT_ROUNDING_MODE_DEFAULT, "", &exe));
  CHK(hsa_executable_load_agent_code_object(exe, gpu, reader, "", NULL));
  CHK(hsa_executable_freeze(exe, ""));
  hsa_executable_symbol_t sym;
  CHK(hsa_executable_get_symbol_by_name(exe, "probe.kd", &gpu, &sym));
  uint64_t obj;
  uint32_t group, priv, arg_size;
  CHK(hsa_executable_symbol_get_info(
      sym, HSA_EXECUTABLE_SYMBOL_INFO_KERNEL_OBJECT, &obj));
  CHK(hsa_executable_symbol_get_info(
      sym, HSA_EXECUTABLE_SYMBOL_INFO_KERNEL_GROUP_SEGMENT_SIZE, &group));
  CHK(hsa_executable_symbol_get_info(
      sym, HSA_EXECUTABLE_SYMBOL_INFO_KERNEL_PRIVATE_SEGMENT_SIZE, &priv));
  CHK(hsa_executable_symbol_get_info(
      sym, HSA_EXECUTABLE_SYMBOL_INFO_KERNEL_KERNARG_SEGMENT_SIZE, &arg_size));
  void *out, *args;
  CHK(hsa_memory_allocate(region, 20480, &out));
  if ((uintptr_t)out % 256 != 0) {
    fprintf(stderr, "HSA allocation is not 256-byte aligned\n");
    return 1;
  }
  CHK(hsa_amd_agents_allow_access(1, &gpu, NULL, out));
  CHK(hsa_memory_allocate(region, arg_size, &args));
  CHK(hsa_amd_agents_allow_access(1, &gpu, NULL, args));
  *(void **)args = out;
  memset(out, 0, 20480);
  uint32_t *node = (uint32_t *)out + 2048;
  node[0] = 0x9;
  node[1] = 0x12;
  node[2] = 0x1b;
  node[3] = 0x24;
  float bounds[24] = {10, 10, 2, 12, 12, 4, -1, -1, 2, 1, 1, 4,
                      -1, -1, 2, 1,  1,  4, -1, -1, 2, 1, 1, 4};
  memcpy(node + 4, bounds, sizeof(bounds));
  uint32_t *halfnode = (uint32_t *)out + 2112;
  memcpy(halfnode, node, 16);
  _Float16 *halfbounds = (_Float16 *)(halfnode + 4);
  for (int i = 0; i < 24; i++)
    halfbounds[i] = bounds[i];
  uint32_t *tri = (uint32_t *)out + 2176;
  float verts[12] = {0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0};
  memcpy(tri, verts, sizeof(verts));
  tri[12] = 0x80;
  tri[13] = 1;
  tri[14] = 2;
  tri[15] = 0x0909;
  uint32_t *scaled = (uint32_t *)out + 2240;
  float scaled_verts[12] = {0, 0, 0, 2, 0, 0, 0, 2, 0, 2, 2, 0};
  memcpy(scaled, scaled_verts, sizeof(scaled_verts));
  scaled[12] = 0x80;
  scaled[13] = 1;
  scaled[14] = 2;
  scaled[15] = 0x0909;
  static const unsigned permutations[6][3] = {{0, 1, 2}, {0, 2, 1}, {1, 0, 2},
                                              {1, 2, 0}, {2, 0, 1}, {2, 1, 0}};
  static const float vertices[3][3][3] = {{{0, 0, 0}, {1, 0, 0}, {0, 1, 0}},
                                          {{0, 0, 0}, {1, 0, 0}, {0, -1, 0}},
                                          {{0, 0, 0}, {1, 1, 0}, {-1, 1, 0}}};
  for (unsigned shape = 0; shape < 3; shape++) {
    for (unsigned p = 0; p < 6; p++) {
      unsigned base = shape == 0 ? 2304 : 2496 + (shape - 1) * 96;
      uint32_t *dst = (uint32_t *)out + base + p * 16;
      for (unsigned v = 0; v < 3; v++)
        memcpy(dst + v * 3, vertices[shape][permutations[p][v]],
               3 * sizeof(float));
      dst[15] = 9;
    }
  }
  const unsigned selectors[3] = {9, 2, 4};
  for (unsigned side = 0; side < 2; side++) {
    for (unsigned r = 0; r < 3; r++) {
      uint32_t *dst = (uint32_t *)out + 2400 + (side * 3 + r) * 16;
      memcpy(dst, tri, 64);
      unsigned shift = side * 8;
      dst[15] = (0x0909u & ~(0xffu << shift)) | (selectors[r] << shift);
    }
  }
  // Seeded finite, exactly representable inputs. Each child is placed around
  // the ray, with some deliberately missed, invalid, or clipped by extent.
  uint32_t random_expected[24][4];
  uint32_t seed = 0x74757862u;
  static const float inverses[6] = {-4.f, -2.f, -1.f, 1.f, 2.f, 4.f};
  for (unsigned test = 0; test < 24; test++) {
    uint32_t *record = (uint32_t *)out + 3008 + test * 48;
    float origin[3], inverse[3], keys[4];
    for (unsigned axis = 0; axis < 3; axis++) {
      origin[axis] = (float)((int)(next_random(&seed) % 9) - 4);
      inverse[axis] = inverses[next_random(&seed) % 6];
      record[32 + axis] = bits(origin[axis]);
      record[35 + axis] = bits(inverse[axis]);
    }
    float extent = (test % 3 == 0) ? 3.f : 16.f;
    int sort = test % 3 != 1;
    record[38] = bits(extent);
    record[39] = sort;
    record[40] = test & 1;
    for (unsigned child = 0; child < 4; child++) {
      record[child] = 9 + child * 9;
      if (child == 3 && test % 4 == 0)
        record[child] = 0xffffffffu;
      float lower[3], upper[3];
      float at = (float)(1 + next_random(&seed) % 7);
      for (unsigned axis = 0; axis < 3; axis++) {
        float center = origin[axis] + at / inverse[axis];
        if (child == 2 && axis == 0 && test % 2 == 0)
          center += 24.f;
        lower[axis] = center - 1.f;
        upper[axis] = center + 1.f;
        if (test & 1) {
          ((float *)(record + 4 + child * 6))[axis] = lower[axis];
          ((float *)(record + 4 + child * 6))[axis + 3] = upper[axis];
        } else {
          ((_Float16 *)(record + 4 + child * 3))[axis] = (_Float16)lower[axis];
          ((_Float16 *)(record + 4 + child * 3))[axis + 3] =
              (_Float16)upper[axis];
        }
      }
      float near = 0.f, far = extent;
      for (unsigned axis = 0; axis < 3; axis++) {
        float t0 = (lower[axis] - origin[axis]) * inverse[axis];
        float t1 = (upper[axis] - origin[axis]) * inverse[axis];
        near = larger(near, smaller(t0, t1));
        far = smaller(far, larger(t0, t1));
      }
      random_expected[test][child] = near <= far ? record[child] : 0xffffffffu;
      keys[child] = near;
    }
    if (sort) {
      static const unsigned left[5] = {0, 1, 0, 2, 1},
                            right[5] = {2, 3, 1, 3, 2};
      for (unsigned i = 0; i < 5; i++) {
        unsigned a = left[i], b = right[i];
        if (random_expected[test][a] == 0xffffffffu ||
            (random_expected[test][b] != 0xffffffffu && keys[b] < keys[a])) {
          uint32_t temp = random_expected[test][a];
          random_expected[test][a] = random_expected[test][b];
          random_expected[test][b] = temp;
          float key = keys[a];
          keys[a] = keys[b];
          keys[b] = key;
        }
      }
    }
  }
  float degenerate[12] = {0, 0, 0, 1, 0, 0, 2, 0, 0, 0, 0, 0};
  uint32_t *degenerate_node = (uint32_t *)out + 2816;
  memcpy(degenerate_node, degenerate, sizeof(degenerate));
  degenerate_node[15] = 9;
  hsa_signal_t signal;
  CHK(hsa_signal_create(1, 0, NULL, &signal));
  uint64_t index = hsa_queue_load_write_index_relaxed(queue);
  hsa_kernel_dispatch_packet_t *packet =
      (hsa_kernel_dispatch_packet_t *)queue->base_address +
      (index & (queue->size - 1));
  memset(packet, 0, sizeof(*packet));
  packet->setup = 1 << HSA_KERNEL_DISPATCH_PACKET_SETUP_DIMENSIONS;
  packet->workgroup_size_x = 1;
  packet->workgroup_size_y = 1;
  packet->workgroup_size_z = 1;
  packet->grid_size_x = 1;
  packet->grid_size_y = 1;
  packet->grid_size_z = 1;
  packet->private_segment_size = priv;
  packet->group_segment_size = group;
  packet->kernel_object = obj;
  packet->kernarg_address = args;
  packet->completion_signal = signal;
  uint16_t header =
      (HSA_PACKET_TYPE_KERNEL_DISPATCH << HSA_PACKET_HEADER_TYPE) |
      (HSA_FENCE_SCOPE_SYSTEM << HSA_PACKET_HEADER_SCACQUIRE_FENCE_SCOPE) |
      (HSA_FENCE_SCOPE_SYSTEM << HSA_PACKET_HEADER_SCRELEASE_FENCE_SCOPE);
  __atomic_store_n(&packet->header, header, __ATOMIC_RELEASE);
  hsa_queue_store_write_index_relaxed(queue, index + 1);
  hsa_signal_store_relaxed(queue->doorbell_signal, index);
  hsa_signal_value_t result =
      hsa_signal_wait_scacquire(signal, HSA_SIGNAL_CONDITION_LT, 1,
                                5000000000ull, HSA_WAIT_STATE_BLOCKED);
  static const uint32_t expected[56] = {
      0x1b,       0x12,       0x24,       0xffffffff, 0x1b,       0x12,
      0x24,       0xffffffff, 0xbf800000, 0xbf800000, 0xbe800000, 0xbe800000,
      0xbf800000, 0xbf800000, 0xbf000000, 0xbe800000, 0xff800000, 0xbf800000,
      0xbf000000, 0xbf000000, 0xff800000, 0xbf800000, 0xc0000000, 0xc0000000,
      0x7f800000, 0x3f800000, 0x3f000000, 0x3f000000, 0x7f800000, 0x3f800000,
      0x40000000, 0x40000000, 0xff800000, 0xbf800000, 0xbf000000, 0xbf000000,
      0xbf800000, 0xbf800000, 0x00000000, 0xbf000000, 0xff800000, 0xbf800000,
      0xbf000000, 0x00000000, 0x00000000, 0xbf800000, 0xbe800000, 0xbe800000,
      0xff800000, 0xc0800000, 0xc0800000, 0xc0800000, 0x7f800000, 0x40800000,
      0x40800000, 0x40800000};
  int failed = result != 0;
  printf("agent=%s\n", agent_name);
  for (int i = 0; i < 56; i++) {
    printf("%08x%c", ((uint32_t *)out)[i], i % 4 == 3 ? '\n' : ' ');
    if (((uint32_t *)out)[i] != expected[i])
      failed = 1;
  }
  // Check hit status separately: these cases map the hardware's edge
  // ownership, without assuming the returned unnormalized barycentrics.
  for (unsigned shape = 0; shape < 3; shape++) {
    for (unsigned p = 0; p < 6; p++) {
      for (unsigned facing = 0; facing < 2; facing++) {
        static const unsigned hit_masks[3][2] = {
            {0x01, 0x0b}, {0x0b, 0x01}, {0x25, 0x01}};
        for (unsigned point = 0; point < 6; point++) {
          unsigned base = shape == 0 ? 56 : 368 + (shape - 1) * 288;
          uint32_t t =
              ((uint32_t *)out)[base + 4 * (p * 12 + facing * 6 + point)];
          int hit = (t & 0x7fffffffu) != 0x7f800000u;
          if (hit != ((hit_masks[shape][facing] >> point) & 1))
            failed = 1;
        }
      }
    }
  }
  static const uint32_t remap[24] = {
      0xbf800000, 0xbf800000, 0xbe800000, 0xbe000000, 0xbf800000, 0xbf800000,
      0xbe000000, 0xbf200000, 0xbf800000, 0xbf800000, 0xbf200000, 0xbe800000,
      0xbf800000, 0xbf800000, 0xbec00000, 0xbe800000, 0xbf800000, 0xbf800000,
      0xbe800000, 0xbec00000, 0xbf800000, 0xbf800000, 0xbec00000, 0xbec00000};
  for (unsigned i = 0; i < 24; i++)
    if (((uint32_t *)out)[344 + i] != remap[i])
      failed = 1;
  printf("permuted_edges=216 paired_remaps=6\n");
  unsigned random_hits = 0;
  for (unsigned test = 0; test < 24; test++) {
    for (unsigned lane = 0; lane < 4; lane++) {
      uint32_t got = ((uint32_t *)out)[944 + test * 4 + lane];
      if (got != random_expected[test][lane])
        failed = 1;
      if (got != 0xffffffffu)
        random_hits++;
    }
  }
  printf("random_box_cases=24 hit_lanes=%u\n", random_hits);
  if (random_hits == 0)
    failed = 1;
  static const unsigned oblique_masks[3][6][2] = {
      {{4, 4}, {3, 2}, {1, 3}, {5, 5}, {2, 2}, {3, 1}},
      {{2, 2}, {5, 4}, {3, 1}, {3, 3}, {4, 4}, {1, 3}},
      {{4, 4}, {3, 3}, {5, 1}, {4, 4}, {3, 3}, {1, 5}}};
  for (unsigned shape = 0; shape < 3; shape++)
    for (unsigned d = 0; d < 6; d++)
      for (unsigned facing = 0; facing < 2; facing++)
        for (unsigned point = 0; point < 3; point++) {
          unsigned index = ((shape * 6 + d) * 2 + facing) * 3 + point;
          uint32_t t = ((uint32_t *)out)[1040 + 4 * index];
          int hit = (t & 0x7fffffffu) != 0x7f800000u;
          if (hit != ((oblique_masks[shape][d][facing] >> point) & 1))
            failed = 1;
        }
  printf("oblique_edges=108\n");
  static const unsigned tied_masks[3][6][2] = {
      {{3, 2}, {3, 2}, {3, 2}, {2, 3}, {3, 2}, {2, 3}},
      {{5, 4}, {5, 4}, {5, 4}, {4, 5}, {5, 4}, {4, 5}},
      {{3, 3}, {3, 3}, {3, 3}, {3, 3}, {3, 3}, {3, 3}}};
  for (unsigned shape = 0; shape < 3; shape++)
    for (unsigned d = 0; d < 6; d++)
      for (unsigned facing = 0; facing < 2; facing++)
        for (unsigned point = 0; point < 3; point++) {
          unsigned i = ((shape * 6 + d) * 2 + facing) * 3 + point;
          uint32_t t = ((uint32_t *)out)[1472 + 4 * i];
          int hit = (t & 0x7fffffffu) != 0x7f800000u;
          if (hit != ((tied_masks[shape][d][facing] >> point) & 1))
            failed = 1;
        }
  static const uint32_t special[36] = {
      0x7f800000, 0,          0xbf800000, 0,          0x7f800000, 0,
      0,          0,          0x7f800000, 0,          0xbf800000, 0,
      0x7f800000, 0,          0x3f000000, 0xbe800000, 0x7f800000, 0,
      0,          0,          0x7f800000, 0,          0,          0,
      0x7f800000, 0,          0x3f800000, 0,          0x7f800000, 0,
      0,          0x3f800000, 0x7f800000, 0,          0xbf000000, 0x3e800000};
  for (unsigned i = 0; i < 36; i++)
    if (((uint32_t *)out)[1904 + i] != special[i])
      failed = 1;
  printf("tied_edges=108 parallel_and_degenerate=9\n");
  static const uint32_t special_float[20] = {
      0x7f800000, 0,          0,          0,          0x7f800000, 0,          0,
      0,          0xff800000, 0xffc00000, 0xff800000, 0xff800000, 0x7f800000, 0,
      0,          0,          0x7f800000, 0x3f800000, 0,          0};
  for (unsigned i = 0; i < 20; i++)
    if (((uint32_t *)out)[1940 + i] != special_float[i])
      failed = 1;
  printf("nonfinite_and_subnormal=5\n");
  if (failed) {
    fprintf(stderr, "instruction probe failed or timed out\n");
    return 1;
  }
  CHK(hsa_signal_destroy(signal));
  CHK(hsa_memory_free(args));
  CHK(hsa_memory_free(out));
  CHK(hsa_executable_destroy(exe));
  CHK(hsa_code_object_reader_destroy(reader));
  CHK(hsa_queue_destroy(queue));
  CHK(hsa_shut_down());
  free(data);
  return 0;
}
