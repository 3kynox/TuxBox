// Independent Xbox ordinal consumer. Read a meter on the actual audio graph;
// silence the submix's route to the speakers so the test can run unattended.
#include <windows.h>
#include <xaudio2.h>
#include <xaudio2fx.h>
#include <xapofx.h>
#include <x3daudio.h>
#include <wrl/client.h>
#include <array>
#include <cmath>
#include <cstdio>
#include <filesystem>
#include <stdexcept>
#include <string>

using Microsoft::WRL::ComPtr;

extern "C" {
__declspec(dllimport) HRESULT WINAPI GuestCreate(IXAudio2**, UINT32, XAUDIO2_PROCESSOR);
__declspec(dllimport) HRESULT WINAPI GuestCreateV29(IXAudio2**, UINT32, XAUDIO2_PROCESSOR);
__declspec(dllimport) HRESULT WINAPI GuestVersioned(IXAudio2**, UINT32, XAUDIO2_PROCESSOR, UINT32);
__declspec(dllimport) HRESULT WINAPI GuestReverb(IUnknown**);
__declspec(dllimport) HRESULT WINAPI GuestMeter(IUnknown**);
__declspec(dllimport) HRESULT WINAPI GuestEffect(REFCLSID, IUnknown**, const void*, UINT32);
__declspec(dllimport) HRESULT WINAPI GuestInitialize(UINT32, float, X3DAUDIO_HANDLE);
__declspec(dllimport) void WINAPI GuestCalculate(const X3DAUDIO_HANDLE, const X3DAUDIO_LISTENER*,
                                                const X3DAUDIO_EMITTER*, UINT32, X3DAUDIO_DSP_SETTINGS*);
}

static void require(bool success, const char* message) {
    if (!success) throw std::runtime_error(message);
}
static void check(HRESULT hr, const char* operation) {
    if (FAILED(hr)) {
        char result[128];
        std::snprintf(result, sizeof(result), "%s returned %#lx", operation, static_cast<unsigned long>(hr));
        throw std::runtime_error(result);
    }
}

template <typename T> struct Voice {
    T* ptr = nullptr;
    ~Voice() { if (ptr) ptr->DestroyVoice(); }
};

static void check_spatial() {
    X3DAUDIO_HANDLE instance{};
    check(GuestInitialize(SPEAKER_STEREO, X3DAUDIO_SPEED_OF_SOUND, instance), "spatial initialization");
    X3DAUDIO_LISTENER listener{};
    listener.OrientFront.z = listener.OrientTop.y = 1;
    X3DAUDIO_EMITTER emitter{};
    emitter.OrientFront.z = emitter.OrientTop.y = 1;
    emitter.ChannelCount = 1;
    emitter.CurveDistanceScaler = emitter.DopplerScaler = 1;
    std::array<float, 2> gains{};
    X3DAUDIO_DSP_SETTINGS dsp{};
    dsp.SrcChannelCount = 1;
    dsp.DstChannelCount = 2;
    dsp.pMatrixCoefficients = gains.data();
    for (int expected = 0; expected < 2; ++expected) {
        emitter.Position.x = expected ? 1.f : -1.f;
        GuestCalculate(instance, &listener, &emitter, X3DAUDIO_CALCULATE_MATRIX, &dsp);
        require(std::isfinite(gains[expected]) && std::isfinite(gains[1-expected]) &&
                gains[expected] > 0.1f && gains[1-expected] < gains[expected]*0.2f, "spatial stereo pan");
    }
}

