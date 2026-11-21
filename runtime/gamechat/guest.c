// Independent consumer of GameChat2's x64 C exports. No Xbox accounts or
// network endpoints are opened. User IDs and endpoint identifiers are synthetic.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>
#include <stdio.h>
#include <wchar.h>

typedef uintptr_t User;
__declspec(dllimport) HRESULT WINAPI ChatUserGetXboxUserId(User, PCWSTR *);
__declspec(dllimport) HRESULT WINAPI ChatUserGetCustomUserContext(User, void **);
__declspec(dllimport) HRESULT WINAPI ChatUserSetCustomUserContext(User, void *);
__declspec(dllimport) HRESULT WINAPI ChatManagerInitialize(UINT, float, UINT, UINT, UINT, UINT);
__declspec(dllimport) HRESULT WINAPI ChatManagerCleanup(void);
__declspec(dllimport) HRESULT WINAPI ChatManagerSetAudioEncodingBitrate(UINT);
__declspec(dllimport) HRESULT WINAPI ChatManagerGetAudioEncodingBitrate(UINT *);
__declspec(dllimport) HRESULT WINAPI ChatManagerAddRemoteUser(PCWSTR, UINT64, User *);
__declspec(dllimport) HRESULT WINAPI ChatManagerGetChatUsers(UINT *, const User **);
__declspec(dllimport) HRESULT WINAPI ChatManagerRemoveUser(User);
__declspec(dllimport) HRESULT WINAPI ChatUserGetIsLocal(User, BYTE *);

#define CHECK(expr) do { if (!(expr)) { fprintf(stderr, "gamechat-session failed at %s\n", #expr); return FALSE; } } while (0)

static BOOL local_provider(void) {
    HMODULE module = GetModuleHandleW(L"GameChat2.dll");
    wchar_t exe[MAX_PATH], dll[MAX_PATH];
    DWORD exe_len = GetModuleFileNameW(NULL, exe, ARRAYSIZE(exe));
    DWORD dll_len = module ? GetModuleFileNameW(module, dll, ARRAYSIZE(dll)) : 0;
    if (!exe_len || exe_len >= ARRAYSIZE(exe) || !dll_len || dll_len >= ARRAYSIZE(dll)) return FALSE;
    wchar_t *a = wcsrchr(exe, L'\\'), *b = wcsrchr(dll, L'\\');
    if (!a || !b || _wcsicmp(b + 1, L"GameChat2.dll")) return FALSE;
    *a = *b = L'\0';
    return !_wcsicmp(exe, dll);
}

