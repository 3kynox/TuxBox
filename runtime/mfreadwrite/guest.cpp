// Independent consumer of the system SDK ABI. Every PCM sample is generated here.
#define NOMINMAX
#include <windows.h>
#include <mfapi.h>
#include <shlwapi.h>
#include <wininet.h>
#include <mferror.h>
#include <mfidl.h>
#include <mfreadwrite.h>
#include <wrl/client.h>
#include <algorithm>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <stdexcept>
#include <vector>

using Microsoft::WRL::ComPtr;
constexpr DWORD frames = 2501, rate = 48000, block = 4;

void require(bool value, const char* reason) {
    if (!value) throw std::runtime_error(reason);
}

void check(HRESULT hr, const char* operation) {
    if (FAILED(hr)) {
        char text[160];
        std::snprintf(text, sizeof(text), "%s: %#lx", operation, static_cast<unsigned long>(hr));
        throw std::runtime_error(text);
    }
}

void le32(std::vector<BYTE>& v, DWORD n) {
    for (unsigned shift = 0; shift < 32; shift += 8) v.push_back(static_cast<BYTE>(n >> shift));
}

std::vector<BYTE> wave(const std::vector<BYTE>& pcm) {
    std::vector<BYTE> file{'R','I','F','F'};
    le32(file, 46 + static_cast<DWORD>(pcm.size()));
    file.insert(file.end(), {'W','A','V','E','J','U','N','K',1,0,0,0,0x7e,0,'f','m','t',' '});
    le32(file, 16);
    file.insert(file.end(), {1,0,2,0});
    le32(file, rate);
    le32(file, rate * block);
    file.insert(file.end(), {4,0,16,0,'d','a','t','a'});
    le32(file, static_cast<DWORD>(pcm.size()));
    file.insert(file.end(), pcm.begin(), pcm.end());
    return file;
}

ComPtr<IMFByteStream> byte_stream(const std::vector<BYTE>& bytes) {
    ComPtr<IStream> mem;
    check(CreateStreamOnHGlobal(nullptr, TRUE, &mem), "memory stream");
    ULONG written{};
    check(mem->Write(bytes.data(), static_cast<ULONG>(bytes.size()), &written), "write stream");
    require(written == bytes.size(), "short stream write");
    check(mem->Seek({}, STREAM_SEEK_SET, nullptr), "rewind stream");
    ComPtr<IMFByteStream> stream;
    check(MFCreateMFByteStreamOnStream(mem.Get(), &stream), "MF byte stream");
    return stream;
}

void rejected(const std::vector<BYTE>& bytes) {
    const auto stream = byte_stream(bytes);
    ComPtr<IMFSourceReader> r;
    require(FAILED(MFCreateSourceReaderFromByteStream(stream.Get(), nullptr, &r)) && !r,
            "invalid media produced a reader");
}

LONGLONG timestamp(DWORD frame) { return frame * 10000000LL / rate; }

void verify_samples(IMFSourceReader* r, const std::vector<BYTE>& pcm, DWORD frame) {
    for (;;) {
        DWORD stream = 17, flags = 0;
        LONGLONG time = -1;
        ComPtr<IMFSample> sample;
        check(r->ReadSample(MF_SOURCE_READER_ANY_STREAM, 0, &stream, &flags, &time, &sample), "read sample");
        require(!stream, "wrong stream index");
        if (flags == MF_SOURCE_READERF_ENDOFSTREAM) {
            require(frame == frames && !sample, "early end of stream");
            break;
        }
        require(!flags && sample && time == timestamp(frame), "bad sample timestamp or flags");
        ComPtr<IMFMediaBuffer> bytes;
        check(sample->ConvertToContiguousBuffer(&bytes), "contiguous PCM");
        BYTE* data{}; DWORD length{};
        check(bytes->Lock(&data, nullptr, &length), "lock PCM");
        const bool matched = length > 0 && length % block == 0 && length <= (frames - frame) * block &&
                             !std::memcmp(data, pcm.data() + frame * block, length);
        check(bytes->Unlock(), "unlock PCM");
        require(matched, "PCM bytes differ");
        frame += length / block;
        LONGLONG start{}, duration{};
        check(sample->GetSampleTime(&start), "sample time");
        check(sample->GetSampleDuration(&duration), "sample duration");
        require(start == time && start + duration == timestamp(frame), "wrong PCM duration");
    }
}

void check_urls(const std::vector<BYTE>& bytes, const std::vector<BYTE>& pcm) {
    struct File {
        wchar_t path[MAX_PATH]{};
        ~File() { if (*path) DeleteFileW(path); }
    } file;
    wchar_t temp[MAX_PATH]{};
    DWORD len = GetTempPathW(MAX_PATH, temp);
    require(len && len < MAX_PATH && GetTempFileNameW(temp, L"T B", 0, file.path), "temporary WAVE file");
    FILE* f{};
    require(!_wfopen_s(&f, file.path, L"wb"), "open temporary WAVE");
    const size_t written = std::fwrite(bytes.data(), 1, bytes.size(), f);
    require(!std::fclose(f) && written == bytes.size(), "write temporary WAVE");
    wchar_t url[INTERNET_MAX_URL_LENGTH]{};
    DWORD capacity = ARRAYSIZE(url);
    check(UrlCreateFromPathW(file.path, url, &capacity, 0), "file URL");
    require(wcsstr(url, L"%20") != nullptr, "escaped space in file URL");
    for (const wchar_t* input : {file.path, url}) {
        ComPtr<IMFSourceReader> r;
        check(MFCreateSourceReaderFromURL(input, nullptr, &r), "open URL as PCM");
        verify_samples(r.Get(), pcm, 0);
    }
    require(DeleteFileW(file.path) != 0, "release file handles");
    IMFSourceReader* missing = reinterpret_cast<IMFSourceReader*>(1);
    require(FAILED(MFCreateSourceReaderFromURL(url, nullptr, &missing)) && !missing, "missing file");
    file.path[0] = L'\0';
}