static void check_mix(IXAudio2* engine, IUnknown* meter) {
    Voice<IXAudio2MasteringVoice> master;
    check(engine->CreateMasteringVoice(&master.ptr, 2, 48000), "master voice");
    XAUDIO2_EFFECT_DESCRIPTOR effect{meter, TRUE, 2};
    XAUDIO2_EFFECT_CHAIN chain{1, &effect};
    Voice<IXAudio2SubmixVoice> mix;
    check(engine->CreateSubmixVoice(&mix.ptr, 2, 48000, 0, 0, nullptr, &chain), "submix voice");
    const float mute[4]{};
    check(mix.ptr->SetOutputMatrix(master.ptr, 2, 2, mute), "mute speakers");
    XAUDIO2_SEND_DESCRIPTOR send{0, mix.ptr};
    XAUDIO2_VOICE_SENDS sends{1, &send};
    WAVEFORMATEX format{WAVE_FORMAT_PCM, 1, 48000, 96000, 2, 16, 0};
    std::array<short, 960> wave{};
    for (size_t i = 0; i < wave.size(); ++i) wave[i] = i%48 < 24 ? 8192 : -8192;
    Voice<IXAudio2SourceVoice> source;
    check(engine->CreateSourceVoice(&source.ptr, &format, 0, 1.f, nullptr, &sends), "source voice");
    XAUDIO2_BUFFER buffer{};
    buffer.pAudioData = reinterpret_cast<const BYTE*>(wave.data());
    buffer.AudioBytes = static_cast<UINT32>(sizeof(wave));
    buffer.LoopCount = XAUDIO2_LOOP_INFINITE;
    check(source.ptr->SubmitSourceBuffer(&buffer), "PCM submit");
    check(source.ptr->Start(), "source start");
    for (int expected = 0; expected < 2; ++expected) {
        float route[2]{};
        route[expected] = 1.f;
        check(source.ptr->SetOutputMatrix(mix.ptr, 1, 2, route), "pan matrix");
        float peak[2]{}, rms[2]{};
        XAUDIO2FX_VOLUMEMETER_LEVELS levels{peak, rms, 2};
        bool matched = false;
        const auto deadline = GetTickCount64()+3000;
        do {
            check(mix.ptr->GetEffectParameters(0, &levels, sizeof(levels)), "read meter");
            matched = std::abs(peak[expected]-.25f) < .015f &&
                      std::abs(rms[expected]-.25f) < .015f &&
                      std::abs(peak[1-expected]) < .001f && std::abs(rms[1-expected]) < .001f;
            if (!matched) Sleep(10);
        } while (!matched && GetTickCount64()<deadline);
        require(matched, "meter did not match routed PCM");
    }
    XAUDIO2_VOICE_STATE state{};
    source.ptr->GetState(&state);
    require(state.SamplesPlayed > 0, "no samples played");
    check(source.ptr->Stop(), "source stop");
}

int wmain(int argc, wchar_t** argv) {
    if (argc != 3 || wcscmp(argv[1], L"--report")) return 2;
    FILE* out{};
    if (_wfopen_s(&out, argv[2], L"wb")) return 2;
    try {
        wchar_t dll_path[MAX_PATH]{}, exe_path[MAX_PATH]{};
        HMODULE provider{};
        require(GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                    reinterpret_cast<LPCWSTR>(&GuestVersioned), &provider) != 0, "guest module lookup");
        require(GetModuleFileNameW(provider, dll_path, MAX_PATH) && GetModuleFileNameW(nullptr, exe_path, MAX_PATH),
                "module paths");
        require(!_wcsicmp(dll_path, (std::filesystem::path(exe_path).parent_path()/L"XAudio2_9.dll").c_str()),
                "expected app-local Xbox provider");
        check(CoInitializeEx(nullptr, COINIT_MULTITHREADED), "initialize COM");
        {
            ComPtr<IXAudio2> ordinary, v29, versioned;
            check(GuestCreate(&ordinary, 0, XAUDIO2_DEFAULT_PROCESSOR), "ordinal 1 factory");
            check(GuestCreateV29(&v29, 0, XAUDIO2_DEFAULT_PROCESSOR), "ordinal 8 factory");
            check(GuestVersioned(&versioned, 0, XAUDIO2_DEFAULT_PROCESSOR, 0x0a000007), "ordinal 9 factory");
            ComPtr<IUnknown> reverb, meter, eq;
            check(GuestReverb(&reverb), "reverb");
            check(GuestMeter(&meter), "volume meter");
            check(GuestEffect(CLSID_FXEQ, &eq, nullptr, 0), "equalizer");
            const GUID unknown{};
            ComPtr<IUnknown> missing;
            require(FAILED(GuestEffect(unknown, &missing, nullptr, 0)) && !missing,
                    "unknown effect must fail");
            check_spatial();
            check_mix(v29.Get(), meter.Get());
            check_mix(versioned.Get(), meter.Get());
        }
        CoUninitialize();
        fputs("xaudio PASS factories=1,8,9 effects=3 spatial=2/2 mix=4/4", out);
        fclose(out);
        return 0;
    } catch (const std::exception& error) {
        fprintf(out, "xaudio failed: %s", error.what());
        fclose(out);
        return 1;
    }
}
