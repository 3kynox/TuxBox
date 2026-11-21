// Dispatch Frosty's original InstanceCull with its embedded Xbox root signature.
#include "mesh_fixture.h"
#include "frame_cli.h"
#include <filesystem>
#include <fstream>
#include <vector>

namespace {
using namespace mesh_fixture;
namespace fs = std::filesystem;
using Pipeline = HRESULT(WINAPI*)(ID3D12Device*, const D3D12_COMPUTE_PIPELINE_STATE_DESC*, UINT,
                                  const void*, REFIID, void**);
using Bytes = std::vector<BYTE>;

Bytes load(const fs::path& path) {
    std::ifstream file(path, std::ios::binary);
    if (!file) throw std::runtime_error("cannot read " + path.string());
    return {std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>()};
}

ComPtr<ID3D12Resource> input(ID3D12Device* device, const Bytes& bytes) {
    auto result = buffer(device, D3D12_HEAP_TYPE_UPLOAD, bytes.size(), D3D12_RESOURCE_STATE_GENERIC_READ);
    upload(result.Get(), bytes.data(), bytes.size());
    return result;
}

void srv(const Context& gpu, UINT index, ID3D12Resource* resource, UINT bytes, UINT stride) {
    D3D12_SHADER_RESOURCE_VIEW_DESC desc{};
    desc.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    desc.ViewDimension = D3D12_SRV_DIMENSION_BUFFER;
    desc.Format = stride ? DXGI_FORMAT_UNKNOWN : DXGI_FORMAT_R32_TYPELESS;
    desc.Buffer.NumElements = bytes / (stride ? stride : 4);
    desc.Buffer.StructureByteStride = stride;
    desc.Buffer.Flags = stride ? D3D12_BUFFER_SRV_FLAG_NONE : D3D12_BUFFER_SRV_FLAG_RAW;
    gpu.device->CreateShaderResourceView(resource, &desc, gpu.cpu(index));
}

void uav(const Context& gpu, UINT index, ID3D12Resource* resource, UINT bytes, UINT stride) {
    D3D12_UNORDERED_ACCESS_VIEW_DESC desc{};
    desc.ViewDimension = D3D12_UAV_DIMENSION_BUFFER;
    desc.Format = stride ? DXGI_FORMAT_UNKNOWN : DXGI_FORMAT_R32_TYPELESS;
    desc.Buffer.NumElements = bytes / (stride ? stride : 4);
    desc.Buffer.StructureByteStride = stride;
    desc.Buffer.Flags = stride ? D3D12_BUFFER_UAV_FLAG_NONE : D3D12_BUFFER_UAV_FLAG_RAW;
    gpu.device->CreateUnorderedAccessView(resource, nullptr, &desc, gpu.cpu(index));
}

struct Image {
    UINT width, height, words;
    DXGI_FORMAT format() const { return words == 2 ? DXGI_FORMAT_R32G32_UINT : DXGI_FORMAT_R32_UINT; }
    UINT row_size() const { return width * words * sizeof(UINT); }
    UINT pitch() const { return (row_size() + 255u) & ~255u; }
};

void check_case(const Context& gpu, ID3D12RootSignature* root, const fs::path& dir, UINT groups, Image image) {
    auto* device = gpu.device.Get();
    auto* list = gpu.list.Get();
    const auto constants = load(dir / "b0");
    auto cb = input(device, constants);
    D3D12_CONSTANT_BUFFER_VIEW_DESC cbv{cb->GetGPUVirtualAddress(), static_cast<UINT>(constants.size())};
    device->CreateConstantBufferView(&cbv, gpu.cpu(64));
    std::array<ComPtr<ID3D12Resource>, 5> sources;
    for (UINT i = 0; i < sources.size(); ++i) {
        const auto bytes = load(dir / ("t" + std::to_string(i)));
        sources[i] = input(device, bytes);
        srv(gpu, i, sources[i].Get(), static_cast<UINT>(bytes.size()), i == 4 ? 0 : i == 3 ? static_cast<UINT>(bytes.size()) : 16);
    }
    const auto pixels = load(dir / "u0");
    if (pixels.size() != size_t(image.row_size()) * image.height)
        throw std::runtime_error("invalid visibility image input");
    Bytes rows(image.pitch() * image.height);
    for (UINT y = 0; y < image.height; ++y)
        std::memcpy(rows.data() + y * image.pitch(), pixels.data() + y * image.row_size(), image.row_size());
    auto image_seed = input(device, rows);
    auto image_result = buffer(device, D3D12_HEAP_TYPE_READBACK, rows.size(), D3D12_RESOURCE_STATE_COPY_DEST);
    D3D12_HEAP_PROPERTIES props{};
    props.Type = D3D12_HEAP_TYPE_DEFAULT;
    props.CreationNodeMask = props.VisibleNodeMask = 1;
    D3D12_RESOURCE_DESC desc{};
    desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = image.width;
    desc.Height = image.height;
    desc.DepthOrArraySize = desc.MipLevels = desc.SampleDesc.Count = 1;
    desc.Format = image.format();
    desc.Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    ComPtr<ID3D12Resource> visibility;
    check(device->CreateCommittedResource(&props, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST,
                                          nullptr, IID_PPV_ARGS(&visibility)), "create visibility image");
    D3D12_TEXTURE_COPY_LOCATION texture{}, linear{};
    texture.pResource = visibility.Get();
    texture.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    linear.pResource = image_seed.Get();
    linear.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    linear.PlacedFootprint.Footprint = {image.format(), image.width, image.height, 1, image.pitch()};
    auto copy = method<Copy>(list, 83);
    copy(list, &texture, 0, 0, 0, &linear, nullptr, 0);
    transition(list, visibility.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    device->CreateUnorderedAccessView(visibility.Get(), nullptr, nullptr, gpu.cpu(80));

    std::array<ComPtr<ID3D12Resource>, 3> seeds, outputs, readbacks;
    std::array<UINT, 3> sizes{};
    for (UINT i = 0; i < outputs.size(); ++i) {
        const auto seed = load(dir / ("u" + std::to_string(i + 1)));
        sizes[i] = static_cast<UINT>(seed.size());
        seeds[i] = input(device, seed);
        outputs[i] = buffer(device, D3D12_HEAP_TYPE_DEFAULT, sizes[i], D3D12_RESOURCE_STATE_COPY_DEST,
                            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS);
        readbacks[i] = buffer(device, D3D12_HEAP_TYPE_READBACK, sizes[i], D3D12_RESOURCE_STATE_COPY_DEST);
        list->CopyBufferRegion(outputs[i].Get(), 0, seeds[i].Get(), 0, sizes[i]);
        transition(list, outputs[i].Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
        uav(gpu, 81 + i, outputs[i].Get(), sizes[i], i == 0 ? 0 : sizes[i]);
    }
    ID3D12DescriptorHeap* heaps[]{gpu.views.Get()};
    method<BindHeaps>(list, 32)(list, 1, heaps);
    method<BindRoot>(list, 33)(list, root);
    const auto set_table = method<SetTable>(list, 35);
    const auto handle = method<GpuHandle>(gpu.views.Get(), 10)(gpu.views.Get());
    for (auto [parameter, index] : {std::pair{0u, 0u}, {1u, 64u}, {3u, 80u}})
        set_table(list, parameter, {handle + UINT64(index) * gpu.stride});
    list->Dispatch(groups, 1, 1);
    for (UINT i = 0; i < outputs.size(); ++i) {
        transition(list, outputs[i].Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
        list->CopyBufferRegion(readbacks[i].Get(), 0, outputs[i].Get(), 0, sizes[i]);
    }
    transition(list, visibility.Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
    linear.pResource = image_result.Get();
    copy(list, &linear, 0, 0, 0, &texture, nullptr, 0);
    gpu.submit();
    D3D12_RANGE span{0, rows.size()}, unread{};
    void* mapped = nullptr;
    check(image_result->Map(0, &span, &mapped), "map visibility image");
    std::ofstream out(dir / "out0", std::ios::binary);
    for (UINT y = 0; y < image.height; ++y)
        out.write(static_cast<const char*>(mapped) + y * image.pitch(), image.row_size());
    image_result->Unmap(0, &unread);
    if (!out) throw std::runtime_error("cannot save visibility image");
    for (UINT i = 0; i < readbacks.size(); ++i) {
        D3D12_RANGE read{0, sizes[i]}, empty{};
        void* values = nullptr;
        check(readbacks[i]->Map(0, &read, &values), "map InstanceCull result");
        std::ofstream file(dir / ("out" + std::to_string(i + 1)), std::ios::binary);
        file.write(static_cast<const char*>(values), sizes[i]);
        readbacks[i]->Unmap(0, &empty);
        if (!file) throw std::runtime_error("cannot save InstanceCull result");
    }
}

std::string run(const wchar_t* adapter, bool) {
    const auto temp = fs::absolute("temp");
    if (!SetEnvironmentVariableW(L"TMP", temp.c_str()) || !SetEnvironmentVariableW(L"TEMP", temp.c_str()))
        throw std::runtime_error("set shader cache directory");
    Context gpu(adapter, 96);
    auto shader = load("instance.dxbc");
    ComPtr<ID3D12RootSignature> root;
    check(gpu.device->CreateRootSignature(0, shader.data(), shader.size(), IID_PPV_ARGS(&root)),
          "create embedded InstanceCull root signature");
    D3D12_COMPUTE_PIPELINE_STATE_DESC desc{};
    desc.pRootSignature = root.Get();
    desc.CS = {shader.data(), shader.size()};
    ComPtr<ID3D12PipelineState> pipeline;
    check(method<Pipeline>(gpu.device.Get(), 62)(gpu.device.Get(), &desc, 0, nullptr, IID_PPV_ARGS(&pipeline)),
          "create Xbox InstanceCull pipeline");
    Image image{};
    std::ifstream layout("image.txt");
    if (!(layout >> image.width >> image.height >> image.words) || image.width != 9 || image.height != 9 ||
        (image.words != 1 && image.words != 2))
        throw std::runtime_error("invalid visibility image layout");
    std::ifstream manifest("cases.txt");
    UINT groups, count = 0;
    std::string name;
    while (manifest >> name >> groups) {
        if (count++) {
            check(gpu.allocator->Reset(), "reset allocator");
            check(gpu.list->Reset(gpu.allocator.Get(), nullptr), "reset command list");
        }
        gpu.list->SetPipelineState(pipeline.Get());
        check_case(gpu, root.Get(), name, groups, image);
    }
    if (!manifest.eof() || !count) throw std::runtime_error("invalid InstanceCull cases");
    char result[120];
    std::snprintf(result, sizeof(result), "instance-cull PASS pci=%04x:%04x cases=%u",
                  gpu.adapter.VendorId, gpu.adapter.DeviceId, count);
    return result;
}
} // namespace

int wmain(int argc, wchar_t** argv) {
    return tuxbox_test::frame_main(argc, argv, "instance-cull", false, run);
}
