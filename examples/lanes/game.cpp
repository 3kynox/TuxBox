#define UNICODE
#define NOMINMAX
#include "course.h"
extern "C" {
#include "../../runtime/xmem/xmem.h"
}
#include <algorithm>
#include <array>
#include <windowsx.h>
#include <Xinput.h>
#include <cstdio>
#include <cwchar>
#include <filesystem>
#include <fstream>
#include <memory>
#include <new>
#include <stdexcept>
#include <string>

enum class Finish { playing, won, crashed };

struct Game {
    Course rows{};
    int lane = 1, score = 0;
    unsigned turn = 0;
    Finish finish = Finish::playing;

    void restart() {
        lane = 1;
        score = 0;
        turn = 0;
        finish = Finish::playing;
    }

    void move(int direction) {
        if (finish != Finish::playing) return;
        lane = std::clamp(lane + direction, 0, 2);
        const auto row = rows[turn++];
        if (unsigned(lane) == (row >> 8)) {
            finish = Finish::crashed;
            return;
        }
        if (unsigned(lane) == (row & 255u)) ++score;
        if (turn == rows.size()) finish = Finish::won;
    }
};

namespace {
constexpr std::uint32_t replay_seed = 0x5194c0d1u;

void expect(bool ok, const char* reason) {
    if (!ok) throw std::runtime_error(reason);
}

struct PadState {
    bool connected = false;
    unsigned held = 0;