static BOOL exercise(void) {
    const HRESULT no_session = (HRESULT)0x800710ddU, roster_full = (HRESULT)0x89234201U;
    const HRESULT invalid_xuid = (HRESULT)0x89234501U;
    const User sentinel = 0xfeef;
    const User *users = &sentinel;
    User first, second, third, duplicate = sentinel;
    UINT count = 47, bitrate = 97;
    BYTE local = 1;
    PCWSTR first_id = (PCWSTR)1, second_id = NULL, again_id = NULL;
    void *context = &first;
    CHECK(local_provider());
    CHECK(ChatUserGetXboxUserId(sentinel, &first_id) == no_session && first_id == (PCWSTR)1);
    CHECK(ChatUserGetCustomUserContext(sentinel, &context) == no_session && context == &first);
    CHECK(ChatUserSetCustomUserContext(sentinel, &second) == no_session);
    CHECK(ChatManagerGetAudioEncodingBitrate(&bitrate) == no_session && bitrate == 97);
    CHECK(ChatManagerGetChatUsers(&count, &users) == no_session && !count && !users);
    CHECK(ChatManagerInitialize(2, 1.f, 0, 0, 0, 0) == S_OK);
    CHECK(ChatManagerInitialize(2, 1.f, 0, 0, 0, 0) == no_session);
    CHECK(ChatManagerGetAudioEncodingBitrate(&bitrate) == S_OK && bitrate == 2);
    for (UINT rate = 0; rate < 3; ++rate) {
        CHECK(ChatManagerSetAudioEncodingBitrate(rate) == S_OK);
        CHECK(ChatManagerGetAudioEncodingBitrate(&bitrate) == S_OK && bitrate == rate);
    }
    CHECK(ChatManagerGetChatUsers(&count, &users) == S_OK && count == 0);
    CHECK(ChatManagerAddRemoteUser(L"invalid", 3, &duplicate) == invalid_xuid && duplicate == sentinel);
    CHECK(ChatManagerAddRemoteUser(L"0070001", 3, &first) == S_OK && first);
    CHECK(ChatManagerAddRemoteUser(L"0070001", 3, &duplicate) == S_OK && duplicate == first);
    CHECK(ChatManagerAddRemoteUser(L"18446744073709551614", 3, &second) == S_OK && second && second != first);
    CHECK(ChatUserGetCustomUserContext(first, &context) == S_OK && !context);
    CHECK(ChatUserSetCustomUserContext(first, &first) == S_OK);
    CHECK(ChatUserGetCustomUserContext(duplicate, &context) == S_OK && context == &first);
    CHECK(ChatUserGetXboxUserId(first, &first_id) == S_OK && first_id && !wcscmp(first_id,L"70001"));
    CHECK(ChatUserGetXboxUserId(duplicate, &again_id) == S_OK && again_id == first_id);
    CHECK(ChatUserGetCustomUserContext(second, &context) == S_OK && !context);
    CHECK(ChatUserSetCustomUserContext(second, &second) == S_OK);
    CHECK(ChatUserGetXboxUserId(second, &second_id) == S_OK && second_id && !wcscmp(second_id,L"18446744073709551614"));
    CHECK(ChatManagerGetChatUsers(&count, &users) == S_OK && count == 2 && users);
    CHECK((users[0] == first && users[1] == second) || (users[0] == second && users[1] == first));
    CHECK(ChatUserGetIsLocal(first, &local) == S_OK && local == 0);
    third = sentinel;
    CHECK(ChatManagerAddRemoteUser(L"70003", 3, &third) == roster_full && third == sentinel);
    CHECK(ChatManagerRemoveUser(first) == S_OK);
    context = &first;
    CHECK(ChatUserGetXboxUserId(first, &again_id) == HRESULT_FROM_WIN32(ERROR_NOT_FOUND) && !again_id);
    CHECK(ChatUserGetCustomUserContext(first, &context) == HRESULT_FROM_WIN32(ERROR_NOT_FOUND) && context == &first);
    CHECK(ChatUserSetCustomUserContext(first, &second) == HRESULT_FROM_WIN32(ERROR_NOT_FOUND));
    CHECK(ChatUserGetXboxUserId(second, &again_id) == S_OK && again_id == second_id && !wcscmp(second_id,L"18446744073709551614"));
    CHECK(ChatUserGetCustomUserContext(second, &context) == S_OK && context == &second);
    CHECK(ChatUserSetCustomUserContext(second, NULL) == S_OK);
    CHECK(ChatUserGetCustomUserContext(second, &context) == S_OK && !context);
    local = 1;
    CHECK(ChatUserGetIsLocal(first, &local) == HRESULT_FROM_WIN32(ERROR_NOT_FOUND) && local == 0);
    CHECK(ChatManagerRemoveUser(first) == HRESULT_FROM_WIN32(ERROR_NOT_FOUND));
    CHECK(ChatManagerGetChatUsers(&count, &users) == S_OK && count == 1 && users[0] == second);
    CHECK(ChatManagerAddRemoteUser(L"70003", 7, &third) == S_OK && third && third != first && third != second);
    CHECK(ChatManagerGetChatUsers(&count, &users) == S_OK && count == 2);
    CHECK(ChatManagerSetAudioEncodingBitrate(1) == S_OK);
    CHECK(ChatManagerCleanup() == S_OK);
    CHECK(ChatManagerInitialize(2, 1.f, 0, 0, 0, 0) == S_OK);
    CHECK(ChatManagerGetAudioEncodingBitrate(&bitrate) == S_OK && bitrate == 2);
    CHECK(ChatManagerGetChatUsers(&count, &users) == S_OK && count == 0);
    CHECK(ChatManagerCleanup() == S_OK && ChatManagerCleanup() == no_session);
    return TRUE;
}

int wmain(int argc, wchar_t **argv) {
    if (argc != 1 && (argc != 3 || wcscmp(argv[1], L"--report") || !*argv[2])) return 2;
    if (!exercise()) return 1;
    const char *line = "gamechat-session PASS sessions=2 remote-users=3 bitrate-settings=3 stale-handles=1 xuids=2 contexts=2";
    if (argc == 3) {
        FILE *file;
        if (_wfopen_s(&file, argv[2], L"wb")) return 1;
        int written = fprintf(file, "%s\n", line);
        if (fclose(file) || written < 0) return 1;
    }
    puts(line);
    return 0;
}
