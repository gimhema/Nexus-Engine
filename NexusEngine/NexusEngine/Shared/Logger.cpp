#include "Logger.h"

#include <cstdio>
#include <ctime>
#include <mutex>
#include <fstream>
#include <format>

// ─────────────────────────────────────────────────────────────────────────────
// Windows 콘솔 컬러 지원
// ─────────────────────────────────────────────────────────────────────────────
#ifdef _WIN32
#include <windows.h>
static void EnableVirtualTerminal()
{
    HANDLE h = GetStdHandle(STD_OUTPUT_HANDLE);
    if (h == INVALID_HANDLE_VALUE) return;
    DWORD mode = 0;
    if (!GetConsoleMode(h, &mode)) return;
    SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
}
#else
static void EnableVirtualTerminal() {}
#endif

// ─────────────────────────────────────────────────────────────────────────────
// 내부 상태
// ─────────────────────────────────────────────────────────────────────────────
LogLevel Logger::s_minLevel = LogLevel::Debug;

namespace
{
    std::mutex    g_mutex;
    std::ofstream g_file;

    constexpr std::string_view LevelTag(LogLevel lv)
    {
        switch (lv)
        {
            case LogLevel::Debug: return "DEBUG";
            case LogLevel::Info:  return " INFO";
            case LogLevel::Warn:  return " WARN";
            case LogLevel::Error: return "ERROR";
        }
        return "?????";
    }

    // ANSI 컬러 코드
    constexpr std::string_view LevelColor(LogLevel lv)
    {
        switch (lv)
        {
            case LogLevel::Debug: return "\x1b[37m";   // 흰색
            case LogLevel::Info:  return "\x1b[32m";   // 초록
            case LogLevel::Warn:  return "\x1b[33m";   // 노랑
            case LogLevel::Error: return "\x1b[31m";   // 빨강
        }
        return "";
    }
    constexpr std::string_view kReset = "\x1b[0m";

    std::string NowString()
    {
        std::time_t t = std::time(nullptr);
        std::tm     tm{};
#ifdef _WIN32
        localtime_s(&tm, &t);
#else
        localtime_r(&t, &tm);
#endif
        return std::format("{:04d}-{:02d}-{:02d} {:02d}:{:02d}:{:02d}",
            tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday,
            tm.tm_hour, tm.tm_min, tm.tm_sec);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
void Logger::Init(std::string_view filePath, LogLevel minLevel)
{
    EnableVirtualTerminal();
    s_minLevel = minLevel;

    std::lock_guard lock(g_mutex);
    if (!filePath.empty())
    {
        g_file.open(filePath.data(), std::ios::app);
        if (!g_file.is_open())
            std::fprintf(stderr, "[Logger] 파일 열기 실패: %s\n", filePath.data());
    }
}

void Logger::Shutdown()
{
    std::lock_guard lock(g_mutex);
    if (g_file.is_open())
        g_file.close();
}

void Logger::Write(LogLevel level, std::string_view msg,
                   const std::source_location& loc)
{
    // 파일명만 추출 (전체 경로 제외)
    std::string_view file = loc.file_name();
    if (auto pos = file.rfind('/');  pos != std::string_view::npos) file = file.substr(pos + 1);
    if (auto pos = file.rfind('\\'); pos != std::string_view::npos) file = file.substr(pos + 1);

    const auto now  = NowString();
    const auto tag  = LevelTag(level);
    const auto col  = LevelColor(level);

    // 콘솔: 컬러 포함
    const auto consoleLine = std::format("{}[{}][{}] {}:{} {}{}\n",
        col, now, tag, file, loc.line(), msg, kReset);

    // 파일: 컬러 제외
    const auto fileLine = std::format("[{}][{}] {}:{} {}\n",
        now, tag, file, loc.line(), msg);

    std::lock_guard lock(g_mutex);
    std::fputs(consoleLine.c_str(), stdout);
    if (g_file.is_open())
        g_file << fileLine;
}
