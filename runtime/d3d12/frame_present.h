#pragma once
#include <windows.h>
#include <d3d12.h>

namespace tuxbox_test {
class DisplayCheck {
public:
    DisplayCheck(UINT width, UINT height);
    ~DisplayCheck();
    DisplayCheck(const DisplayCheck&) = delete;
    DisplayCheck& operator=(const DisplayCheck&) = delete;
    void verify(ID3D12CommandQueue* queue, ID3D12Resource* source, UINT64 token,
                COLORREF corner, COLORREF center);
private:
    HWND window_ = nullptr;
    UINT scale_ = 6;
};
}
