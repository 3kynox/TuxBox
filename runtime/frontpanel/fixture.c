// Controlled XFP transport for the connected comparison only.
#include <windows.h>
#include <stdint.h>
static uint16_t wire = 0x8802;
static unsigned write_count;
static const uint16_t bits[] = {0x8000, 0x2000, 2, 0x0800, 0x0200};

enum { CONNECT = 1, BUTTONS, READ_LEDS, WRITE_LEDS, FRAME };
static unsigned failure_op, failure_count;
void WINAPI FixtureFailNext(unsigned op, unsigned count) {
    failure_op = op;
    failure_count = count;
}
static int fail(unsigned op) {
    if (op != failure_op || !failure_count) return 0;
    --failure_count;
    return 1;
}
HRESULT WINAPI XfpTryConnect(void) { return fail(CONNECT) ? E_FAIL : S_OK; }
HRESULT WINAPI XfpGetKeyState(uint32_t *out) {
    if (fail(BUTTONS)) return E_FAIL;
    *out = 0x1509;
    return S_OK;
}
HRESULT WINAPI XfpGetLEDState(uint16_t *out) {
    if (fail(READ_LEDS)) return E_FAIL;
    *out = wire;
    return S_OK;
}
HRESULT WINAPI XfpSetLEDState(uint16_t value) {
    if (fail(WRITE_LEDS)) return E_FAIL;
    unsigned mask = write_count ? write_count - 1 : 0;
    uint16_t expected = 0;
    for (unsigned i = 0; i < 5; ++i) {
        if (mask & (1u << i)) expected |= bits[i];
    }
    if (value != expected) return E_INVALIDARG;
    ++write_count;
    wire = value;
    return S_OK;
}
HRESULT WINAPI XfpSetDisplayBuffer(const uint8_t *pixels, uint8_t bpp) {
    if (fail(FRAME)) return E_FAIL;
    if (bpp != 8) return E_INVALIDARG;
    for (unsigned i = 0; i < 64 * 256; ++i) {
        if (pixels[i] != (uint8_t)(i * 17 + 3)) return E_INVALIDARG;
    }
    return S_OK;
}