    bool update(Game& game, const XINPUT_GAMEPAD& pad) {
        enum : unsigned { left = 1, right = 2, hold = 4, restart = 8 };
        unsigned current = 0;
        if ((pad.wButtons & XINPUT_GAMEPAD_DPAD_LEFT) || pad.sThumbLX < -(held & left ? 10000 : 16000)) current |= left;
        if ((pad.wButtons & XINPUT_GAMEPAD_DPAD_RIGHT) || pad.sThumbLX > (held & right ? 10000 : 16000)) current |= right;
        if (pad.wButtons & XINPUT_GAMEPAD_A) current |= hold;
        if (pad.wButtons & XINPUT_GAMEPAD_START) current |= restart;
        const unsigned pressed = connected ? current & ~held : 0;
        connected = true;
        held = current;
        if (pressed & restart) game.restart();
        else if (pressed & hold) game.move(0);
        else if ((pressed & left) && !(current & right)) game.move(-1);
        else if ((pressed & right) && !(current & left)) game.move(1);
        else return false;
        return true;
    }
};

void replay(Game& game) {
    // Input regression for independent press edges and stick hysteresis.
    Game controls;
    controls.rows.fill(0xff00);
    PadState port;
    XINPUT_GAMEPAD pad{};
    pad.wButtons = XINPUT_GAMEPAD_A;
    expect(!port.update(controls, pad), "held button advanced on connection");
    pad.wButtons = XINPUT_GAMEPAD_DPAD_RIGHT;
    expect(port.update(controls, pad) && controls.lane == 2, "D-pad press missed");
    pad.wButtons |= XINPUT_GAMEPAD_A;
    expect(port.update(controls, pad) && controls.turn == 2, "A press missed while moving");
    pad.wButtons = XINPUT_GAMEPAD_DPAD_RIGHT;
    expect(!port.update(controls, pad) && controls.turn == 2, "releasing A repeated held direction");
    pad.wButtons = 0;
    pad.sThumbLX = -20000;
    expect(port.update(controls, pad) && controls.lane == 1, "analog press missed");
    pad.sThumbLX = -14000;
    expect(!port.update(controls, pad), "stick jitter released held direction");
    pad.sThumbLX = -20000;
    expect(!port.update(controls, pad), "stick jitter advanced the game");
    // Golden rows for this seed, in order. This also checks unvisited hazards from a winning playthrough.
    constexpr Course reference{
        0x0001, 0x0102, 0x0201, 0x0100,
        0x0001, 0x0102, 0x0201, 0x0100,
        0x0001, 0x0002, 0x0201, 0x0200,
        0x0001, 0x0002, 0x0201, 0x0200,
    };
    expect(game.rows == reference, "course differs from the fixed-seed reference");
    constexpr int moves[] = {0, 1, -1, -1, 1, 1, -1, -1, 1, 1, -1, -1, 1, 1, -1, -1};
    for (int move : moves) game.move(move);
    expect(game.finish == Finish::won && game.score == 16 && game.turn == course_length, "winning replay differs");
    game.move(1);
    expect(game.turn == course_length && game.score == 16, "won game accepted another move");

    game.restart();
    int coin = game.rows.front() & 255, hazard = game.rows.front() >> 8;
    int safe = 3 - coin - hazard;
    game.move(safe - game.lane);
    expect(game.finish == Finish::playing && game.turn == 1 && game.score == 0, "safe miss awarded a point");
    game.restart();
    game.move(hazard - game.lane);
    expect(game.finish == Finish::crashed && game.turn == 1 && game.score == 0, "collision failed to end play");
    game.move(0);
    expect(game.turn == 1 && game.score == 0, "crashed game accepted another move");
}

void fill(HDC dc, RECT rect, COLORREF color) {
    SetDCBrushColor(dc, color);
    FillRect(dc, &rect, static_cast<HBRUSH>(GetStockObject(DC_BRUSH)));
}

void label(HDC dc, RECT rect, const wchar_t* text, int size, int weight, COLORREF color, UINT flags) {
    HFONT font = CreateFontW(-size, 0, 0, 0, weight, FALSE, FALSE, FALSE, DEFAULT_CHARSET,
                             OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                             DEFAULT_PITCH, L"Inter");
    const auto old = SelectObject(dc, font);
    SetTextColor(dc, color);
    DrawTextW(dc, text, -1, &rect, flags | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
    SelectObject(dc, old);
    DeleteObject(font);
}

void coin(HDC dc, int x, int y) {
    SetDCBrushColor(dc, RGB(251, 196, 74));
    Ellipse(dc, x - 13, y - 13, x + 14, y + 14);
    fill(dc, {x - 2, y - 7, x + 2, y + 7}, RGB(118, 76, 19));
}

void hazard(HDC dc, int x, int y) {
    SetDCBrushColor(dc, RGB(250, 105, 88));
    POINT triangle[] = {{x, y - 16}, {x - 16, y + 12}, {x + 16, y + 12}};
    Polygon(dc, triangle, 3);
    fill(dc, {x - 2, y - 5, x + 2, y + 3}, RGB(58, 30, 35));
    fill(dc, {x - 2, y + 6, x + 2, y + 9}, RGB(58, 30, 35));
}

RECT board(RECT area) {
    const auto margin = std::max<LONG>(15, area.right / 18);
    return {margin, 104, area.right - margin, area.bottom - 88};
}

void paint(HDC dc, RECT area, const Game& game) {
    const int width = area.right, height = area.bottom;
    const auto bounds = board(area);
    const int left = bounds.left, top = bounds.top, right = bounds.right, bottom = bounds.bottom;
    const COLORREF ink = RGB(21, 26, 38), paper = RGB(242, 245, 249), muted = RGB(166, 181, 199);
    fill(dc, area, ink);
    SetBkMode(dc, TRANSPARENT);
    label(dc, {left, 12, width / 2, 68}, L"LANES", 41, FW_BOLD, paper, DT_LEFT);
    wchar_t score[80];
    std::swprintf(score, 80, L"%d / %u coins", game.score, course_length);
    label(dc, {width / 2, 12, right, 68}, score, 21, FW_SEMIBOLD, RGB(251, 196, 74), DT_RIGHT);
    std::swprintf(score, 80, L"Turn %u of %u", std::min(game.turn + 1, course_length), course_length);
    label(dc, {left, 67, right, 100}, score, 16, FW_NORMAL, muted, DT_LEFT);
    if (bottom <= top || right <= left) return;
    auto center = [&](int lane) { return left + (2 * lane + 1) * (right - left) / 6; };
    for (int i = 0; i < 3; ++i) {
        int x = left + i * (right - left) / 3;
        int end = left + (i + 1) * (right - left) / 3;
        fill(dc, {x + 1, top, end - 1, bottom}, i == game.lane ? RGB(42, 64, 76) : RGB(34, 42, 59));
    }
    auto brush = SelectObject(dc, GetStockObject(DC_BRUSH));
    auto pen = SelectObject(dc, GetStockObject(NULL_PEN));
    const int spacing = (bottom - top - 42) / 5;
    for (unsigned ahead = 0; ahead < 5 && game.turn + ahead < course_length; ++ahead) {
        const auto row = game.rows[game.turn + ahead];
        const int y = bottom - 42 - int(ahead) * spacing - spacing / 2;
        coin(dc, center(row & 255u), y);
        hazard(dc, center(row >> 8), y);
    }
    const int x = center(game.lane);
    SetDCBrushColor(dc, RGB(95, 235, 202));
    POINT player[] = {{x, bottom - 37}, {x + 19, bottom - 10}, {x, bottom - 16}, {x - 19, bottom - 10}};
    Polygon(dc, player, 4);
    SelectObject(dc, pen);
    SelectObject(dc, brush);
    if (game.finish != Finish::playing) {
        int y = (top + bottom) / 2;
        fill(dc, {left, y - 57, right, y + 57}, ink);
        label(dc, {left, y - 48, right, y + 7}, game.finish == Finish::won ? L"Course complete" : L"Collision",
              30, FW_BOLD, paper, DT_CENTER);
        label(dc, {left, y + 8, right, y + 43}, L"Click the board, press R or press Start to replay", 17, FW_NORMAL, muted, DT_CENTER);
    }
    label(dc, {left, bottom + 9, right, bottom + 39}, L"Move: arrows, D-pad or stick    Hold lane: Space or A",
          16, FW_SEMIBOLD, paper, DT_CENTER);
    label(dc, {left, bottom + 38, right, height - 8}, L"Or click a lane. Collect coins and avoid red barriers.",
          15, FW_NORMAL, muted, DT_CENTER);
}

struct WindowState {
    Game& game;
    std::array<PadState, XUSER_MAX_COUNT> pads{};
    ULONGLONG next_scan = 0;

    bool poll_pads() {
        const auto now = GetTickCount64();
        const bool scan = now >= next_scan;
        if (scan) next_scan = now + 500;
        bool advanced = false;
        for (DWORD slot = 0; slot < pads.size(); ++slot) {
            auto& port = pads[slot];
            if (!scan && !port.connected) continue;
            XINPUT_STATE state{};
            if (XInputGetState(slot, &state) == ERROR_SUCCESS) {
                if (port.update(game, state.Gamepad)) advanced = true;
            } else port = {};
        }
        return advanced;
    }
};

LRESULT CALLBACK window_proc(HWND window, UINT message, WPARAM w, LPARAM l) {
    if (message == WM_NCCREATE) {
        auto* create = reinterpret_cast<CREATESTRUCTW*>(l);
        SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(create->lpCreateParams));
    }
    auto* state = reinterpret_cast<WindowState*>(GetWindowLongPtrW(window, GWLP_USERDATA));
    Game* game = state ? &state->game : nullptr;
    if (state && message == WM_KILLFOCUS) {
        state->pads.fill({});
        state->next_scan = 0;
    }
    if (state && message == WM_TIMER && w == 1) {
        if (GetForegroundWindow() == window && state->poll_pads()) InvalidateRect(window, nullptr, FALSE);
        return 0;
    }
    if (game && message == WM_LBUTTONDOWN) {
        RECT area;
        GetClientRect(window, &area);
        const auto bounds = board(area);
        POINT pos{GET_X_LPARAM(l), GET_Y_LPARAM(l)};
        if (PtInRect(&bounds, pos)) {
            if (game->finish != Finish::playing) game->restart();
            else {
                const int target = 3 * (pos.x - bounds.left) / (bounds.right - bounds.left);
                game->move(std::clamp(target - game->lane, -1, 1));
            }
            InvalidateRect(window, nullptr, FALSE);
        }
        return 0;
    }
    if (game && message == WM_KEYDOWN) {
        if (l & (1u << 30)) return 0; // one turn per physical press
        switch (w) {
        case VK_LEFT: game->move(-1); break;
        case VK_RIGHT: game->move(1); break;
        case VK_SPACE: game->move(0); break;
        case 'R': game->restart(); break;
        case VK_ESCAPE: DestroyWindow(window); return 0;
        default: return DefWindowProcW(window, message, w, l);
        }
        InvalidateRect(window, nullptr, FALSE);
        return 0;
    }
    if (game && message == WM_PAINT) {
        PAINTSTRUCT image{};
        auto dc = BeginPaint(window, &image);
        RECT area;
        GetClientRect(window, &area);
        paint(dc, area, *game);
        EndPaint(window, &image);
        return 0;
    }
    if (message == WM_DESTROY) {
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(window, message, w, l);
}

void play(Game& game) {
    WNDCLASSW type{};
    type.lpfnWndProc = window_proc;
    type.hInstance = GetModuleHandleW(nullptr);
    type.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    type.lpszClassName = L"ProjectTuxBoxLanes";
    type.style = CS_HREDRAW | CS_VREDRAW;
    expect(RegisterClassW(&type) != 0, "cannot register the game window");
    WindowState state{game};
    auto window = CreateWindowExW(0, type.lpszClassName, L"Lanes | Project TuxBox", WS_OVERLAPPEDWINDOW,
                                  CW_USEDEFAULT, CW_USEDEFAULT, 530, 670, nullptr, nullptr, type.hInstance, &state);
    expect(window != nullptr, "cannot create the game window");
    expect(SetTimer(window, 1, 16, nullptr) != 0, "cannot start controller input");
    ShowWindow(window, SW_SHOWNORMAL);
    UpdateWindow(window);
    MSG message;
    BOOL status;
    while ((status = GetMessageW(&message, nullptr, 0, 0)) > 0) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    expect(status == 0, "window message loop failed");
}
}

int wmain(int argc, wchar_t** argv) {
    bool scripted = false;
    const wchar_t* report = nullptr;
    const wchar_t* adapter = nullptr;
    for (int i = 1; i < argc; ++i) {
        if (!std::wcscmp(argv[i], L"--replay") && !scripted) scripted = true;
        else if (!std::wcscmp(argv[i], L"--report") && !report && i + 1 < argc) report = argv[++i];
        else if (!std::wcscmp(argv[i], L"--adapter") && !adapter && i + 1 < argc) adapter = argv[++i];
        else { std::fputs("usage: lanes.exe [--adapter VVVV:DDDD] [--replay [--report FILE]]\n", stderr); return 2; }
    }
    if (report && !scripted) return 2;
    try {
        std::ofstream proof;
        if (report) {
            proof.open(std::filesystem::path(report), std::ios::binary | std::ios::trunc);
            expect(bool(proof), "cannot open replay report");
        }
        auto release = [](Game* p) { p->~Game(); XMemFreeDefault(p, XMEM_ORDINARY_ATTRIBUTES); };
        void* block = XMemAllocDefault(sizeof(Game), XMEM_ORDINARY_ATTRIBUTES);
        expect(block != nullptr, "XMem board allocation failed");
        std::unique_ptr<Game, decltype(release)> game(new(block) Game{}, release);
        auto device = generate_course(game->rows, scripted ? replay_seed : GetTickCount(), adapter);
        if (!scripted) { play(*game); return 0; }
        replay(*game);
        game.reset();
        const auto line = "lanes-replay PASS pci=" + device + " course=16/16 win=16/16 safe-miss=1 collision=1";
        if (report) {
            proof << line << '\n';
            proof.close();
            expect(bool(proof), "cannot write replay report");
        }
        std::puts(line.c_str());
        return 0;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "lanes: %s\n", error.what());
        if (!scripted) MessageBoxA(nullptr, error.what(), "Lanes could not start", MB_ICONERROR);
        return 1;
    }
}