void test() {
    // Wine may prefer its built-in MFReadWrite, so verify the loaded module path.
    wchar_t path[MAX_PATH], dir[MAX_PATH];
    HMODULE module = GetModuleHandleW(L"mfreadwrite.dll");
    require(module && GetModuleFileNameW(module, path, MAX_PATH) > 0, "provider path");
    require(GetFullPathNameW(L"mfreadwrite.dll", MAX_PATH, dir, nullptr) > 0 &&
            !_wcsicmp(path, dir), "not using the app-local provider");
    require(GetProcAddress(module, MAKEINTRESOURCEA(5)) == GetProcAddress(module, "MFCreateSourceReaderFromByteStream"),
            "missing Xbox source-reader ordinal");
    require(GetProcAddress(module, MAKEINTRESOURCEA(7)) == GetProcAddress(module, "MFCreateSourceReaderFromURL"),
            "missing Xbox URL-reader ordinal");

    std::vector<BYTE> pcm(frames * block);
    for (size_t i = 0; i < pcm.size(); ++i) pcm[i] = static_cast<BYTE>(i * 53 + i / 11);
    auto bytes = wave(pcm);
    check_urls(bytes, pcm);
    auto stream = byte_stream(bytes);
    ComPtr<IMFSourceReader> reader;
    check(MFCreateSourceReaderFromByteStream(stream.Get(), nullptr, &reader), "open WAVE");
    stream.Reset(); // The reader retains its own byte-stream reference.
    constexpr DWORD audio = MF_SOURCE_READER_FIRST_AUDIO_STREAM;
    ComPtr<IMFMediaType> type;
    check(reader->GetNativeMediaType(audio, 0, &type), "native audio type");
    GUID subtype{}, major{};
    check(type->GetGUID(MF_MT_SUBTYPE, &subtype), "subtype");
    check(type->GetGUID(MF_MT_MAJOR_TYPE, &major), "major type");
    require(major == MFMediaType_Audio && subtype == MFAudioFormat_PCM &&
            MFGetAttributeUINT32(type.Get(), MF_MT_AUDIO_NUM_CHANNELS, 0) == 2 &&
            MFGetAttributeUINT32(type.Get(), MF_MT_AUDIO_SAMPLES_PER_SECOND, 0) == rate &&
            MFGetAttributeUINT32(type.Get(), MF_MT_AUDIO_BITS_PER_SAMPLE, 0) == 16,
            "wrong PCM format");
    check(reader->SetCurrentMediaType(audio, nullptr, type.Get()), "select native PCM");
    PROPVARIANT duration{};
    check(reader->GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE, MF_PD_DURATION, &duration), "duration");
    require(duration.vt == VT_UI8 && duration.uhVal.QuadPart == static_cast<ULONGLONG>(timestamp(frames)),
            "wrong presentation duration");
    PropVariantClear(&duration);
    verify_samples(reader.Get(), pcm, 0);
    PROPVARIANT pos{};
    pos.vt = VT_I8;
    pos.hVal.QuadPart = timestamp(1200);
    check(reader->SetCurrentPosition(GUID_NULL, pos), "seek");
    verify_samples(reader.Get(), pcm, 1200);
    check(reader->SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS, FALSE), "deselect");
    BOOL selected = TRUE;
    check(reader->GetStreamSelection(audio, &selected), "selection");
    require(!selected, "unexpected selection");
    DWORD flags{};
    IMFSample* sample = reinterpret_cast<IMFSample*>(1);
    require(reader->ReadSample(audio, 0, nullptr, &flags, nullptr, &sample) == MF_E_INVALIDREQUEST && !sample,
            "deselected stream did not clear its sample");
    sample = reinterpret_cast<IMFSample*>(1);
    require(reader->ReadSample(5, 0, nullptr, &flags, nullptr, &sample) == MF_E_INVALIDSTREAMNUMBER && !sample,
            "invalid stream did not clear its sample");
    bytes.pop_back();
    rejected(bytes); // RIFF length extends past the supplied stream.
    rejected(std::vector<BYTE>(20, 0x51));
}

int wmain(int argc, wchar_t** argv) {
    if (argc != 3 || wcscmp(argv[1], L"--report")) return 2;
    FILE* report{};
    if (_wfopen_s(&report, argv[2], L"wb")) return 2;
    int exit_code = 0;
    try {
        check(CoInitializeEx(nullptr, COINIT_MULTITHREADED), "COM");
        check(MFStartup(MF_VERSION), "MFStartup");
        test();
        check(MFShutdown(), "MFShutdown");
        CoUninitialize();
        std::fprintf(report, "mfreadwrite PASS pcm=48000/2/16 frames=2501 bytes=10004 seek=1200 eos=2 invalid=2 file-inputs=2\n");
    } catch (const std::exception& e) {
        std::fprintf(report, "mfreadwrite failed: %s\n", e.what());
        exit_code = 1;
    }
    std::fclose(report);
    return exit_code;
}
