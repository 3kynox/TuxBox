// Verify the visible window in screen coordinates, independently of GPU staging readbacks.
#include "frame_abi.h"
#include "frame_present.h"
#include <stdexcept>
#include <cstdio>

namespace tuxbox_test {
namespace {
void pump() {
    MSG message;
    while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
}
}

DisplayCheck::DisplayCheck(UINT width, UINT height) {
    // The popup's client area fills the window. Keep it above other windows while sampling.
    window_ = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, L"STATIC", L"Project TuxBox GPU check",
                              WS_POPUP, 24, 24, width * scale_, height * scale_, nullptr, nullptr,
                              GetModuleHandleW(nullptr), nullptr);
    if (!window_) throw std::runtime_error("could not create presentation window");
    ShowWindow(window_, SW_SHOW);
    SetActiveWindow(window_);
    UpdateWindow(window_);
    pump();
}
DisplayCheck::~DisplayCheck() { DestroyWindow(window_); }

void DisplayCheck::verify(ID3D12CommandQueue* queue, ID3D12Resource* source, UINT64 token,
                          COLORREF corner, COLORREF center) {
    frame_abi::Plane plane{token, 1, 0, &source};
    const auto present = frame_abi::method<frame_abi::Present>(queue, 30);
    HRESULT hr = present(queue, 1, &plane, nullptr);
    if (hr != S_OK) {
        char message[96];
        std::snprintf(message, sizeof(message), "queue slot 30 PresentX returned %#lx", static_cast<unsigned long>(hr));
        throw std::runtime_error(message);
    }
    RECT client{};
    if (!GetClientRect(window_, &client)) throw std::runtime_error("could not get presentation client area");
    POINT samples[]{{static_cast<LONG>(scale_ / 2), static_cast<LONG>(scale_ / 2)},
                    {client.right / 2, client.bottom / 2}};
    for (auto& p : samples)
        if (!ClientToScreen(window_, &p)) throw std::runtime_error("could not locate presentation window");
    const auto deadline = GetTickCount64() + 4000;
    COLORREF a = CLR_INVALID, b = CLR_INVALID;
    do {
        pump();
        HDC screen = GetDC(nullptr);
        if (!screen) throw std::runtime_error("could not acquire screen DC");
        a = GetPixel(screen, samples[0].x, samples[0].y);
        b = GetPixel(screen, samples[1].x, samples[1].y);
        ReleaseDC(nullptr, screen);
        if (a == corner && b == center) return;
        Sleep(16);
    } while (GetTickCount64() < deadline);
    char message[170];
    std::snprintf(message, sizeof(message), "screen colors on frame %llu: %#lx/%#lx, expected %#lx/%#lx",
                  token, a, b, corner, center);
    throw std::runtime_error(message);
}
}
