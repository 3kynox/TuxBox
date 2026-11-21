// Runs beside the selected xfrontpaneldisplay.dll. "connected" requires the fixture XFP transport.
#include <windows.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#define IMPORT __declspec(dllimport)
IMPORT HRESULT WINAPI XFrontPanelGetButtonStates(uint32_t *);
IMPORT HRESULT WINAPI XFrontPanelGetLightStates(uint32_t *);
IMPORT HRESULT WINAPI XFrontPanelGetScreenDimensions(uint32_t *, uint32_t *);
IMPORT HRESULT WINAPI XFrontPanelGetScreenPixelFormat(uint32_t *);
IMPORT bool WINAPI XFrontPanelIsSupported(void);
IMPORT HRESULT WINAPI XFrontPanelPresentBuffer(uint32_t, const uint8_t *);
IMPORT HRESULT WINAPI XFrontPanelSetDisplayName(const void *);
IMPORT HRESULT WINAPI XFrontPanelSetLightStates(uint32_t);

#define CHECK(x) do { if (!(x)) { fprintf(stderr, "FAIL line %d: %s\n", __LINE__, #x); return 1; } } while (0)
static uint8_t pixels[256 * 64];
static const char *const names[] = {
    "XFrontPanelGetButtonStates", "XFrontPanelGetLightStates", "XFrontPanelGetScreenDimensions",
    "XFrontPanelGetScreenPixelFormat", "XFrontPanelIsSupported", "XFrontPanelPresentBuffer",
    "XFrontPanelSetDisplayName", "XFrontPanelSetLightStates"
};

int main(int argc, char **argv) {
    bool connected = argc > 1 && !strcmp(argv[1], "--connected");
    int arg = connected ? 2 : 1;
    const char *report_path = NULL;
    if (argc == arg + 2 && !strcmp(argv[arg], "--report")) report_path = argv[arg + 1];
    else if (argc != arg) return 2;
    HMODULE mod = GetModuleHandleW(L"xfrontpaneldisplay.dll");
    wchar_t dll_path[MAX_PATH], expected[MAX_PATH];
    CHECK(mod && GetModuleFileNameW(mod, dll_path, MAX_PATH));
    CHECK(GetFullPathNameW(L"xfrontpaneldisplay.dll", MAX_PATH, expected, NULL));
    CHECK(!_wcsicmp(dll_path, expected));
    for (uint16_t i = 0; i < ARRAYSIZE(names); ++i) {
        FARPROC p = GetProcAddress(mod, names[i]);
        CHECK(p && p == GetProcAddress(mod, MAKEINTRESOURCEA(i + 1)));
    }

    uint32_t height = 17, width = 23, format = 42;
    CHECK(XFrontPanelGetScreenDimensions(&height, NULL) == E_INVALIDARG && height == 17);
    CHECK(XFrontPanelGetScreenDimensions(NULL, &width) == E_INVALIDARG && width == 23);
    CHECK(XFrontPanelGetScreenDimensions(&height, &width) == S_OK && height == 64 && width == 256);
    CHECK(XFrontPanelGetScreenPixelFormat(NULL) == E_INVALIDARG);
    CHECK(XFrontPanelGetScreenPixelFormat(&format) == S_OK && format == 61);
    CHECK(XFrontPanelSetDisplayName(NULL) == E_INVALIDARG && XFrontPanelSetDisplayName(L"Panel") == S_OK);
    CHECK(XFrontPanelPresentBuffer(sizeof(pixels) - 1, pixels) == E_INVALIDARG);
    CHECK(XFrontPanelPresentBuffer(sizeof(pixels) + 1, NULL) == E_INVALIDARG);
    CHECK(XFrontPanelPresentBuffer(sizeof(pixels), NULL) == E_POINTER);
    CHECK(XFrontPanelGetLightStates(NULL) == E_INVALIDARG);
    CHECK(XFrontPanelIsSupported() == connected);

    uint32_t buttons = 99, lights = 99;
    for (size_t i = 0; i < sizeof(pixels); ++i) pixels[i] = (uint8_t)(i * 17 + 3);
    HRESULT key = XFrontPanelGetButtonStates(&buttons);
    HRESULT led = XFrontPanelGetLightStates(&lights);
    HRESULT frame = XFrontPanelPresentBuffer(sizeof(pixels), pixels);
    HRESULT set = XFrontPanelSetLightStates(0);
    printf("buttons=%08lx/%u lights=%08lx/%u frame=%08lx set=%08lx\n",
           (DWORD)key, buttons, (DWORD)led, lights, (DWORD)frame, (DWORD)set);
    if (connected) {
        CHECK(key == S_OK && buttons == 0x1509);
        CHECK(led == S_OK && lights == 13 && frame == S_OK && set == S_OK);
        // The controlled backend checks the wire bits independently on every write.
        for (uint32_t i = 0; i < 32; ++i) {
            CHECK(XFrontPanelSetLightStates(0xffff0000 | i) == S_OK);
            CHECK(XFrontPanelGetLightStates(&lights) == S_OK && lights == i);
        }
        typedef void (WINAPI *FailNext)(unsigned, unsigned);
        HMODULE fixture = GetModuleHandleW(L"xfp_gc.dll");
        CHECK(fixture);
        FailNext fail = (FailNext)(void *)GetProcAddress(fixture, "FixtureFailNext");
        CHECK(fail);
        fail(1, 1);
        CHECK(!XFrontPanelIsSupported() && XFrontPanelIsSupported());
        buttons = 42;
        fail(1, 2);
        CHECK(XFrontPanelGetButtonStates(&buttons) == E_FAIL && buttons == 42);
        fail(1, 1);
        CHECK(XFrontPanelGetButtonStates(&buttons) == S_OK && buttons == 42);
        fail(2, 1);
        CHECK(XFrontPanelGetButtonStates(&buttons) == E_FAIL && buttons == 42);
        fail(3, 2);
        CHECK(XFrontPanelGetLightStates(&lights) == E_FAIL && lights == 0);
        fail(3, 1);
        CHECK(XFrontPanelGetLightStates(&lights) == S_OK && lights == 0);
        fail(4, 1);
        CHECK(XFrontPanelSetLightStates(31) == E_FAIL);
        fail(5, 1);
        CHECK(XFrontPanelPresentBuffer(sizeof(pixels), pixels) == E_FAIL);
    } else {
        HRESULT absent = HRESULT_FROM_WIN32(ERROR_PATH_NOT_FOUND);
        CHECK(key == absent && buttons == 99 && led == absent && lights == 0);
        CHECK(frame == absent && set == absent);
    }
    const char *receipt = connected ? "frontpanel PASS connected led-masks=32 pixels=16384 errors=8"
                                    : "frontpanel PASS no-panel exports=8";
    if (report_path) {
        FILE *report;
        if (fopen_s(&report, report_path, "wb")) return 1;
        int written = fprintf(report, "%s\n", receipt);
        if (fclose(report) || written < 0) return 1;
    }
    puts(receipt);
    return 0;
}
