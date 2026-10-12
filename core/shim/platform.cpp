// The slice of melonDS's Platform:: interface its net layer calls.
// (Net_SendPacket / Net_RecvPacket live in mdsnet.cpp, beside the AP.)

#include <chrono>
#include <condition_variable>
#include <cstdarg>
#include <cstdio>
#include <mutex>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#else
#include <dlfcn.h>
#endif

#include "mdsnet.h"
#include "Platform.h"

static MdsNetLogFn g_log = nullptr;

extern "C" void mdsnet_set_log(MdsNetLogFn fn) { g_log = fn; }

namespace melonDS::Platform
{

void Log(LogLevel level, const char* fmt, ...)
{
    if (!g_log) return;
    char buf[1024];
    va_list args;
    va_start(args, fmt);
    vsnprintf(buf, sizeof(buf), fmt, args);
    va_end(args);
    g_log((int)level, buf);
}

struct Mutex { std::mutex m; };

Mutex* Mutex_Create() { return new Mutex(); }
void Mutex_Free(Mutex* mutex) { delete mutex; }
void Mutex_Lock(Mutex* mutex) { mutex->m.lock(); }
void Mutex_Unlock(Mutex* mutex) { mutex->m.unlock(); }
bool Mutex_TryLock(Mutex* mutex) { return mutex->m.try_lock(); }

struct Semaphore
{
    std::mutex m;
    std::condition_variable cv;
    int count = 0;
};

Semaphore* Semaphore_Create() { return new Semaphore(); }
void Semaphore_Free(Semaphore* s) { delete s; }

void Semaphore_Reset(Semaphore* s)
{
    std::lock_guard lock(s->m);
    s->count = 0;
}

void Semaphore_Wait(Semaphore* s)
{
    std::unique_lock lock(s->m);
    s->cv.wait(lock, [s] { return s->count > 0; });
    s->count--;
}

bool Semaphore_TryWait(Semaphore* s, int timeout_ms)
{
    std::unique_lock lock(s->m);
    if (timeout_ms <= 0)
    {
        if (s->count <= 0) return false;
    }
    else if (!s->cv.wait_for(lock, std::chrono::milliseconds(timeout_ms), [s] { return s->count > 0; }))
    {
        return false;
    }
    s->count--;
    return true;
}

void Semaphore_Post(Semaphore* s, int count)
{
    {
        std::lock_guard lock(s->m);
        s->count += count;
    }
    s->cv.notify_all();
}

static const auto kEpoch = std::chrono::steady_clock::now();

u64 GetMSCount()
{
    return std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - kEpoch).count();
}

u64 GetUSCount()
{
    return std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::steady_clock::now() - kEpoch).count();
}

DynamicLibrary* DynamicLibrary_Load(const char* lib)
{
#ifdef _WIN32
    return (DynamicLibrary*)LoadLibraryA(lib);
#else
    return (DynamicLibrary*)dlopen(lib, RTLD_NOW | RTLD_LOCAL);
#endif
}

void DynamicLibrary_Unload(DynamicLibrary* lib)
{
#ifdef _WIN32
    FreeLibrary((HMODULE)lib);
#else
    dlclose(lib);
#endif
}

void* DynamicLibrary_LoadFunction(DynamicLibrary* lib, const char* name)
{
#ifdef _WIN32
    return (void*)GetProcAddress((HMODULE)lib, name);
#else
    return dlsym(lib, name);
#endif
}

}
